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
use commands::window::*;
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

// ───────── 划词工具栏 ─────────

// ───────── 对话 ─────────

// ───────── 历史 ─────────

// ───────── 截屏与文件 ─────────

// ───────── 菜单、托盘、快捷键 ─────────

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
            commands::window::settings_backdrop,
            commands::window::settings_goto,
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
            commands::window::open_settings,
            commands::window::save_position,
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
            commands::window::show_context_menu,
            commands::window::ctx_act,
            commands::window::hide_ball,
            commands::capture::js_log,
            transcribe
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
