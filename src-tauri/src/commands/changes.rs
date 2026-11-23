use crate::error::AppError;
use crate::*;

#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub async fn undo_change(
    app: AppHandle,
    id: String,
    confirm_changed: bool,
) -> Result<bool, AppError> {
    let state = app.state::<AppState>();
    let changes = state.changes.clone();
    let trash = state.data_dir.join("回收站");
    tauri::async_runtime::spawn_blocking(move || changes.undo(&id, confirm_changed, &trash))
        .await
        .map_err(|e| e.to_string())?
}
