mod dragout;
mod draw;
mod drop;
mod icons_worker;
mod shell;
mod text_input;
mod views;

use crate::dock::menu::{Item as MItem, Menu, MENU_CLOSE, MENU_PICK};
use crate::strip::Strip;
use draw::Painter;
use icons_worker::{IconKind, IconReq, IconWorker};
use nacre_core::island::calendar;
use nacre_core::island::timer::{Phase, Timer};
use nacre_core::notch::glance;
use nacre_core::notch::spring::{Spring, SpringParams};
use nacre_core::notch::state::{self, motion, Event, Live, Look, Mode, OpenOn, Size, Tab, View};
use nacre_core::visibility::policy::{should_hide, Inputs, Mode as Vis};
use nacre_platform::loopback::{self, Loopback};
use nacre_platform::media::{self, ArtUpdate, Command, Media};
use nacre_platform::wins::{overlap, tracker};
use nacre_platform::{log, privacy, sys};
use nacre_ui::gfx::Gfx;
use nacre_ui::glass::Tier;
use nacre_ui::host::{Host, Surface};
use shell::Shell;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::time::Instant;
use text_input::{FocusController, InputOutcome, TextInput};
use views::{Action, Data, Frame, LyrState, LyricsView, MediaView, Region};
use windows::core::w;
use windows::UI::Composition::{AnimationDelayBehavior, CompositionDrawingSurface, CompositionObject, Compositor, Visual};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{CombineRgn, CreateRectRgn, DeleteObject, SetWindowRgn, RGN_OR};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_LBUTTON};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_core::{Interface, HSTRING};
use windows_numerics::{Vector2, Vector3};

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_MOUSEHWHEEL: u32 = 0x020E;
const WM_CAPTURECHANGED: u32 = 0x0215;
const WM_DISPLAYCHANGE: u32 = 0x007E;
const WM_DPICHANGED: u32 = 0x02E0;
const WM_POWERBROADCAST: u32 = 0x0218;
const WM_SETTINGCHANGE: u32 = 0x001A;
const STRIP_ENTER: u32 = WM_APP + 14;
const STRIP_LEAVE: u32 = WM_APP + 15;
const WM_TOPMOST: u32 = WM_APP + 72;
const WM_OPEN: u32 = WM_APP + 73;
const WM_MEDIA: u32 = WM_APP + 10;
const WM_BANDS: u32 = WM_APP + 11;
const WM_LYRICS: u32 = WM_APP + 18;
const WM_DND: u32 = WM_APP + 19;
const WM_AFTERDROP: u32 = WM_APP + 33;
const WM_DOWNLOAD: u32 = WM_APP + 34;
const WM_AGENDA: u32 = WM_APP + 23;
const WM_SEND: u32 = WM_APP + 24;
const WM_PRIVACY: u32 = WM_APP + 25;
const WM_RECONF: u32 = WM_APP + 26;
const WM_ICON: u32 = WM_APP + 27;
const WM_START_DRAGOUT: u32 = WM_APP + 28;
const WM_NOTIF: u32 = WM_APP + 29;
const WM_GTASKS: u32 = WM_APP + 30;
const WM_GEMINI: u32 = WM_APP + 31;
const WM_GTASKS_ACTION: u32 = WM_APP + 32;
const WM_PRAYER_TEST: u32 = WM_APP + 35;

const T_DWELL: usize = 1;
const T_LEAVE: usize = 2;
const T_GHOST: usize = 3;
const T_FRAME: usize = 4;
const T_TICK: usize = 5;
const T_CHARGE: usize = 6;
const T_WINCHG: usize = 7;
const T_RECONF: usize = 8;
const T_EDGE: usize = 9;
const T_GRACE: usize = 10;
const T_START: usize = 11;
const T_GLANCE: usize = 13;
const T_MINUTE: usize = 14;
const T_PROGRESS: usize = 15;
const T_LYRIC: usize = 16;
const T_DLDONE: usize = 18;
const T_SEND: usize = 19;
const T_SENDDONE: usize = 20;
const T_TESTSEND: usize = 21;
const T_NOTIF_TOAST: usize = 22;
const T_CARET: usize = 23;
const T_COPIED: usize = 24;
const T_NAV_COLLAPSE: usize = 25;
const T_SCROLL_IDLE: usize = 26;
const T_PRAYER_ALERT: usize = 27;
const NAV_COLLAPSE_MS: u32 = 180;
const NOTIF_TOAST_MS: u32 = 4500;
const PRAYER_ALERT_MS: u32 = 8000;
const SEND_IDLE: std::time::Duration = std::time::Duration::from_secs(15 * 60);

#[derive(Clone, Copy, PartialEq, Eq)]
enum ActiveInput {
    Todo,
    Gemini,
}

struct SendSess {
    server: nacre_platform::sendsrv::Server,
    qr: Option<CompositionDrawingSurface>,
    qr_px: f32,
    names: Vec<String>,
    prog: nacre_platform::sendsrv::Progress,
    last: Instant,
    done_at: Option<Instant>,
}

const WIN_W: f32 = 720.0;
const WIN_H: f32 = 250.0;
const SHADOW_PAD_X: f32 = 36.0;
const SHADOW_PAD_B: f32 = 44.0;
const SHADOW_BLEED_DP: f32 = 28.0;
const EAR_DP: f32 = 21.0;
const DETACHED_TIMER_W_DP: f32 = 86.0;
const DETACHED_TIMER_GAP_DP: f32 = 9.0;

static NOTCH_HWND: AtomicIsize = AtomicIsize::new(0);
static BUSY: AtomicBool = AtomicBool::new(false);
static STAGE_Y_BITS: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Deferred {
    MouseLeave,
    CaptureChanged,
    MenuClose,
    Reconfigure,
    Prefs,
    GfxReset,
    Privacy,
    Icon,
    Notif,
    GTasks,
    Gemini,
}

thread_local! {
    static DEFERRED: RefCell<Vec<Deferred>> = const { RefCell::new(Vec::new()) };
}

fn defer(ev: Deferred) {
    DEFERRED.with(|q| {
        let mut q = q.borrow_mut();
        if !q.contains(&ev) {
            q.push(ev);
        }
    });
}

struct BusyGuard;
impl Drop for BusyGuard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}

pub struct Options {
    pub glass: Option<Tier>,
    pub mode: Vis,
    pub reserve: bool,
}

pub fn request_reconfigure() {
    let h = NOTCH_HWND.load(Ordering::Relaxed);
    if h != 0 {
        post(h, WM_RECONF);
    }
}

pub fn trigger_gtasks_connect() {
    let h = NOTCH_HWND.load(Ordering::Relaxed);
    if h != 0 {
        post_w(h, WM_GTASKS_ACTION, 1);
    }
}

pub fn trigger_gtasks_disconnect() {
    let h = NOTCH_HWND.load(Ordering::Relaxed);
    if h != 0 {
        post_w(h, WM_GTASKS_ACTION, 2);
    }
}

pub fn trigger_prayer_test_alert() {
    let h = NOTCH_HWND.load(Ordering::Relaxed);
    if h != 0 {
        post(h, WM_PRAYER_TEST);
    }
}

fn ts(ms: u32) -> windows::Foundation::TimeSpan {
    windows::Foundation::TimeSpan { Duration: ms as i64 * 10_000 }
}

fn keyframes(comp: &Compositor, obj: &CompositionObject, prop: &str, from: f32, to: f32, ms: u32, delay: u32) -> windows_core::Result<()> {
    let e = crate::motion::ease(comp)?;
    let a = comp.CreateScalarKeyFrameAnimation()?;
    a.InsertKeyFrame(0.0, from)?;
    a.InsertKeyFrameWithEasingFunction(1.0, to, &e)?;
    a.SetDuration(ts(ms))?;
    if delay > 0 {
        a.SetDelayTime(ts(delay))?;
        a.SetDelayBehavior(AnimationDelayBehavior::SetInitialValueBeforeDelay)?;
    }
    obj.StartAnimation(&HSTRING::from(prop), &a)
}

/// Velvety slow-mo glide easing (`cubic-bezier(0.22, 1.0, 0.36, 1.0)`) for satisfying tab transitions.
fn keyframes_glide(comp: &Compositor, obj: &CompositionObject, prop: &str, from: f32, to: f32, ms: u32, delay: u32) -> windows_core::Result<()> {
    let e: windows::UI::Composition::CompositionEasingFunction = comp
        .CreateCubicBezierEasingFunction(Vector2 { X: 0.22, Y: 1.0 }, Vector2 { X: 0.36, Y: 1.0 })?
        .cast()?;
    let a = comp.CreateScalarKeyFrameAnimation()?;
    a.InsertKeyFrame(0.0, from)?;
    a.InsertKeyFrameWithEasingFunction(1.0, to, &e)?;
    a.SetDuration(ts(ms))?;
    if delay > 0 {
        a.SetDelayTime(ts(delay))?;
        a.SetDelayBehavior(AnimationDelayBehavior::SetInitialValueBeforeDelay)?;
    }
    obj.StartAnimation(&HSTRING::from(prop), &a)
}

fn with<R>(hwnd: HWND, f: impl FnOnce(&mut Notch) -> R) -> Option<R> {
    let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Notch;
    if p.is_null() || BUSY.swap(true, Ordering::Acquire) {
        return None;
    }
    let out = {
        let _g = BusyGuard;
        f(unsafe { &mut *p })
    };
    drain_deferred(hwnd);
    Some(out)
}

fn drain_deferred(hwnd: HWND) {
    loop {
        let next = DEFERRED.with(|q| {
            let mut q = q.borrow_mut();
            if q.is_empty() { None } else { Some(q.remove(0)) }
        });
        let Some(ev) = next else { break };
        let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut Notch;
        if p.is_null() || BUSY.swap(true, Ordering::Acquire) {
            defer(ev);
            break;
        }
        let _g = BusyGuard;
        let d = unsafe { &mut *p };
        match ev {
            Deferred::MouseLeave => d.on_leave(),
            Deferred::CaptureChanged => d.on_capture_changed(),
            Deferred::MenuClose => d.on_menu_close(),
            Deferred::Reconfigure => d.on_reconfigure(),
            Deferred::Prefs => {
                d.apply_prefs();
                d.on_reconfigure();
            }
            Deferred::GfxReset => d.on_gfx_reset(),
            Deferred::Privacy => d.on_privacy(),
            Deferred::Icon => d.on_icon(),
            Deferred::Notif => d.on_notif(),
            Deferred::GTasks => d.on_gtasks(),
            Deferred::Gemini => d.on_gemini(),
        }
    }
}

fn timer(hwnd: HWND, id: usize, ms: u32) {
    unsafe {
        SetTimer(Some(hwnd), id, ms, None);
    }
}

fn kill(hwnd: HWND, id: usize) {
    unsafe {
        let _ = KillTimer(Some(hwnd), id);
    }
}

fn post(hwnd: isize, msg: u32) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), msg, WPARAM(0), LPARAM(0));
    }
}

fn post_w(hwnd: isize, msg: u32, w: usize) {
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), msg, WPARAM(w), LPARAM(0));
    }
}

fn on_windows_changed() {
    let h = NOTCH_HWND.load(Ordering::Relaxed);
    if h != 0 {
        timer(HWND(h as *mut _), T_WINCHG, 120);
    }
}

fn on_win_fx(ev: u32, _hwnd: isize) {
    if matches!(
        ev,
        tracker::EV_LOCATION
            | tracker::EV_UNCLOAKED
            | tracker::EV_MOVESIZE_END
            | tracker::EV_MINIMIZE_START
            | tracker::EV_MINIMIZE_END
            | tracker::EV_FOREGROUND
    ) {
        let h = NOTCH_HWND.load(Ordering::Relaxed);
        if h != 0 {
            timer(HWND(h as *mut _), T_WINCHG, 100);
        }
    }
}

fn auto_tier(over: Option<Tier>) -> Tier {
    over.unwrap_or_else(|| {
        let c = sys::conditions();
        if !c.transparency || c.battery_saver || c.remote_session {
            Tier::Solid
        } else {
            Tier::Full
        }
    })
}

const ISLAND_GAP_DP: f32 = 7.0;

fn island_gap(dpi: f32) -> f32 {
    if crate::prefs::get().notch_island {
        (ISLAND_GAP_DP * dpi).round()
    } else {
        0.0
    }
}

fn ear_px(dpi: f32) -> f32 {
    if crate::prefs::get().notch_island {
        0.0
    } else {
        (EAR_DP * dpi).ceil()
    }
}

fn win_size(dpi: f32, screen_w: i32) -> (i32, i32) {
    let pad_x = (SHADOW_PAD_X * dpi).round() as i32;
    let pad_b = (SHADOW_PAD_B * dpi).round() as i32;
    let w = ((WIN_W * dpi).round() as i32 + 2 * pad_x).min(screen_w);
    // Always reserve floating-pill headroom so Notch <-> Capsule morphs inside the same GPU canvas with zero HWND resize blink.
    let h = (WIN_H * dpi + (ISLAND_GAP_DP * dpi).round()).ceil() as i32 + pad_b;
    (w, h)
}

fn frame_interval_ms() -> u32 {
    let hz = sys::refresh_hz().max(30);
    (1000.0 / hz as f32).round().clamp(4.0, 16.0) as u32
}

pub struct Notch {
    hwnd: HWND,
    paint: Painter,
    _surface: Surface,
    shell: Shell,
    dpi: f32,
    win: (i32, i32),
    screen_w: i32,

    mode: Mode,
    look: Look,
    size: Size,
    first: bool,
    tracking: bool,
    hover_act: Option<Action>,
    regions: Vec<Region>,

    sp_w: Spring,
    sp_h: Spring,
    sp_r: Spring,
    sp_ear: Spring,
    sp_island: Spring,
    sp_y: Spring,
    sp_vis: Spring,
    sp_sh_blur: Spring,
    sp_sh_dy: Spring,
    sp_sh_op: Spring,
    sp_csh_blur: Spring,
    sp_csh_dy: Spring,
    sp_csh_op: Spring,
    sp_nav: Spring,
    sp_lyrics: Spring,
    sp_scrub_hover: Spring,
    sp_art_scale: Spring,
    sp_timer_pill: Spring,
    sp_scroll: Spring,
    sp_sb_hover: Spring,
    sp_sb_active: Spring,
    nav_expanded: bool,
    nav_collapse_armed: bool,
    scrub_hovered: bool,
    sb_hovered: bool,
    scroll_drag: Option<f32>,
    pressed_act: Option<Action>,
    prev_tab: Option<Tab>,
    art_pop_pending: bool,
    todo_input_fade: bool,
    muted: bool,
    frame_active: bool,
    frame_last: Instant,
    frame_ms: u32,
    rgn_rect: (i32, i32, i32, i32),
    rgn_timer: (i32, i32, i32, i32),

    timer: Timer,
    t0: Instant,
    last_sec: i64,
    battery: Option<(u8, bool)>,
    charging_fresh: bool,
    glance_idx: usize,

    media: Media,
    has_media: bool,
    title: String,
    artist: String,
    playing: bool,
    pos_ms: i64,
    dur_ms: i64,
    pos_at: Instant,
    can_seek: bool,
    art: Option<CompositionDrawingSurface>,
    tint: Option<[u8; 3]>,
    loopback: Option<Loopback>,
    bar_cur: [f32; 5],
    bar_tgt: [f32; 5],
    marquee_t: f32,
    live_vis: views::Live,
    seek_drag: Option<(f32, f32, f32)>,
    shelf_press: Option<(usize, f32, f32)>,
    dragging_out: bool,
    pinned: bool,
    lyrics_open: bool,
    play_glide_active: bool,
    content_hidden_for_retract: bool,
    swipe: state::SwipeLatch,
    last_pub_half: i32,
    todo_adding: bool,
    todo_editing: bool,
    todo_cal_mode: bool,
    todo_cal_adding: bool,
    todo_cal_scroll: f32,
    todo_cal_err: String,
    focus: FocusController,

    album: String,
    lyr_worker: nacre_platform::lyrics::Lyrics,
    lyr: Option<nacre_core::notch::lyrics::Lyrics>,
    lyr_state: LyrState,
    lyr_key: String,
    lyr_track: String,
    lyr_idx: Option<usize>,

    shelf: nacre_core::notch::shelf::Shelf,
    shelf_icons: std::collections::HashMap<String, CompositionDrawingSurface>,
    icons: IconWorker,
    icons_pending: HashSet<(IconKind, String)>,
    dnd: drop::Shared,
    _drop_target: Option<windows::Win32::System::Ole::IDropTarget>,
    _strip_drop: Option<windows::Win32::System::Ole::IDropTarget>,
    drop_zone: Option<usize>,
    pending: Vec<String>,
    menu: Option<Menu>,
    menu_item: Option<usize>,
    privacy: (bool, bool),
    privacy_mon: Option<privacy::Monitor>,
    notif_mon: Option<nacre_platform::notifications::Notifications>,
    notifs: nacre_core::notch::notif::NotifStore,
    notif_status: nacre_platform::notifications::Status,
    toast_id: Option<u32>,
    notif_peeking: bool,
    prayer_alert: Option<views::PrayerAlertView>,
    prayer_peeking: bool,
    prayer_last_fired: Option<(i32, u32, u32, nacre_core::notch::prayer::Prayer, nacre_core::notch::prayer::AlertKind)>,
    alerts_scroll: f32,
    internal_notif_id: u32,
    todos: nacre_core::notch::todo::TodoStore,
    todo_scroll: f32,
    todo_status: nacre_platform::gtasks::SyncStatus,
    gtasks: nacre_platform::gtasks::GTasks,
    gemini: nacre_platform::gemini::Gemini,
    gemini_req_id: u64,
    gemini_resp: String,
    gemini_err: String,
    gemini_streaming: bool,
    gemini_scroll: f32,
    gemini_auto_follow: bool,
    gemini_copied: bool,
    active_input: Option<ActiveInput>,
    todo_input: Option<TextInput>,
    todo_draft: String,
    gemini_input: Option<TextInput>,
    gemini_draft: String,
    dl_watch: Option<nacre_platform::dlwatch::Watch>,
    dl: Option<nacre_core::notch::download::Download>,
    dl_done: Option<nacre_core::notch::download::Download>,
    dl_icons: std::collections::HashMap<String, Option<CompositionDrawingSurface>>,
    cal_sel: i32,
    agenda: Option<nacre_platform::agenda::Agenda>,
    cal_events: Option<(views::Date, Option<Vec<nacre_platform::agenda::Event>>)>,
    cal_fetched_at: Option<Instant>,
    cal_in_flight: bool,
    send: Option<SendSess>,
    send_err: bool,
    media_app: String,
    app_icons: std::collections::HashMap<String, Option<CompositionDrawingSurface>>,
    vis: Vis,
    reserved: bool,
    off: bool,
    shown: bool,
    starting: bool,
    overlap: overlap::Overlap,
    grace: bool,
    edge_armed: bool,
    strip: Option<Strip>,
    tier: Tier,
    glass_override: Option<Tier>,
}

