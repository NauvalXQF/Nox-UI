#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Play,
    Alerts,
    Shelf,
    Todo,
    Gemini,
    Timer,
}

impl Tab {
    pub const MAIN: [Tab; 5] = [Tab::Play, Tab::Alerts, Tab::Shelf, Tab::Todo, Tab::Gemini];
    pub const ALL: [Tab; 6] = [Tab::Play, Tab::Alerts, Tab::Shelf, Tab::Todo, Tab::Gemini, Tab::Timer];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Play => "Putar",
            Tab::Alerts => "Notifikasi",
            Tab::Shelf => "Rak",
            Tab::Todo => "To-Do",
            Tab::Gemini => "Gemini",
            Tab::Timer => "Timer",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenOn {
    Hover,
    Click,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Rest,
    Peek,
    Open(Tab),
    Drop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Enter,
    Leave,
    Dwell,
    Click,
    SelectTab(Tab),
    DragEnter,
    DragLeave,
    Dropped,
    Escape,
}

pub fn step(mode: Mode, ev: Event, open_on: OpenOn, hovering: bool) -> Mode {
    let rest = if hovering { Mode::Peek } else { Mode::Rest };
    match (mode, ev) {
        (_, Event::DragEnter) => Mode::Drop,
        (Mode::Drop, Event::DragLeave) => rest,
        (Mode::Drop, Event::Dropped) => Mode::Open(Tab::Shelf),
        (Mode::Drop, _) => Mode::Drop,
        (Mode::Rest, Event::Enter) => Mode::Peek,
        (Mode::Peek, Event::Leave) => Mode::Rest,
        (Mode::Peek, Event::Dwell) if open_on == OpenOn::Hover => Mode::Open(Tab::Play),
        (Mode::Rest | Mode::Peek, Event::Click) => Mode::Open(Tab::Play),
        (Mode::Open(_), Event::SelectTab(t)) => Mode::Open(t),
        (Mode::Open(_), Event::Leave) => Mode::Rest,
        (Mode::Open(_), Event::Escape) => rest,
        (m, _) => m,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Notification,
    TimerEnding,
    Download,
    Player,
    Lyrics,
    Music,
    Charging,
    Timer,
    Shelf,
    Glance,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Live {
    pub notification: bool,
    pub media: bool,
    pub playing: bool,
    pub lyric_line: bool,
    pub lyrics_on: bool,
    pub timer_left_s: Option<i64>,
    pub charging_fresh: bool,
    pub shelf: usize,
    pub glance: bool,
    pub download: bool,
    pub wide_music: bool,
}

pub fn activity(l: &Live) -> Option<Activity> {
    if l.notification {
        return Some(Activity::Notification);
    }
    if l.timer_left_s.is_some_and(|s| s <= 60) {
        return Some(Activity::TimerEnding);
    }
    if l.download {
        return Some(Activity::Download);
    }
    if l.media && l.wide_music {
        return Some(Activity::Player);
    }
    if l.media && l.playing && l.lyrics_on && l.lyric_line {
        return Some(Activity::Lyrics);
    }
    if l.media {
        return Some(Activity::Music);
    }
    if l.charging_fresh {
        return Some(Activity::Charging);
    }
    if l.timer_left_s.is_some() {
        return Some(Activity::Timer);
    }
    if l.shelf > 0 {
        return Some(Activity::Shelf);
    }
    if l.glance {
        return Some(Activity::Glance);
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Rest,
    Compact(Activity),
    Open(Tab),
    Drop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub view: View,
    pub peek: bool,
}

pub fn look(mode: Mode, act: Option<Activity>) -> Look {
    let collapsed = |peek| Look { view: act.map_or(View::Rest, View::Compact), peek };
    match mode {
        Mode::Rest => collapsed(false),
        Mode::Peek => collapsed(true),
        Mode::Open(t) => Look { view: View::Open(t), peek: false },
        Mode::Drop => Look { view: View::Drop, peek: false },
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
    pub radius: f32,
}

pub const PEEK_GROW: (f32, f32) = (16.0, 4.0);
pub const COMPACT_MIN_W_DP: f32 = 190.0;
pub const COMPACT_MAX_W_DP: f32 = 540.0;
pub const NAV_EXPAND_H_DP: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavState {
    Hidden,
    Dots,
    Pill,
}

pub fn nav_state(tab: Tab, lyrics_open: bool, nav_hovered: bool) -> NavState {
    if tab == Tab::Play && lyrics_open {
        NavState::Hidden
    } else if nav_hovered {
        NavState::Pill
    } else {
        NavState::Dots
    }
}

/// Accumulator + gesture lock for Windows precision touchpad horizontal swipes (`WM_MOUSEHWHEEL`).
/// Absorbs momentum/inertia tails so a single two-finger swipe always steps by exactly 1 tab,
/// and suppresses accidental diagonal micro-swipes while vertical scrolling is active.
#[derive(Clone, Copy, Debug, Default)]
pub struct SwipeLatch {
    accum: i32,
    last_ev_ms: i64,
    locked_until_ms: i64,
    locked_dir: i32,
    vscroll_until_ms: i64,
}

impl SwipeLatch {
    pub const THRESHOLD: i32 = 60;
    pub const IDLE_RESET_MS: i64 = 120;
    pub const COOLDOWN_MS: i64 = 420;
    pub const VSCROLL_SUPPRESS_MS: i64 = 220;

    /// Records active vertical scroll input to prevent diagonal touchpad drift from switching tabs.
    pub fn note_vertical(&mut self, now_ms: i64) {
        self.accum = 0;
        self.vscroll_until_ms = self.vscroll_until_ms.max(now_ms + Self::VSCROLL_SUPPRESS_MS);
    }

    pub fn feed(&mut self, delta: i16, now_ms: i64) -> i32 {
        if delta == 0 {
            return 0;
        }
        if now_ms < self.vscroll_until_ms {
            self.accum = 0;
            return 0;
        }
        let gap = now_ms.saturating_sub(self.last_ev_ms);
        self.last_ev_ms = now_ms;
        if gap > Self::IDLE_RESET_MS {
            self.accum = 0;
        }
        let dir = if delta > 0 { 1 } else { -1 };
        if now_ms < self.locked_until_ms {
            if dir == self.locked_dir {
                // Continuous inertia stream in the same direction keeps the lock alive briefly.
                if gap < 95 {
                    self.locked_until_ms = self.locked_until_ms.max(now_ms + 150);
                }
                return 0;
            } else if gap < 60 {
                return 0;
            }
            // Deliberate opposite-direction swipe after a short pause breaks the lock.
            self.locked_until_ms = 0;
            self.accum = 0;
        }
        self.accum += delta as i32;
        if self.accum.abs() >= Self::THRESHOLD {
            let step = if self.accum > 0 { 1 } else { -1 };
            self.accum = 0;
            self.locked_dir = step;
            self.locked_until_ms = now_ms + Self::COOLDOWN_MS;
            step
        } else {
            0
        }
    }
}

/// Accumulates Windows `WM_MOUSEWHEEL` vertical deltas (both `±120` discrete mouse wheel notches
/// and high-frequency fractional Precision Touchpad ticks) into controlled line/row steps.
/// Returns positive steps for scrolling down (`delta < 0`) and negative steps for scrolling up (`delta > 0`).
#[derive(Clone, Copy, Debug, Default)]
pub struct WheelAccumulator {
    accum: i32,
    last_ev_ms: i64,
}

impl WheelAccumulator {
    pub const IDLE_RESET_MS: i64 = 180;

    pub fn reset(&mut self) {
        self.accum = 0;
    }

    pub fn feed(&mut self, delta: i16, now_ms: i64, step_delta: i32) -> i32 {
        if delta == 0 || step_delta <= 0 {
            return 0;
        }
        let gap = now_ms.saturating_sub(self.last_ev_ms);
        self.last_ev_ms = now_ms;
        // Negate so positive means scrolling down (increasing list/line offset).
        let d = -(delta as i32);
        if gap > Self::IDLE_RESET_MS || (self.accum != 0 && self.accum.signum() != d.signum()) {
            self.accum = 0;
        }
        self.accum += d;
        let steps = self.accum / step_delta;
        if steps != 0 {
            self.accum -= steps * step_delta;
        }
        steps
    }
}

/// Applies a signed vertical scroll step (`> 0` down, `< 0` up) clamped cleanly to `0..=max_scroll`.
pub fn apply_scroll_step(cur: usize, steps: i32, max_scroll: usize) -> usize {
    let base = cur.min(max_scroll);
    if steps > 0 {
        base.saturating_add(steps as usize).min(max_scroll)
    } else if steps < 0 {
        base.saturating_sub(steps.unsigned_abs() as usize)
    } else {
        base
    }
}

/// Converts a raw Win32 `WM_MOUSEWHEEL` delta (`> 0` up, `< 0` down, `±120` per mouse notch)
/// into a signed vertical scroll pixel delta (`> 0` scrolls down, `< 0` scrolls up).
pub fn wheel_delta_to_px(delta: i16, px_per_notch: f32) -> f32 {
    -(delta as f32 / 120.0) * px_per_notch
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SizeContext {
    pub lyrics_open: bool,
    pub shelf_empty: bool,
    pub nav_expanded: bool,
}

impl Default for SizeContext {
    fn default() -> Self {
        Self { lyrics_open: true, shelf_empty: true, nav_expanded: false }
    }
}

pub fn adaptive_compact_w(content_px: f32, chrome_px: f32, min_px: f32, max_safe_px: f32) -> f32 {
    let hi = max_safe_px.max(min_px);
    (content_px + chrome_px).ceil().clamp(min_px, hi)
}

pub fn size(l: Look, dpi: f32) -> Size {
    size_with(l, dpi, SizeContext::default())
}

pub fn size_with(l: Look, dpi: f32, ctx: SizeContext) -> Size {
    let (mut w, mut h, mut r): (f32, f32, f32) = match l.view {
        View::Rest => (184.0, 30.0, 15.0),
        View::Compact(Activity::Notification) => (360.0, 46.0, 20.0),
        View::Compact(Activity::Lyrics) => (360.0, 36.0, 18.0),
        View::Compact(Activity::Glance) => (220.0, 30.0, 15.0),
        View::Compact(Activity::Download) => (300.0, 36.0, 18.0),
        View::Compact(Activity::Player) => (540.0, 100.0, 30.0),
        View::Compact(_) => (292.0, 30.0, 15.0),
        View::Open(Tab::Play) if ctx.lyrics_open => (530.0, 186.0, 28.0),
        View::Open(_) => (395.0, 186.0, 28.0),
        View::Drop => (480.0, 118.0, 28.0),
    };
    if let View::Open(tab) = l.view {
        if ctx.nav_expanded && nav_state(tab, ctx.lyrics_open, true) == NavState::Pill {
            h += NAV_EXPAND_H_DP;
        }
    }
    if l.peek && matches!(l.view, View::Rest | View::Compact(_)) {
        w += PEEK_GROW.0;
        h += PEEK_GROW.1;
        r += PEEK_GROW.1 / 2.0;
    }
    Size { w: w * dpi, h: h * dpi, radius: r * dpi }
}

/// Soft ambient elevation drop shadow `(blur_dp, offset_y_dp, opacity)`.
pub fn shadow(l: Look) -> (f32, f32, f32) {
    if matches!(l.view, View::Open(_) | View::Drop) {
        (22.0, 7.0, 0.65)
    } else {
        (18.0, 5.5, 0.56)
    }
}

/// Tight dense contact shadow right beneath the notch silhouette `(blur_dp, offset_y_dp, opacity)`.
pub fn contact_shadow(l: Look) -> (f32, f32, f32) {
    if matches!(l.view, View::Open(_) | View::Drop) {
        (5.5, 2.0, 0.88)
    } else {
        (5.0, 2.0, 0.85)
    }
}

pub mod motion {
    pub const DWELL_MS: u32 = 300;
    pub const PEEK_RATIO: f32 = 0.9;
    pub const PEEK_PERIOD_MS: u32 = 34;
    pub const OPEN_RATIO: f32 = 0.78;
    pub const OPEN_PERIOD_MS: u32 = 68;
    pub const CLOSE_RATIO: f32 = 1.0;
    pub const CLOSE_PERIOD_MS: u32 = 45;
    pub const SWAP_RATIO: f32 = 0.84;
    pub const SWAP_PERIOD_MS: u32 = 55;
    pub const CONTENT_DELAY_MS: u32 = 60;
    pub const CONTENT_FADE_MS: u32 = 170;
    pub const CONTENT_SCALE_FROM: f32 = 0.96;
    pub const CONTENT_RISE_DP: f32 = 6.0;
    pub const CLOSE_FADE_MS: u32 = 90;
    pub const SWAP_FADE_MS: u32 = 120;
    pub const LYRIC_SWAP_MS: u32 = 220;
    pub const LYRIC_RISE_DP: f32 = 8.0;
    pub const LEAVE_GRACE_MS: u32 = 260;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_peeks_then_dwell_opens_play_and_leaving_closes() {
        let m = step(Mode::Rest, Event::Enter, OpenOn::Hover, true);
        assert_eq!(m, Mode::Peek);
        let m = step(m, Event::Dwell, OpenOn::Hover, true);
        assert_eq!(m, Mode::Open(Tab::Play));
        assert_eq!(step(m, Event::Leave, OpenOn::Hover, false), Mode::Rest);
    }

    #[test]
    fn click_mode_ignores_dwell_but_opens_on_click() {
        let m = step(Mode::Peek, Event::Dwell, OpenOn::Click, true);
        assert_eq!(m, Mode::Peek);
        assert_eq!(step(m, Event::Click, OpenOn::Click, true), Mode::Open(Tab::Play));
    }

    #[test]
    fn dragging_files_shows_drop_zones_and_dropping_opens_the_shelf() {
        for from in [Mode::Rest, Mode::Peek, Mode::Open(Tab::Timer)] {
            assert_eq!(step(from, Event::DragEnter, OpenOn::Hover, false), Mode::Drop);
        }
        assert_eq!(step(Mode::Drop, Event::Leave, OpenOn::Hover, false), Mode::Drop, "keluar biasa diabaikan selama menyeret");
        assert_eq!(step(Mode::Drop, Event::DragLeave, OpenOn::Hover, false), Mode::Rest);
        assert_eq!(step(Mode::Drop, Event::DragLeave, OpenOn::Hover, true), Mode::Peek);
        assert_eq!(step(Mode::Drop, Event::Dropped, OpenOn::Hover, true), Mode::Open(Tab::Shelf));
    }

    #[test]
    fn tabs_switch_only_while_open_and_escape_collapses() {
        assert_eq!(step(Mode::Open(Tab::Play), Event::SelectTab(Tab::Shelf), OpenOn::Hover, true), Mode::Open(Tab::Shelf));
        assert_eq!(step(Mode::Rest, Event::SelectTab(Tab::Shelf), OpenOn::Hover, false), Mode::Rest);
        assert_eq!(step(Mode::Open(Tab::Shelf), Event::Escape, OpenOn::Hover, true), Mode::Peek);
        assert_eq!(step(Mode::Open(Tab::Shelf), Event::Escape, OpenOn::Hover, false), Mode::Rest);
    }

    #[test]
    fn activity_priority() {
        let base = Live { notification: false, media: true, playing: true, lyric_line: true, lyrics_on: true, timer_left_s: Some(30), charging_fresh: true, shelf: 2, glance: true, download: true, wide_music: false };
        assert_eq!(activity(&Live { notification: true, ..base }), Some(Activity::Notification));
        assert_eq!(activity(&base), Some(Activity::TimerEnding));
        assert_eq!(activity(&Live { timer_left_s: Some(300), ..base }), Some(Activity::Download), "unduhan di atas lirik dan musik");
        let l = Live { timer_left_s: Some(300), download: false, ..base };
        assert_eq!(activity(&Live { wide_music: true, ..l }), Some(Activity::Player), "lebar menggantikan lirik/musik ringkas");
        assert_eq!(activity(&Live { wide_music: true, playing: false, ..l }), Some(Activity::Player), "dijeda: tetap tampil");
        assert_eq!(activity(&Live { wide_music: true, media: false, playing: false, charging_fresh: false, timer_left_s: None, shelf: 0, glance: false, ..l }), None, "tanpa lagu: notch kosong");
        assert_eq!(activity(&Live { wide_music: true, download: true, ..l }), Some(Activity::Download));
        assert_eq!(activity(&l), Some(Activity::Lyrics));
        assert_eq!(activity(&Live { lyrics_on: false, ..l }), Some(Activity::Music));
        assert_eq!(activity(&Live { playing: false, ..l }), Some(Activity::Music), "dijeda: lirik ringkas tidak tampil");
        let no_media = Live { media: false, playing: false, ..l };
        assert_eq!(activity(&no_media), Some(Activity::Charging));
        assert_eq!(activity(&Live { charging_fresh: false, ..no_media }), Some(Activity::Timer));
        assert_eq!(activity(&Live { charging_fresh: false, timer_left_s: None, ..no_media }), Some(Activity::Shelf));
        let idle = Live { charging_fresh: false, timer_left_s: None, shelf: 0, ..no_media };
        assert_eq!(activity(&idle), Some(Activity::Glance), "sapaan & info hanya bila tidak ada apa-apa lagi");
        assert_eq!(activity(&Live { glance: false, ..idle }), None);
        assert_eq!(activity(&Live::default()), None);
    }

    #[test]
    fn sizes_follow_the_view_and_peek_grows_only_when_collapsed() {
        let rest = size(look(Mode::Rest, None), 1.0);
        let peek = size(look(Mode::Peek, None), 1.0);
        assert_eq!((rest.w, rest.h), (184.0, 30.0));
        assert_eq!((peek.w - rest.w, peek.h - rest.h), PEEK_GROW);
        let glance = size(look(Mode::Rest, Some(Activity::Glance)), 1.0);
        assert!(glance.w > rest.w && glance.w < 240.0 && glance.h == rest.h, "sapaan & info sedikit lebih lebar dari notch kosong");
        let player = size(look(Mode::Rest, Some(Activity::Player)), 1.0);
        assert!(player.w > 500.0 && player.w < 600.0 && player.h > 80.0 && player.h < 214.0, "lebar: lebih kecil dari panel terbuka");
        let dl = size(look(Mode::Rest, Some(Activity::Download)), 1.0);
        assert!(dl.h > rest.h && dl.h <= 40.0, "unduhan sedikit lebih tinggi untuk busur + angka");
        let music = size(look(Mode::Rest, Some(Activity::Music)), 1.0);
        let lyr = size(look(Mode::Rest, Some(Activity::Lyrics)), 1.0);
        assert!(music.w > rest.w && music.h == rest.h);
        assert!(lyr.w > music.w && lyr.h >= music.h && lyr.h < 40.0, "lirik ringkas: satu baris yang lebih lebar");
        let open_lyr = size_with(look(Mode::Open(Tab::Play), Some(Activity::Music)), 1.0, SizeContext { lyrics_open: true, shelf_empty: true, nav_expanded: false });
        assert_eq!((open_lyr.w, open_lyr.h, open_lyr.radius), (530.0, 186.0, 28.0));
        let open_lyr_nav = size_with(look(Mode::Open(Tab::Play), Some(Activity::Music)), 1.0, SizeContext { lyrics_open: true, shelf_empty: true, nav_expanded: true });
        assert_eq!(open_lyr_nav.h, open_lyr.h, "mode lirik menyembunyikan nav sehingga tinggi tidak berubah");
        let open_compact = size_with(look(Mode::Open(Tab::Play), Some(Activity::Music)), 1.0, SizeContext { lyrics_open: false, shelf_empty: true, nav_expanded: false });
        let open_compact_exp = size_with(look(Mode::Open(Tab::Play), Some(Activity::Music)), 1.0, SizeContext { lyrics_open: false, shelf_empty: true, nav_expanded: true });
        assert_eq!((open_compact.w, open_compact.h), (395.0, 186.0));
        assert_eq!(open_compact_exp.h - open_compact.h, NAV_EXPAND_H_DP);
        for t in [Tab::Alerts, Tab::Shelf, Tab::Todo, Tab::Gemini, Tab::Timer] {
            let z = size_with(look(Mode::Open(t), None), 1.0, SizeContext { lyrics_open: false, shelf_empty: true, nav_expanded: false });
            assert_eq!((z.w, z.h), (395.0, 186.0), "tab {t:?} seragam dengan Alerts");
        }
        let b = size(look(Mode::Open(Tab::Play), None), 1.5);
        assert_eq!((b.w, b.radius), (795.0, 42.0));
        assert_eq!(adaptive_compact_w(40.0, 100.0, 190.0, 500.0), 190.0);
        assert_eq!(adaptive_compact_w(210.4, 100.0, 190.0, 500.0), 311.0);
        assert_eq!(adaptive_compact_w(600.0, 100.0, 190.0, 480.0), 480.0);
        assert_eq!(nav_state(Tab::Play, true, false), NavState::Hidden);
        assert_eq!(nav_state(Tab::Play, true, true), NavState::Hidden);
        assert_eq!(nav_state(Tab::Play, false, false), NavState::Dots);
        assert_eq!(nav_state(Tab::Play, false, true), NavState::Pill);
        assert_eq!(nav_state(Tab::Shelf, true, false), NavState::Dots);
        assert_eq!(nav_state(Tab::Shelf, true, true), NavState::Pill);
    }

    #[test]
    fn swipe_latch_absorbs_touchpad_inertia_and_steps_once() {
        let mut s = SwipeLatch::default();
        // Realistic touchpad burst + inertia tail over 400ms -> strictly 1 step!
        assert_eq!(s.feed(40, 100), 0);
        assert_eq!(s.feed(40, 116), 1);
        for t in [132, 160, 200, 250, 310, 380, 450] {
            assert_eq!(s.feed(60, t), 0, "inertia pada t={t} harus diserap");
        }
        // Distinct new swipe after fingers lifted & cooldown settled -> steps once again.
        assert_eq!(s.feed(120, 900), 1);
        // Reversing direction cleanly after short pause steps backward.
        assert_eq!(s.feed(-120, 980), -1);
    }

    #[test]
    fn swipe_latch_suppresses_diagonal_hwheel_during_vertical_scroll() {
        let mut s = SwipeLatch::default();
        s.note_vertical(1000);
        // Stray horizontal drift during vertical scroll must never switch tabs.
        assert_eq!(s.feed(40, 1020), 0);
        assert_eq!(s.feed(40, 1050), 0);
        assert_eq!(s.feed(80, 1180), 0);
        // After suppression window elapses, intentional horizontal swipe works normally.
        assert_eq!(s.feed(70, 1250), 1);
    }

    #[test]
    fn wheel_accumulator_handles_mouse_notches_touchpad_fractions_and_direction_flip() {
        let mut w = WheelAccumulator::default();
        // Standard mouse wheel notch (-120 = down 1 row at 120, or 2 lines at 60).
        assert_eq!(w.feed(-120, 100, 120), 1);
        assert_eq!(w.feed(120, 350, 120), -1);
        assert_eq!(w.feed(-120, 600, 60), 2);

        // Precision touchpad fractional vertical deltas accumulate smoothly without jumping 10x.
        let mut total_steps = 0;
        for (i, d) in [-20i16, -25, -30, -35, -30, -20].into_iter().enumerate() {
            total_steps += w.feed(d, 1000 + i as i64 * 16, 120);
        }
        assert_eq!(total_steps, 1, "160 total delta = 1 row step, not 6");

        // Reversing direction clears leftover remainder immediately for crisp response.
        assert_eq!(w.feed(60, 1120, 60), -1);

        // Clamping helper stays strictly within 0..=max_scroll.
        assert_eq!(apply_scroll_step(0, -2, 5), 0);
        assert_eq!(apply_scroll_step(4, 3, 5), 5);
        assert_eq!(apply_scroll_step(2, -1, 5), 1);

        // Smooth pixel scroll conversion maps standard notches & touchpad fractions accurately.
        assert!((wheel_delta_to_px(-120, 48.0) - 48.0).abs() < 0.01);
        assert!((wheel_delta_to_px(120, 48.0) + 48.0).abs() < 0.01);
        assert!((wheel_delta_to_px(-15, 48.0) - 6.0).abs() < 0.01);
    }

    #[test]
    fn shadow_deepens_and_softens_when_expanded() {
        let (b0, y0, o0) = shadow(look(Mode::Rest, None));
        let (b1, y1, o1) = shadow(look(Mode::Open(Tab::Play), None));
        assert_eq!((b0, y0, o0), (18.0, 5.5, 0.56));
        assert_eq!((b1, y1, o1), (22.0, 7.0, 0.65));
        assert!(b1 > b0 && y1 > y0 && o1 > o0);

        let (cb0, cy0, co0) = contact_shadow(look(Mode::Rest, None));
        let (cb1, cy1, co1) = contact_shadow(look(Mode::Open(Tab::Play), None));
        assert_eq!((cb0, cy0, co0), (5.0, 2.0, 0.85));
        assert_eq!((cb1, cy1, co1), (5.5, 2.0, 0.88));
        assert!(co1 >= co0 && cb1 >= cb0);
    }
}
