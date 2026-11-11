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
