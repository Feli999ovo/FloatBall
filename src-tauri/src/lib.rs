//! 悬浮球主程序：一个透明、无边框、置顶的小窗口。
//! 前端（src/）负责小球、胶囊输入框和对话卡片的样子；这里负责系统能力：
//! 托盘、右键菜单、全局快捷键、开机启动、模型调用、钥匙串、历史记录、截屏。

mod agent;
mod applog;
mod capture;
mod catalog;
mod changes;
mod commands;
mod config;
mod handoff;
mod llm;
mod routine;
mod selection;
mod store;
mod stt;

use catalog::Catalog;
use commands::capture::*;
use commands::chat::*;
use commands::config::*;
use commands::history::*;
use commands::routine::*;
use commands::selection::*;
use commands::voice::*;
use config::{Config, Model};
use llm::ChatEvent;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use tauri::async_runtime::JoinHandle;
use tauri::ipc::Channel;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, LogicalPosition, Manager, Runtime, State, WebviewWindow};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub struct AppState {
    cfg: Mutex<Config>,
    cfg_dir: PathBuf,
    /// models.dev 的模型资料（查能力用）；联网时每天更新一次，存在 data_dir
    catalog: Mutex<Catalog>,
    catalog_busy: Mutex<bool>,
    data_dir: PathBuf,
    store: Mutex<Option<store::Store>>,
    chats: Mutex<HashMap<String, JoinHandle<()>>>,
    asks: Arc<agent::Asks>,
    changes: Arc<changes::Changes>,
    /// 已注册的全局快捷键和它触发的动作（发给小球窗口的 menu 事件名）
    shortcuts: Mutex<Vec<(Shortcut, &'static str)>>,
    menu_window: Mutex<Option<capture::WindowKey>>,
    current_window: Mutex<Option<capture::WindowKey>>,
    screen_capture: tokio::sync::Mutex<()>,
    /// 最近出现过划词的应用（设置页「添加应用」时给候选）
    sel_recent: Mutex<Vec<String>>,
    /// 设置窗口的玻璃底图：打开窗口之前截下整块屏幕、缩小模糊（见 open_settings_window）
    settings_bd: Mutex<Option<capture::ScreenBackdrop>>,
    /// 打开设置时直接去编辑哪个 Agent（对话里 Agent 弹卡的「更换」）
    settings_goto: Mutex<Option<String>>,
}

impl AppState {
    #[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
    pub(crate) fn note_app(&self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let mut r = self.sel_recent.lock().unwrap();
        r.retain(|a| a != name);
        r.insert(0, name.to_string());
        r.truncate(8);
    }
}

fn main_window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    app.get_webview_window("main")
}

/// 隐藏前记下位置，显示时放回去（有的窗口管理器重新显示时会把窗口挪到别处）
static LAST_POS: Mutex<Option<tauri::PhysicalPosition<i32>>> = Mutex::new(None);

fn hide_win<R: Runtime>(w: &WebviewWindow<R>) {
    if let Ok(p) = w.outer_position() {
        *LAST_POS.lock().unwrap() = Some(p);
    }
    let _ = w.hide();
}

fn show_win<R: Runtime>(w: &WebviewWindow<R>) {
    let was_hidden = !w.is_visible().unwrap_or(true);
    let _ = w.show();
    if was_hidden {
        if let Some(p) = *LAST_POS.lock().unwrap() {
            let _ = w.set_position(p);
        }
    }
}

fn show_ball<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = main_window(app) {
        show_win(&w);
    }
}

// ───────── 设置 ─────────

#[tauri::command]
fn save_position(state: State<'_, AppState>, x: f64, y: f64) -> Result<(), String> {
    let mut c = state.cfg.lock().unwrap();
    c.position = Some((x, y));
    c.save(&state.cfg_dir)
}

