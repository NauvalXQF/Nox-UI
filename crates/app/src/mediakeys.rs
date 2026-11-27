use crate::services;
use nacre_platform::hookthread::{HookThread, Kind};
use nacre_platform::{audio, log};
use std::sync::Mutex;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

const VK_VOLUME_MUTE: u16 = 0xAD;
const VK_VOLUME_DOWN: u16 = 0xAE;
const VK_VOLUME_UP: u16 = 0xAF;

const VOLUME_STEP: f32 = 0.02;

static HOOK: Mutex<Option<HookThread>> = Mutex::new(None);

pub fn install() {
    let mut g = HOOK.lock().unwrap_or_else(|e| e.into_inner());
    if g.is_some() {
        return;
    }
    match HookThread::start(Kind::Keyboard, Some(proc)) {
        Some(h) => {
            *g = Some(h);
            log::line("mediakeys", "hook volume aktif (thread khusus)");
        }
        None => log::line("mediakeys", "gagal pasang hook volume"),
    }
}

pub fn reinstall() {
    if HOOK.lock().unwrap_or_else(|e| e.into_inner()).is_none() {
        return;
    }
    shutdown();
    install();
}

pub fn shutdown() {
    let h = HOOK.lock().unwrap_or_else(|e| e.into_inner()).take();
    drop(h);
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
