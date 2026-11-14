//! 悬浮球主程序：一个透明、无边框、置顶的小窗口。
//! 前端（src/）负责小球、胶囊输入框和对话卡片的样子；这里负责系统能力：
//! 托盘、右键菜单、全局快捷键、开机启动、模型调用、钥匙串、历史记录、截屏。

mod agent;
mod capture;
mod catalog;
mod config;
mod llm;
mod store;

use catalog::Catalog;
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

/// 给界面看的设置：config.json 里的内容，加上每个模型有没有存 Key、默认模型能不能用。Key 本身不给界面。
#[tauri::command]
fn get_config(app: AppHandle, state: State<'_, AppState>) -> Value {
    let cfg = state.cfg.lock().unwrap().clone();
    let has_key = |id: u32| config::get_api_key(id).is_some();
    let mut v = serde_json::to_value(&cfg).unwrap_or_default();
    for m in v["models"].as_array_mut().into_iter().flatten() {
        let id = m["id"].as_u64().unwrap_or(0) as u32;
        m["has_key"] = json!(has_key(id));
    }
    v["ready"] = json!(cfg.current().is_some_and(|m| m.no_key || has_key(m.id)));
    v["autostart"] = json!(app.autolaunch().is_enabled().unwrap_or(false));
    v["platform"] = json!(std::env::consts::OS);
    v["catalog_date"] = json!(state.catalog.lock().unwrap().fetched.clone());
    v
}

/// 存盘，并告诉小球窗口设置变了
fn commit(app: &AppHandle, state: &AppState, next: Config) -> Result<(), String> {
    next.save(&state.cfg_dir)?;
    *state.cfg.lock().unwrap() = next;
    let _ = app.emit_to("main", "config", ());
    Ok(())
}

/// 新增或修改模型库里的一个模型。api_key：None 表示不改；Some("") 表示删除。返回模型的编号。
#[tauri::command]
fn save_model(app: AppHandle, state: State<'_, AppState>, model: Model, api_key: Option<String>) -> Result<u32, String> {
    let mut next = state.cfg.lock().unwrap().clone();
    let (id, new) = next.upsert(model)?;
    // 新模型用的可能是以前删掉的那个编号：先把那个编号下的 Key 清掉，不能让它接着用别的模型的 Key
    if new {
        config::set_api_key(id, "")?;
    }
    if let Some(k) = api_key {
        config::set_api_key(id, k.trim())?;
    }
    commit(&app, &state, next)?;
    Ok(id)
}

#[tauri::command]
fn delete_model(app: AppHandle, state: State<'_, AppState>, id: u32) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.remove(id);
    commit(&app, &state, next)?;
    config::set_api_key(id, "")
}

#[tauri::command]
fn set_default_model(app: AppHandle, state: State<'_, AppState>, id: u32) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    if !next.models.iter().any(|m| m.id == id) {
        return Err("这个模型已经不在了".into());
    }
    next.default_model = id;
    commit(&app, &state, next)
}

/// 查模型能力（models.dev 资料）
#[tauri::command]
fn model_caps(state: State<'_, AppState>, provider: String, base_url: String, model: String) -> catalog::Caps {
    state.catalog.lock().unwrap().lookup(&provider, &base_url, &model)
}


/// 联网时更新模型资料：每天最多一次，失败就继续用旧的
fn refresh_catalog(app: &AppHandle, force: bool) {
    let st = app.state::<AppState>();
    if !force && !Catalog::stale(&st.data_dir) {
        return;
    }
    {
        let mut busy = st.catalog_busy.lock().unwrap();
        if *busy {
            return;
        }
        *busy = true;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let today = today();
        let res = Catalog::fetch(&today).await;
        let st = app.state::<AppState>();
        match res {
            Ok(c) => {
                if let Err(e) = c.save(&st.data_dir) {
                    log::warn!("模型资料保存失败：{e}");
                }
                *st.catalog.lock().unwrap() = c;
                let _ = app.emit("catalog-updated", today);
            }
            Err(e) => log::info!("模型资料没更新（继续用旧的）：{e}"),
        }
        *st.catalog_busy.lock().unwrap() = false;
    });
}

#[tauri::command]
fn catalog_refresh(app: AppHandle) {
    refresh_catalog(&app, false);
}


/// 今天的日期（UTC，YYYY-MM-DD），只用来标记资料版本
fn today() -> String {
    let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86400).unwrap_or(0) as i64;
    // 公历换算（Howard Hinnant 算法）
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

