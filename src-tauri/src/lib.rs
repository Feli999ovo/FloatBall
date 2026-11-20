//! 悬浮球主程序：一个透明、无边框、置顶的小窗口。
//! 前端（src/）负责小球、胶囊输入框和对话卡片的样子；这里负责系统能力：
//! 托盘、右键菜单、全局快捷键、开机启动、模型调用、钥匙串、历史记录、截屏。

mod agent;
mod applog;
mod capture;
mod catalog;
mod stt;
mod config;
mod llm;
mod routine;
mod handoff;
mod selection;
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
    v["lead_vision"] = json!(cfg.effective_model_for(cfg.lead()).map(|m| m.vision));
    v["autostart"] = json!(app.autolaunch().is_enabled().unwrap_or(false));
    v["platform"] = json!(std::env::consts::OS);
    // fx：系统自带的毛玻璃（只有 macOS 用；Windows 的亚克力在 Server、远程桌面、省电模式下会变成一块灰，改用和卡片同一种截图玻璃）
    v["fx"] = json!(cfg!(target_os = "macos") && system_glass());
    v["catalog_date"] = json!(state.catalog.lock().unwrap().fetched.clone());
    v
}

/// 系统毛玻璃能不能用：macOS 总能；Windows 要看「设置 → 个性化 → 颜色 → 透明效果」，关掉时亚克力会变成一块灰，
/// 这时设置窗口不铺亚克力，改用不透明的墨色底（settings.css 的 body:not(.fx)）。Linux 没有。
fn system_glass() -> bool {
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
        let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0".encode_utf16().collect();
        let name: Vec<u16> = "EnableTransparency\0".encode_utf16().collect();
        let mut v: u32 = 1;
        let mut n = std::mem::size_of::<u32>() as u32;
        // 读不到（老系统没有这个值）就当开着，和系统默认一致
        let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut v as *mut u32 as *mut _, &mut n) };
        r != 0 || v != 0
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

