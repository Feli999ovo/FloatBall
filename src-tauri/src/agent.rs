//! Agent：模型 + 工具的循环。主控回答提问，需要时调用工具，或把一步交给别的 Agent（delegate）。
//!
//! 工具按风险分三档（设置 → 权限）：
//! - 只读（搜索网页、打开网页读内容、读文件、看文件夹）：自动允许
//! - 写入（写文件）、打开浏览器：每个范围第一次问，允许后这次任务里不再问
//! - 发送（发邮件）、删除：每次都问；可以「总是允许」，记在设置里，可撤销
//! 预算（设置 → Agents）：单个任务的 token 上限、最多步数（每次请求模型算一步）、最多同时几个子 Agent。

use crate::config::{Agent, Allow, Config, Model};
use crate::llm::{self, ChatEvent};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{oneshot, Semaphore};

/// 用户对确认卡片的回答
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
#[serde(default)]
pub struct Answer {
    pub allow: bool,
    pub always: bool,
    /// 拒绝时的原因，会告诉 Agent
    pub reason: String,
    /// 「修改后允许」：改过的内容（邮件正文、文件内容等），空着就照原样
    pub edit: String,
}

impl Default for Answer {
    fn default() -> Self {
        Self { allow: false, always: false, reason: String::new(), edit: String::new() }
    }
}

/// 等待中的确认，按编号找回调
#[derive(Default)]
pub struct Asks {
    next: AtomicU64,
    wait: Mutex<HashMap<u64, oneshot::Sender<Answer>>>,
}

impl Asks {
    pub fn answer(&self, id: u64, a: Answer) -> bool {
        self.wait.lock().unwrap().remove(&id).is_some_and(|tx| tx.send(a).is_ok())
    }
}

/// 外部世界：真实程序里打开浏览器 / 邮件、联网搜索；测试里换成假的
pub trait World: Send + Sync {
    fn open(&self, url: &str) -> Result<(), String>;
    fn search<'a>(&'a self, q: &'a str) -> futures_util::future::BoxFuture<'a, Result<String, String>>;
    fn fetch<'a>(&'a self, url: &'a str) -> futures_util::future::BoxFuture<'a, Result<String, String>>;
    /// 把总是允许存进设置
    fn remember(&self, a: Allow);
    fn memory_store(&self) -> Option<&dyn MemoryStore>;
}

pub trait MemoryStore: Send + Sync {
    fn recent(&self, agent: &str) -> Vec<String>;
    fn add(&self, agent: &str, text: &str) -> Result<i64, String>;
}

#[derive(Default)]
struct DelegateStats {
    tokens: std::sync::atomic::AtomicU64,
    prompt: std::sync::atomic::AtomicU64,
    completion: std::sync::atomic::AtomicU64,
    cost: Mutex<Option<f64>>,
    has_usage: std::sync::atomic::AtomicBool,
    writes: std::sync::atomic::AtomicUsize,
}

#[derive(Clone)]
struct DelegateSummary {
    call: String,
    agent_id: String,
    agent: String,
    count: u64,
    unit: String,
    cost: Option<f64>,
}

pub struct Task {
    pub cfg: Config,
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

/// 预算用满时每次「再给」加多少（照原型「再给 2万」）
pub const MORE_TOKENS: u64 = 20_000;
pub const MORE_STEPS: u64 = 20;

/// 工具定义（OpenAI function calling）。group 是设置里「能用的工具」那一项
struct ToolDef {
    name: &'static str,
    group: &'static str,
    desc: &'static str,
    params: fn() -> Value,
}

fn obj(props: &[(&str, &str)]) -> Value {
    let mut p = serde_json::Map::new();
    for (k, d) in props {
        p.insert(k.to_string(), json!({"type": "string", "description": d}));
    }
    json!({"type": "object", "properties": p, "required": props.iter().map(|x| x.0).collect::<Vec<_>>()})
}

const DEFS: &[ToolDef] = &[
    ToolDef { name: "web_search", group: "搜索网页", desc: "联网搜索，返回前几条结果的标题、链接和摘要", params: || obj(&[("query", "搜索词")]) },
    ToolDef { name: "fetch_page", group: "搜索网页", desc: "读取一个网页的正文文字", params: || obj(&[("url", "网址，http 或 https 开头")]) },
    ToolDef { name: "read_file", group: "读文件", desc: "读取本机一个文本文件（最多 200 KB）", params: || obj(&[("path", "文件的完整路径")]) },
    ToolDef { name: "list_dir", group: "读文件", desc: "列出本机一个文件夹里的文件和子文件夹", params: || obj(&[("path", "文件夹的完整路径")]) },
    ToolDef { name: "write_file", group: "写文件", desc: "把文字写进本机一个文件（已存在就覆盖）", params: || obj(&[("path", "文件的完整路径"), ("content", "要写入的全部内容")]) },
    ToolDef { name: "delete_file", group: "删除", desc: "删除本机一个文件（移到悬浮球回收站，可以找回）", params: || obj(&[("path", "文件的完整路径")]) },
    ToolDef { name: "send_email", group: "发邮件", desc: "在用户的邮件程序里打开一封写好的邮件，用户确认后由用户点发送", params: || obj(&[("to", "收件人邮箱"), ("subject", "主题"), ("body", "正文")]) },
    ToolDef { name: "open_url", group: "浏览器", desc: "在用户的浏览器里打开一个网页", params: || obj(&[("url", "网址，http 或 https 开头")]) },
];

/// 风险：0 只读，1 写入/联网（每个范围问一次），2 发送/删除（每次问）
fn risk(name: &str) -> u8 {
    match name {
        "write_file" | "open_url" => 1,
        "delete_file" | "send_email" => 2,
        _ => 0,
    }
}

fn arg(a: &Value, k: &str) -> String {
    a.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string()
}

fn short(s: &str, n: usize) -> String {
    let c: Vec<char> = s.chars().collect();
    if c.len() > n { c[..n].iter().collect::<String>() + "…" } else { s.to_string() }
}

/// 文件所在文件夹的名字（「总是允许」按它记范围）
fn folder(p: &str) -> String {
    Path::new(p).parent().and_then(|d| d.file_name()).map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "根目录".into())
}

fn host(u: &str) -> String {
    u.split("://").nth(1).unwrap_or(u).split(['/', '?', '#']).next().unwrap_or("").to_string()
}

fn http_url(u: &str) -> Result<(), String> {
    if u.starts_with("http://") || u.starts_with("https://") { Ok(()) } else { Err("网址要以 http:// 或 https:// 开头".into()) }
}

/// 确认卡片上的说明：做什么、范围、种类
fn describe(name: &str, a: &Value) -> (String, String, String) {
    match name {
        "write_file" => (format!("写入文件 {}", arg(a, "path")), folder(&arg(a, "path")), "写入".into()),
        "open_url" => (format!("在浏览器打开 {}", arg(a, "url")), host(&arg(a, "url")), "打开网页".into()),
        "delete_file" => (format!("删除 {}", arg(a, "path")), folder(&arg(a, "path")), "删除".into()),
        "send_email" => (format!("发送邮件给 {}：{}", arg(a, "to"), arg(a, "subject")), arg(a, "to"), "发送".into()),
        _ => (name.into(), String::new(), name.into()),
    }
}

