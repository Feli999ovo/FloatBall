use super::*;

#[derive(Default)]
pub(super) struct DelegateStats {
    pub(super) tokens: std::sync::atomic::AtomicU64,
    pub(super) prompt: std::sync::atomic::AtomicU64,
    pub(super) completion: std::sync::atomic::AtomicU64,
    pub(super) cost: Mutex<Option<f64>>,
    pub(super) has_usage: std::sync::atomic::AtomicBool,
    pub(super) writes: std::sync::atomic::AtomicUsize,
}

#[derive(Clone)]
pub(super) struct DelegateSummary {
    pub(super) call: String,
    pub(super) agent_id: String,
    pub(super) agent: String,
    pub(super) count: u64,
    pub(super) unit: String,
    pub(super) cost: Option<f64>,
}

pub(super) fn handoff_summary(result: &str, writes: u64) -> (u64, String) {
    if writes > 0 {
        return (writes, "份文件".into());
    }
    let count = result
        .split("\n\n")
        .filter(|part| !part.trim().is_empty())
        .count() as u64;
    (count, "段结果".into())
}

impl Task {
    pub(super) fn emit_handoff(&self, from: &DelegateSummary, to_id: &str, to_name: &str, to_call: &str) {
        if from.agent_id != to_id {
            (self.emit)(ChatEvent::Handoff {
                from_call: from.call.clone(),
                to_call: to_call.into(),
                from: from.agent.clone(),
                to: to_name.into(),
                count: from.count,
                unit: from.unit.clone(),
                cost: from.cost,
            });
        }
    }

    pub(super) async fn delegate(
        self: &Arc<Self>,
        c: &llm::Call,
        previous: Option<&DelegateSummary>,
    ) -> (String, Option<DelegateSummary>) {
        let a: Value = serde_json::from_str(&c.args).unwrap_or_default();
        let who = arg(&a, "agent");
        let task = arg(&a, "task");
        let call = format!("t{}", self.seq.fetch_add(1, Ordering::Relaxed));
        let Some(to) = self
            .cfg
            .agents
            .iter()
            .skip(1)
            .find(|x| x.name == who || x.id == who)
            .cloned()
        else {
            return (format!("没有叫「{who}」的 Agent"), None);
        };
        let model = self
            .model(&to)
            .map(|m| {
                if m.name.is_empty() {
                    m.model.clone()
                } else {
                    m.name.clone()
                }
            })
            .unwrap_or_default();
        let model_id = self.model(&to).map(|m| m.id).unwrap_or(0);
        let team = |status: &str, tokens: u64, prompt: u64, completion: u64, cost: Option<f64>| {
            (self.emit)(ChatEvent::Team {
                call: call.clone(),
                agent_id: to.id.clone(),
                agent: to.name.clone(),
                task: short(&task, 80),
                model: model.clone(),
                model_id,
                status: status.into(),
                tokens,
                prompt,
                completion,
                cost,
            })
        };
        // 先排队（受「最多同时子 Agent」限制），轮到了才算进行中
        team("wait", 0, 0, 0, None);
        let _permit = self.par.acquire().await;
        if let Some(from) = previous {
            self.emit_handoff(from, &to.id, &to.name, &call);
        }
        team("run", 0, 0, 0, None);
        let stats = Arc::new(DelegateStats {
            cost: Mutex::new(Some(0.0)),
            has_usage: Default::default(),
            ..DelegateStats::default()
        });
        let r = self
            .run_acc(
                &to,
                vec![json!({"role": "user", "content": task})],
                false,
                Some(stats.clone()),
            )
            .await;
        let (tokens, prompt, completion) = (
            stats.tokens.load(Ordering::Relaxed),
            stats.prompt.load(Ordering::Relaxed),
            stats.completion.load(Ordering::Relaxed),
        );

        match r {
            Ok(out) => {
                let cost = stats
                    .has_usage
                    .load(Ordering::Relaxed)
                    .then(|| *stats.cost.lock().unwrap())
                    .flatten();
                team("done", tokens, prompt, completion, cost);
                let (count, unit) =
                    handoff_summary(&out, stats.writes.load(Ordering::Relaxed) as u64);
                (
                    out,
                    Some(DelegateSummary {
                        call,
                        agent_id: to.id,
                        agent: to.name,
                        count,
                        unit,
                        cost,
                    }),
                )
            }
            Err(e) => {
                team("err", tokens, prompt, completion, None);
                (format!("出错了：{e}"), None)
            }
        }
    }
}
