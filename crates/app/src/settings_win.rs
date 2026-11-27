use crate::popup::Popup;
use crate::prefs::{self, Role, WM_PREFS};
use nacre_core::hud::slider;
use nacre_core::i18n::Lang;
use nacre_core::notch::spring::{Spring, SpringParams};
use nacre_core::settings::{Behavior, Material, Theme};
use nacre_platform::icons::Bitmap;
use nacre_platform::text::{measure, render, render_face, Style, ICON_FACE};
use nacre_platform::{log, sys};
use nacre_ui::gfx::Gfx;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;
use windows::core::w;
use windows::UI::Color;
use windows::UI::Composition::{
    CompositionColorBrush, CompositionDrawingSurface, CompositionRoundedRectangleGeometry,
    CompositionStretch, Compositor, ContainerVisual, SpriteVisual,
};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_CONTROL};
use windows::Win32::UI::WindowsAndMessaging::*;
const WM_MOUSELEAVE: u32 = 0x02A3;
use windows_core::Interface;
use windows_numerics::{Vector2, Vector3};

const WM_CLOSE_SELF: u32 = WM_APP + 50;
const WM_GEMINI_MODELS: u32 = WM_APP + 51;
const WM_PRAYER_LOC: u32 = WM_APP + 52;
const T_ANIM: usize = 10;
const T_SB_IDLE: usize = 11;
static FETCHED_GEMINI_RESULT: Mutex<Option<Result<Vec<String>, String>>> = Mutex::new(None);
static PRAYER_LOC_RESULT: Mutex<Option<Result<nacre_platform::prayer_loc::GeoResult, String>>> = Mutex::new(None);
const W: f32 = 780.0;
const H: f32 = 560.0;
const SIDEBAR_W: f32 = 214.0;
const CONTENT_X: f32 = SIDEBAR_W + 24.0;
const CONTENT_W: f32 = W - SIDEBAR_W - 48.0;
const VIEW_TOP: f32 = 56.0;
const SIDE_INSET: f32 = 8.0;
const SIDE_RADIUS: f32 = 18.0;

fn side_glass(mut g: nacre_ui::glass::GlassParams) -> nacre_ui::glass::GlassParams {
    g.blur_sigma = g.blur_sigma.max(16.0);
    g
}

fn round_sdf(px: f32, py: f32, r: (f32, f32, f32, f32), rad: f32) -> f32 {
    let (cx, cy) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
    let rad = rad.min(r.2 / 2.0).min(r.3 / 2.0);
    let qx = (px - cx).abs() - (r.2 / 2.0 - rad);
    let qy = (py - cy).abs() - (r.3 / 2.0 - rad);
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - rad
}

fn frame_pixels(w: i32, h: i32, radius: f32, hole: (f32, f32, f32, f32), hole_radius: f32, dark: bool) -> Vec<u8> {
    let base: [f32; 3] = if dark { [16.0, 16.0, 18.0] } else { [255.0, 255.0, 255.0] };
    let (edge, edge_a): ([f32; 3], f32) = if dark { ([255.0, 255.0, 255.0], 0.16) } else { ([0.0, 0.0, 0.0], 0.12) };
    let outer = (0.0, 0.0, w as f32, h as f32);
    let blur = hole_radius * 1.1;
    let shade_dy = hole_radius * 0.12;
    let (tone, peak, inner): ([f32; 3], f32, f32) = if dark { ([255.0, 255.0, 255.0], 0.0, 0.07) } else { ([0.0, 0.0, 0.0], 0.10, 0.05) };
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let d = round_sdf(px, py, outer, radius);
            let cov = (0.5 - d).clamp(0.0, 1.0);
            if cov <= 0.0 {
                continue;
            }
            let a = cov;
            let sd = round_sdf(px, py - shade_dy, hole, hole_radius);
            let shade = if sd <= 0.0 { inner } else { peak * (1.0 - (sd / blur).clamp(0.0, 1.0)).powi(3) };
            let ring = (cov - (0.5 - (d + 1.0)).clamp(0.0, 1.0)).clamp(0.0, 1.0) * edge_a;
            let i = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                let shaded = base[c] * (1.0 - shade) + tone[c] * shade;
                let v = shaded * (1.0 - ring) + edge[c] * ring;
                out[i + 2 - c] = (v * a).round() as u8;
            }
            out[i + 3] = (a * 255.0).round() as u8;
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
enum Action {
    EditText(u8),
    NotchIdle(u8),
    LyricsCompact,
    NotchDownloads,
    NotchSend,
    NotchNotifPeek,
    NotchNotifs,
    NotchTabMove(nacre_core::notch::state::Tab, i8),
    NotchTabToggle(nacre_core::notch::state::Tab),
    NotchTabReset,
    PrayerAutoLoc,
    PrayerDetectLoc,
    PrayerMethod(u8),
    PrayerAlert10m,
    PrayerAlert5m,
    PrayerAlertNow,
    PrayerAlertSound,
    PrayerTestAlert,
    NotifSetup,
    GoogleTasksToggle,
    GeminiModel(u8),
    GeminiFetchModels,
    GeminiCycleFetched,
    LyricsOnline,
    LyricsLead(u16),
    LyricsDir,
    Close,
    Takeover,
    Dock(Behavior),
    Notch(Behavior),
    Rail(Behavior),
    Reveal(u8, nacre_core::settings::Reveal),
    Screen(u8, nacre_core::display::Screen),
    Preset(nacre_core::theme::Preset),
    Visualizer(nacre_core::theme::VisStyle),
    Font(nacre_core::theme::FontChoice),
    ReduceMotion,
    CapLayout(nacre_core::notch::glance::Layout),
    CapDecor(nacre_core::notch::glance::Decor),
    CapToggle(u8),
    CapMove(u8, bool),
    PillDevices,
    Meeting,
    Relaunch,
    Theme(Theme),
    Lang(Lang),
    Tab(u8),
    Autostart,
    Anim,
    Refraction,
    DockOn,
    MenuBarOn,
    Clock24,
    BarStyle(u8),
    HotCycle(usize),
    HideIcons,
    DockPlates,
    IconPack,
    DownloadsStack,
    RecentApps,
    AboveFull,
    ClipHistory,
    MacKeys,
    Accent(u8),
    IconVariant(u8),
    MinEffect(u8),
    Back,
    Forward,
    Search,
    IconStyle(u8),
    IconTint(u8),
    NotchOn,
    HoverOpen,
    WinShadow,
    Material(bool, Material),
    Slider(u8, f32, f32),
    MenuLogoPick,
    MenuLogoReset,
    UiScale(u8),
    NotchShape(bool),
    WidgetStyle(u8),
    WidgetGallery,
    BatteryPct(u8),
    WinColors,
    WinPower,
}

struct Region {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    a: Action,
    scroll: bool,
}

