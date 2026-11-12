//! 截屏提问：截鼠标所在的那块屏幕，缩到最长边 1600 像素，转成 JPEG data URL 交给前端。
//! macOS 第一次会弹「屏幕录制」权限；在 系统设置 → 隐私与安全性 → 屏幕与系统录音 里打开本应用后需重启应用。
//! Windows 不需要授权。

use base64::Engine;
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use std::io::Cursor;

pub const MAX_SIDE: u32 = 1600;

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
    encode(small)
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
