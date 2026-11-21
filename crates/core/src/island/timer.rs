pub const MAX_SECONDS: i64 = 180 * 60;
pub const POMO_FOCUS_SECS: i64 = 25 * 60;
pub const POMO_SHORT_BREAK_SECS: i64 = 5 * 60;
pub const POMO_LONG_BREAK_SECS: i64 = 15 * 60;
pub const POMO_SESSIONS: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PomodoroStage {
    Focus(u8),
    ShortBreak(u8),
    LongBreak,
}

impl PomodoroStage {
    pub fn duration_secs(self) -> i64 {
        match self {
            PomodoroStage::Focus(_) => POMO_FOCUS_SECS,
            PomodoroStage::ShortBreak(_) => POMO_SHORT_BREAK_SECS,
            PomodoroStage::LongBreak => POMO_LONG_BREAK_SECS,
        }
    }

    pub fn next(self) -> PomodoroStage {
        match self {
            PomodoroStage::Focus(n) if n < POMO_SESSIONS => PomodoroStage::ShortBreak(n),
            PomodoroStage::Focus(_) => PomodoroStage::LongBreak,
            PomodoroStage::ShortBreak(n) => PomodoroStage::Focus((n + 1).min(POMO_SESSIONS)),
            PomodoroStage::LongBreak => PomodoroStage::Focus(1),
        }
    }

