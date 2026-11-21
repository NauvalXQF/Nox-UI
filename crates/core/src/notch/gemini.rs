use super::json::{self, Json};
use super::todo::push_json_str;

pub const MODELS: [&str; 2] = ["gemini-2.5-flash", "gemini-2.5-pro"];

pub fn model_id(idx: u8) -> &'static str {
    MODELS.get(idx as usize).copied().unwrap_or(MODELS[0])
}

pub fn model_label(idx: u8) -> &'static str {
    match idx {
        1 => "Gemini 2.5 Pro",
        _ => "Gemini 2.5 Flash",
    }
}

pub fn build_request_body(prompt: &str) -> String {
    let mut out = String::from("{\"systemInstruction\":{\"parts\":[{\"text\":\"Be concise, clear, and helpful. Keep answers compact for a desktop HUD card.\"}]},\"contents\":[{\"role\":\"user\",\"parts\":[{\"text\":");
    push_json_str(&mut out, prompt);
    out.push_str("}]}]}");
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SsePiece {
    Text(String),
    Error(String),
}

/// Parses one SSE `data: ...` payload from Gemini `streamGenerateContent?alt=sse`.
pub fn parse_sse_data(payload: &str) -> Option<SsePiece> {
    let trimmed = payload.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return None;
    }
    let root = json::parse(trimmed)?;
    if let Some(err) = root.get("error").and_then(|e| e.get("message")).and_then(Json::as_str) {
        return Some(SsePiece::Error(err.to_string()));
    }
    let parts = root
        .get("candidates")
        .and_then(Json::as_arr)?
        .first()?
        .get("content")?
        .get("parts")
        .and_then(Json::as_arr)?;
    let mut text = String::new();
    for p in parts {
        if let Some(t) = p.get("text").and_then(Json::as_str) {
            text.push_str(t);
        }
    }
    (!text.is_empty()).then_some(SsePiece::Text(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_json_and_parses_sse_delta() {
        let body = build_request_body("Halo \"Gemini\"\nbaris 2");
        assert!(json::parse(&body).is_some());
        let sse = r#"{"candidates":[{"content":{"parts":[{"text":"Halo! "},{"text":"Ada yang bisa dibantu?"}]}}]}"#;
        assert_eq!(parse_sse_data(sse), Some(SsePiece::Text("Halo! Ada yang bisa dibantu?".into())));
        let err = r#"{"error":{"code":400,"message":"API key not valid"}}"#;
        assert_eq!(parse_sse_data(err), Some(SsePiece::Error("API key not valid".into())));
    }
}
