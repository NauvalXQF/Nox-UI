use std::sync::mpsc::{channel, Receiver, Sender};
use windows::core::HSTRING;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY, SIIGBF_RESIZETOFIT};

#[derive(Clone)]
pub struct Bitmap {
    pub w: u32,
    pub h: u32,
    pub bgra: Vec<u8>,
}

pub fn fit(b: Bitmap, px: u32) -> Bitmap {
    if b.w <= px && b.h <= px {
        return b;
    }
    let k = (px as f32 / b.w.max(b.h) as f32).min(1.0);
    let (ow, oh) = (((b.w as f32 * k).round() as u32).max(1), ((b.h as f32 * k).round() as u32).max(1));
    let (sx, sy) = (b.w as f32 / ow as f32, b.h as f32 / oh as f32);
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    for y in 0..oh {
        let (y0, y1) = (y as f32 * sy, (y + 1) as f32 * sy);
        for x in 0..ow {
            let (x0, x1) = (x as f32 * sx, (x + 1) as f32 * sx);
            let mut acc = [0f32; 4];
            let mut wsum = 0f32;
            let mut yy = y0.floor() as u32;
            while (yy as f32) < y1 && yy < b.h {
                let wy = ((yy + 1) as f32).min(y1) - (yy as f32).max(y0);
                let mut xx = x0.floor() as u32;
                while (xx as f32) < x1 && xx < b.w {
                    let wx = ((xx + 1) as f32).min(x1) - (xx as f32).max(x0);
                    let w = wx * wy;
                    let i = ((yy * b.w + xx) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += b.bgra[i + c] as f32 * w;
                    }
                    wsum += w;
                    xx += 1;
                }
                yy += 1;
            }
            let o = ((y * ow + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (acc[c] / wsum.max(1e-6)).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Bitmap { w: ow, h: oh, bgra: out }
}

static OVERRIDES: std::sync::Mutex<Option<std::collections::HashMap<String, String>>> = std::sync::Mutex::new(None);

fn key(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

pub fn set_override(path: &str, image: Option<String>) {
    let mut g = OVERRIDES.lock().unwrap();
    let m = g.get_or_insert_with(Default::default);
    match image {
        Some(i) => {
            m.insert(key(path), i);
        }
        None => {
            m.remove(&key(path));
        }
    }
}

pub fn override_for(path: &str) -> Option<String> {
    OVERRIDES.lock().unwrap().as_ref().and_then(|m| m.get(&key(path)).cloned())
}

pub fn is_themed(path: &str) -> bool {
    override_for(path).is_some() || crate::iconpack::lookup(path).is_some()
}

pub fn load(path: &str, px: i32) -> Option<Bitmap> {
    if let Some(img) = override_for(path) {
        if let Some(b) = crate::wic::decode_file_fit(&img, px as u32) {
            return Some(b);
        }
    }
    if let Some(img) = crate::iconpack::lookup(path) {
        if let Some(b) = crate::wic::decode_file_fit(&img, px as u32) {
            return Some(b);
        }
    }
    if path.to_ascii_lowercase().contains("\\windowsapps\\") {
        if let Some(family) = nacre_core::dock::items::package_family(path) {
            let prefix = format!("shell:appsfolder\\{}!", family.to_ascii_lowercase());
            if let Some(app) = crate::apps::find_prefix(&prefix) {
                if let Some(b) = load_shell(&app.path, px) {
                    return Some(b);
                }
            }
        }
    }
    if let Some(b) = load_shell(path, px) {
        return Some(b);
    }
    // Fallback for Chromium PWAs (`shell:AppsFolder\Chrome._crx_<id>...`) whose window AUMID has a
    // profile suffix or lacks a Start Menu shortcut: resolve via AppsFolder `_crx_` match, then live HICON.
    if let Some(crx) = crate::wins::enumerate::extract_crx_id(path) {
        if let Some(app) = crate::apps::find_crx(crx) {
            if !app.path.eq_ignore_ascii_case(path) {
                if let Some(b) = load_shell(&app.path, px) {
                    return Some(b);
                }
            }
        }
    }
    if let Some(aumid) = strip_shell_appsfolder(path) {
        if let Some(b) = load_window_icon_for_aumid(aumid, px) {
            return Some(b);
        }
    }
    None
}

fn strip_shell_appsfolder(path: &str) -> Option<&str> {
    let lower = path.to_ascii_lowercase();
    let prefix = "shell:appsfolder\\";
    if lower.starts_with(prefix) {
        Some(path[prefix.len()..].split('?').next().unwrap_or(""))
    } else {
        None
    }
}

fn load_window_icon_for_aumid(target_aumid: &str, px: i32) -> Option<Bitmap> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassLongPtrW, GetIconInfo, IsWindowVisible, SendMessageTimeoutW, GCLP_HICON, HICON,
        ICONINFO, ICON_BIG, ICON_SMALL2, SMTO_ABORTIFHUNG, WM_GETICON,
    };

    struct Find {
        target: String,
        crx: Option<String>,
        found: Option<HWND>,
    }
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        let f = &mut *(lp.0 as *mut Find);
        if !IsWindowVisible(hwnd).as_bool() {
            return BOOL(1);
        }
        if let Some(id) = crate::wins::identity::aumid(hwnd.0 as isize) {
            let matched = id.eq_ignore_ascii_case(&f.target)
                || f.crx
                    .as_deref()
                    .is_some_and(|c| crate::wins::enumerate::extract_crx_id(&id).is_some_and(|x| x.eq_ignore_ascii_case(c)));
            if matched {
                f.found = Some(hwnd);
                return BOOL(0);
            }
        }
        BOOL(1)
    }
    let mut f = Find {
        target: target_aumid.to_string(),
        crx: crate::wins::enumerate::extract_crx_id(target_aumid).map(str::to_string),
        found: None,
    };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut f as *mut Find as isize));
    }
    let hwnd = f.found?;

    unsafe {
        let mut hicon_raw: usize = 0;
        let _ = SendMessageTimeoutW(hwnd, WM_GETICON, WPARAM(ICON_BIG as usize), LPARAM(0), SMTO_ABORTIFHUNG, 150, Some(&mut hicon_raw));
        if hicon_raw == 0 {
            hicon_raw = GetClassLongPtrW(hwnd, GCLP_HICON);
        }
        if hicon_raw == 0 {
            let _ = SendMessageTimeoutW(hwnd, WM_GETICON, WPARAM(ICON_SMALL2 as usize), LPARAM(0), SMTO_ABORTIFHUNG, 150, Some(&mut hicon_raw));
        }
        if hicon_raw == 0 {
            return None;
        }
        let hicon = HICON(hicon_raw as *mut _);
        let mut ii = ICONINFO::default();
        if GetIconInfo(hicon, &mut ii).is_err() {
            return None;
        }
        let hbmp = ii.hbmColor;
        if hbmp.is_invalid() {
            if !ii.hbmMask.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
            }
            return None;
        }
        let mut bm = BITMAP::default();
        let got = GetObjectW(HGDIOBJ(hbmp.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
        let res = if got != 0 && bm.bmWidth > 0 && bm.bmHeight > 0 {
            let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);
            let mut info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut buf = vec![0u8; (w * h * 4) as usize];
            let dc = GetDC(None);
            let lines = GetDIBits(dc, hbmp, 0, h, Some(buf.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
            ReleaseDC(None, dc);
            if lines > 0 {
                if buf.chunks_exact(4).all(|p| p[3] == 0) {
                    for p in buf.chunks_exact_mut(4) {
                        p[3] = 255;
                    }
                }
                Some(fit(Bitmap { w, h, bgra: buf }, px.max(1) as u32))
            } else {
                None
            }
        } else {
            None
        };
        let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        if !ii.hbmMask.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        }
        res
    }
}

