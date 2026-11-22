use std::ffi::c_void;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId,
    IsIconic, IsWindow, IsWindowVisible, IsZoomed, GWL_EXSTYLE, GWL_STYLE, WS_CAPTION, WS_EX_TOOLWINDOW,
    WS_EX_TRANSPARENT,
};
use windows_core::BOOL;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overlap {
    pub overlapped: bool,
    pub fullscreen: bool,
}

fn class_of(h: HWND) -> String {
    let mut b = [0u16; 128];
    let n = unsafe { GetClassNameW(h, &mut b) };
    String::from_utf16_lossy(&b[..n.max(0) as usize])
}

fn is_shell_class(c: &str) -> bool {
    matches!(
        c,
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "Windows.UI.Core.CoreWindow"
            | "SimpleWindow"
            | "XamlExplorerHostIslandWindow"
            | "NotifyIconOverflowWindow"
            | "TopLevelWindowForOverflowXamlIsland"
    )
}

pub fn is_shell_flyout(fg: isize) -> bool {
    if fg == 0 {
        return false;
    }
    let h = HWND(fg as *mut _);
    let cls = class_of(h);
    if matches!(
        cls.as_str(),
        "Windows.UI.Core.CoreWindow"
            | "XamlExplorerHostIslandWindow"
            | "NotifyIconOverflowWindow"
            | "TopLevelWindowForOverflowXamlIsland"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "MultitaskingViewFrame"
            | "ForegroundStaging"
    ) {
        return true;
    }
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(h, Some(&mut pid));
    }
    if pid == 0 {
        return false;
    }
    if let Some(path) = super::enumerate::exe_path_of_pid(pid) {
        let exe = path.rsplit('\\').next().unwrap_or("").to_ascii_lowercase();
        if matches!(
            exe.as_str(),
            "startmenuexperiencehost.exe" | "searchhost.exe" | "shellexperiencehost.exe" | "lockapp.exe" | "screencapturehost.exe"
        ) {
            return true;
        }
    }
    false
}

fn frame_rect(h: HWND) -> Option<RECT> {
    let mut r = RECT::default();
    unsafe {
        if DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut c_void, std::mem::size_of::<RECT>() as u32)
            .is_ok()
        {
            return Some(r);
        }
        GetWindowRect(h, &mut r).ok()?;
    }
    Some(r)
}

pub fn evaluate(fg: isize, own_pid: u32, dock: (i32, i32, i32, i32), screen: (i32, i32)) -> Overlap {
    let none = Overlap::default();
    let h = HWND(fg as *mut _);
    unsafe {
        if fg == 0 || !IsWindow(Some(h)).as_bool() || !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
            return none;
        }
        let mut cloaked = 0u32;
        if DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4).is_ok() && cloaked != 0 {
            return none;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(h, Some(&mut pid));
        if pid == own_pid || is_shell_class(&class_of(h)) {
            return none;
        }
        if GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return none;
        }
        let Some(r) = frame_rect(h) else { return none };
        let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
        if cx < 0 || cy < 0 || cx >= screen.0 || cy >= screen.1 {
            return none;
        }
        let covers = r.left <= 0 && r.top <= 0 && r.right >= screen.0 && r.bottom >= screen.1;
        let captioned_max = IsZoomed(h).as_bool() && (GetWindowLongW(h, GWL_STYLE) as u32 & WS_CAPTION.0) != 0;
        let fullscreen = covers && !captioned_max;
        if fullscreen || IsZoomed(h).as_bool() {
            return Overlap { overlapped: true, fullscreen };
        }
        let hit = r.bottom > dock.1 && r.top < dock.3 && r.right > dock.0 && r.left < dock.2;
        Overlap { overlapped: hit, fullscreen: false }
    }
}

struct NotchScan {
    own_pid: u32,
    fg_hwnd: isize,
    notch: (i32, i32, i32, i32),
    screen: (i32, i32),
    overlapped: bool,
}

unsafe extern "system" fn scan_notch_window(h: HWND, lp: LPARAM) -> BOOL {
    let st = &mut *(lp.0 as *mut NotchScan);
    if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
        return BOOL(1);
    }
    let cls = class_of(h);
    // If Show Desktop (Win+D) raised Progman/WorkerW above normal windows, stop scanning
    // everything behind the desktop wallpaper while still honoring any windows above it.
    if h.0 as isize == st.fg_hwnd && matches!(cls.as_str(), "Progman" | "WorkerW") {
        return BOOL(0);
    }
    let mut cloaked = 0u32;
    if DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4).is_ok() && cloaked != 0 {
        return BOOL(1);
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    if pid == st.own_pid || is_shell_class(&cls) {
        return BOOL(1);
    }
    let ex = GetWindowLongW(h, GWL_EXSTYLE) as u32;
    if (ex & WS_EX_TOOLWINDOW.0) != 0 || (ex & WS_EX_TRANSPARENT.0) != 0 {
        return BOOL(1);
    }
    let Some(r) = frame_rect(h) else {
        return BOOL(1);
    };
    if r.right - r.left <= 1 || r.bottom - r.top <= 1 {
        return BOOL(1);
    }
    let (cx, cy) = ((r.left + r.right) / 2, (r.top + r.bottom) / 2);
    if cx < 0 || cy < 0 || cx >= st.screen.0 || cy >= st.screen.1 {
        return BOOL(1);
    }
    let zoomed = IsZoomed(h).as_bool();
    let hit = r.bottom > st.notch.1 && r.top < st.notch.3 && r.right > st.notch.0 && r.left < st.notch.2;
    if zoomed || hit {
        st.overlapped = true;
        return BOOL(0);
    }
    BOOL(1)
}

/// Evaluates overlap & fullscreen state specifically for the Notch.
/// Scans visible normal windows in top-to-bottom Z-order so Smart mode consistently hides
/// whenever any visible window on the primary monitor is maximized or intersects the Notch,
/// even if the user interacts with a small floating dialog or NOX's own UI.
pub fn evaluate_notch(ext_fg: isize, own_pid: u32, notch: (i32, i32, i32, i32), screen: (i32, i32)) -> Overlap {
    let real_fg = unsafe { GetForegroundWindow() };
    let fg_for_fs = unsafe {
        let mut pid = 0u32;
        if !real_fg.0.is_null() {
            GetWindowThreadProcessId(real_fg, Some(&mut pid));
        }
        if !real_fg.0.is_null() && pid != own_pid {
            real_fg.0 as isize
        } else {
            ext_fg
        }
    };
    let fg_ov = evaluate(fg_for_fs, own_pid, notch, screen);
    if fg_ov.fullscreen {
        return Overlap { overlapped: true, fullscreen: true };
    }
    let mut scan = NotchScan {
        own_pid,
        fg_hwnd: real_fg.0 as isize,
        notch,
        screen,
        overlapped: false,
    };
    unsafe {
        let _ = EnumWindows(Some(scan_notch_window), LPARAM(&mut scan as *mut _ as isize));
    }
    Overlap { overlapped: scan.overlapped, fullscreen: false }
}
