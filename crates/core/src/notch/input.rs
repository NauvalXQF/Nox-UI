/// Pure-Rust single-line UTF-8 text buffer with cursor, select-all, and UTF-16 surrogate pair
/// assembly (for Win32 `WM_CHAR`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextBuffer {
    text: String,
    /// Byte offset of the caret (always on a UTF-8 char boundary).
    cursor: usize,
    /// True when the entire text is selected (e.g., via Ctrl+A).
    all_selected: bool,
    /// Pending UTF-16 high surrogate (0xD800..=0xDBFF) waiting for its low surrogate.
    high_surrogate: Option<u16>,
    /// Maximum allowed Unicode scalar values.
    max_chars: usize,
}

impl TextBuffer {
    pub fn new(max_chars: usize) -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            all_selected: false,
            high_surrogate: None,
            max_chars: max_chars.max(1),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn cursor_byte(&self) -> usize {
        self.cursor
    }

    pub fn before_cursor(&self) -> &str {
        &self.text[..self.cursor]
    }

    pub fn is_all_selected(&self) -> bool {
        self.all_selected && !self.text.is_empty()
    }

    pub fn set(&mut self, s: &str) {
        self.text = s.chars().filter(|c| !c.is_control()).take(self.max_chars).collect();
        self.cursor = self.text.len();
        self.all_selected = false;
        self.high_surrogate = None;
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.all_selected = false;
        self.high_surrogate = None;
    }

    pub fn select_all(&mut self) {
        self.all_selected = !self.text.is_empty();
        self.cursor = self.text.len();
    }

    fn delete_selection_if_any(&mut self) -> bool {
        if self.all_selected {
            self.text.clear();
            self.cursor = 0;
            self.all_selected = false;
            true
        } else {
            false
        }
    }

    pub fn insert_char(&mut self, c: char) -> bool {
        if c.is_control() {
            return false;
        }
        self.delete_selection_if_any();
        if self.text.chars().count() >= self.max_chars {
            return false;
        }
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        true
    }

    /// Processes a raw Win32 `WM_CHAR` UTF-16 code unit, assembling surrogate pairs for emoji.
    pub fn push_utf16(&mut self, unit: u16) -> bool {
        if (0xD800..=0xDBFF).contains(&unit) {
            self.high_surrogate = Some(unit);
            return false;
        }
        if (0xDC00..=0xDFFF).contains(&unit) {
            if let Some(hi) = self.high_surrogate.take() {
                let cp = 0x10000 + (((hi as u32 - 0xD800) << 10) | (unit as u32 - 0xDC00));
                if let Some(c) = char::from_u32(cp) {
                    return self.insert_char(c);
                }
            }
            return false;
        }
        self.high_surrogate = None;
        if unit < 32 || unit == 127 {
            return false;
        }
        char::from_u32(unit as u32).is_some_and(|c| self.insert_char(c))
    }

    pub fn paste(&mut self, s: &str) -> bool {
        self.high_surrogate = None;
        let clean: String = s
            .chars()
            .map(|c| if c == '\r' || c == '\n' || c == '\t' { ' ' } else { c })
            .filter(|c| !c.is_control())
            .collect();
        if clean.is_empty() {
            return false;
        }
        self.delete_selection_if_any();
        let avail = self.max_chars.saturating_sub(self.text.chars().count());
        if avail == 0 {
            return false;
        }
        let add: String = clean.chars().take(avail).collect();
        self.text.insert_str(self.cursor, &add);
        self.cursor += add.len();
        true
    }

    pub fn backspace(&mut self) -> bool {
        self.high_surrogate = None;
        if self.delete_selection_if_any() {
            return true;
        }
        if self.cursor == 0 {
            return false;
        }
        let prev = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.text.drain(prev..self.cursor);
        self.cursor = prev;
        true
    }

    pub fn delete(&mut self) -> bool {
        self.high_surrogate = None;
        if self.delete_selection_if_any() {
            return true;
        }
        if self.cursor >= self.text.len() {
            return false;
        }
        let next = self.text[self.cursor..]
            .chars()
            .next()
            .map(|c| self.cursor + c.len_utf8())
            .unwrap_or(self.text.len());
        self.text.drain(self.cursor..next);
        true
    }

    pub fn move_left(&mut self) -> bool {
        self.high_surrogate = None;
        let was_sel = self.all_selected;
        self.all_selected = false;
        if self.cursor == 0 {
            return was_sel;
        }
        self.cursor = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
        true
    }

    pub fn move_right(&mut self) -> bool {
        self.high_surrogate = None;
        let was_sel = self.all_selected;
        self.all_selected = false;
        if self.cursor >= self.text.len() {
            return was_sel;
        }
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.cursor += c.len_utf8();
        }
        true
    }

    pub fn move_home(&mut self) -> bool {
        self.high_surrogate = None;
        let changed = self.cursor != 0 || self.all_selected;
        self.cursor = 0;
        self.all_selected = false;
        changed
    }

    pub fn move_end(&mut self) -> bool {
        self.high_surrogate = None;
        let changed = self.cursor != self.text.len() || self.all_selected;
        self.cursor = self.text.len();
        self.all_selected = false;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_cursor_surrogates_and_paste() {
        let mut b = TextBuffer::new(32);
        assert!(b.paste("Halo\ndunia"));
        assert_eq!(b.as_str(), "Halo dunia");
        assert!(b.move_left());
        assert!(b.move_left());
        assert!(b.insert_char('!'));
        assert_eq!(b.as_str(), "Halo dun!ia");
        assert!(b.backspace());
        assert_eq!(b.as_str(), "Halo dunia");
        assert!(b.move_end());
        // Emoji 🚀 = U+1F680 = D83D DE80
        assert!(!b.push_utf16(0xD83D));
        assert!(b.push_utf16(0xDE80));
        assert_eq!(b.as_str(), "Halo dunia🚀");
        assert!(b.backspace());
        assert_eq!(b.as_str(), "Halo dunia");
        b.select_all();
        assert!(b.is_all_selected());
        assert!(b.insert_char('X'));
        assert_eq!(b.as_str(), "X");
        assert!(b.move_home());
        assert!(b.delete());
        assert!(b.is_empty());
    }
}