impl Notch {
    pub fn create(host: &Host, opts: Options) -> windows_core::Result<HWND> {
        unsafe {
            let hi: HINSTANCE = GetModuleHandleW(None)?.into();
            let class = w!("AbraNotch");
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: hi,
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW)?,
                ..Default::default()
            });
            let hwnd = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class,
                w!("abra-notch"),
                WS_POPUP,
                0, 0, 400, 200,
                None, None, Some(hi), None,
            )?;
            let dpi = GetDpiForWindow(hwnd).max(96) as f32 / 96.0 * crate::prefs::ui_scale();
            let screen_w = sys::screen_size().0;
            let win = win_size(dpi, screen_w);
            let wx = (screen_w - win.0) / 2;
            let _ = SetWindowPos(hwnd, None, wx, 0, win.0, win.1, SWP_NOACTIVATE | SWP_NOZORDER);
            let comp = host.compositor.clone();
            let surface = host.attach(hwnd, (win.0 as f32, win.1 as f32))?;
            let tier = auto_tier(opts.glass);
            let gfx = Gfx::shared(&comp)?;
            let shell = Shell::new(host, hwnd, &surface.root, win, wx, tier)?;
            let rest = state::look(Mode::Rest, None);
            let init_size = state::size(rest, dpi);
            let init_ear = ear_px(dpi);
            let init_gap = island_gap(dpi);
            let off = !crate::prefs::get().notch_enabled;
            let start_hidden = off || opts.mode == Vis::OnDemand;
            let init_isl = crate::prefs::get().notch_island;
            let (init_w, init_h, init_r, init_y) = if start_hidden {
                if init_isl {
                    let hw = (init_size.w * 0.84).max(120.0 * dpi);
                    let hh = (init_size.h * 0.74).max(18.0 * dpi);
                    let hr = (init_size.radius * 0.74).min(hh * 0.5);
                    (hw, hh, hr, -(hh + 10.0 * dpi))
                } else {
                    ((init_size.w * 0.74).max(110.0 * dpi), 2.0 * dpi, 2.0 * dpi, -4.0 * dpi)
                }
            } else {
                (init_size.w, init_size.h, init_size.radius, init_gap)
            };
            let init_vis = if start_hidden { 0.0 } else { 1.0 };
            let (init_sh_blur, init_sh_dy, rest_sh_op) = state::shadow(rest);
            let init_sh_op = if start_hidden { 0.0 } else { rest_sh_op };
            let (init_csh_blur, init_csh_dy, rest_csh_op) = state::contact_shadow(rest);
            let init_csh_op = if start_hidden { 0.0 } else { rest_csh_op };
            let frame_ms = frame_interval_ms();
            let hw = hwnd.0 as isize;
            let privacy_mon = Some(privacy::spawn(move || post(hw, WM_PRIVACY)));
            let icons = icons_worker::spawn(move || post(hw, WM_ICON));
            let gtasks = nacre_platform::gtasks::spawn(move || post(hw, WM_GTASKS));
            let gemini = nacre_platform::gemini::spawn(move || post(hw, WM_GEMINI));
            let shelf = load_persisted_shelf();
            let todos = load_persisted_todos();

            let mut n = Box::new(Notch {
                hwnd,
                paint: Painter::new(comp, gfx, dpi),
                _surface: surface,
                shell,
                dpi,
                win,
                screen_w,
                mode: Mode::Rest,
                look: rest,
                size: init_size,
                first: true,
                tracking: false,
                hover_act: None,
                regions: Vec::new(),
                sp_w: Spring::new(init_w, SpringParams::swap()),
                sp_h: Spring::new(init_h, SpringParams::swap()),
                sp_r: Spring::new(init_r, SpringParams::swap()),
                sp_ear: Spring::new(init_ear, SpringParams::swap()),
                sp_island: Spring::new_unit(
                    if init_isl { 1.0 } else { 0.0 },
                    SpringParams::shape_morph(),
                ),
                sp_y: Spring::new(init_y, SpringParams::stage_show()),
                sp_vis: Spring::new_unit(init_vis, SpringParams::stage_show()),
                sp_sh_blur: Spring::new(init_sh_blur, SpringParams::swap()),
                sp_sh_dy: Spring::new(init_sh_dy, SpringParams::swap()),
                sp_sh_op: Spring::new_unit(init_sh_op, SpringParams::swap()),
                sp_csh_blur: Spring::new(init_csh_blur, SpringParams::swap()),
                sp_csh_dy: Spring::new(init_csh_dy, SpringParams::swap()),
                sp_csh_op: Spring::new_unit(init_csh_op, SpringParams::swap()),
                sp_nav: Spring::new_unit(0.0, SpringParams::swap()),
                sp_lyrics: Spring::new_unit(
                    if crate::prefs::get().lyrics_panel_open { 1.0 } else { 0.0 },
                    SpringParams::lyrics_glide(),
                ),
                sp_scrub_hover: Spring::new_unit(0.0, SpringParams::tactile()),
                sp_art_scale: Spring::new_unit(1.0, SpringParams::open()),
                sp_timer_pill: Spring::new_unit(0.0, SpringParams::swap()),
                sp_scroll: Spring::new(0.0, SpringParams::smooth_scroll()),
                sp_sb_hover: Spring::new_unit(0.0, SpringParams::tactile()),
                sp_sb_active: Spring::new_unit(0.0, SpringParams::tactile()),
                nav_expanded: false,
                nav_collapse_armed: false,
                scrub_hovered: false,
                sb_hovered: false,
                scroll_drag: None,
                pressed_act: None,
                prev_tab: None,
                art_pop_pending: false,
                todo_input_fade: false,
                muted: nacre_platform::audio::latest().muted,
                frame_active: false,
                frame_last: Instant::now(),
                frame_ms,
                rgn_rect: (-1, -1, -1, -1),
                rgn_timer: (0, 0, 0, 0),
                timer: Timer::new(std::env::var("NACRE_TIMER_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(25 * 60)),
                t0: Instant::now(),
                last_sec: -1,
                battery: sys::battery(),
                charging_fresh: false,
                glance_idx: 0,
                media: {
                    if std::env::var_os("NACRE_NO_MEDIA").is_some() {
                        let (commands, _) = std::sync::mpsc::channel();
                        let (_, snapshots) = std::sync::mpsc::channel();
                        Media { commands, snapshots }
                    } else if std::env::var_os("NACRE_FAKE_MEDIA").is_some() {
                        nacre_platform::media_fake::spawn(move || post(hw, WM_MEDIA))
                    } else {
                        media::spawn(move || post(hw, WM_MEDIA))
                    }
                },
                has_media: false,
                title: String::new(),
                artist: String::new(),
                playing: false,
                pos_ms: 0,
                dur_ms: 0,
                pos_at: Instant::now(),
                can_seek: false,
                art: None,
                tint: None,
                loopback: None,
                bar_cur: [0.15; 5],
                bar_tgt: [0.15; 5],
                marquee_t: 0.0,
                live_vis: views::Live::default(),
                seek_drag: None,
                shelf_press: None,
                dragging_out: false,
                pinned: false,
                lyrics_open: crate::prefs::get().lyrics_panel_open,
                play_glide_active: false,
                content_hidden_for_retract: false,
                swipe: state::SwipeLatch::default(),
                last_pub_half: -1,
                todo_adding: false,
                todo_editing: false,
                todo_cal_mode: false,
                todo_cal_adding: false,
                todo_cal_scroll: 0.0,
                todo_cal_err: String::new(),
                focus: FocusController::new(),
                album: String::new(),
                lyr_worker: nacre_platform::lyrics::spawn(crate::paths::config_dir().join("lyrics"), move || post(hw, WM_LYRICS)),
                lyr: None,
                lyr_state: LyrState::Idle,
                lyr_key: String::new(),
                lyr_track: String::new(),
                lyr_idx: None,
                shelf,
                shelf_icons: std::collections::HashMap::new(),
                icons,
                icons_pending: HashSet::new(),
                dnd: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
                _drop_target: None,
                _strip_drop: None,
                drop_zone: None,
                pending: Vec::new(),
                menu: None,
                menu_item: None,
                privacy: (false, false),
                privacy_mon,
                notif_mon: None,
                notifs: nacre_core::notch::notif::NotifStore::new(24),
                notif_status: nacre_platform::notifications::status(),
                toast_id: None,
                notif_peeking: false,
                prayer_alert: None,
                prayer_peeking: false,
                prayer_last_fired: None,
                alerts_scroll: 0.0,
                internal_notif_id: 800_000,
                todos,
                todo_scroll: 0.0,
                todo_status: nacre_platform::gtasks::status(),
                gtasks,
                gemini,
                gemini_req_id: 0,
                gemini_resp: String::new(),
                gemini_err: String::new(),
                gemini_streaming: false,
                gemini_scroll: 0.0,
                gemini_auto_follow: true,
                gemini_copied: false,
                active_input: None,
                todo_input: None,
                todo_draft: String::new(),
                gemini_input: None,
                gemini_draft: String::new(),
                dl_watch: None,
                dl: None,
                dl_done: None,
                dl_icons: std::collections::HashMap::new(),
                cal_sel: 0,
                agenda: None,
                send: None,
                send_err: false,
                cal_events: None,
                cal_fetched_at: None,
                cal_in_flight: false,
                media_app: String::new(),
                app_icons: std::collections::HashMap::new(),
                vis: opts.mode,
                reserved: false,
                off,
                shown: !start_hidden,
                starting: !start_hidden,
                overlap: overlap::Overlap::default(),
                grace: false,
                edge_armed: false,
                strip: None,
                tier,
                glass_override: opts.glass,
            });
            n.refresh();
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, Box::into_raw(n) as isize);
            NOTCH_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
            crate::services::register_island(hwnd);
            crate::services::listen_gfx(hwnd);
            tracker::install(on_windows_changed);
            tracker::install_fx(on_win_fx);
            if !start_hidden {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                with(hwnd, |d| d.shell.set_visible(true, hwnd));
                timer(hwnd, T_START, 1600);
            }
            timer(hwnd, T_MINUTE, nacre_platform::time::ms_to_next_minute());
            with(hwnd, |d| {
                d._drop_target = drop::register(hwnd, WM_DND, d.dnd.clone());
                if d._drop_target.is_none() {
                    log::line("notch", "seret-taruh tidak tersedia");
                }
                d.ensure_strip();
                if opts.reserve && !d.off && !crate::prefs::get().menubar {
                    d.set_reserved(true);
                }
                d.publish_notch();
                d.refresh_overlap();
                d.update_visibility();
                d.sync_download_watch();
                d.sync_notif_watch();
            });
            crate::prefs::listen(hwnd);
            if std::env::var_os("NACRE_TEST_SEND").is_some() {
                timer(hwnd, T_TESTSEND, 2500);
            }
            log::line("notch", &format!("dibuat tier={tier:?} dpi={dpi} jendela={win:?} layar_w={screen_w} frame_ms={frame_ms}"));
            Ok(hwnd)
        }
    }

    fn now_ms(&self) -> i64 {
        self.t0.elapsed().as_millis() as i64
    }

    fn x0_for(&self, w: f32) -> f32 {
        ((self.win.0 as f32 - w) / 2.0).floor()
    }

    fn x0(&self) -> f32 {
        self.x0_for(self.size.w)
    }

    fn start_frame(&mut self) {
        if !self.frame_active {
            self.frame_active = true;
            self.frame_last = Instant::now();
            timer(self.hwnd, T_FRAME, self.frame_ms);
        }
    }

    fn stop_frame(&mut self) {
        if self.frame_active {
            self.frame_active = false;
            kill(self.hwnd, T_FRAME);
        }
    }

    fn want_detached_timer(&self) -> bool {
        matches!(self.look.view, View::Rest | View::Compact(_))
            && self.timer.is_active()
            && !matches!(self.look.view, View::Compact(state::Activity::Timer | state::Activity::TimerEnding))
    }

    /// Computes `(tx, tw, th, tr, t_ear, opacity)` for the detached right-side Timer pill
    /// while guaranteed disjoint from the main Visor capsule on every spring frame.
    fn detached_timer_geom(&self, x0: f32, w: f32, h: f32, r: f32, ear: f32) -> Option<(f32, f32, f32, f32, f32, f32)> {
        let p = self.sp_timer_pill.pos.clamp(0.0, 1.0);
        if p <= 0.02 || !matches!(self.look.view, View::Rest | View::Compact(_)) {
            return None;
        }
        let th = h.min(36.0 * self.dpi).max(2.0);
        let t_ear = if ear > 0.5 { (ear * 0.65).min(th * 0.42) } else { 0.0 };
        let ew_main = if ear > 0.5 { ear.min(h * 0.52) } else { 0.0 };
        let gap_walls = if ear > 0.5 {
            ew_main + t_ear + 3.0 * self.dpi
        } else {
            DETACHED_TIMER_GAP_DP * self.dpi
        };
        let tx = (x0 + w + gap_walls).round();
        let max_avail = (self.win.0 as f32 - t_ear - SHADOW_BLEED_DP * self.dpi - tx).max(0.0);
        let target_w = (DETACHED_TIMER_W_DP * self.dpi).min(max_avail);
        if target_w < 36.0 * self.dpi {
            return None;
        }
        let tw = (target_w * p).max(16.0 * self.dpi);
        let tr = r.min((th - t_ear).max(4.0)).min(tw * 0.5);
        Some((tx, tw, th, tr, t_ear, p))
    }

    /// Computes the tucked-away `(w, h, r, y)` pose when hidden:
    /// - Bezel-attached Notch: collapses liquidly straight into the top bezel (`h -> 2dp, y -> -4dp`)
    ///   so concave bezel ears stay flush & curved all the way until fully absorbed.
    /// - Floating Pill/Capsule: gently gathers (`0.84x` w, `0.74x` h) while lifting above the top edge
    ///   as stage opacity dissolves weightlessly.
    fn hidden_pose(&self, z: Size) -> (f32, f32, f32, f32) {
        if crate::prefs::get().notch_island {
            let w = (z.w * 0.84).max(120.0 * self.dpi);
            let h = (z.h * 0.74).max(18.0 * self.dpi);
            let r = (z.radius * 0.74).min(h * 0.5);
            let y = -(h + 10.0 * self.dpi);
            (w, h, r, y)
        } else {
            let w = (z.w * 0.74).max(110.0 * self.dpi);
            let h = 2.0 * self.dpi;
            let r = 2.0 * self.dpi;
            let y = -4.0 * self.dpi;
            (w, h, r, y)
        }
    }

    fn apply_springs_to_shell(&mut self) {
        let w = self.sp_w.pos.max(8.0);
        let h = self.sp_h.pos.max(2.0);
        let r = self.sp_r.pos.max(0.0);
        let isl = self.sp_island.pos.clamp(0.0, 1.0);
        let vis = self.sp_vis.pos.clamp(0.0, 1.0);
        let smoothstep = |e0: f32, e1: f32, v: f32| {
            let t = ((v - e0) / (e1 - e0)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        // Continuous liquid morph (Notch <-> Floating Pill):
        // 0.00..0.46: concave bezel ears gracefully retract to 0px while detaching downward;
        // 0.42..1.00: top corners smoothly round from 0px into a full floating pill (`top_r -> r`).
        // Both phases meet continuously at `(ear = 0, top_r = 0)` -> zero geometry pop or blink!
        let ear = (EAR_DP * self.dpi).ceil() * smoothstep(0.46, 0.0, isl);
        let top_r = r * smoothstep(0.42, 1.0, isl);
        let y = self.sp_y.pos;
        // Bezel-attached Notch stays crisp solid black while absorbing into the bezel and dissolves only
        // on the final 35% tuck; Floating Pill dissolves smoothly across 88%..0% for weightless float-out.
        let fade_edge = 0.35 + 0.53 * isl;
        let stage_op = smoothstep(0.0, fade_edge, vis);
        let x0 = self.x0_for(w);
        let detached = self.detached_timer_geom(x0, w, h, r, ear);
        let detached_top_r = |tr: f32| tr * smoothstep(0.42, 1.0, isl);

        self.shell.set_geometry_with_detached(
            x0,
            w,
            h,
            r,
            top_r,
            ear,
            detached.map(|(tx, tw, th, tr, te, _)| (tx, tw, th, tr, detached_top_r(tr), te)),
        );
        self.shell.set_stage_y(y);
        self.shell.set_stage_opacity(stage_op);
        STAGE_Y_BITS.store(y.to_bits(), Ordering::Relaxed);
        self.shell.set_dual_shadow(
            self.sp_sh_blur.pos * self.dpi,
            self.sp_sh_dy.pos * self.dpi,
            self.sp_sh_op.pos,
            self.sp_csh_blur.pos * self.dpi,
            self.sp_csh_dy.pos * self.dpi,
            self.sp_csh_op.pos,
        );

        // Keep compact left/right chrome, compact lyric viewport, bottom ambient glow, and detached timer pill glued to animated edges.
        if let Some(left_c) = &self.live_vis.compact_left {
            let _ = left_c.SetOffset(Vector3 { X: x0, Y: 0.0, Z: 0.0 });
        }
        if let Some(right_c) = &self.live_vis.compact_right {
            let _ = right_c.SetOffset(Vector3 { X: x0 + w, Y: 0.0, Z: 0.0 });
        }
        if let Some(cl) = self.live_vis.compact_lyric.as_mut() {
            let vw = (w - cl.left_pad - cl.right_pad).max(24.0 * self.dpi);
            cl.vw = vw;
            let _ = cl.viewport.SetOffset(Vector3 { X: x0 + cl.left_pad, Y: 0.0, Z: 0.0 });
            let _ = cl.viewport.SetSize(Vector2 { X: vw, Y: cl.h });
            if !cl.active_marquee {
                if let Some(act) = &cl.active {
                    let _ = act.SetOffset(Vector3 { X: ((vw - cl.active_tw) / 2.0).max(0.0), Y: 0.0, Z: 0.0 });
                }
            }
        }
        if let Some(glow) = &self.live_vis.compact_glow {
            let _ = glow.SetOffset(Vector3 { X: x0, Y: 0.0, Z: 0.0 });
            let _ = glow.SetSize(Vector2 { X: w.ceil(), Y: h.ceil() });
        }
        if let Some(glow) = &self.live_vis.tab_glow {
            let _ = glow.SetOffset(Vector3 { X: x0, Y: 0.0, Z: 0.0 });
            let _ = glow.SetSize(Vector2 { X: w.ceil(), Y: h.ceil() });
        }
        if !self.play_glide_active {
            if let Some(glow) = self.live_vis.play_layout.as_ref().and_then(|pl| pl.lyrics_glow.as_ref()) {
                let _ = glow.SetOffset(Vector3 { X: x0, Y: 0.0, Z: 0.0 });
                let _ = glow.SetSize(Vector2 { X: w.ceil(), Y: h.ceil() });
            }
        }
        if let Some(dt_vis) = &self.live_vis.detached_timer {
            if let Some((tx, _, _, _, _, op)) = detached {
                let _ = dt_vis.SetOffset(Vector3 { X: tx.round(), Y: 0.0, Z: 0.0 });
                let _ = dt_vis.SetOpacity(op);
            } else {
                let _ = dt_vis.SetOpacity(0.0);
            }
        }
        if self.shown && !self.off && matches!(self.look.view, View::Rest | View::Compact(_)) {
            self.publish_notch_for_width(w, false);
        }

        let fully_gone = (self.sp_y.is_settled()
            && self.sp_h.is_settled()
            && self.sp_vis.is_settled()
            && self.sp_sh_op.is_settled())
            || (stage_op <= 0.002 && self.sp_sh_op.pos <= 0.005 && self.sp_csh_op.pos <= 0.005);
        if !self.shown && fully_gone {
            self.sp_w.snap(self.sp_w.target);
            self.sp_h.snap(self.sp_h.target);
            self.sp_r.snap(self.sp_r.target);
            self.sp_y.snap(self.sp_y.target);
            self.sp_vis.snap(0.0);
            self.sp_sh_op.snap(0.0);
            self.sp_csh_op.snap(0.0);
            self.sp_timer_pill.snap(0.0);
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.shell.set_visible(false, self.hwnd);
            self.rgn_rect = (0, 0, 0, 0);
            self.rgn_timer = (0, 0, 0, 0);
            return;
        }

        let l = (x0 - ear).floor().max(0.0) as i32;
        let t = y.floor().max(0.0) as i32;
        let rr = (x0 + w + ear).ceil().min(self.win.0 as f32) as i32;
        let b = (y + h).ceil().clamp(0.0, self.win.1 as f32) as i32;
        let next = (l, t, rr.max(l), b.max(t));
        let next_t = match detached {
            Some((tx, tw, th, _, te, _)) => {
                let tl = (tx - te).floor().max(0.0) as i32;
                let trr = (tx + tw + te).ceil().min(self.win.0 as f32) as i32;
                let tb = (y + th).ceil().clamp(0.0, self.win.1 as f32) as i32;
                (tl, t, trr.max(tl), tb.max(t))
            }
            None => (0, 0, 0, 0),
        };
        if next != self.rgn_rect || next_t != self.rgn_timer {
            self.rgn_rect = next;
            self.rgn_timer = next_t;
            unsafe {
                let rgn = CreateRectRgn(next.0, next.1, next.2, next.3);
                if next_t.2 > next_t.0 && next_t.3 > next_t.1 {
                    let rgn_t = CreateRectRgn(next_t.0, next_t.1, next_t.2, next_t.3);
                    let _ = CombineRgn(Some(rgn), Some(rgn), Some(rgn_t), RGN_OR);
                    let _ = DeleteObject(rgn_t.into());
                }
                SetWindowRgn(self.hwnd, Some(rgn), false);
            }
        }
    }

    fn sync_play_glide(&mut self) {
        let Some(pl) = &self.live_vis.play_layout else { return };
        let cur_w = self.sp_w.pos.max(8.0);
        let cur_h = self.sp_h.pos.max(8.0);
        let cur_x0 = self.x0_for(cur_w);
        let lp = self.sp_lyrics.pos.clamp(0.0, 1.0);
        let pad = 18.0 * self.dpi;
        let top = 14.0 * self.dpi;
        let cur_mx = cur_x0 + pad + 4.0 * self.dpi;
        let mw_full = (cur_w - 2.0 * pad - 8.0 * self.dpi).max(180.0 * self.dpi);
        let mw_split = 224.0 * self.dpi;
        let cur_mw = mw_full + (mw_split - mw_full) * lp;
        let cur_div_x = cur_mx + cur_mw + 14.0 * self.dpi;
        let base_h = (cur_h - state::NAV_EXPAND_H_DP * self.dpi * self.sp_nav.pos.clamp(0.0, 1.0)).max(100.0 * self.dpi);

        let smoothstep = |e0: f32, e1: f32, v: f32| {
            let t = ((v - e0) / (e1 - e0)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };

        // Keep album art + title/artist glued to the animated left edge (subpixel-smooth, zero jitter)
        // and smoothly expand/contract the title clip width with the column boundary.
        if let Some(lg) = &pl.left_group {
            let _ = lg.SetOffset(Vector3 { X: cur_mx - pl.base_mx, Y: 0.0, Z: 0.0 });
        }
        if let Some((base_cx, ec)) = &pl.empty_center {
            let _ = ec.SetOffset(Vector3 { X: (cur_mx + cur_mw / 2.0) - *base_cx, Y: 0.0, Z: 0.0 });
        }
        if let Some(tc) = &pl.title_clip {
            let live_tw = (cur_mw - 70.0 * self.dpi - (1.0 - lp) * 34.0 * self.dpi).max(40.0 * self.dpi);
            let _ = tc.SetSize(Vector2 { X: live_tw.ceil(), Y: pl.title_clip_h });
        }

        // Smoothly stretch/contract scrub bar + timestamps in lockstep with `sp_w` & `lp`.
        // Guarded on `scrub.is_some()` so empty ("Not Playing") state never recurses into `render()`.
        let sy = top + 58.0 * self.dpi + (36.0 + 8.0 * lp) * self.dpi;
        if let Some(scrub) = self.live_vis.scrub.as_mut() {
            scrub.set_bounds(cur_mx, cur_mx + cur_mw, sy, 37.0 * self.dpi, 43.0 * self.dpi, 40.0 * self.dpi);
            self.update_scrub_in_place();
        }

        // Continuously glide all 5 transport controls (X, Y, GPU Scale, and Quote opacity)
        // so both settled endpoints match static layout to 0.00px with zero size or brightness pops.
        let Some(pl) = &self.live_vis.play_layout else { return };
        let cx = cur_mx + cur_mw / 2.0;
        let step = (52.0 + (44.0 - 52.0) * lp) * self.dpi;
        let bot_margin = (24.0 + (12.0 - 24.0) * lp) * self.dpi;
        let col_h = (base_h - bot_margin - top).max(80.0 * self.dpi);
        let target_cy = (top + col_h - 18.0 * self.dpi).max(sy + 28.0 * self.dpi);
        let dy_ctrl = target_cy - pl.base_cy;
        let btn_sc = 1.0 - 0.04 * lp;
        for (act, base_bx, vis) in &pl.ctrl_groups {
            let target_bx = match act {
                Action::ToggleLyrics => (cur_mx + 24.0 * self.dpi) * (1.0 - lp) + (cx - 2.0 * step) * lp,
                Action::Prev => cx - step,
                Action::PlayPause => cx,
                Action::Next => cx + step,
                Action::ToggleMute => (cur_mx + cur_mw - 24.0 * self.dpi) * (1.0 - lp) + (cx + 2.0 * step) * lp,
                _ => *base_bx,
            };
            let _ = vis.SetOffset(Vector3 { X: target_bx - *base_bx, Y: dy_ctrl, Z: 0.0 });
            let _ = vis.SetScale(Vector3 { X: btn_sc, Y: btn_sc, Z: 1.0 });
            match act {
                Action::ToggleLyrics => {
                    let _ = vis.SetOpacity(0.56 + 0.44 * lp);
                }
                Action::Prev | Action::Next => {
                    let _ = vis.SetOpacity(0.98 - 0.02 * lp);
                }
                _ => {}
            }
        }

        // Sync full-bleed bottom-up media gradient with animated shell width/height + `sp_lyrics`.
        if let Some(gv) = &pl.lyrics_glow {
            let _ = gv.SetOpacity(0.92 + 0.08 * lp);
            let _ = gv.SetOffset(Vector3 { X: cur_x0, Y: 0.0, Z: 0.0 });
            let _ = gv.SetSize(Vector2 {
                X: cur_w.ceil(),
                Y: cur_h.ceil(),
            });
        }

        // 1px vertical divider anchored strictly at `cur_div_x` (blooms vertically, zero left-column overlap).
        if let Some(dv) = &pl.lyrics_divider {
            let div_t = smoothstep(0.16, 0.72, lp);
            let _ = dv.SetOffset(Vector3 { X: cur_div_x.round(), Y: (top + 4.0 * self.dpi).round(), Z: 0.0 });
            let _ = dv.SetScale(Vector3 { X: 1.0, Y: 0.55 + 0.45 * div_t, Z: 1.0 });
            let _ = dv.SetOpacity(div_t);
        }

        // Clipped right-column viewport + asymmetric open/close lyrics choreography:
        // Opening waits for breathing room then glides gently in from the right;
        // Closing dissolves briskly ahead of the shrinking right edge so text is never crushed.
        if let Some(vp) = &pl.lyrics_clip {
            let clip_x = cur_div_x + 4.0 * self.dpi;
            let clip_w = (cur_x0 + cur_w - pad - clip_x + 4.0 * self.dpi).max(1.0);
            let lyr_h = (base_h - 12.0 * self.dpi - top).max(80.0 * self.dpi);
            let _ = vp.SetOffset(Vector3 { X: clip_x, Y: top, Z: 0.0 });
            let _ = vp.SetSize(Vector2 { X: clip_w, Y: lyr_h });
        }
        if let Some(panel) = &pl.lyrics_panel {
            let panel_t = if self.lyrics_open {
                smoothstep(0.20, 0.94, lp)
            } else {
                smoothstep(0.32, 0.96, lp)
            };
            let travel = if self.lyrics_open { 16.0 * self.dpi } else { 12.0 * self.dpi };
            let slide_x = (1.0 - panel_t) * travel;
            let sc = 0.965 + 0.035 * panel_t;
            let _ = panel.SetScale(Vector3 { X: sc, Y: sc, Z: 1.0 });
            let _ = panel.SetOpacity(panel_t);
            let _ = panel.SetOffset(Vector3 { X: slide_x, Y: 0.0, Z: 0.0 });
        }

        // Keep top-right chrome (Pin, Bell, 5-bar waveform) glued to the animated right edge
        // while crossfading the waveform and sliding Pin smoothly into/out of the corner slot.
        if let Some((base_right, group)) = &self.live_vis.top_right_group {
            let _ = group.SetOffset(Vector3 { X: (cur_x0 + cur_w) - *base_right, Y: 0.0, Z: 0.0 });
        }
        if let Some(wave) = &self.live_vis.top_right_wave {
            let wave_op = smoothstep(0.0, 0.60, 1.0 - lp);
            let wave_sc = 0.78 + 0.22 * wave_op;
            let _ = wave.SetOpacity(wave_op);
            let _ = wave.SetScale(Vector3 { X: wave_sc, Y: wave_sc, Z: 1.0 });
        }
        if self.live_vis.top_right_pin_shift > 0.0 {
            if let Some(pin) = &self.live_vis.pin_vis {
                let _ = pin.SetOffset(Vector3 { X: self.live_vis.top_right_pin_shift * lp, Y: 0.0, Z: 0.0 });
            }
        }

        // Choreograph bottom page dots in lockstep with `lp` (graceful fade + vertical drift
        // as controls slide down/up, eliminating the 1-frame vanish on open and 2-stage pop on close).
        if let Some(nav) = &self.live_vis.nav {
            let dots_raw = ((0.55 - lp) / 0.55).clamp(0.0, 1.0);
            let dots_op = dots_raw * dots_raw * (3.0 - 2.0 * dots_raw);
            let dots_dy = (1.0 - dots_op) * 6.0 * self.dpi;
            let _ = nav.anchor.SetOpacity(dots_op);
            let _ = nav.anchor.SetOffset(Vector3 { X: 0.0, Y: nav.cur_cy + dots_dy, Z: 0.0 });
        }
    }

    fn tick_frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.frame_last).as_secs_f32().clamp(0.001, 0.05);
        self.frame_last = now;

        let mut springs_moving = false;
        springs_moving |= self.sp_w.step(dt);
        springs_moving |= self.sp_h.step(dt);
        springs_moving |= self.sp_r.step(dt);
        springs_moving |= self.sp_ear.step(dt);
        springs_moving |= self.sp_island.step(dt);
        springs_moving |= self.sp_y.step(dt);
        springs_moving |= self.sp_vis.step(dt);
        springs_moving |= self.sp_sh_blur.step(dt);
        springs_moving |= self.sp_sh_dy.step(dt);
        springs_moving |= self.sp_sh_op.step(dt);
        springs_moving |= self.sp_csh_blur.step(dt);
        springs_moving |= self.sp_csh_dy.step(dt);
        springs_moving |= self.sp_csh_op.step(dt);
        springs_moving |= self.sp_timer_pill.step(dt);

        if springs_moving {
            self.apply_springs_to_shell();
        } else if !self.shown && self.rgn_rect != (0, 0, 0, 0) {
            self.apply_springs_to_shell();
        }

        let nav_moving = self.sp_nav.step(dt);
        if nav_moving || springs_moving {
            if let Some(nav) = self.live_vis.nav.as_mut() {
                nav.set_progress_and_height(self.sp_nav.pos, self.sp_h.pos);
            }
            if nav_moving {
                springs_moving = true;
            }
        }

        // Smooth gliding reveal/hide for Tab::Play (controls, scrub bar, divider, lyrics panel & glow).
        let lyrics_moving = self.sp_lyrics.step(dt);
        if lyrics_moving {
            springs_moving = true;
        }
        if self.play_glide_active && self.live_vis.play_layout.is_some() && (springs_moving || lyrics_moving) {
            self.sync_play_glide();
        }
        if self.play_glide_active
            && !lyrics_moving
            && self.sp_w.is_settled()
            && self.sp_h.is_settled()
            && self.sp_lyrics.is_settled()
            && self.mode == Mode::Open(Tab::Play)
        {
            // Glide completed at exact target coordinates: commit final hit-regions & drop hidden transition layers
            // (every visible element is already at its exact settled pixel, scale, and opacity -> zero pop).
            self.play_glide_active = false;
            self.render();
        }

        // Smooth scrub-bar hover bloom (track thicken + thumb expand).
        if self.sp_scrub_hover.step(dt) {
            self.update_scrub_in_place();
            springs_moving = true;
        }

        // Smooth GPU vertical scroll + interactive scrollbar rail animations (60–120 fps).
        let mut scroll_moved = false;
        scroll_moved |= self.sp_scroll.step(dt);
        scroll_moved |= self.sp_sb_hover.step(dt);
        scroll_moved |= self.sp_sb_active.step(dt);
        if scroll_moved {
            if let Some(max_scroll) = self.live_vis.scroll.as_ref().map(|s| s.max_scroll) {
                let clamped = self.sp_scroll.pos.clamp(0.0, max_scroll);
                self.set_active_scroll_px(clamped);
            }
            self.sync_scroll_visual();
            springs_moving = true;
        }

        // Album art Play/Pause continuity scale + subtle opacity breath.
        if self.sp_art_scale.step(dt) {
            if let Some(art) = &self.live_vis.album_art {
                let sc = self.sp_art_scale.pos.clamp(0.75, 1.15);
                let _ = art.SetScale(Vector3 { X: sc, Y: sc, Z: 1.0 });
                let op = (0.78 + 0.22 * ((sc - 0.90) / 0.10).clamp(0.0, 1.0)).clamp(0.76, 1.0);
                let _ = art.SetOpacity(op);
            }
            springs_moving = true;
        }

        // Smooth audio visualizer bar interpolation at monitor refresh rate.
        let mut vis_active = false;
        if self.shown && !self.off && !self.live_vis.bars.is_empty() {
            let t_sec = self.t0.elapsed().as_secs_f32();
            let raw_energy: f32 = self.bar_tgt.iter().sum();
            for (i, bar) in self.live_vis.bars.iter().enumerate() {
                let tgt = if !self.playing {
                    0.08
                } else if raw_energy > 0.02 {
                    self.bar_tgt.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0)
                } else {
                    // Subtle alive wave if WASAPI loopback is silent while media reports Playing.
                    0.18 + 0.16 * (t_sec * 4.2 + i as f32 * 0.95).sin().abs()
                };
                let cur = self.bar_cur[i];
                let rate = if tgt > cur { 24.0 } else { 9.5 };
                let next = cur + (tgt - cur) * (1.0 - (-rate * dt).exp());
                self.bar_cur[i] = next;
                let _ = bar.SetScale(Vector3 { X: 1.0, Y: 0.22 + 0.78 * next.clamp(0.0, 1.0), Z: 1.0 });
                if self.playing || (next - tgt).abs() > 0.01 {
                    vis_active = true;
                }
            }
        }

        // Smooth compact media marquee scrolling (pause -> scroll -> pause -> return).
        let mut marquee_active = false;
        if self.shown && !self.off {
            if let Some(mq) = &self.live_vis.marquee {
                self.marquee_t += dt;
                const PAUSE: f32 = 1.6;
                let speed = 28.0 * self.dpi;
                let dur = (mq.travel / speed.max(10.0)).max(1.2);
                let cycle = PAUSE + dur + PAUSE + dur;
                let phase = self.marquee_t % cycle;
                let frac = if phase < PAUSE {
                    0.0
                } else if phase < PAUSE + dur {
                    (phase - PAUSE) / dur
                } else if phase < PAUSE + dur + PAUSE {
                    1.0
                } else {
                    1.0 - (phase - PAUSE - dur - PAUSE) / dur
                };
                // Smoothstep easing on both ends.
                let s = frac * frac * (3.0 - 2.0 * frac);
                let _ = mq.vis.SetOffset(Vector3 { X: -(mq.travel * s), Y: mq.base_y, Z: 0.0 });
                marquee_active = true;
            }
        }

        if !springs_moving && !vis_active && !marquee_active {
            self.stop_frame();
        }
    }

    fn live(&self) -> Live {
        let running = self.timer.phase == Phase::Running;
        Live {
            notification: crate::prefs::get().notch_notifs && self.toast_id.is_some_and(|id| self.notifs.get(id).is_some()),
            prayer_alert: self.prayer_alert.is_some(),
            media: self.has_media,
            playing: self.playing,
            lyric_line: self.lyr.as_ref().is_some_and(|l| l.synced),
            lyrics_on: crate::prefs::get().lyrics_compact && self.lyrics_open,
            // When media is active, the Detached Timer Pill displays the countdown on the right
            // so Live Lyrics / Now Playing stays intact in the main capsule.
            timer_left_s: (running && !self.has_media).then(|| self.timer.remaining_secs(self.now_ms())),
            charging_fresh: self.charging_fresh,
            shelf: self.shelf.len(),
            glance: crate::prefs::get().notch_idle == 0,
            download: (crate::prefs::get().notch_downloads && (self.dl.is_some() || self.dl_done.is_some())) || self.send_compact().is_some(),
            wide_music: crate::prefs::get().notch_idle == 2,
        }
    }

    fn start_send(&mut self, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        self.send = None;
        let en = crate::prefs::get().lang == nacre_core::i18n::Lang::En;
        let hw = self.hwnd.0 as isize;
        match nacre_platform::sendsrv::start(&paths, en, move || post(hw, WM_SEND)) {
            Ok(server) => {
                let px = (86.0 * self.dpi).floor();
                let qr = nacre_core::qr::encode(&server.url).and_then(|m| {
                    let bmp = nacre_core::qr::render(&m, px as u32, 1);
                    self.paint.gfx.surface_from_bgra(px as u32, px as u32, &bmp).ok()
                });
                let names = paths.iter().filter_map(|p| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().to_string())).collect();
                let total = paths.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
                self.send = Some(SendSess {
                    server,
                    qr,
                    qr_px: px,
                    names,
                    prog: nacre_platform::sendsrv::Progress { total, ..Default::default() },
                    last: Instant::now(),
                    done_at: None,
                });
                self.send_err = false;
                timer(self.hwnd, T_SEND, 30_000);
            }
            Err(e) => {
                log::line("kirim", &format!("gagal memulai: {e}"));
                self.send_err = true;
            }
        }
        match self.mode {
            Mode::Drop => self.dispatch(Event::Dropped),
            Mode::Open(Tab::Shelf) => self.refresh(),
            Mode::Open(_) => self.dispatch(Event::SelectTab(Tab::Shelf)),
            _ => {
                self.dispatch(Event::Click);
                self.dispatch(Event::SelectTab(Tab::Shelf));
            }
        }
    }

    fn stop_send(&mut self) {
        if self.send.take().is_some() {
            log::line("kirim", "sesi diakhiri");
        }
        self.send_err = false;
        kill(self.hwnd, T_SEND);
        kill(self.hwnd, T_SENDDONE);
        self.refresh();
        if !self.tracking && matches!(self.mode, Mode::Open(_)) {
            self.dispatch(Event::Leave);
        }
    }

    fn send_pinned(&self) -> bool {
        self.mode == Mode::Open(Tab::Shelf) && (self.send.is_some() || self.send_err)
    }

    fn on_send(&mut self) {
        let Some(s) = self.send.as_mut() else { return };
        let mut changed = false;
        while let Ok(p) = s.server.progress.try_recv() {
            s.prog = p;
            s.last = Instant::now();
            changed = true;
        }
        let n = s.names.len();
        if s.prog.files_done >= n && !s.prog.active && s.done_at.is_none() && s.prog.sent > 0 {
            s.done_at = Some(Instant::now());
            log::line("kirim", &format!("{n} berkas terkirim"));
            timer(self.hwnd, T_SENDDONE, nacre_core::notch::download::DONE_LINGER_MS);
        } else if s.prog.active {
            s.done_at = None;
        }
        if changed {
            self.refresh();
        }
    }

    fn send_compact(&self) -> Option<(f32, bool)> {
        let s = self.send.as_ref()?;
        let done = s.done_at.is_some_and(|t| t.elapsed().as_millis() < nacre_core::notch::download::DONE_LINGER_MS as u128);
        (s.prog.active || done).then(|| (nacre_core::notch::send::fraction(s.prog.sent, s.prog.total), done && !s.prog.active))
    }

    fn today() -> views::Date {
        let t = nacre_platform::time::local_now();
        (t.year, t.month, t.day)
    }

    fn cal_date(&self) -> views::Date {
        let (y, m, d) = Self::today();
        nacre_core::island::calendar::add_days(y, m, d, self.cal_sel)
    }

    fn ensure_events(&mut self) {
        let date = self.cal_date();
        let same_date = self.cal_events.as_ref().is_some_and(|(d, _)| *d == date);
        let fresh = same_date
            && self
                .cal_fetched_at
                .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(30));
        if fresh || (same_date && self.cal_in_flight) {
            return;
        }
        if self.agenda.is_none() {
            let hw = self.hwnd.0 as isize;
            self.agenda = Some(nacre_platform::agenda::spawn(move || post(hw, WM_AGENDA)));
        }
        if let Some(a) = &self.agenda {
            self.cal_in_flight = true;
            let _ = a.commands.send(nacre_platform::agenda::Cmd::Fetch(date));
        }
    }

    fn on_agenda(&mut self) {
        let Some(a) = &self.agenda else { return };
        let mut got = false;
        while let Ok(msg) = a.answers.try_recv() {
            match msg {
                nacre_platform::agenda::AgendaMsg::Loaded(d, list) => {
                    self.cal_events = Some((d, list));
                    self.cal_fetched_at = Some(Instant::now());
                    self.cal_in_flight = false;
                    got = true;
                }
                nacre_platform::agenda::AgendaMsg::CreateFailed(err) => {
                    self.todo_cal_err = err;
                    got = true;
                }
            }
        }
        if got && self.mode == Mode::Open(Tab::Todo) {
            self.render();
        }
    }

    fn request_icon(&mut self, kind: IconKind, key: String, px: i32) {
        if key.is_empty() {
            return;
        }
        let k = (kind, key.clone());
        if self.icons_pending.insert(k) {
            let _ = self.icons.requests.send(IconReq { kind, key, px });
        }
    }

    fn on_icon(&mut self) {
        let mut need_render = false;
        while let Ok(resp) = self.icons.answers.try_recv() {
            self.icons_pending.remove(&(resp.kind, resp.key.clone()));
            let surf = resp.bmp.and_then(|b| self.paint.gfx.surface_from_bgra(b.w, b.h, &b.bgra).ok());
            match resp.kind {
                IconKind::Shelf => {
                    if let Some(s) = surf {
                        self.shelf_icons.insert(resp.key, s);
                        if self.mode == Mode::Open(Tab::Shelf) {
                            need_render = true;
                        }
                    }
                }
                IconKind::MediaApp => {
                    let cur = (resp.key == self.media_app && self.art.is_none())
                        || self.mode == Mode::Open(Tab::Alerts)
                        || self.look.view == View::Compact(state::Activity::Notification);
                    self.app_icons.insert(resp.key, surf);
                    if cur {
                        need_render = true;
                    }
                }
                IconKind::Download => {
                    self.dl_icons.insert(resp.key, surf);
                    if self.look.view == View::Compact(state::Activity::Download) {
                        need_render = true;
                    }
                }
            }
        }
        if need_render {
            self.render();
        }
    }

    fn ensure_app_icon(&mut self) {
        let id = self.media_app.clone();
        if id.is_empty() || self.app_icons.contains_key(&id) {
            return;
        }
        let px = (48.0 * self.dpi).round() as i32;
        self.request_icon(IconKind::MediaApp, id, px);
    }

    fn sync_notif_watch(&mut self) {
        let on = crate::prefs::get().notch_notifs;
        if on && self.notif_mon.is_none() {
            let hw = self.hwnd.0 as isize;
            self.notif_mon = Some(nacre_platform::notifications::spawn(move || post(hw, WM_NOTIF)));
        } else if !on && self.notif_mon.is_some() {
            self.notif_mon = None;
            self.toast_id = None;
            self.notif_peeking = false;
            kill(self.hwnd, T_NOTIF_TOAST);
        }
    }

    fn push_notification(&mut self, notif: nacre_core::notch::notif::Notification) {
        let aumid = notif.aumid.clone();
        let id = notif.id;
        let is_new = self.notifs.push(notif);
        if !aumid.is_empty() && !self.app_icons.contains_key(&aumid) {
            let px = (48.0 * self.dpi).round() as i32;
            self.request_icon(IconKind::MediaApp, aumid, px);
        }
        if is_new && crate::prefs::get().notch_notifs {
            self.toast_id = Some(id);
            timer(self.hwnd, T_NOTIF_TOAST, NOTIF_TOAST_MS);
            if crate::prefs::get().notch_notif_peek && !self.overlap.fullscreen {
                self.notif_peeking = true;
            }
        }
    }

    fn save_todos(&self) {
        save_persisted_todos(&self.todos);
    }

    fn focus_input(&mut self, which: ActiveInput) {
        self.active_input = Some(which);
        self.focus.acquire(self.hwnd);
        if let Some(ti) = self.todo_input.as_mut() {
            ti.set_focused(which == ActiveInput::Todo, &mut self.paint);
        }
        if let Some(gi) = self.gemini_input.as_mut() {
            gi.set_focused(which == ActiveInput::Gemini, &mut self.paint);
        }
        timer(self.hwnd, T_CARET, 530);
    }

    fn blur_inputs(&mut self, restore_prev: bool) {
        let was_todo = self.active_input == Some(ActiveInput::Todo);
        self.active_input = None;
        kill(self.hwnd, T_CARET);
        if let Some(ti) = self.todo_input.as_mut() {
            self.todo_draft = ti.buf.as_str().to_string();
            ti.set_focused(false, &mut self.paint);
        }
        if let Some(gi) = self.gemini_input.as_mut() {
            self.gemini_draft = gi.buf.as_str().to_string();
            gi.set_focused(false, &mut self.paint);
        }
        self.focus.release(self.hwnd, restore_prev);
        if was_todo && self.todo_draft.trim().is_empty() && (self.todo_adding || self.todo_cal_adding) {
            self.todo_adding = false;
            self.todo_cal_adding = false;
            if self.mode == Mode::Open(Tab::Todo) {
                self.render();
            }
        }
        self.update_visibility();
    }

    fn submit_todo(&mut self) {
        let text = self
            .todo_input
            .as_ref()
            .map(|ti| ti.buf.as_str().to_string())
            .unwrap_or_else(|| self.todo_draft.clone());
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        // In Calendar Schedule mode, "+ Add Schedule" creates a Google Calendar event (NOT a checklist task).
        if self.todo_cal_mode && self.todo_cal_adding {
            let input = trimmed.to_string();
            self.todo_draft.clear();
            if let Some(ti) = self.todo_input.as_mut() {
                ti.buf.clear();
            }
            self.todo_cal_adding = false;
            self.todo_cal_err.clear();
            self.todo_cal_scroll = 0.0;
            if self.agenda.is_none() {
                let hw = self.hwnd.0 as isize;
                self.agenda = Some(nacre_platform::agenda::spawn(move || post(hw, WM_AGENDA)));
            }
            if let Some(a) = &self.agenda {
                self.cal_in_flight = true;
                self.cal_fetched_at = None;
                let _ = a.commands.send(nacre_platform::agenda::Cmd::Create {
                    date: self.cal_date(),
                    input,
                });
            }
            self.blur_inputs(false);
            self.render();
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if let Some(item) = self.todos.add_local(trimmed, now) {
            self.todo_draft.clear();
            if let Some(ti) = self.todo_input.as_mut() {
                ti.buf.clear();
            }
            self.todo_scroll = 0.0;
            self.save_todos();
            let _ = self.gtasks.commands.send(nacre_platform::gtasks::Cmd::Add {
                local_id: item.id,
                title: item.title,
            });
            self.render();
        }
    }

    fn submit_gemini(&mut self) {
        let prompt = self
            .gemini_input
            .as_ref()
            .map(|gi| gi.buf.as_str().to_string())
            .unwrap_or_else(|| self.gemini_draft.clone());
        let trimmed = prompt.trim();
        if trimmed.is_empty() {
            return;
        }
        self.submit_gemini_prompt(trimmed.to_string());
    }

    fn submit_gemini_prompt(&mut self, prompt: String) {
        self.gemini_req_id = self.gemini_req_id.wrapping_add(1);
        let id = self.gemini_req_id;
        self.gemini_resp.clear();
        self.gemini_err.clear();
        self.gemini_streaming = true;
        self.gemini_scroll = 0.0;
        self.sp_scroll.snap(0.0);
        self.gemini_auto_follow = true;
        self.gemini_copied = false;
        self.gemini_draft.clear();
        if let Some(gi) = self.gemini_input.as_mut() {
            gi.buf.clear();
        }
        let s = crate::prefs::get();
        let model = nacre_core::notch::gemini::active_model_id(s.gemini_model, s.gemini_custom_model.as_str()).to_string();
        let _ = self.gemini.commands.send(nacre_platform::gemini::Cmd::Ask {
            id,
            prompt,
            model,
        });
        self.render();
    }

    fn on_char(&mut self, unit: u16) {
        match self.active_input {
            Some(ActiveInput::Todo) => {
                if let Some(ti) = self.todo_input.as_mut() {
                    ti.on_char(unit, &mut self.paint);
                    self.todo_draft = ti.buf.as_str().to_string();
                }
            }
            Some(ActiveInput::Gemini) => {
                if let Some(gi) = self.gemini_input.as_mut() {
                    gi.on_char(unit, &mut self.paint);
                    self.gemini_draft = gi.buf.as_str().to_string();
                }
            }
            None => {}
        }
    }

    fn on_keydown(&mut self, vk: u16) {
        let outcome = match self.active_input {
            Some(ActiveInput::Todo) => self.todo_input.as_mut().map(|ti| {
                let r = ti.on_keydown(vk, &mut self.paint);
                self.todo_draft = ti.buf.as_str().to_string();
                r
            }),
            Some(ActiveInput::Gemini) => self.gemini_input.as_mut().map(|gi| {
                let r = gi.on_keydown(vk, &mut self.paint);
                self.gemini_draft = gi.buf.as_str().to_string();
                r
            }),
            None => None,
        };
        match (self.active_input, outcome) {
            (Some(ActiveInput::Todo), Some(InputOutcome::Submit)) => self.submit_todo(),
            (Some(ActiveInput::Gemini), Some(InputOutcome::Submit)) => self.submit_gemini(),
            (_, Some(InputOutcome::Blur)) => self.blur_inputs(true),
            _ => {}
        }
    }

    fn on_gtasks(&mut self) {
        let mut changed = false;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        while let Ok(ev) = self.gtasks.events.try_recv() {
            match ev {
                nacre_platform::gtasks::Event::Status(st) => {
                    let newly_synced = st == nacre_platform::gtasks::SyncStatus::Synced
                        && self.todo_status != nacre_platform::gtasks::SyncStatus::Synced;
                    self.todo_status = st;
                    changed = true;
                    if newly_synced {
                        // Refresh Google Calendar agendas right after unified Google OAuth syncs.
                        self.cal_events = None;
                        self.cal_fetched_at = None;
                        self.cal_in_flight = false;
                        self.ensure_events();
                    }
                }
                nacre_platform::gtasks::Event::Synced(list) => {
                    self.todos.merge_remote(list, now);
                    self.save_todos();
                    // Allow calendar to re-fetch fresh events from Google Calendar on next open / immediately if open.
                    self.cal_fetched_at = None;
                    self.todo_cal_err.clear();
                    if self.mode == Mode::Open(Tab::Todo) {
                        self.ensure_events();
                    }
                    changed = true;
                }
                nacre_platform::gtasks::Event::Created { local_id, remote_id } => {
                    self.todos.bind_remote(&local_id, remote_id);
                    self.save_todos();
                    changed = true;
                }
                nacre_platform::gtasks::Event::MutationDone { local_id } => {
                    self.todos.mark_synced(&local_id);
                    self.save_todos();
                }
            }
        }
        if changed && self.mode == Mode::Open(Tab::Todo) {
            self.render();
        }
    }

    fn on_gemini(&mut self) {
        let mut delta = false;
        let mut state_changed = false;
        while let Ok(ev) = self.gemini.events.try_recv() {
            match ev {
                nacre_platform::gemini::Event::Delta { id, text } if id == self.gemini_req_id => {
                    self.gemini_resp.push_str(&text);
                    delta = true;
                }
                nacre_platform::gemini::Event::Done { id } if id == self.gemini_req_id => {
                    self.gemini_streaming = false;
                    state_changed = true;
                }
                nacre_platform::gemini::Event::Error { id, message } if id == self.gemini_req_id => {
                    self.gemini_streaming = false;
                    self.gemini_err = message;
                    self.gemini_scroll = 0.0;
                    self.sp_scroll.snap(0.0);
                    self.gemini_auto_follow = false;
                    state_changed = true;
                }
                _ => {}
            }
        }
        if self.mode != Mode::Open(Tab::Gemini) {
            return;
        }
        if state_changed {
            self.render();
        } else if delta {
            let has_key = nacre_platform::gemini::has_api_key();
            let cur_px = self.sp_scroll.pos;
            let max_scroll = views::refresh_gemini_response(
                &mut self.paint,
                &mut self.live_vis,
                &self.gemini_resp,
                &self.gemini_err,
                has_key,
                self.gemini_streaming,
                false,
                cur_px,
                self.sp_sb_hover.pos,
                self.scroll_drag.is_some(),
                self.sp_sb_active.pos,
            );
            if self.gemini_auto_follow {
                self.sp_scroll.set_target(max_scroll, SpringParams::smooth_scroll());
                self.gemini_scroll = max_scroll;
                self.start_frame();
            } else {
                let clamped = self.gemini_scroll.clamp(0.0, max_scroll);
                self.gemini_scroll = clamped;
                if self.sp_scroll.target > max_scroll {
                    self.sp_scroll.set_target(clamped, SpringParams::smooth_scroll());
                }
            }
        }
    }

    fn on_notif(&mut self) {
        let Some(mon) = &self.notif_mon else { return };
        let mut events = Vec::new();
        while let Ok(ev) = mon.events.try_recv() {
            events.push(ev);
        }
        if events.is_empty() {
            return;
        }
        let mut changed = false;
        for ev in events {
            match ev {
                nacre_platform::notifications::Event::Status(st) => {
                    self.notif_status = st;
                    if self.mode == Mode::Open(Tab::Alerts) {
                        changed = true;
                    }
                }
                nacre_platform::notifications::Event::Seed(list) => {
                    for raw in list {
                        let aumid = raw.aumid.clone();
                        self.notifs.push(nacre_core::notch::notif::Notification {
                            id: raw.id,
                            app_name: raw.app_name,
                            aumid: raw.aumid,
                            title: raw.title,
                            body: raw.body,
                            timestamp_s: raw.timestamp_s,
                            read: true,
                        });
                        if !aumid.is_empty() && !self.app_icons.contains_key(&aumid) {
                            let px = (48.0 * self.dpi).round() as i32;
                            self.request_icon(IconKind::MediaApp, aumid, px);
                        }
                    }
                    changed = true;
                }
                nacre_platform::notifications::Event::Arrived(raw) => {
                    self.push_notification(nacre_core::notch::notif::Notification {
                        id: raw.id,
                        app_name: raw.app_name,
                        aumid: raw.aumid,
                        title: raw.title,
                        body: raw.body,
                        timestamp_s: raw.timestamp_s,
                        read: self.mode == Mode::Open(Tab::Alerts),
                    });
                    changed = true;
                }
                nacre_platform::notifications::Event::Removed(id) => {
                    if self.notifs.dismiss(id) {
                        changed = true;
                    }
                    if self.toast_id == Some(id) {
                        self.toast_id = None;
                        self.notif_peeking = false;
                        kill(self.hwnd, T_NOTIF_TOAST);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            self.refresh();
            self.update_visibility();
        }
    }

    fn sync_download_watch(&mut self) {
        let on = crate::prefs::get().notch_downloads;
        if on && self.dl_watch.is_none() {
            let hw = self.hwnd.0 as isize;
            self.dl_watch = Some(nacre_platform::dlwatch::spawn(move || post(hw, WM_DOWNLOAD)));
        } else if !on && self.dl_watch.is_some() {
            self.dl_watch = None;
            self.dl = None;
            self.dl_done = None;
        }
    }

    fn on_download(&mut self) {
        let Some(w) = &self.dl_watch else { return };
        let mut changed = false;
        while let Ok(s) = w.snapshots.try_recv() {
            self.dl = nacre_core::notch::download::primary(&s.active).cloned();
            if self.dl.is_some() {
                self.dl_done = None;
            } else if let Some(f) = s.finished {
                log::line("unduhan", &format!("selesai ({:?})", f.source));
                self.dl_done = Some(f);
                timer(self.hwnd, T_DLDONE, nacre_core::notch::download::DONE_LINGER_MS);
            }
            changed = true;
        }
        if changed {
            let app = self.dl.as_ref().or(self.dl_done.as_ref()).map(|d| d.app.clone()).unwrap_or_default();
            if !app.is_empty() && !self.dl_icons.contains_key(&app) {
                let px = (44.0 * self.dpi).round() as i32;
                self.request_icon(IconKind::Download, app, px);
            }
            self.refresh();
        }
    }

    fn download_view<'a>(
        dl: &'a Option<nacre_core::notch::download::Download>,
        dl_done: &'a Option<nacre_core::notch::download::Download>,
        icons: &'a std::collections::HashMap<String, Option<CompositionDrawingSurface>>,
        send: Option<(f32, bool)>,
    ) -> Option<views::DownloadView<'a>> {
        let (d, done) = match (dl, dl_done) {
            (Some(d), _) => (d, false),
            (None, Some(d)) => (d, true),
            _ => {
                let (fr, done) = send?;
                let pct = (fr * 100.0).floor() as u32;
                return Some(views::DownloadView { icon: None, label: format!("{}", if done { 100 } else { pct.min(99) }), frac: Some(fr), done, up: true });
            }
        };
        Some(views::DownloadView {
            icon: icons.get(&d.app).and_then(|s| s.as_ref()),
            label: nacre_core::notch::download::label(d),
            frac: d.percent.map(|p| p / 100.0),
            done,
            up: false,
        })
    }

    fn glance_items(&self) -> Vec<glance::Item> {
        glance::items(self.battery.is_some())
    }

    fn glance_text(&self, item: glance::Item) -> String {
        let s = crate::prefs::get();
        let now = nacre_platform::time::local_now();
        let lang = s.lang;
        match item {
            glance::Item::Greeting => {
                let name = if s.greet_name.is_empty() { std::env::var("USERNAME").unwrap_or_default() } else { s.greet_name.as_str().to_string() };
                glance::greeting_text(s.greet_text.as_str(), &name, now.hour, lang)
            }
            glance::Item::Clock => glance::clock(now.hour, now.minute, s.clock_24h, lang),
            glance::Item::Date => {
                let wd = calendar::weekday(now.year, now.month, now.day) as usize;
                let days = nacre_core::i18n::weekday_labels(lang, false, calendar::weekday_labels(false));
                let month: String = nacre_core::i18n::month_name(lang, now.month, calendar::month_name(now.month)).chars().take(3).collect();
                format!("{}, {} {}", days[wd], now.day, month)
            }
            glance::Item::Battery => self.battery.map_or(String::new(), |(p, _)| format!("{p}%")),
        }
    }

    fn sync_glance(&self) {
        if self.look == state::look(Mode::Rest, Some(state::Activity::Glance)) {
            let ms = std::env::var("NACRE_GLANCE_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(glance::ROTATE_MS);
            timer(self.hwnd, T_GLANCE, ms);
        } else {
            kill(self.hwnd, T_GLANCE);
        }
    }

    fn rotate_glance(&mut self) {
        let n = self.glance_items().len();
        self.glance_idx = glance::next(self.glance_idx, n);
        self.sync_adaptive_size();
        self.retire_content(120);
        let rise = 6.0 * self.dpi;
        if let Ok(g) = self.shell.ghost.cast::<CompositionObject>() {
            let _ = keyframes(&self.paint.comp, &g, "Offset.Y", 0.0, -rise, 160, 0);
        }
        self.render();
        if let Ok(c) = self.shell.content.cast::<CompositionObject>() {
            let comp = &self.paint.comp;
            let _ = self.shell.content.SetScale(Vector3 { X: 1.0, Y: 1.0, Z: 1.0 });
            let _ = keyframes(comp, &c, "Opacity", 0.0, 1.0, 180, 60);
            let _ = keyframes(comp, &c, "Offset.Y", rise, 0.0, 220, 40);
        }
    }

    fn dispatch(&mut self, ev: Event) {
        let s = crate::prefs::get();
        let open_on = if s.notch_hover_open { OpenOn::Hover } else { OpenOn::Click };
        let next = state::step_with_default(self.mode, ev, open_on, self.tracking, s.default_notch_tab());
        if next != self.mode {
            log::line("notch", &format!("{:?} --{ev:?}--> {next:?}", self.mode));
            if next == Mode::Open(Tab::Alerts) {
                self.notifs.mark_all_read();
                self.alerts_scroll = 0.0;
            }
            self.mode = next;
            self.refresh();
            self.update_visibility();
        }
    }

    fn refresh(&mut self) {
        let look = state::look(self.mode, state::activity(&self.live()));
        if look != self.look || self.first {
            self.transition(look);
        } else {
            self.sync_adaptive_size();
            self.render();
        }
    }

    fn max_safe_compact_w(&self) -> f32 {
        let detached_reserve = if self.want_detached_timer() {
            // Reserve room on the right for the detached timer pill + valley gap while keeping main pill centered.
            2.0 * (DETACHED_TIMER_W_DP + 36.0) * self.dpi
        } else {
            0.0
        };
        let max_win = (self.win.0 as f32 - 2.0 * SHADOW_PAD_X * self.dpi - detached_reserve).max(200.0 * self.dpi);
        let cap = (state::COMPACT_MAX_W_DP * self.dpi).min(max_win);
        let min_safe = (state::COMPACT_MIN_W_DP * self.dpi).min(cap);
        if !crate::prefs::get().menubar {
            return cap;
        }
        let (bar_l, bar_r) = crate::services::bar_occupied();
        if bar_l == 0 && bar_r == 0 {
            return cap.min((self.screen_w as f32 * 0.42).max(min_safe));
        }
        let side = bar_l.max(bar_r) as f32 + 16.0 * self.dpi;
        let avail = self.screen_w as f32 - 2.0 * side - 2.0 * ear_px(self.dpi) - detached_reserve * 0.5;
        avail.clamp(min_safe, cap)
    }

    fn target_size(&self, look: Look) -> Size {
        let ctx = state::SizeContext {
            lyrics_open: self.lyrics_open,
            shelf_empty: self.shelf.is_empty() && self.send.is_none() && !self.send_err,
            nav_expanded: self.nav_expanded,
        };
        let mut z = state::size_with(look, self.dpi, ctx);
        let peek_extra = if look.peek && matches!(look.view, View::Rest | View::Compact(_)) {
            state::PEEK_GROW.0 * self.dpi
        } else {
            0.0
        };
        let max_safe = self.max_safe_compact_w();
        let min_w = state::COMPACT_MIN_W_DP * self.dpi;

        match look.view {
            View::Compact(state::Activity::Lyrics) => {
                let line = self
                    .lyr
                    .as_ref()
                    .and_then(|l| l.current(self.lyric_pos_ms()))
                    .unwrap_or("\u{266A}");
                let text_px = self.paint.measure_dwrite(line, 13.0, 600).max(self.paint.measure(line, 13.0, 600));
                let chrome_px = 108.0 * self.dpi;
                z.w = state::adaptive_compact_w(text_px, chrome_px, min_w, max_safe) + peek_extra;
                z.w = z.w.min(max_safe);
            }
            View::Compact(state::Activity::Music) if self.has_media => {
                let label = if self.artist.is_empty() {
                    self.title.clone()
                } else {
                    format!("{} \u{00B7} {}", self.title, self.artist)
                };
                let text_px = self.paint.measure_dwrite(&label, 12.0, 600).max(self.paint.measure(&label, 12.0, 600));
                let chrome_px = 106.0 * self.dpi;
                let cap = max_safe.min(380.0 * self.dpi);
                z.w = state::adaptive_compact_w(text_px, chrome_px, 204.0 * self.dpi, cap) + peek_extra;
                z.w = z.w.min(cap);
            }
            View::Compact(state::Activity::Glance) => {
                let items = self.glance_items();
                let item = items[self.glance_idx.min(items.len() - 1)];
                let text = self.glance_text(item);
                let text_px = self.paint.measure_dwrite(&text, 12.5, 600).max(self.paint.measure(&text, 12.5, 600));
                let chrome_px = if item == glance::Item::Greeting { 40.0 * self.dpi } else { 66.0 * self.dpi };
                let cap = max_safe.min(290.0 * self.dpi);
                z.w = state::adaptive_compact_w(text_px, chrome_px, min_w, cap) + peek_extra;
                z.w = z.w.min(cap);
            }
            View::Compact(state::Activity::Notification) => {
                if let Some(n) = self.toast_id.and_then(|id| self.notifs.get(id)) {
                    let head = if n.app_name.is_empty() || n.title.eq_ignore_ascii_case(&n.app_name) {
                        n.title.clone()
                    } else {
                        format!("{} \u{00B7} {}", n.app_name, n.title)
                    };
                    let hw = self.paint.measure_dwrite(&head, 12.0, 700);
                    let bw = self.paint.measure_dwrite(&n.body, 11.0, 400);
                    let cap = max_safe.min(400.0 * self.dpi);
                    z.w = state::adaptive_compact_w(hw.max(bw), 84.0 * self.dpi, 260.0 * self.dpi, cap) + peek_extra;
                    z.w = z.w.min(cap);
                }
            }
            View::Compact(state::Activity::PrayerAlert) => {
                if let Some(al) = self.prayer_alert {
                    let hhmm = nacre_core::notch::prayer::format_hhmm(al.time_min);
                    let head = format!("{} {} \u{00B7} {hhmm}", crate::prefs::tr("Waktunya"), al.prayer.name());
                    let sub = crate::prefs::tr("Selamat menunaikan ibadah shalat");
                    let hw = self.paint.measure_dwrite(&head, 12.0, 700);
                    let bw = self.paint.measure_dwrite(sub, 11.0, 500);
                    let cap = max_safe.min(380.0 * self.dpi);
                    z.w = state::adaptive_compact_w(hw.max(bw), 74.0 * self.dpi, 270.0 * self.dpi, cap) + peek_extra;
                    z.w = z.w.min(cap);
                }
            }
            _ => {}
        }

        z.radius *= crate::prefs::round(crate::prefs::Role::Notch);
        z.w = z.w.round();
        z.h = z.h.round();
        let gap = island_gap(self.dpi);
        if gap > 0.0 {
            z.radius = z.radius.min(z.h / 2.0).min(36.0 * self.dpi);
        } else {
            z.radius = z.radius.min((z.h - ear_px(self.dpi)).max(2.0)).min(z.w / 2.0);
        }
        z
    }

    fn sync_adaptive_size(&mut self) {
        let next = self.target_size(self.look);
        let target_tp = if self.want_detached_timer() { 1.0 } else { 0.0 };
        if (next.w - self.size.w).abs() >= 1.5
            || (next.h - self.size.h).abs() >= 1.5
            || (target_tp - self.sp_timer_pill.target).abs() > 0.01
        {
            self.apply_size(Some(SpringParams::swap()));
            self.publish_notch();
        }
    }

    fn spring_for(old: Look, new: Look) -> SpringParams {
        let big = |v: View| matches!(v, View::Open(_) | View::Drop);
        if big(new.view) && !big(old.view) {
            SpringParams::open()
        } else if big(old.view) && !big(new.view) {
            SpringParams::close()
        } else if old.view == new.view {
            SpringParams::peek()
        } else if matches!((old.view, new.view), (View::Open(_), View::Open(_))) {
            SpringParams::tab_glide()
        } else {
            SpringParams::swap()
        }
    }

    fn tab_direction(from: View, to: View) -> i32 {
        let (View::Open(a), View::Open(b)) = (from, to) else { return 0 };
        if a == b {
            return 0;
        }
        let s = crate::prefs::get();
        let vis = s.visible_notch_tabs();
        let (list, ia, ib) = if let (Some(ia), Some(ib)) = (
            vis.iter().position(|t| *t == a),
            vis.iter().position(|t| *t == b),
        ) {
            (vis.len() as i32, ia as i32, ib as i32)
        } else {
            let ia = s.notch_tab_order.iter().position(|t| *t == a).unwrap_or(0) as i32;
            let ib = s.notch_tab_order.iter().position(|t| *t == b).unwrap_or(0) as i32;
            (s.notch_tab_order.len() as i32, ia, ib)
        };
        if list <= 1 {
            return 0;
        }
        let diff = (ib - ia).rem_euclid(list);
        if diff <= list / 2 { 1 } else { -1 }
    }

    fn transition(&mut self, new: Look) {
        let old = self.look;
        let first = self.first;
        self.first = false;
        self.look = new;
        let view_changed = old.view != new.view || first;
        let tab_dir = if first { 0 } else { Self::tab_direction(old.view, new.view) };
        if let (View::Open(a), View::Open(b)) = (old.view, new.view) {
            if a != b {
                self.prev_tab = Some(a);
            }
        } else {
            self.prev_tab = None;
        }
        if view_changed {
            self.play_glide_active = false;
            self.scroll_drag = None;
            self.sb_hovered = false;
            self.sp_sb_hover.snap(0.0);
            self.sp_sb_active.snap(0.0);
            kill(self.hwnd, T_SCROLL_IDLE);
        }
        if view_changed && self.focus.is_active() {
            self.blur_inputs(false);
        }
        if !matches!(new.view, View::Open(_)) || (new.view == View::Open(Tab::Play) && self.lyrics_open) {
            self.nav_expanded = false;
            self.nav_collapse_armed = false;
            self.sp_nav.snap(0.0);
            kill(self.hwnd, T_NAV_COLLAPSE);
        }
        if !matches!(new.view, View::Open(_)) {
            self.cal_sel = 0;
            self.todo_adding = false;
            self.todo_editing = false;
            self.todo_cal_adding = false;
            self.scrub_hovered = false;
            self.sp_scrub_hover.snap(0.0);
            self.sp_lyrics.snap(if self.lyrics_open { 1.0 } else { 0.0 });
        } else {
            if new.view != View::Open(Tab::Todo) {
                self.todo_adding = false;
                self.todo_editing = false;
                self.todo_cal_adding = false;
            }
            if new.view == View::Open(Tab::Play) && old.view != View::Open(Tab::Play) {
                self.sp_lyrics.snap(if self.lyrics_open { 1.0 } else { 0.0 });
            }
        }
        let sp = (!first).then(|| Self::spring_for(old, new));
        let hiding_now = !first && self.shown && self.will_hide();
        if view_changed && !first {
            let closing = matches!(old.view, View::Open(_) | View::Drop) && !matches!(new.view, View::Open(_) | View::Drop);
            let fade_ms = if hiding_now {
                135
            } else if closing {
                motion::CLOSE_FADE_MS
            } else {
                motion::SWAP_FADE_MS
            };
            self.retire_content_dir(fade_ms, tab_dir, closing || hiding_now);
        }
        self.apply_size(sp);
        self.publish_notch();
        self.render();
        self.sync_glance();
        if hiding_now {
            // Keep incoming compact/glance content hidden while the Notch smoothly retracts away
            // so Glance text never flashes in right before disappearing.
            let _ = self.shell.content.SetOpacity(0.0);
            self.content_hidden_for_retract = true;
        } else if view_changed && !first {
            let opening = matches!(new.view, View::Open(_) | View::Drop) && !matches!(old.view, View::Open(_) | View::Drop);
            self.intro(opening, tab_dir);
        }
    }

    fn apply_size(&mut self, sp: Option<SpringParams>) {
        let z = self.target_size(self.look);
        self.size = z;
        let gap = island_gap(self.dpi);
        let target_ear = ear_px(self.dpi);
        let target_isl = if crate::prefs::get().notch_island { 1.0 } else { 0.0 };
        let (target_w, target_h, target_r, target_y, target_vis) = if self.shown {
            (z.w, z.h, z.radius, gap, 1.0)
        } else {
            let (hw, hh, hr, hy) = self.hidden_pose(z);
            (hw, hh, hr, hy, 0.0)
        };
        let (target_sh_blur, target_sh_dy, sh_op) = state::shadow(self.look);
        let target_sh_op = if self.shown { sh_op } else { 0.0 };
        let (target_csh_blur, target_csh_dy, csh_op) = state::contact_shadow(self.look);
        let target_csh_op = if self.shown { csh_op } else { 0.0 };
        let target_timer_pill = if self.shown && self.want_detached_timer() { 1.0 } else { 0.0 };

        match sp {
            None => {
                self.sp_w.snap(target_w);
                self.sp_h.snap(target_h);
                self.sp_r.snap(target_r);
                self.sp_ear.snap(target_ear);
                self.sp_island.snap(target_isl);
                self.sp_y.snap(target_y);
                self.sp_vis.snap(target_vis);
                self.sp_sh_blur.snap(target_sh_blur);
                self.sp_sh_dy.snap(target_sh_dy);
                self.sp_sh_op.snap(target_sh_op);
                self.sp_csh_blur.snap(target_csh_blur);
                self.sp_csh_dy.snap(target_csh_dy);
                self.sp_csh_op.snap(target_csh_op);
                self.sp_timer_pill.snap(target_timer_pill);
                self.stop_frame();
                self.apply_springs_to_shell();
            }
            Some(p) => {
                let shape_changing = (self.sp_island.target - target_isl).abs() > 0.01;
                let y_sp = if !self.shown {
                    SpringParams::stage_hide()
                } else if shape_changing {
                    SpringParams::shape_morph()
                } else {
                    SpringParams::stage_show()
                };
                let dim_sp = if !self.shown { SpringParams::stage_hide() } else { p };
                self.sp_w.set_target(target_w, dim_sp);
                self.sp_h.set_target(target_h, dim_sp);
                self.sp_r.set_target(target_r, if shape_changing { SpringParams::shape_morph() } else { dim_sp });
                self.sp_ear.set_target(target_ear, SpringParams::shape_morph());
                self.sp_island.set_target(target_isl, SpringParams::shape_morph());
                self.sp_y.set_target(target_y, y_sp);
                self.sp_vis.set_target(target_vis, if self.shown { SpringParams::vis_show() } else { SpringParams::stage_hide() });
                self.sp_sh_blur.set_target(target_sh_blur, dim_sp);
                self.sp_sh_dy.set_target(target_sh_dy, dim_sp);
                self.sp_sh_op.set_target(target_sh_op, dim_sp);
                self.sp_csh_blur.set_target(target_csh_blur, dim_sp);
                self.sp_csh_dy.set_target(target_csh_dy, dim_sp);
                self.sp_csh_op.set_target(target_csh_op, dim_sp);
                self.sp_timer_pill.set_target(target_timer_pill, dim_sp);
                self.start_frame();
            }
        }
    }

    fn render(&mut self) {
        if let Some(ti) = &self.todo_input {
            self.todo_draft = ti.buf.as_str().to_string();
        }
        if let Some(gi) = &self.gemini_input {
            self.gemini_draft = gi.buf.as_str().to_string();
        }
        self.todo_input = None;
        self.gemini_input = None;
        let c = self.shell.content.clone();
        if matches!(self.mode, Mode::Open(_)) {
            self.content_hidden_for_retract = false;
            let _ = c.SetOpacity(1.0);
        }
        if let Ok(k) = c.Children() {
            let _ = k.RemoveAll();
        }
        self.regions.clear();
        let f = Frame { x0: self.x0(), w: self.size.w, h: self.size.h };
        let now_ms = self.now_ms();
        let items = self.glance_items();
        let item = items[self.glance_idx.min(items.len() - 1)];
        let glance = (self.look.view == View::Compact(state::Activity::Glance)).then(|| (item, self.glance_text(item)));
        self.live_vis = views::Live::default();
        if self.mode == Mode::Open(Tab::Shelf) {
            self.load_shelf_icons();
        }
        if self.mode == Mode::Open(Tab::Todo) {
            self.ensure_events();
        }
        if self.has_media && self.art.is_none() {
            self.ensure_app_icon();
        }
        let cal_date = self.cal_date();
        let events = match &self.cal_events {
            Some((d, Some(list))) if *d == cal_date => views::Events::List(list),
            Some((d, None)) if *d == cal_date => views::Events::Unavailable,
            _ => views::Events::Loading,
        };
        let send_c = self.send_compact();
        let cal = views::CalView { today: Self::today(), sel: cal_date, events };
        let send = match &self.send {
            Some(ss) => {
                let n = ss.names.len();
                let title = if n == 1 { ss.names[0].clone() } else { format!("{} {}", crate::prefs::tr("Kirim"), views::count_text(n)) };
                let frac = nacre_core::notch::send::fraction(ss.prog.sent, ss.prog.total);
                let done = ss.done_at.is_some() && !ss.prog.active;
                let status = if done {
                    format!("{} \u{2713}", crate::prefs::tr("Terkirim"))
                } else if ss.prog.active || ss.prog.sent > 0 {
                    format!("{} {}%", crate::prefs::tr("Mengirim"), (frac * 100.0).floor() as u32)
                } else {
                    crate::prefs::tr("Menunggu HP\u{2026}").to_string()
                };
                Some(views::SendView { qr: ss.qr.as_ref(), qr_px: ss.qr_px, title, status, frac, done, error: false })
            }
            None if self.send_err => Some(views::SendView { qr: None, qr_px: 0.0, title: String::new(), status: String::new(), frac: 0.0, done: false, error: true }),
            None => None,
        };
        let app_icon = if self.art.is_none() { self.app_icons.get(&self.media_app).and_then(|s| s.as_ref()) } else { None };
        let pos_ms = self.position_ms();
        let media = self.has_media.then(|| MediaView {
            title: &self.title,
            artist: &self.artist,
            playing: self.playing,
            pos_ms,
            dur_ms: self.dur_ms,
            can_seek: self.can_seek,
            art: self.art.as_ref().or(app_icon),
            tint: self.tint,
            drag: self.seek_drag.map(|(_, _, fr)| fr),
        });
        let lyrics = LyricsView { lines: self.lyr.as_ref(), state: self.lyr_state, pos_ms: self.lyric_pos_ms() };
        let shelf = self.shelf.items().iter().map(|i| (i.name(), self.shelf_icons.get(&i.path))).collect();
        let drop_count = if self.mode == Mode::Drop { self.dnd.borrow().len() } else { 0 };
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let en = crate::prefs::get().lang == nacre_core::i18n::Lang::En;
        let app_icons = &self.app_icons;
        let notif_toast = self.toast_id.and_then(|id| self.notifs.get(id)).map(|n| notif_row(n, app_icons, now_unix, en));
        let notif_list: Vec<views::NotifRow> = self.notifs.items().iter().map(|n| notif_row(n, app_icons, now_unix, en)).collect();
        let todos = self.todos.visible();
        let todo_total = todos.len();
        let todo_done = todos.iter().filter(|t| t.completed).count();
        let s_prefs = crate::prefs::get();
        let gemini_model_label = nacre_core::notch::gemini::display_model_name(
            s_prefs.gemini_model,
            s_prefs.gemini_custom_model.as_str(),
        );
        let gemini_has_key = nacre_platform::gemini::has_api_key();
        let (prayer_city, prayer_snap) = self.prayer_snapshot();
        let detached_timer = if self.want_detached_timer() || self.sp_timer_pill.pos > 0.02 {
            let ear = self.sp_ear.pos.max(0.0);
            let h = self.size.h.max(28.0 * self.dpi);
            let th = h.min(36.0 * self.dpi);
            let t_ear = if ear > 0.5 { (ear * 0.65).min(th * 0.42) } else { 0.0 };
            let ew_main = if ear > 0.5 { ear.min(h * 0.52) } else { 0.0 };
            let gap_walls = if ear > 0.5 {
                ew_main + t_ear + 3.0 * self.dpi
            } else {
                DETACHED_TIMER_GAP_DP * self.dpi
            };
            let tx = (self.x0() + self.size.w + gap_walls).round();
            let max_avail = (self.win.0 as f32 - t_ear - SHADOW_BLEED_DP * self.dpi - tx).max(0.0);
            let tw = (DETACHED_TIMER_W_DP * self.dpi).min(max_avail);
            (tw >= 36.0 * self.dpi).then_some((tx, tw, th, self.sp_timer_pill.pos.clamp(0.0, 1.0)))
        } else {
            None
        };
        let d = Data {
            battery: self.battery,
            timer: &self.timer,
            now_ms,
            glance,
            media,
            lyrics,
            lyrics_open: self.lyrics_open,
            lyrics_progress: self.sp_lyrics.pos,
            play_gliding: self.play_glide_active,
            muted: self.muted,
            hovered: self.tracking,
            nav_expanded: self.nav_expanded,
            nav_progress: self.sp_nav.pos,
            prev_tab: self.prev_tab.take(),
            scrub_hover: self.sp_scrub_hover.pos,
            art_scale: self.sp_art_scale.pos,
            shelf,
            drop_zone: self.drop_zone,
            drop_count,
            privacy: self.privacy,
            download: (self.look.view == View::Compact(state::Activity::Download)).then(|| Self::download_view(&self.dl, &self.dl_done, &self.dl_icons, send_c)).flatten(),
            cal,
            send,
            send_zone: crate::prefs::get().notch_send,
            pinned: self.pinned,
            notif_toast,
            notif_list,
            notif_unread: self.notifs.unread_count(),
            notif_scroll: self.alerts_scroll,
            notif_status: self.notif_status,
            timer_running: self.timer.phase == Phase::Running,
            detached_timer,
            todos,
            todo_done,
            todo_total,
            todo_scroll: self.todo_scroll,
            todo_adding: self.todo_adding,
            todo_editing: self.todo_editing,
            todo_cal_mode: self.todo_cal_mode,
            todo_cal_adding: self.todo_cal_adding,
            todo_cal_scroll: self.todo_cal_scroll,
            todo_cal_err: &self.todo_cal_err,
            todo_status: self.todo_status,
            gemini_text: &self.gemini_resp,
            gemini_streaming: self.gemini_streaming,
            gemini_error: &self.gemini_err,
            gemini_has_key,
            gemini_scroll: self.gemini_scroll,
            gemini_auto_follow: self.gemini_auto_follow,
            gemini_model_label: &gemini_model_label,
            gemini_copied: self.gemini_copied,
            prayer: views::PrayerView { city: &prayer_city, snap: prayer_snap },
            prayer_alert: self.prayer_alert,
            sb_hover: self.sp_sb_hover.pos,
            sb_active: self.sp_sb_active.pos,
            sb_dragging: self.scroll_drag.is_some(),
        };
        views::draw(&mut self.paint, &c, self.look, f, &d, &mut self.regions, &mut self.live_vis);
        if let Some(sv) = &self.live_vis.scroll {
            let clamped = sv.scroll_px.clamp(0.0, sv.max_scroll);
            self.set_active_scroll_px(clamped);
            self.sp_scroll.snap(clamped);
        } else {
            self.sp_scroll.snap(0.0);
            self.scroll_drag = None;
            self.sb_hovered = false;
            self.sp_sb_hover.snap(0.0);
        }
        if let Some(nav) = self.live_vis.nav.as_mut() {
            nav.set_progress_and_height(self.sp_nav.pos, self.sp_h.pos);
        }
        if self.play_glide_active && self.live_vis.play_layout.is_some() {
            self.sync_play_glide();
        }
        if self.art_pop_pending {
            if let Some(art) = &self.live_vis.album_art {
                self.art_pop_pending = false;
                let tgt = self.sp_art_scale.target.clamp(0.85, 1.0);
                let _ = crate::motion::scalar(&self.paint.comp, art, "Scale.X", 0.86, tgt, 240);
                let _ = crate::motion::scalar(&self.paint.comp, art, "Scale.Y", 0.86, tgt, 240);
                let _ = crate::motion::scalar(&self.paint.comp, art, "Opacity", 0.45, 1.0, 220);
            }
        }
        if let Some((ix, iy, iw, ih)) = self.live_vis.todo_input_box {
            let ph = if self.todo_cal_mode && self.todo_cal_adding {
                crate::prefs::tr("14:00 Rapat Tim…")
            } else {
                crate::prefs::tr("Tambah tugas…")
            };
            if let Ok(mut ti) = TextInput::new(&self.paint.comp, &c, ix, iy, iw, ih, self.dpi, ph, 180) {
                ti.buf.set(&self.todo_draft);
                ti.set_focused(self.active_input == Some(ActiveInput::Todo), &mut self.paint);
                if self.todo_input_fade {
                    self.todo_input_fade = false;
                    ti.fade_in(&self.paint.comp);
                }
                self.todo_input = Some(ti);
            }
        }
        if let Some((ix, iy, iw, ih)) = self.live_vis.gemini_input_box {
            if let Ok(mut gi) = TextInput::new(&self.paint.comp, &c, ix, iy, iw, ih, self.dpi, crate::prefs::tr("Tanya Gemini…"), 512) {
                gi.buf.set(&self.gemini_draft);
                gi.set_focused(self.active_input == Some(ActiveInput::Gemini), &mut self.paint);
                self.gemini_input = Some(gi);
            }
        }
        if let Some(r) = self.hover_act.and_then(|a| self.find_hover_region(a)) {
            if let Some(v) = &r.vis {
                let k = hover_scale(r.action);
                let _ = v.SetCenterPoint(Vector3 { X: r.x + r.w / 2.0, Y: r.y + r.h / 2.0, Z: 0.0 });
                let _ = v.SetScale(Vector3 { X: k, Y: k, Z: 1.0 });
            }
        }
        self.sync_visualizer();
        self.sync_progress();
        if self.shown && !self.off && (self.live_vis.marquee.is_some() || (!self.live_vis.bars.is_empty() && self.playing)) {
            self.start_frame();
        }
    }

    fn retire_content(&mut self, ms: u32) {
        self.retire_content_dir(ms, 0, false);
    }

    fn retire_content_dir(&mut self, ms: u32, dir: i32, closing: bool) {
        let go = || -> windows_core::Result<()> {
            let kids = self.shell.content.Children()?;
            let old: Vec<Visual> = kids.clone().into_iter().collect();
            let ghost = self.shell.ghost.Children()?;
            ghost.RemoveAll()?;
            let cx = self.x0() + self.size.w / 2.0;
            self.shell.ghost.SetCenterPoint(Vector3 { X: cx, Y: 0.0, Z: 0.0 })?;
            self.shell.ghost.SetScale(Vector3 { X: 1.0, Y: 1.0, Z: 1.0 })?;
            self.shell.ghost.SetOffset(Vector3 { X: 0.0, Y: 0.0, Z: 0.0 })?;
            for v in &old {
                kids.Remove(v)?;
                ghost.InsertAtTop(v)?;
            }
            let g: CompositionObject = self.shell.ghost.cast()?;
            let comp = &self.paint.comp;
            if dir != 0 {
                // Slow-mo glide outgoing tab: smooth crossfade + 18 dp lateral drift.
                let shift = -(dir as f32) * 18.0 * self.dpi;
                keyframes_glide(comp, &g, "Opacity", 1.0, 0.0, 240, 0)?;
                keyframes_glide(comp, &g, "Offset.X", 0.0, shift, 350, 0)?;
            } else {
                keyframes(comp, &g, "Opacity", 1.0, 0.0, ms, 0)?;
                if closing {
                    keyframes(comp, &g, "Scale.X", 1.0, 0.94, ms + 20, 0)?;
                    keyframes(comp, &g, "Scale.Y", 1.0, 0.94, ms + 20, 0)?;
                    keyframes(comp, &g, "Offset.Y", 0.0, -6.0 * self.dpi, ms + 20, 0)?;
                }
            }
            Ok(())
        };
        let _ = go();
        timer(self.hwnd, T_GHOST, if dir != 0 { 420 } else { 260 });
    }

    fn intro(&self, opening: bool, dir: i32) {
        let go = || -> windows_core::Result<()> {
            let c: CompositionObject = self.shell.content.cast()?;
            let comp = &self.paint.comp;
            if opening {
                let cx = self.x0() + self.size.w / 2.0;
                self.shell.content.SetCenterPoint(Vector3 { X: cx, Y: 0.0, Z: 0.0 })?;
                self.shell.content.SetOffset(Vector3 { X: 0.0, Y: 0.0, Z: 0.0 })?;
                let (d, ms) = (35, 195);
                keyframes(comp, &c, "Opacity", 0.0, 1.0, ms, d)?;
                keyframes(comp, &c, "Scale.X", 0.94, 1.0, ms + 45, d)?;
                keyframes(comp, &c, "Scale.Y", 0.94, 1.0, ms + 45, d)?;
                keyframes(comp, &c, "Offset.Y", -5.0 * self.dpi, 0.0, ms + 45, d)
            } else if dir != 0 {
                // Slow-mo glide incoming tab (~380 ms velvety deceleration, ±18 dp).
                self.shell.content.SetScale(Vector3 { X: 1.0, Y: 1.0, Z: 1.0 })?;
                self.shell.content.SetOffset(Vector3 { X: 0.0, Y: 0.0, Z: 0.0 })?;
                let shift = (dir as f32) * 18.0 * self.dpi;
                keyframes_glide(comp, &c, "Opacity", 0.0, 1.0, 320, 30)?;
                keyframes_glide(comp, &c, "Offset.X", shift, 0.0, 380, 0)
            } else {
                self.shell.content.SetScale(Vector3 { X: 1.0, Y: 1.0, Z: 1.0 })?;
                self.shell.content.SetOffset(Vector3 { X: 0.0, Y: 0.0, Z: 0.0 })?;
                keyframes(comp, &c, "Opacity", 0.0, 1.0, 150, 40)
            }
        };
        let _ = go();
    }

    fn position_ms(&self) -> i64 {
        let extra = if self.playing { self.pos_at.elapsed().as_millis() as i64 } else { 0 };
        let p = self.pos_ms + extra;
        if self.dur_ms > 0 {
            p.clamp(0, self.dur_ms)
        } else {
            p.max(0)
        }
    }

    /// Playback position offset by the user's configured lyric lead time (`lyrics_lead_ms`)
    /// so synced lyrics appear slightly earlier to compensate for SMTC / Bluetooth audio lag.
    fn lyric_pos_ms(&self) -> i64 {
        let lead = crate::prefs::get().lyrics_lead_ms.min(2000) as i64;
        self.position_ms().saturating_add(lead)
    }

    fn on_media(&mut self) {
        let was_playing = self.playing;
        let mut changed = false;
        while let Ok(s) = self.media.snapshots.try_recv() {
            changed = true;
            self.has_media = s.has_media;
            self.title = s.title;
            self.artist = s.artist;
            self.album = s.album;
            self.media_app = s.app;
            self.playing = s.playing;
            self.pos_ms = s.position_ms;
            self.dur_ms = s.duration_ms;
            self.pos_at = Instant::now();
            self.can_seek = s.can_seek;
            match s.art {
                ArtUpdate::Unchanged => {}
                ArtUpdate::None => {
                    self.art = None;
                    self.tint = None;
                }
                ArtUpdate::New(b) => {
                    self.tint = nacre_core::notch::tint::from_art(&b.bgra, b.w, b.h);
                    self.art = self.paint.gfx.surface_from_bgra(b.w, b.h, &b.bgra).ok();
                }
            }
        }
        if changed {
            let track = if self.has_media { format!("{}\u{1}{}", self.title, self.artist) } else { String::new() };
            if track != self.lyr_track {
                self.lyr_track = track;
                self.marquee_t = 0.0;
                self.art_pop_pending = self.has_media;
                self.request_lyrics();
            }
            if self.playing != was_playing {
                self.sp_art_scale.set_target(if self.playing { 1.0 } else { 0.90 }, SpringParams::open());
                self.start_frame();
            }
            self.refresh();
            self.schedule_lyric();
            self.update_visibility();
        }
    }

    fn request_lyrics(&mut self) {
        self.lyr = None;
        self.lyr_idx = None;
        if !self.has_media {
            self.lyr_state = LyrState::Idle;
            self.lyr_key.clear();
            return;
        }
        let (title, artist) = nacre_core::notch::lrclib::clean(&self.title, &self.artist);
        let track = nacre_core::notch::lrclib::Track {
            title,
            artist,
            album: self.album.clone(),
            duration_s: (self.dur_ms > 0).then(|| ((self.dur_ms + 500) / 1000) as u32),
        };
        self.lyr_key = nacre_core::notch::lrclib::cache_key(&track);
        self.lyr_state = LyrState::Loading;
        let online = crate::prefs::get().lyrics_online;
        let _ = self.lyr_worker.requests.send(nacre_platform::lyrics::Request { track, online });
    }

    fn on_lyrics(&mut self) {
        let mut got = false;
        while let Ok(a) = self.lyr_worker.answers.try_recv() {
            if a.key != self.lyr_key {
                continue;
            }
            got = true;
            let parsed = a.text.map(|t| nacre_core::notch::lyrics::parse(&t)).filter(|l| !l.is_empty());
            self.lyr_state = if parsed.is_some() { LyrState::Ready } else { LyrState::Missing };
            log::line("lirik", &format!("{:?} sumber={:?} baris={}", self.lyr_state, a.source, parsed.as_ref().map_or(0, |l| l.lines.len())));
            self.lyr = parsed;
            self.lyr_idx = None;
        }
        if got {
            self.refresh();
            self.schedule_lyric();
            self.update_visibility();
        }
    }

    fn schedule_lyric(&self) {
        kill(self.hwnd, T_LYRIC);
        let Some(l) = &self.lyr else { return };
        if !l.synced || !self.playing {
            return;
        }
        let pos = self.lyric_pos_ms();
        if let Some(next) = l.next_change_ms(pos) {
            timer(self.hwnd, T_LYRIC, ((next - pos).max(10) + 15).min(60_000) as u32);
        }
    }

    fn on_lyric_tick(&mut self) {
        let pos = self.lyric_pos_ms();
        let idx = self.lyr.as_ref().and_then(|l| l.index_at(pos));
        if idx != self.lyr_idx {
            let dir = match (self.lyr_idx, idx) {
                (Some(a), Some(b)) if b == a + 1 => 1,
                (None, Some(0)) => 1,
                (Some(a), Some(b)) if b < a => -1,
                (Some(a), Some(b)) if b > a => 1,
                _ => 0,
            };
            self.lyr_idx = idx;
            if self.look.view == View::Compact(state::Activity::Lyrics) {
                self.marquee_t = 0.0;
                self.sync_adaptive_size();
                let line = self
                    .lyr
                    .as_ref()
                    .and_then(|l| l.current(pos))
                    .unwrap_or("\u{266A}")
                    .to_string();
                if !views::swap_compact_lyric(&mut self.paint, &mut self.live_vis, &line, self.size.w) {
                    self.render();
                } else if self.live_vis.marquee.is_some() {
                    self.start_frame();
                }
            } else if self.mode == Mode::Open(Tab::Play) || self.look.view == View::Compact(state::Activity::Player) {
                if self.live_vis.lyrics_host.is_some() {
                    let ly = LyricsView { lines: self.lyr.as_ref(), state: self.lyr_state, pos_ms: pos };
                    views::refresh_lyrics_in_place(&mut self.paint, &mut self.live_vis, self.has_media, &ly, dir);
                } else {
                    self.render();
                }
            }
        }
        self.schedule_lyric();
    }

    fn sync_visualizer(&mut self) {
        let want = self.has_media && self.playing && self.shown && !self.overlap.fullscreen && !self.off;
        if want && self.loopback.is_none() {
            let hw = self.hwnd.0 as isize;
            self.loopback = Some(loopback::start(move || post(hw, WM_BANDS)));
        } else if !want && self.loopback.is_some() {
            self.loopback = None;
        }
    }

    fn on_bands(&mut self) {
        let b = loopback::latest();
        for i in 0..5 {
            self.bar_tgt[i] = b.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        }
        if self.shown && !self.off && !self.live_vis.bars.is_empty() {
            self.start_frame();
        }
    }

    fn sync_progress(&self) {
        if self.mode == Mode::Open(Tab::Play) && self.has_media && self.playing && self.seek_drag.is_none() {
            timer(self.hwnd, T_PROGRESS, 1000);
        } else {
            kill(self.hwnd, T_PROGRESS);
        }
    }

    fn update_scrub_in_place(&mut self) {
        if self.live_vis.scrub.is_none() {
            self.render();
            return;
        }
        let (frac, dragging, pos_ms) = match self.seek_drag {
            Some((_, _, fr)) => (fr, true, (self.dur_ms as f32 * fr) as i64),
            None => {
                let pos = self.position_ms();
                let fr = if self.dur_ms > 0 { pos as f32 / self.dur_ms as f32 } else { 0.0 };
                (fr, false, pos)
            }
        };
        let hover_t = self.sp_scrub_hover.pos;
        if let Some(scrub) = self.live_vis.scrub.as_mut() {
            scrub.update(&mut self.paint, frac, hover_t, dragging, pos_ms);
        }
    }

    fn release_press(&mut self) {
        if let Some(a) = self.pressed_act.take() {
            if let Some(r) = self.find_hover_region(a) {
                if let Some(v) = &r.vis {
                    if let Ok(comp) = v.Compositor() {
                        let to = if self.hover_act == Some(a) { hover_scale(a) } else { 1.0 };
                        let _ = crate::motion::hover_pop(&comp, v, Vector3 { X: r.x + r.w / 2.0, Y: r.y + r.h / 2.0, Z: 0.0 }, to);
                    }
                }
            }
        }
    }

    fn on_down(&mut self, x: f32, y: f32) -> bool {
        if !matches!(self.mode, Mode::Open(_)) {
            return false;
        }
        // Interactive browser-style scrollbar rail: press & hold thumb or track to scrub smoothly.
        if let Some((grab, jump_tgt)) = self.live_vis.scroll.as_ref().and_then(|sv| {
            let bar = sv.bar.as_ref()?;
            if !bar.hit_rail(x, y) || sv.max_scroll <= 0.5 {
                return None;
            }
            if bar.hit_thumb(y) {
                Some((y - bar.thumb_y, None))
            } else {
                let g = bar.thumb_h / 2.0;
                let tgt = bar.scroll_for_thumb_top(y - g, sv.max_scroll);
                Some((g, Some((tgt, sv.max_scroll))))
            }
        }) {
            if let Some((tgt, max_scroll)) = jump_tgt {
                if self.mode == Mode::Open(Tab::Gemini) {
                    self.gemini_auto_follow = tgt >= max_scroll - 2.0;
                }
                self.sp_scroll.set_target(tgt, SpringParams::smooth_scroll());
                self.set_active_scroll_px(tgt);
            }
            self.scroll_drag = Some(grab);
            self.sb_hovered = true;
            self.sp_sb_hover.set_target(1.0, SpringParams::tactile());
            self.sp_sb_active.set_target(1.0, SpringParams::tactile());
            kill(self.hwnd, T_SCROLL_IDLE);
            self.sync_scroll_visual();
            self.start_frame();
            return true;
        }
        let nav_hit = if self.nav_expanded || self.sp_nav.pos > 0.35 {
            self.live_vis.nav.as_ref().and_then(|n| n.hit_region(x, y))
        } else {
            None
        };
        let hit = nav_hit
            .or_else(|| self.live_vis.scroll.as_ref().and_then(|s| s.hit_region(x, y)))
            .or_else(|| self.regions.iter().find(|r| r.hit(x, y)).map(|r| r.action));
        match hit {
            Some(Action::Seek(sx, sw)) => {
                self.seek_drag = Some((sx, sw, ((x - sx) / sw).clamp(0.0, 1.0)));
                self.update_scrub_in_place();
                true
            }
            Some(Action::ShelfItem(i)) => {
                self.shelf_press = Some((i, x, y));
                self.press_visual(Action::ShelfItem(i));
                false
            }
            Some(a) => {
                self.press_visual(a);
                false
            }
            None => false,
        }
    }

    fn press_visual(&mut self, a: Action) {
        self.release_press();
        if let Some(r) = self.find_hover_region(a) {
            if let Some(v) = &r.vis {
                let _ = v.SetCenterPoint(Vector3 { X: r.x + r.w / 2.0, Y: r.y + r.h / 2.0, Z: 0.0 });
                let _ = crate::motion::vec3(
                    &self.paint.comp,
                    v,
                    "Scale",
                    v.Scale().unwrap_or(Vector3 { X: 1.0, Y: 1.0, Z: 1.0 }),
                    Vector3 { X: 0.90, Y: 0.90, Z: 1.0 },
                    85,
                );
                self.pressed_act = Some(a);
            }
        }
    }

    fn on_drag(&mut self, x: f32, y: f32) {
        if self.seek_drag.is_some() || self.shelf_press.is_some() || self.scroll_drag.is_some() {
            let down = unsafe { (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 };
            if !down {
                self.on_capture_changed();
                return;
            }
        }
        if let Some(grab) = self.scroll_drag {
            if let Some((next, max_scroll)) = self.live_vis.scroll.as_ref().and_then(|sv| {
                sv.bar
                    .as_ref()
                    .map(|b| (b.scroll_for_thumb_top(y - grab, sv.max_scroll), sv.max_scroll))
            }) {
                if self.mode == Mode::Open(Tab::Gemini) {
                    self.gemini_auto_follow = next >= max_scroll - 2.0;
                }
                self.sp_scroll.snap(next);
                self.set_active_scroll_px(next);
                self.sync_scroll_visual();
            }
            return;
        }
        if let Some((sx, sw, _)) = self.seek_drag {
            self.seek_drag = Some((sx, sw, ((x - sx) / sw).clamp(0.0, 1.0)));
            self.update_scrub_in_place();
            return;
        }
        if let Some((i, x0, y0)) = self.shelf_press {
            let dist = ((x - x0).powi(2) + (y - y0).powi(2)).sqrt();
            if dist > 6.0 * self.dpi {
                self.shelf_press = None;
                self.dragging_out = true;
                post_w(self.hwnd.0 as isize, WM_START_DRAGOUT, i);
            }
        }
    }

    fn on_capture_changed(&mut self) {
        self.shelf_press = None;
        self.release_press();
        if self.scroll_drag.take().is_some() {
            self.sync_scroll_visual();
            timer(self.hwnd, T_SCROLL_IDLE, 650);
            self.start_frame();
        }
        if self.seek_drag.take().is_some() {
            self.update_scrub_in_place();
            self.sync_progress();
        }
    }

    fn on_up(&mut self) -> bool {
        self.shelf_press = None;
        self.release_press();
        if self.scroll_drag.take().is_some() {
            self.sync_scroll_visual();
            timer(self.hwnd, T_SCROLL_IDLE, 650);
            self.start_frame();
            return true;
        }
        let Some((_, _, fr)) = self.seek_drag.take() else { return false };
        if self.dur_ms > 0 {
            let ms = (self.dur_ms as f32 * fr) as i64;
            let _ = self.media.commands.send(Command::SeekMs(ms));
            self.pos_ms = ms;
            self.pos_at = Instant::now();
        }
        self.update_scrub_in_place();
        self.sync_progress();
        self.schedule_lyric();
        true
    }

    fn load_shelf_icons(&mut self) {
        let px = (40.0 * self.dpi).round() as i32;
        let paths: Vec<String> = self
            .shelf
            .items()
            .iter()
            .take(8)
            .filter(|it| !self.shelf_icons.contains_key(&it.path))
            .map(|it| it.path.clone())
            .collect();
        for path in paths {
            self.request_icon(IconKind::Shelf, path, px);
        }
    }

    fn save_shelf(&self) {
        save_persisted_shelf(&self.shelf);
    }

    fn zone_at(&self, x: f32, y: f32) -> Option<usize> {
        self.regions.iter().find(|r| matches!(r.action, Action::Zone(_)) && r.hit(x, y)).and_then(|r| match r.action {
            Action::Zone(i) => Some(i),
            _ => None,
        })
    }

    fn on_dnd(&mut self, kind: usize, lp: LPARAM) {
        let (sx, sy) = ((lp.0 as u32 & 0xFFFF) as u16 as i16 as f32, ((lp.0 as u32 >> 16) & 0xFFFF) as u16 as i16 as f32);
        let (cx, cy) = (sx - ((self.screen_w - self.win.0) / 2) as f32, sy - self.sp_y.pos);
        match kind {
            drop::ENTER => {
                self.drop_zone = None;
                self.dispatch(Event::DragEnter);
                self.drop_zone = self.zone_at(cx, cy);
                self.render();
            }
            drop::OVER => {
                let z = self.zone_at(cx, cy);
                if z != self.drop_zone {
                    self.drop_zone = z;
                    if !self.live_vis.drop_zones.is_empty() {
                        let accent = crate::prefs::accent();
                        for dz in &self.live_vis.drop_zones {
                            dz.set_active(z == Some(dz.id), accent);
                        }
                    } else {
                        self.render();
                    }
                }
            }
            drop::LEAVE => {
                self.drop_zone = None;
                self.dispatch(Event::DragLeave);
            }
            drop::DROP => {
                let zone = self.zone_at(cx, cy).unwrap_or(0);
                let paths = self.dnd.borrow().clone();
                self.drop_zone = None;
                log::line("notch", &format!("ditaruh {} berkas di zona {zone}", paths.len()));
                if zone == 0 {
                    for p in &paths {
                        self.shelf.add(p);
                    }
                    self.save_shelf();
                    self.dispatch(Event::Dropped);
                } else if zone == 1 {
                    self.start_send(paths);
                } else {
                    self.pending = paths;
                    self.dispatch(Event::DragLeave);
                    post_w(self.hwnd.0 as isize, WM_AFTERDROP, zone);
                }
            }
            _ => {}
        }
    }

    fn shelf_menu(&mut self, i: usize, x: f32, y: f32) {
        if self.menu.is_none() {
            match Menu::new(&self.paint.comp, self.hwnd, self.dpi, sys::screen_size(), Tier::Full) {
                Ok(m) => self.menu = Some(m),
                Err(_) => return,
            }
        }
        let tr = crate::prefs::tr;
        let items = vec![
            MItem::action(1, tr("Buka"), true),
            MItem::action(2, tr("Tampilkan di folder"), true),
            MItem::action(3, tr("Salin jalur"), true),
            MItem::action(5, tr("Kirim ke HP"), crate::prefs::get().notch_send),
            MItem::separator(),
            MItem::action(4, tr("Hapus dari Rak"), true),
        ];
        self.menu_item = Some(i);
        let wx = (self.screen_w - self.win.0) / 2;
        if let Some(m) = self.menu.as_mut() {
            let _ = m.show(&self.paint.comp, &self.paint.gfx, items, wx + x as i32, (y + self.sp_y.pos) as i32);
        }
    }

    fn menu_pick(&mut self, id: usize) {
        if let Some(m) = &self.menu {
            m.hide();
        }
        let Some(i) = self.menu_item.take() else { return };
        let Some(path) = self.shelf.items().get(i).map(|it| it.path.clone()) else { return };
        match id {
            1 => nacre_platform::launch::open(&path),
            2 => nacre_platform::launch::run("explorer.exe", &format!("/select,\"{path}\"")),
            3 => {
                let _ = nacre_platform::clipboard::copy_text(&path);
            }
            4 => {
                self.shelf.remove(&path);
                self.shelf_icons.remove(&path);
                self.save_shelf();
                self.refresh();
            }
            5 => self.start_send(vec![path]),
            _ => {}
        }
    }

    fn on_menu_close(&mut self) {
        if let Some(mn) = &self.menu {
            mn.hide();
        }
        if !self.tracking && matches!(self.mode, Mode::Open(_)) {
            timer(self.hwnd, T_LEAVE, motion::LEAVE_GRACE_MS);
        }
    }

    fn on_right_click(&mut self, x: f32, y: f32) {
        if self.mode != Mode::Open(Tab::Shelf) {
            return;
        }
        if let Some(Action::ShelfItem(i)) = self.regions.iter().find(|r| r.hit(x, y)).map(|r| r.action) {
            self.shelf_menu(i, x, y);
        }
    }

    fn on_privacy(&mut self) {
        let Some(mon) = &self.privacy_mon else { return };
        let mut latest = None;
        while let Ok(v) = mon.snapshots.try_recv() {
            latest = Some(v);
        }
        if let Some(v) = latest {
            if v != self.privacy {
                self.privacy = v;
                if self.shown && !self.off {
                    self.render();
                }
            }
        }
    }

    fn sync_hover_chrome(&self) {
        if let Some(pin) = &self.live_vis.pin_vis {
            let to = if self.pinned { 1.0 } else if self.tracking { 0.54 } else { 0.0 };
            let from = pin.Opacity().unwrap_or(to);
            if (from - to).abs() > 0.01 {
                if let Ok(o) = pin.cast::<CompositionObject>() {
                    let _ = keyframes(&self.paint.comp, &o, "Opacity", from, to, 140, 0);
                }
            }
        }
    }

    fn set_nav_expanded(&mut self, on: bool) {
        if on == self.nav_expanded {
            return;
        }
        self.nav_expanded = on;
        let sp = if on { SpringParams::swap() } else { SpringParams::close() };
        self.sp_nav.set_target(if on { 1.0 } else { 0.0 }, sp);
        self.apply_size(Some(sp));
        self.start_frame();
    }

    fn update_nav_hover(&mut self, x: f32, y: f32) {
        let Some(nav) = &self.live_vis.nav else { return };
        if !self.nav_expanded {
            if nav.hit_dots(x, y) {
                kill(self.hwnd, T_NAV_COLLAPSE);
                self.nav_collapse_armed = false;
                self.set_nav_expanded(true);
            }
        } else if nav.hit_pill(x, y) {
            if self.nav_collapse_armed {
                kill(self.hwnd, T_NAV_COLLAPSE);
                self.nav_collapse_armed = false;
            }
        } else if !self.nav_collapse_armed {
            self.nav_collapse_armed = true;
            timer(self.hwnd, T_NAV_COLLAPSE, NAV_COLLAPSE_MS);
        }
    }

    fn on_move(&mut self) {
        let was_tracking = self.tracking;
        if !self.tracking {
            let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: self.hwnd, dwHoverTime: 0 };
            if unsafe { TrackMouseEvent(&mut t) }.is_ok() {
                self.tracking = true;
            }
        }
        if !was_tracking && self.tracking {
            self.sync_hover_chrome();
        }
        kill(self.hwnd, T_LEAVE);
        if self.grace {
            self.grace = false;
            kill(self.hwnd, T_GRACE);
        }
        self.dispatch(Event::Enter);
        if self.mode == Mode::Peek && crate::prefs::get().notch_hover_open {
            if self.cursor_on_media_button() {
                kill(self.hwnd, T_DWELL);
            } else {
                timer(self.hwnd, T_DWELL, motion::DWELL_MS);
            }
        }
    }

    fn find_hover_region(&self, act: Action) -> Option<&Region> {
        if self.nav_expanded {
            if let Some(r) = self.live_vis.nav.as_ref().and_then(|n| n.find_region(act)) {
                return Some(r);
            }
        }
        if let Some(r) = self.live_vis.scroll.as_ref().and_then(|s| s.find_region(act)) {
            return Some(r);
        }
        self.regions.iter().find(|r| r.action == act)
    }

    fn on_hover(&mut self, x: f32, y: f32) {
        self.update_nav_hover(x, y);

        // Animated scrollbar rail hover bloom (smoothly widens from 3 dp -> 5.5 dp on hover/hold).
        let on_sb = self.scroll_drag.is_some()
            || self
                .live_vis
                .scroll
                .as_ref()
                .and_then(|s| s.bar.as_ref())
                .is_some_and(|b| b.hit_rail(x, y));
        if on_sb != self.sb_hovered {
            self.sb_hovered = on_sb;
            self.sp_sb_hover.set_target(if on_sb { 1.0 } else { 0.0 }, SpringParams::tactile());
            self.start_frame();
        }

        // Scrub bar hover bloom (independent of Region::vis since Seek has no scale vis).
        let on_scrub = self.regions.iter().any(|r| matches!(r.action, Action::Seek(..)) && r.hit(x, y));
        if on_scrub != self.scrub_hovered {
            self.scrub_hovered = on_scrub;
            self.sp_scrub_hover.set_target(if on_scrub { 1.0 } else { 0.0 }, SpringParams::tactile());
            self.start_frame();
        }

        let nav_hit = if self.nav_expanded {
            self.live_vis.nav.as_ref().and_then(|n| n.hit_region(x, y))
        } else {
            None
        };
        let hit = nav_hit
            .or_else(|| self.live_vis.scroll.as_ref().and_then(|s| s.hit_hover_region(x, y)))
            .or_else(|| self.regions.iter().find(|r| r.vis.is_some() && r.hit(x, y)).map(|r| r.action));
        if hit == self.hover_act {
            return;
        }
        let prev = self.hover_act;
        self.hover_act = hit;

        for (a, on) in [(prev, false), (hit, true)] {
            let Some(r) = a.and_then(|a| self.find_hover_region(a)) else { continue };
            let Some(v) = &r.vis else { continue };
            let to = if on { hover_scale(r.action) } else { 1.0 };
            let Ok(comp) = v.Compositor() else { continue };
            let _ = crate::motion::hover_pop(&comp, v, Vector3 { X: r.x + r.w / 2.0, Y: r.y + r.h / 2.0, Z: 0.0 }, to);
            if let Some(bg) = &r.hover_bg {
                if let Ok(o) = bg.cast::<CompositionObject>() {
                    let from = bg.Opacity().unwrap_or(if on { 0.0 } else { 1.0 });
                    let _ = keyframes(&self.paint.comp, &o, "Opacity", from, if on { 1.0 } else { 0.0 }, 120, 0);
                }
            }
        }
        if let Some(pin) = &self.live_vis.pin_vis {
            let on_pin = hit == Some(Action::Pin);
            let to = if self.pinned || on_pin { 1.0 } else if self.tracking { 0.54 } else { 0.0 };
            let from = pin.Opacity().unwrap_or(to);
            if (from - to).abs() > 0.01 {
                if let Ok(o) = pin.cast::<CompositionObject>() {
                    let _ = keyframes(&self.paint.comp, &o, "Opacity", from, to, 120, 0);
                }
            }
        }
    }

    fn on_leave(&mut self) {
        self.release_press();
        self.on_hover(-1.0, -1.0);
        self.tracking = false;
        self.sync_hover_chrome();
        kill(self.hwnd, T_DWELL);
        if self.nav_expanded || self.nav_collapse_armed {
            kill(self.hwnd, T_NAV_COLLAPSE);
            self.nav_collapse_armed = false;
            self.set_nav_expanded(false);
        }
        if self.seek_drag.is_some() || self.scroll_drag.is_some() || self.menu.as_ref().is_some_and(|m| m.is_open()) {
            return;
        }
        if matches!(self.mode, Mode::Open(_)) {
            timer(self.hwnd, T_LEAVE, motion::LEAVE_GRACE_MS);
        } else {
            self.dispatch(Event::Leave);
        }
        self.grace = true;
        timer(self.hwnd, T_GRACE, if self.vis == Vis::OnDemand { 380 } else { 800 });
        self.update_visibility();
    }

    fn on_compact_action(&self, x: f32, y: f32) -> bool {
        self.regions.iter().any(|r| r.hit(x, y) && matches!(r.action, Action::OpenTimerPill | Action::OpenPrayer))
            || (self.look.view == View::Compact(state::Activity::Player)
                && self.regions.iter().any(|r| r.hit(x, y) && matches!(r.action, Action::PlayPause | Action::Next | Action::Prev)))
            || (self.look.view == View::Compact(state::Activity::Notification)
                && self.regions.iter().any(|r| r.hit(x, y) && matches!(r.action, Action::NotifFocus(_) | Action::NotifDismiss(_))))
    }

    fn cursor_on_media_button(&self) -> bool {
        let mut pt = windows::Win32::Foundation::POINT::default();
        unsafe {
            if GetCursorPos(&mut pt).is_err() {
                return false;
            }
            let _ = windows::Win32::Graphics::Gdi::ScreenToClient(self.hwnd, &mut pt);
        }
        self.on_compact_action(pt.x as f32, pt.y as f32 - self.sp_y.pos)
    }

    fn on_click(&mut self, x: f32, y: f32) {
        if !matches!(self.mode, Mode::Open(_)) && !self.on_compact_action(x, y) {
            kill(self.hwnd, T_DWELL);
            self.dispatch(Event::Click);
            return;
        }
        let mut hit_act = None;
        if let Some(nav) = &self.live_vis.nav {
            if self.nav_expanded || self.sp_nav.pos > 0.35 {
                hit_act = nav.hit_region(x, y);
            } else if nav.hit_dots(x, y) {
                kill(self.hwnd, T_NAV_COLLAPSE);
                self.nav_collapse_armed = false;
                self.set_nav_expanded(true);
                return;
            }
        }
        let Some(a) = hit_act
            .or_else(|| self.live_vis.scroll.as_ref().and_then(|s| s.hit_region(x, y)))
            .or_else(|| self.regions.iter().find(|r| r.hit(x, y)).map(|r| r.action))
        else {
            return;
        };
        let now = self.now_ms();
        match a {
            Action::PlayPause => {
                let _ = self.media.commands.send(Command::PlayPause);
            }
            Action::Next => {
                let _ = self.media.commands.send(Command::Next);
            }
            Action::Prev => {
                let _ = self.media.commands.send(Command::Prev);
            }
            Action::ToggleLyrics => {
                self.lyrics_open = !self.lyrics_open;
                let open = self.lyrics_open;
                crate::prefs::update_silent(|s| s.lyrics_panel_open = open);
                let glide = SpringParams::lyrics_glide();
                if self.lyrics_open && (self.nav_expanded || self.sp_nav.pos > 0.01) {
                    self.nav_expanded = false;
                    self.nav_collapse_armed = false;
                    self.sp_nav.set_target(0.0, glide);
                    kill(self.hwnd, T_NAV_COLLAPSE);
                }
                let was_gliding = self.play_glide_active;
                self.play_glide_active = true;
                self.sp_lyrics.set_target(if self.lyrics_open { 1.0 } else { 0.0 }, glide);
                self.apply_size(Some(glide));
                self.publish_notch();
                if !was_gliding {
                    // Mount all transitional layers once at current `sp_w.pos` & `sp_lyrics.pos`
                    // (Frame 0 is pixel-identical to the pre-click frame -> zero jump; rapid re-clicks reverse in-flight).
                    self.render();
                }
                self.update_visibility();
                self.start_frame();
            }
            Action::ToggleMute => {
                crate::services::toggle_mute();
                self.muted = !self.muted;
                self.render();
            }
            Action::Seek(..) | Action::Zone(_) => {}
            Action::ShelfItem(i) => {
                if let Some(it) = self.shelf.items().get(i) {
                    nacre_platform::launch::open(&it.path);
                }
            }
            Action::ShelfClear => {
                self.shelf.clear();
                self.shelf_icons.clear();
                self.save_shelf();
                self.refresh();
            }
            Action::Tab(t) => {
                if t == Tab::Alerts {
                    self.notifs.mark_all_read();
                    self.alerts_scroll = 0.0;
                }
                self.dispatch(Event::SelectTab(t));
            }
            Action::Pin => {
                self.pinned = !self.pinned;
                self.render();
                self.update_visibility();
            }
            Action::Settings => crate::settings_win::open(&self.paint.comp),
            Action::TimerToggle => {
                if self.timer.phase == Phase::Running {
                    self.timer.pause(now);
                } else {
                    self.timer.start(now);
                }
                self.sync_tick();
                self.refresh();
            }
            Action::TimerReset => {
                self.timer.reset_pomodoro();
                self.sync_tick();
                self.refresh();
            }
            Action::TimerStage(stage) => {
                self.timer.select_stage(stage);
                self.sync_tick();
                self.refresh();
            }
            Action::OpenTimerPill => {
                kill(self.hwnd, T_DWELL);
                self.dispatch(Event::Click);
                self.dispatch(Event::SelectTab(Tab::Timer));
            }
            Action::OpenPrayer => {
                kill(self.hwnd, T_DWELL);
                kill(self.hwnd, T_PRAYER_ALERT);
                self.prayer_alert = None;
                self.prayer_peeking = false;
                self.dispatch(Event::Click);
                self.dispatch(Event::SelectTab(Tab::Prayer));
            }
            Action::SendDone => self.stop_send(),
            Action::CalDay(n) => {
                self.cal_sel = (self.cal_sel + n).clamp(-3650, 3650);
                self.todo_cal_scroll = 0.0;
                self.todo_cal_err.clear();
                self.cal_fetched_at = None;
                self.cal_in_flight = false;
                self.ensure_events();
                self.render();
            }
            Action::CalToday => {
                self.cal_sel = 0;
                self.todo_cal_scroll = 0.0;
                self.todo_cal_err.clear();
                self.cal_fetched_at = None;
                self.cal_in_flight = false;
                self.ensure_events();
                self.render();
            }
            Action::CalRefresh => {
                self.todo_cal_err.clear();
                self.cal_fetched_at = None;
                self.cal_in_flight = false;
                self.ensure_events();
                self.render();
            }
            Action::CalAddOpen => {
                self.todo_cal_mode = true;
                self.todo_cal_adding = true;
                self.todo_cal_err.clear();
                self.todo_draft.clear();
                self.todo_input_fade = true;
                self.render();
                self.focus_input(ActiveInput::Todo);
            }
            Action::NotifFocus(id) => {
                let target = self
                    .notifs
                    .get(id)
                    .map(|n| (n.aumid.clone(), n.app_name.clone(), n.title.clone(), n.body.clone()));
                self.notifs.mark_read(id);
                if self.toast_id == Some(id) {
                    self.toast_id = None;
                    self.notif_peeking = false;
                    kill(self.hwnd, T_NOTIF_TOAST);
                }
                if matches!(self.mode, Mode::Open(_)) && !self.pinned {
                    if !self.cursor_in_notch_zone() {
                        self.grace = false;
                        kill(self.hwnd, T_GRACE);
                    }
                    self.dispatch(Event::Leave);
                } else {
                    self.refresh();
                    self.update_visibility();
                }
                if let Some((aumid, app_name, title, body)) = target {
                    if !aumid.is_empty() || !app_name.is_empty() {
                        let ok = nacre_platform::notifications::activate_notification(
                            id,
                            &aumid,
                            &app_name,
                            &title,
                            &body,
                        );
                        log::line(
                            "notif",
                            &format!("fokus app={app_name} aumid={aumid} title={title} ok={ok}"),
                        );
                    }
                }
            }
            Action::NotifDismiss(id) => {
                self.notifs.dismiss(id);
                if self.toast_id == Some(id) {
                    self.toast_id = None;
                    self.notif_peeking = false;
                    kill(self.hwnd, T_NOTIF_TOAST);
                }
                self.refresh();
                self.update_visibility();
            }
            Action::NotifClearAll => {
                self.notifs.clear();
                self.toast_id = None;
                self.notif_peeking = false;
                self.alerts_scroll = 0.0;
                kill(self.hwnd, T_NOTIF_TOAST);
                self.refresh();
                self.update_visibility();
            }
            Action::TodoCalToggle => {
                self.todo_cal_mode = !self.todo_cal_mode;
                self.todo_adding = false;
                self.todo_editing = false;
                self.todo_cal_adding = false;
                self.todo_cal_err.clear();
                if self.todo_cal_mode {
                    self.cal_fetched_at = None;
                    self.cal_in_flight = false;
                    self.ensure_events();
                }
                self.render();
            }
            Action::TodoAddOpen => {
                self.todo_cal_mode = false;
                self.todo_cal_adding = false;
                self.todo_adding = true;
                self.todo_draft.clear();
                self.todo_input_fade = true;
                self.render();
                self.focus_input(ActiveInput::Todo);
            }
            Action::TodoEditToggle => {
                self.todo_editing = !self.todo_editing;
                self.render();
            }
            Action::TodoFocus => self.focus_input(ActiveInput::Todo),
            Action::TodoToggle(idx) => {
                let id_opt = self.todos.visible().get(idx).map(|t| (t.id.clone(), t.remote_id.clone()));
                if let Some((id, remote_id)) = id_opt {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    if let Some(updated) = self.todos.toggle(&id, now) {
                        self.save_todos();
                        let _ = self.gtasks.commands.send(nacre_platform::gtasks::Cmd::Toggle {
                            local_id: updated.id,
                            remote_id,
                            completed: updated.completed,
                        });
                        self.render();
                    }
                }
            }
            Action::TodoDelete(idx) => {
                let id_opt = self.todos.visible().get(idx).map(|t| (t.id.clone(), t.remote_id.clone()));
                if let Some((id, remote_id)) = id_opt {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    if let Some(deleted) = self.todos.delete(&id, now) {
                        self.save_todos();
                        let _ = self.gtasks.commands.send(nacre_platform::gtasks::Cmd::Delete {
                            local_id: deleted.id,
                            remote_id,
                        });
                        self.render();
                    }
                }
            }
            Action::TodoConnect => {
                let _ = self.gtasks.commands.send(nacre_platform::gtasks::Cmd::Connect);
            }
            Action::GeminiFocus => self.focus_input(ActiveInput::Gemini),
            Action::GeminiSend => self.submit_gemini(),
            Action::GeminiStop => {
                self.gemini.cancel_current();
                self.gemini_streaming = false;
                self.render();
            }
            Action::GeminiCopy => {
                if !self.gemini_resp.is_empty() && nacre_platform::clipboard::copy_text(&self.gemini_resp) {
                    self.gemini_copied = true;
                    timer(self.hwnd, T_COPIED, 1500);
                    self.render();
                }
            }
            Action::GeminiClear => {
                self.gemini.cancel_current();
                self.gemini_streaming = false;
                self.gemini_resp.clear();
                self.gemini_err.clear();
                self.gemini_scroll = 0.0;
                self.sp_scroll.snap(0.0);
                self.gemini_auto_follow = true;
                self.gemini_copied = false;
                self.render();
            }
            Action::GeminiQuick(kind) => {
                let typed = self
                    .gemini_input
                    .as_ref()
                    .map(|gi| gi.buf.as_str().to_string())
                    .unwrap_or_else(|| self.gemini_draft.clone());
                let src = if !typed.trim().is_empty() {
                    typed.trim().chars().take(4000).collect::<String>()
                } else {
                    nacre_platform::clipboard::paste_text()
                        .map(|s| s.trim().chars().take(4000).collect::<String>())
                        .unwrap_or_default()
                };
                if src.is_empty() {
                    self.gemini_err = crate::prefs::tr("Ketik teks di atas atau salin teks ke clipboard dulu.").to_string();
                    self.render();
                } else {
                    let en = crate::prefs::get().lang == nacre_core::i18n::Lang::En;
                    let prompt = match (kind, en) {
                        (0, true) => format!("Summarize the following text concisely in short bullet points using the same language as the input text:\n\n{src}"),
                        (0, false) => format!("Ringkas teks berikut secara padat dalam poin-poin singkat dengan bahasa yang sama seperti teks aslinya:\n\n{src}"),
                        (1, _) => format!("Detect the language of the following text. If it is in Indonesian, translate it naturally into English. If it is in English or another language, translate it naturally into Indonesian. Output ONLY the translation without extra commentary:\n\n{src}"),
                        (_, true) => format!("Fix grammar, spelling, clarity, and natural phrasing of the following text in its original language without changing its meaning. Output ONLY the improved text:\n\n{src}"),
                        (_, false) => format!("Perbaiki tata bahasa, ejaan, dan kejelasan teks berikut dalam bahasa aslinya (Indonesia tetap Indonesia, Inggris tetap Inggris) tanpa mengubah maknanya. Berikan hasil akhirnya saja:\n\n{src}"),
                    };
                    self.submit_gemini_prompt(prompt);
                }
            }
        }
    }

    fn set_active_scroll_px(&mut self, px: f32) {
        let px = px.max(0.0);
        match self.mode {
            Mode::Open(Tab::Alerts) => self.alerts_scroll = px,
            Mode::Open(Tab::Todo) => {
                if self.todo_cal_mode {
                    self.todo_cal_scroll = px;
                } else {
                    self.todo_scroll = px;
                }
            }
            Mode::Open(Tab::Gemini) => self.gemini_scroll = px,
            _ => {}
        }
    }

    fn sync_scroll_visual(&mut self) {
        let Some(sv) = self.live_vis.scroll.as_mut() else { return };
        let px = self.sp_scroll.pos.clamp(0.0, sv.max_scroll);
        sv.apply(px, self.sp_sb_hover.pos, self.scroll_drag.is_some(), self.sp_sb_active.pos);
    }

    fn on_wheel(&mut self, delta: i16) {
        if !matches!(self.mode, Mode::Open(_)) || delta == 0 {
            return;
        }
        let now = self.now_ms();
        // Axis-lock: suppress stray diagonal touchpad horizontal drift during vertical scroll.
        self.swipe.note_vertical(now);

        let Some(max_scroll) = self.live_vis.scroll.as_ref().map(|s| s.max_scroll) else {
            return;
        };
        if max_scroll <= 0.5 {
            return;
        }
        let px_per_notch = match self.mode {
            Mode::Open(Tab::Gemini) => 46.0 * self.dpi,
            Mode::Open(Tab::Alerts) => 54.0 * self.dpi,
            _ => 42.0 * self.dpi,
        };
        let dpx = state::wheel_delta_to_px(delta, px_per_notch);
        let base = self.sp_scroll.target.clamp(0.0, max_scroll);
        let next = (base + dpx).clamp(0.0, max_scroll);
        if self.mode == Mode::Open(Tab::Gemini) {
            self.gemini_auto_follow = next >= max_scroll - 2.0;
        }
        self.sp_scroll.set_target(next, SpringParams::smooth_scroll());
        self.set_active_scroll_px(next);
        self.sp_sb_active.set_target(1.0, SpringParams::tactile());
        timer(self.hwnd, T_SCROLL_IDLE, 650);
        self.start_frame();
    }

    fn step_tab(&mut self, dir: i32) {
        if let Mode::Open(cur) = self.mode {
            let tabs = crate::prefs::get().visible_notch_tabs();
            if tabs.is_empty() {
                return;
            }
            let next = if let Some(i) = tabs.iter().position(|t| *t == cur) {
                if tabs.len() <= 1 {
                    return;
                }
                let n = tabs.len() as i32;
                tabs[(i as i32 + dir).rem_euclid(n) as usize]
            } else if dir < 0 {
                *tabs.last().unwrap()
            } else {
                tabs[0]
            };
            if next == Tab::Alerts {
                self.notifs.mark_all_read();
                self.alerts_scroll = 0.0;
            }
            self.dispatch(Event::SelectTab(next));
        }
    }

    fn on_hwheel(&mut self, delta: i16) {
        if !matches!(self.mode, Mode::Open(_)) || delta == 0 {
            return;
        }
        let step = self.swipe.feed(delta, self.now_ms());
        if step != 0 {
            self.step_tab(step);
        }
    }

    fn sync_tick(&self) {
        if self.timer.phase == Phase::Running {
            timer(self.hwnd, T_TICK, 250);
        } else {
            kill(self.hwnd, T_TICK);
        }
    }

    fn on_tick(&mut self) {
        let now = self.now_ms();
        let finished = self.timer.tick(now);
        let s = self.timer.remaining_secs(now);
        if finished || s != self.last_sec {
            self.last_sec = s;
            self.refresh();
        }
        if finished {
            log::line("notch", &format!("pomodoro selesai stage={:?}", self.timer.stage));
            self.timer.advance_stage();
            self.sync_tick();
            if crate::prefs::get().notch_notifs {
                self.internal_notif_id = self.internal_notif_id.wrapping_add(1);
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                self.push_notification(nacre_core::notch::notif::Notification {
                    id: self.internal_notif_id,
                    app_name: "NOX".into(),
                    aumid: String::new(),
                    title: crate::prefs::tr("Timer selesai").into(),
                    body: String::new(),
                    timestamp_s: ts,
                    read: false,
                });
            }
            self.refresh();
            self.update_visibility();
        }
    }

    fn on_power(&mut self) {
        let b = sys::battery();
        let was = self.battery.is_some_and(|x| x.1);
        let now = b.is_some_and(|x| x.1);
        self.battery = b;
        if now && !was {
            self.charging_fresh = true;
            timer(self.hwnd, T_CHARGE, 4000);
        }
        self.refresh();
    }

    fn pill_screen_rect(&self) -> (i32, i32, i32, i32) {
        let base = state::size(state::look(Mode::Rest, None), self.dpi);
        let w = base.w.max(self.size.w);
        let l = ((self.screen_w as f32 - w) / 2.0) as i32;
        let top = island_gap(self.dpi).round() as i32;
        (l, 0, l + w.round() as i32, top + base.h.round() as i32)
    }

    fn cursor_in_notch_zone(&self) -> bool {
        let mut pt = windows::Win32::Foundation::POINT::default();
        if unsafe { GetCursorPos(&mut pt) }.is_err() {
            return false;
        }
        let w = self.size.w.max(200.0 * self.dpi);
        let pad = 12.0 * self.dpi;
        let cx = self.screen_w as f32 / 2.0;
        let left = cx - w / 2.0 - pad;
        let right = cx + w / 2.0 + pad;
        let bottom = island_gap(self.dpi) + self.size.h + 10.0 * self.dpi;
        (pt.x as f32) >= left && (pt.x as f32) <= right && pt.y >= 0 && (pt.y as f32) <= bottom
    }

    fn refresh_overlap(&mut self) {
        let mut ov = overlap::evaluate_notch(
            tracker::external_foreground(),
            std::process::id(),
            self.pill_screen_rect(),
            sys::screen_size(),
        );
        if ov.fullscreen && crate::prefs::get().above_fullscreen {
            ov.fullscreen = false;
        }
        if ov != self.overlap {
            self.overlap = ov;
            self.update_visibility();
            self.sync_visualizer();
        }
    }

    fn ensure_strip(&mut self) {
        if self.vis == Vis::Always {
            self._strip_drop = None;
            self.strip = None;
            self.edge_armed = false;
            return;
        }
        if self.strip.is_none() {
            if let Ok(s) = Strip::new_notch(self.hwnd, STRIP_ENTER, STRIP_LEAVE) {
                self._strip_drop = drop::register_for(s.hwnd(), self.hwnd, WM_DND, self.dnd.clone());
                self.strip = Some(s);
            }
        }
        self.place_strip();
    }

    fn place_strip(&self) {
        if let Some(s) = &self.strip {
            let w = ((240.0 * self.dpi).round() as i32).max(self.size.w.round() as i32);
            let h = if crate::prefs::get().menubar {
                (6.0 * self.dpi).round().clamp(4.0, 10.0) as i32
            } else {
                (4.0 * self.dpi).round().clamp(3.0, 6.0) as i32
            };
            let x = ((self.screen_w - w) / 2).max(0);
            s.place(x, 0, w, h);
            let show_strip = !self.shown && !self.overlap.fullscreen && !self.off;
            s.set_visible(show_strip);
            if show_strip {
                s.to_top();
            }
        }
    }

    fn interacting(&self) -> bool {
        self.tracking
            || matches!(self.mode, Mode::Open(_) | Mode::Drop)
            || self.grace
            || self.seek_drag.is_some()
            || self.scroll_drag.is_some()
            || self.dragging_out
            || self.pinned
            || self.focus.is_active()
            || self.send_pinned()
            || self.menu.as_ref().is_some_and(|m| m.is_open())
    }

    fn will_hide(&self) -> bool {
        // In Auto-hide (`Vis::OnDemand`), keep the compact media/lyrics pill visible whenever
        // lyrics are enabled in the media player and a track is actively playing (except fullscreen),
        // showing Live Lyrics when synced lyrics exist and falling back to the compact Music pill otherwise.
        let auto_hide_lyrics_reveal = self.vis == Vis::OnDemand
            && !self.overlap.fullscreen
            && self.lyrics_open
            && crate::prefs::get().lyrics_compact
            && self.has_media
            && self.playing;
        let inputs = Inputs {
            overlapped: self.overlap.overlapped,
            interacting: self.interacting(),
            edge_armed: self.edge_armed,
            starting: self.starting,
            fullscreen: self.overlap.fullscreen,
            revealing: self.notif_peeking
                || (!self.overlap.fullscreen && self.prayer_peeking)
                || auto_hide_lyrics_reveal,
        };
        self.off || should_hide(self.vis, &inputs)
    }

    fn update_visibility(&mut self) {
        let hide = self.will_hide();
        if hide && self.shown {
            self.shown = false;
            self.edge_armed = false;
            self.grace = false;
            kill(self.hwnd, T_EDGE);
            kill(self.hwnd, T_GRACE);
            kill(self.hwnd, T_DWELL);
            if self.mode == Mode::Peek {
                self.dispatch(Event::Leave);
            }
            let (hw, hh, hr, hy) = self.hidden_pose(self.size);
            let from_large = self.sp_h.pos > self.size.h + 12.0 * self.dpi || self.sp_w.pos > self.size.w + 24.0 * self.dpi;
            // In Floating Pill mode, fold a large panel slightly ahead of the upward float;
            // in Bezel-attached Notch mode, collapse width/height/y in lockstep straight into the top bezel.
            let dim_sp = if from_large && crate::prefs::get().notch_island {
                SpringParams::hide_collapse()
            } else {
                SpringParams::stage_hide()
            };
            let hide_sp = SpringParams::stage_hide();
            self.sp_w.set_target(hw, dim_sp);
            self.sp_h.set_target(hh, dim_sp);
            self.sp_r.set_target(hr, dim_sp);
            self.sp_y.set_target(hy, hide_sp);
            self.sp_vis.set_target(0.0, hide_sp);
            self.sp_sh_op.set_target(0.0, hide_sp);
            self.sp_csh_op.set_target(0.0, hide_sp);
            self.sp_timer_pill.set_target(0.0, hide_sp);
            self.start_frame();
            self.publish_notch();
            self.sync_visualizer();
            log::line("notch", "sembunyi");
        } else if !hide && !self.shown {
            self.shown = true;
            self.rgn_rect = (-1, -1, -1, -1);
            self.rgn_timer = (-1, -1, -1, -1);
            // If emerging from a fully settled hidden state (not reversing mid-hide), park springs cleanly
            // at the tucked hidden pose for the current size so emergence begins immediately with zero dead air.
            if !self.frame_active || self.sp_vis.pos <= 0.02 {
                let (hw, hh, hr, hy) = self.hidden_pose(self.size);
                self.sp_w.snap(hw);
                self.sp_h.snap(hh);
                self.sp_r.snap(hr);
                self.sp_y.snap(hy);
                self.sp_vis.snap(0.0);
                self.sp_sh_op.snap(0.0);
                self.sp_csh_op.snap(0.0);
            }
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
            self.shell.set_visible(true, self.hwnd);
            crate::topmost::request_raise();
            let show_sp = if matches!(self.look.view, View::Open(_) | View::Drop) {
                SpringParams::open()
            } else {
                SpringParams::stage_show()
            };
            let (sh_blur, sh_dy, sh_op) = state::shadow(self.look);
            let (csh_blur, csh_dy, csh_op) = state::contact_shadow(self.look);
            self.sp_w.set_target(self.size.w, show_sp);
            self.sp_h.set_target(self.size.h, show_sp);
            self.sp_r.set_target(self.size.radius, show_sp);
            self.sp_y.set_target(island_gap(self.dpi), SpringParams::stage_show());
            self.sp_vis.set_target(1.0, SpringParams::vis_show());
            self.sp_sh_blur.set_target(sh_blur, show_sp);
            self.sp_sh_dy.set_target(sh_dy, show_sp);
            self.sp_sh_op.set_target(sh_op, show_sp);
            self.sp_csh_blur.set_target(csh_blur, show_sp);
            self.sp_csh_dy.set_target(csh_dy, show_sp);
            self.sp_csh_op.set_target(csh_op, show_sp);
            self.sp_timer_pill.set_target(if self.want_detached_timer() { 1.0 } else { 0.0 }, show_sp);
            if self.content_hidden_for_retract {
                self.content_hidden_for_retract = false;
                if let Ok(c) = self.shell.content.cast::<CompositionObject>() {
                    let _ = self.shell.content.SetScale(Vector3 { X: 1.0, Y: 1.0, Z: 1.0 });
                    let _ = self.shell.content.SetOffset(Vector3 { X: 0.0, Y: 0.0, Z: 0.0 });
                    let _ = keyframes(&self.paint.comp, &c, "Opacity", 0.0, 1.0, 180, 30);
                }
            }
            self.apply_springs_to_shell();
            self.start_frame();
            self.publish_notch();
            self.sync_visualizer();
            log::line("notch", "tampil");
        }
        if let Some(s) = &self.strip {
            let show_strip = !self.shown && !self.overlap.fullscreen && !self.off;
            s.set_visible(show_strip);
            if show_strip {
                s.to_top();
            }
        }
    }

    fn publish_notch(&mut self) {
        self.publish_notch_for_width(self.size.w, true);
    }

    fn publish_notch_for_width(&mut self, w: f32, force: bool) {
        if self.off {
            self.last_pub_half = -1;
            crate::services::set_notch_layout(None, None);
            return;
        }
        let rail_h = (30.0 * self.dpi).round() as i32;
        if !self.shown {
            self.last_pub_half = -1;
            crate::services::set_notch_layout(None, Some(rail_h));
            return;
        }
        let ear = ear_px(self.dpi);
        let pad = 4.0 * self.dpi;
        let half = (w / 2.0 + ear + pad).ceil() as i32;
        if !force && (half - self.last_pub_half).abs() < 2 {
            return;
        }
        self.last_pub_half = half;
        let cx = self.screen_w / 2;
        let gap = ((cx - half).max(0), (cx + half).min(self.screen_w));
        crate::services::set_notch_layout(Some(gap), Some(rail_h));
    }

    fn set_reserved(&mut self, on: bool) {
        self.reserved = on;
        if on {
            let strip = (island_gap(self.dpi) + 34.0 * self.dpi).ceil() as i32;
            let r = nacre_platform::shell::appbar::reserve_top(self.hwnd, self.screen_w, strip);
            log::line("appbar", &format!("atas strip={strip} hasil={r:?}"));
        } else {
            nacre_platform::shell::appbar::release_top_if_owner(self.hwnd);
        }
    }

    fn apply_prefs(&mut self) {
        let s = crate::prefs::get();
        self.off = !s.notch_enabled;
        let (vis, reserve) = (s.notch.mode(), s.notch.reserves() && s.notch_enabled && !s.menubar);
        if vis != self.vis {
            self.vis = vis;
            if vis == Vis::OnDemand {
                self.starting = false;
            }
            self.ensure_strip();
        } else {
            self.place_strip();
        }
        if reserve {
            if !self.reserved || !nacre_platform::shell::appbar::is_top_owner(self.hwnd) {
                self.set_reserved(true);
            }
        } else if self.reserved {
            self.set_reserved(false);
        }
        self.shell.apply_material(self.tier);
        self.refresh_overlap();
        self.update_visibility();
        self.publish_notch();
        self.sync_download_watch();
        self.sync_notif_watch();
        if let Mode::Open(cur) = self.mode {
            if !s.notch_tab_visible(cur) {
                self.mode = Mode::Open(s.default_notch_tab());
            }
        }
        let look = state::look(self.mode, state::activity(&self.live()));
        let target_isl = if s.notch_island { 1.0 } else { 0.0 };
        let shape_changed = (self.sp_island.target - target_isl).abs() > 0.01;
        if look != self.look {
            self.transition(look);
        } else {
            self.apply_size(Some(if shape_changed { SpringParams::shape_morph() } else { SpringParams::swap() }));
            if !shape_changed {
                self.render();
            }
        }
        self.update_visibility();
        if self.lyr.is_some() {
            self.lyr_idx = self.lyr.as_ref().and_then(|l| l.index_at(self.lyric_pos_ms()));
            self.schedule_lyric();
        }
        log::line("notch", &format!("pengaturan: perilaku={vis:?} cadangan={reserve} mati={}", self.off));
    }

    fn refresh_tier(&mut self) {
        let t = auto_tier(self.glass_override);
        if t != self.tier {
            self.tier = t;
            self.shell.apply_material(t);
        }
    }

    fn prayer_snapshot(&self) -> (String, nacre_core::notch::prayer::PrayerSnapshot) {
        let s = crate::prefs::get();
        let now = nacre_platform::time::local_now();
        let tz = nacre_platform::time::tz_offset_minutes();
        let lat = s.prayer_lat_e4 as f64 / 10_000.0;
        let lon = s.prayer_lon_e4 as f64 / 10_000.0;
        let method = nacre_core::notch::prayer::CalcMethod::from_u8(s.prayer_method);
        let sched = nacre_core::notch::prayer::compute_day(now.year, now.month, now.day, lat, lon, tz, method);
        let snap = nacre_core::notch::prayer::snapshot(sched, now.hour, now.minute, now.second);
        let city = if s.prayer_city.is_empty() {
            "Jakarta".to_string()
        } else {
            s.prayer_city.as_str().to_string()
        };
        (city, snap)
    }

    fn show_prayer_alert(
        &mut self,
        prayer: nacre_core::notch::prayer::Prayer,
        time_min: u16,
        kind: nacre_core::notch::prayer::AlertKind,
    ) {
        self.prayer_alert = Some(views::PrayerAlertView { prayer, time_min, kind });
        self.prayer_peeking = !self.overlap.fullscreen;
        timer(self.hwnd, T_PRAYER_ALERT, PRAYER_ALERT_MS);
        if crate::prefs::get().prayer_alert_sound {
            nacre_platform::time::prayer_chime(kind == nacre_core::notch::prayer::AlertKind::Now);
        }
        self.refresh();
        self.update_visibility();
        log::line(
            "prayer",
            &format!(
                "alert prayer={} kind={kind:?} time={}",
                prayer.name(),
                nacre_core::notch::prayer::format_hhmm(time_min)
            ),
        );
    }

    fn check_prayer_alerts(&mut self) {
        let s = crate::prefs::get();
        if !s.prayer_alert_10m && !s.prayer_alert_5m && !s.prayer_alert_now {
            return;
        }
        let now = nacre_platform::time::local_now();
        let tz = nacre_platform::time::tz_offset_minutes();
        let lat = s.prayer_lat_e4 as f64 / 10_000.0;
        let lon = s.prayer_lon_e4 as f64 / 10_000.0;
        let method = nacre_core::notch::prayer::CalcMethod::from_u8(s.prayer_method);
        let sched = nacre_core::notch::prayer::compute_day(now.year, now.month, now.day, lat, lon, tz, method);
        let now_min = (now.hour.min(23) * 60 + now.minute.min(59)) as u16;
        if let Some((pr, kind)) = nacre_core::notch::prayer::check_alert(
            sched,
            now_min,
            s.prayer_alert_10m,
            s.prayer_alert_5m,
            s.prayer_alert_now,
        ) {
            let key = (now.year, now.month, now.day, pr, kind);
            if self.prayer_last_fired != Some(key) {
                self.prayer_last_fired = Some(key);
                self.show_prayer_alert(pr, sched.times[pr.idx()], kind);
            }
        }
    }

    fn on_reconfigure(&mut self) {
        let screen_w = sys::screen_size().0;
        let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96) as f32 / 96.0 * crate::prefs::ui_scale();
        let win = win_size(dpi, screen_w);
        self.frame_ms = frame_interval_ms();
        let changed = win != self.win || screen_w != self.screen_w || (dpi - self.dpi).abs() >= 0.01;
        if changed {
            log::line("notch", &format!("konfigurasi ulang layar_w={screen_w} dpi={dpi} frame_ms={}", self.frame_ms));
            self.screen_w = screen_w;
            self.dpi = dpi;
            self.win = win;
            self.rgn_rect = (-1, -1, -1, -1);
            self.rgn_timer = (-1, -1, -1, -1);
            let wx = (screen_w - win.0) / 2;
            unsafe {
                let _ = SetWindowPos(self.hwnd, None, wx, 0, win.0, win.1, SWP_NOACTIVATE | SWP_NOZORDER);
            }
            let _ = self._surface.root.SetSize(Vector2 { X: win.0 as f32, Y: win.1 as f32 });
            self.shell.place(self.hwnd, wx, win);
            self.paint.reset(dpi);
            self.menu = None;
        }
        if self.reserved {
            self.set_reserved(true);
        }
        self.place_strip();
        self.refresh_overlap();
        self.publish_notch();
        if changed {
            self.first = true;
            let look = self.look;
            self.transition(look);
        }
    }

    fn on_gfx_reset(&mut self) {
        self.paint.reset(self.dpi);
        self.art = None;
        self.shelf_icons.clear();
        self.dl_icons.clear();
        self.app_icons.clear();
        self.icons_pending.clear();
        self.menu = None;
        self.shell.apply_material(self.tier);
        self.first = true;
        self.refresh();
    }

    fn on_timer(&mut self, id: usize) {
        if id != T_TICK && id != T_FRAME && id != T_CARET {
            kill(self.hwnd, id);
        }
        match id {
            T_FRAME => self.tick_frame(),
            T_CARET => {
                match self.active_input {
                    Some(ActiveInput::Todo) => {
                        if let Some(ti) = self.todo_input.as_mut() {
                            ti.blink_caret();
                        }
                    }
                    Some(ActiveInput::Gemini) => {
                        if let Some(gi) = self.gemini_input.as_mut() {
                            gi.blink_caret();
                        }
                    }
                    None => kill(self.hwnd, T_CARET),
                }
            }
            T_COPIED => {
                self.gemini_copied = false;
                if self.mode == Mode::Open(Tab::Gemini) {
                    self.render();
                }
            }
            T_NAV_COLLAPSE => {
                self.nav_collapse_armed = false;
                self.set_nav_expanded(false);
            }
            T_SCROLL_IDLE => {
                if self.scroll_drag.is_none() {
                    self.sp_sb_active.set_target(0.0, SpringParams::tactile());
                    self.start_frame();
                }
            }
            T_DWELL => {
                if (self.tracking || (self.grace && self.cursor_in_notch_zone())) && !self.cursor_on_media_button() {
                    self.dispatch(Event::Dwell);
                }
            }
            T_LEAVE => {
                if !self.tracking && !self.send_pinned() && !self.pinned && !self.dragging_out && !self.focus.is_active() {
                    // Open panel already waited `LEAVE_GRACE_MS` (260ms) since pointer left;
                    // drop leftover leave-grace so Auto-hide / overlapped Smart mode folds and
                    // tucks away in ONE continuous fluid motion instead of pausing mid-air as a pill.
                    if !self.cursor_in_notch_zone() {
                        self.grace = false;
                        kill(self.hwnd, T_GRACE);
                    }
                    self.dispatch(Event::Leave);
                }
            }
            T_GHOST => {
                if let Ok(k) = self.shell.ghost.Children() {
                    let _ = k.RemoveAll();
                }
            }
            T_TICK => self.on_tick(),
            T_CHARGE => {
                self.charging_fresh = false;
                self.refresh();
            }
            T_WINCHG => self.refresh_overlap(),
            T_RECONF => self.on_reconfigure(),
            T_EDGE => {
                self.edge_armed = true;
                self.grace = true;
                timer(self.hwnd, T_GRACE, 650);
                self.update_visibility();
                if self.vis == Vis::OnDemand && crate::prefs::get().notch_hover_open && self.mode == Mode::Rest {
                    self.dispatch(Event::Enter);
                    if self.mode == Mode::Peek {
                        timer(self.hwnd, T_DWELL, motion::DWELL_MS);
                    }
                }
            }
            T_GRACE => {
                self.edge_armed = false;
                if self.shown && self.cursor_in_notch_zone() {
                    self.grace = true;
                    timer(self.hwnd, T_GRACE, 220);
                } else {
                    self.grace = false;
                    self.update_visibility();
                }
            }
            T_START => {
                self.starting = false;
                self.update_visibility();
            }
            T_GLANCE => {
                self.rotate_glance();
                self.sync_glance();
            }
            T_LYRIC => self.on_lyric_tick(),
            T_SEND => {
                let idle = self.send.as_ref().is_some_and(|s| !s.prog.active && s.last.elapsed() > SEND_IDLE);
                if idle {
                    log::line("kirim", "sesi menganggur 15 menit");
                    self.stop_send();
                } else if self.send.is_some() {
                    timer(self.hwnd, T_SEND, 30_000);
                }
            }
            T_SENDDONE => {
                kill(self.hwnd, T_SENDDONE);
                self.refresh();
            }
            T_TESTSEND => {
                kill(self.hwnd, T_TESTSEND);
                if let Ok(p) = std::env::var("NACRE_TEST_SEND") {
                    self.start_send(p.split(';').map(|s| s.to_string()).collect());
                }
            }
            T_DLDONE => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), T_DLDONE);
                }
                self.dl_done = None;
                self.refresh();
            }
            T_NOTIF_TOAST => {
                self.toast_id = None;
                self.notif_peeking = false;
                self.refresh();
                self.update_visibility();
            }
            T_PRAYER_ALERT => {
                kill(self.hwnd, T_PRAYER_ALERT);
                self.prayer_alert = None;
                self.prayer_peeking = false;
                self.refresh();
                self.update_visibility();
            }
            T_PROGRESS => {
                if self.mode == Mode::Open(Tab::Play) {
                    self.update_scrub_in_place();
                }
            }
            T_MINUTE => {
                self.check_prayer_alerts();
                if sys::battery() != self.battery {
                    self.on_power();
                } else if self.look.view == View::Compact(state::Activity::Glance)
                    || self.mode == Mode::Open(Tab::Prayer)
                {
                    self.render();
                }
                timer(self.hwnd, T_MINUTE, nacre_platform::time::ms_to_next_minute());
            }
            _ => {}
        }
    }
}

