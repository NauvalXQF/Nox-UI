use nacre_core::settings::Material;
use nacre_platform::log;
use nacre_ui::glass::{GlassParams, Tier};
use windows::UI::Color;
use windows::UI::Composition::{
    CompositionBrush, CompositionColorBrush, CompositionGeometricClip, CompositionPathGeometry,
    CompositionSpriteShape, CompositionVisualSurface, Compositor, ContainerVisual, DropShadow,
    ShapeVisual, SpriteVisual,
};
use windows_core::Interface;
use windows_numerics::{Vector2, Vector3};

pub const ONYX: (u8, u8, u8) = (0, 0, 0);

pub fn glass_params(radius: f32) -> GlassParams {
    let look = crate::prefs::look(crate::prefs::Role::Notch);
    let mut g = crate::prefs::apply(GlassParams::dark((10.0, 10.0), radius), &look);
    if is_black() {
        g.tint = Color { A: 255, R: ONYX.0, G: ONYX.1, B: ONYX.2 };
        g.solid = Color { A: 255, R: ONYX.0, G: ONYX.1, B: ONYX.2 };
    } else {
        g.tint.A = g.tint.A.max(96);
    }
    g.rim = if is_black() {
        Color { A: 0, R: 255, G: 255, B: 255 }
    } else {
        Color { A: 48, R: 255, G: 255, B: 255 }
    };
    g.highlight = 0.0;
    g
}

pub fn is_black() -> bool {
    crate::prefs::get().notch_material == Material::Black
}

pub struct Shell {
    comp: Compositor,
    pub stage: ContainerVisual,
    shadow_host: SpriteVisual,
    drop_shadow: DropShadow,
    mask_vis: ShapeVisual,
    mask_surface: CompositionVisualSurface,
    shell_root: ContainerVisual,
    backdrop: SpriteVisual,
    tint: SpriteVisual,
    tint_brush: CompositionColorBrush,
    pub ghost: ContainerVisual,
    pub content: ContainerVisual,
    rim: ShapeVisual,
    rim_brush: CompositionColorBrush,
    path_geo: CompositionPathGeometry,
    tier: Tier,
}

impl Shell {
    pub fn new(comp: &Compositor, root: &ContainerVisual, canvas: (i32, i32), tier: Tier) -> windows_core::Result<Shell> {
        let sz = Vector2 { X: canvas.0 as f32, Y: canvas.1 as f32 };
        let stage = comp.CreateContainerVisual()?;
        stage.SetSize(sz)?;
        root.Children()?.InsertAtTop(&stage)?;

        // Shared single CompositionPathGeometry for clip, rim, and DropShadow mask.
        let path_geo = comp.CreatePathGeometry()?;
        let shell_clip: CompositionGeometricClip = comp.CreateGeometricClip()?;
        shell_clip.SetGeometry(&path_geo)?;

        // Off-tree mask visual rendered into a CompositionVisualSurface so DropShadow matches the
        // exact Visor concave-ear or Capsule silhouette on this single HWND.
        let mask_shape: CompositionSpriteShape = comp.CreateSpriteShapeWithGeometry(&path_geo)?;
        mask_shape.SetFillBrush(&comp.CreateColorBrushWithColor(Color { A: 255, R: 255, G: 255, B: 255 })?)?;
        let mask_vis = comp.CreateShapeVisual()?;
        mask_vis.SetSize(sz)?;
        mask_vis.Shapes()?.Append(&mask_shape)?;

        let mask_surface = comp.CreateVisualSurface()?;
        mask_surface.SetSourceVisual(&mask_vis)?;
        mask_surface.SetSourceSize(sz)?;
        let mask_brush = comp.CreateSurfaceBrushWithSurface(&mask_surface)?;

        let drop_shadow = comp.CreateDropShadow()?;
        drop_shadow.SetColor(Color { A: 255, R: 0, G: 0, B: 0 })?;
        drop_shadow.SetBlurRadius(18.0)?;
        drop_shadow.SetOffset(Vector3 { X: 0.0, Y: 4.0, Z: 0.0 })?;
        drop_shadow.SetOpacity(0.24)?;
        drop_shadow.SetMask(&mask_brush)?;

        let shadow_host = comp.CreateSpriteVisual()?;
        shadow_host.SetSize(sz)?;
        shadow_host.SetShadow(&drop_shadow)?;
        stage.Children()?.InsertAtTop(&shadow_host)?;

        let shell_root = comp.CreateContainerVisual()?;
        shell_root.SetSize(sz)?;
        shell_root.SetClip(&shell_clip)?;
        stage.Children()?.InsertAtTop(&shell_root)?;

        let backdrop = comp.CreateSpriteVisual()?;
        backdrop.SetSize(sz)?;
        shell_root.Children()?.InsertAtTop(&backdrop)?;

        let tint = comp.CreateSpriteVisual()?;
        tint.SetSize(sz)?;
        let tint_brush = comp.CreateColorBrushWithColor(Color { A: 255, R: ONYX.0, G: ONYX.1, B: ONYX.2 })?;
        tint.SetBrush(&tint_brush)?;
        shell_root.Children()?.InsertAtTop(&tint)?;

        let ghost = comp.CreateContainerVisual()?;
        ghost.SetSize(sz)?;
        shell_root.Children()?.InsertAtTop(&ghost)?;

        let content = comp.CreateContainerVisual()?;
        content.SetSize(sz)?;
        shell_root.Children()?.InsertAtTop(&content)?;

        let rim_shape = comp.CreateSpriteShapeWithGeometry(&path_geo)?;
        let rim_brush = comp.CreateColorBrushWithColor(Color { A: 0, R: 255, G: 255, B: 255 })?;
        rim_shape.SetStrokeBrush(&rim_brush)?;
        rim_shape.SetStrokeThickness(1.25)?;
        let rim = comp.CreateShapeVisual()?;
        rim.SetSize(sz)?;
        rim.Shapes()?.Append(&rim_shape)?;
        shell_root.Children()?.InsertAtTop(&rim)?;

        let mut shell = Shell {
            comp: comp.clone(),
            stage,
            shadow_host,
            drop_shadow,
            mask_vis,
            mask_surface,
            shell_root,
            backdrop,
            tint,
            tint_brush,
            ghost,
            content,
            rim,
            rim_brush,
            path_geo,
            tier,
        };
        shell.apply_material(tier);
        Ok(shell)
    }

