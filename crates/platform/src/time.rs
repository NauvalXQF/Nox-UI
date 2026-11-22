use std::f32::consts::TAU;
use std::sync::OnceLock;
use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

pub fn local_now() -> Local {
    let t = unsafe { GetLocalTime() };
    Local {
        year: t.wYear as i32,
        month: t.wMonth as u32,
        day: t.wDay as u32,
        hour: t.wHour as u32,
        minute: t.wMinute as u32,
        second: t.wSecond as u32,
    }
}

/// Returns local UTC offset in minutes (e.g. `+420` for WIB / UTC+7, accounting for DST).
pub fn tz_offset_minutes() -> i32 {
    unsafe {
        let mut tzi = TIME_ZONE_INFORMATION::default();
        let state = GetTimeZoneInformation(&mut tzi);
        // GetTimeZoneInformation Bias satisfies UTC = local + bias (so local = UTC - bias).
        let bias = match state {
            2 => tzi.Bias + tzi.DaylightBias,
            1 => tzi.Bias + tzi.StandardBias,
            _ => tzi.Bias,
        };
        -bias
    }
}

pub fn ms_to_next_minute() -> u32 {
    let n = local_now();
    ((60 - n.second) * 1000).max(500)
}

pub fn chime() {
    unsafe {
        let _ = MessageBeep(MB_ICONASTERISK);
    }
}

/// Synthesizes a soft, warm bell/chime PCM WAV buffer in memory (zero external audio files).
fn synth_chime_wav(notes: &[(f32, f32, f32)]) -> Vec<u8> {
    const SR: u32 = 44_100;
    let total_s = notes
        .iter()
        .map(|&(_, start, dur)| start + dur)
        .fold(0.8f32, f32::max);
    let n_samples = (total_s * SR as f32).ceil() as usize;
    let mut mix = vec![0.0f32; n_samples];

    for &(freq, start, dur) in notes {
        let i0 = (start * SR as f32) as usize;
        let len = (dur * SR as f32) as usize;
        for k in 0..len {
            let idx = i0 + k;
            if idx >= n_samples {
                break;
            }
            let t = k as f32 / SR as f32;
            // Fast velvety attack (12ms) + smooth exponential bell decay
            let attack = (t / 0.012).min(1.0);
            let env = attack * (-t * 4.2 / dur.max(0.25)).exp();
            // Fundamental + soft warm 2nd & 3rd overtones
            let sig = (TAU * freq * t).sin()
                + 0.24 * (TAU * freq * 2.0 * t).sin() * (-t * 7.0).exp()
                + 0.08 * (TAU * freq * 3.0 * t).sin() * (-t * 11.0).exp();
            mix[idx] += sig * env * 0.28;
        }
    }

    let data_bytes = (n_samples * 2) as u32;
    let mut wav = Vec::with_capacity(44 + data_bytes as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // Mono
    wav.extend_from_slice(&SR.to_le_bytes());
    wav.extend_from_slice(&(SR * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_bytes.to_le_bytes());
    for s in mix {
        let v = (s.clamp(-0.95, 0.95) * i16::MAX as f32).round() as i16;
        wav.extend_from_slice(&v.to_le_bytes());
    }
    wav
}

static CHIME_SOFT: OnceLock<Vec<u8>> = OnceLock::new();
static CHIME_ADZAN: OnceLock<Vec<u8>> = OnceLock::new();

/// Plays a gentle harmonic chime for prayer reminders (`adzan = false`: soft 2-note reminder;
/// `adzan = true`: serene 3-note ascending chime when prayer time arrives).
pub fn prayer_chime(adzan: bool) {
    let wav = if adzan {
        CHIME_ADZAN.get_or_init(|| {
            // Calm ascending D5 -> F#5 -> A5 -> D6 bell arpeggio
            synth_chime_wav(&[
                (587.33, 0.00, 0.85),
                (739.99, 0.16, 0.85),
                (880.00, 0.32, 0.95),
                (1174.66, 0.50, 1.15),
            ])
        })
    } else {
        CHIME_SOFT.get_or_init(|| {
            // Gentle 2-note E5 -> B5 reminder bell
            synth_chime_wav(&[(659.25, 0.00, 0.70), (987.77, 0.18, 0.90)])
        })
    };
    unsafe {
        let ok = PlaySoundW(
            windows::core::PCWSTR(wav.as_ptr() as *const u16),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        )
        .as_bool();
        if !ok {
            chime();
        }
    }
}
