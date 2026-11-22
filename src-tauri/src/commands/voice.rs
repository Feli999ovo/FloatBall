use crate::*;

/// 选了服务商识别但没配好时的提示
pub(crate) const VOICE_NO_PROVIDER: &str =
    "语音识别选的是服务商，但还没有添加模型：设置 → 模型里添加，或在设置 → 语音里改用系统识别";

pub(crate) const VOICE_NO_KEY: &str =
    "语音识别选的服务商还没填 API Key：设置 → 模型里补上，或在设置 → 语音里改用系统识别";

/// 按下说话时调用：系统识别在 Windows 上从这里开始听麦克风（其他情况什么都不做）
#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub async fn stt_begin(state: State<'_, AppState>) -> Result<(), String> {
    let v = state.cfg.lock().unwrap().voice.clone();
    if v.engine != "system" {
        return Ok(());
    }
    if !stt::SUPPORTED {
        return Err(stt::UNSUPPORTED.into());
    }
    tauri::async_runtime::spawn_blocking(move || stt::begin(&v.lang))
        .await
        .map_err(|e| e.to_string())?
}

/// 取消录音：系统识别不要结果
#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub async fn stt_cancel() {
    let _ = tauri::async_runtime::spawn_blocking(stt::cancel).await;
}

/// 系统识别：Windows 停下取文字；macOS 把录音写成临时文件交给本机识别
pub(crate) async fn transcribe_system(
    bytes: Vec<u8>,
    mime: &str,
    lang: String,
) -> Result<String, String> {
    if cfg!(target_os = "macos") {
        let ext = if mime.contains("mp4") || mime.contains("m4a") || mime.contains("aac") {
            "m4a"
        } else if mime.contains("wav") {
            "wav"
        } else {
            "webm"
        };
        let path =
            std::env::temp_dir().join(format!("floatball-voice-{}.{ext}", std::process::id()));
        std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
        let p2 = path.clone();
        let r = tauri::async_runtime::spawn_blocking(move || recognize_file(&p2, &lang))
            .await
            .map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&path);
        r
    } else {
        let _ = (bytes, lang);
        tauri::async_runtime::spawn_blocking(stt::end)
            .await
            .map_err(|e| e.to_string())?
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn recognize_file(p: &std::path::Path, lang: &str) -> Result<String, String> {
    stt::file(p, lang)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn recognize_file(_p: &std::path::Path, _lang: &str) -> Result<String, String> {
    Err(stt::UNSUPPORTED.into())
}

/// 语音识别：系统自带（默认）或语音设置里选的模型服务商。返回识别出的文字
#[tauri::command]
#[cfg_attr(feature = "gen-ipc", specta::specta)]
pub async fn transcribe(
    state: State<'_, AppState>,
    audio: String,
    mime: String,
) -> Result<String, String> {
    use base64::Engine;
    let engine = state.cfg.lock().unwrap().voice.engine.clone();
    if engine == "system" {
        let lang = state.cfg.lock().unwrap().voice.lang.clone();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(audio.trim())
            .map_err(|_| "录音数据不对".to_string())?;
        return transcribe_system(bytes, &mime, lang).await;
    }
    let (m, v) = {
        let c = state.cfg.lock().unwrap();
        let v = c.voice.clone();
        let m = c
            .models
            .iter()
            .find(|m| m.id == v.model)
            .or_else(|| c.models.iter().find(|m| m.id == c.default_model))
            .or(c.models.first())
            .cloned();
        (m, v)
    };
    let m = m.ok_or(VOICE_NO_PROVIDER)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(audio.trim())
        .map_err(|_| "录音数据不对".to_string())?;
    if bytes.len() > 24 * 1024 * 1024 {
        return Err("录音太长了，请分几次说".into());
    }
    let key = if m.no_key {
        None
    } else {
        Some(config::get_api_key(m.id).ok_or(VOICE_NO_KEY)?)
    };
    llm::transcribe(&m.base(), key.as_deref(), &v.stt, &v.lang, bytes, &mime).await
}
