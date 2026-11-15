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
    /// 单独按着右 Alt（右 ⌥）：语音的「按住说话」
    pub ralt: bool,
    pub x: f64,
    pub y: f64,
}

#[cfg(windows)]
mod plat {
    use super::Pointer;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LBUTTON, VK_LMENU, VK_LWIN, VK_MENU, VK_RBUTTON, VK_RMENU, VK_RWIN, VK_SHIFT};
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
            // AltGr 等于 Ctrl + 右 Alt：按着 Ctrl 不算
            let ralt = held(VK_RMENU) && !held(VK_LMENU) && !held(VK_CONTROL) && !held(VK_SHIFT) && !held(VK_LWIN) && !held(VK_RWIN);
            Some(Pointer { down: held(left), alt: held(VK_MENU), esc: held(VK_ESCAPE), ibeam, ralt, x: ci.ptScreenPos.x as f64, y: ci.ptScreenPos.y as f64 })
        }
    }

    pub fn trusted(_prompt: bool) -> bool {
        true
    }

    /// 只按一下 Alt 再松开，Windows 会让前台窗口进菜单模式（记事本的菜单栏被选中、下一次点击被吃掉）。
    /// 按住说话一开始就补一个没有用途的键（0xE8，系统没分配），Alt 就不算「单按」了。
    pub fn mask_alt() {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYEVENTF_KEYUP};
        unsafe {
            keybd_event(0xE8, 0, 0, 0);
            keybd_event(0xE8, 0, KEYEVENTF_KEYUP, 0);
        }
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
    const KEY_RALT: u16 = 61;
    const KEY_LALT: u16 = 58;
    /// ⌘、⌃、⇧
    const OTHER_MASK: u64 = 0x0010_0000 | 0x0004_0000 | 0x0002_0000;

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
                ralt: CGEventSourceKeyState(COMBINED, KEY_RALT) && !CGEventSourceKeyState(COMBINED, KEY_LALT) && CGEventSourceFlagsState(COMBINED) & OTHER_MASK == 0,
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
/// 读选中的文字。和 SnipDo、PopClip、Easydict 的做法一样分两步：
/// 1. 先问系统的无障碍接口（Windows 的 UI Automation，mac 的辅助功能）：不碰剪贴板，也不管光标是什么形状；
/// 2. 读不到再模拟复制（Ctrl+C / ⌘C）读剪贴板，再放回去。Windows 上只在 I 形光标下做这一步，免得在按钮、图标上乱复制。
fn read_selection(own_pid: u64, ibeam: bool) -> Option<(String, String)> {
    let w = active_win_pos_rs::get_active_window().ok()?;
    if w.process_id == own_pid {
        return None;
    }
    let exe = w.process_path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    if cfg!(windows) && never_read(&exe) {
        return None;
    }
    let who = format!("{}\u{1}{}", w.app_name, exe);
    #[cfg(windows)]
    {
        match uia::selected_text() {
            Ok(t) if !t.trim().is_empty() => {
                log::info!("前台程序 {exe}：无障碍接口读到了");
                return Some((who, t));
            }
            Ok(_) => log::info!("前台程序 {exe}：无障碍接口没有选中文字"),
            Err(e) => log::info!("前台程序 {exe}：无障碍接口读不了（{e}）"),
        }
        if !ibeam {
            return None;
        }
    }
    let _ = ibeam;
    log::info!("前台程序 {exe}，模拟复制读选中文字");
    let t = get_selected_text::get_selected_text();
    if let Err(e) = &t {
        log::warn!("模拟复制失败：{e}");
    }
    Some((who, t.ok()?))
}

/// Windows UI Automation：取有焦点的控件（找不到文字就往上找几层，网页里焦点常在子元素上）的文本选区。
#[cfg(windows)]
mod uia {
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId};

    /// 最多读这么多字（工具栏本来也会截断）
    const MAX: i32 = 20_000;

    pub fn selected_text() -> windows::core::Result<String> {
        unsafe {
            // 每次都在新开的后台线程里调用：自己初始化 COM（MTA，不占用任何窗口线程）
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let ua: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            let walker = ua.RawViewWalker()?;
            let mut el = ua.GetFocusedElement()?;
            for _ in 0..8 {
                if let Ok(tp) = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
                    let ranges = tp.GetSelection()?;
                    let mut out = String::new();
                    for i in 0..ranges.Length()? {
                        let t = ranges.GetElement(i)?.GetText(MAX)?.to_string();
                        if !out.is_empty() && !t.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(&t);
                    }
                    if !out.trim().is_empty() {
                        return Ok(out);
                    }
                }
                match walker.GetParentElement(&el) {
                    Ok(p) => el = p,
                    Err(_) => break,
                }
            }
            Ok(String::new())
        }
    }
}

/// 后台线程的句柄：划词和右 Alt 说话都关时线程停住不读键鼠，设置改了用 wake 叫醒它
static WATCHER: std::sync::OnceLock<std::thread::Thread> = std::sync::OnceLock::new();

