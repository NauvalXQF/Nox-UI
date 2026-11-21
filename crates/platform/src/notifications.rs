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
use windows::Win32::Foundation::{CloseHandle, APPMODEL_ERROR_NO_PACKAGE, HWND, LPARAM, POINT, RECT};
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
    IUIAutomationEventHandler, IUIAutomationEventHandler_Impl, IUIAutomationInvokePattern,
    IUIAutomationSelectionItemPattern, SetWinEventHook, TreeScope_Descendants, TreeScope_Subtree,
    UnhookWinEvent, HWINEVENTHOOK, UIA_ButtonControlTypeId, UIA_ControlTypePropertyId,
    UIA_DataItemControlTypeId, UIA_EVENT_ID, UIA_InvokePatternId, UIA_ListItemControlTypeId,
    UIA_SelectionItemPatternId, UIA_TextControlTypeId, UIA_TreeItemControlTypeId,
    UIA_Window_WindowOpenedEventId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEINPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetClassNameW, GetCursorPos, GetForegroundWindow, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, PeekMessageW, SetCursorPos,
    TranslateMessage, EVENT_OBJECT_NAMECHANGE, EVENT_OBJECT_SHOW, MSG, PM_REMOVE,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};
use windows_core::{implement, BOOL};

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
    Seed(Vec<RawNotification>),
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
    // Allow high-DPI (175%-250%) and Win11 tall/reused ShellExperienceHost toast windows;
    // parse_toast_nodes strictly verifies toast AutomationIds/markers so non-toast shells are rejected.
    w >= 120 && w <= 1600 && h >= 40 && h <= 1600
}

fn is_toast_host_class(cls: &str) -> bool {
    cls == "Windows.UI.Core.CoreWindow"
        || cls == "XamlExplorerHostIslandWindow"
        || cls == "ApplicationFrameWindow"
}

