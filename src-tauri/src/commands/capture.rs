use crate::*;

/// 卡片的毛玻璃底图（见 capture::backdrop_at）。拿不到就返回空，前端用不透明的底。
#[tauri::command]
pub async fn backdrop(px: i32, py: i32, x: i32, y: i32, w: u32, h: u32) -> Option<String> {
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(move || capture::backdrop_at(px, py, x, y, w, h))
            .await
            .ok()
            .and_then(|r| r.ok())
    }
    #[cfg(not(windows))]
    {
        let _ = (px, py, x, y, w, h);
        None
    }
}

#[tauri::command]
pub async fn capture_screen(app: AppHandle) -> Result<String, String> {
    let (cx, cy) = app
        .cursor_position()
        .map(|p| (p.x as i32, p.y as i32))
        .unwrap_or((0, 0));
    capture_without_ball(&app, move || capture::capture_at(cx, cy)).await
}

pub(crate) async fn capture_without_ball(
    app: &AppHandle,
    take: impl FnOnce() -> Result<String, String> + Send + 'static,
) -> Result<String, String> {
    capture::check_permission()?;
    let state = app.state::<AppState>();
    let _guard = state.screen_capture.lock().await;
    let win = main_window(app).ok_or("no window")?;
    let was_visible = win.is_visible().unwrap_or(false);
    let was_focused = win.is_focused().unwrap_or(false);
    if was_visible {
        hide_win(&win);
        tokio::time::sleep(std::time::Duration::from_millis(220)).await;
    }
    let res = tauri::async_runtime::spawn_blocking(take).await;
    if was_visible {
        show_win(&win);
    }
    if was_focused {
        let _ = win.set_focus();
    }
    res.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn capture_window(app: AppHandle) -> Result<String, String> {
    let key = app
        .state::<AppState>()
        .current_window
        .lock()
        .unwrap()
        .take()
        .ok_or(capture::WINDOW_UNAVAILABLE)?;
    capture::check_permission()?;
    tauri::async_runtime::spawn_blocking(move || capture::capture_window(key))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn share_sources() -> Result<Vec<capture::Source>, String> {
    capture::check_permission()?;
    tauri::async_runtime::spawn_blocking(capture::sources)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn capture_shared(app: AppHandle, target: capture::Target) -> Result<Option<String>, String> {
    let supported = {
        let state = app.state::<AppState>();
        let cfg = state.cfg.lock().unwrap();
        cfg.effective_model_for(cfg.lead())
            .is_some_and(|model| model.vision)
    };
    if !supported {
        return Ok(None);
    }
    let image = match target {
        capture::Target::Screen { id } => {
            capture_without_ball(&app, move || capture::capture_monitor(id)).await?
        }
        capture::Target::Window { key } => {
            capture::check_permission()?;
            tauri::async_runtime::spawn_blocking(move || capture::capture_window(key))
                .await
                .map_err(|e| e.to_string())??
        }
    };
    Ok(Some(image))
}

#[tauri::command]
pub fn open_screen_privacy() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        open_external(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
        )
    } else {
        Ok(())
    }
}

#[derive(Serialize)]
pub(crate) struct TextFile {
    name: String,
    text: String,
}

#[tauri::command]
pub fn read_text_file(path: String) -> Result<TextFile, String> {
    capture::read_text_file(&path).map(|(name, text)| TextFile { name, text })
}

/// 前端的报错打到终端（npm run dev 时能看到），方便排查
#[tauri::command]
pub fn js_log(msg: String) {
    eprintln!("[前端] {msg}");
    log::warn!("[前端] {msg}");
}
