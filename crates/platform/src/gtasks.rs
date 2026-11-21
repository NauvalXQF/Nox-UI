use crate::{cred, http};
use nacre_core::notch::json::{self, Json};
use nacre_core::notch::todo::{push_json_str, RemoteTask};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static TOKEN_LOCK: Mutex<()> = Mutex::new(());

/// Returns a valid Google OAuth Bearer access token (refreshing if expired) shared across
/// Google Tasks and Google Calendar read-only sync.
pub fn access_token() -> Option<String> {
    let _g = TOKEN_LOCK.lock().ok()?;
    let mut t = load_tokens()?;
    ensure_access_token(&mut t).ok()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SyncStatus {
    Disconnected = 0,
    NeedsClientConfig = 1,
    Connecting = 2,
    Syncing = 3,
    Synced = 4,
    Error = 5,
}

impl SyncStatus {
    fn from_u8(v: u8) -> Self {
        match v {
            1 => SyncStatus::NeedsClientConfig,
            2 => SyncStatus::Connecting,
            3 => SyncStatus::Syncing,
            4 => SyncStatus::Synced,
            5 => SyncStatus::Error,
            _ => SyncStatus::Disconnected,
        }
    }
}

static STATUS: AtomicU8 = AtomicU8::new(SyncStatus::Disconnected as u8);

pub fn status() -> SyncStatus {
    SyncStatus::from_u8(STATUS.load(Ordering::Relaxed))
}

fn set_status(s: SyncStatus) {
    STATUS.store(s as u8, Ordering::Relaxed);
}

pub enum Cmd {
    Connect,
    Disconnect,
    SyncNow,
    Add { local_id: String, title: String },
    Toggle { local_id: String, remote_id: Option<String>, completed: bool },
    Delete { local_id: String, remote_id: Option<String> },
}

pub enum Event {
    Status(SyncStatus),
    Synced(Vec<RemoteTask>),
    Created { local_id: String, remote_id: String },
    MutationDone { local_id: String },
}

pub struct GTasks {
    pub commands: Sender<Cmd>,
    pub events: Receiver<Event>,
}

#[derive(Clone, Debug)]
struct Tokens {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
}

fn now_s() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn client_creds() -> Option<(String, String)> {
    let cid = cred::read_secret(cred::TARGET_GTASKS_CLIENT_ID)
        .or_else(|| std::env::var("ABRA_GOOGLE_CLIENT_ID").ok())
        .filter(|s| !s.trim().is_empty())?;
    let csec = cred::read_secret(cred::TARGET_GTASKS_CLIENT_SECRET)
        .or_else(|| std::env::var("ABRA_GOOGLE_CLIENT_SECRET").ok())
        .unwrap_or_default();
    Some((cid, csec))
}

fn load_tokens() -> Option<Tokens> {
    let raw = cred::read_secret(cred::TARGET_GTASKS_TOKENS)?;
    let j = json::parse(&raw)?;
    Some(Tokens {
        access_token: j.get("access_token")?.as_str()?.to_string(),
        refresh_token: j.get("refresh_token").and_then(Json::as_str).unwrap_or("").to_string(),
        expires_at: j.get("expires_at").and_then(Json::as_f64).unwrap_or(0.0) as i64,
    })
}

fn save_tokens(t: &Tokens) {
    let mut s = String::from("{\"access_token\":");
    push_json_str(&mut s, &t.access_token);
    s.push_str(",\"refresh_token\":");
    push_json_str(&mut s, &t.refresh_token);
    s.push_str(&format!(",\"expires_at\":{}}}", t.expires_at));
    let _ = cred::write_secret(cred::TARGET_GTASKS_TOKENS, &s);
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

fn base64url(bytes: &[u8]) -> String {
    const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8) | (bytes[i + 2] as u32);
        out.push(TBL[((n >> 18) & 63) as usize] as char);
        out.push(TBL[((n >> 12) & 63) as usize] as char);
        out.push(TBL[((n >> 6) & 63) as usize] as char);
        out.push(TBL[(n & 63) as usize] as char);
        i += 3;
    }
    let rem = bytes.len() - i;
    if rem == 1 {
        let n = (bytes[i] as u32) << 16;
        out.push(TBL[((n >> 18) & 63) as usize] as char);
        out.push(TBL[((n >> 12) & 63) as usize] as char);
    } else if rem == 2 {
        let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
        out.push(TBL[((n >> 18) & 63) as usize] as char);
        out.push(TBL[((n >> 12) & 63) as usize] as char);
        out.push(TBL[((n >> 6) & 63) as usize] as char);
    }
    out
}