/// 存盘，并告诉小球窗口设置变了
fn commit(app: &AppHandle, state: &AppState, next: Config) -> Result<(), String> {
    next.save(&state.cfg_dir)?;
    *state.cfg.lock().unwrap() = next;
    selection::wake();
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


/// 联网时更新模型资料：用户在设置里打开了才更新，每天最多一次，失败就继续用旧的
fn refresh_catalog(app: &AppHandle, force: bool) {
    let st = app.state::<AppState>();
    if !st.cfg.lock().unwrap().catalog_auto {
        return;
    }
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
    win_shortcut: Option<String>,
    share_shortcut: Option<String>,
    feed_shortcut: Option<String>,
    #[serde(default)]
    new_shortcut: Option<String>,
    #[serde(default)]
    tasks_shortcut: Option<String>,
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
    #[serde(default)]
    hover: Option<config::Hover>,
    #[serde(default)]
    screen: Option<String>,
    #[serde(default)]
    screen_slide: Option<bool>,
    #[serde(default)]
    catalog_auto: Option<bool>,
    #[serde(default)]
    sel: Option<config::Sel>,
    #[serde(default)]
    voice: Option<config::Voice>,
    #[serde(default)]
    talk_shortcut: Option<String>,
}

/// 多显示器：鼠标位置和所有屏幕（都是物理像素），给小球判断该待在哪块屏幕
#[derive(Serialize)]
struct Screen { x: i32, y: i32, w: u32, h: u32, scale: f64, primary: bool }
#[derive(Serialize)]
struct Screens { cursor: Option<(f64, f64)>, list: Vec<Screen> }
#[tauri::command]
fn screens(app: AppHandle) -> Screens {
    let prim = app.primary_monitor().ok().flatten().map(|m| *m.position());
    let list = app.available_monitors().unwrap_or_default().iter().map(|m| {
        let (p, z) = (m.position(), m.size());
        Screen { x: p.x, y: p.y, w: z.width, h: z.height, scale: m.scale_factor(), primary: prim == Some(*p) }
    }).collect();
    Screens { cursor: app.cursor_position().ok().map(|c| (c.x, c.y)), list }
}

/// 模型以外的设置。返回 Some(提示)：已经保存，但有快捷键被别的程序占用了
#[tauri::command]
fn save_prefs(app: AppHandle, state: State<'_, AppState>, prefs: Prefs) -> Result<Option<String>, String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.shortcut = prefs.shortcut.trim().to_string();
    next.capture_shortcut = prefs.capture_shortcut.trim().to_string();
    if let Some(v) = prefs.win_shortcut { next.win_shortcut = v.trim().to_string(); }
    if let Some(v) = prefs.share_shortcut { next.share_shortcut = v.trim().to_string(); }
    if let Some(v) = prefs.feed_shortcut { next.feed_shortcut = v.trim().to_string(); }
    if let Some(v) = prefs.new_shortcut { next.new_shortcut = v.trim().to_string(); }
    if let Some(v) = prefs.tasks_shortcut { next.tasks_shortcut = v.trim().to_string(); }
    next.orb_size = prefs.orb_size.clamp(56, 88);
    next.glass = prefs.glass.min(60);
    if prefs.eye.len() == 7 && prefs.eye.starts_with('#') && prefs.eye[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        next.eye = prefs.eye;
    }
    if let Some(v) = prefs.idle_ms { next.idle_ms = v.min(24 * 3600 * 1000); }
    if let Some(v) = prefs.follow_system { next.follow_system = v; }
    if let Some(v) = prefs.auto_dock { next.auto_dock = v; }
    if let Some(v) = prefs.calm { next.calm = v; }
    if let Some(v) = prefs.hover { next.hover = v.normalized(); }
    if let Some(v) = prefs.sel { next.sel = v.normalized(); }
    if let Some(v) = prefs.voice { next.voice = v.normalized(); }
    if let Some(v) = prefs.talk_shortcut { next.talk_shortcut = v.trim().to_string(); }
    if let Some(v) = prefs.screen { if matches!(v.as_str(), "mouse" | "main" | "last") { next.screen = v; } }
    if let Some(v) = prefs.screen_slide { next.screen_slide = v; }
    let turned_on = prefs.catalog_auto == Some(true) && !next.catalog_auto;
    if let Some(v) = prefs.catalog_auto { next.catalog_auto = v; }
    let busy = register_shortcuts(&app, &state, &next)?;
    commit(&app, &state, next)?;
    if turned_on {
        refresh_catalog(&app, false);
    }
    Ok(busy)
}

/// 新建或修改一个 Agent，返回它的 id
#[tauri::command]
fn save_agent(app: AppHandle, state: State<'_, AppState>, agent: config::Agent) -> Result<String, String> {
    let mut cfg = state.cfg.lock().unwrap();
    let mut next = cfg.clone();
    let id = next.upsert_agent(agent)?;
    next.save(&state.cfg_dir)?;
    *cfg = next;
    drop(cfg);
    selection::wake();
    let _ = app.emit_to("main", "config", ());
    Ok(id)
}

#[tauri::command]
fn save_skill(app: AppHandle, state: State<'_, AppState>, skill: config::Skill) -> Result<(), String> {
    let mut cfg = state.cfg.lock().unwrap();
    let mut next = cfg.clone();
    next.save_skill(skill)?;
    next.save(&state.cfg_dir)?;
    *cfg = next;
    drop(cfg);
    let _ = app.emit("skills:changed", ());
    Ok(())
}

#[tauri::command]
fn delete_skill(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut cfg = state.cfg.lock().unwrap();
    let mut next = cfg.clone();
    next.remove_skill(&id);
    next.save(&state.cfg_dir)?;
    *cfg = next;
    drop(cfg);
    let _ = app.emit("skills:changed", ());
    Ok(())
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

/// 交给终端偏好：只保存设置，不打开终端或运行自定义命令
#[tauri::command]
fn save_handoff_prefs(app: AppHandle, state: State<'_, AppState>, custom: String, hint: bool) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.handoff.custom = custom;
    next.handoff.hint = hint;
    commit(&app, &state, next)
}

/// 定时任务：新增或修改（id 为空就是新的），返回 id
#[tauri::command]
fn save_routine(app: AppHandle, state: State<'_, AppState>, routine: routine::Routine) -> Result<String, String> {
    let mut next = state.cfg.lock().unwrap().clone();
    let mut r = routine::clean(routine);
    if r.prompt.is_empty() { return Err("要写上每次让它做什么".into()); }
    if let Some(old) = next.routines.iter_mut().find(|x| !r.id.is_empty() && x.id == r.id) {
        // 改了时间不补跑之前的：从现在开始算
        if old.every != r.every || old.time != r.time || old.weekday != r.weekday || !old.on && r.on {
            r.last = chrono::Local::now().timestamp();
        } else {
            r.last = old.last;
        }
        r.runs = old.runs.clone();
        *old = r.clone();
    } else {
        if next.routines.len() >= routine::MAX_ROUTINES { return Err(format!("最多 {} 个定时任务", routine::MAX_ROUTINES)); }
        r.id = (1..).map(|i| format!("rt{i}")).find(|id| !next.routines.iter().any(|x| &x.id == id)).unwrap();
        r.last = chrono::Local::now().timestamp(); // 新建的从下一次到点开始
        r.runs.clear();
        next.routines.push(r.clone());
    }
    commit(&app, &state, next)?;
    let _ = app.emit_to("settings", "config", ());
    Ok(r.id)
}

/// 交给终端：扫描本机装了的编程 CLI 和终端（只读 PATH，不运行它们）
#[tauri::command]
async fn handoff_scan() -> handoff::Scan {
    tauri::async_runtime::spawn_blocking(handoff::scan).await.unwrap_or(handoff::Scan { tools: vec![], terms: vec![] })
}

#[tauri::command]
async fn handoff_pick(start: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || handoff::pick_folder(&start)).await.map_err(|e| e.to_string())?
}

/// 交给终端：写好交接说明，在终端里、项目文件夹下打开工具。
/// tool = "custom" 时运行自定义命令。返回 (说明文件, 要粘贴的那句话；能直接带上时为空)
#[tauri::command]
fn handoff_go(app: AppHandle, state: State<'_, AppState>, md: String, tool: String, term: String, dir: String, save: bool, custom: String) -> Result<(String, String), String> {
    let d = PathBuf::from(dir.trim());
    if !d.is_dir() { return Err("项目文件夹不存在".into()); }
    if md.trim().is_empty() { return Err("交接说明是空的".into()); }
    let file = handoff::write(&md, &d, save, &state.data_dir)?;
    let prompt = handoff::prompt_for(&file);
    let (line, paste) = if tool == "custom" {
        if custom.trim().is_empty() { return Err("自定义命令是空的".into()); }
        (handoff::custom_line(&custom, &file), if custom.contains("{file}") { String::new() } else { prompt.clone() })
    } else {
        let found = handoff::scan().tools.into_iter().find(|f| f.id == tool).ok_or("没找到这个工具，重新扫描一下")?;
        let args = handoff::args_for(&tool, &prompt, &file);
        (handoff::line_for(&found.bin, &args), if found.direct { String::new() } else { prompt.clone() })
    };
    handoff::open(&term, &d, &line)?;
    log::info!("交给终端：{tool} 在 {term}");
    // 记住这次的选择：最近用的文件夹排最前，最多 5 个
    let mut next = state.cfg.lock().unwrap().clone();
    let h = &mut next.handoff;
    h.tool = tool; h.term = term; h.save = save; h.custom = custom.trim().to_string();
    let ds = d.display().to_string();
    h.dirs.retain(|x| x != &ds); h.dirs.insert(0, ds); h.dirs.truncate(5);
    commit(&app, &state, next)?;
    Ok((file.display().to_string(), paste))
}

#[tauri::command]
fn delete_routine(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.routines.retain(|r| r.id != id);
    commit(&app, &state, next)
}

/// 各个定时任务下一次的时间（秒，本地时区算），界面显示「下次 明天 09:00」
#[tauri::command]
fn routine_next(state: State<'_, AppState>) -> HashMap<String, i64> {
    state.cfg.lock().unwrap().routines.iter().filter(|r| r.on).filter_map(|r| Some((r.id.clone(), routine::next_local(r)?))).collect()
}

/// 定时任务跑完（界面报回来）：记一条运行记录
#[tauri::command]
fn routine_done(app: AppHandle, state: State<'_, AppState>, id: String, ok: bool, note: String) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    let Some(r) = next.routines.iter_mut().find(|r| r.id == id) else { return Ok(()) };
    r.runs.insert(0, routine::Run { at: chrono::Local::now().timestamp(), ok, note: note.chars().take(60).collect() });
    r.runs.truncate(5);
    commit(&app, &state, next)?;
    let _ = app.emit_to("settings", "config", ());
    Ok(())
}

/// 每 30 秒看一次哪些定时任务到点了：记下这次的时间（不会重复），通知小球窗口去跑
fn spawn_routines(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let st = app.state::<AppState>();
            let now = chrono::Local::now();
            let mut next = st.cfg.lock().unwrap().clone();
            let fire: Vec<routine::Routine> = next.routines.iter_mut().filter_map(|r| {
                let ts = routine::due(r, &now)?;
                r.last = ts;
                Some(r.clone())
            }).collect();
            if fire.is_empty() { continue; }
            if let Err(e) = commit(&app, &st, next) { log::warn!("定时任务保存失败：{e}"); continue; }
            for r in fire {
                log::info!("定时任务到点：{}", r.name);
                let _ = app.emit_to("main", "routine", &r);
            }
        }
    });
}

