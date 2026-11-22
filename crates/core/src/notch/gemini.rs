use super::json::{self, Json};
use super::todo::push_json_str;

pub const MODELS: [&str; 4] = [
    "gemini-2.5-flash",
    "gemini-2.5-pro",
    "gemini-2.5-flash-lite",
    "gemini-2.0-flash",
];

pub fn model_id(idx: u8) -> &'static str {
    MODELS.get(idx as usize).copied().unwrap_or(MODELS[0])
}

pub fn model_label(idx: u8) -> &'static str {
    match idx {
        0 => "Gemini 2.5 Flash",
        1 => "Gemini 2.5 Pro",
        2 => "Gemini 2.5 Flash-Lite",
        3 => "Gemini 2.0 Flash",
        _ => "Gemini 2.5 Flash",
    }
}

pub fn model_short_label(idx: u8) -> &'static str {
    match idx {
        0 => "2.5 Flash",
        1 => "2.5 Pro",
        2 => "2.5 Lite",
        3 => "2.0 Flash",
        _ => "2.5 Flash",
    }
}

/// Returns the trimmed custom model ID when non-empty, otherwise the selected preset ID.
pub fn active_model_id<'a>(preset_idx: u8, custom: &'a str) -> &'a str {
    let trimmed = custom.trim();
    if !trimmed.is_empty() {
        trimmed
    } else {
        model_id(preset_idx)
    }
}

/// Human-friendly header label for the active Gemini model.
pub fn display_model_name(preset_idx: u8, custom: &str) -> String {
    let trimmed = custom.trim();
    if trimmed.is_empty() {
        return model_label(preset_idx).to_string();
    }
    if let Some(i) = MODELS.iter().position(|m| m.eq_ignore_ascii_case(trimmed)) {
        return model_label(i as u8).to_string();
    }
    trimmed.to_string()
}

/// Parses `GET /v1beta/models` JSON and returns active Gemini `generateContent` model IDs
/// sorted newest-first so users always see the latest released models at the top.
pub fn parse_models_list(json_body: &str) -> Vec<String> {
    let Some(root) = json::parse(json_body) else {
        return Vec::new();
    };
    let Some(arr) = root.get("models").and_then(Json::as_arr) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for item in arr {
        let Some(raw_name) = item.get("name").and_then(Json::as_str) else {
            continue;
        };
        let id = raw_name.strip_prefix("models/").unwrap_or(raw_name).trim();
        let lower = id.to_ascii_lowercase();
        if !lower.contains("gemini") {
            continue;
        }
        // Filter out non-chat / specialized endpoints.
        if lower.contains("embedding")
            || lower.contains("imagen")
            || lower.contains("veo")
            || lower.contains("tts")
            || lower.contains("aqa")
            || lower.contains("bisoci")
        {
            continue;
        }
        let supports_gen = item
            .get("supportedGenerationMethods")
            .and_then(Json::as_arr)
            .map(|methods| methods.iter().any(|m| m.as_str() == Some("generateContent")))
            .unwrap_or(true);
        if !supports_gen {
            continue;
        }
        if !out.iter().any(|existing| existing == id) {
            out.push(id.to_string());
        }
    }
    // Sort newest version first (descending major.minor, then pro/flash preference).
    out.sort_by(|a, b| model_sort_key(b).cmp(&model_sort_key(a)).then_with(|| a.cmp(b)));
    out
}

fn model_sort_key(id: &str) -> (u32, u32, u8, u8) {
    let mut major = 0u32;
    let mut minor = 0u32;
    for seg in id.split('-') {
        if let Some((a, b)) = seg.split_once('.') {
            if let (Ok(mj), Ok(mn)) = (a.parse::<u32>(), b.parse::<u32>()) {
                major = mj;
                minor = mn;
                break;
            }
        } else if let Ok(mj) = seg.parse::<u32>() {
            if mj <= 20 && major == 0 {
                major = mj;
            }
        }
    }
    let lower = id.to_ascii_lowercase();
    let stable = if lower.contains("exp") || lower.contains("preview") || lower.contains("lite") { 0 } else { 1 };
    let tier = if lower.contains("pro") {
        2
    } else if lower.contains("flash") {
        1
    } else {
        0
    };
    (major, minor, stable, tier)
}

pub fn build_request_body(prompt: &str) -> String {
    let mut out = String::from("{\"systemInstruction\":{\"parts\":[{\"text\":\"Be concise, clear, and helpful. Keep answers compact for a desktop HUD card.\"}]},\"contents\":[{\"role\":\"user\",\"parts\":[{\"text\":");
    push_json_str(&mut out, prompt);
    out.push_str("}]}],\"generationConfig\":{\"maxOutputTokens\":2048}}");
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SsePiece {
    Text(String),
    Thought,
    Error(String),
}

/// Extracts an API error message from either `{ "error": { "message": "..." } }`
/// or an array-wrapped `[{ "error": { "message": "..." } }]` Gemini error body.
pub fn parse_error_message(body: &str) -> Option<String> {
    let root = json::parse(body.trim())?;
    extract_error_from_json(&root)
}

fn extract_error_from_json(root: &Json) -> Option<String> {
    if let Some(msg) = root
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(Json::as_str)
    {
        return Some(msg.to_string());
    }
    if let Some(arr) = root.as_arr() {
        for item in arr {
            if let Some(msg) = item
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Json::as_str)
            {
                return Some(msg.to_string());
            }
        }
    }
    None
}

