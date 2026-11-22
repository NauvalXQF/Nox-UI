use crate::i18n::Lang;
use crate::visibility::policy::Mode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behavior {
    Fixed,
    Overlay,
    Smart,
    AutoHide,
}

impl Behavior {
    pub fn parse(s: &str) -> Option<Behavior> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fixed" | "tetap" | "permanent" => Some(Behavior::Fixed),
            "overlay" | "menimpa" => Some(Behavior::Overlay),
            "smart" | "auto" => Some(Behavior::Smart),
            "autohide" | "auto-hide" | "peek" | "hidden" | "ondemand" => Some(Behavior::AutoHide),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Behavior::Fixed => "fixed",
            Behavior::Overlay => "overlay",
            Behavior::Smart => "smart",
            Behavior::AutoHide => "autohide",
        }
    }

    pub fn mode(self) -> Mode {
        match self {
            Behavior::Fixed | Behavior::Overlay => Mode::Always,
            Behavior::Smart => Mode::AutoHide,
            Behavior::AutoHide => Mode::OnDemand,
        }
    }

    pub fn reserves(self) -> bool {
        self == Behavior::Fixed
    }

    pub const ALL: [Behavior; 4] = [Behavior::Fixed, Behavior::Overlay, Behavior::Smart, Behavior::AutoHide];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Auto,
    Light,
    Dark,
}

impl Theme {
    pub fn parse(s: &str) -> Option<Theme> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" | "system" => Some(Theme::Auto),
            "light" | "terang" => Some(Theme::Light),
            "dark" | "gelap" => Some(Theme::Dark),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Theme::Auto => "auto",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }
    pub const ALL: [Theme; 3] = [Theme::Auto, Theme::Light, Theme::Dark];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    Blur,
    Glass,
    Black,
}

impl Material {
    pub fn parse(s: &str) -> Option<Material> {
        match s.trim().to_ascii_lowercase().as_str() {
            "blur" | "buram" | "frosted" => Some(Material::Blur),
            "glass" | "liquid" | "liquidglass" => Some(Material::Glass),
            "black" | "hitam" => Some(Material::Black),
            "clear" | "transparan" | "transparent" => Some(Material::Glass),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Material::Blur => "blur",
            Material::Glass => "glass",
            Material::Black => "black",
        }
    }
    pub const ALL: [Material; 3] = [Material::Blur, Material::Glass, Material::Black];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarStyle {
    Light,
    Dark,
}

impl BarStyle {
    pub fn parse(s: &str) -> Option<BarStyle> {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" | "terang" => Some(BarStyle::Light),
            "dark" | "gelap" => Some(BarStyle::Dark),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            BarStyle::Light => "light",
            BarStyle::Dark => "dark",
        }
    }
}

pub const ACCENT_MULTI: u8 = 255;

pub const ACCENTS: [[u8; 3]; 8] = [[10, 132, 255], [175, 82, 222], [255, 55, 95], [255, 69, 58], [255, 149, 10], [255, 204, 0], [48, 209, 88], [142, 142, 147]];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconStyle {
    Default,
    Dark,
    Clear,
    Tinted,
}

impl IconStyle {
    pub fn parse(s: &str) -> Option<IconStyle> {
        match s.trim().to_ascii_lowercase().as_str() {
            "default" | "bawaan" => Some(IconStyle::Default),
            "dark" | "gelap" => Some(IconStyle::Dark),
            "clear" | "bening" => Some(IconStyle::Clear),
            "tinted" | "berwarna" => Some(IconStyle::Tinted),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            IconStyle::Default => "default",
            IconStyle::Dark => "dark",
            IconStyle::Clear => "clear",
            IconStyle::Tinted => "tinted",
        }
    }
}