#[tauri::command]
fn save_position(state: State<'_, AppState>, x: f64, y: f64) -> Result<(), String> {
    let mut c = state.cfg.lock().unwrap();
    c.position = Some((x, y));
    c.save(&state.cfg_dir)
}

/// 选了服务商识别但没配好时的提示
const VOICE_NO_PROVIDER: &str = "语音识别选的是服务商，但还没有添加模型：设置 → 模型里添加，或在设置 → 语音里改用系统识别";
const VOICE_NO_KEY: &str = "语音识别选的服务商还没填 API Key：设置 → 模型里补上，或在设置 → 语音里改用系统识别";

/// 按下说话时调用：系统识别在 Windows 上从这里开始听麦克风（其他情况什么都不做）
#[tauri::command]
async fn stt_begin(state: State<'_, AppState>) -> Result<(), String> {
    let v = state.cfg.lock().unwrap().voice.clone();
    if v.engine != "system" {
        return Ok(());
    }
    if !stt::SUPPORTED {
        return Err(stt::UNSUPPORTED.into());
    }
    tauri::async_runtime::spawn_blocking(move || stt::begin(&v.lang)).await.map_err(|e| e.to_string())?
}

/// 取消录音：系统识别不要结果
#[tauri::command]
async fn stt_cancel() {
    let _ = tauri::async_runtime::spawn_blocking(stt::cancel).await;
}

