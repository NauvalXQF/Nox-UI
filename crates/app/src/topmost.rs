use nacre_platform::wins::tracker;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicIsize, Ordering};
use windows::core::w;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows_core::BOOL;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_RAISE: u32 = WM_APP + 150;
const T_RAISE: usize = 1;

static CTL: AtomicIsize = AtomicIsize::new(0);

static RAISE_QUEUED: AtomicBool = AtomicBool::new(false);
static LAST_RAISE: AtomicI64 = AtomicI64::new(0);

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn noop() {}

fn hook(ev: u32, _hwnd: isize) {
    if ev == tracker::EV_FOREGROUND || ev == tracker::EV_MOVESIZE_END || ev == tracker::EV_MINIMIZE_END {
        let h = CTL.load(Ordering::Relaxed);
        if h != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(h as *mut _)), WM_RAISE, WPARAM(0), LPARAM(0));
            }
        }
    }
}

pub fn request_raise() {
    let h = CTL.load(Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut _)), WM_RAISE, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn is_own_window(hwnd: isize) -> bool {
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid));
    }
    pid == std::process::id()
}

pub fn init() -> windows_core::Result<()> {
    unsafe {
        let hi: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = w!("AbraTopmostCtl");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(ctl_proc), hInstance: hi, lpszClassName: class, ..Default::default() });
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("abra-topmost"), WS_POPUP, 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(hi), None)?;
        CTL.store(hwnd.0 as isize, Ordering::Relaxed);
        tracker::install(noop);
        tracker::install_fx(hook);
    }
    Ok(())
}

unsafe extern "system" fn collect(h: HWND, lp: LPARAM) -> BOOL {
    let v = &mut *(lp.0 as *mut Vec<isize>);
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    if pid == std::process::id() && IsWindowVisible(h).as_bool() && GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0 {
        v.push(h.0 as isize);
    }
    BOOL(1)
}

const BASE_LAYERS: [&str; 7] = [
    "AbraWidget",
    "NacreEdgeStrip",
    "NacreDock",
    "AbraMenuBar",
    "NacreNotchStrip",
    "AbraNotchShadow",
    "AbraNotch",
];

fn class_of(h: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(h, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

pub fn raise_all() {
    let fg = nacre_platform::wins::focus::foreground();
    if nacre_platform::wins::overlap::is_shell_flyout(fg) {
        return;
    }
    if !crate::prefs::get().above_fullscreen {
        let ov = nacre_platform::wins::overlap::evaluate(
            fg,
            std::process::id(),
            (0, 0, 0, 0),
            nacre_platform::sys::screen_size(),
        );
        if ov.fullscreen {
            return;
        }
    }
    let mut v: Vec<isize> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut v as *mut _ as isize));
    }
    let classes: Vec<String> = v.iter().map(|h| class_of(HWND(*h as *mut _))).collect();
    let hc_strips = crate::hotcorners::strip_hwnds();

    // Build the desired Z-order strictly TOP-MOST -> BOTTOM-MOST and apply with `insert_after`
    // chaining so lower layers (e.g. AbraMenuBar) never momentarily jump above higher layers
    // (e.g. AbraNotch) and trigger spurious WM_MOUSELEAVE flickers.
    let mut ordered: Vec<isize> = Vec::with_capacity(v.len() + hc_strips.len());
    let mut push_unique = |h: isize| {
        if !ordered.contains(&h) {
            ordered.push(h);
        }
    };

    // 1. Popups / transient overlays stay above everything else (preserving EnumWindows top-down order).
    for (h, c) in v.iter().zip(&classes) {
        if !BASE_LAYERS.contains(&c.as_str()) {
            push_unique(*h);
        }
    }
    // 2. Hot-corner trigger strips sit right below popups, above base shell layers.
    for h in &hc_strips {
        push_unique(*h);
    }
    // 3. Base shell layers from highest (AbraNotch) down to lowest (AbraWidget).
    for layer in BASE_LAYERS.into_iter().rev() {
        if layer == "AbraWidget" && crate::widgets::floating() {
            continue;
        }
        for (h, c) in v.iter().zip(&classes) {
            if c == layer {
                push_unique(*h);
            }
        }
    }

    let mut after = HWND_TOPMOST;
    for h in ordered {
        let hwnd = HWND(h as *mut _);
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(after),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        after = hwnd;
    }
}

unsafe extern "system" fn ctl_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_RAISE => {
            if RAISE_QUEUED.swap(true, Ordering::Relaxed) {
                LRESULT(0)
            } else {
                SetTimer(Some(hwnd), T_RAISE, 80, None);
                LRESULT(0)
            }
        }
        WM_TIMER => {
            let _ = KillTimer(Some(hwnd), wp.0);
            let now = now_ms();
            let elapsed = now - LAST_RAISE.load(Ordering::Relaxed);
            if elapsed >= 250 {
                RAISE_QUEUED.store(false, Ordering::Relaxed);
                LAST_RAISE.store(now, Ordering::Relaxed);
                raise_all();
            } else {
                let wait = (250 - elapsed).clamp(20, 250) as u32;
                SetTimer(Some(hwnd), T_RAISE, wait, None);
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
