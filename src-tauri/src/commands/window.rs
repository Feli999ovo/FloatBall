use crate::*;

#[tauri::command]
pub fn save_position(state: State<'_, AppState>, x: f64, y: f64) -> Result<(), String> {
    let mut c = state.cfg.lock().unwrap();
    c.position = Some((x, y));
    c.save(&state.cfg_dir)
}

/// 设置是一个单独的普通窗口，已经开着就把它带到前面。
/// 另起一个线程来建：在 Windows 上，从菜单事件或同步命令里直接建窗口会卡死。
pub(crate) fn open_settings_window<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Some(w) = app.get_webview_window("settings") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
            return;
        }
        // Windows：和对话卡片同一种玻璃。窗口还没出现，先截下鼠标所在的整块屏幕、缩小模糊，
        // 页面把它按窗口位置垫在墨层下面，拖动时跟着挪（见 settings.js 的 placeBackdrop）。
        // 「透明效果」被用户关掉时不截，用不透明的墨色底。
        #[cfg(windows)]
        if system_glass() {
            let at = app
                .cursor_position()
                .map(|p| (p.x as i32, p.y as i32))
                .unwrap_or((0, 0));
            let bd = capture::screen_backdrop(at.0, at.1)
                .map_err(|e| log::warn!("设置窗口底图：{e}"))
                .ok();
            if let Some(st) = app.try_state::<AppState>() {
                *st.settings_bd.lock().unwrap() = bd;
            }
        }
        // 去掉系统标题栏（界面里自己画），窗口透明。macOS 后面铺系统材质；Linux 保留系统标题栏和不透明的底。
        let b = tauri::WebviewWindowBuilder::new(
            &app,
            "settings",
            tauri::WebviewUrl::App("settings.html".into()),
        )
        .title("悬浮球设置")
        .inner_size(480.0, 600.0)
        .min_inner_size(440.0, 480.0)
        .center();
        #[cfg(any(windows, target_os = "macos"))]
        let b = b.decorations(false).transparent(true).shadow(true);
        #[cfg(target_os = "macos")]
        let b = if system_glass() {
            use tauri::{utils::config::WindowEffectsConfig, window::Effect};
            b.effects(WindowEffectsConfig {
                effects: vec![Effect::HudWindow],
                radius: Some(12.0),
                ..Default::default()
            })
        } else {
            b
        };
        let built = b.build();
        if let Err(e) = built {
            log::error!("settings window: {e}");
        }
    });
}

/// 设置窗口取走它的玻璃底图（只取一次，图只在内存里）
#[tauri::command]
pub fn settings_backdrop(state: State<'_, AppState>) -> Option<capture::ScreenBackdrop> {
    state.settings_bd.lock().unwrap().take()
}

/// agent：直接打开这个 Agent 的编辑页（Agent 弹卡的「更换」模型）
#[tauri::command]
pub fn open_settings(app: AppHandle, agent: Option<String>) {
    if let Some(id) = agent.filter(|x| !x.is_empty()) {
        if let Some(st) = app.try_state::<AppState>() {
            *st.settings_goto.lock().unwrap() = Some(id.clone());
        }
        // 设置窗口已经开着：直接让它跳过去
        if app.get_webview_window("settings").is_some() {
            let _ = app.emit_to("settings", "settings:goto", id);
        }
    }
    open_settings_window(&app);
}

/// 设置窗口启动时取走「要去编辑的 Agent」
#[tauri::command]
pub fn settings_goto(state: State<'_, AppState>) -> Option<String> {
    state.settings_goto.lock().unwrap().take()
}

pub(crate) fn build_menu<R: Runtime>(app: &AppHandle<R>, for_tray: bool) -> tauri::Result<Menu<R>> {
    let auto = app.autolaunch().is_enabled().unwrap_or(false);
    let hidden = main_window(app)
        .map(|w| !w.is_visible().unwrap_or(true))
        .unwrap_or(false);
    let m = Menu::new(app)?;
    if for_tray {
        m.append(&MenuItem::with_id(
            app,
            "toggle_ball",
            if hidden {
                "显示小球"
            } else {
                "隐藏小球"
            },
            true,
            None::<&str>,
        )?)?;
        m.append(&PredefinedMenuItem::separator(app)?)?;
    }
    m.append(&MenuItem::with_id(
        app,
        "new_chat",
        "新对话",
        true,
        None::<&str>,
    )?)?;
    m.append(&MenuItem::with_id(
        app,
        "capture",
        "截屏提问",
        true,
        None::<&str>,
    )?)?;
    m.append(&MenuItem::with_id(
        app,
        "history",
        "历史记录",
        true,
        None::<&str>,
    )?)?;
    m.append(&PredefinedMenuItem::separator(app)?)?;
    m.append(&MenuItem::with_id(
        app,
        "settings",
        "设置",
        true,
        None::<&str>,
    )?)?;
    m.append(&CheckMenuItem::with_id(
        app,
        "autostart",
        "开机启动",
        true,
        auto,
        None::<&str>,
    )?)?;
    if !for_tray {
        m.append(&MenuItem::with_id(
            app,
            "toggle_ball",
            "隐藏小球",
            true,
            None::<&str>,
        )?)?;
    }
    m.append(&PredefinedMenuItem::separator(app)?)?;
    m.append(&MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?)?;
    Ok(m)
}

