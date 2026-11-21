use crate::{cred, http};
use nacre_core::notch::gemini::{
    build_request_body, parse_error_message, parse_generate_content_response, parse_sse_data,
    SsePiece,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Remember within this session when SSE streaming stalls on the local network/proxy
/// while non-streaming `:generateContent` succeeds quickly, so follow-up prompts answer fast.
static PREFER_NON_STREAM: AtomicBool = AtomicBool::new(false);

const SSE_RECV_TIMEOUT_MS: i32 = 20_000;
const FALLBACK_RECV_TIMEOUT_MS: i32 = 45_000;

pub enum Cmd {
    Ask { id: u64, prompt: String, model: String },
    Cancel,
}

pub enum Event {
    Delta { id: u64, text: String },
    Done { id: u64 },
    Error { id: u64, message: String },
}

pub struct Gemini {
    pub commands: Sender<Cmd>,
    pub events: Receiver<Event>,
    active_cancel: Arc<Mutex<Arc<AtomicBool>>>,
}

impl Gemini {
    pub fn cancel_current(&self) {
        if let Ok(guard) = self.active_cancel.lock() {
            guard.store(true, Ordering::Relaxed);
        }
        let _ = self.commands.send(Cmd::Cancel);
    }
}

pub fn has_api_key() -> bool {
    api_key().is_some()
}

fn api_key() -> Option<String> {
    cred::read_secret(cred::TARGET_GEMINI_KEY)
        .or_else(|| std::env::var("GEMINI_API_KEY").ok().filter(|s| !s.trim().is_empty()))
}

/// Fetches active Gemini models supporting `generateContent` from `v1beta/models`.
pub fn fetch_models() -> Result<Vec<String>, String> {
    let Some(key) = api_key() else {
        return Err("Gemini API Key belum diatur.".into());
    };
    let (status, body) = http::request(
        "GET",
        "https://generativelanguage.googleapis.com/v1beta/models?pageSize=100",
        &[("x-goog-api-key", key.as_str())],
        None,
    )?;
    if status >= 400 {
        let msg = parse_error_message(&body).unwrap_or_else(|| format!("HTTP {status}"));
        return Err(msg);
    }
    let models = nacre_core::notch::gemini::parse_models_list(&body);
    if models.is_empty() {
        return Err("Tidak ada model Gemini aktif ditemukan.".into());
    }
    Ok(models)
}

pub fn spawn(notify: impl Fn() + Send + Sync + 'static) -> Gemini {
    let (cmd_tx, cmd_rx) = channel::<Cmd>();
    let (ev_tx, ev_rx) = channel::<Event>();
    let active_cancel = Arc::new(Mutex::new(Arc::new(AtomicBool::new(false))));
    let active_cancel_w = active_cancel.clone();
    let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(notify);

    let _ = std::thread::Builder::new().name("abra-gemini".into()).spawn(move || {
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                Cmd::Cancel => {
                    if let Ok(guard) = active_cancel_w.lock() {
                        guard.store(true, Ordering::Relaxed);
                    }
                }
                Cmd::Ask { id, prompt, model } => {
                    let req_cancel = Arc::new(AtomicBool::new(false));
                    if let Ok(mut guard) = active_cancel_w.lock() {
                        guard.store(true, Ordering::Relaxed);
                        *guard = req_cancel.clone();
                    }
                    let tx = ev_tx.clone();
                    let wake = notify.clone();
                    // Spawn per-request thread so Cancel or a new prompt NEVER queues behind a stalled WinHTTP call.
                    let _ = std::thread::Builder::new().name(format!("abra-gemini-{id}")).spawn(move || {
                        run_ask(id, prompt, model, req_cancel, tx, wake);
                    });
                }
            }
        }
    });

    Gemini { commands: cmd_tx, events: ev_rx, active_cancel }
}

