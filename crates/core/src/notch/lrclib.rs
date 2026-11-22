use super::json::{self, Json};

pub const API: &str = "https://lrclib.net/api";
pub const BETTER_LYRICS_API: &str = "https://lyrics-api.boidu.dev/getLyrics";
pub const NETEASE_SEARCH_API: &str = "https://music.163.com/api/search/get";
pub const NETEASE_LYRIC_API: &str = "https://music.163.com/api/song/lyric";
pub const LYRICS_OVH_API: &str = "https://api.lyrics.ovh/v1";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_s: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub synced: Option<String>,
    pub plain: Option<String>,
    pub instrumental: bool,
}

impl Found {
    pub fn text(&self) -> String {
        if self.instrumental {
            return String::new();
        }
        self.synced.clone().filter(|s| !s.trim().is_empty()).or_else(|| self.plain.clone()).unwrap_or_default()
    }
}

pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

const NOISE: [&str; 16] = [
    "official", "video", "audio", "lyric", "lyrics", "visualizer", "visualiser", "mv", "m/v", "hd", "4k", "remaster", "remastered",
    "explicit", "clean", "color coded",
];

fn is_noise(inner: &str) -> bool {
    let l = inner.to_lowercase();
    NOISE.iter().any(|w| l.split(|c: char| !c.is_alphanumeric() && c != '/').any(|t| t == *w) || (w.contains(' ') && l.contains(w)))
}

pub fn clean(title: &str, artist: &str) -> (String, String) {
    let mut artist = artist.trim().to_string();
    for suffix in [" - Topic", "VEVO"] {
        if artist.len() > suffix.len() && artist.to_lowercase().ends_with(&suffix.to_lowercase()) {
            artist.truncate(artist.len() - suffix.len());
            artist = artist.trim().to_string();
        }
    }
    let mut t = String::new();
    let mut rest = title.trim();
    while let Some(i) = rest.find(['(', '[']) {
        let close = if rest.as_bytes()[i] == b'(' { ')' } else { ']' };
        match rest[i + 1..].find(close) {
            Some(j) if is_noise(&rest[i + 1..i + 1 + j]) => {
                t.push_str(&rest[..i]);
                rest = &rest[i + 2 + j..];
            }
            _ => {
                t.push_str(&rest[..=i]);
                rest = &rest[i + 1..];
            }
        }
    }
    t.push_str(rest);
    let mut title = t.split_whitespace().collect::<Vec<_>>().join(" ");
    for sep in [" - ", " – ", " — "] {
        if let Some((left, right)) = title.split_once(sep) {
            let (l, r) = (left.trim().to_string(), right.trim().to_string());
            if artist.is_empty() {
                artist = l;
                title = r;
            } else if l.eq_ignore_ascii_case(&artist) {
                title = r;
            }
            break;
        }
    }
    for sep in [" - ", " – "] {
        if let Some((left, right)) = title.split_once(sep) {
            if is_noise(right) {
                title = left.trim().to_string();
            }
        }
    }
    (title, artist)
}

fn query(t: &Track, with_album: bool) -> String {
    let mut q = format!("track_name={}&artist_name={}", encode(&t.title), encode(&t.artist));
    if with_album && !t.album.trim().is_empty() {
        q.push_str(&format!("&album_name={}", encode(&t.album)));
    }
    if let Some(d) = t.duration_s {
        q.push_str(&format!("&duration={d}"));
    }
    q
}

pub fn get_url(t: &Track) -> Option<String> {
    (!t.title.is_empty() && !t.artist.is_empty() && !t.album.trim().is_empty() && t.duration_s.is_some())
        .then(|| format!("{API}/get?{}", query(t, true)))
}

pub fn search_url(t: &Track) -> Option<String> {
    (!t.title.is_empty()).then(|| {
        let mut q = format!("track_name={}", encode(&t.title));
        if !t.artist.is_empty() {
            q.push_str(&format!("&artist_name={}", encode(&t.artist)));
        }
        format!("{API}/search?{q}")
    })
}

/// Fuzzy `q=` search on LRCLIB when strict `track_name` + `artist_name` misses
/// (e.g., differing artist credits or `"Artist - Title"` entries).
pub fn fuzzy_search_url(t: &Track) -> Option<String> {
    if t.title.is_empty() || t.artist.is_empty() {
        return None;
    }
    let q = format!("{} {}", t.artist, t.title);
    Some(format!("{API}/search?q={}", encode(&q)))
}

