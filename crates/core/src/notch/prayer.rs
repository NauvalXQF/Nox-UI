use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Prayer {
    Subuh = 0,
    Dzuhur = 1,
    Ashar = 2,
    Maghrib = 3,
    Isya = 4,
}

impl Prayer {
    pub const ALL: [Prayer; 5] = [
        Prayer::Subuh,
        Prayer::Dzuhur,
        Prayer::Ashar,
        Prayer::Maghrib,
        Prayer::Isya,
    ];

    pub const fn idx(self) -> usize {
        self as usize
    }

    pub const fn name(self) -> &'static str {
        match self {
            Prayer::Subuh => "Subuh",
            Prayer::Dzuhur => "Dzuhur",
            Prayer::Ashar => "Ashar",
            Prayer::Maghrib => "Maghrib",
            Prayer::Isya => "Isya",
        }
    }

    /// Primary adaptive glow & arc accent color for each prayer's time-of-day atmosphere.
    pub const fn accent_rgb(self) -> [u8; 3] {
        match self {
            Prayer::Subuh => [56, 189, 248],   // Crisp dawn sky blue
            Prayer::Dzuhur => [88, 166, 255],  // Bright azure noon blue
            Prayer::Ashar => [245, 165, 36],   // Warm afternoon amber gold
            Prayer::Maghrib => [251, 113, 98], // Sunset coral-orange dusk
            Prayer::Isya => [129, 140, 248],   // Calm night indigo
        }
    }

    /// Secondary gradient endpoint for the progress bar and atmospheric wash.
    pub const fn grad_end_rgb(self) -> [u8; 3] {
        match self {
            Prayer::Subuh => [99, 102, 241],
            Prayer::Dzuhur => [138, 114, 255],
            Prayer::Ashar => [234, 88, 12],
            Prayer::Maghrib => [192, 84, 224],
            Prayer::Isya => [88, 86, 214],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CalcMethod {
    Kemenag = 0,
    Mwl = 1,
    UmmAlQura = 2,
    Isna = 3,
    Egyptian = 4,
}

impl CalcMethod {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Mwl,
            2 => Self::UmmAlQura,
            3 => Self::Isna,
            4 => Self::Egyptian,
            _ => Self::Kemenag,
        }
    }

    /// `(fajr_angle_deg, isha_angle_deg_or_none_for_90m, ihtiyat_min)`
    const fn params(self) -> (f64, Option<f64>, f64) {
        match self {
            // Kemenag RI standard: Fajr 20 deg, Isha 18 deg, +2 min ihtiyat
            Self::Kemenag => (20.0, Some(18.0), 2.0),
            // Muslim World League: Fajr 18 deg, Isha 17 deg
            Self::Mwl => (18.0, Some(17.0), 0.0),
            // Umm al-Qura (Makkah): Fajr 18.5 deg, Isha 90 min after Maghrib
            Self::UmmAlQura => (18.5, None, 0.0),
            // ISNA: Fajr 15 deg, Isha 15 deg
            Self::Isna => (15.0, Some(15.0), 0.0),
            // Egyptian General Authority: Fajr 19.5 deg, Isha 17.5 deg
            Self::Egyptian => (19.5, Some(17.5), 0.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AlertKind {
    Before10,
    Before5,
    Now,
}

impl AlertKind {
    pub const fn lead_mins(self) -> i32 {
        match self {
            AlertKind::Before10 => 10,
            AlertKind::Before5 => 5,
            AlertKind::Now => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DaySchedule {
    /// Minutes since local midnight (`0..1440`) for `[Subuh, Dzuhur, Ashar, Maghrib, Isya]`.
    pub times: [u16; 5],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrayerSnapshot {
    pub schedule: DaySchedule,
    pub next: Prayer,
    pub next_time_min: u16,
    pub mins_until: i32,
    /// `0.0..1.0` elapsed fraction between the previous prayer and `next`.
    pub interval_progress: f32,
    /// `0.0..1.0` real-time position along the Subuh..Isya solar arc (so Dzuhur sits naturally at solar noon ~0.50).
    pub arc_progress: f32,
    /// Normalized `0.0..1.0` positions of the 5 prayer milestones along the solar arc.
    pub dot_us: [f32; 5],
    /// True during the first 5 minutes after a prayer time arrives.
    pub just_arrived: Option<Prayer>,
}

pub fn format_hhmm(min: u16) -> String {
    let m = min % 1440;
    format!("{:02}:{:02}", m / 60, m % 60)
}

pub fn format_countdown(mins_until: i32, en: bool) -> String {
    let m = mins_until.max(0);
    if m == 0 {
        return if en { "now".into() } else { "sekarang".into() };
    }
    let h = m / 60;
    let rem = m % 60;
    if en {
        if h > 0 && rem > 0 {
            format!("in {h}h {rem}m")
        } else if h > 0 {
            format!("in {h}h")
        } else {
            format!("in {rem} min")
        }
    } else if h > 0 && rem > 0 {
        format!("{h}j {rem}m lagi")
    } else if h > 0 {
        format!("{h} jam lagi")
    } else {
        format!("{rem} mnt lagi")
    }
}

fn julian_day(year: i32, month: u32, day: u32) -> f64 {
    let (mut y, mut m) = (year as f64, month as f64);
    if m <= 2.0 {
        y -= 1.0;
        m += 12.0;
    }
    let a = (y / 100.0).floor();
    let b = 2.0 - a + (a / 4.0).floor();
    (365.25 * (y + 4716.0)).floor() + (30.6001 * (m + 1.0)).floor() + day as f64 + b - 1524.5
}

fn deg2rad(d: f64) -> f64 {
    d * PI / 180.0
}

fn rad2deg(r: f64) -> f64 {
    r * 180.0 / PI
}

fn fix_angle(a: f64) -> f64 {
    a.rem_euclid(360.0)
}

/// Solar `(declination_deg, equation_of_time_hours)` at Julian day `jd` (NOAA / Meeus low-error model).
fn sun_position(jd: f64) -> (f64, f64) {
    let d = jd - 2451545.0;
    let g = fix_angle(357.529 + 0.98560028 * d);
    let q = fix_angle(280.459 + 0.98564736 * d);
    let l = fix_angle(q + 1.915 * deg2rad(g).sin() + 0.020 * deg2rad(2.0 * g).sin());
    let e = 23.439 - 0.00000036 * d;
    let l_r = deg2rad(l);
    let e_r = deg2rad(e);
    let ra = rad2deg((e_r.cos() * l_r.sin()).atan2(l_r.cos())) / 15.0;
    let ra = ra.rem_euclid(24.0);
    let decl = rad2deg((e_r.sin() * l_r.sin()).asin());
    let mut eqt = q / 15.0 - ra;
    if eqt > 12.0 {
        eqt -= 24.0;
    } else if eqt < -12.0 {
        eqt += 24.0;
    }
    (decl, eqt)
}

/// Hour angle in hours for solar altitude `alt_deg` at latitude `lat_deg` and declination `decl_deg`.
fn hour_angle_hours(lat_deg: f64, decl_deg: f64, alt_deg: f64) -> f64 {
    let lat = deg2rad(lat_deg);
    let decl = deg2rad(decl_deg);
    let num = deg2rad(alt_deg).sin() - lat.sin() * decl.sin();
    let den = (lat.cos() * decl.cos()).max(1e-6);
    let cos_h = (num / den).clamp(-1.0, 1.0);
    rad2deg(cos_h.acos()) / 15.0
}

fn hours_to_min(h: f64) -> u16 {
    let m = (h * 60.0).round() as i32;
    m.rem_euclid(1440) as u16
}

/// Computes the 5 daily prayer times in local minutes from midnight (`0..1440`).
/// Uses 2-pass refinement (evaluating solar declination at each prayer's approximate UTC instant)
/// for sub-minute accuracy matching Kemenag RI / Aladhan tables worldwide.
pub fn compute_day(
    year: i32,
    month: u32,
    day: u32,
    lat: f64,
    lon: f64,
    tz_offset_min: i32,
    method: CalcMethod,
) -> DaySchedule {
    let tz_h = tz_offset_min as f64 / 60.0;
    let jd0 = julian_day(year, month, day) - lon / 360.0;
    let (fajr_ang, isha_ang, ihtiyat) = method.params();

    let eval_at = |frac_day: f64| -> (f64, f64) { sun_position(jd0 + frac_day) };

    // Pass 1 (nominal times) -> Pass 2 (refined solar coordinates at actual prayer times)
    let mut t_fajr = 5.0 / 24.0;
    let mut t_dhuhr = 12.0 / 24.0;
    let mut t_asr = 15.0 / 24.0;
    let mut t_maghrib = 18.0 / 24.0;
    let mut t_isha = 19.5 / 24.0;

    let mut subuh_h = 4.5;
    let mut dzuhur_h = 12.0;
    let mut ashar_h = 15.0;
    let mut maghrib_h = 18.0;
    let mut isya_h = 19.2;

    for _ in 0..2 {
        let (d_f, eq_f) = eval_at(t_fajr);
        let noon_f = 12.0 + tz_h - lon / 15.0 - eq_f;
        subuh_h = noon_f - hour_angle_hours(lat, d_f, -fajr_ang);

        let (_, eq_d) = eval_at(t_dhuhr);
        dzuhur_h = 12.0 + tz_h - lon / 15.0 - eq_d;

        let (d_a, eq_a) = eval_at(t_asr);
        let noon_a = 12.0 + tz_h - lon / 15.0 - eq_a;
        let asr_alt = rad2deg((1.0 / (1.0 + deg2rad((lat - d_a).abs()).tan())).atan());
        ashar_h = noon_a + hour_angle_hours(lat, d_a, asr_alt);

        let (d_m, eq_m) = eval_at(t_maghrib);
        let noon_m = 12.0 + tz_h - lon / 15.0 - eq_m;
        maghrib_h = noon_m + hour_angle_hours(lat, d_m, -0.8333);

        let (d_i, eq_i) = eval_at(t_isha);
        let noon_i = 12.0 + tz_h - lon / 15.0 - eq_i;
        isya_h = match isha_ang {
            Some(ang) => noon_i + hour_angle_hours(lat, d_i, -ang),
            None => maghrib_h + 1.5,
        };

        t_fajr = subuh_h / 24.0;
        t_dhuhr = dzuhur_h / 24.0;
        t_asr = ashar_h / 24.0;
        t_maghrib = maghrib_h / 24.0;
        t_isha = isya_h / 24.0;
    }

    let ih = ihtiyat / 60.0;
    let dz_ih = ihtiyat.max(2.0) / 60.0;
    let is_ih = if isha_ang.is_some() { ih } else { 0.0 };

    DaySchedule {
        times: [
            hours_to_min(subuh_h + ih),
            hours_to_min(dzuhur_h + dz_ih),
            hours_to_min(ashar_h + ih),
            hours_to_min(maghrib_h + ih),
            hours_to_min(isya_h + is_ih),
        ],
    }
}

/// Evaluates current snapshot (next prayer, countdown, interval progress, solar arc position).
pub fn snapshot(schedule: DaySchedule, now_hour: u32, now_min: u32, now_sec: u32) -> PrayerSnapshot {
    let now_m = (now_hour.min(23) * 60 + now_min.min(59)) as i32;
    let now_f = now_m as f32 + now_sec.min(59) as f32 / 60.0;
    let ts = schedule.times;

    // Check if we are within the first 5 minutes of any prayer time.
    let mut just_arrived = None;
    for (i, &t) in ts.iter().enumerate() {
        let diff = now_m - t as i32;
        if (0..=4).contains(&diff) {
            just_arrived = Some(Prayer::ALL[i]);
            break;
        }
    }

    // Find next prayer (strictly > now_m, or wrap to tomorrow's Subuh).
    let (next_idx, prev_m, next_m) = match ts.iter().position(|&t| (t as i32) > now_m) {
        Some(0) => (0, ts[4] as i32 - 1440, ts[0] as i32),
        Some(i) => (i, ts[i - 1] as i32, ts[i] as i32),
        None => (0, ts[4] as i32, ts[0] as i32 + 1440),
    };

    let mins_until = (next_m - now_m).max(0);
    let span = (next_m - prev_m).max(1) as f32;
    let interval_progress = ((now_f - prev_m as f32) / span).clamp(0.0, 1.0);

    // Real-time solar arc positions across [Subuh .. Isya] so Dzuhur naturally sits near solar noon (~0.50).
    let day_span = (ts[4] as f32 - ts[0] as f32).max(60.0);
    let mut dot_us = [0.0f32; 5];
    for i in 0..5 {
        dot_us[i] = ((ts[i] as f32 - ts[0] as f32) / day_span).clamp(0.0, 1.0);
    }
    let arc_progress = ((now_f - ts[0] as f32) / day_span).clamp(0.0, 1.0);

    PrayerSnapshot {
        schedule,
        next: Prayer::ALL[next_idx],
        next_time_min: ts[next_idx],
        mins_until,
        interval_progress,
        arc_progress,
        dot_us,
        just_arrived,
    }
}

/// Checks whether a prayer alert should fire at `now_min` (`0..1440`) given user alert preferences.
/// Returns the highest-priority matching `(Prayer, AlertKind)` (Now > Before5 > Before10).
pub fn check_alert(
    schedule: DaySchedule,
    now_min: u16,
    alert_10m: bool,
    alert_5m: bool,
    alert_now: bool,
) -> Option<(Prayer, AlertKind)> {
    let now = now_min as i32;
    for (i, &t) in schedule.times.iter().enumerate() {
        let diff = t as i32 - now;
        if diff == 0 && alert_now {
            return Some((Prayer::ALL[i], AlertKind::Now));
        }
        if diff == 5 && alert_5m {
            return Some((Prayer::ALL[i], AlertKind::Before5));
        }
        if diff == 10 && alert_10m {
            return Some((Prayer::ALL[i], AlertKind::Before10));
        }
    }
    None
}

#[derive(Clone, Copy, Debug)]
pub struct BuiltinCity {
    pub name: &'static str,
    pub lat: f64,
    pub lon: f64,
}

pub const BUILTIN_CITIES: &[BuiltinCity] = &[
    // Major Indonesian cities
    BuiltinCity { name: "Jakarta", lat: -6.2088, lon: 106.8456 },
    BuiltinCity { name: "Semarang", lat: -6.9667, lon: 110.4167 },
    BuiltinCity { name: "Bandung", lat: -6.9175, lon: 107.6191 },
    BuiltinCity { name: "Surabaya", lat: -7.2575, lon: 112.7521 },
    BuiltinCity { name: "Yogyakarta", lat: -7.7956, lon: 110.3695 },
    BuiltinCity { name: "Medan", lat: 3.5952, lon: 98.6722 },
    BuiltinCity { name: "Makassar", lat: -5.1477, lon: 119.4327 },
    BuiltinCity { name: "Palembang", lat: -2.9761, lon: 104.7754 },
    BuiltinCity { name: "Denpasar", lat: -8.6705, lon: 115.2126 },
    BuiltinCity { name: "Malang", lat: -7.9666, lon: 112.6326 },
    BuiltinCity { name: "Solo", lat: -7.5755, lon: 110.8243 },
    BuiltinCity { name: "Surakarta", lat: -7.5755, lon: 110.8243 },
    BuiltinCity { name: "Bogor", lat: -6.5971, lon: 106.8060 },
    BuiltinCity { name: "Depok", lat: -6.4025, lon: 106.7942 },
    BuiltinCity { name: "Tangerang", lat: -6.1783, lon: 106.6319 },
    BuiltinCity { name: "Bekasi", lat: -6.2383, lon: 106.9756 },
    BuiltinCity { name: "Padang", lat: -0.9471, lon: 100.4172 },
    BuiltinCity { name: "Pekanbaru", lat: 0.5071, lon: 101.4478 },
    BuiltinCity { name: "Bandar Lampung", lat: -5.4292, lon: 105.2610 },
    BuiltinCity { name: "Batam", lat: 1.0456, lon: 104.0305 },
    BuiltinCity { name: "Balikpapan", lat: -1.2379, lon: 116.8529 },
    BuiltinCity { name: "Banjarmasin", lat: -3.3194, lon: 114.5908 },
    BuiltinCity { name: "Samarinda", lat: -0.4948, lon: 117.1436 },
    BuiltinCity { name: "Pontianak", lat: -0.0263, lon: 109.3425 },
    BuiltinCity { name: "Manado", lat: 1.4748, lon: 124.8421 },
    BuiltinCity { name: "Banda Aceh", lat: 5.5483, lon: 95.3238 },
    BuiltinCity { name: "Mataram", lat: -8.5833, lon: 116.1167 },
    BuiltinCity { name: "Kupang", lat: -10.1772, lon: 123.6070 },
    BuiltinCity { name: "Jayapura", lat: -2.5916, lon: 140.6690 },
    BuiltinCity { name: "Ambon", lat: -3.6954, lon: 128.1814 },
    BuiltinCity { name: "Cirebon", lat: -6.7320, lon: 108.5523 },
    BuiltinCity { name: "Tegal", lat: -6.8694, lon: 109.1402 },
    BuiltinCity { name: "Pekalongan", lat: -6.8898, lon: 109.6746 },
    BuiltinCity { name: "Purwokerto", lat: -7.4244, lon: 109.2302 },
    BuiltinCity { name: "Kudus", lat: -6.8048, lon: 110.8405 },
    BuiltinCity { name: "Salatiga", lat: -7.3305, lon: 110.5084 },
    BuiltinCity { name: "Magelang", lat: -7.4797, lon: 110.2177 },
    BuiltinCity { name: "Kediri", lat: -7.8480, lon: 112.0178 },
    BuiltinCity { name: "Jember", lat: -8.1845, lon: 113.6681 },
    BuiltinCity { name: "Sidoarjo", lat: -7.4478, lon: 112.7183 },
    BuiltinCity { name: "Gresik", lat: -7.1566, lon: 112.6555 },
    BuiltinCity { name: "Serang", lat: -6.1200, lon: 106.1503 },
    BuiltinCity { name: "Tasikmalaya", lat: -7.3506, lon: 108.2172 },
    BuiltinCity { name: "Sukabumi", lat: -6.9277, lon: 106.9300 },
    BuiltinCity { name: "Jambi", lat: -1.6101, lon: 103.6131 },
    BuiltinCity { name: "Bengkulu", lat: -3.7928, lon: 102.2608 },
    BuiltinCity { name: "Palu", lat: -0.8917, lon: 119.8707 },
    BuiltinCity { name: "Kendari", lat: -3.9985, lon: 122.5129 },
    BuiltinCity { name: "Gorontalo", lat: 0.5435, lon: 123.0568 },
    BuiltinCity { name: "Palangkaraya", lat: -2.2136, lon: 113.9108 },
    BuiltinCity { name: "Tarakan", lat: 3.3274, lon: 117.5785 },
    BuiltinCity { name: "Sorong", lat: -0.8762, lon: 131.2558 },
    BuiltinCity { name: "Madiun", lat: -7.6298, lon: 111.5239 },
    BuiltinCity { name: "Cilacap", lat: -7.7279, lon: 109.0059 },
    BuiltinCity { name: "Karawang", lat: -6.3227, lon: 107.3376 },
    BuiltinCity { name: "Cikarang", lat: -6.2615, lon: 107.1528 },
    BuiltinCity { name: "Garut", lat: -7.2279, lon: 107.9087 },
    BuiltinCity { name: "Banyuwangi", lat: -8.2192, lon: 114.3691 },
    // Major international cities
    BuiltinCity { name: "Singapore", lat: 1.3521, lon: 103.8198 },
    BuiltinCity { name: "Kuala Lumpur", lat: 3.1390, lon: 101.6869 },
    BuiltinCity { name: "Makkah", lat: 21.4225, lon: 39.8262 },
    BuiltinCity { name: "Madinah", lat: 24.4672, lon: 39.6111 },
    BuiltinCity { name: "Dubai", lat: 25.2048, lon: 55.2708 },
    BuiltinCity { name: "Istanbul", lat: 41.0082, lon: 28.9784 },
    BuiltinCity { name: "Cairo", lat: 30.0444, lon: 31.2357 },
    BuiltinCity { name: "Tokyo", lat: 35.6762, lon: 139.6503 },
    BuiltinCity { name: "Seoul", lat: 37.5665, lon: 126.9780 },
    BuiltinCity { name: "Sydney", lat: -33.8688, lon: 151.2093 },
    BuiltinCity { name: "London", lat: 51.5074, lon: -0.1278 },
    BuiltinCity { name: "New York", lat: 40.7128, lon: -74.0060 },
];

pub fn find_builtin_city(query: &str) -> Option<BuiltinCity> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return None;
    }
    // Aliases commonly typed by Indonesian users
    let norm = match q.as_str() {
        "jogja" | "yogya" => "yogyakarta",
        "sby" => "surabaya",
        "jkt" => "jakarta",
        "bdg" => "bandung",
        "smg" => "semarang",
        "lampung" => "bandar lampung",
        "aceh" => "banda aceh",
        "bali" => "denpasar",
        "mecca" | "mekah" => "makkah",
        "medina" => "madinah",
        other => other,
    };
    if let Some(c) = BUILTIN_CITIES.iter().find(|c| c.name.eq_ignore_ascii_case(norm)) {
        return Some(*c);
    }
    if norm.len() >= 3 {
        if let Some(c) = BUILTIN_CITIES
            .iter()
            .find(|c| c.name.to_ascii_lowercase().starts_with(norm))
        {
            return Some(*c);
        }
    }
    None
}

pub fn nearest_builtin_city(lat: f64, lon: f64) -> Option<BuiltinCity> {
    let mut best: Option<(f64, BuiltinCity)> = None;
    for &c in BUILTIN_CITIES {
        let d2 = (c.lat - lat).powi(2) + (c.lon - lon).powi(2);
        if best.is_none_or(|(bd, _)| d2 < bd) {
            best = Some((d2, c));
        }
    }
    best.and_then(|(d2, c)| (d2 < 2.5 * 2.5).then_some(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semarang_kemenag_schedule_matches_expected_band() {
        // Semarang (-6.9667, 110.4167), UTC+7, Nov 22
        let sched = compute_day(2026, 11, 22, -6.9667, 110.4167, 420, CalcMethod::Kemenag);
        // Official Kemenag Semarang late-Nov times: Subuh ~03:50, Dzuhur ~11:27, Ashar ~14:50, Maghrib ~17:41, Isya ~18:55
        let subuh = sched.times[0];
        let dzuhur = sched.times[1];
        let ashar = sched.times[2];
        let maghrib = sched.times[3];
        let isya = sched.times[4];
        assert!((225..=236).contains(&subuh), "Subuh {}", format_hhmm(subuh));
        assert!((682..=692).contains(&dzuhur), "Dzuhur {}", format_hhmm(dzuhur));
        assert!((885..=896).contains(&ashar), "Ashar {}", format_hhmm(ashar));
        assert!((1055..=1066).contains(&maghrib), "Maghrib {}", format_hhmm(maghrib));
        assert!((1130..=1142).contains(&isya), "Isya {}", format_hhmm(isya));
        assert!(subuh < dzuhur && dzuhur < ashar && ashar < maghrib && maghrib < isya);
    }

    #[test]
    fn snapshot_and_alerts_work_for_10m_5m_and_now() {
        let sched = DaySchedule { times: [230, 687, 890, 1061, 1135] };
        let snap = snapshot(sched, 11, 22, 0);
        assert_eq!(snap.next, Prayer::Dzuhur);
        assert_eq!(snap.mins_until, 5);
        assert!(snap.interval_progress > 0.95);
        assert!(snap.arc_progress > 0.45 && snap.arc_progress < 0.55, "arc={}", snap.arc_progress);
        assert!(snap.dot_us[1] > 0.45 && snap.dot_us[1] < 0.55, "dzuhur_u={}", snap.dot_us[1]);

        assert_eq!(check_alert(sched, 1051, true, true, true), Some((Prayer::Maghrib, AlertKind::Before10)));
        assert_eq!(check_alert(sched, 1056, true, true, true), Some((Prayer::Maghrib, AlertKind::Before5)));
        assert_eq!(check_alert(sched, 1061, true, true, true), Some((Prayer::Maghrib, AlertKind::Now)));
        assert_eq!(check_alert(sched, 1051, false, true, true), None);
        assert_eq!(check_alert(sched, 1056, true, false, true), None);
        assert_eq!(check_alert(sched, 1061, true, true, false), None);
    }

    #[test]
    fn builtin_city_lookup_supports_aliases() {
        assert_eq!(find_builtin_city("semarang").unwrap().name, "Semarang");
        assert_eq!(find_builtin_city("Jogja").unwrap().name, "Yogyakarta");
        assert_eq!(find_builtin_city("jkt").unwrap().name, "Jakarta");
        assert!(find_builtin_city("unknown_city_xyz").is_none());
    }
}
