use nacre_platform::{audio, brightness};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::OnceLock;
use std::sync::mpsc::Sender;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_APP};

pub const WM_VOLUME: u32 = WM_APP + 20;
pub const WM_BRIGHT: u32 = WM_APP + 21;
pub const WM_RADIO: u32 = WM_APP + 22;
pub const WM_GFX_RESET: u32 = WM_APP + 160;

static HUD: AtomicIsize = AtomicIsize::new(0);
static ISLAND: AtomicIsize = AtomicIsize::new(0);
static GFX_LISTENERS: std::sync::Mutex<Vec<isize>> = std::sync::Mutex::new(Vec::new());
static AUDIO: OnceLock<Sender<audio::Cmd>> = OnceLock::new();
static BRIGHT: OnceLock<Sender<brightness::Cmd>> = OnceLock::new();

pub fn listen_gfx(h: HWND) {
    let mut g = GFX_LISTENERS.lock().unwrap_or_else(|e| e.into_inner());
    let v = h.0 as isize;
    if !g.contains(&v) {
        g.push(v);
    }
}

pub fn unlisten_gfx(h: HWND) {
    let v = h.0 as isize;
    GFX_LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).retain(|x| *x != v);
}

pub fn broadcast_gfx_reset() {
    let list: Vec<isize> = GFX_LISTENERS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    for h in list {
        if h != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(h as *mut _)), WM_GFX_RESET, WPARAM(0), LPARAM(0));
            }
        }
    }
}

pub fn register_hud(h: HWND) {
    HUD.store(h.0 as isize, Ordering::Relaxed);
}

pub fn register_island(h: HWND) {
    ISLAND.store(h.0 as isize, Ordering::Relaxed);
}

fn post_all(msg: u32) {
    for h in [HUD.load(Ordering::Relaxed), ISLAND.load(Ordering::Relaxed)] {
        if h != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(h as *mut _)), msg, WPARAM(0), LPARAM(0));
            }
        }
    }
}

pub fn init() {
    nacre_ui::gfx::set_on_reset(broadcast_gfx_reset);
    if AUDIO.get().is_some() {
        return;
    }
    let a = audio::spawn(|| post_all(WM_VOLUME));
    let _ = AUDIO.set(a.commands);
    let b = brightness::spawn(|| post_all(WM_BRIGHT));
    let _ = BRIGHT.set(b.commands);
}

pub fn set_volume(v: f32) {
    if let Some(tx) = AUDIO.get() {
        let _ = tx.send(audio::Cmd::SetLevel(v));
    }
}

pub fn toggle_mute() {
    if let Some(tx) = AUDIO.get() {
        let _ = tx.send(audio::Cmd::ToggleMute);
    }
}

pub fn set_brightness(frac: f32) {
    if let Some(tx) = BRIGHT.get() {
        let _ = tx.send(brightness::Cmd::Set((frac.clamp(0.0, 1.0) * 100.0).round() as u8));
    }
}

use std::sync::atomic::AtomicI32;
static NOTCH_L: AtomicI32 = AtomicI32::new(0);
static NOTCH_R: AtomicI32 = AtomicI32::new(0);
static NOTCH_H: AtomicI32 = AtomicI32::new(0);
static BAR_LEFT_W: AtomicI32 = AtomicI32::new(0);
static BAR_RIGHT_W: AtomicI32 = AtomicI32::new(0);

pub fn set_bar_occupied(left_px: i32, right_px: i32) {
    BAR_LEFT_W.store(left_px.max(0), Ordering::Relaxed);
    BAR_RIGHT_W.store(right_px.max(0), Ordering::Relaxed);
}

pub fn bar_occupied() -> (i32, i32) {
    (BAR_LEFT_W.load(Ordering::Relaxed), BAR_RIGHT_W.load(Ordering::Relaxed))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotchLayout {
    pub gap: Option<(i32, i32)>,
    pub rail_h: Option<i32>,
}

pub fn set_notch_layout(gap: Option<(i32, i32)>, rail_h: Option<i32>) {
    let (l, r) = gap.unwrap_or((0, 0));
    let h = rail_h.unwrap_or(0).max(0);
    let prev_l = NOTCH_L.swap(l, Ordering::Relaxed);
    let prev_r = NOTCH_R.swap(r, Ordering::Relaxed);
    let prev_h = NOTCH_H.swap(h, Ordering::Relaxed);
    if (prev_l, prev_r, prev_h) == (l, r, h) {
        return;
    }
    let bar = BAR.load(Ordering::Relaxed);
    if bar != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(bar as *mut _)), WM_APP + 71, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn notch_layout() -> NotchLayout {
    let l = NOTCH_L.load(Ordering::Relaxed);
    let r = NOTCH_R.load(Ordering::Relaxed);
    let h = NOTCH_H.load(Ordering::Relaxed);
    NotchLayout {
        gap: (r > l).then_some((l, r)),
        rail_h: (h > 0).then_some(h),
    }
}

static BAR: AtomicIsize = AtomicIsize::new(0);
pub fn register_bar(h: HWND) {
    BAR.store(h.0 as isize, Ordering::Relaxed);
}

pub fn post_island(msg: u32) {
    let h = ISLAND.load(Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut _)), msg, WPARAM(0), LPARAM(0));
        }
    }
}