fn collect_parts_from_response(root: &Json, text: &mut String, saw_thought: &mut bool) {
    let Some(cands) = root.get("candidates").and_then(Json::as_arr) else {
        return;
    };
    let Some(parts) = cands
        .first()
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(Json::as_arr)
    else {
        return;
    };
    for p in parts {
        // Skip internal reasoning/thought chunks emitted by Gemini 2.5/3/4 thinking models.
        if p.get("thought").and_then(Json::as_bool) == Some(true) {
            *saw_thought = true;
            continue;
        }
        if let Some(t) = p.get("text").and_then(Json::as_str) {
            text.push_str(t);
        }
    }
}

/// Parses one SSE `data: ...` payload from Gemini `streamGenerateContent?alt=sse`.
pub fn parse_sse_data(payload: &str) -> Option<SsePiece> {
    let trimmed = payload.trim();
    if trimmed.is_empty() || trimmed == "[DONE]" {
        return None;
    }
    let root = json::parse(trimmed)?;
    if let Some(err) = extract_error_from_json(&root) {
        return Some(SsePiece::Error(err));
    }
    let mut text = String::new();
    let mut saw_thought = false;
    collect_parts_from_response(&root, &mut text, &mut saw_thought);
    if !text.is_empty() {
        Some(SsePiece::Text(text))
    } else if saw_thought {
        Some(SsePiece::Thought)
    } else {
        None
    }
}

/// Parses a complete JSON response from Gemini `:generateContent` (or an array of chunks).
pub fn parse_generate_content_response(body: &str) -> Option<SsePiece> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    let root = json::parse(trimmed)?;
    if let Some(err) = extract_error_from_json(&root) {
        return Some(SsePiece::Error(err));
    }
    let mut text = String::new();
    let mut saw_thought = false;
    if let Some(arr) = root.as_arr() {
        for item in arr {
            collect_parts_from_response(item, &mut text, &mut saw_thought);
        }
    } else {
        collect_parts_from_response(&root, &mut text, &mut saw_thought);
    }
    if !text.is_empty() {
        return Some(SsePiece::Text(text));
    }
    if let Some(reason) = root
        .get("promptFeedback")
        .and_then(|f| f.get("blockReason"))
        .and_then(Json::as_str)
    {
        return Some(SsePiece::Error(format!("Diblokir oleh filter keamanan ({reason}).")));
    }
    if saw_thought {
        Some(SsePiece::Thought)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_valid_json_and_parses_sse_delta() {
        let body = build_request_body("Halo \"Gemini\"\nbaris 2");
        let parsed = json::parse(&body).expect("valid request json");
        assert!(parsed.get("generationConfig").is_some());
        let sse = r#"{"candidates":[{"content":{"parts":[{"text":"Halo! "},{"text":"Ada yang bisa dibantu?"}]}}]}"#;
        assert_eq!(parse_sse_data(sse), Some(SsePiece::Text("Halo! Ada yang bisa dibantu?".into())));
        let err = r#"{"error":{"code":400,"message":"API key not valid"}}"#;
        assert_eq!(parse_sse_data(err), Some(SsePiece::Error("API key not valid".into())));
    }

    #[test]
    fn filters_thoughts_and_parses_generate_content_fallback() {
        let thought_only = r#"{"candidates":[{"content":{"parts":[{"thought":true,"text":"internal reasoning"}]}}]}"#;
        assert_eq!(parse_sse_data(thought_only), Some(SsePiece::Thought));

        let mixed = r#"{"candidates":[{"content":{"parts":[{"thought":true,"text":"plan"},{"text":"Jawaban ringkas."}]}}]}"#;
        assert_eq!(
            parse_generate_content_response(mixed),
            Some(SsePiece::Text("Jawaban ringkas.".into()))
        );

        let arr_err = r#"[{"error":{"code":503,"message":"Model overloaded"}}]"#;
        assert_eq!(parse_error_message(arr_err), Some("Model overloaded".into()));
        assert_eq!(
            parse_generate_content_response(arr_err),
            Some(SsePiece::Error("Model overloaded".into()))
        );

        let blocked = r#"{"promptFeedback":{"blockReason":"SAFETY"}}"#;
        assert_eq!(
            parse_generate_content_response(blocked),
            Some(SsePiece::Error("Diblokir oleh filter keamanan (SAFETY).".into()))
        );
    }

    #[test]
    fn resolves_custom_model_and_parses_models_endpoint() {
        assert_eq!(active_model_id(0, ""), "gemini-2.5-flash");
        assert_eq!(active_model_id(1, "  gemini-2.5-pro  "), "gemini-2.5-pro");
        let resp = r#"{"models":[
            {"name":"models/text-embedding-004","supportedGenerationMethods":["embedContent"]},
            {"name":"models/gemini-2.5-flash","supportedGenerationMethods":["generateContent"]},
            {"name":"models/gemini-3.6-flash","supportedGenerationMethods":["generateContent"]},
            {"name":"models/gemini-3.5-pro","supportedGenerationMethods":["generateContent","countTokens"]}
        ]}"#;
        let list = parse_models_list(resp);
        assert_eq!(list, vec!["gemini-3.6-flash", "gemini-3.5-pro", "gemini-2.5-flash"]);
    }
}
