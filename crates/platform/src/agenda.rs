use nacre_core::island::calendar::add_days;
use nacre_core::notch::json::{self, Json};
use std::sync::mpsc::{channel, Receiver, Sender};
use windows::ApplicationModel::Appointments::{
    AppointmentManager, AppointmentProperties, AppointmentStore, AppointmentStoreAccessType,
    FindAppointmentsOptions,
};
use windows::Foundation::{DateTime, TimeSpan};
use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::Time::{
    FileTimeToSystemTime, SystemTimeToFileTime, SystemTimeToTzSpecificLocalTime,
    TzSpecificLocalTimeToSystemTime,
};

pub type Date = (i32, u32, u32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub title: String,
    pub minute: Option<u32>,
}

pub struct Agenda {
    pub requests: Sender<Date>,
    pub answers: Receiver<(Date, Option<Vec<Event>>)>,
}

fn local_midnight_utc_st(d: Date) -> Option<SYSTEMTIME> {
    unsafe {
        let local = SYSTEMTIME {
            wYear: d.0 as u16,
            wMonth: d.1 as u16,
            wDay: d.2 as u16,
            ..Default::default()
        };
        let mut utc = SYSTEMTIME::default();
        TzSpecificLocalTimeToSystemTime(None, &local, &mut utc).ok()?;
        Some(utc)
    }
}

fn local_midnight(d: Date) -> Option<DateTime> {
    unsafe {
        let utc = local_midnight_utc_st(d)?;
        let mut ft = FILETIME::default();
        SystemTimeToFileTime(&utc, &mut ft).ok()?;
        Some(DateTime {
            UniversalTime: ((ft.dwHighDateTime as i64) << 32) | ft.dwLowDateTime as i64,
        })
    }
}

fn format_rfc3339_utc(st: SYSTEMTIME) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond
    )
}

fn to_local(t: DateTime) -> Option<(Date, u32)> {
    unsafe {
        let v = t.UniversalTime as u64;
        let ft = FILETIME { dwLowDateTime: v as u32, dwHighDateTime: (v >> 32) as u32 };
        let mut utc = SYSTEMTIME::default();
        FileTimeToSystemTime(&ft, &mut utc).ok()?;
        let mut local = SYSTEMTIME::default();
        SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).ok()?;
        Some((
            (local.wYear as i32, local.wMonth as u32, local.wDay as u32),
            local.wHour as u32 * 60 + local.wMinute as u32,
        ))
    }
}

fn url_enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Parses an RFC3339 `dateTime` string (either `...Z` UTC or `...+HH:MM` local offset)
/// into `(local_date, minute_of_day)`.
fn parse_gcal_datetime(s: &str, target_date: Date) -> Option<(Date, u32)> {
    let (date_part, time_part) = s.split_once('T')?;
    let mut dp = date_part.split('-');
    let y: i32 = dp.next()?.parse().ok()?;
    let m: u32 = dp.next()?.parse().ok()?;
    let day: u32 = dp.next()?.parse().ok()?;

    let h: u32 = time_part.get(0..2)?.parse().ok()?;
    let min: u32 = time_part.get(3..5)?.parse().ok()?;
    let sec: u32 = time_part.get(6..8).and_then(|v| v.parse().ok()).unwrap_or(0);

    if time_part.ends_with('Z') || time_part.ends_with('z') {
        unsafe {
            let utc = SYSTEMTIME {
                wYear: y as u16,
                wMonth: m as u16,
                wDay: day as u16,
                wHour: h as u16,
                wMinute: min as u16,
                wSecond: sec as u16,
                ..Default::default()
            };
            let mut local = SYSTEMTIME::default();
            if SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).is_ok() {
                return Some((
                    (local.wYear as i32, local.wMonth as u32, local.wDay as u32),
                    local.wHour as u32 * 60 + local.wMinute as u32,
                ));
            }
        }
    }
    // Offset-qualified timestamp returned by Google Calendar uses the calendar's local timezone.
    let parsed_date = (y, m, day);
    if parsed_date == target_date {
        Some((parsed_date, h * 60 + min))
    } else {
        Some((parsed_date, h * 60 + min))
    }
}

