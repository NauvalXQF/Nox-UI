#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpringParams {
    pub stiffness: f32,
    pub damping: f32,
}

impl SpringParams {
    pub const fn new(stiffness: f32, damping: f32) -> Self {
        Self { stiffness, damping }
    }

    pub fn from_response(response_s: f32, damping_ratio: f32) -> Self {
        let r = response_s.max(0.05);
        let omega = std::f32::consts::TAU / r;
        Self {
            stiffness: omega * omega,
            damping: 2.0 * damping_ratio.clamp(0.1, 2.0) * omega,
        }
    }

    pub fn peek() -> Self {
        Self::from_response(0.24, 0.88)
    }

    pub fn open() -> Self {
        Self::from_response(0.34, 0.80)
    }

    pub fn close() -> Self {
        Self::from_response(0.26, 0.94)
    }

    pub fn swap() -> Self {
        Self::from_response(0.30, 0.84)
    }

    /// Satisfying slow-mo critically-damped glide for tab switching (~360-400ms settle).
    pub fn tab_glide() -> Self {
        Self::from_response(0.42, 0.96)
    }

    /// Silky horizontal glide for toggling the lyrics panel (395 <-> 530 dp, ~360ms).
    pub fn lyrics_glide() -> Self {
        Self::from_response(0.36, 0.94)
    }

    pub fn tactile() -> Self {
        Self::from_response(0.20, 0.76)
    }

    /// Critically-damped silky smooth scroll glide (~160ms settle, zero overshoot).
    pub fn smooth_scroll() -> Self {
        Self::from_response(0.16, 1.0)
    }

    pub fn stage_show() -> Self {
        Self::from_response(0.32, 0.82)
    }

    pub fn stage_hide() -> Self {
        Self::from_response(0.22, 0.98)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pub pos: f32,
    pub vel: f32,
    pub target: f32,
    pub params: SpringParams,
    pos_eps: f32,
    vel_eps: f32,
}

impl Spring {
    /// Pixel-space spring (`pos_eps = 0.15 px`, `vel_eps = 0.5 px/s`).
    pub fn new(initial: f32, params: SpringParams) -> Self {
        Self {
            pos: initial,
            vel: 0.0,
            target: initial,
            params,
            pos_eps: 0.15,
            vel_eps: 0.5,
        }
    }

    /// Normalized `0.0..1.0` spring with fine tolerance (`0.002`) so animations never snap early.
    pub fn new_unit(initial: f32, params: SpringParams) -> Self {
        Self {
            pos: initial,
            vel: 0.0,
            target: initial,
            params,
            pos_eps: 0.002,
            vel_eps: 0.01,
        }
    }

    pub fn snap(&mut self, value: f32) {
        self.pos = value;
        self.target = value;
        self.vel = 0.0;
    }

    pub fn set_target(&mut self, target: f32, params: SpringParams) {
        self.target = target;
        self.params = params;
    }

    pub fn is_settled(&self) -> bool {
        (self.pos - self.target).abs() <= self.pos_eps && self.vel.abs() <= self.vel_eps
    }

    pub fn step(&mut self, dt_s: f32) -> bool {
        if self.is_settled() {
            self.pos = self.target;
            self.vel = 0.0;
            return false;
        }
        let dt = dt_s.clamp(0.001, 0.033);
        const SUBSTEPS: usize = 4;
        let h = dt / SUBSTEPS as f32;
        let k = self.params.stiffness;
        let c = self.params.damping;
        for _ in 0..SUBSTEPS {
            let disp = self.pos - self.target;
            let acc = -k * disp - c * self.vel;
            self.vel += acc * h;
            self.pos += self.vel * h;
        }
        if self.is_settled() {
            self.pos = self.target;
            self.vel = 0.0;
            false
        } else {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_converges_consistently_at_60hz_and_120hz() {
        let mut s60 = Spring::new(184.0, SpringParams::open());
        let mut s120 = Spring::new(184.0, SpringParams::open());
        s60.set_target(600.0, SpringParams::open());
        s120.set_target(600.0, SpringParams::open());
        for _ in 0..12 {
            s60.step(1.0 / 60.0);
        }
        for _ in 0..24 {
            s120.step(1.0 / 120.0);
        }
        assert!((s60.pos - s120.pos).abs() < 4.0, "60Hz={} 120Hz={}", s60.pos, s120.pos);
        for _ in 0..120 {
            if !s120.step(1.0 / 120.0) {
                break;
            }
        }
        assert!(s120.is_settled());
        assert_eq!(s120.pos, 600.0);
        assert_eq!(s120.vel, 0.0);
    }

    #[test]
    fn open_spring_has_gentle_overshoot_and_close_is_nearly_critical() {
        let mut open = Spring::new(0.0, SpringParams::open());
        open.set_target(100.0, SpringParams::open());
        let mut peak_open = 0.0f32;
        for _ in 0..180 {
            open.step(1.0 / 120.0);
            peak_open = peak_open.max(open.pos);
        }
        assert!(peak_open > 100.5 && peak_open < 108.0, "peak_open={peak_open}");

        let mut close = Spring::new(100.0, SpringParams::close());
        close.set_target(0.0, SpringParams::close());
        let mut min_close = 100.0f32;
        for _ in 0..180 {
            close.step(1.0 / 120.0);
            min_close = min_close.min(close.pos);
        }
        assert!(min_close > -2.0, "min_close={min_close}");
    }

    #[test]
    fn unit_spring_settles_smoothly_without_early_snap() {
        let mut s = Spring::new_unit(0.0, SpringParams::lyrics_glide());
        s.set_target(1.0, SpringParams::lyrics_glide());
        let mut last_before_settle = 0.0f32;
        for _ in 0..240 {
            let prev = s.pos;
            if !s.step(1.0 / 120.0) {
                last_before_settle = prev;
                break;
            }
        }
        assert!(s.is_settled());
        assert_eq!(s.pos, 1.0);
        assert!(last_before_settle > 0.99, "unit spring snapped too early at {last_before_settle}");
    }
}
