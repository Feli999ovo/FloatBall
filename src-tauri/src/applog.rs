//! 程序自己的日志：写到日志目录的 floatball.log（Windows 是 %LOCALAPPDATA%\com.floatball.desktop\logs）。
//! 只记程序在做哪一步、出了什么错，不记对话内容、选中的文字和 Key。超过 1 MB 就从头重写。
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

struct FileLog(Mutex<File>);

impl log::Log for FileLog {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }
    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let th = std::thread::current();
        let line = format!("{ms} [{}] [{}] {}\n", r.level(), th.name().unwrap_or("-"), r.args());
        if let Ok(mut f) = self.0.lock() {
            let _ = f.write_all(line.as_bytes());
            let _ = f.flush();
        }
    }
    fn flush(&self) {}
}

pub fn init(dir: &Path) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let path = dir.join("floatball.log");
    let big = std::fs::metadata(&path).map(|m| m.len() > 1 << 20).unwrap_or(false);
    let f = if big { File::create(&path) } else { OpenOptions::new().create(true).append(true).open(&path) };
    if let Ok(f) = f {
        if log::set_logger(Box::leak(Box::new(FileLog(Mutex::new(f))))).is_ok() {
            log::set_max_level(log::LevelFilter::Info);
            log::info!("启动 {}", env!("CARGO_PKG_VERSION"));
            // 线程崩了也记下来（图形界面程序没有控制台，不记就看不到）
            let prev = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                log::error!("崩溃（线程 {}）：{info}", std::thread::current().name().unwrap_or("-"));
                prev(info);
            }));
        }
    }
}
