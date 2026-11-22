use nacre_core::notch::lrclib::{self, Track};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};
use windows::Foundation::Uri;
use windows::Web::Http::HttpClient;
use windows_core::HSTRING;

const NONE_TTL: Duration = Duration::from_secs(3 * 24 * 3600);
const TIMEOUT: Duration = Duration::from_secs(6);

pub struct Request {
    pub track: Track,
    pub online: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Local,
    Lrclib,
    BetterLyrics,
    NetEase,
    LyricsOvh,
    None,
}

pub struct Answer {
    pub key: String,
    pub text: Option<String>,
    pub source: Source,
}

pub struct Lyrics {
    pub requests: Sender<Request>,
    pub answers: Receiver<Answer>,
}

fn file_safe(s: &str) -> String {
    s.chars().map(|c| if r#"\/:*?"<>|"#.contains(c) || c.is_control() { '_' } else { c }).collect::<String>().trim().to_string()
}

fn local_file(dir: &Path, t: &Track) -> Option<String> {
    let mut want = Vec::new();
    if !t.artist.is_empty() {
        want.push(format!("{} - {}.lrc", file_safe(&t.artist), file_safe(&t.title)).to_lowercase());
    }
    want.push(format!("{}.lrc", file_safe(&t.title)).to_lowercase());
    let names: Vec<PathBuf> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    for w in &want {
        if let Some(p) = names.iter().find(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_lowercase() == *w)) {
            if let Ok(s) = std::fs::read_to_string(p) {
                return Some(s);
            }
        }
    }
    None
}

