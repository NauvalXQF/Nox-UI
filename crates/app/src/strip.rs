use std::sync::Once;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, BLACK_BRUSH, HBRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_core::Result;

const WM_MOUSELEAVE: u32 = 0x02A3;
static REGISTER_DOCK: Once = Once::new();
static REGISTER_NOTCH: Once = Once::new();

struct State {
    target: HWND,
    enter: u32,
    leave: u32,
    inside: bool,
}

unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let st = GetWindowLongPtrW(h, GWLP_USERDATA) as *mut State;
    match m {
        WM_MOUSEMOVE if !st.is_null() => {
            let s = &mut *st;
            let mut pt = POINT::default();
            if GetCursorPos(&mut pt).is_err() {
                return LRESULT(0);
            }
            let mut r = RECT::default();
            if GetWindowRect(h, &mut r).is_err()
                || pt.x < r.left
                || pt.x >= r.right
                || pt.y < r.top
                || pt.y >= r.bottom
            {
                return LRESULT(0);
            }
            if !s.inside {
                s.inside = true;
                let mut t = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: h,
                    dwHoverTime: 0,
                };
                let _ = TrackMouseEvent(&mut t);
                let _ = PostMessageW(Some(s.target), s.enter, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE if !st.is_null() => {
            let s = &mut *st;
            s.inside = false;
            let _ = PostMessageW(Some(s.target), s.leave, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCDESTROY => {
            if !st.is_null() {
                drop(Box::from_raw(st));
                SetWindowLongPtrW(h, GWLP_USERDATA, 0);
            }
            DefWindowProcW(h, m, w, l)
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}

pub struct Strip {
    hwnd: HWND,
}

impl Drop for Strip {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

impl Strip {
    pub fn new(target: HWND, enter: u32, leave: u32) -> Result<Strip> {
        Self::create(w!("NacreEdgeStrip"), &REGISTER_DOCK, target, enter, leave)
    }

    pub fn new_notch(target: HWND, enter: u32, leave: u32) -> Result<Strip> {
        Self::create(w!("NacreNotchStrip"), &REGISTER_NOTCH, target, enter, leave)
    }

    fn create(class: PCWSTR, once: &Once, target: HWND, enter: u32, leave: u32) -> Result<Strip> {
        unsafe {
            let hi: HINSTANCE = GetModuleHandleW(None)?.into();
            once.call_once(|| {
                RegisterClassW(&WNDCLASSW {
                    lpfnWndProc: Some(proc),
                    hInstance: hi,
                    lpszClassName: class,
                    hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0),
                    ..Default::default()
                });
            });
            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                class,
                w!("nacre-strip"),
                WS_POPUP,
                0, 0, 10, 2,
                None, None, Some(hi), None,
            )?;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(Box::new(State { target, enter, leave, inside: false })) as isize);
            SetLayeredWindowAttributes(hwnd, COLORREF(0), 1, LWA_ALPHA)?;
            Ok(Strip { hwnd })
        }
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn place(&self, x: i32, y: i32, w: i32, h: i32) {
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
        }
    }

    pub fn set_visible(&self, on: bool) {
        unsafe {
            if !on {
                let st = GetWindowLongPtrW(self.hwnd, GWLP_USERDATA) as *mut State;
                if !st.is_null() {
                    (*st).inside = false;
                }
            }
            let _ = ShowWindow(self.hwnd, if on { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }

    pub fn to_top(&self) {
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}