pub fn thumbnail(path: &str, px: i32) -> Option<Bitmap> {
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?;
        let hbmp = factory.GetImage(SIZE { cx: px, cy: px }, SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK).ok()?;
        let mut bm = BITMAP::default();
        let got = GetObjectW(HGDIOBJ(hbmp.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
        if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
            let _ = DeleteObject(HGDIOBJ(hbmp.0));
            return None;
        }
        let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let lines = GetDIBits(dc, hbmp, 0, h, Some(buf.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        if lines == 0 {
            return None;
        }
        if buf.chunks_exact(4).all(|p| p[3] == 0) {
            for p in buf.chunks_exact_mut(4) {
                p[3] = 255;
            }
        }
        Some(Bitmap { w, h, bgra: buf })
    }
}

fn load_shell(path: &str, px: i32) -> Option<Bitmap> {
    let path = path.split('?').next().unwrap_or(path);
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?;
        let req = (px * 2).clamp(128, 256);
        let hbmp = factory.GetImage(SIZE { cx: req, cy: req }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok()?;

        let mut bm = BITMAP::default();
        let got = GetObjectW(HGDIOBJ(hbmp.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut _));
        if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
            let _ = DeleteObject(HGDIOBJ(hbmp.0));
            return None;
        }
        let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let dc = GetDC(None);
        let lines = GetDIBits(dc, hbmp, 0, h, Some(buf.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(HGDIOBJ(hbmp.0));
        if lines == 0 {
            return None;
        }
        Some(fit(Bitmap { w, h, bgra: buf }, px.max(1) as u32))
    }
}

pub struct Worker {
    pub requests: Sender<String>,
    pub results: Receiver<(String, Option<Bitmap>)>,
}

pub fn spawn_worker(px: i32, notify: impl Fn() + Send + 'static) -> Worker {
    spawn_worker_with(px, false, notify)
}

pub fn spawn_thumb_worker(px: i32, notify: impl Fn() + Send + 'static) -> Worker {
    spawn_worker_with(px, true, notify)
}

fn spawn_worker_with(px: i32, thumbs: bool, notify: impl Fn() + Send + 'static) -> Worker {
    let (req_tx, req_rx) = channel::<String>();
    let (res_tx, res_rx) = channel();
    std::thread::spawn(move || {
        let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
        while let Ok(path) = req_rx.recv() {
            let bmp = if thumbs { thumbnail(&path, px).or_else(|| load(&path, px)) } else { load(&path, px) };
            if res_tx.send((path, bmp)).is_err() {
                break;
            }
            notify();
        }
        if com {
            unsafe { CoUninitialize() };
        }
    });
    Worker { requests: req_tx, results: res_rx }
}
