use crate::*;

/// 交给终端：扫描本机装了的编程 CLI 和终端（只读 PATH，不运行它们）
#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub async fn handoff_scan() -> handoff::Scan {
    tauri::async_runtime::spawn_blocking(handoff::scan)
        .await
        .unwrap_or(handoff::Scan {
            tools: vec![],
            terms: vec![],
        })
}

#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub async fn handoff_pick(start: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || handoff::pick_folder(&start))
        .await
        .map_err(|e| e.to_string())?
}

/// 交给终端：写好交接说明，在终端里、项目文件夹下打开工具。
/// tool = "custom" 时运行自定义命令。返回 (说明文件, 要粘贴的那句话；能直接带上时为空)
#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
#[allow(clippy::too_many_arguments)] // Tauri 命令的参数由前端传入，不能合并
pub fn handoff_go(
    app: AppHandle,
    state: State<'_, AppState>,
    md: String,
    tool: String,
    term: String,
    dir: String,
    save: bool,
    custom: String,
) -> Result<(String, String), String> {
    let d = PathBuf::from(dir.trim());
    if !d.is_dir() {
        return Err("项目文件夹不存在".into());
    }
    if md.trim().is_empty() {
        return Err("交接说明是空的".into());
    }
    let file = handoff::write(&md, &d, save, &state.data_dir)?;
    let prompt = handoff::prompt_for(&file);
    let (line, paste) = if tool == "custom" {
        if custom.trim().is_empty() {
            return Err("自定义命令是空的".into());
        }
        (
            handoff::custom_line(&custom, &file),
            if custom.contains("{file}") {
                String::new()
            } else {
                prompt.clone()
            },
        )
    } else {
        let found = handoff::scan()
            .tools
            .into_iter()
            .find(|f| f.id == tool)
            .ok_or("没找到这个工具，重新扫描一下")?;
        let args = handoff::args_for(&tool, &prompt, &file);
        (
            handoff::line_for(&found.bin, &args),
            if found.direct {
                String::new()
            } else {
                prompt.clone()
            },
        )
    };
    handoff::open(&term, &d, &line)?;
    log::info!("交给终端：{tool} 在 {term}");
    // 记住这次的选择：最近用的文件夹排最前，最多 5 个
    let mut next = state.cfg.lock().unwrap().clone();
    let h = &mut next.handoff;
    h.tool = tool;
    h.term = term;
    h.save = save;
    h.custom = custom.trim().to_string();
    let ds = d.display().to_string();
    h.dirs.retain(|x| x != &ds);
    h.dirs.insert(0, ds);
    h.dirs.truncate(5);
    commit(&app, &state, next)?;
    Ok((file.display().to_string(), paste))
}
