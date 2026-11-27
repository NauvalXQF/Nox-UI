use crate::icons::Bitmap;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager, GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    MediaPropertiesChangedEventArgs, PlaybackInfoChangedEventArgs, SessionsChangedEventArgs,
    TimelinePropertiesChangedEventArgs,
};
use windows::Storage::Streams::{DataReader, IRandomAccessStreamReference};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows_core::{IUnknown, Interface};

pub enum ArtUpdate {
    Unchanged,
    None,
    New(Bitmap),
}

pub struct Snapshot {
    pub has_media: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub can_seek: bool,
    pub art: ArtUpdate,
    pub app: String,
}

pub enum Command {
    Refresh,
    PlayPause,
    Next,
    Prev,
    SeekMs(i64),
}

pub struct Media {
    pub commands: Sender<Command>,
    pub snapshots: Receiver<Snapshot>,
}

fn read_art(r: Option<IRandomAccessStreamReference>) -> Option<Vec<u8>> {
    let stream = r?.OpenReadAsync().ok()?.join().ok()?;
    let reader = DataReader::CreateDataReader(&stream).ok()?;
    let mut all = Vec::new();
    loop {
        let n = reader.LoadAsync(65536).ok()?.join().ok()?;
        if n == 0 {
            break;
        }
        let mut chunk = vec![0u8; n as usize];
        reader.ReadBytes(&mut chunk).ok()?;
        all.extend_from_slice(&chunk);
        if all.len() > 8 * 1024 * 1024 {
            return None;
        }
    }
    (!all.is_empty()).then_some(all)
}

static PROPS_DIRTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct ArtState {
    key: String,
    hash: u64,
    since: std::time::Instant,
}