enum RowDef {
    Toggle(&'static str, bool, Action),
    Seg(&'static str, Vec<(&'static str, Action)>, usize),
    Slider(&'static str, u16, u8),
    Pick(&'static str, String, Action),
    Info(&'static str, String),
    Swatches(&'static str, usize),
    Thumbs(&'static str, Vec<(&'static str, Thumb, Action)>, usize),
    Radios(Vec<(&'static str, Action)>, usize),
    Text(&'static str, String, String, u8),
    Order(&'static str, bool, u8, bool, bool),
    /// `(label, index_1based, tab, visible, is_default, can_up, can_down, can_hide)`
    TabOrder(&'static str, usize, nacre_core::notch::state::Tab, bool, bool, bool, bool, bool),
}

#[derive(Clone, Copy)]
enum Thumb {
    Appear(u8),
    Icon(nacre_core::settings::IconStyle),
}

const ACCENT_NAMES: [&str; 9] = ["Multiwarna", "Biru", "Ungu", "Merah Muda", "Merah", "Oranye", "Kuning", "Hijau", "Grafit"];

fn row_label(r: &RowDef) -> &'static str {
    match r {
        RowDef::Toggle(l, ..) | RowDef::Seg(l, ..) | RowDef::Slider(l, ..) | RowDef::Pick(l, ..) | RowDef::Info(l, ..) | RowDef::Swatches(l, ..) | RowDef::Thumbs(l, ..) | RowDef::Text(l, ..) | RowDef::Order(l, ..) | RowDef::TabOrder(l, ..) => l,
        RowDef::Radios(..) => "",
    }
}

fn user_name() -> String {
    std::env::var("USERNAME").ok().filter(|u| !u.is_empty()).unwrap_or_else(|| "NOX".to_string())
}

struct ContentScrollBar {
    track: (f32, f32, f32, f32),
    thumb_h: f32,
    max_scroll: f32,
    right_edge: f32,
    thumb: SpriteVisual,
    clip_geo: CompositionRoundedRectangleGeometry,
    brush: CompositionColorBrush,
    fg: [u8; 3],
}

struct Win {
    popup: Popup,
    gfx: Gfx,
    comp: Compositor,
    dpi: f32,
    size: (f32, f32),
    regions: Vec<Region>,
    cache: HashMap<String, (CompositionDrawingSurface, i32, i32)>,
    dragging: Option<(u8, f32, f32)>,
    side: Vec<(u8, (f32, f32, f32, f32), ContainerVisual)>,
    side_hover: Option<u8>,
    side_scroll: f32,
    side_view: (f32, f32),
    side_bar: Option<((f32, f32, f32, f32), f32)>,
    side_drag: Option<f32>,
    press: Option<(f32, f32)>,
    tab: u8,
    scroll: f32,
    sp_scroll: Spring,
    sp_sb_hover: Spring,
    sp_sb_active: Spring,
    scroll_body: Option<ContainerVisual>,
    content_sb: Option<ContentScrollBar>,
    content_drag: Option<f32>,
    content_sb_hover: bool,
    last_tick: Option<Instant>,
    anim_running: bool,
    content_h: f32,
    history: Vec<u8>,
    hpos: usize,
    search: String,
    search_focus: bool,
    thumbs: HashMap<String, CompositionDrawingSurface>,
    edit: Option<u8>,
    edit_buf: String,
    fetched_models: Vec<String>,
    models_status: String,
    fetching_models: bool,
    prayer_loc_status: String,
    prayer_loc_busy: bool,
    frame: Option<windows::UI::Composition::SpriteVisual>,
    frame_key: (i32, i32, bool),
}

thread_local! {
    static WIN: RefCell<Option<Box<Win>>> = const { RefCell::new(None) };
}

fn screen_row(part: u8, cur: nacre_core::display::Screen) -> Option<RowDef> {
    use nacre_core::display::Screen;
    let count = nacre_platform::monitors::count().min(nacre_core::display::MAX_SCREENS as usize);
    if count <= 1 && cur == Screen::Primary {
        return None;
    }
    let names = ["Layar 1", "Layar 2", "Layar 3", "Layar 4"];
    let mut opts = vec![(prefs::tr("Utama"), Action::Screen(part, Screen::Primary))];
    for (i, n) in names.iter().enumerate().take(count.max(1)) {
        opts.push((prefs::tr(n), Action::Screen(part, Screen::Nth(i as u8 + 1))));
    }
    let sel = match cur {
        Screen::Primary => 0,
        Screen::Nth(n) => (n as usize).min(opts.len() - 1),
    };
    Some(RowDef::Seg(prefs::tr("Tampilkan di"), opts, sel))
}

fn close_button(d: f32) -> (f32, f32, f32) {
    (W * d - 44.0 * d, 13.0 * d, 28.0 * d)
}

pub fn open(comp: &Compositor) {
    let existing = WIN.with(|w| w.borrow().as_ref().map(|b| b.popup.hwnd));
    if let Some(h) = existing {
        unsafe {
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            let _ = SetForegroundWindow(h);
        }
        return;
    }
    match create(comp) {
        Ok(()) => log::line("settings", "dibuka"),
        Err(e) => log::line("settings", &format!("gagal: {e}")),
    }
}

fn create(comp: &Compositor) -> windows_core::Result<()> {
    let glass = side_glass(prefs::glass((10.0, 10.0), SIDE_RADIUS, Role::Themed));
    let mut popup = Popup::new(comp, w!("AbraSettings"), Some(proc), WINDOW_EX_STYLE(0), glass)?;
    let mon = nacre_platform::monitors::under_cursor();
    let dpi = mon.scale();
    let gfx = Gfx::shared(comp)?;
    let tier = {
        let c = sys::conditions();
        if !c.transparency || c.battery_saver || c.remote_session { nacre_ui::glass::Tier::Solid } else { nacre_ui::glass::Tier::Full }
    };
    let _ = popup.panel.set_tier(tier);
    let _ = popup.panel.set_radius(24.0 * dpi);
    let mut win = Box::new(Win {
        popup,
        gfx,
        comp: comp.clone(),
        dpi,
        size: (0.0, 0.0),
        regions: vec![],
        cache: HashMap::new(),
        dragging: None,
        side: vec![],
        side_hover: None,
        side_scroll: 0.0,
        side_view: (0.0, 0.0),
        side_bar: None,
        side_drag: None,
        press: None,
        tab: 0,
        scroll: 0.0,
        sp_scroll: Spring::new(0.0, SpringParams::smooth_scroll()),
        sp_sb_hover: Spring::new_unit(0.0, SpringParams::from_response(0.18, 0.94)),
        sp_sb_active: Spring::new_unit(0.0, SpringParams::from_response(0.18, 0.94)),
        scroll_body: None,
        content_sb: None,
        content_drag: None,
        content_sb_hover: false,
        last_tick: None,
        anim_running: false,
        content_h: 0.0,
        history: vec![0],
        hpos: 0,
        search: String::new(),
        search_focus: false,
        thumbs: HashMap::new(),
        edit: None,
        edit_buf: String::new(),
        fetched_models: Vec::new(),
        models_status: String::new(),
        fetching_models: false,
        prayer_loc_status: String::new(),
        prayer_loc_busy: false,
        frame: None,
        frame_key: (0, 0, false),
    });
    win.render();
    let (sw, sh) = mon.area.size();
    let (w, h) = (win.size.0 as i32, win.size.1 as i32);
    let top = ((sh - h) / 2).clamp(12, (sh - h - 12).max(12));
    log::line("settings", &format!("layar={sw}x{sh}@({},{}) jendela={w}x{h} atas={top}", mon.area.x, mon.area.y));
    win.popup.set_rect(mon.area.x + (sw - w) / 2, mon.area.y + top, w, h)?;
    win.frame();
    win.popup.show();
    prefs::listen(win.popup.hwnd);
    WIN.with(|s| *s.borrow_mut() = Some(win));
    Ok(())
}

fn recolor(b: &Bitmap, rgb: [u8; 3]) -> Vec<u8> {
    let mut out = b.bgra.clone();
    for px in out.chunks_exact_mut(4) {
        let a = px[3] as u32;
        px[0] = (rgb[2] as u32 * a / 255) as u8;
        px[1] = (rgb[1] as u32 * a / 255) as u8;
        px[2] = (rgb[0] as u32 * a / 255) as u8;
    }
    out
}

fn col(rgb: [u8; 3], a: u8) -> Color {
    Color { A: a, R: rgb[0], G: rgb[1], B: rgb[2] }
}

impl Win {
    fn surface(&mut self, s: &str, px: i32, weight: i32, rgb: [u8; 3]) -> Option<(CompositionDrawingSurface, i32, i32)> {
        let key = format!("{px}|{weight}|{rgb:?}|{s}");
        if let Some(v) = self.cache.get(&key) {
            return Some(v.clone());
        }
        if self.cache.len() > 160 {
            self.cache.clear();
        }
        let style = Style { px, weight };
        let (tw, th) = measure(s, &style);
        let tw = tw + 2;
        let bmp = render(s, &style, tw, th)?;
        let data = recolor(&bmp, rgb);
        let surf = self.gfx.surface_from_bgra(bmp.w, bmp.h, &data).ok()?;
        let v = (surf, tw, th);
        self.cache.insert(key, v.clone());
        Some(v)
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, parent: &ContainerVisual, s: &str, x: f32, y: f32, size: f32, weight: i32, rgb: [u8; 3], alpha: f32, anchor: u8) -> f32 {
        let px = (size * self.dpi).round() as i32;
        let Some((surf, w, h)) = self.surface(s, px, weight, rgb) else { return 0.0 };
        let left = match anchor {
            1 => x - w as f32 / 2.0,
            2 => x - w as f32,
            _ => x,
        };
        let make = || -> windows_core::Result<()> {
            let b = self.comp.CreateSurfaceBrushWithSurface(&surf)?;
            b.SetStretch(CompositionStretch::None)?;
            let v = self.comp.CreateSpriteVisual()?;
            v.SetBrush(&b)?;
            v.SetSize(Vector2 { X: w as f32, Y: h as f32 })?;
            v.SetOffset(Vector3 { X: left, Y: y, Z: 0.0 })?;
            v.SetOpacity(alpha)?;
            parent.Children()?.InsertAtTop(&v)
        };
        let _ = make();
        w as f32
    }

    fn rect(&self, parent: &ContainerVisual, x: f32, y: f32, w: f32, h: f32, r: f32, c: Color) {
        let make = || -> windows_core::Result<()> {
            let v = self.comp.CreateSpriteVisual()?;
            v.SetSize(Vector2 { X: w, Y: h })?;
            v.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
            v.SetBrush(&self.comp.CreateColorBrushWithColor(c)?)?;
            if r > 0.0 {
                let g = self.comp.CreateRoundedRectangleGeometry()?;
                g.SetSize(Vector2 { X: w, Y: h })?;
                g.SetCornerRadius(Vector2 { X: r, Y: r })?;
                let clip = self.comp.CreateGeometricClip()?;
                clip.SetGeometry(&g)?;
                v.SetClip(&clip)?;
            }
            parent.Children()?.InsertAtTop(&v)
        };
        let _ = make();
    }

    fn glass_knob(&self, parent: &ContainerVisual, x: f32, y: f32, w: f32, h: f32) {
        let d = self.dpi;
        self.rect(parent, x - 1.0 * d, y + 1.0 * d, w + 2.0 * d, h + 2.0 * d, h / 2.0 + d, Color { A: 46, R: 0, G: 0, B: 0 });
        let make = || -> windows_core::Result<()> {
            let geo = self.comp.CreateRoundedRectangleGeometry()?;
            geo.SetOffset(Vector2 { X: 0.5, Y: 0.5 })?;
            geo.SetSize(Vector2 { X: (w - 1.0).max(0.0), Y: (h - 1.0).max(0.0) })?;
            geo.SetCornerRadius(Vector2 { X: h / 2.0, Y: h / 2.0 })?;
            let shape = self.comp.CreateSpriteShapeWithGeometry(&geo)?;
            shape.SetFillBrush(&self.comp.CreateColorBrushWithColor(Color { A: 92, R: 255, G: 255, B: 255 })?)?;
            shape.SetStrokeBrush(&self.comp.CreateColorBrushWithColor(Color { A: 190, R: 255, G: 255, B: 255 })?)?;
            shape.SetStrokeThickness(1.0)?;
            let v = self.comp.CreateShapeVisual()?;
            v.SetSize(Vector2 { X: w, Y: h })?;
            v.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
            v.Shapes()?.Append(&shape)?;
            parent.Children()?.InsertAtTop(&v)
        };
        let _ = make();
    }

    fn glyph_icon(&mut self, parent: &ContainerVisual, g: &str, cx: f32, cy: f32, size: f32, rgb: [u8; 3]) {
        let px = (size * self.dpi).round() as i32;
        let b = (size * 1.8 * self.dpi).round() as i32;
        let key = format!("ico|{px}|{rgb:?}|{g}");
        let hit = self.cache.get(&key).cloned();
        let (surf, w, h) = match hit {
            Some(v) => v,
            None => {
                let Some(bmp) = render_face(g, &Style { px, weight: 400 }, ICON_FACE, b, b) else { return };
                let Some((cd, cw, ch)) = nacre_core::symbols::crop_ink(&recolor(&bmp, rgb), bmp.w, bmp.h) else { return };
                let Ok(sf) = self.gfx.surface_from_bgra(cw, ch, &cd) else { return };
                let v = (sf, cw as i32, ch as i32);
                self.cache.insert(key, v.clone());
                v
            }
        };
        let make = || -> windows_core::Result<()> {
            let br = self.comp.CreateSurfaceBrushWithSurface(&surf)?;
            br.SetStretch(CompositionStretch::None)?;
            let v = self.comp.CreateSpriteVisual()?;
            v.SetBrush(&br)?;
            v.SetSize(Vector2 { X: w as f32, Y: h as f32 })?;
            v.SetOffset(Vector3 { X: cx - w as f32 / 2.0, Y: cy - h as f32 / 2.0, Z: 0.0 })?;
            parent.Children()?.InsertAtTop(&v)
        };
        let _ = make();
    }

    fn sprite(&self, parent: &ContainerVisual, s: &CompositionDrawingSurface, x: f32, y: f32, w: f32, h: f32) {
        let make = || -> windows_core::Result<()> {
            let b = self.comp.CreateSurfaceBrushWithSurface(s)?;
            b.SetStretch(CompositionStretch::Fill)?;
            let v = self.comp.CreateSpriteVisual()?;
            v.SetBrush(&b)?;
            v.SetSize(Vector2 { X: w, Y: h })?;
            v.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
            parent.Children()?.InsertAtTop(&v)
        };
        let _ = make();
    }

    fn thumb_surface(&mut self, key: String, w: u32, h: u32, make: impl FnOnce() -> Vec<u8>) -> Option<CompositionDrawingSurface> {
        if let Some(s) = self.thumbs.get(&key) {
            return Some(s.clone());
        }
        if self.thumbs.len() > 64 {
            self.thumbs.clear();
        }
        let s = self.gfx.surface_from_bgra(w, h, &make()).ok()?;
        self.thumbs.insert(key, s.clone());
        Some(s)
    }

    fn go(&mut self, t: u8, push: bool) {
        if push && t != self.tab {
            self.history.truncate(self.hpos + 1);
            self.history.push(t);
            self.hpos = self.history.len() - 1;
        }
        self.tab = t;
        self.scroll = 0.0;
        self.sp_scroll.snap(0.0);
        self.content_drag = None;
        self.content_sb_hover = false;
        self.sp_sb_hover.snap(0.0);
        self.render();
    }

    fn start_anim(&mut self) {
        if !self.anim_running {
            self.anim_running = true;
            self.last_tick = Some(Instant::now());
            unsafe {
                let _ = SetTimer(Some(self.popup.hwnd), T_ANIM, 8, None);
            }
        }
    }

    fn stop_anim(&mut self) {
        if self.anim_running {
            self.anim_running = false;
            self.last_tick = None;
            unsafe {
                let _ = KillTimer(Some(self.popup.hwnd), T_ANIM);
            }
        }
    }

    fn wake_sb(&mut self) {
        self.sp_sb_active.target = 1.0;
        unsafe {
            let _ = SetTimer(Some(self.popup.hwnd), T_SB_IDLE, 1400, None);
        }
        self.start_anim();
    }

    fn sync_scroll_visuals(&mut self) {
        let d = self.dpi;
        if let Some(body) = &self.scroll_body {
            let _ = body.SetOffset(Vector3 { X: 0.0, Y: -self.scroll, Z: 0.0 });
        }
        if let Some(sb) = &self.content_sb {
            let h = self.sp_sb_hover.pos.clamp(0.0, 1.0);
            let act = self.sp_sb_active.pos.clamp(0.0, 1.0);
            let bw = (4.0 + 3.0 * h) * d;
            let frac = (self.scroll / sb.max_scroll.max(1.0)).clamp(0.0, 1.0);
            let ty = sb.track.1 + (sb.track.3 - sb.thumb_h) * frac;
            let bx = sb.right_edge - bw;
            let _ = sb.thumb.SetOffset(Vector3 { X: bx, Y: ty, Z: 0.0 });
            let _ = sb.thumb.SetSize(Vector2 { X: bw, Y: sb.thumb_h });
            let _ = sb.clip_geo.SetSize(Vector2 { X: bw, Y: sb.thumb_h });
            let _ = sb.clip_geo.SetCornerRadius(Vector2 { X: bw / 2.0, Y: bw / 2.0 });
            let alpha = (52.0 + 68.0 * act.max(h) + 55.0 * h).clamp(0.0, 220.0).round() as u8;
            let _ = sb.brush.SetColor(col(sb.fg, alpha));
        }
    }

    fn tick_anim(&mut self) {
        let now = Instant::now();
        let dt = self
            .last_tick
            .map(|p| now.duration_since(p).as_secs_f32())
            .unwrap_or(1.0 / 120.0)
            .clamp(0.001, 0.04);
        self.last_tick = Some(now);
        let max_s = self.content_sb.as_ref().map(|s| s.max_scroll).unwrap_or(0.0);
        self.sp_scroll.target = self.sp_scroll.target.clamp(0.0, max_s);
        let moving_scroll = if self.content_drag.is_some() {
            false
        } else {
            let m = self.sp_scroll.step(dt);
            self.sp_scroll.pos = self.sp_scroll.pos.clamp(0.0, max_s);
            self.scroll = self.sp_scroll.pos;
            m
        };
        let m_h = self.sp_sb_hover.step(dt);
        let m_a = self.sp_sb_active.step(dt);
        self.sync_scroll_visuals();
        if !moving_scroll && !m_h && !m_a {
            self.stop_anim();
        }
    }

    fn page_matches(&self, idx: u8, name: &str) -> bool {
        let q = self.search.trim().to_lowercase();
        if q.is_empty() || name.to_lowercase().contains(&q) {
            return true;
        }
        self.page(idx).iter().any(|(h, rows)| h.is_some_and(|h| h.to_lowercase().contains(&q)) || rows.iter().any(|r| row_label(r).to_lowercase().contains(&q)))
    }

    fn region(&mut self, x: f32, y: f32, w: f32, h: f32, a: Action) {
        self.regions.push(Region { x, y, w, h, a, scroll: true });
    }

    fn hover_side(&mut self, hit: Option<u8>) {
        if hit == self.side_hover {
            return;
        }
        for (page, on) in [(self.side_hover, false), (hit, true)] {
            let Some((_, r, ic)) = self.side.iter().find(|e| Some(e.0) == page) else { continue };
            let to = if on { crate::motion::HOVER_ROW } else { 1.0 };
            let _ = crate::motion::hover_pop(&self.comp, ic, Vector3 { X: r.0 + r.2 / 2.0, Y: r.1 + r.3 / 2.0, Z: 0.0 }, to);
        }
        self.side_hover = hit;
    }

    fn region_fixed(&mut self, x: f32, y: f32, w: f32, h: f32, a: Action) {
        self.regions.push(Region { x, y, w, h, a, scroll: false });
    }

    fn spec(id: u8) -> (f32, f32, &'static str) {
        match id {
            4 => (32.0, 72.0, " dp"),
            5 => (0.0, 24.0, " dp"),
            6 => (100.0, 250.0, "%"),
            7 => (0.0, 60.0, " dp"),
            8 => (50.0, 300.0, " ms"),
            _ => (0.0, 100.0, "%"),
        }
    }

    fn commit_edit(&mut self) {
        let Some(f) = self.edit.take() else { return };
        match f {
            0 | 1 => {
                let t = nacre_core::settings::ShortText::new(&self.edit_buf);
                prefs::update(|s| {
                    if f == 0 {
                        s.greet_name = t;
                    } else {
                        s.greet_text = t;
                    }
                });
            }
            2 => {
                nacre_platform::cred::write_secret(nacre_platform::cred::TARGET_GEMINI_KEY, &self.edit_buf);
                self.fetched_models.clear();
                self.models_status.clear();
                prefs::broadcast();
            }
            3 => {
                nacre_platform::cred::write_secret(nacre_platform::cred::TARGET_GTASKS_CLIENT_ID, &self.edit_buf);
                prefs::broadcast();
            }
            4 => {
                nacre_platform::cred::write_secret(nacre_platform::cred::TARGET_GTASKS_CLIENT_SECRET, &self.edit_buf);
                prefs::broadcast();
            }
            5 => {
                let m = nacre_core::settings::ShortText::new(&self.edit_buf);
                prefs::update(|s| s.gemini_custom_model = m);
            }
            6 => {
                let q = self.edit_buf.trim().to_string();
                if !q.is_empty() && !self.prayer_loc_busy {
                    self.prayer_loc_busy = true;
                    self.prayer_loc_status = prefs::tr("Mencari…").to_string();
                    let hw = self.popup.hwnd.0 as isize;
                    std::thread::spawn(move || {
                        let res = nacre_platform::prayer_loc::search_city(&q);
                        if let Ok(mut g) = PRAYER_LOC_RESULT.lock() {
                            *g = Some(res);
                        }
                        unsafe {
                            let _ = PostMessageW(
                                Some(HWND(hw as *mut core::ffi::c_void)),
                                WM_PRAYER_LOC,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    });
                }
            }
            _ => {}
        }
        self.render();
    }

    fn row_height(r: &RowDef) -> f32 {
        match r {
            RowDef::Slider(..) => 50.0,
            RowDef::Thumbs(..) => 96.0,
            RowDef::Radios(..) => 34.0,
            RowDef::Swatches(..) => 58.0,
            _ => 44.0,
        }
    }

    fn card(&mut self, c: &ContainerVisual, y0: f32, rows: &[RowDef], look: &nacre_core::settings::Look) -> f32 {
        let d = self.dpi;
        let (x0, cw) = (CONTENT_X * d, CONTENT_W * d);
        let total: f32 = rows.iter().map(|r| Self::row_height(r) * d).sum();
        self.rect(c, x0, y0, cw, total, 12.0 * d, col(look.fg, if look.dark { 22 } else { 16 }));
        let mut y = y0;
        let accent = prefs::accent();
        for (i, r) in rows.iter().enumerate() {
            let rh = Self::row_height(r) * d;
            if i > 0 && !matches!(r, RowDef::Radios(..)) {
                self.rect(c, x0 + 16.0 * d, y, cw - 32.0 * d, 1.0, 0.0, col(look.fg, 28));
            }
            let (lx, rx) = (x0 + 16.0 * d, x0 + cw - 16.0 * d);
            match r {
                RowDef::Toggle(label, on, act) => {
                    self.text(c, label, lx, y + (rh - 17.0 * d) / 2.0, 13.5, 400, look.fg, 0.95, 0);
                    let (tw, th) = (46.0 * d, 22.0 * d);
                    let (tx, ty) = (rx - tw, y + (rh - th) / 2.0);
                    self.rect(c, tx, ty, tw, th, th / 2.0, if *on { col(accent, 255) } else { col(look.fg, 60) });
                    let knob = th - 4.0 * d;
                    let pressed = self.press.map_or(false, |(px, py)| px >= x0 && px < x0 + cw && py + self.scroll >= y && py + self.scroll < y + rh && py > VIEW_TOP * d);
                    let kw = if pressed { knob * 1.8 } else { knob * 1.55 };
                    let kx = if *on { tx + tw - kw - 2.0 * d } else { tx + 2.0 * d };
                    if pressed {
                        self.glass_knob(c, kx, ty + 2.0 * d, kw, knob);
                    } else {
                        self.rect(c, kx, ty + 2.0 * d, kw, knob, knob / 2.0, Color { A: 255, R: 255, G: 255, B: 255 });
                    }
                    self.region(x0, y, cw, rh, *act);
                }
                RowDef::Seg(label, opts, sel) => {
                    self.text(c, label, lx, y + (rh - 17.0 * d) / 2.0, 13.5, 400, look.fg, 0.95, 0);
                    let texts: Vec<f32> = opts.iter().map(|(t, _)| measure(t, &Style { px: (12.5 * d).round() as i32, weight: 500 }).0 as f32).collect();
                    let avail = (rx - lx) - measure(label, &Style { px: (13.5 * d).round() as i32, weight: 400 }).0 as f32 - 28.0 * d;
                    let need = |pad: f32, min: f32| texts.iter().map(|w| (w + pad).max(min)).sum::<f32>() + 6.0 * d;
                    let (pad, min) = if need(32.0 * d, 66.0 * d) <= avail { (32.0 * d, 66.0 * d) } else { (18.0 * d, 48.0 * d) };
                    let widths: Vec<f32> = texts.iter().map(|w| (w + pad).max(min)).collect();
                    let total_w: f32 = widths.iter().sum::<f32>() + 6.0 * d;
                    let (bx, bh) = (rx - total_w, 26.0 * d);
                    let by = y + (rh - bh) / 2.0;
                    self.rect(c, bx, by, total_w, bh, bh / 2.0, col(look.fg, 30));
                    let inverse = if look.dark { [10u8, 10, 12] } else { [255u8, 255, 255] };
                    let mut sx = bx + 3.0 * d;
                    for (i, (t, act)) in opts.iter().enumerate() {
                        let on = i == *sel;
                        if on {
                            self.rect(c, sx, by + 3.0 * d, widths[i], bh - 6.0 * d, (bh - 6.0 * d) / 2.0, col(look.fg, 235));
                        }
                        let rgb = if on { inverse } else { look.fg };
                        self.text(c, t, sx + widths[i] / 2.0, by + (bh - 16.0 * d) / 2.0 - 1.0, 12.5, if on { 600 } else { 500 }, rgb, if on { 1.0 } else { 0.85 }, 1);
                        self.region(sx, y, widths[i], rh, *act);
                        sx += widths[i];
                    }
                }
                RowDef::Slider(label, value, id) => {
                    let (min, max, unit) = Self::spec(*id);
                    let frac = ((*value as f32 - min) / (max - min)).clamp(0.0, 1.0);
                    self.text(c, label, lx, y + (rh - 17.0 * d) / 2.0, 13.5, 400, look.fg, 0.95, 0);
                    self.text(c, &format!("{value}{unit}"), rx, y + (rh - 17.0 * d) / 2.0, 12.5, 500, look.fg, 0.6, 2);
                    let (tw, th) = (190.0 * d, 5.0 * d);
                    let tx = rx - 56.0 * d - tw;
                    let ty = y + rh / 2.0 - th / 2.0;
                    self.rect(c, tx, ty, tw, th, th / 2.0, col(look.fg, 50));
                    self.rect(c, tx, ty, (tw * frac).max(th), th, th / 2.0, col(accent, 255));
                    let knob = 18.0 * d;
                    if self.dragging.map(|(i, _, _)| i) == Some(*id) {
                        let (kw, kh) = (knob * 1.9, knob * 1.25);
                        let cx = (tx + tw * frac).clamp(tx + kw / 2.0 - knob / 4.0, tx + tw - kw / 2.0 + knob / 4.0);
                        self.glass_knob(c, cx - kw / 2.0, ty + th / 2.0 - kh / 2.0, kw, kh);
                    } else {
                        let kx = (tx + tw * frac - knob / 2.0).clamp(tx - knob / 4.0, tx + tw - knob * 0.75);
                        self.rect(c, kx - 1.0 * d, ty + th / 2.0 - knob / 2.0, knob + 2.0 * d, knob + 2.0 * d, knob / 2.0 + d, Color { A: 60, R: 0, G: 0, B: 0 });
                        self.rect(c, kx, ty + th / 2.0 - knob / 2.0 - 1.0 * d, knob, knob, knob / 2.0, Color { A: 255, R: 255, G: 255, B: 255 });
                    }
                    self.region(tx - 8.0 * d, y + 4.0 * d, tw + 16.0 * d, rh - 8.0 * d, Action::Slider(*id, tx, tw));
                }
                RowDef::Swatches(label, sel) => {
                    self.text(c, label, lx, y + 13.0 * d, 13.5, 400, look.fg, 0.95, 0);
                    let (dia, gap) = (19.0 * d, 10.0 * d);
                    let n = nacre_core::settings::ACCENTS.len() + 1;
                    let total_w = n as f32 * (dia + gap) - gap;
                    let mut sx = rx - total_w;
                    let cy = y + 12.0 * d;
                    for i in 0..n {
                        if i == 0 {
                            let key = format!("multi|{}", dia.round());
                            let px = dia.round() as u32;
                            if let Some(s) = self.thumb_surface(key, px, px, || nacre_core::preview::multicolor(px)) {
                                self.sprite(c, &s, sx, cy, dia, dia);
                            }
                        } else {
                            self.rect(c, sx, cy, dia, dia, dia / 2.0, col(nacre_core::settings::ACCENTS[i - 1], 255));
                        }
                        if i == *sel {
                            self.rect(c, sx + dia * 0.32, cy + dia * 0.32, dia * 0.36, dia * 0.36, dia * 0.18, Color { A: 255, R: 255, G: 255, B: 255 });
                            self.text(c, prefs::tr(ACCENT_NAMES[i]), sx + dia / 2.0, cy + dia + 5.0 * d, 10.5, 500, look.fg, 0.7, 1);
                        }
                        let v = if i == 0 { nacre_core::settings::ACCENT_MULTI } else { (i - 1) as u8 };
                        self.region(sx - 2.0 * d, y, dia + 4.0 * d, rh, Action::Accent(v));
                        sx += dia + gap;
                    }
                }
                RowDef::Thumbs(label, opts, sel) => {
                    self.text(c, label, lx, y + 14.0 * d, 13.5, 400, look.fg, 0.95, 0);
                    let icon_kind = matches!(opts.first().map(|o| o.1), Some(Thumb::Icon(_)));
                    let (tw, th) = if icon_kind { (48.0 * d, 48.0 * d) } else { (70.0 * d, 46.0 * d) };
                    let gap = 16.0 * d;
                    let total = opts.len() as f32 * tw + (opts.len() as f32 - 1.0) * gap;
                    let mut x = rx - total;
                    let ty = y + 12.0 * d;
                    let dark_icons = prefs::icon_dark();
                    let s0 = prefs::get();
                    for (i, (name, kind, act)) in opts.iter().enumerate() {
                        let on = i == *sel;
                        let r = if icon_kind { tw * 0.22 } else { th * 0.14 };
                        if on {
                            self.rect(c, x - 3.0 * d, ty - 3.0 * d, tw + 6.0 * d, th + 6.0 * d, r + 3.0 * d, col(accent, 255));
                            self.rect(c, x - 1.0 * d, ty - 1.0 * d, tw + 2.0 * d, th + 2.0 * d, r + 1.0 * d, col(if look.dark { [30, 30, 34] } else { [246, 246, 248] }, 255));
                        }
                        let (pw, ph) = (tw.round() as u32, th.round() as u32);
                        let surf = match *kind {
                            Thumb::Appear(k) => self.thumb_surface(format!("appear|{k}|{pw}x{ph}"), pw, ph, || nacre_core::preview::appearance(k, pw, ph)),
                            Thumb::Icon(st) => {
                                let tint = s0.icon_tint;
                                self.thumb_surface(format!("icon|{st:?}|{tint}|{dark_icons}|{pw}"), pw, pw, || nacre_core::preview::icon_tile(st, tint, dark_icons, pw))
                            }
                        };
                        if let Some(sf) = surf {
                            self.sprite(c, &sf, x, ty, tw, th);
                        }
                        self.text(c, name, x + tw / 2.0, ty + th + 6.0 * d, 11.5, if on { 600 } else { 400 }, look.fg, if on { 1.0 } else { 0.7 }, 1);
                        self.region(x - 4.0 * d, y, tw + 8.0 * d, rh, *act);
                        x += tw + gap;
                    }
                }
                RowDef::Radios(opts, sel) => {
                    let rpx = (12.5 * d).round() as i32;
                    let dia = 14.0 * d;
                    let widths: Vec<f32> = opts.iter().map(|(t, _)| dia + 6.0 * d + measure(t, &Style { px: rpx, weight: 400 }).0 as f32 + 16.0 * d).collect();
                    let mut x = rx - widths.iter().sum::<f32>() + 16.0 * d;
                    let cy = y + rh / 2.0 - 4.0 * d;
                    for (i, (t, act)) in opts.iter().enumerate() {
                        let on = i == *sel;
                        if on {
                            self.rect(c, x, cy - dia / 2.0, dia, dia, dia / 2.0, col(accent, 255));
                            self.rect(c, x + dia * 0.32, cy - dia * 0.18, dia * 0.36, dia * 0.36, dia * 0.18, Color { A: 255, R: 255, G: 255, B: 255 });
                        } else {
                            self.rect(c, x, cy - dia / 2.0, dia, dia, dia / 2.0, col(look.fg, 70));
                            self.rect(c, x + 1.2 * d, cy - dia / 2.0 + 1.2 * d, dia - 2.4 * d, dia - 2.4 * d, dia / 2.0 - 1.2 * d, col(if look.dark { [44, 44, 48] } else { [244, 244, 246] }, 255));
                        }
                        self.text(c, t, x + dia + 6.0 * d, cy - 8.0 * d, 12.5, 400, look.fg, 0.9, 0);
                        self.region(x - 4.0 * d, y, widths[i], rh, *act);
                        x += widths[i];
                    }
                }
                RowDef::Text(label, value, hint, field) => {
                    self.text(c, label, lx, y + (rh - 17.0 * d) / 2.0, 13.5, 400, look.fg, 0.95, 0);
                    let editing = self.edit == Some(*field);
                    let (bw, bh) = (240.0 * d, 26.0 * d);
                    let (bx, by) = (rx - bw, y + (rh - bh) / 2.0);
                    self.rect(c, bx, by, bw, bh, bh / 2.0, col(look.fg, if editing { 40 } else { 26 }));
                    if editing {
                        self.rect(c, bx, by, bw, bh, bh / 2.0, col(accent, 40));
                    }
                    let (shown, op) = if editing {
                        (format!("{}|", self.edit_buf), 1.0)
                    } else if value.is_empty() {
                        (hint.clone(), 0.45)
                    } else {
                        (value.clone(), 0.95)
                    };
                    let inner_w = (bw - 20.0 * d).max(1.0);
                    let tw = measure(&shown, &Style { px: (12.5 * d).round() as i32, weight: 500 }).0 as f32;
                    let tx = if editing && tw > inner_w { inner_w - tw } else { 2.0 * d };
                    if let Ok(clip_box) = self.comp.CreateContainerVisual() {
                        let _ = clip_box.SetOffset(Vector3 { X: bx + 10.0 * d, Y: by, Z: 0.0 });
                        let _ = clip_box.SetSize(Vector2 { X: inner_w, Y: bh });
                        if let Ok(clip) = self.comp.CreateInsetClip() {
                            let _ = clip_box.SetClip(&clip);
                        }
                        let _ = c.Children().and_then(|k| k.InsertAtTop(&clip_box));
                        self.text(&clip_box, &shown, tx, (bh - 16.0 * d) / 2.0 - 1.0, 12.5, 500, look.fg, op, 0);
                    } else {
                        self.text(c, &shown, bx + 12.0 * d, by + (bh - 16.0 * d) / 2.0 - 1.0, 12.5, 500, look.fg, op, 0);
                    }
                    self.region(bx, y, bw, rh, Action::EditText(*field));
                }
                RowDef::Info(label, value) => {
                    self.text(c, label, lx, y + (rh - 17.0 * d) / 2.0, 13.5, 400, look.fg, 0.95, 0);
                    self.text(c, value, rx, y + (rh - 16.0 * d) / 2.0, 12.5, 500, look.fg, 0.55, 2);
                }
                RowDef::Pick(label, value, act) => {
                    self.text(c, label, lx, y + (rh - 17.0 * d) / 2.0, 13.5, 400, look.fg, 0.95, 0);
                    let vw = (measure(value, &Style { px: (12.5 * d).round() as i32, weight: 500 }).0 as f32 + 44.0 * d).max(96.0 * d);
                    let (bx, bh) = (rx - vw, 26.0 * d);
                    let by = y + (rh - bh) / 2.0;
                    self.rect(c, bx, by, vw, bh, bh / 2.0, col(look.fg, 30));
                    self.text(c, value, bx + 14.0 * d, by + (bh - 16.0 * d) / 2.0 - 1.0, 12.5, 500, look.fg, 1.0, 0);
                    self.text(c, "\u{2303}", bx + vw - 14.0 * d, by + (bh - 16.0 * d) / 2.0, 10.0, 600, look.fg, 0.6, 1);
                    self.region(x0, y, cw, rh, *act);
                }
                RowDef::Order(label, on, idx, up, down) => {
                    self.text(c, label, lx, y + (rh - 17.0 * d) / 2.0, 13.5, 400, look.fg, if *on { 0.95 } else { 0.50 }, 0);
                    let (tw, th) = (46.0 * d, 22.0 * d);
                    let (tx, ty) = (rx - tw, y + (rh - th) / 2.0);
                    self.rect(c, tx, ty, tw, th, th / 2.0, if *on { col(accent, 255) } else { col(look.fg, 60) });
                    let knob = th - 4.0 * d;
                    let kw = knob * 1.55;
                    let kx = if *on { tx + tw - kw - 2.0 * d } else { tx + 2.0 * d };
                    self.rect(c, kx, ty + 2.0 * d, kw, knob, knob / 2.0, Color { A: 255, R: 255, G: 255, B: 255 });
                    self.region(tx - 4.0 * d, y, tw + 8.0 * d, rh, Action::CapToggle(*idx));
                    if *on {
                        let (bw, bh, gap) = (26.0 * d, 24.0 * d, 6.0 * d);
                        let by = y + (rh - bh) / 2.0;
                        let dn_x = tx - 10.0 * d - bw;
                        let up_x = dn_x - gap - bw;
                        self.rect(c, up_x, by, bw, bh, 7.0 * d, col(look.fg, if *up { 30 } else { 12 }));
                        self.text(c, "\u{2191}", up_x + bw / 2.0, by + (bh - 15.0 * d) / 2.0 - 0.5, 12.0, 600, look.fg, if *up { 0.92 } else { 0.25 }, 1);
                        if *up {
                            self.region(up_x, y, bw, rh, Action::CapMove(*idx, true));
                        }
                        self.rect(c, dn_x, by, bw, bh, 7.0 * d, col(look.fg, if *down { 30 } else { 12 }));
                        self.text(c, "\u{2193}", dn_x + bw / 2.0, by + (bh - 15.0 * d) / 2.0 - 0.5, 12.0, 600, look.fg, if *down { 0.92 } else { 0.25 }, 1);
                        if *down {
                            self.region(dn_x, y, bw, rh, Action::CapMove(*idx, false));
                        }
                    }
                }
                RowDef::TabOrder(label, num, tab, visible, is_default, can_up, can_down, can_hide) => {
                    let lbl_txt = format!("{num}. {label}");
                    let lbl_op = if *visible { 0.95 } else { 0.42 };
                    let lw = measure(&lbl_txt, &Style { px: (13.5 * d).round() as i32, weight: 400 }).0 as f32;
                    self.text(c, &lbl_txt, lx, y + (rh - 17.0 * d) / 2.0, 13.5, 400, look.fg, lbl_op, 0);
                    if *is_default {
                        let badge = prefs::tr("Utama");
                        let bw = measure(badge, &Style { px: (10.5 * d).round() as i32, weight: 600 }).0 as f32 + 14.0 * d;
                        let bh = 18.0 * d;
                        let bx = lx + lw + 10.0 * d;
                        let by = y + (rh - bh) / 2.0;
                        self.rect(c, bx, by, bw, bh, bh / 2.0, col(accent, 42));
                        self.text(c, badge, bx + bw / 2.0, by + (bh - 13.0 * d) / 2.0 - 0.5, 10.5, 600, accent, 1.0, 1);
                    }

                    // Right-side controls: [↑] [↓] [Toggle]
                    let (tw, th) = (46.0 * d, 22.0 * d);
                    let (tx, ty) = (rx - tw, y + (rh - th) / 2.0);
                    let toggle_alpha = if *visible && !*can_hide { 145 } else { 255 };
                    let track_col = if *visible { col(accent, toggle_alpha) } else { col(look.fg, 60) };
                    self.rect(c, tx, ty, tw, th, th / 2.0, track_col);
                    let knob = th - 4.0 * d;
                    let kw = knob * 1.55;
                    let kx = if *visible { tx + tw - kw - 2.0 * d } else { tx + 2.0 * d };
                    self.rect(c, kx, ty + 2.0 * d, kw, knob, knob / 2.0, Color { A: if *visible && !*can_hide { 200 } else { 255 }, R: 255, G: 255, B: 255 });
                    if *can_hide || !*visible {
                        self.region(tx - 4.0 * d, y, tw + 8.0 * d, rh, Action::NotchTabToggle(*tab));
                    }

                    let (bw, bh, gap) = (26.0 * d, 24.0 * d, 6.0 * d);
                    let by = y + (rh - bh) / 2.0;
                    let dn_x = tx - 10.0 * d - bw;
                    let up_x = dn_x - gap - bw;

                    self.rect(c, up_x, by, bw, bh, 7.0 * d, col(look.fg, if *can_up { 30 } else { 12 }));
                    self.text(c, "\u{2191}", up_x + bw / 2.0, by + (bh - 15.0 * d) / 2.0 - 0.5, 12.0, 600, look.fg, if *can_up { 0.92 } else { 0.25 }, 1);
                    if *can_up {
                        self.region(up_x, y, bw, rh, Action::NotchTabMove(*tab, -1));
                    }

                    self.rect(c, dn_x, by, bw, bh, 7.0 * d, col(look.fg, if *can_down { 30 } else { 12 }));
                    self.text(c, "\u{2193}", dn_x + bw / 2.0, by + (bh - 15.0 * d) / 2.0 - 0.5, 12.0, 600, look.fg, if *can_down { 0.92 } else { 0.25 }, 1);
                    if *can_down {
                        self.region(dn_x, y, bw, rh, Action::NotchTabMove(*tab, 1));
                    }
                }
            }
            y += rh;
        }
        total
    }

    fn page(&self, page: u8) -> Vec<(Option<&'static str>, Vec<RowDef>)> {
        let mut groups = self.page_rows(page);
        let s = prefs::get();
        let (part, cur, at) = match page {
            1 => (0u8, s.deck_screen, 2),
            2 => (2, s.notch_screen, 3),
            3 => (1, s.rail_screen, 1),
            7 => (3, s.widget_screen, 0),
            _ => return groups,
        };
        if let Some(row) = screen_row(part, cur) {
            if let Some(g) = groups.first_mut() {
                let at = at.min(g.1.len());
                g.1.insert(at, row);
            }
        }
        if part < 3 {
            use nacre_core::settings::Reveal;
            let cur = match part {
                0 => s.dock_reveal,
                1 => s.rail_reveal,
                _ => s.notch_reveal,
            };
            let row = RowDef::Seg(
                prefs::tr("Kepekaan"),
                Reveal::ALL.iter().map(|r| (prefs::tr(r.title()), Action::Reveal(part, *r))).collect(),
                Reveal::ALL.iter().position(|r| *r == cur).unwrap_or(0),
            );
            if let Some(g) = groups.first_mut() {
                g.1.push(row);
            }
        }
        groups
    }

    fn page_rows(&self, page: u8) -> Vec<(Option<&'static str>, Vec<RowDef>)> {
        let s = prefs::get();
        let behaviors = |dock: bool| -> Vec<(&'static str, Action)> {
            let mut list = vec![
                (prefs::tr("Tetap"), Behavior::Fixed),
                (prefs::tr("Menimpa"), Behavior::Overlay),
                (prefs::tr("Pintar"), Behavior::Smart),
            ];
            if !dock {
                list.push((prefs::tr("Auto-hide"), Behavior::AutoHide));
            }
            list.into_iter()
                .map(|(t, b)| (t, if dock { Action::Dock(b) } else { Action::Notch(b) }))
                .collect()
        };
        let beh_idx = |b: Behavior| match b {
            Behavior::Fixed => 0,
            Behavior::Overlay => 1,
            Behavior::Smart => 2,
            Behavior::AutoHide => 3,
        };
        match page {
            0 => vec![
                (None, vec![RowDef::Seg(prefs::tr("Bahasa"), vec![("Indonesia", Action::Lang(Lang::Id)), ("English", Action::Lang(Lang::En))], if s.lang == Lang::Id { 0 } else { 1 })]),
                (
                    Some(prefs::tr("Sistem")),
                    vec![
                        RowDef::Toggle(prefs::tr("Jalankan saat login Windows"), s.autostart, Action::Autostart),
                        RowDef::Toggle(prefs::tr("Sembunyikan taskbar Windows"), s.takeover, Action::Takeover),
                        RowDef::Toggle(prefs::tr("Sembunyikan ikon desktop saat NOX berjalan"), s.hide_desktop_icons, Action::HideIcons),
                        RowDef::Toggle(prefs::tr("Selalu di atas jendela layar penuh"), s.above_fullscreen, Action::AboveFull),
                        RowDef::Toggle(prefs::tr("Pintasan cepat (Alt+` ganti jendela app)"), s.mac_shortcuts, Action::MacKeys),
                    ],
                ),
                (
                    Some(prefs::tr("Jendela")),
                    vec![
                        RowDef::Toggle(prefs::tr("Animasi jendela (minimize, tutup, buka)"), s.anim, Action::Anim),
                        RowDef::Toggle(prefs::tr("Bayangan halus di bawah jendela aktif"), s.win_shadow, Action::WinShadow),
                    ],
                ),
                (
                    Some(prefs::tr("Tentang")),
                    vec![
                        RowDef::Info(prefs::tr("Versi"), format!("NOX v{}", env!("CARGO_PKG_VERSION"))),
                        RowDef::Info(prefs::tr("Dibuat oleh opankcs.com"), "opankcs.com".to_string()),
                    ],
                ),
            ],
            1 => vec![
                (
                    None,
                    vec![
                        RowDef::Toggle(prefs::tr("Aktifkan Deck"), s.dock_enabled, Action::DockOn),
                        RowDef::Seg(prefs::tr("Perilaku"), behaviors(true), beh_idx(s.dock)),
                        RowDef::Seg(prefs::tr("Efek minimize"), vec![(prefs::tr("Funnel"), Action::MinEffect(0)), (prefs::tr("Skala"), Action::MinEffect(1))], s.min_effect as usize),
                        RowDef::Toggle(prefs::tr("Ikon seragam (squircle)"), s.dock_plates, Action::DockPlates),
                        RowDef::Toggle(prefs::tr("Tumpukan Unduhan"), s.downloads_stack, Action::DownloadsStack),
                        RowDef::Toggle(prefs::tr("App terakhir dipakai"), s.recent_apps, Action::RecentApps),
                    ],
                ),
                (
                    Some(prefs::tr("Ukuran")),
                    vec![
                        RowDef::Slider(prefs::tr("Ukuran ikon"), s.dock_size as u16, 4),
                        RowDef::Slider(prefs::tr("Jarak antar ikon"), s.dock_gap as u16, 5),
                        RowDef::Slider(prefs::tr("Zoom saat hover"), s.dock_zoom as u16, 6),
                        RowDef::Slider(prefs::tr("Jarak dari tepi bawah"), s.dock_margin as u16, 7),
                        RowDef::Slider(prefs::tr("Kebulatan sudut"), s.dock_round as u16, 2),
                    ],
                ),
            ],
            2 => {
                let mut groups = vec![(
                    None,
                    vec![
                        RowDef::Toggle(prefs::tr("Aktifkan Notch"), s.notch_enabled, Action::NotchOn),
                        RowDef::Seg(prefs::tr("Bentuk"), vec![(prefs::tr("Notch"), Action::NotchShape(false)), (prefs::tr("Capsule"), Action::NotchShape(true))], if s.notch_island { 1 } else { 0 }),
                        RowDef::Seg(prefs::tr("Perilaku"), behaviors(false), beh_idx(s.notch)),
                        RowDef::Seg(
                            prefs::tr("Visualizer musik"),
                            nacre_core::theme::VisStyle::ALL.iter().map(|v| (prefs::tr(v.title()), Action::Visualizer(*v))).collect(),
                            nacre_core::theme::VisStyle::ALL.iter().position(|v| *v == s.visualizer).unwrap_or(0),
                        ),
                        RowDef::Slider(prefs::tr("Kebulatan sudut"), s.notch_round as u16, 3),
                        RowDef::Toggle(prefs::tr("Buka otomatis saat kursor di Notch"), s.notch_hover_open, Action::HoverOpen),
                    ],
                )];
                if matches!(s.notch, Behavior::Smart | Behavior::AutoHide) {
                    groups.push((
                        Some(prefs::tr("Auto-hide")),
                        vec![
                            RowDef::Slider(prefs::tr("Jeda hover Auto-hide"), s.notch_autohide_delay, 8),
                            RowDef::Toggle(prefs::tr("Intip otomatis saat notifikasi masuk"), s.notch_notif_peek, Action::NotchNotifPeek),
                        ],
                    ));
                }
                groups.push((
                    Some(prefs::tr("Notifikasi Sistem")),
                    {
                        use nacre_platform::notifications::{status, Status};
                        let st = match status() {
                            Status::Connected => prefs::tr("Aktif (WinRT)"),
                            Status::ConnectedUia => prefs::tr("Aktif (Zero-Config)"),
                            Status::NeedsIdentity => prefs::tr("Mode Zero-Config"),
                            Status::Denied => prefs::tr("Ditolak"),
                            Status::Unsupported => prefs::tr("Tidak didukung"),
                            Status::Connecting => prefs::tr("Menyambung…"),
                        };
                        let mut rows = vec![RowDef::Toggle(prefs::tr("Tampilkan notifikasi Windows di Notch"), s.notch_notifs, Action::NotchNotifs)];
                        if s.notch_notifs {
                            rows.push(RowDef::Pick(prefs::tr("Listener notifikasi Windows"), st.to_string(), Action::NotifSetup));
                        }
                        rows.push(RowDef::Toggle(prefs::tr("Perangkat Bluetooth & flashdisk tersambung"), s.pill_devices, Action::PillDevices));
                        rows.push(RowDef::Toggle(prefs::tr("Pengingat rapat (Google & Kalender Windows)"), s.meeting_companion, Action::Meeting));
                        rows
                    },
                ));
                groups.push((
                    Some(prefs::tr("Saat Notch diam")),
                    {
                        let name = if s.greet_name.is_empty() { user_name() } else { s.greet_name.as_str().to_string() };
                        let hour = nacre_platform::time::local_now().hour;
                        let auto = nacre_core::notch::glance::greeting_text("", &name, hour, s.lang);
                        let mut rows = vec![RowDef::Seg(
                            prefs::tr("Tampilkan"),
                            vec![(prefs::tr("Sapaan & info"), Action::NotchIdle(0)), (prefs::tr("Musik & lirik"), Action::NotchIdle(2)), (prefs::tr("Kosong"), Action::NotchIdle(1))],
                            match s.notch_idle {
                                0 => 0,
                                2 => 1,
                                _ => 2,
                            },
                        )];
                        if s.notch_idle == 0 {
                            rows.push(RowDef::Text(prefs::tr("Nama panggilan"), s.greet_name.as_str().to_string(), user_name(), 0));
                            rows.push(RowDef::Text(prefs::tr("Teks sapaan"), s.greet_text.as_str().to_string(), auto, 1));
                        }
                        rows
                    },
                ));
                if s.notch_idle == 0 {
                    use nacre_core::notch::glance::{Decor, Layout};
                    let mut rows = vec![
                        RowDef::Seg(
                            prefs::tr("Tata letak"),
                            vec![(prefs::tr("Bergantian"), Action::CapLayout(Layout::Rotate)), (prefs::tr("Berdampingan"), Action::CapLayout(Layout::Split))],
                            usize::from(s.capsule_layout == Layout::Split),
                        ),
                        RowDef::Seg(
                            prefs::tr("Hiasan"),
                            vec![
                                (prefs::tr("Tidak ada"), Action::CapDecor(Decor::None)),
                                (prefs::tr("Titik aksen"), Action::CapDecor(Decor::Dot)),
                                (prefs::tr("Kilau tepi"), Action::CapDecor(Decor::Edge)),
                            ],
                            match s.capsule_decor {
                                Decor::None => 0,
                                Decor::Dot => 1,
                                Decor::Edge => 2,
                            },
                        ),
                    ];
                    let on = s.capsule.on as usize;
                    for (i, it) in s.capsule.order.iter().enumerate() {
                        rows.push(RowDef::Order(prefs::tr(it.title()), i < on, i as u8, i < on && i > 0, i + 1 < on));
                    }
                    groups.push((Some(prefs::tr("Modul Capsule")), rows));
                }
                groups.push((
                    Some(prefs::tr("Lirik")),
                    {
                        let lead_sel = match s.lyrics_lead_ms {
                            0..=149 => 0,
                            150..=399 => 1,
                            400..=649 => 2,
                            650..=899 => 3,
                            _ => 4,
                        };
                        vec![
                            RowDef::Toggle(prefs::tr("Tampilkan lirik di Notch"), s.lyrics_compact, Action::LyricsCompact),
                            RowDef::Toggle(prefs::tr("Cari lirik online (BetterLyrics + cadangan)"), s.lyrics_online, Action::LyricsOnline),
                            RowDef::Seg(
                                prefs::tr("Lirik lebih awal"),
                                vec![
                                    ("0s", Action::LyricsLead(0)),
                                    ("0.3s", Action::LyricsLead(300)),
                                    ("0.5s", Action::LyricsLead(500)),
                                    ("0.8s", Action::LyricsLead(800)),
                                    ("1s", Action::LyricsLead(1000)),
                                ],
                                lead_sel,
                            ),
                            RowDef::Pick(prefs::tr("Folder lirik (.lrc)"), prefs::tr("Buka").to_string(), Action::LyricsDir),
                        ]
                    },
                ));
                groups.push((
                    Some(prefs::tr("Jadwal Shalat")),
                    {
                        let loc_lbl = if !self.prayer_loc_status.is_empty() {
                            self.prayer_loc_status.clone()
                        } else {
                            prefs::tr("Deteksi sekarang").to_string()
                        };
                        vec![
                            RowDef::Text(
                                prefs::tr("Kota"),
                                s.prayer_city.as_str().to_string(),
                                prefs::tr("Ketik nama kota (Enter)…").to_string(),
                                6,
                            ),
                            RowDef::Toggle(prefs::tr("Izinkan akses lokasi Windows"), s.prayer_auto_loc, Action::PrayerAutoLoc),
                            RowDef::Pick(prefs::tr("Deteksi lokasi otomatis"), loc_lbl, Action::PrayerDetectLoc),
                            RowDef::Seg(
                                prefs::tr("Metode hisab"),
                                vec![
                                    (prefs::tr("Kemenag"), Action::PrayerMethod(0)),
                                    ("MWL", Action::PrayerMethod(1)),
                                    (prefs::tr("Makkah"), Action::PrayerMethod(2)),
                                    ("ISNA", Action::PrayerMethod(3)),
                                    (prefs::tr("Mesir"), Action::PrayerMethod(4)),
                                ],
                                (s.prayer_method as usize).min(4),
                            ),
                            RowDef::Toggle(prefs::tr("Pengingat 10 menit sebelum adzan"), s.prayer_alert_10m, Action::PrayerAlert10m),
                            RowDef::Toggle(prefs::tr("Pengingat 5 menit sebelum adzan"), s.prayer_alert_5m, Action::PrayerAlert5m),
                            RowDef::Toggle(prefs::tr("Pengingat saat tiba waktu adzan"), s.prayer_alert_now, Action::PrayerAlertNow),
                            RowDef::Toggle(prefs::tr("Suara pengingat adzan (chime)"), s.prayer_alert_sound, Action::PrayerAlertSound),
                            RowDef::Pick(prefs::tr("Uji pengingat adzan"), prefs::tr("Uji sekarang").to_string(), Action::PrayerTestAlert),
                        ]
                    },
                ));
                groups.push((
                    Some(prefs::tr("Tab Notch")),
                    {
                        use nacre_core::notch::state::Tab;
                        let vis_count = s.visible_notch_tabs().len();
                        let default_tab = s.default_notch_tab();
                        let mut rows: Vec<RowDef> = s
                            .notch_tab_order
                            .iter()
                            .copied()
                            .enumerate()
                            .map(|(idx, tab)| {
                                let vis = s.notch_tab_visible(tab);
                                RowDef::TabOrder(
                                    prefs::tr(tab.title()),
                                    idx + 1,
                                    tab,
                                    vis,
                                    vis && tab == default_tab,
                                    idx > 0,
                                    idx + 1 < s.notch_tab_order.len(),
                                    vis_count > 1,
                                )
                            })
                            .collect();
                        if s.notch_tab_order != Tab::ALL || s.notch_tab_mask != nacre_core::settings::ALL_NOTCH_TABS_MASK {
                            rows.push(RowDef::Pick(
                                prefs::tr("Urutan tab"),
                                prefs::tr("Kembalikan bawaan").to_string(),
                                Action::NotchTabReset,
                            ));
                        }
                        rows
                    },
                ));
                groups.push((
                    Some(prefs::tr("Aktivitas")),
                    vec![
                        RowDef::Toggle(prefs::tr("Tampilkan unduhan (Store, Steam, peramban)"), s.notch_downloads, Action::NotchDownloads),
                        RowDef::Toggle(prefs::tr("Kirim ke HP lewat QR (Wi-Fi yang sama)"), s.notch_send, Action::NotchSend),
                    ],
                ));
                groups
            }
            3 => {
                let cur = if s.menubar_material {
                    3
                } else if !s.menubar_glass {
                    0
                } else {
                    match s.menubar_style {
                        nacre_core::settings::BarStyle::Light => 1,
                        nacre_core::settings::BarStyle::Dark => 2,
                    }
                };
                vec![(
                    None,
                    vec![
                        RowDef::Toggle(prefs::tr("Aktifkan Rail"), s.menubar, Action::MenuBarOn),
                        RowDef::Seg(
                            prefs::tr("Perilaku"),
                            vec![
                                (prefs::tr("Tetap"), Action::Rail(Behavior::Fixed)),
                                (prefs::tr("Menimpa"), Action::Rail(Behavior::Overlay)),
                                (prefs::tr("Pintar"), Action::Rail(Behavior::Smart)),
                            ],
                            match s.rail {
                                Behavior::Fixed => 0,
                                Behavior::Overlay => 1,
                                Behavior::Smart | Behavior::AutoHide => 2,
                            },
                        ),
                        RowDef::Seg(
                            prefs::tr("Latar Rail"),
                            vec![(prefs::tr("Transparan"), Action::BarStyle(0)), (prefs::tr("Terang"), Action::BarStyle(1)), (prefs::tr("Gelap"), Action::BarStyle(2)), (prefs::tr("Material"), Action::BarStyle(3))],
                            cur,
                        ),
                        RowDef::Toggle(prefs::tr("Jam 24 jam"), s.clock_24h, Action::Clock24),
                        RowDef::Seg(
                            prefs::tr("Persentase baterai"),
                            vec![(prefs::tr("Di dalam"), Action::BatteryPct(1)), (prefs::tr("Di luar"), Action::BatteryPct(2)), (prefs::tr("Sembunyi"), Action::BatteryPct(0))],
                            match s.battery_pct {
                                1 => 0,
                                2 => 1,
                                _ => 2,
                            },
                        ),
                    ],
                ), (
                    Some(prefs::tr("Logo Rail")),
                    {
                        let custom = crate::menulogo::path().is_some();
                        let mut rows = vec![RowDef::Pick(
                            prefs::tr("Logo Rail"),
                            prefs::tr(if custom { "Gambar sendiri" } else { "Lingkaran bawaan" }).to_string(),
                            Action::MenuLogoPick,
                        )];
                        if custom {
                            rows.push(RowDef::Pick(prefs::tr("Logo kustom"), prefs::tr("Kembalikan bawaan").to_string(), Action::MenuLogoReset));
                        }
                        rows
                    },
                )]
            }
            4 => {
                use nacre_core::settings::IconStyle;
                let theme_idx = match s.theme {
                    Theme::Auto => 0,
                    Theme::Light => 1,
                    Theme::Dark => 2,
                };
                let mat_idx = |m: Material| Material::ALL.iter().position(|x| *x == m).unwrap_or(0);
                let mat_opts = |dock: bool| vec![(prefs::tr("Blur"), Action::Material(dock, Material::Blur)), (prefs::tr("Crystal"), Action::Material(dock, Material::Glass)), (prefs::tr("Hitam OLED"), Action::Material(dock, Material::Black))];
                let ist = match s.icon_style {
                    IconStyle::Default => 0,
                    IconStyle::Dark => 1,
                    IconStyle::Clear => 2,
                    IconStyle::Tinted => 3,
                };
                let accent_idx = if s.accent == nacre_core::settings::ACCENT_MULTI { 0 } else { s.accent as usize + 1 };
                let mut theme = vec![
                    RowDef::Swatches(prefs::tr("Warna"), accent_idx),
                    RowDef::Thumbs(
                        prefs::tr("Gaya ikon & widget"),
                        vec![
                            (prefs::tr("Bawaan"), Thumb::Icon(IconStyle::Default), Action::IconStyle(0)),
                            (prefs::tr("Gelap"), Thumb::Icon(IconStyle::Dark), Action::IconStyle(1)),
                            (prefs::tr("Bening"), Thumb::Icon(IconStyle::Clear), Action::IconStyle(2)),
                            (prefs::tr("Berwarna"), Thumb::Icon(IconStyle::Tinted), Action::IconStyle(3)),
                        ],
                        ist,
                    ),
                ];
                if s.icon_style != IconStyle::Default {
                    let v = match s.icon_variant {
                        1 => 0,
                        2 => 1,
                        _ => 2,
                    };
                    theme.push(RowDef::Radios(vec![(prefs::tr("Terang"), Action::IconVariant(1)), (prefs::tr("Gelap"), Action::IconVariant(2)), (prefs::tr("Otomatis"), Action::IconVariant(0))], v));
                }
                if s.icon_style == IconStyle::Tinted {
                    theme.push(RowDef::Seg(
                        prefs::tr("Warna tint"),
                        vec![(prefs::tr("Biru"), Action::IconTint(0)), (prefs::tr("Ungu"), Action::IconTint(1)), (prefs::tr("Merah Muda"), Action::IconTint(2)), (prefs::tr("Oranye"), Action::IconTint(3)), (prefs::tr("Hijau"), Action::IconTint(4))],
                        s.icon_tint as usize,
                    ));
                }
                theme.push(RowDef::Toggle(prefs::tr("Paket ikon (folder iconpack)"), s.icon_pack, Action::IconPack));
                let scale_idx = match s.ui_scale {
                    0..=112 => 0,
                    113..=137 => 1,
                    _ => 2,
                };
                let mut glass_rows = vec![
                    RowDef::Seg(prefs::tr("Material"), mat_opts(true), mat_idx(s.dock_material)),
                    RowDef::Seg(prefs::tr("Material Notch"), mat_opts(false), mat_idx(s.notch_material)),
                ];
                if s.dock_material != Material::Black || s.notch_material != Material::Black {
                    glass_rows.push(RowDef::Slider(prefs::tr("Tingkat blur"), s.blur as u16, 0));
                    glass_rows.push(RowDef::Slider(prefs::tr("Transparansi"), s.transparency as u16, 1));
                }
                if s.dock_material == Material::Glass {
                    glass_rows.push(RowDef::Toggle(prefs::tr("Refraksi tepi kaca (Deck)"), s.refraction, Action::Refraction));
                }
                {
                    let c = sys::conditions();
                    if !c.transparency {
                        glass_rows.insert(0, RowDef::Pick(prefs::tr("Efek transparansi Windows mati, Deck tampil solid"), prefs::tr("Nyalakan").to_string(), Action::WinColors));
                    } else if c.battery_saver {
                        glass_rows.insert(0, RowDef::Pick(prefs::tr("Penghemat baterai aktif, Deck tampil solid"), prefs::tr("Buka").to_string(), Action::WinPower));
                    }
                }
                vec![
                    (
                        None,
                        vec![RowDef::Seg(
                            prefs::tr("Preset"),
                            nacre_core::theme::Preset::ALL.iter().map(|p| (prefs::tr(p.title()), Action::Preset(*p))).collect(),
                            nacre_core::theme::Preset::ALL.iter().position(|p| *p == s.preset).unwrap_or(0),
                        )],
                    ),
                    (
                        None,
                        vec![RowDef::Seg(
                            prefs::tr("Skala Deck, Notch, Rail & Quick Panel"),
                            vec![("100%", Action::UiScale(100)), ("125%", Action::UiScale(125)), ("150%", Action::UiScale(150))],
                            scale_idx,
                        )],
                    ),
                    (
                        None,
                        vec![RowDef::Thumbs(
                            prefs::tr("Tampilan"),
                            vec![
                                (prefs::tr("Otomatis"), Thumb::Appear(0), Action::Theme(Theme::Auto)),
                                (prefs::tr("Terang"), Thumb::Appear(1), Action::Theme(Theme::Light)),
                                (prefs::tr("Gelap"), Thumb::Appear(2), Action::Theme(Theme::Dark)),
                            ],
                            theme_idx,
                        )],
                    ),
                    (Some(prefs::tr("Tema")), theme),
                    (Some(prefs::tr("Kaca")), glass_rows),
                    (Some(prefs::tr("Huruf dan gerak")), {
                        use nacre_core::theme::FontChoice;
                        let mut rows = vec![
                            RowDef::Seg(
                                prefs::tr("Font"),
                                FontChoice::ALL.iter().map(|f| (f.title(), Action::Font(*f))).collect(),
                                FontChoice::ALL.iter().position(|f| *f == s.font).unwrap_or(0),
                            ),
                            RowDef::Toggle(prefs::tr("Kurangi gerakan"), s.reduce_motion, Action::ReduceMotion),
                        ];
                        if s.font != prefs::launch_font() {
                            rows.insert(1, RowDef::Pick(prefs::tr("Font baru berlaku setelah NOX dibuka ulang"), prefs::tr("Buka ulang").to_string(), Action::Relaunch));
                        }
                        rows
                    }),
                ]
            }
            5 => {
                let names = ["Kiri atas", "Kanan atas", "Kiri bawah", "Kanan bawah"];
                vec![(None, names.iter().enumerate().map(|(i, nm)| RowDef::Pick(prefs::tr(nm), prefs::tr(hot_label(s.hot[i])).to_string(), Action::HotCycle(i))).collect())]
            }
            7 => vec![(
                None,
                vec![
                    RowDef::Seg(prefs::tr("Material widget"), vec![(prefs::tr("Original"), Action::WidgetStyle(0)), (prefs::tr("Crystal"), Action::WidgetStyle(1))], s.widget_style.min(1) as usize),
                    RowDef::Pick(prefs::tr("Widget desktop"), prefs::tr("Tambah widget").to_string(), Action::WidgetGallery),
                ],
            )],
            8 => {
                use nacre_platform::cred;
                use nacre_platform::gtasks::{status, SyncStatus};
                let st = match status() {
                    SyncStatus::Synced => prefs::tr("Terhubung · Putuskan"),
                    SyncStatus::Syncing => prefs::tr("Sinkronisasi…"),
                    SyncStatus::Connecting => prefs::tr("Menyambung…"),
                    SyncStatus::NeedsClientConfig => prefs::tr("Butuh Client ID"),
                    SyncStatus::Disconnected | SyncStatus::Error => prefs::tr("Sambungkan"),
                };
                let cid = cred::read_secret(cred::TARGET_GTASKS_CLIENT_ID).map(|v| cred::mask_secret(&v)).unwrap_or_default();
                let csec = cred::read_secret(cred::TARGET_GTASKS_CLIENT_SECRET).map(|v| cred::mask_secret(&v)).unwrap_or_default();

                let key = cred::read_secret(cred::TARGET_GEMINI_KEY).map(|v| cred::mask_secret(&v)).unwrap_or_default();
                let custom = s.gemini_custom_model.as_str().to_string();
                let seg_sel = if custom.is_empty() {
                    s.gemini_model.min(3) as usize
                } else {
                    nacre_core::notch::gemini::MODELS
                        .iter()
                        .position(|m| m.eq_ignore_ascii_case(&custom))
                        .unwrap_or(usize::MAX)
                };
                let (live_lbl, live_act) = if self.fetching_models {
                    (prefs::tr("Mengambil…").to_string(), Action::GeminiFetchModels)
                } else if !self.fetched_models.is_empty() {
                    let cur_idx = self
                        .fetched_models
                        .iter()
                        .position(|m| m.eq_ignore_ascii_case(&custom))
                        .unwrap_or(0);
                    (
                        format!("{} ({}/{})", self.fetched_models[cur_idx], cur_idx + 1, self.fetched_models.len()),
                        Action::GeminiCycleFetched,
                    )
                } else if !self.models_status.is_empty() {
                    (self.models_status.clone(), Action::GeminiFetchModels)
                } else {
                    (prefs::tr("Ambil daftar model").to_string(), Action::GeminiFetchModels)
                };

                vec![
                    (
                        Some(prefs::tr("Google (Tasks & Calendar)")),
                        vec![
                            RowDef::Pick(prefs::tr("Akun Google (Tasks & Calendar)"), st.to_string(), Action::GoogleTasksToggle),
                            RowDef::Text(prefs::tr("Google Client ID"), cid, prefs::tr("Tempel ID…").to_string(), 3),
                            RowDef::Text(prefs::tr("Google Client Secret"), csec, prefs::tr("Tempel rahasia…").to_string(), 4),
                        ],
                    ),
                    (
                        Some(prefs::tr("Ask Gemini")),
                        vec![
                            RowDef::Text(prefs::tr("Gemini API Key"), key, prefs::tr("Tempel kunci…").to_string(), 2),
                            RowDef::Seg(
                                prefs::tr("Model Gemini"),
                                vec![
                                    ("2.5 Flash", Action::GeminiModel(0)),
                                    ("2.5 Pro", Action::GeminiModel(1)),
                                    ("2.5 Lite", Action::GeminiModel(2)),
                                    ("2.0 Flash", Action::GeminiModel(3)),
                                ],
                                seg_sel,
                            ),
                            RowDef::Text(
                                prefs::tr("Model kustom"),
                                custom,
                                prefs::tr("Kosong = pakai preset…").to_string(),
                                5,
                            ),
                            RowDef::Pick(prefs::tr("Model live API"), live_lbl, live_act),
                        ],
                    ),
                ]
            }
            _ => vec![(None, vec![RowDef::Toggle(prefs::tr("Riwayat Papan Klip di Quick Search"), s.clipboard_history, Action::ClipHistory)])],
        }
    }

    fn render(&mut self) {
        let content = self.popup.content.clone();
        if let Ok(k) = content.Children() {
            let _ = k.RemoveAll();
        }
        self.regions.clear();
        let d = self.dpi;
        let look = prefs::look(Role::Themed);
        let c = &content;

        for (i, rgb) in [[255u8, 95, 87], [254, 188, 46], [40, 200, 64]].iter().enumerate() {
            let (cx, cy) = ((26.0 + i as f32 * 20.0) * d, 27.0 * d);
            self.rect(c, cx - 6.0 * d, cy - 6.0 * d, 12.0 * d, 12.0 * d, 6.0 * d, col(*rgb, 255));
        }
        self.region_fixed(18.0 * d, 17.0 * d, 20.0 * d, 20.0 * d, Action::Close);
        let (bx, by, bs) = close_button(d);
        self.rect(c, bx, by, bs, bs, bs / 2.0, col(look.fg, 22));
        self.glyph_icon(c, "\u{E711}", bx + bs / 2.0, by + bs / 2.0, 11.0, look.fg);
        self.region_fixed(bx, by, bs, bs, Action::Close);

        let (fx, fy, fw, fh) = (16.0 * d, 48.0 * d, (SIDEBAR_W - 32.0) * d, 30.0 * d);
        self.rect(c, fx, fy, fw, fh, 9.0 * d, col(look.fg, if self.search_focus { 36 } else { 22 }));
        self.glyph_icon(c, "\u{E721}", fx + 15.0 * d, fy + fh / 2.0, 11.0, look.fg);
        let (shown, op) = if self.search.is_empty() && !self.search_focus {
            (prefs::tr("Cari").to_string(), 0.45)
        } else {
            (format!("{}{}", self.search, if self.search_focus { "|" } else { "" }), 0.95)
        };
        self.text(c, &shown, fx + 29.0 * d, fy + (fh - 17.0 * d) / 2.0, 13.0, 400, look.fg, op, 0);
        self.region_fixed(fx, fy, fw, fh, Action::Search);

        let (px0, py0, av) = (20.0 * d, 90.0 * d, 38.0 * d);
        self.rect(c, px0, py0, av, av, av / 2.0, crate::prefs::accent_c(255));
        let user = user_name();
        let initial: String = user.chars().take(1).collect::<String>().to_uppercase();
        self.text(c, &initial, px0 + av / 2.0, py0 + (av - 20.0 * d) / 2.0, 15.0, 600, [255, 255, 255], 1.0, 1);
        self.text(c, &user, px0 + av + 10.0 * d, py0 + 2.0 * d, 13.5, 600, look.fg, 0.95, 0);
        self.text(c, prefs::tr("Akun Windows"), px0 + av + 10.0 * d, py0 + 20.0 * d, 11.0, 400, look.fg, 0.6, 0);

        let pages: [(&'static str, &'static str, [u8; 3], u8); 9] = [
            (prefs::tr("Umum"), "\u{E713}", [142, 142, 147], 0),
            (prefs::tr("Deck"), "\u{E7C4}", [10, 132, 255], 1),
            (prefs::tr("Notch"), "\u{E7F4}", [58, 58, 60], 2),
            (prefs::tr("Rail"), "\u{E700}", [90, 200, 250], 3),
            (prefs::tr("Tampilan"), "\u{E790}", [88, 86, 214], 4),
            (prefs::tr("Screen Corners"), "\u{E73F}", [175, 82, 222], 5),
            (prefs::tr("Quick Search"), "\u{E721}", [255, 149, 10], 6),
            (prefs::tr("Widget"), "\u{E737}", [0, 122, 255], 7),
            (prefs::tr("Integrasi"), "\u{E71B}", [52, 199, 89], 8),
        ];
        let mut sy = 140.0 * d;
        self.side.clear();
        let (view_top, view_bot) = (134.0 * d, (H - 42.0) * d);
        let side_body = match (self.comp.CreateContainerVisual(), self.comp.CreateContainerVisual()) {
            (Ok(v), Ok(b)) => {
                let _ = v.SetOffset(Vector3 { X: 0.0, Y: view_top, Z: 0.0 });
                let _ = v.SetSize(Vector2 { X: SIDEBAR_W * d, Y: view_bot - view_top });
                if let Ok(clip) = self.comp.CreateInsetClip() {
                    let _ = v.SetClip(&clip);
                }
                let _ = b.SetOffset(Vector3 { X: 0.0, Y: -view_top - self.side_scroll, Z: 0.0 });
                let _ = c.Children().and_then(|k| k.InsertAtTop(&v));
                let _ = v.Children().and_then(|k| k.InsertAtTop(&b));
                b
            }
            _ => c.clone(),
        };
        let parent = &side_body;
        for (name, glyph, color, idx) in pages {
            if !self.page_matches(idx, name) {
                continue;
            }
            let sel = idx == self.tab;
            let (rx, rw, rh) = (16.0 * d, (SIDEBAR_W - 32.0) * d, 36.0 * d);
            let Ok(ic) = self.comp.CreateContainerVisual() else { continue };
            let _ = parent.Children().and_then(|k| k.InsertAtTop(&ic));
            if self.side_hover == Some(idx) {
                let _ = ic.SetCenterPoint(Vector3 { X: rx + rw / 2.0, Y: sy + rh / 2.0, Z: 0.0 });
                let _ = ic.SetScale(Vector3 { X: crate::motion::HOVER_ROW, Y: crate::motion::HOVER_ROW, Z: 1.0 });
            }
            self.side.push((idx, (rx, sy, rw, rh), ic.clone()));
            let c = &ic;
            if sel {
                self.rect(c, rx, sy, rw, rh, 10.0 * d, crate::prefs::accent_c(235));
            }
            self.rect(c, rx + 8.0 * d, sy + 6.0 * d, 24.0 * d, 24.0 * d, 7.0 * d, col(color, 255));
            self.glyph_icon(c, glyph, rx + 20.0 * d, sy + 18.0 * d, 13.0, [255, 255, 255]);
            self.text(c, name, rx + 42.0 * d, sy + (rh - 17.0 * d) / 2.0, 13.5, if sel { 600 } else { 400 }, if sel { [255, 255, 255] } else { look.fg }, 1.0, 0);
            let vis_top = (sy - self.side_scroll).max(view_top);
            let vis_bot = (sy - self.side_scroll + rh).min(view_bot);
            if vis_bot > vis_top {
                self.region_fixed(rx, vis_top, rw, vis_bot - vis_top, Action::Tab(idx));
            }
            sy += 40.0 * d;
        }
        let side_max = (sy - 4.0 * d - view_bot).max(0.0);
        self.side_view = (view_top, side_max);
        if self.side_scroll > side_max {
            self.side_scroll = side_max;
        }
        self.side_bar = None;
        if side_max > 0.5 {
            let (track_top, track_h) = (view_top + 4.0 * d, view_bot - view_top - 8.0 * d);
            let content = view_bot - view_top + side_max;
            let th = (track_h * (view_bot - view_top) / content).max(28.0 * d);
            let ty = track_top + (track_h - th) * (self.side_scroll / side_max);
            let bw = if self.side_drag.is_some() { 7.0 * d } else { 5.0 * d };
            let bx = (SIDEBAR_W - SIDE_INSET - 5.0) * d - bw;
            self.rect(c, bx, ty, bw, th, bw / 2.0, col(look.fg, if self.side_drag.is_some() { 110 } else { 70 }));
            self.side_bar = Some(((bx - 6.0 * d, track_top, bw + 12.0 * d, track_h), th));
        }
        let (kx, kw) = (20.0 * d, (SIDEBAR_W - 40.0) * d);
        let ky = (H - 28.0) * d;
        self.text(c, prefs::tr("Dibuat oleh opankcs.com"), kx + kw / 2.0, ky, 11.0, 500, look.fg, 0.50, 1);

        let (bx, by, bw, bh) = (CONTENT_X * d, 11.0 * d, 64.0 * d, 28.0 * d);
        self.rect(c, bx, by, bw, bh, 14.0 * d, col(look.fg, 24));
        let (can_back, can_fwd) = (self.hpos > 0, self.hpos + 1 < self.history.len());
        let dim = |on: bool| if on { look.fg } else if look.dark { [110, 110, 116] } else { [170, 170, 176] };
        self.glyph_icon(c, "\u{E76B}", bx + bw * 0.27, by + bh / 2.0, 11.0, dim(can_back));
        self.glyph_icon(c, "\u{E76C}", bx + bw * 0.73, by + bh / 2.0, 11.0, dim(can_fwd));
        self.rect(c, bx + bw / 2.0, by + 7.0 * d, 1.0, bh - 14.0 * d, 0.0, col(look.fg, 34));
        self.region_fixed(bx, by, bw / 2.0, bh, Action::Back);
        self.region_fixed(bx + bw / 2.0, by, bw / 2.0, bh, Action::Forward);
        let title = pages.iter().find(|p| p.3 == self.tab).map(|p| p.0).unwrap_or("");
        self.text(c, title, bx + bw + 12.0 * d, by + (bh - 19.0 * d) / 2.0, 15.0, 700, look.fg, 1.0, 0);

        let view = match (self.comp.CreateContainerVisual(), self.comp.CreateContainerVisual()) {
            (Ok(v), Ok(b)) => Some((v, b)),
            _ => None,
        };
        let Some((viewport, body)) = view else { return };
        let (vt, vh) = (VIEW_TOP * d, (H - VIEW_TOP - 10.0) * d);
        let _ = viewport.SetOffset(Vector3 { X: 0.0, Y: vt, Z: 0.0 });
        let _ = viewport.SetSize(Vector2 { X: W * d, Y: vh });
        if let Ok(clip) = self.comp.CreateInsetClip() {
            let _ = clip.SetLeftInset(0.0);
            let _ = clip.SetRightInset(0.0);
            let _ = clip.SetTopInset(0.0);
            let _ = clip.SetBottomInset(0.0);
            let _ = viewport.SetClip(&clip);
        }
        let _ = body.SetOffset(Vector3 { X: 0.0, Y: -self.scroll, Z: 0.0 });
        let _ = c.Children().and_then(|k| k.InsertAtTop(&viewport));
        let _ = viewport.Children().and_then(|k| k.InsertAtTop(&body));

        let mut y = VIEW_TOP * d - vt;
        y += 4.0 * d;
        let groups = self.page(self.tab);
        for (header, rows) in groups {
            if let Some(h) = header {
                self.text_abs(&body, h, CONTENT_X * d + 8.0 * d, y + 2.0 * d, 12.0, 600, look.fg, 0.55);
                y += 24.0 * d;
            }
            let used = self.card_in(&body, y, &rows, &look, vt);
            y += used + 20.0 * d;
        }
        self.content_h = y + 6.0 * d;
        let max_scroll = (self.content_h - vh).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        self.sp_scroll.pos = self.sp_scroll.pos.clamp(0.0, max_scroll);
        self.sp_scroll.target = self.sp_scroll.target.clamp(0.0, max_scroll);
        self.scroll_body = Some(body.clone());
        self.content_sb = None;
        if max_scroll > 0.5 {
            let track_top = vt + 4.0 * d;
            let track_h = (vh - 8.0 * d).max(1.0);
            let th = (track_h * vh / self.content_h.max(vh)).max(32.0 * d).min(track_h);
            let right_edge = (W - 7.0) * d;
            let make_sb = || -> windows_core::Result<ContentScrollBar> {
                let thumb = self.comp.CreateSpriteVisual()?;
                let brush = self.comp.CreateColorBrushWithColor(col(look.fg, 52))?;
                thumb.SetBrush(&brush)?;
                let clip_geo = self.comp.CreateRoundedRectangleGeometry()?;
                let clip = self.comp.CreateGeometricClip()?;
                clip.SetGeometry(&clip_geo)?;
                thumb.SetClip(&clip)?;
                c.Children()?.InsertAtTop(&thumb)?;
                Ok(ContentScrollBar {
                    track: ((W - 22.0) * d, track_top, 20.0 * d, track_h),
                    thumb_h: th,
                    max_scroll,
                    right_edge,
                    thumb,
                    clip_geo,
                    brush,
                    fg: look.fg,
                })
            };
            if let Ok(sb) = make_sb() {
                self.content_sb = Some(sb);
            }
        }
        self.sync_scroll_visuals();
        self.size = (W * d, H * d);
        let (x, yy, ww, hh) = self.popup.rect;
        let (nw, nh) = ((W * d) as i32, (H * d) as i32);
        if (nw, nh) != (ww, hh) {
            let _ = self.popup.set_rect(x, yy, nw, nh);
        }
        self.frame();
    }

    fn frame(&mut self) {
        let d = self.dpi;
        let (w, h) = ((W * d) as i32, (H * d) as i32);
        let (hx, hy, hw, hh) = (SIDE_INSET * d, SIDE_INSET * d, (SIDEBAR_W - 2.0 * SIDE_INSET) * d, (H - 2.0 * SIDE_INSET) * d);
        let _ = self.popup.panel.set_rect(hx, hy, hw, hh);
        let _ = self.popup.panel.set_radius(SIDE_RADIUS * d);
        let dark = prefs::look(Role::Themed).dark;
        if self.frame.is_some() && self.frame_key == (w, h, dark) {
            return;
        }
        let data = frame_pixels(w, h, 24.0 * d, (hx, hy, hw, hh), SIDE_RADIUS * d, dark);
        let go = || -> windows_core::Result<windows::UI::Composition::SpriteVisual> {
            let surf = self.gfx.surface_from_bgra(w as u32, h as u32, &data)?;
            let br = self.comp.CreateSurfaceBrushWithSurface(&surf)?;
            br.SetStretch(CompositionStretch::Fill)?;
            let v = match &self.frame {
                Some(v) => v.clone(),
                None => {
                    let v = self.comp.CreateSpriteVisual()?;
                    self.popup.root().Children()?.InsertAtBottom(&v)?;
                    v
                }
            };
            v.SetBrush(&br)?;
            v.SetSize(Vector2 { X: w as f32, Y: h as f32 })?;
            Ok(v)
        };
        match go() {
            Ok(v) => {
                self.frame = Some(v);
                self.frame_key = (w, h, dark);
            }
            Err(e) => log::line("settings", &format!("bingkai gagal: {e}")),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn text_abs(&mut self, parent: &ContainerVisual, s: &str, x: f32, y: f32, size: f32, weight: i32, rgb: [u8; 3], alpha: f32) {
        self.text(parent, s, x, y, size, weight, rgb, alpha, 0);
    }

    fn card_in(&mut self, body: &ContainerVisual, y: f32, rows: &[RowDef], look: &nacre_core::settings::Look, vt: f32) -> f32 {
        let before = self.regions.len();
        let used = self.card(body, y, rows, look);
        for r in self.regions[before..].iter_mut() {
            r.y += vt;
        }
        used
    }

    fn hit(&self, x: f32, y: f32) -> Option<Action> {
        let d = self.dpi;
        let (vt, vb) = (VIEW_TOP * d, (H - 10.0) * d);
        self.regions
            .iter()
            .rev()
            .find(|r| {
                let yy = if r.scroll {
                    if y < vt || y > vb {
                        return false;
                    }
                    y + self.scroll
                } else {
                    y
                };
                x >= r.x && x < r.x + r.w && yy >= r.y && yy < r.y + r.h
            })
            .map(|r| r.a)
    }

    fn slide(&self, id: u8, x: f32, sx: f32, sw: f32, save: bool) {
        let (min, max, _) = Self::spec(id);
        let v = (min + slider::value_at(x, sx, sw, false) * (max - min)).round() as u16;
        prefs::update_with(save, |s| {
            match id {
                0 => s.blur = v as u8,
                1 => s.transparency = v as u8,
                2 => s.dock_round = v as u8,
                3 => s.notch_round = v as u8,
                4 => s.dock_size = v as u8,
                5 => s.dock_gap = v as u8,
                6 => s.dock_zoom = v as u8,
                7 => s.dock_margin = v as u8,
                _ => s.notch_autohide_delay = v.clamp(50, 300),
            }
        });
    }
}

fn hot_label(a: nacre_core::settings::HotAction) -> &'static str {
    use nacre_core::settings::HotAction::*;
    match a {
        None => "Tidak ada",
        Mission => "Overview",
        Launchpad => "Apps",
        Desktop => "Tampilkan Desktop",
        Notifications => "Pusat Notifikasi",
        Lock => "Kunci Layar",
    }
}

fn xy(lp: LPARAM) -> (f32, f32) {
    ((lp.0 as i16) as f32, ((lp.0 >> 16) as i16) as f32)
}

fn with_win<R>(f: impl FnOnce(&mut Win) -> R) -> Option<R> {
    WIN.with(|s| s.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(|w| f(w))))
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => {
            let mut r = windows::Win32::Foundation::RECT::default();
            let _ = GetWindowRect(hwnd, &mut r);
            let (px, py) = ((lp.0 as i16) as i32, ((lp.0 >> 16) as i16) as i32);
            let drag = with_win(|w| {
                let d = w.dpi;
                let (bx, by, bs) = close_button(d);
                let (lx, ly) = ((px - r.left) as f32, (py - r.top) as f32);
                if lx >= bx && lx < bx + bs && ly >= by && ly < by + bs {
                    return false;
                }
                py - r.top < (VIEW_TOP * d) as i32 && px - r.left > ((CONTENT_X + 70.0) * d) as i32
            })
            .unwrap_or(false);
            LRESULT(if drag { HTCAPTION } else { HTCLIENT } as isize)
        }
        WM_TIMER => {
            if wp.0 == T_ANIM {
                with_win(|w| w.tick_anim());
            } else if wp.0 == T_SB_IDLE {
                let _ = KillTimer(Some(hwnd), T_SB_IDLE);
                with_win(|w| {
                    if w.content_drag.is_none() && !w.content_sb_hover {
                        w.sp_sb_active.target = 0.0;
                        w.start_anim();
                    }
                });
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) as i16) as f32;
            with_win(|w| {
                let d = w.dpi;
                let vh = (H - VIEW_TOP - 10.0) * d;
                let mut p = windows::Win32::Foundation::POINT::default();
                let _ = GetCursorPos(&mut p);
                if ((p.x - w.popup.rect.0) as f32) < SIDEBAR_W * d {
                    let max = w.side_view.1;
                    w.side_scroll = (w.side_scroll - delta / 120.0 * 40.0 * d).clamp(0.0, max);
                    w.render();
                    return;
                }
                let max = (w.content_h - vh).max(0.0);
                if max > 0.5 {
                    w.sp_scroll.target = (w.sp_scroll.target - delta / 120.0 * 64.0 * d).clamp(0.0, max);
                    w.wake_sb();
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = xy(lp);
            if GetForegroundWindow() != hwnd {
                let _ = SetForegroundWindow(hwnd);
            }
            let grabbed_content = with_win(|w| {
                let Some(sb) = &w.content_sb else { return false };
                let r = sb.track;
                if !(x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3) {
                    return false;
                }
                let (th, max) = (sb.thumb_h, sb.max_scroll);
                let ty = r.1 + (r.3 - th) * (w.scroll / max.max(1.0));
                let grab = if y >= ty && y < ty + th { y - ty } else { th / 2.0 };
                let s = (((y - grab - r.1) / (r.3 - th).max(1.0)) * max).clamp(0.0, max);
                w.content_drag = Some(grab);
                w.content_sb_hover = true;
                w.sp_sb_hover.target = 1.0;
                w.scroll = s;
                w.sp_scroll.snap(s);
                w.sync_scroll_visuals();
                w.wake_sb();
                true
            })
            .unwrap_or(false);
            if grabbed_content {
                SetCapture(hwnd);
                return LRESULT(0);
            }
            let grabbed = with_win(|w| {
                let Some((r, th)) = w.side_bar else { return false };
                if !(x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3) {
                    return false;
                }
                let max = w.side_view.1;
                let ty = r.1 + (r.3 - th) * (w.side_scroll / max.max(0.001));
                let grab = if y >= ty && y < ty + th { y - ty } else { th / 2.0 };
                w.side_drag = Some(grab);
                w.side_scroll = (((y - grab - r.1) / (r.3 - th).max(1.0)) * max).clamp(0.0, max);
                w.render();
                true
            })
            .unwrap_or(false);
            if grabbed {
                SetCapture(hwnd);
                return LRESULT(0);
            }
            let started = with_win(|w| match w.hit(x, y) {
                Some(Action::Slider(id, sx, sw)) => {
                    w.dragging = Some((id, sx, sw));
                    w.slide(id, x, sx, sw, false);
                    w.render();
                    true
                }
                Some(_) => {
                    w.press = Some((x, y));
                    w.render();
                    false
                }
                None => false,
            })
            .unwrap_or(false);
            if started {
                let _ = SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = xy(lp);
            with_win(|w| {
                if let Some(grab) = w.content_drag {
                    if let Some((r, th, max)) = w.content_sb.as_ref().map(|sb| (sb.track, sb.thumb_h, sb.max_scroll)) {
                        let s = (((y - grab - r.1) / (r.3 - th).max(1.0)) * max).clamp(0.0, max);
                        w.scroll = s;
                        w.sp_scroll.snap(s);
                        w.sync_scroll_visuals();
                    }
                    return;
                }
                if let (Some(grab), Some((r, th))) = (w.side_drag, w.side_bar) {
                    let max = w.side_view.1;
                    w.side_scroll = (((y - grab - r.1) / (r.3 - th).max(1.0)) * max).clamp(0.0, max);
                    w.render();
                    return;
                }
                if let Some((id, sx, sw)) = w.dragging {
                    w.slide(id, x, sx, sw, false);
                    return;
                }
                let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut t);
                let sb_hit = w.content_sb.as_ref().is_some_and(|sb| {
                    let r = sb.track;
                    x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
                });
                if sb_hit != w.content_sb_hover {
                    w.content_sb_hover = sb_hit;
                    w.sp_sb_hover.target = if sb_hit { 1.0 } else { 0.0 };
                    if sb_hit {
                        w.wake_sb();
                    } else {
                        w.start_anim();
                    }
                }
                let (top, s) = (w.side_view.0, w.side_scroll);
                let hit = if y < top || y > (H - 42.0) * w.dpi { None } else { w.side.iter().find(|(_, r, _)| x >= r.0 && x < r.0 + r.2 && y + s >= r.1 && y + s < r.1 + r.3).map(|e| e.0) };
                w.hover_side(hit);
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_win(|w| {
                w.hover_side(None);
                if w.content_sb_hover {
                    w.content_sb_hover = false;
                    w.sp_sb_hover.target = 0.0;
                    w.start_anim();
                }
            });
            LRESULT(0)
        }
        WM_CHAR => {
            let ch = wp.0 as u32;
            with_win(|w| {
                if w.edit.is_some() {
                    if ch >= 32 && ch != 127 && w.edit_buf.chars().count() < 256 {
                        if let Some(c) = char::from_u32(ch) {
                            w.edit_buf.push(c);
                            w.render();
                        }
                    }
                    return;
                }
                if w.search_focus && ch >= 32 && ch != 127 {
                    if let Some(c) = char::from_u32(ch) {
                        w.search.push(c);
                        w.render();
                    }
                }
            });
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wp.0 as u16;
            let ctrl = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
            with_win(|w| {
                if w.edit.is_some() {
                    if ctrl && vk == b'V' as u16 {
                        if let Some(pasted) = nacre_platform::clipboard::paste_text() {
                            let clean: String = pasted.chars().filter(|c| !c.is_control()).collect();
                            let avail = 256usize.saturating_sub(w.edit_buf.chars().count());
                            if avail > 0 && !clean.is_empty() {
                                w.edit_buf.extend(clean.chars().take(avail));
                                w.render();
                            }
                        }
                        return;
                    }
                    if ctrl && vk == 0x08 {
                        w.edit_buf.clear();
                        w.render();
                        return;
                    }
                    match vk {
                        0x08 => {
                            w.edit_buf.pop();
                            w.render();
                        }
                        0x1B => {
                            w.edit = None;
                            w.render();
                        }
                        0x0D => w.commit_edit(),
                        _ => {}
                    }
                    return;
                }
                if !w.search_focus {
                    if vk == 0x1B {
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE_SELF, WPARAM(0), LPARAM(0));
                    }
                    return;
                }
                if ctrl && vk == b'V' as u16 {
                    if let Some(pasted) = nacre_platform::clipboard::paste_text() {
                        let clean: String = pasted.chars().filter(|c| !c.is_control()).take(80).collect();
                        if !clean.is_empty() {
                            w.search.push_str(&clean);
                            w.render();
                        }
                    }
                    return;
                }
                match vk {
                    0x08 => {
                        w.search.pop();
                    }
                    0x1B => {
                        w.search.clear();
                        w.search_focus = false;
                    }
                    0x0D => {
                        if let Some(t) = (0u8..9).find(|i| w.page_matches(*i, "")) {
                            w.go(t, true);
                            return;
                        }
                    }
                    _ => return,
                }
                w.render();
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = xy(lp);
            let _ = ReleaseCapture();
            if with_win(|w| {
                if w.content_drag.take().is_some() {
                    let still_over = w.content_sb.as_ref().is_some_and(|sb| {
                        let r = sb.track;
                        x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3
                    });
                    w.content_sb_hover = still_over;
                    w.sp_sb_hover.target = if still_over { 1.0 } else { 0.0 };
                    w.wake_sb();
                    true
                } else {
                    false
                }
            })
            .unwrap_or(false)
            {
                return LRESULT(0);
            }
            if with_win(|w| w.side_drag.take().is_some()).unwrap_or(false) {
                with_win(|w| w.render());
                return LRESULT(0);
            }
            let act = with_win(|w| {
                let was_pressed = w.press.take().is_some();
                if let Some((id, sx, sw)) = w.dragging.take() {
                    w.slide(id, x, sx, sw, true);
                    w.render();
                    return None;
                }
                if was_pressed {
                    w.render();
                }
                w.hit(x, y)
            })
            .flatten();
            if !matches!(act, Some(Action::Search)) {
                with_win(|w| {
                    if w.search_focus {
                        w.search_focus = false;
                        w.render();
                    }
                });
            }
            if !matches!(act, Some(Action::EditText(_))) {
                with_win(|w| {
                    if w.edit.is_some() {
                        w.commit_edit();
                    }
                });
            }
            match act {
                Some(Action::EditText(f)) => {
                    with_win(|w| {
                        if w.edit != Some(f) {
                            if w.edit.is_some() {
                                w.commit_edit();
                            }
                            let s = prefs::get();
                            w.edit_buf = match f {
                                0 => s.greet_name.as_str().to_string(),
                                1 => s.greet_text.as_str().to_string(),
                                2 => nacre_platform::cred::read_secret(nacre_platform::cred::TARGET_GEMINI_KEY).unwrap_or_default(),
                                3 => nacre_platform::cred::read_secret(nacre_platform::cred::TARGET_GTASKS_CLIENT_ID).unwrap_or_default(),
                                4 => nacre_platform::cred::read_secret(nacre_platform::cred::TARGET_GTASKS_CLIENT_SECRET).unwrap_or_default(),
                                5 => s.gemini_custom_model.as_str().to_string(),
                                6 => s.prayer_city.as_str().to_string(),
                                _ => String::new(),
                            };
                            w.edit = Some(f);
                            w.render();
                        }
                    });
                    let _ = SetForegroundWindow(hwnd);
                    let _ = SetFocus(Some(hwnd));
                }
                Some(Action::NotchIdle(v)) => prefs::update(|s| s.notch_idle = v),
                Some(Action::LyricsCompact) => prefs::update(|s| s.lyrics_compact = !s.lyrics_compact),
                Some(Action::NotchDownloads) => prefs::update(|s| s.notch_downloads = !s.notch_downloads),
                Some(Action::NotchSend) => prefs::update(|s| s.notch_send = !s.notch_send),
                Some(Action::NotchNotifPeek) => prefs::update(|s| s.notch_notif_peek = !s.notch_notif_peek),
                Some(Action::NotchNotifs) => prefs::update(|s| s.notch_notifs = !s.notch_notifs),
                Some(Action::NotchTabMove(tab, delta)) => prefs::update(|s| s.move_notch_tab(tab, delta as i32)),
                Some(Action::NotchTabToggle(tab)) => prefs::update(|s| s.toggle_notch_tab(tab)),
                Some(Action::NotchTabReset) => prefs::update(|s| s.reset_notch_tabs()),
                Some(Action::PrayerMethod(m)) => prefs::update(|s| s.prayer_method = m.min(4)),
                Some(Action::PrayerAlert10m) => prefs::update(|s| s.prayer_alert_10m = !s.prayer_alert_10m),
                Some(Action::PrayerAlert5m) => prefs::update(|s| s.prayer_alert_5m = !s.prayer_alert_5m),
                Some(Action::PrayerAlertNow) => prefs::update(|s| s.prayer_alert_now = !s.prayer_alert_now),
                Some(Action::PrayerAlertSound) => prefs::update(|s| s.prayer_alert_sound = !s.prayer_alert_sound),
                Some(Action::PrayerTestAlert) => crate::notch::trigger_prayer_test_alert(),
                Some(Action::PrayerAutoLoc) => {
                    let mut turned_on = false;
                    prefs::update(|s| {
                        s.prayer_auto_loc = !s.prayer_auto_loc;
                        turned_on = s.prayer_auto_loc;
                    });
                    if turned_on {
                        let already = with_win(|w| {
                            if w.prayer_loc_busy {
                                return true;
                            }
                            w.prayer_loc_busy = true;
                            w.prayer_loc_status = prefs::tr("Mencari…").to_string();
                            w.render();
                            false
                        })
                        .unwrap_or(true);
                        if !already {
                            let hw = hwnd.0 as isize;
                            std::thread::spawn(move || {
                                let res = nacre_platform::prayer_loc::detect_location(true);
                                if let Ok(mut g) = PRAYER_LOC_RESULT.lock() {
                                    *g = Some(res);
                                }
                                unsafe {
                                    let _ = PostMessageW(
                                        Some(HWND(hw as *mut core::ffi::c_void)),
                                        WM_PRAYER_LOC,
                                        WPARAM(0),
                                        LPARAM(0),
                                    );
                                }
                            });
                        }
                    }
                }
                Some(Action::PrayerDetectLoc) => {
                    let use_win = prefs::get().prayer_auto_loc;
                    let already = with_win(|w| {
                        if w.prayer_loc_busy {
                            return true;
                        }
                        w.prayer_loc_busy = true;
                        w.prayer_loc_status = prefs::tr("Mencari…").to_string();
                        w.render();
                        false
                    })
                    .unwrap_or(true);
                    if !already {
                        let hw = hwnd.0 as isize;
                        std::thread::spawn(move || {
                            let res = nacre_platform::prayer_loc::detect_location(use_win);
                            if let Ok(mut g) = PRAYER_LOC_RESULT.lock() {
                                *g = Some(res);
                            }
                            unsafe {
                                let _ = PostMessageW(
                                    Some(HWND(hw as *mut core::ffi::c_void)),
                                    WM_PRAYER_LOC,
                                    WPARAM(0),
                                    LPARAM(0),
                                );
                            }
                        });
                    }
                }
                Some(Action::GeminiModel(m)) => prefs::update(|s| {
                    s.gemini_model = m;
                    s.gemini_custom_model = nacre_core::settings::ShortText::EMPTY;
                }),
                Some(Action::GeminiFetchModels) => {
                    let already = with_win(|w| {
                        if w.fetching_models {
                            return true;
                        }
                        w.fetching_models = true;
                        w.models_status = prefs::tr("Mengambil…").to_string();
                        w.render();
                        false
                    })
                    .unwrap_or(true);
                    if !already {
                        let hw = hwnd.0 as isize;
                        std::thread::spawn(move || {
                            let res = nacre_platform::gemini::fetch_models();
                            if let Ok(mut g) = FETCHED_GEMINI_RESULT.lock() {
                                *g = Some(res);
                            }
                            unsafe {
                                let _ = PostMessageW(
                                    Some(HWND(hw as *mut core::ffi::c_void)),
                                    WM_GEMINI_MODELS,
                                    WPARAM(0),
                                    LPARAM(0),
                                );
                            }
                        });
                    }
                }
                Some(Action::GeminiCycleFetched) => {
                    let next = with_win(|w| {
                        if w.fetched_models.is_empty() {
                            return None;
                        }
                        let cur = prefs::get().gemini_custom_model;
                        let cur_s = cur.as_str();
                        let next_i = w
                            .fetched_models
                            .iter()
                            .position(|m| m.eq_ignore_ascii_case(cur_s))
                            .map(|i| (i + 1) % w.fetched_models.len())
                            .unwrap_or(0);
                        Some(w.fetched_models[next_i].clone())
                    })
                    .flatten();
                    if let Some(m) = next {
                        let st = nacre_core::settings::ShortText::new(&m);
                        prefs::update(|s| s.gemini_custom_model = st);
                    }
                }
                Some(Action::GoogleTasksToggle) => {
                    use nacre_platform::gtasks::{status, SyncStatus};
                    match status() {
                        SyncStatus::Synced => crate::notch::trigger_gtasks_disconnect(),
                        _ => crate::notch::trigger_gtasks_connect(),
                    }
                    with_win(|w| w.render());
                }
                Some(Action::NotifSetup) => {
                    use nacre_platform::notifications::{status, Status};
                    match status() {
                        Status::NeedsIdentity => {
                            let dir = std::env::current_exe()
                                .ok()
                                .and_then(|p| p.parent().and_then(|d| d.parent()).and_then(|d| d.parent()).map(|r| r.join("packaging")))
                                .filter(|p| p.exists());
                            if let Some(p) = dir {
                                nacre_platform::launch::open(&p.to_string_lossy());
                            } else {
                                nacre_platform::launch::open("ms-settings:privacy-notifications");
                            }
                        }
                        _ => nacre_platform::launch::open("ms-settings:privacy-notifications"),
                    }
                }
                Some(Action::LyricsOnline) => prefs::update(|s| s.lyrics_online = !s.lyrics_online),
                Some(Action::LyricsLead(ms)) => prefs::update(|s| s.lyrics_lead_ms = ms.min(2000)),
                Some(Action::MenuLogoPick) => {
                    let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                    let picked = nacre_platform::pick::open_file(prefs::tr("Pilih gambar"), &[(prefs::tr("Gambar logo"), "*.png;*.jpg;*.jpeg;*.bmp;*.ico;*.gif;*.webp;*.tif;*.tiff")]);
                    let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                    if let Some(p) = picked {
                        if crate::menulogo::set(&p) {
                            prefs::broadcast();
                        }
                    }
                }
                Some(Action::UiScale(v)) => prefs::update(|s| s.ui_scale = v),
                Some(Action::WidgetStyle(v)) => prefs::update(|s| s.widget_style = v),
                Some(Action::BatteryPct(v)) => prefs::update(|s| s.battery_pct = v),
                Some(Action::WidgetGallery) => crate::widgets::open_gallery(),
                Some(Action::NotchShape(b)) => prefs::update(|s| s.notch_island = b),
                Some(Action::MenuLogoReset) => {
                    crate::menulogo::reset();
                    prefs::broadcast();
                }
                Some(Action::LyricsDir) => {
                    let dir = crate::paths::config_dir().join("lyrics");
                    let _ = std::fs::create_dir_all(&dir);
                    nacre_platform::launch::open(&dir.to_string_lossy());
                }
                Some(Action::Search) => {
                    with_win(|w| {
                        w.search_focus = true;
                        w.render();
                    });
                    let _ = SetForegroundWindow(hwnd);
                    let _ = SetFocus(Some(hwnd));
                }
                Some(Action::Back) => {
                    with_win(|w| {
                        if w.hpos > 0 {
                            w.hpos -= 1;
                            let t = w.history[w.hpos];
                            w.go(t, false);
                        }
                    });
                }
                Some(Action::Forward) => {
                    with_win(|w| {
                        if w.hpos + 1 < w.history.len() {
                            w.hpos += 1;
                            let t = w.history[w.hpos];
                            w.go(t, false);
                        }
                    });
                }
                Some(Action::IconVariant(v)) => prefs::update(|s| s.icon_variant = v),
                Some(Action::MinEffect(v)) => prefs::update(|s| s.min_effect = v),
                Some(Action::Close) => {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE_SELF, WPARAM(0), LPARAM(0));
                }
                Some(Action::WinColors) => nacre_platform::launch::open("ms-settings:personalization-colors"),
                Some(Action::WinPower) => nacre_platform::launch::open("ms-settings:batterysaver"),
                Some(Action::Takeover) => prefs::update(|s| s.takeover = !s.takeover),
                Some(Action::Dock(b)) => prefs::update(|s| s.dock = b),
                Some(Action::Notch(b)) => prefs::update(|s| s.notch = b),
                Some(Action::Rail(b)) => prefs::update(|s| s.rail = b),
                Some(Action::Reveal(part, r)) => prefs::update(|s| match part {
                    0 => s.dock_reveal = r,
                    1 => s.rail_reveal = r,
                    _ => s.notch_reveal = r,
                }),
                Some(Action::Screen(part, v)) => prefs::update(|s| match part {
                    0 => s.deck_screen = v,
                    1 => s.rail_screen = v,
                    2 => s.notch_screen = v,
                    _ => s.widget_screen = v,
                }),
                Some(Action::Preset(p)) => prefs::update(|s| nacre_core::theme::apply(s, p)),
                Some(Action::Visualizer(v)) => prefs::update(|s| s.visualizer = v),
                Some(Action::Font(f)) => prefs::update(|s| s.font = f),
                Some(Action::ReduceMotion) => prefs::update(|s| s.reduce_motion = !s.reduce_motion),
                Some(Action::CapLayout(l)) => prefs::update(|s| s.capsule_layout = l),
                Some(Action::CapDecor(d)) => prefs::update(|s| s.capsule_decor = d),
                Some(Action::CapToggle(i)) => prefs::update(|s| s.capsule.toggle(i as usize)),
                Some(Action::CapMove(i, up)) => prefs::update(|s| s.capsule.shift(i as usize, up)),
                Some(Action::PillDevices) => prefs::update(|s| s.pill_devices = !s.pill_devices),
                Some(Action::Meeting) => prefs::update(|s| s.meeting_companion = !s.meeting_companion),
                Some(Action::Relaunch) => crate::mainwin::relaunch(),
                Some(Action::Theme(t)) => prefs::update(|s| s.theme = t),
                Some(Action::Lang(l)) => prefs::update(|s| s.lang = l),
                Some(Action::WinShadow) => prefs::update(|s| s.win_shadow = !s.win_shadow),
                Some(Action::HoverOpen) => prefs::update(|s| s.notch_hover_open = !s.notch_hover_open),
                Some(Action::MenuBarOn) => prefs::update(|s| s.menubar = !s.menubar),
                Some(Action::HotCycle(i)) => prefs::update(|s| s.hot[i] = s.hot[i].next()),
                Some(Action::MacKeys) => prefs::update(|s| s.mac_shortcuts = !s.mac_shortcuts),
                Some(Action::Accent(i)) => prefs::update(|s| s.accent = i),
                Some(Action::IconStyle(m)) => prefs::update(|s| {
                    s.icon_style = match m {
                        0 => nacre_core::settings::IconStyle::Default,
                        1 => nacre_core::settings::IconStyle::Dark,
                        2 => nacre_core::settings::IconStyle::Clear,
                        _ => nacre_core::settings::IconStyle::Tinted,
                    }
                }),
                Some(Action::IconTint(i)) => prefs::update(|s| s.icon_tint = i),
                Some(Action::ClipHistory) => prefs::update(|s| s.clipboard_history = !s.clipboard_history),
                Some(Action::AboveFull) => prefs::update(|s| s.above_fullscreen = !s.above_fullscreen),
                Some(Action::DockPlates) => prefs::update(|s| s.dock_plates = !s.dock_plates),
                Some(Action::IconPack) => prefs::update(|s| s.icon_pack = !s.icon_pack),
                Some(Action::DownloadsStack) => prefs::update(|s| s.downloads_stack = !s.downloads_stack),
                Some(Action::RecentApps) => prefs::update(|s| s.recent_apps = !s.recent_apps),
                Some(Action::HideIcons) => prefs::update(|s| s.hide_desktop_icons = !s.hide_desktop_icons),
                Some(Action::BarStyle(m)) => prefs::update(|s| match m {
                    0 => {
                        s.menubar_glass = false;
                        s.menubar_material = false;
                    }
                    3 => {
                        s.menubar_glass = false;
                        s.menubar_material = true;
                    }
                    1 => {
                        s.menubar_material = false;
                        s.menubar_glass = true;
                        s.menubar_style = nacre_core::settings::BarStyle::Light;
                    }
                    _ => {
                        s.menubar_material = false;
                        s.menubar_glass = true;
                        s.menubar_style = nacre_core::settings::BarStyle::Dark;
                    }
                }),
                Some(Action::Clock24) => prefs::update(|s| s.clock_24h = !s.clock_24h),
                Some(Action::DockOn) => prefs::update(|s| s.dock_enabled = !s.dock_enabled),
                Some(Action::NotchOn) => prefs::update(|s| s.notch_enabled = !s.notch_enabled),
                Some(Action::Refraction) => prefs::update(|s| s.refraction = !s.refraction),
                Some(Action::Anim) => prefs::update(|s| s.anim = !s.anim),
                Some(Action::Autostart) => prefs::update(|s| s.autostart = !s.autostart),
                Some(Action::Material(true, m)) => prefs::update(|s| s.dock_material = m),
                Some(Action::Material(false, m)) => prefs::update(|s| s.notch_material = m),
                Some(Action::Tab(t)) => {
                    with_win(|w| w.go(t, true));
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_GEMINI_MODELS => {
            let res = FETCHED_GEMINI_RESULT.lock().ok().and_then(|mut g| g.take());
            with_win(|w| {
                w.fetching_models = false;
                match res {
                    Some(Ok(list)) => {
                        w.models_status = format!("{} model ditemukan", list.len());
                        w.fetched_models = list;
                    }
                    Some(Err(e)) => {
                        let short: String = e.chars().take(36).collect();
                        w.models_status = short;
                    }
                    None => {}
                }
                w.render();
            });
            LRESULT(0)
        }
        WM_PRAYER_LOC => {
            let res = PRAYER_LOC_RESULT.lock().ok().and_then(|mut g| g.take());
            match res {
                Some(Ok(geo)) => {
                    let lat_e4 = (geo.lat * 10_000.0).round() as i32;
                    let lon_e4 = (geo.lon * 10_000.0).round() as i32;
                    let city = nacre_core::settings::ShortText::new(&geo.city);
                    let badge = match geo.source {
                        "windows" => format!("{} (GPS)", geo.city),
                        "ip" => format!("{} (IP)", geo.city),
                        _ => geo.city.clone(),
                    };
                    prefs::update(|s| {
                        s.prayer_city = city;
                        s.prayer_lat_e4 = lat_e4;
                        s.prayer_lon_e4 = lon_e4;
                    });
                    with_win(|w| {
                        w.prayer_loc_busy = false;
                        w.prayer_loc_status = badge;
                        w.render();
                    });
                }
                Some(Err(e)) => {
                    let short: String = e.chars().take(32).collect();
                    with_win(|w| {
                        w.prayer_loc_busy = false;
                        w.prayer_loc_status = short;
                        w.render();
                    });
                }
                None => {
                    with_win(|w| {
                        w.prayer_loc_busy = false;
                        w.render();
                    });
                }
            }
            LRESULT(0)
        }
        WM_PREFS => {
            with_win(|w| w.render());
            let (themed, _) = (prefs::look(Role::Themed), ());
            with_win(|w| {
                let base = nacre_ui::glass::GlassParams::dark((10.0, 10.0), 24.0 * w.dpi);
                let _ = w.popup.panel.set_params(side_glass(prefs::apply(base, &themed)));
            });
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE_SELF, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        WM_CLOSE_SELF => {
            let _ = KillTimer(Some(hwnd), T_ANIM);
            let _ = KillTimer(Some(hwnd), T_SB_IDLE);
            prefs::unlisten(hwnd);
            let win = WIN.with(|s| s.borrow_mut().take());
            drop(win);
            let _ = DestroyWindow(hwnd);
            nacre_platform::sys::trim_later(5000);
            log::line("settings", "ditutup");
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