fn notif_row<'a>(
    n: &'a nacre_core::notch::notif::Notification,
    app_icons: &'a std::collections::HashMap<String, Option<CompositionDrawingSurface>>,
    now_unix: i64,
    en: bool,
) -> views::NotifRow<'a> {
    views::NotifRow {
        id: n.id,
        app_name: &n.app_name,
        title: &n.title,
        body: &n.body,
        time: nacre_core::notch::notif::relative_time(now_unix, n.timestamp_s, en),
        unread: !n.read,
        icon: app_icons.get(&n.aumid).and_then(|s| s.as_ref()),
    }
}

fn todos_file() -> std::path::PathBuf {
    crate::paths::config_dir().join("todos.json")
}

fn load_persisted_todos() -> nacre_core::notch::todo::TodoStore {
    let Ok(src) = std::fs::read_to_string(todos_file()) else {
        return nacre_core::notch::todo::TodoStore::new();
    };
    nacre_core::notch::todo::TodoStore::parse(&src)
}

fn save_persisted_todos(store: &nacre_core::notch::todo::TodoStore) {
    let _ = std::fs::create_dir_all(crate::paths::config_dir());
    let _ = std::fs::write(todos_file(), store.serialize());
}

fn shelf_file() -> std::path::PathBuf {
    crate::paths::config_dir().join("shelf.txt")
}