fn token_overlap(a: &str, b: &str) -> bool {
    let norm = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() >= 2)
            .map(str::to_string)
            .collect()
    };
    let wa = norm(a);
    let wb = norm(b);
    if wa.is_empty() || wb.is_empty() {
        return true;
    }
    wa.iter().any(|x| wb.iter().any(|y| y == x))
}

pub fn pick_fuzzy_search(body: &str, t: &Track) -> Option<Found> {
    let j = json::parse(body)?;
    let mut best: Option<(Found, (bool, bool, i64))> = None;
    for o in j.as_arr()? {
        let Some((f, d)) = found(o) else { continue };
        let cand_title = o.get("trackName").and_then(Json::as_str).unwrap_or("");
        let cand_artist = o.get("artistName").and_then(Json::as_str).unwrap_or("");
        if !token_overlap(&t.title, cand_title) {
            continue;
        }
        let artist_ok = t.artist.is_empty()
            || token_overlap(&t.artist, cand_artist)
            || token_overlap(&t.artist, cand_title);
        if !artist_ok {
            continue;
        }
        let diff = match (t.duration_s, d) {
            (Some(want), Some(have)) => {
                let x = (have - want as f64).abs();
                if x > 8.0 {
                    continue;
                }
                (x * 1000.0) as i64
            }
            _ => 0,
        };
        let key = (f.synced.is_none(), !token_overlap(&t.artist, cand_artist), diff);
        if best.as_ref().is_none_or(|(_, k)| key < *k) {
            best = Some((f, key));
        }
    }
    best.map(|(f, _)| f)
}

/// BetterLyrics public Apple Music TTML endpoint (synced lyrics fallback #1).
pub fn better_lyrics_url(t: &Track) -> Option<String> {
    if t.title.is_empty() || t.artist.is_empty() {
        return None;
    }
    let mut u = format!("{BETTER_LYRICS_API}?s={}&a={}", encode(&t.title), encode(&t.artist));
    if let Some(d) = t.duration_s {
        u.push_str(&format!("&d={d}"));
    }
    Some(u)
}

fn parse_ttml_time_ms(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(ms) = s.strip_suffix("ms") {
        return ms.trim().parse::<f64>().ok().map(|v| v.round() as i64);
    }
    let s = s.strip_suffix('s').unwrap_or(s);
    let parts: Vec<&str> = s.split(':').collect();
    let secs = match parts.as_slice() {
        [sec] => sec.parse::<f64>().ok()?,
        [min, sec] => min.parse::<f64>().ok()? * 60.0 + sec.parse::<f64>().ok()?,
        [hr, min, sec] => hr.parse::<f64>().ok()? * 3600.0 + min.parse::<f64>().ok()? * 60.0 + sec.parse::<f64>().ok()?,
        _ => return None,
    };
    if !secs.is_finite() || secs < 0.0 {
        return None;
    }
    Some((secs * 1000.0).round() as i64)
}

fn decode_xml_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        if let Some(semi) = after.find(';') {
            let ent = &after[..semi];
            let decoded = match ent {
                "amp" => Some('&'),
                "apos" => Some('\''),
                "quot" => Some('"'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "nbsp" => Some(' '),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                    u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32)
                }
                _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
                _ => None,
            };
            if let Some(ch) = decoded {
                out.push(ch);
                rest = &after[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    out
}

fn strip_xml_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    let mut tag_buf = String::new();
    for ch in s.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag_buf.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let t = tag_buf.trim().to_ascii_lowercase();
                if t.starts_with("br") {
                    out.push(' ');
                }
            }
            _ if in_tag => tag_buf.push(ch),
            _ => out.push(ch),
        }
    }
    decode_xml_entities(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let pat = format!("{name}=\"");
    let start = attrs.find(&pat)? + pat.len();
    let end = attrs[start..].find('"')?;
    Some(&attrs[start..start + end])
}

