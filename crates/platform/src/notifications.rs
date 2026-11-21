use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::Management::{
    UserNotificationListener, UserNotificationListenerAccessStatus,
};
use windows::UI::Notifications::{
    KnownNotificationBindings, NotificationKinds, UserNotification,
    UserNotificationChangedEventArgs, UserNotificationChangedKind,
};
use windows::Win32::Foundation::APPMODEL_ERROR_NO_PACKAGE;
use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    Connecting = 0,
    Connected = 1,
    NeedsIdentity = 2,
    Denied = 3,
    Unsupported = 4,
}

impl Status {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => Status::Connected,
            2 => Status::NeedsIdentity,
            3 => Status::Denied,
            4 => Status::Unsupported,
            _ => Status::Connecting,
        }
    }
}

static STATUS: AtomicU8 = AtomicU8::new(Status::Connecting as u8);

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

fn emit(tx: &Sender<Event>, notify: &(dyn Fn() + Send), ev: Event) -> bool {
    if tx.send(ev).is_err() {
        return false;
    }
    notify();
    true
}

pub fn spawn(notify: impl Fn() + Send + 'static) -> Notifications {
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

        if !has_package_identity() {
            set_status(Status::NeedsIdentity);
            crate::log::line("notif", "tanpa Package Identity (unpackaged); mirror notifikasi Windows menunggu Sparse Package");
            let _ = emit(&tx, &notify, Event::Status(Status::NeedsIdentity));
            return;
        }

        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
        let Ok(listener) = UserNotificationListener::Current() else {
            set_status(Status::Unsupported);
            let _ = emit(&tx, &notify, Event::Status(Status::Unsupported));
            if com {
                unsafe { windows::Win32::System::Com::CoUninitialize() };
            }
            return;
        };

        let access = listener
            .RequestAccessAsync()
            .ok()
            .and_then(|op| op.join().ok());
        match access {
            Some(UserNotificationListenerAccessStatus::Allowed) => {
                set_status(Status::Connected);
                crate::log::line("notif", "UserNotificationListener terhubung");
                let _ = emit(&tx, &notify, Event::Status(Status::Connected));
            }
            Some(UserNotificationListenerAccessStatus::Denied) => {
                set_status(Status::Denied);
                crate::log::line("notif", "izin UserNotificationListener ditolak");
                let _ = emit(&tx, &notify, Event::Status(Status::Denied));
                if com {
                    unsafe { windows::Win32::System::Com::CoUninitialize() };
                }
                return;
            }
            _ => {
                set_status(Status::NeedsIdentity);
                crate::log::line("notif", "izin UserNotificationListener tidak tersedia");
                let _ = emit(&tx, &notify, Event::Status(Status::NeedsIdentity));
                if com {
                    unsafe { windows::Win32::System::Com::CoUninitialize() };
                }
                return;
            }
        }

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
                            if !emit(&tx, &notify, Event::Arrived(raw)) {
                                break;
                            }
                        }
                    }
                }
                Ok((false, id)) => {
                    seen.remove(&id);
                    if !emit(&tx, &notify, Event::Removed(id)) {
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
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    });

    Notifications { stop, events: rx }
}
