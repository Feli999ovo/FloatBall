use crate::*;

pub(crate) fn with_store<T>(
    state: &State<'_, AppState>,
    f: impl FnOnce(&store::Store) -> rusqlite::Result<T>,
) -> Result<T, String> {
    let g = state.store.lock().unwrap();
    let s = g.as_ref().ok_or("历史记录数据库打不开")?;
    f(s).map_err(|e| e.to_string())
}

pub(crate) struct AppMemory(pub(crate) AppHandle);

impl agent::MemoryStore for AppMemory {
    fn recent(&self, agent: &str) -> Vec<String> {
        with_store(&self.0.state::<AppState>(), |s| s.memory(agent, 20))
            .unwrap_or_default()
            .into_iter()
            .map(|m| m.text)
            .collect()
    }

    fn add(&self, agent: &str, text: &str) -> Result<i64, String> {
        let state = self.0.state::<AppState>();
        let cfg = state.cfg.lock().unwrap();
        if !cfg.agents.iter().any(|a| a.id == agent) {
            return Err("这个 Agent 已被删除".into());
        }
        let id = with_store(&state, |s| s.remember(agent, text))?;
        drop(cfg);
        let _ = self.0.emit("memory:changed", agent.to_owned());
        Ok(id)
    }
}

#[tauri::command]
pub fn agent_memory_counts(
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, u32>, String> {
    with_store(&state, store::Store::memory_counts)
}

#[tauri::command]
pub fn agent_memory_list(
    state: State<'_, AppState>,
    agent: String,
) -> Result<Vec<store::Memory>, String> {
    with_store(&state, |s| s.memory(&agent, 100))
}

#[tauri::command]
pub fn agent_memory_delete(
    app: AppHandle,
    state: State<'_, AppState>,
    agent: String,
    id: Option<i64>,
) -> Result<(), String> {
    with_store(&state, |s| s.delete_memory(&agent, id))?;
    let _ = app.emit("memory:changed", agent);
    Ok(())
}

/// 撤销对话里的自动记忆，只删除指定 Agent 下这一条
#[tauri::command]
pub fn forget_memory(
    app: AppHandle,
    state: State<'_, AppState>,
    agent: String,
    id: i64,
) -> Result<(), String> {
    with_store(&state, |s| s.delete_memory(&agent, Some(id)))?;
    let _ = app.emit("memory:changed", agent);
    Ok(())
}

#[tauri::command]
pub fn history_append(
    state: State<'_, AppState>,
    conv_id: Option<i64>,
    role: String,
    content: String,
) -> Result<i64, String> {
    with_store(&state, |s| s.append(conv_id, &role, &content))
}

#[tauri::command]
pub fn history_list(state: State<'_, AppState>) -> Result<Vec<store::ConvSummary>, String> {
    with_store(&state, |s| s.list(200))
}

#[tauri::command]
pub fn history_load(state: State<'_, AppState>, conv_id: i64) -> Result<Vec<store::Msg>, String> {
    with_store(&state, |s| s.load(conv_id))
}

#[tauri::command]
pub fn history_delete(state: State<'_, AppState>, conv_id: i64) -> Result<(), String> {
    with_store(&state, |s| s.delete(conv_id))
}

#[tauri::command]
pub fn history_clear(state: State<'_, AppState>) -> Result<(), String> {
    with_store(&state, |s| s.clear())
}
