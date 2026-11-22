use nacre_core::symbols::{self, Sym};
use nacre_platform::text::{self, Style};
use nacre_ui::gfx::Gfx;
use std::collections::HashMap;
use windows::UI::Color;
use windows::UI::Composition::{
    CompositionBitmapInterpolationMode, CompositionClip, CompositionDrawingSurface,
    CompositionStretch, Compositor, ContainerVisual, SpriteVisual,
};
use windows_numerics::{Vector2, Vector3};

pub const WHITE: [u8; 3] = [255, 255, 255];

#[derive(Clone, Copy)]
pub enum Anchor {
    Left,
    Center,
    Right,
}

pub struct Painter {
    pub comp: Compositor,
    pub gfx: Gfx,
    pub dpi: f32,
    cache: HashMap<String, (CompositionDrawingSurface, i32, i32)>,
}

pub fn clip(comp: &Compositor, w: f32, h: f32, r: f32) -> windows_core::Result<CompositionClip> {
    let g = comp.CreateRoundedRectangleGeometry()?;
    g.SetSize(Vector2 { X: w, Y: h })?;
    g.SetCornerRadius(Vector2 { X: r, Y: r })?;
    let c = comp.CreateGeometricClip()?;
    c.SetGeometry(&g)?;
    windows_core::Interface::cast(&c)
}

fn recolor(b: &[u8], rgb: [u8; 3]) -> Vec<u8> {
    let mut out = b.to_vec();
    for px in out.chunks_exact_mut(4) {
        let a = px[3] as u32;
        px[0] = (rgb[2] as u32 * a / 255) as u8;
        px[1] = (rgb[1] as u32 * a / 255) as u8;
        px[2] = (rgb[0] as u32 * a / 255) as u8;
    }
    out
}

impl Painter {
    pub fn new(comp: Compositor, gfx: Gfx, dpi: f32) -> Painter {
        Painter { comp, gfx, dpi, cache: HashMap::new() }
    }

    pub fn reset(&mut self, dpi: f32) {
        self.dpi = dpi;
        self.cache.clear();
    }

    fn trim(&mut self) {
        if self.cache.len() > 160 {
            self.cache.clear();
        }
    }

    pub fn px(&self, dp: f32) -> f32 {
        dp * self.dpi
    }

    pub fn measure(&self, s: &str, size: f32, weight: i32) -> f32 {
        text::measure(s, &Style { px: (size * self.dpi).ceil() as i32, weight }).0 as f32
    }

    pub fn measure_dwrite(&self, s: &str, size: f32, weight: i32) -> f32 {
        text::measure_dwrite(s, (size * self.dpi).ceil(), weight).0
    }

    pub fn place(&self, parent: &ContainerVisual, s: &CompositionDrawingSurface, x: f32, y: f32, w: f32, h: f32, opacity: f32) -> Option<SpriteVisual> {
        let go = || -> windows_core::Result<SpriteVisual> {
            let brush = self.comp.CreateSurfaceBrushWithSurface(s)?;
            brush.SetStretch(CompositionStretch::None)?;
            brush.SetBitmapInterpolationMode(CompositionBitmapInterpolationMode::NearestNeighbor)?;
            let v = self.comp.CreateSpriteVisual()?;
            v.SetBrush(&brush)?;
            v.SetSize(Vector2 { X: w, Y: h })?;
            v.SetOffset(Vector3 { X: x.round(), Y: y.round(), Z: 0.0 })?;
            v.SetOpacity(opacity)?;
            parent.Children()?.InsertAtTop(&v)?;
            Ok(v)
        };
        go().ok()
    }

