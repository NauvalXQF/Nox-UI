use crate::{paths, single_instance};
use nacre_platform::shell::taskbar;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TRAY: u32 = WM_APP + 1;
const WM_POWERBROADCAST: u32 = 0x0218;
const PBT_APMRESUMESUSPEND: usize = 0x0007;
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;
const ID_TOGGLE_TASKBAR: usize = 1;
pub const ID_QUIT: usize = 2;
const ID_SETTINGS: usize = 3;
const TIMER_EXIT: usize = 1;
const TIMER_TRAY: usize = 3;
const TIMER_DISPLAY: usize = 4;
const TIMER_FAKE_MON: usize = 5;
const TIMER_GPU: usize = 6;
const TRAY_TRIES: u32 = 30;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static MAIN: AtomicIsize = AtomicIsize::new(0);
static TRAY_LEFT: AtomicU32 = AtomicU32::new(TRAY_TRIES);
static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
static FAKE_SPEC: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

thread_local! {
    static COMP: std::cell::RefCell<Option<windows::UI::Composition::Compositor>> = const { std::cell::RefCell::new(None) };
}

pub fn set_compositor(c: &windows::UI::Composition::Compositor) {
    COMP.with(|s| *s.borrow_mut() = Some(c.clone()));
}

fn fake_spec() -> Option<String> {
    std::env::var_os("NACRE_FAKE_MONITORS_FILE")
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
}

pub fn watch_displays(hwnd: HWND) {
    let _ = STARTED.set(std::time::Instant::now());
    unsafe {
        SetTimer(Some(hwnd), TIMER_GPU, 15_000, None);
    }
    if let Some(s) = fake_spec() {
        if let Ok(mut g) = FAKE_SPEC.lock() {
            *g = s;
        }
        unsafe {
            SetTimer(Some(hwnd), TIMER_FAKE_MON, 1000, None);
        }
    }
}

fn displays_changed() {
    let list = nacre_platform::monitors::list();
    let desc: Vec<String> = list
        .iter()
        .map(|m| format!("{}x{}@({},{}) dpi={}{}", m.area.w, m.area.h, m.area.x, m.area.y, m.dpi, if m.primary { " utama" } else { "" }))
        .collect();
    nacre_platform::log::line("layar", &format!("{} layar: {}", list.len(), desc.join(", ")));
    crate::services::refresh_brightness();
    crate::prefs::broadcast();
}

fn open_settings() {
    if let Some(c) = COMP.with(|s| s.borrow().clone()) {
        crate::settings_win::open(&c);
    }
}

pub fn request_quit() {
    let h = MAIN.load(Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut _)), WM_COMMAND, WPARAM(ID_QUIT), LPARAM(0));
        }
    }
}

pub fn relaunch() {
    let Ok(exe) = std::env::current_exe() else { return };
    let pid = std::process::id();
    let _ = std::thread::Builder::new().name("nox-relaunch".into()).spawn(move || {
        use std::os::windows::process::CommandExt;
        let script = format!(
            "Wait-Process -Id {pid} -Timeout 8 -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 250; Start-Process -FilePath $env:NOX_RELAUNCH_EXE"
        );
        let _ = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script])
            .env("NOX_RELAUNCH_EXE", &exe)
            .creation_flags(0x0800_0000)
            .spawn();
    });
    let h = MAIN.load(Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut _)), WM_COMMAND, WPARAM(ID_QUIT), LPARAM(0));
        }
    }
}

pub fn create() -> windows::core::Result<HWND> {
    unsafe {
        let hi = GetModuleHandleW(None)?.into();
        let class = w!("NacreMain");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(proc), hInstance: hi, lpszClassName: class, ..Default::default() });
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
        CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("NOX"),
            WS_POPUP,
            0, 0, 0, 0,
            None, None, Some(hi), None,
        )
    }
}

