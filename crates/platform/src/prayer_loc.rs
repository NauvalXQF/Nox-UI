use nacre_core::notch::json::parse as parse_json;
use nacre_core::notch::prayer::{find_builtin_city, nearest_builtin_city};
use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator};

#[derive(Clone, Debug)]
pub struct GeoResult {
    pub city: String,
    pub lat: f64,
    pub lon: f64,
    pub source: &'static str,
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Searches for a city name: checks offline built-in directory first (0ms),
/// then falls back to Open-Meteo Geocoding API (free, no API key required).
pub fn search_city(query: &str) -> Result<GeoResult, String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("Nama kota kosong".into());
    }
    if let Some(c) = find_builtin_city(q) {
        return Ok(GeoResult {
            city: c.name.to_string(),
            lat: c.lat,
            lon: c.lon,
            source: "offline",
        });
    }

    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=id&format=json",
        url_encode(q)
    );
    let (status, body) = crate::http::request_with_timeout("GET", &url, &[], None, 10_000)?;
    if !(200..300).contains(&status) {
        return Err(format!("Pencarian kota gagal (HTTP {status})"));
    }
    let root = parse_json(&body).ok_or_else(|| "Respons pencarian kota tidak valid".to_string())?;
    let first = root
        .get("results")
        .and_then(|v| v.as_arr())
        .and_then(|a| a.first())
        .ok_or_else(|| format!("Kota \"{q}\" tidak ditemukan"))?;
    let name = first.get("name").and_then(|v| v.as_str()).unwrap_or(q).trim();
    let lat = first.get("latitude").and_then(|v| v.as_f64()).ok_or("Lintang tidak ditemukan")?;
    let lon = first.get("longitude").and_then(|v| v.as_f64()).ok_or("Bujur tidak ditemukan")?;
    Ok(GeoResult {
        city: name.to_string(),
        lat,
        lon,
        source: "geocoding",
    })
}

fn reverse_geocode_city(lat: f64, lon: f64) -> String {
    let url = format!(
        "https://api.bigdatacloud.net/data/reverse-geocode-client?latitude={lat:.5}&longitude={lon:.5}&localityLanguage=id"
    );
    if let Ok((status, body)) = crate::http::request_with_timeout("GET", &url, &[], None, 8_000) {
        if (200..300).contains(&status) {
            if let Some(j) = parse_json(&body) {
                for key in ["city", "locality", "principalSubdivision"] {
                    if let Some(s) = j.get(key).and_then(|v| v.as_str()) {
                        let clean = s
                            .trim()
                            .trim_start_matches("Kota ")
                            .trim_start_matches("Kabupaten ")
                            .trim();
                        if !clean.is_empty() {
                            return clean.to_string();
                        }
                    }
                }
            }
        }
    }
    if let Some(c) = nearest_builtin_city(lat, lon) {
        return c.name.to_string();
    }
    format!("{lat:.2}, {lon:.2}")
}

fn detect_windows_location() -> Result<GeoResult, String> {
    let access = Geolocator::RequestAccessAsync()
        .map_err(|e| format!("Windows Location tidak tersedia: {e}"))?
        .join()
        .map_err(|e| format!("Gagal meminta izin lokasi: {e}"))?;
    if access != GeolocationAccessStatus::Allowed {
        return Err("Izin lokasi Windows ditolak".into());
    }
    let locator = Geolocator::new().map_err(|e| format!("Geolocator gagal: {e}"))?;
    let pos = locator
        .GetGeopositionAsync()
        .map_err(|e| format!("GetGeopositionAsync gagal: {e}"))?
        .join()
        .map_err(|e| format!("Gagal membaca koordinat Windows: {e}"))?;
    let coord = pos
        .Coordinate()
        .and_then(|c| c.Point())
        .and_then(|p| p.Position())
        .map_err(|e| format!("Koordinat kosong: {e}"))?;
    let lat = coord.Latitude;
    let lon = coord.Longitude;
    let city = reverse_geocode_city(lat, lon);
    Ok(GeoResult { city, lat, lon, source: "windows" })
}

fn detect_ip_location() -> Result<GeoResult, String> {
    let (status, body) = crate::http::request_with_timeout("GET", "https://ipwho.is/", &[], None, 10_000)?;
    if !(200..300).contains(&status) {
        return Err(format!("Deteksi lokasi IP gagal (HTTP {status})"));
    }
    let j = parse_json(&body).ok_or_else(|| "Respons lokasi IP tidak valid".to_string())?;
    if j.get("success").and_then(|v| v.as_bool()) == Some(false) {
        return Err("Layanan lokasi IP tidak tersedia".into());
    }
    let lat = j.get("latitude").and_then(|v| v.as_f64()).ok_or("Lintang IP kosong")?;
    let lon = j.get("longitude").and_then(|v| v.as_f64()).ok_or("Bujur IP kosong")?;
    let city = j
        .get("city")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| reverse_geocode_city(lat, lon));
    Ok(GeoResult { city, lat, lon, source: "ip" })
}

/// Detects the user's city & coordinates.
/// When `use_windows_location` is true, tries native Windows Location API first and falls back to IP geolocation;
/// when false, uses IP geolocation directly without requiring Windows Location permission.
pub fn detect_location(use_windows_location: bool) -> Result<GeoResult, String> {
    if use_windows_location {
        if let Ok(res) = detect_windows_location() {
            return Ok(res);
        }
    }
    detect_ip_location()
}