    /// Zero-based timeline slot index (`0..=3` for Focus 1..4, `4` for Long Break).
    pub fn focus_slot(self) -> u8 {
        match self {
            PomodoroStage::Focus(n) => n.saturating_sub(1).min(POMO_SESSIONS - 1),
            PomodoroStage::ShortBreak(n) => n.saturating_sub(1).min(POMO_SESSIONS - 1),
            PomodoroStage::LongBreak => POMO_SESSIONS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Running,
    Paused,
    Finished,
}

#[derive(Clone, Copy, Debug)]
pub struct Timer {
    pub duration: i64,
    remaining_ms: i64,
    deadline_ms: i64,
    pub phase: Phase,
    pub stage: PomodoroStage,
}

impl Timer {
    pub fn new(duration_secs: i64) -> Timer {
        let d = duration_secs.clamp(1, MAX_SECONDS);
        Timer {
            duration: d,
            remaining_ms: d * 1000,
            deadline_ms: 0,
            phase: Phase::Idle,
            stage: PomodoroStage::Focus(1),
        }
    }

    pub fn set_duration(&mut self, secs: i64) {
        let stage = self.stage;
        *self = Timer::new(secs);
        self.stage = stage;
    }

    pub fn select_stage(&mut self, stage: PomodoroStage) {
        *self = Timer::new(stage.duration_secs());
        self.stage = stage;
    }

    pub fn advance_stage(&mut self) {
        let next = self.stage.next();
        self.select_stage(next);
    }

    pub fn adjust(&mut self, delta_secs: i64) {
        if matches!(self.phase, Phase::Idle | Phase::Finished) {
            self.set_duration(self.duration + delta_secs);
        }
    }

    pub fn start(&mut self, now_ms: i64) {
        match self.phase {
            Phase::Finished => {
                let stage = self.stage;
                *self = Timer::new(self.duration);
                self.stage = stage;
            }
            Phase::Running => return,
            _ => {}
        }
        self.deadline_ms = now_ms + self.remaining_ms;
        self.phase = Phase::Running;
    }

    pub fn pause(&mut self, now_ms: i64) {
        if self.phase == Phase::Running {
            self.remaining_ms = (self.deadline_ms - now_ms).max(0);
            self.phase = Phase::Paused;
        }
    }

    pub fn reset(&mut self) {
        let stage = self.stage;
        *self = Timer::new(self.duration);
        self.stage = stage;
    }

    /// Resets the active Pomodoro session; if already idle at full duration on a later stage,
    /// resets the entire round back to `Focus(1)`.
    pub fn reset_pomodoro(&mut self) {
        if self.phase == Phase::Idle
            && self.remaining_ms >= self.duration * 1000
            && self.stage != PomodoroStage::Focus(1)
        {
            self.select_stage(PomodoroStage::Focus(1));
        } else {
            self.select_stage(self.stage);
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self.phase, Phase::Running | Phase::Paused)
    }

    pub fn tick(&mut self, now_ms: i64) -> bool {
        if self.phase == Phase::Running && now_ms >= self.deadline_ms {
            self.remaining_ms = 0;
            self.phase = Phase::Finished;
            return true;
        }
        false
    }

    pub fn remaining_ms(&self, now_ms: i64) -> i64 {
        if self.phase == Phase::Running {
            (self.deadline_ms - now_ms).max(0)
        } else {
            self.remaining_ms
        }
    }

    pub fn remaining_secs(&self, now_ms: i64) -> i64 {
        (self.remaining_ms(now_ms) + 999) / 1000
    }

    pub fn mins_left(&self, now_ms: i64) -> i64 {
        let secs = self.remaining_secs(now_ms);
        if secs <= 0 { 0 } else { (secs + 59) / 60 }
    }

    pub fn progress(&self, now_ms: i64) -> f32 {
        let total = (self.duration * 1000).max(1) as f32;
        let rem = self.remaining_ms(now_ms).clamp(0, self.duration * 1000) as f32;
        (1.0 - rem / total).clamp(0.0, 1.0)
    }

    pub fn total_round_mins() -> i64 {
        (POMO_SESSIONS as i64 * POMO_FOCUS_SECS
            + (POMO_SESSIONS as i64 - 1) * POMO_SHORT_BREAK_SECS
            + POMO_LONG_BREAK_SECS)
            / 60
    }
}

pub fn format_mmss(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_down_and_finishes_once() {
        let mut t = Timer::new(10);
        t.start(1000);
        assert_eq!(t.remaining_secs(1000), 10);
        assert_eq!(t.remaining_secs(5500), 6);
        assert!(!t.tick(10_999));
        assert!(t.tick(11_000));
        assert!(!t.tick(12_000), "selesai hanya dilaporkan sekali");
        assert_eq!(t.phase, Phase::Finished);
        assert_eq!(t.remaining_secs(99_999), 0);
    }

    #[test]
    fn pause_keeps_remaining_and_resume_continues() {
        let mut t = Timer::new(60);
        t.start(0);
        t.pause(20_000);
        assert_eq!(t.remaining_secs(50_000), 40);
        t.start(50_000);
        assert_eq!(t.remaining_secs(60_000), 30);
    }

    #[test]
    fn start_after_finish_restarts_from_chosen_duration() {
        let mut t = Timer::new(5);
        t.start(0);
        t.tick(5000);
        t.start(9000);
        assert_eq!(t.phase, Phase::Running);
        assert_eq!(t.remaining_secs(9000), 5);
    }

    #[test]
    fn duration_is_clamped_and_adjust_only_when_not_running() {
        assert_eq!(Timer::new(10_000_000).duration, MAX_SECONDS);
        assert_eq!(Timer::new(-5).duration, 1);
        let mut t = Timer::new(300);
        t.adjust(60);
        assert_eq!(t.duration, 360);
        t.start(0);
        t.adjust(60);
        assert_eq!(t.duration, 360, "tidak boleh berubah saat berjalan");
    }

    #[test]
    fn formatting() {
        assert_eq!(format_mmss(65), "01:05");
        assert_eq!(format_mmss(3725), "1:02:05");
        assert_eq!(format_mmss(-3), "00:00");
    }

    #[test]
    fn pomodoro_stages_progress_and_total_round() {
        assert_eq!(Timer::total_round_mins(), 130); // 2 h 10 min
        let mut t = Timer::new(POMO_FOCUS_SECS);
        assert_eq!(t.stage, PomodoroStage::Focus(1));
        assert!(!t.is_active());
        t.start(0);
        assert!(t.is_active());
        // 31 seconds elapsed -> 24:29 left -> 25 min left
        t.pause(31_000);
        assert!(t.is_active());
        assert_eq!(format_mmss(t.remaining_secs(31_000)), "24:29");
        assert_eq!(t.mins_left(31_000), 25);
        assert!(t.progress(31_000) > 0.01 && t.progress(31_000) < 0.05);

        t.advance_stage();
        assert_eq!(t.stage, PomodoroStage::ShortBreak(1));
        assert_eq!(t.duration, POMO_SHORT_BREAK_SECS);
        t.select_stage(PomodoroStage::Focus(4));
        t.advance_stage();
        assert_eq!(t.stage, PomodoroStage::LongBreak);
        assert_eq!(t.duration, POMO_LONG_BREAK_SECS);
        t.advance_stage();
        assert_eq!(t.stage, PomodoroStage::Focus(1));
    }
}
