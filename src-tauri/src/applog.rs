//! 程序自己的日志：写到日志目录的 floatball.log（Windows 是 %LOCALAPPDATA%\com.floatball.desktop\logs）。
//! 只记程序在做哪一步、出了什么错，不记对话内容、选中的文字和 Key。超过 1 MB 就从头重写。
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

struct FileLog;

/// 别的库可能先装了 log 的记录器，那时 log:: 宏写不到这里；所以文件单独留一份，自己也能直接写。
static FILE: Mutex<Option<File>> = Mutex::new(None);

pub fn line(level: &str, msg: &str) {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let th = std::thread::current();
    let text = format!("{ms} [{level}] [{}] {msg}\n", th.name().unwrap_or("-"));
    if let Ok(mut g) = FILE.lock() {
        if let Some(f) = g.as_mut() {
            let _ = f.write_all(text.as_bytes());
            let _ = f.flush();
        }
    }
}

impl log::Log for FileLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        line(r.level().as_str(), &r.args().to_string());
    }
    fn flush(&self) {}
}

pub fn init(dir: &Path) {
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("建不了日志目录 {}：{e}", dir.display());
        return;
    }
    let path = dir.join("floatball.log");
    eprintln!("日志：{}", path.display());
    let big = std::fs::metadata(&path)
        .map(|m| m.len() > 1 << 20)
        .unwrap_or(false);
    let f = if big {
        File::create(&path)
    } else {
        OpenOptions::new().create(true).append(true).open(&path)
    };
    let Ok(f) = f else { return };
    *FILE.lock().unwrap() = Some(f);
    let ok = log::set_logger(&FileLog).is_ok();
    if ok {
        log::set_max_level(log::LevelFilter::Info);
    }
    line(
        "INFO",
        &format!(
            "启动 {}，log 记录器 {}",
            env!("CARGO_PKG_VERSION"),
            if ok {
                "装上了"
            } else {
                "已被别的库占用"
            }
        ),
    );
    // 线程崩了也记下来（图形界面程序没有控制台，不记就看不到）
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        line(
            "ERROR",
            &format!(
                "崩溃（线程 {}）：{info}",
                std::thread::current().name().unwrap_or("-")
            ),
        );
        prev(info);
    }));
}

#[cfg(test)]
mod tests {
    #[test]
    fn writes_file() {
        let d = std::env::temp_dir().join(format!("fblog-{}", std::process::id()));
        super::init(&d);
        log::info!("测试一行");
        let s = std::fs::read_to_string(d.join("floatball.log")).unwrap();
        assert!(s.contains("启动"), "{s}");
        assert!(s.contains("测试一行"), "{s}");
    }
}
