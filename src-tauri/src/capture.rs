//! 截屏提问：截鼠标所在的那块屏幕，缩到最长边 1600 像素，转成 JPEG data URL 交给前端。
//! macOS 第一次会弹「屏幕录制」权限；在 系统设置 → 隐私与安全性 → 屏幕与系统录音 里打开本应用后需重启应用。
//! Windows 不需要授权。

use base64::Engine;
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use std::io::Cursor;

pub const MAX_SIDE: u32 = 1600;
pub const SCREEN_PERMISSION: &str = "screen_permission";
pub const WINDOW_UNAVAILABLE: &str = "这个窗口截不了，改用截屏提问";

pub fn check_permission() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGPreflightScreenCaptureAccess() -> bool;
            fn CGRequestScreenCaptureAccess() -> bool;
        }
        if unsafe { !CGPreflightScreenCaptureAccess() && !CGRequestScreenCaptureAccess() } {
            return Err(SCREEN_PERMISSION.into());
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WindowKey { pub id: u32, pub pid: u32 }

fn window_key(w: &xcap::Window) -> Option<WindowKey> {
    Some(WindowKey { id: w.id().ok()?, pid: w.pid().ok()? })
}

fn eligible(pid: u32, own: u32, title: &str, width: u32, height: u32, minimized: bool) -> bool {
    pid != own && !title.trim().is_empty() && width > 0 && height > 0 && !minimized
}

fn find_window<T>(windows: impl IntoIterator<Item = T>, key: WindowKey, id: impl Fn(&T) -> Option<WindowKey>) -> Option<T> {
    windows.into_iter().find(|w| id(w) == Some(key))
}

#[cfg(windows)]
mod foreground {
    use super::WindowKey;
    use std::sync::Mutex;
    use windows::Win32::{
        Foundation::HWND,
        UI::{Accessibility::{HWINEVENTHOOK, SetWinEventHook}, WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId, EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
        }},
    };

    static LAST: Mutex<Option<WindowKey>> = Mutex::new(None);

    fn key(hwnd: HWND) -> Option<WindowKey> {
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)); }
        (pid != 0).then_some(WindowKey { id: hwnd.0 as u32, pid })
    }

    extern "system" fn changed(_: HWINEVENTHOOK, _: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
        if let Some(key) = key(hwnd) {
            *LAST.lock().unwrap() = Some(key);
        }
    }

    pub fn watch() {
        let active = current();
        *LAST.lock().unwrap() = active;
        let hook = unsafe { SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(changed), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS) };
        if hook.0.is_null() { log::warn!("无法跟踪前台窗口"); }
    }

    pub fn current() -> Option<WindowKey> {
        let active = key(unsafe { GetForegroundWindow() })?;
        if active.pid == std::process::id() { *LAST.lock().unwrap() } else { Some(active) }
    }
}

#[cfg(windows)]
pub use foreground::watch as watch_foreground;
#[cfg(windows)]
pub use foreground::current as foreground_window;