pub(crate) fn refresh_menus<R: Runtime>(app: &AppHandle<R>) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), build_menu(app, true)) {
        let _ = tray.set_menu(Some(menu));
    }
}

pub(crate) fn on_menu<R: Runtime>(app: &AppHandle<R>, id: &str) {
    match id {
        "quit" => app.exit(0),
        "toggle_ball" => {
            if let Some(w) = main_window(app) {
                if w.is_visible().unwrap_or(false) {
                    hide_win(&w);
                } else {
                    show_win(&w);
                }
            }
            refresh_menus(app);
        }
        "autostart" => {
            let al = app.autolaunch();
            let on = al.is_enabled().unwrap_or(false);
            let _ = if on { al.disable() } else { al.enable() };
            refresh_menus(app);
        }
        "settings" => open_settings_window(app),
        "capture_window" => {
            let state = app.state::<AppState>();
            let key = state
                .menu_window
                .lock()
                .unwrap()
                .take()
                .or_else(capture::foreground_window);
            *state.current_window.lock().unwrap() = key;
            show_ball(app);
            let _ = app.emit_to("main", "menu", id.to_string());
        }
        other => {
            show_ball(app);
            let _ = app.emit_to("main", "menu", other.to_string());
        }
    }
}

/// 右键菜单：照原型 .x-menu，自绘在一个透明小窗口（ctxmenu.html）里，和卡片同一种玻璃。
/// 项目先放程序已有的功能；以后的功能 PR 再往 ctx_items 里加。
#[derive(Serialize, Clone)]
pub(crate) struct CtxItem {
    id: &'static str,
    label: &'static str,
    check: Option<bool>,
    sep: bool,
}

pub(crate) fn ctx_items<R: Runtime>(app: &AppHandle<R>, sharing: bool) -> Vec<CtxItem> {
    let auto = app.autolaunch().is_enabled().unwrap_or(false);
    let it = |id, label| CtxItem {
        id,
        label,
        check: None,
        sep: false,
    };
    vec![
        it("new_chat", "新对话"),
        it("new_tmp", "新临时对话"),
        it("history", "历史记录"),
        it("capture", "截屏提问"),
        it("capture_window", "发送当前窗口"),
        it(
            "share",
            if sharing {
                "停止共享"
            } else {
                "共享屏幕"
            },
        ),
        it("handoff", "交给终端"),
        it("routines", "定时任务"),
        it("settings", "设置"),
        it("sleep", "睡眠"),
        it("toggle_ball", "隐藏小球"),
        CtxItem {
            id: "autostart",
            label: "开机启动",
            check: Some(auto),
            sep: false,
        },
        CtxItem {
            id: "quit",
            label: "退出",
            check: None,
            sep: true,
        },
    ]
}

/// 菜单窗口的逻辑大小：原型 .x-menu 宽 200、上下各 6 内边距、每项 38、分隔线 11；四周留 8 给阴影
pub(crate) const CTX_PAD: f64 = 8.0;

pub(crate) fn ctx_size(items: &[CtxItem]) -> (f64, f64) {
    let seps = items.iter().filter(|i| i.sep).count() as f64;
    (
        200.0 + CTX_PAD * 2.0,
        12.0 + 38.0 * items.len() as f64 + 11.0 * seps + CTX_PAD * 2.0,
    )
}

/// 放在鼠标右下方；右边或下边放不下就翻到左边、上边（和系统菜单一样）。返回物理像素的 (左, 上, 宽, 高)
pub(crate) fn ctx_rect(cx: f64, cy: f64, w: f64, h: f64, mon: (f64, f64, f64, f64)) -> (f64, f64) {
    let (mx, my, mw, mh) = mon;
    let x = if cx + w > mx + mw {
        (cx - w).max(mx)
    } else {
        cx
    };
    let y = if cy + h > my + mh {
        (cy - h).max(my)
    } else {
        cy
    };
    (x, y)
}

#[derive(Serialize, Clone)]
pub(crate) struct CtxShow {
    items: Vec<CtxItem>,
    bd: Option<String>,
}

