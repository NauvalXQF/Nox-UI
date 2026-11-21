use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::core::PWSTR;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{
    KnownNotificationBindings, NotificationKinds, UserNotification,
    UserNotificationChangedEventArgs, UserNotificationChangedKind,
};
use windows::Win32::Foundation::{CloseHandle, APPMODEL_ERROR_NO_PACKAGE, HWND, RECT};
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationCondition, IUIAutomationElement,
    IUIAutomationEventHandler, IUIAutomationEventHandler_Impl, SetWinEventHook, TreeScope_Descendants,
    TreeScope_Subtree, UnhookWinEvent, HWINEVENTHOOK, UIA_ControlTypePropertyId,
    UIA_EVENT_ID, UIA_TextControlTypeId, UIA_Window_WindowOpenedEventId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetClassNameW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
    PeekMessageW, TranslateMessage, EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW, MSG, PM_REMOVE,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};
use windows_core::implement;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    Connecting = 0,
    Connected = 1,
    NeedsIdentity = 2,
    Denied = 3,
    Unsupported = 4,
    ConnectedUia = 5,
}

impl Status {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Status::Connected,
            2 => Status::NeedsIdentity,
            3 => Status::Denied,
            4 => Status::Unsupported,
            5 => Status::ConnectedUia,
            _ => Status::Connecting,
        }
    }
}

static STATUS: AtomicU8 = AtomicU8::new(Status::Connecting as u8);
static UIA_NEXT_ID: AtomicU32 = AtomicU32::new(1_000_000);

pub fn status() -> Status {
    Status::from_u8(STATUS.load(Ordering::Relaxed))
}

fn set_status(s: Status) {
    STATUS.store(s as u8, Ordering::Relaxed);
}

pub fn has_package_identity() -> bool {
    let mut len = 0u32;
    let err = unsafe { GetCurrentPackageFullName(&mut len, None) };
    err != APPMODEL_ERROR_NO_PACKAGE
}

#[derive(Clone, Debug)]
pub struct RawNotification {
    pub id: u32,
    pub app_name: String,
    pub aumid: String,
    pub title: String,
    pub body: String,
    pub timestamp_s: i64,
}

#[derive(Clone, Debug)]
pub enum Event {
    Status(Status),
    Arrived(RawNotification),
    Removed(u32),
}

pub struct Notifications {
    stop: Arc<AtomicBool>,
    pub events: Receiver<Event>,
}

impl Drop for Notifications {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn now_unix_s() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn extract_notification(u: &UserNotification) -> Option<RawNotification> {
    let id = u.Id().ok()?;
    let app_info = u.AppInfo().ok();
    let aumid = app_info
        .as_ref()
        .and_then(|a| a.AppUserModelId().ok())
        .map(|h| h.to_string())
        .unwrap_or_default();
    let app_name = app_info
        .as_ref()
        .and_then(|a| a.DisplayInfo().ok())
        .and_then(|d| d.DisplayName().ok())
        .map(|h| h.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            aumid
                .rsplit(['\\', '/', '!'])
                .next()
                .unwrap_or("Windows")
                .to_string()
        });

