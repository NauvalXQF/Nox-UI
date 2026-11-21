use super::draw::{white, Anchor, Painter, WHITE};
use crate::prefs::tr;
use nacre_core::island::timer::{format_mmss, Phase, Timer};
use nacre_core::notch::glance::Item;
use nacre_core::notch::state::{Activity, Look, Tab, View};
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
    Seek(f32, f32),
    Tab(Tab),
    PrevTab,
    NextTab,
    ShelfItem(usize),
    ShelfClear,
    Zone(usize),
    Pin,
    Settings,
    TimerToggle,
    TimerReset,
    TimerPlus,
    TimerMinus,
    CalDay(i32),
    SendDone,
    NotifFocus(u32),
    NotifDismiss(u32),
    NotifClearAll,
    TodoFocus,
    TodoToggle(usize),
    TodoDelete(usize),
    TodoSync,
    TodoConnect,
    GeminiFocus,
    GeminiSend,
    GeminiStop,
    GeminiCopy,
    GeminiClear,
}

pub struct Region {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub action: Action,
    pub vis: Option<ContainerVisual>,
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
    pub notif_scroll: usize,
    pub notif_status: nacre_platform::notifications::Status,
    pub timer_running: bool,
    pub todos: Vec<&'a nacre_core::notch::todo::TodoItem>,
    pub todo_done: usize,
    pub todo_total: usize,
    pub todo_scroll: usize,
    pub todo_status: nacre_platform::gtasks::SyncStatus,
    pub gemini_text: &'a str,
    pub gemini_streaming: bool,
    pub gemini_error: &'a str,
    pub gemini_has_key: bool,
    pub gemini_scroll: usize,
    pub gemini_model_label: &'static str,
    pub gemini_copied: bool,
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
    pub lrclib: bool,
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
    pub th: f32,
    pub dur_ms: i64,
    pub fill: SpriteVisual,
    pub fill_geo: CompositionRoundedRectangleGeometry,
    pub knob: SpriteVisual,
    pub knob_geo: CompositionRoundedRectangleGeometry,
    pub knob_brush: CompositionColorBrush,
    pub pos_vis: SpriteVisual,
    pub rem_vis: SpriteVisual,
    last_pos_sec: i64,
}