#[tauri::command]
pub async fn show_context_menu(
    app: AppHandle,
    window: WebviewWindow,
    sharing: bool,
) -> Result<(), String> {
    let key = capture::foreground_window();
    *app.state::<AppState>().menu_window.lock().unwrap() = key;
    let Some(menu) = app.get_webview_window("ctxmenu") else {
        // 没有菜单窗口（不该发生）：退回系统菜单
        let m = build_menu(&app, false).map_err(|e| e.to_string())?;
        return window.popup_menu(&m).map_err(|e| e.to_string());
    };
    let items = ctx_items(&app, sharing);
    let c = app.cursor_position().map_err(|e| e.to_string())?;
    let mon = app
        .monitor_from_point(c.x, c.y)
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten());
    // 翻向按工作区算（不压住任务栏 / Dock）；截底图按整块显示器的坐标算
    #[cfg_attr(not(windows), allow(unused_variables))] // mrect 只在 Windows 取底图时用
    let (sf, mrect, work) = match &mon {
        Some(m) => {
            let (wp, wz) = (m.work_area().position, m.work_area().size);
            (
                m.scale_factor(),
                (m.position().x as f64, m.position().y as f64),
                (wp.x as f64, wp.y as f64, wz.width as f64, wz.height as f64),
            )
        }
        None => (
            1.0,
            (0.0, 0.0),
            (c.x - 4000.0, c.y - 4000.0, 8000.0, 8000.0),
        ),
    };
    let (lw, lh) = ctx_size(&items);
    let (w, h) = (lw * sf, lh * sf);
    // 菜单的左上角（不含阴影边）对准鼠标
    let (x, y) = ctx_rect(c.x - CTX_PAD * sf, c.y - CTX_PAD * sf, w, h, work);
    let _ = menu.set_size(tauri::PhysicalSize::new(w.round() as u32, h.round() as u32));
    let _ = menu.set_position(tauri::PhysicalPosition::new(
        x.round() as i32,
        y.round() as i32,
    ));
    #[cfg(windows)]
    let bd = {
        let (bx, by) = ((x - mrect.0).round() as i32, (y - mrect.1).round() as i32);
        let (bw, bh) = (w.round() as u32, h.round() as u32);
        let (px, py) = ((x + w / 2.0) as i32, (y + h / 2.0) as i32);
        let shot = tauri::async_runtime::spawn_blocking(move || {
            capture::backdrop_at(px, py, bx, by, bw, bh)
        });
        match tokio::time::timeout(std::time::Duration::from_millis(250), shot).await {
            Ok(Ok(Ok(url))) => Some(url),
            _ => None,
        }
    };
    #[cfg(not(windows))]
    let bd = None;
    let _ = app.emit_to("ctxmenu", "ctx:show", CtxShow { items, bd });
    let _ = app.emit_to("main", "ctx", true);
    menu.show().map_err(|e| e.to_string())?;
    let _ = menu.set_focus();
    Ok(())
}

/// 菜单里点了一项（或点别处、Esc 关掉时 id 为空）
#[tauri::command]
pub fn ctx_act(app: AppHandle, id: String) {
    if let Some(m) = app.get_webview_window("ctxmenu") {
        let _ = m.hide();
    }
    let _ = app.emit_to("main", "ctx", false);
    if !id.is_empty() {
        on_menu(&app, &id);
    }
    *app.state::<AppState>().menu_window.lock().unwrap() = None;
}

#[tauri::command]
pub fn hide_ball(app: AppHandle) {
    if let Some(w) = main_window(&app) {
        hide_win(&w);
    }
    refresh_menus(&app);
}

#[cfg(test)]
mod ctx_tests {
    use super::*;
    #[test]
    fn ctx_menu_flips_near_edges() {
        let mon = (0.0, 0.0, 1920.0, 1080.0);
        assert_eq!(ctx_rect(100.0, 100.0, 216.0, 300.0, mon), (100.0, 100.0));
        assert_eq!(ctx_rect(1800.0, 100.0, 216.0, 300.0, mon), (1584.0, 100.0));
        assert_eq!(ctx_rect(100.0, 1000.0, 216.0, 300.0, mon), (100.0, 700.0));
        // 第二块屏幕在左边（负坐标）
        assert_eq!(
            ctx_rect(-50.0, 10.0, 216.0, 300.0, (-1920.0, 0.0, 1920.0, 1080.0)),
            (-266.0, 10.0)
        );
    }
    #[test]
    fn ctx_menu_size_matches_prototype() {
        let it = |sep| CtxItem {
            id: "a",
            label: "a",
            check: None,
            sep,
        };
        // 高：13 项×38 + 1 条分隔线×11 + 上下 6×2 + 四周 8×2 = 533；宽：200 + 8×2 = 216
        let items: Vec<_> = (0..13).map(|i| it(i == 12)).collect();
        let (w, h) = ctx_size(&items);
        assert_eq!((w, h), (216.0, 533.0));
        assert_eq!(
            ctx_rect(1000.0, 740.0, w, h, (0.0, 0.0, 1024.0, 768.0)),
            (784.0, 207.0)
        );
    }
}
