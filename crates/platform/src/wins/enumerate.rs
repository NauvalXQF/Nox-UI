use nacre_core::dock::items::Win;
use std::ffi::c_void;
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowLongW, GetWindowTextW, GetWindowThreadProcessId,
    IsWindowVisible, GWL_EXSTYLE, GW_OWNER, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

const HIDDEN_EXES: &[&str] = &[
    "conhost.exe",
    "shellexperiencehost.exe",
    "searchhost.exe",
    "textinputhost.exe",
    "startmenuexperiencehost.exe",
    "systemsettingsbroker.exe",
];

/// Extracts the Chromium PWA `_crx_<id>` token (e.g. `_crx_hnpfjngllnobngcgfapefoaidbinmjnm`)
/// from a window AUMID or `shell:AppsFolder\...` path.
pub fn extract_crx_id(s: &str) -> Option<&str> {
    let lower = s.to_ascii_lowercase();
    let start = lower.find("_crx_")?;
    let rest = &s[start + 5..];
    let len = rest.bytes().take_while(|b| b.is_ascii_alphanumeric()).count();
    if len >= 8 {
        Some(&s[start..start + 5 + len])
    } else {
        None
    }
}

/// Returns `true` if a Chromium browser window's `PKEY_AppUserModel_ID` identifies an installed
/// PWA / web app window rather than a regular browser window.
/// - Edge MSIX PWAs end with `!App` (e.g. `www.youtube.com-54E21B02_... !App`)
/// - Chrome / Brave / Vivaldi / Opera / Edge desktop PWAs contain `_crx_<id>`
///   (e.g. `Chrome._crx_hnpfjngllnobngcgfapefoaidbinmjnm` or with `.UserData.Profile1`).
pub fn is_browser_pwa_aumid(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    lower.ends_with("!app") || extract_crx_id(id).is_some()
}

fn clean_pwa_title(title: &str) -> String {
    let mut t = title.trim();
    for suffix in [" - Google Chrome", " - Microsoft Edge", " - Brave", " - Vivaldi", " - Opera", " - Chromium"] {
        if let Some(base) = t.strip_suffix(suffix) {
            if !base.trim().is_empty() {
                t = base.trim();
                break;
            }
        }
    }
    t.to_string()
}

fn text_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

pub fn exe_path_of_pid(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

pub fn exe_path_of_running(name: &str) -> Option<String> {
    struct Find {
        name: String,
        found: Option<String>,
        seen: Vec<u32>,
    }
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        let f = &mut *(lp.0 as *mut Find);
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || f.seen.contains(&pid) {
            return BOOL(1);
        }
        f.seen.push(pid);
        if let Some(path) = exe_path_of_pid(pid) {
            let file = path.rsplit('\\').next().unwrap_or("").to_ascii_lowercase();
            if file == f.name {
                f.found = Some(path);
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut f = Find { name: name.to_ascii_lowercase(), found: None, seen: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut f as *mut Find as isize));
    }
    f.found
}

pub fn cloaked(hwnd: isize) -> bool {
    is_cloaked(HWND(hwnd as *mut _))
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4).is_ok() && cloaked != 0
    }
}

struct Ctx {
    own_pid: u32,
    out: Vec<Win>,
}

unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam.0 as *mut Ctx);
    if !IsWindowVisible(hwnd).as_bool() || is_cloaked(hwnd) {
        return BOOL(1);
    }
    let title = text_of(hwnd);
    if title.is_empty() || title == "Program Manager" {
        return BOOL(1);
    }
    let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    let app_window = ex & WS_EX_APPWINDOW.0 != 0;
    if ex & WS_EX_TOOLWINDOW.0 != 0 && !app_window {
        return BOOL(1);
    }
    if !app_window {
        if let Ok(owner) = GetWindow(hwnd, GW_OWNER) {
            if !owner.0.is_null() && IsWindowVisible(owner).as_bool() {
                return BOOL(1);
            }
        }
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 || pid == ctx.own_pid {
        return BOOL(1);
    }
    let Some(path) = exe_path_of_pid(pid) else { return BOOL(1) };
    let lower = path.to_lowercase();
    let file = lower.rsplit('\\').next().unwrap_or("");
    if HIDDEN_EXES.contains(&file) {
        return BOOL(1);
    }
    let class = class_of(hwnd);
    if file == "explorer.exe" && class != "CabinetWClass" && class != "ExploreWClass" {
        return BOOL(1);
    }
    let mut path = path;
    let mut pwa = false;
    if file == "applicationframehost.exe" {
        if class != "ApplicationFrameWindow" {
            return BOOL(1);
        }
        path = match super::identity::aumid(hwnd.0 as isize) {
            Some(id) => super::identity::shell_path(&id),
            None => format!("{path}?{title}"),
        };
    } else if matches!(file, "msedge.exe" | "chrome.exe" | "chrome_proxy.exe" | "brave.exe" | "opera.exe" | "vivaldi.exe" | "chromium.exe") {
        if let Some(id) = super::identity::aumid(hwnd.0 as isize) {
            if is_browser_pwa_aumid(&id) {
                let candidate = super::identity::shell_path(&id);
                if crate::apps::name_for_path(&candidate).is_some() {
                    path = candidate;
                } else if let Some(crx) = extract_crx_id(&id) {
                    path = crate::apps::find_crx_cached(crx).map(|a| a.path).unwrap_or(candidate);
                } else {
                    path = candidate;
                }
                pwa = true;
            }
        }
    }
    let mut name = if pwa {
        crate::apps::name_for_path(&path).unwrap_or_else(|| clean_pwa_title(&title))
    } else {
        super::names::friendly(&path)
    };
    if !pwa && path.to_ascii_lowercase().contains("\\windowsapps\\") {
        if let Some(family) = nacre_core::dock::items::package_family(&path) {
            let prefix = format!("shell:appsfolder\\{}!", family.to_ascii_lowercase());
            if let Some(n) = crate::apps::name_for_prefix(&prefix) {
                name = n;
            }
        }
    }
    ctx.out.push(Win { hwnd: hwnd.0 as isize, path, title, name });
    BOOL(1)
}

pub fn list(own_pid: u32) -> Vec<Win> {
    let mut ctx = Ctx { own_pid, out: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    ctx.out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_chrome_edge_and_brave_pwa_aumids_without_false_positives() {
        assert!(is_browser_pwa_aumid("Chrome._crx_hnpfjngllnobngcgfapefoaidbinmjnm"));
        assert!(is_browser_pwa_aumid("Chrome._crx_agimnkijcaahngcdmfeangaknmldooml.UserData.Profile1"));
        assert!(is_browser_pwa_aumid("Brave._crx_abcdefgh12345678"));
        assert!(is_browser_pwa_aumid("www.youtube.com-54E21B02_pd8mbgmqs65xy!App"));
        assert!(!is_browser_pwa_aumid("Chrome"));
        assert!(!is_browser_pwa_aumid("Chrome.Y24ABCDEF12345"));
        assert!(!is_browser_pwa_aumid("MSEdge"));

        assert_eq!(
            extract_crx_id("shell:AppsFolder\\Chrome._crx_hnpfjngllnobngcgfapefoaidbinmjnm.UserData.Profile1"),
            Some("_crx_hnpfjngllnobngcgfapefoaidbinmjnm")
        );
        assert_eq!(clean_pwa_title("WhatsApp - Google Chrome"), "WhatsApp");
        assert_eq!(clean_pwa_title("YouTube Music"), "YouTube Music");
    }
}
