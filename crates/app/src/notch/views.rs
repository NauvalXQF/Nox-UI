use super::draw::{white, Anchor, Painter, WHITE};
use crate::prefs::tr;
use nacre_core::island::timer::{format_mmss, Phase, PomodoroStage, Timer};
use nacre_core::notch::glance::Item;
use nacre_core::notch::state::{Activity, Look, NavState, Tab, View, NAV_EXPAND_H_DP};
use nacre_core::symbols::Sym;
use windows::UI::Color;
use windows::UI::Composition::{
    CompositionBitmapInterpolationMode, CompositionColorBrush, CompositionDrawingSurface,
    CompositionRoundedRectangleGeometry, CompositionStretch, ContainerVisual, SpriteVisual,
};
use windows_numerics::{Vector2, Vector3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    PlayPause,
    Next,
    Prev,
    ToggleLyrics,
    ToggleMute,
    Seek(f32, f32),
    Tab(Tab),
    ShelfItem(usize),
    ShelfClear,
    Zone(usize),
    Pin,
    Settings,
    TimerToggle,
    TimerReset,
    TimerStage(PomodoroStage),
    OpenTimerPill,
    OpenPrayer,
    CalDay(i32),
    CalToday,
    CalRefresh,
    CalAddOpen,
    SendDone,
    NotifFocus(u32),
    NotifDismiss(u32),
    NotifClearAll,
    TodoCalToggle,
    TodoAddOpen,
    TodoEditToggle,
    TodoFocus,
    TodoToggle(usize),
    TodoDelete(usize),
    TodoConnect,
    GeminiFocus,
    GeminiSend,
    GeminiStop,
    GeminiCopy,
    GeminiClear,
    GeminiQuick(u8),
}

pub struct Region {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub action: Action,
    pub vis: Option<ContainerVisual>,
    pub hover_bg: Option<SpriteVisual>,
}

fn kids(c: &ContainerVisual) -> i32 {
    c.Children().and_then(|k| k.Count()).unwrap_or(0)
}

fn group_since(c: &ContainerVisual, n0: i32) -> Option<ContainerVisual> {
    let col = c.Children().ok()?;
    let mut all = Vec::new();
    let it = col.First().ok()?;
    while it.HasCurrent().unwrap_or(false) {
        all.push(it.Current().ok()?);
        let _ = it.MoveNext();
    }
    let added = all.get(n0.max(0) as usize..)?;
    if added.is_empty() {
        return None;
    }
    let g = c.Compositor().ok()?.CreateContainerVisual().ok()?;
    let gk = g.Children().ok()?;
    for v in added {
        col.Remove(v).ok()?;
        gk.InsertAtTop(v).ok()?;
    }
    col.InsertAtTop(&g).ok()?;
    Some(g)
}

impl Region {
    pub fn hit(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

#[derive(Clone, Copy)]
pub struct Frame {
    pub x0: f32,
    pub w: f32,
    pub h: f32,
}

pub struct Data<'a> {
    pub battery: Option<(u8, bool)>,
    pub timer: &'a Timer,
    pub now_ms: i64,
    pub glance: Option<(Item, String)>,
    pub media: Option<MediaView<'a>>,
    pub lyrics: LyricsView<'a>,
    pub lyrics_open: bool,
    pub lyrics_progress: f32,
    pub play_gliding: bool,
    pub muted: bool,
    pub hovered: bool,
    pub nav_expanded: bool,
    pub nav_progress: f32,
    pub prev_tab: Option<Tab>,
    pub scrub_hover: f32,
    pub art_scale: f32,
    pub shelf: Vec<(&'a str, Option<&'a CompositionDrawingSurface>)>,
    pub drop_zone: Option<usize>,
    pub drop_count: usize,
    pub privacy: (bool, bool),
    pub download: Option<DownloadView<'a>>,
    pub cal: CalView<'a>,
    pub send: Option<SendView<'a>>,
    pub send_zone: bool,
    pub pinned: bool,
    pub notif_toast: Option<NotifRow<'a>>,
    pub notif_list: Vec<NotifRow<'a>>,
    pub notif_unread: usize,
    pub notif_scroll: f32,
    pub notif_status: nacre_platform::notifications::Status,
    pub timer_running: bool,
    pub detached_timer: Option<(f32, f32, f32, f32)>,
    pub todos: Vec<&'a nacre_core::notch::todo::TodoItem>,
    pub todo_done: usize,
    pub todo_total: usize,
    pub todo_scroll: f32,
    pub todo_adding: bool,
    pub todo_editing: bool,
    pub todo_cal_mode: bool,
    pub todo_cal_adding: bool,
    pub todo_cal_scroll: f32,
    pub todo_cal_err: &'a str,
    pub todo_status: nacre_platform::gtasks::SyncStatus,
    pub gemini_text: &'a str,
    pub gemini_streaming: bool,
    pub gemini_error: &'a str,
    pub gemini_has_key: bool,
    pub gemini_scroll: f32,
    pub gemini_auto_follow: bool,
    pub gemini_model_label: &'a str,
    pub gemini_copied: bool,
    pub prayer: PrayerView<'a>,
    pub prayer_alert: Option<PrayerAlertView>,
    pub sb_hover: f32,
    pub sb_active: f32,
    pub sb_dragging: bool,
}

pub struct PrayerView<'a> {
    pub city: &'a str,
    pub snap: nacre_core::notch::prayer::PrayerSnapshot,
}

#[derive(Clone, Copy, Debug)]
pub struct PrayerAlertView {
    pub prayer: nacre_core::notch::prayer::Prayer,
    pub time_min: u16,
    pub kind: nacre_core::notch::prayer::AlertKind,
}

pub struct NotifRow<'a> {
    pub id: u32,
    pub app_name: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub time: String,
    pub unread: bool,
    pub icon: Option<&'a CompositionDrawingSurface>,
}

pub struct SendView<'a> {
    pub qr: Option<&'a CompositionDrawingSurface>,
    pub qr_px: f32,
    pub title: String,
    pub status: String,
    pub frac: f32,
    pub done: bool,
    pub error: bool,
}

pub type Date = (i32, u32, u32);

pub struct CalView<'a> {
    pub today: Date,
    pub sel: Date,
    pub events: Events<'a>,
}

pub enum Events<'a> {
    Loading,
    Unavailable,
    List(&'a [nacre_platform::agenda::Event]),
}

pub struct DownloadView<'a> {
    pub icon: Option<&'a CompositionDrawingSurface>,
    pub label: String,
    pub frac: Option<f32>,
    pub done: bool,
    pub up: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LyrState {
    Idle,
    Loading,
    Ready,
    Missing,
}

pub struct LyricsView<'a> {
    pub lines: Option<&'a nacre_core::notch::lyrics::Lyrics>,
    pub state: LyrState,
    pub pos_ms: i64,
}

pub struct MediaView<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub playing: bool,
    pub pos_ms: i64,
    pub dur_ms: i64,
    pub can_seek: bool,
    pub art: Option<&'a CompositionDrawingSurface>,
    pub tint: Option<[u8; 3]>,
    pub drag: Option<f32>,
}

pub struct MarqueeLive {
    pub vis: SpriteVisual,
    pub travel: f32,
    pub base_y: f32,
}

pub struct ScrubLive {
    pub left_x: f32,
    pub right_x: f32,
    pub sx: f32,
    pub sw: f32,
    pub sy: f32,
    pub base_th: f32,
    pub dur_ms: i64,
    pub track: SpriteVisual,
    pub track_geo: CompositionRoundedRectangleGeometry,
    pub fill: SpriteVisual,
    pub fill_geo: CompositionRoundedRectangleGeometry,
    pub knob: SpriteVisual,
    pub knob_geo: CompositionRoundedRectangleGeometry,
    pub knob_brush: CompositionColorBrush,
    pub pos_vis: SpriteVisual,
    pub rem_vis: SpriteVisual,
    last_pos_sec: i64,
    last_pos_h: f32,
    last_rem_w: f32,
    last_rem_h: f32,
}

impl ScrubLive {
    pub fn set_bounds(&mut self, left_x: f32, right_x: f32, sy: f32, left_time_w: f32, right_time_w: f32, min_sw: f32) {
        self.left_x = left_x;
        self.right_x = right_x;
        self.sy = sy;
        self.sx = left_x + left_time_w;
        self.sw = (right_x - left_x - left_time_w - right_time_w).max(min_sw);
    }

    pub fn update(&mut self, p: &mut Painter, frac: f32, hover_t: f32, dragging: bool, pos_ms: i64) {
        let fr = frac.clamp(0.0, 1.0);
        let ht = if dragging { 1.0 } else { hover_t.clamp(0.0, 1.0) };
        let th = self.base_th + p.px(1.5) * ht;
        let ty = (self.sy - th / 2.0).round();

        let _ = self.track.SetSize(Vector2 { X: self.sw, Y: th });
        let _ = self.track.SetOffset(Vector3 { X: self.sx, Y: ty, Z: 0.0 });
        let _ = self.track_geo.SetSize(Vector2 { X: self.sw, Y: th });
        let _ = self.track_geo.SetCornerRadius(Vector2 { X: th / 2.0, Y: th / 2.0 });

        let fw = (self.sw * fr).max(th);
        let _ = self.fill.SetSize(Vector2 { X: fw, Y: th });
        let _ = self.fill.SetOffset(Vector3 { X: self.sx, Y: ty, Z: 0.0 });
        let _ = self.fill_geo.SetSize(Vector2 { X: fw, Y: th });
        let _ = self.fill_geo.SetCornerRadius(Vector2 { X: th / 2.0, Y: th / 2.0 });

        // Clean flush bar while idle (matching Nusa); circular thumb blooms smoothly on hover/drag.
        let kx = self.sx + self.sw * fr;
        let k_size = if dragging {
            p.px(13.0)
        } else {
            (p.px(10.0) * ht).max(0.5)
        };
        let k_op = if dragging { 1.0 } else { ht };
        let _ = self.knob.SetSize(Vector2 { X: k_size, Y: k_size });
        let _ = self.knob.SetOffset(Vector3 { X: (kx - k_size / 2.0).round(), Y: (self.sy - k_size / 2.0).round(), Z: 0.0 });
        let _ = self.knob.SetOpacity(k_op);
        let _ = self.knob_geo.SetSize(Vector2 { X: k_size, Y: k_size });
        let _ = self.knob_geo.SetCornerRadius(Vector2 { X: k_size / 2.0, Y: k_size / 2.0 });
        let _ = self.knob_brush.SetColor(white(if dragging { 255 } else { 248 }));

        let sec = (pos_ms.max(0) + 500) / 1000;
        if sec != self.last_pos_sec {
            self.last_pos_sec = sec;
            let pos_s = mmss_pad(pos_ms);
            if let Some((surf, tw, th)) = p.render_text_surface(&pos_s, 11.0, 500, WHITE, p.px(42.0)) {
                if let Ok(br) = p.comp.CreateSurfaceBrushWithSurface(&surf) {
                    let _ = br.SetStretch(CompositionStretch::None);
                    let _ = br.SetBitmapInterpolationMode(CompositionBitmapInterpolationMode::NearestNeighbor);
                    let _ = self.pos_vis.SetBrush(&br);
                }
                self.last_pos_h = th as f32;
                let _ = self.pos_vis.SetSize(Vector2 { X: tw as f32, Y: th as f32 });
            }
            let rem_s = format!("-{}", mmss_pad((self.dur_ms - pos_ms).max(0)));
            if let Some((surf, tw, th)) = p.render_text_surface(&rem_s, 11.0, 500, WHITE, p.px(48.0)) {
                if let Ok(br) = p.comp.CreateSurfaceBrushWithSurface(&surf) {
                    let _ = br.SetStretch(CompositionStretch::None);
                    let _ = br.SetBitmapInterpolationMode(CompositionBitmapInterpolationMode::NearestNeighbor);
                    let _ = self.rem_vis.SetBrush(&br);
                }
                self.last_rem_w = tw as f32;
                self.last_rem_h = th as f32;
                let _ = self.rem_vis.SetSize(Vector2 { X: tw as f32, Y: th as f32 });
            }
        }
        // Always track animated left_x / right_x without re-rasterizing text surfaces.
        if self.last_pos_h > 0.0 {
            let _ = self.pos_vis.SetOffset(Vector3 {
                X: self.left_x.round(),
                Y: (self.sy - self.last_pos_h / 2.0).round(),
                Z: 0.0,
            });
        }
        if self.last_rem_h > 0.0 {
            let _ = self.rem_vis.SetOffset(Vector3 {
                X: (self.right_x - self.last_rem_w).round(),
                Y: (self.sy - self.last_rem_h / 2.0).round(),
                Z: 0.0,
            });
        }
    }
}

pub struct PlayLayoutLive {
    pub base_mx: f32,
    pub base_cy: f32,
    pub left_group: Option<ContainerVisual>,
    pub empty_center: Option<(f32, ContainerVisual)>,
    pub title_clip: Option<ContainerVisual>,
    pub title_clip_h: f32,
    pub ctrl_groups: Vec<(Action, f32, ContainerVisual)>,
    pub lyrics_divider: Option<SpriteVisual>,
    pub lyrics_clip: Option<ContainerVisual>,
    pub lyrics_panel: Option<ContainerVisual>,
    pub lyrics_glow: Option<SpriteVisual>,
}

pub struct DropZoneLive {
    pub id: usize,
    pub brush: CompositionColorBrush,
    pub fg: ContainerVisual,
}

impl DropZoneLive {
    pub fn set_active(&self, on: bool, accent: [u8; 3]) {
        let fill = if on {
            Color { A: 230, R: accent[0], G: accent[1], B: accent[2] }
        } else {
            white(20)
        };
        let _ = self.brush.SetColor(fill);
        let _ = self.fg.SetOpacity(if on { 1.0 } else { 0.85 });
    }
}

pub struct CompactLyricLive {
    pub viewport: ContainerVisual,
    pub active: Option<ContainerVisual>,
    pub ghost: Option<ContainerVisual>,
    pub active_tw: f32,
    pub active_marquee: bool,
    pub vw: f32,
    pub h: f32,
    pub left_pad: f32,
    pub right_pad: f32,
}

pub struct LyricsHostLive {
    pub host: ContainerVisual,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub compact_player: bool,
}

pub struct GeminiRespHost {
    pub host: ContainerVisual,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub struct ScrollBarLive {
    pub right_x: f32,
    pub track_y: f32,
    pub track_h: f32,
    pub base_w: f32,
    pub hover_w: f32,
    pub hit_x: f32,
    pub hit_w: f32,
    pub thumb_h: f32,
    pub thumb_y: f32,
    pub track: SpriteVisual,
    pub track_geo: CompositionRoundedRectangleGeometry,
    pub track_brush: CompositionColorBrush,
    pub thumb: SpriteVisual,
    pub thumb_geo: CompositionRoundedRectangleGeometry,
    pub thumb_brush: CompositionColorBrush,
}

impl ScrollBarLive {
    pub fn hit_rail(&self, x: f32, y: f32) -> bool {
        x >= self.hit_x && x <= self.hit_x + self.hit_w && y >= self.track_y - 4.0 && y <= self.track_y + self.track_h + 4.0
    }

    pub fn hit_thumb(&self, y: f32) -> bool {
        y >= self.thumb_y - 4.0 && y <= self.thumb_y + self.thumb_h + 4.0
    }

    pub fn scroll_for_thumb_top(&self, thumb_top: f32, max_scroll: f32) -> f32 {
        let travel = (self.track_h - self.thumb_h).max(1.0);
        (((thumb_top - self.track_y) / travel).clamp(0.0, 1.0)) * max_scroll.max(0.0)
    }
}

pub struct ScrollViewLive {
    pub vx: f32,
    pub vy: f32,
    pub vw: f32,
    pub vh: f32,
    pub max_scroll: f32,
    pub scroll_px: f32,
    pub content: ContainerVisual,
    pub bar: Option<ScrollBarLive>,
    pub regions: Vec<Region>,
}

impl ScrollViewLive {
    pub fn apply(&mut self, scroll_px: f32, hover_t: f32, dragging: bool, active_t: f32) {
        let s = scroll_px.clamp(0.0, self.max_scroll);
        self.scroll_px = s;
        let _ = self.content.SetOffset(Vector3 { X: 0.0, Y: -s, Z: 0.0 });
        if let Some(bar) = self.bar.as_mut() {
            let ht = if dragging { 1.0 } else { hover_t.clamp(0.0, 1.0) };
            let at = active_t.clamp(0.0, 1.0).max(ht);
            let bw = bar.base_w + (bar.hover_w - bar.base_w) * ht;
            let bx = bar.right_x - bw;
            let r = bw / 2.0;

            let _ = bar.track.SetSize(Vector2 { X: bw, Y: bar.track_h });
            let _ = bar.track.SetOffset(Vector3 { X: bx, Y: bar.track_y, Z: 0.0 });
            let _ = bar.track_geo.SetSize(Vector2 { X: bw, Y: bar.track_h });
            let _ = bar.track_geo.SetCornerRadius(Vector2 { X: r, Y: r });
            let tr_a = (14.0 + 22.0 * ht + 10.0 * at).round().clamp(0.0, 255.0) as u8;
            let _ = bar.track_brush.SetColor(white(tr_a));

            let frac = if self.max_scroll > 0.01 { (s / self.max_scroll).clamp(0.0, 1.0) } else { 0.0 };
            bar.thumb_y = bar.track_y + (bar.track_h - bar.thumb_h) * frac;
            let _ = bar.thumb.SetSize(Vector2 { X: bw, Y: bar.thumb_h });
            let _ = bar.thumb.SetOffset(Vector3 { X: bx, Y: bar.thumb_y, Z: 0.0 });
            let _ = bar.thumb_geo.SetSize(Vector2 { X: bw, Y: bar.thumb_h });
            let _ = bar.thumb_geo.SetCornerRadius(Vector2 { X: r, Y: r });
            let th_a = if dragging {
                244u8
            } else {
                (84.0 + 58.0 * at + 52.0 * ht).round().clamp(0.0, 240.0) as u8
            };
            let _ = bar.thumb_brush.SetColor(white(th_a));
        }
    }

    pub fn hit_viewport(&self, x: f32, y: f32) -> bool {
        x >= self.vx && x < self.vx + self.vw && y >= self.vy && y < self.vy + self.vh
    }

    pub fn hit_region(&self, x: f32, y: f32) -> Option<Action> {
        if !self.hit_viewport(x, y) || self.bar.as_ref().is_some_and(|b| b.hit_rail(x, y)) {
            return None;
        }
        let lx = x - self.vx;
        let ly = y - self.vy + self.scroll_px;
        self.regions.iter().find(|r| r.hit(lx, ly)).map(|r| r.action)
    }

    pub fn hit_hover_region(&self, x: f32, y: f32) -> Option<Action> {
        if !self.hit_viewport(x, y) || self.bar.as_ref().is_some_and(|b| b.hit_rail(x, y)) {
            return None;
        }
        let lx = x - self.vx;
        let ly = y - self.vy + self.scroll_px;
        self.regions.iter().find(|r| r.vis.is_some() && r.hit(lx, ly)).map(|r| r.action)
    }

    pub fn find_region(&self, act: Action) -> Option<&Region> {
        self.regions.iter().find(|r| r.action == act)
    }
}

pub struct NavLive {
    pub cx: f32,
    pub dpi: f32,
    pub dots_w: f32,
    pub dots_h: f32,
    pub pill_w: f32,
    pub pill_h: f32,
    pub anchor: ContainerVisual,
    pub bg: SpriteVisual,
    pub bg_geo: CompositionRoundedRectangleGeometry,
    pub bg_brush: CompositionColorBrush,
    pub dots_group: ContainerVisual,
    pub pill_group: ContainerVisual,
    pub dots_rect: (f32, f32, f32, f32),
    pub pill_rect: (f32, f32, f32, f32),
    pub pill_regions: Vec<Region>,
    pub cur_cy: f32,
}

impl NavLive {
    pub fn set_progress_and_height(&mut self, raw_t: f32, cur_h: f32) {
        let t = raw_t.clamp(0.0, 1.0);
        let cy = (cur_h - (11.5 + 8.5 * t) * self.dpi).round();
        self.cur_cy = cy;
        let _ = self.anchor.SetOffset(Vector3 { X: 0.0, Y: cy, Z: 0.0 });

        let w = self.dots_w + (self.pill_w - self.dots_w) * t;
        let h = self.dots_h + (self.pill_h - self.dots_h) * t;
        let r = h / 2.0;
        let _ = self.bg.SetSize(Vector2 { X: w, Y: h });
        let _ = self.bg.SetOffset(Vector3 { X: (self.cx - w / 2.0).round(), Y: (-h / 2.0).round(), Z: 0.0 });
        let _ = self.bg_geo.SetSize(Vector2 { X: w, Y: h });
        let _ = self.bg_geo.SetCornerRadius(Vector2 { X: r, Y: r });
        let alpha = (24.0 * t).round().clamp(0.0, 255.0) as u8;
        let _ = self.bg_brush.SetColor(white(alpha));

        let dots_op = (1.0 - t * 2.6).clamp(0.0, 1.0);
        let _ = self.dots_group.SetOpacity(dots_op);
        let _ = self.dots_group.SetScale(Vector3 { X: 1.0 + 0.34 * t, Y: 1.0, Z: 1.0 });

        let pill_op = ((t - 0.16) / 0.84).clamp(0.0, 1.0);
        let pill_sc = 0.84 + 0.16 * t;
        let _ = self.pill_group.SetOpacity(pill_op);
        let _ = self.pill_group.SetScale(Vector3 { X: pill_sc, Y: pill_sc, Z: 1.0 });
    }

