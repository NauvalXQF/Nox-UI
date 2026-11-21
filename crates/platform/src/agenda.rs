use nacre_core::island::calendar::add_days;
use nacre_core::notch::json::{self, Json};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
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

fn parse_ymd(s: &str) -> Option<Date> {
    let mut dp = s.get(0..10)?.split('-');
    let y: i32 = dp.next()?.parse().ok()?;
    let m: u32 = dp.next()?.parse().ok()?;
    let d: u32 = dp.next()?.parse().ok()?;
    Some((y, m, d))
}

fn all_day_includes(start: Date, end_excl: Option<Date>, target: Date) -> bool {
    match end_excl {
        Some(e) if e > start => target >= start && target < e,
        _ => target == start,
    }
}

/// Parses an RFC3339 `dateTime` string (`...Z` or `...±HH:MM`) and converts it accurately
/// into the machine's local `(Date, minute_of_day)`.
fn parse_gcal_datetime(s: &str) -> Option<(Date, u32)> {
    let (date_part, time_part) = s.split_once('T')?;
    let (y, m, day) = parse_ymd(date_part)?;
    let h: u32 = time_part.get(0..2)?.parse().ok()?;
    let min: u32 = time_part.get(3..5)?.parse().ok()?;
    let sec: u32 = time_part.get(6..8).and_then(|v| v.parse().ok()).unwrap_or(0);

    // Parse timezone suffix (`Z` or `±HH:MM`) into signed offset minutes from UTC.
    let tz_part = time_part.get(8..).unwrap_or("");
    let offset_min: Option<i64> = if tz_part.ends_with('Z') || tz_part.ends_with('z') {
        Some(0)
    } else if let Some(pos) = tz_part.rfind(['+', '-']) {
        let sign: i64 = if tz_part.as_bytes()[pos] == b'-' { -1 } else { 1 };
        let rest = &tz_part[pos + 1..];
        let oh: i64 = rest.get(0..2).and_then(|v| v.parse().ok()).unwrap_or(0);
        let om: i64 = rest
            .get(3..5)
            .or_else(|| rest.get(2..4))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        Some(sign * (oh * 60 + om))
    } else {
        None
    };

    if let Some(off_m) = offset_min {
        unsafe {
            let st = SYSTEMTIME {
                wYear: y as u16,
                wMonth: m as u16,
                wDay: day as u16,
                wHour: h as u16,
                wMinute: min as u16,
                wSecond: sec as u16,
                ..Default::default()
            };
            let mut ft = FILETIME::default();
            if SystemTimeToFileTime(&st, &mut ft).is_ok() {
                let raw = (((ft.dwHighDateTime as u64) << 32) | ft.dwLowDateTime as u64) as i64;
                // Subtract offset to get true UTC ticks (100ns units).
                let utc_ticks = raw - off_m * 60 * 10_000_000;
                if let Some(res) = to_local(DateTime { UniversalTime: utc_ticks }) {
                    return Some(res);
                }
            }
        }
    }
    Some(((y, m, day), h * 60 + min))
}

struct GCalState {
    cal_ids: Vec<String>,
    cal_ids_at: Option<Instant>,
    logged_no_token: bool,
}

impl GCalState {
    fn new() -> Self {
        Self {
            cal_ids: Vec::new(),
            cal_ids_at: None,
            logged_no_token: false,
        }
    }

    fn calendar_ids(&mut self, token: &str) -> Vec<String> {
        let fresh = self
            .cal_ids_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(300));
        if fresh && !self.cal_ids.is_empty() {
            return self.cal_ids.clone();
        }
        let auth = format!("Bearer {token}");
        let mut ids = vec!["primary".to_string()];
        if let Ok((status, body)) = crate::http::request(
            "GET",
            "https://www.googleapis.com/calendar/v3/users/me/calendarList?minAccessRole=reader&maxResults=25",
            &[("Authorization", auth.as_str())],
            None,
        ) {
            if status < 400 {
                if let Some(root) = json::parse(&body) {
                    if let Some(items) = root.get("items").and_then(Json::as_arr) {
                        for item in items {
                            if item.get("selected").and_then(Json::as_bool) == Some(false) {
                                continue;
                            }
                            if item.get("primary").and_then(Json::as_bool) == Some(true) {
                                continue;
                            }
                            let Some(id) = item.get("id").and_then(Json::as_str) else { continue };
                            if id.is_empty() || ids.iter().any(|x| x == id) {
                                continue;
                            }
                            ids.push(id.to_string());
                            if ids.len() >= 6 {
                                break;
                            }
                        }
                    }
                }
            }
        }
        self.cal_ids = ids.clone();
        self.cal_ids_at = Some(Instant::now());
        ids
    }
}