/// Converts a BetterLyrics JSON response (`{"ttml": "<tt ...>..."}`) into synced LRC.
pub fn parse_better_lyrics(body: &str) -> Option<Found> {
    let j = json::parse(body)?;
    let ttml = j.get("ttml").and_then(Json::as_str)?;
    let scope = match (ttml.find("<body"), ttml.rfind("</body>")) {
        (Some(a), Some(b)) if b > a => &ttml[a..b],
        _ => ttml,
    };
    let mut timed = Vec::new();
    let mut plain = Vec::new();
    let mut rest = scope;
    while let Some(p_idx) = rest.find("<p") {
        let after_p = &rest[p_idx + 2..];
        let Some(first_ch) = after_p.chars().next() else { break };
        if !first_ch.is_whitespace() && first_ch != '>' {
            rest = after_p;
            continue;
        }
        let Some(gt) = after_p.find('>') else { break };
        let attrs = &after_p[..gt];
        let body_start = &after_p[gt + 1..];
        let Some(close_p) = body_start.find("</p>") else { break };
        let inner = &body_start[..close_p];
        rest = &body_start[close_p + 4..];

        let text = strip_xml_tags(inner);
        if text.is_empty() {
            continue;
        }
        if let Some(ms) = extract_attr(attrs, "begin").and_then(parse_ttml_time_ms) {
            let min = ms / 60_000;
            let sec = (ms % 60_000) / 1000;
            let cs = (ms % 1000) / 10;
            timed.push(format!("[{min:02}:{sec:02}.{cs:02}]{text}"));
        } else {
            plain.push(text);
        }
    }
    if !timed.is_empty() {
        Some(Found { synced: Some(timed.join("\n")), plain: None, instrumental: false })
    } else if !plain.is_empty() {
        Some(Found { synced: None, plain: Some(plain.join("\n")), instrumental: false })
    } else {
        None
    }
}

/// NetEase Cloud Music public search URL (synced lyrics fallback #2 — strong Asian & global catalog).
pub fn netease_search_url(t: &Track) -> Option<String> {
    if t.title.is_empty() {
        return None;
    }
    let q = if t.artist.is_empty() {
        t.title.clone()
    } else {
        format!("{} {}", t.artist, t.title)
    };
    Some(format!("{NETEASE_SEARCH_API}?type=1&limit=5&s={}", encode(&q)))
}

pub fn netease_lyric_url(id: u64) -> String {
    format!("{NETEASE_LYRIC_API}?id={id}&lv=1")
}

pub fn pick_netease_id(body: &str, t: &Track) -> Option<u64> {
    let j = json::parse(body)?;
    let songs = j.get("result")?.get("songs")?.as_arr()?;
    let mut best: Option<(u64, (bool, i64))> = None;
    for s in songs {
        let Some(id) = s.get("id").and_then(Json::as_f64).map(|v| v as u64) else { continue };
        let name = s.get("name").and_then(Json::as_str).unwrap_or("");
        if !token_overlap(&t.title, name) {
            continue;
        }
        let artist_match = if t.artist.is_empty() {
            true
        } else {
            s.get("artists")
                .and_then(Json::as_arr)
                .is_some_and(|arr| arr.iter().any(|a| a.get("name").and_then(Json::as_str).is_some_and(|n| token_overlap(&t.artist, n))))
        };
        let dur_s = s.get("duration").and_then(Json::as_f64).map(|ms| ms / 1000.0);
        let diff = match (t.duration_s, dur_s) {
            (Some(want), Some(have)) => {
                let x = (have - want as f64).abs();
                if x > 8.0 {
                    continue;
                }
                (x * 1000.0) as i64
            }
            _ => 0,
        };
        let key = (!artist_match, diff);
        if best.as_ref().is_none_or(|(_, k)| key < *k) {
            best = Some((id, key));
        }
    }
    best.map(|(id, _)| id)
}

fn is_netease_credit_line(line: &str) -> bool {
    let text = match line.rfind(']') {
        Some(i) => line[i + 1..].trim(),
        None => line.trim(),
    };
    const PREFIXES: [&str; 10] = [
        "作词", "作曲", "编曲", "制作人", "词：", "曲：", "词:", "曲:", "纯音乐", "Produced by",
    ];
    PREFIXES.iter().any(|p| text.starts_with(p))
}

pub fn parse_netease_lyric(body: &str) -> Option<Found> {
    let j = json::parse(body)?;
    let raw = j.get("lrc")?.get("lyric").and_then(Json::as_str)?.trim();
    if raw.is_empty() {
        return None;
    }
    let cleaned: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !is_netease_credit_line(l))
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    let joined = cleaned.join("\n");
    let has_sync = cleaned.iter().any(|l| {
        l.starts_with('[')
            && l.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
            && l.contains(':')
            && l.contains(']')
    });
    if has_sync {
        Some(Found { synced: Some(joined), plain: None, instrumental: false })
    } else {
        Some(Found { synced: None, plain: Some(joined), instrumental: false })
    }
}

/// Lyrics.ovh public plain-lyrics fallback (used only if all synced sources fail).
pub fn lyrics_ovh_url(t: &Track) -> Option<String> {
    if t.title.is_empty() || t.artist.is_empty() {
        return None;
    }
    Some(format!("{LYRICS_OVH_API}/{}/{}", encode(&t.artist), encode(&t.title)))
}