#[derive(Deserialize)]
struct Prefs {
    shortcut: String,
    capture_shortcut: String,
    orb_size: u32,
    glass: u8,
    eye: String,
    #[serde(default)]
    idle_ms: Option<u64>,
    #[serde(default)]
    follow_system: Option<bool>,
    #[serde(default)]
    auto_dock: Option<bool>,
    #[serde(default)]
    calm: Option<bool>,
}

/// 模型以外的设置。返回 Some(提示)：已经保存，但有快捷键被别的程序占用了
#[tauri::command]
fn save_prefs(app: AppHandle, state: State<'_, AppState>, prefs: Prefs) -> Result<Option<String>, String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.shortcut = prefs.shortcut.trim().to_string();
    next.capture_shortcut = prefs.capture_shortcut.trim().to_string();
    next.orb_size = prefs.orb_size.clamp(56, 88);
    next.glass = prefs.glass.min(60);
    if prefs.eye.len() == 7 && prefs.eye.starts_with('#') && prefs.eye[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        next.eye = prefs.eye;
    }
    if let Some(v) = prefs.idle_ms { next.idle_ms = v.min(24 * 3600 * 1000); }
    if let Some(v) = prefs.follow_system { next.follow_system = v; }
    if let Some(v) = prefs.auto_dock { next.auto_dock = v; }
    if let Some(v) = prefs.calm { next.calm = v; }
    let busy = register_shortcuts(&app, &state, &next)?;
    commit(&app, &state, next)?;
    Ok(busy)
}

/// 新建或修改一个 Agent，返回它的 id
#[tauri::command]
fn save_agent(app: AppHandle, state: State<'_, AppState>, agent: config::Agent) -> Result<String, String> {
    let mut next = state.cfg.lock().unwrap().clone();
    let id = next.upsert_agent(agent)?;
    commit(&app, &state, next)?;
    Ok(id)
}

#[tauri::command]
fn delete_agent(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.remove_agent(&id)?;
    commit(&app, &state, next)
}

#[tauri::command]
fn save_limits(app: AppHandle, state: State<'_, AppState>, limits: config::Limits) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.limits = config::Limits { tokens: limits.tokens, steps: limits.steps.clamp(1, 500), parallel: limits.parallel.clamp(1, 10) };
    commit(&app, &state, next)
}

#[tauri::command]
fn save_position(state: State<'_, AppState>, x: f64, y: f64) -> Result<(), String> {
    let mut c = state.cfg.lock().unwrap();
    c.position = Some((x, y));
    c.save(&state.cfg_dir)
}

/// 表单里填了 Key 就用填的；没填就用这个模型已经存的
fn key_for(api_key: Option<String>, id: Option<u32>) -> Option<String> {
    api_key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty()).or_else(|| id.and_then(config::get_api_key))
}

#[tauri::command]
async fn test_connection(base_url: String, model: String, api_key: Option<String>, id: Option<u32>) -> Result<String, String> {
    let m = Model { base_url, model, ..Model::default() };
    llm::test_connection(&m.endpoint(), key_for(api_key, id).as_deref(), m.model.trim()).await
}

#[tauri::command]
async fn list_models(base_url: String, api_key: Option<String>, id: Option<u32>) -> Result<Vec<String>, String> {
    let m = Model { base_url, ..Model::default() };
    llm::list_models(&m.base(), key_for(api_key, id).as_deref()).await
}

#[tauri::command]
fn set_autostart(app: AppHandle, on: bool) -> Result<bool, String> {
    let al = app.autolaunch();
    if on { al.enable() } else { al.disable() }.map_err(|e| e.to_string())?;
    let now = al.is_enabled().unwrap_or(on);
    refresh_menus(&app);
    Ok(now)
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
        // 去掉系统标题栏（界面里自己画），窗口透明，后面铺系统自带的毛玻璃：Windows 用亚克力，macOS 用系统材质。
        // Linux 没有这个效果，保留系统标题栏和不透明的底。
        let b = tauri::WebviewWindowBuilder::new(&app, "settings", tauri::WebviewUrl::App("settings.html".into()))
            .title("悬浮球设置")
            .inner_size(480.0, 600.0)
            .min_inner_size(440.0, 480.0)
            .center();
        #[cfg(any(windows, target_os = "macos"))]
        use tauri::{utils::config::WindowEffectsConfig, window::Effect};
        #[cfg(any(windows, target_os = "macos"))]
        let b = b.decorations(false).transparent(true).shadow(true).effects(WindowEffectsConfig {
            effects: vec![Effect::Acrylic, Effect::HudWindow],
            radius: Some(12.0),
            ..Default::default()
        });
        let built = b.build();
        if let Err(e) = built {
            log::error!("settings window: {e}");
        }
    });
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    open_settings_window(&app);
}

