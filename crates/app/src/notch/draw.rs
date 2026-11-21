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
        let key = format!("glow2|{compact}|{rgb:?}");
        let surf = match self.cache.get(&key) {
            Some((s, _, _)) => s.clone(),
            None => {
                const GW: u32 = 128;
                const GH: u32 = 64;
                let mut bgra = vec![0u8; (GW * GH * 4) as usize];
                for yy in 0..GH {
                    for xx in 0..GW {
                        let nx = (xx as f32 + 0.5) / GW as f32;
                        let ny = (yy as f32 + 0.5) / GH as f32;
                        let wash = if compact {
                            // Subtle Nusa-style bottom bloom fading into pure pitch-black top bezel.
                            let v = ((ny - 0.16) / 0.84).clamp(0.0, 1.0).powf(1.9);
                            let edge = 1.0 - 0.15 * ((nx - 0.5) * 2.0).abs();
                            0.42 * v * edge
                        } else {
                            // Expanded OLED canvas glow: pitch-black top, rich warm bleed along bottom & under album art.
                            let v = ((ny - 0.22) / 0.78).clamp(0.0, 1.0).powf(2.15);
                            let dx = (nx - 0.18) * 1.1;
                            let dy = (ny - 0.88) * 1.35;
                            let art_halo = (1.0 - (dx * dx + dy * dy).sqrt().min(1.0)).powi(2);
                            let right_corner = ((nx - 0.55) / 0.45).clamp(0.0, 1.0).powi(2) * v * 0.22;
                            0.34 * v + 0.24 * art_halo + right_corner
                        };
                        let a = (wash.clamp(0.0, 0.52) * 255.0).round() as u32;
                        let i = ((yy * GW + xx) * 4) as usize;
                        bgra[i] = (rgb[2] as u32 * a / 255) as u8;
                        bgra[i + 1] = (rgb[1] as u32 * a / 255) as u8;
                        bgra[i + 2] = (rgb[0] as u32 * a / 255) as u8;
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
            if radius > 0.0 {
                v.SetClip(&clip(&self.comp, w, h, radius)?)?;
            }
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
