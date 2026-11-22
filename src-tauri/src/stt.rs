//! 系统自带的语音识别（免费，不用配模型）。
//! - Windows：Windows.Media.SpeechRecognition 听写。按下时开始听麦克风，松开时停下取文字。
//!   听写要打开系统的「联机语音识别」，录音可能经微软云端处理。
//! - macOS：Speech 框架（SFSpeechRecognizer），把录好的音频文件交给它，只用本机识别，不上传。
//! - 其他系统没有，提示改用服务商识别。

/// 设置里的语言 → 系统语言标签；auto 用系统默认语言
#[cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]
pub fn lang_tag(lang: &str) -> Option<&'static str> {
    match lang {
        "zh" => Some("zh-CN"),
        "en" => Some("en-US"),
        "yue" => Some("zh-HK"),
        _ => None,
    }
}

/// 这个系统有没有自带识别
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

/// 系统识别出错时给用户看的话（按 Windows 的错误码）
#[cfg_attr(not(windows), allow(dead_code))]
pub fn explain_win(code: u32, raw: &str) -> String {
    match code {
        0x8004_5509 => "Windows 的「联机语音识别」没打开：设置 → 隐私和安全性 → 语音，打开后再试；或在悬浮球设置 → 语音里改用服务商识别".into(),
        0x8007_0005 => "没有麦克风权限：Windows 设置 → 隐私和安全性 → 麦克风，打开「允许桌面应用访问麦克风」".into(),
        0x8004_503A | 0x8004_5503 => "Windows 没装这个语言的语音识别：设置 → 时间和语言 → 语言，给该语言装上「语音识别」，或在悬浮球设置 → 语音里换语言".into(),
        _ => format!("系统语音识别出错：{raw}"),
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::{Arc, Mutex};
    use windows::core::{Interface, HSTRING};
    use windows::Foundation::TypedEventHandler;
    use windows::Globalization::Language;
    use windows::Media::SpeechRecognition::*;

    struct Live {
        rec: SpeechRecognizer,
        text: Arc<Mutex<String>>,
    }
    static LIVE: Mutex<Option<Live>> = Mutex::new(None);

    fn err(e: windows::core::Error) -> String {
        super::explain_win(e.code().0 as u32, &e.message())
    }

    /// 按下：开始听麦克风
    pub fn begin(lang: &str) -> Result<(), String> {
        cancel();
        let rec = match super::lang_tag(lang) {
            Some(t) => Language::CreateLanguage(&HSTRING::from(t))
                .and_then(|l| SpeechRecognizer::Create(&l))
                .map_err(err)?,
            None => SpeechRecognizer::new().map_err(err)?,
        };
        let c = SpeechRecognitionTopicConstraint::Create(
            SpeechRecognitionScenario::Dictation,
            &HSTRING::from("dictation"),
        )
        .map_err(err)?;
        rec.Constraints()
            .and_then(|v| v.Append(&c.cast::<ISpeechRecognitionConstraint>()?))
            .map_err(err)?;
        let r = rec
            .CompileConstraintsAsync()
            .and_then(|op| op.get())
            .map_err(err)?;
        let st = r.Status().map_err(err)?;
        if st != SpeechRecognitionResultStatus::Success {
            return Err(
                if st == SpeechRecognitionResultStatus::TopicLanguageNotSupported {
                    "Windows 的语音识别不支持这个语言，在悬浮球设置 → 语音里换语言或改用服务商识别"
                        .into()
                } else if st == SpeechRecognitionResultStatus::NetworkFailure {
                    "Windows 语音识别要联网，现在连不上".into()
                } else {
                    format!("Windows 语音识别用不了（{}）", st.0)
                },
            );
        }
        let text = Arc::new(Mutex::new(String::new()));
        let t2 = text.clone();
        let sess = rec.ContinuousRecognitionSession().map_err(err)?;
        sess.ResultGenerated(&TypedEventHandler::<
            SpeechContinuousRecognitionSession,
            SpeechContinuousRecognitionResultGeneratedEventArgs,
        >::new(move |_, args| {
            if let Some(a) = args.as_ref() {
                let r = a.Result()?;
                if r.Status()? == SpeechRecognitionResultStatus::Success {
                    t2.lock().unwrap().push_str(&r.Text()?.to_string());
                }
            }
            Ok(())
        }))
        .map_err(err)?;
        sess.StartAsync().and_then(|a| a.get()).map_err(err)?;
        *LIVE.lock().unwrap() = Some(Live { rec, text });
        Ok(())
    }

    /// 松开：停下，返回听到的文字
    pub fn end() -> Result<String, String> {
        let Some(live) = LIVE.lock().unwrap().take() else {
            return Err("没在听".into());
        };
        live.rec
            .ContinuousRecognitionSession()
            .and_then(|s| s.StopAsync())
            .and_then(|a| a.get())
            .map_err(err)?;
        // 停下时最后一句的结果可能稍后才到
        std::thread::sleep(std::time::Duration::from_millis(150));
        let t = live.text.lock().unwrap().trim().to_string();
        Ok(t)
    }

    /// 取消：不要结果
    pub fn cancel() {
        if let Some(live) = LIVE.lock().unwrap().take() {
            let _ = live
                .rec
                .ContinuousRecognitionSession()
                .and_then(|s| s.CancelAsync())
                .and_then(|a| a.get());
        }
    }
}

#[cfg(target_os = "macos")]
#[allow(unused_unsafe)]
mod imp {
    use block2::RcBlock;
    use objc2::AnyThread;
    use objc2_foundation::{NSError, NSLocale, NSString, NSURL};
    use objc2_speech::*;
    use std::sync::mpsc;
    use std::time::Duration;

    pub fn begin(_lang: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn end() -> Result<String, String> {
        Err("macOS 用录音文件识别".into())
    }
    pub fn cancel() {}

    fn authorize() -> Result<(), String> {
        let denied = "语音识别权限没打开：系统设置 → 隐私与安全性 → 语音识别，允许悬浮球；或在悬浮球设置 → 语音里改用服务商识别";
        let st = unsafe { SFSpeechRecognizer::authorizationStatus() };
        if st == SFSpeechRecognizerAuthorizationStatus::Authorized {
            return Ok(());
        }
        if st != SFSpeechRecognizerAuthorizationStatus::NotDetermined {
            return Err(denied.into());
        }
        // 第一次用：弹系统授权
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |s: SFSpeechRecognizerAuthorizationStatus| {
            let _ = tx.send(s);
        });
        unsafe { SFSpeechRecognizer::requestAuthorization(&block) };
        match rx.recv_timeout(Duration::from_secs(120)) {
            Ok(s) if s == SFSpeechRecognizerAuthorizationStatus::Authorized => Ok(()),
            _ => Err(denied.into()),
        }
    }

    /// 识别一个录音文件，只用本机识别
    pub fn file(path: &std::path::Path, lang: &str) -> Result<String, String> {
        authorize()?;
        let rec = unsafe {
            match super::lang_tag(lang) {
                Some(t) => SFSpeechRecognizer::initWithLocale(
                    SFSpeechRecognizer::alloc(),
                    &NSLocale::initWithLocaleIdentifier(NSLocale::alloc(), &NSString::from_str(t)),
                ),
                None => SFSpeechRecognizer::init(SFSpeechRecognizer::alloc()),
            }
        }
        .ok_or("macOS 的语音识别不支持这个语言，在悬浮球设置 → 语音里换语言或改用服务商识别")?;
        if !unsafe { rec.supportsOnDeviceRecognition() } {
            return Err("这台 Mac 没有这个语言的本机识别：系统设置 → 键盘 → 听写，下载该语言，或在悬浮球设置 → 语音里改用服务商识别".into());
        }
        let url = unsafe { NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy())) };
        let req = unsafe {
            SFSpeechURLRecognitionRequest::initWithURL(SFSpeechURLRecognitionRequest::alloc(), &url)
        };
        unsafe {
            req.setRequiresOnDeviceRecognition(true);
            req.setShouldReportPartialResults(false);
        }
        let (tx, rx) = mpsc::channel::<Result<String, String>>();
        let block = RcBlock::new(
            move |res: *mut SFSpeechRecognitionResult, e: *mut NSError| {
                if let Some(e) = unsafe { e.as_ref() } {
                    let _ = tx.send(Err(format!("系统语音识别出错：{}", unsafe {
                        e.localizedDescription()
                    })));
                } else if let Some(r) = unsafe { res.as_ref() } {
                    if unsafe { r.isFinal() } {
                        let _ = tx.send(Ok(
                            unsafe { r.bestTranscription().formattedString() }.to_string()
                        ));
                    }
                }
            },
        );
        let _task = unsafe { rec.recognitionTaskWithRequest_resultHandler(&req, &block) };
        rx.recv_timeout(Duration::from_secs(60))
            .map_err(|_| "系统语音识别超时".to_string())?
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    pub fn begin(_lang: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn end() -> Result<String, String> {
        Err(super::UNSUPPORTED.into())
    }
    pub fn cancel() {}
}

pub const UNSUPPORTED: &str = "这个系统没有自带语音识别，请在设置 → 语音里改用服务商识别";

pub use imp::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_languages_and_errors() {
        assert_eq!(lang_tag("zh"), Some("zh-CN"));
        assert_eq!(lang_tag("yue"), Some("zh-HK"));
        assert_eq!(lang_tag("auto"), None);
        assert!(explain_win(0x8004_5509, "x").contains("联机语音识别"));
        assert!(explain_win(0x8007_0005, "x").contains("麦克风"));
        assert!(explain_win(1, "坏了").ends_with("坏了"));
    }
}