/// 药丸上显示的工具名和参数
fn label(name: &str, a: &Value) -> (String, String) {
    let (n, v) = match name {
        "web_search" => ("搜索网页", arg(a, "query")),
        "fetch_page" => ("读取网页", host(&arg(a, "url"))),
        "read_file" => ("读取文件", file_name(&arg(a, "path"))),
        "list_dir" => ("查看文件夹", arg(a, "path")),
        "write_file" => ("写入文件", file_name(&arg(a, "path"))),
        "delete_file" => ("删除文件", file_name(&arg(a, "path"))),
        "send_email" => ("发送邮件", arg(a, "to")),
        "open_url" => ("打开网页", host(&arg(a, "url"))),
        _ => (name, String::new()),
    };
    (n.to_string(), short(&v, 40))
}

fn file_name(p: &str) -> String {
    Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string())
}

fn handoff_summary(result: &str, writes: u64) -> (u64, String) {
    if writes > 0 { return (writes, "份文件".into()); }
    let count = result.split("\n\n").filter(|part| !part.trim().is_empty()).count() as u64;
    (count, "段结果".into())
}

impl Task {
    pub fn new(cfg: Config, world: Arc<dyn World>, asks: Arc<Asks>, trash: PathBuf, emit: Arc<dyn Fn(ChatEvent) + Send + Sync>) -> Arc<Self> {
        let par = cfg.limits.parallel.max(1) as usize;
        Arc::new(Self { cfg, world, asks, trash, emit, tokens: AtomicU64::new(0), prompt: AtomicU64::new(0), completion: AtomicU64::new(0), cost: Mutex::new(Some(0.0)), has_usage: Default::default(), steps: AtomicU64::new(0), allowed: Mutex::new(HashSet::new()), par: Semaphore::new(par), seq: AtomicU64::new(0), more_tokens: AtomicU64::new(0), more_steps: AtomicU64::new(0), budget_ask: tokio::sync::Mutex::new(()), budget_stop: Default::default(), catalog: Mutex::new(None), last_delegate: Mutex::new(None) })
    }

    fn price_for(&self, base: &str, model: &str, provider: &str, prompt: u64, completion: u64) -> Option<f64> {
        if let Some(cache) = self.catalog.lock().ok().and_then(|guard| guard.clone()) {
            return crate::catalog::estimate_cny(cache.price(base, model, provider), prompt, completion, self.cfg.usd_cny);
        }
        None
    }

    pub fn tokens(&self) -> u64 { self.tokens.load(Ordering::Relaxed) }
    pub fn estimate(&self) -> Option<f64> {
        self.has_usage.load(Ordering::Relaxed).then(|| *self.cost.lock().unwrap()).flatten()
    }

    fn add_estimate(&self, amount: Option<f64>) {
        let mut total = self.cost.lock().unwrap();
        *total = match (*total, amount) { (Some(a), Some(b)) => Some(a + b), _ => None };
    }

    /// 用满了哪一项：(种类, 已用, 上限)
    fn over(&self) -> Option<(&'static str, u64, u64)> {
        let l = &self.cfg.limits;
        let tl = l.tokens + self.more_tokens.load(Ordering::Relaxed);
        if l.tokens > 0 && self.tokens() >= tl {
            return Some(("tokens", self.tokens(), tl));
        }
        let sl = l.steps as u64 + self.more_steps.load(Ordering::Relaxed);
        let st = self.steps.load(Ordering::Relaxed);
        if st >= sl {
            return Some(("steps", st, sl));
        }
        None
    }

    /// 预算用满：暂停，问用户「再给一些 / 停止」（照原型的预算卡）。再给就加上继续，停止或没人回答就结束
    async fn budget(&self) -> Result<(), String> {
        if self.over().is_none() {
            return Ok(());
        }
        let _one = self.budget_ask.lock().await;
        // 排队时别人可能已经问过、用户已经加了
        let Some((kind, used, limit)) = self.over() else { return Ok(()) };
        let stopped = self.budget_stop.load(Ordering::Relaxed);
        let more = if kind == "tokens" { MORE_TOKENS } else { MORE_STEPS };
        let r = if stopped { None } else { self.budget_wait(kind, used, limit, more).await };
        if r.is_some_and(|a| a.allow) {
            let add = if kind == "tokens" { &self.more_tokens } else { &self.more_steps };
            // 加到「已用 + 再给」，一步超了很多时也真能再走
            add.fetch_add(used.saturating_sub(limit) + more, Ordering::Relaxed);
            return Ok(());
        }
        self.budget_stop.store(true, Ordering::Relaxed);
        Err(if kind == "tokens" {
            format!("已用 {used} tokens，到了单个任务的上限（设置 → Agents → 预算），已停止")
        } else {
            format!("已经走了 {used} 步，到了最多步数（设置 → Agents → 预算），已停止")
        })
    }

    async fn budget_wait(&self, kind: &str, used: u64, limit: u64, more: u64) -> Option<Answer> {
        let id = self.asks.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.asks.wait.lock().unwrap().insert(id, tx);
        (self.emit)(ChatEvent::Budget { ask: id, kind: kind.into(), used, limit, more });
        let r = if self.cfg.pause_away { rx.await.ok() } else { tokio::time::timeout(Duration::from_secs(300), rx).await.ok().and_then(|r| r.ok()) };
        self.asks.wait.lock().unwrap().remove(&id);
        r
    }

    /// 这个 Agent 用哪个模型：自动省钱时优先用本地免费的，其次自己选的，再次默认
    fn model(&self, a: &Agent) -> Option<Model> {
        self.cfg.effective_model_for(a).cloned()
    }

    fn tools_for(&self, a: &Agent, m: &Model, lead: bool) -> Vec<Value> {
        if m.tools == Some(false) {
            return vec![];
        }
        let mut v: Vec<Value> = DEFS
            .iter()
            .filter(|d| a.tools.iter().any(|t| t == d.group))
            .map(|d| json!({"type": "function", "function": {"name": d.name, "description": d.desc, "parameters": (d.params)()}}))
            .collect();
        if self.world.memory_store().is_some() {
            v.push(json!({"type":"function","function":{
                "name":"remember",
                "description":"记住以后对用户有用的事实或偏好。只存一句话，别存密码、密钥或敏感信息。",
                "parameters":obj(&[("text", "简短的事实或偏好")])
            }}));
        }
        let others: Vec<&Agent> = self.cfg.agents.iter().skip(1).collect();
        if lead && !others.is_empty() {
            let list = others.iter().map(|o| format!("{}（{}）", o.name, if o.role.is_empty() { "没写职责" } else { &o.role })).collect::<Vec<_>>().join("；");
            v.push(json!({"type": "function", "function": {
                "name": "delegate",
                "description": format!("把一个步骤交给别的 Agent 去做，返回它的结果。可以在一轮里同时交给几个。可选：{list}"),
                "parameters": obj(&[("agent", "Agent 的名字"), ("task", "交给它的具体任务，写清楚要什么结果")])
            }}));
        }
        v
    }

