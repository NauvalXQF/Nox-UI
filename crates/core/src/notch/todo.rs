use super::json::{self, Json};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TodoItem {
    pub id: String,
    pub remote_id: Option<String>,
    pub title: String,
    pub completed: bool,
    pub due: Option<String>,
    pub dirty: bool,
    pub deleted: bool,
    pub updated_ms: i64,
}

#[derive(Clone, Debug)]
pub struct RemoteTask {
    pub id: String,
    pub title: String,
    pub completed: bool,
    pub due: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct TodoStore {
    items: Vec<TodoItem>,
    next_local: u64,
}

impl TodoStore {
    pub fn new() -> Self {
        Self { items: Vec::new(), next_local: 1 }
    }

    pub fn visible(&self) -> Vec<&TodoItem> {
        let mut active: Vec<&TodoItem> = self.items.iter().filter(|t| !t.deleted && !t.completed).collect();
        let mut done: Vec<&TodoItem> = self.items.iter().filter(|t| !t.deleted && t.completed).collect();
        active.append(&mut done);
        active
    }

    pub fn active_count(&self) -> usize {
        self.items.iter().filter(|t| !t.deleted && !t.completed).count()
    }

    pub fn all(&self) -> &[TodoItem] {
        &self.items
    }

    pub fn get(&self, id: &str) -> Option<&TodoItem> {
        self.items.iter().find(|t| t.id == id)
    }

    pub fn add_local(&mut self, title: &str, now_ms: i64) -> Option<TodoItem> {
        let clean = title.trim();
        if clean.is_empty() {
            return None;
        }
        let id = format!("local-{}-{}", now_ms, self.next_local);
        self.next_local += 1;
        let item = TodoItem {
            id,
            remote_id: None,
            title: clean.to_string(),
            completed: false,
            due: None,
            dirty: true,
            deleted: false,
            updated_ms: now_ms,
        };
        self.items.insert(0, item.clone());
        Some(item)
    }

    pub fn toggle(&mut self, id: &str, now_ms: i64) -> Option<TodoItem> {
        let it = self.items.iter_mut().find(|t| t.id == id && !t.deleted)?;
        it.completed = !it.completed;
        it.dirty = true;
        it.updated_ms = now_ms;
        Some(it.clone())
    }

    pub fn delete(&mut self, id: &str, now_ms: i64) -> Option<TodoItem> {
        let pos = self.items.iter().position(|t| t.id == id)?;
        if self.items[pos].remote_id.is_none() {
            return Some(self.items.remove(pos));
        }
        self.items[pos].deleted = true;
        self.items[pos].dirty = true;
        self.items[pos].updated_ms = now_ms;
        Some(self.items[pos].clone())
    }

    pub fn bind_remote(&mut self, local_id: &str, remote_id: String) {
        if let Some(it) = self.items.iter_mut().find(|t| t.id == local_id) {
            it.remote_id = Some(remote_id);
            it.dirty = false;
        }
    }

    pub fn mark_synced(&mut self, id: &str) {
        if let Some(pos) = self.items.iter().position(|t| t.id == id) {
            if self.items[pos].deleted {
                self.items.remove(pos);
            } else {
                self.items[pos].dirty = false;
            }
        }
    }

    /// Merges remote Google Tasks list while preserving local unsynced (`dirty`) edits.
    pub fn merge_remote(&mut self, remote: Vec<RemoteTask>, now_ms: i64) {
        let mut seen_remote = std::collections::HashSet::new();
        for r in remote {
            seen_remote.insert(r.id.clone());
            if let Some(local) = self.items.iter_mut().find(|t| t.remote_id.as_deref() == Some(r.id.as_str())) {
                if !local.dirty {
                    local.title = r.title;
                    local.completed = r.completed;
                    local.due = r.due;
                }
            } else {
                self.items.push(TodoItem {
                    id: format!("g-{}", r.id),
                    remote_id: Some(r.id),
                    title: r.title,
                    completed: r.completed,
                    due: r.due,
                    dirty: false,
                    deleted: false,
                    updated_ms: now_ms,
                });
            }
        }
        // Remove clean remote items that were deleted on mobile/web.
        self.items.retain(|t| {
            if t.dirty {
                return true;
            }
            match &t.remote_id {
                Some(rid) => seen_remote.contains(rid),
                None => !t.deleted,
            }
        });
    }

    pub fn serialize(&self) -> String {
        let mut out = String::from("[");
        for (i, t) in self.items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"id\":");
            push_json_str(&mut out, &t.id);
            out.push_str(",\"remote_id\":");
            match &t.remote_id {
                Some(r) => push_json_str(&mut out, r),
                None => out.push_str("null"),
            }
            out.push_str(",\"title\":");
            push_json_str(&mut out, &t.title);
            out.push_str(",\"completed\":");
            out.push_str(if t.completed { "true" } else { "false" });
            out.push_str(",\"due\":");
            match &t.due {
                Some(d) => push_json_str(&mut out, d),
                None => out.push_str("null"),
            }
            out.push_str(",\"dirty\":");
            out.push_str(if t.dirty { "true" } else { "false" });
            out.push_str(",\"deleted\":");
            out.push_str(if t.deleted { "true" } else { "false" });
            out.push_str(&format!(",\"updated_ms\":{}}}", t.updated_ms));
        }
        out.push(']');
        out
    }

    pub fn parse(src: &str) -> Self {
        let mut store = Self::new();
        let Some(Json::Arr(arr)) = json::parse(src) else { return store };
        for v in arr {
            let Some(id) = v.get("id").and_then(Json::as_str) else { continue };
            let Some(title) = v.get("title").and_then(Json::as_str) else { continue };
            store.items.push(TodoItem {
                id: id.to_string(),
                remote_id: v.get("remote_id").and_then(Json::as_str).map(String::from),
                title: title.to_string(),
                completed: v.get("completed").and_then(Json::as_bool).unwrap_or(false),
                due: v.get("due").and_then(Json::as_str).map(String::from),
                dirty: v.get("dirty").and_then(Json::as_bool).unwrap_or(false),
                deleted: v.get("deleted").and_then(Json::as_bool).unwrap_or(false),
                updated_ms: v.get("updated_ms").and_then(Json::as_f64).unwrap_or(0.0) as i64,
            });
        }
        store
    }
}

pub fn push_json_str(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todo_crud_roundtrip_and_remote_merge() {
        let mut s = TodoStore::new();
        let a = s.add_local("Beli kopi", 1000).unwrap();
        let b = s.add_local("Kirim laporan", 1001).unwrap();
        assert_eq!(s.active_count(), 2);
        s.toggle(&a.id, 1002);
        assert_eq!(s.active_count(), 1);
        s.bind_remote(&b.id, "gt-1".into());
        s.merge_remote(
            vec![
                RemoteTask { id: "gt-1".into(), title: "Kirim laporan (revisi)".into(), completed: false, due: Some("2026-11-22".into()) },
                RemoteTask { id: "gt-2".into(), title: "Dari HP".into(), completed: false, due: None },
            ],
            2000,
        );
        assert_eq!(s.visible().len(), 3);
        let serialized = s.serialize();
        let back = TodoStore::parse(&serialized);
        assert_eq!(back.all().len(), s.all().len());
        assert_eq!(back.get(&b.id).unwrap().title, "Kirim laporan (revisi)");
    }
}
