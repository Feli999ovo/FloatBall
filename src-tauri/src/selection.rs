//! 划词：在别的应用里选中文字后，在旁边弹出工具栏（原型的 selBar）。
//!
//! 做法：后台线程每 25ms 看一次鼠标左键、Alt、Esc 和指针位置（只读状态，不装全局钩子），
//! 认出「拖选」或「双击」后，读前台应用选中的文字：
//! - macOS：先用辅助功能接口（AXSelectedText）读，读不到再模拟 ⌘C（需要「辅助功能」权限）；
//! - Windows：模拟 Ctrl+C 读剪贴板，读完把剪贴板原来的文字/图片放回去。
//!   只在按下或松开时指针是文字光标（I 形）时才读，避免拖文件、拖窗口时误触发；
//!   终端（Ctrl+C 会中断程序）和资源管理器一律不读。
//! Linux 上暂不支持。

// Linux 上不跑监听线程，手势识别等只在测试里用到
#![cfg_attr(not(any(windows, target_os = "macos")), allow(dead_code))]

use serde::Serialize;
#[cfg(any(windows, target_os = "macos", test))]
use std::time::Duration;
use std::time::Instant;

/// 拖选：按下到松开移动超过这么多像素
const DRAG_PX: f64 = 8.0;
/// 双击：两次松开的间隔与距离
const DCLICK_MS: u128 = 500;
const DCLICK_PX: f64 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    Drag,
    DoubleClick,
}

/// 只看按下/松开的位置和时间，判断是不是一次选字的手势
#[derive(Default)]
pub struct Recognizer {
    press: Option<(f64, f64, Instant)>,
    last_click: Option<(f64, f64, Instant)>,
}

impl Recognizer {
    pub fn down(&mut self, x: f64, y: f64, t: Instant) {
        self.press = Some((x, y, t));
    }
    pub fn up(&mut self, x: f64, y: f64, t: Instant) -> Option<Gesture> {
        let (px, py, _) = self.press.take()?;
        if (x - px).hypot(y - py) >= DRAG_PX {
            self.last_click = None;
            return Some(Gesture::Drag);
        }
        if let Some((lx, ly, lt)) = self.last_click {
            if t.duration_since(lt).as_millis() <= DCLICK_MS && (x - lx).hypot(y - ly) <= DCLICK_PX {
                self.last_click = None;
                return Some(Gesture::DoubleClick);
            }
        }
        self.last_click = Some((x, y, t));
        None
    }
}

/// 这些程序里不读选中文字：终端里 Ctrl+C 会中断正在跑的程序；资源管理器里会复制文件
pub fn never_read(exe: &str) -> bool {
    const NEVER: [&str; 13] = [
        "windowsterminal", "cmd", "powershell", "pwsh", "conhost", "openconsole", "mintty", "wezterm-gui", "alacritty", "putty", "explorer", "kitty", "wt",
    ];
    let e = exe.to_lowercase();
    let e = e.rsplit(['/', '\\']).next().unwrap_or("").trim_end_matches(".exe").to_string();
    NEVER.contains(&e.as_str())
}

/// 读到的文字太长就截断，避免把整篇文档塞进工具栏
pub fn clip(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    Some(t.chars().take(20000).collect())
}

/// 给工具栏的一次选中
#[derive(Debug, Clone, Serialize)]
pub struct Picked {
    pub text: String,
    pub app: String,
    /// 松开鼠标的位置（屏幕坐标；Windows 为物理像素，macOS 为逻辑点）
    pub x: f64,
    pub y: f64,
}

#[allow(dead_code)]
pub struct Pointer {
    pub down: bool,
    pub alt: bool,
    pub esc: bool,
    pub ibeam: bool,
    pub x: f64,
    pub y: f64,
}

#[cfg(windows)]
mod plat {
    use super::Pointer;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON, VK_MENU, VK_RBUTTON};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorInfo, GetSystemMetrics, LoadCursorW, CURSORINFO, IDC_IBEAM, SM_SWAPBUTTON};

    pub const PHYSICAL: bool = true;

    pub fn pointer() -> Option<Pointer> {
        unsafe {
            let mut ci: CURSORINFO = std::mem::zeroed();
            ci.cbSize = std::mem::size_of::<CURSORINFO>() as u32;
            if GetCursorInfo(&mut ci) == 0 {
                return None;
            }
            let ibeam = !ci.hCursor.is_null() && ci.hCursor == LoadCursorW(std::ptr::null_mut(), IDC_IBEAM);
            let left = if GetSystemMetrics(SM_SWAPBUTTON) != 0 { VK_RBUTTON } else { VK_LBUTTON };
            let held = |vk: u16| (GetAsyncKeyState(vk as i32) as u16) & 0x8000 != 0;
            Some(Pointer { down: held(left), alt: held(VK_MENU), esc: held(VK_ESCAPE), ibeam, x: ci.ptScreenPos.x as f64, y: ci.ptScreenPos.y as f64 })
        }
    }

    pub fn trusted(_prompt: bool) -> bool {
        true
    }
}