fn run_ask(
    id: u64,
    prompt: String,
    model: String,
    cancel: Arc<AtomicBool>,
    ev_tx: Sender<Event>,
    notify: Arc<dyn Fn() + Send + Sync>,
) {
    let Some(key) = api_key() else {
        if !cancel.load(Ordering::Relaxed) {
            let _ = ev_tx.send(Event::Error {
                id,
                message: "Gemini API Key belum diatur (Settings → Visor).".into(),
            });
            notify();
        }
        return;
    };

    let clean_model = model.trim().strip_prefix("models/").unwrap_or(model.trim());
    let body = build_request_body(&prompt);
    let prefer_ns = PREFER_NON_STREAM.load(Ordering::Relaxed);
    crate::log::line(
        "gemini",
        &format!("start id={id} model={clean_model} prefer_non_stream={prefer_ns}"),
    );

    let mut tried_sse = false;
    let mut sse_timed_out = false;

    if !prefer_ns {
        tried_sse = true;
        let t0 = Instant::now();
        let sse_url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{clean_model}:streamGenerateContent?alt=sse"
        );
        let sse_headers = [
            ("Content-Type", "application/json"),
            ("Accept", "text/event-stream"),
            ("Accept-Encoding", "identity"),
            ("Cache-Control", "no-cache"),
            ("x-goog-api-key", key.as_str()),
        ];

        let mut raw_line_buf: Vec<u8> = Vec::new();
        let mut raw_err: Vec<u8> = Vec::new();
        let mut got_any = false;
        let mut chars = 0usize;
        let mut thoughts = 0usize;
        let mut sse_err: Option<String> = None;

        let handle_line = |line: &str,
                           got_any: &mut bool,
                           chars: &mut usize,
                           thoughts: &mut usize,
                           sse_err: &mut Option<String>| {
            if let Some(payload) = line.strip_prefix("data:") {
                match parse_sse_data(payload) {
                    Some(SsePiece::Text(t)) => {
                        *got_any = true;
                        *chars += t.len();
                        let _ = ev_tx.send(Event::Delta { id, text: t });
                        notify();
                    }
                    Some(SsePiece::Thought) => {
                        *thoughts += 1;
                    }
                    Some(SsePiece::Error(msg)) => {
                        *sse_err = Some(msg);
                    }
                    None => {}
                }
            }
        };

        let res = http::stream_post_with_timeout(
            &sse_url,
            &sse_headers,
            body.as_bytes(),
            SSE_RECV_TIMEOUT_MS,
            &cancel,
            |chunk| {
                raw_line_buf.extend_from_slice(chunk);
                if raw_err.len() < 4096 {
                    let take = (4096 - raw_err.len()).min(chunk.len());
                    raw_err.extend_from_slice(&chunk[..take]);
                }
                while let Some(nl) = raw_line_buf.iter().position(|&b| b == b'\n') {
                    let mut line_bytes = raw_line_buf.drain(..=nl).collect::<Vec<u8>>();
                    if line_bytes.last() == Some(&b'\n') {
                        line_bytes.pop();
                    }
                    if line_bytes.last() == Some(&b'\r') {
                        line_bytes.pop();
                    }
                    let line = String::from_utf8_lossy(&line_bytes);
                    handle_line(&line, &mut got_any, &mut chars, &mut thoughts, &mut sse_err);
                }
            },
        );

        if !raw_line_buf.is_empty() && !cancel.load(Ordering::Relaxed) {
            let line = String::from_utf8_lossy(&raw_line_buf);
            handle_line(line.trim(), &mut got_any, &mut chars, &mut thoughts, &mut sse_err);
        }

        let elapsed = t0.elapsed().as_millis();
        if cancel.load(Ordering::Relaxed) {
            crate::log::line("gemini", &format!("cancelled id={id} model={clean_model} after {elapsed}ms"));
            return;
        }

        match res {
            Ok(status) if status >= 400 => {
                let raw_s = String::from_utf8_lossy(&raw_err);
                let msg = parse_error_message(&raw_s).unwrap_or_else(|| format!("HTTP {status}"));
                crate::log::line(
                    "gemini",
                    &format!("sse id={id} model={clean_model} HTTP {status} ({elapsed}ms): {msg}"),
                );
                let _ = ev_tx.send(Event::Error { id, message: msg });
                notify();
                return;
            }
            Ok(status) if got_any => {
                crate::log::line(
                    "gemini",
                    &format!("done id={id} model={clean_model} mode=sse status={status} chars={chars} thoughts={thoughts} elapsed={elapsed}ms"),
                );
                let _ = ev_tx.send(Event::Done { id });
                notify();
                return;
            }
            Ok(status) => {
                if let Some(msg) = sse_err {
                    crate::log::line(
                        "gemini",
                        &format!("sse id={id} model={clean_model} payload error ({elapsed}ms): {msg}"),
                    );
                    let _ = ev_tx.send(Event::Error { id, message: msg });
                    notify();
                    return;
                }
                crate::log::line(
                    "gemini",
                    &format!("sse id={id} model={clean_model} status={status} empty text (thoughts={thoughts}, elapsed={elapsed}ms); falling back to generateContent"),
                );
            }
            Err(e) => {
                if got_any {
                    crate::log::line(
                        "gemini",
                        &format!("sse id={id} model={clean_model} ended after {chars} chars ({elapsed}ms): {e}"),
                    );
                    let _ = ev_tx.send(Event::Done { id });
                    notify();
                    return;
                }
                if http::is_timeout_err(&e) {
                    sse_timed_out = true;
                    crate::log::line(
                        "gemini",
                        &format!("sse id={id} model={clean_model} timed out ({elapsed}ms); falling back to generateContent"),
                    );
                } else {
                    crate::log::line(
                        "gemini",
                        &format!("sse id={id} model={clean_model} error ({elapsed}ms): {e}"),
                    );
                    let _ = ev_tx.send(Event::Error { id, message: e });
                    notify();
                    return;
                }
            }
        }
    }

    // Fallback (or direct when session switched to non-streaming): plain JSON `:generateContent`.
    let t1 = Instant::now();
    let gen_url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{clean_model}:generateContent"
    );
    let gen_headers = [
        ("Content-Type", "application/json"),
        ("Accept", "application/json"),
        ("Accept-Encoding", "identity"),
        ("x-goog-api-key", key.as_str()),
    ];
    let res = http::request_with_timeout(
        "POST",
        &gen_url,
        &gen_headers,
        Some(body.as_bytes()),
        FALLBACK_RECV_TIMEOUT_MS,
    );
    let elapsed = t1.elapsed().as_millis();
    if cancel.load(Ordering::Relaxed) {
        crate::log::line(
            "gemini",
            &format!("cancelled id={id} model={clean_model} during generateContent ({elapsed}ms)"),
        );
        return;
    }

    match res {
        Ok((status, resp_body)) if status >= 400 => {
            let msg = parse_error_message(&resp_body).unwrap_or_else(|| format!("HTTP {status}"));
            crate::log::line(
                "gemini",
                &format!("generateContent id={id} model={clean_model} HTTP {status} ({elapsed}ms): {msg}"),
            );
            let _ = ev_tx.send(Event::Error { id, message: msg });
        }
        Ok((status, resp_body)) => match parse_generate_content_response(&resp_body) {
            Some(SsePiece::Text(text)) => {
                if tried_sse && sse_timed_out && elapsed < 20_000 {
                    PREFER_NON_STREAM.store(true, Ordering::Relaxed);
                    crate::log::line(
                        "gemini",
                        "switching session to non-streaming generateContent (SSE timed out while JSON POST succeeded quickly)",
                    );
                }
                crate::log::line(
                    "gemini",
                    &format!("done id={id} model={clean_model} mode=generateContent status={status} chars={} elapsed={elapsed}ms", text.len()),
                );
                let _ = ev_tx.send(Event::Delta { id, text });
                let _ = ev_tx.send(Event::Done { id });
            }
            Some(SsePiece::Error(msg)) => {
                crate::log::line(
                    "gemini",
                    &format!("generateContent id={id} model={clean_model} error ({elapsed}ms): {msg}"),
                );
                let _ = ev_tx.send(Event::Error { id, message: msg });
            }
            Some(SsePiece::Thought) => {
                crate::log::line(
                    "gemini",
                    &format!("generateContent id={id} model={clean_model} returned only thoughts ({elapsed}ms)"),
                );
                let _ = ev_tx.send(Event::Error {
                    id,
                    message: "Model hanya mengembalikan pemikiran internal tanpa jawaban akhir. Coba model Flash.".into(),
                });
            }
            None => {
                let preview: String = resp_body.chars().take(180).collect();
                crate::log::line(
                    "gemini",
                    &format!("generateContent id={id} model={clean_model} HTTP {status} empty ({elapsed}ms): {preview}"),
                );
                let _ = ev_tx.send(Event::Error {
                    id,
                    message: format!("Respons kosong dari Gemini (HTTP {status})."),
                });
            }
        },
        Err(e) => {
            crate::log::line(
                "gemini",
                &format!("generateContent id={id} model={clean_model} error ({elapsed}ms): {e}"),
            );
            let _ = ev_tx.send(Event::Error { id, message: e });
        }
    }
    notify();
}
