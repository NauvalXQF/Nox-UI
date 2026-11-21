use nacre_platform::icons::Bitmap;
use std::sync::mpsc::{channel, Receiver, Sender};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconKind {
    Shelf,
    MediaApp,
    Download,
}

pub struct IconReq {
    pub kind: IconKind,
    pub key: String,
    pub px: i32,
}

pub struct IconResp {
    pub kind: IconKind,
    pub key: String,
    pub bmp: Option<Bitmap>,
}

pub struct IconWorker {
    pub requests: Sender<IconReq>,
    pub answers: Receiver<IconResp>,
}

pub fn spawn(notify: impl Fn() + Send + 'static) -> IconWorker {
    let (req_tx, req_rx) = channel::<IconReq>();
    let (ans_tx, ans_rx) = channel::<IconResp>();
    let _ = std::thread::Builder::new().name("abra-notch-icons".into()).spawn(move || {
        let com = unsafe {
            windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_APARTMENTTHREADED,
            )
            .is_ok()
        };
        while let Ok(req) = req_rx.recv() {
            let bmp = match req.kind {
                IconKind::Shelf => nacre_platform::icons::thumbnail(&req.key, req.px)
                    .or_else(|| nacre_platform::icons::load(&req.key, req.px)),
                IconKind::MediaApp => {
                    nacre_platform::icons::load(&format!("shell:AppsFolder\\{}", req.key), req.px).or_else(|| {
                        req.key
                            .to_ascii_lowercase()
                            .ends_with(".exe")
                            .then(|| nacre_platform::wins::enumerate::exe_path_of_running(&req.key))
                            .flatten()
                            .and_then(|p| nacre_platform::icons::load(&p, req.px))
                    })
                }
                IconKind::Download => nacre_platform::icons::load(&req.key, req.px),
            };
            if ans_tx.send(IconResp { kind: req.kind, key: req.key, bmp }).is_err() {
                break;
            }
            notify();
        }
        if com {
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    });
    IconWorker { requests: req_tx, answers: ans_rx }
}
