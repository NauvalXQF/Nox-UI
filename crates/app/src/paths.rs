use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("NACRE_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join("NOX");
    migrate_legacy(&base.join("ABRA"), &dir);
    migrate_legacy(&base.join("Nacre"), &dir);
    dir
}

fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) {
    let _ = std::fs::create_dir_all(dst);
    let Ok(rd) = std::fs::read_dir(src) else { return };
    for entry in rd.flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to);
        } else if !to.exists() {
            let _ = std::fs::copy(&from, &to);
        }
    }
}

fn migrate_legacy(old: &std::path::Path, new: &std::path::Path) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    if new.exists() || !old.exists() {
        return;
    }
    ONCE.call_once(|| {
        let _ = std::fs::create_dir_all(new);
        for f in [
            "pins.txt",
            "settings.cfg",
            "recents.txt",
            "icons.txt",
            "cc_layout.txt",
            "todos.json",
            "widgets.cfg",
            "quick_keys.txt",
            "menulogo.png",
        ] {
            let p = old.join(f);
            if p.exists() {
                let _ = std::fs::copy(&p, new.join(f));
            }
        }
        for sub in ["lyrics", "iconpack", "icons"] {
            let p = old.join(sub);
            if p.is_dir() {
                copy_dir_recursive(&p, &new.join(sub));
            }
        }
    });
}

pub fn marker() -> PathBuf {
    config_dir().join("taskbar_hidden.flag")
}

pub fn log() -> PathBuf {
    config_dir().join("nox.log")
}

pub fn guard_exe() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("nox-guard.exe")))
        .unwrap_or_else(|| PathBuf::from("nox-guard.exe"))
}