fn fetch_single_calendar(token: &str, cal_id: &str, d: Date, time_min: &str, time_max: &str) -> Result<Vec<Event>, String> {
    let url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/{}/events?timeMin={}&timeMax={}&singleEvents=true&orderBy=startTime&maxResults=15",
        url_enc(cal_id),
        url_enc(time_min),
        url_enc(time_max),
    );
    let auth = format!("Bearer {token}");
    let (status, body) = crate::http::request("GET", &url, &[("Authorization", auth.as_str())], None)?;
    if status >= 400 {
        let msg = json::parse(&body)
            .and_then(|j| j.get("error")?.get("message")?.as_str().map(String::from))
            .unwrap_or_default();
        return Err(format!("HTTP {status} {msg}"));
    }
    let Some(root) = json::parse(&body) else {
        return Err("respons bukan JSON".into());
    };
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
            let end = item.get("end");

            if let Some(dt) = start.and_then(|st| st.get("dateTime")).and_then(Json::as_str) {
                if let Some((ev_d, m)) = parse_gcal_datetime(dt) {
                    if ev_d == d {
                        out.push(Event { title, minute: Some(m) });
                    }
                }
            } else if let Some(sd_str) = start.and_then(|st| st.get("date")).and_then(Json::as_str) {
                if let Some(sd) = parse_ymd(sd_str) {
                    let ed = end
                        .and_then(|en| en.get("date"))
                        .and_then(Json::as_str)
                        .and_then(parse_ymd);
                    if all_day_includes(sd, ed, d) {
                        out.push(Event { title, minute: None });
                    }
                }
            }
        }
    }
    Ok(out)
}

fn fetch_google_calendar(state: &mut GCalState, d: Date) -> Option<Vec<Event>> {
    let Some(token) = crate::gtasks::access_token() else {
        if !state.logged_no_token {
            state.logged_no_token = true;
            crate::log::line("kalender", "Google Calendar: belum ada token OAuth");
        }
        return None;
    };
    state.logged_no_token = false;

    let start_utc = local_midnight_utc_st(d)?;
    let next_day = add_days(d.0, d.1, d.2, 1);
    let end_utc = local_midnight_utc_st(next_day)?;
    let time_min = format_rfc3339_utc(start_utc);
    let time_max = format_rfc3339_utc(end_utc);

    let cals = state.calendar_ids(&token);
    let mut combined = Vec::new();
    let mut any_ok = false;
    for cal_id in &cals {
        match fetch_single_calendar(&token, cal_id, d, &time_min, &time_max) {
            Ok(list) => {
                any_ok = true;
                for ev in list {
                    if !combined
                        .iter()
                        .any(|x: &Event| x.minute == ev.minute && x.title.eq_ignore_ascii_case(&ev.title))
                    {
                        combined.push(ev);
                    }
                }
            }
            Err(e) => {
                if cal_id == "primary" {
                    crate::log::line("kalender", &format!("Google Calendar ({cal_id}) gagal: {e}"));
                }
            }
        }
    }
    if !any_ok {
        return None;
    }
    combined.sort_by_key(|e| e.minute.map_or(0, |m| m + 1));
    crate::log::line(
        "kalender",
        &format!("Google Calendar {:04}-{:02}-{:02} ({} kalender) -> {} acara", d.0, d.1, d.2, cals.len(), combined.len()),
    );
    Some(combined)
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
        let mut gcal_state = GCalState::new();
        while let Ok(mut d) = req_rx.recv() {
            while let Ok(n) = req_rx.try_recv() {
                d = n;
            }
            let ans = if fake_mode {
                let t = crate::time::local_now();
                fake(d, (t.year, t.month, t.day))
            } else {
                let gcal = fetch_google_calendar(&mut gcal_state, d);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_day_range_excludes_previous_day() {
        assert!(!all_day_includes((2026, 11, 20), Some((2026, 11, 21)), (2026, 11, 21)));
        assert!(all_day_includes((2026, 11, 21), Some((2026, 11, 22)), (2026, 11, 21)));
        assert!(all_day_includes((2026, 11, 20), Some((2026, 11, 23)), (2026, 11, 21)));
    }

    #[test]
    fn parses_rfc3339_with_offset() {
        assert!(parse_gcal_datetime("2026-11-21T14:30:00+07:00").is_some());
        assert!(parse_gcal_datetime("2026-11-21T07:30:00Z").is_some());
    }
}
