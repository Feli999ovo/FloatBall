use crate::*;

#[cfg_attr(feature = "gen-ipc", derive(specta::Type))]
#[derive(Serialize)]
pub(crate) struct SelStatus {
    supported: bool,
    trusted: bool,
    recent: Vec<String>,
}

#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub fn sel_status(state: State<'_, AppState>) -> SelStatus {
    SelStatus {
        supported: selection::SUPPORTED,
        trusted: selection::trusted(false),
        recent: state.sel_recent.lock().unwrap().clone(),
    }
}

/// 工具栏量好自己的大小后调用：放到松开点旁边并显示（不抢焦点，原应用的选区保持不变）
/// 返回毛玻璃底图（和对话卡片同一种：截下工具栏将要盖住的桌面、模糊）。只有 Windows 有，拿不到就是 None，前端用不透明的底。
#[cfg_attr(feature = "gen-ipc", derive(specta::Type))]
#[derive(serde::Serialize)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct SelBackdrop {
    url: String,
    w: f64,
    h: f64,
}

#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub async fn sel_place(app: AppHandle, w: f64, h: f64, x: f64, y: f64) -> Option<SelBackdrop> {
    log::info!("sel_place 开始 {w}x{h} @ {x},{y}");
    let win = app.get_webview_window("selbar")?;
    #[cfg(windows)]
    let was = selection::bar_shown();
    #[cfg(windows)]
    if was {
        selection::bar_hide(&win); // 截图时别把上一次的工具栏截进去
    }
    let placed = selection::place(&win, x, y, w, h);
    #[cfg(windows)]
    let bd = match placed {
        Some(p) => {
            let (bx, by, bw, bh) = selection::backdrop_rect(&p, 220.0);
            let (px, py) = (p.left + p.w as i32 / 2, p.top + p.h as i32 / 2);
            let shot = tauri::async_runtime::spawn_blocking(move || {
                capture::backdrop_at(px, py, bx, by, bw, bh)
            });
            match tokio::time::timeout(std::time::Duration::from_millis(250), shot).await {
                Ok(Ok(Ok(url))) => Some(SelBackdrop {
                    url,
                    w: bw as f64 / p.sf,
                    h: bh as f64 / p.sf,
                }),
                _ => None,
            }
        }
        None => None,
    };
    #[cfg(not(windows))]
    let bd = {
        let _ = placed;
        None
    };
    log::info!("sel_place 显示工具栏，底图 {}", bd.is_some());
    selection::bar_show(&win);
    log::info!("sel_place 完成");
    bd
}

/// 展开或收起「⋯」时只改大小，左上角不动（底部超出屏幕时往上挪）
#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub fn sel_resize(app: AppHandle, w: f64, h: f64) {
    let Some(win) = app.get_webview_window("selbar") else {
        return;
    };
    let sf = win.scale_factor().unwrap_or(1.0);
    let (wp, hp) = ((w * sf).round() as u32, (h * sf).round() as u32);
    if let (Ok(pos), Ok(Some(m))) = (win.outer_position(), win.current_monitor()) {
        let bottom = m.position().y + m.size().height as i32 - 8;
        let y = if pos.y + hp as i32 > bottom {
            (bottom - hp as i32).max(m.position().y)
        } else {
            pos.y
        };
        let _ = win.set_position(tauri::PhysicalPosition::new(pos.x, y));
        selection::bar_rect((pos.x, y, wp, hp));
    }
    let _ = win.set_size(tauri::PhysicalSize::new(wp, hp));
}

#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub fn sel_hide(app: AppHandle) {
    if let Some(win) = app.get_webview_window("selbar") {
        selection::bar_hide(&win);
    }
}

/// 工具栏上点了某个动作：交给小球（展开对话并发出去）
#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub fn sel_act(app: AppHandle, act: String, text: String) {
    if let Some(win) = app.get_webview_window("selbar") {
        selection::bar_hide(&win);
    }
    show_ball(&app);
    let _ = app.emit_to(
        "main",
        "sel:act",
        serde_json::json!({ "act": act, "text": text }),
    );
}

#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub fn sel_copy(text: String) -> Result<(), String> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        arboard::Clipboard::new()
            .and_then(|mut c| c.set_text(text))
            .map_err(|e| e.to_string())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = text;
        Err("这个系统上还不支持".into())
    }
}

/// 用默认浏览器打开网页（只许 http/https）
#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("只能打开网页".into());
    }
    open_external(&url)
}

/// macOS：打开 系统设置 → 隐私与安全性 → 辅助功能
#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub fn sel_open_privacy() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        let _ = selection::trusted(true);
        open_external(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        )
    } else {
        Ok(())
    }
}

pub(crate) fn open_external(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    let r = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", target])
            .creation_flags(0x0800_0000)
            .spawn()
    };
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(target).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(target).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}
