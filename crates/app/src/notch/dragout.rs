use std::ffi::c_void;
use windows::core::{implement, BOOL, HRESULT, Ref};
use windows::Win32::Foundation::{
    DATA_S_SAMEFORMATETC, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS,
    DV_E_FORMATETC, E_NOTIMPL, E_OUTOFMEMORY, POINT, S_OK,
};
use windows::Win32::System::Com::{
    IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA, DATADIR_GET,
    DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT};
use windows::Win32::System::Ole::{
    DoDragDrop, IDropSource, IDropSource_Impl, OleInitialize, CF_HDROP, DROPEFFECT,
    DROPEFFECT_COPY, DROPEFFECT_LINK,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::{SHCreateStdEnumFmtEtc, DROPFILES};

#[implement(IDataObject)]
struct FileDataObject {
    paths: Vec<String>,
}

fn hdrop_fmt() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn matches_hdrop(fmt: *const FORMATETC) -> bool {
    if fmt.is_null() {
        return false;
    }
    let f = unsafe { &*fmt };
    f.cfFormat == CF_HDROP.0
        && (f.tymed & TYMED_HGLOBAL.0 as u32) != 0
        && (f.dwAspect == DVASPECT_CONTENT.0 || f.dwAspect == 0)
}

fn build_hdrop_hglobal(paths: &[String]) -> windows_core::Result<windows::Win32::Foundation::HGLOBAL> {
    let mut wide: Vec<u16> = Vec::new();
    for p in paths {
        wide.extend(p.encode_utf16());
        wide.push(0);
    }
    wide.push(0);

    let header_sz = std::mem::size_of::<DROPFILES>();
    let total_sz = header_sz + wide.len() * 2;
    unsafe {
        let h = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, total_sz)?;
        let ptr = GlobalLock(h) as *mut u8;
        if ptr.is_null() {
            return Err(E_OUTOFMEMORY.into());
        }
        let df = DROPFILES {
            pFiles: header_sz as u32,
            pt: POINT::default(),
            fNC: BOOL(0),
            fWide: BOOL(1),
        };
        std::ptr::copy_nonoverlapping(&df as *const DROPFILES as *const u8, ptr, header_sz);
        std::ptr::copy_nonoverlapping(
            wide.as_ptr() as *const u8,
            ptr.add(header_sz),
            wide.len() * 2,
        );
        let _ = GlobalUnlock(h);
        Ok(h)
    }
}

impl IDataObject_Impl for FileDataObject_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> windows_core::Result<STGMEDIUM> {
        if !matches_hdrop(pformatetcin) {
            return Err(DV_E_FORMATETC.into());
        }
        let h = build_hdrop_hglobal(&self.paths)?;
        Ok(STGMEDIUM {
            tymed: TYMED_HGLOBAL.0 as u32,
            u: STGMEDIUM_0 { hGlobal: h },
            pUnkForRelease: std::mem::ManuallyDrop::new(None),
        })
    }

    fn GetDataHere(&self, _pformatetcin: *const FORMATETC, _pmedium: *mut STGMEDIUM) -> windows_core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn QueryGetData(&self, pformatetcin: *const FORMATETC) -> HRESULT {
        if matches_hdrop(pformatetcin) { S_OK } else { DV_E_FORMATETC }
    }

    fn GetCanonicalFormatEtc(&self, _pformatectin: *const FORMATETC, pformatetcout: *mut FORMATETC) -> HRESULT {
        if !pformatetcout.is_null() {
            unsafe { (*pformatetcout).ptd = std::ptr::null_mut() };
        }
        DATA_S_SAMEFORMATETC
    }

    fn SetData(&self, _pformatetc: *const FORMATETC, _pmedium: *const STGMEDIUM, _frelease: BOOL) -> windows_core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> windows_core::Result<IEnumFORMATETC> {
        if dwdirection != DATADIR_GET.0 as u32 {
            return Err(E_NOTIMPL.into());
        }
        let fmt = [hdrop_fmt()];
        unsafe { SHCreateStdEnumFmtEtc(&fmt) }
    }

    fn DAdvise(&self, _pformatetc: *const FORMATETC, _advf: u32, _padvsink: Ref<'_, IAdviseSink>) -> windows_core::Result<u32> {
        Err(E_NOTIMPL.into())
    }

    fn DUnadvise(&self, _dwconnection: u32) -> windows_core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EnumDAdvise(&self) -> windows_core::Result<IEnumSTATDATA> {
        Err(E_NOTIMPL.into())
    }
}

#[implement(IDropSource)]
struct SimpleDropSource;

impl IDropSource_Impl for SimpleDropSource_Impl {
    fn QueryContinueDrag(&self, fescapepressed: BOOL, grfkeystate: MODIFIERKEYS_FLAGS) -> HRESULT {
        if fescapepressed.as_bool() {
            DRAGDROP_S_CANCEL
        } else if (grfkeystate.0 & MK_LBUTTON.0) == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Runs a modal OLE drag-out operation for one or more file paths.
/// Must be called OUTSIDE any re-entrancy guard (`BusyGuard`) because OLE pumps Win32 messages.
pub fn start(paths: Vec<String>) -> bool {
    if paths.is_empty() {
        return false;
    }
    unsafe {
        let _ = OleInitialize(None);
        let data: IDataObject = FileDataObject { paths }.into();
        let src: IDropSource = SimpleDropSource.into();
        let mut effect = DROPEFFECT(0);
        let hr = DoDragDrop(&data, &src, DROPEFFECT_COPY | DROPEFFECT_LINK, &mut effect);
        let _ = std::ptr::null::<c_void>();
        hr == DRAGDROP_S_DROP && effect.0 != 0
    }
}
