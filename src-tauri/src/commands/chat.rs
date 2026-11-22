use crate::*;

/// 加系统提示词；模型不支持图片时把图片换成一句说明
pub(crate) fn prepare_messages(
    system_prompt: &str,
    vision: bool,
    messages: Vec<Value>,
) -> Vec<Value> {
    let mut out = Vec::with_capacity(messages.len() + 1);
    if !system_prompt.trim().is_empty() {
        out.push(json!({"role":"system","content":system_prompt}));
    }
    for mut m in messages {
        if !vision {
            if let Some(parts) = m.get("content").and_then(Value::as_array).cloned() {
                let text: Vec<String> = parts
                    .iter()
                    .filter_map(|p| match p.get("type").and_then(Value::as_str) {
                        Some("text") => p.get("text").and_then(Value::as_str).map(str::to_string),
                        Some("image_url") => {
                            Some("[附了一张截图，但当前模型设置为不支持图片]".into())
                        }
                        _ => None,
                    })
                    .collect();
                m["content"] = Value::String(text.join("\n"));
            }
        }
        out.push(m);
    }
    out
}

pub(crate) fn review_agent_for(cfg: &Config, id: &str) -> Result<config::Agent, String> {
    let mut agent = cfg
        .agents
        .iter()
        .find(|a| a.id == id)
        .cloned()
        .ok_or("找不到这个 Agent")?;
    if !agent.tools.iter().any(|tool| tool == "读文件") {
        agent.tools.push("读文件".into());
    }
    Ok(agent)
}

/// 提问交给主控（agent.rs）：主控用自己的指令和模型，可以用工具、把步骤交给别的 Agent。
/// 主模型出错（还没回一个字）时换备用模型再试一次。
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri 命令的参数由前端传入，不能合并
pub fn chat(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    messages: Vec<Value>,
    temporary: Option<bool>,
    plan: Option<bool>,
    pre_allow: Option<Vec<String>>,
    review_agent: Option<String>,
    on_event: Channel<ChatEvent>,
) -> Result<(), String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let review = review_agent.is_some();
    let lead = match review_agent {
        Some(id) => review_agent_for(&cfg, &id)?,
        None => cfg.lead().clone(),
    };
    let plan = review || plan.unwrap_or_default();
    let catalog = state.catalog.lock().unwrap().clone();
    let got = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (g2, ch) = (got.clone(), on_event.clone());
    // 每轮的 Done / Usage 不转给界面，整个任务结束时发一次
    let emit: Arc<dyn Fn(ChatEvent) + Send + Sync> = Arc::new(move |ev: ChatEvent| {
        if matches!(
            ev,
            ChatEvent::Done | ChatEvent::Usage { .. } | ChatEvent::ToolCall { .. }
        ) {
            return;
        }
        if !matches!(ev, ChatEvent::Error { .. }) {
            g2.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let _ = ch.send(ev);
    });
    let app2 = app.clone();
    let memory = if temporary == Some(false) {
        Some(Box::new(AppMemory(app.clone())) as Box<dyn agent::MemoryStore>)
    } else {
        None
    };
    let world: Arc<dyn agent::World> = Arc::new(agent::RealWorld {
        memory,
        remember: Box::new(move |a: config::Allow| {
            let st = app2.state::<AppState>();
            let mut next = st.cfg.lock().unwrap().clone();
            if !next.always_allow.contains(&a) {
                next.always_allow.push(a);
                let _ = commit(&app2, &st, next);
                let _ = app2.emit_to("settings", "config", ());
            }
        }),
    });
    let trash = state.data_dir.join("回收站");
    let asks = state.asks.clone();
    let changes = state.changes.clone();
    let backup = state
        .data_dir
        .join("改动备份")
        .join(uuid::Uuid::new_v4().to_string());
    let task = tauri::async_runtime::spawn(async move {
        let pre_allow = if review {
            vec![]
        } else {
            pre_allow.unwrap_or_default()
        };
        let t = agent::Task::new(
            cfg.clone(),
            world.clone(),
            asks.clone(),
            trash.clone(),
            emit.clone(),
            plan,
            pre_allow.clone(),
            changes.clone(),
            backup.clone(),
        );
        *t.catalog.lock().unwrap() = Some(catalog.clone());
        let mut r = t.run(&lead, messages.clone(), !review).await;
        let mut prompt = t.prompt.load(std::sync::atomic::Ordering::Relaxed);
        let mut completion = t.completion.load(std::sync::atomic::Ordering::Relaxed);
        let mut estimate = t.estimate();
        let has_fallback = cfg
            .models
            .iter()
            .any(|m| m.id == lead.fallback && Some(m.id) != cfg.model_for(&lead).map(|x| x.id));
        if let (Err(e), true, false) = (
            &r,
            has_fallback,
            got.load(std::sync::atomic::Ordering::Relaxed),
        ) {
            js_log(format!("主模型出错，换备用模型：{e}"));
            let mut l2 = lead.clone();
            l2.model = lead.fallback;
            l2.auto = false;
            let t2 = agent::Task::new(
                cfg, world, asks, trash, emit, plan, pre_allow, changes, backup,
            );
            *t2.catalog.lock().unwrap() = Some(catalog);
            r = t2.run(&l2, messages, !review).await;
            prompt += t2.prompt.load(std::sync::atomic::Ordering::Relaxed);
            completion += t2.completion.load(std::sync::atomic::Ordering::Relaxed);
            estimate = match (estimate, t2.estimate()) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            };
        }
        match r {
            Ok(_) => {
                let _ = on_event.send(ChatEvent::Usage { prompt, completion });
                let _ = on_event.send(ChatEvent::Cost { amount: estimate });
                let _ = on_event.send(ChatEvent::Done);
            }
            Err(message) => {
                let _ = on_event.send(ChatEvent::Error { message });
            }
        }
    });
    let mut chats = state.chats.lock().unwrap();
    chats.retain(|_, h| !h.inner().is_finished());
    chats.insert(id, task);
    Ok(())
}

/// 确认卡片上的选择
#[tauri::command]
pub fn answer_ask(state: State<'_, AppState>, ask: u64, answer: agent::Answer) -> bool {
    state.asks.answer(ask, answer)
}

/// 权限页：撤销一条「总是允许」、开关「离开电脑时暂停敏感操作」
#[tauri::command]
pub fn save_perm(
    app: AppHandle,
    state: State<'_, AppState>,
    always_allow: Vec<config::Allow>,
    pause_away: bool,
) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.always_allow = always_allow;
    next.pause_away = pause_away;
    commit(&app, &state, next)
}

#[tauri::command]
pub fn cancel_chat(state: State<'_, AppState>, id: String) {
    if let Some(h) = state.chats.lock().unwrap().remove(&id) {
        h.abort();
    }
}