#[cfg(target_os = "macos")]
mod plat {
    use super::Pointer;
    use std::ffi::c_void;

    #[repr(C)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
        fn CGEventSourceFlagsState(state: i32) -> u64;
        fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
        fn CGEventCreate(source: *const c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    pub const PHYSICAL: bool = false;
    const COMBINED: i32 = 0;
    const ALT_MASK: u64 = 0x0008_0000;
    const KEY_ESC: u16 = 53;

    pub fn pointer() -> Option<Pointer> {
        unsafe {
            let ev = CGEventCreate(std::ptr::null());
            if ev.is_null() {
                return None;
            }
            let p = CGEventGetLocation(ev);
            CFRelease(ev as *const c_void);
            Some(Pointer {
                down: CGEventSourceButtonState(COMBINED, 0),
                alt: CGEventSourceFlagsState(COMBINED) & ALT_MASK != 0,
                esc: CGEventSourceKeyState(COMBINED, KEY_ESC),
                ibeam: true,
                x: p.x,
                y: p.y,
            })
        }
    }

    /// 是否已在「辅助功能」里允许本应用
    pub fn trusted(_prompt: bool) -> bool {
        unsafe { AXIsProcessTrusted() }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod plat {
    use super::Pointer;
    pub const PHYSICAL: bool = true;
    pub fn pointer() -> Option<Pointer> {
        None
    }
    pub fn trusted(_prompt: bool) -> bool {
        true
    }
}

pub use plat::trusted;

/// 这个系统上能不能用划词
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

#[cfg(any(windows, target_os = "macos"))]
fn read_selection(own_pid: u64) -> Option<(String, String)> {
    let w = active_win_pos_rs::get_active_window().ok()?;
    if w.process_id == own_pid {
        return None;
    }
    let exe = w.process_path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    if cfg!(windows) && never_read(&exe) {
        return None;
    }
    Some((format!("{}\u{1}{}", w.app_name, exe), get_selected_text::get_selected_text().ok()?))
}

/// 后台线程：认手势、读文字、通知工具栏窗口；别处按下或按 Esc 时收起
pub fn spawn<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    #[cfg(any(windows, target_os = "macos"))]
    std::thread::spawn(move || watch(app));
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = app;
}

#[cfg(any(windows, target_os = "macos"))]
fn watch<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    use tauri::{Emitter, Manager};
    let mut rec = Recognizer::default();
    let mut was_down = false;
    let mut press_ibeam = false;
    let own_pid = std::process::id() as u64;
    loop {
        std::thread::sleep(Duration::from_millis(25));
        let Some(p) = plat::pointer() else { continue };
        let st = app.state::<crate::AppState>();
        let sel = st.cfg.lock().unwrap().sel.clone();
        let bar = app.get_webview_window("selbar");
        let shown = bar.as_ref().map(|b| b.is_visible().unwrap_or(false)).unwrap_or(false);
        if !sel.on {
            if shown {
                let _ = bar.as_ref().unwrap().hide();
            }
            was_down = p.down;
            continue;
        }
        if shown && p.esc {
            let _ = bar.as_ref().unwrap().hide();
        }
        let now = Instant::now();
        if p.down && !was_down {
            // 在工具栏上按下：交给工具栏自己；在别处按下：收起
            let on_bar = shown && bar.as_ref().map(|b| inside(b, p.x, p.y)).unwrap_or(false);
            if on_bar {
                was_down = true;
                continue;
            }
            if shown {
                let _ = bar.as_ref().unwrap().hide();
            }
            rec.down(p.x, p.y, now);
            press_ibeam = p.ibeam;
        } else if !p.down && was_down {
            if let Some(_g) = rec.up(p.x, p.y, now) {
                let want = (sel.mode != "alt" || p.alt) && (press_ibeam || p.ibeam);
                if want {
                    let app2 = app.clone();
                    let (x, y) = (p.x, p.y);
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(60));
                        let st = app2.state::<crate::AppState>();
                        if cfg!(target_os = "macos") && !plat::trusted(false) {
                            let _ = app2.emit_to("selbar", "sel:hint", Picked { text: String::new(), app: String::new(), x, y });
                            return;
                        }
                        let Some((who, text)) = read_selection(own_pid) else { return };
                        let (name, exe) = who.split_once('\u{1}').unwrap_or((&who, ""));
                        if st.cfg.lock().unwrap().sel.excluded(name, exe) {
                            return;
                        }
                        if let Some(text) = clip(&text) {
                            st.note_app(name);
                            let _ = app2.emit_to("selbar", "sel:show", Picked { text, app: name.to_string(), x, y });
                        }
                    });
                }
            }
        }
        was_down = p.down;
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn inside<R: tauri::Runtime>(w: &tauri::WebviewWindow<R>, x: f64, y: f64) -> bool {
    let (Ok(pos), Ok(size)) = (w.outer_position(), w.outer_size()) else { return false };
    let s = if plat::PHYSICAL { 1.0 } else { w.scale_factor().unwrap_or(1.0) };
    let (x, y) = (x * s, y * s);
    x >= pos.x as f64 && y >= pos.y as f64 && x < (pos.x + size.width as i32) as f64 && y < (pos.y + size.height as i32) as f64
}

