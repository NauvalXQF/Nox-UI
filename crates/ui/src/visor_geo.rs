use std::cell::RefCell;
use windows::Graphics::{IGeometrySource2D, IGeometrySource2D_Impl};
use windows::UI::Composition::CompositionPath;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1Geometry, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_SWEEP_DIRECTION_CLOCKWISE,
    D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
};
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    IGeometrySource2DInterop, IGeometrySource2DInterop_Impl,
};
use windows_core::{implement, Interface, Result};
use windows_numerics::Vector2;

thread_local! {
    static D2D_FACTORY: RefCell<Option<ID2D1Factory>> = const { RefCell::new(None) };
}

fn factory() -> Result<ID2D1Factory> {
    D2D_FACTORY.with(|slot| {
        if let Some(f) = slot.borrow().as_ref() {
            return Ok(f.clone());
        }
        let f: ID2D1Factory = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        *slot.borrow_mut() = Some(f.clone());
        Ok(f)
    })
}

#[implement(IGeometrySource2D, IGeometrySource2DInterop)]
struct PathSource {
    geo: ID2D1Geometry,
}

impl IGeometrySource2D_Impl for PathSource_Impl {}

impl IGeometrySource2DInterop_Impl for PathSource_Impl {
    fn GetGeometry(&self) -> Result<ID2D1Geometry> {
        Ok(self.geo.clone())
    }

    fn TryGetGeometryUsingFactory(&self, _factory: windows_core::Ref<'_, ID2D1Factory>) -> Result<ID2D1Geometry> {
        Ok(self.geo.clone())
    }
}