fn fetch_google_calendar(d: Date) -> Option<Vec<Event>> {
    let token = crate::gtasks::access_token()?;
    let start_utc = local_midnight_utc_st(d)?;
    let next_day = add_days(d.0, d.1, d.2, 1);
    let end_utc = local_midnight_utc_st(next_day)?;

    let url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/primary/events?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime&maxResults=12",
        url_enc(&format_rfc3339_utc(start_utc)),
        url_enc(&format_rfc3339_utc(end_utc)),
    );
    let auth = format!("Bearer {token}");
    let (status, body) = crate::http::request("GET", &url, &[("Authorization", auth.as_str())], None).ok()?;
    if status >= 400 {
        crate::log::line("kalender", &format!("Google Calendar HTTP {status}"));
        return None;
    }
    let root = json::parse(&body)?;
    let mut out = Vec::new();
    if let Some(items) = root.get("items").and_then(Json::as_arr) {
        for item in items {
            if item.get("status").and_then(Json::as_str) == Some("cancelled") {
                continue;
            }
            let title = item
                .get("summary")
                .and_then(Json::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("(Tanpa judul)")
                .to_string();
            let start = item.get("start");
            let minute = start
                .and_then(|st| st.get("dateTime"))
                .and_then(Json::as_str)
                .and_then(|dt| parse_gcal_datetime(dt, d))
                .and_then(|(ev_d, m)| (ev_d == d).then_some(m));
            out.push(Event { title, minute });
        }
    }
    out.sort_by_key(|e| e.minute.map_or(0, |m| m + 1));
    Some(out)
}

fn read(store: &AppointmentStore, d: Date) -> Option<Vec<Event>> {
    let start = local_midnight(d)?;
    let opts = FindAppointmentsOptions::new().ok()?;
    if let Ok(props) = opts.FetchProperties() {
        for p in [
            AppointmentProperties::Subject(),
            AppointmentProperties::StartTime(),
            AppointmentProperties::AllDay(),
            AppointmentProperties::Duration(),
        ]
        .into_iter()
        .flatten()
        {
            let _ = props.Append(&p);
        }
    }
    let _ = opts.SetMaxCount(20);
    let day = TimeSpan { Duration: 24 * 3600 * 10_000_000 };
    let list = store.FindAppointmentsAsyncWithOptions(start, day, &opts).ok()?.join().ok()?;
    let mut out = Vec::new();
    for a in list {
        let title = a.Subject().map(|s| s.to_string()).unwrap_or_default();
        let all_day = a.AllDay().unwrap_or(false);
        let minute = match a.StartTime().ok().and_then(to_local) {
            Some((date, m)) if date == d && !all_day => Some(m),
            _ => None,
        };
        out.push(Event { title, minute });
    }
    out.sort_by_key(|e| e.minute.map_or(0, |m| m + 1));
    Some(out)
}

fn merge_events(gcal: Option<Vec<Event>>, win: Option<Vec<Event>>) -> Option<Vec<Event>> {
    match (gcal, win) {
        (None, None) => None,
        (Some(g), None) => Some(g),
        (None, Some(w)) => Some(w),
        (Some(mut g), Some(w)) => {
            for ev in w {
                if !g.iter().any(|x| x.minute == ev.minute && x.title.eq_ignore_ascii_case(&ev.title)) {
                    g.push(ev);
                }
            }
            g.sort_by_key(|e| e.minute.map_or(0, |m| m + 1));
            Some(g)
        }
    }
}

fn fake(d: Date, today: Date) -> Option<Vec<Event>> {
    Some(if d == today {
        vec![
            Event { title: "Rapat Uji".into(), minute: Some(14 * 60 + 30) },
            Event { title: "Olahraga Uji".into(), minute: Some(18 * 60) },
        ]
    } else {
        vec![]
    })
}

pub fn spawn(notify: impl Fn() + Send + 'static) -> Agenda {
    let (req_tx, req_rx) = channel::<Date>();
    let (ans_tx, ans_rx) = channel::<(Date, Option<Vec<Event>>)>();
    let fake_mode = std::env::var_os("NACRE_FAKE_CAL").is_some();
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let store = if fake_mode {
            None
        } else {
            AppointmentManager::RequestStoreAsync(AppointmentStoreAccessType::AllCalendarsReadOnly)
                .ok()
                .and_then(|o| o.join().ok())
        };
        if !fake_mode {
            crate::log::line(
                "kalender",
                if store.is_some() {
                    "kalender Windows terbaca"
                } else {
                    "kalender Windows tidak tersedia (menggunakan Google Calendar jika terhubung)"
                },
            );
        }
        while let Ok(mut d) = req_rx.recv() {
            while let Ok(n) = req_rx.try_recv() {
                d = n;
            }
            let ans = if fake_mode {
                let t = crate::time::local_now();
                fake(d, (t.year, t.month, t.day))
            } else {
                let gcal = fetch_google_calendar(d);
                let win = store.as_ref().and_then(|s| read(s, d));
                merge_events(gcal, win)
            };
            if ans_tx.send((d, ans)).is_err() {
                return;
            }
            notify();
        }
    });
    Agenda { requests: req_tx, answers: ans_rx }
}