fn load_persisted_shelf() -> nacre_core::notch::shelf::Shelf {
    let Ok(src) = std::fs::read_to_string(shelf_file()) else {
        return nacre_core::notch::shelf::Shelf::new(24);
    };
    let parsed = nacre_core::notch::shelf::Shelf::parse(&src, 24);
    let mut kept = nacre_core::notch::shelf::Shelf::new(24);
    for it in parsed.items().iter().rev() {
        if std::path::Path::new(&it.path).exists() {
            kept.add(&it.path);
        }
    }
    kept
}

fn save_persisted_shelf(shelf: &nacre_core::notch::shelf::Shelf) {
    let _ = std::fs::create_dir_all(crate::paths::config_dir());
    let _ = std::fs::write(shelf_file(), shelf.serialize());
}

fn after_drop(paths: &[String], zone: usize) {
    if paths.is_empty() {
        return;
    }
    if zone == 2 {
        let Some(dir) = nacre_platform::pick::folder(crate::prefs::tr("Salin ke folder")) else { return };
        let from: Vec<u16> = paths.iter().flat_map(|p| p.encode_utf16().chain(std::iter::once(0))).chain(std::iter::once(0)).collect();
        let to: Vec<u16> = dir.encode_utf16().chain([0, 0]).collect();
        let mut op = windows::Win32::UI::Shell::SHFILEOPSTRUCTW {
            wFunc: windows::Win32::UI::Shell::FO_COPY,
            pFrom: windows::core::PCWSTR(from.as_ptr()),
            pTo: windows::core::PCWSTR(to.as_ptr()),
            fFlags: windows::Win32::UI::Shell::FOF_ALLOWUNDO.0 as u16,
            ..Default::default()
        };
        let r = unsafe { windows::Win32::UI::Shell::SHFileOperationW(&mut op) };
        log::line("notch", &format!("salin {} berkas ke folder: hasil={r}", paths.len()));
    } else {
        let file: Vec<u16> = paths[0].encode_utf16().chain(std::iter::once(0)).collect();
        let info = windows::Win32::UI::Shell::OPENASINFO {
            pcszFile: windows::core::PCWSTR(file.as_ptr()),
            pcszClass: windows::core::PCWSTR::null(),
            oaifInFlags: windows::Win32::UI::Shell::OAIF_ALLOW_REGISTRATION | windows::Win32::UI::Shell::OAIF_EXEC,
        };
        let _ = unsafe { windows::Win32::UI::Shell::SHOpenWithDialog(None, &info) };
    }
}

