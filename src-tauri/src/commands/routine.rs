use crate::*;

/// 定时任务：新增或修改（id 为空就是新的），返回 id
#[tauri::command]
pub fn save_routine(
    app: AppHandle,
    state: State<'_, AppState>,
    routine: routine::Routine,
) -> Result<String, String> {
    let mut next = state.cfg.lock().unwrap().clone();
    let mut r = routine::clean(routine);
    if r.prompt.is_empty() {
        return Err("要写上每次让它做什么".into());
    }
    let now = chrono::Local::now();
    if r.every == "once" {
        let when = routine::once_at(&r, &chrono::Local).ok_or("请选择有效的日期和时间")?;
        if when <= now {
            return Err("这个时间已经过了".into());
        }
    }
    if let Some(old) = next
        .routines
        .iter_mut()
        .find(|x| !r.id.is_empty() && x.id == r.id)
    {
        // 改了时间不补跑之前的：从现在开始算
        if old.every != r.every
            || old.time != r.time
            || old.weekday != r.weekday
            || old.monthday != r.monthday
            || old.date != r.date
            || !old.on && r.on
        {
            r.last = now.timestamp();
        } else {
            r.last = old.last;
        }
        r.runs = old.runs.clone();
        *old = r.clone();
    } else {
        if next.routines.len() >= routine::MAX_ROUTINES {
            return Err(format!("最多 {} 个定时任务", routine::MAX_ROUTINES));
        }
        r.id = (1..)
            .map(|i| format!("rt{i}"))
            .find(|id| !next.routines.iter().any(|x| &x.id == id))
            .unwrap();
        r.last = now.timestamp(); // 新建的从下一次到点开始
        r.runs.clear();
        next.routines.push(r.clone());
    }
    commit(&app, &state, next)?;
    let _ = app.emit_to("settings", "config", ());
    Ok(r.id)
}

#[tauri::command]
pub fn delete_routine(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    next.routines.retain(|r| r.id != id);
    commit(&app, &state, next)
}

/// 各个定时任务下一次的时间（秒，本地时区算），界面显示「下次 明天 09:00」
#[tauri::command]
pub fn routine_next(state: State<'_, AppState>) -> HashMap<String, i64> {
    state
        .cfg
        .lock()
        .unwrap()
        .routines
        .iter()
        .filter(|r| r.on)
        .filter_map(|r| Some((r.id.clone(), routine::next_local(r)?)))
        .collect()
}

/// 定时任务跑完（界面报回来）：记一条运行记录
#[tauri::command]
pub fn routine_done(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    ok: bool,
    note: String,
) -> Result<(), String> {
    let mut next = state.cfg.lock().unwrap().clone();
    let Some(r) = next.routines.iter_mut().find(|r| r.id == id) else {
        return Ok(());
    };
    r.runs.insert(
        0,
        routine::Run {
            at: chrono::Local::now().timestamp(),
            ok,
            note: note.chars().take(60).collect(),
        },
    );
    r.runs.truncate(5);
    commit(&app, &state, next)?;
    let _ = app.emit_to("settings", "config", ());
    Ok(())
}

/// 每 30 秒看一次哪些定时任务到点了：记下这次的时间（不会重复），通知小球窗口去跑
pub(crate) fn spawn_routines(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            let st = app.state::<AppState>();
            let now = chrono::Local::now();
            let mut next = st.cfg.lock().unwrap().clone();
            let fire: Vec<routine::Routine> = next
                .routines
                .iter_mut()
                .filter_map(|r| routine::mark_due(r, &now).then(|| r.clone()))
                .collect();
            if fire.is_empty() {
                continue;
            }
            if let Err(e) = commit(&app, &st, next) {
                log::warn!("定时任务保存失败：{e}");
                continue;
            }
            for r in fire {
                log::info!("定时任务到点：{}", r.name);
                let _ = app.emit_to("main", "routine", &r);
            }
        }
    });
}