/// 系统识别：Windows 停下取文字；macOS 把录音写成临时文件交给本机识别
async fn transcribe_system(bytes: Vec<u8>, mime: &str, lang: String) -> Result<String, String> {
    if cfg!(target_os = "macos") {
        let ext = if mime.contains("mp4") || mime.contains("m4a") || mime.contains("aac") { "m4a" } else if mime.contains("wav") { "wav" } else { "webm" };
        let path = std::env::temp_dir().join(format!("floatball-voice-{}.{ext}", std::process::id()));
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
        let p2 = path.clone();
        let r = tauri::async_runtime::spawn_blocking(move || recognize_file(&p2, &lang)).await.map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&path);
        r
    } else {
        let _ = (bytes, lang);
        tauri::async_runtime::spawn_blocking(stt::end).await.map_err(|e| e.to_string())?
    }
}

#[cfg(target_os = "macos")]
fn recognize_file(p: &std::path::Path, lang: &str) -> Result<String, String> {
    stt::file(p, lang)
}
#[cfg(not(target_os = "macos"))]
fn recognize_file(_p: &std::path::Path, _lang: &str) -> Result<String, String> {
    Err(stt::UNSUPPORTED.into())
}

/// 语音识别：系统自带（默认）或语音设置里选的模型服务商。返回识别出的文字
#[tauri::command]
async fn transcribe(state: State<'_, AppState>, audio: String, mime: String) -> Result<String, String> {
    use base64::Engine;
    let engine = state.cfg.lock().unwrap().voice.engine.clone();
    if engine == "system" {
        let lang = state.cfg.lock().unwrap().voice.lang.clone();
        let bytes = base64::engine::general_purpose::STANDARD.decode(audio.trim()).map_err(|_| "录音数据不对".to_string())?;
        return transcribe_system(bytes, &mime, lang).await;
    }
    let (m, v) = {
        let c = state.cfg.lock().unwrap();
        let v = c.voice.clone();
        let m = c.models.iter().find(|m| m.id == v.model).or_else(|| c.models.iter().find(|m| m.id == c.default_model)).or(c.models.first()).cloned();
        (m, v)
    };
    let m = m.ok_or(VOICE_NO_PROVIDER)?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(audio.trim()).map_err(|_| "录音数据不对".to_string())?;
    if bytes.len() > 24 * 1024 * 1024 {
        return Err("录音太长了，请分几次说".into());
    }
    let key = if m.no_key { None } else { Some(config::get_api_key(m.id).ok_or(VOICE_NO_KEY)?) };
    llm::transcribe(&m.base(), key.as_deref(), &v.stt, &v.lang, bytes, &mime).await
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
        // Windows：和对话卡片同一种玻璃。窗口还没出现，先截下鼠标所在的整块屏幕、缩小模糊，
        // 页面把它按窗口位置垫在墨层下面，拖动时跟着挪（见 settings.js 的 placeBackdrop）。
        // 「透明效果」被用户关掉时不截，用不透明的墨色底。
        #[cfg(windows)]
        if system_glass() {
            let at = app.cursor_position().map(|p| (p.x as i32, p.y as i32)).unwrap_or((0, 0));
            let bd = capture::screen_backdrop(at.0, at.1).map_err(|e| log::warn!("设置窗口底图：{e}")).ok();
            if let Some(st) = app.try_state::<AppState>() {
                *st.settings_bd.lock().unwrap() = bd;
            }
        }
        // 去掉系统标题栏（界面里自己画），窗口透明。macOS 后面铺系统材质；Linux 保留系统标题栏和不透明的底。
        let b = tauri::WebviewWindowBuilder::new(&app, "settings", tauri::WebviewUrl::App("settings.html".into()))
            .title("悬浮球设置")
            .inner_size(480.0, 600.0)
            .min_inner_size(440.0, 480.0)
            .center();
        #[cfg(any(windows, target_os = "macos"))]
        let b = b.decorations(false).transparent(true).shadow(true);
        #[cfg(target_os = "macos")]
        let b = if system_glass() {
            use tauri::{utils::config::WindowEffectsConfig, window::Effect};
            b.effects(WindowEffectsConfig { effects: vec![Effect::HudWindow], radius: Some(12.0), ..Default::default() })
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

#[derive(Serialize)]
struct SelStatus { supported: bool, trusted: bool, recent: Vec<String> }

#[tauri::command]
fn sel_status(state: State<'_, AppState>) -> SelStatus {
    SelStatus { supported: selection::SUPPORTED, trusted: selection::trusted(false), recent: state.sel_recent.lock().unwrap().clone() }
}

/// 工具栏量好自己的大小后调用：放到松开点旁边并显示（不抢焦点，原应用的选区保持不变）
/// 返回毛玻璃底图（和对话卡片同一种：截下工具栏将要盖住的桌面、模糊）。只有 Windows 有，拿不到就是 None，前端用不透明的底。
#[derive(serde::Serialize)]
#[cfg_attr(not(windows), allow(dead_code))]
struct SelBackdrop { url: String, w: f64, h: f64 }

#[tauri::command]
async fn sel_place(app: AppHandle, w: f64, h: f64, x: f64, y: f64) -> Option<SelBackdrop> {
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
            let shot = tauri::async_runtime::spawn_blocking(move || capture::backdrop_at(px, py, bx, by, bw, bh));
            match tokio::time::timeout(std::time::Duration::from_millis(250), shot).await {
                Ok(Ok(Ok(url))) => Some(SelBackdrop { url, w: bw as f64 / p.sf, h: bh as f64 / p.sf }),
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
fn sel_resize(app: AppHandle, w: f64, h: f64) {
    let Some(win) = app.get_webview_window("selbar") else { return };
    let sf = win.scale_factor().unwrap_or(1.0);
    let (wp, hp) = ((w * sf).round() as u32, (h * sf).round() as u32);
    if let (Ok(pos), Ok(Some(m))) = (win.outer_position(), win.current_monitor()) {
        let bottom = m.position().y + m.size().height as i32 - 8;
        let y = if pos.y + hp as i32 > bottom { (bottom - hp as i32).max(m.position().y) } else { pos.y };
        let _ = win.set_position(tauri::PhysicalPosition::new(pos.x, y));
        selection::bar_rect((pos.x, y, wp, hp));
    }
    let _ = win.set_size(tauri::PhysicalSize::new(wp, hp));
}

#[tauri::command]
fn sel_hide(app: AppHandle) {
    if let Some(win) = app.get_webview_window("selbar") {
        selection::bar_hide(&win);
    }
}

/// 工具栏上点了某个动作：交给小球（展开对话并发出去）
#[tauri::command]
fn sel_act(app: AppHandle, act: String, text: String) {
    if let Some(win) = app.get_webview_window("selbar") {
        selection::bar_hide(&win);
    }
    show_ball(&app);
    let _ = app.emit_to("main", "sel:act", serde_json::json!({ "act": act, "text": text }));
}

#[tauri::command]
fn sel_copy(text: String) -> Result<(), String> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        arboard::Clipboard::new().and_then(|mut c| c.set_text(text)).map_err(|e| e.to_string())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = text;
        Err("这个系统上还不支持".into())
    }
}

/// 用默认浏览器打开网页（只许 http/https）
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("只能打开网页".into());
    }
    open_external(&url)
}

