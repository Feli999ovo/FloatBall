//! Agent：模型 + 工具的循环。主控回答提问，需要时调用工具，或把一步交给别的 Agent（delegate）。
//!
//! 工具按风险分三档（设置 → 权限）：
//! - 只读（搜索网页、打开网页读内容、读文件、看文件夹）：自动允许
//! - 写入（写文件）、打开浏览器：每个范围第一次问，允许后这次任务里不再问
//! - 发送（发邮件）、删除：每次都问；可以「总是允许」，记在设置里，可撤销
//!
//! 预算（设置 → Agents）：单个任务的 token 上限、最多步数（每次请求模型算一步）、最多同时几个子 Agent。

mod ask;

pub use ask::{Answer, Asks};
#[cfg(test)]
use ask::{MORE_STEPS, MORE_TOKENS};

mod cost;

mod tools;

use tools::{arg, describe, http_url, label, plan_tool, risk, short, DEFS};
#[cfg(test)]
use tools::{folder, host};

mod exec;

mod delegate;

#[cfg(test)]
use delegate::handoff_summary;
use delegate::{DelegateStats, DelegateSummary};

mod world;

pub use world::RealWorld;
#[cfg(test)]
use world::{html_text, parse_ddg};

use crate::config::{Agent, Allow, Config, Model};
use crate::llm::{self, ChatEvent};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{oneshot, Semaphore};

/// 外部世界：真实程序里打开浏览器 / 邮件、联网搜索；测试里换成假的
pub trait World: Send + Sync {
    fn open(&self, url: &str) -> Result<(), String>;
    fn search<'a>(
        &'a self,
        q: &'a str,
    ) -> futures_util::future::BoxFuture<'a, Result<String, String>>;
    fn fetch<'a>(
        &'a self,
        url: &'a str,
    ) -> futures_util::future::BoxFuture<'a, Result<String, String>>;
    /// 把总是允许存进设置
    fn remember(&self, a: Allow);
    fn memory_store(&self) -> Option<&dyn MemoryStore>;
}

pub trait MemoryStore: Send + Sync {
    fn recent(&self, agent: &str) -> Vec<String>;
    fn add(&self, agent: &str, text: &str) -> Result<i64, String>;
}

pub struct Task {
    pub cfg: Config,
    plan_only: bool,
    pre_allow: Vec<String>,
    changes: Arc<crate::changes::Changes>,
    backup: PathBuf,
    pub world: Arc<dyn World>,
    pub asks: Arc<Asks>,
    /// 删除的文件移到这里（悬浮球回收站），可以找回
    pub trash: PathBuf,
    pub emit: Arc<dyn Fn(ChatEvent) + Send + Sync>,
    tokens: AtomicU64,
    pub prompt: AtomicU64,
    pub completion: AtomicU64,
    cost: Mutex<Option<f64>>,
    has_usage: std::sync::atomic::AtomicBool,
    steps: AtomicU64,
    /// 这次任务里已经允许过的「工具 · 范围」（写文件、打开浏览器只问一次）
    allowed: Mutex<HashSet<String>>,
    par: Semaphore,
    seq: AtomicU64,
    /// 预算用满后用户「再给」加上的 token / 步数
    more_tokens: AtomicU64,
    more_steps: AtomicU64,
    /// 几个子 Agent 同时撞上预算时只问一次
    budget_ask: tokio::sync::Mutex<()>,
    /// 用户在预算卡上选了「停止」：之后谁再撞上预算都直接停，不再问
    budget_stop: std::sync::atomic::AtomicBool,
    pub(crate) catalog: Mutex<Option<crate::catalog::Catalog>>,
    last_delegate: Mutex<Option<DelegateSummary>>,
}

