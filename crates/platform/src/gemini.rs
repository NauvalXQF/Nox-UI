use crate::{cred, http};
use nacre_core::notch::gemini::{build_request_body, parse_sse_data, SsePiece};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

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
    cancel: Arc<AtomicBool>,
}

impl Gemini {
    pub fn cancel_current(&self) {
        self.cancel.store(true, Ordering::Relaxed);
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
        let msg = nacre_core::notch::json::parse(&body)
            .and_then(|j| j.get("error")?.get("message")?.as_str().map(String::from))
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(msg);
    }
    let models = nacre_core::notch::gemini::parse_models_list(&body);
    if models.is_empty() {
        return Err("Tidak ada model Gemini aktif ditemukan.".into());
    }
    Ok(models)
}

pub fn spawn(notify: impl Fn() + Send + 'static) -> Gemini {
    let (cmd_tx, cmd_rx) = channel::<Cmd>();
    let (ev_tx, ev_rx) = channel::<Event>();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_w = cancel.clone();

    let _ = std::thread::Builder::new().name("abra-gemini".into()).spawn(move || {
        while let Ok(cmd) = cmd_rx.recv() {
            match cmd {
                Cmd::Cancel => {}
                Cmd::Ask { id, prompt, model } => {
                    cancel_w.store(false, Ordering::Relaxed);
                    let Some(key) = api_key() else {
                        let _ = ev_tx.send(Event::Error {
                            id,
                            message: "Gemini API Key belum diatur (Settings → Visor).".into(),
                        });
                        notify();
                        continue;
                    };
                    let url = format!(
                        "https://generativelanguage.googleapis.com/v1beta/models/{model}:streamGenerateContent?alt=sse"
                    );
                    let body = build_request_body(&prompt);
                    let headers = [
                        ("Content-Type", "application/json"),
                        ("x-goog-api-key", key.as_str()),
                    ];

                    let mut line_buf = String::new();
                    let mut raw_err = String::new();
                    let mut got_any = false;

                    let res = http::stream_post(&url, &headers, body.as_bytes(), &cancel_w, |chunk| {
                        let s = String::from_utf8_lossy(chunk);
                        line_buf.push_str(&s);
                        if raw_err.len() < 1024 {
                            raw_err.push_str(&s);
                        }
                        while let Some(nl) = line_buf.find('\n') {
                            let line = line_buf[..nl].trim_end_matches('\r').to_string();
                            line_buf.drain(..=nl);
                            if let Some(payload) = line.strip_prefix("data:") {
                                match parse_sse_data(payload) {
                                    Some(SsePiece::Text(t)) => {
                                        got_any = true;
                                        let _ = ev_tx.send(Event::Delta { id, text: t });
                                        notify();
                                    }
                                    Some(SsePiece::Error(msg)) => {
                                        let _ = ev_tx.send(Event::Error { id, message: msg });
                                        notify();
                                    }
                                    None => {}
                                }
                            }
                        }
                    });

                    // Flush trailing line if server closed without newline.
                    if let Some(payload) = line_buf.trim().strip_prefix("data:") {
                        if let Some(SsePiece::Text(t)) = parse_sse_data(payload) {
                            got_any = true;
                            let _ = ev_tx.send(Event::Delta { id, text: t });
                            notify();
                        }
                    }

                    match res {
                        Ok(status) if status >= 400 => {
                            let msg = nacre_core::notch::json::parse(&raw_err)
                                .and_then(|j| j.get("error")?.get("message")?.as_str().map(String::from))
                                .unwrap_or_else(|| format!("HTTP {status}"));
                            let _ = ev_tx.send(Event::Error { id, message: msg });
                        }
                        Ok(_) => {
                            if !got_any && !cancel_w.load(Ordering::Relaxed) {
                                let _ = ev_tx.send(Event::Error {
                                    id,
                                    message: "Respons kosong dari Gemini.".into(),
                                });
                            } else {
                                let _ = ev_tx.send(Event::Done { id });
                            }
                        }
                        Err(e) => {
                            let _ = ev_tx.send(Event::Error { id, message: e });
                        }
                    }
                    notify();
                }
            }
        }
    });

    Gemini { commands: cmd_tx, events: ev_rx, cancel }
}
