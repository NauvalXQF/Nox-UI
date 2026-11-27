pub fn find_link(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let start = ["https://", "http://"].iter().filter_map(|s| lower.find(s)).min()?;
    let rest = &text[start..];
    let end = rest.find(|c: char| c.is_whitespace() || matches!(c, '"' | '<' | '>' | '\'' | ')' | ']' | '}')).unwrap_or(rest.len());
    let link = rest[..end].trim_end_matches(['.', ',', ';', ':', '!', '?']);
    (link.len() > 10).then(|| link.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    pub title: String,
    pub start: u32,
    pub minutes: u32,
    pub link: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Soon(u32),
    Live(u32),
}

pub const LEAD_MIN: u32 = 10;

pub fn current(slots: &[Slot], now: u32) -> Option<(usize, When)> {
    let live = slots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.start <= now && now < s.start + s.minutes.max(1))
        .max_by_key(|(_, s)| s.start)
        .map(|(i, s)| (i, When::Live(now - s.start)));
    let soon = slots
        .iter()
        .enumerate()
        .filter(|(_, s)| s.start > now && s.start - now <= LEAD_MIN)
        .min_by_key(|(_, s)| s.start)
        .map(|(i, s)| (i, When::Soon(s.start - now)));
    match (live, soon) {
        (Some(_), Some(s)) if matches!(s.1, When::Soon(m) if m <= 2) => Some(s),
        (Some(l), _) => Some(l),
        (None, s) => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_found_in_messy_text() {
        assert_eq!(find_link("Join: https://meet.google.com/abc-defg-hij."), Some("https://meet.google.com/abc-defg-hij".into()));
        assert_eq!(find_link("<https://teams.microsoft.com/l/meetup-join/xyz>"), Some("https://teams.microsoft.com/l/meetup-join/xyz".into()));
        assert_eq!(find_link("Ruang 3, lantai 2"), None);
        assert_eq!(find_link("HTTPS://ZOOM.US/j/123456789 pwd"), Some("HTTPS://ZOOM.US/j/123456789".into()));
    }

    fn slot(start: u32, minutes: u32) -> Slot {
        Slot { title: format!("r{start}"), start, minutes, link: None }
    }

    #[test]
    fn meeting_states() {
        let s = vec![slot(600, 30), slot(660, 60)];
        assert_eq!(current(&s, 580), None);
        assert_eq!(current(&s, 592), Some((0, When::Soon(8))));
        assert_eq!(current(&s, 600), Some((0, When::Live(0))));
        assert_eq!(current(&s, 615), Some((0, When::Live(15))));
        assert_eq!(current(&s, 635), None);
        assert_eq!(current(&s, 655), Some((1, When::Soon(5))));
        let b2b = vec![slot(600, 30), slot(630, 30)];
        assert_eq!(current(&b2b, 628), Some((1, When::Soon(2))));
    }
}