fn pt(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn arc(x: f32, y: f32, r: f32, cw: bool) -> D2D1_ARC_SEGMENT {
    D2D1_ARC_SEGMENT {
        point: pt(x, y),
        size: D2D_SIZE_F { width: r, height: r },
        rotationAngle: 0.0,
        sweepDirection: if cw {
            D2D1_SWEEP_DIRECTION_CLOCKWISE
        } else {
            D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE
        },
        arcSize: D2D1_ARC_SIZE_SMALL,
    }
}

fn bezier(c1: Vector2, c2: Vector2, end: Vector2) -> D2D1_BEZIER_SEGMENT {
    D2D1_BEZIER_SEGMENT { point1: c1, point2: c2, point3: end }
}

/// Builds a unified filled `CompositionPath` for either Visor (`ear > 0.3`: top-attached with
/// smooth continuous cubic Bézier shoulders and -1px top bezel overlap) or Capsule (`ear <= 0.3`,
/// with continuous `top_radius` from `0.0` to `radius` so Notch <-> Pill morphs seamlessly).
pub fn visor_path(x0: f32, y0: f32, w: f32, h: f32, radius: f32, ear: f32) -> Result<CompositionPath> {
    let top_r = if ear > 0.3 { 0.0 } else { radius };
    build_visor_multi(x0, y0, w, h, radius, top_r, ear, None, false)
}

/// Builds the `1px` inner-rim stroke `CompositionPath`. In Visor mode (`ear > 0.3`), the figure is
/// left **open** along the top (`y = 0`) so no horizontal seam line is ever drawn at the screen bezel.
pub fn visor_rim_path(x0: f32, y0: f32, w: f32, h: f32, radius: f32, ear: f32) -> Result<CompositionPath> {
    let top_r = if ear > 0.3 { 0.0 } else { radius };
    build_visor_multi(x0, y0, w, h, radius, top_r, ear, None, true)
}

/// Builds a filled `CompositionPath` containing the primary Visor capsule plus an optional
/// detached right-hand pill (`(tx, tw, th, tr, t_top_r, t_ear)`) in a single GPU geometry so
/// geometric clip, GPU DropShadow mask, and inner rim stay 100% synchronized.
#[allow(clippy::too_many_arguments)]
pub fn visor_path_with_detached(
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
    radius: f32,
    top_radius: f32,
    ear: f32,
    detached: Option<(f32, f32, f32, f32, f32, f32)>,
) -> Result<CompositionPath> {
    build_visor_multi(x0, y0, w, h, radius, top_radius, ear, detached, false)
}

/// Builds the `1px` inner-rim stroke `CompositionPath` for both the primary Visor capsule and an
/// optional detached right-hand pill (`(tx, tw, th, tr, t_top_r, t_ear)`).
#[allow(clippy::too_many_arguments)]
pub fn visor_rim_path_with_detached(
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
    radius: f32,
    top_radius: f32,
    ear: f32,
    detached: Option<(f32, f32, f32, f32, f32, f32)>,
) -> Result<CompositionPath> {
    build_visor_multi(x0, y0, w, h, radius, top_radius, ear, detached, true)
}

#[allow(clippy::too_many_arguments)]
unsafe fn append_visor_figure(
    sink: &windows::Win32::Graphics::Direct2D::ID2D1GeometrySink,
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
    radius: f32,
    top_radius: f32,
    ear: f32,
    open_top_rim: bool,
) {
    let w = w.max(2.0);
    let h = h.max(2.0);
    if ear > 0.25 {
        // Overlap up to 1px above y = 0 when attached to the bezel, smoothly releasing as `ear -> 0`.
        let top_y = y0 - (ear / 2.5).clamp(0.0, 1.0);
        let ew = ear.min(h * 0.56);
        let eh = (ew * 1.08).min(h * 0.56);
        let r = radius.clamp(0.0, (w * 0.5).min((h - eh).max(0.0)));

        let begin = if open_top_rim { D2D1_FIGURE_BEGIN_HOLLOW } else { D2D1_FIGURE_BEGIN_FILLED };
        sink.BeginFigure(pt(x0 - ew, top_y), begin);
        // Constant-curvature liquid shoulder (`κ ≈ 0.56` handles): tangent-horizontal at screen bezel,
        // tangent-vertical at notch wall, zero pinched elbow.
        sink.AddBezier(&bezier(
            pt(x0 - ew * 0.44, top_y),
            pt(x0, y0 + eh * 0.44),
            pt(x0, y0 + eh),
        ));
        if h - r > eh + 0.01 {
            sink.AddLine(pt(x0, y0 + h - r));
        }
        if r > 0.5 {
            sink.AddArc(&arc(x0 + r, y0 + h, r, false));
        }
        sink.AddLine(pt(x0 + w - r, y0 + h));
        if r > 0.5 {
            sink.AddArc(&arc(x0 + w, y0 + h - r, r, false));
        }
        if h - r > eh + 0.01 {
            sink.AddLine(pt(x0 + w, y0 + eh));
        }
        sink.AddBezier(&bezier(
            pt(x0 + w, y0 + eh * 0.44),
            pt(x0 + w + ew * 0.44, top_y),
            pt(x0 + w + ew, top_y),
        ));
        sink.EndFigure(if open_top_rim { D2D1_FIGURE_END_OPEN } else { D2D1_FIGURE_END_CLOSED });
    } else {
        // Continuous morph figure: bottom corners stay at `rb`, while top corners smoothly round
        // from `rt = 0.0` (seamless meeting point with `ear = 0`) up to `rt = rb` (floating Pill).
        let rb = radius.clamp(0.0, (w * 0.5).min(h * 0.5));
        let rt = top_radius.clamp(0.0, (w * 0.5).min((h - rb).max(0.0)));
        let begin = if open_top_rim { D2D1_FIGURE_BEGIN_HOLLOW } else { D2D1_FIGURE_BEGIN_FILLED };

        sink.BeginFigure(pt(x0 + rt, y0), begin);
        sink.AddLine(pt(x0 + w - rt, y0));
        if rt > 0.35 {
            sink.AddArc(&arc(x0 + w, y0 + rt, rt, true));
        }
        if h - rb > rt + 0.01 {
            sink.AddLine(pt(x0 + w, y0 + h - rb));
        }
        if rb > 0.35 {
            sink.AddArc(&arc(x0 + w - rb, y0 + h, rb, true));
        }
        sink.AddLine(pt(x0 + rb, y0 + h));
        if rb > 0.35 {
            sink.AddArc(&arc(x0, y0 + h - rb, rb, true));
        }
        if h - rb > rt + 0.01 {
            sink.AddLine(pt(x0, y0 + rt));
        }
        if rt > 0.35 {
            sink.AddArc(&arc(x0 + rt, y0, rt, true));
        }
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    }
}

#[allow(clippy::too_many_arguments)]
fn build_visor_multi(
    x0: f32,
    y0: f32,
    w: f32,
    h: f32,
    radius: f32,
    top_radius: f32,
    ear: f32,
    detached: Option<(f32, f32, f32, f32, f32, f32)>,
    open_top_rim: bool,
) -> Result<CompositionPath> {
    let fac = factory()?;
    let path = unsafe { fac.CreatePathGeometry()? };
    let sink = unsafe { path.Open()? };

    unsafe {
        append_visor_figure(&sink, x0, y0, w, h, radius, top_radius, ear, open_top_rim);
        if let Some((tx, tw, th, tr, t_top_r, t_ear)) = detached {
            if tw > 2.0 && th > 2.0 {
                append_visor_figure(&sink, tx, y0, tw, th, tr, t_top_r, t_ear, open_top_rim);
            }
        }
        sink.Close()?;
    }

    let source: IGeometrySource2D = PathSource { geo: path.cast()? }.into();
    CompositionPath::Create(&source)
}