fn sha256(msg: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut data = msg.to_vec();
    let bit_len = (msg.len() as u64) * 8;
    data.push(0x80);
    while (data.len() % 64) != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in data.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        let vals = [a, b, c, d, e, f, g, hh];
        for i in 0..8 {
            h[i] = h[i].wrapping_add(vals[i]);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

fn random_bytes(n: usize) -> Vec<u8> {
    use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
    let mut buf = vec![0u8; n];
    unsafe {
        let _ = BCryptGenRandom(None, &mut buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG);
    }
    buf
}

fn oauth_connect(cid: &str, csec: &str) -> Result<Tokens, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind loopback: {e}"))?;
    let port = listener.local_addr().map_err(|e| format!("local_addr: {e}"))?.port();
    let _ = listener.set_nonblocking(true);

    let redirect = format!("http://127.0.0.1:{port}/callback");
    let verifier = base64url(&random_bytes(32));
    let challenge = base64url(&sha256(verifier.as_bytes()));
    let state = base64url(&random_bytes(16));

    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
        url_enc(cid),
        url_enc(&redirect),
        url_enc("https://www.googleapis.com/auth/tasks https://www.googleapis.com/auth/calendar.readonly"),
        url_enc(&challenge),
        url_enc(&state),
    );
    crate::launch::open(&auth_url);

    let deadline = std::time::Instant::now() + Duration::from_secs(180);
    let mut code = None;
    while std::time::Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let mut req_buf = [0u8; 4096];
                let n = stream.read(&mut req_buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&req_buf[..n]);
                let first = req.lines().next().unwrap_or("");
                let target = first.split_whitespace().nth(1).unwrap_or("");
                let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
                let mut got_code = None;
                let mut got_state = None;
                for pair in query.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        if k == "code" {
                            got_code = Some(percent_decode(v));
                        } else if k == "state" {
                            got_state = Some(percent_decode(v));
                        }
                    }
                }
                let ok = got_code.is_some() && got_state.as_deref() == Some(state.as_str());
                let html = if ok {
                    "<html><body style='font-family:Segoe UI,sans-serif;background:#111;color:#fff;display:flex;align-items:center;justify-content:center;height:100vh'><div><h2>ABRA terhubung ke Google Tasks &amp; Calendar</h2><p>Anda dapat menutup tab ini sekarang.</p></div></body></html>"
                } else {
                    "<html><body style='font-family:Segoe UI,sans-serif;background:#111;color:#f66;display:flex;align-items:center;justify-content:center;height:100vh'><div><h2>Otorisasi dibatalkan / gagal</h2></div></body></html>"
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    html.len(),
                    html
                );
                let _ = stream.write_all(resp.as_bytes());
                if ok {
                    code = got_code;
                }
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(120));
            }
            Err(e) => return Err(format!("accept: {e}")),
        }
    }

    let code = code.ok_or_else(|| "Otorisasi Google timeout/dibatalkan".to_string())?;
    let mut form = format!(
        "client_id={}&code={}&code_verifier={}&grant_type=authorization_code&redirect_uri={}",
        url_enc(cid),
        url_enc(&code),
        url_enc(&verifier),
        url_enc(&redirect),
    );
    if !csec.is_empty() {
        form.push_str(&format!("&client_secret={}", url_enc(csec)));
    }
    let (st, body) = http::request(
        "POST",
        "https://oauth2.googleapis.com/token",
        &[("Content-Type", "application/x-www-form-urlencoded")],
        Some(form.as_bytes()),
    )?;
    if st >= 400 {
        return Err(format!("Token HTTP {st}: {body}"));
    }
    let j = json::parse(&body).ok_or_else(|| "Respons token bukan JSON".to_string())?;
    let access_token = j.get("access_token").and_then(Json::as_str).ok_or("access_token hilang")?.to_string();
    let refresh_token = j.get("refresh_token").and_then(Json::as_str).unwrap_or("").to_string();
    let expires_in = j.get("expires_in").and_then(Json::as_f64).unwrap_or(3600.0) as i64;
    let tokens = Tokens { access_token, refresh_token, expires_at: now_s() + expires_in - 60 };
    save_tokens(&tokens);
    Ok(tokens)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn ensure_access_token(tokens: &mut Tokens) -> Result<String, String> {
    if now_s() < tokens.expires_at && !tokens.access_token.is_empty() {
        return Ok(tokens.access_token.clone());
    }
    if tokens.refresh_token.is_empty() {
        return Err("Refresh token tidak tersedia".into());
    }
    let (cid, csec) = client_creds().ok_or("Google Client ID belum diatur")?;
    let mut form = format!(
        "client_id={}&refresh_token={}&grant_type=refresh_token",
        url_enc(&cid),
        url_enc(&tokens.refresh_token),
    );
    if !csec.is_empty() {
        form.push_str(&format!("&client_secret={}", url_enc(&csec)));
    }
    let (st, body) = http::request(
        "POST",
        "https://oauth2.googleapis.com/token",
        &[("Content-Type", "application/x-www-form-urlencoded")],
        Some(form.as_bytes()),
    )?;
    if st >= 400 {
        return Err(format!("Refresh HTTP {st}"));
    }
    let j = json::parse(&body).ok_or("Respons refresh bukan JSON")?;
    tokens.access_token = j.get("access_token").and_then(Json::as_str).ok_or("access_token hilang")?.to_string();
    let expires_in = j.get("expires_in").and_then(Json::as_f64).unwrap_or(3600.0) as i64;
    tokens.expires_at = now_s() + expires_in - 60;
    save_tokens(tokens);
    Ok(tokens.access_token.clone())
}