pub fn parse_lyrics_ovh(body: &str) -> Option<Found> {
    let j = json::parse(body)?;
    let raw = j.get("lyrics").and_then(Json::as_str)?;
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.lines().peekable();
    if lines.peek().is_some_and(|l| l.trim_start().starts_with("Paroles de la chanson ")) {
        lines.next();
    }
    let text = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(Found { synced: None, plain: Some(text), instrumental: false })
}

fn found(o: &Json) -> Option<(Found, Option<f64>)> {
    if !matches!(o, Json::Obj(_)) {
        return None;
    }
    let s = |k: &str| o.get(k).and_then(Json::as_str).map(str::to_string).filter(|s| !s.trim().is_empty());
    let f = Found { synced: s("syncedLyrics"), plain: s("plainLyrics"), instrumental: o.get("instrumental").and_then(Json::as_bool).unwrap_or(false) };
    if f.synced.is_none() && f.plain.is_none() && !f.instrumental {
        return None;
    }
    Some((f, o.get("duration").and_then(Json::as_f64)))
}

pub fn parse_get(body: &str) -> Option<Found> {
    found(&json::parse(body)?).map(|(f, _)| f)
}

pub fn pick_search(body: &str, duration_s: Option<u32>) -> Option<Found> {
    let j = json::parse(body)?;
    let mut best: Option<(Found, (bool, i64))> = None;
    for o in j.as_arr()? {
        let Some((f, d)) = found(o) else { continue };
        let diff = match (duration_s, d) {
            (Some(want), Some(have)) => {
                let x = (have - want as f64).abs();
                if x > 8.0 {
                    continue;
                }
                (x * 1000.0) as i64
            }
            _ => 0,
        };
        let key = (f.synced.is_none(), diff);
        if best.as_ref().is_none_or(|(_, k)| key < *k) {
            best = Some((f, key));
        }
    }
    best.map(|(f, _)| f)
}

