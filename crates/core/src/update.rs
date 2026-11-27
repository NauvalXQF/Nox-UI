use crate::notch::json;

pub const REPO: &str = "NauvalXQF/Nox-UI";
pub const API: &str = "https://api.github.com/repos/NauvalXQF/Nox-UI/releases/latest";
pub const RELEASES: &str = "https://github.com/NauvalXQF/Nox-UI/releases/latest";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub page: String,
    pub asset_url: Option<String>,
    pub asset_name: Option<String>,
    pub sha256: Option<String>,
}

pub fn parse_version(s: &str) -> Option<(u32, u32, u32, bool)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match s.split_once(['-', '+']) {
        Some((c, _)) => (c, true),
        None => (s, false),
    };
    let mut it = core.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next().unwrap_or("0").parse().ok()?;
    let c = it.next().unwrap_or("0").parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((a, b, c, pre))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some((a, b, c, pa)), Some((x, y, z, px))) => {
            let (l, r) = ((a, b, c), (x, y, z));
            l > r || (l == r && px && !pa)
        }
        _ => false,
    }
}

pub fn parse_release(body: &str) -> Option<Release> {
    parse_release_from(body, &format!("https://github.com/{REPO}/releases/download/"))
}

pub fn parse_release_from(body: &str, allowed: &str) -> Option<Release> {
    let j = json::parse(body)?;
    if j.get("draft").and_then(|v| v.as_bool()) == Some(true) || j.get("prerelease").and_then(|v| v.as_bool()) == Some(true) {
        return None;
    }
    let tag = j.get("tag_name")?.as_str()?;
    parse_version(tag)?;
    let version = tag.trim_start_matches(['v', 'V']).to_string();
    let page = j.get("html_url").and_then(|v| v.as_str()).unwrap_or(RELEASES).to_string();
    let mut out = Release { version, page, asset_url: None, asset_name: None, sha256: None };
    for a in j.get("assets").and_then(|v| v.as_arr()).unwrap_or(&[]) {
        let Some(name) = a.get("name").and_then(|v| v.as_str()) else { continue };
        let lower = name.to_lowercase();
        let is_nox_zip = lower.starts_with("nox-") && lower.ends_with(".zip");
        let is_nox_setup = lower.starts_with("nox-setup-") && lower.ends_with(".exe");
        if !is_nox_zip && !is_nox_setup {
            continue;
        }
        let Some(url) = a.get("browser_download_url").and_then(|v| v.as_str()) else { continue };
        if !url.starts_with(allowed) {
            continue;
        }
        out.asset_url = Some(url.to_string());
        out.asset_name = Some(name.to_string());
        out.sha256 = a
            .get("digest")
            .and_then(|v| v.as_str())
            .and_then(|d| d.strip_prefix("sha256:"))
            .filter(|h| h.len() == 64 && h.bytes().all(|c| c.is_ascii_hexdigit()))
            .map(|h| h.to_ascii_lowercase());
        if is_nox_zip {
            break;
        }
    }
    Some(out)
}

pub fn safe_file_name(name: &str) -> bool {
    !name.is_empty() && name.len() < 80 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) && !name.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("v0.6.1", "0.6.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.12"));
        assert!(!is_newer("0.6.0", "0.6.0"));
        assert!(!is_newer("0.5.9", "0.6.0"));
        assert!(is_newer("0.6.0", "0.6.0-beta"));
        assert!(!is_newer("0.6.0-beta", "0.6.0"));
        assert!(!is_newer("abc", "0.6.0"));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3, false)));
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn picks_the_nox_zip_asset_and_digest() {
        let body = r#"{"tag_name":"v0.6.1","html_url":"https://github.com/NauvalXQF/Nox-UI/releases/tag/v0.6.1","draft":false,"prerelease":false,
          "assets":[{"name":"notes.txt","browser_download_url":"https://github.com/NauvalXQF/Nox-UI/releases/download/v0.6.1/notes.txt","digest":"sha256:aa"},
                    {"name":"NOX-v0.6.1-win64.zip","browser_download_url":"https://github.com/NauvalXQF/Nox-UI/releases/download/v0.6.1/NOX-v0.6.1-win64.zip","digest":"sha256:16BBBADC940B1EC031B0CCB8FD2B7C1F1D2EC5D2C0342114A13A0DEBF3120DFB"}]}"#;
        let r = parse_release(body).unwrap();
        assert_eq!(r.version, "0.6.1");
        assert_eq!(r.asset_name.as_deref(), Some("NOX-v0.6.1-win64.zip"));
        assert_eq!(r.sha256.as_deref(), Some("16bbbadc940b1ec031b0ccb8fd2b7c1f1d2ec5d2c0342114a13a0debf3120dfb"));
    }

    #[test]
    fn rejects_foreign_urls_missing_digest_and_prereleases() {
        let foreign = r#"{"tag_name":"v9.0.0","assets":[{"name":"NOX-v9.0.0-win64.zip","browser_download_url":"https://evil.example/NOX-v9.0.0-win64.zip","digest":"sha256:16bbbadc940b1ec031b0ccb8fd2b7c1f1d2ec5d2c0342114a13a0debf3120dfb"}]}"#;
        assert_eq!(parse_release(foreign).unwrap().asset_url, None);
        let nodigest = r#"{"tag_name":"v9.0.0","assets":[{"name":"NOX-v9.0.0-win64.zip","browser_download_url":"https://github.com/NauvalXQF/Nox-UI/releases/download/v9.0.0/NOX-v9.0.0-win64.zip"}]}"#;
        let r = parse_release(nodigest).unwrap();
        assert!(r.asset_url.is_some() && r.sha256.is_none());
        assert!(parse_release(r#"{"tag_name":"v9.0.0","prerelease":true,"assets":[]}"#).is_none());
        assert!(parse_release(r#"{"tag_name":"latest"}"#).is_none());
        assert!(parse_release("not json").is_none());
    }

    #[test]
    fn file_names_are_checked() {
        assert!(safe_file_name("NOX-v0.6.0-win64.zip"));
        assert!(!safe_file_name("..\\evil.zip"));
        assert!(!safe_file_name("a/b.zip"));
    }
}