fn finish(status: impl Fn() -> Option<i32>, cancel: impl Fn(), limit: Duration) -> bool {
    let start = Instant::now();
    loop {
        match status() {
            Some(1) => return true,
            Some(0) => {}
            _ => return false,
        }
        if start.elapsed() > limit {
            cancel();
            return false;
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

fn http_get(url: &str) -> Option<(i32, String)> {
    http_get_with(url, None)
}

fn http_get_with(url: &str, referer: Option<&str>) -> Option<(i32, String)> {
    let client = HttpClient::new().ok()?;
    if let Ok(h) = client.DefaultRequestHeaders() {
        if let Ok(ua) = h.UserAgent() {
            let ua_str = if referer.is_some() {
                "Mozilla/5.0"
            } else {
                concat!("NOX/", env!("CARGO_PKG_VERSION"))
            };
            let _ = ua.TryParseAdd(&HSTRING::from(ua_str));
        }
        if let Some(ref_url) = referer {
            if let Ok(uri) = Uri::CreateUri(&HSTRING::from(ref_url)) {
                let _ = h.SetReferer(&uri);
            }
        }
    }
    let op = client.GetAsync(&Uri::CreateUri(&HSTRING::from(url)).ok()?).ok()?;
    if !finish(|| op.Status().ok().map(|s| s.0), || drop(op.Cancel()), TIMEOUT) {
        return None;
    }
    let resp = op.GetResults().ok()?;
    let status = resp.StatusCode().ok()?.0;
    let read = resp.Content().ok()?.ReadAsStringAsync().ok()?;
    if !finish(|| read.Status().ok().map(|s| s.0), || drop(read.Cancel()), TIMEOUT) {
        return None;
    }
    Some((status, read.GetResults().ok()?.to_string()))
}

fn ask_lrclib(t: &Track) -> Result<Option<lrclib::Found>, ()> {
    let mut plain_fallback: Option<lrclib::Found> = None;
    let mut had_err = false;
    if let Some(url) = lrclib::get_url(t) {
        match http_get(&url) {
            Some((200, body)) => {
                if let Some(f) = lrclib::parse_get(&body) {
                    if f.synced.is_some() || f.instrumental {
                        return Ok(Some(f));
                    }
                    plain_fallback = Some(f);
                }
            }
            Some((404, _)) => {}
            _ => had_err = true,
        }
    }
    if let Some(url) = lrclib::search_url(t) {
        match http_get(&url) {
            Some((200, body)) => {
                if let Some(f) = lrclib::pick_search(&body, t.duration_s) {
                    if f.synced.is_some() || f.instrumental {
                        return Ok(Some(f));
                    }
                    if plain_fallback.is_none() {
                        plain_fallback = Some(f);
                    }
                }
            }
            Some((404, _)) => {}
            _ => had_err = true,
        }
    }
    if let Some(url) = lrclib::fuzzy_search_url(t) {
        match http_get(&url) {
            Some((200, body)) => {
                if let Some(f) = lrclib::pick_fuzzy_search(&body, t) {
                    if f.synced.is_some() || f.instrumental {
                        return Ok(Some(f));
                    }
                    if plain_fallback.is_none() {
                        plain_fallback = Some(f);
                    }
                }
            }
            Some((404, _)) => {}
            _ => had_err = true,
        }
    }
    if let Some(f) = plain_fallback {
        Ok(Some(f))
    } else if had_err {
        Err(())
    } else {
        Ok(None)
    }
}

fn ask_better_lyrics(t: &Track) -> Result<Option<lrclib::Found>, ()> {
    let Some(url) = lrclib::better_lyrics_url(t) else { return Ok(None) };
    match http_get(&url) {
        Some((200, body)) => Ok(lrclib::parse_better_lyrics(&body)),
        Some((404, _)) => Ok(None),
        _ => Err(()),
    }
}

fn ask_netease(t: &Track) -> Result<Option<lrclib::Found>, ()> {
    let Some(search_url) = lrclib::netease_search_url(t) else { return Ok(None) };
    let referer = Some("https://music.163.com/");
    let sid = match http_get_with(&search_url, referer) {
        Some((200, body)) => match lrclib::pick_netease_id(&body, t) {
            Some(id) => id,
            None => return Ok(None),
        },
        Some((404, _)) => return Ok(None),
        _ => return Err(()),
    };
    let lyric_url = lrclib::netease_lyric_url(sid);
    match http_get_with(&lyric_url, referer) {
        Some((200, body)) => Ok(lrclib::parse_netease_lyric(&body)),
        Some((404, _)) => Ok(None),
        _ => Err(()),
    }
}

fn ask_lyrics_ovh(t: &Track) -> Result<Option<lrclib::Found>, ()> {
    let Some(url) = lrclib::lyrics_ovh_url(t) else { return Ok(None) };
    match http_get(&url) {
        Some((200, body)) => Ok(lrclib::parse_lyrics_ovh(&body)),
        Some((404, _)) => Ok(None),
        _ => Err(()),
    }
}

fn cache_and_return(cache: &Path, hit: &Path, key: String, f: lrclib::Found, source: Source) -> Answer {
    let text = f.text();
    let _ = std::fs::create_dir_all(cache);
    let _ = std::fs::write(hit, &text);
    crate::log::line("lirik", &format!("{source:?}: ditemukan ({} byte)", text.len()));
    Answer { key, text: Some(text), source }
}

fn lookup(dir: &Path, req: &Request) -> Answer {
    let t = &req.track;
    let key = lrclib::cache_key(t);
    if let Some(s) = local_file(dir, t) {
        return Answer { key, text: Some(s), source: Source::Local };
    }
    let cache = dir.join("cache");
    let hit = cache.join(format!("{key}.lrc"));
    if let Ok(s) = std::fs::read_to_string(&hit) {
        return Answer { key, text: Some(s), source: Source::Lrclib };
    }
    // v2 negative cache so tracks previously missed by LRCLIB-only retry across backup providers.
    let none = cache.join(format!("{key}.none2"));
    let fresh_none = std::fs::metadata(&none)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < NONE_TTL);
    if fresh_none || !req.online {
        return Answer { key, text: None, source: Source::None };
    }

    let mut plain_backup: Option<(lrclib::Found, Source)> = None;
    let mut had_net_err = false;

    // 1. BetterLyrics (Primary — Apple Music synced TTML, ~170ms)
    match ask_better_lyrics(t) {
        Ok(Some(f)) if f.synced.is_some() => {
            return cache_and_return(&cache, &hit, key, f, Source::BetterLyrics);
        }
        Ok(Some(f)) => plain_backup = Some((f, Source::BetterLyrics)),
        Ok(None) => {}
        Err(()) => had_net_err = true,
    }

    // 2. LRCLIB (Backup #1 — exact -> structured search -> fuzzy search)
    match ask_lrclib(t) {
        Ok(Some(f)) if f.synced.is_some() || f.instrumental => {
            return cache_and_return(&cache, &hit, key, f, Source::Lrclib);
        }
        Ok(Some(f)) => {
            if plain_backup.is_none() {
                plain_backup = Some((f, Source::Lrclib));
            }
        }
        Ok(None) => {}
        Err(()) => had_net_err = true,
    }

    // 3. NetEase Cloud Music (Backup #2 — synced LRC, strong Asian/Indie/Global catalog)
    match ask_netease(t) {
        Ok(Some(f)) if f.synced.is_some() => {
            return cache_and_return(&cache, &hit, key, f, Source::NetEase);
        }
        Ok(Some(f)) => {
            if plain_backup.is_none() {
                plain_backup = Some((f, Source::NetEase));
            }
        }
        Ok(None) => {}
        Err(()) => had_net_err = true,
    }

    // 4. Plain-text fallback: use earlier plain hit if any, otherwise query Lyrics.ovh
    if let Some((f, src)) = plain_backup {
        return cache_and_return(&cache, &hit, key, f, src);
    }
    match ask_lyrics_ovh(t) {
        Ok(Some(f)) => return cache_and_return(&cache, &hit, key, f, Source::LyricsOvh),
        Ok(None) => {}
        Err(()) => had_net_err = true,
    }

    if !had_net_err {
        let _ = std::fs::create_dir_all(&cache);
        let _ = std::fs::write(&none, b"");
        crate::log::line("lirik", "semua sumber: tidak ada");
    } else {
        crate::log::line("lirik", "pencarian lirik gagal (jaringan/batas waktu)");
    }
    Answer { key, text: None, source: Source::None }
}

pub fn spawn(dir: PathBuf, notify: impl Fn() + Send + 'static) -> Lyrics {
    let (req_tx, req_rx) = channel::<Request>();
    let (ans_tx, ans_rx) = channel::<Answer>();
    let _ = std::fs::create_dir_all(&dir);
    std::thread::spawn(move || {
        let com = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED).is_ok() };
        while let Ok(mut req) = req_rx.recv() {
            while let Ok(newer) = req_rx.try_recv() {
                req = newer;
            }
            let a = lookup(&dir, &req);
            if ans_tx.send(a).is_err() {
                break;
            }
            notify();
        }
        if com {
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    });
    Lyrics { requests: req_tx, answers: ans_rx }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_lrc_is_found_case_insensitively_and_names_are_file_safe() {
        let dir = std::env::temp_dir().join(format!("abra-lyrics-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join("coldplay - clocks.LRC"), "[00:01.00]Lights go out").unwrap();
        let t = Track { title: "Clocks".into(), artist: "Coldplay".into(), album: String::new(), duration_s: None };
        assert_eq!(local_file(&dir, &t).as_deref(), Some("[00:01.00]Lights go out"));
        let other = Track { title: "AC/DC: T.N.T?".into(), ..t.clone() };
        assert_eq!(local_file(&dir, &other), None);
        assert_eq!(file_safe("AC/DC: T.N.T?"), "AC_DC_ T.N.T_");
        let a = lookup(&dir, &Request { track: t, online: false });
        assert_eq!(a.source, Source::Local);
        let none = lookup(&dir, &Request { track: other, online: false });
        assert_eq!((none.text, none.source), (None, Source::None), "luring: tidak menyentuh jaringan");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
