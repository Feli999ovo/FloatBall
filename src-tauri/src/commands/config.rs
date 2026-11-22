use crate::*;

/// 给界面看的设置：config.json 里的内容，加上每个模型有没有存 Key、默认模型能不能用。Key 本身不给界面。
#[tauri::command]
pub fn get_config(app: AppHandle, state: State<'_, AppState>) -> Value {
    let cfg = state.cfg.lock().unwrap().clone();
    let has_key = |id: u32| config::get_api_key(id).is_some();
    let catalog = state.catalog.lock().unwrap();
    let mut v = serde_json::to_value(&cfg).unwrap_or_default();
    for m in v["models"].as_array_mut().into_iter().flatten() {
        let id = m["id"].as_u64().unwrap_or(0) as u32;
        m["has_key"] = json!(has_key(id));
        let base = m["base_url"].as_str().unwrap_or("");
        let model = m["model"].as_str().unwrap_or("");
        let provider = m["provider"].as_str().unwrap_or("");
        if let Some((input, output)) = catalog.price(base, model, provider) {
            m["input_cost"] = json!(input);
            m["output_cost"] = json!(output);
        }
    }
    drop(catalog);
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
pub(crate) fn system_glass() -> bool {
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Registry::{
            RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD,
        };
        let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
            .encode_utf16()
            .collect();
        let name: Vec<u16> = "EnableTransparency\0".encode_utf16().collect();
        let mut v: u32 = 1;
        let mut n = std::mem::size_of::<u32>() as u32;
        // 读不到（老系统没有这个值）就当开着，和系统默认一致
        let r = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                &mut v as *mut u32 as *mut _,
                &mut n,
            )
        };
        r != 0 || v != 0
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

/// 存盘，并告诉小球窗口设置变了
pub(crate) fn commit(app: &AppHandle, state: &AppState, next: Config) -> Result<(), String> {
    next.save(&state.cfg_dir)?;
    *state.cfg.lock().unwrap() = next;
    selection::wake();
    let _ = app.emit_to("main", "config", ());
    Ok(())
}

/// 新增或修改模型库里的一个模型。api_key：None 表示不改；Some("") 表示删除。返回模型的编号。
#[tauri::command]
pub fn save_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model: Model,
    api_key: Option<String>,
) -> Result<u32, String> {
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
pub fn delete_model(app: AppHandle, state: State<'_, AppState>, id: u32) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.remove(id);
    commit(&app, &state, next)?;
    config::set_api_key(id, "")
}

#[tauri::command]
pub fn set_default_model(app: AppHandle, state: State<'_, AppState>, id: u32) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    if !next.models.iter().any(|m| m.id == id) {
        return Err("这个模型已经不在了".into());
    }
    next.default_model = id;
    commit(&app, &state, next)
}

/// 查模型能力（models.dev 资料）
#[tauri::command]
pub fn model_caps(
    state: State<'_, AppState>,
    provider: String,
    base_url: String,
    model: String,
) -> catalog::Caps {
    state
        .catalog
        .lock()
        .unwrap()
        .lookup(&provider, &base_url, &model)
}

/// 联网时更新模型资料：用户在设置里打开了才更新，每天最多一次，失败就继续用旧的
pub(crate) fn refresh_catalog(app: &AppHandle, force: bool) {
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
pub fn catalog_refresh(app: AppHandle) {
    refresh_catalog(&app, false);
}

/// 今天的日期（UTC，YYYY-MM-DD），只用来标记资料版本
pub(crate) fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86400)
        .unwrap_or(0) as i64;
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
pub(crate) struct Prefs {
    shortcut: String,
    capture_shortcut: String,
    win_shortcut: Option<String>,
    share_shortcut: Option<String>,
    feed_shortcut: Option<String>,
    usd_cny: Option<f64>,
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
pub(crate) struct Screen {
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    scale: f64,
    primary: bool,
}

#[derive(Serialize)]
pub(crate) struct Screens {
    cursor: Option<(f64, f64)>,
    list: Vec<Screen>,
}

#[tauri::command]
pub fn screens(app: AppHandle) -> Screens {
    let prim = app.primary_monitor().ok().flatten().map(|m| *m.position());
    let list = app
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let (p, z) = (m.position(), m.size());
            Screen {
                x: p.x,
                y: p.y,
                w: z.width,
                h: z.height,
                scale: m.scale_factor(),
                primary: prim == Some(*p),
            }
        })
        .collect();
    Screens {
        cursor: app.cursor_position().ok().map(|c| (c.x, c.y)),
        list,
    }
}