/// 把工具栏放在松开点上方（放不下放下方），水平居中，夹在那块屏幕里。w、h 是工具栏的逻辑尺寸
pub fn place<R: tauri::Runtime>(w: &tauri::WebviewWindow<R>, x: f64, y: f64, bw: f64, bh: f64) {
    use tauri::{PhysicalPosition, PhysicalSize};
    // 先找到点所在的屏幕，算出它的缩放
    let mons = w.available_monitors().unwrap_or_default();
    let pick = mons.iter().find(|m| {
        let s = if plat::PHYSICAL { 1.0 } else { m.scale_factor() };
        let (p, z) = (m.position(), m.size());
        let (px, py) = (x * s, y * s);
        px >= p.x as f64 && py >= p.y as f64 && px < p.x as f64 + z.width as f64 && py < p.y as f64 + z.height as f64
    });
    let Some(m) = pick.or(mons.first()) else { return };
    let sf = m.scale_factor();
    let (px, py) = if plat::PHYSICAL { (x, y) } else { (x * sf, y * sf) };
    let (w_px, h_px) = (bw * sf, bh * sf);
    let (mp, mz) = (m.position(), m.size());
    let gap = 14.0 * sf;
    let mut top = py - h_px - gap;
    if top < mp.y as f64 + 8.0 {
        top = py + gap + 8.0 * sf;
    }
    let left = (px - w_px / 2.0).clamp(mp.x as f64 + 8.0, (mp.x as f64 + mz.width as f64 - w_px - 8.0).max(mp.x as f64 + 8.0));
    let top = top.clamp(mp.y as f64 + 8.0, (mp.y as f64 + mz.height as f64 - h_px - 8.0).max(mp.y as f64 + 8.0));
    let _ = w.set_size(PhysicalSize::new(w_px.round() as u32, h_px.round() as u32));
    let _ = w.set_position(PhysicalPosition::new(left.round() as i32, top.round() as i32));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_drag_and_double_click() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut r = Recognizer::default();
        r.down(100.0, 100.0, t0);
        assert_eq!(r.up(160.0, 102.0, ms(300)), Some(Gesture::Drag));
        // 单击不算
        r.down(10.0, 10.0, ms(1000));
        assert_eq!(r.up(11.0, 10.0, ms(1080)), None);
        // 紧接着第二下：双击
        r.down(11.0, 10.0, ms(1200));
        assert_eq!(r.up(11.0, 11.0, ms(1260)), Some(Gesture::DoubleClick));
        // 两下隔太久：不算
        r.down(10.0, 10.0, ms(3000));
        assert_eq!(r.up(10.0, 10.0, ms(3050)), None);
        r.down(10.0, 10.0, ms(3700));
        assert_eq!(r.up(10.0, 10.0, ms(3750)), None);
        // 没按下就松开（比如按下发生在工具栏上）：不算
        assert_eq!(Recognizer::default().up(0.0, 0.0, t0), None);
    }

    #[test]
    fn never_reads_terminals_and_explorer() {
        for e in ["WindowsTerminal.exe", "C:\\Windows\\System32\\cmd.exe", "pwsh.exe", "explorer.exe", "conhost.exe"] {
            assert!(never_read(e), "{e}");
        }
        for e in ["msedge.exe", "WINWORD.EXE", "Code.exe", ""] {
            assert!(!never_read(e), "{e}");
        }
    }

    #[test]
    fn clips_text() {
        assert_eq!(clip("  \n "), None);
        assert_eq!(clip(" 你好 ").as_deref(), Some("你好"));
        assert_eq!(clip(&"字".repeat(30000)).unwrap().chars().count(), 20000);
    }
}