pub fn set_exit_timer(hwnd: HWND, seconds: u32) {
    unsafe {
        SetTimer(Some(hwnd), TIMER_EXIT, seconds * 1000, None);
    }
}

fn tray_icon() -> HICON {
    unsafe {
        let hi = GetModuleHandleW(None).map(|h| HINSTANCE(h.0)).ok();
        let (cx, cy) = (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON));
        match LoadImageW(hi, PCWSTR(1 as *const u16), IMAGE_ICON, cx, cy, LR_SHARED) {
            Ok(h) if !h.is_invalid() => HICON(h.0),
            _ => LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
        }
    }
}

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: tray_icon(),
        ..Default::default()
    };
    for (i, c) in "NOX".encode_utf16().enumerate() {
        nid.szTip[i] = c;
    }
    nid
}

pub fn add_tray(hwnd: HWND) {
    MAIN.store(hwnd.0 as isize, Ordering::Relaxed);
    let nid = tray_data(hwnd);
    let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &nid).as_bool() || Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool() };
    if ok {
        TRAY_LEFT.store(TRAY_TRIES, Ordering::Relaxed);
        return;
    }
    let left = TRAY_LEFT.load(Ordering::Relaxed);
    nacre_platform::log::line("tray", &format!("ikon tray belum bisa dipasang, sisa coba {left}"));
    if left > 0 {
        TRAY_LEFT.store(left - 1, Ordering::Relaxed);
        unsafe {
            SetTimer(Some(hwnd), TIMER_TRAY, 2000, None);
        }
    }
}

