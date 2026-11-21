#![allow(dead_code)]

use super::draw::{clip, white, Painter, WHITE};
use nacre_core::notch::input::TextBuffer;
use windows::UI::Color;
use windows::UI::Composition::{
    CompositionColorBrush, CompositionStretch, Compositor, ContainerVisual, SpriteVisual,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_BACK, VK_CONTROL, VK_DELETE, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT,
    VK_RETURN, VK_RIGHT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
};
use windows_numerics::{Vector2, Vector3};

/// Result of feeding a keyboard event to a retained `TextInput`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputOutcome {
    Ignored,
    Changed,
    Submit,
    Blur,
}

/// Dynamic keyboard focus controller for `AbraNotch`.
/// Normally keeps `WS_EX_NOACTIVATE` set so clicks on media/shelf never steal focus;
/// removes it only while a text field (To-Do / Ask Gemini) is focused.
pub struct FocusController {
    active: bool,
    prev_fg: isize,
}

impl FocusController {
    pub fn new() -> Self {
        Self { active: false, prev_fg: 0 }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn acquire(&mut self, hwnd: HWND) {
        if self.active {
            return;
        }
        self.prev_fg = nacre_platform::wins::focus::foreground();
        unsafe {
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongW(hwnd, GWL_EXSTYLE, (ex & !WS_EX_NOACTIVATE.0) as i32);
        }
        self.active = true;
        let _ = nacre_platform::wins::focus::force_foreground(hwnd.0 as isize);
        unsafe {
            let _ = SetFocus(Some(hwnd));
        }
    }

    pub fn release(&mut self, hwnd: HWND, restore_prev: bool) {
        if !self.active {
            return;
        }
        self.active = false;
        unsafe {
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongW(hwnd, GWL_EXSTYLE, (ex | WS_EX_NOACTIVATE.0) as i32);
        }
        if restore_prev && self.prev_fg != 0 && self.prev_fg != hwnd.0 as isize {
            if nacre_platform::wins::focus::is_alive(self.prev_fg) {
                let _ = nacre_platform::wins::focus::force_foreground(self.prev_fg);
            }
        }
        self.prev_fg = 0;
    }
}

/// Retained `Windows.UI.Composition` single-line text input widget.
/// Updates text surface, scroll offset, and caret opacity in-place without rebuilding parents.
pub struct TextInput {
    pub buf: TextBuffer,
    pub placeholder: String,
    pub focused: bool,
    caret_on: bool,
    root: ContainerVisual,
    bg: SpriteVisual,
    bg_brush: CompositionColorBrush,
    viewport: ContainerVisual,
    text_vis: SpriteVisual,
    caret: SpriteVisual,
    size: (f32, f32),
    pad_x: f32,
    font_dp: f32,
}

impl TextInput {
    pub fn new(
        comp: &Compositor,
        parent: &ContainerVisual,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        dpi: f32,
        placeholder: &str,
        max_chars: usize,
    ) -> windows_core::Result<Self> {
        let root = comp.CreateContainerVisual()?;
        root.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
        root.SetSize(Vector2 { X: w, Y: h })?;
        parent.Children()?.InsertAtTop(&root)?;

        let bg = comp.CreateSpriteVisual()?;
        bg.SetSize(Vector2 { X: w, Y: h })?;
        let bg_brush = comp.CreateColorBrushWithColor(white(24))?;
        bg.SetBrush(&bg_brush)?;
        bg.SetClip(&clip(comp, w, h, h / 2.0)?)?;
        root.Children()?.InsertAtTop(&bg)?;

        let pad_x = 12.0 * dpi;
        let vw = (w - 2.0 * pad_x).max(4.0);
        let viewport = comp.CreateContainerVisual()?;
        viewport.SetOffset(Vector3 { X: pad_x, Y: 0.0, Z: 0.0 })?;
        viewport.SetSize(Vector2 { X: vw, Y: h })?;
        viewport.SetClip(&comp.CreateInsetClip()?)?;
        root.Children()?.InsertAtTop(&viewport)?;

        let text_vis = comp.CreateSpriteVisual()?;
        viewport.Children()?.InsertAtTop(&text_vis)?;

        let caret = comp.CreateSpriteVisual()?;
        let ch = (14.0 * dpi).min(h - 8.0 * dpi).max(8.0);
        caret.SetSize(Vector2 { X: (1.5 * dpi).max(1.0), Y: ch })?;
        caret.SetOffset(Vector3 { X: 0.0, Y: (h - ch) / 2.0, Z: 0.0 })?;
        caret.SetBrush(&comp.CreateColorBrushWithColor(Color { A: 255, R: 255, G: 255, B: 255 })?)?;
        caret.SetOpacity(0.0)?;
        viewport.Children()?.InsertAtTop(&caret)?;

        Ok(Self {
            buf: TextBuffer::new(max_chars),
            placeholder: placeholder.to_string(),
            focused: false,
            caret_on: false,
            root,
            bg,
            bg_brush,
            viewport,
            text_vis,
            caret,
            size: (w, h),
            pad_x,
            font_dp: 12.5,
        })
    }

