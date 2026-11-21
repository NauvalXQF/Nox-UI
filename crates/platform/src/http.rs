use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_ADDREQ_FLAG_REPLACE, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

struct Handle(*mut c_void);

impl Handle {
    fn new(raw: *mut c_void) -> Result<Self, String> {
        if raw.is_null() {
            Err("WinHttp handle null".into())
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
}

struct ParsedUrl {
    https: bool,
    host: String,
    port: u16,
    path: String,
}

fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err("Skema URL tidak didukung".into());
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (
            h.to_string(),
            p.parse::<u16>().map_err(|_| "Port URL tidak valid".to_string())?,
        ),
        None => (authority.to_string(), if https { 443 } else { 80 }),
    };
    if host.is_empty() {
        return Err("Host URL kosong".into());
    }
    Ok(ParsedUrl { https, host, port, path: path.to_string() })
}

fn open_and_send(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    recv_timeout_ms: i32,
) -> Result<(Handle, Handle, Handle, u16), String> {
    let parsed = parse_url(url)?;
    let ua = HSTRING::from(concat!("ABRA/", env!("CARGO_PKG_VERSION")));
    let host_h = HSTRING::from(&parsed.host);
    let method_h = HSTRING::from(method);
    let path_h = HSTRING::from(&parsed.path);

    unsafe {
        let session = Handle::new(WinHttpOpen(
            &ua,
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ))?;
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, recv_timeout_ms);
        let connect = Handle::new(WinHttpConnect(session.0, &host_h, parsed.port, 0))?;
        let flags = if parsed.https { WINHTTP_FLAG_SECURE } else { windows::Win32::Networking::WinHttp::WINHTTP_OPEN_REQUEST_FLAGS(0) };
        let req = Handle::new(WinHttpOpenRequest(
            connect.0,
            &method_h,
            &path_h,
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            flags,
        ))?;

        for (k, v) in headers {
            let line = format!("{k}: {v}\r\n");
            let wide: Vec<u16> = line.encode_utf16().collect();
            let _ = WinHttpAddRequestHeaders(
                req.0,
                &wide,
                WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE,
            );
        }

        let (ptr, len) = match body {
            Some(b) if !b.is_empty() => (Some(b.as_ptr() as *const c_void), b.len() as u32),
            _ => (None, 0),
        };
        WinHttpSendRequest(req.0, None, ptr, len, len, 0)
            .map_err(|e| format!("WinHttpSendRequest: {e}"))?;
        WinHttpReceiveResponse(req.0, std::ptr::null_mut())
            .map_err(|e| format!("WinHttpReceiveResponse: {e}"))?;

        let mut status: u32 = 0;
        let mut sz = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            req.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut c_void),
            &mut sz,
            std::ptr::null_mut(),
        )
        .map_err(|e| format!("WinHttpQueryHeaders: {e}"))?;

        Ok((session, connect, req, status as u16))
    }
}

pub fn request(
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
) -> Result<(u16, String), String> {
    let (_s, _c, req, status) = open_and_send(method, url, headers, body, 20_000)?;
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    unsafe {
        loop {
            let mut avail = 0u32;
            WinHttpQueryDataAvailable(req.0, &mut avail)
                .map_err(|e| format!("WinHttpQueryDataAvailable: {e}"))?;
            if avail == 0 {
                break;
            }
            let want = (avail as usize).min(buf.len()) as u32;
            let mut read = 0u32;
            WinHttpReadData(req.0, buf.as_mut_ptr() as *mut c_void, want, &mut read)
                .map_err(|e| format!("WinHttpReadData: {e}"))?;
            if read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..read as usize]);
            if out.len() > 8 * 1024 * 1024 {
                break;
            }
        }
    }
    Ok((status, String::from_utf8_lossy(&out).into_owned()))
}

/// Streams a POST response incrementally chunk-by-chunk (e.g. Gemini SSE) while respecting `cancel`.
pub fn stream_post(
    url: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    cancel: &AtomicBool,
    mut on_chunk: impl FnMut(&[u8]),
) -> Result<u16, String> {
    let (_s, _c, req, status) = open_and_send("POST", url, headers, Some(body), 60_000)?;
    let mut buf = [0u8; 4096];
    unsafe {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Ok(status);
            }
            let mut avail = 0u32;
            WinHttpQueryDataAvailable(req.0, &mut avail)
                .map_err(|e| format!("WinHttpQueryDataAvailable: {e}"))?;
            if avail == 0 {
                break;
            }
            if cancel.load(Ordering::Relaxed) {
                return Ok(status);
            }
            let want = (avail as usize).min(buf.len()) as u32;
            let mut read = 0u32;
            WinHttpReadData(req.0, buf.as_mut_ptr() as *mut c_void, want, &mut read)
                .map_err(|e| format!("WinHttpReadData: {e}"))?;
            if read == 0 {
                break;
            }
            on_chunk(&buf[..read as usize]);
        }
    }
    Ok(status)
}