fn remove_tray(hwnd: HWND) {
    let nid = tray_data(hwnd);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

pub static TAKEOVER_ALLOWED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LAST_AUTOSTART: std::sync::atomic::AtomicI8 = std::sync::atomic::AtomicI8::new(-1);

pub fn apply_autostart() {
    let want = crate::prefs::get().autostart;
    let code = if want { 1i8 } else { 0i8 };
    if LAST_AUTOSTART.swap(code, Ordering::Relaxed) == code {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let _ = std::thread::Builder::new().name("nox-autostart".into()).spawn(move || {
        use nacre_platform::startup;
        let cur = startup::current();
        let ok = match (want, &cur) {
            (true, Some(c)) if *c == startup::command_for(&exe) && startup::run_value_absent() => return,
            (false, None) => return,
            _ => startup::set(want, &exe),
        };
        nacre_platform::log::line("startup", &format!("autostart={want} hasil={ok:?}"));
    });
}

pub fn apply_desktop_icons() {
    nacre_platform::desktopicons::set_visible(!crate::prefs::get().hide_desktop_icons);
}

pub fn set_taskbar_hidden(on: bool) {
    if on {
        if let Err(e) = taskbar::hide(&paths::marker(), &paths::guard_exe()) {
            nacre_platform::log::line("taskbar", &format!("hide gagal: {e}"));
        }
    } else {
        taskbar::restore();
    }
    crate::dock::request_appbar_resync();
}

fn show_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let check = if taskbar::is_hidden() { MF_CHECKED } else { MF_UNCHECKED };
        let tr = |s: &'static str| windows::core::HSTRING::from(crate::prefs::tr(s));
        let _ = AppendMenuW(menu, MF_STRING | MF_DEFAULT, ID_SETTINGS, &tr("Pengaturan NOX…"));
        let _ = AppendMenuW(menu, MF_STRING | check, ID_TOGGLE_TASKBAR, &tr("Sembunyikan taskbar Windows"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, &tr("Keluar dari NOX"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, Some(0), hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let tc = TASKBAR_CREATED.load(Ordering::Relaxed);
    if tc != 0 && msg == tc {
        add_tray(hwnd);
        nacre_platform::shell::appbar::invalidate_all();
        taskbar::reassert();
        crate::dock::request_appbar_resync();
        crate::menubar::request_resync();
        crate::notch::request_reconfigure();
        nacre_platform::log::line("shell", "TaskbarCreated");
        return LRESULT(0);
    }
    match msg {
        WM_POWERBROADCAST => {
            if wp.0 == PBT_APMRESUMEAUTOMATIC || wp.0 == PBT_APMRESUMESUSPEND {
                nacre_platform::log::line("power", "bangun dari tidur; menyinkronkan ulang shell & hook");
                taskbar::reassert();
                nacre_platform::wins::tracker::reinstall();
                crate::mediakeys::reinstall();
                nacre_platform::shell::appbar::invalidate_all();
                crate::dock::request_reconfigure();
                crate::menubar::request_resync();
                crate::notch::request_reconfigure();
                crate::topmost::request_raise();
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        crate::prefs::WM_PREFS => {
            apply_autostart();
            apply_desktop_icons();
            if TAKEOVER_ALLOWED.load(Ordering::Relaxed) {
                let p = crate::prefs::get();
                let want = p.takeover && p.dock_enabled;
                if want != taskbar::is_hidden() {
                    nacre_platform::log::line("taskbar", &format!("pengaturan mode penuh → {want}"));
                    set_taskbar_hidden(want);
                }
            }
            LRESULT(0)
        }
        WM_TRAY => {
            match lp.0 as u32 & 0xFFFF {
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                WM_LBUTTONUP => open_settings(),
                _ => {}
            }
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            SetTimer(Some(hwnd), TIMER_DISPLAY, 800, None);
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_DISPLAY => {
            let _ = KillTimer(Some(hwnd), TIMER_DISPLAY);
            displays_changed();
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_FAKE_MON => {
            if let (Some(now), Ok(mut g)) = (fake_spec(), FAKE_SPEC.lock()) {
                if *g != now {
                    *g = now;
                    drop(g);
                    displays_changed();
                }
            }
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_TRAY => {
            let _ = KillTimer(Some(hwnd), TIMER_TRAY);
            add_tray(hwnd);
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_GPU => {
            let fake = std::env::var("NACRE_TEST_GPU_LOST")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .is_some_and(|s| STARTED.get().is_some_and(|t| t.elapsed().as_secs() >= s));
            if fake || nacre_ui::gfx::Gfx::shared_lost() {
                let _ = KillTimer(Some(hwnd), TIMER_GPU);
                let mark = paths::config_dir().join("gpu_relaunch.txt");
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let last = std::fs::read_to_string(&mark).ok().and_then(|t| t.trim().parse::<u64>().ok()).unwrap_or(0);
                if now.saturating_sub(last) < 600 {
                    nacre_platform::log::line("gpu", "perangkat grafis hilang lagi, tidak dibuka ulang (baru saja dibuka ulang)");
                } else {
                    let _ = std::fs::write(&mark, now.to_string());
                    nacre_platform::log::line("gpu", "perangkat grafis hilang atau diganti, NOX dibuka ulang");
                    relaunch();
                }
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match wp.0 & 0xFFFF {
                ID_SETTINGS => open_settings(),
                ID_TOGGLE_TASKBAR => {
                    if TAKEOVER_ALLOWED.load(Ordering::Relaxed) {
                        let hide = !taskbar::is_hidden();
                        crate::prefs::update(|s| s.takeover = hide);
                    } else {
                        set_taskbar_hidden(!taskbar::is_hidden());
                    }
                }
                ID_QUIT => {
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        single_instance::WM_WAKE => {
            nacre_platform::log::line("instance", "peluncuran kedua terdeteksi");
            open_settings();
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_EXIT => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_ENDSESSION => {
            if wp.0 != 0 {
                nacre_platform::desktopicons::set_visible(true);
                nacre_platform::brightness::restore();
                crate::fx::restore_all();
                nacre_platform::shell::appbar::release();
                taskbar::restore();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            crate::fx::restore_all();
            nacre_platform::desktopicons::set_visible(true);
            taskbar::restore();
            remove_tray(hwnd);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