    /// 让一个 Agent 跑完：请求模型 → 执行工具 → 再请求，直到它不再调用工具。
    /// lead：主控的正文流给界面；子 Agent 的正文只作为 delegate 的结果。
    pub fn run<'a>(self: &'a Arc<Self>, agent: &'a Agent, msgs: Vec<Value>, lead: bool) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
        self.run_acc(agent, msgs, lead, None)
    }

    /// 同 run；acc 另外记下这个 Agent 自己用掉的 token（多 Agent 计划卡每一步显示）
    fn run_acc<'a>(self: &'a Arc<Self>, agent: &'a Agent, mut msgs: Vec<Value>, lead: bool, stats: Option<Arc<DelegateStats>>) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let m = self.model(agent).ok_or("还没有设置模型。右键小球 → 设置，添加一个模型。")?;
            let key = crate::config::get_api_key(m.id);
            if key.is_none() && !m.no_key {
                return Err(format!("模型「{}」还没有填 API Key。右键小球 → 设置 → 模型。", if m.name.is_empty() { &m.model } else { &m.name }));
            }
            let mut tools = self.tools_for(agent, &m, lead);
            let mut prompt = self.cfg.agent_prompt(agent);
            if let Some(store) = self.world.memory_store() {
                let facts = store.recent(&agent.id);
                if !facts.is_empty() {
                    prompt.push_str("\n\n独立记忆（用户事实与偏好）：\n");
                    for fact in facts { prompt.push_str(&format!("- {fact}\n")); }
                }
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
                let r = llm::turn(&m.endpoint(), key.as_deref(), &m.model, &msgs, &extra, |ev| {
                    if lead {
                        emit(ev)
                    }
                })
                .await;
                let t = match r {
                    // 有的服务 / 模型不认 tools：去掉工具再问一次
                    Err(e) if !tools.is_empty() && (e.contains("tool") || e.contains("function")) => {
                        tools.clear();
                        continue;
                    }
                    r => r?,
                };
                self.tokens.fetch_add(t.tokens, Ordering::Relaxed);
                if t.tokens > 0 { self.has_usage.store(true, Ordering::Relaxed); }
                self.prompt.fetch_add(t.prompt, Ordering::Relaxed);
                self.completion.fetch_add(t.completion, Ordering::Relaxed);
                let local = crate::catalog::is_local_host(&crate::catalog::host_of(&m.base_url).unwrap_or_default());
                let cost = if !local && t.tokens > 0 { self.price_for(&m.base_url, &m.model, &m.provider, t.prompt, t.completion) } else { None };
                self.add_estimate(cost);
                if let Some(stats) = &stats {
                    stats.tokens.fetch_add(t.tokens, Ordering::Relaxed);
                    if t.tokens > 0 { stats.has_usage.store(true, Ordering::Relaxed); }
                    stats.prompt.fetch_add(t.prompt, Ordering::Relaxed);
                    stats.completion.fetch_add(t.completion, Ordering::Relaxed);
                    let mut total = stats.cost.lock().unwrap();
                    *total = match (*total, cost) { (Some(a), Some(b)) => Some(a + b), _ => None };
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
                let jobs = t.calls.iter().enumerate().filter(|(_, c)| c.name == "delegate").enumerate().map(|(step, (i, c))| {
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
                        results[i] = Some(self.call(agent, c, stats.as_ref().map(Arc::clone)).await);
                    }
                }
                for (c, r) in t.calls.iter().zip(results) {
                    msgs.push(json!({"role": "tool", "tool_call_id": c.id, "content": r.unwrap_or_default()}));
                }
            }
        })
    }

    fn pill(&self, call: &str, agent: &Agent, name: &str, arg: &str, status: &str, result: &str) {
        (self.emit)(ChatEvent::Tool {
            call: call.into(), agent: agent.name.clone(), name: name.into(), arg: arg.into(),
            status: status.into(), result: short(result, 600), memory_id: None, agent_id: None,
        });
    }

    fn memory_pill(&self, call: &str, agent: &Agent, text: &str, status: &str, result: &str, undo_id: Option<i64>) {
        let name = format!("已记住：{}", text.chars().take(30).collect::<String>());
        (self.emit)(ChatEvent::Tool {
            call: call.into(), agent: agent.name.clone(), name, arg: String::new(),
            status: status.into(), result: short(result, 600),
            memory_id: undo_id, agent_id: undo_id.map(|_| agent.id.clone()),
        });
    }

    fn emit_handoff(&self, from: &DelegateSummary, to_id: &str, to_name: &str, to_call: &str) {
        if from.agent_id != to_id {
            (self.emit)(ChatEvent::Handoff { from_call: from.call.clone(), to_call: to_call.into(), from: from.agent.clone(), to: to_name.into(), count: from.count, unit: from.unit.clone(), cost: from.cost });
        }
    }

    async fn delegate(self: &Arc<Self>, c: &llm::Call, previous: Option<&DelegateSummary>) -> (String, Option<DelegateSummary>) {
        let a: Value = serde_json::from_str(&c.args).unwrap_or_default();
        let who = arg(&a, "agent");
        let task = arg(&a, "task");
        let call = format!("t{}", self.seq.fetch_add(1, Ordering::Relaxed));
        let Some(to) = self.cfg.agents.iter().skip(1).find(|x| x.name == who || x.id == who).cloned() else {
            return (format!("没有叫「{who}」的 Agent"), None);
        };
        let model = self.model(&to).map(|m| if m.name.is_empty() { m.model.clone() } else { m.name.clone() }).unwrap_or_default();
        let model_id = self.model(&to).map(|m| m.id).unwrap_or(0);
        let team = |status: &str, tokens: u64, prompt: u64, completion: u64, cost: Option<f64>| {
            (self.emit)(ChatEvent::Team { call: call.clone(), agent_id: to.id.clone(), agent: to.name.clone(), task: short(&task, 80), model: model.clone(), model_id, status: status.into(), tokens, prompt, completion, cost })
        };
        // 先排队（受「最多同时子 Agent」限制），轮到了才算进行中
        team("wait", 0, 0, 0, None);
        let _permit = self.par.acquire().await;
        if let Some(from) = previous {
            self.emit_handoff(from, &to.id, &to.name, &call);
        }
        team("run", 0, 0, 0, None);
        let stats = Arc::new(DelegateStats { cost: Mutex::new(Some(0.0)), has_usage: Default::default(), ..DelegateStats::default() });
        let r = self.run_acc(&to, vec![json!({"role": "user", "content": task})], false, Some(stats.clone())).await;
        let (tokens, prompt, completion) = (stats.tokens.load(Ordering::Relaxed), stats.prompt.load(Ordering::Relaxed), stats.completion.load(Ordering::Relaxed));

        match r {
            Ok(out) => {
                let cost = stats.has_usage.load(Ordering::Relaxed).then(|| *stats.cost.lock().unwrap()).flatten();
                team("done", tokens, prompt, completion, cost);
                let (count, unit) = handoff_summary(&out, stats.writes.load(Ordering::Relaxed) as u64);
                (out, Some(DelegateSummary { call, agent_id: to.id, agent: to.name, count, unit, cost }))
            }
            Err(e) => {
                team("err", tokens, prompt, completion, None);
                (format!("出错了：{e}"), None)
            }
        }
    }

    /// 执行一个工具调用，返回给模型的结果文字
    async fn call(self: &Arc<Self>, agent: &Agent, c: &llm::Call, stats: Option<Arc<DelegateStats>>) -> String {
        let call = format!("t{}", self.seq.fetch_add(1, Ordering::Relaxed));
        let mut a: Value = match serde_json::from_str(if c.args.trim().is_empty() { "{}" } else { &c.args }) {
            Ok(v) => v,
            Err(_) => return "参数不是合法的 JSON".into(),
        };
        if c.name == "remember" {
            let Some(store) = self.world.memory_store() else { return "当前对话不能写入记忆".into() };
            if self.model(agent).is_some_and(|m| m.tools == Some(false)) { return "当前模型不能使用工具".into(); }
            let text = arg(&a, "text").split_whitespace().collect::<Vec<_>>().join(" ");
            if text.is_empty() || text.chars().count() > 500 { return "请提供 1 至 500 字的简短事实或偏好".into(); }
            let text = if self.cfg.memory_ask {
                let ans = self.ask(&call, "记住", &format!("记住：{text}"), &agent.name, false, true).await;
                if !ans.allow {
                    let why = if ans.reason.is_empty() {
                        "用户拒绝记住这条".to_string()
                    } else {
                        format!("用户拒绝记住这条，原因：{}", ans.reason)
                    };
                    self.pill(&call, agent, "记住偏好", "", "no", &why);
                    return why;
                }
                if !ans.edit.trim().is_empty() {
                    ans.edit.split_whitespace().collect::<Vec<_>>().join(" ")
                } else {
                    text
                }
            } else {
                text
            };
            if text.is_empty() || text.chars().count() > 500 {
                return "请提供 1 至 500 字的简短事实或偏好".into();
            }
            return match store.add(&agent.id, &text) {
                Ok(id) => {
                    let undo_id = (!self.cfg.memory_ask).then_some(id);
                    self.memory_pill(&call, agent, &text, "done", "已保存到独立记忆", undo_id);
                    "已记住".into()
                }
                Err(e) => format!("未能保存记忆：{e}"),
            };
        }
        let Some(def) = DEFS.iter().find(|d| d.name == c.name) else {
            return format!("没有这个工具：{}", c.name);
        };
        let (name, shown) = label(&c.name, &a);
        if !agent.tools.iter().any(|t| t == def.group) {
            return format!("{} 没有「{}」这个工具的权限", agent.name, def.group);
        }
        // 需要确认的先问
        let rk = risk(&c.name);
        if rk > 0 {
            let (desc, scope, kind) = describe(&c.name, &a);
            let key = format!("{kind} · {scope}");
            let always = self.cfg.always_allow.iter().any(|x| x.kind == kind && x.scope == scope);
            let once = rk == 1 && self.allowed.lock().unwrap().contains(&key);
            if !always && !once {
                self.pill(&call, agent, &name, &shown, "wait", "");
                let ans = self.ask(&call, &kind, &desc, &scope, rk == 2, false).await;
                if !ans.allow {
                    let why = if ans.reason.is_empty() { "用户拒绝了这个操作".to_string() } else { format!("用户拒绝了这个操作，原因：{}", ans.reason) };
                    self.pill(&call, agent, &name, &shown, "no", &why);
                    return why;
                }
                if !ans.edit.trim().is_empty() {
                    let k = match c.name.as_str() { "write_file" => "content", "send_email" => "body", "open_url" => "url", _ => "path" };
                    a[k] = json!(ans.edit.trim());
                }
                if ans.always {
                    self.world.remember(Allow { kind: kind.clone(), scope: scope.clone() });
                }
                self.allowed.lock().unwrap().insert(key);
            }
        }
        self.pill(&call, agent, &name, &shown, "run", "");
        let r = self.exec(&c.name, &a).await;
        if c.name == "write_file" && r.is_ok() {
            if let Some(stats) = stats { stats.writes.fetch_add(1, Ordering::Relaxed); }
        }
        match &r {
            Ok(out) => self.pill(&call, agent, &name, &shown, "done", out),
            Err(e) => self.pill(&call, agent, &name, &shown, "err", e),
        }
        r.unwrap_or_else(|e| format!("出错了：{e}"))
    }

    async fn ask(&self, call: &str, kind: &str, desc: &str, scope: &str, hi: bool, no_always: bool) -> Answer {
        let id = self.asks.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.asks.wait.lock().unwrap().insert(id, tx);
        (self.emit)(ChatEvent::Ask { ask: id, call: call.into(), kind: kind.into(), desc: desc.into(), scope: scope.into(), hi, no_always });
        // 「离开电脑时暂停敏感操作」开着：一直等到回来确认；关着：5 分钟没确认就跳过
        let r = if self.cfg.pause_away {
            rx.await.ok()
        } else {
            tokio::time::timeout(Duration::from_secs(300), rx).await.ok().and_then(|r| r.ok())
        };
        self.asks.wait.lock().unwrap().remove(&id);
        r.unwrap_or(Answer { reason: "5 分钟没有确认，已跳过".into(), ..Answer::default() })
    }

    async fn exec(&self, name: &str, a: &Value) -> Result<String, String> {
        match name {
            "web_search" => self.world.search(&arg(a, "query")).await,
            "fetch_page" => {
                let u = arg(a, "url");
                http_url(&u)?;
                self.world.fetch(&u).await
            }
            "read_file" => crate::capture::read_text_file(&arg(a, "path")).map(|(_, t)| t),
            "list_dir" => list_dir(&arg(a, "path")),
            "write_file" => {
                let p = arg(a, "path");
                if p.is_empty() {
                    return Err("没有给文件路径".into());
                }
                let content = a.get("content").and_then(Value::as_str).unwrap_or("");
                if let Some(d) = Path::new(&p).parent() {
                    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                }
                std::fs::write(&p, content).map_err(|e| e.to_string())?;
                Ok(format!("已写入 {p}（{} 字）", content.chars().count()))
            }
            "delete_file" => {
                let p = PathBuf::from(arg(a, "path"));
                if !p.is_file() {
                    return Err("找不到这个文件（文件夹不能删）".into());
                }
                std::fs::create_dir_all(&self.trash).map_err(|e| e.to_string())?;
                let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
                let to = self.trash.join(format!("{stamp}-{}", file_name(&p.to_string_lossy())));
                if std::fs::rename(&p, &to).is_err() {
                    // 不在同一个磁盘：先复制再删
                    std::fs::copy(&p, &to).map_err(|e| e.to_string())?;
                    std::fs::remove_file(&p).map_err(|e| e.to_string())?;
                }
                Ok(format!("已删除 {}，移到了悬浮球回收站：{}", p.display(), to.display()))
            }
            "send_email" => {
                let enc = |s: &str| s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~@".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect::<String>();
                let url = format!("mailto:{}?subject={}&body={}", enc(&arg(a, "to")), enc(&arg(a, "subject")), enc(a.get("body").and_then(Value::as_str).unwrap_or("")));
                self.world.open(&url)?;
                Ok("已在邮件程序里打开这封邮件，等用户点发送".into())
            }
            "open_url" => {
                let u = arg(a, "url");
                http_url(&u)?;
                self.world.open(&u)?;
                Ok(format!("已在浏览器打开 {u}"))
            }
            _ => Err("没有这个工具".into()),
        }
    }
}

