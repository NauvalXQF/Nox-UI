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
        Self::from_response(0.38, 0.78)
    }

    pub fn close() -> Self {
        Self::from_response(0.28, 0.94)
    }

    pub fn swap() -> Self {
        Self::from_response(0.32, 0.84)
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
}

impl Spring {
    pub fn new(initial: f32, params: SpringParams) -> Self {
        Self {
            pos: initial,
            vel: 0.0,
            target: initial,
            params,
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
        (self.pos - self.target).abs() <= 0.15 && self.vel.abs() <= 0.5
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
}