// ───────── 对话 ─────────

/// 加系统提示词；模型不支持图片时把图片换成一句说明
pub(crate) fn prepare_messages(system_prompt: &str, vision: bool, messages: Vec<Value>) -> Vec<Value> {
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

/// 提问交给主控（agent.rs）：主控用自己的指令和模型，可以用工具、把步骤交给别的 Agent。
/// 主模型出错（还没回一个字）时换备用模型再试一次。
#[tauri::command]
fn chat(app: AppHandle, state: State<'_, AppState>, id: String, messages: Vec<Value>, on_event: Channel<ChatEvent>) -> Result<(), String> {
    let cfg = state.cfg.lock().unwrap().clone();
    let lead = cfg.lead().clone();
    let got = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (g2, ch) = (got.clone(), on_event.clone());
    // 每轮的 Done / Usage 不转给界面，整个任务结束时发一次
    let emit: Arc<dyn Fn(ChatEvent) + Send + Sync> = Arc::new(move |ev: ChatEvent| {
        if matches!(ev, ChatEvent::Done | ChatEvent::Usage { .. } | ChatEvent::ToolCall { .. }) {
            return;
        }
        if !matches!(ev, ChatEvent::Error { .. }) {
            g2.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let _ = ch.send(ev);
    });
    let app2 = app.clone();
    let world: Arc<dyn agent::World> = Arc::new(agent::RealWorld {
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
    let task = tauri::async_runtime::spawn(async move {
        let t = agent::Task::new(cfg.clone(), world.clone(), asks.clone(), trash.clone(), emit.clone());
        let mut r = t.run(&lead, messages.clone(), true).await;
        let mut used = t.tokens();
        let backup = cfg.models.iter().any(|m| m.id == lead.fallback && Some(m.id) != cfg.model_for(&lead).map(|x| x.id));
        if let (Err(e), true, false) = (&r, backup, got.load(std::sync::atomic::Ordering::Relaxed)) {
            js_log(format!("主模型出错，换备用模型：{e}"));
            let mut l2 = lead.clone();
            l2.model = lead.fallback;
            l2.auto = false;
            let t2 = agent::Task::new(cfg, world, asks, trash, emit);
            r = t2.run(&l2, messages, true).await;
            used += t2.tokens();
        }
        match r {
            Ok(_) => {
                let _ = on_event.send(ChatEvent::Usage { prompt: used, completion: 0 });
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
fn answer_ask(state: State<'_, AppState>, ask: u64, answer: agent::Answer) -> bool {
    state.asks.answer(ask, answer)
}

/// 权限页：撤销一条「总是允许」、开关「离开电脑时暂停敏感操作」
#[tauri::command]
fn save_perm(app: AppHandle, state: State<'_, AppState>, always_allow: Vec<config::Allow>, pause_away: bool) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.always_allow = always_allow;
    next.pause_away = pause_away;
    commit(&app, &state, next)
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

/// 卡片的毛玻璃底图（见 capture::backdrop_at）。拿不到就返回空，前端用不透明的底。
#[tauri::command]
async fn backdrop(px: i32, py: i32, x: i32, y: i32, w: u32, h: u32) -> Option<String> {
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
        "settings" => open_settings_window(app),
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

/// 写法不对或两个一样时返回 Err；写法没问题但被别的程序占用时返回 Ok(Some(提示))
fn register_shortcuts<R: Runtime>(app: &AppHandle<R>, state: &AppState, cfg: &Config) -> Result<Option<String>, String> {
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
    Ok(if errs.is_empty() { None } else { Some(errs.join("；")) })
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
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            show_ball(app);
            if args.iter().any(|a| a == "--settings") {
                open_settings_window(app);
            } else {
                let _ = app.emit_to("main", "menu", "open".to_string());
            }
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
                catalog: Mutex::new(Catalog::load(&data_dir)),
                catalog_busy: Mutex::new(false),
                data_dir: data_dir.clone(),
                store: Mutex::new(store),
                chats: Mutex::new(HashMap::new()),
                asks: Arc::new(agent::Asks::default()),
                shortcuts: Mutex::new((None, None)),
            });
            let handle = app.handle().clone();
            refresh_catalog(&handle, false);
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
            get_config,
            save_model,
            delete_model,
            set_default_model,
            save_prefs, save_agent, delete_agent, save_limits, answer_ask, save_perm,
            list_models,
            model_caps,
            catalog_refresh,
            open_settings,
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
            backdrop,
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