fn fetch_tasks(token: &str) -> Result<Vec<RemoteTask>, String> {
    let auth = format!("Bearer {token}");
    let (st, body) = http::request(
        "GET",
        "https://tasks.googleapis.com/tasks/v1/lists/@default/tasks?showCompleted=true&showHidden=false&maxResults=50",
        &[("Authorization", auth.as_str())],
        None,
    )?;
    if st >= 400 {
        return Err(format!("GET tasks HTTP {st}"));
    }
    let j = json::parse(&body).ok_or("Respons tasks bukan JSON")?;
    let mut out = Vec::new();
    if let Some(arr) = j.get("items").and_then(Json::as_arr) {
        for item in arr {
            let Some(id) = item.get("id").and_then(Json::as_str) else { continue };
            let Some(title) = item.get("title").and_then(Json::as_str) else { continue };
            if title.trim().is_empty() {
                continue;
            }
            let completed = item.get("status").and_then(Json::as_str) == Some("completed");
            let due = item.get("due").and_then(Json::as_str).map(|d| d.chars().take(10).collect());
            out.push(RemoteTask { id: id.to_string(), title: title.to_string(), completed, due });
        }
    }
    Ok(out)
}

fn add_remote(token: &str, title: &str) -> Result<String, String> {
    let auth = format!("Bearer {token}");
    let mut payload = String::from("{\"title\":");
    push_json_str(&mut payload, title);
    payload.push('}');
    let (st, body) = http::request(
        "POST",
        "https://tasks.googleapis.com/tasks/v1/lists/@default/tasks",
        &[("Authorization", auth.as_str()), ("Content-Type", "application/json")],
        Some(payload.as_bytes()),
    )?;
    if st >= 400 {
        return Err(format!("POST task HTTP {st}"));
    }
    let j = json::parse(&body).ok_or("Respons POST bukan JSON")?;
    j.get("id").and_then(Json::as_str).map(String::from).ok_or_else(|| "id hilang".into())
}

fn patch_remote(token: &str, remote_id: &str, completed: bool) -> Result<(), String> {
    let auth = format!("Bearer {token}");
    let url = format!("https://tasks.googleapis.com/tasks/v1/lists/@default/tasks/{}", url_enc(remote_id));
    let payload = if completed { "{\"status\":\"completed\"}" } else { "{\"status\":\"needsAction\",\"completed\":null}" };
    let (st, _) = http::request(
        "PATCH",
        &url,
        &[("Authorization", auth.as_str()), ("Content-Type", "application/json")],
        Some(payload.as_bytes()),
    )?;
    if st >= 400 { Err(format!("PATCH HTTP {st}")) } else { Ok(()) }
}

