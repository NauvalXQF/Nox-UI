use crate::services;
use nacre_platform::{audio, log};
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

const VK_VOLUME_MUTE: u16 = 0xAD;
const VK_VOLUME_DOWN: u16 = 0xAE;
const VK_VOLUME_UP: u16 = 0xAF;

const VOLUME_STEP: f32 = 0.02;

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
                log::line("mediakeys", "hook volume aktif");
            }
            Err(e) => log::line("mediakeys", &format!("gagal pasang hook: {e}")),
        }
    }
}

pub fn reinstall() {
    if HOOK.load(Ordering::Relaxed) == 0 {
        return;
    }
    shutdown();
    install();
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