fn xy(lp: LPARAM) -> (f32, f32) {
    let stage_y = f32::from_bits(STAGE_Y_BITS.load(Ordering::Relaxed));
    ((lp.0 as i16) as f32, ((lp.0 >> 16) as i16) as f32 - stage_y)
}

/// Exact capsule/visor hit-test so all shadow-bleed margin and outside rounded-corner areas
/// strictly return `HTTRANSPARENT` while the active capsule body remains interactive.
fn point_in_capsule(cx: f32, cy: f32, x0: f32, w: f32, h: f32, r: f32, ear: f32) -> bool {
    if cy < 0.0 || cy > h {
        return false;
    }
    let ew = if ear > 0.5 { ear.min(h * 0.56) } else { 0.0 };
    if cx < x0 - ew || cx > x0 + w + ew {
        return false;
    }
    // Outside the main vertical walls: only interactive inside the upper shoulder band.
    if cx < x0 || cx > x0 + w {
        let eh = (ew * 1.08).min(h * 0.56);
        return cy <= eh;
    }
    // Exclude outside the bottom rounded corners so clicks just outside the curve pass through.
    let cr = r.clamp(0.0, (w * 0.5).min(h * 0.5));
    if cr > 1.0 && cy > h - cr {
        let dy = cy - (h - cr);
        if cx < x0 + cr {
            let dx = (x0 + cr) - cx;
            return dx * dx + dy * dy <= cr * cr;
        }
        if cx > x0 + w - cr {
            let dx = cx - (x0 + w - cr);
            return dx * dx + dy * dy <= cr * cr;
        }
    }
    true
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_NCHITTEST => {
            let mut pt = windows::Win32::Foundation::POINT {
                x: (lp.0 as i16) as i32,
                y: ((lp.0 >> 16) as i16) as i32,
            };
            let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut pt);
            let inside = with(hwnd, |d| {
                let w = d.sp_w.pos.max(8.0);
                let h = d.sp_h.pos.max(8.0);
                let r = d.sp_r.pos.max(0.0);
                let ear = d.sp_ear.pos.max(0.0);
                let x0 = d.x0_for(w);
                let cx = pt.x as f32;
                let cy = pt.y as f32 - d.sp_y.pos;
                if !d.shown || cy < 0.0 {
                    return false;
                }
                let in_main = point_in_capsule(cx, cy, x0, w, h, r, ear);
                let in_detached = d.detached_timer_geom(x0, w, h, r, ear).is_some_and(
                    |(tx, tw, th, tr, te, _)| point_in_capsule(cx, cy, tx, tw, th, tr, te),
                );
                in_main || in_detached
            })
            .unwrap_or(false);
            LRESULT(if inside { HTCLIENT as isize } else { HTTRANSPARENT as isize })
        }
        WM_MOUSEACTIVATE => {
            let active = with(hwnd, |d| d.focus.is_active()).unwrap_or(false);
            LRESULT(if active { MA_ACTIVATE } else { MA_NOACTIVATE } as isize)
        }
        WM_ACTIVATE => {
            if (wp.0 & 0xFFFF) == 0 {
                with(hwnd, |d| {
                    if d.focus.is_active() {
                        d.blur_inputs(false);
                    }
                });
            }
            LRESULT(0)
        }
        WM_CHAR => {
            let unit = wp.0 as u16;
            with(hwnd, |d| d.on_char(unit));
            LRESULT(0)
        }
        WM_KEYDOWN => {
            let vk = wp.0 as u16;
            with(hwnd, |d| d.on_keydown(vk));
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = xy(lp);
            if with(hwnd, |d| d.on_down(x, y)).unwrap_or(false) {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = xy(lp);
            with(hwnd, |d| {
                d.on_drag(x, y);
                d.on_move();
                d.on_hover(x, y);
            });
            LRESULT(0)
        }
        WM_CAPTURECHANGED => {
            if with(hwnd, |d| d.on_capture_changed()).is_none() {
                defer(Deferred::CaptureChanged);
            }
            LRESULT(0)
        }
        WM_MEDIA => {
            with(hwnd, |d| d.on_media());
            LRESULT(0)
        }
        crate::services::WM_VOLUME => {
            with(hwnd, |d| {
                let muted = nacre_platform::audio::latest().muted;
                if muted != d.muted {
                    d.muted = muted;
                    if d.mode == Mode::Open(Tab::Play) {
                        d.render();
                    }
                }
            });
            LRESULT(0)
        }
        WM_BANDS => {
            with(hwnd, |d| d.on_bands());
            LRESULT(0)
        }
        WM_LYRICS => {
            with(hwnd, |d| d.on_lyrics());
            LRESULT(0)
        }
        WM_DOWNLOAD => {
            with(hwnd, |d| d.on_download());
            LRESULT(0)
        }
        WM_AGENDA => {
            with(hwnd, |d| d.on_agenda());
            LRESULT(0)
        }
        WM_SEND => {
            with(hwnd, |d| d.on_send());
            LRESULT(0)
        }
        WM_PRIVACY => {
            if with(hwnd, |d| d.on_privacy()).is_none() {
                defer(Deferred::Privacy);
            }
            LRESULT(0)
        }
        WM_RECONF => {
            if with(hwnd, |d| d.on_reconfigure()).is_none() {
                defer(Deferred::Reconfigure);
            }
            LRESULT(0)
        }
        WM_ICON => {
            if with(hwnd, |d| d.on_icon()).is_none() {
                defer(Deferred::Icon);
            }
            LRESULT(0)
        }
        WM_NOTIF => {
            if with(hwnd, |d| d.on_notif()).is_none() {
                defer(Deferred::Notif);
            }
            LRESULT(0)
        }
        WM_GTASKS => {
            if with(hwnd, |d| d.on_gtasks()).is_none() {
                defer(Deferred::GTasks);
            }
            LRESULT(0)
        }
        WM_GEMINI => {
            if with(hwnd, |d| d.on_gemini()).is_none() {
                defer(Deferred::Gemini);
            }
            LRESULT(0)
        }
        WM_GTASKS_ACTION => {
            with(hwnd, |d| match wp.0 {
                1 => {
                    let _ = d.gtasks.commands.send(nacre_platform::gtasks::Cmd::Connect);
                }
                2 => {
                    let _ = d.gtasks.commands.send(nacre_platform::gtasks::Cmd::Disconnect);
                }
                _ => {}
            });
            LRESULT(0)
        }
        WM_PRAYER_TEST => {
            with(hwnd, |d| {
                let (_, snap) = d.prayer_snapshot();
                d.show_prayer_alert(snap.next, snap.next_time_min, nacre_core::notch::prayer::AlertKind::Before5);
            });
            LRESULT(0)
        }
        WM_START_DRAGOUT => {
            let path = with(hwnd, |d| d.shelf.items().get(wp.0).map(|it| it.path.clone())).flatten();
            if let Some(p) = path {
                let dropped = dragout::start(vec![p]);
                log::line("notch", &format!("seret-keluar selesai dropped={dropped}"));
            }
            with(hwnd, |d| {
                d.dragging_out = false;
                if !d.tracking && !d.pinned && !d.send_pinned() && matches!(d.mode, Mode::Open(_)) {
                    timer(hwnd, T_LEAVE, motion::LEAVE_GRACE_MS);
                }
                d.update_visibility();
            });
            LRESULT(0)
        }
        WM_DND => {
            with(hwnd, |d| d.on_dnd(wp.0, lp));
            LRESULT(0)
        }
        WM_AFTERDROP => {
            let pending = with(hwnd, |d| std::mem::take(&mut d.pending)).unwrap_or_default();
            after_drop(&pending, wp.0);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let (x, y) = xy(lp);
            with(hwnd, |d| d.on_right_click(x, y));
            LRESULT(0)
        }
        m if m == MENU_PICK => {
            let id = wp.0;
            with(hwnd, |d| d.menu_pick(id));
            LRESULT(0)
        }
        m if m == MENU_CLOSE => {
            if with(hwnd, |d| d.on_menu_close()).is_none() {
                defer(Deferred::MenuClose);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            if with(hwnd, |d| d.on_leave()).is_none() {
                defer(Deferred::MouseLeave);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = xy(lp);
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();
            if with(hwnd, |d| d.on_up()).unwrap_or(false) {
                return LRESULT(0);
            }
            with(hwnd, |d| d.on_click(x, y));
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = (wp.0 >> 16) as i16;
            let shift = (wp.0 & 0x0004) != 0; // MK_SHIFT
            with(hwnd, |d| {
                if shift {
                    d.on_hwheel(delta.saturating_neg());
                } else {
                    d.on_wheel(delta);
                }
            });
            LRESULT(0)
        }
        WM_MOUSEHWHEEL => {
            let delta = (wp.0 >> 16) as i16;
            with(hwnd, |d| d.on_hwheel(delta));
            LRESULT(0)
        }
        STRIP_ENTER => {
            with(hwnd, |d| {
                if d.vis != Vis::Always {
                    let delay = crate::prefs::get().notch_autohide_delay.clamp(50, 300) as u32;
                    timer(hwnd, T_EDGE, delay);
                }
            });
            LRESULT(0)
        }
        STRIP_LEAVE => {
            with(hwnd, |d| {
                kill(hwnd, T_EDGE);
                if d.edge_armed {
                    d.edge_armed = false;
                    d.grace = true;
                    timer(hwnd, T_GRACE, if d.vis == Vis::OnDemand { 550 } else { 700 });
                    d.update_visibility();
                }
            });
            LRESULT(0)
        }
        WM_TIMER => {
            with(hwnd, |d| d.on_timer(wp.0));
            LRESULT(0)
        }
        crate::prefs::WM_PREFS => {
            if with(hwnd, |d| {
                d.apply_prefs();
                d.on_reconfigure();
            })
            .is_none()
            {
                defer(Deferred::Prefs);
            }
            LRESULT(0)
        }
        WM_TOPMOST => {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            with(hwnd, |d| {
                d.shell.raise_behind(hwnd);
                if let Some(s) = &d.strip {
                    if !d.shown && !d.overlap.fullscreen && !d.off {
                        s.to_top();
                    }
                }
            });
            LRESULT(0)
        }
        WM_OPEN => {
            with(hwnd, |d| {
                if !d.off && !matches!(d.mode, Mode::Open(_)) {
                    d.dispatch(Event::Click);
                }
            });
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_DPICHANGED => {
            timer(hwnd, T_RECONF, 400);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_POWERBROADCAST => {
            with(hwnd, |d| {
                d.refresh_tier();
                d.on_power();
            });
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_SETTINGCHANGE => {
            with(hwnd, |d| d.refresh_tier());
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        crate::services::WM_GFX_RESET => {
            if with(hwnd, |d| d.on_gfx_reset()).is_none() {
                defer(Deferred::GfxReset);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            crate::services::unlisten_gfx(hwnd);
            NOTCH_HWND.store(0, Ordering::Relaxed);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn hover_scale(a: Action) -> f32 {
    if a == Action::Pin {
        1.15
    } else if matches!(a, Action::ShelfItem(_)) {
        crate::motion::HOVER_ROW
    } else {
        crate::motion::HOVER_BUTTON
    }
}
