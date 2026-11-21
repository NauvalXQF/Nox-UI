#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub aumid: String,
    pub title: String,
    pub body: String,
    pub timestamp_s: i64,
    pub read: bool,
}

#[derive(Clone, Debug)]
pub struct NotifStore {
    items: Vec<Notification>,
    max: usize,
}

impl NotifStore {
    pub fn new(max: usize) -> Self {
        Self { items: Vec::new(), max: max.max(1) }
    }

    pub fn items(&self) -> &[Notification] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn unread_count(&self) -> usize {
        self.items.iter().filter(|n| !n.read).count()
    }

    pub fn get(&self, id: u32) -> Option<&Notification> {
        self.items.iter().find(|n| n.id == id)
    }

    /// Inserts or updates a notification at the front (newest-first). Returns `true` if newly added.
    pub fn push(&mut self, notif: Notification) -> bool {
        let existed = if let Some(pos) = self.items.iter().position(|n| n.id == notif.id) {
            self.items.remove(pos);
            true
        } else {
            false
        };
        self.items.insert(0, notif);
        self.items.truncate(self.max);
        !existed
    }

    pub fn mark_read(&mut self, id: u32) {
        if let Some(n) = self.items.iter_mut().find(|n| n.id == id) {
            n.read = true;
        }
    }

    pub fn mark_all_read(&mut self) {
        for n in &mut self.items {
            n.read = true;
        }
    }

    pub fn dismiss(&mut self, id: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|n| n.id != id);
        self.items.len() != before
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }
}

pub fn relative_time(now_s: i64, ts_s: i64, en: bool) -> String {
    let diff = (now_s - ts_s).max(0);
    if diff < 60 {
        if en { "now".into() } else { "baru".into() }
    } else if diff < 3600 {
        let m = diff / 60;
        if en { format!("{m}m") } else { format!("{m}m") }
    } else if diff < 86400 {
        let h = diff / 3600;
        if en { format!("{h}h") } else { format!("{h}j") }
    } else {
        let d = diff / 86400;
        if en { format!("{d}d") } else { format!("{d}h") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(id: u32, title: &str) -> Notification {
        Notification {
            id,
            app_name: "WhatsApp".into(),
            aumid: "WhatsApp!App".into(),
            title: title.into(),
            body: "Halo".into(),
            timestamp_s: 1000 + id as i64,
            read: false,
        }
    }

    #[test]
    fn store_dedups_caps_tracks_unread_and_formats_time() {
        let mut s = NotifStore::new(3);
        assert!(s.push(mk(1, "A")));
        assert!(s.push(mk(2, "B")));
        assert!(!s.push(mk(1, "A2")));
        assert_eq!(s.len(), 2);
        assert_eq!(s.items()[0].title, "A2");
        assert_eq!(s.unread_count(), 2);
        s.mark_read(1);
        assert_eq!(s.unread_count(), 1);
        s.push(mk(3, "C"));
        s.push(mk(4, "D"));
        assert_eq!(s.len(), 3);
        assert!(s.dismiss(3));
        assert!(!s.dismiss(999));
        assert_eq!(relative_time(1000, 990, true), "now");
        assert_eq!(relative_time(1300, 1000, true), "5m");
        assert_eq!(relative_time(9000, 1000, false), "2j");
    }
}
