use std::cell::RefCell;
use windows::Graphics::{IGeometrySource2D, IGeometrySource2D_Impl};
use windows::UI::Composition::CompositionPath;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D_SIZE_F,
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

/// Builds a unified `CompositionPath` for either Visor (`ear > 0.5`: top-attached with concave ears
/// and rounded bottom corners) or Capsule (`ear <= 0.5`: floating rounded pill).
pub fn visor_path(x0: f32, y0: f32, w: f32, h: f32, radius: f32, ear: f32) -> Result<CompositionPath> {
    let w = w.max(2.0);
    let h = h.max(2.0);
    let fac = factory()?;
    let path = unsafe { fac.CreatePathGeometry()? };
    let sink = unsafe { path.Open()? };

    unsafe {
        if ear > 0.5 {
            let e = ear.min(h * 0.45);
            let r = radius.clamp(0.0, (w * 0.5).min((h - e).max(0.0)));
            sink.BeginFigure(pt(x0 - e, y0), D2D1_FIGURE_BEGIN_FILLED);
            sink.AddArc(&arc(x0, y0 + e, e, true));
            if h - r > e + 0.01 {
                sink.AddLine(pt(x0, y0 + h - r));
            }
            if r > 0.5 {
                sink.AddArc(&arc(x0 + r, y0 + h, r, false));
            }
            sink.AddLine(pt(x0 + w - r, y0 + h));
            if r > 0.5 {
                sink.AddArc(&arc(x0 + w, y0 + h - r, r, false));
            }
            if h - r > e + 0.01 {
                sink.AddLine(pt(x0 + w, y0 + e));
            }
            sink.AddArc(&arc(x0 + w + e, y0, e, true));
            sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        } else {
            let r = radius.clamp(0.0, (w * 0.5).min(h * 0.5));
            if r <= 0.5 {
                sink.BeginFigure(pt(x0, y0), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(pt(x0 + w, y0));
                sink.AddLine(pt(x0 + w, y0 + h));
                sink.AddLine(pt(x0, y0 + h));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            } else {
                sink.BeginFigure(pt(x0 + r, y0), D2D1_FIGURE_BEGIN_FILLED);
                sink.AddLine(pt(x0 + w - r, y0));
                sink.AddArc(&arc(x0 + w, y0 + r, r, true));
                sink.AddLine(pt(x0 + w, y0 + h - r));
                sink.AddArc(&arc(x0 + w - r, y0 + h, r, true));
                sink.AddLine(pt(x0 + r, y0 + h));
                sink.AddArc(&arc(x0, y0 + h - r, r, true));
                sink.AddLine(pt(x0, y0 + r));
                sink.AddArc(&arc(x0 + r, y0, r, true));
                sink.EndFigure(D2D1_FIGURE_END_CLOSED);
            }
        }
        sink.Close()?;
    }

    let source: IGeometrySource2D = PathSource { geo: path.cast()? }.into();
    CompositionPath::Create(&source)
}