pub fn cache_key(t: &Track) -> String {
    let norm = format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}",
        t.artist.trim().to_lowercase(),
        t.title.trim().to_lowercase(),
        t.album.trim().to_lowercase(),
        t.duration_s.unwrap_or(0)
    );
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in norm.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(title: &str, artist: &str, album: &str, d: Option<u32>) -> Track {
        Track { title: title.into(), artist: artist.into(), album: album.into(), duration_s: d }
    }

    #[test]
    fn encodes_like_rfc3986() {
        assert_eq!(encode("Rock & Roll/é ~ok"), "Rock%20%26%20Roll%2F%C3%A9%20~ok");
    }

    #[test]
    fn urls_include_only_what_is_known() {
        let t = tr("Clocks", "Coldplay", "A Rush of Blood to the Head", Some(307));
        assert_eq!(
            get_url(&t).unwrap(),
            "https://lrclib.net/api/get?track_name=Clocks&artist_name=Coldplay&album_name=A%20Rush%20of%20Blood%20to%20the%20Head&duration=307"
        );
        assert_eq!(get_url(&tr("Clocks", "Coldplay", "", Some(307))), None, "tanpa album: pakai pencarian");
        assert_eq!(search_url(&tr("Clocks", "Coldplay", "", None)).unwrap(), "https://lrclib.net/api/search?track_name=Clocks&artist_name=Coldplay");
        assert_eq!(search_url(&tr("", "Coldplay", "", None)), None);
    }

    #[test]
    fn cleans_streaming_titles() {
        assert_eq!(clean("Coldplay - Clocks (Official Video)", ""), ("Clocks".into(), "Coldplay".into()));
        assert_eq!(clean("Coldplay - Clocks [Lyrics]", "Coldplay"), ("Clocks".into(), "Coldplay".into()));
        assert_eq!(clean("Clocks", "Coldplay - Topic"), ("Clocks".into(), "Coldplay".into()));
        assert_eq!(clean("Clocks", "ColdplayVEVO"), ("Clocks".into(), "Coldplay".into()));
        assert_eq!(clean("Here Comes the Sun - Remastered 2009", "The Beatles"), ("Here Comes the Sun".into(), "The Beatles".into()));
        assert_eq!(clean("Stay (feat. Justin Bieber)", "The Kid LAROI"), ("Stay (feat. Justin Bieber)".into(), "The Kid LAROI".into()));
        assert_eq!(clean("Anti-Hero", "Taylor Swift"), ("Anti-Hero".into(), "Taylor Swift".into()));
    }

    #[test]
    fn reads_get_and_picks_the_best_search_result() {
        let one = r#"{"id":1,"trackName":"Clocks","duration":307.0,"instrumental":false,"plainLyrics":"Lights go out","syncedLyrics":"[00:10.00]Lights go out"}"#;
        let f = parse_get(one).unwrap();
        assert_eq!(f.text(), "[00:10.00]Lights go out");
        let many = r#"[
            {"duration":200.0,"plainLyrics":"wrong song","syncedLyrics":"[00:01.00]wrong"},
            {"duration":309.0,"plainLyrics":"plain only","syncedLyrics":null},
            {"duration":306.0,"plainLyrics":"p","syncedLyrics":"[00:02.00]synced"},
            {"duration":307.0,"plainLyrics":null,"syncedLyrics":null,"instrumental":false}
        ]"#;
        assert_eq!(pick_search(many, Some(307)).unwrap().text(), "[00:02.00]synced");
        assert_eq!(pick_search(r#"[{"duration":200.0,"plainLyrics":"x"}]"#, Some(307)), None, "durasi terlalu jauh");
        let inst = parse_get(r#"{"instrumental":true,"plainLyrics":null,"syncedLyrics":null}"#).unwrap();
        assert_eq!(inst.text(), "");
        assert_eq!(parse_get(r#"{"code":404,"name":"TrackNotFound"}"#), None);
    }

    #[test]
    fn cache_key_is_stable_and_case_insensitive() {
        let a = cache_key(&tr("Clocks", "Coldplay", "X", Some(307)));
        assert_eq!(a, cache_key(&tr(" clocks ", "COLDPLAY", "x", Some(307))));
        assert_ne!(a, cache_key(&tr("Clocks", "Coldplay", "X", Some(308))));
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn parses_better_lyrics_ttml_with_syllables_entities_and_timestamps() {
        let body = r#"{"ttml":"<tt><head><metadata><songwriters><songwriter>Tulus</songwriter></songwriters></metadata></head><body><div><p begin=\"13.274\" end=\"17.779\"><span begin=\"13.274\">Perja</span><span begin=\"13.674\">la</span><span begin=\"15.347\">nan</span> <span begin=\"15.787\">mem</span><span>ba</span><span>wa</span><span>mu</span></p><p begin=\"3:10.363\" end=\"3:15.000\">You &amp; I&#39;m home</p></div></body></tt>"}"#;
        let f = parse_better_lyrics(body).unwrap();
        assert_eq!(
            f.synced.as_deref(),
            Some("[00:13.27]Perjalanan membawamu\n[03:10.36]You & I'm home")
        );
        let parsed = crate::notch::lyrics::parse(&f.text());
        assert!(parsed.synced);
        assert_eq!(parsed.lines.len(), 2);
        assert_eq!(parsed.lines[0].text, "Perjalanan membawamu");
        assert_eq!(parsed.lines[1].text, "You & I'm home");
    }

    #[test]
    fn parses_netease_and_strips_credit_lines() {
        let search = r#"{"result":{"songs":[{"id":1582613,"name":"Yellow","duration":266773,"artists":[{"name":"Coldplay"}]},{"id":999,"name":"Yellow Remix","duration":400000,"artists":[{"name":"Other"}]}]}}"#;
        let t = tr("Yellow", "Coldplay", "", Some(267));
        assert_eq!(pick_netease_id(search, &t), Some(1582613));

        let lyric = r#"{"lrc":{"lyric":"[00:00.000] 作词 : Chris Martin\n[00:01.000] 作曲 : Chris Martin\n[00:33.790]Look at the stars\n[00:36.200]Look how they shine for you"}}"#;
        let f = parse_netease_lyric(lyric).unwrap();
        assert_eq!(
            f.synced.as_deref(),
            Some("[00:33.790]Look at the stars\n[00:36.200]Look how they shine for you")
        );
    }

    #[test]
    fn parses_lyrics_ovh_plain_and_fuzzy_lrclib() {
        let ovh = r#"{"lyrics":"Paroles de la chanson Yellow par Coldplay\r\nLook at the stars\r\nLook how they shine"}"#;
        let f = parse_lyrics_ovh(ovh).unwrap();
        assert!(f.synced.is_none());
        assert_eq!(f.plain.as_deref(), Some("Look at the stars\nLook how they shine"));

        let fuzzy = r#"[{"trackName":"TULUS - Hati-Hati di Jalan","artistName":"Tulus","duration":242.0,"syncedLyrics":"[00:13.27]Perjalanan membawamu"},{"trackName":"Unrelated Song","artistName":"Other","duration":242.0,"syncedLyrics":"[00:01.00]Wrong"}]"#;
        let hit = pick_fuzzy_search(fuzzy, &tr("Hati-Hati di Jalan", "Tulus", "", Some(242))).unwrap();
        assert_eq!(hit.synced.as_deref(), Some("[00:13.27]Perjalanan membawamu"));
    }
}
