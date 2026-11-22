use super::*;

/// 用户对确认卡片的回答
#[derive(Debug, Clone, Default, serde::Deserialize, PartialEq)]
#[serde(default)]
pub struct Answer {
    pub allow: bool,
    pub always: bool,
    /// 拒绝时的原因，会告诉 Agent
    pub reason: String,
    /// 「修改后允许」：改过的内容（邮件正文、文件内容等），空着就照原样
    pub edit: String,
}

/// 等待中的确认，按编号找回调
#[derive(Default)]
pub struct Asks {
    pub(super) next: AtomicU64,
    pub(super) wait: Mutex<HashMap<u64, oneshot::Sender<Answer>>>,
}

impl Asks {
    pub fn answer(&self, id: u64, a: Answer) -> bool {
        self.wait
            .lock()
            .unwrap()
            .remove(&id)
            .is_some_and(|tx| tx.send(a).is_ok())
    }
}

/// 预算用满时每次「再给」加多少（照原型「再给 2万」）
pub const MORE_TOKENS: u64 = 20_000;

pub const MORE_STEPS: u64 = 20;

impl Task {
    /// 用满了哪一项：(种类, 已用, 上限)
    pub(super) fn over(&self) -> Option<(&'static str, u64, u64)> {
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
    pub(super) async fn budget(&self) -> Result<(), String> {
        if self.over().is_none() {
            return Ok(());
        }
        let _one = self.budget_ask.lock().await;
        // 排队时别人可能已经问过、用户已经加了
        let Some((kind, used, limit)) = self.over() else {
            return Ok(());
        };
        let stopped = self.budget_stop.load(Ordering::Relaxed);
        let more = if kind == "tokens" {
            MORE_TOKENS
        } else {
            MORE_STEPS
        };
        let r = if stopped {
            None
        } else {
            self.budget_wait(kind, used, limit, more).await
        };
        if r.is_some_and(|a| a.allow) {
            let add = if kind == "tokens" {
                &self.more_tokens
            } else {
                &self.more_steps
            };
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

    pub(super) async fn budget_wait(
        &self,
        kind: &str,
        used: u64,
        limit: u64,
        more: u64,
    ) -> Option<Answer> {
        let id = self.asks.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.asks.wait.lock().unwrap().insert(id, tx);
        (self.emit)(ChatEvent::Budget {
            ask: id,
            kind: kind.into(),
            used,
            limit,
            more,
        });
        let r = if self.cfg.pause_away {
            rx.await.ok()
        } else {
            tokio::time::timeout(Duration::from_secs(300), rx)
                .await
                .ok()
                .and_then(|r| r.ok())
        };
        self.asks.wait.lock().unwrap().remove(&id);
        r
    }

    pub(super) fn pre_authorized(&self, kind: &str) -> bool {
        let routine_kind = match kind {
            "打开网页" => "打开",
            "写入" | "删除" | "发送" => kind,
            _ => return false,
        };
        self.pre_allow.iter().any(|k| k == routine_kind)
    }

    pub(super) async fn ask(
        &self,
        call: &str,
        kind: &str,
        desc: &str,
        scope: &str,
        hi: bool,
        no_always: bool,
    ) -> (Answer, bool) {
        if self.pre_authorized(kind) {
            return (
                Answer {
                    allow: true,
                    ..Answer::default()
                },
                true,
            );
        }
        let id = self.asks.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (tx, rx) = oneshot::channel();
        self.asks.wait.lock().unwrap().insert(id, tx);
        (self.emit)(ChatEvent::Ask {
            ask: id,
            call: call.into(),
            kind: kind.into(),
            desc: desc.into(),
            scope: scope.into(),
            hi,
            no_always,
        });
        // 「离开电脑时暂停敏感操作」开着：一直等到回来确认；关着：5 分钟没确认就跳过
        let r = if self.cfg.pause_away {
            rx.await.ok()
        } else {
            tokio::time::timeout(Duration::from_secs(300), rx)
                .await
                .ok()
                .and_then(|r| r.ok())
        };
        self.asks.wait.lock().unwrap().remove(&id);
        (
            r.unwrap_or(Answer {
                reason: "5 分钟没有确认，已跳过".into(),
                ..Answer::default()
            }),
            false,
        )
    }
}