impl ScrubLive {
    pub fn update(&mut self, p: &mut Painter, frac: f32, dragging: bool, pos_ms: i64) {
        let fr = frac.clamp(0.0, 1.0);
        let fw = (self.sw * fr).max(self.th);
        let _ = self.fill.SetSize(Vector2 { X: fw, Y: self.th });
        let _ = self.fill_geo.SetSize(Vector2 { X: fw, Y: self.th });

        let kx = self.sx + self.sw * fr;
        let (kw, kh, alpha) = if dragging {
            (p.px(13.0), p.px(13.0), 255u8)
        } else {
            (p.px(8.0), p.px(8.0), 235u8)
        };
        let _ = self.knob.SetSize(Vector2 { X: kw, Y: kh });
        let _ = self.knob.SetOffset(Vector3 { X: (kx - kw / 2.0).round(), Y: (self.sy - kh / 2.0).round(), Z: 0.0 });
        let _ = self.knob_geo.SetSize(Vector2 { X: kw, Y: kh });
        let _ = self.knob_geo.SetCornerRadius(Vector2 { X: kh / 2.0, Y: kh / 2.0 });
        let _ = self.knob_brush.SetColor(white(alpha));

        let sec = (pos_ms.max(0) + 500) / 1000;
        if sec != self.last_pos_sec {
            self.last_pos_sec = sec;
            let pos_s = mmss_pad(pos_ms);
            if let Some((surf, tw, th)) = p.render_text_surface(&pos_s, 11.0, 500, WHITE, p.px(44.0)) {
                if let Ok(br) = p.comp.CreateSurfaceBrushWithSurface(&surf) {
                    let _ = br.SetStretch(CompositionStretch::None);
                    let _ = br.SetBitmapInterpolationMode(CompositionBitmapInterpolationMode::NearestNeighbor);
                    let _ = self.pos_vis.SetBrush(&br);
                }
                let _ = self.pos_vis.SetSize(Vector2 { X: tw as f32, Y: th as f32 });
                let _ = self.pos_vis.SetOffset(Vector3 {
                    X: self.left_x.round(),
                    Y: (self.sy - th as f32 / 2.0).round(),
                    Z: 0.0,
                });
            }
            let rem_s = format!("-{}", mmss_pad((self.dur_ms - pos_ms).max(0)));
            if let Some((surf, tw, th)) = p.render_text_surface(&rem_s, 11.0, 500, WHITE, p.px(50.0)) {
                if let Ok(br) = p.comp.CreateSurfaceBrushWithSurface(&surf) {
                    let _ = br.SetStretch(CompositionStretch::None);
                    let _ = br.SetBitmapInterpolationMode(CompositionBitmapInterpolationMode::NearestNeighbor);
                    let _ = self.rem_vis.SetBrush(&br);
                }
                let _ = self.rem_vis.SetSize(Vector2 { X: tw as f32, Y: th as f32 });
                let _ = self.rem_vis.SetOffset(Vector3 {
                    X: (self.right_x - tw as f32).round(),
                    Y: (self.sy - th as f32 / 2.0).round(),
                    Z: 0.0,
                });
            }
        }
    }
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

#[derive(Default)]
pub struct Live {
    pub bars: Vec<SpriteVisual>,
    pub marquee: Option<MarqueeLive>,
    pub scrub: Option<ScrubLive>,
    pub drop_zones: Vec<DropZoneLive>,
    pub lyrics_host: Option<LyricsHostLive>,
    pub todo_input_box: Option<(f32, f32, f32, f32)>,
    pub gemini_input_box: Option<(f32, f32, f32, f32)>,
    pub gemini_resp_host: Option<GeminiRespHost>,
    pub lyric: Option<SpriteVisual>,
    pub lyric_scroll: Option<ContainerVisual>,
    pub lyric_shift: f32,
    pub compact_left: Option<ContainerVisual>,
    pub compact_right: Option<ContainerVisual>,
    pub compact_glow: Option<SpriteVisual>,
}

const ORANGE: [u8; 3] = [255, 159, 10];
const GREEN: [u8; 3] = [48, 209, 88];

const NAV_H_DP: f32 = 34.0;
const EDGE_PAD_DP: f32 = 26.0;

pub fn draw(p: &mut Painter, c: &ContainerVisual, look: Look, f: Frame, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    match look.view {
        View::Rest => {}
        View::Compact(Activity::Notification) => compact_notification(p, c, f, d, regions),
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
            match tab {
                Tab::Play => play(p, c, f, d, regions, live),
                Tab::Alerts => alerts_tab(p, c, f, d, regions),
                Tab::Shelf if d.send.is_some() => send_card(p, c, f, d, regions),
                Tab::Shelf if d.shelf.is_empty() => shelf_empty(p, c, f),
                Tab::Shelf => shelf_tab(p, c, f, d, regions),
                Tab::Todo => todo_tab(p, c, f, d, regions, live),
                Tab::Gemini => gemini_tab(p, c, f, d, regions, live),
                Tab::Timer => timer(p, c, f, d, regions),
            }
            edge_chevrons(p, c, f, regions);
            top_pin(p, c, f, d, regions);
            bottom_nav(p, c, f, tab, d, regions);
        }
        View::Drop => drop_zones(p, c, f, d, regions, live),
    }
    if matches!(look.view, View::Rest | View::Compact(_)) {
        privacy_dots(p, c, f, d.privacy);
    }
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

fn top_pin(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>) {
    let px_btn = f.x0 + f.w - p.px(16.0);
    let py_btn = p.px(16.0);
    let n0 = kids(c);
    let pin_rgb = if d.pinned { crate::prefs::accent() } else { WHITE };
    p.sym(c, Sym::Pin, px_btn, py_btn, 12.0, pin_rgb, if d.pinned { 1.0 } else { 0.34 });
    regions.push(Region {
        x: px_btn - p.px(12.0),
        y: py_btn - p.px(12.0),
        w: p.px(24.0),
        h: p.px(24.0),
        action: Action::Pin,
        vis: group_since(c, n0),
    });
}

fn edge_chevrons(p: &mut Painter, c: &ContainerVisual, f: Frame, regions: &mut Vec<Region>) {
    let cy = (f.h - p.px(NAV_H_DP)) / 2.0 + p.px(4.0);
    let lx = f.x0 + p.px(12.0);
    let rx = f.x0 + f.w - p.px(12.0);
    let hit_w = p.px(22.0);
    let hit_h = p.px(56.0);

    let n0 = kids(c);
    p.sym(c, Sym::ChevronLeft, lx, cy, 11.5, WHITE, 0.34);
    regions.push(Region {
        x: lx - hit_w / 2.0,
        y: cy - hit_h / 2.0,
        w: hit_w,
        h: hit_h,
        action: Action::PrevTab,
        vis: group_since(c, n0),
    });

    let n1 = kids(c);
    p.sym(c, Sym::ChevronRight, rx, cy, 11.5, WHITE, 0.34);
    regions.push(Region {
        x: rx - hit_w / 2.0,
        y: cy - hit_h / 2.0,
        w: hit_w,
        h: hit_h,
        action: Action::NextTab,
        vis: group_since(c, n1),
    });
}

fn bottom_nav(p: &mut Painter, c: &ContainerVisual, f: Frame, cur: Tab, d: &Data, regions: &mut Vec<Region>) {
    let items: [(Option<Tab>, Sym); 7] = [
        (Some(Tab::Play), Sym::Note),
        (Some(Tab::Alerts), Sym::Bell),
        (Some(Tab::Shelf), Sym::Tray),
        (Some(Tab::Todo), Sym::Checklist),
        (Some(Tab::Gemini), Sym::Sparkle),
        (Some(Tab::Timer), Sym::Stopwatch),
        (None, Sym::Gear),
    ];
    let slot_w = p.px(28.0);
    let pill_h = p.px(22.0);
    let pad_x = p.px(5.0);
    let sep_w = p.px(7.0);
    let total_w = items.len() as f32 * slot_w + sep_w + 2.0 * pad_x;
    let cx = f.x0 + f.w / 2.0;
    let cy = f.h - p.px(16.0);
    let x0 = (cx - total_w / 2.0).round();
    let y0 = (cy - pill_h / 2.0).round();

    p.rect(c, x0, y0, total_w, pill_h, pill_h / 2.0, white(16));

    let mut x = x0 + pad_x;
    let accent = crate::prefs::accent();
    for (idx, (tab_opt, sym)) in items.into_iter().enumerate() {
        if idx == 6 {
            p.divider_v(c, x + sep_w / 2.0, y0 + p.px(5.0), pill_h - p.px(10.0));
            x += sep_w;
        }
        let on = tab_opt == Some(cur);
        let icx = x + slot_w / 2.0;
        let n0 = kids(c);
        if on {
            let iw = slot_w - p.px(2.0);
            let ih = pill_h - p.px(4.0);
            p.rect(c, (x + p.px(1.0)).round(), (y0 + p.px(2.0)).round(), iw, ih, ih / 2.0, white(44));
        }
        let (rgb, op) = match tab_opt {
            Some(Tab::Timer) if d.timer_running => (ORANGE, 1.0),
            Some(_) if on => (WHITE, 1.0),
            _ => (WHITE, 0.48),
        };
        p.sym(c, sym, icx, cy, 11.5, rgb, op);

        if tab_opt == Some(Tab::Alerts) && d.notif_unread > 0 {
            let dot = p.px(4.5);
            p.rect(c, icx + p.px(5.0), cy - p.px(7.5), dot, dot, dot / 2.0, Color { A: 255, R: accent[0], G: accent[1], B: accent[2] });
        } else if tab_opt == Some(Tab::Shelf) && !d.shelf.is_empty() && !on {
            let dot = p.px(3.5);
            p.rect(c, icx + p.px(5.5), cy - p.px(6.5), dot, dot, dot / 2.0, white(150));
        }

        let act = match tab_opt {
            Some(t) => Action::Tab(t),
            None => Action::Settings,
        };
        regions.push(Region {
            x,
            y: y0 - p.px(3.0),
            w: slot_w,
            h: pill_h + p.px(6.0),
            action: act,
            vis: group_since(c, n0),
        });
        x += slot_w;
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
    regions.push(Region { x: dx - p.px(12.0), y: cy - p.px(12.0), w: p.px(24.0), h: p.px(24.0), action: Action::NotifDismiss(n.id), vis: group_since(c, n0) });
    regions.push(Region { x: f.x0, y: 0.0, w: dx - p.px(12.0) - f.x0, h: f.h, action: Action::NotifFocus(n.id), vis: None });
}

fn alerts_tab(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>) {
    use nacre_platform::notifications::Status;
    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(12.0);
    let bot = f.h - p.px(NAV_H_DP + 4.0);
    let h = (bot - top).max(p.px(40.0));
    let (x0, w) = (f.x0 + pad, f.w - 2.0 * pad);
    let accent = crate::prefs::accent();

    if d.notif_list.is_empty() {
        p.text(c, tr("Notifikasi"), x0, top, 14.5, 700, WHITE, 0.96, Anchor::Left, w * 0.6);
        let cx = x0 + w / 2.0;
        let cy = top + h / 2.0 + p.px(4.0);
        p.sym(c, Sym::Bell, cx, cy - p.px(20.0), 22.0, WHITE, 0.34);
        p.text(c, tr("Belum ada notifikasi"), cx, cy + p.px(4.0), 13.0, 600, WHITE, 0.88, Anchor::Center, w - p.px(12.0));
        let sub = match d.notif_status {
            Status::NeedsIdentity => tr("Jalankan packaging\\register-sparse.ps1 untuk membaca notifikasi Windows."),
            Status::Denied => tr("Aktifkan izin notifikasi untuk ABRA di Pengaturan Privasi Windows."),
            _ => tr("Notifikasi baru dari aplikasi Windows akan muncul di sini."),
        };
        p.text(c, &p.fit(sub, 11.0, 400, w - p.px(12.0)), cx, cy + p.px(24.0), 11.0, 400, WHITE, 0.46, Anchor::Center, w);
        return;
    }

    let hdr = format!("{} ({})", tr("Notifikasi"), d.notif_list.len());
    p.text(c, &hdr, x0, top, 14.0, 700, WHITE, 0.96, Anchor::Left, w * 0.6);
    let clear = tr("Kosongkan");
    let cw = p.measure(clear, 11.5, 600) + p.px(4.0);
    let clr_x = x0 + w - p.px(8.0);
    p.text(c, clear, clr_x, top + p.px(2.0), 11.5, 600, accent, 0.95, Anchor::Right, cw + 4.0);
    regions.push(Region { x: clr_x - cw - p.px(6.0), y: top - p.px(2.0), w: cw + p.px(12.0), h: p.px(24.0), action: Action::NotifClearAll, vis: None });

    let row_h = p.px(42.0);
    let gap = p.px(5.0);
    let list_top = top + p.px(28.0);
    let max_rows = ((bot - list_top) / (row_h + gap)).floor().max(1.0) as usize;
    let start = d.notif_scroll.min(d.notif_list.len().saturating_sub(1));
    let isz = p.px(24.0);
    for (idx, n) in d.notif_list.iter().skip(start).take(max_rows).enumerate() {
        let ry = list_top + idx as f32 * (row_h + gap);
        let n0 = kids(c);
        p.rect(c, x0, ry, w, row_h, p.px(12.0), white(if n.unread { 18 } else { 9 }));
        let ix = x0 + p.px(10.0);
        let icy = ry + row_h / 2.0;
        match n.icon {
            Some(s) => {
                p.image(c, s, ix, icy - isz / 2.0, isz, isz, p.px(6.0));
            }
            None => {
                p.rect(c, ix, icy - isz / 2.0, isz, isz, p.px(6.0), Color { A: 200, R: accent[0], G: accent[1], B: accent[2] });
                p.sym(c, Sym::Bell, ix + isz / 2.0, icy, 12.0, WHITE, 0.95);
            }
        }
        let close_cx = x0 + w - p.px(16.0);
        let time_x = close_cx - p.px(14.0);
        let tx = ix + isz + p.px(10.0);
        let tw = (time_x - p.px(36.0) - tx).max(p.px(40.0));
        let head = if n.app_name.is_empty() || n.title.eq_ignore_ascii_case(n.app_name) {
            n.title.to_string()
        } else {
            format!("{} \u{00B7} {}", n.app_name, n.title)
        };
        p.text(c, &p.fit(&head, 12.0, 600, tw), tx, ry + p.px(6.0), 12.0, 600, WHITE, 0.96, Anchor::Left, tw + p.px(2.0));
        if !n.body.is_empty() {
            p.text(c, &p.fit(n.body, 11.0, 400, tw), tx, ry + p.px(22.0), 11.0, 400, WHITE, 0.58, Anchor::Left, tw + p.px(2.0));
        }
        p.text(c, &n.time, time_x, ry + p.px(7.0), 10.0, 500, WHITE, 0.44, Anchor::Right, p.px(40.0));
        let row_vis = group_since(c, n0);
        let c0 = kids(c);
        p.sym(c, Sym::Close, close_cx, icy, 9.0, WHITE, 0.48);
        regions.push(Region { x: close_cx - p.px(12.0), y: ry, w: p.px(24.0), h: row_h, action: Action::NotifDismiss(n.id), vis: group_since(c, c0) });
        regions.push(Region { x: x0, y: ry, w: close_cx - p.px(12.0) - x0, h: row_h, action: Action::NotifFocus(n.id), vis: row_vis });
    }
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
        regions.push(Region { x, y: top, w: zw, h: zh, action: Action::Zone(i), vis: None });
    }
    if d.drop_count > 0 {
        let th = p.px(10.5) * 1.3;
        p.text(c, &count_text(d.drop_count), f.x0 + f.w / 2.0, f.h - pad / 2.0 - th / 2.0 - p.px(1.0), 10.5, 500, WHITE, 0.55, Anchor::Center, f.w);
    }
}

fn send_card(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>) {
    let Some(sv) = &d.send else { return };
    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(14.0);
    let h = (f.h - p.px(NAV_H_DP + 6.0) - top).max(p.px(80.0));
    let (x0, w) = (f.x0 + pad, f.w - 2.0 * pad);
    let s = (h - p.px(8.0)).min(p.px(116.0));
    let (qx, qy) = (x0, top + (h - s) / 2.0);
    if sv.error {
        p.rect(c, qx, qy, s, s, p.px(14.0), white(16));
        p.sym(c, Sym::Wifi, qx + s / 2.0, qy + s / 2.0, 28.0, WHITE, 0.45);
    } else {
        p.rect(c, qx, qy, s, s, p.px(14.0), Color { A: 255, R: 255, G: 255, B: 255 });
        if let Some(q) = sv.qr {
            let inset = ((s - sv.qr_px) / 2.0).round();
            p.place(c, q, qx + inset, qy + inset, sv.qr_px, sv.qr_px, 1.0);
        }
    }
    let tx = qx + s + p.px(18.0);
    let tw = x0 + w - tx;
    let accent = crate::prefs::accent();
    if sv.error {
        p.text(c, tr("Tidak tersambung ke jaringan lokal"), tx, top + p.px(14.0), 14.0, 700, WHITE, 1.0, Anchor::Left, tw);
        p.text(c, tr("Sambungkan laptop ke Wi-Fi yang sama dengan HP."), tx, top + p.px(38.0), 12.0, 500, WHITE, 0.56, Anchor::Left, tw);
    } else {
        p.text(c, &p.fit(&sv.title, 14.0, 700, tw), tx, top + p.px(8.0), 14.0, 700, WHITE, 1.0, Anchor::Left, tw + p.px(4.0));
        p.text(c, &p.fit(tr("Pindai QR dengan kamera HP (Wi-Fi yang sama)"), 11.5, 500, tw), tx, top + p.px(30.0), 11.5, 500, WHITE, 0.56, Anchor::Left, tw + p.px(4.0));
        let green = Color { A: 255, R: GREEN[0], G: GREEN[1], B: GREEN[2] };
        p.text(c, &sv.status, tx, top + p.px(52.0), 12.0, 600, if sv.done { GREEN } else { WHITE }, 0.95, Anchor::Left, tw);
        let (by, bh) = (top + p.px(74.0), p.px(4.0));
        let bar = (tw - p.measure(tr("Selesai"), 12.0, 600) - p.px(34.0)).max(p.px(40.0));
        p.rect(c, tx, by, bar, bh, bh / 2.0, white(30));
        if sv.frac > 0.0 {
            p.rect(c, tx, by, (bar * sv.frac.clamp(0.0, 1.0)).max(bh), bh, bh / 2.0, green);
        }
        p.text(c, tr("Izinkan ABRA di Windows Firewall bila diminta."), tx, top + h - p.px(18.0), 10.0, 500, WHITE, 0.38, Anchor::Left, tw - p.px(92.0));
    }
    let label = tr("Selesai");
    let (bw, bh) = (p.measure(label, 12.0, 600) + p.px(26.0), p.px(26.0));
    let (bx, by) = (x0 + w - bw, top + h - bh - p.px(4.0));
    p.rect(c, bx, by, bw, bh, bh / 2.0, Color { A: 255, R: accent[0], G: accent[1], B: accent[2] });
    let th = p.px(12.0) * 1.3;
    p.text(c, label, bx + bw / 2.0, by + bh / 2.0 - th / 2.0, 12.0, 600, WHITE, 1.0, Anchor::Center, bw);
    regions.push(Region { x: bx, y: by, w: bw, h: bh, action: Action::SendDone, vis: None });
}

fn shelf_tab(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>) {
    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(12.0);
    let (x0, w) = (f.x0 + pad, f.w - 2.0 * pad);
    let hdr = format!("{} \u{00B7} {}", tr("Rak"), count_text(d.shelf.len()));
    p.text(c, &hdr, x0, top, 14.5, 700, WHITE, 0.96, Anchor::Left, w * 0.65);

    let clear = tr("Kosongkan");
    let cw = p.measure(clear, 11.5, 600) + p.px(4.0);
    let accent = crate::prefs::accent();
    let clr_x = x0 + w - p.px(8.0);
    p.text(c, clear, clr_x, top + p.px(2.0), 11.5, 600, accent, 0.95, Anchor::Right, cw + 4.0);
    regions.push(Region { x: clr_x - cw - p.px(6.0), y: top - p.px(2.0), w: cw + p.px(12.0), h: p.px(24.0), action: Action::ShelfClear, vis: None });

    let (tw, th, gap) = (p.px(88.0), p.px(86.0), p.px(8.0));
    let ty = top + p.px(32.0);
    let max = ((w + gap) / (tw + gap)).floor().max(1.0) as usize;
    let extra = d.shelf.len().saturating_sub(max);
    for (i, (name, icon)) in d.shelf.iter().take(max).enumerate() {
        let x = x0 + i as f32 * (tw + gap);
        if extra > 0 && i == max - 1 {
            p.rect(c, x, ty, tw, th, p.px(14.0), white(12));
            let t = p.px(13.0) * 1.3;
            p.text(c, &format!("+{}", extra + 1), x + tw / 2.0, ty + th / 2.0 - t / 2.0, 13.0, 600, WHITE, 0.72, Anchor::Center, tw);
            break;
        }
        let n0 = kids(c);
        p.rect(c, x, ty, tw, th, p.px(14.0), white(10));
        let isz = p.px(40.0);
        match icon {
            Some(s) => {
                p.image(c, s, x + (tw - isz) / 2.0, ty + p.px(10.0), isz, isz, p.px(6.0));
            }
            None => p.sym(c, Sym::Folder, x + tw / 2.0, ty + p.px(30.0), 26.0, WHITE, 0.7),
        }
        let shown = p.fit(name, 11.0, 500, tw - p.px(10.0));
        p.text(c, &shown, x + tw / 2.0, ty + p.px(58.0), 11.0, 500, WHITE, 0.9, Anchor::Center, tw - p.px(6.0));
        regions.push(Region { x, y: ty, w: tw, h: th, action: Action::ShelfItem(i), vis: group_since(c, n0) });
    }
}

fn shelf_empty(p: &mut Painter, c: &ContainerVisual, f: Frame) {
    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(12.0);
    let (x0, w) = (f.x0 + pad, f.w - 2.0 * pad);
    p.text(c, tr("Rak"), x0, top, 14.5, 700, WHITE, 0.96, Anchor::Left, w * 0.5);

    let zy = top + p.px(26.0);
    let zh = (f.h - p.px(NAV_H_DP + 6.0) - zy).max(p.px(64.0));
    p.dashed_rect(c, x0, zy, w, zh, p.px(14.0), white(56));

    let cx = x0 + w / 2.0;
    let cy = zy + zh / 2.0;
    p.sym(c, Sym::Tray, cx, cy - p.px(16.0), 22.0, WHITE, 0.8);
    p.text(c, tr("Taruh berkas di sini"), cx, cy + p.px(2.0), 13.0, 600, WHITE, 0.94, Anchor::Center, w - p.px(16.0));
    p.text(c, tr("Seret keluar kapan saja · klik untuk buka"), cx, cy + p.px(20.0), 10.5, 400, WHITE, 0.48, Anchor::Center, w - p.px(16.0));
}

fn compact_timer(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, urgent: bool) {
    let cy = f.h / 2.0;
    let rgb = if urgent { ORANGE } else { WHITE };
    p.sym(c, Sym::Stopwatch, f.x0 + p.px(20.0), cy, 15.0, rgb, 0.95);
    let t = format_mmss(d.timer.remaining_secs(d.now_ms));
    let th = p.px(13.0) * 1.3;
    p.text(c, &t, f.x0 + f.w - p.px(16.0), cy - th / 2.0, 13.0, 600, rgb, 1.0, Anchor::Right, f.w);
}

fn accent_or(t: Option<[u8; 3]>) -> [u8; 3] {
    t.unwrap_or_else(crate::prefs::accent)
}

fn current_line(d: &Data) -> String {
    match d.lyrics.lines {
        Some(l) => l.current(d.lyrics.pos_ms).unwrap_or("\u{266A}").to_string(),
        None => String::new(),
    }
}

fn compact_lyrics(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, live: &mut Live) {
    compact_music(p, c, f, d, live, f.h, false);
    let left = f.x0 + p.px(42.0);
    let right = f.x0 + f.w - p.px(50.0);
    let vw = (right - left).max(p.px(24.0));
    let line = current_line(d);
    let full_w = p.measure_dwrite(&line, 13.0, 600).max(p.measure(&line, 13.0, 600));
    let cy = f.h / 2.0;

    if full_w <= vw + p.px(2.0) {
        if let Some((surf, tw, th)) = p.render_text_surface(&line, 13.0, 600, WHITE, vw + p.px(6.0)) {
            let x = ((left + right - tw as f32) / 2.0).round();
            let y = (cy - th as f32 / 2.0).round();
            live.lyric = p.place(c, &surf, x, y, tw as f32, th as f32, 1.0);
        }
    } else if let (Ok(vp), Ok(cl)) = (p.comp.CreateContainerVisual(), p.comp.CreateInsetClip()) {
        let _ = vp.SetOffset(Vector3 { X: left.round(), Y: 0.0, Z: 0.0 });
        let _ = vp.SetSize(Vector2 { X: vw.ceil(), Y: f.h });
        let _ = vp.SetClip(&cl);
        let _ = c.Children().and_then(|k| k.InsertAtTop(&vp));
        if let Some((surf, tw, th)) = p.render_text_surface(&line, 13.0, 600, WHITE, full_w + p.px(12.0)) {
            let ty = (cy - th as f32 / 2.0).round();
            let travel = (tw as f32 - vw).max(0.0);
            if let Some(v) = p.place(&vp, &surf, 0.0, ty, tw as f32, th as f32, 1.0) {
                live.lyric = Some(v.clone());
                if travel > 2.0 {
                    live.marquee = Some(MarqueeLive { vis: v, travel, base_y: ty });
                }
            }
        }
    }
}

fn compact_player(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    let Some(m) = &d.media else { return };
    let glow = accent_or(m.tint);
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
        regions.push(Region { x: cx - hit / 2.0, y: cy - hit / 2.0, w: hit, h: hit, action: act, vis: group_since(c, n0) });
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
    let rgb = accent_or(m.tint);
    live.compact_glow = p.ambient_glow(c, f.x0, 0.0, f.w, row_h, 0.0, rgb, true);

    let cy = row_h / 2.0;
    let a = p.px(20.0);

    // Edge-anchored left container so album art glides smoothly with animated spring width.
    if let Ok(left_c) = p.comp.CreateContainerVisual() {
        let _ = left_c.SetOffset(Vector3 { X: f.x0.round(), Y: 0.0, Z: 0.0 });
        let _ = c.Children().and_then(|k| k.InsertAtTop(&left_c));
        match m.art {
            Some(s) => {
                p.image(&left_c, s, p.px(12.0), cy - a / 2.0, a, a, p.px(6.0));
            }
            None => {
                p.rect(&left_c, p.px(12.0), cy - a / 2.0, a, a, p.px(6.0), white(30));
                p.sym(&left_c, Sym::Note, p.px(12.0) + a / 2.0, cy, 12.0, WHITE, 0.85);
            }
        }
        live.compact_left = Some(left_c);
    }

    let (bw, gap, bh) = (p.px(3.0), p.px(3.0), p.px(13.0));
    let vis_w = 5.0 * bw + 4.0 * gap;
    let right_pad = p.px(14.0);

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

    // Edge-anchored right container so 5-bar visualizer glides smoothly with animated spring width.
    if let Ok(right_c) = p.comp.CreateContainerVisual() {
        let _ = right_c.SetOffset(Vector3 { X: (f.x0 + f.w).round(), Y: 0.0, Z: 0.0 });
        let _ = c.Children().and_then(|k| k.InsertAtTop(&right_c));
        let bx0 = -(right_pad + vis_w);
        for i in 0..5 {
            if let Some(v) = p.rect(&right_c, bx0 + i as f32 * (bw + gap), cy - bh / 2.0, bw, bh, bw / 2.0, Color { A: 240, R: rgb[0], G: rgb[1], B: rgb[2] }) {
                let _ = v.SetCenterPoint(Vector3 { X: bw / 2.0, Y: bh / 2.0, Z: 0.0 });
                let _ = v.SetScale(Vector3 { X: 1.0, Y: 0.28, Z: 1.0 });
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
    lyrics_open: bool,
    regions: &mut Vec<Region>,
    live: &mut Live,
) {
    let art = p.px(54.0);
    let (ax, ay) = (x, y);
    match m.art {
        Some(s) => {
            p.image(c, s, ax, ay, art, art, p.px(13.0));
        }
        None => {
            p.rect(c, ax, ay, art, art, p.px(13.0), white(24));
            p.sym(c, Sym::Note, ax + art / 2.0, ay + art / 2.0, 22.0, WHITE, 0.8);
        }
    }
    let tx = ax + art + p.px(12.0);
    let tw = (x + w - tx).max(p.px(40.0));
    let title = p.fit(m.title, 14.5, 700, tw);
    p.text(c, &title, tx, ay + p.px(9.0), 14.5, 700, WHITE, 1.0, Anchor::Left, tw + 2.0);
    let artist = p.fit(m.artist, 12.0, 500, tw);
    p.text(c, &artist, tx, ay + p.px(30.0), 12.0, 500, WHITE, 0.56, Anchor::Left, tw + 2.0);

    // Inline Nusa scrub row: `00:34  [=====-----]  -02:27`
    let sy = ay + art + p.px(20.0);
    let left_time_w = p.px(40.0);
    let right_time_w = p.px(46.0);
    let sx = x + left_time_w;
    let sw = (w - left_time_w - right_time_w).max(p.px(40.0));
    let th = p.px(4.5);
    let frac = m.drag.unwrap_or(if m.dur_ms > 0 { m.pos_ms as f32 / m.dur_ms as f32 } else { 0.0 }).clamp(0.0, 1.0);
    p.rect(c, sx, (sy - th / 2.0).round(), sw, th, th / 2.0, white(58));

    let pos = if let Some(fr) = m.drag { (m.dur_ms as f32 * fr) as i64 } else { m.pos_ms };
    let build_scrub = || -> windows_core::Result<ScrubLive> {
        let fill_geo = p.comp.CreateRoundedRectangleGeometry()?;
        fill_geo.SetCornerRadius(Vector2 { X: th / 2.0, Y: th / 2.0 })?;
        let fill_clip = p.comp.CreateGeometricClip()?;
        fill_clip.SetGeometry(&fill_geo)?;
        let fill = p.comp.CreateSpriteVisual()?;
        fill.SetOffset(Vector3 { X: sx, Y: (sy - th / 2.0).round(), Z: 0.0 })?;
        fill.SetBrush(&p.comp.CreateColorBrushWithColor(white(245))?)?;
        fill.SetClip(&fill_clip)?;
        c.Children()?.InsertAtTop(&fill)?;

        let knob_geo = p.comp.CreateRoundedRectangleGeometry()?;
        let knob_clip = p.comp.CreateGeometricClip()?;
        knob_clip.SetGeometry(&knob_geo)?;
        let knob_brush = p.comp.CreateColorBrushWithColor(white(240))?;
        let knob = p.comp.CreateSpriteVisual()?;
        knob.SetBrush(&knob_brush)?;
        knob.SetClip(&knob_clip)?;
        c.Children()?.InsertAtTop(&knob)?;

        let pos_vis = p.comp.CreateSpriteVisual()?;
        pos_vis.SetOpacity(0.76)?;
        c.Children()?.InsertAtTop(&pos_vis)?;
        let rem_vis = p.comp.CreateSpriteVisual()?;
        rem_vis.SetOpacity(0.76)?;
        c.Children()?.InsertAtTop(&rem_vis)?;

        Ok(ScrubLive {
            left_x: x,
            right_x: x + w,
            sx,
            sw,
            sy,
            th,
            dur_ms: m.dur_ms,
            fill,
            fill_geo,
            knob,
            knob_geo,
            knob_brush,
            pos_vis,
            rem_vis,
            last_pos_sec: -1,
        })
    };
    if let Ok(mut sl) = build_scrub() {
        sl.update(p, frac, m.drag.is_some(), pos);
        live.scrub = Some(sl);
    }

    if m.can_seek && m.dur_ms > 0 {
        regions.push(Region { x: sx - p.px(6.0), y: sy - p.px(12.0), w: sw + p.px(12.0), h: p.px(24.0), action: Action::Seek(sx, sw), vis: None });
    }

    // Playback controls row + Lyrics Quote toggle icon (exact Nusa ordering: Quote | << | || | >>)
    let cy = (y + h - p.px(16.0)).max(sy + p.px(24.0));
    let step = p.px(42.0);
    let cx = x + w / 2.0;
    let controls = [
        (Sym::Quote, cx - 1.5 * step, 15.5, Action::ToggleLyrics, if lyrics_open { 1.0 } else { 0.42 }),
        (Sym::Backward, cx - 0.5 * step, 16.5, Action::Prev, 0.96),
        (if m.playing { Sym::Pause } else { Sym::Play }, cx + 0.5 * step, 21.0, Action::PlayPause, 1.0),
        (Sym::Forward, cx + 1.5 * step, 16.5, Action::Next, 0.96),
    ];
    for (s, bx, size, a, op) in controls {
        let n0 = kids(c);
        p.sym(c, s, bx, cy, size, WHITE, op);
        regions.push(Region { x: bx - p.px(18.0), y: cy - p.px(15.0), w: p.px(36.0), h: p.px(30.0), action: a, vis: group_since(c, n0) });
    }
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
    let top = p.px(12.0);
    let bot = f.h - p.px(NAV_H_DP + 6.0);
    let h = (bot - top).max(p.px(80.0));
    let (x0, w) = (f.x0 + pad, f.w - 2.0 * pad);
    let accent = crate::prefs::accent();

    // Nusa-inspired 2-column OLED layout: left summary + sync pill | 1px divider | right input + task list.
    let lw = p.px(162.0);
    let div_x = x0 + lw + p.px(12.0);
    let rx = div_x + p.px(14.0);
    let rw = (x0 + w - rx).max(p.px(120.0));

    // Left column: Title ("To-Do") + big counter ("0/1") + subtitle + Google sync pill.
    p.text(c, tr("To-Do"), x0, top, 14.5, 700, GREEN, 0.96, Anchor::Left, lw);
    let ratio = format!("{}/{}", d.todo_done, d.todo_total);
    p.text(c, &ratio, x0, top + p.px(28.0), 32.0, 700, WHITE, 1.0, Anchor::Left, lw);
    p.text(c, tr("Selesai hari ini"), x0, top + p.px(74.0), 11.5, 500, WHITE, 0.50, Anchor::Left, lw);

    let (btn_label, btn_act, btn_fill) = match d.todo_status {
        SyncStatus::Synced => (tr("Tersinkron"), Action::TodoSync, Color { A: 48, R: GREEN[0], G: GREEN[1], B: GREEN[2] }),
        SyncStatus::Syncing => (tr("Sinkronisasi…"), Action::TodoSync, white(22)),
        SyncStatus::Connecting => (tr("Menyambung…"), Action::TodoSync, white(22)),
        SyncStatus::NeedsClientConfig => (tr("Butuh Client ID"), Action::Settings, white(20)),
        SyncStatus::Disconnected | SyncStatus::Error => (tr("Sambungkan"), Action::TodoConnect, Color { A: 215, R: accent[0], G: accent[1], B: accent[2] }),
    };
    let bh = p.px(28.0);
    let bw = (lw - p.px(8.0)).max(p.px(100.0));
    let by = bot - bh - p.px(6.0);
    let n0 = kids(c);
    p.rect(c, x0, by, bw, bh, bh / 2.0, btn_fill);
    let th = p.px(11.0) * 1.3;
    p.text(c, &p.fit(btn_label, 11.0, 600, bw - p.px(14.0)), x0 + bw / 2.0, by + bh / 2.0 - th / 2.0, 11.0, 600, WHITE, 0.96, Anchor::Center, bw - p.px(10.0));
    regions.push(Region { x: x0, y: by, w: bw, h: bh, action: btn_act, vis: group_since(c, n0) });

    p.divider_v(c, div_x, top + p.px(6.0), h - p.px(12.0));

    // Right column: retained input + checklist rows.
    let (iy, ih) = (top, p.px(28.0));
    let iw = rw - p.px(8.0);
    live.todo_input_box = Some((rx, iy, iw, ih));
    regions.push(Region { x: rx, y: iy, w: iw, h: ih, action: Action::TodoFocus, vis: None });

    let list_top = iy + ih + p.px(8.0);
    if d.todos.is_empty() {
        let cx = rx + rw / 2.0;
        let cy = list_top + (bot - list_top) / 2.0;
        p.text(c, tr("Belum ada tugas"), cx, cy - p.px(12.0), 12.5, 600, WHITE, 0.78, Anchor::Center, rw);
        p.text(c, &p.fit(tr("Ketik di atas dan tekan Enter untuk menambah tugas."), 10.5, 400, rw), cx, cy + p.px(6.0), 10.5, 400, WHITE, 0.44, Anchor::Center, rw);
        return;
    }

    let row_h = p.px(32.0);
    let gap = p.px(4.0);
    let max_rows = ((bot - list_top) / (row_h + gap)).floor().max(1.0) as usize;
    let start = d.todo_scroll.min(d.todos.len().saturating_sub(1));
    let chk = p.px(15.0);
    for (idx, it) in d.todos.iter().enumerate().skip(start).take(max_rows) {
        let vis_idx = idx - start;
        let ry = list_top + vis_idx as f32 * (row_h + gap);
        let n0 = kids(c);
        p.rect(c, rx, ry, rw, row_h, p.px(10.0), white(if it.completed { 7 } else { 13 }));
        let cx = rx + p.px(15.0);
        let cy = ry + row_h / 2.0;
        if it.completed {
            p.rect(c, cx - chk / 2.0, cy - chk / 2.0, chk, chk, chk / 2.0, Color { A: 240, R: GREEN[0], G: GREEN[1], B: GREEN[2] });
            p.sym(c, Sym::Check, cx, cy, 9.5, WHITE, 1.0);
        } else {
            p.rect(c, cx - chk / 2.0, cy - chk / 2.0, chk, chk, chk / 2.0, white(64));
            p.rect(c, cx - chk / 2.0 + p.px(1.5), cy - chk / 2.0 + p.px(1.5), chk - p.px(3.0), chk - p.px(3.0), (chk - p.px(3.0)) / 2.0, Color { A: 255, R: 0, G: 0, B: 0 });
        }
        let del_cx = rx + rw - p.px(15.0);
        let mut right_edge = del_cx - p.px(14.0);
        if let Some(due) = &it.due {
            let dw = p.measure(due, 10.0, 500) + p.px(6.0);
            let dth = p.px(10.0) * 1.3;
            p.text(c, due, right_edge, cy - dth / 2.0, 10.0, 500, WHITE, 0.45, Anchor::Right, dw);
            right_edge -= dw + p.px(4.0);
        }
        let tx = cx + chk / 2.0 + p.px(9.0);
        let tw = (right_edge - tx).max(p.px(40.0));
        let tth = p.px(12.0) * 1.3;
        p.text(c, &p.fit(&it.title, 12.0, if it.completed { 400 } else { 500 }, tw), tx, cy - tth / 2.0, 12.0, if it.completed { 400 } else { 500 }, WHITE, if it.completed { 0.38 } else { 0.95 }, Anchor::Left, tw + p.px(2.0));
        let row_vis = group_since(c, n0);
        let d0 = kids(c);
        p.sym(c, Sym::Close, del_cx, cy, 8.5, WHITE, 0.42);
        regions.push(Region { x: del_cx - p.px(12.0), y: ry, w: p.px(24.0), h: row_h, action: Action::TodoDelete(idx), vis: group_since(c, d0) });
        regions.push(Region { x: rx, y: ry, w: del_cx - p.px(12.0) - rx, h: row_h, action: Action::TodoToggle(idx), vis: row_vis });
    }
}

fn gemini_tab(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(12.0);
    let bot = f.h - p.px(NAV_H_DP + 6.0);
    let (x0, w) = (f.x0 + pad, f.w - 2.0 * pad);
    let accent = crate::prefs::accent();

    let (iy, ih) = (top, p.px(30.0));
    let btn_w = p.px(74.0);
    let (ix, iw) = (x0, w - p.px(16.0) - btn_w);
    live.gemini_input_box = Some((ix, iy, iw, ih));
    regions.push(Region { x: ix, y: iy, w: iw, h: ih, action: Action::GeminiFocus, vis: None });

    let (bx, by) = (ix + iw + p.px(8.0), iy);
    let n0 = kids(c);
    let (label, act, fill) = if d.gemini_streaming {
        (tr("Hentikan"), Action::GeminiStop, Color { A: 225, R: 255, G: 69, B: 58 })
    } else {
        (tr("Kirim"), Action::GeminiSend, Color { A: 235, R: accent[0], G: accent[1], B: accent[2] })
    };
    p.rect(c, bx, by, btn_w, ih, ih / 2.0, fill);
    let th = p.px(11.5) * 1.3;
    p.text(c, label, bx + btn_w / 2.0, by + ih / 2.0 - th / 2.0, 11.5, 600, WHITE, 1.0, Anchor::Center, btn_w);
    regions.push(Region { x: bx, y: by, w: btn_w, h: ih, action: act, vis: group_since(c, n0) });

    let bar_y = iy + ih + p.px(8.0);
    p.sym(c, Sym::Sparkle, x0 + p.px(7.0), bar_y + p.px(7.0), 12.0, accent, 0.95);
    let st_text = if d.gemini_streaming { tr("Berpikir…") } else { d.gemini_model_label };
    p.text(c, st_text, x0 + p.px(18.0), bar_y, 10.5, 600, WHITE, 0.52, Anchor::Left, w * 0.5);
    if !d.gemini_text.is_empty() {
        let clr = tr("Kosongkan");
        let clw = p.measure(clr, 10.5, 600) + p.px(4.0);
        let clx = x0 + w - p.px(4.0);
        p.text(c, clr, clx, bar_y, 10.5, 600, WHITE, 0.52, Anchor::Right, clw + 4.0);
        regions.push(Region { x: clx - clw - p.px(4.0), y: bar_y - p.px(4.0), w: clw + p.px(8.0), h: p.px(20.0), action: Action::GeminiClear, vis: None });

        let cpy_x = clx - clw - p.px(18.0);
        let n0 = kids(c);
        let sym = if d.gemini_copied { Sym::Check } else { Sym::Copy };
        let rgb = if d.gemini_copied { GREEN } else { WHITE };
        p.sym(c, sym, cpy_x, bar_y + p.px(7.0), 12.0, rgb, 0.85);
        regions.push(Region { x: cpy_x - p.px(12.0), y: bar_y - p.px(4.0), w: p.px(24.0), h: p.px(20.0), action: Action::GeminiCopy, vis: group_since(c, n0) });
    }

    let rx = x0;
    let ry = bar_y + p.px(20.0);
    let rw = w;
    let rh = (bot - ry).max(p.px(40.0));
    if let Ok(host) = p.comp.CreateContainerVisual() {
        let _ = c.Children().and_then(|k| k.InsertAtTop(&host));
        draw_gemini_response(p, &host, rx, ry, rw, rh, d.gemini_text, d.gemini_error, d.gemini_has_key, d.gemini_streaming, d.gemini_scroll);
        live.gemini_resp_host = Some(GeminiRespHost { host, x: rx, y: ry, w: rw, h: rh });
    }
}

#[allow(clippy::too_many_arguments)]
pub fn refresh_gemini_response(
    p: &mut Painter,
    live: &Live,
    text: &str,
    error: &str,
    has_key: bool,
    streaming: bool,
    scroll: usize,
) {
    let Some(h) = live.gemini_resp_host.as_ref() else { return };
    if let Ok(k) = h.host.Children() {
        let _ = k.RemoveAll();
    }
    draw_gemini_response(p, &h.host, h.x, h.y, h.w, h.h, text, error, has_key, streaming, scroll);
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
    scroll: usize,
) {
    if !error.is_empty() {
        p.text(c, &p.fit(error, 12.0, 500, w), x, y + p.px(8.0), 12.0, 500, [255, 110, 100], 0.95, Anchor::Left, w + p.px(2.0));
        return;
    }
    if text.is_empty() {
        let msg = if streaming {
            tr("Berpikir…")
        } else if !has_key {
            tr("Atur Gemini API Key di Pengaturan → Visor untuk mulai bertanya.")
        } else {
            tr("Ketik pertanyaan dan tekan Enter.")
        };
        p.text(c, &p.fit(msg, 12.0, 400, w), x + w / 2.0, y + h / 2.0 - p.px(10.0), 12.0, 400, WHITE, 0.45, Anchor::Center, w + p.px(2.0));
        return;
    }
    let font = 12.0;
    let line_h = p.px(font) * 1.38;
    let mut wrapped: Vec<String> = Vec::new();
    for para in text.lines() {
        let t = para.trim_end();
        if t.is_empty() {
            wrapped.push(String::new());
            continue;
        }
        for line in nacre_core::notch::typeset::wrap(t, w - p.px(4.0), 40, |s| p.measure(s, font, 400)) {
            wrapped.push(line);
        }
    }
    let max_lines = (h / line_h).floor().max(1.0) as usize;
    let start = if streaming {
        wrapped.len().saturating_sub(max_lines)
    } else {
        scroll.min(wrapped.len().saturating_sub(max_lines))
    };
    let mut yy = y;
    for line in wrapped.iter().skip(start).take(max_lines) {
        if !line.is_empty() {
            p.text(c, line, x, yy, font, 400, WHITE, 0.92, Anchor::Left, w + p.px(4.0));
        }
        yy += line_h;
    }
}

fn play(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>, live: &mut Live) {
    if let Some(m) = &d.media {
        let glow = accent_or(m.tint);
        p.ambient_glow(c, f.x0, 0.0, f.w, f.h, 0.0, glow, false);
    }

    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(14.0);
    let bot = f.h - p.px(NAV_H_DP + 4.0);
    let h = (bot - top).max(p.px(80.0));

    if d.lyrics_open {
        let mx = f.x0 + pad;
        let mw = p.px(248.0).min((f.w - 2.0 * pad) * 0.48);
        let div_x = mx + mw + p.px(16.0);
        let lx = div_x + p.px(18.0);
        let lw = (f.x0 + f.w - pad - lx).max(p.px(80.0));

        match &d.media {
            Some(m) => music_column(p, c, mx, top, mw, h, m, true, regions, live),
            None => {
                p.sym(c, Sym::Note, mx + mw / 2.0, top + h / 2.0 - p.px(14.0), 26.0, WHITE, 0.34);
                p.text(c, tr("Tidak diputar"), mx + mw / 2.0, top + h / 2.0 + p.px(8.0), 12.5, 500, WHITE, 0.56, Anchor::Center, mw - p.px(12.0));
                // Keep Lyrics toggle reachable even when idle.
                let qx = mx + mw / 2.0;
                let qy = top + h - p.px(14.0);
                let n0 = kids(c);
                p.sym(c, Sym::Quote, qx, qy, 14.0, WHITE, 0.72);
                regions.push(Region { x: qx - p.px(16.0), y: qy - p.px(14.0), w: p.px(32.0), h: p.px(28.0), action: Action::ToggleLyrics, vis: group_since(c, n0) });
            }
        }
        p.divider_v(c, div_x, top + p.px(6.0), h - p.px(12.0));
        lyrics_slot(p, c, lx, top, lw, h, d, live, false);
    } else {
        let mx = f.x0 + pad + p.px(4.0);
        let mw = (f.w - 2.0 * pad - p.px(8.0)).max(p.px(180.0));
        match &d.media {
            Some(m) => music_column(p, c, mx, top, mw, h, m, false, regions, live),
            None => {
                p.sym(c, Sym::Note, mx + mw / 2.0, top + h / 2.0 - p.px(14.0), 26.0, WHITE, 0.34);
                p.text(c, tr("Tidak diputar"), mx + mw / 2.0, top + h / 2.0 + p.px(8.0), 12.5, 500, WHITE, 0.56, Anchor::Center, mw - p.px(12.0));
                let qx = mx + mw / 2.0;
                let qy = top + h - p.px(14.0);
                let n0 = kids(c);
                p.sym(c, Sym::Quote, qx, qy, 14.0, WHITE, 0.42);
                regions.push(Region { x: qx - p.px(16.0), y: qy - p.px(14.0), w: p.px(32.0), h: p.px(28.0), action: Action::ToggleLyrics, vis: group_since(c, n0) });
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn lyrics_slot(p: &mut Painter, c: &ContainerVisual, x: f32, y: f32, w: f32, h: f32, d: &Data, live: &mut Live, compact_player: bool) {
    if let Ok(host) = p.comp.CreateContainerVisual() {
        let _ = c.Children().and_then(|k| k.InsertAtTop(&host));
        lyrics_3line_stack(p, &host, x, y, w, h, d.media.is_some(), &d.lyrics, live, compact_player);
        live.lyrics_host = Some(LyricsHostLive { host, x, y, w, h, compact_player });
    }
}

pub fn refresh_lyrics_in_place(p: &mut Painter, live: &mut Live, has_media: bool, ly: &LyricsView) {
    let Some(h) = live.lyrics_host.as_ref() else { return };
    let host = h.host.clone();
    let (x, y, w, h_px, compact_player) = (h.x, h.y, h.w, h.h, h.compact_player);
    if let Ok(k) = host.Children() {
        let _ = k.RemoveAll();
    }
    live.lyric_scroll = None;
    live.lyric_shift = 0.0;
    lyrics_3line_stack(p, &host, x, y, w, h_px, has_media, ly, live, compact_player);
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
    live: &mut Live,
    compact_player: bool,
) {
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

    let go = || -> windows_core::Result<(ContainerVisual, ContainerVisual)> {
        let frame = p.comp.CreateContainerVisual()?;
        frame.SetOffset(Vector3 { X: rx, Y: ry, Z: 0.0 })?;
        frame.SetSize(Vector2 { X: rw, Y: rh })?;
        frame.SetClip(&p.comp.CreateInsetClip()?)?;
        c.Children()?.InsertAtTop(&frame)?;
        let scroll = p.comp.CreateContainerVisual()?;
        scroll.SetSize(Vector2 { X: rw, Y: rh })?;
        frame.Children()?.InsertAtTop(&scroll)?;
        Ok((frame, scroll))
    };
    let Ok((frame, sc)) = go() else { return };

    let maxw = (rw - p.px(6.0)).max(p.px(40.0));
    let active_sz = if compact_player { 15.0 } else { 15.5 };
    let dim_sz = if compact_player { 13.0 } else { 13.5 };
    let active_lh = (p.px(active_sz) * 1.28).ceil();
    let dim_lh = (p.px(dim_sz) * 1.26).ceil();
    let gap = (p.px(if compact_player { 6.0 } else { 10.0 })).round();

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

        let mut yy = ((rh - total_h) / 2.0).max(p.px(2.0)).round();
        if !prev_lines.is_empty() {
            for t in &prev_lines {
                p.text(&sc, t, 0.0, yy, dim_sz, 600, WHITE, 0.34, Anchor::Left, maxw + p.px(4.0));
                yy += dim_lh;
            }
            live.lyric_shift = (prev_h + gap).round();
            yy += gap;
        } else {
            live.lyric_shift = (active_lh + gap).round();
        }

        let is_active = active.is_some();
        for t in &cur_lines {
            p.text(&sc, t, 0.0, yy, active_sz, 700, WHITE, if is_active { 1.0 } else { 0.45 }, Anchor::Left, maxw + p.px(4.0));
            yy += active_lh;
        }
        if !next_lines.is_empty() {
            yy += gap;
            for t in &next_lines {
                if yy + dim_lh * 0.5 > rh {
                    break;
                }
                p.text(&sc, t, 0.0, yy, dim_sz, 600, WHITE, 0.34, Anchor::Left, maxw + p.px(4.0));
                yy += dim_lh;
            }
        }
        live.lyric_scroll = Some(sc);
    } else {
        let mut yy = p.px(4.0);
        for line in l.lines.iter().filter(|x| !x.text.is_empty()) {
            if yy > rh - dim_lh {
                break;
            }
            for t in nacre_core::notch::typeset::wrap(&line.text, maxw, 2, |s| p.measure(s, dim_sz, 500)) {
                p.text(&sc, &t, 0.0, yy, dim_sz, 500, WHITE, 0.78, Anchor::Left, maxw + p.px(4.0));
                yy += dim_lh;
            }
            yy += gap * 0.6;
        }
    }
    if !compact_player {
        let note = match l.synced {
            false => tr("Lirik tanpa waktu"),
            true if ly.lrclib => "LRCLIB",
            true => "",
        };
        if !note.is_empty() {
            p.text(&frame, note, rw - p.px(2.0), rh - p.px(12.0), 9.0, 500, WHITE, 0.24, Anchor::Right, rw * 0.5);
        }
    }
}

fn round_button(p: &mut Painter, c: &ContainerVisual, cx: f32, cy: f32, d: f32, fill: Color) {
    p.rect(c, (cx - d / 2.0).round(), (cy - d / 2.0).round(), d, d, d / 2.0, fill);
}

fn timer(p: &mut Painter, c: &ContainerVisual, f: Frame, d: &Data, regions: &mut Vec<Region>) {
    let pad = p.px(EDGE_PAD_DP);
    let top = p.px(14.0);
    let bot = f.h - p.px(NAV_H_DP + 6.0);
    let h = (bot - top).max(p.px(80.0));
    let cw = (f.w - 2.0 * pad - p.px(24.0)) / 2.0;
    let x_left = f.x0 + pad;
    let div_x = x_left + cw + p.px(12.0);
    let x_right = div_x + p.px(12.0);
    timer_col(p, c, x_left, top, cw, h, d, regions);
    p.divider_v(c, div_x, top + p.px(6.0), h - p.px(12.0));
    calendar_col(p, c, x_right, top, cw, h, d, regions);
}

#[allow(clippy::too_many_arguments)]
fn timer_col(p: &mut Painter, c: &ContainerVisual, x: f32, y: f32, w: f32, h: f32, d: &Data, regions: &mut Vec<Region>) {
    let t = d.timer;
    let secs = t.remaining_secs(d.now_ms);
    let (rgb, status) = match t.phase {
        Phase::Running if secs <= 60 => (ORANGE, tr("Berjalan")),
        Phase::Running => (WHITE, tr("Berjalan")),
        Phase::Paused => (WHITE, tr("Dijeda")),
        Phase::Finished => (ORANGE, tr("Selesai")),
        Phase::Idle => (WHITE, tr("Siap")),
    };
    p.text(c, &format_mmss(secs), x + p.px(4.0), y + p.px(4.0), 30.0, 700, rgb, 1.0, Anchor::Left, w * 0.72);
    p.text(c, status, x + w - p.px(4.0), y + p.px(14.0), 11.5, 500, WHITE, 0.50, Anchor::Right, w * 0.34);
    let (bd, bgap) = (p.px(34.0), p.px(12.0));
    let total = 4.0 * bd + 3.0 * bgap;
    let cy = y + h - p.px(8.0) - bd / 2.0;
    let mut cx = x + (w - total) / 2.0 + bd / 2.0;
    let idle = matches!(t.phase, Phase::Idle | Phase::Finished);
    let accent = crate::prefs::accent();
    let buttons: [(Action, bool); 4] = [(Action::TimerMinus, idle), (Action::TimerToggle, true), (Action::TimerReset, true), (Action::TimerPlus, idle)];
    for (a, enabled) in buttons {
        let primary = a == Action::TimerToggle;
        let fill = if primary { Color { A: 255, R: accent[0], G: accent[1], B: accent[2] } } else { white(if enabled { 24 } else { 11 }) };
        let n0 = kids(c);
        round_button(p, c, cx, cy, bd, fill);
        let op = if enabled { 1.0 } else { 0.32 };
        match a {
            Action::TimerToggle => p.sym(c, if t.phase == Phase::Running { Sym::Pause } else { Sym::Play }, cx, cy, 15.0, WHITE, 1.0),
            Action::TimerReset => p.sym(c, Sym::Restart, cx, cy, 14.5, WHITE, 0.9),
            Action::TimerMinus | Action::TimerPlus => {
                let th = p.px(12.0) * 1.3;
                let s = if a == Action::TimerMinus { "\u{2212}5" } else { "+5" };
                p.text(c, s, cx, cy - th / 2.0, 12.0, 600, WHITE, op, Anchor::Center, bd);
            }
            _ => {}
        }
        if enabled {
            regions.push(Region { x: cx - bd / 2.0, y: cy - bd / 2.0, w: bd, h: bd, action: a, vis: group_since(c, n0) });
        }
        cx += bd + bgap;
    }
}

#[allow(clippy::too_many_arguments)]
fn calendar_col(p: &mut Painter, c: &ContainerVisual, x: f32, y: f32, w: f32, h: f32, d: &Data, regions: &mut Vec<Region>) {
    use nacre_core::island::calendar::{add_days, month_short, weekday, weekday_short};
    let en = crate::prefs::get().lang == nacre_core::i18n::Lang::En;
    let cal = &d.cal;
    let accent = crate::prefs::accent();
    let (sy, sm, sd) = cal.sel;
    p.text(c, month_short(sm, en), x + p.px(4.0), y + p.px(6.0), 17.0, 700, WHITE, 1.0, Anchor::Left, p.px(56.0));
    p.text(c, &sy.to_string(), x + p.px(4.0), y + p.px(29.0), 11.0, 500, WHITE, 0.45, Anchor::Left, p.px(56.0));
    let (left, right) = (x + p.px(54.0), x + w);
    let cell = (right - left) / 5.0;
    let (wy, ny, dia) = (y + p.px(6.0), y + p.px(28.0), p.px(26.0));
    for i in -2i32..=2 {
        let (yy, mm, dd) = add_days(sy, sm, sd, i);
        let cx = left + cell * (i + 2) as f32 + cell / 2.0;
        let selected = i == 0;
        let today = (yy, mm, dd) == cal.today;
        let wd = weekday_short(weekday(yy, mm, dd), en);
        let n0 = kids(c);
        p.text(c, wd, cx, wy, 9.0, 600, if selected { accent } else { WHITE }, if selected { 1.0 } else { 0.4 }, Anchor::Center, cell);
        if selected {
            p.rect(c, (cx - dia / 2.0).round(), ny.round(), dia, dia, dia / 2.0, Color { A: 255, R: accent[0], G: accent[1], B: accent[2] });
        }
        let th = p.px(13.5) * 1.3;
        let (rgb, op) = if selected { (WHITE, 1.0) } else if today { (accent, 1.0) } else { (WHITE, 0.8) };
        p.text(c, &format!("{dd:02}"), cx, ny + dia / 2.0 - th / 2.0, 13.5, if selected || today { 700 } else { 500 }, rgb, op, Anchor::Center, cell);
        if !selected {
            regions.push(Region { x: cx - cell / 2.0, y: wy, w: cell, h: ny + dia - wy, action: Action::CalDay(i), vis: group_since(c, n0) });
        }
    }
    let is_today = cal.sel == cal.today;
    let line = match &cal.events {
        Events::Loading => "\u{2026}".to_string(),
        Events::Unavailable => String::new(),
        Events::List([]) => tr(if is_today { "Tidak ada acara hari ini" } else { "Tidak ada acara" }).to_string(),
        Events::List(list) => {
            let e = &list[0];
            let when = match e.minute {
                Some(m) if crate::prefs::get().clock_24h => format!("{:02}.{:02}  ", m / 60, m % 60),
                Some(m) => format!("{}.{:02} {}  ", (m / 60 + 11) % 12 + 1, m % 60, if m < 720 { "AM" } else { "PM" }),
                None => String::new(),
            };
            let more = if list.len() > 1 { format!("  +{}", list.len() - 1) } else { String::new() };
            format!("{when}{}{more}", e.title)
        }
    };
    if !line.is_empty() {
        let ly = y + h - p.px(22.0);
        p.sym(c, Sym::Calendar, x + p.px(10.0), ly + p.px(7.5), 11.5, WHITE, 0.5);
        let maxw = w - p.px(26.0);
        let shown = p.fit(&line, 11.5, 500, maxw);
        p.text(c, &shown, x + p.px(22.0), ly, 11.5, 500, WHITE, 0.72, Anchor::Left, maxw + p.px(4.0));
    }
}
