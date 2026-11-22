use std::sync::Mutex;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};
use windows::Win32::UI::Shell::{
    BHID_EnumItems, FOLDERID_AppsFolder, IEnumShellItems, IShellItem, SHGetKnownFolderItem, KF_FLAG_DEFAULT, SIGDN_NORMALDISPLAY,
    SIGDN_PARENTRELATIVEPARSING,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub name: String,
    pub path: String,
}

static CACHE: Mutex<Option<Vec<App>>> = Mutex::new(None);
static LOADING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn load() -> Vec<App> {
    let mut out = Vec::new();
    unsafe {
        let Ok(folder) = SHGetKnownFolderItem::<IShellItem>(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None) else { return out };
        let Ok(en) = folder.BindToHandler::<_, IEnumShellItems>(None, &BHID_EnumItems) else { return out };
        loop {
            let mut batch: [Option<IShellItem>; 16] = Default::default();
            let mut got = 0u32;
            if en.Next(&mut batch, Some(&mut got)).is_err() || got == 0 {
                break;
            }
            for item in batch.iter().take(got as usize).flatten() {
                let name = item.GetDisplayName(SIGDN_NORMALDISPLAY).ok().and_then(|p| {
                    let s = p.to_string().ok();
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                });
                let rel = item.GetDisplayName(SIGDN_PARENTRELATIVEPARSING).ok().and_then(|p| {
                    let s = p.to_string().ok();
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                });
                if let (Some(name), Some(rel)) = (name, rel) {
                    if !name.trim().is_empty() {
                        out.push(App { name, path: format!("shell:AppsFolder\\{rel}") });
                    }
                }
            }
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.path == b.path);
    out
}

pub fn refresh(notify: impl Fn() + Send + 'static) {
    if LOADING.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let list = load();
        crate::log::line("apps", &format!("indeks: {} aplikasi", list.len()));
        *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some(list);
        LOADING.store(false, std::sync::atomic::Ordering::Release);
        notify();
    });
}

pub fn snapshot() -> Vec<App> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}

pub fn name_for_path(path: &str) -> Option<String> {
    let g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    g.as_ref()?.iter().find(|a| a.path.eq_ignore_ascii_case(path)).map(|a| a.name.clone())
}

pub fn ensure_loaded() {
    if is_loaded() {
        return;
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let list = load();
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_none() {
        *g = Some(list);
    }
}

pub fn find_prefix(prefix: &str) -> Option<App> {
    ensure_loaded();
    let g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    g.as_ref()?.iter().find(|a| a.path.to_ascii_lowercase().starts_with(prefix)).cloned()
}

/// Non-blocking cache lookup for a Chromium PWA `_crx_<id>` token inside `shell:AppsFolder`.
pub fn find_crx_cached(crx_token: &str) -> Option<App> {
    let needle = crx_token.to_ascii_lowercase();
    let g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    g.as_ref()?.iter().find(|a| a.path.to_ascii_lowercase().contains(&needle)).cloned()
}

/// Loads `shell:AppsFolder` if needed and finds a Chromium PWA matching `_crx_<id>`.
pub fn find_crx(crx_token: &str) -> Option<App> {
    ensure_loaded();
    find_crx_cached(crx_token)
}

/// Finds an installed Windows app by display name (exact match first, then substring/token match).
pub fn find_by_name(query: &str) -> Option<App> {
    let q = query.trim().to_ascii_lowercase();
    if q.len() < 2 || q == "windows" {
        return None;
    }
    ensure_loaded();
    let g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let list = g.as_ref()?;
    // 1. Exact case-insensitive match (e.g. "WhatsApp" == "WhatsApp").
    if let Some(a) = list.iter().find(|a| a.name.trim().eq_ignore_ascii_case(&q)) {
        return Some(a.clone());
    }
    // 2. Display name starts with or ends with query (e.g. "Chrome" -> "Google Chrome").
    if let Some(a) = list.iter().find(|a| {
        let n = a.name.trim().to_ascii_lowercase();
        n.starts_with(&q) || n.ends_with(&q)
    }) {
        return Some(a.clone());
    }
    // 3. Substring match on name or AUMID (e.g. "WhatsApp" in "5319275A.WhatsAppDesktop_...").
    list.iter()
        .find(|a| {
            let n = a.name.to_ascii_lowercase();
            let p = a.path.to_ascii_lowercase();
            n.contains(&q) || (q.len() >= 4 && p.contains(&q))
        })
        .cloned()
}

pub fn name_for_prefix(prefix: &str) -> Option<String> {
    let g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    g.as_ref()?.iter().find(|a| a.path.to_ascii_lowercase().starts_with(prefix)).map(|a| a.name.clone())
}

pub fn is_loaded() -> bool {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}
