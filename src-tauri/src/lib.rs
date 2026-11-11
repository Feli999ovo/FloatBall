//! 悬浮球主程序：一个透明、无边框、置顶的小窗口。
//! 前端（src/）负责小球、胶囊输入框和对话卡片的样子；这里负责系统能力：
//! 托盘、右键菜单、全局快捷键、开机启动、模型调用、钥匙串、历史记录、截屏。

mod capture;
mod config;
mod llm;
mod store;

use config::Config;
use llm::ChatEvent;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Mutex;
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
    store: Mutex<Option<store::Store>>,
    chats: Mutex<HashMap<String, JoinHandle<()>>>,
    shortcuts: Mutex<(Option<Shortcut>, Option<Shortcut>)>,
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

#[derive(Serialize)]
struct ConfigView {
    #[serde(flatten)]
    cfg: Config,
    has_key: bool,
    autostart: bool,
    platform: &'static str,
}

#[tauri::command]
fn get_config(app: AppHandle, state: State<'_, AppState>) -> ConfigView {
    ConfigView {
        cfg: state.cfg.lock().unwrap().clone(),
        has_key: config::get_api_key().is_some(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        platform: std::env::consts::OS,
    }
}

/// api_key：None 表示不改；Some("") 表示删除
#[tauri::command]
fn save_config(app: AppHandle, state: State<'_, AppState>, cfg: Config, api_key: Option<String>) -> Result<(), String> {
    if let Some(k) = api_key {
        config::set_api_key(k.trim())?;
    }
    let mut next = cfg;
    next.position = state.cfg.lock().unwrap().position;
    register_shortcuts(&app, &state, &next)?;
    next.save(&state.cfg_dir)?;
    *state.cfg.lock().unwrap() = next;
    Ok(())
}

#[tauri::command]
fn save_position(state: State<'_, AppState>, x: f64, y: f64) -> Result<(), String> {
    let mut c = state.cfg.lock().unwrap();
    c.position = Some((x, y));
    c.save(&state.cfg_dir)
}

#[tauri::command]
async fn test_connection(base_url: String, model: String, api_key: Option<String>) -> Result<String, String> {
    let c = Config { base_url, model: model.clone(), ..Config::default() };
    let key = api_key.filter(|k| !k.trim().is_empty()).or_else(config::get_api_key);
    llm::test_connection(&c.endpoint(), key.as_deref(), &model).await
}

#[tauri::command]
fn set_autostart(app: AppHandle, on: bool) -> Result<bool, String> {
    let al = app.autolaunch();
    if on { al.enable() } else { al.disable() }.map_err(|e| e.to_string())?;
    let now = al.is_enabled().unwrap_or(on);
    refresh_menus(&app);
    Ok(now)
}

// ───────── 对话 ─────────

/// 加系统提示词；模型不支持图片时把图片换成一句说明
fn prepare_messages(cfg: &Config, messages: Vec<Value>) -> Vec<Value> {
    let mut out = Vec::with_capacity(messages.len() + 1);
    if !cfg.system_prompt.trim().is_empty() {
        out.push(json!({"role":"system","content":cfg.system_prompt}));
    }
    for mut m in messages {
        if !cfg.vision {
            if let Some(parts) = m.get("content").and_then(Value::as_array).cloned() {
                let text: Vec<String> = parts
                    .iter()
                    .filter_map(|p| match p.get("type").and_then(Value::as_str) {
                        Some("text") => p.get("text").and_then(Value::as_str).map(str::to_string),
                        Some("image_url") => Some("[附了一张截图，但当前模型设置为不支持图片]".into()),
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

#[tauri::command]
fn chat(state: State<'_, AppState>, id: String, messages: Vec<Value>, on_event: Channel<ChatEvent>) -> Result<(), String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let key = config::get_api_key();
    if key.is_none() && !cfg.allow_no_key {
        let _ = on_event.send(ChatEvent::Error {
            message: "还没有填 API Key。右键小球 → 设置 → 模型。用本地模型（如 Ollama）可勾选「不需要 Key」。".into(),
        });
        return Ok(());
    }
    let msgs = prepare_messages(&cfg, messages);
    let endpoint = cfg.endpoint();
    let model = cfg.model.clone();
    let task = tauri::async_runtime::spawn(async move {
        llm::stream_chat(&endpoint, key.as_deref(), &model, &msgs, |ev| {
            let _ = on_event.send(ev);
        })
        .await;
    });
    let mut chats = state.chats.lock().unwrap();
    chats.retain(|_, h| !h.inner().is_finished());
    chats.insert(id, task);
    Ok(())
}

#[tauri::command]
fn cancel_chat(state: State<'_, AppState>, id: String) {
    if let Some(h) = state.chats.lock().unwrap().remove(&id) {
        h.abort();
    }
}

// ───────── 历史 ─────────

fn with_store<T>(state: &State<'_, AppState>, f: impl FnOnce(&store::Store) -> rusqlite::Result<T>) -> Result<T, String> {
    let g = state.store.lock().unwrap();
    let s = g.as_ref().ok_or("历史记录数据库打不开")?;
    f(s).map_err(|e| e.to_string())
}

#[tauri::command]
fn history_append(state: State<'_, AppState>, conv_id: Option<i64>, role: String, content: String) -> Result<i64, String> {
    with_store(&state, |s| s.append(conv_id, &role, &content))
}
#[tauri::command]
fn history_list(state: State<'_, AppState>) -> Result<Vec<store::ConvSummary>, String> {
    with_store(&state, |s| s.list(200))
}
#[tauri::command]
fn history_load(state: State<'_, AppState>, conv_id: i64) -> Result<Vec<store::Msg>, String> {
    with_store(&state, |s| s.load(conv_id))
}
#[tauri::command]
fn history_delete(state: State<'_, AppState>, conv_id: i64) -> Result<(), String> {
    with_store(&state, |s| s.delete(conv_id))
}
#[tauri::command]
fn history_clear(state: State<'_, AppState>) -> Result<(), String> {
    with_store(&state, |s| s.clear())
}

// ───────── 截屏与文件 ─────────

#[tauri::command]
async fn capture_screen(app: AppHandle) -> Result<String, String> {
    let win = main_window(&app).ok_or("no window")?;
    let (cx, cy) = app
        .cursor_position()
        .map(|p| (p.x as i32, p.y as i32))
        .unwrap_or((0, 0));
    // 先把自己藏起来，别把小球截进去
    hide_win(&win);
    tokio::time::sleep(std::time::Duration::from_millis(220)).await;
    let res = tauri::async_runtime::spawn_blocking(move || capture::capture_at(cx, cy))
        .await
        .map_err(|e| e.to_string())?;
    show_win(&win);
    let _ = win.set_focus();
    res
}

#[derive(Serialize)]
struct TextFile {
    name: String,
    text: String,
}

#[tauri::command]
fn read_text_file(path: String) -> Result<TextFile, String> {
    capture::read_text_file(&path).map(|(name, text)| TextFile { name, text })
}

/// 前端的报错打到终端（npm run dev 时能看到），方便排查
#[tauri::command]
fn js_log(msg: String) {
    eprintln!("[前端] {msg}");
}

// ───────── 菜单、托盘、快捷键 ─────────

fn build_menu<R: Runtime>(app: &AppHandle<R>, for_tray: bool) -> tauri::Result<Menu<R>> {
    let auto = app.autolaunch().is_enabled().unwrap_or(false);
    let hidden = main_window(app).map(|w| !w.is_visible().unwrap_or(true)).unwrap_or(false);
    let m = Menu::new(app)?;
    if for_tray {
        m.append(&MenuItem::with_id(app, "toggle_ball", if hidden { "显示小球" } else { "隐藏小球" }, true, None::<&str>)?)?;
        m.append(&PredefinedMenuItem::separator(app)?)?;
    }
    m.append(&MenuItem::with_id(app, "new_chat", "新对话", true, None::<&str>)?)?;
    m.append(&MenuItem::with_id(app, "capture", "截屏提问", true, None::<&str>)?)?;
    m.append(&MenuItem::with_id(app, "history", "历史记录", true, None::<&str>)?)?;
    m.append(&PredefinedMenuItem::separator(app)?)?;
    m.append(&MenuItem::with_id(app, "settings", "设置", true, None::<&str>)?)?;
    m.append(&CheckMenuItem::with_id(app, "autostart", "开机启动", true, auto, None::<&str>)?)?;
    if !for_tray {
        m.append(&MenuItem::with_id(app, "toggle_ball", "隐藏小球", true, None::<&str>)?)?;
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
        other => {
            show_ball(app);
            let _ = app.emit_to("main", "menu", other.to_string());
        }
    }
}

#[tauri::command]
fn show_context_menu(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    let menu = build_menu(&app, false).map_err(|e| e.to_string())?;
    window.popup_menu(&menu).map_err(|e| e.to_string())
}

#[tauri::command]
fn hide_ball(app: AppHandle) {
    if let Some(w) = main_window(&app) {
        hide_win(&w);
    }
    refresh_menus(&app);
}

fn register_shortcuts<R: Runtime>(app: &AppHandle<R>, state: &AppState, cfg: &Config) -> Result<(), String> {
    let parse = |s: &str| -> Result<Option<Shortcut>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        Shortcut::from_str(s).map(Some).map_err(|e| format!("快捷键「{s}」写法不对：{e}"))
    };
    let main = parse(&cfg.shortcut)?;
    let cap = parse(&cfg.capture_shortcut)?;
    if main.is_some() && main == cap {
        return Err("两个快捷键不能一样".into());
    }
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let mut errs = vec![];
    for s in [&main, &cap].into_iter().flatten() {
        if let Err(e) = gs.register(*s) {
            errs.push(format!("{s} 被别的程序占用了（{e}）"));
        }
    }
    *state.shortcuts.lock().unwrap() = (main, cap);
    if errs.is_empty() { Ok(()) } else { Err(errs.join("；")) }
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
        let _ = w.set_position(LogicalPosition::new(p.x + z.width - 96.0 - 24.0, p.y + z.height * 0.45));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_ball(app);
            let _ = app.emit_to("main", "menu", "open".to_string());
        }))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, sc, ev| {
                    if ev.state() != ShortcutState::Pressed {
                        return;
                    }
                    let st = app.state::<AppState>();
                    let (main, cap) = st.shortcuts.lock().unwrap().clone();
                    if main.as_ref() == Some(sc) {
                        show_ball(app);
                        let _ = app.emit_to("main", "menu", "toggle".to_string());
                    } else if cap.as_ref() == Some(sc) {
                        show_ball(app);
                        let _ = app.emit_to("main", "menu", "capture".to_string());
                    }
                })
                .build(),
        )
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let cfg_dir = app.path().app_config_dir()?;
            let data_dir = app.path().app_data_dir()?;
            let cfg = Config::load(&cfg_dir);
            let store = store::Store::open(&data_dir).map_err(|e| log::error!("history db: {e}")).ok();
            app.manage(AppState {
                cfg: Mutex::new(cfg.clone()),
                cfg_dir,
                store: Mutex::new(store),
                chats: Mutex::new(HashMap::new()),
                shortcuts: Mutex::new((None, None)),
            });
            let handle = app.handle().clone();
            if let Err(e) = register_shortcuts(&handle, &app.state::<AppState>(), &cfg) {
                log::warn!("shortcut: {e}");
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            save_position,
            test_connection,
            set_autostart,
            chat,
            cancel_chat,
            history_append,
            history_list,
            history_load,
            history_delete,
            history_clear,
            capture_screen,
            read_text_file,
            show_context_menu,
            hide_ball,
            js_log
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_images_for_text_models() {
        let mut c = Config::default();
        c.vision = false;
        let msgs = vec![json!({"role":"user","content":[
            {"type":"text","text":"这是什么"},
            {"type":"image_url","image_url":{"url":"data:image/jpeg;base64,xx"}}
        ]})];
        let out = prepare_messages(&c, msgs.clone());
        assert_eq!(out[0]["role"], "system");
        assert!(out[1]["content"].as_str().unwrap().contains("这是什么"));
        c.vision = true;
        c.system_prompt.clear();
        let out = prepare_messages(&c, msgs);
        assert_eq!(out.len(), 1);
        assert!(out[0]["content"].is_array());
    }
}
