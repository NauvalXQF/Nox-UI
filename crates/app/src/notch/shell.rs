use nacre_core::settings::Material;
use nacre_platform::log;
use nacre_ui::glass::{GlassParams, Tier};
use nacre_ui::host::{Host, Surface};
use windows::core::w;
use windows::UI::Color;
use windows::UI::Composition::{
    CompositionBrush, CompositionColorBrush, CompositionGeometricClip, CompositionPathGeometry,
    CompositionSpriteShape, CompositionVisualSurface, Compositor, ContainerVisual, DropShadow,
    ShapeVisual, SpriteVisual,
};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SetWindowPos, ShowWindow,
    HTTRANSPARENT, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE,
    WM_NCHITTEST, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows_core::Interface;
use windows_numerics::{Vector2, Vector3};

unsafe extern "system" fn shadow_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NCHITTEST {
        return LRESULT(HTTRANSPARENT as isize);
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

pub const ONYX: (u8, u8, u8) = (0, 0, 0);
const INNER_RIM_BLACK: Color = Color { A: 20, R: 255, G: 255, B: 255 }; // rgba(255,255,255,0.08)

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
        INNER_RIM_BLACK
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
    sh_hwnd: HWND,
    _sh_surface: Surface,
    sh_stage: ContainerVisual,
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
    rim_geo: CompositionPathGeometry,
    tier: Tier,
}

impl Drop for Shell {
    fn drop(&mut self) {
        if !self.sh_hwnd.is_invalid() {
            unsafe {
                let _ = DestroyWindow(self.sh_hwnd);
            }
        }
    }
}

impl Shell {
    pub fn new(
        host: &Host,
        owner_hwnd: HWND,
        root: &ContainerVisual,
        canvas: (i32, i32),
        wx: i32,
        tier: Tier,
    ) -> windows_core::Result<Shell> {
        let comp = &host.compositor;
        let sz = Vector2 { X: canvas.0 as f32, Y: canvas.1 as f32 };

        // Click-through GPU shadow HWND sharing the exact same Compositor and CompositionPathGeometry
        // so the 32dp ambient DropShadow renders completely unclipped while clicks anywhere outside
        // the interactive Visor capsule pass straight through to other processes (Chrome, VS Code, etc.).
        let sh_hwnd = unsafe {
            let hi: HINSTANCE = GetModuleHandleW(None)?.into();
            let class = w!("AbraNotchShadow");
            RegisterClassW(&WNDCLASSW {
                lpfnWndProc: Some(shadow_proc),
                hInstance: hi,
                lpszClassName: class,
                ..Default::default()
            });
            let h = CreateWindowExW(
                WS_EX_NOREDIRECTIONBITMAP
                    | WS_EX_TOPMOST
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT
                    | WS_EX_LAYERED,
                class,
                w!("abra-notch-shadow"),
                WS_POPUP,
                wx,
                0,
                canvas.0,
                canvas.1,
                None,
                None,
                Some(hi),
                None,
            )?;
            let _ = SetWindowPos(h, Some(owner_hwnd), wx, 0, canvas.0, canvas.1, SWP_NOACTIVATE);
            h
        };
        let sh_surface = host.attach(sh_hwnd, (sz.X, sz.Y))?;

        let sh_stage = comp.CreateContainerVisual()?;
        sh_stage.SetSize(sz)?;
        sh_surface.root.Children()?.InsertAtTop(&sh_stage)?;

        let stage = comp.CreateContainerVisual()?;
        stage.SetSize(sz)?;
        root.Children()?.InsertAtTop(&stage)?;

        // Shared single CompositionPathGeometry for clip, inner rim, and GPU DropShadow mask.
        let path_geo = comp.CreatePathGeometry()?;
        let shell_clip: CompositionGeometricClip = comp.CreateGeometricClip()?;
        shell_clip.SetGeometry(&path_geo)?;

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
        drop_shadow.SetBlurRadius(14.0)?;
        drop_shadow.SetOffset(Vector3 { X: 0.0, Y: 4.0, Z: 0.0 })?;
        drop_shadow.SetOpacity(0.38)?;
        drop_shadow.SetMask(&mask_brush)?;

        let shadow_host = comp.CreateSpriteVisual()?;
        shadow_host.SetSize(sz)?;
        shadow_host.SetShadow(&drop_shadow)?;
        sh_stage.Children()?.InsertAtTop(&shadow_host)?;

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

        // Subtle inner edge rim (~1px visible after shell_root geometric clip), open at top bezel.
        let rim_geo = comp.CreatePathGeometry()?;
        let rim_shape = comp.CreateSpriteShapeWithGeometry(&rim_geo)?;
        let rim_brush = comp.CreateColorBrushWithColor(INNER_RIM_BLACK)?;
        rim_shape.SetStrokeBrush(&rim_brush)?;
        rim_shape.SetStrokeThickness(1.6)?;
        let rim = comp.CreateShapeVisual()?;
        rim.SetSize(sz)?;
        rim.Shapes()?.Append(&rim_shape)?;
        shell_root.Children()?.InsertAtTop(&rim)?;

        let mut shell = Shell {
            comp: comp.clone(),
            sh_hwnd,
            _sh_surface: sh_surface,
            sh_stage,
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
            rim_geo,
            tier,
        };
        shell.apply_material(tier);
        Ok(shell)
    }