pub fn round_factor(round: u8) -> f32 {
    0.2 + 1.6 * (round.min(100) as f32 / 100.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotAction {
    None,
    Mission,
    Launchpad,
    Desktop,
    Notifications,
    Lock,
}

impl HotAction {
    pub const ALL: [HotAction; 6] =
        [HotAction::None, HotAction::Mission, HotAction::Launchpad, HotAction::Desktop, HotAction::Notifications, HotAction::Lock];

    pub fn parse(s: &str) -> Option<HotAction> {
        Self::ALL.into_iter().find(|a| a.key() == s.trim().to_ascii_lowercase())
    }
    pub fn key(self) -> &'static str {
        match self {
            HotAction::None => "none",
            HotAction::Mission => "mission",
            HotAction::Launchpad => "launchpad",
            HotAction::Desktop => "desktop",
            HotAction::Notifications => "notifications",
            HotAction::Lock => "lock",
        }
    }
    pub fn next(self) -> HotAction {
        let i = Self::ALL.iter().position(|a| *a == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ShortText {
    len: u8,
    buf: [u8; 96],
}

impl ShortText {
    pub const EMPTY: ShortText = ShortText { len: 0, buf: [0; 96] };

    pub fn new(s: &str) -> ShortText {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        let clean = clean.trim();
        let mut end = clean.len().min(96);
        while !clean.is_char_boundary(end) {
            end -= 1;
        }
        let mut buf = [0u8; 96];
        buf[..end].copy_from_slice(&clean.as_bytes()[..end]);
        ShortText { len: end as u8, buf }
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.buf[..self.len as usize]).unwrap_or("")
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for ShortText {
    fn default() -> Self {
        ShortText::EMPTY
    }
}

impl std::fmt::Debug for ShortText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub takeover: bool,
    pub dock: Behavior,
    pub notch: Behavior,
    pub blur: u8,
    pub transparency: u8,
    pub theme: Theme,
    pub lang: Lang,
    pub dock_material: Material,
    pub dock_round: u8,
    pub notch_material: Material,
    pub notch_round: u8,
    pub autostart: bool,
    pub dock_size: u8,
    pub dock_gap: u8,
    pub dock_zoom: u8,
    pub dock_margin: u8,
    pub anim: bool,
    pub refraction: bool,
    pub dock_enabled: bool,
    pub notch_enabled: bool,
    pub notch_hover_open: bool,
    pub win_shadow: bool,
    pub menubar: bool,
    pub clock_24h: bool,
    pub menubar_glass: bool,
    pub menubar_material: bool,
    pub menubar_style: BarStyle,
    pub hot: [HotAction; 4],
    pub hide_desktop_icons: bool,
    pub dock_plates: bool,
    pub above_fullscreen: bool,
    pub clipboard_history: bool,
    pub icon_style: IconStyle,
    pub icon_tint: u8,
    pub accent: u8,
    pub icon_variant: u8,
    pub min_effect: u8,
    pub notch_idle: u8,
    pub greet_name: ShortText,
    pub greet_text: ShortText,
    pub lyrics_compact: bool,
    pub lyrics_panel_open: bool,
    pub lyrics_online: bool,
    pub notch_downloads: bool,
    pub notch_send: bool,
    pub mac_shortcuts: bool,
    pub downloads_stack: bool,
    pub recent_apps: bool,
    pub icon_pack: bool,
    pub ui_scale: u8,
    pub notch_island: bool,
    pub notch_autohide_delay: u16,
    pub notch_notif_peek: bool,
    pub notch_notifs: bool,
    pub notch_tab_order: [crate::notch::state::Tab; 7],
    pub notch_tab_mask: u8,
    pub prayer_city: ShortText,
    pub prayer_lat_e4: i32,
    pub prayer_lon_e4: i32,
    pub prayer_method: u8,
    pub prayer_auto_loc: bool,
    pub prayer_alert_10m: bool,
    pub prayer_alert_5m: bool,
    pub prayer_alert_now: bool,
    pub prayer_alert_sound: bool,
    pub gemini_model: u8,
    pub gemini_custom_model: ShortText,
    pub widget_style: u8,
    pub battery_pct: u8,
}

pub const ALL_NOTCH_TABS_MASK: u8 = 0b0111_1111;

impl Default for Settings {
    fn default() -> Self {
        Settings {
            takeover: true,
            dock: Behavior::Smart,
            notch: Behavior::Fixed,
            blur: 100,
            transparency: 50,
            theme: Theme::Auto,
            lang: Lang::Id,
            dock_material: Material::Glass,
            dock_round: 62,
            notch_material: Material::Black,
            notch_round: 60,
            autostart: false,
            dock_size: 48,
            dock_gap: 8,
            dock_zoom: 184,
            dock_margin: 8,
            anim: true,
            refraction: true,
            dock_enabled: true,
            notch_enabled: true,
            notch_hover_open: true,
            win_shadow: true,
            menubar: true,
            clock_24h: false,
            menubar_glass: false,
            menubar_material: true,
            menubar_style: BarStyle::Dark,
            hot: [HotAction::None; 4],
            hide_desktop_icons: true,
            dock_plates: true,
            above_fullscreen: true,
            clipboard_history: true,
            icon_style: IconStyle::Default,
            icon_tint: 0,
            accent: 0,
            icon_variant: 1,
            min_effect: 0,
            notch_idle: 0,
            greet_name: ShortText::EMPTY,
            greet_text: ShortText::EMPTY,
            lyrics_compact: true,
            lyrics_panel_open: true,
            lyrics_online: true,
            notch_downloads: true,
            notch_send: true,
            mac_shortcuts: true,
            downloads_stack: true,
            recent_apps: true,
            icon_pack: true,
            ui_scale: 100,
            notch_island: false,
            notch_autohide_delay: 100,
            notch_notif_peek: true,
            notch_notifs: true,
            notch_tab_order: crate::notch::state::Tab::ALL,
            notch_tab_mask: ALL_NOTCH_TABS_MASK,
            prayer_city: ShortText::new("Jakarta"),
            prayer_lat_e4: -62088,
            prayer_lon_e4: 1068456,
            prayer_method: 0,
            prayer_auto_loc: false,
            prayer_alert_10m: true,
            prayer_alert_5m: true,
            prayer_alert_now: true,
            prayer_alert_sound: true,
            gemini_model: 0,
            gemini_custom_model: ShortText::EMPTY,
            widget_style: 0,
            battery_pct: 2,
        }
    }
}

impl Settings {
    pub fn parse(text: &str) -> Settings {
        let mut s = Settings::default();
        let mut legacy_glass = false;
        for line in text.trim_start_matches('\u{feff}').lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            match k {
                "takeover" => {
                    if let Some(b) = parse_bool(v) {
                        s.takeover = b;
                    }
                }
                "dock" => s.dock = Behavior::parse(v).unwrap_or(s.dock),
                "notch" => s.notch = Behavior::parse(v).unwrap_or(s.notch),
                "blur" => s.blur = v.parse::<i32>().map(|n| n.clamp(0, 100) as u8).unwrap_or(s.blur),
                "style" => {}
                "transparency" => s.transparency = v.parse::<i32>().map(|n| n.clamp(0, 100) as u8).unwrap_or(s.transparency),
                "theme" => s.theme = Theme::parse(v).unwrap_or(s.theme),
                "lang" => s.lang = Lang::parse(v).unwrap_or(s.lang),
                "dock_material" => s.dock_material = Material::parse(v).unwrap_or(s.dock_material),
                "notch_material" => s.notch_material = Material::parse(v).unwrap_or(s.notch_material),
                "dock_round" => s.dock_round = v.parse::<i32>().map(|n| n.clamp(0, 100) as u8).unwrap_or(s.dock_round),
                "notch_round" => s.notch_round = v.parse::<i32>().map(|n| n.clamp(0, 100) as u8).unwrap_or(s.notch_round),
                "dock_size" => s.dock_size = v.parse::<i32>().map(|n| n.clamp(32, 72) as u8).unwrap_or(s.dock_size),
                "dock_gap" => s.dock_gap = v.parse::<i32>().map(|n| n.clamp(0, 24) as u8).unwrap_or(s.dock_gap),
                "dock_zoom" => s.dock_zoom = v.parse::<i32>().map(|n| n.clamp(100, 250) as u8).unwrap_or(s.dock_zoom),
                "dock_margin" => s.dock_margin = v.parse::<i32>().map(|n| n.clamp(0, 60) as u8).unwrap_or(s.dock_margin),
                "dock_enabled" => {
                    if let Some(b) = parse_bool(v) {
                        s.dock_enabled = b;
                    }
                }
                "menubar" => {
                    if let Some(b) = parse_bool(v) {
                        s.menubar = b;
                    }
                }
                "hot_tl" => s.hot[0] = HotAction::parse(v).unwrap_or(s.hot[0]),
                "hot_tr" => s.hot[1] = HotAction::parse(v).unwrap_or(s.hot[1]),
                "hot_bl" => s.hot[2] = HotAction::parse(v).unwrap_or(s.hot[2]),
                "hot_br" => s.hot[3] = HotAction::parse(v).unwrap_or(s.hot[3]),
                "icon_style" => s.icon_style = IconStyle::parse(v).unwrap_or(s.icon_style),
                "downloads_stack" => {
                    if let Some(b) = parse_bool(v) {
                        s.downloads_stack = b;
                    }
                }
                "icon_pack" => {
                    if let Some(b) = parse_bool(v) {
                        s.icon_pack = b;
                    }
                }
                "recent_apps" => {
                    if let Some(b) = parse_bool(v) {
                        s.recent_apps = b;
                    }
                }
                "mac_shortcuts" => {
                    if let Some(b) = parse_bool(v) {
                        s.mac_shortcuts = b;
                    }
                }
                "accent" => s.accent = v.parse::<i32>().map(|n| if n == ACCENT_MULTI as i32 { ACCENT_MULTI } else { n.clamp(0, 7) as u8 }).unwrap_or(s.accent),
                "min_effect" => s.min_effect = v.parse::<i32>().map(|n| n.clamp(0, 1) as u8).unwrap_or(s.min_effect),
                "notch_idle" => s.notch_idle = v.parse::<i32>().map(|n| n.clamp(0, 2) as u8).unwrap_or(s.notch_idle),
                "greet_name" => s.greet_name = ShortText::new(v),
                "greet_text" => s.greet_text = ShortText::new(v),
                "lyrics_compact" => {
                    if let Some(b) = parse_bool(v) {
                        s.lyrics_compact = b;
                    }
                }
                "lyrics_panel_open" => {
                    if let Some(b) = parse_bool(v) {
                        s.lyrics_panel_open = b;
                    }
                }
                "lyrics_online" => {
                    if let Some(b) = parse_bool(v) {
                        s.lyrics_online = b;
                    }
                }
                "notch_downloads" => {
                    if let Some(b) = parse_bool(v) {
                        s.notch_downloads = b;
                    }
                }
                "notch_send" => {
                    if let Some(b) = parse_bool(v) {
                        s.notch_send = b;
                    }
                }
                "icon_variant" => s.icon_variant = v.parse::<i32>().map(|n| n.clamp(0, 2) as u8).unwrap_or(s.icon_variant),
                "icon_tint" => s.icon_tint = v.parse::<i32>().map(|n| n.clamp(0, 4) as u8).unwrap_or(s.icon_tint),
                "clipboard_history" => {
                    if let Some(b) = parse_bool(v) {
                        s.clipboard_history = b;
                    }
                }
                "above_fullscreen" => {
                    if let Some(b) = parse_bool(v) {
                        s.above_fullscreen = b;
                    }
                }
                "dock_plates" => {
                    if let Some(b) = parse_bool(v) {
                        s.dock_plates = b;
                    }
                }
                "hide_desktop_icons" => {
                    if let Some(b) = parse_bool(v) {
                        s.hide_desktop_icons = b;
                    }
                }
                "menubar_style" => {
                    if matches!(v.to_ascii_lowercase().as_str(), "glass" | "kaca") {
                        legacy_glass = true;
                    } else {
                        s.menubar_style = BarStyle::parse(v).unwrap_or(s.menubar_style);
                    }
                }
                "menubar_material" => {
                    if let Some(b) = parse_bool(v) {
                        s.menubar_material = b;
                    }
                }
                "menubar_glass" => {
                    if let Some(b) = parse_bool(v) {
                        s.menubar_glass = b;
                    }
                }
                "clock_24h" => {
                    if let Some(b) = parse_bool(v) {
                        s.clock_24h = b;
                    }
                }
                "win_shadow" => {
                    if let Some(b) = parse_bool(v) {
                        s.win_shadow = b;
                    }
                }
                "notch_hover_open" => {
                    if let Some(b) = parse_bool(v) {
                        s.notch_hover_open = b;
                    }
                }
                "notch_enabled" => {
                    if let Some(b) = parse_bool(v) {
                        s.notch_enabled = b;
                    }
                }
                "refraction" => {
                    if let Some(b) = parse_bool(v) {
                        s.refraction = b;
                    }
                }
                "anim" => {
                    if let Some(b) = parse_bool(v) {
                        s.anim = b;
                    }
                }
                "ui_scale" => s.ui_scale = v.trim_end_matches('%').parse::<i32>().map(|n| n.clamp(75, 200) as u8).unwrap_or(s.ui_scale),
                "notch_island" => {
                    if let Some(b) = parse_bool(v) {
                        s.notch_island = b;
                    }
                }
                "notch_autohide_delay" => {
                    s.notch_autohide_delay = v.parse::<i32>().map(|n| n.clamp(50, 300) as u16).unwrap_or(s.notch_autohide_delay);
                }
                "notch_notif_peek" => {
                    if let Some(b) = parse_bool(v) {
                        s.notch_notif_peek = b;
                    }
                }
                "notch_notifs" => {
                    if let Some(b) = parse_bool(v) {
                        s.notch_notifs = b;
                    }
                }
                "notch_tab_order" => {
                    use crate::notch::state::Tab;
                    let mut out = [Tab::Play; 7];
                    let mut n = 0usize;
                    for tok in v.split(',') {
                        if let Some(t) = Tab::from_id(tok) {
                            if !out[..n].contains(&t) && n < 7 {
                                out[n] = t;
                                n += 1;
                            }
                        }
                    }
                    for t in Tab::ALL {
                        if !out[..n].contains(&t) && n < 7 {
                            out[n] = t;
                            n += 1;
                        }
                    }
                    if n == 7 {
                        s.notch_tab_order = out;
                    }
                }
                "notch_tab_hidden" => {
                    use crate::notch::state::Tab;
                    let mut mask = ALL_NOTCH_TABS_MASK;
                    for tok in v.split(',') {
                        if let Some(t) = Tab::from_id(tok) {
                            mask &= !(1u8 << t.bit());
                        }
                    }
                    if mask != 0 {
                        s.notch_tab_mask = mask;
                    }
                }
                "prayer_city" => {
                    let st = ShortText::new(v);
                    if !st.is_empty() {
                        s.prayer_city = st;
                    }
                }
                "prayer_lat_e4" => s.prayer_lat_e4 = v.parse::<i32>().map(|n| n.clamp(-900_000, 900_000)).unwrap_or(s.prayer_lat_e4),
                "prayer_lon_e4" => s.prayer_lon_e4 = v.parse::<i32>().map(|n| n.clamp(-1_800_000, 1_800_000)).unwrap_or(s.prayer_lon_e4),
                "prayer_method" => s.prayer_method = v.parse::<u8>().map(|n| n.min(4)).unwrap_or(s.prayer_method),
                "prayer_auto_loc" => {
                    if let Some(b) = parse_bool(v) {
                        s.prayer_auto_loc = b;
                    }
                }
                "prayer_alert_10m" => {
                    if let Some(b) = parse_bool(v) {
                        s.prayer_alert_10m = b;
                    }
                }
                "prayer_alert_5m" => {
                    if let Some(b) = parse_bool(v) {
                        s.prayer_alert_5m = b;
                    }
                }
                "prayer_alert_now" => {
                    if let Some(b) = parse_bool(v) {
                        s.prayer_alert_now = b;
                    }
                }
                "prayer_alert_sound" => {
                    if let Some(b) = parse_bool(v) {
                        s.prayer_alert_sound = b;
                    }
                }
                "gemini_model" => s.gemini_model = v.parse::<u8>().map(|n| n.min(3)).unwrap_or(s.gemini_model),
                "gemini_custom_model" => s.gemini_custom_model = ShortText::new(v),
                "widget_style" => s.widget_style = v.parse::<u8>().map(|n| n.min(1)).unwrap_or(s.widget_style),
                "battery_pct" => s.battery_pct = v.parse::<u8>().map(|n| n.min(2)).unwrap_or(s.battery_pct),
                "autostart" => {
                    if let Some(b) = parse_bool(v) {
                        s.autostart = b;
                    }
                }
                _ => {}
            }
        }
        if legacy_glass {
            s.menubar_glass = false;
        }
        s
    }

    pub fn notch_tab_visible(&self, tab: crate::notch::state::Tab) -> bool {
        (self.notch_tab_mask & (1u8 << tab.bit())) != 0
    }

    pub fn visible_notch_tabs(&self) -> Vec<crate::notch::state::Tab> {
        let v: Vec<_> = self.notch_tab_order.iter().copied().filter(|&t| self.notch_tab_visible(t)).collect();
        if v.is_empty() {
            vec![self.notch_tab_order[0]]
        } else {
            v
        }
    }

    pub fn default_notch_tab(&self) -> crate::notch::state::Tab {
        self.visible_notch_tabs()[0]
    }

    pub fn move_notch_tab(&mut self, tab: crate::notch::state::Tab, delta: i32) {
        let Some(i) = self.notch_tab_order.iter().position(|&t| t == tab) else { return };
        let j = i as i32 + delta;
        if j >= 0 && (j as usize) < self.notch_tab_order.len() {
            self.notch_tab_order.swap(i, j as usize);
        }
    }

    pub fn toggle_notch_tab(&mut self, tab: crate::notch::state::Tab) {
        let bit = 1u8 << tab.bit();
        if (self.notch_tab_mask & bit) != 0 {
            let next = self.notch_tab_mask & !bit;
            if next != 0 {
                self.notch_tab_mask = next;
            }
        } else {
            self.notch_tab_mask |= bit;
        }
    }

    pub fn reset_notch_tabs(&mut self) {
        self.notch_tab_order = crate::notch::state::Tab::ALL;
        self.notch_tab_mask = ALL_NOTCH_TABS_MASK;
    }

    pub fn serialize(&self) -> String {
        let tab_order = self.notch_tab_order.iter().map(|t| t.id()).collect::<Vec<_>>().join(",");
        let tab_hidden = self
            .notch_tab_order
            .iter()
            .copied()
            .filter(|&t| !self.notch_tab_visible(t))
            .map(|t| t.id())
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "# NOX settings\ntakeover={}\ndock={}\nnotch={}\nblur={}\ntransparency={}\ntheme={}\nlang={}\ndock_material={}\ndock_round={}\nnotch_material={}\nnotch_round={}\nautostart={}\ndock_size={}\ndock_gap={}\ndock_zoom={}\ndock_margin={}\nanim={}\nrefraction={}\ndock_enabled={}\nnotch_enabled={}\nnotch_hover_open={}\nwin_shadow={}\nmenubar={}\nclock_24h={}\nmenubar_glass={}\nhot_tl={}\nhot_tr={}\nhot_bl={}\nhot_br={}\nhide_desktop_icons={}\nmenubar_style={}\ndock_plates={}\nabove_fullscreen={}\nclipboard_history={}\nicon_style={}\nicon_tint={}\naccent={}\nmac_shortcuts={}\ndownloads_stack={}\nrecent_apps={}\nicon_pack={}\nmenubar_material={}\nicon_variant={}\nmin_effect={}\nnotch_idle={}\ngreet_name={}\ngreet_text={}\nlyrics_compact={}\nlyrics_panel_open={}\nlyrics_online={}\nnotch_downloads={}\nnotch_send={}\nui_scale={}\nnotch_island={}\nnotch_autohide_delay={}\nnotch_notif_peek={}\nnotch_notifs={}\nnotch_tab_order={}\nnotch_tab_hidden={}\nprayer_city={}\nprayer_lat_e4={}\nprayer_lon_e4={}\nprayer_method={}\nprayer_auto_loc={}\nprayer_alert_10m={}\nprayer_alert_5m={}\nprayer_alert_now={}\nprayer_alert_sound={}\ngemini_model={}\ngemini_custom_model={}\nwidget_style={}\nbattery_pct={}\n",
            self.takeover,
            self.dock.key(),
            self.notch.key(),
            self.blur,
            self.transparency,
            self.theme.key(),
            self.lang.key(),
            self.dock_material.key(),
            self.dock_round,
            self.notch_material.key(),
            self.notch_round,
            self.autostart,
            self.dock_size,
            self.dock_gap,
            self.dock_zoom,
            self.dock_margin,
            self.anim,
            self.refraction,
            self.dock_enabled,
            self.notch_enabled,
            self.notch_hover_open,
            self.win_shadow,
            self.menubar,
            self.clock_24h,
            self.menubar_glass,
            self.hot[0].key(),
            self.hot[1].key(),
            self.hot[2].key(),
            self.hot[3].key(),
            self.hide_desktop_icons,
            self.menubar_style.key(),
            self.dock_plates,
            self.above_fullscreen,
            self.clipboard_history,
            self.icon_style.key(),
            self.icon_tint,
            self.accent,
            self.mac_shortcuts,
            self.downloads_stack,
            self.recent_apps,
            self.icon_pack,
            self.menubar_material,
            self.icon_variant,
            self.min_effect,
            self.notch_idle,
            self.greet_name.as_str(),
            self.greet_text.as_str(),
            self.lyrics_compact,
            self.lyrics_panel_open,
            self.lyrics_online,
            self.notch_downloads,
            self.notch_send,
            self.ui_scale,
            self.notch_island,
            self.notch_autohide_delay,
            self.notch_notif_peek,
            self.notch_notifs,
            tab_order,
            tab_hidden,
            self.prayer_city.as_str(),
            self.prayer_lat_e4,
            self.prayer_lon_e4,
            self.prayer_method,
            self.prayer_auto_loc,
            self.prayer_alert_10m,
            self.prayer_alert_5m,
            self.prayer_alert_now,
            self.prayer_alert_sound,
            self.gemini_model,
            self.gemini_custom_model.as_str(),
            self.widget_style,
            self.battery_pct
        )
    }
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub dark: bool,
    pub blur_sigma: f32,
    pub saturation: f32,
    pub tint: [u8; 4],
    pub rim: [u8; 4],
    pub solid: [u8; 4],
    pub highlight: f32,
    pub fg: [u8; 3],
    pub liquify: bool,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn look(s: &Settings, system_dark: bool, material: Material) -> Look {
    let dark = match (material, s.theme) {
        (Material::Black, _) => true,
        (_, Theme::Auto) => system_dark,
        (_, Theme::Dark) => true,
        (_, Theme::Light) => false,
    };
    let b = s.blur.min(100) as f32 / 100.0;
    let t = s.transparency.min(100) as f32 / 100.0;
    let (sigma_k, sat) = (0.45, 1.05);
    let mut blur_sigma = lerp(1.0, 40.0, b) * sigma_k;
    let mut saturation: f32 = sat;
    let (tr, tg, tb, a_max, a_min) = if dark { (20u8, 20u8, 24u8, 232.0, 40.0) } else { (255, 255, 255, 215.0, 24.0) };
    let mut a = lerp(a_max, a_min, t);
    let highlight: f32;
    let (mut rim, mut solid, fg) = if dark {
        ([70u8, 255, 255, 255], [235u8, 28, 28, 32], [255u8, 255, 255])
    } else {
        ([150, 255, 255, 255], [235, 238, 240, 246], [24, 24, 28])
    };
    let (mut tr, mut tg, mut tb) = (tr, tg, tb);
    match material {
        Material::Glass => {
            a = if dark { lerp(120.0, 22.0, t) } else { lerp(80.0, 18.0, t) };
            if !dark {
                (tr, tg, tb) = (226, 231, 240);
            }
            blur_sigma = lerp(1.5, 7.0, b) * sigma_k;
            saturation = saturation.max(1.4);
            rim = [140, 255, 255, 255];
            highlight = 0.35;
        }
        Material::Blur => {
            a = if dark { lerp(100.0, 30.0, t) } else { lerp(120.0, 40.0, t) };
            (tr, tg, tb) = if dark { (26, 28, 30) } else { (240, 242, 246) };
            blur_sigma = lerp(8.0, 36.0, b);
            saturation = 1.1;
            rim = if dark { [34, 255, 255, 255] } else { [60, 255, 255, 255] };
            highlight = 0.2;
        }
        Material::Black => {
            (tr, tg, tb) = (0, 0, 0);
            a = lerp(255.0, 232.0, t);
            saturation = 1.0;
            blur_sigma *= 0.8;
            rim = [0, 255, 255, 255];
            solid = [255, 0, 0, 0];
            highlight = 0.0;
        }
    }
    let liquify = material == Material::Glass && s.refraction;
    Look { dark, blur_sigma, saturation, tint: [a.round() as u8, tr, tg, tb], rim, solid, highlight, fg, liquify }
}

pub fn bar_look(s: &Settings, system_dark: bool) -> Look {
    let mut t = s.clone();
    match s.menubar_style {
        BarStyle::Light => t.theme = Theme::Light,
        BarStyle::Dark => t.theme = Theme::Dark,
    }
    let mut l = look(&t, system_dark, Material::Glass);
    l.tint[0] = 238;
    l.blur_sigma = l.blur_sigma.min(14.0);
    l.saturation = 1.0;
    l.highlight = 0.25;
    l.rim[0] = l.rim[0].min(60);
    l.liquify = false;
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_styles_force_theme_and_are_denser_than_glass() {
        let mut s = Settings::default();
        s.menubar_style = BarStyle::Light;
        let light = bar_look(&s, true);
        s.menubar_style = BarStyle::Dark;
        let dark = bar_look(&s, false);
        assert!(!light.dark && dark.dark);
        assert!(light.tint[0] >= 230 && dark.tint[0] >= 230);
        assert_eq!(BarStyle::parse("terang"), Some(BarStyle::Light));
        let again = Settings::parse(&s.serialize());
        assert_eq!(again.menubar_style, BarStyle::Dark);
    }

    #[test]
    fn legacy_glass_bar_becomes_transparent_and_light_glass_is_brighter() {
        let s = Settings::parse("menubar_glass=true
menubar_style=glass
");
        assert!(!s.menubar_glass, "latar kaca lama menjadi transparan");
        let mut st = Settings::default();
        st.theme = Theme::Light;
        let light = look(&st, false, Material::Glass);
        st.theme = Theme::Dark;
        let dark = look(&st, false, Material::Glass);
        assert!(light.tint[0] >= 40 && light.tint[0] <= 60, "kaca terang cukup terang: {}", light.tint[0]);
        assert!(dark.tint[0] >= 60);
    }

    #[test]
    fn roundtrip() {
        let s = Settings { takeover: false, dock: Behavior::Smart, notch: Behavior::Overlay, blur: 12, transparency: 90, theme: Theme::Light, lang: Lang::En, dock_material: Material::Black, dock_round: 80, notch_material: Material::Glass, notch_round: 10, autostart: true, dock_size: 60, dock_gap: 10, dock_zoom: 200, dock_margin: 30, anim: false, refraction: false, dock_enabled: false, notch_enabled: false, notch_hover_open: false, win_shadow: false, menubar: true, clock_24h: false, menubar_glass: true, hot: [HotAction::Mission, HotAction::Launchpad, HotAction::Lock, HotAction::Desktop], ..Default::default() };
        assert_eq!(Settings::parse(&s.serialize()), s);
    }

    #[test]
    fn defaults_show_rail_visor_and_dock() {
        let d = Settings::default();
        assert!(d.takeover && d.menubar && d.notch_enabled && d.dock_enabled && d.notch.reserves());
        assert_eq!((d.dock, d.menubar_style, d.autostart), (Behavior::Smart, BarStyle::Dark, false));
    }

    #[test]
    fn dock_metrics_clamped() {
        let s = Settings::parse("dock_size=500
dock_zoom=5
dock_margin=900");
        assert_eq!((s.dock_size, s.dock_zoom, s.dock_margin), (72, 100, 60));
    }

    #[test]
    fn junk_is_ignored_and_clamped() {
        let s = Settings::parse("\u{feff}blur=999\ntransparency=-5\ndock=banana\nwat\ntheme=gelap\n#x=1");
        assert_eq!(s.blur, 100);
        assert_eq!(s.transparency, 0);
        assert_eq!(s.dock, Behavior::Smart);
        assert_eq!(s.theme, Theme::Dark);
    }

    #[test]
    fn behavior_maps_to_policy() {
        assert_eq!(Behavior::Fixed.mode(), Mode::Always);
        assert!(Behavior::Fixed.reserves());
        assert!(!Behavior::Overlay.reserves());
        assert_eq!(Behavior::Overlay.mode(), Mode::Always);
        assert_eq!(Behavior::Smart.mode(), Mode::AutoHide);
        assert!(!Behavior::Smart.reserves());
        assert_eq!(Behavior::AutoHide.mode(), Mode::OnDemand);
        assert!(!Behavior::AutoHide.reserves());
        assert_eq!(Behavior::parse("smart"), Some(Behavior::Smart));
        for ah in ["autohide", "auto-hide", "peek", "hidden"] {
            assert_eq!(Behavior::parse(ah), Some(Behavior::AutoHide));
        }
    }

    #[test]
    fn materials_differ() {
        let s = Settings::default();
        let g = look(&s, false, Material::Glass);
        let k = look(&s, false, Material::Black);
        assert!(!g.dark && k.dark, "hitam selalu gelap");
        assert!(g.tint[0] < k.tint[0], "alfa: glass < black");
        assert_eq!(Material::parse("clear"), Some(Material::Glass));
    }

    #[test]
    fn round_factor_scales() {
        assert!((round_factor(50) - 1.0).abs() < 1e-6);
        assert!(round_factor(0) < 0.5 && round_factor(100) > 1.5);
    }

    #[test]
    fn blur_material_is_foggier_than_glass_and_follows_theme() {
        let s = Settings::default();
        let glass = look(&s, true, Material::Glass);
        let blur = look(&s, true, Material::Blur);
        assert!(blur.blur_sigma > glass.blur_sigma * 1.5, "blur {} vs glass {}", blur.blur_sigma, glass.blur_sigma);
        assert!(glass.liquify && !blur.liquify, "Crystal = gaya Liquify");
        assert!(glass.blur_sigma <= 7.0 && glass.rim[0] == 140, "Liquify: blur tipis, kilau atas 55%");
        let no_refr = Settings { refraction: false, ..Settings::default() };
        assert!(!look(&no_refr, true, Material::Glass).liquify, "refraksi dimatikan: kaca biasa");
        assert!(blur.dark && !look(&s, false, Material::Blur).dark);
        assert!(blur.tint[0] >= 60 && blur.rim[0] > 0 && blur.rim[0] < 60, "rim Blur sangat tipis");
        assert_eq!(Material::parse("blur"), Some(Material::Blur));
        assert_eq!(Material::Blur.key(), "blur");
        assert_eq!(Material::ALL.len(), 3);
    }

    #[test]
    fn notch_greeting_settings_round_trip() {
        let s = Settings::parse("notch_idle=1\ngreet_name=  Rani =)  \ngreet_text=Semangat, \u{1F680}!\n");
        assert_eq!(s.notch_idle, 1);
        assert_eq!(s.greet_name.as_str(), "Rani =)");
        let again = Settings::parse(&s.serialize());
        assert_eq!((again.notch_idle, again.greet_name, again.greet_text), (1, s.greet_name, s.greet_text));
        assert_eq!(Settings::default().notch_idle, 0);
        assert_eq!(Settings::parse(&Settings::parse("notch_idle=2\n").serialize()).notch_idle, 2, "musik & lirik");
        assert_eq!(Settings::parse("notch_idle=9\n").notch_idle, 2);
        let d = Settings::default();
        assert!(d.lyrics_compact && d.lyrics_online, "lirik online aktif bawaan (keputusan pengguna)");
        let off = Settings::parse(&Settings::parse("lyrics_online=false\nlyrics_compact=0\nnotch_downloads=0\n").serialize());
        assert!(!off.lyrics_online && !off.lyrics_compact && !off.notch_downloads);
        assert!(d.notch_downloads, "unduhan di notch aktif bawaan");
        assert!(d.notch_send, "zona Kirim aktif bawaan");
        assert!(!Settings::parse(&Settings::parse("notch_send=0\n").serialize()).notch_send);
        assert!(Settings::default().greet_name.is_empty());
        assert_eq!(ShortText::new("a\nb").as_str(), "ab");
        let long = "é".repeat(80);
        let t = ShortText::new(&long);
        assert!(t.as_str().len() <= 96 && t.as_str().chars().all(|c| c == 'é'));
    }

    #[test]
    fn multicolor_accent_and_icon_variant_round_trip() {
        let s = Settings::parse("accent=255\nicon_variant=2\n");
        assert_eq!(s.accent, ACCENT_MULTI);
        assert_eq!(s.icon_variant, 2);
        let again = Settings::parse(&s.serialize());
        assert_eq!((again.accent, again.icon_variant), (ACCENT_MULTI, 2));
        assert_eq!(Settings::parse("accent=9\n").accent, 7);
    }

    #[test]
    fn legacy_style_key_is_safely_ignored() {
        let s = Settings::parse("style=vivid\nmenubar_material=true\n");
        assert!(s.menubar_material);
        assert!(!s.serialize().lines().any(|l| l.starts_with("style=")));
    }

    #[test]
    fn look_responds_to_controls() {
        let mut s = Settings::default();
        let base = look(&s, true, Material::Glass);
        s.blur = 20;
        assert!(look(&s, true, Material::Glass).blur_sigma < base.blur_sigma);
        s = Settings::default();
        s.transparency = 100;
        assert!(look(&s, true, Material::Glass).tint[0] < base.tint[0]);
        s = Settings::default();
        s.theme = Theme::Light;
        assert!(!look(&s, true, Material::Glass).dark);
        s.theme = Theme::Auto;
        assert!(look(&s, true, Material::Glass).dark && !look(&s, false, Material::Glass).dark);
    }

    #[test]
    fn scale_island_and_widget_style_round_trip() {
        let s = Settings::parse("ui_scale=125%\nnotch_island=1\nwidget_style=1\n");
        assert_eq!((s.ui_scale, s.notch_island, s.widget_style), (125, true, 1));
        let again = Settings::parse(&s.serialize());
        assert_eq!((again.ui_scale, again.notch_island, again.widget_style), (125, true, 1));
        assert_eq!(Settings::parse("ui_scale=999").ui_scale, 200);
        assert_eq!(Settings::default().ui_scale, 100);
        assert_eq!(Settings::parse(&Settings::parse("battery_pct=2").serialize()).battery_pct, 2);
        assert_eq!(Settings::parse("battery_pct=9").battery_pct, 2);
        assert_eq!(Settings::default().battery_pct, 2);
    }

    #[test]
    fn notch_tabs_and_lyrics_panel_round_trip() {
        use crate::notch::state::Tab;
        let mut s = Settings::default();
        assert_eq!(s.notch_tab_order, Tab::ALL);
        assert_eq!(s.visible_notch_tabs(), Tab::ALL.to_vec());
        assert_eq!(s.default_notch_tab(), Tab::Play);
        assert!(s.lyrics_panel_open);

        s.move_notch_tab(Tab::Gemini, -1);
        s.move_notch_tab(Tab::Gemini, -1);
        s.move_notch_tab(Tab::Gemini, -1);
        s.move_notch_tab(Tab::Gemini, -1);
        assert_eq!(s.notch_tab_order[0], Tab::Gemini);
        assert_eq!(s.default_notch_tab(), Tab::Gemini);

        s.toggle_notch_tab(Tab::Shelf);
        s.toggle_notch_tab(Tab::Todo);
        assert!(!s.notch_tab_visible(Tab::Shelf));
        assert!(!s.notch_tab_visible(Tab::Todo));
        assert_eq!(s.visible_notch_tabs().len(), 5);

        s.lyrics_panel_open = false;
        s.prayer_city = ShortText::new("Semarang");
        s.prayer_lat_e4 = -69667;
        s.prayer_lon_e4 = 1104167;
        s.prayer_alert_sound = false;
        let rt = Settings::parse(&s.serialize());
        assert_eq!(rt.notch_tab_order, s.notch_tab_order);
        assert_eq!(rt.notch_tab_mask, s.notch_tab_mask);
        assert!(!rt.lyrics_panel_open);
        assert_eq!(rt.prayer_city.as_str(), "Semarang");
        assert_eq!((rt.prayer_lat_e4, rt.prayer_lon_e4, rt.prayer_alert_sound), (-69667, 1104167, false));

        // Hiding every tab must keep at least 1 visible tab.
        let mut only = Settings::default();
        for t in Tab::ALL {
            only.toggle_notch_tab(t);
        }
        assert_eq!(only.visible_notch_tabs().len(), 1);

        // Corrupt / duplicate / partial tab order is safely normalized to all 7 unique tabs.
        let norm = Settings::parse("notch_tab_order=timer,timer,bogus,gemini\nnotch_tab_hidden=play,alerts,shelf,todo,gemini,timer,prayer\n");
        assert_eq!(norm.notch_tab_order[0], Tab::Timer);
        assert_eq!(norm.notch_tab_order[1], Tab::Gemini);
        assert_eq!(norm.notch_tab_order.len(), 7);
        assert_eq!(norm.notch_tab_mask, ALL_NOTCH_TABS_MASK, "all-hidden config falls back to all visible");

        s.reset_notch_tabs();
        assert_eq!(s.notch_tab_order, Tab::ALL);
        assert_eq!(s.notch_tab_mask, ALL_NOTCH_TABS_MASK);
    }
}