/// macOS：打开 系统设置 → 隐私与安全性 → 辅助功能
#[tauri::command]
fn sel_open_privacy() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        let _ = selection::trusted(true);
        open_external("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
    } else {
        Ok(())
    }
}

fn open_external(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    let r = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", target]).creation_flags(0x0800_0000).spawn()
    };
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(target).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(target).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}

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
    let (cx, cy) = app.cursor_position().map(|p| (p.x as i32, p.y as i32)).unwrap_or((0, 0));
    capture_without_ball(&app, move || capture::capture_at(cx, cy)).await
}

async fn capture_without_ball(app: &AppHandle, take: impl FnOnce() -> Result<String, String> + Send + 'static) -> Result<String, String> {
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
    if was_visible { show_win(&win); }
    if was_focused { let _ = win.set_focus(); }
    res.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn capture_window(app: AppHandle) -> Result<String, String> {
    let key = app.state::<AppState>().current_window.lock().unwrap().take().ok_or(capture::WINDOW_UNAVAILABLE)?;
    capture::check_permission()?;
    tauri::async_runtime::spawn_blocking(move || capture::capture_window(key)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn share_sources() -> Result<Vec<capture::Source>, String> {
    capture::check_permission()?;
    tauri::async_runtime::spawn_blocking(capture::sources).await.map_err(|e| e.to_string())?
}

#[tauri::command]
async fn capture_shared(app: AppHandle, target: capture::Target) -> Result<Option<String>, String> {
    let supported = {
        let state = app.state::<AppState>();
        let cfg = state.cfg.lock().unwrap();
        cfg.effective_model_for(cfg.lead()).is_some_and(|model| model.vision)
    };
    if !supported { return Ok(None); }
    let image = match target {
        capture::Target::Screen { id } => capture_without_ball(&app, move || capture::capture_monitor(id)).await?,
        capture::Target::Window { key } => {
            capture::check_permission()?;
            tauri::async_runtime::spawn_blocking(move || capture::capture_window(key)).await.map_err(|e| e.to_string())??
        }
    };
    Ok(Some(image))
}

#[tauri::command]
fn open_screen_privacy() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        open_external("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
    } else {
        Ok(())
    }
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
    log::warn!("[前端] {msg}");
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
        "capture_window" => {
            let state = app.state::<AppState>();
            let key = state.menu_window.lock().unwrap().take().or_else(capture::foreground_window);
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
struct CtxItem { id: &'static str, label: &'static str, check: Option<bool>, sep: bool }

fn ctx_items<R: Runtime>(app: &AppHandle<R>, sharing: bool) -> Vec<CtxItem> {
    let auto = app.autolaunch().is_enabled().unwrap_or(false);
    let it = |id, label| CtxItem { id, label, check: None, sep: false };
    vec![
        it("new_chat", "新对话"),
        it("new_tmp", "新临时对话"),
        it("history", "历史记录"),
        it("capture", "截屏提问"),
        it("capture_window", "发送当前窗口"),
        it("share", if sharing { "停止共享" } else { "共享屏幕" }),
        it("handoff", "交给终端"),
        it("routines", "定时任务"),
        it("settings", "设置"),
        it("sleep", "睡眠"),
        it("toggle_ball", "隐藏小球"),
        CtxItem { id: "autostart", label: "开机启动", check: Some(auto), sep: false },
        CtxItem { id: "quit", label: "退出", check: None, sep: true },
    ]
}

/// 菜单窗口的逻辑大小：原型 .x-menu 宽 200、上下各 6 内边距、每项 38、分隔线 11；四周留 8 给阴影
const CTX_PAD: f64 = 8.0;
fn ctx_size(items: &[CtxItem]) -> (f64, f64) {
    let seps = items.iter().filter(|i| i.sep).count() as f64;
    (200.0 + CTX_PAD * 2.0, 12.0 + 38.0 * items.len() as f64 + 11.0 * seps + CTX_PAD * 2.0)
}

/// 放在鼠标右下方；右边或下边放不下就翻到左边、上边（和系统菜单一样）。返回物理像素的 (左, 上, 宽, 高)
fn ctx_rect(cx: f64, cy: f64, w: f64, h: f64, mon: (f64, f64, f64, f64)) -> (f64, f64) {
    let (mx, my, mw, mh) = mon;
    let x = if cx + w > mx + mw { (cx - w).max(mx) } else { cx };
    let y = if cy + h > my + mh { (cy - h).max(my) } else { cy };
    (x, y)
}

#[derive(Serialize, Clone)]
struct CtxShow { items: Vec<CtxItem>, bd: Option<String> }

#[tauri::command]
async fn show_context_menu(app: AppHandle, window: WebviewWindow, sharing: bool) -> Result<(), String> {
    let key = capture::foreground_window();
    *app.state::<AppState>().menu_window.lock().unwrap() = key;
    let Some(menu) = app.get_webview_window("ctxmenu") else {
        // 没有菜单窗口（不该发生）：退回系统菜单
        let m = build_menu(&app, false).map_err(|e| e.to_string())?;
        return window.popup_menu(&m).map_err(|e| e.to_string());
    };
    let items = ctx_items(&app, sharing);
    let c = app.cursor_position().map_err(|e| e.to_string())?;
    let mon = app.monitor_from_point(c.x, c.y).ok().flatten().or_else(|| window.current_monitor().ok().flatten());
    // 翻向按工作区算（不压住任务栏 / Dock）；截底图按整块显示器的坐标算
    let (sf, mrect, work) = match &mon {
        Some(m) => {
            let (wp, wz) = (m.work_area().position, m.work_area().size);
            (m.scale_factor(), (m.position().x as f64, m.position().y as f64), (wp.x as f64, wp.y as f64, wz.width as f64, wz.height as f64))
        }
        None => (1.0, (0.0, 0.0), (c.x - 4000.0, c.y - 4000.0, 8000.0, 8000.0)),
    };
    let (lw, lh) = ctx_size(&items);
    let (w, h) = (lw * sf, lh * sf);
    // 菜单的左上角（不含阴影边）对准鼠标
    let (x, y) = ctx_rect(c.x - CTX_PAD * sf, c.y - CTX_PAD * sf, w, h, work);
    let _ = menu.set_size(tauri::PhysicalSize::new(w.round() as u32, h.round() as u32));
    let _ = menu.set_position(tauri::PhysicalPosition::new(x.round() as i32, y.round() as i32));
    #[cfg(windows)]
    let bd = {
        let (bx, by) = ((x - mrect.0).round() as i32, (y - mrect.1).round() as i32);
        let (bw, bh) = (w.round() as u32, h.round() as u32);
        let (px, py) = ((x + w / 2.0) as i32, (y + h / 2.0) as i32);
        let shot = tauri::async_runtime::spawn_blocking(move || capture::backdrop_at(px, py, bx, by, bw, bh));
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
fn register_shortcuts<R: Runtime>(app: &AppHandle<R>, state: &AppState, cfg: &Config) -> Result<Option<String>, String> {
    let parse = |s: &str| -> Result<Option<Shortcut>, String> {
        let s = s.trim();
        if s.is_empty() {
            return Ok(None);
        }
        Shortcut::from_str(s).map(Some).map_err(|e| format!("快捷键「{s}」写法不对：{e}"))
    };
    let mut list: Vec<(Shortcut, &'static str)> = vec![];
    for (v, act) in cfg.shortcut_bindings() {
        // 默认的右 Alt 由划词线程读按键状态。
        if act == "talk" && v == config::TALK_RALT { continue; }
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
                    let st = app.state::<AppState>();
                    let act = st.shortcuts.lock().unwrap().iter().find(|(s, _)| s == sc).map(|(_, a)| *a);
                    // 按住说话：按下开始、松开结束
                    if act == Some("talk") {
                        let down = ev.state() == ShortcutState::Pressed;
                        if down { show_ball(app); }
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
            get_config,
            save_model,
            delete_model,
            set_default_model,
            save_prefs, settings_backdrop, settings_goto, screens, sel_status, sel_place, sel_resize, sel_hide, sel_act, sel_copy, open_url, sel_open_privacy, save_agent, delete_agent, save_skill, delete_skill, save_limits, save_handoff_prefs, handoff_scan, handoff_pick, handoff_go, save_routine, delete_routine, routine_next, routine_done, answer_ask, save_perm,
            list_models,
            model_caps,
            catalog_refresh,
            stt_begin,
            stt_cancel,
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
            capture_screen, capture_window, share_sources, capture_shared, open_screen_privacy,
            backdrop,
            read_text_file,
            show_context_menu, ctx_act,
            hide_ball,
            js_log,
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
        assert_eq!(ctx_rect(-50.0, 10.0, 216.0, 300.0, (-1920.0, 0.0, 1920.0, 1080.0)), (-266.0, 10.0));
    }
    #[test]
    fn ctx_menu_size_matches_prototype() {
        let it = |sep| CtxItem { id: "a", label: "a", check: None, sep };
        // 高：13 项×38 + 1 条分隔线×11 + 上下 6×2 + 四周 8×2 = 533；宽：200 + 8×2 = 216
        let items: Vec<_> = (0..13).map(|i| it(i == 12)).collect();
        let (w, h) = ctx_size(&items);
        assert_eq!((w, h), (216.0, 533.0));
        assert_eq!(ctx_rect(1000.0, 740.0, w, h, (0.0, 0.0, 1024.0, 768.0)), (784.0, 207.0));
    }
}
