use windows::core::{HSTRING, PWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_QWORD,
};

const BASE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InUse {
    pub camera: bool,
    pub app: String,
}

fn subkeys(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), Some(0), KEY_READ, &mut key).is_err() {
            return out;
        }
        for i in 0..512u32 {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            if RegEnumKeyExW(key, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None).is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
        let _ = RegCloseKey(key);
    }
    out
}

fn qword(path: &str, name: &str) -> Option<u64> {
    let mut v: u64 = 0;
    let mut size = std::mem::size_of::<u64>() as u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(path), &HSTRING::from(name), RRF_RT_REG_QWORD, None, Some(&mut v as *mut u64 as *mut _), Some(&mut size)).ok().ok()?;
    }
    Some(v)
}

pub fn display_name(key: &str, packaged: bool) -> String {
    if packaged {
        let family = key.split('_').next().unwrap_or(key);
        return family.rsplit('.').next().unwrap_or(family).to_string();
    }
    let file = key.rsplit(['#', '\\', '/']).next().unwrap_or(key);
    file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file).to_string()
}

fn in_use(path: &str) -> bool {
    qword(path, "LastUsedTimeStop") == Some(0) && qword(path, "LastUsedTimeStart").unwrap_or(0) != 0
}

pub fn active() -> Vec<InUse> {
    if let Ok(t) = std::env::var("NACRE_TEST_INUSE") {
        return t.split(';').filter_map(|e| e.split_once(':')).map(|(k, n)| InUse { camera: k == "c", app: n.to_string() }).collect();
    }
    let mut out = Vec::new();
    for (cap, camera) in [("microphone", false), ("webcam", true)] {
        let root = format!("{BASE}\\{cap}");
        for k in subkeys(&root) {
            if k == "NonPackaged" {
                let np = format!("{root}\\NonPackaged");
                for e in subkeys(&np) {
                    if in_use(&format!("{np}\\{e}")) {
                        out.push(InUse { camera, app: display_name(&e, false) });
                    }
                }
            } else if in_use(&format!("{root}\\{k}")) {
                out.push(InUse { camera, app: display_name(&k, true) });
            }
        }
    }
    out
}

pub struct Monitor {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub snapshots: std::sync::mpsc::Receiver<(bool, bool)>,
}

impl Drop for Monitor {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

pub fn spawn(notify: impl Fn() + Send + 'static) -> Monitor {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc::channel, Arc};
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let _ = std::thread::Builder::new().name("abra-privacy".into()).spawn(move || {
        let mut last = (false, false);
        for _ in 0..3 {
            if s.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        while !s.load(Ordering::Relaxed) {
            let list = active();
            let v = (list.iter().any(|u| !u.camera), list.iter().any(|u| u.camera));
            if v != last {
                last = v;
                if tx.send(v).is_err() {
                    return;
                }
                notify();
            }
            for _ in 0..6 {
                if s.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    });
    Monitor { stop, snapshots: rx }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_registry_keys() {
        assert_eq!(display_name("Microsoft.WindowsSoundRecorder_8wekyb3d8bbwe", true), "WindowsSoundRecorder");
        assert_eq!(display_name("C:#Program Files#Zoom#bin#Zoom.exe", false), "Zoom");
    }
}
