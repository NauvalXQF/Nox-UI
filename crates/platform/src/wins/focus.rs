use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, FlashWindow, GetAncestor, GetForegroundWindow, IsIconic, IsWindow, IsWindowVisible,
    PostMessageW, SetForegroundWindow, ShowWindow, GA_ROOTOWNER, SW_MINIMIZE, SW_RESTORE, SW_SHOW, WM_CLOSE,
};

fn h(x: isize) -> HWND {
    HWND(x as *mut _)
}

pub fn is_alive(hwnd: isize) -> bool {
    unsafe { IsWindow(Some(h(hwnd))).as_bool() }
}

pub fn title(hwnd: isize) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(h(hwnd), &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

pub fn is_minimized(hwnd: isize) -> bool {
    unsafe { IsIconic(h(hwnd)).as_bool() }
}

pub fn foreground() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

pub fn activate(target: isize, dock: isize) -> bool {
    unsafe {
        let hwnd = h(target);
        if !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }

        let mut fg = GetForegroundWindow();
        if fg.0 as isize == dock {
            if let Some(last) = super::tracker::last_focused_except(dock, is_alive) {
                fg = h(last);
            }
        }
        let is_front = fg == hwnd || GetAncestor(fg, GA_ROOTOWNER) == hwnd;
        if is_front && IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
            return IsIconic(hwnd).as_bool();
        }

        if !IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        if SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd {
            return true;
        }
        let _ = BringWindowToTop(hwnd);
        if GetForegroundWindow() == hwnd || (SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd) {
            return true;
        }
        let _ = FlashWindow(hwnd, true);
        false
    }
}

pub fn request_close(hwnd: isize) {
    unsafe {
        let _ = PostMessageW(Some(h(hwnd)), WM_CLOSE, WPARAM(0), LPARAM(0));
    }
}

pub fn force_foreground(target: isize) -> bool {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_MENU};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    unsafe {
        let hwnd = h(target);
        if !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }
        if !IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        if GetForegroundWindow() == hwnd {
            return true;
        }
        let fg = GetForegroundWindow();
        let (me, other) = (GetCurrentThreadId(), GetWindowThreadProcessId(fg, None));
        let attached = other != 0 && other != me && AttachThreadInput(me, other, true).as_bool();
        let _ = BringWindowToTop(hwnd);
        let mut ok = SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd;
        if attached {
            let _ = AttachThreadInput(me, other, false);
        }
        if !ok {
            keybd_event(VK_MENU.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
            keybd_event(VK_MENU.0 as u8, 0, KEYEVENTF_KEYUP, 0);
            let _ = BringWindowToTop(hwnd);
            ok = SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd;
        }
        if !ok {
            let _ = FlashWindow(hwnd, true);
        }
        ok
    }
}

/// Locates the window matching a notification's `aumid` / `app_name` (including minimized windows),
/// restores it if minimized, and brings it to the foreground. Falls back to launching/activating via
/// `shell:AppsFolder\<aumid>` for apps minimized to the system tray (e.g. WhatsApp, Discord, Telegram).
pub fn focus_app_by_identity(aumid: &str, app_name: &str) -> bool {
    use windows::core::BOOL;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowLongW, GetWindowThreadProcessId, GWL_EXSTYLE, WS_EX_TOOLWINDOW,
    };

    let own_pid = std::process::id();
    let aumid_lo = aumid.trim().to_ascii_lowercase();
    let family_lo = aumid_lo.split('!').next().unwrap_or("").to_string();
    let exe_hint = aumid_lo
        .rsplit(['\\', '/', '!'])
        .next()
        .filter(|s| s.ends_with(".exe"))
        .unwrap_or("")
        .to_string();
    let name_lo = app_name.trim().to_ascii_lowercase();

    struct Search {
        own_pid: u32,
        aumid_lo: String,
        family_lo: String,
        exe_hint: String,
        name_lo: String,
        best_score: u8,
        best_hwnd: isize,
    }

    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        let s = &mut *(lp.0 as *mut Search);
        if !IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() {
            return BOOL(1);
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if (ex & WS_EX_TOOLWINDOW.0) != 0 {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == s.own_pid {
            return BOOL(1);
        }
        let mut score = 0u8;
        if !s.aumid_lo.is_empty() {
            if let Some(win_aumid) = super::identity::aumid(hwnd.0 as isize) {
                let wa = win_aumid.to_ascii_lowercase();
                if wa == s.aumid_lo {
                    score = 100;
                } else if !s.family_lo.is_empty() && wa.starts_with(&s.family_lo) {
                    score = score.max(85);
                }
            }
        }
        if score < 85 {
            if let Some(path) = super::enumerate::exe_path_of_pid(pid) {
                let p_lo = path.to_ascii_lowercase();
                let file = p_lo.rsplit('\\').next().unwrap_or("");
                if !s.family_lo.is_empty() && p_lo.contains("\\windowsapps\\") && p_lo.contains(&s.family_lo) {
                    score = score.max(80);
                } else if !s.exe_hint.is_empty() && file == s.exe_hint {
                    score = score.max(75);
                } else if !s.name_lo.is_empty() {
                    let friendly = super::names::friendly(&path).to_ascii_lowercase();
                    let stem = file.strip_suffix(".exe").unwrap_or(file);
                    if friendly == s.name_lo || stem == s.name_lo {
                        score = score.max(65);
                    } else if friendly.contains(&s.name_lo) || s.name_lo.contains(&friendly) && friendly.len() >= 3 {
                        score = score.max(45);
                    }
                }
            }
        }
        if score < 40 && !s.name_lo.is_empty() {
            let t = title(hwnd.0 as isize).to_ascii_lowercase();
            if t == s.name_lo || t.ends_with(&format!(" - {}", s.name_lo)) || t.contains(&s.name_lo) {
                score = score.max(35);
            }
        }
        if score > s.best_score {
            s.best_score = score;
            s.best_hwnd = hwnd.0 as isize;
            if score >= 100 {
                return BOOL(0);
            }
        }
        BOOL(1)
    }

    let mut search = Search {
        own_pid,
        aumid_lo,
        family_lo,
        exe_hint,
        name_lo,
        best_score: 0,
        best_hwnd: 0,
    };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut search as *mut Search as isize));
    }
    if search.best_hwnd != 0 {
        return force_foreground(search.best_hwnd);
    }
    if !aumid.trim().is_empty() {
        crate::launch::open(&super::identity::shell_path(aumid.trim()));
        return true;
    }
    false
}