/// 设置变了：叫醒后台线程重新看一遍要不要读键鼠
pub fn wake() {
    if let Some(t) = WATCHER.get() {
        t.unpark();
    }
}

/// 划词和按住右 Alt 说话都关时，后台线程完全不运行
pub fn idle(sel_on: bool, talk_on: bool) -> bool {
    !sel_on && !talk_on
}

/// 后台线程：认手势、读文字、通知工具栏窗口；别处按下或按 Esc 时收起
pub fn spawn<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    #[cfg(any(windows, target_os = "macos"))]
    {
        let h = std::thread::Builder::new().name("划词".into()).spawn(move || watch(app)).expect("划词线程");
        let _ = WATCHER.set(h.thread().clone());
    }
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
    let mut was_talk = false;
    let mut beat = 0u32;
    loop {
        // 每 5 秒记一次，看线程有没有卡住（下面读窗口状态要等主线程）
        beat = beat.wrapping_add(1);
        let st = app.state::<crate::AppState>();
        let (sel, talk_on) = { let c = st.cfg.lock().unwrap(); (c.sel.clone(), c.talk_shortcut == crate::config::TALK_RALT) };
        if beat % 200 == 0 {
            log::info!("划词线程在跑（划词 {}，右 Alt 说话 {talk_on}）", if sel.on { "开" } else { "关" });
        }
        if idle(sel.on, talk_on) {
            // 两样都关：收起工具栏，停住线程，不再读鼠标和键盘，直到设置改了被 wake 叫醒
            if bar_shown() {
                if let Some(b) = app.get_webview_window("selbar") {
                    bar_hide(&b);
                }
            }
            if was_talk {
                was_talk = false;
                let _ = app.emit_to("main", "voice:key", false);
            }
            was_down = false;
            std::thread::park();
            continue;
        }
        std::thread::sleep(Duration::from_millis(25));
        let Some(p) = plat::pointer() else { continue };
        // 按住说话（默认的右 Alt）：按下、松开都告诉小球，小球自己判断按够 300ms 才开始
        let talk = talk_on && p.ralt;
        if talk != was_talk {
            was_talk = talk;
            #[cfg(windows)]
            if talk {
                plat::mask_alt();
            }
            let _ = app.emit_to("main", "voice:key", talk);
        }
        let shown = bar_shown();
        let bar = if shown { app.get_webview_window("selbar") } else { None };
        let shown = shown && bar.is_some();
        if !sel.on {
            if shown {
                bar_hide(bar.as_ref().unwrap());
            }
            was_down = p.down;
            continue;
        }
        if shown && p.esc {
            bar_hide(bar.as_ref().unwrap());
        }
        let now = Instant::now();
        if p.down && !was_down {
            // 在工具栏上按下：交给工具栏自己；在别处按下：收起
            let on_bar = shown && inside(p.x, p.y, f64::from_bits(BAR_SF.load(std::sync::atomic::Ordering::Relaxed)));
            if on_bar {
                was_down = true;
                continue;
            }
            if shown {
                bar_hide(bar.as_ref().unwrap());
            }
            rec.down(p.x, p.y, now);
            press_ibeam = p.ibeam;
        } else if !p.down && was_down {
            if let Some(_g) = rec.up(p.x, p.y, now) {
                // Windows 不再要求 I 形光标才读：先走无障碍接口，光标只决定要不要退到模拟复制
                let ibeam = press_ibeam || p.ibeam;
                let want = (sel.mode != "alt" || p.alt) && (cfg!(windows) || ibeam);
                log::info!("认出划词手势，读不读：{want}");
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
                        log::info!("开始读选中文字");
                        let got = read_selection(own_pid, ibeam);
                        log::info!("读完：{}", got.as_ref().map(|(_, t)| format!("{} 字", t.chars().count())).unwrap_or_else(|| "没读到".into()));
                        let Some((who, text)) = got else { return };
                        let (name, exe) = who.split_once('\u{1}').unwrap_or((&who, ""));
                        if st.cfg.lock().unwrap().sel.excluded(name, exe) {
                            return;
                        }
                        if let Some(text) = clip(&text) {
                            st.note_app(name);
                            let r = app2.emit_to("selbar", "sel:show", Picked { text, app: name.to_string(), x, y });
                            log::info!("通知工具栏：{:?}", r.is_ok());
                        }
                    });
                }
            }
        }
        was_down = p.down;
    }
}

/// 工具栏现在是否显示、放在哪（物理像素）。后台线程每 25 毫秒要看一次：
/// 不去问窗口（那要等主线程回话，主线程一忙整个程序就跟着卡），只读这里记下的。
static BAR_SHOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 工具栏所在屏幕的缩放（f64 的位）
static BAR_SF: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0x3FF0000000000000);
static BAR_RECT: std::sync::Mutex<Option<(i32, i32, u32, u32)>> = std::sync::Mutex::new(None);