/// 设置是一个单独的普通窗口，已经开着就把它带到前面。
/// 另起一个线程来建：在 Windows 上，从菜单事件或同步命令里直接建窗口会卡死。
fn open_settings_window<R: Runtime>(app: &AppHandle<R>) {
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
fn settings_backdrop(state: State<'_, AppState>) -> Option<capture::ScreenBackdrop> {
    state.settings_bd.lock().unwrap().take()
}

// ───────── 划词工具栏 ─────────

/// agent：直接打开这个 Agent 的编辑页（Agent 弹卡的「更换」模型）
#[tauri::command]
fn open_settings(app: AppHandle, agent: Option<String>) {
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
fn settings_goto(state: State<'_, AppState>) -> Option<String> {
    state.settings_goto.lock().unwrap().take()
}

// ───────── 对话 ─────────

// ───────── 历史 ─────────

// ───────── 截屏与文件 ─────────

// ───────── 菜单、托盘、快捷键 ─────────

fn build_menu<R: Runtime>(app: &AppHandle<R>, for_tray: bool) -> tauri::Result<Menu<R>> {
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

fn refresh_menus<R: Runtime>(app: &AppHandle<R>) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id("main"), build_menu(app, true)) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn on_menu<R: Runtime>(app: &AppHandle<R>, id: &str) {
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
struct CtxItem {
    id: &'static str,
    label: &'static str,
    check: Option<bool>,
    sep: bool,
}

fn ctx_items<R: Runtime>(app: &AppHandle<R>, sharing: bool) -> Vec<CtxItem> {
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
const CTX_PAD: f64 = 8.0;
fn ctx_size(items: &[CtxItem]) -> (f64, f64) {
    let seps = items.iter().filter(|i| i.sep).count() as f64;
    (
        200.0 + CTX_PAD * 2.0,
        12.0 + 38.0 * items.len() as f64 + 11.0 * seps + CTX_PAD * 2.0,
    )
}

/// 放在鼠标右下方；右边或下边放不下就翻到左边、上边（和系统菜单一样）。返回物理像素的 (左, 上, 宽, 高)
fn ctx_rect(cx: f64, cy: f64, w: f64, h: f64, mon: (f64, f64, f64, f64)) -> (f64, f64) {
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
struct CtxShow {
    items: Vec<CtxItem>,
    bd: Option<String>,
}

#[tauri::command]
async fn show_context_menu(
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
fn ctx_act(app: AppHandle, id: String) {
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
fn hide_ball(app: AppHandle) {
    if let Some(w) = main_window(&app) {
        hide_win(&w);
    }
    refresh_menus(&app);
}

/// 写法不对或两个一样时返回 Err；写法没问题但被别的程序占用时返回 Ok(Some(提示))
fn register_shortcuts<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    cfg: &Config,
) -> Result<Option<String>, String> {
    let parse = |s: &str| -> Result<Option<Shortcut>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        Shortcut::from_str(s)
            .map(Some)
            .map_err(|e| format!("快捷键「{s}」写法不对：{e}"))
    };
    let mut list: Vec<(Shortcut, &'static str)> = vec![];
    for (v, act) in cfg.shortcut_bindings() {
        // 默认的右 Alt 由划词线程读按键状态。
        if act == "talk" && v == config::TALK_RALT {
            continue;
        }
        if let Some(sc) = parse(v)? {
            if list.iter().any(|(o, _)| *o == sc) {
                return Err(format!("快捷键「{v}」重复了，每个只能用一次"));
            }
            list.push((sc, act));
        }
    }
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut errs = vec![];
    for (s, _) in &list {
        if let Err(e) = gs.register(*s) {
            errs.push(format!("{s} 被别的程序占用了（{e}）"));
        }
    }
    *state.shortcuts.lock().unwrap() = list;
    Ok(if errs.is_empty() {
        None
    } else {
        Some(errs.join("；"))
    })
}

/// 首次启动或上次的位置已经不在任何屏幕上（拔了外接屏）时，放到主屏右侧中间
fn place_initial<R: Runtime>(w: &WebviewWindow<R>, saved: Option<(f64, f64)>) {
    let monitors = w.available_monitors().unwrap_or_default();
    if let Some((x, y)) = saved {
        let on_screen = monitors.iter().any(|m| {
            let s = m.scale_factor();
            let p = m.position().to_logical::<f64>(s);
            let z = m.size().to_logical::<f64>(s);
            x >= p.x - 40.0 && x < p.x + z.width - 40.0 && y >= p.y && y < p.y + z.height - 40.0
        });
        if on_screen {
            let _ = w.set_position(LogicalPosition::new(x, y));
            return;
        }
    }
    if let Ok(Some(m)) = w.primary_monitor() {
        let s = m.scale_factor();
        let p = m.position().to_logical::<f64>(s);
        let z = m.size().to_logical::<f64>(s);
        let _ = w.set_position(LogicalPosition::new(
            p.x + z.width - 96.0 - 24.0,
            p.y + z.height * 0.45,
        ));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            show_ball(app);
            if args.iter().any(|a| a == "--settings") {
                open_settings_window(app);
            } else {
                let _ = app.emit_to("main", "menu", "open".to_string());
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, sc, ev| {
                    let st = app.state::<AppState>();
                    let act = st
                        .shortcuts
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|(s, _)| s == sc)
                        .map(|(_, a)| *a);
                    // 按住说话：按下开始、松开结束
                    if act == Some("talk") {
                        let down = ev.state() == ShortcutState::Pressed;
                        if down {
                            show_ball(app);
                        }
                        let _ = app.emit_to("main", "voice:key", down);
                        return;
                    }
                    if act == Some("feed") {
                        if ev.state() == ShortcutState::Released {
                            let app = app.clone();
                            let settings = st.cfg.lock().unwrap().sel.clone();
                            tauri::async_runtime::spawn_blocking(move || {
                                let result = selection::feed(&settings);
                                show_ball(&app);
                                let _ = app.emit_to("main", "sel:feed", result);
                            });
                        }
                        return;
                    }
                    if ev.state() != ShortcutState::Pressed {
                        return;
                    }
                    if let Some(act) = act {
                        if act == "capture_window" {
                            let key = capture::foreground_window();
                            *st.current_window.lock().unwrap() = key;
                        }
                        show_ball(app);
                        let _ = app.emit_to("main", "menu", act.to_string());
                    }
                })
                .build(),
        )
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            match app.path().app_log_dir() {
                Ok(d) => applog::init(&d),
                Err(e) => eprintln!("拿不到日志目录：{e}"),
            }
            let cfg_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            log::info!("数据目录 {}", data_dir.display());
            let cfg = Config::load(&cfg_dir);
            log::info!("配置目录 {}，划词 {}", cfg_dir.display(), cfg.sel.on);
            changes::prune(&data_dir.join("改动备份"), std::time::SystemTime::now());
            let store = store::Store::open(&data_dir)
                .map_err(|e| log::error!("history db: {e}"))
                .ok();
            app.manage(AppState {
                cfg: Mutex::new(cfg.clone()),
                cfg_dir,
                catalog: Mutex::new(Catalog::load(&data_dir)),
                catalog_busy: Mutex::new(false),
                data_dir: data_dir.clone(),
                store: Mutex::new(store),
                chats: Mutex::new(HashMap::new()),
                asks: Arc::new(agent::Asks::default()),
                changes: Arc::new(changes::Changes::default()),
                shortcuts: Mutex::new(vec![]),
                menu_window: Mutex::new(None),
                current_window: Mutex::new(None),
                screen_capture: tokio::sync::Mutex::new(()),
                sel_recent: Mutex::new(vec![]),
                settings_bd: Mutex::new(None),
                settings_goto: Mutex::new(None),
            });
            #[cfg(windows)]
            capture::watch_foreground();
            let handle = app.handle().clone();
            refresh_catalog(&handle, false);
            selection::spawn(handle.clone());
            spawn_routines(handle.clone());
            match register_shortcuts(&handle, &app.state::<AppState>(), &cfg) {
                Ok(None) => {}
                Ok(Some(e)) | Err(e) => eprintln!("[快捷键] {e}"),
            }

            // 托盘 / 菜单栏图标
            #[cfg(target_os = "macos")]
            let (tray_png, template) = (&include_bytes!("../icons/tray-template.png")[..], true);
            #[cfg(not(target_os = "macos"))]
            let (tray_png, template) = (&include_bytes!("../icons/tray.png")[..], false);
            TrayIconBuilder::with_id("main")
                .icon(tauri::image::Image::from_bytes(tray_png)?)
                .icon_as_template(template)
                .tooltip("悬浮球")
                .menu(&build_menu(&handle, true)?)
                .show_menu_on_left_click(true)
                .build(app)?;
            app.on_menu_event(|app, ev| on_menu(app, ev.id().as_ref()));

            if let Some(w) = app.get_webview_window("main") {
                place_initial(&w, cfg.position);
                w.show()?;
            }
            // 命令行带 --settings：启动后直接打开设置窗口
            if std::env::args().any(|a| a == "--settings") {
                open_settings_window(&handle);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::config::get_config,
            commands::config::save_model,
            commands::config::delete_model,
            commands::config::set_default_model,
            commands::config::save_prefs,
            settings_backdrop,
            settings_goto,
            commands::config::screens,
            commands::selection::sel_status,
            commands::selection::sel_place,
            commands::selection::sel_resize,
            commands::selection::sel_hide,
            commands::selection::sel_act,
            commands::selection::sel_copy,
            commands::selection::open_url,
            commands::selection::sel_open_privacy,
            commands::config::save_agent,
            commands::config::delete_agent,
            commands::config::save_skill,
            commands::config::delete_skill,
            commands::config::save_limits,
            commands::config::save_memory_prefs,
            commands::config::save_handoff_prefs,
            commands::handoff::handoff_scan,
            commands::handoff::handoff_pick,
            commands::handoff::handoff_go,
            commands::routine::save_routine,
            commands::routine::delete_routine,
            commands::routine::routine_next,
            commands::routine::routine_done,
            commands::changes::undo_change,
            commands::chat::answer_ask,
            commands::chat::save_perm,
            commands::config::list_models,
            commands::config::model_caps,
            commands::config::catalog_refresh,
            commands::voice::stt_begin,
            commands::voice::stt_cancel,
            open_settings,
            save_position,
            commands::config::test_connection,
            commands::config::set_autostart,
            commands::chat::chat,
            commands::chat::cancel_chat,
            commands::history::history_append,
            commands::history::history_list,
            commands::history::history_load,
            commands::history::history_delete,
            commands::history::history_clear,
            commands::history::agent_memory_counts,
            commands::history::agent_memory_list,
            commands::history::agent_memory_delete,
            commands::history::forget_memory,
            commands::capture::capture_screen,
            commands::capture::capture_window,
            commands::capture::share_sources,
            commands::capture::capture_shared,
            commands::capture::open_screen_privacy,
            commands::capture::backdrop,
            commands::capture::read_text_file,
            show_context_menu,
            ctx_act,
            hide_ball,
            commands::capture::js_log,
            transcribe
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_images_for_text_models() {
        let msgs = vec![json!({"role":"user","content":[
            {"type":"text","text":"这是什么"},
            {"type":"image_url","image_url":{"url":"data:image/jpeg;base64,xx"}}
        ]})];
        let out = prepare_messages("你是助手", false, msgs.clone());
        assert_eq!(out[0]["role"], "system");
        assert!(out[1]["content"].as_str().unwrap().contains("这是什么"));
        let out = prepare_messages(" ", true, msgs);
        assert_eq!(out.len(), 1);
        assert!(out[0]["content"].is_array());
    }
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