    pub fn set_focused(&mut self, on: bool, paint: &mut Painter) {
        self.focused = on;
        self.caret_on = on;
        let _ = self.bg_brush.SetColor(if on { white(38) } else { white(24) });
        let _ = self.caret.SetOpacity(if on { 1.0 } else { 0.0 });
        self.sync_visual(paint);
    }

    pub fn blink_caret(&mut self) {
        if !self.focused {
            let _ = self.caret.SetOpacity(0.0);
            return;
        }
        self.caret_on = !self.caret_on;
        let _ = self.caret.SetOpacity(if self.caret_on { 1.0 } else { 0.0 });
    }

    pub fn on_char(&mut self, unit: u16, paint: &mut Painter) -> InputOutcome {
        if !self.focused {
            return InputOutcome::Ignored;
        }
        if self.buf.push_utf16(unit) {
            self.caret_on = true;
            self.sync_visual(paint);
            InputOutcome::Changed
        } else {
            InputOutcome::Ignored
        }
    }

    pub fn on_keydown(&mut self, vk: u16, paint: &mut Painter) -> InputOutcome {
        if !self.focused {
            return InputOutcome::Ignored;
        }
        let ctrl = unsafe { (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0 };
        if ctrl {
            match vk {
                x if x == b'A' as u16 => {
                    self.buf.select_all();
                    self.sync_visual(paint);
                    return InputOutcome::Changed;
                }
                x if x == b'C' as u16 => {
                    if !self.buf.is_empty() {
                        let _ = nacre_platform::clipboard::copy_text(self.buf.as_str());
                    }
                    return InputOutcome::Ignored;
                }
                x if x == b'V' as u16 => {
                    if let Some(s) = nacre_platform::clipboard::paste_text() {
                        if self.buf.paste(&s) {
                            self.caret_on = true;
                            self.sync_visual(paint);
                            return InputOutcome::Changed;
                        }
                    }
                    return InputOutcome::Ignored;
                }
                x if x == VK_BACK.0 => {
                    self.buf.clear();
                    self.caret_on = true;
                    self.sync_visual(paint);
                    return InputOutcome::Changed;
                }
                _ => {}
            }
        }
        let changed = match vk {
            x if x == VK_RETURN.0 => return InputOutcome::Submit,
            x if x == VK_ESCAPE.0 => return InputOutcome::Blur,
            x if x == VK_BACK.0 => self.buf.backspace(),
            x if x == VK_DELETE.0 => self.buf.delete(),
            x if x == VK_LEFT.0 => self.buf.move_left(),
            x if x == VK_RIGHT.0 => self.buf.move_right(),
            x if x == VK_HOME.0 => self.buf.move_home(),
            x if x == VK_END.0 => self.buf.move_end(),
            _ => return InputOutcome::Ignored,
        };
        if changed {
            self.caret_on = true;
            self.sync_visual(paint);
            InputOutcome::Changed
        } else {
            InputOutcome::Ignored
        }
    }

    pub fn sync_visual(&mut self, paint: &mut Painter) {
        let (w, h) = self.size;
        let vw = (w - 2.0 * self.pad_x).max(4.0);
        let empty = self.buf.is_empty();
        let shown = if empty { self.placeholder.as_str() } else { self.buf.as_str() };
        let op = if empty { 0.42 } else { 0.96 };
        let caret_x = if empty { 0.0 } else { paint.measure(self.buf.before_cursor(), self.font_dp, 500) };
        let scroll = (caret_x - (vw - 6.0 * paint.dpi)).max(0.0);

        if let Some((surf, tw, th)) = paint.render_text_surface(shown, self.font_dp, 500, WHITE, (vw + scroll + 64.0).max(vw)) {
            if let Ok(b) = paint.comp.CreateSurfaceBrushWithSurface(&surf) {
                let _ = b.SetStretch(CompositionStretch::None);
                let _ = self.text_vis.SetBrush(&b);
            }
            let _ = self.text_vis.SetSize(Vector2 { X: tw as f32, Y: th as f32 });
            let _ = self.text_vis.SetOffset(Vector3 { X: -scroll.round(), Y: ((h - th as f32) / 2.0).round(), Z: 0.0 });
            let _ = self.text_vis.SetOpacity(op);
        } else {
            let _ = self.text_vis.SetOpacity(0.0);
        }

        let cx = (caret_x - scroll).clamp(0.0, vw - 2.0);
        if let Ok(cur) = self.caret.Offset() {
            let _ = self.caret.SetOffset(Vector3 { X: cx.round(), Y: cur.Y, Z: 0.0 });
        }
        let _ = self.caret.SetOpacity(if self.focused && self.caret_on { 1.0 } else { 0.0 });
    }
}
