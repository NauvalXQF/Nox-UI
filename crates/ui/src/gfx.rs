use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;
use windows::Foundation::{Size, TypedEventHandler};
use windows::Graphics::DirectX::{DirectXAlphaMode, DirectXPixelFormat};
use windows::UI::Composition::{
    CompositionDrawingSurface, CompositionGraphicsDevice, Compositor, RenderingDeviceReplacedEventArgs,
};
use windows::Win32::Foundation::{HMODULE, POINT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BOX, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::IDXGIAdapter;
use windows::Win32::System::WinRT::Composition::{
    ICompositionDrawingSurfaceInterop, ICompositionGraphicsDeviceInterop, ICompositorInterop,
};
use windows_core::{Interface, Result};

struct GfxInner {
    comp: Compositor,
    prefer_hw: bool,
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    graphics: CompositionGraphicsDevice,
}

#[derive(Clone)]
pub struct Gfx {
    inner: Rc<RefCell<GfxInner>>,
}

static RESET_CB: Mutex<Option<fn()>> = Mutex::new(None);
static REPLACED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_on_reset(cb: fn()) {
    *RESET_CB.lock().unwrap_or_else(|e| e.into_inner()) = Some(cb);
}

fn notify_reset() {
    if let Some(cb) = *RESET_CB.lock().unwrap_or_else(|e| e.into_inner()) {
        cb();
    }
}

thread_local! {
    static SHARED: RefCell<Option<Gfx>> = const { RefCell::new(None) };
}

impl Gfx {
    pub fn shared(comp: &Compositor) -> Result<Gfx> {
        if let Some(g) = SHARED.with(|c| c.borrow().clone()) {
            return Ok(g);
        }
        let g = Self::new(comp)?;
        SHARED.with(|c| *c.borrow_mut() = Some(g.clone()));
        Ok(g)
    }

    pub fn lost(&self) -> bool {
        unsafe { self.inner.borrow().device.GetDeviceRemovedReason().is_err() }
    }

    pub fn shared_lost() -> bool {
        REPLACED.load(std::sync::atomic::Ordering::Relaxed)
            || SHARED.with(|c| c.borrow().as_ref().is_some_and(|g| g.lost()))
    }

    pub fn new(comp: &Compositor) -> Result<Gfx> {
        Self::with_hardware(comp, std::env::var_os("NACRE_GFX").is_some_and(|v| v == "hw"))
    }

    fn with_hardware(comp: &Compositor, prefer_hw: bool) -> Result<Gfx> {
        let (device, ctx) = create_best(prefer_hw)?;
        let interop: ICompositorInterop = comp.cast()?;
        let graphics = unsafe { interop.CreateGraphicsDevice(&device)? };
        let inner = Rc::new(RefCell::new(GfxInner { comp: comp.clone(), prefer_hw, device, ctx, graphics: graphics.clone() }));
        let _ = graphics.RenderingDeviceReplaced(&TypedEventHandler::<
            CompositionGraphicsDevice,
            RenderingDeviceReplacedEventArgs,
        >::new(move |_, _| {
            REPLACED.store(true, std::sync::atomic::Ordering::Relaxed);
            notify_reset();
            Ok(())
        }));
        Ok(Gfx { inner })
    }

    fn recover(&self) -> Result<()> {
        let mut g = self.inner.borrow_mut();
        let (device, ctx) = create_best(g.prefer_hw)?;
        let set_ok = g
            .graphics
            .cast::<ICompositionGraphicsDeviceInterop>()
            .and_then(|gi| unsafe { gi.SetRenderingDevice(&device) })
            .is_ok();
        if !set_ok {
            let interop: ICompositorInterop = g.comp.cast()?;
            let graphics = unsafe { interop.CreateGraphicsDevice(&device)? };
            let _ = graphics.RenderingDeviceReplaced(&TypedEventHandler::<
                CompositionGraphicsDevice,
                RenderingDeviceReplacedEventArgs,
            >::new(move |_, _| {
                REPLACED.store(true, std::sync::atomic::Ordering::Relaxed);
                notify_reset();
                Ok(())
            }));
            g.graphics = graphics;
        }
        g.device = device;
        g.ctx = ctx;
        REPLACED.store(false, std::sync::atomic::Ordering::Relaxed);
        drop(g);
        notify_reset();
        Ok(())
    }

    fn upload(&self, w: u32, h: u32, bgra: &[u8]) -> Result<CompositionDrawingSurface> {
        let g = self.inner.borrow();
        let surface = g.graphics.CreateDrawingSurface(
            Size { Width: w as f32, Height: h as f32 },
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            DirectXAlphaMode::Premultiplied,
        )?;
        let interop: ICompositionDrawingSurfaceInterop = surface.cast()?;
        unsafe {
            let mut offset = POINT::default();
            let tex: ID3D11Texture2D = interop.BeginDraw(None, &mut offset)?;
            let region = D3D11_BOX {
                left: offset.x as u32,
                top: offset.y as u32,
                front: 0,
                right: offset.x as u32 + w,
                bottom: offset.y as u32 + h,
                back: 1,
            };
            g.ctx.UpdateSubresource(&tex, 0, Some(&region), bgra.as_ptr() as *const _, w * 4, 0);
            interop.EndDraw()?;
        }
        Ok(surface)
    }

    pub fn surface_from_bgra(&self, w: u32, h: u32, bgra: &[u8]) -> Result<CompositionDrawingSurface> {
        let expected = (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4));
        if w == 0 || h == 0 || expected != Some(bgra.len()) {
            return Err(windows_core::Error::empty());
        }
        if REPLACED.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = self.recover();
        }
        match self.upload(w, h, bgra) {
            Ok(s) => Ok(s),
            Err(e) => {
                let removed = unsafe { self.inner.borrow().device.GetDeviceRemovedReason() }.is_err();
                if removed || self.recover().is_ok() {
                    if removed {
                        let _ = self.recover();
                    }
                    self.upload(w, h, bgra)
                } else {
                    Err(e)
                }
            }
        }
    }
}

fn create_best(prefer_hw: bool) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    if prefer_hw {
        create_device(D3D_DRIVER_TYPE_HARDWARE).or_else(|_| create_device(D3D_DRIVER_TYPE_WARP))
    } else {
        create_device(D3D_DRIVER_TYPE_WARP).or_else(|_| create_device(D3D_DRIVER_TYPE_HARDWARE))
    }
}

fn create_device(kind: windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut ctx = None;
    unsafe {
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            kind,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut ctx),
        )?;
    }
    Ok((device.ok_or(windows_core::Error::empty())?, ctx.ok_or(windows_core::Error::empty())?))
}