fn fnv(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

struct Subscriptions {
    session: Option<GlobalSystemMediaTransportControlsSession>,
    tokens: (i64, i64, i64),
}

fn same_com_session(a: &GlobalSystemMediaTransportControlsSession, b: &GlobalSystemMediaTransportControlsSession) -> bool {
    match (a.cast::<IUnknown>(), b.cast::<IUnknown>()) {
        (Ok(ua), Ok(ub)) => ua.as_raw() == ub.as_raw(),
        _ => false,
    }
}

fn is_playing(s: &GlobalSystemMediaTransportControlsSession) -> bool {
    s.GetPlaybackInfo()
        .ok()
        .and_then(|p| p.PlaybackStatus().ok())
        .is_some_and(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
}

fn pick_session(manager: &GlobalSystemMediaTransportControlsSessionManager) -> Option<GlobalSystemMediaTransportControlsSession> {
    let cur = manager.GetCurrentSession().ok();
    if let Some(ref c) = cur {
        if is_playing(c) {
            return cur;
        }
    }
    if let Ok(list) = manager.GetSessions() {
        let n = list.Size().unwrap_or(0);
        for i in 0..n {
            if let Ok(s) = list.GetAt(i) {
                if is_playing(&s) {
                    return Some(s);
                }
            }
        }
        if cur.is_none() && n > 0 {
            return list.GetAt(0).ok();
        }
    }
    cur
}

impl Subscriptions {
    fn drop_session(&mut self) {
        if let Some(s) = self.session.take() {
            let _ = s.RemoveMediaPropertiesChanged(self.tokens.0);
            let _ = s.RemovePlaybackInfoChanged(self.tokens.1);
            let _ = s.RemoveTimelinePropertiesChanged(self.tokens.2);
        }
    }

    /// Re-subscribes when the underlying COM session object changes (even if AUMID matches,
    /// e.g. switching between browser tabs). Returns `true` if the session identity changed.
    fn follow(&mut self, session: Option<GlobalSystemMediaTransportControlsSession>, tx: &Sender<Command>) -> bool {
        let same = match (&self.session, &session) {
            (Some(a), Some(b)) => same_com_session(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return false;
        }
        self.drop_session();
        let Some(s) = session else { return true };
        let t1 = tx.clone();
        let a = s.MediaPropertiesChanged(&TypedEventHandler::<_, MediaPropertiesChangedEventArgs>::new(move |_, _| {
            PROPS_DIRTY.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = t1.send(Command::Refresh);
            Ok(())
        }));
        let t2 = tx.clone();
        let b = s.PlaybackInfoChanged(&TypedEventHandler::<_, PlaybackInfoChangedEventArgs>::new(move |_, _| {
            let _ = t2.send(Command::Refresh);
            Ok(())
        }));
        let t3 = tx.clone();
        let c = s.TimelinePropertiesChanged(&TypedEventHandler::<_, TimelinePropertiesChangedEventArgs>::new(move |_, _| {
            let _ = t3.send(Command::Refresh);
            Ok(())
        }));
        self.tokens = (a.unwrap_or(0), b.unwrap_or(0), c.unwrap_or(0));
        self.session = Some(s);
        true
    }
}

#[allow(unused_assignments)]
pub fn spawn(notify: impl Fn() + Send + 'static) -> Media {
    let (cmd_tx, cmd_rx) = channel::<Command>();
    let (snap_tx, snap_rx) = channel::<Snapshot>();
    let tx_for_events = cmd_tx.clone();
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let Some(manager) = GlobalSystemMediaTransportControlsSessionManager::RequestAsync().ok().and_then(|o| o.join().ok())
        else {
            crate::log::line("media", "manajer sesi tidak tersedia");
            return;
        };
        let t = tx_for_events.clone();
        let _ = manager.CurrentSessionChanged(&TypedEventHandler::<_, CurrentSessionChangedEventArgs>::new(move |_, _| {
            let _ = t.send(Command::Refresh);
            Ok(())
        }));
        let t = tx_for_events.clone();
        let _ = manager.SessionsChanged(&TypedEventHandler::<_, SessionsChangedEventArgs>::new(move |_, _| {
            let _ = t.send(Command::Refresh);
            Ok(())
        }));

        let mut subs = Subscriptions { session: None, tokens: (0, 0, 0) };
        let mut art_state = ArtState { key: String::new(), hash: 0, since: std::time::Instant::now() };
        let mut pending = true;

        loop {
            if pending {
                pending = false;
                let current = pick_session(&manager);
                let switched = subs.follow(current.clone(), &tx_for_events);
                if switched {
                    art_state.key.clear();
                    art_state.hash = 0;
                    art_state.since = std::time::Instant::now();
                }
                let recheck = switched || PROPS_DIRTY.swap(false, std::sync::atomic::Ordering::Relaxed);
                let snap = build(current.as_ref(), &mut art_state, recheck);
                if snap_tx.send(snap).is_err() {
                    return;
                }
                notify();
            }
            match cmd_rx.recv() {
                Ok(Command::Refresh) => {
                    loop {
                        match cmd_rx.recv_timeout(Duration::from_millis(80)) {
                            Ok(Command::Refresh) => continue,
                            Ok(other) => {
                                act(&manager, other);
                                break;
                            }
                            Err(RecvTimeoutError::Timeout) => break,
                            Err(RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    pending = true;
                }
                Ok(other) => {
                    act(&manager, other);
                    std::thread::sleep(Duration::from_millis(120));
                    pending = true;
                }
                Err(_) => return,
            }
        }
    });
    Media { commands: cmd_tx, snapshots: snap_rx }
}

fn act(manager: &GlobalSystemMediaTransportControlsSessionManager, cmd: Command) {
    let Some(s) = pick_session(manager) else { return };
    match cmd {
        Command::PlayPause => {
            let _ = s.TryTogglePlayPauseAsync();
        }
        Command::Next => {
            let _ = s.TrySkipNextAsync();
        }
        Command::Prev => {
            let _ = s.TrySkipPreviousAsync();
        }
        Command::SeekMs(ms) => {
            let _ = s.TryChangePlaybackPositionAsync(ms * 10_000);
        }
        Command::Refresh => {}
    }
}

fn build(session: Option<&GlobalSystemMediaTransportControlsSession>, art_state: &mut ArtState, recheck: bool) -> Snapshot {
    let empty = |st: &mut ArtState| {
        st.key.clear();
        st.hash = 0;
        Snapshot {
            has_media: false,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            playing: false,
            position_ms: 0,
            duration_ms: 0,
            can_seek: false,
            art: ArtUpdate::None,
            app: String::new(),
        }
    };
    let Some(s) = session else { return empty(art_state) };
    let Some(props) = s.TryGetMediaPropertiesAsync().ok().and_then(|o| o.join().ok()) else { return empty(art_state) };
    let title = props.Title().map(|t| t.to_string()).unwrap_or_default();
    if title.is_empty() {
        return empty(art_state);
    }
    let artist = props.Artist().map(|t| t.to_string()).unwrap_or_default();
    let album = props.AlbumTitle().map(|t| t.to_string()).unwrap_or_default();

    let key = format!("{title}\u{1}{artist}");
    let new_track = art_state.key != key;
    if new_track {
        art_state.key = key;
        art_state.hash = 0;
        art_state.since = std::time::Instant::now();
    }
    let art = if new_track || recheck || art_state.since.elapsed() < Duration::from_secs(8) {
        match read_art(props.Thumbnail().ok()) {
            Some(bytes) => {
                let h = fnv(&bytes);
                if h == art_state.hash && !new_track {
                    ArtUpdate::Unchanged
                } else {
                    art_state.hash = h;
                    match crate::wic::decode_square(&bytes, 128) {
                        Some(bmp) => ArtUpdate::New(bmp),
                        None if new_track => ArtUpdate::None,
                        None => ArtUpdate::Unchanged,
                    }
                }
            }
            None if new_track => ArtUpdate::None,
            None => ArtUpdate::Unchanged,
        }
    } else {
        ArtUpdate::Unchanged
    };

    let playing = s
        .GetPlaybackInfo()
        .ok()
        .and_then(|p| p.PlaybackStatus().ok())
        .is_some_and(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing);
    let can_seek = s
        .GetPlaybackInfo()
        .ok()
        .and_then(|p| p.Controls().ok())
        .and_then(|c| c.IsPlaybackPositionEnabled().ok())
        .unwrap_or(false);
    let (position_ms, duration_ms) = match s.GetTimelineProperties() {
        Ok(t) => {
            let ms = |r: windows::core::Result<windows::Foundation::TimeSpan>| r.map(|v| v.Duration / 10_000).unwrap_or(0);
            let (start, end, mut pos) = (ms(t.StartTime()), ms(t.EndTime()), ms(t.Position()));
            let dur = (end - start).max(0);
            // SMTC `Position()` is a checkpoint recorded at `LastUpdatedTime()`; while playing,
            // add the elapsed time since that checkpoint so refreshes never snap the timeline backward.
            if playing {
                const UNIX_EPOCH_FT_100NS: i64 = 116_444_736_000_000_000;
                if let Ok(lu) = t.LastUpdatedTime() {
                    if lu.UniversalTime > UNIX_EPOCH_FT_100NS {
                        if let Ok(since_unix) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
                            let now_ft = (since_unix.as_millis() as i64)
                                .saturating_mul(10_000)
                                .saturating_add(UNIX_EPOCH_FT_100NS);
                            let elapsed_ms = (now_ft - lu.UniversalTime) / 10_000;
                            if elapsed_ms > 0 && elapsed_ms < 4 * 3_600_000 {
                                pos = pos.saturating_add(elapsed_ms);
                            }
                        }
                    }
                }
            }
            if dur > 0 {
                pos = pos.clamp(0, dur);
            } else {
                pos = pos.max(0);
            }
            (pos, dur)
        }
        Err(_) => (0, 0),
    };
    let app = s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
    Snapshot { has_media: true, title, artist, album, playing, position_ms, duration_ms, can_seek, art, app }
}