    let mut lines: Vec<String> = Vec::new();
    if let Ok(notif) = u.Notification() {
        if let Ok(visual) = notif.Visual() {
            if let Ok(tpl) = KnownNotificationBindings::ToastGeneric() {
                if let Ok(binding) = visual.GetBinding(&tpl) {
                    if let Ok(elems) = binding.GetTextElements() {
                        let n = elems.Size().unwrap_or(0);
                        for i in 0..n {
                            if let Ok(el) = elems.GetAt(i) {
                                if let Ok(txt) = el.Text() {
                                    let s = txt.to_string();
                                    let trimmed = s.trim();
                                    if !trimmed.is_empty() {
                                        lines.push(trimmed.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if lines.is_empty() {
        return None;
    }
    let title = lines.remove(0);
    let body = lines.join(" \u{00B7} ");
    // CreationTime is Windows FILETIME (100ns ticks since 1601-01-01).
    let timestamp_s = u
        .CreationTime()
        .ok()
        .map(|dt| (dt.UniversalTime / 10_000_000) - 11_644_473_600)
        .filter(|&ts| ts > 946_684_800)
        .unwrap_or_else(now_unix_s);

    Some(RawNotification {
        id,
        app_name,
        aumid,
        title,
        body,
        timestamp_s,
    })
}

fn emit(tx: &Sender<Event>, notify: &(dyn Fn() + Send + Sync), ev: Event) -> bool {
    if tx.send(ev).is_err() {
        return false;
    }
    notify();
    true
}

// =============================================================================
// Zero-Config UI Automation Backend (Works on unpacked .exe, no Package ID)
// =============================================================================

static PID_CACHE: Mutex<Option<HashMap<u32, bool>>> = Mutex::new(None);
static HOOK_TX: Mutex<Option<Sender<isize>>> = Mutex::new(None);

fn is_toast_host_pid(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    if let Ok(mut guard) = PID_CACHE.lock() {
        let map = guard.get_or_insert_with(HashMap::new);
        if let Some(&cached) = map.get(&pid) {
            return cached;
        }
        let ok = query_is_toast_host(pid);
        if map.len() > 256 {
            map.clear();
        }
        map.insert(pid, ok);
        ok
    } else {
        query_is_toast_host(pid)
    }
}

fn query_is_toast_host(pid: u32) -> bool {
    unsafe {
        let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return false;
        };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        if !ok || len == 0 {
            return false;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let exe = path.rsplit(['\\', '/']).next().unwrap_or("");
        exe.eq_ignore_ascii_case("ShellExperienceHost.exe")
            || exe.eq_ignore_ascii_case("ShellHost.exe")
    }
}

fn window_class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    if n <= 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if n <= 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

fn looks_like_toast_rect(hwnd: HWND) -> bool {
    let mut rc = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut rc) }.is_err() {
        return false;
    }
    let w = rc.right - rc.left;
    let h = rc.bottom - rc.top;
    // Standard Windows 10/11 toast cards are compact popups (~320-480dp wide, ~60-380dp tall).
    w >= 160 && w <= 900 && h >= 44 && h <= 640
}

fn is_chrome_noise(s: &str) -> bool {
    let l = s.trim().to_ascii_lowercase();
    l.is_empty()
        || matches!(
            l.as_str(),
            "close"
                | "tutup"
                | "dismiss"
                | "abaikan"
                | "settings"
                | "pengaturan"
                | "more options"
                | "opsi lainnya"
                | "new notification"
                | "notifikasi baru"
                | "reply"
                | "balas"
                | "send"
                | "kirim"
                | "now"
                | "sekarang"
                | "just now"
                | "baru saja"
                | "notification"
                | "notifikasi"
        )
}

fn is_toast_window_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    l.contains("notification")
        || l.contains("notifikasi")
        || l.contains("benachrichtigung")
        || l.contains("notificación")
        || l.contains("toast")
}

fn parse_toast_lines(
    win_name: &str,
    items: &[(String, String)],
) -> Option<(String, String, String)> {
    let mut has_toast_marker = is_toast_window_name(win_name);
    let mut app_id: Option<String> = None;
    let mut title_id: Option<String> = None;
    let mut body_parts: Vec<String> = Vec::new();
    let mut generic_lines: Vec<String> = Vec::new();

    for (aid, txt) in items {
        let al = aid.to_ascii_lowercase();
        if al.contains("toast")
            || al == "appnametext"
            || al == "titletext"
            || al == "messagetext"
            || al == "attributiontext"
            || al == "headertitle"
            || al == "dismissbutton"
        {
            has_toast_marker = true;
        }
        let clean = txt.trim();
        if is_chrome_noise(clean) {
            continue;
        }
        if al == "appnametext" || al == "headertitle" {
            if app_id.is_none() {
                app_id = Some(clean.to_string());
            }
            continue;
        }
        if al == "titletext" {
            if title_id.is_none() {
                title_id = Some(clean.to_string());
            }
            continue;
        }
        if al == "messagetext" || al == "attributiontext" {
            if !body_parts.iter().any(|x| x == clean) {
                body_parts.push(clean.to_string());
            }
            continue;
        }
        if !generic_lines.iter().any(|x| x == clean) {
            generic_lines.push(clean.to_string());
        }
    }

    if !has_toast_marker {
        return None;
    }

    // Prefer explicit toast AutomationId slots when present.
    if app_id.is_some() || title_id.is_some() || !body_parts.is_empty() {
        let mut all_extra = body_parts;
        for g in generic_lines {
            if app_id.as_deref() != Some(g.as_str())
                && title_id.as_deref() != Some(g.as_str())
                && !all_extra.iter().any(|x| x == &g)
            {
                all_extra.push(g);
            }
        }
        let app_name = app_id.unwrap_or_else(|| "Windows".to_string());
        let (title, body) = match title_id {
            Some(t) => (t, all_extra.join(" \u{00B7} ")),
            None if !all_extra.is_empty() => {
                let t = all_extra.remove(0);
                (t, all_extra.join(" \u{00B7} "))
            }
            None => (app_name.clone(), String::new()),
        };
        if title.is_empty() && body.is_empty() {
            return None;
        }
        return Some((app_name, title, body));
    }

    // Positional fallback across extracted Text controls.
    if generic_lines.is_empty() {
        return None;
    }
    if generic_lines.len() >= 3 {
        let app = generic_lines.remove(0);
        let title = generic_lines.remove(0);
        let body = generic_lines.join(" \u{00B7} ");
        Some((app, title, body))
    } else if generic_lines.len() == 2 {
        let a = generic_lines.remove(0);
        let b = generic_lines.remove(0);
        Some((a.clone(), a, b))
    } else {
        Some(("Windows".to_string(), generic_lines.remove(0), String::new()))
    }
}

fn extract_uia_toast(
    el: &IUIAutomationElement,
    text_cond: &IUIAutomationCondition,
    fallback_win_name: &str,
) -> Option<RawNotification> {
    let win_name = unsafe { el.CurrentName() }
        .ok()
        .map(|b| b.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback_win_name.to_string());
    let root_aid = unsafe { el.CurrentAutomationId() }
        .ok()
        .map(|b| b.to_string())
        .unwrap_or_default();

    let mut pairs: Vec<(String, String)> = Vec::new();
    if !root_aid.is_empty() {
        pairs.push((root_aid, String::new()));
    }

    let arr = unsafe { el.FindAll(TreeScope_Descendants, text_cond) }.ok()?;
    let n = unsafe { arr.Length() }.unwrap_or(0).min(32);
    for i in 0..n {
        let Ok(child) = (unsafe { arr.GetElement(i) }) else {
            continue;
        };
        let aid = unsafe { child.CurrentAutomationId() }
            .ok()
            .map(|b| b.to_string())
            .unwrap_or_default();
        let name = unsafe { child.CurrentName() }
            .ok()
            .map(|b| b.to_string())
            .unwrap_or_default();
        if !aid.is_empty() || !name.trim().is_empty() {
            pairs.push((aid, name));
        }
    }

    let (app_name, title, body) = parse_toast_lines(&win_name, &pairs)?;
    let id = UIA_NEXT_ID.fetch_add(1, Ordering::Relaxed);
    Some(RawNotification {
        id,
        app_name,
        aumid: String::new(),
        title,
        body,
        timestamp_s: now_unix_s(),
    })
}

struct DedupCache {
    recent: HashMap<String, Instant>,
}

impl DedupCache {
    fn new() -> Self {
        Self { recent: HashMap::new() }
    }

    fn accept(&mut self, app: &str, title: &str, body: &str) -> bool {
        let now = Instant::now();
        self.recent.retain(|_, t| now.duration_since(*t) < Duration::from_secs(12));
        let key = format!("{}\u{1}{}\u{1}{}", app.trim(), title.trim(), body.trim());
        if self.recent.contains_key(&key) {
            return false;
        }
        self.recent.insert(key, now);
        true
    }
}

#[implement(IUIAutomationEventHandler)]
struct ToastOpenHandler {
    hwnd_tx: Sender<isize>,
}

impl IUIAutomationEventHandler_Impl for ToastOpenHandler_Impl {
    fn HandleAutomationEvent(
        &self,
        sender: windows_core::Ref<'_, IUIAutomationElement>,
        _eventid: UIA_EVENT_ID,
    ) -> windows_core::Result<()> {
        if let Some(el) = sender.as_ref() {
            let pid = unsafe { el.CurrentProcessId() }.unwrap_or(0) as u32;
            if is_toast_host_pid(pid) {
                if let Ok(hwnd) = unsafe { el.CurrentNativeWindowHandle() } {
                    if !hwnd.0.is_null() {
                        let _ = self.hwnd_tx.send(hwnd.0 as isize);
                    }
                }
            }
        }
        Ok(())
    }
}

unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // Filter to top-level window events (`OBJID_WINDOW == 0`).
    if hwnd.0.is_null() || id_object != 0 {
        return;
    }
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if !is_toast_host_pid(pid) {
        return;
    }
    if window_class_name(hwnd) != "Windows.UI.Core.CoreWindow" {
        return;
    }
    if let Ok(guard) = HOOK_TX.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(hwnd.0 as isize);
        }
    }
}

fn run_uia_backend(tx: &Sender<Event>, notify: &(dyn Fn() + Send + Sync), flag: &AtomicBool) {
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
    let uia: IUIAutomation = match unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
        Ok(u) => u,
        Err(e) => {
            crate::log::line("notif", &format!("IUIAutomation gagal diinisialisasi: {e}"));
            set_status(Status::Unsupported);
            let _ = emit(tx, notify, Event::Status(Status::Unsupported));
            if com {
                unsafe { CoUninitialize() };
            }
            return;
        }
    };

    let Ok(text_cond) = (unsafe {
        uia.CreatePropertyCondition(
            UIA_ControlTypePropertyId,
            &VARIANT::from(UIA_TextControlTypeId.0 as i32),
        )
    }) else {
        set_status(Status::Unsupported);
        let _ = emit(tx, notify, Event::Status(Status::Unsupported));
        if com {
            unsafe { CoUninitialize() };
        }
        return;
    };

    let (hwnd_tx, hwnd_rx) = channel::<isize>();
    if let Ok(mut guard) = HOOK_TX.lock() {
        *guard = Some(hwnd_tx.clone());
    }

    let handler: IUIAutomationEventHandler = ToastOpenHandler { hwnd_tx: hwnd_tx.clone() }.into();
    let desktop = unsafe { uia.GetRootElement() }.ok();
    let uia_hooked = if let Some(ref root) = desktop {
        unsafe {
            uia.AddAutomationEventHandler(
                UIA_Window_WindowOpenedEventId,
                root,
                TreeScope_Subtree,
                None,
                &handler,
            )
            .is_ok()
        }
    } else {
        false
    };

    let win_hook_show = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_SHOW,
            EVENT_OBJECT_SHOW,
            None,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };
    let win_hook_name = unsafe {
        SetWinEventHook(
            EVENT_OBJECT_NAMECHANGE,
            EVENT_OBJECT_NAMECHANGE,
            None,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        )
    };

    set_status(Status::ConnectedUia);
    crate::log::line(
        "notif",
        &format!(
            "Zero-Config UI Automation aktif (uia_event={uia_hooked}, win_hook={})",
            !win_hook_show.is_invalid()
        ),
    );
    let _ = emit(tx, notify, Event::Status(Status::ConnectedUia));

    let mut dedup = DedupCache::new();
    let mut pending: Vec<(isize, Instant)> = Vec::new();

    while !flag.load(Ordering::Relaxed) {
        // Pump WinEventHook messages on this thread.
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        while let Ok(raw_hwnd) = hwnd_rx.try_recv() {
            if !pending.iter().any(|(h, _)| *h == raw_hwnd) {
                pending.push((raw_hwnd, Instant::now() + Duration::from_millis(75)));
            }
        }

        let now = Instant::now();
        let mut ready = Vec::new();
        pending.retain(|&(h, at)| {
            if now >= at {
                ready.push(h);
                false
            } else {
                true
            }
        });

        for raw_hwnd in ready {
            let hwnd = HWND(raw_hwnd as *mut core::ffi::c_void);
            if !looks_like_toast_rect(hwnd) {
                continue;
            }
            let title = window_title(hwnd);
            if let Ok(el) = unsafe { uia.ElementFromHandle(hwnd) } {
                if let Some(raw) = extract_uia_toast(&el, &text_cond, &title) {
                    if dedup.accept(&raw.app_name, &raw.title, &raw.body) {
                        if !emit(tx, notify, Event::Arrived(raw)) {
                            flag.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
            }
        }

        std::thread::sleep(Duration::from_millis(40));
    }

    if let Ok(mut guard) = HOOK_TX.lock() {
        *guard = None;
    }
    if !win_hook_show.is_invalid() {
        unsafe {
            let _ = UnhookWinEvent(win_hook_show);
        }
    }
    if !win_hook_name.is_invalid() {
        unsafe {
            let _ = UnhookWinEvent(win_hook_name);
        }
    }
    if uia_hooked {
        if let Some(ref root) = desktop {
            unsafe {
                let _ = uia.RemoveAutomationEventHandler(UIA_Window_WindowOpenedEventId, root, &handler);
            }
        }
    }
    if com {
        unsafe { CoUninitialize() };
    }
}

/// Attempts the WinRT `UserNotificationListener` backend (auto-upgrade when Package Identity
/// is registered and access is granted). Returns `true` if WinRT ran to completion, or `false`
/// if we should seamlessly fall back to Zero-Config UI Automation.
fn try_winrt_backend(tx: &Sender<Event>, notify: &(dyn Fn() + Send + Sync), flag: &AtomicBool) -> bool {
    if !has_package_identity() {
        return false;
    }

    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
    let Ok(listener) = UserNotificationListener::Current() else {
        if com {
            unsafe { CoUninitialize() };
        }
        return false;
    };

    let access = listener
        .RequestAccessAsync()
        .ok()
        .and_then(|op| op.join().ok());
    if access != Some(UserNotificationListenerAccessStatus::Allowed) {
        crate::log::line(
            "notif",
            "WinRT UserNotificationListener belum diizinkan — beralih otomatis ke Zero-Config UI Automation",
        );
        if com {
            unsafe { CoUninitialize() };
        }
        return false;
    }

    set_status(Status::Connected);
    crate::log::line("notif", "UserNotificationListener (WinRT) terhubung");
    let _ = emit(tx, notify, Event::Status(Status::Connected));

    let (poke_tx, poke_rx) = channel::<(bool, u32)>();
    let token = listener
        .NotificationChanged(&TypedEventHandler::<_, UserNotificationChangedEventArgs>::new(
            move |_, args| {
                if let Some(a) = args.as_ref() {
                    let id = a.UserNotificationId().unwrap_or(0);
                    let added = a.ChangeKind().ok() == Some(UserNotificationChangedKind::Added);
                    let _ = poke_tx.send((added, id));
                }
                Ok(())
            },
        ))
        .ok();

    let mut seen: HashSet<u32> = HashSet::new();
    if let Ok(list) = listener
        .GetNotificationsAsync(NotificationKinds::Toast)
        .and_then(|op| op.join())
    {
        let n = list.Size().unwrap_or(0);
        for i in 0..n {
            if let Ok(u) = list.GetAt(i) {
                if let Ok(id) = u.Id() {
                    seen.insert(id);
                }
            }
        }
    }

    while !flag.load(Ordering::Relaxed) {
        match poke_rx.recv_timeout(Duration::from_secs(2)) {
            Ok((true, id)) => {
                if !seen.insert(id) {
                    continue;
                }
                if let Ok(u) = listener.GetNotification(id) {
                    if let Some(raw) = extract_notification(&u) {
                        if !emit(tx, notify, Event::Arrived(raw)) {
                            break;
                        }
                    }
                }
            }
            Ok((false, id)) => {
                seen.remove(&id);
                if !emit(tx, notify, Event::Removed(id)) {
                    break;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if let Some(tok) = token {
        let _ = listener.RemoveNotificationChanged(tok);
    }
    if com {
        unsafe { CoUninitialize() };
    }
    true
}

pub fn spawn(notify: impl Fn() + Send + Sync + 'static) -> Notifications {
    let (tx, rx) = channel::<Event>();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();

    let _ = std::thread::Builder::new().name("abra-notif".into()).spawn(move || {
        // Optional test injection: NACRE_TEST_NOTIF="WhatsApp|Budi|Halo jadi meeting?"
        if let Ok(test) = std::env::var("NACRE_TEST_NOTIF") {
            set_status(Status::Connected);
            let _ = emit(&tx, &notify, Event::Status(Status::Connected));
            std::thread::sleep(Duration::from_secs(2));
            let parts: Vec<&str> = test.split('|').collect();
            if parts.len() >= 2 && !flag.load(Ordering::Relaxed) {
                let _ = emit(
                    &tx,
                    &notify,
                    Event::Arrived(RawNotification {
                        id: 900_001,
                        app_name: parts[0].to_string(),
                        aumid: parts.get(3).copied().unwrap_or("").to_string(),
                        title: parts[1].to_string(),
                        body: parts.get(2).copied().unwrap_or("").to_string(),
                        timestamp_s: now_unix_s(),
                    }),
                );
            }
            return;
        }

        // Auto-upgrade to WinRT when Package Identity is active and allowed;
        // otherwise default seamlessly to Zero-Config UI Automation.
        if !try_winrt_backend(&tx, &notify, &flag) && !flag.load(Ordering::Relaxed) {
            run_uia_backend(&tx, &notify, &flag);
        }
    });

    Notifications { stop, events: rx }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_toast_automation_ids_and_filters_chrome() {
        let items = vec![
            ("NormalToastView".into(), String::new()),
            ("AppNameText".into(), "WhatsApp".into()),
            ("TitleText".into(), "Budi".into()),
            ("MessageText".into(), "Halo jadi meeting jam 3?".into()),
            ("DismissButton".into(), "Close".into()),
        ];
        assert_eq!(
            parse_toast_lines("New notification", &items),
            Some((
                "WhatsApp".into(),
                "Budi".into(),
                "Halo jadi meeting jam 3?".into()
            ))
        );
    }

    #[test]
    fn ignores_non_toast_shell_windows() {
        let items = vec![
            ("QuickSettings".into(), "Wi-Fi".into()),
            ("".into(), "Bluetooth".into()),
        ];
        assert_eq!(parse_toast_lines("Quick settings", &items), None);
    }
}