pub fn bar_shown() -> bool {
    BAR_SHOWN.load(std::sync::atomic::Ordering::Relaxed)
}
pub fn bar_rect(r: (i32, i32, u32, u32)) {
    *BAR_RECT.lock().unwrap() = Some(r);
}
pub fn bar_show<R: tauri::Runtime>(w: &tauri::WebviewWindow<R>) {
    BAR_SHOWN.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = w.show();
}
pub fn bar_hide<R: tauri::Runtime>(w: &tauri::WebviewWindow<R>) {
    BAR_SHOWN.store(false, std::sync::atomic::Ordering::Relaxed);
    let _ = w.hide();
}

#[cfg(any(windows, target_os = "macos"))]
/// 点 (x, y)（Windows 物理像素；其他系统逻辑像素，乘这块屏幕的缩放）落在工具栏上没有
fn inside(x: f64, y: f64, sf: f64) -> bool {
    let Some((l, t, w, h)) = *BAR_RECT.lock().unwrap() else { return false };
    let s = if plat::PHYSICAL { 1.0 } else { sf };
    let (x, y) = (x * s, y * s);
    x >= l as f64 && y >= t as f64 && x < (l + w as i32) as f64 && y < (t + h as i32) as f64
}

/// 把工具栏放在松开点上方（放不下放下方），水平居中，夹在那块屏幕里。w、h 是工具栏的逻辑尺寸
/// 工具栏放好的位置（物理像素）：窗口左上角和大小、所在屏幕的左上角和大小、缩放。毛玻璃底图按它截。
#[derive(Clone, Copy, Debug)]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct Placed { pub left: i32, pub top: i32, pub w: u32, pub h: u32, pub mx: i32, pub my: i32, pub mw: u32, pub mh: u32, pub sf: f64 }

pub fn place<R: tauri::Runtime>(w: &tauri::WebviewWindow<R>, x: f64, y: f64, bw: f64, bh: f64) -> Option<Placed> {
    use tauri::{PhysicalPosition, PhysicalSize};
    // 先找到点所在的屏幕，算出它的缩放
    let mons = w.available_monitors().unwrap_or_default();
    let pick = mons.iter().find(|m| {
        let s = if plat::PHYSICAL { 1.0 } else { m.scale_factor() };
        let (p, z) = (m.position(), m.size());
        let (px, py) = (x * s, y * s);
        px >= p.x as f64 && py >= p.y as f64 && px < p.x as f64 + z.width as f64 && py < p.y as f64 + z.height as f64
    });
    let m = pick.or(mons.first())?;
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
    let (l, t, ww, hh) = (left.round() as i32, top.round() as i32, w_px.round() as u32, h_px.round() as u32);
    let _ = w.set_size(PhysicalSize::new(ww, hh));
    let _ = w.set_position(PhysicalPosition::new(l, t));
    bar_rect((l, t, ww, hh));
    BAR_SF.store(sf.to_bits(), std::sync::atomic::Ordering::Relaxed);
    Some(Placed { left: l, top: t, w: ww, h: hh, mx: mp.x, my: mp.y, mw: mz.width, mh: mz.height, sf })
}

/// 底图截多大：工具栏往下多截一块（「⋯」展开的菜单会往下长），但不超出屏幕
#[cfg_attr(not(windows), allow(dead_code))]
pub fn backdrop_rect(p: &Placed, extra_down: f64) -> (i32, i32, u32, u32) {
    let x = p.left - p.mx;
    let y = p.top - p.my;
    let w = p.w.min(p.mw.saturating_sub(x.max(0) as u32));
    let want = p.h + (extra_down * p.sf).round() as u32;
    let h = want.min(p.mh.saturating_sub(y.max(0) as u32));
    (x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watcher_idles_only_when_both_off() {
        assert!(idle(false, false));
        // 只关划词：还要读右 Alt
        assert!(!idle(false, true));
        assert!(!idle(true, false));
        assert!(!idle(true, true));
        // 还没起线程时叫醒不出错
        wake();
    }

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

#[cfg(test)]
mod backdrop_tests {
    use super::*;
    #[test]
    fn backdrop_rect_clips_to_screen() {
        let p = Placed { left: 1900, top: 1000, w: 300, h: 44, mx: 1920, my: 0, mw: 1920, mh: 1080, sf: 1.0 };
        // 屏幕左边是另一块屏：x 为负，交给 blur_region 判断不完全在屏幕里
        assert_eq!(backdrop_rect(&p, 220.0).0, -20);
        let p = Placed { left: 2000, top: 900, w: 300, h: 44, mx: 1920, my: 0, mw: 1920, mh: 1080, sf: 2.0 };
        assert_eq!(backdrop_rect(&p, 220.0), (80, 900, 300, 180));
        let p = Placed { left: 2000, top: 100, w: 300, h: 44, mx: 1920, my: 0, mw: 1920, mh: 1080, sf: 1.5 };
        assert_eq!(backdrop_rect(&p, 220.0), (80, 100, 300, 44 + 330));
    }
}
