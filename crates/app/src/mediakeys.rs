use crate::services;
use nacre_platform::{audio, brightness, log};
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

const VK_VOLUME_MUTE: u16 = 0xAD;
const VK_VOLUME_DOWN: u16 = 0xAE;
const VK_VOLUME_UP: u16 = 0xAF;
const VK_BRIGHTNESS_DOWN: u16 = 0x70;
const VK_BRIGHTNESS_UP: u16 = 0x6F;

const VOLUME_STEP: f32 = 0.02;
const BRIGHTNESS_STEP: f32 = 0.05;

static HOOK: AtomicIsize = AtomicIsize::new(0);

pub fn install() {
    if HOOK.load(Ordering::Relaxed) != 0 {
        return;
    }
    unsafe {
        let hi = GetModuleHandleW(None).ok().map(|m| HINSTANCE(m.0));
        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(proc), hi, 0) {
            Ok(h) => {
                HOOK.store(h.0 as isize, Ordering::Relaxed);
                log::line("mediakeys", "hook volume/kecerahan aktif");
            }
            Err(e) => log::line("mediakeys", &format!("gagal pasang hook: {e}")),
        }
    }
}

pub fn shutdown() {
    let h = HOOK.swap(0, Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(h as *mut _));
        }
    }
}

unsafe extern "system" fn proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let msg = wp.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
        if down || up {
            let handled = match k.vkCode as u16 {
                VK_VOLUME_UP => down.then(|| step_volume(1.0)).unwrap_or(true),
                VK_VOLUME_DOWN => down.then(|| step_volume(-1.0)).unwrap_or(true),
                VK_VOLUME_MUTE => down.then(mute).unwrap_or(true),
                VK_BRIGHTNESS_UP => down.then(|| step_brightness(1.0)).unwrap_or(true),
                VK_BRIGHTNESS_DOWN => down.then(|| step_brightness(-1.0)).unwrap_or(true),
                _ => false,
            };
            if handled {
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

fn step_volume(dir: f32) -> bool {
    let v = audio::latest();
    if !v.available {
        return false;
    }
    services::set_volume((v.level + dir * VOLUME_STEP).clamp(0.0, 1.0));
    true
}

fn mute() -> bool {
    if !audio::latest().available {
        return false;
    }
    services::toggle_mute();
    true
}

fn step_brightness(dir: f32) -> bool {
    let Some(level) = brightness::latest() else {
        return false;
    };
    services::set_brightness((level as f32 / 100.0 + dir * BRIGHTNESS_STEP).clamp(0.0, 1.0));
    true
}
