use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY,
};

pub const CTRL: u16 = 0x11;
pub const SHIFT: u16 = 0x10;
pub const ALT: u16 = 0x12;
pub const WIN: u16 = 0x5B;

fn is_extended(vk: u16) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x5B | 0x5C | 0x5D | 0x6F | 0xA3 | 0xA5)
}

fn key(vk: u16, up: bool) -> INPUT {
    let mut flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
    if is_extended(vk) {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16 };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

pub fn chord(vks: &[u16]) {
    // Avoid sending KEYUP for modifiers the user is already physically holding so modifier state never desyncs.
    let mut pressed: Vec<u16> = Vec::with_capacity(vks.len());
    let mut seq: Vec<INPUT> = Vec::with_capacity(vks.len() * 2);
    for &vk in vks {
        let is_mod = matches!(vk, CTRL | SHIFT | ALT | WIN);
        let held = is_mod && unsafe { GetAsyncKeyState(vk as i32) < 0 };
        if !held {
            seq.push(key(vk, false));
            pressed.push(vk);
        }
    }
    seq.extend(pressed.iter().rev().map(|v| key(*v, true)));
    if !seq.is_empty() {
        unsafe {
            SendInput(&seq, std::mem::size_of::<INPUT>() as i32);
        }
    }
}