fn delete_remote(token: &str, remote_id: &str) -> Result<(), String> {
    let auth = format!("Bearer {token}");
    let url = format!("https://tasks.googleapis.com/tasks/v1/lists/@default/tasks/{}", url_enc(remote_id));
    let (st, _) = http::request("DELETE", &url, &[("Authorization", auth.as_str())], None)?;
    if st >= 400 { Err(format!("DELETE HTTP {st}")) } else { Ok(()) }
}

pub fn spawn(notify: impl Fn() + Send + 'static) -> GTasks {
    let (cmd_tx, cmd_rx) = channel::<Cmd>();
    let (ev_tx, ev_rx) = channel::<Event>();

    let _ = std::thread::Builder::new().name("abra-tasks".into()).spawn(move || {
        let mut tokens = load_tokens();
        let update_st = |s: SyncStatus| {
            set_status(s);
            let _ = ev_tx.send(Event::Status(s));
            notify();
        };

        if tokens.is_some() {
            update_st(SyncStatus::Syncing);
            if let Some(ref mut t) = tokens {
                if let Ok(tok) = ensure_access_token(t) {
                    if let Ok(list) = fetch_tasks(&tok) {
                        let _ = ev_tx.send(Event::Synced(list));
                        update_st(SyncStatus::Synced);
                    } else {
                        update_st(SyncStatus::Error);
                    }
                } else {
                    update_st(SyncStatus::Disconnected);
                }
            }
        } else if client_creds().is_none() {
            update_st(SyncStatus::NeedsClientConfig);
        } else {
            update_st(SyncStatus::Disconnected);
        }

        loop {
            match cmd_rx.recv_timeout(Duration::from_secs(60)) {
                Ok(Cmd::Connect) => {
                    let Some((cid, csec)) = client_creds() else {
                        update_st(SyncStatus::NeedsClientConfig);
                        continue;
                    };
                    update_st(SyncStatus::Connecting);
                    match oauth_connect(&cid, &csec) {
                        Ok(mut t) => {
                            if let Ok(tok) = ensure_access_token(&mut t) {
                                if let Ok(list) = fetch_tasks(&tok) {
                                    let _ = ev_tx.send(Event::Synced(list));
                                }
                            }
                            tokens = Some(t);
                            update_st(SyncStatus::Synced);
                        }
                        Err(e) => {
                            crate::log::line("gtasks", &format!("oauth gagal: {e}"));
                            update_st(SyncStatus::Error);
                        }
                    }
                }
                Ok(Cmd::Disconnect) => {
                    tokens = None;
                    cred::delete_secret(cred::TARGET_GTASKS_TOKENS);
                    update_st(if client_creds().is_some() { SyncStatus::Disconnected } else { SyncStatus::NeedsClientConfig });
                }
                Ok(Cmd::SyncNow) | Err(RecvTimeoutError::Timeout) => {
                    let Some(ref mut t) = tokens else { continue };
                    update_st(SyncStatus::Syncing);
                    match ensure_access_token(t).and_then(|tok| fetch_tasks(&tok)) {
                        Ok(list) => {
                            let _ = ev_tx.send(Event::Synced(list));
                            update_st(SyncStatus::Synced);
                        }
                        Err(e) => {
                            crate::log::line("gtasks", &format!("sync gagal: {e}"));
                            update_st(SyncStatus::Error);
                        }
                    }
                }
                Ok(Cmd::Add { local_id, title }) => {
                    let Some(ref mut t) = tokens else { continue };
                    if let Ok(tok) = ensure_access_token(t) {
                        if let Ok(rid) = add_remote(&tok, &title) {
                            let _ = ev_tx.send(Event::Created { local_id, remote_id: rid });
                            notify();
                        }
                    }
                }
                Ok(Cmd::Toggle { local_id, remote_id, completed }) => {
                    let (Some(ref mut t), Some(rid)) = (&mut tokens, remote_id) else { continue };
                    if let Ok(tok) = ensure_access_token(t) {
                        if patch_remote(&tok, &rid, completed).is_ok() {
                            let _ = ev_tx.send(Event::MutationDone { local_id });
                            notify();
                        }
                    }
                }
                Ok(Cmd::Delete { local_id, remote_id }) => {
                    let (Some(ref mut t), Some(rid)) = (&mut tokens, remote_id) else { continue };
                    if let Ok(tok) = ensure_access_token(t) {
                        if delete_remote(&tok, &rid).is_ok() {
                            let _ = ev_tx.send(Event::MutationDone { local_id });
                            notify();
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    GTasks { commands: cmd_tx, events: ev_rx }
}