    pub fn resize(&self, canvas: (i32, i32)) {
        let sz = Vector2 { X: canvas.0 as f32, Y: canvas.1 as f32 };
        let _ = self.stage.SetSize(sz);
        let _ = self.shadow_host.SetSize(sz);
        let _ = self.mask_vis.SetSize(sz);
        let _ = self.mask_surface.SetSourceSize(sz);
        let _ = self.shell_root.SetSize(sz);
        let _ = self.backdrop.SetSize(sz);
        let _ = self.tint.SetSize(sz);
        let _ = self.ghost.SetSize(sz);
        let _ = self.content.SetSize(sz);
        let _ = self.rim.SetSize(sz);
    }

    pub fn apply_material(&mut self, tier: Tier) {
        self.tier = tier;
        let g = glass_params(15.0);
        let go = || -> windows_core::Result<()> {
            if is_black() {
                let b: CompositionBrush = self.comp.CreateColorBrushWithColor(g.solid)?.cast()?;
                self.backdrop.SetBrush(&b)?;
                self.tint.SetOpacity(0.0)?;
                self.rim_brush.SetColor(Color { A: 0, R: 255, G: 255, B: 255 })?;
                return Ok(());
            }
            let base: CompositionBrush = match tier {
                Tier::Full => match nacre_ui::glass::backdrop_blur_brush(&self.comp, g.blur_sigma, g.saturation) {
                    Ok(b) => b.cast()?,
                    Err(_) => self.comp.CreateColorBrushWithColor(g.solid)?.cast()?,
                },
                Tier::Light => match nacre_ui::glass::backdrop_blur_brush(&self.comp, g.blur_sigma * 0.4, g.saturation * 0.8) {
                    Ok(b) => b.cast()?,
                    Err(_) => self.comp.CreateColorBrushWithColor(g.solid)?.cast()?,
                },
                Tier::Solid => self.comp.CreateColorBrushWithColor(g.solid)?.cast()?,
            };
            self.backdrop.SetBrush(&base)?;
            self.tint_brush.SetColor(g.tint)?;
            self.tint.SetOpacity(if tier == Tier::Solid { 0.0 } else { 1.0 })?;
            self.rim_brush.SetColor(g.rim)?;
            Ok(())
        };
        if let Err(e) = go() {
            log::line("notch", &format!("material gagal: {e}"));
        }
    }

    pub fn set_geometry(&self, x0: f32, w: f32, h: f32, radius: f32, ear: f32) {
        if let Ok(path) = nacre_ui::visor_geo::visor_path(x0, 0.0, w, h, radius, ear) {
            let _ = self.path_geo.SetPath(&path);
        }
    }

    pub fn set_shadow(&self, blur_px: f32, dy_px: f32, opacity: f32) {
        let _ = self.drop_shadow.SetBlurRadius(blur_px.clamp(0.0, 72.0));
        let _ = self.drop_shadow.SetOffset(Vector3 { X: 0.0, Y: dy_px, Z: 0.0 });
        let _ = self.drop_shadow.SetOpacity(opacity.clamp(0.0, 1.0));
    }

    pub fn set_stage_y(&self, y: f32) {
        let _ = self.stage.SetOffset(Vector3 { X: 0.0, Y: y, Z: 0.0 });
    }
}