fn is_chrome_noise(s: &str) -> bool {
    let l = s.trim().to_ascii_lowercase();
    l.is_empty()
        || matches!(
            l.as_str(),
            "close"
                | "tutup"
                | "dismiss"
                | "dismiss notification"
                | "abaikan"
                | "abaikan notifikasi"
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
                | "mark as read"
                | "tandai sudah dibaca"
                | "mute"
                | "bisukan"
                | "open"
                | "buka"
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

/// Extracts the sender application name from Windows 11's accessibility toast title
/// (e.g. `"New notification from WhatsApp, Budi, Halo"` -> `"WhatsApp"`).
fn extract_app_from_win_name(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    for marker in [
        "new notification from ",
        "notification from ",
        "notifikasi baru dari ",
        "notifikasi dari ",
    ] {
        if let Some(pos) = lower.find(marker) {
            let after = name[pos + marker.len()..].trim();
            let app = after
                .split([',', '\n', '\r', ':', '·'])
                .next()
                .unwrap_or("")
                .trim();
            if !app.is_empty() && !is_chrome_noise(app) {
                return Some(app.to_string());
            }
        }
    }
    None
}

/// Parses `(automation_id, name, is_text)` tuples extracted from a Windows toast UIA tree.
fn parse_toast_nodes(
    win_name: &str,
    nodes: &[(String, String, bool)],
) -> Option<(String, String, String)> {
    let mut has_toast_marker = is_toast_window_name(win_name);
    let mut app_id: Option<String> = extract_app_from_win_name(win_name);
    let mut title_id: Option<String> = None;
    let mut body_parts: Vec<String> = Vec::new();
    let mut generic_texts: Vec<String> = Vec::new();

    for (aid, txt, is_text) in nodes {
        let al = aid.to_ascii_lowercase();
        if al.contains("toast")
            || al == "appnametext"
            || al == "titletext"
            || al == "messagetext"
            || al == "attributiontext"
            || al == "headertitle"
            || al == "headerbutton"
            || al == "applogo"
            || al == "dismissbutton"
        {
            has_toast_marker = true;
        }
        let clean = txt.trim();
        if clean.is_empty() {
            continue;
        }
        if app_id.is_none() {
            if let Some(from_name) = extract_app_from_win_name(clean) {
                app_id = Some(from_name);
                has_toast_marker = true;
                continue;
            }
        }
        if is_chrome_noise(clean) {
            continue;
        }
        if al == "appnametext" || al == "headertitle" || al == "headerbutton" || al == "applogo" {
            if app_id.is_none() {
                // Strip leading "Open "/"Buka " on header buttons if present.
                let stripped = clean
                    .strip_prefix("Open ")
                    .or_else(|| clean.strip_prefix("Buka "))
                    .unwrap_or(clean)
                    .trim();
                if !stripped.is_empty() && !is_chrome_noise(stripped) {
                    app_id = Some(stripped.to_string());
                }
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
        // Only collect generic lines from Text controls so action buttons don't pollute body text.
        if *is_text && !generic_texts.iter().any(|x| x == clean) {
            generic_texts.push(clean.to_string());
        }
    }

    if !has_toast_marker {
        return None;
    }

    // Prefer explicit toast AutomationId slots when present.
    if app_id.is_some() || title_id.is_some() || !body_parts.is_empty() {
        let mut all_extra = body_parts;
        for g in generic_texts {
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
    if generic_texts.is_empty() {
        return None;
    }
    if generic_texts.len() >= 3 {
        let app = generic_texts.remove(0);
        let title = generic_texts.remove(0);
        let body = generic_texts.join(" \u{00B7} ");
        Some((app, title, body))
    } else if generic_texts.len() == 2 {
        let a = generic_texts.remove(0);
        let b = generic_texts.remove(0);
        Some((a.clone(), a, b))
    } else {
        Some(("Windows".to_string(), generic_texts.remove(0), String::new()))
    }
}

fn extract_uia_toast(
    el: &IUIAutomationElement,
    all_cond: &IUIAutomationCondition,
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

    let mut nodes: Vec<(String, String, bool)> = Vec::new();
    if !root_aid.is_empty() {
        nodes.push((root_aid, win_name.clone(), false));
    }

    let arr = unsafe { el.FindAll(TreeScope_Descendants, all_cond) }.ok()?;
    let n = unsafe { arr.Length() }.unwrap_or(0).min(96);
    for i in 0..n {
        let Ok(child) = (unsafe { arr.GetElement(i) }) else {
            continue;
        };
        let ctype = unsafe { child.CurrentControlType() }.ok();
        let is_text = ctype == Some(UIA_TextControlTypeId);
        let aid = unsafe { child.CurrentAutomationId() }
            .ok()
            .map(|b| b.to_string())
            .unwrap_or_default();
        let name = unsafe { child.CurrentName() }
            .ok()
            .map(|b| b.to_string())
            .unwrap_or_default();
        if !aid.is_empty() || !name.trim().is_empty() {
            nodes.push((aid, name, is_text));
        }
    }

    let (mut app_name, title, body) = parse_toast_nodes(&win_name, &nodes)?;

    // Resolve AUMID (and refine app_name if it fell back to "Windows") from installed AppsFolder cache.
    let mut aumid = String::new();
    if let Some(app) = crate::apps::find_by_name(&app_name) {
        app_name = app.name;
        if let Some(id) = app.path.strip_prefix("shell:AppsFolder\\") {
            aumid = id.to_string();
        }
    } else if app_name.eq_ignore_ascii_case("windows") {
        // Check if any non-text node or title starts with an installed app name (e.g. "WhatsApp").
        for (_, candidate, _) in &nodes {
            let c = candidate.trim();
            if c.len() >= 3 && c.len() <= 40 && !is_chrome_noise(c) {
                if let Some(app) = crate::apps::find_by_name(c) {
                    app_name = app.name;
                    if let Some(id) = app.path.strip_prefix("shell:AppsFolder\\") {
                        aumid = id.to_string();
                    }
                    break;
                }
            }
        }
    }

    crate::log::line(
        "notif",
        &format!("uia app={app_name} aumid={aumid} title={title}"),
    );
    let id = UIA_NEXT_ID.fetch_add(1, Ordering::Relaxed);
    Some(RawNotification {
        id,
        app_name,
        aumid,
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
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    // Accept both top-level and child XAML events (`id_object != 0`), because Windows 11 reuses
    // ShellExperienceHost's CoreWindow for consecutive toasts and updates child NormalToastView nodes.
    if hwnd.0.is_null() {
        return;
    }
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if !is_toast_host_pid(pid) {
        return;
    }
    let cls = window_class_name(hwnd);
    if !is_toast_host_class(&cls) {
        return;
    }
    if let Ok(guard) = HOOK_TX.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.send(hwnd.0 as isize);
        }
    }
}

fn collect_visible_toast_hosts() -> Vec<isize> {
    let mut out: Vec<isize> = Vec::new();
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let cls = window_class_name(hwnd);
        let is_host = is_toast_host_pid(pid) && is_toast_host_class(&cls);
        let is_named_toast = !is_host && is_toast_host_class(&cls) && is_toast_window_name(&window_title(hwnd));
        if (is_host || is_named_toast) && looks_like_toast_rect(hwnd) {
            let list = &mut *(lp.0 as *mut Vec<isize>);
            list.push(hwnd.0 as isize);
        }
        BOOL(1)
    }
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut out as *mut Vec<isize> as isize));
    }
    out
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

    let Ok(all_cond) = (unsafe { uia.CreateTrueCondition() }) else {
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
    let mut last_sweep = Instant::now();
    let mut active_keys: HashSet<String> = HashSet::new();

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
        let do_sweep = now.duration_since(last_sweep) >= Duration::from_millis(400);
        if do_sweep {
            last_sweep = now;
        }

        let mut ready = Vec::new();
        pending.retain(|&(h, at)| {
            if now >= at {
                ready.push(h);
                false
            } else {
                true
            }
        });

        let mut seen_this_sweep: HashSet<String> = HashSet::new();
        if do_sweep {
            for h in collect_visible_toast_hosts() {
                if !ready.contains(&h) {
                    ready.push(h);
                }
            }
        }

        for raw_hwnd in ready {
            let hwnd = HWND(raw_hwnd as *mut core::ffi::c_void);
            if !unsafe { IsWindowVisible(hwnd).as_bool() } || !looks_like_toast_rect(hwnd) {
                continue;
            }
            let title = window_title(hwnd);
            if let Ok(el) = unsafe { uia.ElementFromHandle(hwnd) } {
                if let Some(raw) = extract_uia_toast(&el, &all_cond, &title) {
                    let key = format!("{}\u{1}{}\u{1}{}", raw.app_name.trim(), raw.title.trim(), raw.body.trim());
                    seen_this_sweep.insert(key.clone());
                    if active_keys.contains(&key) {
                        continue;
                    }
                    if dedup.accept(&raw.app_name, &raw.title, &raw.body) {
                        active_keys.insert(key);
                        if !emit(tx, notify, Event::Arrived(raw)) {
                            flag.store(true, Ordering::Relaxed);
                            break;
                        }
                    }
                }
            }
        }
        if do_sweep {
            active_keys.retain(|k| seen_this_sweep.contains(k));
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
    let mut initial: Vec<RawNotification> = Vec::new();
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
                if let Some(raw) = extract_notification(&u) {
                    initial.push(raw);
                }
            }
        }
    }
    if !initial.is_empty() {
        initial.sort_by_key(|n| n.timestamp_s);
        let keep_from = initial.len().saturating_sub(12);
        let seeded = initial.split_off(keep_from);
        crate::log::line("notif", &format!("WinRT seed: {} notifikasi aktif dimuat", seeded.len()));
        if !emit(tx, notify, Event::Seed(seeded)) {
            if let Some(tok) = token {
                let _ = listener.RemoveNotificationChanged(tok);
            }
            if com {
                unsafe { CoUninitialize() };
            }
            return true;
        }
    }

    while !flag.load(Ordering::Relaxed) {
        match poke_rx.recv_timeout(Duration::from_millis(1500)) {
            Ok((true, id)) => {
                if !seen.insert(id) {
                    continue;
                }
                if let Ok(u) = listener.GetNotification(id) {
                    if let Some(raw) = extract_notification(&u) {
                        crate::log::line(
                            "notif",
                            &format!("winrt app={} aumid={} title={}", raw.app_name, raw.aumid, raw.title),
                        );
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
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Periodic reconciliation guarantees notifications are never missed even if
                // Windows drops a WinRT NotificationChanged callback on sparse Win32 apps.
                if let Ok(list) = listener
                    .GetNotificationsAsync(NotificationKinds::Toast)
                    .and_then(|op| op.join())
                {
                    let n = list.Size().unwrap_or(0);
                    let mut live_ids: HashSet<u32> = HashSet::with_capacity(n as usize);
                    let mut newly_arrived: Vec<RawNotification> = Vec::new();
                    for i in 0..n {
                        if let Ok(u) = list.GetAt(i) {
                            if let Ok(id) = u.Id() {
                                live_ids.insert(id);
                                if seen.insert(id) {
                                    if let Some(raw) = extract_notification(&u) {
                                        newly_arrived.push(raw);
                                    }
                                }
                            }
                        }
                    }
                    newly_arrived.sort_by_key(|r| r.timestamp_s);
                    let mut closed = false;
                    for raw in newly_arrived {
                        crate::log::line(
                            "notif",
                            &format!("winrt-poll app={} aumid={} title={}", raw.app_name, raw.aumid, raw.title),
                        );
                        if !emit(tx, notify, Event::Arrived(raw)) {
                            closed = true;
                            break;
                        }
                    }
                    if closed {
                        break;
                    }
                    let removed: Vec<u32> = seen.iter().copied().filter(|id| !live_ids.contains(id)).collect();
                    for id in removed {
                        seen.remove(&id);
                        if !emit(tx, notify, Event::Removed(id)) {
                            closed = true;
                            break;
                        }
                    }
                    if closed {
                        break;
                    }
                }
            }
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

// =============================================================================
// Deep Notification Activation (Live Toast Invoke + Chat Room UIA Navigation)
// =============================================================================

/// Strips invisible Unicode bidi/format markers (WhatsApp/Telegram routinely wrap contact names
/// in `\u{2068}`/`\u{2069}`/`\u{200e}`/`\u{200f}`) and collapses whitespace for robust matching.
fn normalize_label(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for ch in s.chars() {
        if matches!(
            ch,
            '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
        ) {
            continue;
        }
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            for lc in ch.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Strips trailing unread count suffixes like `"papa (3 messages)"` or `"Grup Kelas (2 pesan)"`.
fn strip_count_suffix(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(open) = trimmed.rfind(" (") {
        if trimmed.ends_with(')') {
            let inside = &trimmed[open + 2..trimmed.len() - 1];
            let starts_digit = inside.chars().next().is_some_and(|c| c.is_ascii_digit());
            if starts_digit {
                return trimmed[..open].trim();
            }
        }
    }
    trimmed
}

/// Extracts ordered chat-room search candidates from a notification's `(title, body)`.
/// Handles direct chats (`"papa"`), unread counters (`"papa (2 messages)"`), and group formats
/// (`"Budi @ Grup Kelas"`, `"Grup Kelas: Budi"`, `"Budi in Grup Kelas"`).
fn extract_chat_targets(title: &str, app_name: &str) -> Vec<String> {
    let clean = strip_count_suffix(title);
    let norm_app = normalize_label(app_name);
    let mut out: Vec<String> = Vec::new();
    let mut push = |raw: &str| {
        let n = normalize_label(strip_count_suffix(raw));
        if n.len() >= 2 && n != norm_app && !out.iter().any(|x| x == &n) {
            out.push(n);
        }
    };

    // Group notifications: prefer the group/room name first.
    if let Some((left, right)) = clean.split_once(" @ ") {
        push(right);
        push(left);
    } else if let Some((left, right)) = clean.split_once(": ") {
        push(left);
        push(right);
    } else if let Some((left, right)) = clean.split_once(" in ") {
        push(right);
        push(left);
    }
    push(clean);
    out
}

fn is_chat_app(aumid: &str, app_name: &str) -> bool {
    let hay = format!("{} {}", aumid.to_ascii_lowercase(), app_name.to_ascii_lowercase());
    [
        "whatsapp",
        "telegram",
        "discord",
        "slack",
        "teams",
        "line",
        "signal",
        "messenger",
        "wechat",
        "viber",
        "element",
    ]
    .iter()
    .any(|k| hay.contains(k))
}

fn score_chat_candidate(candidate_norm: &str, target_norm: &str) -> u8 {
    if candidate_norm.is_empty() || target_norm.is_empty() {
        return 0;
    }
    if candidate_norm == target_norm {
        return 100;
    }
    if let Some(rest) = candidate_norm.strip_prefix(target_norm) {
        if rest
            .chars()
            .next()
            .is_some_and(|c| matches!(c, ' ' | ',' | ':' | ';' | '·' | '-' | '–' | '—' | '(' | '\n'))
        {
            return 90;
        }
    }
    if let Some(pos) = candidate_norm.find(target_norm) {
        let before_ok = pos == 0
            || candidate_norm[..pos]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_alphanumeric());
        let after_pos = pos + target_norm.len();
        let after_ok = after_pos == candidate_norm.len()
            || candidate_norm[after_pos..]
                .chars()
                .next()
                .is_some_and(|c| !c.is_alphanumeric());
        if before_ok && after_ok {
            return 78;
        }
    }
    0
}

/// Tier 1: If Windows' native toast banner matching `(app_name, title)` is still alive on screen,
/// invoke it via `IUIAutomationInvokePattern` so Windows delivers the app's native `launch` payload.
fn try_invoke_live_toast(app_name: &str, title: &str) -> bool {
    let target_title = normalize_label(strip_count_suffix(title));
    if target_title.is_empty() {
        return false;
    }
    let target_app = normalize_label(app_name);

    let mut toast_hwnds: Vec<isize> = Vec::new();
    unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if is_toast_host_pid(pid)
            && is_toast_host_class(&window_class_name(hwnd))
            && looks_like_toast_rect(hwnd)
        {
            let list = &mut *(lp.0 as *mut Vec<isize>);
            list.push(hwnd.0 as isize);
        }
        BOOL(1)
    }
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut toast_hwnds as *mut Vec<isize> as isize));
    }
    if toast_hwnds.is_empty() {
        return false;
    }

    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
    let invoked = (|| -> bool {
        let Ok(uia) = (unsafe { CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }) else {
            return false;
        };
        let Ok(all_cond) = (unsafe { uia.CreateTrueCondition() }) else {
            return false;
        };
        for raw_h in toast_hwnds {
            let hwnd = HWND(raw_h as *mut core::ffi::c_void);
            let Ok(el) = (unsafe { uia.ElementFromHandle(hwnd) }) else {
                continue;
            };
            let win_name = unsafe { el.CurrentName() }
                .ok()
                .map(|b| b.to_string())
                .unwrap_or_else(|| window_title(hwnd));
            let Ok(arr) = (unsafe { el.FindAll(TreeScope_Descendants, &all_cond) }) else {
                continue;
            };
            let n = unsafe { arr.Length() }.unwrap_or(0).min(48);
            let mut nodes: Vec<(String, String, bool)> = Vec::new();
            let mut invoke_target: Option<IUIAutomationElement> = None;
            for i in 0..n {
                let Ok(child) = (unsafe { arr.GetElement(i) }) else {
                    continue;
                };
                let ctype = unsafe { child.CurrentControlType() }.ok();
                let is_text = ctype == Some(UIA_TextControlTypeId);
                let aid = unsafe { child.CurrentAutomationId() }
                    .ok()
                    .map(|b| b.to_string())
                    .unwrap_or_default();
                let name = unsafe { child.CurrentName() }
                    .ok()
                    .map(|b| b.to_string())
                    .unwrap_or_default();
                let al = aid.to_ascii_lowercase();
                if invoke_target.is_none()
                    && (al.contains("normaltoastview") || al.contains("toastcontent") || al == "toastbackground")
                    && unsafe { child.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }.is_ok()
                {
                    invoke_target = Some(child);
                }
                if !aid.is_empty() || !name.trim().is_empty() {
                    nodes.push((aid, name, is_text));
                }
            }
            let Some((parsed_app, parsed_title, _)) = parse_toast_nodes(&win_name, &nodes) else {
                continue;
            };
            let pt = normalize_label(strip_count_suffix(&parsed_title));
            let pa = normalize_label(&parsed_app);
            let title_match = pt == target_title || pt.contains(&target_title) || target_title.contains(&pt);
            let app_match = target_app.is_empty()
                || target_app == "windows"
                || pa == "windows"
                || pa == target_app
                || pa.contains(&target_app)
                || target_app.contains(&pa);
            if !title_match || !app_match {
                continue;
            }
            let target_el = invoke_target.unwrap_or(el);
            if let Ok(pat) = unsafe { target_el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) } {
                if unsafe { pat.Invoke() }.is_ok() {
                    crate::log::line("notif", &format!("nav app={app_name} target=\"{title}\" via=live_toast"));
                    return true;
                }
            }
        }
        false
    })();
    if com {
        unsafe { CoUninitialize() };
    }
    invoked
}

fn click_point_safely(expected_hwnd: HWND, cx: i32, cy: i32) -> bool {
    unsafe {
        if GetForegroundWindow() != expected_hwnd {
            return false;
        }
        let mut prev = POINT::default();
        let have_prev = GetCursorPos(&mut prev).is_ok();
        if SetCursorPos(cx, cy).is_err() {
            return false;
        }
        std::thread::sleep(Duration::from_millis(12));
        let inputs = [
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dwFlags: MOUSEEVENTF_LEFTDOWN,
                        ..Default::default()
                    },
                },
            },
            INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dwFlags: MOUSEEVENTF_LEFTUP,
                        ..Default::default()
                    },
                },
            },
        ];
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        std::thread::sleep(Duration::from_millis(18));
        if have_prev {
            let _ = SetCursorPos(prev.x, prev.y);
        }
        sent == 2
    }
}