    pub fn hit_dots(&self, x: f32, y: f32) -> bool {
        let (rx, ry, rw, rh) = self.dots_rect;
        x >= rx && x < rx + rw && y >= ry && y < ry + rh
    }

    pub fn hit_pill(&self, x: f32, y: f32) -> bool {
        let (rx, ry, rw, rh) = self.pill_rect;
        x >= rx && x < rx + rw && y >= ry && y < ry + rh
    }

    pub fn hit_region(&self, x: f32, y: f32) -> Option<Action> {
        let ly = y - self.cur_cy;
        self.pill_regions.iter().find(|r| r.hit(x, ly)).map(|r| r.action)
    }

    pub fn find_region(&self, act: Action) -> Option<&Region> {
        self.pill_regions.iter().find(|r| r.action == act)
    }
}

#[derive(Default)]
pub struct Live {
    pub bars: Vec<SpriteVisual>,
    pub vis: nacre_core::theme::VisStyle,
    pub marquee: Option<MarqueeLive>,
    pub scrub: Option<ScrubLive>,
    pub drop_zones: Vec<DropZoneLive>,
    pub lyrics_host: Option<LyricsHostLive>,
    pub todo_input_box: Option<(f32, f32, f32, f32)>,
    pub gemini_input_box: Option<(f32, f32, f32, f32)>,
    pub gemini_resp_host: Option<GeminiRespHost>,
    pub scroll: Option<ScrollViewLive>,
    pub compact_lyric: Option<CompactLyricLive>,
    pub compact_left: Option<ContainerVisual>,
    pub compact_right: Option<ContainerVisual>,
    pub compact_glow: Option<SpriteVisual>,
    pub tab_glow: Option<SpriteVisual>,
    pub detached_timer: Option<ContainerVisual>,
    pub nav: Option<NavLive>,
    pub pin_vis: Option<ContainerVisual>,
    pub top_right_group: Option<(f32, ContainerVisual)>,
    pub top_right_wave: Option<ContainerVisual>,
    pub top_right_pin_shift: f32,
    pub play_layout: Option<PlayLayoutLive>,
    pub album_art: Option<ContainerVisual>,
}

const ORANGE: [u8; 3] = [255, 159, 10];
const GREEN: [u8; 3] = [48, 209, 88];
const BLUE: [u8; 3] = [10, 132, 255];
const RED: [u8; 3] = [255, 59, 48];
const CARD_DARK: Color = Color { A: 255, R: 20, G: 20, B: 22 }; // #141416

const TRACK_PALETTE: [[u8; 3]; 8] = [
    [240, 93, 78],  // Coral Red
    [245, 158, 11], // Warm Amber
    [52, 211, 153], // Emerald Mint
    [56, 189, 248], // Sky Cyan
    [99, 102, 241], // Indigo Blue
    [168, 85, 247], // Electric Violet
    [236, 72, 153], // Rose Magenta
    [20, 184, 166], // Deep Teal
];

/// Gently lifts darker hues toward white so text/icons stay crisp on pure black OLED pills
/// while preserving the exact color tone of the active Abra theme or Live-Lyrics album tint.
fn lift_for_dark_pill(rgb: [u8; 3]) -> [u8; 3] {
    let lum = 0.299 * rgb[0] as f32 + 0.587 * rgb[1] as f32 + 0.114 * rgb[2] as f32;
    if lum >= 165.0 {
        return rgb;
    }
    let mix = ((165.0 - lum) / 185.0).clamp(0.0, 0.48);
    [
        (rgb[0] as f32 + (255.0 - rgb[0] as f32) * mix).round() as u8,
        (rgb[1] as f32 + (255.0 - rgb[1] as f32) * mix).round() as u8,
        (rgb[2] as f32 + (255.0 - rgb[2] as f32) * mix).round() as u8,
    ]
}

const DOTS_STRIP_DP: f32 = 22.0;
const EDGE_PAD_DP: f32 = 18.0;

fn base_height(p: &Painter, f: Frame, tab: Tab, d: &Data) -> f32 {
    if d.nav_expanded && nacre_core::notch::state::nav_state(tab, d.lyrics_open, true) == NavState::Pill {
        (f.h - p.px(NAV_EXPAND_H_DP)).max(p.px(100.0))
    } else {
        f.h
    }
}

pub fn draw(p: &mut Painter, c: &ContainerVisual, look: Look, f: Frame, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    match look.view {
        View::Rest => {}
        View::Compact(Activity::Notification) => compact_notification(p, c, f, d, regions),
        View::Compact(Activity::PrayerAlert) => compact_prayer_alert(p, c, f, d, regions),
        View::Compact(Activity::Timer) | View::Compact(Activity::TimerEnding) => {
            let urgent = look.view == View::Compact(Activity::TimerEnding);
            compact_timer(p, c, f, d, urgent)
        }
        View::Compact(Activity::Charging) => compact_charging(p, c, f, d),
        View::Compact(Activity::Glance) => compact_glance(p, c, f, d),
        View::Compact(Activity::Music) => compact_music(p, c, f, d, live, f.h, true),
        View::Compact(Activity::Lyrics) => compact_lyrics(p, c, f, d, live),
        View::Compact(Activity::Shelf) => compact_shelf(p, c, f, d),
        View::Compact(Activity::Download) => compact_download(p, c, f, d),
        View::Compact(Activity::Player) => compact_player(p, c, f, d, regions, live),
        View::Open(tab) => {
            let bh = base_height(p, f, tab, d);
            let bf = Frame { x0: f.x0, w: f.w, h: bh };
            match tab {
                Tab::Play => play(p, c, bf, f.h, d, regions, live),
                Tab::Alerts => alerts_tab(p, c, bf, d, regions, live),
                Tab::Shelf if d.send.is_some() => send_card(p, c, bf, d, regions),
                Tab::Shelf if d.shelf.is_empty() => shelf_empty(p, c, bf),
                Tab::Shelf => shelf_tab(p, c, bf, d, regions),
                Tab::Todo => todo_tab(p, c, bf, d, regions, live),
                Tab::Gemini => gemini_tab(p, c, bf, f.h, d, regions, live),
                Tab::Timer => timer(p, c, bf, f.h, d, regions, live),
                Tab::Prayer => prayer_tab(p, c, bf, f.h, d, live),
            }
            top_utilities(p, c, bf, tab, d, regions, live);
            let show_nav = nacre_core::notch::state::nav_state(tab, d.lyrics_open, false) != NavState::Hidden
                || (tab == Tab::Play && (d.play_gliding || d.lyrics_progress < 0.995));
            if show_nav {
                bottom_nav(p, c, bf, f.h, tab, d, live);
            }
        }
        View::Drop => drop_zones(p, c, f, d, regions, live),
    }
    if matches!(look.view, View::Rest | View::Compact(_)) {
        privacy_dots(p, c, f, d.privacy);
        if let Some((tx, tw, th, op)) = d.detached_timer {
            detached_timer_pill(p, c, tx, tw, th, op, d, regions, live);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn detached_timer_pill(
    p: &mut Painter,
    c: &ContainerVisual,
    tx: f32,
    tw: f32,
    th: f32,
    op: f32,
    d: &Data,
    regions: &mut Vec<Region>,
    live: &mut Live,
) {
    let Ok(wrap) = p.comp.CreateContainerVisual() else { return };
    let _ = wrap.SetOffset(Vector3 { X: tx.round(), Y: 0.0, Z: 0.0 });
    let _ = wrap.SetOpacity(op.clamp(0.0, 1.0));
    let _ = c.Children().and_then(|k| k.InsertAtTop(&wrap));

    let cy = th / 2.0;
    let base_rgb = d.media.as_ref().map(media_tint).unwrap_or_else(crate::prefs::accent);
    let fg_rgb = lift_for_dark_pill(base_rgb);
    if let Some(glow) = p.ambient_glow(&wrap, 0.0, 0.0, tw, th, 0.0, base_rgb, true) {
        let _ = glow.SetOpacity(if d.timer.phase == Phase::Running { 0.85 } else { 0.45 });
    }
    let content_op = if d.timer.phase == Phase::Running { 1.0 } else { 0.78 };
    p.sym(&wrap, Sym::Stopwatch, p.px(19.0), cy, 14.5, fg_rgb, content_op);
    let txt = format_mmss(d.timer.remaining_secs(d.now_ms));
    let txt_h = p.px(13.5) * 1.28;
    p.text(
        &wrap,
        &txt,
        tw - p.px(12.0),
        cy - txt_h / 2.0,
        13.5,
        700,
        fg_rgb,
        content_op,
        Anchor::Right,
        (tw - p.px(30.0)).max(p.px(24.0)),
    );
    regions.push(Region {
        x: tx,
        y: 0.0,
        w: tw,
        h: th,
        action: Action::OpenTimerPill,
        vis: None,
        hover_bg: None,
    });
    live.detached_timer = Some(wrap);
}

fn privacy_dots(p: &mut Painter, c: &ContainerVisual, f: Frame, (mic, cam): (bool, bool)) {
    let (r, cy) = (p.px(3.0), f.h / 2.0);
    let mut x = f.x0 + f.w - p.px(7.0);
    for (on, rgb) in [(mic, ORANGE), (cam, GREEN)] {
        if on {
            p.rect(c, x - r, cy - r, 2.0 * r, 2.0 * r, r, Color { A: 255, R: rgb[0], G: rgb[1], B: rgb[2] });
            x -= p.px(9.0);
        }
    }
}

pub(super) fn count_text(n: usize) -> String {
    if crate::prefs::get().lang == nacre_core::i18n::Lang::En {
        format!("{n} {}", if n == 1 { "file" } else { "files" })
    } else {
        format!("{n} berkas")
    }
}

fn make_hover_circle(p: &Painter, parent: &ContainerVisual, cx: f32, cy: f32, dia: f32) -> Option<SpriteVisual> {
    let bg = p.rect(
        parent,
        (cx - dia / 2.0).round(),
        (cy - dia / 2.0).round(),
        dia,
        dia,
        dia / 2.0,
        white(36),
    )?;
    let _ = bg.SetOpacity(0.0);
    Some(bg)
}

fn top_utilities(p: &mut Painter, c: &ContainerVisual, f: Frame, cur: Tab, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    let Ok(right_anchor) = p.comp.CreateContainerVisual() else { return };
    let _ = c.Children().and_then(|k| k.InsertAtTop(&right_anchor));
    live.top_right_group = Some((f.x0 + f.w, right_anchor.clone()));

    let lp = d.lyrics_progress.clamp(0.0, 1.0);
    let cy = if cur == Tab::Timer { p.px(15.0) } else { p.px(18.0) };
    let mut right_x = f.x0 + f.w - p.px(22.0);
    let media_playing = d.media.as_ref().is_some_and(|m| m.playing);

    // Reserve right slot only when Shelf (non-empty) or Alerts already has a top-right Clear action.
    let shelf_has_clear = cur == Tab::Shelf && d.send.is_none() && !d.shelf.is_empty();
    let alerts_has_clear = cur == Tab::Alerts && !d.notif_list.is_empty();
    let gemini_has_btn = cur == Tab::Gemini;
    let timer_clean_top = matches!(cur, Tab::Timer | Tab::Prayer);
    let show_play_wave = cur == Tab::Play && media_playing && (!d.lyrics_open || d.play_gliding || lp < 0.995);

    if shelf_has_clear {
        right_x -= p.px(66.0);
    } else if alerts_has_clear {
        right_x -= p.px(68.0);
    } else if gemini_has_btn {
        // Keep Pin neatly tucked away from the Gemini send button.
        right_x = f.x0 + f.w - p.px(13.0);
    } else if timer_clean_top {
        // Keep Pomodoro right column clean; only tuck Pin neatly at top-right corner when hovered/pinned.
        right_x = f.x0 + f.w - p.px(18.0);
    } else if d.notif_unread > 0 && cur != Tab::Alerts {
        // Unread alerts bell with crisp red dot.
        let n0 = kids(&right_anchor);
        p.sym(&right_anchor, Sym::Bell, right_x, cy, 14.5, WHITE, 0.96);
        let dot = p.px(5.0);
        p.rect(&right_anchor, right_x + p.px(3.5), cy - p.px(7.5), dot, dot, dot / 2.0, Color { A: 255, R: RED[0], G: RED[1], B: RED[2] });
        regions.push(Region {
            x: right_x - p.px(13.0),
            y: cy - p.px(13.0),
            w: p.px(26.0),
            h: p.px(26.0),
            action: Action::Tab(Tab::Alerts),
            vis: group_since(&right_anchor, n0),
            hover_bg: None,
        });
        right_x -= p.px(28.0);
    } else if (media_playing && cur != Tab::Play) || show_play_wave {
        // Live audio visualizer badge in top-right (follows selected VisStyle; click jumps to Play).
        let prefs = crate::prefs::get();
        let style = prefs.visualizer;
        let square = prefs.preset == nacre_core::theme::Preset::Pixel;
        live.vis = style;
        let n = style.count();
        let (bw, gap, bh) = match style {
            nacre_core::theme::VisStyle::Bars => (p.px(2.4), p.px(2.2), p.px(14.0)),
            nacre_core::theme::VisStyle::Spectrum => (p.px(1.4), p.px(0.7), p.px(14.0)),
            nacre_core::theme::VisStyle::Wave => (p.px(1.0), p.px(0.45), p.px(14.0)),
            nacre_core::theme::VisStyle::Dots => (p.px(3.2), p.px(1.1), p.px(3.2)),
        };
        let total_w = n as f32 * bw + (n.saturating_sub(1)) as f32 * gap;
        let x0 = (right_x + p.px(10.0) - total_w).round();
        let shift = (total_w + p.px(10.0)).max(p.px(30.0));
        let target_parent = if cur == Tab::Play {
            if let Ok(wave_wrap) = p.comp.CreateContainerVisual() {
                let w_raw = ((0.60 - lp) / 0.60).clamp(0.0, 1.0);
                let wave_op = w_raw * w_raw * (3.0 - 2.0 * w_raw);
                let wave_sc = 0.78 + 0.22 * wave_op;
                let _ = wave_wrap.SetCenterPoint(Vector3 { X: x0 + total_w / 2.0, Y: cy, Z: 0.0 });
                let _ = wave_wrap.SetScale(Vector3 { X: wave_sc, Y: wave_sc, Z: 1.0 });
                let _ = wave_wrap.SetOpacity(wave_op);
                let _ = right_anchor.Children().and_then(|k| k.InsertAtTop(&wave_wrap));
                live.top_right_wave = Some(wave_wrap.clone());
                live.top_right_pin_shift = shift;
                wave_wrap
            } else {
                right_anchor.clone()
            }
        } else {
            right_anchor.clone()
        };
        let n0 = kids(&target_parent);
        let radius = if square { 0.0 } else { bw / 2.0 };
        let start = if style == nacre_core::theme::VisStyle::Dots {
            Vector3 { X: 0.7, Y: 0.7, Z: 1.0 }
        } else {
            Vector3 { X: 1.0, Y: 0.45, Z: 1.0 }
        };
        let bars_h = [8.0f32, 11.5, 14.0, 11.5, 8.0];
        for i in 0..n {
            let h_px = if style == nacre_core::theme::VisStyle::Bars { p.px(bars_h[i % bars_h.len()]) } else { bh };
            if let Some(v) = p.rect(&target_parent, x0 + i as f32 * (bw + gap), cy - h_px / 2.0, bw, h_px, radius, white(245)) {
                let _ = v.SetCenterPoint(Vector3 { X: bw / 2.0, Y: h_px / 2.0, Z: 0.0 });
                let _ = v.SetScale(start);
                live.bars.push(v);
            }
        }
        let vis = group_since(&target_parent, n0);
        if cur != Tab::Play {
            regions.push(Region {
                x: x0 - p.px(6.0),
                y: cy - p.px(12.0),
                w: total_w + p.px(12.0),
                h: p.px(24.0),
                action: Action::Tab(Tab::Play),
                vis,
                hover_bg: None,
            });
        }
        right_x -= shift;
    }

    // Pin button: hidden when not hovered (unless pinned), with 26x26 hover circle pill + 1.15x scale.
    if !gemini_has_btn {
        if let Ok(pin_wrap) = p.comp.CreateContainerVisual() {
            let op = if d.pinned { 1.0 } else if d.hovered { 0.54 } else { 0.0 };
            let _ = pin_wrap.SetOpacity(op);
            if live.top_right_pin_shift > 0.0 {
                let _ = pin_wrap.SetOffset(Vector3 { X: live.top_right_pin_shift * lp, Y: 0.0, Z: 0.0 });
            }
            let _ = right_anchor.Children().and_then(|k| k.InsertAtTop(&pin_wrap));
            let n0 = kids(&pin_wrap);
            let dia = p.px(26.0);
            let hover_bg = make_hover_circle(p, &pin_wrap, right_x, cy, dia);
            let pin_rgb = if d.pinned { crate::prefs::accent() } else { WHITE };
            p.sym(&pin_wrap, Sym::Pin, right_x, cy, 12.0, pin_rgb, 1.0);
            let hit_x = right_x + live.top_right_pin_shift * lp;
            regions.push(Region {
                x: hit_x - dia / 2.0,
                y: cy - dia / 2.0,
                w: dia,
                h: dia,
                action: Action::Pin,
                vis: group_since(&pin_wrap, n0),
                hover_bg,
            });
            live.pin_vis = Some(pin_wrap);
        }
    }
}

fn tab_sym(t: Tab) -> Sym {
    match t {
        Tab::Play => Sym::Note,
        Tab::Alerts => Sym::Bell,
        Tab::Shelf => Sym::Tray,
        Tab::Todo => Sym::Checklist,
        Tab::Gemini => Sym::Sparkle,
        Tab::Timer => Sym::Stopwatch,
        Tab::Prayer => Sym::Moon,
    }
}

fn bottom_nav(p: &mut Painter, c: &ContainerVisual, bf: Frame, cur_h: f32, cur: Tab, d: &Data, live: &mut Live) {
    let visible = crate::prefs::get().visible_notch_tabs();
    let mut items: Vec<(Option<Tab>, Sym)> = visible.iter().copied().map(|t| (Some(t), tab_sym(t))).collect();
    let sep_idx = items.len();
    items.push((None, Sym::Gear));

    let cx = bf.x0 + bf.w / 2.0;

    // Collapsed page dots geometry (`— · · · · ·`): 16x4 active capsule + 4x4 inactive dots.
    let active_w = p.px(16.0);
    let dot_d = p.px(4.0);
    let dot_gap = p.px(5.0);
    let active_in_list = visible.contains(&cur);
    let n_dots = visible.len();
    let dots_w = if n_dots == 0 {
        active_w
    } else if active_in_list {
        active_w + (n_dots.saturating_sub(1) as f32) * dot_d + (n_dots.saturating_sub(1) as f32) * dot_gap
    } else {
        (n_dots as f32) * dot_d + (n_dots.saturating_sub(1) as f32) * dot_gap
    };
    let dots_h = p.px(8.0);

    // Expanded pill geometry (custom visible tabs | Settings).
    let slot_w = p.px(28.0);
    let pill_h = p.px(26.0);
    let pad_x = p.px(6.0);
    let sep_w = p.px(7.0);
    let pill_w = items.len() as f32 * slot_w + sep_w + 2.0 * pad_x;

    let build = || -> windows_core::Result<NavLive> {
        let anchor = p.comp.CreateContainerVisual()?;
        c.Children()?.InsertAtTop(&anchor)?;

        let bg_geo = p.comp.CreateRoundedRectangleGeometry()?;
        let bg_clip = p.comp.CreateGeometricClip()?;
        bg_clip.SetGeometry(&bg_geo)?;
        let bg_brush = p.comp.CreateColorBrushWithColor(white(0))?;
        let bg = p.comp.CreateSpriteVisual()?;
        bg.SetBrush(&bg_brush)?;
        bg.SetClip(&bg_clip)?;
        anchor.Children()?.InsertAtTop(&bg)?;

        // State 2 visual group: minimal page dots centered at (cx, 0) in anchor space.
        let dots_group = p.comp.CreateContainerVisual()?;
        dots_group.SetCenterPoint(Vector3 { X: cx, Y: 0.0, Z: 0.0 })?;
        anchor.Children()?.InsertAtTop(&dots_group)?;
        let mut dx = (cx - dots_w / 2.0).round();
        let dy = (-dot_d / 2.0).round();
        let tab_switched = d.prev_tab.is_some_and(|pt| pt != cur);
        for &t in &visible {
            let on = t == cur;
            let was = d.prev_tab == Some(t);
            let dw = if on { active_w } else { dot_d };
            let col = if on { white(245) } else { white(90) };
            if let Some(dot_vis) = p.rect(&dots_group, dx, dy, dw, dot_d, dot_d / 2.0, col) {
                if tab_switched && on {
                    let _ = dot_vis.SetCenterPoint(Vector3 { X: dw / 2.0, Y: dot_d / 2.0, Z: 0.0 });
                    let _ = crate::motion::scalar(&p.comp, &dot_vis, "Scale.X", 0.28, 1.0, 360);
                } else if tab_switched && was {
                    let _ = dot_vis.SetCenterPoint(Vector3 { X: dw / 2.0, Y: dot_d / 2.0, Z: 0.0 });
                    let _ = crate::motion::scalar(&p.comp, &dot_vis, "Scale.X", 2.4, 1.0, 320);
                }
            }
            dx += dw + dot_gap;
        }

        // State 3 visual group: full icon pill centered at (cx, 0) in anchor space.
        let pill_group = p.comp.CreateContainerVisual()?;
        pill_group.SetCenterPoint(Vector3 { X: cx, Y: 0.0, Z: 0.0 })?;
        anchor.Children()?.InsertAtTop(&pill_group)?;

        let mut pill_regions = Vec::new();
        let x0 = (cx - pill_w / 2.0).round();
        let y0 = (-pill_h / 2.0).round();
        let mut x = x0 + pad_x;
        let accent = crate::prefs::accent();
        for (idx, (tab_opt, sym)) in items.into_iter().enumerate() {
            if idx == sep_idx {
                p.divider_v(&pill_group, x + sep_w / 2.0, y0 + p.px(5.0), pill_h - p.px(10.0));
                x += sep_w;
            }
            let on = tab_opt == Some(cur);
            let icx = x + slot_w / 2.0;
            let n0 = kids(&pill_group);
            let (rgb, op) = match tab_opt {
                Some(Tab::Timer) if d.timer_running => (lift_for_dark_pill(accent), 1.0),
                Some(_) if on => (accent, 1.0),
                _ => (WHITE, 0.56),
            };
            p.sym(&pill_group, sym, icx, 0.0, 12.5, rgb, op);

            if tab_opt == Some(Tab::Alerts) && d.notif_unread > 0 {
                let dot = p.px(4.5);
                p.rect(&pill_group, icx + p.px(5.0), -p.px(7.5), dot, dot, dot / 2.0, Color { A: 255, R: RED[0], G: RED[1], B: RED[2] });
            } else if tab_opt == Some(Tab::Shelf) && !d.shelf.is_empty() && !on {
                let dot = p.px(3.5);
                p.rect(&pill_group, icx + p.px(5.5), -p.px(6.5), dot, dot, dot / 2.0, white(150));
            }

            let act = match tab_opt {
                Some(t) => Action::Tab(t),
                None => Action::Settings,
            };
            pill_regions.push(Region {
                x,
                y: y0 - p.px(4.0),
                w: slot_w,
                h: pill_h + p.px(8.0),
                action: act,
                vis: group_since(&pill_group, n0),
                hover_bg: None,
            });
            x += slot_w;
        }

        let dots_hit_w = dots_w + p.px(36.0);
        let dots_hit_h = p.px(24.0);
        let dots_rect = (cx - dots_hit_w / 2.0, bf.h - dots_hit_h, dots_hit_w, dots_hit_h);

        let expanded_h = bf.h + p.px(NAV_EXPAND_H_DP);
        let pill_hit_w = pill_w + p.px(24.0);
        let pill_hit_top = bf.h - p.px(22.0);
        let pill_rect = (cx - pill_hit_w / 2.0, pill_hit_top, pill_hit_w, expanded_h - pill_hit_top + p.px(4.0));

        let mut nav = NavLive {
            cx,
            dpi: p.dpi,
            dots_w,
            dots_h,
            pill_w,
            pill_h,
            anchor,
            bg,
            bg_geo,
            bg_brush,
            dots_group,
            pill_group,
            dots_rect,
            pill_rect,
            pill_regions,
            cur_cy: 0.0,
        };
        nav.set_progress_and_height(d.nav_progress, cur_h);
        Ok(nav)
    };
    if let Ok(nav) = build() {
        live.nav = Some(nav);
    }
}

fn compact_notification(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>) {
    let Some(n) = &d.notif_toast else { return };
    let cy = f.h / 2.0;
    let isz = p.px(28.0);
    let ix = f.x0 + p.px(12.0);
    match n.icon {
        Some(s) => {
            p.image(c, s, ix, cy - isz / 2.0, isz, isz, p.px(7.0));
        }
        None => {
            let acc = crate::prefs::accent();
            p.rect(c, ix, cy - isz / 2.0, isz, isz, p.px(7.0), Color { A: 220, R: acc[0], G: acc[1], B: acc[2] });
            p.sym(c, Sym::Bell, ix + isz / 2.0, cy, 14.0, WHITE, 0.95);
        }
    }
    let dx = f.x0 + f.w - p.px(20.0);
    let tx = ix + isz + p.px(10.0);
    let tw = (dx - p.px(14.0) - tx).max(p.px(40.0));
    let head = if n.app_name.is_empty() || n.title.eq_ignore_ascii_case(n.app_name) {
        n.title.to_string()
    } else {
        format!("{} \u{00B7} {}", n.app_name, n.title)
    };
    let top_y = if n.body.is_empty() { cy - p.px(8.0) } else { p.px(7.0) };
    p.text(c, &p.fit(&head, 12.0, 700, tw), tx, top_y, 12.0, 700, WHITE, 0.98, Anchor::Left, tw + p.px(2.0));
    if !n.body.is_empty() {
        p.text(c, &p.fit(n.body, 11.0, 400, tw), tx, p.px(24.0), 11.0, 400, WHITE, 0.72, Anchor::Left, tw + p.px(2.0));
    }
    let n0 = kids(c);
    p.sym(c, Sym::Close, dx, cy, 9.5, WHITE, 0.6);
    regions.push(Region { x: dx - p.px(12.0), y: cy - p.px(12.0), w: p.px(24.0), h: p.px(24.0), action: Action::NotifDismiss(n.id), vis: group_since(c, n0), hover_bg: None });
    regions.push(Region { x: f.x0, y: 0.0, w: dx - p.px(12.0) - f.x0, h: f.h, action: Action::NotifFocus(n.id), vis: None, hover_bg: None });
}

fn create_scroll_viewport(
    p: &Painter,
    parent: &ContainerVisual,
    vx: f32,
    vy: f32,
    vw: f32,
    vh: f32,
) -> windows_core::Result<ContainerVisual> {
    let vp = p.comp.CreateContainerVisual()?;
    vp.SetOffset(Vector3 { X: vx.round(), Y: vy.round(), Z: 0.0 })?;
    vp.SetSize(Vector2 { X: vw.ceil(), Y: vh.ceil() })?;
    vp.SetClip(&p.comp.CreateInsetClip()?)?;
    parent.Children()?.InsertAtTop(&vp)?;
    let content = p.comp.CreateContainerVisual()?;
    content.SetSize(Vector2 { X: vw.ceil(), Y: vh.ceil() })?;
    vp.Children()?.InsertAtTop(&content)?;
    Ok(content)
}

#[allow(clippy::too_many_arguments)]
fn finish_scroll_view(
    p: &Painter,
    parent: &ContainerVisual,
    vx: f32,
    vy: f32,
    vw: f32,
    vh: f32,
    content: ContainerVisual,
    content_h: f32,
    sb_right_x: f32,
    sb_top_y: f32,
    sb_h: f32,
    scroll_px: f32,
    hover_t: f32,
    dragging: bool,
    active_t: f32,
    regions: Vec<Region>,
) -> ScrollViewLive {
    let max_scroll = (content_h - vh).max(0.0);
    let bar = if max_scroll > 1.0 && sb_h > p.px(14.0) {
        let build = || -> windows_core::Result<ScrollBarLive> {
            let track_geo = p.comp.CreateRoundedRectangleGeometry()?;
            let track_clip = p.comp.CreateGeometricClip()?;
            track_clip.SetGeometry(&track_geo)?;
            let track_brush = p.comp.CreateColorBrushWithColor(white(14))?;
            let track = p.comp.CreateSpriteVisual()?;
            track.SetBrush(&track_brush)?;
            track.SetClip(&track_clip)?;
            parent.Children()?.InsertAtTop(&track)?;

            let thumb_geo = p.comp.CreateRoundedRectangleGeometry()?;
            let thumb_clip = p.comp.CreateGeometricClip()?;
            thumb_clip.SetGeometry(&thumb_geo)?;
            let thumb_brush = p.comp.CreateColorBrushWithColor(white(84))?;
            let thumb = p.comp.CreateSpriteVisual()?;
            thumb.SetBrush(&thumb_brush)?;
            thumb.SetClip(&thumb_clip)?;
            parent.Children()?.InsertAtTop(&thumb)?;

            let base_w = p.px(3.0).max(2.0);
            let hover_w = p.px(5.5).max(4.0);
            let hit_w = p.px(14.0);
            let hit_x = sb_right_x - hit_w + p.px(2.5);
            let thumb_h = ((vh / content_h.max(1.0)) * sb_h).clamp(p.px(18.0), sb_h);

            Ok(ScrollBarLive {
                right_x: sb_right_x,
                track_y: sb_top_y,
                track_h: sb_h,
                base_w,
                hover_w,
                hit_x,
                hit_w,
                thumb_h,
                thumb_y: sb_top_y,
                track,
                track_geo,
                track_brush,
                thumb,
                thumb_geo,
                thumb_brush,
            })
        };
        build().ok()
    } else {
        None
    };
    let mut sv = ScrollViewLive {
        vx,
        vy,
        vw,
        vh,
        max_scroll,
        scroll_px: 0.0,
        content,
        bar,
        regions,
    };
    sv.apply(scroll_px, hover_t, dragging, active_t);
    sv
}

fn alerts_tab(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    use nacre_platform::notifications::Status;
    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(12.0);
    let bot = f.h - p.px(DOTS_STRIP_DP + 4.0);
    let h = (bot - top).max(p.px(40.0));
    let (x0, w) = (f.x0 + pad, f.w - 2.0 * pad);
    let accent = crate::prefs::accent();

    if d.notif_list.is_empty() {
        p.text(c, tr("Notifikasi"), x0, top, 14.0, 700, WHITE, 0.96, Anchor::Left, w * 0.6);
        let cx = x0 + w / 2.0;
        let cy = top + h / 2.0 + p.px(4.0);
        p.sym(c, Sym::Bell, cx, cy - p.px(18.0), 20.0, WHITE, 0.34);
        p.text(c, tr("Belum ada notifikasi"), cx, cy + p.px(4.0), 12.5, 600, WHITE, 0.88, Anchor::Center, w - p.px(8.0));
        let sub = match d.notif_status {
            Status::Denied => tr("Aktifkan izin notifikasi untuk NOX di Pengaturan Privasi Windows."),
            Status::Unsupported => tr("Listener notifikasi tidak tersedia pada sesi Windows ini."),
            _ => tr("Notifikasi baru dari aplikasi Windows akan muncul di sini."),
        };
        p.text(c, &p.fit(sub, 10.5, 400, w - p.px(8.0)), cx, cy + p.px(22.0), 10.5, 400, WHITE, 0.46, Anchor::Center, w);
        return;
    }

    let hdr = format!("{} ({})", tr("Notifikasi"), d.notif_list.len());
    p.text(c, &hdr, x0, top, 13.5, 700, WHITE, 0.96, Anchor::Left, w * 0.6);
    let clear = tr("Kosongkan");
    let cw = p.measure(clear, 11.0, 600) + p.px(4.0);
    let clr_x = x0 + w;
    p.text(c, clear, clr_x, top + p.px(2.0), 11.0, 600, accent, 0.95, Anchor::Right, cw + 4.0);
    regions.push(Region { x: clr_x - cw - p.px(6.0), y: top - p.px(2.0), w: cw + p.px(12.0), h: p.px(24.0), action: Action::NotifClearAll, vis: None, hover_bg: None });

    let row_h = p.px(38.0);
    let gap = p.px(4.0);
    let list_top = top + p.px(26.0);
    let list_h = (bot - list_top).max(row_h);
    let n_items = d.notif_list.len();
    let content_h = if n_items == 0 {
        0.0
    } else {
        n_items as f32 * row_h + (n_items.saturating_sub(1)) as f32 * gap + p.px(2.0)
    };
    let will_scroll = content_h > list_h + 1.0;
    let row_w = if will_scroll { w - p.px(9.0) } else { w };
    let Ok(sc) = create_scroll_viewport(p, c, x0, list_top, w, list_h) else { return };
    let mut sc_regions = Vec::new();
    let isz = p.px(22.0);
    for (idx, n) in d.notif_list.iter().enumerate() {
        let ry = idx as f32 * (row_h + gap);
        let n0 = kids(&sc);
        p.rect(&sc, 0.0, ry, row_w, row_h, p.px(11.0), white(if n.unread { 18 } else { 9 }));
        let ix = p.px(9.0);
        let icy = ry + row_h / 2.0;
        match n.icon {
            Some(s) => {
                p.image(&sc, s, ix, icy - isz / 2.0, isz, isz, p.px(5.5));
            }
            None => {
                p.rect(&sc, ix, icy - isz / 2.0, isz, isz, p.px(5.5), Color { A: 200, R: accent[0], G: accent[1], B: accent[2] });
                p.sym(&sc, Sym::Bell, ix + isz / 2.0, icy, 11.0, WHITE, 0.95);
            }
        }
        let close_cx = row_w - p.px(14.0);
        let time_x = close_cx - p.px(12.0);
        let tx = ix + isz + p.px(9.0);
        let tw = (time_x - p.px(32.0) - tx).max(p.px(40.0));
        let head = if n.app_name.is_empty() || n.title.eq_ignore_ascii_case(n.app_name) {
            n.title.to_string()
        } else {
            format!("{} \u{00B7} {}", n.app_name, n.title)
        };
        p.text(&sc, &p.fit(&head, 11.5, 600, tw), tx, ry + p.px(5.0), 11.5, 600, WHITE, 0.96, Anchor::Left, tw + p.px(2.0));
        if !n.body.is_empty() {
            p.text(&sc, &p.fit(n.body, 10.5, 400, tw), tx, ry + p.px(20.0), 10.5, 400, WHITE, 0.56, Anchor::Left, tw + p.px(2.0));
        }
        p.text(&sc, &n.time, time_x, ry + p.px(6.0), 9.5, 500, WHITE, 0.44, Anchor::Right, p.px(36.0));
        let row_vis = group_since(&sc, n0);
        let c0 = kids(&sc);
        p.sym(&sc, Sym::Close, close_cx, icy, 8.5, WHITE, 0.48);
        sc_regions.push(Region { x: close_cx - p.px(12.0), y: ry, w: p.px(24.0), h: row_h, action: Action::NotifDismiss(n.id), vis: group_since(&sc, c0), hover_bg: None });
        sc_regions.push(Region { x: 0.0, y: ry, w: close_cx - p.px(12.0), h: row_h, action: Action::NotifFocus(n.id), vis: row_vis, hover_bg: None });
    }
    live.scroll = Some(finish_scroll_view(
        p,
        c,
        x0,
        list_top,
        w,
        list_h,
        sc,
        content_h,
        x0 + w,
        list_top + p.px(2.0),
        (list_h - p.px(4.0)).max(p.px(14.0)),
        d.notif_scroll,
        d.sb_hover,
        d.sb_dragging,
        d.sb_active,
        sc_regions,
    ));
}

fn compact_shelf(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data) {
    let cy = f.h / 2.0;
    p.sym(c, Sym::Tray, f.x0 + p.px(22.0), cy, 15.0, WHITE, 0.9);
    let th = p.px(12.5) * 1.3;
    p.text(c, &count_text(d.shelf.len()), f.x0 + f.w - p.px(16.0), cy - th / 2.0, 12.5, 600, WHITE, 0.95, Anchor::Right, f.w / 2.0);
}

fn drop_zones(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    let (pad, gap) = (p.px(14.0), p.px(10.0));
    let all = [(0usize, Sym::Tray, "Simpan ke Rak"), (1, Sym::ArrowUp, "Kirim"), (2, Sym::Folder, "Salin ke\u{2026}"), (3, Sym::Grid, "Buka dengan\u{2026}")];
    let zones: Vec<_> = all.into_iter().filter(|z| z.0 != 1 || d.send_zone).collect();
    let n = zones.len() as f32;
    let zw = (f.w - 2.0 * pad - (n - 1.0) * gap) / n;
    let (top, zh) = (pad, f.h - 2.0 * pad);
    let accent = crate::prefs::accent();
    for (k, (i, s, label)) in zones.iter().enumerate() {
        let i = *i;
        let x = f.x0 + pad + k as f32 * (zw + gap);
        let on = d.drop_zone == Some(i);
        let fill = if on { Color { A: 230, R: accent[0], G: accent[1], B: accent[2] } } else { white(20) };
        if let (Ok(bg), Ok(brush), Ok(fg)) = (
            p.comp.CreateSpriteVisual(),
            p.comp.CreateColorBrushWithColor(fill),
            p.comp.CreateContainerVisual(),
        ) {
            let _ = bg.SetSize(Vector2 { X: zw, Y: zh });
            let _ = bg.SetOffset(Vector3 { X: x, Y: top, Z: 0.0 });
            let _ = bg.SetBrush(&brush);
            if let Ok(cl) = super::draw::clip(&p.comp, zw, zh, p.px(16.0)) {
                let _ = bg.SetClip(&cl);
            }
            let _ = c.Children().and_then(|k| k.InsertAtTop(&bg));
            let _ = fg.SetOpacity(if on { 1.0 } else { 0.85 });
            let _ = c.Children().and_then(|k| k.InsertAtTop(&fg));
            p.sym(&fg, *s, x + zw / 2.0, top + zh / 2.0 - p.px(12.0), 22.0, WHITE, 1.0);
            let th = p.px(12.5) * 1.3;
            p.text(&fg, tr(label), x + zw / 2.0, top + zh / 2.0 + p.px(10.0) - th / 2.0 + p.px(6.0), 12.5, 600, WHITE, 1.0, Anchor::Center, zw - p.px(12.0));
            live.drop_zones.push(DropZoneLive { id: i, brush, fg });
        }
        regions.push(Region { x, y: top, w: zw, h: zh, action: Action::Zone(i), vis: None, hover_bg: None });
    }
    if d.drop_count > 0 {
        let th = p.px(10.5) * 1.3;
        p.text(c, &count_text(d.drop_count), f.x0 + f.w / 2.0, f.h - pad / 2.0 - th / 2.0 - p.px(1.0), 10.5, 500, WHITE, 0.55, Anchor::Center, f.w);
    }
}

fn send_card(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>) {
    let Some(sv) = &d.send else { return };
    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(12.0);
    let h = (f.h - p.px(DOTS_STRIP_DP + 6.0) - top).max(p.px(74.0));
    let (x0, w) = (f.x0 + pad, f.w - 2.0 * pad);
    let s = (h - p.px(6.0)).min(p.px(108.0));
    let (qx, qy) = (x0, top + (h - s) / 2.0);
    if sv.error {
        p.rect(c, qx, qy, s, s, p.px(14.0), white(16));
        p.sym(c, Sym::Wifi, qx + s / 2.0, qy + s / 2.0, 26.0, WHITE, 0.45);
    } else {
        p.rect(c, qx, qy, s, s, p.px(14.0), Color { A: 255, R: 255, G: 255, B: 255 });
        if let Some(q) = sv.qr {
            let inset = ((s - sv.qr_px) / 2.0).round();
            p.place(c, q, qx + inset, qy + inset, sv.qr_px, sv.qr_px, 1.0);
        }
    }
    let tx = qx + s + p.px(14.0);
    let tw = (x0 + w - tx).max(p.px(80.0));
    let accent = crate::prefs::accent();
    if sv.error {
        p.text(c, tr("Tidak tersambung ke jaringan lokal"), tx, top + p.px(10.0), 13.0, 700, WHITE, 1.0, Anchor::Left, tw);
        p.text(c, tr("Sambungkan laptop ke Wi-Fi yang sama dengan HP."), tx, top + p.px(32.0), 11.0, 500, WHITE, 0.56, Anchor::Left, tw);
    } else {
        p.text(c, &p.fit(&sv.title, 13.0, 700, tw), tx, top + p.px(6.0), 13.0, 700, WHITE, 1.0, Anchor::Left, tw + p.px(4.0));
        p.text(c, &p.fit(tr("Pindai QR dengan kamera HP (Wi-Fi yang sama)"), 10.5, 500, tw), tx, top + p.px(25.0), 10.5, 500, WHITE, 0.56, Anchor::Left, tw + p.px(4.0));
        let green = Color { A: 255, R: GREEN[0], G: GREEN[1], B: GREEN[2] };
        p.text(c, &sv.status, tx, top + p.px(45.0), 11.5, 600, if sv.done { GREEN } else { WHITE }, 0.95, Anchor::Left, tw);
        let (by, bh) = (top + p.px(66.0), p.px(4.0));
        let bar = (tw - p.measure(tr("Selesai"), 11.5, 600) - p.px(30.0)).max(p.px(36.0));
        p.rect(c, tx, by, bar, bh, bh / 2.0, white(30));
        if sv.frac > 0.0 {
            p.rect(c, tx, by, (bar * sv.frac.clamp(0.0, 1.0)).max(bh), bh, bh / 2.0, green);
        }
    }
    let label = tr("Selesai");
    let (bw, bh) = (p.measure(label, 11.5, 600) + p.px(22.0), p.px(24.0));
    let (bx, by) = (x0 + w - bw, top + h - bh - p.px(2.0));
    p.rect(c, bx, by, bw, bh, bh / 2.0, Color { A: 255, R: accent[0], G: accent[1], B: accent[2] });
    let th = p.px(11.5) * 1.3;
    p.text(c, label, bx + bw / 2.0, by + bh / 2.0 - th / 2.0, 11.5, 600, WHITE, 1.0, Anchor::Center, bw);
    regions.push(Region { x: bx, y: by, w: bw, h: bh, action: Action::SendDone, vis: None, hover_bg: None });
}

fn shelf_tab(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>) {
    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(12.0);
    let (x0, w) = (f.x0 + pad, f.w - 2.0 * pad);
    let hdr = format!("{} \u{00B7} {}", tr("Rak"), count_text(d.shelf.len()));
    p.text(c, &hdr, x0, top, 14.0, 700, WHITE, 0.96, Anchor::Left, w * 0.65);

    let clear = tr("Kosongkan");
    let cw = p.measure(clear, 11.0, 600) + p.px(4.0);
    let accent = crate::prefs::accent();
    let clr_x = x0 + w;
    p.text(c, clear, clr_x, top + p.px(2.0), 11.0, 600, accent, 0.95, Anchor::Right, cw + 4.0);
    regions.push(Region { x: clr_x - cw - p.px(6.0), y: top - p.px(2.0), w: cw + p.px(12.0), h: p.px(24.0), action: Action::ShelfClear, vis: None, hover_bg: None });

    let (tw, th, gap) = (p.px(82.0), p.px(78.0), p.px(7.0));
    let ty = top + p.px(28.0);
    let max = ((w + gap) / (tw + gap)).floor().max(1.0) as usize;
    let extra = d.shelf.len().saturating_sub(max);
    for (i, (name, icon)) in d.shelf.iter().take(max).enumerate() {
        let x = x0 + i as f32 * (tw + gap);
        if extra > 0 && i == max - 1 {
            p.rect(c, x, ty, tw, th, p.px(13.0), white(12));
            let t = p.px(12.5) * 1.3;
            p.text(c, &format!("+{}", extra + 1), x + tw / 2.0, ty + th / 2.0 - t / 2.0, 12.5, 600, WHITE, 0.72, Anchor::Center, tw);
            break;
        }
        let n0 = kids(c);
        p.rect(c, x, ty, tw, th, p.px(13.0), CARD_DARK);
        let isz = p.px(36.0);
        match icon {
            Some(s) => {
                p.image(c, s, x + (tw - isz) / 2.0, ty + p.px(9.0), isz, isz, p.px(6.0));
            }
            None => p.sym(c, Sym::Folder, x + tw / 2.0, ty + p.px(27.0), 24.0, WHITE, 0.7),
        }
        let shown = p.fit(name, 10.5, 500, tw - p.px(10.0));
        p.text(c, &shown, x + tw / 2.0, ty + p.px(52.0), 10.5, 500, WHITE, 0.9, Anchor::Center, tw - p.px(6.0));
        regions.push(Region { x, y: ty, w: tw, h: th, action: Action::ShelfItem(i), vis: group_since(c, n0), hover_bg: None });
    }
}

fn shelf_empty(p: &mut Painter, c: &ContainerVisual, f: Frame) {
    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(11.0);
    let (x0, w) = (f.x0 + pad, f.w - 2.0 * pad);
    p.text(c, tr("Rak"), x0 + p.px(4.0), top, 14.5, 700, WHITE, 0.98, Anchor::Left, w * 0.5);

    let zy = top + p.px(27.0);
    let zh = (f.h - p.px(DOTS_STRIP_DP + 6.0) - zy).max(p.px(60.0));
    p.dashed_rect(c, x0, zy, w, zh, p.px(14.0), white(54));

    let cx = x0 + w / 2.0;
    let cy = zy + zh / 2.0;
    p.sym(c, Sym::Tray, cx, cy - p.px(15.0), 21.0, WHITE, 0.84);
    p.text(c, tr("Taruh berkas di sini"), cx, cy + p.px(2.0), 12.5, 600, WHITE, 0.96, Anchor::Center, w - p.px(14.0));
    p.text(c, tr("Seret keluar kapan saja · klik untuk buka"), cx, cy + p.px(19.0), 10.0, 400, WHITE, 0.46, Anchor::Center, w - p.px(14.0));
}

fn compact_timer(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, urgent: bool) {
    let cy = f.h / 2.0;
    let base_rgb = crate::prefs::accent();
    let rgb = if urgent { lift_for_dark_pill(base_rgb) } else { WHITE };
    if let Some(glow) = p.ambient_glow(c, f.x0, 0.0, f.w, f.h, 0.0, base_rgb, true) {
        let _ = glow.SetOpacity(if urgent { 0.9 } else { 0.55 });
    }
    p.sym(c, Sym::Stopwatch, f.x0 + p.px(20.0), cy, 15.0, lift_for_dark_pill(base_rgb), 0.96);
    let t = format_mmss(d.timer.remaining_secs(d.now_ms));
    let th = p.px(13.0) * 1.3;
    p.text(c, &t, f.x0 + f.w - p.px(16.0), cy - th / 2.0, 13.0, 600, rgb, 1.0, Anchor::Right, f.w);
}

/// Single source of truth for adaptive media color across Compact Live-Lyrics, Compact Music,
/// Compact Player, Expanded Now Playing, and the adjacent Detached Timer Pill.
/// Prefers extracted album art tint; falls back deterministically per track so every view always matches.
fn media_tint(m: &MediaView) -> [u8; 3] {
    if let Some(rgb) = m.tint {
        return rgb;
    }
    if m.title.is_empty() && m.artist.is_empty() {
        return crate::prefs::accent();
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in m.title.as_bytes().iter().chain(b"\x1f").chain(m.artist.as_bytes()) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    TRACK_PALETTE[(h as usize) % TRACK_PALETTE.len()]
}

fn glide_scalar(
    comp: &windows::UI::Composition::Compositor,
    obj: &windows::UI::Composition::CompositionObject,
    prop: &str,
    from: f32,
    to: f32,
    ms: u32,
    delay: u32,
) {
    use windows::Foundation::TimeSpan;
    use windows::UI::Composition::{AnimationDelayBehavior, CompositionEasingFunction};
    use windows_core::{Interface, HSTRING};
    let go = || -> windows_core::Result<()> {
        let ease: CompositionEasingFunction = comp
            .CreateCubicBezierEasingFunction(Vector2 { X: 0.22, Y: 1.0 }, Vector2 { X: 0.36, Y: 1.0 })?
            .cast()?;
        let a = comp.CreateScalarKeyFrameAnimation()?;
        a.InsertKeyFrame(0.0, from)?;
        a.InsertKeyFrameWithEasingFunction(1.0, to, &ease)?;
        a.SetDuration(TimeSpan { Duration: ms as i64 * 10_000 })?;
        if delay > 0 {
            a.SetDelayTime(TimeSpan { Duration: delay as i64 * 10_000 })?;
            a.SetDelayBehavior(AnimationDelayBehavior::SetInitialValueBeforeDelay)?;
        }
        obj.StartAnimation(&HSTRING::from(prop), &a)
    };
    let _ = go();
}

fn current_line(d: &Data) -> String {
    match d.lyrics.lines {
        Some(l) => l.current(d.lyrics.pos_ms).unwrap_or("\u{266A}").to_string(),
        None => String::new(),
    }
}

fn build_compact_lyric_row(
    p: &mut Painter,
    vp: &ContainerVisual,
    line: &str,
    vw: f32,
    h: f32,
    animate_in: bool,
) -> Option<(ContainerVisual, f32, bool, Option<MarqueeLive>)> {
    use windows::UI::Composition::CompositionObject;
    use windows_core::Interface;
    let full_w = p.measure_dwrite(line, 13.0, 600).max(p.measure(line, 13.0, 600));
    let cy = h / 2.0;
    let is_marquee = full_w > vw + p.px(2.0);
    let max_w = if is_marquee { full_w + p.px(12.0) } else { vw + p.px(6.0) };
    let (surf, tw, th) = p.render_text_surface(line, 13.0, 600, WHITE, max_w)?;
    let tw_f = tw as f32;
    let th_f = th as f32;
    let ty = (cy - th_f / 2.0).round();

    let outer = p.comp.CreateContainerVisual().ok()?;
    let x_init = if is_marquee { 0.0 } else { ((vw - tw_f) / 2.0).max(0.0) };
    let _ = outer.SetOffset(Vector3 { X: x_init, Y: 0.0, Z: 0.0 });
    let _ = vp.Children().and_then(|k| k.InsertAtTop(&outer));

    let inner = p.comp.CreateContainerVisual().ok()?;
    let _ = inner.SetCenterPoint(Vector3 { X: (tw_f / 2.0).min(vw / 2.0), Y: cy, Z: 0.0 });
    let _ = outer.Children().and_then(|k| k.InsertAtTop(&inner));

    let mut mq = None;
    if let Some(v) = p.place(&inner, &surf, 0.0, ty, tw_f, th_f, 1.0) {
        if is_marquee {
            let travel = (tw_f - vw).max(0.0);
            if travel > 2.0 {
                mq = Some(MarqueeLive { vis: v, travel, base_y: ty });
            }
        }
    }
    if animate_in {
        if let Ok(o) = inner.cast::<CompositionObject>() {
            let rise = p.px(11.0);
            glide_scalar(&p.comp, &o, "Offset.Y", rise, 0.0, 360, 45);
            glide_scalar(&p.comp, &o, "Opacity", 0.0, 1.0, 300, 45);
            glide_scalar(&p.comp, &o, "Scale.X", 0.94, 1.0, 360, 45);
            glide_scalar(&p.comp, &o, "Scale.Y", 0.94, 1.0, 360, 45);
        }
    }
    Some((outer, tw_f, is_marquee, mq))
}

fn compact_lyrics(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, live: &mut Live) {
    compact_music(p, c, f, d, live, f.h, false);
    let left_pad = p.px(44.0);
    let right_pad = if crate::prefs::get().visualizer == nacre_core::theme::VisStyle::Bars { p.px(50.0) } else { p.px(58.0) };
    let vw = (f.w - left_pad - right_pad).max(p.px(24.0));
    let Ok(vp) = p.comp.CreateContainerVisual() else { return };
    let _ = vp.SetOffset(Vector3 { X: f.x0 + left_pad, Y: 0.0, Z: 0.0 });
    let _ = vp.SetSize(Vector2 { X: vw, Y: f.h });
    if let Ok(cl) = p.comp.CreateInsetClip() {
        let _ = vp.SetClip(&cl);
    }
    let _ = c.Children().and_then(|k| k.InsertAtTop(&vp));

    let line = current_line(d);
    let mut active = None;
    let mut active_tw = 0.0;
    let mut active_marquee = false;
    if let Some((row, tw, is_mq, mq)) = build_compact_lyric_row(p, &vp, &line, vw, f.h, false) {
        active = Some(row);
        active_tw = tw;
        active_marquee = is_mq;
        live.marquee = mq;
    }
    live.compact_lyric = Some(CompactLyricLive {
        viewport: vp,
        active,
        ghost: None,
        active_tw,
        active_marquee,
        vw,
        h: f.h,
        left_pad,
        right_pad,
    });
}

pub fn swap_compact_lyric(p: &mut Painter, live: &mut Live, line: &str, target_w: f32) -> bool {
    use windows::UI::Composition::CompositionObject;
    use windows_core::Interface;
    let Some(cl) = live.compact_lyric.as_mut() else {
        return false;
    };
    let vp = cl.viewport.clone();
    let target_vw = (target_w - cl.left_pad - cl.right_pad).max(p.px(24.0));
    let h = cl.h;

    // Retire previous ghost cleanly.
    if let Some(old_ghost) = cl.ghost.take() {
        let _ = vp.Children().and_then(|k| k.Remove(&old_ghost));
    }
    // Smoothly float outgoing lyric up, scale slightly down, and dissolve.
    if let Some(prev) = cl.active.take() {
        if let Ok(o) = prev.cast::<CompositionObject>() {
            let cy = h / 2.0;
            let _ = prev.SetCenterPoint(Vector3 { X: (cl.active_tw / 2.0).min(cl.vw / 2.0), Y: cy, Z: 0.0 });
            let rise = p.px(11.0);
            glide_scalar(&p.comp, &o, "Offset.Y", 0.0, -rise, 270, 0);
            glide_scalar(&p.comp, &o, "Opacity", 1.0, 0.0, 230, 0);
            glide_scalar(&p.comp, &o, "Scale.X", 1.0, 0.94, 270, 0);
            glide_scalar(&p.comp, &o, "Scale.Y", 1.0, 0.94, 270, 0);
        }
        cl.ghost = Some(prev);
    }

    live.marquee = None;
    if let Some((row, tw, is_mq, mq)) = build_compact_lyric_row(p, &vp, line, target_vw, h, true) {
        if !is_mq {
            let _ = row.SetOffset(Vector3 { X: ((cl.vw - tw) / 2.0).max(0.0), Y: 0.0, Z: 0.0 });
        }
        cl.active = Some(row);
        cl.active_tw = tw;
        cl.active_marquee = is_mq;
        live.marquee = mq;
    }
    true
}

fn compact_player(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    let Some(m) = &d.media else { return };
    let glow = media_tint(m);
    p.ambient_glow(c, f.x0, 0.0, f.w, f.h, 0.0, glow, false);
    let a = p.px(52.0);
    let (ax, ay) = (f.x0 + p.px(22.0), p.px(10.0));
    match m.art {
        Some(s) => {
            p.image(c, s, ax, ay, a, a, p.px(12.0));
        }
        None => {
            p.rect(c, ax, ay, a, a, p.px(12.0), white(26));
            p.sym(c, Sym::Note, ax + a / 2.0, ay + a / 2.0, 20.0, WHITE, 0.7);
        }
    }
    let (cx0, cy, step, hit) = (ax + a / 2.0, ay + a + p.px(17.0), p.px(24.0), p.px(24.0));
    let buttons = [(Action::Prev, Sym::Backward, 12.0), (Action::PlayPause, if m.playing { Sym::Pause } else { Sym::Play }, 14.0), (Action::Next, Sym::Forward, 12.0)];
    for (i, (act, sym, size)) in buttons.into_iter().enumerate() {
        let cx = cx0 + (i as f32 - 1.0) * step;
        let n0 = kids(c);
        p.sym(c, sym, cx, cy, size, WHITE, 0.95);
        regions.push(Region { x: cx - hit / 2.0, y: cy - hit / 2.0, w: hit, h: hit, action: act, vis: group_since(c, n0), hover_bg: None });
    }
    let div_x = ax + a + p.px(16.0);
    p.divider_v(c, div_x, p.px(14.0), f.h - p.px(28.0));
    let lx = div_x + p.px(16.0);
    let lw = f.x0 + f.w - p.px(22.0) - lx;
    let synced = d.lyrics.lines.is_some_and(|l| l.synced);
    if synced {
        lyrics_slot(p, c, lx, p.px(6.0), lw, f.h - p.px(12.0), d, live, true);
    } else {
        let th = p.px(15.5) * 1.3;
        let cy = f.h / 2.0;
        p.text(c, &p.fit(m.title, 15.5, 700, lw), lx, cy - th, 15.5, 700, WHITE, 1.0, Anchor::Left, lw + p.px(4.0));
        p.text(c, &p.fit(m.artist, 12.5, 500, lw), lx, cy + p.px(2.0), 12.5, 500, WHITE, 0.58, Anchor::Left, lw + p.px(4.0));
    }
}

fn compact_music(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, live: &mut Live, row_h: f32, show_marquee: bool) {
    let Some(m) = &d.media else { return };
    let is_lyrics = !show_marquee;
    let rgb = media_tint(m);
    let prefs = crate::prefs::get();
    live.compact_glow = if prefs.notch_glow && !prefs.reduce_motion {
        p.ambient_glow(c, f.x0, 0.0, f.w, row_h, 0.0, rgb, true)
    } else {
        None
    };

    let cy = row_h / 2.0;
    let a = if is_lyrics { p.px(23.0) } else { p.px(20.0) };
    let art_pad = if is_lyrics { p.px(11.0) } else { p.px(12.0) };

    if let Ok(left_c) = p.comp.CreateContainerVisual() {
        let _ = left_c.SetOffset(Vector3 { X: f.x0.round(), Y: 0.0, Z: 0.0 });
        let _ = c.Children().and_then(|k| k.InsertAtTop(&left_c));
        match m.art {
            Some(s) => {
                p.image(&left_c, s, art_pad, cy - a / 2.0, a, a, p.px(6.0));
            }
            None => {
                p.rect(&left_c, art_pad, cy - a / 2.0, a, a, p.px(6.0), white(30));
                p.sym(&left_c, Sym::Note, art_pad + a / 2.0, cy, 12.0, WHITE, 0.85);
            }
        }
        live.compact_left = Some(left_c);
    }

    let style = prefs.visualizer;
    let square = prefs.preset == nacre_core::theme::Preset::Pixel;
    live.vis = style;
    let n = style.count();
    let (bw, gap, bh) = match style {
        nacre_core::theme::VisStyle::Bars if is_lyrics => (p.px(2.8), p.px(2.6), p.px(14.0)),
        nacre_core::theme::VisStyle::Bars => (p.px(3.0), p.px(3.0), p.px(13.0)),
        nacre_core::theme::VisStyle::Spectrum => (p.px(1.6), p.px(0.8), p.px(14.0)),
        nacre_core::theme::VisStyle::Wave => (p.px(1.0), p.px(0.5), p.px(15.0)),
        nacre_core::theme::VisStyle::Dots => (p.px(3.6), p.px(1.2), p.px(3.6)),
    };
    let vis_w = n as f32 * bw + (n.saturating_sub(1)) as f32 * gap;
    let right_pad = if is_lyrics { p.px(13.0) } else { p.px(14.0) };

    if show_marquee {
        let left = f.x0 + p.px(40.0);
        let vw = (f.x0 + f.w - right_pad - vis_w - p.px(10.0) - left).max(p.px(20.0));
        let label = if m.artist.is_empty() {
            m.title.to_string()
        } else {
            format!("{} \u{00B7} {}", m.title, m.artist)
        };
        if let (Ok(vp), Ok(cl)) = (p.comp.CreateContainerVisual(), p.comp.CreateInsetClip()) {
            let _ = vp.SetOffset(Vector3 { X: left.round(), Y: 0.0, Z: 0.0 });
            let _ = vp.SetSize(Vector2 { X: vw.ceil(), Y: row_h });
            let _ = vp.SetClip(&cl);
            let _ = c.Children().and_then(|k| k.InsertAtTop(&vp));
            let full_w = p.measure_dwrite(&label, 12.0, 600).max(p.measure(&label, 12.0, 600));
            if let Some((surf, tw, th)) = p.render_text_surface(&label, 12.0, 600, WHITE, full_w.max(vw) + p.px(8.0)) {
                let ty = (cy - th as f32 / 2.0).round();
                let travel = (tw as f32 - vw).max(0.0);
                let x_init = if travel <= 0.5 { ((vw - tw as f32) / 2.0).round() } else { 0.0 };
                if let Some(v) = p.place(&vp, &surf, x_init, ty, tw as f32, th as f32, 0.96) {
                    if travel > 2.0 {
                        live.marquee = Some(MarqueeLive { vis: v, travel, base_y: ty });
                    }
                }
            }
        }
    }

    if let Ok(right_c) = p.comp.CreateContainerVisual() {
        let _ = right_c.SetOffset(Vector3 { X: (f.x0 + f.w).round(), Y: 0.0, Z: 0.0 });
        let _ = c.Children().and_then(|k| k.InsertAtTop(&right_c));
        let bx0 = -(right_pad + vis_w);
        let radius = if square { 0.0 } else { bw / 2.0 };
        let start = if style == nacre_core::theme::VisStyle::Dots {
            Vector3 { X: 0.7, Y: 0.7, Z: 1.0 }
        } else {
            Vector3 { X: 1.0, Y: 0.28, Z: 1.0 }
        };
        for i in 0..n {
            if let Some(v) = p.rect(&right_c, bx0 + i as f32 * (bw + gap), cy - bh / 2.0, bw, bh, radius, Color { A: 240, R: rgb[0], G: rgb[1], B: rgb[2] }) {
                let _ = v.SetCenterPoint(Vector3 { X: bw / 2.0, Y: bh / 2.0, Z: 0.0 });
                let _ = v.SetScale(start);
                live.bars.push(v);
            }
        }
        live.compact_right = Some(right_c);
    }
}

fn mmss_pad(ms: i64) -> String {
    let s = (ms.max(0) + 500) / 1000;
    format!("{:02}:{:02}", s / 60, s % 60)
}

#[allow(clippy::too_many_arguments)]
fn music_column(
    p: &mut Painter,
    c: &ContainerVisual,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    m: &MediaView,
    lp: f32,
    gliding: bool,
    muted: bool,
    scrub_hover: f32,
    art_scale: f32,
    regions: &mut Vec<Region>,
    live: &mut Live,
) -> Option<(ContainerVisual, Option<ContainerVisual>, f32, f32, Vec<(Action, f32, ContainerVisual)>)> {
    let left_group = p.comp.CreateContainerVisual().ok()?;
    let _ = c.Children().and_then(|k| k.InsertAtTop(&left_group));

    let art = p.px(58.0);
    let (ax, ay) = (x, y);
    if let Ok(art_wrap) = p.comp.CreateContainerVisual() {
        let _ = art_wrap.SetCenterPoint(Vector3 { X: ax + art / 2.0, Y: ay + art / 2.0, Z: 0.0 });
        let sc = art_scale.clamp(0.75, 1.15);
        let _ = art_wrap.SetScale(Vector3 { X: sc, Y: sc, Z: 1.0 });
        let art_op = (0.78 + 0.22 * ((sc - 0.90) / 0.10).clamp(0.0, 1.0)).clamp(0.76, 1.0);
        let _ = art_wrap.SetOpacity(art_op);
        let _ = left_group.Children().and_then(|k| k.InsertAtTop(&art_wrap));
        match m.art {
            Some(s) => {
                p.image(&art_wrap, s, ax, ay, art, art, p.px(13.0));
            }
            None => {
                p.rect(&art_wrap, ax, ay, art, art, p.px(13.0), white(24));
                p.sym(&art_wrap, Sym::Note, ax + art / 2.0, ay + art / 2.0, 24.0, WHITE, 0.8);
            }
        }
        live.album_art = Some(art_wrap);
    }
    let tx = ax + art + p.px(12.0);
    let tw_live = (x + w - tx - (1.0 - lp) * p.px(34.0)).max(p.px(40.0));
    let tw_wide = (p.px(395.0 - 2.0 * EDGE_PAD_DP - 8.0) - (art + p.px(12.0))).max(tw_live);
    let fit_w = if gliding { tw_wide } else { tw_live };
    let title_clip_h = art;
    let mut title_clip_vis = None;
    if let (Ok(tc), Ok(iclip)) = (p.comp.CreateContainerVisual(), p.comp.CreateInsetClip()) {
        let _ = tc.SetOffset(Vector3 { X: tx.round(), Y: ay.round(), Z: 0.0 });
        let _ = tc.SetSize(Vector2 { X: tw_live.ceil(), Y: title_clip_h });
        let _ = tc.SetClip(&iclip);
        let _ = left_group.Children().and_then(|k| k.InsertAtTop(&tc));
        let title = p.fit(m.title, 15.0, 700, fit_w);
        p.text(&tc, &title, 0.0, p.px(11.0), 15.0, 700, WHITE, 1.0, Anchor::Left, fit_w + 2.0);
        let artist = p.fit(m.artist, 12.0, 500, fit_w);
        p.text(&tc, &artist, 0.0, p.px(33.0), 12.0, 500, WHITE, 0.56, Anchor::Left, fit_w + 2.0);
        title_clip_vis = Some(tc);
    } else {
        let title = p.fit(m.title, 15.0, 700, tw_live);
        p.text(&left_group, &title, tx, ay + p.px(11.0), 15.0, 700, WHITE, 1.0, Anchor::Left, tw_live + 2.0);
        let artist = p.fit(m.artist, 12.0, 500, tw_live);
        p.text(&left_group, &artist, tx, ay + p.px(33.0), 12.0, 500, WHITE, 0.56, Anchor::Left, tw_live + 2.0);
    }

    // Scrub bar positioned closer above the transport controls (y ≈ 108dp compact / 116dp split).
    let sy = ay + art + p.px(36.0 + 8.0 * lp);
    let left_time_w = p.px(37.0);
    let right_time_w = p.px(43.0);
    let sx = x + left_time_w;
    let sw = (w - left_time_w - right_time_w).max(p.px(40.0));
    let base_th = p.px(4.0);
    let frac = m.drag.unwrap_or(if m.dur_ms > 0 { m.pos_ms as f32 / m.dur_ms as f32 } else { 0.0 }).clamp(0.0, 1.0);

    let pos = if let Some(fr) = m.drag { (m.dur_ms as f32 * fr) as i64 } else { m.pos_ms };
    let build_scrub = || -> windows_core::Result<ScrubLive> {
        let track_geo = p.comp.CreateRoundedRectangleGeometry()?;
        let track_clip = p.comp.CreateGeometricClip()?;
        track_clip.SetGeometry(&track_geo)?;
        let track = p.comp.CreateSpriteVisual()?;
        track.SetBrush(&p.comp.CreateColorBrushWithColor(white(58))?)?;
        track.SetClip(&track_clip)?;
        c.Children()?.InsertAtTop(&track)?;

        let fill_geo = p.comp.CreateRoundedRectangleGeometry()?;
        let fill_clip = p.comp.CreateGeometricClip()?;
        fill_clip.SetGeometry(&fill_geo)?;
        let fill = p.comp.CreateSpriteVisual()?;
        fill.SetBrush(&p.comp.CreateColorBrushWithColor(white(245))?)?;
        fill.SetClip(&fill_clip)?;
        c.Children()?.InsertAtTop(&fill)?;

        let knob_geo = p.comp.CreateRoundedRectangleGeometry()?;
        let knob_clip = p.comp.CreateGeometricClip()?;
        knob_clip.SetGeometry(&knob_geo)?;
        let knob_brush = p.comp.CreateColorBrushWithColor(white(248))?;
        let knob = p.comp.CreateSpriteVisual()?;
        knob.SetBrush(&knob_brush)?;
        knob.SetClip(&knob_clip)?;
        c.Children()?.InsertAtTop(&knob)?;

        let pos_vis = p.comp.CreateSpriteVisual()?;
        pos_vis.SetOpacity(0.88)?;
        c.Children()?.InsertAtTop(&pos_vis)?;
        let rem_vis = p.comp.CreateSpriteVisual()?;
        rem_vis.SetOpacity(0.88)?;
        c.Children()?.InsertAtTop(&rem_vis)?;

        Ok(ScrubLive {
            left_x: x,
            right_x: x + w,
            sx,
            sw,
            sy,
            base_th,
            dur_ms: m.dur_ms,
            track,
            track_geo,
            fill,
            fill_geo,
            knob,
            knob_geo,
            knob_brush,
            pos_vis,
            rem_vis,
            last_pos_sec: -1,
            last_pos_h: 0.0,
            last_rem_w: 0.0,
            last_rem_h: 0.0,
        })
    };
    if let Ok(mut sl) = build_scrub() {
        sl.update(p, frac, scrub_hover, m.drag.is_some(), pos);
        live.scrub = Some(sl);
    }

    if m.can_seek && m.dur_ms > 0 {
        regions.push(Region { x: sx - p.px(6.0), y: sy - p.px(12.0), w: sw + p.px(12.0), h: p.px(24.0), action: Action::Seek(sx, sw), vis: None, hover_bg: None });
    }

    // Unified tactile transport controls: continuous GPU scale & opacity interpolation across `lp`
    // so opening/closing lyrics never pops icon sizes, brightness, or vertical position.
    let cy = (y + h - p.px(18.0)).max(sy + p.px(28.0));
    let cx = x + w / 2.0;
    let vol_sym = if muted { Sym::SpeakerMute } else { Sym::SpeakerMid };
    let vol_op = if muted { 0.48 } else { 0.92 };
    let step = p.px(52.0 + (44.0 - 52.0) * lp);
    let btn_sc = 1.0 - 0.04 * lp;
    let controls = [
        (Sym::Quote, (x + p.px(24.0)) * (1.0 - lp) + (cx - 2.0 * step) * lp, 24.0, Action::ToggleLyrics, 0.56 + 0.44 * lp),
        (Sym::Backward, cx - step, 26.0, Action::Prev, 0.98 - 0.02 * lp),
        (if m.playing { Sym::Pause } else { Sym::Play }, cx, 30.5, Action::PlayPause, 1.0),
        (Sym::Forward, cx + step, 26.0, Action::Next, 0.98 - 0.02 * lp),
        (vol_sym, (x + w - p.px(24.0)) * (1.0 - lp) + (cx + 2.0 * step) * lp, 21.0, Action::ToggleMute, vol_op),
    ];
    let mut ctrl_groups = Vec::with_capacity(5);
    for (s, bx, size, a, op) in controls {
        if let Ok(wrap) = p.comp.CreateContainerVisual() {
            let _ = wrap.SetCenterPoint(Vector3 { X: bx, Y: cy, Z: 0.0 });
            let _ = wrap.SetScale(Vector3 { X: btn_sc, Y: btn_sc, Z: 1.0 });
            let _ = wrap.SetOpacity(op);
            let _ = c.Children().and_then(|k| k.InsertAtTop(&wrap));
            let n0 = kids(&wrap);
            p.sym(&wrap, s, bx, cy, size, WHITE, 1.0);
            regions.push(Region { x: bx - p.px(20.0), y: cy - p.px(18.0), w: p.px(40.0), h: p.px(36.0), action: a, vis: group_since(&wrap, n0), hover_bg: None });
            ctrl_groups.push((a, bx, wrap));
        }
    }
    Some((left_group, title_clip_vis, title_clip_h, cy, ctrl_groups))
}

fn compact_glance(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data) {
    let Some((item, text)) = &d.glance else { return };
    let cy = f.h / 2.0;
    let th = p.px(12.5) * 1.3;
    if *item == Item::Greeting {
        let max = f.w - p.px(32.0);
        let shown = p.fit(text, 12.5, 600, max);
        p.text(c, &shown, f.x0 + f.w / 2.0, cy - th / 2.0, 12.5, 600, WHITE, 0.95, Anchor::Center, max + p.px(2.0));
        return;
    }
    let left = f.x0 + p.px(14.0);
    let icon_w = match item {
        Item::Battery => {
            let (pct, ch) = d.battery.unwrap_or((0, false));
            p.battery(c, left, cy, pct, ch)
        }
        _ => {
            let sym = if *item == Item::Clock { Sym::Clock } else { Sym::Calendar };
            p.sym(c, sym, left + p.px(8.0), cy, 15.0, WHITE, 0.9);
            p.px(16.0)
        }
    };
    let right = f.x0 + f.w - p.px(16.0);
    let max = right - (left + icon_w + p.px(10.0));
    let shown = p.fit(text, 12.5, 600, max);
    p.text(c, &shown, right, cy - th / 2.0, 12.5, 600, WHITE, 0.95, Anchor::Right, max + p.px(2.0));
}

fn compact_download(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data) {
    let Some(dl) = &d.download else { return };
    let cy = f.h / 2.0;
    let a = p.px(22.0);
    let ix = f.x0 + p.px(14.0);
    match dl.icon {
        Some(s) => {
            p.image(c, s, ix, cy - a / 2.0, a, a, p.px(6.0));
        }
        None => {
            p.rect(c, ix, cy - a / 2.0, a, a, a / 2.0, white(32));
            p.sym(c, if dl.up { Sym::ArrowUp } else { Sym::ArrowDown }, ix + a / 2.0, cy, 11.0, WHITE, 0.9);
        }
    }
    let (r, w) = (11.0, 2.6);
    let gx = f.x0 + f.w - p.px(16.0) - p.px(r + w / 2.0);
    let gy = p.px(15.0);
    p.arc(c, gx, gy, r, w, 0.0, 1.0, WHITE, 56);
    match (dl.frac, dl.done) {
        (_, true) => {
            p.arc(c, gx, gy, r, w, 0.0, 1.0, GREEN, 255);
        }
        (Some(fr), false) => {
            if fr > 0.002 {
                p.arc(c, gx, gy, r, w, 0.0, fr.clamp(0.0, 1.0), GREEN, 255);
            }
        }
        (None, false) => {
            if let Some((v, n)) = p.arc(c, gx, gy, r, w, 0.0, 0.24, GREEN, 255) {
                spin(p, &v, n, d.now_ms);
            }
        }
    }
    p.sym(c, if dl.done { Sym::Check } else if dl.up { Sym::ArrowUp } else { Sym::ArrowDown }, gx, gy, 12.0, WHITE, 0.95);
    let th = p.px(9.5) * 1.3;
    p.text(c, &dl.label, gx, f.h - p.px(2.0) - th, 9.5, 600, WHITE, 0.95, Anchor::Center, p.px(40.0));
}

fn spin(p: &Painter, v: &SpriteVisual, n: f32, now_ms: i64) {
    use windows::Foundation::TimeSpan;
    use windows::UI::Composition::{AnimationIterationBehavior, CompositionEasingFunction};
    use windows_core::{Interface, HSTRING};
    const PERIOD: i64 = 1100;
    let go = || -> windows_core::Result<()> {
        let phase = (now_ms.rem_euclid(PERIOD)) as f32 / PERIOD as f32 * 360.0;
        v.SetCenterPoint(Vector3 { X: n / 2.0, Y: n / 2.0, Z: 0.0 })?;
        let lin: CompositionEasingFunction = p.comp.CreateLinearEasingFunction()?.cast()?;
        let a = p.comp.CreateScalarKeyFrameAnimation()?;
        a.InsertKeyFrame(0.0, phase)?;
        a.InsertKeyFrameWithEasingFunction(1.0, phase + 360.0, &lin)?;
        a.SetDuration(TimeSpan { Duration: PERIOD * 10_000 })?;
        a.SetIterationBehavior(AnimationIterationBehavior::Forever)?;
        v.StartAnimation(&HSTRING::from("RotationAngleInDegrees"), &a)
    };
    let _ = go();
}

fn compact_charging(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data) {
    let cy = f.h / 2.0;
    let th = p.px(13.0) * 1.3;
    p.text(c, tr("Mengisi daya"), f.x0 + p.px(16.0), cy - th / 2.0, 12.5, 500, WHITE, 0.85, Anchor::Left, f.w / 2.0);
    let pct = d.battery.map_or(String::new(), |(b, _)| format!("\u{26A1} {b}%"));
    p.text(c, &pct, f.x0 + f.w - p.px(16.0), cy - th / 2.0, 13.0, 600, GREEN, 1.0, Anchor::Right, f.w / 2.0);
}

fn todo_tab(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    use nacre_platform::gtasks::SyncStatus;
    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(11.0);
    let bot = f.h - p.px(DOTS_STRIP_DP + 4.0);
    let (x0, w) = (f.x0 + pad, f.w - 2.0 * pad);

    // Slimmer left column (116 dp) with vertically stacked pills -> wider right panel (~233 dp).
    let lw = p.px(116.0);
    let rx = x0 + lw + p.px(10.0);
    let rw = (x0 + w - rx).max(p.px(150.0));

    let cal_events = match &d.cal.events {
        Events::List(list) => Some(*list),
        _ => None,
    };
    let ev_count = cal_events.map_or(0, |l| l.len());
    let has_today_events = ev_count > 0;

    // Two-tone "TodoList" title matching Nusa reference.
    let todo_w = p.text(c, "Todo", x0 + p.px(2.0), top, 14.5, 700, WHITE, 0.98, Anchor::Left, p.px(56.0));
    p.text(
        c,
        "List",
        x0 + p.px(2.0) + todo_w - p.px(1.0),
        top,
        14.5,
        700,
        if d.todo_cal_mode { BLUE } else { GREEN },
        0.96,
        Anchor::Left,
        p.px(56.0),
    );

    // Big progress counter + circular Google Calendar toggle button (blue dot when events exist today).
    let ratio = if d.todo_cal_mode {
        format!("{ev_count}")
    } else {
        format!("{}/{}", d.todo_done, d.todo_total)
    };
    let num_w = p.text(c, &ratio, x0 + p.px(2.0), top + p.px(19.0), 28.0, 700, WHITE, 1.0, Anchor::Left, p.px(82.0));
    let cal_dia = p.px(25.0);
    let scx = (x0 + num_w + p.px(20.0)).min(x0 + lw - cal_dia / 2.0 - p.px(2.0));
    let scy = top + p.px(37.0);
    let n0 = kids(c);
    let cal_fill = if d.todo_cal_mode {
        Color { A: 225, R: BLUE[0], G: BLUE[1], B: BLUE[2] }
    } else {
        white(20)
    };
    p.rect(c, (scx - cal_dia / 2.0).round(), (scy - cal_dia / 2.0).round(), cal_dia, cal_dia, cal_dia / 2.0, cal_fill);
    p.sym(c, Sym::Calendar, scx, scy, 13.0, WHITE, if d.todo_cal_mode { 1.0 } else { 0.78 });
    if has_today_events || matches!(d.cal.events, Events::Loading) {
        let dot_rgb = if has_today_events { BLUE } else { ORANGE };
        let dot = p.px(6.0);
        p.rect(
            c,
            scx + p.px(5.5),
            scy - p.px(11.5),
            dot,
            dot,
            dot / 2.0,
            Color { A: 255, R: dot_rgb[0], G: dot_rgb[1], B: dot_rgb[2] },
        );
    }
    regions.push(Region {
        x: scx - cal_dia / 2.0,
        y: scy - cal_dia / 2.0,
        w: cal_dia,
        h: cal_dia,
        action: Action::TodoCalToggle,
        vis: group_since(c, n0),
        hover_bg: None,
    });

    if !d.todo_cal_mode {
        p.text(c, &p.fit(tr("Selesai hari ini"), 10.5, 500, lw), x0 + p.px(2.0), top + p.px(55.0), 10.5, 500, WHITE, 0.50, Anchor::Left, lw);
    } else {
        // Interactive Date Picker stepper: [ <  Today / 23 Nov  > ] for viewing & creating events on any date.
        use nacre_core::island::calendar::{add_days, month_short};
        let en = crate::prefs::get().lang == nacre_core::i18n::Lang::En;
        let (ty, tm, td) = d.cal.today;
        let (_, sm, sd) = d.cal.sel;
        let date_label = if d.cal.sel == d.cal.today {
            tr("Hari ini").to_string()
        } else if d.cal.sel == add_days(ty, tm, td, 1) {
            tr("Besok").to_string()
        } else if d.cal.sel == add_days(ty, tm, td, -1) {
            tr("Kemarin").to_string()
        } else if en {
            format!("{} {sd}", month_short(sm, true))
        } else {
            format!("{sd} {}", month_short(sm, false))
        };
        let dp_y = top + p.px(54.0);
        let dp_h = p.px(21.0);
        let nav_w = p.px(22.0);
        p.rect(c, x0, dp_y, lw, dp_h, dp_h / 2.0, white(16));
        let dcy = dp_y + dp_h / 2.0;

        let n_prev = kids(c);
        p.sym(c, Sym::ChevronLeft, x0 + nav_w / 2.0, dcy, 9.5, WHITE, 0.78);
        regions.push(Region {
            x: x0,
            y: dp_y - p.px(2.0),
            w: nav_w,
            h: dp_h + p.px(4.0),
            action: Action::CalDay(-1),
            vis: group_since(c, n_prev),
            hover_bg: None,
        });

        let n_next = kids(c);
        p.sym(c, Sym::ChevronRight, x0 + lw - nav_w / 2.0, dcy, 9.5, WHITE, 0.78);
        regions.push(Region {
            x: x0 + lw - nav_w,
            y: dp_y - p.px(2.0),
            w: nav_w,
            h: dp_h + p.px(4.0),
            action: Action::CalDay(1),
            vis: group_since(c, n_next),
            hover_bg: None,
        });

        let mid_w = (lw - 2.0 * nav_w).max(p.px(40.0));
        let dth = p.px(10.5) * 1.3;
        let is_today = d.cal.sel == d.cal.today;
        let n_mid = kids(c);
        p.text(
            c,
            &p.fit(&date_label, 10.5, 600, mid_w),
            x0 + lw / 2.0,
            dcy - dth / 2.0,
            10.5,
            600,
            if is_today { WHITE } else { BLUE },
            0.96,
            Anchor::Center,
            mid_w + p.px(2.0),
        );
        if !is_today {
            regions.push(Region {
                x: x0 + nav_w,
                y: dp_y,
                w: mid_w,
                h: dp_h,
                action: Action::CalToday,
                vis: group_since(c, n_mid),
                hover_bg: None,
            });
        }
    }

    // Vertically stacked pill buttons (uniform 116 x 26 dp, 6 dp gap).
    let btn_h = p.px(26.0);
    let btn_gap = p.px(6.0);
    let bw = lw;
    let btn2_y = (bot - btn_h).max(top + p.px(110.0));
    let btn1_y = btn2_y - btn_gap - btn_h;
    let th_btn = p.px(10.5) * 1.3;

    if !d.todo_cal_mode {
        // + Add Task pill (top)
        let n0 = kids(c);
        p.rect(c, x0, btn1_y, bw, btn_h, btn_h / 2.0, if d.todo_adding { white(36) } else { white(18) });
        p.sym(c, Sym::Plus, x0 + p.px(15.0), btn1_y + btn_h / 2.0, 11.0, WHITE, 0.95);
        p.text(c, &p.fit(tr("Tambah Tugas"), 10.5, 600, bw - p.px(28.0)), x0 + p.px(26.0), btn1_y + btn_h / 2.0 - th_btn / 2.0, 10.5, 600, WHITE, 0.94, Anchor::Left, bw - p.px(26.0));
        regions.push(Region { x: x0, y: btn1_y, w: bw, h: btn_h, action: Action::TodoAddOpen, vis: group_since(c, n0), hover_bg: None });

        // Edit Task pill (bottom)
        let n1 = kids(c);
        p.rect(c, x0, btn2_y, bw, btn_h, btn_h / 2.0, if d.todo_editing { white(36) } else { white(18) });
        p.sym(c, Sym::Pencil, x0 + p.px(15.0), btn2_y + btn_h / 2.0, 10.5, WHITE, 0.92);
        p.text(c, &p.fit(tr("Edit Tugas"), 10.5, 600, bw - p.px(28.0)), x0 + p.px(26.0), btn2_y + btn_h / 2.0 - th_btn / 2.0, 10.5, 600, WHITE, 0.94, Anchor::Left, bw - p.px(26.0));
        regions.push(Region { x: x0, y: btn2_y, w: bw, h: btn_h, action: Action::TodoEditToggle, vis: group_since(c, n1), hover_bg: None });
    } else {
        // + Add Schedule pill (top) -> creates Google Calendar event
        let n0 = kids(c);
        let add_fill = if d.todo_cal_adding {
            Color { A: 215, R: BLUE[0], G: BLUE[1], B: BLUE[2] }
        } else {
            white(18)
        };
        p.rect(c, x0, btn1_y, bw, btn_h, btn_h / 2.0, add_fill);
        p.sym(c, Sym::Plus, x0 + p.px(15.0), btn1_y + btn_h / 2.0, 11.0, WHITE, 0.95);
        p.text(c, &p.fit(tr("Tambah Jadwal"), 10.5, 600, bw - p.px(28.0)), x0 + p.px(26.0), btn1_y + btn_h / 2.0 - th_btn / 2.0, 10.5, 600, WHITE, 0.95, Anchor::Left, bw - p.px(26.0));
        regions.push(Region { x: x0, y: btn1_y, w: bw, h: btn_h, action: Action::CalAddOpen, vis: group_since(c, n0), hover_bg: None });

        // Bottom pill: Refresh (when connected) or Connect Google (when disconnected)
        let (sec_sym, sec_lbl, sec_act) = match d.todo_status {
            SyncStatus::Synced | SyncStatus::Syncing | SyncStatus::Connecting => {
                (Sym::Restart, tr("Muat Ulang"), Action::CalRefresh)
            }
            SyncStatus::NeedsClientConfig => (Sym::Gear, tr("Sambungkan"), Action::Settings),
            SyncStatus::Disconnected | SyncStatus::Error => (Sym::Calendar, tr("Sambungkan"), Action::TodoConnect),
        };
        let n1 = kids(c);
        p.rect(c, x0, btn2_y, bw, btn_h, btn_h / 2.0, white(18));
        p.sym(c, sec_sym, x0 + p.px(15.0), btn2_y + btn_h / 2.0, 10.5, WHITE, 0.92);
        p.text(c, &p.fit(sec_lbl, 10.5, 600, bw - p.px(28.0)), x0 + p.px(26.0), btn2_y + btn_h / 2.0 - th_btn / 2.0, 10.5, 600, WHITE, 0.94, Anchor::Left, bw - p.px(26.0));
        regions.push(Region { x: x0, y: btn2_y, w: bw, h: btn_h, action: sec_act, vis: group_since(c, n1), hover_bg: None });
    }

    // Right rounded #141416 container (Checklist Tasks OR Today's Google Calendar Schedule)
    let card_y = top + p.px(24.0);
    let card_h = (bot - card_y).max(p.px(64.0));
    p.rect(c, rx, card_y, rw, card_h, p.px(15.0), CARD_DARK);

    let mut list_top = card_y + p.px(8.0);
    if (!d.todo_cal_mode && d.todo_adding) || (d.todo_cal_mode && d.todo_cal_adding) {
        let (ix, iy, iw, ih) = (rx + p.px(7.0), card_y + p.px(7.0), rw - p.px(14.0), p.px(26.0));
        live.todo_input_box = Some((ix, iy, iw, ih));
        regions.push(Region { x: ix, y: iy, w: iw, h: ih, action: Action::TodoFocus, vis: None, hover_bg: None });
        list_top = iy + ih + p.px(5.0);
    }

    if d.todo_cal_mode {
        if !d.todo_cal_err.is_empty() {
            let err_h = p.px(18.0);
            p.text(
                c,
                &p.fit(d.todo_cal_err, 9.5, 600, rw - p.px(18.0)),
                rx + p.px(10.0),
                list_top,
                9.5,
                600,
                [255, 130, 110],
                0.95,
                Anchor::Left,
                rw - p.px(16.0),
            );
            regions.push(Region {
                x: rx,
                y: list_top - p.px(2.0),
                w: rw,
                h: err_h + p.px(4.0),
                action: Action::Settings,
                vis: None,
                hover_bg: None,
            });
            list_top += err_h + p.px(2.0);
        }

        match &d.cal.events {
            Events::Loading if !d.todo_cal_adding => {
                let cx = rx + rw / 2.0;
                let cy = card_y + card_h / 2.0;
                p.text(c, tr("Memuat jadwal…"), cx, cy - p.px(7.0), 11.0, 500, WHITE, 0.48, Anchor::Center, rw - p.px(16.0));
            }
            Events::Unavailable if !d.todo_cal_adding => {
                let cx = rx + rw / 2.0;
                let cy = card_y + card_h / 2.0;
                p.text(c, &p.fit(tr("Sambungkan Google di Pengaturan"), 10.5, 500, rw - p.px(18.0)), cx, cy - p.px(7.0), 10.5, 500, WHITE, 0.52, Anchor::Center, rw - p.px(14.0));
                regions.push(Region { x: rx, y: card_y, w: rw, h: card_h, action: Action::Settings, vis: None, hover_bg: None });
            }
            Events::List([]) if !d.todo_cal_adding => {
                let cx = rx + rw / 2.0;
                let cy = card_y + card_h / 2.0;
                let empty_msg = if d.cal.sel == d.cal.today {
                    tr("Tidak ada acara hari ini")
                } else {
                    tr("Tidak ada acara")
                };
                p.text(c, empty_msg, cx, cy - p.px(7.0), 11.5, 500, WHITE, 0.45, Anchor::Center, rw - p.px(14.0));
                regions.push(Region { x: rx, y: card_y, w: rw, h: card_h, action: Action::CalAddOpen, vis: None, hover_bg: None });
            }
            Events::List(list) => {
                let row_h = p.px(25.0);
                let gap = p.px(2.0);
                let list_h = (card_y + card_h - p.px(6.0) - list_top).max(row_h);
                let n_items = list.len();
                let content_h = if n_items == 0 {
                    0.0
                } else {
                    n_items as f32 * row_h + (n_items.saturating_sub(1)) as f32 * gap + p.px(4.0)
                };
                let will_scroll = content_h > list_h + 1.0;
                let right_pad = if will_scroll { p.px(16.0) } else { p.px(10.0) };
                if let Ok(sc) = create_scroll_viewport(p, c, rx, list_top, rw, list_h) {
                    let dot = p.px(5.0);
                    let tth = p.px(11.5) * 1.3;
                    for (idx, ev) in list.iter().enumerate() {
                        let ry = idx as f32 * (row_h + gap);
                        let cy = ry + row_h / 2.0;
                        p.rect(&sc, p.px(11.0), cy - dot / 2.0, dot, dot, dot / 2.0, Color { A: 245, R: BLUE[0], G: BLUE[1], B: BLUE[2] });
                        let when = match ev.minute {
                            Some(m) if crate::prefs::get().clock_24h => format!("{:02}:{:02}", m / 60, m % 60),
                            Some(m) => format!("{}:{:02}{}", (m / 60 + 11) % 12 + 1, m % 60, if m < 720 { "a" } else { "p" }),
                            None => tr("Sepanjang hari").to_string(),
                        };
                        let time_x = p.px(22.0);
                        let time_w = p.text(&sc, &when, time_x, cy - tth / 2.0, 10.5, 600, BLUE, 0.95, Anchor::Left, p.px(56.0));
                        let tx = time_x + time_w + p.px(7.0);
                        let tw = (rw - right_pad - tx).max(p.px(40.0));
                        p.text(&sc, &p.fit(&ev.title, 11.5, 500, tw), tx, cy - tth / 2.0, 11.5, 500, WHITE, 0.94, Anchor::Left, tw + p.px(2.0));
                    }
                    live.scroll = Some(finish_scroll_view(
                        p,
                        c,
                        rx,
                        list_top,
                        rw,
                        list_h,
                        sc,
                        content_h,
                        rx + rw - p.px(5.0),
                        list_top + p.px(2.0),
                        (list_h - p.px(4.0)).max(p.px(14.0)),
                        d.todo_cal_scroll,
                        d.sb_hover,
                        d.sb_dragging,
                        d.sb_active,
                        Vec::new(),
                    ));
                }
            }
            _ => {}
        }
        return;
    }

    if d.todos.is_empty() && !d.todo_adding {
        let cx = rx + rw / 2.0;
        let cy = card_y + card_h / 2.0;
        p.text(c, tr("Belum ada tugas"), cx, cy - p.px(7.0), 11.5, 500, WHITE, 0.45, Anchor::Center, rw - p.px(14.0));
        regions.push(Region { x: rx, y: card_y, w: rw, h: card_h, action: Action::TodoAddOpen, vis: None, hover_bg: None });
        return;
    }

    let row_h = p.px(26.0);
    let gap = p.px(2.0);
    let list_h = (card_y + card_h - p.px(6.0) - list_top).max(row_h);
    let n_items = d.todos.len();
    let content_h = if n_items == 0 {
        0.0
    } else {
        n_items as f32 * row_h + (n_items.saturating_sub(1)) as f32 * gap + p.px(4.0)
    };
    let will_scroll = content_h > list_h + 1.0;
    let sb_pad = if will_scroll { p.px(10.0) } else { 0.0 };
    let Ok(sc) = create_scroll_viewport(p, c, rx, list_top, rw, list_h) else { return };
    let mut sc_regions = Vec::new();
    let chk = p.px(14.0);
    for (idx, it) in d.todos.iter().enumerate() {
        let ry = idx as f32 * (row_h + gap);
        let n0 = kids(&sc);
        let cx = p.px(17.0);
        let cy = ry + row_h / 2.0;
        if it.completed {
            p.rect(&sc, cx - chk / 2.0, cy - chk / 2.0, chk, chk, chk / 2.0, Color { A: 245, R: GREEN[0], G: GREEN[1], B: GREEN[2] });
            p.sym(&sc, Sym::Check, cx, cy, 9.0, WHITE, 1.0);
        } else {
            p.rect(&sc, cx - chk / 2.0, cy - chk / 2.0, chk, chk, chk / 2.0, white(115));
            let inner = chk - p.px(2.6);
            p.rect(&sc, cx - inner / 2.0, cy - inner / 2.0, inner, inner, inner / 2.0, CARD_DARK);
        }
        let del_cx = rw - p.px(14.0) - sb_pad;
        let right_edge = if d.todo_editing { del_cx - p.px(12.0) } else { rw - p.px(10.0) - sb_pad };
        let tx = cx + chk / 2.0 + p.px(9.0);
        let tw = (right_edge - tx).max(p.px(36.0));
        let tth = p.px(12.0) * 1.3;
        p.text(&sc, &p.fit(&it.title, 12.0, if it.completed { 400 } else { 500 }, tw), tx, cy - tth / 2.0, 12.0, if it.completed { 400 } else { 500 }, WHITE, if it.completed { 0.38 } else { 0.96 }, Anchor::Left, tw + p.px(2.0));
        let row_vis = group_since(&sc, n0);
        if d.todo_editing {
            let d0 = kids(&sc);
            p.sym(&sc, Sym::Close, del_cx, cy, 8.5, RED, 0.85);
            sc_regions.push(Region { x: del_cx - p.px(12.0), y: ry, w: p.px(24.0), h: row_h, action: Action::TodoDelete(idx), vis: group_since(&sc, d0), hover_bg: None });
            sc_regions.push(Region { x: 0.0, y: ry, w: del_cx - p.px(12.0), h: row_h, action: Action::TodoToggle(idx), vis: row_vis, hover_bg: None });
        } else {
            sc_regions.push(Region { x: 0.0, y: ry, w: rw - sb_pad, h: row_h, action: Action::TodoToggle(idx), vis: row_vis, hover_bg: None });
        }
    }
    live.scroll = Some(finish_scroll_view(
        p,
        c,
        rx,
        list_top,
        rw,
        list_h,
        sc,
        content_h,
        rx + rw - p.px(5.0),
        list_top + p.px(2.0),
        (list_h - p.px(4.0)).max(p.px(14.0)),
        d.todo_scroll,
        d.sb_hover,
        d.sb_dragging,
        d.sb_active,
        sc_regions,
    ));
}

fn gemini_tab(p: &mut Painter, c: &ContainerVisual, f: Frame, full_h: f32, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(12.0);
    let bot = f.h - p.px(DOTS_STRIP_DP + 6.0);
    let (x0, w) = (f.x0 + pad, f.w - 2.0 * pad);

    const GEM_BLUE: [u8; 3] = [66, 133, 244];
    const GEM_VIOLET: [u8; 3] = [150, 110, 255];
    const GEM_SOFT: [u8; 3] = [164, 186, 255];

    // Signature two-tone Sapphire -> Violet bottom-up OLED aura (subtle when reading, brighter while streaming).
    if let Some(glow) = p.ambient_glow_duo(c, f.x0, 0.0, f.w, full_h, GEM_BLUE, GEM_VIOLET) {
        let _ = glow.SetOpacity(if d.gemini_streaming { 0.72 } else { 0.46 });
        live.tab_glow = Some(glow);
    }

    let (iy, ih) = (top, p.px(28.0));
    let btn_w = p.px(66.0);
    let (ix, iw) = (x0, w - p.px(8.0) - btn_w);
    live.gemini_input_box = Some((ix, iy, iw, ih));
    regions.push(Region { x: ix, y: iy, w: iw, h: ih, action: Action::GeminiFocus, vis: None, hover_bg: None });

    let (bx, by) = (ix + iw + p.px(8.0), iy);
    let n0 = kids(c);
    let (label, act) = if d.gemini_streaming {
        p.rect(c, bx, by, btn_w, ih, ih / 2.0, Color { A: 225, R: 255, G: 69, B: 58 });
        (tr("Hentikan"), Action::GeminiStop)
    } else {
        p.gradient_bar(c, bx, by, btn_w, ih, ih / 2.0, GEM_BLUE, GEM_VIOLET);
        (tr("Kirim"), Action::GeminiSend)
    };
    let th = p.px(11.0) * 1.3;
    p.text(c, label, bx + btn_w / 2.0, by + ih / 2.0 - th / 2.0, 11.0, 600, WHITE, 1.0, Anchor::Center, btn_w);
    regions.push(Region { x: bx, y: by, w: btn_w, h: ih, action: act, vis: group_since(c, n0), hover_bg: None });

    // Glass model badge pill + Copy / Clear controls
    let bar_y = iy + ih + p.px(6.0);
    let pill_h = p.px(18.0);
    let st_text = if d.gemini_streaming { tr("Berpikir…") } else { d.gemini_model_label };
    let st_w = p.measure(st_text, 9.8, 600).min(w * 0.52);
    let pill_w = st_w + p.px(25.0);
    let pill_bg = if d.gemini_streaming {
        Color { A: 44, R: GEM_VIOLET[0], G: GEM_VIOLET[1], B: GEM_VIOLET[2] }
    } else {
        white(16)
    };
    p.rect(c, x0, bar_y, pill_w, pill_h, pill_h / 2.0, pill_bg);
    p.sym(c, Sym::Sparkle, x0 + p.px(9.5), bar_y + pill_h / 2.0, 10.5, GEM_SOFT, 0.98);
    let st_h = p.px(9.8) * 1.25;
    p.text(
        c,
        st_text,
        x0 + p.px(17.5),
        bar_y + (pill_h - st_h) / 2.0,
        9.8,
        600,
        if d.gemini_streaming { GEM_SOFT } else { WHITE },
        if d.gemini_streaming { 0.98 } else { 0.74 },
        Anchor::Left,
        st_w + p.px(4.0),
    );
    if !d.gemini_text.is_empty() || !d.gemini_error.is_empty() {
        let clr = tr("Kosongkan");
        let clw = p.measure(clr, 10.0, 600) + p.px(4.0);
        let clx = x0 + w;
        p.text(c, clr, clx, bar_y + p.px(2.0), 10.0, 600, WHITE, 0.58, Anchor::Right, clw + 4.0);
        regions.push(Region { x: clx - clw - p.px(4.0), y: bar_y - p.px(2.0), w: clw + p.px(8.0), h: p.px(22.0), action: Action::GeminiClear, vis: None, hover_bg: None });

        if !d.gemini_text.is_empty() {
            let cpy_x = clx - clw - p.px(16.0);
            let n0 = kids(c);
            let sym = if d.gemini_copied { Sym::Check } else { Sym::Copy };
            let rgb = if d.gemini_copied { GREEN } else { WHITE };
            p.sym(c, sym, cpy_x, bar_y + pill_h / 2.0, 11.5, rgb, 0.88);
            regions.push(Region { x: cpy_x - p.px(12.0), y: bar_y - p.px(2.0), w: p.px(24.0), h: p.px(22.0), action: Action::GeminiCopy, vis: group_since(c, n0), hover_bg: None });
        }
    }

    let rx = x0;
    let ry = bar_y + pill_h + p.px(7.0);
    let rw = w;
    let rh = (bot - ry).max(p.px(40.0));

    let show_quick_chips = d.gemini_has_key && d.gemini_text.is_empty() && d.gemini_error.is_empty() && !d.gemini_streaming;
    if show_quick_chips {
        // Subtle frosted card with 3 one-click clipboard AI chips when idle.
        p.rect(c, rx, ry, rw, rh, p.px(10.0), white(10));
        let hint_y = ry + p.px(10.0);
        p.text(c, tr("Aksi cepat (teks di atas / clipboard):"), rx + p.px(12.0), hint_y, 10.5, 500, WHITE, 0.50, Anchor::Left, rw - p.px(24.0));

        let chips: [(u8, &str, [u8; 3]); 3] = [
            (0, tr("Ringkas"), GEM_BLUE),
            (1, tr("Terjemahkan"), GEM_VIOLET),
            (2, tr("Rapikan"), [56, 189, 248]),
        ];
        let chip_gap = p.px(7.0);
        let inner_x = rx + p.px(10.0);
        let inner_w = rw - p.px(20.0);
        let chip_w = ((inner_w - 2.0 * chip_gap) / 3.0).max(p.px(64.0));
        let chip_h = p.px(30.0);
        let chip_y = ry + rh - chip_h - p.px(11.0);
        let cth = p.px(11.0) * 1.3;

        for (idx, (id, lbl, tint)) in chips.into_iter().enumerate() {
            let cx = inner_x + idx as f32 * (chip_w + chip_gap);
            let n0 = kids(c);
            p.rect(c, cx, chip_y, chip_w, chip_h, p.px(9.0), Color { A: 24, R: tint[0], G: tint[1], B: tint[2] });
            p.sym(c, Sym::Sparkle, cx + p.px(12.0), chip_y + chip_h / 2.0, 9.5, lift_for_dark_pill(tint), 0.92);
            p.text(
                c,
                lbl,
                cx + p.px(21.0),
                chip_y + (chip_h - cth) / 2.0,
                11.0,
                600,
                WHITE,
                0.92,
                Anchor::Left,
                (chip_w - p.px(24.0)).max(p.px(32.0)),
            );
            regions.push(Region {
                x: cx,
                y: chip_y,
                w: chip_w,
                h: chip_h,
                action: Action::GeminiQuick(id),
                vis: group_since(c, n0),
                hover_bg: None,
            });
        }
        return;
    }

    // Frosted tinted glass reading card behind response / streaming / error text.
    p.rect(c, rx, ry, rw, rh, p.px(10.0), white(12));
    let pad_x = p.px(9.0);
    let pad_y = p.px(6.0);
    let (irx, iry) = (rx + pad_x, ry + pad_y);
    let (irw, irh) = ((rw - 2.0 * pad_x).max(p.px(40.0)), (rh - 2.0 * pad_y).max(p.px(28.0)));
    if let Ok(host) = p.comp.CreateContainerVisual() {
        let _ = c.Children().and_then(|k| k.InsertAtTop(&host));
        live.scroll = draw_gemini_response(
            p,
            &host,
            irx,
            iry,
            irw,
            irh,
            d.gemini_text,
            d.gemini_error,
            d.gemini_has_key,
            d.gemini_streaming,
            d.gemini_auto_follow,
            d.gemini_scroll,
            d.sb_hover,
            d.sb_dragging,
            d.sb_active,
        );
        live.gemini_resp_host = Some(GeminiRespHost { host, x: irx, y: iry, w: irw, h: irh });
    }
}

#[allow(clippy::too_many_arguments)]
pub fn refresh_gemini_response(
    p: &mut Painter,
    live: &mut Live,
    text: &str,
    error: &str,
    has_key: bool,
    streaming: bool,
    auto_follow: bool,
    scroll_px: f32,
    sb_hover: f32,
    sb_dragging: bool,
    sb_active: f32,
) -> f32 {
    let Some(h) = live.gemini_resp_host.as_ref() else {
        return 0.0;
    };
    let (host, x, y, w, hh) = (h.host.clone(), h.x, h.y, h.w, h.h);
    if let Ok(k) = host.Children() {
        let _ = k.RemoveAll();
    }
    live.scroll = draw_gemini_response(
        p,
        &host,
        x,
        y,
        w,
        hh,
        text,
        error,
        has_key,
        streaming,
        auto_follow,
        scroll_px,
        sb_hover,
        sb_dragging,
        sb_active,
    );
    live.scroll.as_ref().map_or(0.0, |s| s.max_scroll)
}

#[allow(clippy::too_many_arguments)]
fn draw_gemini_response(
    p: &mut Painter,
    c: &ContainerVisual,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    text: &str,
    error: &str,
    has_key: bool,
    streaming: bool,
    auto_follow: bool,
    scroll_px: f32,
    sb_hover: f32,
    sb_dragging: bool,
    sb_active: f32,
) -> Option<ScrollViewLive> {
    let font = 11.5;
    let line_h = p.px(font) * 1.38;
    let wrap_w = (w - p.px(12.0)).max(p.px(40.0));

    let (src, weight, rgb, op, follow) = if !error.is_empty() {
        (error, 500, [255, 110, 100], 0.95, false)
    } else if !text.is_empty() {
        (text, 400, WHITE, 0.92, streaming && auto_follow)
    } else {
        let msg = if streaming {
            tr("Berpikir…")
        } else if !has_key {
            tr("Atur Gemini API Key di Pengaturan → Integrasi untuk mulai bertanya.")
        } else {
            tr("Ketik pertanyaan dan tekan Enter.")
        };
        p.text(c, &p.fit(msg, 11.5, 400, w), x + w / 2.0, y + h / 2.0 - p.px(8.0), 11.5, 400, WHITE, 0.45, Anchor::Center, w + p.px(2.0));
        return None;
    };

    let mut wrapped: Vec<String> = Vec::new();
    for para in src.lines() {
        let t = para.trim_end();
        if t.is_empty() {
            wrapped.push(String::new());
            continue;
        }
        for line in nacre_core::notch::typeset::wrap(t, wrap_w, 120, |s| p.measure(s, font, weight)) {
            wrapped.push(line);
            if wrapped.len() >= 240 {
                break;
            }
        }
        if wrapped.len() >= 240 {
            break;
        }
    }

    let sc = create_scroll_viewport(p, c, x, y, w, h).ok()?;
    let mut yy = 0.0;
    for line in &wrapped {
        if !line.is_empty() {
            p.text(&sc, line, 0.0, yy, font, weight, rgb, op, Anchor::Left, wrap_w + p.px(4.0));
        }
        yy += line_h;
    }
    let content_h = (yy + p.px(4.0)).max(h);
    let max_scroll = (content_h - h).max(0.0);
    let init_scroll = if follow { max_scroll } else { scroll_px.clamp(0.0, max_scroll) };
    Some(finish_scroll_view(
        p,
        c,
        x,
        y,
        w,
        h,
        sc,
        content_h,
        x + w,
        y + p.px(2.0),
        (h - p.px(4.0)).max(p.px(14.0)),
        init_scroll,
        sb_hover,
        sb_dragging,
        sb_active,
        Vec::new(),
    ))
}

fn smoothstep01(e0: f32, e1: f32, v: f32) -> f32 {
    let t = ((v - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn play(p: &mut Painter, c: &ContainerVisual, bf: Frame, full_h: f32, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    let lp = d.lyrics_progress.clamp(0.0, 1.0);
    let show_lyrics_layer = d.lyrics_open || d.play_gliding || lp > 0.01;

    // Bottom-up OLED atmospheric media gradient: pure #000000 along the top bezel/ears,
    // blooming seamlessly into the album art's dominant color across the full rounded bottom edge.
    let mut lyrics_glow = None;
    if let Some(m) = &d.media {
        let glow = media_tint(m);
        if let Some(gv) = p.ambient_glow(c, bf.x0, 0.0, bf.w, full_h, 0.0, glow, false) {
            let _ = gv.SetOpacity(0.92 + 0.08 * lp);
            lyrics_glow = Some(gv);
        }
    }

    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(14.0);
    let bot_margin = p.px(DOTS_STRIP_DP + 2.0) + (p.px(12.0) - p.px(DOTS_STRIP_DP + 2.0)) * lp;
    let h = (bf.h - bot_margin - top).max(p.px(80.0));
    let mx = bf.x0 + pad + p.px(4.0);
    let mw_full = (bf.w - 2.0 * pad - p.px(8.0)).max(p.px(180.0));
    let mw_split = p.px(224.0);
    let mw = mw_full + (mw_split - mw_full) * lp;
    let div_x = mx + mw + p.px(14.0);

    let mut empty_center = None;
    let mc = match &d.media {
        Some(m) => music_column(p, c, mx, top, mw, h, m, lp, d.play_gliding, d.muted, d.scrub_hover, d.art_scale, regions, live),
        None => {
            let qx = mx + mw / 2.0;
            let qy = top + h - p.px(14.0);
            if let Ok(ec) = p.comp.CreateContainerVisual() {
                let _ = c.Children().and_then(|k| k.InsertAtTop(&ec));
                p.sym(&ec, Sym::Note, qx, top + h / 2.0 - p.px(14.0), 26.0, WHITE, 0.34);
                p.text(&ec, tr("Tidak diputar"), qx, top + h / 2.0 + p.px(8.0), 12.5, 500, WHITE, 0.56, Anchor::Center, mw - p.px(12.0));
                let n0 = kids(&ec);
                p.sym(&ec, Sym::Quote, qx, qy, 20.5, WHITE, 0.52 + 0.43 * lp);
                regions.push(Region { x: qx - p.px(16.0), y: qy - p.px(14.0), w: p.px(32.0), h: p.px(28.0), action: Action::ToggleLyrics, vis: group_since(&ec, n0), hover_bg: None });
                empty_center = Some((qx, ec));
            } else {
                p.sym(c, Sym::Note, qx, top + h / 2.0 - p.px(14.0), 26.0, WHITE, 0.34);
                p.text(c, tr("Tidak diputar"), qx, top + h / 2.0 + p.px(8.0), 12.5, 500, WHITE, 0.56, Anchor::Center, mw - p.px(12.0));
                let n0 = kids(c);
                p.sym(c, Sym::Quote, qx, qy, 20.5, WHITE, 0.52 + 0.43 * lp);
                regions.push(Region { x: qx - p.px(16.0), y: qy - p.px(14.0), w: p.px(32.0), h: p.px(28.0), action: Action::ToggleLyrics, vis: group_since(c, n0), hover_bg: None });
            }
            None
        }
    };

    let mut lyrics_divider = None;
    let mut lyrics_clip = None;
    let mut lyrics_panel = None;
    if show_lyrics_layer {
        // Canonical split-view lyrics width (530 dp open shell) so lyrics text never reflows
        // or re-wraps when mounting during a close transition.
        let split_w = p.px(530.0);
        let canonical_lw = (split_w - 2.0 * pad - p.px(4.0) - mw_split - p.px(14.0) - p.px(15.0)).max(p.px(80.0));
        let lyr_h = (bf.h - p.px(12.0) - top).max(p.px(80.0));
        let div_h = (lyr_h - p.px(8.0)).max(p.px(40.0));

        // 1px vertical divider anchored strictly at `div_x` (blooms vertically from center, never crosses left).
        let div_t = smoothstep01(0.16, 0.72, lp);
        if let Some(dv) = p.rect(c, div_x.round(), (top + p.px(4.0)).round(), 1.0, div_h.round(), 0.0, white(26)) {
            let _ = dv.SetCenterPoint(Vector3 { X: 0.5, Y: div_h / 2.0, Z: 0.0 });
            let _ = dv.SetScale(Vector3 { X: 1.0, Y: 0.55 + 0.45 * div_t, Z: 1.0 });
            let _ = dv.SetOpacity(div_t);
            lyrics_divider = Some(dv);
        }

        // Clipped right-column viewport prevents lyrics text from ever bleeding across the divider.
        let clip_x = div_x + p.px(4.0);
        let clip_w = (bf.x0 + bf.w - pad - clip_x + p.px(4.0)).max(1.0);
        if let (Ok(vp), Ok(iclip), Ok(panel)) = (
            p.comp.CreateContainerVisual(),
            p.comp.CreateInsetClip(),
            p.comp.CreateContainerVisual(),
        ) {
            let _ = vp.SetOffset(Vector3 { X: clip_x, Y: top, Z: 0.0 });
            let _ = vp.SetSize(Vector2 { X: clip_w, Y: lyr_h });
            let _ = vp.SetClip(&iclip);
            let _ = c.Children().and_then(|k| k.InsertAtTop(&vp));

            let panel_t = if d.lyrics_open {
                smoothstep01(0.20, 0.94, lp)
            } else {
                smoothstep01(0.32, 0.96, lp)
            };
            let travel = if d.lyrics_open { p.px(16.0) } else { p.px(12.0) };
            let slide_x = (1.0 - panel_t) * travel;
            let sc = 0.965 + 0.035 * panel_t;
            let _ = panel.SetCenterPoint(Vector3 { X: p.px(11.0) + canonical_lw / 2.0, Y: lyr_h / 2.0, Z: 0.0 });
            let _ = panel.SetScale(Vector3 { X: sc, Y: sc, Z: 1.0 });
            let _ = panel.SetOpacity(panel_t);
            let _ = panel.SetOffset(Vector3 { X: slide_x, Y: 0.0, Z: 0.0 });
            let _ = vp.Children().and_then(|k| k.InsertAtTop(&panel));

            lyrics_slot(p, &panel, p.px(11.0), 0.0, canonical_lw, lyr_h, d, live, false);
            lyrics_clip = Some(vp);
            lyrics_panel = Some(panel);
        } else {
            let lx = div_x + p.px(15.0);
            lyrics_slot(p, c, lx, top, canonical_lw, lyr_h, d, live, false);
        }
    }

    let (left_group, title_clip, title_clip_h, base_cy, ctrl_groups) = match mc {
        Some((lg, tc, tch, cy, cg)) => (Some(lg), tc, tch, cy, cg),
        None => (None, None, 0.0, top + h - p.px(18.0), Vec::new()),
    };
    live.play_layout = Some(PlayLayoutLive {
        base_mx: mx,
        base_cy,
        left_group,
        empty_center,
        title_clip,
        title_clip_h,
        ctrl_groups,
        lyrics_divider,
        lyrics_clip,
        lyrics_panel,
        lyrics_glow,
    });
}

#[allow(clippy::too_many_arguments)]
fn lyrics_slot(p: &mut Painter, c: &ContainerVisual, x: f32, y: f32, w: f32, h: f32, d: &Data, live: &mut Live, compact_player: bool) {
    if let Ok(host) = p.comp.CreateContainerVisual() {
        let _ = c.Children().and_then(|k| k.InsertAtTop(&host));
        lyrics_3line_stack(p, &host, x, y, w, h, d.media.is_some(), &d.lyrics, compact_player, 0);
        live.lyrics_host = Some(LyricsHostLive { host, x, y, w, h, compact_player });
    }
}

pub fn refresh_lyrics_in_place(p: &mut Painter, live: &mut Live, has_media: bool, ly: &LyricsView, dir: i32) {
    let Some(h) = live.lyrics_host.as_ref() else { return };
    let host = h.host.clone();
    let (x, y, w, h_px, compact_player) = (h.x, h.y, h.w, h.h, h.compact_player);
    if let Ok(k) = host.Children() {
        let _ = k.RemoveAll();
    }
    lyrics_3line_stack(p, &host, x, y, w, h_px, has_media, ly, compact_player, dir);
}

#[allow(clippy::too_many_arguments)]
fn lyrics_3line_stack(
    p: &mut Painter,
    c: &ContainerVisual,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    has_media: bool,
    ly: &LyricsView,
    compact_player: bool,
    dir: i32,
) {
    use windows::UI::Composition::CompositionObject;
    use windows_core::Interface;

    let rx = x.round();
    let ry = y.round();
    let rw = w.ceil();
    let rh = h.ceil();

    let center_msg = |p: &mut Painter, s: &str| {
        p.text(c, s, rx + rw / 2.0, ry + rh / 2.0 - p.px(8.0), 12.5, 500, WHITE, 0.42, Anchor::Center, rw - p.px(8.0));
    };
    let Some(l) = ly.lines else {
        match (ly.state, has_media) {
            (LyrState::Loading, _) => center_msg(p, tr("Mencari lirik\u{2026}")),
            (_, true) => center_msg(p, tr("Lirik tidak tersedia")),
            _ => center_msg(p, tr("Lirik tampil di sini saat lagu diputar.")),
        }
        return;
    };

    let go = || -> windows_core::Result<ContainerVisual> {
        let frame = p.comp.CreateContainerVisual()?;
        frame.SetOffset(Vector3 { X: rx, Y: ry, Z: 0.0 })?;
        frame.SetSize(Vector2 { X: rw, Y: rh })?;
        frame.SetClip(&p.comp.CreateInsetClip()?)?;
        c.Children()?.InsertAtTop(&frame)?;
        Ok(frame)
    };
    let Ok(frame) = go() else { return };

    let maxw = (rw - p.px(4.0)).max(p.px(40.0));
    let active_sz = if compact_player { 14.5 } else { 15.0 };
    let dim_sz = if compact_player { 12.5 } else { 13.0 };
    let active_lh = (p.px(active_sz) * 1.28).ceil();
    let dim_lh = (p.px(dim_sz) * 1.26).ceil();
    let gap = (p.px(if compact_player { 6.0 } else { 9.0 })).round();

    fn clean_line(text: &str) -> &str {
        if text.trim().is_empty() { "\u{266A}" } else { text }
    }

    if l.synced {
        let active = l.index_at(ly.pos_ms);
        let center = active.unwrap_or(0);

        let prev_lines: Vec<String> = if center > 0 {
            let t = clean_line(&l.lines[center - 1].text);
            vec![p.fit(t, dim_sz, 600, maxw)]
        } else {
            Vec::new()
        };
        let cur_lines: Vec<String> = if center < l.lines.len() {
            let t = clean_line(&l.lines[center].text);
            nacre_core::notch::typeset::wrap(t, maxw, 2, |s| p.measure_dwrite(s, active_sz, 700).max(p.measure(s, active_sz, 700)))
        } else {
            Vec::new()
        };
        let next_lines: Vec<String> = if center + 1 < l.lines.len() {
            let t = clean_line(&l.lines[center + 1].text);
            nacre_core::notch::typeset::wrap(t, maxw, 2, |s| p.measure_dwrite(s, dim_sz, 600).max(p.measure(s, dim_sz, 600)))
        } else {
            Vec::new()
        };

        let prev_h = if prev_lines.is_empty() { 0.0 } else { prev_lines.len() as f32 * dim_lh };
        let cur_h = cur_lines.len() as f32 * active_lh;
        let next_h = if next_lines.is_empty() { 0.0 } else { next_lines.len() as f32 * dim_lh };
        let total_h = prev_h
            + if prev_h > 0.0 { gap } else { 0.0 }
            + cur_h
            + if next_h > 0.0 { gap + next_h } else { 0.0 };

        let y_top = ((rh - total_h) / 2.0).max(p.px(2.0)).round();
        let shift = if prev_h > 0.0 { prev_h + gap } else { active_lh + gap };
        let mut yy = y_top;

        // Outgoing top line (`center - 2`) softly floats up and dissolves on forward advance.
        if dir > 0 && center >= 2 {
            let t = clean_line(&l.lines[center - 2].text);
            let shown = p.fit(t, dim_sz, 600, maxw);
            if let Ok(slot) = p.comp.CreateContainerVisual() {
                let _ = slot.SetOffset(Vector3 { X: 0.0, Y: y_top, Z: 0.0 });
                let _ = frame.Children().and_then(|k| k.InsertAtTop(&slot));
                p.text(&slot, &shown, 0.0, 0.0, dim_sz, 600, WHITE, 1.0, Anchor::Left, maxw + p.px(4.0));
                if let Ok(o) = slot.cast::<CompositionObject>() {
                    glide_scalar(&p.comp, &o, "Offset.Y", y_top, y_top - (dim_lh + gap), 360, 0);
                    glide_scalar(&p.comp, &o, "Opacity", 0.34, 0.0, 260, 0);
                }
            }
        }

        // Previous line (`center - 1`): glides up from active slot, smoothly dims & relaxes scale.
        if !prev_lines.is_empty() {
            if let Ok(slot) = p.comp.CreateContainerVisual() {
                let _ = slot.SetOffset(Vector3 { X: 0.0, Y: yy, Z: 0.0 });
                let _ = slot.SetCenterPoint(Vector3 { X: 0.0, Y: prev_h / 2.0, Z: 0.0 });
                let _ = slot.SetOpacity(0.34);
                let _ = frame.Children().and_then(|k| k.InsertAtTop(&slot));
                let mut ly_off = 0.0;
                for t in &prev_lines {
                    p.text(&slot, t, 0.0, ly_off, dim_sz, 600, WHITE, 1.0, Anchor::Left, maxw + p.px(4.0));
                    ly_off += dim_lh;
                }
                if dir > 0 {
                    if let Ok(o) = slot.cast::<CompositionObject>() {
                        glide_scalar(&p.comp, &o, "Offset.Y", yy + shift, yy, 440, 0);
                        glide_scalar(&p.comp, &o, "Opacity", 0.92, 0.34, 420, 0);
                        glide_scalar(&p.comp, &o, "Scale.X", 1.04, 1.0, 440, 0);
                        glide_scalar(&p.comp, &o, "Scale.Y", 1.04, 1.0, 440, 0);
                    }
                } else if dir < 0 {
                    if let Ok(o) = slot.cast::<CompositionObject>() {
                        glide_scalar(&p.comp, &o, "Offset.Y", yy - shift * 0.45, yy, 360, 0);
                        glide_scalar(&p.comp, &o, "Opacity", 0.0, 0.34, 320, 0);
                    }
                }
            }
            yy += prev_h + gap;
        }

        // Active current line (`center`): staggered Apple-Music glide + brightness & scale bloom.
        let is_active = active.is_some();
        let cur_op = if is_active { 1.0 } else { 0.45 };
        if let Ok(slot) = p.comp.CreateContainerVisual() {
            let _ = slot.SetOffset(Vector3 { X: 0.0, Y: yy, Z: 0.0 });
            let _ = slot.SetCenterPoint(Vector3 { X: 0.0, Y: cur_h / 2.0, Z: 0.0 });
            let _ = slot.SetOpacity(cur_op);
            let _ = frame.Children().and_then(|k| k.InsertAtTop(&slot));
            let mut ly_off = 0.0;
            for t in &cur_lines {
                p.text(&slot, t, 0.0, ly_off, active_sz, 700, WHITE, 1.0, Anchor::Left, maxw + p.px(4.0));
                ly_off += active_lh;
            }
            if dir > 0 {
                if let Ok(o) = slot.cast::<CompositionObject>() {
                    glide_scalar(&p.comp, &o, "Offset.Y", yy + shift, yy, 460, 28);
                    glide_scalar(&p.comp, &o, "Opacity", 0.34, cur_op, 400, 28);
                    glide_scalar(&p.comp, &o, "Scale.X", 0.96, 1.0, 460, 28);
                    glide_scalar(&p.comp, &o, "Scale.Y", 0.96, 1.0, 460, 28);
                }
            } else if dir < 0 {
                if let Ok(o) = slot.cast::<CompositionObject>() {
                    glide_scalar(&p.comp, &o, "Offset.Y", yy - shift * 0.45, yy, 380, 0);
                    glide_scalar(&p.comp, &o, "Opacity", 0.34, cur_op, 340, 0);
                    glide_scalar(&p.comp, &o, "Scale.X", 0.96, 1.0, 380, 0);
                    glide_scalar(&p.comp, &o, "Scale.Y", 0.96, 1.0, 380, 0);
                }
            }
        }
        yy += cur_h;

        // Upcoming next line (`center + 1`): gentle staggered rise & soft fade-in.
        if !next_lines.is_empty() {
            yy += gap;
            if let Ok(slot) = p.comp.CreateContainerVisual() {
                let _ = slot.SetOffset(Vector3 { X: 0.0, Y: yy, Z: 0.0 });
                let _ = slot.SetOpacity(0.34);
                let _ = frame.Children().and_then(|k| k.InsertAtTop(&slot));
                let mut ly_off = 0.0;
                for t in &next_lines {
                    if yy + ly_off + dim_lh * 0.5 > rh {
                        break;
                    }
                    p.text(&slot, t, 0.0, ly_off, dim_sz, 600, WHITE, 1.0, Anchor::Left, maxw + p.px(4.0));
                    ly_off += dim_lh;
                }
                if dir > 0 {
                    if let Ok(o) = slot.cast::<CompositionObject>() {
                        glide_scalar(&p.comp, &o, "Offset.Y", yy + shift, yy, 480, 56);
                        glide_scalar(&p.comp, &o, "Opacity", 0.0, 0.34, 420, 56);
                    }
                } else if dir < 0 {
                    if let Ok(o) = slot.cast::<CompositionObject>() {
                        glide_scalar(&p.comp, &o, "Offset.Y", yy - shift * 0.45, yy, 380, 20);
                        glide_scalar(&p.comp, &o, "Opacity", 0.85, 0.34, 340, 20);
                    }
                }
            }
        }
    } else {
        let mut yy = p.px(4.0);
        for line in l.lines.iter().filter(|x| !x.text.is_empty()) {
            if yy > rh - dim_lh {
                break;
            }
            for t in nacre_core::notch::typeset::wrap(&line.text, maxw, 2, |s| p.measure(s, dim_sz, 500)) {
                p.text(&frame, &t, 0.0, yy, dim_sz, 500, WHITE, 0.78, Anchor::Left, maxw + p.px(4.0));
                yy += dim_lh;
            }
            yy += gap * 0.6;
        }
    }
}

fn round_button(p: &mut Painter, c: &ContainerVisual, cx: f32, cy: f32, d: f32, fill: Color) {
    p.rect(c, (cx - d / 2.0).round(), (cy - d / 2.0).round(), d, d, d / 2.0, fill);
}

fn timer(p: &mut Painter, c: &ContainerVisual, f: Frame, full_h: f32, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    let pad = p.px(EDGE_PAD_DP + 2.0);
    let top = p.px(13.0);
    let bot = f.h - p.px(DOTS_STRIP_DP + 8.0);
    let h = (bot - top).max(p.px(110.0));
    let x_left = f.x0 + pad;
    let lw = p.px(148.0);
    let div_x = x_left + lw + p.px(12.0);
    let x_right = div_x + p.px(14.0);
    let rw = (f.x0 + f.w - pad - x_right).max(p.px(152.0));

    let t = d.timer;
    let secs = t.remaining_secs(d.now_ms);
    let mins = t.mins_left(d.now_ms);
    let prog = t.progress(d.now_ms);
    let en = crate::prefs::get().lang == nacre_core::i18n::Lang::En;
    let accent = crate::prefs::accent();
    let accent_soft = lift_for_dark_pill(accent);

    // Full-bleed bottom-up OLED atmospheric wash matching the Media Player aesthetic:
    // pure #000000 across the top bezel/title, blooming smoothly into the rounded bottom corners.
    let glow_rgb = match t.stage {
        PomodoroStage::Focus(_) => accent,
        PomodoroStage::ShortBreak(_) => GREEN,
        PomodoroStage::LongBreak => [132, 114, 255],
    };
    let glow_op = match t.phase {
        Phase::Running => 0.92,
        Phase::Paused => 0.72,
        Phase::Idle | Phase::Finished => 0.68,
    };
    if let Some(glow) = p.ambient_glow(c, f.x0, 0.0, f.w, full_h, 0.0, glow_rgb, false) {
        let _ = glow.SetOpacity(glow_op);
        live.tab_glow = Some(glow);
    }

    // --- Left Column: Pomodoro title, session subtitle, big countdown, status & thin progress bar ---
    p.text(c, "Pomodoro", x_left, top, 15.0, 700, WHITE, 0.98, Anchor::Left, lw);

    let stage_sub = match t.stage {
        PomodoroStage::Focus(n) => {
            if en {
                format!("Focus \u{00B7} {n} of 4")
            } else {
                format!("Fokus \u{00B7} {n} dari 4")
            }
        }
        PomodoroStage::ShortBreak(n) => {
            if en {
                format!("Short Break \u{00B7} {n} of 4")
            } else {
                format!("Istirahat Singkat \u{00B7} {n} dari 4")
            }
        }
        PomodoroStage::LongBreak => tr("Istirahat Panjang").to_string(),
    };
    p.text(c, &stage_sub, x_left, top + p.px(25.0), 11.0, 500, WHITE, 0.48, Anchor::Left, lw);

    let time_rgb = if t.phase == Phase::Running && secs <= 60 { accent_soft } else { WHITE };
    p.text(c, &format_mmss(secs), x_left, top + p.px(42.0), 31.0, 700, time_rgb, 1.0, Anchor::Left, lw);

    let mins_str = if en { format!("{mins} min left") } else { format!("{mins} mnt lagi") };
    let st_str = match t.phase {
        Phase::Running => tr("Berjalan"),
        Phase::Paused => tr("Dijeda"),
        Phase::Finished => tr("Selesai"),
        Phase::Idle => tr("Siap"),
    };
    let status_y = top + p.px(92.0);
    let mw = p.text(c, &mins_str, x_left, status_y, 12.0, 700, WHITE, 0.96, Anchor::Left, lw * 0.68);
    p.text(c, st_str, x_left + mw + p.px(6.0), status_y, 12.0, 500, WHITE, 0.48, Anchor::Left, (lw - mw - p.px(6.0)).max(p.px(24.0)));

    let bar_y = top + p.px(118.0);
    let bar_w = (lw - p.px(4.0)).max(p.px(60.0));
    let bar_h = p.px(3.5);
    p.rect(c, x_left, bar_y, bar_w, bar_h, bar_h / 2.0, white(32));
    let fill_w = if t.is_active() || t.phase == Phase::Finished {
        (bar_w * prog).max(p.px(4.0)).min(bar_w)
    } else {
        0.0
    };
    if fill_w > 0.5 {
        p.rect(
            c,
            x_left,
            bar_y,
            fill_w,
            bar_h,
            bar_h / 2.0,
            Color { A: 255, R: accent[0], G: accent[1], B: accent[2] },
        );
    }

    // --- Center 1px Vertical Divider ---
    p.divider_v(c, div_x, top + p.px(22.0), (h - p.px(26.0)).max(p.px(76.0)));

    // --- Right Column: "This round" + [Start/Resume/Pause] pill + Reset + 1..4 Long timeline ---
    let row_cy = top + p.px(33.0);
    let lbl_h = p.px(11.5) * 1.3;
    p.text(c, tr("Ronde ini"), x_right, row_cy - lbl_h / 2.0, 11.5, 500, WHITE, 0.50, Anchor::Left, p.px(68.0));

    let rst_dia = p.px(25.0);
    let rst_cx = x_right + rw - rst_dia / 2.0;
    let n_rst = kids(c);
    round_button(p, c, rst_cx, row_cy, rst_dia, white(24));
    p.sym(c, Sym::Restart, rst_cx, row_cy, 12.0, WHITE, 0.90);
    regions.push(Region {
        x: rst_cx - rst_dia / 2.0,
        y: row_cy - rst_dia / 2.0,
        w: rst_dia,
        h: rst_dia,
        action: Action::TimerReset,
        vis: group_since(c, n_rst),
        hover_bg: None,
    });

    let pill_label = match t.phase {
        Phase::Running => tr("Jeda"),
        Phase::Paused => tr("Lanjutkan"),
        Phase::Idle | Phase::Finished => tr("Mulai"),
    };
    let pill_h = p.px(25.0);
    let pill_w = (p.measure(pill_label, 11.5, 700) + p.px(22.0)).clamp(p.px(56.0), p.px(78.0));
    let pill_x = rst_cx - rst_dia / 2.0 - p.px(7.0) - pill_w;
    let pill_y = row_cy - pill_h / 2.0;
    let n_btn = kids(c);
    // Calm tinted glass surface when Running (Pause), vibrant theme accent when Idle/Paused (Start/Resume).
    let (pill_fill, pill_txt_rgb, pill_txt_op) = if t.phase == Phase::Running {
        (Color { A: 58, R: accent[0], G: accent[1], B: accent[2] }, accent_soft, 0.98)
    } else {
        (Color { A: 242, R: accent[0], G: accent[1], B: accent[2] }, WHITE, 1.0)
    };
    p.rect(c, pill_x, pill_y, pill_w, pill_h, pill_h / 2.0, pill_fill);
    let bth = p.px(11.5) * 1.3;
    p.text(c, pill_label, pill_x + pill_w / 2.0, row_cy - bth / 2.0, 11.5, 700, pill_txt_rgb, pill_txt_op, Anchor::Center, pill_w);
    regions.push(Region {
        x: pill_x,
        y: pill_y,
        w: pill_w,
        h: pill_h,
        action: Action::TimerToggle,
        vis: group_since(c, n_btn),
        hover_bg: None,
    });

    // 4 Focus segments separated by 3 short-break dots + Long Break segment
    let track_cy = top + p.px(78.0);
    let label_y = top + p.px(92.0);
    let seg_h = p.px(5.5);
    let dot_d = p.px(4.0);
    let seg_gap = p.px(3.5);
    let long_w = p.px(20.0);
    // Total fixed overhead: 3 break dots + 6 gaps around them + 1 gap before Long + Long width
    let fixed = 3.0 * dot_d + 7.0 * seg_gap + long_w;
    let seg_w = ((rw - fixed) / 4.0).max(p.px(18.0));
    let active_slot = t.stage.focus_slot();
    let in_short_break = matches!(t.stage, PomodoroStage::ShortBreak(_));

    let mut cur_x = x_right;
    let mut orb_pos: Option<(f32, f32)> = None;

    for i in 0u8..4 {
        let sx = cur_x;
        let completed = i < active_slot || (in_short_break && i == active_slot);
        let active_focus = matches!(t.stage, PomodoroStage::Focus(n) if n == i + 1);
        let track_col = if completed {
            Color { A: 220, R: accent[0], G: accent[1], B: accent[2] }
        } else {
            white(32)
        };
        let n_seg = kids(c);
        p.rect(c, sx, track_cy - seg_h / 2.0, seg_w, seg_h, seg_h / 2.0, track_col);
        if active_focus {
            let fw = (seg_w * prog).clamp(0.0, seg_w);
            if fw > 1.0 {
                p.rect(
                    c,
                    sx,
                    track_cy - seg_h / 2.0,
                    fw,
                    seg_h,
                    seg_h / 2.0,
                    Color { A: 245, R: accent[0], G: accent[1], B: accent[2] },
                );
            }
            orb_pos = Some((sx + (seg_w * prog).clamp(p.px(3.0), seg_w - p.px(3.0)), track_cy));
        }
        let lbl = (i + 1).to_string();
        let is_cur = active_focus;
        p.text(
            c,
            &lbl,
            sx + seg_w / 2.0,
            label_y,
            10.0,
            if is_cur { 700 } else { 500 },
            WHITE,
            if is_cur { 0.98 } else { 0.48 },
            Anchor::Center,
            seg_w + p.px(6.0),
        );
        regions.push(Region {
            x: sx - p.px(2.0),
            y: track_cy - p.px(12.0),
            w: seg_w + p.px(4.0),
            h: p.px(34.0),
            action: Action::TimerStage(PomodoroStage::Focus(i + 1)),
            vis: group_since(c, n_seg),
            hover_bg: None,
        });
        cur_x += seg_w + seg_gap;

        if i < 3 {
            let dcx = cur_x + dot_d / 2.0;
            let brk_done = i < active_slot && !(in_short_break && i == active_slot);
            let brk_active = matches!(t.stage, PomodoroStage::ShortBreak(n) if n == i + 1);
            let dot_col = if brk_done || brk_active {
                Color { A: 230, R: accent[0], G: accent[1], B: accent[2] }
            } else {
                white(36)
            };
            p.rect(c, dcx - dot_d / 2.0, track_cy - dot_d / 2.0, dot_d, dot_d, dot_d / 2.0, dot_col);
            if brk_active {
                orb_pos = Some((dcx, track_cy));
            }
            cur_x += dot_d + seg_gap;
        }
    }

    // Long break segment
    let lx = cur_x;
    let active_long = t.stage == PomodoroStage::LongBreak;
    let n_long = kids(c);
    p.rect(c, lx, track_cy - seg_h / 2.0, long_w, seg_h, seg_h / 2.0, white(32));
    if active_long {
        let fw = (long_w * prog).clamp(0.0, long_w);
        if fw > 1.0 {
            p.rect(
                c,
                lx,
                track_cy - seg_h / 2.0,
                fw,
                seg_h,
                seg_h / 2.0,
                Color { A: 245, R: accent[0], G: accent[1], B: accent[2] },
            );
        }
        orb_pos = Some((lx + (long_w * prog).clamp(p.px(3.0), long_w - p.px(3.0)), track_cy));
    }
    p.text(
        c,
        tr("Panjang"),
        lx + long_w / 2.0,
        label_y,
        9.5,
        if active_long { 700 } else { 500 },
        WHITE,
        if active_long { 0.98 } else { 0.48 },
        Anchor::Center,
        long_w + p.px(14.0),
    );
    regions.push(Region {
        x: lx - p.px(3.0),
        y: track_cy - p.px(12.0),
        w: long_w + p.px(6.0),
        h: p.px(34.0),
        action: Action::TimerStage(PomodoroStage::LongBreak),
        vis: group_since(c, n_long),
        hover_bg: None,
    });

    // Glowing white + soft theme-accent orb on the active session segment (drawn on top of track).
    if let Some((ox, oy)) = orb_pos {
        let h1 = p.px(21.0);
        let h2 = p.px(13.5);
        let core = p.px(8.0);
        p.rect(
            c,
            (ox - h1 / 2.0).round(),
            (oy - h1 / 2.0).round(),
            h1,
            h1,
            h1 / 2.0,
            Color { A: 50, R: accent_soft[0], G: accent_soft[1], B: accent_soft[2] },
        );
        p.rect(
            c,
            (ox - h2 / 2.0).round(),
            (oy - h2 / 2.0).round(),
            h2,
            h2,
            h2 / 2.0,
            Color { A: 128, R: accent_soft[0], G: accent_soft[1], B: accent_soft[2] },
        );
        p.rect(
            c,
            (ox - core / 2.0).round(),
            (oy - core / 2.0).round(),
            core,
            core,
            core / 2.0,
            white(255),
        );
    }

    // Bottom round summary: "2 h 10 min · 4 sessions"
    let total_m = Timer::total_round_mins();
    let summary = if en {
        format!("{} h {} min \u{00B7} 4 sessions", total_m / 60, total_m % 60)
    } else {
        format!("{} j {} mnt \u{00B7} 4 sesi", total_m / 60, total_m % 60)
    };
    p.text(c, &summary, x_right, top + p.px(114.0), 10.5, 500, WHITE, 0.44, Anchor::Left, rw);
}

fn prayer_tab(p: &mut Painter, c: &ContainerVisual, f: Frame, full_h: f32, d: &Data, live: &mut Live) {
    use nacre_core::notch::prayer::{format_countdown, format_hhmm, Prayer};

    let pad = p.px(EDGE_PAD_DP + 2.0);
    let top = p.px(13.0);
    let bot = f.h - p.px(DOTS_STRIP_DP + 8.0);
    let h = (bot - top).max(p.px(110.0));
    let x_left = f.x0 + pad;
    let lw = p.px(136.0);
    let div_x = x_left + lw + p.px(10.0);
    let x_right = div_x + p.px(10.0);
    let rw = (f.x0 + f.w - pad - x_right + p.px(4.0)).max(p.px(172.0));

    let snap = d.prayer.snap;
    let active_prayer = snap.just_arrived.unwrap_or(snap.next);
    let accent = active_prayer.accent_rgb();
    let grad_end = active_prayer.grad_end_rgb();
    let accent_soft = lift_for_dark_pill(accent);
    let en = crate::prefs::get().lang == nacre_core::i18n::Lang::En;

    // Subtle full-bleed bottom-up OLED atmospheric glow adapting to the active prayer's time of day.
    if let Some(glow) = p.ambient_glow(c, f.x0, 0.0, f.w, full_h, 0.0, accent, false) {
        let _ = glow.SetOpacity(0.56);
        live.tab_glow = Some(glow);
    }

    // --- Left Column: Header, Next + City, Big Prayer Name, Time + Countdown, Gradient Progress Bar ---
    p.text(c, tr("Jadwal Shalat"), x_left, top, 15.0, 700, WHITE, 0.98, Anchor::Left, lw);

    let next_lbl = tr("Berikutnya");
    let sub_y = top + p.px(27.0);
    let nw = p.text(c, next_lbl, x_left, sub_y, 11.5, 500, WHITE, 0.48, Anchor::Left, lw * 0.55);
    let city_x = x_left + nw + p.px(5.0);
    let city_max = (lw - nw - p.px(5.0)).max(p.px(36.0));
    let city_shown = p.fit(d.prayer.city, 11.5, 500, city_max);
    p.text(c, &city_shown, city_x, sub_y, 11.5, 500, accent_soft, 0.92, Anchor::Left, city_max + p.px(2.0));

    let big_name = snap.next.name();
    p.text(c, big_name, x_left, top + p.px(46.0), 27.5, 700, WHITE, 1.0, Anchor::Left, lw + p.px(4.0));

    let time_y = top + p.px(88.0);
    let hhmm = format_hhmm(snap.next_time_min);
    let tw = p.text(c, &hhmm, x_left, time_y, 14.0, 700, WHITE, 0.98, Anchor::Left, lw * 0.48);
    let cd = format_countdown(snap.mins_until, en);
    let cd_x = x_left + tw + p.px(7.0);
    let cd_max = (lw - tw - p.px(7.0)).max(p.px(40.0));
    let cd_shown = p.fit(&cd, 13.0, 600, cd_max);
    p.text(c, &cd_shown, cd_x, time_y + p.px(0.5), 13.0, 600, accent_soft, 0.96, Anchor::Left, cd_max + p.px(2.0));

    let bar_y = top + p.px(116.0);
    let bar_w = (lw - p.px(2.0)).max(p.px(60.0));
    let bar_h = p.px(4.5);
    p.rect(c, x_left, bar_y, bar_w, bar_h, bar_h / 2.0, white(30));
    let fill_w = (bar_w * snap.interval_progress.clamp(0.06, 1.0)).max(bar_h * 1.6).min(bar_w);
    p.gradient_bar(c, x_left, bar_y, fill_w, bar_h, bar_h / 2.0, accent, grad_end);

    // --- Center 1px Vertical Divider ---
    p.divider_v(c, div_x, top + p.px(20.0), (h - p.px(24.0)).max(p.px(76.0)));

    // --- Right Column: Solar Arc + 5-prayer schedule strip (Subuh..Isya) ---
    let arc_y = top + p.px(14.0);
    let arc_h = p.px(76.0);
    p.prayer_arc(c, x_right, arc_y, rw, arc_h, snap.arc_progress, snap.dot_us, accent);

    let col_w = rw / 5.0;
    let name_y = arc_y + arc_h + p.px(4.0);
    let clock_y = name_y + p.px(14.0);
    for (i, pr) in Prayer::ALL.into_iter().enumerate() {
        let cx = x_right + (i as f32 + 0.5) * col_w;
        let is_next = pr == snap.next;
        let wgt = if is_next { 700 } else { 500 };
        let op = if is_next { 1.0 } else { 0.46 };
        let pr_lbl = pr.name();
        p.text(c, pr_lbl, cx, name_y, 8.8, wgt, WHITE, op, Anchor::Center, col_w + p.px(8.0));
        let t_lbl = format_hhmm(snap.schedule.times[i]);
        p.text(c, &t_lbl, cx, clock_y, 8.8, wgt, WHITE, op, Anchor::Center, col_w + p.px(8.0));
    }
}

fn compact_prayer_alert(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>) {
    use nacre_core::notch::prayer::{format_hhmm, AlertKind};

    let Some(al) = d.prayer_alert else { return };
    // Refined Champagne Gold mist for 10m/5m reminders, serene Emerald mist when Adzan arrives.
    let (glow_rgb, accent_soft, glow_op) = match al.kind {
        AlertKind::Now => ([42u8, 198, 138], [126u8, 232, 186], 0.58f32),
        AlertKind::Before5 => ([216u8, 180, 106], [238u8, 210, 148], 0.56f32),
        AlertKind::Before10 => ([216u8, 180, 106], [238u8, 210, 148], 0.48f32),
    };
    let hhmm = format_hhmm(al.time_min);
    let pname = al.prayer.name();

    // Subtle full-bleed bottom-up OLED mist matching the Media Player curve (pure #000000 top -> velvety bottom rim).
    if let Some(glow) = p.ambient_glow(c, f.x0, 0.0, f.w, f.h, 0.0, glow_rgb, false) {
        let _ = glow.SetOpacity(glow_op);
    }

    let cy = f.h / 2.0;
    let badge = p.px(26.0);
    let bx = f.x0 + p.px(12.0);
    p.rect(
        c,
        bx,
        cy - badge / 2.0,
        badge,
        badge,
        badge / 2.0,
        Color { A: 38, R: glow_rgb[0], G: glow_rgb[1], B: glow_rgb[2] },
    );
    p.sym(c, Sym::Moon, bx + badge / 2.0, cy, 13.5, accent_soft, 0.98);

    let tx = bx + badge + p.px(10.0);
    let tw = (f.x0 + f.w - p.px(14.0) - tx).max(p.px(60.0));
    let (head, sub) = match al.kind {
        AlertKind::Now => (
            format!("{} {pname} \u{00B7} {hhmm}", tr("Waktunya")),
            tr("Selamat menunaikan ibadah shalat").to_string(),
        ),
        AlertKind::Before5 => (
            format!("{pname} \u{00B7} {hhmm}"),
            format!("5 {}", tr("menit menuju adzan")),
        ),
        AlertKind::Before10 => (
            format!("{pname} \u{00B7} {hhmm}"),
            format!("10 {}", tr("menit menuju adzan")),
        ),
    };
    p.text(c, &p.fit(&head, 12.0, 700, tw), tx, p.px(7.0), 12.0, 700, WHITE, 0.98, Anchor::Left, tw + p.px(2.0));
    p.text(c, &p.fit(&sub, 11.0, 500, tw), tx, p.px(24.0), 11.0, 500, accent_soft, 0.92, Anchor::Left, tw + p.px(2.0));

    regions.push(Region {
        x: f.x0,
        y: 0.0,
        w: f.w,
        h: f.h,
        action: Action::OpenPrayer,
        vis: None,
        hover_bg: None,
    });
}
