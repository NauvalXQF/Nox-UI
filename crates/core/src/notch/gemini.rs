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
    let mut out = String::from("{\"systemInstruction\":{\"parts\":[{\"text\":\"Be concise, clear, and helpful. Keep answers compact for a desktop HUD card. For math formulas, prefer clean Unicode symbols (e.g. c² = a² + b², √x, ×, ÷, π, ≤, ≥, ≠, →) instead of raw LaTeX.\"}]},\"contents\":[{\"role\":\"user\",\"parts\":[{\"text\":");
    push_json_str(&mut out, prompt);
    out.push_str("}]}],\"generationConfig\":{\"maxOutputTokens\":2048}}");
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanStyle {
    Normal,
    Bold,
    Italic,
    Code,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MdSpan {
    pub style: SpanStyle,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MdBlock {
    Blank,
    Heading(Vec<MdSpan>),
    Formula(String),
    Bullet { indent: u8, marker: String, spans: Vec<MdSpan> },
    CodeBlock(Vec<String>),
    Paragraph(Vec<MdSpan>),
}

fn to_superscript(s: &str) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(s.len() * 2);
    for ch in s.chars() {
        let sup = match ch {
            '0' => '⁰',
            '1' => '¹',
            '2' => '²',
            '3' => '³',
            '4' => '⁴',
            '5' => '⁵',
            '6' => '⁶',
            '7' => '⁷',
            '8' => '⁸',
            '9' => '⁹',
            '+' => '⁺',
            '-' => '⁻',
            '=' => '⁼',
            '(' => '⁽',
            ')' => '⁾',
            'n' => 'ⁿ',
            'i' => 'ⁱ',
            ' ' => ' ',
            _ => return None,
        };
        out.push(sup);
    }
    Some(out)
}

fn to_subscript(s: &str) -> Option<String> {
    if s.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(s.len() * 2);
    for ch in s.chars() {
        let sub = match ch {
            '0' => '₀',
            '1' => '₁',
            '2' => '₂',
            '3' => '₃',
            '4' => '₄',
            '5' => '₅',
            '6' => '₆',
            '7' => '₇',
            '8' => '₈',
            '9' => '₉',
            '+' => '₊',
            '-' => '₋',
            '=' => '₌',
            '(' => '₍',
            ')' => '₎',
            ' ' => ' ',
            _ => return None,
        };
        out.push(sub);
    }
    Some(out)
}

fn read_braced(chars: &[char], idx: &mut usize) -> Option<String> {
    while *idx < chars.len() && chars[*idx] == ' ' {
        *idx += 1;
    }
    if *idx >= chars.len() {
        return None;
    }
    if chars[*idx] != '{' {
        let ch = chars[*idx];
        *idx += 1;
        return Some(ch.to_string());
    }
    *idx += 1;
    let mut depth = 1usize;
    let mut out = String::new();
    while *idx < chars.len() {
        let ch = chars[*idx];
        *idx += 1;
        if ch == '{' {
            depth += 1;
            out.push(ch);
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                break;
            }
            out.push(ch);
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

fn is_simple_math_atom(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty() && t.chars().all(|c| c.is_alphanumeric() || "⁰¹²³⁴⁵⁶⁷⁸⁹₀₁₂₃₄₅₆₇₈₉πθασβγΔΩ".contains(c))
}

/// Converts a LaTeX math expression (without outer `$` / `$$` / `\[` delimiters) into clean,
/// readable Unicode math suitable for DirectWrite/Segoe UI rendering.
pub fn latex_to_unicode(input: &str) -> String {
    let chars: Vec<char> = input.trim().chars().collect();
    let mut i = 0usize;
    let mut out = String::with_capacity(input.len());

    while i < chars.len() {
        let ch = chars[i];
        if ch == '\\' {
            i += 1;
            if i >= chars.len() {
                break;
            }
            if !chars[i].is_ascii_alphabetic() {
                let sym = chars[i];
                i += 1;
                match sym {
                    ',' | ';' | ':' | ' ' => out.push(' '),
                    '!' => {}
                    '\\' => out.push_str(", "),
                    '{' | '}' | '%' | '#' | '&' | '_' | '$' => out.push(sym),
                    _ => out.push(sym),
                }
                continue;
            }
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            let cmd: String = chars[start..i].iter().collect();
            match cmd.as_str() {
                "mathbf" | "mathrm" | "text" | "textbf" | "textit" | "mathit" | "mathbb"
                | "mathcal" | "mathsf" | "operatorname" | "boldsymbol" | "bm" | "boxed" => {
                    if let Some(inner) = read_braced(&chars, &mut i) {
                        out.push_str(&latex_to_unicode(&inner));
                    }
                }
                "frac" | "dfrac" | "tfrac" => {
                    let num = read_braced(&chars, &mut i).map(|s| latex_to_unicode(&s)).unwrap_or_default();
                    let den = read_braced(&chars, &mut i).map(|s| latex_to_unicode(&s)).unwrap_or_default();
                    if is_simple_math_atom(&num) && is_simple_math_atom(&den) {
                        out.push_str(&num);
                        out.push('/');
                        out.push_str(&den);
                    } else {
                        if is_simple_math_atom(&num) {
                            out.push_str(&num);
                        } else {
                            out.push('(');
                            out.push_str(&num);
                            out.push(')');
                        }
                        out.push_str(" / ");
                        if is_simple_math_atom(&den) {
                            out.push_str(&den);
                        } else {
                            out.push('(');
                            out.push_str(&den);
                            out.push(')');
                        }
                    }
                }
                "sqrt" => {
                    // Optional nth-root `[n]`
                    if i < chars.len() && chars[i] == '[' {
                        i += 1;
                        let mut root = String::new();
                        while i < chars.len() && chars[i] != ']' {
                            root.push(chars[i]);
                            i += 1;
                        }
                        if i < chars.len() && chars[i] == ']' {
                            i += 1;
                        }
                        if let Some(sup) = to_superscript(root.trim()) {
                            out.push_str(&sup);
                        }
                    }
                    let arg = read_braced(&chars, &mut i).map(|s| latex_to_unicode(&s)).unwrap_or_default();
                    out.push('√');
                    if is_simple_math_atom(&arg) {
                        out.push_str(&arg);
                    } else {
                        out.push('(');
                        out.push_str(&arg);
                        out.push(')');
                    }
                }
                "left" | "right" | "big" | "Big" | "bigg" | "Bigg" | "displaystyle" | "limits" => {}
                "times" => out.push('×'),
                "cdot" | "bullet" => out.push('·'),
                "div" => out.push('÷'),
                "pm" => out.push('±'),
                "mp" => out.push('∓'),
                "neq" | "ne" => out.push('≠'),
                "leq" | "le" => out.push('≤'),
                "geq" | "ge" => out.push('≥'),
                "approx" => out.push('≈'),
                "equiv" => out.push('≡'),
                "sim" => out.push('∼'),
                "propto" => out.push('∝'),
                "infty" => out.push('∞'),
                "to" | "rightarrow" => out.push('→'),
                "leftarrow" => out.push('←'),
                "leftrightarrow" => out.push('↔'),
                "Rightarrow" | "implies" => out.push('⇒'),
                "Leftarrow" => out.push('⇐'),
                "Leftrightarrow" | "iff" => out.push('⇔'),
                "circ" | "degree" => out.push('°'),
                "sum" => out.push('∑'),
                "prod" => out.push('∏'),
                "int" => out.push('∫'),
                "partial" => out.push('∂'),
                "nabla" => out.push('∇'),
                "in" => out.push('∈'),
                "notin" => out.push('∉'),
                "subset" => out.push('⊂'),
                "subseteq" => out.push('⊆'),
                "cup" => out.push('∪'),
                "cap" => out.push('∩'),
                "forall" => out.push('∀'),
                "exists" => out.push('∃'),
                "therefore" => out.push('∴'),
                "angle" => out.push('∠'),
                "perp" => out.push('⊥'),
                "parallel" => out.push('∥'),
                "triangle" => out.push('△'),
                "ldots" | "cdots" | "dots" => out.push('…'),
                "quad" | "qquad" => out.push(' '),
                "alpha" => out.push('α'),
                "beta" => out.push('β'),
                "gamma" => out.push('γ'),
                "Gamma" => out.push('Γ'),
                "delta" => out.push('δ'),
                "Delta" => out.push('Δ'),
                "epsilon" | "varepsilon" => out.push('ε'),
                "zeta" => out.push('ζ'),
                "eta" => out.push('η'),
                "theta" | "vartheta" => out.push('θ'),
                "Theta" => out.push('Θ'),
                "lambda" => out.push('λ'),
                "Lambda" => out.push('Λ'),
                "mu" => out.push('μ'),
                "nu" => out.push('ν'),
                "xi" => out.push('ξ'),
                "pi" | "varpi" => out.push('π'),
                "Pi" => out.push('Π'),
                "rho" => out.push('ρ'),
                "sigma" => out.push('σ'),
                "Sigma" => out.push('Σ'),
                "tau" => out.push('τ'),
                "phi" | "varphi" => out.push('φ'),
                "Phi" => out.push('Φ'),
                "chi" => out.push('χ'),
                "psi" => out.push('ψ'),
                "Psi" => out.push('Ψ'),
                "omega" => out.push('ω'),
                "Omega" => out.push('Ω'),
                "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "log" | "ln" | "exp" | "lim" | "max" | "min" => {
                    out.push_str(&cmd);
                }
                _ => out.push_str(&cmd),
            }
            continue;
        }
        if ch == '^' {
            i += 1;
            if let Some(raw) = read_braced(&chars, &mut i) {
                let inner = latex_to_unicode(&raw);
                if inner == "°" {
                    out.push('°');
                } else if let Some(sup) = to_superscript(&inner) {
                    out.push_str(&sup);
                } else if is_simple_math_atom(&inner) {
                    out.push('^');
                    out.push_str(&inner);
                } else {
                    out.push_str("^(");
                    out.push_str(&inner);
                    out.push(')');
                }
            }
            continue;
        }
        if ch == '_' {
            i += 1;
            if let Some(raw) = read_braced(&chars, &mut i) {
                let inner = latex_to_unicode(&raw);
                if let Some(sub) = to_subscript(&inner) {
                    out.push_str(&sub);
                } else {
                    out.push('_');
                    out.push_str(&inner);
                }
            }
            continue;
        }
        if ch == '{' || ch == '}' {
            i += 1;
            continue;
        }
        out.push(ch);
        i += 1;
    }

    // Collapse multiple consecutive spaces cleanly.
    let mut clean = String::with_capacity(out.len());
    let mut prev_sp = false;
    for c in out.trim().chars() {
        if c == ' ' {
            if !prev_sp {
                clean.push(' ');
                prev_sp = true;
            }
        } else {
            clean.push(c);
            prev_sp = false;
        }
    }
    clean
}

/// Replaces inline LaTeX math segments (`$$...$$`, `\[...\]`, `\(...\)`, `$...$`) with clean Unicode.
fn replace_inline_math(s: &str) -> String {
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;
    let mut out = String::with_capacity(n);

    while i < n {
        if bytes[i] == b'$' {
            if i + 1 < n && bytes[i + 1] == b'$' {
                if let Some(rel) = s[i + 2..].find("$$") {
                    out.push_str(&latex_to_unicode(&s[i + 2..i + 2 + rel]));
                    i += 4 + rel;
                    continue;
                }
                // Unclosed `$$` during live streaming: strip delimiter and convert what's arrived.
                out.push_str(&latex_to_unicode(&s[i + 2..]));
                break;
            }
            // Single `$...$` inline math (avoid matching currency like `$10` by requiring non-space inner bounds).
            if i + 1 < n && bytes[i + 1] != b' ' {
                if let Some(rel) = s[i + 1..].find('$') {
                    let inner = &s[i + 1..i + 1 + rel];
                    let looks_currency = inner.bytes().next().is_some_and(|b| b.is_ascii_digit())
                        && inner.contains(' ')
                        && !inner.contains('\\')
                        && !inner.contains('^')
                        && !inner.contains('=');
                    if !inner.is_empty() && !inner.ends_with(' ') && !looks_currency {
                        out.push_str(&latex_to_unicode(inner));
                        i += 2 + rel;
                        continue;
                    }
                }
            }
        }
        if bytes[i] == b'\\' && i + 1 < n && (bytes[i + 1] == b'(' || bytes[i + 1] == b'[') {
            let close = if bytes[i + 1] == b'(' { "\\)" } else { "\\]" };
            if let Some(rel) = s[i + 2..].find(close) {
                out.push_str(&latex_to_unicode(&s[i + 2..i + 2 + rel]));
                i += 4 + rel;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn push_span(out: &mut Vec<MdSpan>, style: SpanStyle, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut() {
        if last.style == style {
            last.text.push_str(text);
            return;
        }
    }
    out.push(MdSpan { style, text: text.to_string() });
}

fn parse_inline_spans(raw: &str) -> Vec<MdSpan> {
    let s = replace_inline_math(raw);
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;
    let mut out = Vec::new();
    let mut buf = String::new();

    while i < n {
        // Inline code `` `...` ``
        if bytes[i] == b'`' {
            if let Some(rel) = s[i + 1..].find('`') {
                if !buf.is_empty() {
                    push_span(&mut out, SpanStyle::Normal, &buf);
                    buf.clear();
                }
                push_span(&mut out, SpanStyle::Code, &s[i + 1..i + 1 + rel]);
                i += 2 + rel;
                continue;
            }
        }
        // Bold `**...**` or `__...__`
        if i + 1 < n && ((bytes[i] == b'*' && bytes[i + 1] == b'*') || (bytes[i] == b'_' && bytes[i + 1] == b'_')) {
            let delim = if bytes[i] == b'*' { "**" } else { "__" };
            if let Some(rel) = s[i + 2..].find(delim) {
                if rel > 0 {
                    if !buf.is_empty() {
                        push_span(&mut out, SpanStyle::Normal, &buf);
                        buf.clear();
                    }
                    push_span(&mut out, SpanStyle::Bold, &s[i + 2..i + 2 + rel]);
                    i += 4 + rel;
                    continue;
                }
            } else {
                // Streaming unclosed bold marker: drop `**` so user never sees raw asterisks.
                if !buf.is_empty() {
                    push_span(&mut out, SpanStyle::Normal, &buf);
                    buf.clear();
                }
                push_span(&mut out, SpanStyle::Bold, &s[i + 2..]);
                break;
            }
        }
        // Italic `*...*` (single asterisk not followed by space)
        if bytes[i] == b'*' && i + 1 < n && bytes[i + 1] != b' ' && bytes[i + 1] != b'*' {
            if let Some(rel) = s[i + 1..].find('*') {
                let inner = &s[i + 1..i + 1 + rel];
                if !inner.is_empty() && !inner.ends_with(' ') {
                    if !buf.is_empty() {
                        push_span(&mut out, SpanStyle::Normal, &buf);
                        buf.clear();
                    }
                    push_span(&mut out, SpanStyle::Italic, inner);
                    i += 2 + rel;
                    continue;
                }
            }
        }
        let ch = s[i..].chars().next().unwrap();
        buf.push(ch);
        i += ch.len_utf8();
    }
    if !buf.is_empty() {
        push_span(&mut out, SpanStyle::Normal, &buf);
    }
    out
}

fn parse_numbered_prefix(t: &str) -> Option<(&str, &str)> {
    let b = t.as_bytes();
    let mut k = 0usize;
    while k < b.len() && k < 3 && b[k].is_ascii_digit() {
        k += 1;
    }
    if k > 0 && k + 1 < b.len() && (b[k] == b'.' || b[k] == b')') && b[k + 1] == b' ' {
        Some((&t[..=k], t[k + 2..].trim_start()))
    } else {
        None
    }
}

/// Parses a Gemini Markdown/LaTeX response into clean visual blocks for the HUD reading card.
pub fn parse_markdown(src: &str) -> Vec<MdBlock> {
    let mut blocks = Vec::new();
    let mut in_code = false;
    let mut code_lines: Vec<String> = Vec::new();
    let mut in_math = false;
    let mut math_buf = String::new();

    for raw_line in src.lines() {
        let trimmed_end = raw_line.trim_end();
        let trimmed = trimmed_end.trim();

        // Fenced code blocks ```
        if trimmed.starts_with("```") {
            if in_code {
                blocks.push(MdBlock::CodeBlock(std::mem::take(&mut code_lines)));
                in_code = false;
            } else {
                in_code = true;
            }
            continue;
        }
        if in_code {
            code_lines.push(trimmed_end.to_string());
            continue;
        }

        // Multi-line display math blocks `$$` or `\[` ... `\]`
        if in_math {
            if trimmed == "$$" || trimmed == "\\]" {
                let expr = latex_to_unicode(&math_buf);
                math_buf.clear();
                in_math = false;
                if !expr.is_empty() {
                    blocks.push(MdBlock::Formula(expr));
                }
            } else if let Some(before) = trimmed.strip_suffix("$$").or_else(|| trimmed.strip_suffix("\\]")) {
                if !math_buf.is_empty() {
                    math_buf.push(' ');
                }
                math_buf.push_str(before);
                let expr = latex_to_unicode(&math_buf);
                math_buf.clear();
                in_math = false;
                if !expr.is_empty() {
                    blocks.push(MdBlock::Formula(expr));
                }
            } else {
                if !math_buf.is_empty() {
                    math_buf.push(' ');
                }
                math_buf.push_str(trimmed);
            }
            continue;
        }

        if trimmed.is_empty() {
            if !matches!(blocks.last(), None | Some(MdBlock::Blank)) {
                blocks.push(MdBlock::Blank);
            }
            continue;
        }

        // Horizontal rule `---` / `***` -> clean separator blank
        if trimmed.len() >= 3 && (trimmed.chars().all(|c| c == '-') || trimmed.chars().all(|c| c == '*')) {
            if !matches!(blocks.last(), None | Some(MdBlock::Blank)) {
                blocks.push(MdBlock::Blank);
            }
            continue;
        }

        // Single-line display formula `$$...$$` or `\[...\]`
        if trimmed == "$$" || trimmed == "\\[" {
            in_math = true;
            math_buf.clear();
            continue;
        }
        if let Some(inner) = trimmed
            .strip_prefix("$$")
            .and_then(|s| s.strip_suffix("$$"))
            .or_else(|| trimmed.strip_prefix("\\[").and_then(|s| s.strip_suffix("\\]")))
        {
            let expr = latex_to_unicode(inner);
            if !expr.is_empty() {
                blocks.push(MdBlock::Formula(expr));
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("$$").or_else(|| trimmed.strip_prefix("\\[")) {
            in_math = true;
            math_buf.clear();
            math_buf.push_str(rest);
            continue;
        }

        // Headings `# `, `## `, `### `
        let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
        if hashes >= 1 && hashes <= 6 && trimmed.as_bytes().get(hashes) == Some(&b' ') {
            let body = trimmed[hashes + 1..].trim();
            blocks.push(MdBlock::Heading(parse_inline_spans(body)));
            continue;
        }

        // Bullet items `* `, `- `, `• ` (with optional leading indentation)
        let lead_spaces = trimmed_end.bytes().take_while(|&b| b == b' ').count();
        let indent = (lead_spaces / 2).min(2) as u8;
        if let Some(rest) = trimmed
            .strip_prefix("* ")
            .or_else(|| trimmed.strip_prefix("- "))
            .or_else(|| trimmed.strip_prefix("• "))
        {
            blocks.push(MdBlock::Bullet {
                indent,
                marker: "•".to_string(),
                spans: parse_inline_spans(rest.trim_start()),
            });
            continue;
        }

        // Numbered list `1. `, `2. `
        if let Some((num, rest)) = parse_numbered_prefix(trimmed) {
            blocks.push(MdBlock::Bullet {
                indent,
                marker: num.to_string(),
                spans: parse_inline_spans(rest),
            });
            continue;
        }

        // Blockquote `> `
        if let Some(rest) = trimmed.strip_prefix("> ") {
            blocks.push(MdBlock::Paragraph(parse_inline_spans(rest)));
            continue;
        }

        blocks.push(MdBlock::Paragraph(parse_inline_spans(trimmed)));
    }

    if in_code && !code_lines.is_empty() {
        blocks.push(MdBlock::CodeBlock(code_lines));
    }
    if in_math && !math_buf.is_empty() {
        let expr = latex_to_unicode(&math_buf);
        if !expr.is_empty() {
            blocks.push(MdBlock::Formula(expr));
        }
    }
    while matches!(blocks.last(), Some(MdBlock::Blank)) {
        blocks.pop();
    }
    blocks
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

    #[test]
    fn formats_latex_and_markdown_cleanly() {
        assert_eq!(latex_to_unicode(r"\mathbf{c^2 = a^2 + b^2}"), "c² = a² + b²");
        assert_eq!(latex_to_unicode(r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}"), "x = (-b ± √(b² - 4ac)) / 2a");
        assert_eq!(latex_to_unicode(r"A = \pi r^2"), "A = π r²");

        let sample = "Rumus Pythagoras untuk segitiga siku-siku:\n\n$$\\mathbf{c^2 = a^2 + b^2}$$\n\n* **c** = Sisi miring\n* **a** dan **b** = Sisi tegak";
        let blocks = parse_markdown(sample);
        assert!(matches!(&blocks[0], MdBlock::Paragraph(_)));
        assert_eq!(blocks[2], MdBlock::Formula("c² = a² + b²".to_string()));
        assert_eq!(
            blocks[4],
            MdBlock::Bullet {
                indent: 0,
                marker: "•".into(),
                spans: vec![
                    MdSpan { style: SpanStyle::Bold, text: "c".into() },
                    MdSpan { style: SpanStyle::Normal, text: " = Sisi miring".into() },
                ],
            }
        );
    }
}