    pub fn render_text_surface(
        &mut self,
        s: &str,
        size: f32,
        weight: i32,
        rgb: [u8; 3],
        max_w: f32,
    ) -> Option<(CompositionDrawingSurface, i32, i32)> {
        if s.is_empty() {
            return None;
        }
        let px = (size * self.dpi).ceil().max(1.0) as i32;
        let st = Style { px, weight };
        let (tw, th) = text::measure(s, &st);
        let w = (tw + 2).min(max_w.ceil().max(1.0) as i32).max(1);
        let key = format!("t|{px}|{weight}|{rgb:?}|{w}|{s}");
        if let Some(v) = self.cache.get(&key) {
            return Some(v.clone());
        }
        let bmp = text::render(s, &st, w, th)?;
        let sf = self.gfx.surface_from_bgra(bmp.w, bmp.h, &recolor(&bmp.bgra, rgb)).ok()?;
        self.trim();
        let v = (sf, bmp.w as i32, bmp.h as i32);
        self.cache.insert(key, v.clone());
        Some(v)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ambient_glow(
        &mut self,
        parent: &ContainerVisual,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        rgb: [u8; 3],
        compact: bool,
    ) -> Option<SpriteVisual> {
        let key = format!("glow5|{compact}|{rgb:?}");
        let surf = match self.cache.get(&key) {
            Some((s, _, _)) => s.clone(),
            None => {
                let (gw, gh): (u32, u32) = if compact { (160, 64) } else { (256, 128) };
                let mut bgra = vec![0u8; (gw * gh * 4) as usize];
                // 2x2 Bayer matrix in [-0.375, +0.375] LSB to eliminate 8-bit gradient banding on dark OLED washes.
                const BAYER2: [[f32; 2]; 2] = [[-0.375, 0.125], [0.375, -0.125]];
                for yy in 0..gh {
                    for xx in 0..gw {
                        let nx = (xx as f32 + 0.5) / gw as f32;
                        let ny = (yy as f32 + 0.5) / gh as f32;
                        let (wash, max_a) = if compact {
                            // Full-bleed bottom-up OLED pill glow: pure #000000 across the top bezel edge,
                            // rising smoothly across the entire width all the way into both rounded bottom corners.
                            let v_ramp = ((ny - 0.14) / 0.86).clamp(0.0, 1.0);
                            let vert = v_ramp.powf(1.85);
                            let h_mod = 0.96 + 0.04 * (1.0 - (2.0 * nx - 1.0).powi(2));
                            (0.48 * vert * h_mod, 0.50)
                        } else {
                            // Bottom-up OLED atmospheric wash: pure #000000 across the top ~28% bezel/ears,
                            // smoothly blooming into rich album-art warmth along the entire bottom curve.
                            let v_ramp = ((ny - 0.26) / 0.74).clamp(0.0, 1.0);
                            let vert = v_ramp.powf(2.05);
                            let h_mod = 0.80
                                + 0.20 * (-((nx - 0.22) / 0.30).powi(2)).exp()
                                + 0.13 * (-((nx - 0.86) / 0.28).powi(2)).exp();
                            (0.44 * vert * h_mod, 0.48)
                        };
                        let dither = if wash > 0.001 { BAYER2[(yy & 1) as usize][(xx & 1) as usize] } else { 0.0 };
                        let a = (wash.clamp(0.0, max_a) * 255.0 + dither).round().clamp(0.0, 255.0) as u32;
                        let i = ((yy * gw + xx) * 4) as usize;
                        bgra[i] = (rgb[2] as u32 * a / 255) as u8;
                        bgra[i + 1] = (rgb[1] as u32 * a / 255) as u8;
                        bgra[i + 2] = (rgb[0] as u32 * a / 255) as u8;
                        bgra[i + 3] = a as u8;
                    }
                }
                let sf = self.gfx.surface_from_bgra(gw, gh, &bgra).ok()?;
                self.trim();
                self.cache.insert(key, (sf.clone(), gw as i32, gh as i32));
                sf
            }
        };
        let go = || -> windows_core::Result<SpriteVisual> {
            let b = self.comp.CreateSurfaceBrushWithSurface(&surf)?;
            b.SetStretch(CompositionStretch::Fill)?;
            b.SetBitmapInterpolationMode(CompositionBitmapInterpolationMode::Linear)?;
            let v = self.comp.CreateSpriteVisual()?;
            v.SetBrush(&b)?;
            v.SetSize(Vector2 { X: w, Y: h })?;
            v.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
            if radius > 0.0 {
                v.SetClip(&clip(&self.comp, w, h, radius)?)?;
            }
            parent.Children()?.InsertAtTop(&v)?;
            Ok(v)
        };
        go().ok()
    }

    /// Two-tone bottom-up OLED atmospheric wash (smoothly blends `rgb_l` on the left into `rgb_r`
    /// on the right while keeping the top bezel pure `#000000`).
    #[allow(clippy::too_many_arguments)]
    pub fn ambient_glow_duo(
        &mut self,
        parent: &ContainerVisual,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rgb_l: [u8; 3],
        rgb_r: [u8; 3],
    ) -> Option<SpriteVisual> {
        let key = format!("glowduo1|{rgb_l:?}|{rgb_r:?}");
        let surf = match self.cache.get(&key) {
            Some((s, _, _)) => s.clone(),
            None => {
                const GW: u32 = 256;
                const GH: u32 = 128;
                let mut bgra = vec![0u8; (GW * GH * 4) as usize];
                const BAYER2: [[f32; 2]; 2] = [[-0.375, 0.125], [0.375, -0.125]];
                for yy in 0..GH {
                    for xx in 0..GW {
                        let nx = (xx as f32 + 0.5) / GW as f32;
                        let ny = (yy as f32 + 0.5) / GH as f32;
                        let v_ramp = ((ny - 0.28) / 0.72).clamp(0.0, 1.0);
                        let vert = v_ramp.powf(2.05);
                        let h_mod = 0.82
                            + 0.18 * (-((nx - 0.24) / 0.34).powi(2)).exp()
                            + 0.16 * (-((nx - 0.80) / 0.32).powi(2)).exp();
                        let wash = 0.42 * vert * h_mod;
                        let mix = nx * nx * (3.0 - 2.0 * nx);
                        let rr = rgb_l[0] as f32 + (rgb_r[0] as f32 - rgb_l[0] as f32) * mix;
                        let gg = rgb_l[1] as f32 + (rgb_r[1] as f32 - rgb_l[1] as f32) * mix;
                        let bb = rgb_l[2] as f32 + (rgb_r[2] as f32 - rgb_l[2] as f32) * mix;
                        let dither = if wash > 0.001 { BAYER2[(yy & 1) as usize][(xx & 1) as usize] } else { 0.0 };
                        let a = (wash.clamp(0.0, 0.46) * 255.0 + dither).round().clamp(0.0, 255.0) as u32;
                        let i = ((yy * GW + xx) * 4) as usize;
                        bgra[i] = ((bb * a as f32) / 255.0).round().clamp(0.0, 255.0) as u8;
                        bgra[i + 1] = ((gg * a as f32) / 255.0).round().clamp(0.0, 255.0) as u8;
                        bgra[i + 2] = ((rr * a as f32) / 255.0).round().clamp(0.0, 255.0) as u8;
                        bgra[i + 3] = a as u8;
                    }
                }
                let sf = self.gfx.surface_from_bgra(GW, GH, &bgra).ok()?;
                self.trim();
                self.cache.insert(key, (sf.clone(), GW as i32, GH as i32));
                sf
            }
        };
        let go = || -> windows_core::Result<SpriteVisual> {
            let b = self.comp.CreateSurfaceBrushWithSurface(&surf)?;
            b.SetStretch(CompositionStretch::Fill)?;
            b.SetBitmapInterpolationMode(CompositionBitmapInterpolationMode::Linear)?;
            let v = self.comp.CreateSpriteVisual()?;
            v.SetBrush(&b)?;
            v.SetSize(Vector2 { X: w, Y: h })?;
            v.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
            parent.Children()?.InsertAtTop(&v)?;
            Ok(v)
        };
        go().ok()
    }

    pub fn divider_v(&self, parent: &ContainerVisual, x: f32, y: f32, h: f32) {
        let _ = self.rect(parent, x.round(), y.round(), 1.0, h.round().max(1.0), 0.0, white(26));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn dashed_rect(&self, parent: &ContainerVisual, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
        let go = || -> windows_core::Result<()> {
            let stroke = (1.25 * self.dpi).max(1.0);
            let geo = self.comp.CreateRoundedRectangleGeometry()?;
            geo.SetOffset(Vector2 { X: stroke / 2.0, Y: stroke / 2.0 })?;
            geo.SetSize(Vector2 { X: (w - stroke).max(1.0), Y: (h - stroke).max(1.0) })?;
            geo.SetCornerRadius(Vector2 { X: radius, Y: radius })?;
            let shape = self.comp.CreateSpriteShapeWithGeometry(&geo)?;
            shape.SetStrokeBrush(&self.comp.CreateColorBrushWithColor(color)?)?;
            shape.SetStrokeThickness(stroke)?;
            let dashes = shape.StrokeDashArray()?;
            dashes.Append(5.0 * self.dpi)?;
            dashes.Append(4.0 * self.dpi)?;
            let vis = self.comp.CreateShapeVisual()?;
            vis.SetOffset(Vector3 { X: x.round(), Y: y.round(), Z: 0.0 })?;
            vis.SetSize(Vector2 { X: w.ceil(), Y: h.ceil() })?;
            vis.Shapes()?.Append(&shape)?;
            parent.Children()?.InsertAtTop(&vis)?;
            Ok(())
        };
        let _ = go();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text(&mut self, parent: &ContainerVisual, s: &str, x: f32, y: f32, size: f32, weight: i32, rgb: [u8; 3], opacity: f32, anchor: Anchor, max_w: f32) -> f32 {
        self.text_v(parent, s, x, y, size, weight, rgb, opacity, anchor, max_w).0
    }

    #[allow(clippy::too_many_arguments)]
    pub fn text_v(&mut self, parent: &ContainerVisual, s: &str, x: f32, y: f32, size: f32, weight: i32, rgb: [u8; 3], opacity: f32, anchor: Anchor, max_w: f32) -> (f32, Option<SpriteVisual>) {
        if s.is_empty() {
            return (0.0, None);
        }
        let px = (size * self.dpi).ceil().max(1.0) as i32;
        let st = Style { px, weight };
        let (tw, th) = text::measure(s, &st);
        let w = (tw + 2).min(max_w.ceil().max(1.0) as i32).max(1);
        let key = format!("t|{px}|{weight}|{rgb:?}|{w}|{s}");
        let hit = self.cache.get(&key).cloned();
        let (surf, w, h) = match hit {
            Some(v) => v,
            None => {
                let Some(bmp) = text::render(s, &st, w, th) else { return (0.0, None) };
                let Ok(sf) = self.gfx.surface_from_bgra(bmp.w, bmp.h, &recolor(&bmp.bgra, rgb)) else { return (0.0, None) };
                self.trim();
                let v = (sf, bmp.w as i32, bmp.h as i32);
                self.cache.insert(key, v.clone());
                v
            }
        };
        let left = match anchor {
            Anchor::Left => x,
            Anchor::Center => x - w as f32 / 2.0,
            Anchor::Right => x - w as f32,
        };
        let v = self.place(parent, &surf, left, y, w as f32, h as f32, opacity);
        (w as f32, v)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sym(&mut self, parent: &ContainerVisual, s: Sym, cx: f32, cy: f32, size: f32, rgb: [u8; 3], opacity: f32) {
        let n = (size * self.dpi).round().max(8.0) as u32;
        let key = format!("s|{s:?}|{n}|{rgb:?}");
        let hit = self.cache.get(&key).cloned();
        let (surf, w, h) = match hit {
            Some(v) => v,
            None => {
                let data = symbols::render(s, n, rgb);
                let Some((cd, cw, ch)) = symbols::crop_ink(&data, n, n) else { return };
                let Ok(sf) = self.gfx.surface_from_bgra(cw, ch, &cd) else { return };
                self.trim();
                let v = (sf, cw as i32, ch as i32);
                self.cache.insert(key, v.clone());
                v
            }
        };
        self.place(parent, &surf, cx - w as f32 / 2.0, cy - h as f32 / 2.0, w as f32, h as f32, opacity);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn arc(&mut self, parent: &ContainerVisual, cx: f32, cy: f32, r: f32, w: f32, from: f32, to: f32, rgb: [u8; 3], alpha: u8) -> Option<(SpriteVisual, f32)> {
        let (rp, wp) = (r * self.dpi, w * self.dpi);
        let n = ((rp + wp) * 2.0).ceil() as u32 + 2;
        let key = format!("a|{n}|{rp:.1}|{wp:.1}|{from:.3}|{to:.3}|{rgb:?}|{alpha}");
        let hit = self.cache.get(&key).cloned();
        let surf = match hit {
            Some((s, _, _)) => s,
            None => {
                let data = nacre_core::notch::download::arc_bitmap(n, rp, wp, from, to, rgb, alpha);
                let sf = self.gfx.surface_from_bgra(n, n, &data).ok()?;
                self.trim();
                self.cache.insert(key, (sf.clone(), n as i32, n as i32));
                sf
            }
        };
        let half = n as f32 / 2.0;
        self.place(parent, &surf, cx - half, cy - half, n as f32, n as f32, 1.0).map(|v| (v, n as f32))
    }

    pub fn rect(&self, parent: &ContainerVisual, x: f32, y: f32, w: f32, h: f32, radius: f32, c: Color) -> Option<SpriteVisual> {
        let go = || -> windows_core::Result<SpriteVisual> {
            let v = self.comp.CreateSpriteVisual()?;
            v.SetSize(Vector2 { X: w, Y: h })?;
            v.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
            v.SetBrush(&self.comp.CreateColorBrushWithColor(c)?)?;
            if radius > 0.0 {
                v.SetClip(&clip(&self.comp, w, h, radius)?)?;
            }
            parent.Children()?.InsertAtTop(&v)?;
            Ok(v)
        };
        go().ok()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn gradient_bar(
        &self,
        parent: &ContainerVisual,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        c0: [u8; 3],
        c1: [u8; 3],
    ) -> Option<SpriteVisual> {
        let go = || -> windows_core::Result<SpriteVisual> {
            let g = self.comp.CreateLinearGradientBrush()?;
            g.SetStartPoint(Vector2 { X: 0.0, Y: 0.5 })?;
            g.SetEndPoint(Vector2 { X: 1.0, Y: 0.5 })?;
            let stops = g.ColorStops()?;
            stops.Append(&self.comp.CreateColorGradientStopWithOffsetAndColor(
                0.0,
                Color { A: 255, R: c0[0], G: c0[1], B: c0[2] },
            )?)?;
            stops.Append(&self.comp.CreateColorGradientStopWithOffsetAndColor(
                1.0,
                Color { A: 255, R: c1[0], G: c1[1], B: c1[2] },
            )?)?;
            let v = self.comp.CreateSpriteVisual()?;
            v.SetSize(Vector2 { X: w, Y: h })?;
            v.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
            v.SetBrush(&g)?;
            if radius > 0.0 {
                v.SetClip(&clip(&self.comp, w, h, radius)?)?;
            }
            parent.Children()?.InsertAtTop(&v)?;
            Ok(v)
        };
        go().ok()
    }

    /// Renders the high-DPI anti-aliased solar prayer arc with luminous elapsed curtain,
    /// 5 milestone dots, and a softly glowing sun orb at `arc_progress`.
    #[allow(clippy::too_many_arguments)]
    pub fn prayer_arc(
        &mut self,
        parent: &ContainerVisual,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        arc_progress: f32,
        dot_us: [f32; 5],
        accent: [u8; 3],
    ) -> Option<SpriteVisual> {
        let pw = w.ceil().max(16.0) as u32;
        let ph = h.ceil().max(16.0) as u32;
        let prog_q = (arc_progress.clamp(0.0, 1.0) * 200.0).round() as i32;
        let dots_q: [i32; 5] = dot_us.map(|u| (u.clamp(0.0, 1.0) * 100.0).round() as i32);
        let key = format!("parc1|{pw}|{ph}|{prog_q}|{dots_q:?}|{accent:?}");
        let surf = match self.cache.get(&key) {
            Some((s, _, _)) => s.clone(),
            None => {
                let mut bgra = vec![0u8; (pw * ph * 4) as usize];
                let d = self.dpi;
                let pad_x = 11.0 * d;
                let span_x = (pw as f32 - 2.0 * pad_x).max(8.0);
                let y_base = ph as f32 - 6.0 * d;
                let y_peak = 16.0 * d;
                let arc_h = (y_base - y_peak).max(12.0);

                let arc_y = |u: f32| y_base - arc_h * (4.0 * u * (1.0 - u));
                let arc_dy_dx = |u: f32| -arc_h * (4.0 - 8.0 * u) / span_x;

                let orb_u = arc_progress.clamp(0.0, 1.0);
                let orb_x = pad_x + orb_u * span_x;
                let orb_y = arc_y(orb_u);

                // Lift accent slightly for crisp luminous curve stroke.
                let hi_r = (accent[0] as f32 + (255.0 - accent[0] as f32) * 0.22).min(255.0);
                let hi_g = (accent[1] as f32 + (255.0 - accent[1] as f32) * 0.22).min(255.0);
                let hi_b = (accent[2] as f32 + (255.0 - accent[2] as f32) * 0.22).min(255.0);

                let dots_xy: [(f32, f32, f32); 5] = std::array::from_fn(|i| {
                    let u = dot_us[i].clamp(0.0, 1.0);
                    (pad_x + u * span_x, arc_y(u), u)
                });

                let smoothstep = |e0: f32, e1: f32, v: f32| {
                    let t = ((v - e0) / (e1 - e0)).clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };

                let stroke_l = 1.25 * d;
                let stroke_r = 0.95 * d;
                let dot_r = 2.6 * d;
                let orb_core_r = 3.6 * d;
                let orb_halo_r = 14.0 * d;

                for yy in 0..ph {
                    let fy = yy as f32 + 0.5;
                    for xx in 0..pw {
                        let fx = xx as f32 + 0.5;
                        let mut cr = 0.0f32;
                        let mut cg = 0.0f32;
                        let mut cb = 0.0f32;
                        let mut ca = 0.0f32;

                        let blend = |cr: &mut f32, cg: &mut f32, cb: &mut f32, ca: &mut f32, r: f32, g: f32, b: f32, a: f32| {
                            let a = a.clamp(0.0, 1.0);
                            if a <= 0.001 {
                                return;
                            }
                            let inv = 1.0 - a;
                            *cr = r * a + *cr * inv;
                            *cg = g * a + *cg * inv;
                            *cb = b * a + *cb * inv;
                            *ca = a + *ca * inv;
                        };

                        let u = (fx - pad_x) / span_x;
                        if (0.0..=1.0).contains(&u) {
                            let cy = arc_y(u);
                            // 1. Soft atmospheric luminous curtain beneath elapsed portion of the arc.
                            if fx <= orb_x + 0.5 && fy >= cy && fy <= y_base {
                                let depth = ((fy - cy) / (y_base - cy).max(1.0)).clamp(0.0, 1.0);
                                let v_fade = (1.0 - depth).powf(1.55);
                                let h_edge = smoothstep(orb_x + 0.5, orb_x - 1.5, fx);
                                let h_ramp = 0.35 + 0.65 * (u / orb_u.max(0.05)).clamp(0.0, 1.0);
                                let a = 0.40 * v_fade * h_edge * h_ramp;
                                blend(&mut cr, &mut cg, &mut cb, &mut ca, accent[0] as f32, accent[1] as f32, accent[2] as f32, a);
                            }

                            // 2. Perpendicular anti-aliased solar curve stroke.
                            let slope = arc_dy_dx(u);
                            let dist = (fy - cy).abs() / (1.0 + slope * slope).sqrt();
                            if fx <= orb_x {
                                let a = smoothstep(stroke_l + 0.9, stroke_l - 0.5, dist);
                                blend(&mut cr, &mut cg, &mut cb, &mut ca, hi_r, hi_g, hi_b, a * 0.96);
                            } else {
                                let a = smoothstep(stroke_r + 0.85, stroke_r - 0.45, dist);
                                blend(&mut cr, &mut cg, &mut cb, &mut ca, 108.0, 112.0, 120.0, a * 0.56);
                            }
                        }

                        // 3. 5 prayer milestone dots along the arc.
                        for &(dx, dy, du) in &dots_xy {
                            let dist = ((fx - dx).powi(2) + (fy - dy).powi(2)).sqrt();
                            if dist <= dot_r + 1.2 {
                                let a = smoothstep(dot_r + 0.8, dot_r - 0.5, dist);
                                if du <= orb_u + 0.005 {
                                    blend(&mut cr, &mut cg, &mut cb, &mut ca, hi_r, hi_g, hi_b, a * 0.95);
                                    let inner = smoothstep(dot_r * 0.55 + 0.5, dot_r * 0.55 - 0.4, dist);
                                    blend(&mut cr, &mut cg, &mut cb, &mut ca, 240.0, 246.0, 255.0, inner * 0.85);
                                } else {
                                    blend(&mut cr, &mut cg, &mut cb, &mut ca, 175.0, 178.0, 184.0, a * 0.72);
                                }
                            }
                        }

                        // 4. Glowing solar orb at current time-of-day position.
                        let od = ((fx - orb_x).powi(2) + (fy - orb_y).powi(2)).sqrt();
                        if od <= orb_halo_r {
                            let nt = od / orb_halo_r;
                            // Warm atmospheric outer bloom + golden inner aura
                            let halo = (-nt * nt * 4.4).exp() * 0.62;
                            let halo_r = 255.0 * 0.72 + accent[0] as f32 * 0.28;
                            let halo_g = 224.0 * 0.72 + accent[1] as f32 * 0.28;
                            let halo_b = 158.0 * 0.72 + accent[2] as f32 * 0.28;
                            blend(&mut cr, &mut cg, &mut cb, &mut ca, halo_r, halo_g, halo_b, halo);

                            let mid = (-(od / (orb_core_r * 1.9)).powi(2) * 2.2).exp() * 0.70;
                            blend(&mut cr, &mut cg, &mut cb, &mut ca, 255.0, 246.0, 220.0, mid);

                            let core = smoothstep(orb_core_r + 0.9, orb_core_r - 0.6, od);
                            blend(&mut cr, &mut cg, &mut cb, &mut ca, 255.0, 255.0, 255.0, core);
                        }

                        let idx = ((yy * pw + xx) * 4) as usize;
                        // Write premultiplied BGRA (cr/cg/cb are already alpha-blended over black).
                        bgra[idx] = cb.round().clamp(0.0, 255.0) as u8;
                        bgra[idx + 1] = cg.round().clamp(0.0, 255.0) as u8;
                        bgra[idx + 2] = cr.round().clamp(0.0, 255.0) as u8;
                        bgra[idx + 3] = (ca * 255.0).round().clamp(0.0, 255.0) as u8;
                    }
                }
                let sf = self.gfx.surface_from_bgra(pw, ph, &bgra).ok()?;
                self.trim();
                self.cache.insert(key, (sf.clone(), pw as i32, ph as i32));
                sf
            }
        };
        self.place(parent, &surf, x, y, pw as f32, ph as f32, 1.0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn image(&self, parent: &ContainerVisual, s: &CompositionDrawingSurface, x: f32, y: f32, w: f32, h: f32, radius: f32) -> Option<SpriteVisual> {
        let go = || -> windows_core::Result<SpriteVisual> {
            let b = self.comp.CreateSurfaceBrushWithSurface(s)?;
            b.SetStretch(CompositionStretch::UniformToFill)?;
            let v = self.comp.CreateSpriteVisual()?;
            v.SetBrush(&b)?;
            v.SetSize(Vector2 { X: w, Y: h })?;
            v.SetOffset(Vector3 { X: x, Y: y, Z: 0.0 })?;
            if radius > 0.0 {
                v.SetClip(&clip(&self.comp, w, h, radius)?)?;
            }
            parent.Children()?.InsertAtTop(&v)?;
            Ok(v)
        };
        go().ok()
    }
}

impl Painter {
    pub fn battery(&mut self, parent: &ContainerVisual, x: f32, cy: f32, pct: u8, charging: bool) -> f32 {
        let (bw, bh) = ((24.0 * self.dpi).round() as u32, (11.0 * self.dpi).round() as u32);
        let fill = if charging { [52, 199, 89] } else if pct <= 10 { [255, 69, 58] } else { WHITE };
        let key = format!("b|{bw}|{bh}|{pct}|{fill:?}");
        let hit = self.cache.get(&key).cloned();
        let (surf, w, h) = match hit {
            Some(v) => v,
            None => {
                let data = nacre_core::logo::render_battery(bw, bh, pct, WHITE, fill);
                let Ok(s) = self.gfx.surface_from_bgra(bw, bh, &data) else { return 0.0 };
                self.trim();
                let v = (s, bw as i32, bh as i32);
                self.cache.insert(key, v.clone());
                v
            }
        };
        self.place(parent, &surf, x, cy - h as f32 / 2.0, w as f32, h as f32, 0.95);
        w as f32
    }

    pub fn fit(&self, s: &str, size: f32, weight: i32, max: f32) -> String {
        if self.measure(s, size, weight) <= max {
            return s.to_string();
        }
        let mut chars: Vec<char> = s.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let t: String = chars.iter().collect::<String>().trim_end().to_string() + "\u{2026}";
            if self.measure(&t, size, weight) <= max {
                return t;
            }
        }
        String::new()
    }
}

pub fn white(a: u8) -> Color {
    Color { A: a, R: 255, G: 255, B: 255 }
}