impl Task {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: Config,
        world: Arc<dyn World>,
        asks: Arc<Asks>,
        trash: PathBuf,
        emit: Arc<dyn Fn(ChatEvent) + Send + Sync>,
        plan_only: bool,
        pre_allow: Vec<String>,
        changes: Arc<crate::changes::Changes>,
        backup: PathBuf,
    ) -> Arc<Self> {
        let par = cfg.limits.parallel.max(1) as usize;
        Arc::new(Self {
            cfg,
            plan_only,
            pre_allow,
            changes,
            backup,
            world,
            asks,
            trash,
            emit,
            tokens: AtomicU64::new(0),
            prompt: AtomicU64::new(0),
            completion: AtomicU64::new(0),
            cost: Mutex::new(Some(0.0)),
            has_usage: Default::default(),
            steps: AtomicU64::new(0),
            allowed: Mutex::new(HashSet::new()),
            par: Semaphore::new(par),
            seq: AtomicU64::new(0),
            more_tokens: AtomicU64::new(0),
            more_steps: AtomicU64::new(0),
            budget_ask: tokio::sync::Mutex::new(()),
            budget_stop: Default::default(),
            catalog: Mutex::new(None),
            last_delegate: Mutex::new(None),
        })
    }

    /// 这个 Agent 用哪个模型：自动省钱时优先用本地免费的，其次自己选的，再次默认
    fn model(&self, a: &Agent) -> Option<Model> {
        self.cfg.effective_model_for(a).cloned()
    }

    /// 让一个 Agent 跑完：请求模型 → 执行工具 → 再请求，直到它不再调用工具。
    /// lead：主控的正文流给界面；子 Agent 的正文只作为 delegate 的结果。
    pub fn run<'a>(
        self: &'a Arc<Self>,
        agent: &'a Agent,
        msgs: Vec<Value>,
        lead: bool,
    ) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
        self.run_acc(agent, msgs, lead, None)
    }

    /// 同 run；acc 另外记下这个 Agent 自己用掉的 token（多 Agent 计划卡每一步显示）
    fn run_acc<'a>(
        self: &'a Arc<Self>,
        agent: &'a Agent,
        mut msgs: Vec<Value>,
        lead: bool,
        stats: Option<Arc<DelegateStats>>,
    ) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let m = self
                .model(agent)
                .ok_or("还没有设置模型。右键小球 → 设置，添加一个模型。")?;
            let key = crate::config::get_api_key(m.id);
            if key.is_none() && !m.no_key {
                return Err(format!(
                    "模型「{}」还没有填 API Key。右键小球 → 设置 → 模型。",
                    if m.name.is_empty() { &m.model } else { &m.name }
                ));
            }
            let mut tools = self.tools_for(agent, &m, lead);
            let mut prompt = self.cfg.agent_prompt(agent);
            if let Some(store) = self.world.memory_store() {
                let facts = store.recent(&agent.id);
                if !facts.is_empty() {
                    prompt.push_str("\n\n独立记忆（用户事实与偏好）：\n");
                    for fact in facts {
                        prompt.push_str(&format!("- {fact}\n"));
                    }
                }
            }
            if self.plan_only {
                prompt.push_str(
                    "\n\n现在是只规划模式：只分析、读资料、给出方案和步骤，不要修改任何东西。",
                );
            }
            msgs = crate::prepare_messages(&prompt, m.vision, msgs);
            loop {
                self.budget().await?;
                self.steps.fetch_add(1, Ordering::Relaxed);
                let mut extra = json!({});
                if !tools.is_empty() {
                    extra["tools"] = json!(tools);
                }
                let emit = self.emit.clone();
                let r = llm::turn(
                    &m.endpoint(),
                    key.as_deref(),
                    &m.model,
                    &msgs,
                    &extra,
                    |ev| {
                        if lead {
                            emit(ev)
                        }
                    },
                )
                .await;
                let t = match r {
                    // 有的服务 / 模型不认 tools：去掉工具再问一次
                    Err(e)
                        if !tools.is_empty() && (e.contains("tool") || e.contains("function")) =>
                    {
                        tools.clear();
                        continue;
                    }
                    r => r?,
                };
                self.tokens.fetch_add(t.tokens, Ordering::Relaxed);
                if t.tokens > 0 {
                    self.has_usage.store(true, Ordering::Relaxed);
                }
                self.prompt.fetch_add(t.prompt, Ordering::Relaxed);
                self.completion.fetch_add(t.completion, Ordering::Relaxed);
                let local = crate::catalog::is_local_host(
                    &crate::catalog::host_of(&m.base_url).unwrap_or_default(),
                );
                let cost = if !local && t.tokens > 0 {
                    self.price_for(&m.base_url, &m.model, &m.provider, t.prompt, t.completion)
                } else {
                    None
                };
                self.add_estimate(cost);
                if let Some(stats) = &stats {
                    stats.tokens.fetch_add(t.tokens, Ordering::Relaxed);
                    if t.tokens > 0 {
                        stats.has_usage.store(true, Ordering::Relaxed);
                    }
                    stats.prompt.fetch_add(t.prompt, Ordering::Relaxed);
                    stats.completion.fetch_add(t.completion, Ordering::Relaxed);
                    let mut total = stats.cost.lock().unwrap();
                    *total = match (*total, cost) {
                        (Some(a), Some(b)) => Some(a + b),
                        _ => None,
                    };
                }
                if t.calls.is_empty() {
                    return Ok(t.content);
                }
                msgs.push(json!({
                    "role": "assistant",
                    "content": if t.content.is_empty() { Value::Null } else { json!(t.content) },
                    "tool_calls": t.calls.iter().map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.args}})).collect::<Vec<_>>()
                }));
                // 交给别的 Agent 的几步同时做（受「最多同时子 Agent」限制）；其余工具按顺序做
                let previous = self.last_delegate.lock().unwrap().clone();
                let jobs = t
                    .calls
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.name == "delegate")
                    .enumerate()
                    .map(|(step, (i, c))| {
                        let from = if step == 0 { previous.as_ref() } else { None };
                        async move { (i, self.delegate(c, from).await) }
                    });
                let mut results: Vec<Option<String>> = vec![None; t.calls.len()];
                let mut delegated = Vec::new();
                for (i, (result, summary)) in futures_util::future::join_all(jobs).await {
                    results[i] = Some(result);
                    delegated.push(summary);
                }
                for pair in delegated.windows(2) {
                    if let [Some(from), Some(to)] = pair {
                        self.emit_handoff(from, &to.agent_id, &to.agent, &to.call);
                    }
                }
                if let Some(last) = delegated.pop() {
                    *self.last_delegate.lock().unwrap() = last;
                }
                for (i, c) in t.calls.iter().enumerate() {
                    if results[i].is_none() {
                        results[i] =
                            Some(self.call(agent, c, stats.as_ref().map(Arc::clone)).await);
                    }
                }
                for (c, r) in t.calls.iter().zip(results) {
                    msgs.push(json!({"role": "tool", "tool_call_id": c.id, "content": r.unwrap_or_default()}));
                }
            }
        })
    }
}

#[cfg(test)]
mod tests;