fn list_dir(p: &str) -> Result<String, String> {
    let mut v: Vec<String> = std::fs::read_dir(p)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| {
            let dir = e.file_type().is_ok_and(|t| t.is_dir());
            let n = e.file_name().to_string_lossy().to_string();
            if dir { n + "/" } else { format!("{n}（{} 字节）", e.metadata().map_or(0, |m| m.len())) }
        })
        .collect();
    v.sort();
    let n = v.len();
    v.truncate(200);
    Ok(if n == 0 { "空文件夹".into() } else if n > 200 { v.join("\n") + &format!("\n…共 {n} 项，只列了前 200 项") } else { v.join("\n") })
}

// ───────── 真实的外部世界 ─────────

pub struct RealWorld {
    pub remember: Box<dyn Fn(Allow) + Send + Sync>,
    pub memory: Option<Box<dyn MemoryStore>>,
}

/// 网页 → 纯文字：去掉 script/style 和标签，合并空白，最多 8000 字
pub fn html_text(html: &str) -> String {
    let mut s = html.to_string();
    for tag in ["script", "style", "noscript", "svg"] {
        while let Some(i) = s.to_ascii_lowercase().find(&format!("<{tag}")) {
            let lower = s.to_ascii_lowercase();
            let end = lower[i..].find(&format!("</{tag}>")).map(|j| i + j + tag.len() + 3).unwrap_or(s.len());
            s.replace_range(i..end, " ");
        }
    }
    let mut out = String::new();
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&nbsp;", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'");
    let text = out.split_whitespace().collect::<Vec<_>>().join(" ");
    short(&text, 8000)
}