fn has_webview_child(hwnd: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::EnumChildWindows;
    struct Found(bool);
    unsafe extern "system" fn cb(child: HWND, lp: LPARAM) -> BOOL {
        let cls = window_class_name(child);
        if cls.starts_with("Chrome_") || cls.contains("WebView") {
            let f = &mut *(lp.0 as *mut Found);
            f.0 = true;
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut found = Found(false);
    unsafe {
        let _ = EnumChildWindows(Some(hwnd), Some(cb), LPARAM(&mut found as *mut Found as isize));
    }
    found.0
}

fn try_select_chat_in_hwnd(
    uia: &IUIAutomation,
    cond: &IUIAutomationCondition,
    hwnd: HWND,
    targets: &[String],
    app_name: &str,
) -> bool {
    let mut win_rc = RECT::default();
    if unsafe { GetWindowRect(hwnd, &mut win_rc) }.is_err() {
        return false;
    }
    let win_w = win_rc.right - win_rc.left;
    let win_h = win_rc.bottom - win_rc.top;
    if win_w < 240 || win_h < 200 {
        return false;
    }

    // Chat lists live in the left sidebar (below titlebar + search box); widen when app is compact.
    let left_min = win_rc.left + 8;
    let left_max = if win_w < 680 {
        win_rc.left + (win_w as f32 * 0.85) as i32
    } else {
        win_rc.left + (win_w as f32 * 0.46) as i32
    };
    let top_min = win_rc.top + 86;
    let bottom_max = win_rc.bottom - 12;

    let Ok(root) = (unsafe { uia.ElementFromHandle(hwnd) }) else {
        return false;
    };
    let Ok(arr) = (unsafe { root.FindAll(TreeScope_Descendants, cond) }) else {
        return false;
    };
    let total = unsafe { arr.Length() }.unwrap_or(0).min(500);

    let mut best: Option<(u8, u8, i32, IUIAutomationElement, RECT)> = None;
    for i in 0..total {
        let Ok(el) = (unsafe { arr.GetElement(i) }) else {
            continue;
        };
        let Ok(rc) = (unsafe { el.CurrentBoundingRectangle() }) else {
            continue;
        };
        let ew = rc.right - rc.left;
        let eh = rc.bottom - rc.top;
        if ew <= 4 || eh <= 4 || eh > 150 {
            continue;
        }
        let cx = (rc.left + rc.right) / 2;
        let cy = (rc.top + rc.bottom) / 2;
        if cx < left_min || cx > left_max || cy < top_min || cy > bottom_max {
            continue;
        }
        let Ok(name_bstr) = (unsafe { el.CurrentName() }) else {
            continue;
        };
        let norm_name = normalize_label(&name_bstr.to_string());
        if norm_name.is_empty() {
            continue;
        }
        let mut score = 0u8;
        for (ti, t) in targets.iter().enumerate() {
            let s = score_chat_candidate(&norm_name, t);
            // Slightly favor higher-priority extracted target (e.g. group name before sender).
            let adjusted = s.saturating_sub((ti as u8).saturating_mul(3));
            if adjusted > score {
                score = adjusted;
            }
        }
        if score == 0 {
            continue;
        }
        let ctype = unsafe { el.CurrentControlType() }.ok();
        let is_row = ctype == Some(UIA_ListItemControlTypeId)
            || ctype == Some(UIA_DataItemControlTypeId)
            || ctype == Some(UIA_TreeItemControlTypeId);
        let kind_rank: u8 = if is_row { 2 } else { 1 };
        let better = match &best {
            None => true,
            Some((bs, bk, by, _, _)) => {
                score > *bs || (score == *bs && (kind_rank > *bk || (kind_rank == *bk && cy < *by)))
            }
        };
        if better {
            best = Some((score, kind_rank, cy, el, rc));
        }
    }

    let Some((score, _, _, matched_el, matched_rc)) = best else {
        return false;
    };

    // If we matched a child Text node inside a chat row, walk up to its enclosing row item if present.
    let mut activate_el = matched_el;
    let mut activate_rc = matched_rc;
    if let Ok(walker) = unsafe { uia.ControlViewWalker() } {
        let mut cur = activate_el.clone();
        for _ in 0..4 {
            let Ok(parent) = (unsafe { walker.GetParentElement(&cur) }) else {
                break;
            };
            let ptype = unsafe { parent.CurrentControlType() }.ok();
            if ptype == Some(UIA_ListItemControlTypeId)
                || ptype == Some(UIA_DataItemControlTypeId)
                || ptype == Some(UIA_TreeItemControlTypeId)
                || ptype == Some(UIA_ButtonControlTypeId)
            {
                if let Ok(prc) = unsafe { parent.CurrentBoundingRectangle() } {
                    let ph = prc.bottom - prc.top;
                    if ph > 12 && ph <= 140 && prc.left >= win_rc.left && prc.right <= left_max + 80 {
                        activate_el = parent;
                        activate_rc = prc;
                        break;
                    }
                }
            }
            cur = parent;
        }
    }

    let is_webview = has_webview_child(hwnd);
    if !is_webview {
        if let Ok(sel) = unsafe {
            activate_el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
        } {
            if unsafe { sel.Select() }.is_ok() {
                crate::log::line(
                    "notif",
                    &format!("nav app={app_name} target=\"{}\" score={score} via=uia_select", targets[0]),
                );
                return true;
            }
        }
        if let Ok(inv) =
            unsafe { activate_el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }
        {
            if unsafe { inv.Invoke() }.is_ok() {
                crate::log::line(
                    "notif",
                    &format!("nav app={app_name} target=\"{}\" score={score} via=uia_invoke", targets[0]),
                );
                return true;
            }
        }
    }

    // Chromium/WebView2 & Electron chat rows (including WhatsApp WebView2) reliably switch rooms on click.
    let cx = (activate_rc.left + activate_rc.right) / 2;
    let cy = (activate_rc.top + activate_rc.bottom) / 2;
    if click_point_safely(hwnd, cx, cy) {
        crate::log::line(
            "notif",
            &format!(
                "nav app={app_name} target=\"{}\" score={score} via=uia_click({cx},{cy}) webview={is_webview}",
                targets[0]
            ),
        );
        return true;
    }
    false
}

fn spawn_chat_navigator(aumid: String, app_name: String, initial_hwnd: isize, targets: Vec<String>) {
    let _ = std::thread::Builder::new().name("abra-notif-nav".into()).spawn(move || {
        // Wait briefly for window restoration/foreground transition to settle.
        std::thread::sleep(Duration::from_millis(140));
        let mut raw_hwnd = initial_hwnd;
        for _ in 0..8 {
            if raw_hwnd != 0 && unsafe { GetForegroundWindow().0 as isize == raw_hwnd } {
                break;
            }
            if let Some(found) = crate::wins::focus::find_app_window(&aumid, &app_name) {
                raw_hwnd = found;
                if unsafe { GetForegroundWindow().0 as isize == raw_hwnd } {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(90));
        }
        if raw_hwnd == 0 {
            crate::log::line("notif", &format!("nav app={app_name} aborted: window not found"));
            return;
        }
        let hwnd = HWND(raw_hwnd as *mut core::ffi::c_void);
        if unsafe { GetForegroundWindow() != hwnd } {
            crate::log::line("notif", &format!("nav app={app_name} aborted: window not foreground"));
            return;
        }

        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
        if let Ok(uia) = unsafe { CoCreateInstance::<_, IUIAutomation>(&CUIAutomation, None, CLSCTX_INPROC_SERVER) } {
            let cond = (|| -> windows_core::Result<IUIAutomationCondition> {
                let c_list = unsafe {
                    uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_ListItemControlTypeId.0))
                }?;
                let c_data = unsafe {
                    uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_DataItemControlTypeId.0))
                }?;
                let c_tree = unsafe {
                    uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_TreeItemControlTypeId.0))
                }?;
                let c_text = unsafe {
                    uia.CreatePropertyCondition(UIA_ControlTypePropertyId, &VARIANT::from(UIA_TextControlTypeId.0))
                }?;
                let c1 = unsafe { uia.CreateOrCondition(&c_list, &c_data) }?;
                let c2 = unsafe { uia.CreateOrCondition(&c_tree, &c_text) }?;
                unsafe { uia.CreateOrCondition(&c1, &c2) }
            })();
            if let Ok(cond) = cond {
                if !try_select_chat_in_hwnd(&uia, &cond, hwnd, &targets, &app_name) {
                    // Retry once after 280ms in case the app was just restored from tray and still rendering.
                    std::thread::sleep(Duration::from_millis(280));
                    if unsafe { GetForegroundWindow() == hwnd }
                        && !try_select_chat_in_hwnd(&uia, &cond, hwnd, &targets, &app_name)
                    {
                        crate::log::line(
                            "notif",
                            &format!("nav app={app_name} target=\"{}\" not visible in chat sidebar", targets[0]),
                        );
                    }
                }
            }
        }
        if com {
            unsafe { CoUninitialize() };
        }
    });
}