    pub fn place(&self, owner_hwnd: HWND, wx: i32, canvas: (i32, i32)) {
        let sz = Vector2 { X: canvas.0 as f32, Y: canvas.1 as f32 };
        unsafe {
            let _ = SetWindowPos(self.sh_hwnd, Some(owner_hwnd), wx, 0, canvas.0, canvas.1, SWP_NOACTIVATE);
        }
        let _ = self._sh_surface.root.SetSize(sz);
        let _ = self.sh_stage.SetSize(sz);
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

    pub fn raise_behind(&self, owner_hwnd: HWND) {
        unsafe {
            let _ = SetWindowPos(self.sh_hwnd, Some(owner_hwnd), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }

    pub fn set_visible(&self, show: bool, owner_hwnd: HWND) {
        unsafe {
            if show {
                let _ = ShowWindow(self.sh_hwnd, SW_SHOWNOACTIVATE);
                self.raise_behind(owner_hwnd);
            } else {
                let _ = ShowWindow(self.sh_hwnd, SW_HIDE);
            }
        }
    }

    pub fn apply_material(&mut self, tier: Tier) {
        self.tier = tier;
        let g = glass_params(15.0);
        let go = || -> windows_core::Result<()> {
            if is_black() {
                let b: CompositionBrush = self.comp.CreateColorBrushWithColor(g.solid)?.cast()?;
                self.backdrop.SetBrush(&b)?;
                self.tint.SetOpacity(0.0)?;
                self.rim_brush.SetColor(INNER_RIM_BLACK)?;
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
        if let Ok(rim) = nacre_ui::visor_geo::visor_rim_path(x0, 0.0, w, h, radius, ear) {
            let _ = self.rim_geo.SetPath(&rim);
        }
    }

    pub fn set_shadow(&self, blur_px: f32, dy_px: f32, opacity: f32) {
        let _ = self.drop_shadow.SetBlurRadius(blur_px.clamp(0.0, 96.0));
        let _ = self.drop_shadow.SetOffset(Vector3 { X: 0.0, Y: dy_px, Z: 0.0 });
        let _ = self.drop_shadow.SetOpacity(opacity.clamp(0.0, 1.0));
    }

    pub fn set_stage_y(&self, y: f32) {
        let off = Vector3 { X: 0.0, Y: y, Z: 0.0 };
        let _ = self.stage.SetOffset(off);
        let _ = self.sh_stage.SetOffset(off);
    }
}