/// DuckDuckGo 的 HTML 版结果页 → 「标题 / 链接 / 摘要」
pub fn parse_ddg(html: &str) -> Vec<(String, String, String)> {
    let mut out = vec![];
    for part in html.split("class=\"result__a\"").skip(1) {
        let href = part.split("href=\"").nth(1).and_then(|x| x.split('"').next()).unwrap_or("");
        let title = part.split_once('>').map(|(_, x)| x.split("</a").next().unwrap_or(x)).unwrap_or("");
        let snip = part.split("result__snippet").nth(1).and_then(|x| x.split('>').skip(1).collect::<Vec<_>>().join(">").split("</a").next().map(str::to_string)).unwrap_or_default();
        let mut url = href.replace("&amp;", "&");
        if let Some(u) = url.split("uddg=").nth(1) {
            let u = u.split('&').next().unwrap_or(u);
            url = pct_decode(u);
        }
        if url.starts_with("//") {
            url = format!("https:{url}");
        }
        if !url.is_empty() {
            out.push((html_text(title), url, html_text(&snip)));
        }
        if out.len() >= 8 {
            break;
        }
    }
    out
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 <= b.len() - 1 {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

impl World for RealWorld {
    fn open(&self, url: &str) -> Result<(), String> {
        #[cfg(target_os = "windows")]
        let r = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
        #[cfg(target_os = "macos")]
        let r = std::process::Command::new("open").arg(url).spawn();
        #[cfg(all(unix, not(target_os = "macos")))]
        let r = std::process::Command::new("xdg-open").arg(url).spawn();
        r.map(|_| ()).map_err(|e| format!("打不开：{e}"))
    }

    fn search<'a>(&'a self, q: &'a str) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let url = "https://html.duckduckgo.com/html/";
            let r = llm::client(url).post(url).form(&[("q", q)]).header("User-Agent", "Mozilla/5.0 FloatBall").timeout(Duration::from_secs(20)).send().await.map_err(|e| format!("搜索连不上：{e}"))?;
            let html = r.text().await.map_err(|e| e.to_string())?;
            let res = parse_ddg(&html);
            if res.is_empty() {
                return Ok("没有搜到结果".into());
            }
            Ok(res.iter().enumerate().map(|(i, (t, u, s))| format!("{}. {t}\n{u}\n{s}", i + 1)).collect::<Vec<_>>().join("\n\n"))
        })
    }

    fn fetch<'a>(&'a self, url: &'a str) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let r = llm::client(url).get(url).header("User-Agent", "Mozilla/5.0 FloatBall").timeout(Duration::from_secs(20)).send().await.map_err(|e| format!("打不开：{e}"))?;
            if !r.status().is_success() {
                return Err(format!("网页返回 {}", r.status().as_u16()));
            }
            Ok(html_text(&r.text().await.map_err(|e| e.to_string())?))
        })
    }

    fn remember(&self, a: Allow) {
        (self.remember)(a)
    }
    fn memory_store(&self) -> Option<&dyn MemoryStore> { self.memory.as_deref() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Limits, Model};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// 本地假模型：按顺序回 SSE，记下每次收到的请求体
    fn server(replies: Vec<String>) -> (String, Arc<Mutex<Vec<Value>>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("http://{}/v1", l.local_addr().unwrap());
        let got = Arc::new(Mutex::new(vec![]));
        let g = got.clone();
        std::thread::spawn(move || {
            for body in replies {
                let Ok((mut s, _)) = l.accept() else { return };
                let mut req = vec![];
                let mut buf = vec![0u8; 65536];
                loop {
                    let n = s.read(&mut buf).unwrap();
                    req.extend_from_slice(&buf[..n]);
                    let t = String::from_utf8_lossy(&req).to_string();
                    if let Some(i) = t.find("\r\n\r\n") {
                        let len = t.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                        if req.len() >= i + 4 + len {
                            g.lock().unwrap().push(serde_json::from_slice(&req[i + 4..]).unwrap_or(Value::Null));
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
                s.write_all(head.as_bytes()).unwrap();
                s.write_all(body.as_bytes()).unwrap();
            }
        });
        (addr, got)
    }

    fn text(t: &str) -> String {
        format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":{"content":t}}]}), json!({"usage":{"prompt_tokens":10,"completion_tokens":5}}))
    }

    /// 工具调用分两段到达（名字和参数拆开），模拟真实的流
    fn tool(name: &str, args: Value) -> String {
        let a = args.to_string();
        let (a1, a2) = a.split_at(a.floor_char_boundary(a.len() / 2));
        format!(
            "data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":{"content":"","tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":name,"arguments":a1}}]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":a2}}]}}]}),
            json!({"usage":{"prompt_tokens":20,"completion_tokens":5}})
        )
    }

    struct Fake {
        opened: Mutex<Vec<String>>,
        remembered: Mutex<Vec<Allow>>,
        memory: Option<Mutex<crate::store::Store>>,
    }
    impl MemoryStore for Mutex<crate::store::Store> {
        fn recent(&self, agent: &str) -> Vec<String> {
            self.lock().unwrap().memory(agent, 20).unwrap().into_iter().map(|m| m.text).collect()
        }
        fn add(&self, agent: &str, text: &str) -> Result<i64, String> {
            self.lock().unwrap().remember(agent, text).map_err(|e| e.to_string())
        }
    }
    impl World for Fake {
        fn open(&self, url: &str) -> Result<(), String> {
            self.opened.lock().unwrap().push(url.into());
            Ok(())
        }
        fn search<'a>(&'a self, q: &'a str) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
            Box::pin(async move { Ok(format!("1. 关于{q}的结果\nhttps://example.com")) })
        }
        fn fetch<'a>(&'a self, _: &'a str) -> futures_util::future::BoxFuture<'a, Result<String, String>> {
            Box::pin(async { Ok("网页正文".into()) })
        }
        fn remember(&self, a: Allow) {
            self.remembered.lock().unwrap().push(a)
        }
        fn memory_store(&self) -> Option<&dyn MemoryStore> { self.memory.as_ref().map(|m| m as &dyn MemoryStore) }
    }

    struct Env {
        task: Arc<Task>,
        fake: Arc<Fake>,
        events: Arc<Mutex<Vec<ChatEvent>>>,
        got: Arc<Mutex<Vec<Value>>>,
        dir: tempfile::TempDir,
    }

    fn env(replies: Vec<String>, f: impl FnOnce(&mut Config)) -> Env {
        env_with_memory(replies, f, None)
    }

    fn env_with_memory(replies: Vec<String>, f: impl FnOnce(&mut Config), memory: Option<crate::store::Store>) -> Env {
        let (base, got) = server(replies);
        let mut cfg = Config::default();
        // 测试从只有主控开始；要子 Agent 的测试自己加
        cfg.agents.truncate(1);
        cfg.upsert(Model { model: "m".into(), base_url: base, no_key: true, tools: Some(true), ..Model::default() }).unwrap();
        f(&mut cfg);
        let fake = Arc::new(Fake { opened: Mutex::new(vec![]), remembered: Mutex::new(vec![]), memory: memory.map(Mutex::new) });
        let events = Arc::new(Mutex::new(vec![]));
        let ev = events.clone();
        let dir = tempfile::tempdir().unwrap();
        let task = Task::new(cfg, fake.clone(), Arc::new(Asks::default()), dir.path().join("回收站"), Arc::new(move |e| ev.lock().unwrap().push(e)));
        Env { task, fake, events, got, dir }
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    fn pills(e: &Env) -> Vec<(String, String, String)> {
        e.events.lock().unwrap().iter().filter_map(|x| match x { ChatEvent::Tool { name, arg, status, .. } => Some((name.clone(), arg.clone(), status.clone())), _ => None }).collect()
    }

    /// 等确认卡片出现，然后回答
    async fn answer(e: &Env, a: Answer) -> (String, String, bool) {
        for _ in 0..200 {
            let ask = e.events.lock().unwrap().iter().find_map(|x| match x { ChatEvent::Ask { ask, desc, scope, hi, .. } => Some((*ask, desc.clone(), scope.clone(), *hi)), _ => None });
            if let Some((id, d, s, hi)) = ask {
                assert!(e.task.asks.answer(id, a));
                return (d, s, hi);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("没有出现确认卡片")
    }

    #[test]
    fn reads_file_then_answers() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("报告.txt");
        std::fs::write(&f, "营收 +18%").unwrap();
        let e = env(vec![tool("read_file", json!({"path": f.to_string_lossy()})), text("营收涨了 18%")], |_| {});
        let lead = e.task.cfg.lead().clone();
        let out = rt().block_on(e.task.run(&lead, vec![json!({"role":"user","content":"总结报告"})], true)).unwrap();
        assert_eq!(out, "营收涨了 18%");
        assert_eq!(pills(&e), vec![("读取文件".into(), "报告.txt".into(), "run".into()), ("读取文件".into(), "报告.txt".into(), "done".into())]);
        let regular_tool = serde_json::to_value(e.events.lock().unwrap().iter().find(|event| matches!(event, ChatEvent::Tool { .. })).unwrap()).unwrap();
        assert!(regular_tool.get("memory_id").is_none() && regular_tool.get("agent_id").is_none());
        let got = e.got.lock().unwrap();
        // 第一次带上主控默认能用的工具（读文件、写文件）；搜索网页要用户自己勾，删除 / 发邮件 / 浏览器也不带
        let names: Vec<&str> = got[0]["tools"].as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap()).collect();
        assert_eq!(names, vec!["read_file", "list_dir", "write_file"]);
        assert_eq!(got[0]["messages"][0]["role"], "system");
        // 第二次把工具结果带回去
        let m = got[1]["messages"].as_array().unwrap();
        assert_eq!(m[m.len() - 1], json!({"role":"tool","tool_call_id":"c1","content":"营收 +18%"}));
        assert_eq!(m[m.len() - 2]["tool_calls"][0]["function"]["name"], "read_file");
        assert_eq!(e.task.tokens(), 25 + 15);
    }

    #[test]
    fn write_asks_once_and_always_remembers() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("out").join("a.md");
        let e = env(vec![tool("write_file", json!({"path": f.to_string_lossy(), "content": "# 周报"})), text("写好了")], |_| {});
        let lead = e.task.cfg.lead().clone();
        let r = rt();
        let (out, ans) = r.block_on(async {
            let t = e.task.clone();
            let h = tokio::spawn(async move { t.run(&t.cfg.lead().clone(), vec![json!({"role":"user","content":"写周报"})], true).await });
            let a = answer(&e, Answer { allow: true, always: true, ..Answer::default() }).await;
            (h.await.unwrap(), a)
        });
        let _ = lead;
        assert_eq!(out.unwrap(), "写好了");
        assert_eq!(ans, (format!("写入文件 {}", f.to_string_lossy()), "out".into(), false));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "# 周报");
        assert_eq!(*e.fake.remembered.lock().unwrap(), vec![Allow { kind: "写入".into(), scope: "out".into() }]);
        let ask = serde_json::to_value(e.events.lock().unwrap().iter().find(|event| matches!(event, ChatEvent::Ask { .. })).unwrap()).unwrap();
        assert!(ask.get("no_always").is_none());
        let st: Vec<String> = pills(&e).into_iter().map(|p| p.2).collect();
        assert_eq!(st, vec!["wait", "run", "done"]);
    }

    #[test]
    fn delete_denied_keeps_file_and_tells_why() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("旧.txt");
        std::fs::write(&f, "x").unwrap();
        let e = env(vec![tool("delete_file", json!({"path": f.to_string_lossy()})), text("好的，不删了")], |c| c.agents[0].tools.push("删除".into()));
        let out = rt().block_on(async {
            let t = e.task.clone();
            let h = tokio::spawn(async move { t.run(&t.cfg.lead().clone(), vec![json!({"role":"user","content":"删掉旧文件"})], true).await });
            let a = answer(&e, Answer { reason: "还要用".into(), ..Answer::default() }).await;
            assert!(a.2, "删除是高风险");
            h.await.unwrap()
        });
        assert_eq!(out.unwrap(), "好的，不删了");
        assert!(f.exists());
        let m = e.got.lock().unwrap()[1]["messages"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(m["content"], "用户拒绝了这个操作，原因：还要用");
        assert_eq!(pills(&e).last().unwrap().2, "no");
    }

    #[test]
    fn delete_allowed_moves_to_trash_and_always_allow_skips_ask() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("dl").join("旧.txt");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, "x").unwrap();
        let e = env(vec![tool("delete_file", json!({"path": f.to_string_lossy()})), text("删好了")], |c| {
            c.agents[0].tools.push("删除".into());
            c.always_allow.push(Allow { kind: "删除".into(), scope: "dl".into() });
        });
        let out = rt().block_on(e.task.run(&e.task.cfg.lead().clone(), vec![json!({"role":"user","content":"删"})], true));
        assert_eq!(out.unwrap(), "删好了");
        assert!(!f.exists());
        let trashed: Vec<_> = std::fs::read_dir(e.dir.path().join("回收站")).unwrap().collect();
        assert_eq!(trashed.len(), 1);
        assert!(!e.events.lock().unwrap().iter().any(|x| matches!(x, ChatEvent::Ask { .. })));
    }

    #[test]
    fn tool_not_granted_is_refused() {
        let e = env(vec![tool("send_email", json!({"to":"a@b.c","subject":"hi","body":"x"})), text("发不了")], |_| {});
        let out = rt().block_on(e.task.run(&e.task.cfg.lead().clone(), vec![json!({"role":"user","content":"发邮件"})], true));
        assert_eq!(out.unwrap(), "发不了");
        assert!(e.fake.opened.lock().unwrap().is_empty());
        let m = e.got.lock().unwrap()[1]["messages"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(m["content"], "主控 没有「发邮件」这个工具的权限");
    }

    #[test]
    fn email_opens_draft_after_edit() {
        let e = env(vec![tool("send_email", json!({"to":"wang@x.com","subject":"周报","body":"见附件"})), text("已打开")], |c| c.agents[0].tools.push("发邮件".into()));
        let out = rt().block_on(async {
            let t = e.task.clone();
            let h = tokio::spawn(async move { t.run(&t.cfg.lead().clone(), vec![json!({"role":"user","content":"发周报"})], true).await });
            let a = answer(&e, Answer { allow: true, edit: "改过的正文".into(), ..Answer::default() }).await;
            assert_eq!(a, ("发送邮件给 wang@x.com：周报".into(), "wang@x.com".into(), true));
            h.await.unwrap()
        });
        assert_eq!(out.unwrap(), "已打开");
        let o = e.fake.opened.lock().unwrap();
        assert_eq!(o[0], "mailto:wang@x.com?subject=%E5%91%A8%E6%8A%A5&body=%E6%94%B9%E8%BF%87%E7%9A%84%E6%AD%A3%E6%96%87");
    }

    #[test]
    fn ordinary_agents_can_remember_and_only_receive_the_latest_twenty() {
        let store = crate::store::Store::from_conn(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
        for n in 0..21 { store.remember("lead", &format!("fact-{n:02}")).unwrap(); }
        store.remember("other", "other-agent").unwrap();
        let e = env_with_memory(vec![tool("remember", json!({"text":"new preference"})), text("ok")], |_| {}, Some(store));
        let result = rt().block_on(e.task.run(e.task.cfg.lead(), vec![], true));
        assert_eq!(result.unwrap(), "ok");
        let got = e.got.lock().unwrap();
        assert!(got[0]["tools"].as_array().unwrap().iter().any(|t| t["function"]["name"] == "remember"));
        let prompt = got[0]["messages"][0]["content"].as_str().unwrap();
        assert!(!prompt.contains("fact-00") && !prompt.contains("other-agent"));
        for n in 1..21 { assert!(prompt.contains(&format!("fact-{n:02}"))); }
        assert_eq!(e.fake.memory.as_ref().unwrap().lock().unwrap().memory("lead", 1).unwrap()[0].text, "new preference");
    }

    #[test]
    fn automatic_memory_event_carries_the_inserted_scoped_row_id() {
        let store = crate::store::Store::from_conn(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
        let e = env_with_memory(vec![tool("remember", json!({"text":"用户喜欢短答案"})), text("已记下")], |_| {}, Some(store));
        let out = rt().block_on(e.task.run(e.task.cfg.lead(), vec![], true));
        assert_eq!(out.unwrap(), "已记下");
        let rows = e.fake.memory.as_ref().unwrap().lock().unwrap().memory("lead", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "用户喜欢短答案");
        let event = e.events.lock().unwrap().iter().find_map(|event| match event {
            ChatEvent::Tool { memory_id, agent_id, name, arg, status, .. } => Some((*memory_id, agent_id.clone(), name.clone(), arg.clone(), status.clone())),
            _ => None,
        }).unwrap();
        assert_eq!((event.0, event.1.as_deref(), event.2.as_str(), event.3.as_str(), event.4.as_str()), (Some(rows[0].id), Some("lead"), "已记住：用户喜欢短答案", "", "done"));
        let serialized = serde_json::to_value(e.events.lock().unwrap().iter().find(|ev| matches!(ev, ChatEvent::Tool { .. })).unwrap()).unwrap();
        assert_eq!(serialized["memory_id"], rows[0].id);
        assert_eq!(serialized["agent_id"], "lead");
        assert!(serialized.get("memoryId").is_none());
    }

    #[test]
    fn memory_ask_refusal_does_not_write_and_disallows_always() {
        let store = crate::store::Store::from_conn(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
        let e = env_with_memory(vec![tool("remember", json!({"text":"用户喜欢短答案"})), text("明白")], |c| c.memory_ask = true, Some(store));
        let r = rt();
        let (out, result) = r.block_on(async {
            let t = e.task.clone();
            let h = tokio::spawn(async move { t.run(&t.cfg.lead().clone(), vec![], true).await });
            let (desc, _, _) = answer(&e, Answer { reason: "这条不准确".into(), ..Answer::default() }).await;
            (h.await.unwrap(), desc)
        });
        assert_eq!(out.unwrap(), "明白");
        assert_eq!(result, "记住：用户喜欢短答案");
        assert!(e.fake.memory.as_ref().unwrap().lock().unwrap().memory("lead", 10).unwrap().is_empty());
        let events = e.events.lock().unwrap();
        assert!(events.iter().any(|event| matches!(event, ChatEvent::Ask { no_always: true, .. })));
        let ask = events.iter().find(|event| matches!(event, ChatEvent::Ask { .. })).unwrap();
        assert_eq!(serde_json::to_value(ask).unwrap()["no_always"], true);
        assert_eq!(e.got.lock().unwrap()[1]["messages"].as_array().unwrap().last().unwrap()["content"], "用户拒绝记住这条，原因：这条不准确");
    }

    #[test]
    fn memory_ask_writes_the_edited_text_without_undo_ids() {
        let store = crate::store::Store::from_conn(rusqlite::Connection::open_in_memory().unwrap()).unwrap();
        let e = env_with_memory(vec![tool("remember", json!({"text":"原始偏好"})), text("好的")], |c| c.memory_ask = true, Some(store));
        let r = rt();
        let out = r.block_on(async {
            let t = e.task.clone();
            let h = tokio::spawn(async move { t.run(&t.cfg.lead().clone(), vec![], true).await });
            let (desc, _, _) = answer(&e, Answer { allow: true, edit: "修改后的偏好".into(), ..Answer::default() }).await;
            assert_eq!(desc, "记住：原始偏好");
            h.await.unwrap()
        });
        assert_eq!(out.unwrap(), "好的");
        let rows = e.fake.memory.as_ref().unwrap().lock().unwrap().memory("lead", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "修改后的偏好");
        assert!(e.events.lock().unwrap().iter().any(|event| matches!(event, ChatEvent::Tool { name, memory_id: None, agent_id: None, .. } if name == "已记住：修改后的偏好")));
    }

    #[test]
    fn task_without_memory_rejects_unadvertised_remember() {
        let e = env(vec![tool("remember", json!({"text":"secret draft"})), text("ok")], |_| {});
        assert!(rt().block_on(e.task.run(e.task.cfg.lead(), vec![], true)).is_ok());
        let got = e.got.lock().unwrap();
        assert!(!got[0]["tools"].as_array().unwrap().iter().any(|t| t["function"]["name"] == "remember"));
        assert_eq!(got[1]["messages"].as_array().unwrap().last().unwrap()["content"], "当前对话不能写入记忆");
    }

    #[test]
    fn handoff_count_prefers_successful_file_writes_then_result_paragraphs() {
        assert_eq!(handoff_summary("a\n\nb\n\nc", 0), (3, "段结果".into()));
        assert_eq!(handoff_summary("仍然按换行保留分段", 2), (2, "份文件".into()));
    }

    #[test]
    fn delegate_handoff_event_connects_successive_agents() {
        let e = env(vec![
            tool("delegate", json!({"agent":"研究员","task":"先收集三个来源"})),
            text("来源一\n\n来源二"),
            tool("delegate", json!({"agent":"整理员","task":"依据来源做对比"})),
            text("比较完成"),
            text("汇总"),
        ], |c| {
            c.upsert_agent(Agent { name:"研究员".into(), role:"找来源".into(), ..Agent::default() }).unwrap();
            c.upsert_agent(Agent { name:"整理员".into(), role:"整理资料".into(), ..Agent::default() }).unwrap();
        });
        let out = rt().block_on(e.task.run(&e.task.cfg.lead().clone(), vec![], true));
        assert_eq!(out.unwrap(), "汇总");
        let events = e.events.lock().unwrap();
        let handoffs: Vec<_> = events.iter().filter_map(|event| match event {
            ChatEvent::Handoff { from, to, count, unit, to_call, .. } => Some((from.as_str(), to.as_str(), *count, unit.as_str(), to_call.as_str())),
            _ => None,
        }).collect();
        let target = events.iter().find_map(|event| match event {
            ChatEvent::Team { agent, call, status, .. } if agent == "整理员" && status == "run" => Some(call.as_str()),
            _ => None,
        }).unwrap();
        assert_eq!(handoffs, vec![("研究员", "整理员", 2, "段结果", target)]);
    }

    #[test]
    fn delegate_handoff_does_not_skip_failed_calls() {
        let batch = |agents: &[&str]| {
            let calls: Vec<_> = agents.iter().enumerate().map(|(index, agent)| json!({
                "index": index, "id": format!("call{index}"), "type": "function",
                "function": {"name": "delegate", "arguments": json!({"agent":agent,"task":"test"}).to_string()}
            })).collect();
            format!("data: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":{"tool_calls":calls}}]}))
        };
        let e = env(vec![
            batch(&["A", "missing", "C"]), text("result"), text("result"),
            batch(&["C", "missing"]), text("result"),
            batch(&["A"]), text("result"), text("summary"),
        ], |c| {
            for name in ["A", "C"] {
                c.upsert_agent(Agent { name: name.into(), ..Agent::default() }).unwrap();
            }
        });
        let out = rt().block_on(e.task.run(e.task.cfg.lead(), vec![], true));
        assert_eq!(out.unwrap(), "summary");
        let events = e.events.lock().unwrap();
        assert_eq!(events.iter().filter(|event| matches!(event, ChatEvent::Team { status, .. } if status == "done")).count(), 4);
        assert!(!events.iter().any(|event| matches!(event, ChatEvent::Handoff { .. })));
    }

    #[test]
    fn delegate_handoff_cost_uses_the_selected_provider_rate() {
        let e = env(vec![], |c| {
            c.usd_cny = 8.0;
            c.models[0].base_url = "https://api.openai.com/v1".into();
            c.models[0].provider = "OpenAI".into();
            c.agents[0].model = c.models[0].id;
        });
        e.task.catalog.lock().unwrap().replace(crate::catalog::Catalog::trim(
            &json!({"openai":{"name":"OpenAI","api":"https://api.openai.com/v1","models":{"m":{"modalities":{"input":["text"],"output":["text"]},"tool_call":true,"cost":{"input":0.15,"output":0.6}}}}}),
            &Value::Null, "2026-11-21"));
        assert!((e.task.price_for(&e.task.cfg.models[0].base_url, "m", "OpenAI", 1_000, 2_000).unwrap() - 0.0108).abs() < 1e-12);
        assert_eq!(e.task.price_for("https://unknown.example/v1", "m", "", 1_000, 2_000), None);
    }

    #[test]
    fn delegates_to_sub_agent() {
        let e = env(
            vec![tool("delegate", json!({"agent":"审查员","task":"检查错别字"})), text("没有错别字"), text("审查员说没有错别字")],
            |c| {
                c.save_skill(crate::config::Skill { id: String::new(), name: "核对出处".into(), when: "查资料时".into(), steps: "先找原文".into() }).unwrap();
                c.upsert_agent(Agent { name: "审查员".into(), role: "检查错别字".into(), prompt: "你是审查员".into(), skills: vec![c.skills[0].id.clone()], ..Agent::default() }).unwrap();
            },
        );
        let out = rt().block_on(e.task.run(&e.task.cfg.lead().clone(), vec![json!({"role":"user","content":"检查一下"})], true));
        assert_eq!(out.unwrap(), "审查员说没有错别字");
        let got = e.got.lock().unwrap();
        assert!(got[0]["tools"].as_array().unwrap().iter().any(|t| t["function"]["name"] == "delegate"));
        // 子 Agent 用自己的指令，不能再往下交
        assert_eq!(got[1]["messages"][0], json!({"role":"system","content":"你是审查员\n\n技能：核对出处\n什么时候用：查资料时\n步骤：\n先找原文"}));
        assert!(!got[0]["messages"][0]["content"].as_str().unwrap().contains("核对出处"));
        assert!(!got[1]["tools"].as_array().unwrap().iter().any(|t| t["function"]["name"] == "delegate"));
        // 交给子 Agent 不再是工具药丸，而是计划卡的一行：开始 → 完成（带这一步用掉的 token）
        assert!(pills(&e).is_empty());
        let team: Vec<(String, String, String)> = e.events.lock().unwrap().iter().filter_map(|x| match x { ChatEvent::Team { agent, task, status, .. } => Some((agent.clone(), task.clone(), status.clone())), _ => None }).collect();
        assert_eq!(team, vec![("审查员".into(), "检查错别字".into(), "wait".into()), ("审查员".into(), "检查错别字".into(), "run".into()), ("审查员".into(), "检查错别字".into(), "done".into())]);
        // 子 Agent 的正文不流到主控的气泡里
        let deltas: String = e.events.lock().unwrap().iter().filter_map(|x| match x { ChatEvent::Delta { text } => Some(text.clone()), _ => None }).collect();
        assert_eq!(deltas, "审查员说没有错别字");
    }

    async fn answer_budget(e: &Env, allow: bool) -> (String, u64, u64, u64) {
        for _ in 0..200 {
            let b = e.events.lock().unwrap().iter().find_map(|x| match x { ChatEvent::Budget { ask, kind, used, limit, more } => Some((*ask, kind.clone(), *used, *limit, *more)), _ => None });
            if let Some((id, k, u, l, m)) = b {
                assert!(e.task.asks.answer(id, Answer { allow, ..Answer::default() }));
                return (k, u, l, m);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("没有出现预算卡")
    }

    #[test]
    fn budget_pauses_and_asks() {
        // 步数用满：暂停问用户，选「停止」就结束
        let e = env((0..5).map(|_| tool("web_search", json!({"query":"x"}))).collect(), |c| c.limits = Limits { tokens: 0, steps: 3, parallel: 1 });
        let (out, b) = rt().block_on(async {
            let t = e.task.clone();
            let h = tokio::spawn(async move { t.run(&t.cfg.lead().clone(), vec![json!({"role":"user","content":"搜"})], true).await });
            let b = answer_budget(&e, false).await;
            (h.await.unwrap(), b)
        });
        assert!(out.unwrap_err().contains("最多步数"));
        assert_eq!(b, ("steps".into(), 3, 3, MORE_STEPS));
        // token 用满：选「再给 2万」就接着做完
        let e = env(vec![tool("web_search", json!({"query":"x"})), tool("web_search", json!({"query":"y"})), text("好了")], |c| c.limits = Limits { tokens: 50, steps: 50, parallel: 1 });
        let (out, b) = rt().block_on(async {
            let t = e.task.clone();
            let h = tokio::spawn(async move { t.run(&t.cfg.lead().clone(), vec![json!({"role":"user","content":"搜"})], true).await });
            let b = answer_budget(&e, true).await;
            (h.await.unwrap(), b)
        });
        assert_eq!(out.unwrap(), "好了");
        assert_eq!((b.0.as_str(), b.2, b.3), ("tokens", 50, MORE_TOKENS));
        assert!(b.1 >= 50);
        assert_eq!(e.got.lock().unwrap().len(), 3);
        // 只问了一次
        assert_eq!(e.events.lock().unwrap().iter().filter(|x| matches!(x, ChatEvent::Budget { .. })).count(), 1);
    }

    #[test]
    fn model_without_tools_gets_plain_chat() {
        let e = env(vec![text("你好")], |c| c.models[0].tools = Some(false));
        let out = rt().block_on(e.task.run(&e.task.cfg.lead().clone(), vec![json!({"role":"user","content":"hi"})], true));
        assert_eq!(out.unwrap(), "你好");
        assert!(e.got.lock().unwrap()[0].get("tools").is_none());
    }

    #[test]
    fn html_and_search_parsing() {
        assert_eq!(html_text("<html><head><title>T</title><style>a{}</style></head><body><p>你好&amp;世界</p><script>x()</script></body></html>"), "T 你好&世界");
        let h = r#"<a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2F%E4%BD%A0&amp;rut=1">Example <b>站</b></a><a class="result__snippet" href="x">摘要 <b>一</b></a>"#;
        assert_eq!(parse_ddg(h), vec![("Example 站".into(), "https://example.com/你".into(), "摘要 一".into())]);
        assert_eq!(folder("/a/b/c.txt"), "b");
        assert_eq!(host("https://x.com/a?b"), "x.com");
    }
}