/// Activates a clicked notification:
/// 1. If Windows' native toast banner is still visible on screen, invokes it directly so the app
///    receives its original deep-link `launch` payload.
/// 2. Otherwise brings the source app window to the foreground.
/// 3. For messaging apps (WhatsApp, Telegram, Discord, Slack, Teams, etc.), navigates to the
///    matching chat room (`title`) inside the app's sidebar via UI Automation.
pub fn activate_notification(aumid: &str, app_name: &str, title: &str, _body: &str) -> bool {
    if try_invoke_live_toast(app_name, title) {
        return true;
    }
    let Some(hwnd) = crate::wins::focus::focus_app_by_identity_hwnd(aumid, app_name) else {
        return false;
    };
    if is_chat_app(aumid, app_name) {
        let targets = extract_chat_targets(title, app_name);
        if !targets.is_empty() {
            spawn_chat_navigator(aumid.to_string(), app_name.to_string(), hwnd, targets);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_toast_automation_ids_and_filters_chrome() {
        let items = vec![
            ("NormalToastView".into(), String::new(), false),
            ("AppNameText".into(), "WhatsApp".into(), true),
            ("TitleText".into(), "Budi".into(), true),
            ("MessageText".into(), "Halo jadi meeting jam 3?".into(), true),
            ("DismissButton".into(), "Close".into(), false),
        ];
        assert_eq!(
            parse_toast_nodes("New notification", &items),
            Some((
                "WhatsApp".into(),
                "Budi".into(),
                "Halo jadi meeting jam 3?".into()
            ))
        );
    }

    #[test]
    fn extracts_app_name_from_win11_toast_header_and_win_name() {
        let items = vec![
            ("NormalToastView".into(), "New notification from WhatsApp, Budi, Halo".into(), false),
            ("TitleText".into(), "Budi".into(), true),
            ("MessageText".into(), "Halo".into(), true),
        ];
        assert_eq!(
            parse_toast_nodes("New notification", &items),
            Some(("WhatsApp".into(), "Budi".into(), "Halo".into()))
        );

        let header_btn = vec![
            ("HeaderButton".into(), "WhatsApp".into(), false),
            ("TitleText".into(), "Ani".into(), true),
            ("MessageText".into(), "Otw".into(), true),
        ];
        assert_eq!(
            parse_toast_nodes("Notifikasi baru", &header_btn),
            Some(("WhatsApp".into(), "Ani".into(), "Otw".into()))
        );
    }

    #[test]
    fn ignores_non_toast_shell_windows() {
        let items = vec![
            ("QuickSettings".into(), "Wi-Fi".into(), false),
            ("".into(), "Bluetooth".into(), true),
        ];
        assert_eq!(parse_toast_nodes("Quick settings", &items), None);
    }

    #[test]
    fn normalizes_bidi_markers_and_extracts_chat_targets() {
        assert_eq!(normalize_label("\u{2068}Papa\u{2069} \u{200E}"), "papa");
        assert_eq!(
            extract_chat_targets("\u{2068}Papa\u{2069} (2 messages)", "WhatsApp"),
            vec!["papa"]
        );
        assert_eq!(
            extract_chat_targets("Budi @ Grup Kelas", "WhatsApp"),
            vec!["grup kelas", "budi", "budi @ grup kelas"]
        );
        assert_eq!(
            score_chat_candidate("papa, 22:15, lagi dimana?", "papa"),
            90
        );
        assert_eq!(score_chat_candidate("papa", "papa"), 100);
        assert_eq!(score_chat_candidate("papaya", "papa"), 0);
        assert!(is_chat_app("5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App", "WhatsApp"));
        assert!(!is_chat_app("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad"));
    }
}