#[cfg(not(windows))]
pub fn foreground_window() -> Option<WindowKey> {
    let windows = xcap::Window::all().ok()?;
    let own = std::process::id();
    let mut fallback = None;
    for w in windows {
        let Some(key) = window_key(&w) else { continue };
        if !eligible(key.pid, own, &w.title().unwrap_or_default(), w.width().unwrap_or(0), w.height().unwrap_or(0), w.is_minimized().unwrap_or(true)) { continue; }
        if w.is_focused().unwrap_or(false) { return Some(key); }
        if fallback.is_none() { fallback = Some(key); }
    }
    fallback
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target { Window { key: WindowKey }, Screen { id: u32 } }

#[derive(serde::Serialize)]
pub struct Source { target: Target, title: String, thumbnail: Option<String> }

pub fn sources() -> Result<Vec<Source>, String> {
    let screens = xcap::Monitor::all().map_err(|e| e.to_string())?;
    let num_screens = screens.len();
    let mut sources: Vec<_> = screens.into_iter().enumerate().filter_map(|(i, screen)| {
        let id = screen.id().ok()?;
        let title = if num_screens == 1 { "整个屏幕".into() } else { format!("整个屏幕 {}", i + 1) };
        Some(Source { target: Target::Screen { id }, title, thumbnail: None })
    }).collect();
    for window in xcap::Window::all().map_err(|e| e.to_string())? {
        let Some(key) = window_key(&window) else { continue };
        let title = window.title().unwrap_or_default();
        if !eligible(key.pid, std::process::id(), &title, window.width().unwrap_or(0), window.height().unwrap_or(0), window.is_minimized().unwrap_or(true)) { continue; }
        let thumbnail = window.capture_image().ok().and_then(|img| {
            encode(DynamicImage::ImageRgba8(img).thumbnail(112, 72)).ok()
        });
        sources.push(Source { target: Target::Window { key }, title, thumbnail });
    }
    Ok(sources)
}

pub fn capture_window(key: WindowKey) -> Result<String, String> {
    let unavailable = || WINDOW_UNAVAILABLE.to_string();
    let windows = xcap::Window::all().map_err(|_| unavailable())?;
    let window = find_window(windows, key, window_key).filter(|_| key.pid != std::process::id()).ok_or_else(unavailable)?;
    if window.is_minimized().unwrap_or(true) { return Err(unavailable()); }
    let img = window.capture_image().map_err(|_| unavailable())?;
    if img.pixels().all(|p| p.0[..3] == [0, 0, 0]) { return Err(unavailable()); }
    encode(DynamicImage::ImageRgba8(img))
}

pub fn capture_monitor(id: u32) -> Result<String, String> {
    let monitor = xcap::Monitor::all().map_err(|e| e.to_string())?.into_iter()
        .find(|m| m.id().ok() == Some(id)).ok_or("找不到共享的屏幕")?;
    let img = monitor.capture_image().map_err(|e| e.to_string())?;
    encode(DynamicImage::ImageRgba8(img))
}

pub fn capture_at(x: i32, y: i32) -> Result<String, String> {
    let monitor = xcap::Monitor::from_point(x, y)
        .or_else(|_| {
            xcap::Monitor::all()
                .map_err(|e| e.to_string())?
                .into_iter()
                .next()
                .ok_or_else(|| "没有找到显示器".to_string())
        })
        .map_err(|e| e.to_string())?;
    let img = monitor
        .capture_image()
        .map_err(|e| format!("截屏失败（macOS 请检查「屏幕录制」权限）：{e}"))?;
    encode(DynamicImage::ImageRgba8(img))
}

/// 卡片毛玻璃的底图：截下 (px, py) 所在的屏幕，取出窗口将要盖住的那一块，缩小并模糊。
/// x、y、w、h 是相对这块屏幕左上角的物理像素。只在 Windows 上用（macOS 截屏要授权，不为了好看去要权限）。
#[cfg(windows)]
pub fn backdrop_at(px: i32, py: i32, x: i32, y: i32, w: u32, h: u32) -> Result<String, String> {
    let monitor = xcap::Monitor::from_point(px, py).map_err(|e| e.to_string())?;
    let img = monitor.capture_image().map_err(|e| e.to_string())?;
    blur_region(DynamicImage::ImageRgba8(img), x, y, w, h)
}

/// 设置窗口的玻璃底：整块屏幕缩到八分之一、模糊、提饱和度。x、y、w、h 是这块屏幕的物理像素位置和大小，
/// 页面按自己窗口的位置把对应那一块露出来。
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct ScreenBackdrop { pub url: String, pub x: i32, pub y: i32, pub w: u32, pub h: u32 }

#[cfg(windows)]
pub fn screen_backdrop(px: i32, py: i32) -> Result<ScreenBackdrop, String> {
    let monitor = xcap::Monitor::from_point(px, py)
        .or_else(|_| xcap::Monitor::all().map_err(|e| e.to_string())?.into_iter().next().ok_or_else(|| "没有找到显示器".to_string()))
        .map_err(|e| e.to_string())?;
    let (x, y) = (monitor.x().map_err(|e| e.to_string())?, monitor.y().map_err(|e| e.to_string())?);
    let img = monitor.capture_image().map_err(|e| e.to_string())?;
    let (w, h) = (img.width(), img.height());
    Ok(ScreenBackdrop { url: blur_region(DynamicImage::ImageRgba8(img), 0, 0, w, h)?, x, y, w, h })
}

/// 取出一块、缩到八分之一再模糊：图很小，又快又省内存，放大回去正好是毛玻璃的糊。
#[cfg(any(windows, test))]
pub fn blur_region(img: DynamicImage, x: i32, y: i32, w: u32, h: u32) -> Result<String, String> {
    let inside = x >= 0 && y >= 0 && w >= 8 && h >= 8
        && (x as u32).checked_add(w).is_some_and(|r| r <= img.width())
        && (y as u32).checked_add(h).is_some_and(|b| b <= img.height());
    if !inside {
        return Err("窗口不完全在这块屏幕里".into());
    }
    let small = img
        .crop_imm(x as u32, y as u32, w, h)
        .resize_exact(w / 8, h / 8, FilterType::Triangle)
        .blur(2.5);
    encode(saturate(small, 1.65))
}

/// 照原型 .x-glassy 的 backdrop-filter: saturate(165%)：按 CSS 滤镜规范的饱和度矩阵调色，让透过玻璃的桌面颜色更鲜一点
#[cfg(any(windows, test))]
pub fn saturate(img: DynamicImage, s: f32) -> DynamicImage {
    let mut rgb = img.to_rgb8();
    let m = [
        [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
    ];
    for p in rgb.pixels_mut() {
        let [r, g, b] = [p[0] as f32, p[1] as f32, p[2] as f32];
        for (i, row) in m.iter().enumerate() {
            p[i] = (row[0] * r + row[1] * g + row[2] * b).round().clamp(0.0, 255.0) as u8;
        }
    }
    DynamicImage::ImageRgb8(rgb)
}

pub fn encode(img: DynamicImage) -> Result<String, String> {
    let (w, h) = (img.width(), img.height());
    let img = if w.max(h) > MAX_SIDE {
        img.resize(MAX_SIDE, MAX_SIDE, FilterType::Triangle)
    } else {
        img
    };
    let mut buf = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(img.to_rgb8())
        .write_to(&mut buf, ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(buf.into_inner())
    ))
}

/// 拖进来的文件：只读文本，最多 200 KB
pub fn read_text_file(path: &str) -> Result<(String, String), String> {
    let p = std::path::Path::new(path);
    let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let meta = std::fs::metadata(p).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        return Err(format!("「{name}」是文件夹，暂时只支持文本文件"));
    }
    if meta.len() > 200 * 1024 {
        return Err(format!("「{name}」超过 200 KB，暂时只读小文本文件"));
    }
    let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|_| format!("「{name}」不是文本文件（PDF、图片等之后支持）"))?;
    Ok((name, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_window_requires_external_visible_content() {
        assert!(eligible(3, 7, "编辑器", 800, 600, false));
        assert!(!eligible(7, 7, "悬浮球", 800, 600, false));
        assert!(!eligible(3, 7, "  ", 800, 600, false));
        assert!(!eligible(3, 7, "编辑器", 0, 600, false));
        assert!(!eligible(3, 7, "编辑器", 800, 0, false));
        assert!(!eligible(3, 7, "编辑器", 800, 600, true));
    }

    #[test]
    fn matches_foreground_by_window_and_process() {
        let target = WindowKey { id: 8, pid: 2 };
        let reused = WindowKey { id: 8, pid: 3 };
        let sibling = WindowKey { id: 6, pid: 2 };
        assert_eq!(find_window([reused, sibling, target], target, |w| Some(*w)), Some(target));
        assert_eq!(find_window([reused, sibling], target, |w| Some(*w)), None);
    }

    #[test]
    fn saturate_keeps_grey_and_boosts_color() {
        let mut img = image::RgbImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgb([128, 128, 128]));
        img.put_pixel(1, 0, image::Rgb([200, 120, 100]));
        let out = saturate(DynamicImage::ImageRgb8(img), 1.65).to_rgb8();
        assert_eq!(out.get_pixel(0, 0).0, [128, 128, 128]);
        let p = out.get_pixel(1, 0).0;
        assert!(p[0] > 200 && p[2] < 100, "{p:?}");
    }

    #[test]
    fn encode_downscales() {
        let img = DynamicImage::new_rgba8(3200, 1800);
        let url = encode(img).unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,"));
        let b = base64::engine::general_purpose::STANDARD
            .decode(url.trim_start_matches("data:image/jpeg;base64,"))
            .unwrap();
        let back = image::load_from_memory(&b).unwrap();
        assert_eq!((back.width(), back.height()), (1600, 900));
    }

    #[test]
    fn blur_region_crops_and_rejects_outside() {
        let img = DynamicImage::new_rgba8(400, 300);
        let url = blur_region(img.clone(), 80, 40, 160, 80).unwrap();
        let b = base64::engine::general_purpose::STANDARD
            .decode(url.trim_start_matches("data:image/jpeg;base64,"))
            .unwrap();
        let back = image::load_from_memory(&b).unwrap();
        assert_eq!((back.width(), back.height()), (20, 10));
        assert!(blur_region(img.clone(), 300, 40, 160, 80).is_err());
        assert!(blur_region(img.clone(), -1, 0, 160, 80).is_err());
        assert!(blur_region(img, 0, 0, 4, 4).is_err());
    }

    #[test]
    fn text_file_rules() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.md");
        std::fs::write(&f, "# 你好").unwrap();
        assert_eq!(read_text_file(f.to_str().unwrap()).unwrap(), ("a.md".into(), "# 你好".into()));
        let b = d.path().join("b.bin");
        std::fs::write(&b, [0xff, 0xfe, 0x00]).unwrap();
        assert!(read_text_file(b.to_str().unwrap()).is_err());
        assert!(read_text_file(d.path().to_str().unwrap()).is_err());
    }
}