/// 模型以外的设置。返回 Some(提示)：已经保存，但有快捷键被别的程序占用了
#[tauri::command]
pub fn save_prefs(
    app: AppHandle,
    state: State<'_, AppState>,
    prefs: Prefs,
) -> Result<Option<String>, String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.shortcut = prefs.shortcut.trim().to_string();
    next.capture_shortcut = prefs.capture_shortcut.trim().to_string();
    if let Some(v) = prefs.win_shortcut {
        next.win_shortcut = v.trim().to_string();
    }
    if let Some(v) = prefs.share_shortcut {
        next.share_shortcut = v.trim().to_string();
    }
    if let Some(v) = prefs.feed_shortcut {
        next.feed_shortcut = v.trim().to_string();
    }
    if let Some(v) = prefs
        .usd_cny
        .filter(|v| v.is_finite() && *v > 0.0 && *v <= 1000.0)
    {
        next.usd_cny = v;
    }
    if let Some(v) = prefs.new_shortcut {
        next.new_shortcut = v.trim().to_string();
    }
    if let Some(v) = prefs.tasks_shortcut {
        next.tasks_shortcut = v.trim().to_string();
    }
    next.orb_size = prefs.orb_size.clamp(56, 88);
    next.glass = prefs.glass.min(60);
    if prefs.eye.len() == 7
        && prefs.eye.starts_with('#')
        && prefs.eye[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        next.eye = prefs.eye;
    }
    if let Some(v) = prefs.idle_ms {
        next.idle_ms = v.min(24 * 3600 * 1000);
    }
    if let Some(v) = prefs.follow_system {
        next.follow_system = v;
    }
    if let Some(v) = prefs.auto_dock {
        next.auto_dock = v;
    }
    if let Some(v) = prefs.calm {
        next.calm = v;
    }
    if let Some(v) = prefs.hover {
        next.hover = v.normalized();
    }
    if let Some(v) = prefs.sel {
        next.sel = v.normalized();
    }
    if let Some(v) = prefs.voice {
        next.voice = v.normalized();
    }
    if let Some(v) = prefs.talk_shortcut {
        next.talk_shortcut = v.trim().to_string();
    }
    if let Some(v) = prefs.screen {
        if matches!(v.as_str(), "mouse" | "main" | "last") {
            next.screen = v;
        }
    }
    if let Some(v) = prefs.screen_slide {
        next.screen_slide = v;
    }
    let turned_on = prefs.catalog_auto == Some(true) && !next.catalog_auto;
    if let Some(v) = prefs.catalog_auto {
        next.catalog_auto = v;
    }
    let busy = register_shortcuts(&app, &state, &next)?;
    commit(&app, &state, next)?;
    if turned_on {
        refresh_catalog(&app, false);
    }
    Ok(busy)
}

/// 新建或修改一个 Agent，返回它的 id
#[tauri::command]
pub fn save_agent(
    app: AppHandle,
    state: State<'_, AppState>,
    agent: config::Agent,
) -> Result<String, String> {
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
pub fn save_skill(
    app: AppHandle,
    state: State<'_, AppState>,
    skill: config::Skill,
) -> Result<(), String> {
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
pub fn delete_skill(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
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
pub fn delete_agent(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut cfg = state.cfg.lock().unwrap();
    let mut next = cfg.clone();
    next.remove_agent(&id)?;
    with_store(&state, |s| s.delete_memory(&id, None))?;
    next.save(&state.cfg_dir)?;
    *cfg = next;
    drop(cfg);
    selection::wake();
    let _ = app.emit_to("main", "config", ());
    let _ = app.emit("memory:changed", id);
    Ok(())
}

#[tauri::command]
pub fn save_limits(
    app: AppHandle,
    state: State<'_, AppState>,
    limits: config::Limits,
) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.limits = config::Limits {
        tokens: limits.tokens,
        steps: limits.steps.clamp(1, 500),
        parallel: limits.parallel.clamp(1, 10),
    };
    commit(&app, &state, next)
}

/// 记忆偏好：只保存是否在记住前询问
#[tauri::command]
pub fn save_memory_prefs(
    app: AppHandle,
    state: State<'_, AppState>,
    memory_ask: bool,
) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.memory_ask = memory_ask;
    commit(&app, &state, next)
}

/// 交给终端偏好：只保存设置，不打开终端或运行自定义命令
#[tauri::command]
pub fn save_handoff_prefs(
    app: AppHandle,
    state: State<'_, AppState>,
    custom: String,
    hint: bool,
) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.handoff.custom = custom;
    next.handoff.hint = hint;
    commit(&app, &state, next)
}

/// 表单里填了 Key 就用填的；没填就用这个模型已经存的
pub(crate) fn key_for(api_key: Option<String>, id: Option<u32>) -> Option<String> {
    api_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .or_else(|| id.and_then(config::get_api_key))
}

#[tauri::command]
pub async fn test_connection(
    base_url: String,
    model: String,
    api_key: Option<String>,
    id: Option<u32>,
) -> Result<String, String> {
    let m = Model {
        base_url,
        model,
        ..Model::default()
    };
    llm::test_connection(
        &m.endpoint(),
        key_for(api_key, id).as_deref(),
        m.model.trim(),
    )
    .await
}

#[tauri::command]
pub async fn list_models(
    base_url: String,
    api_key: Option<String>,
    id: Option<u32>,
) -> Result<Vec<String>, String> {
    let m = Model {
        base_url,
        ..Model::default()
    };
    llm::list_models(&m.base(), key_for(api_key, id).as_deref()).await
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, on: bool) -> Result<bool, String> {
    let al = app.autolaunch();
    if on { al.enable() } else { al.disable() }.map_err(|e| e.to_string())?;
    let now = al.is_enabled().unwrap_or(on);
    refresh_menus(&app);
    Ok(now)
}
