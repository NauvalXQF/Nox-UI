use windows::core::{HSTRING, PWSTR};
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};

pub const TARGET_GEMINI_KEY: &str = "NOX/Gemini/ApiKey";
pub const TARGET_GTASKS_TOKENS: &str = "NOX/GoogleTasks/Tokens";
pub const TARGET_GTASKS_CLIENT_ID: &str = "NOX/GoogleTasks/ClientId";
pub const TARGET_GTASKS_CLIENT_SECRET: &str = "NOX/GoogleTasks/ClientSecret";

fn legacy_target(target: &str) -> Option<String> {
    target.strip_prefix("NOX/").map(|rest| format!("ABRA/{rest}"))
}

pub fn write_secret(target: &str, secret: &str) -> bool {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        return delete_secret(target);
    }
    let mut target_w: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let mut user_w: Vec<u16> = "NOX".encode_utf16().chain(std::iter::once(0)).collect();
    let mut blob = trimmed.as_bytes().to_vec();
    let cred = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target_w.as_mut_ptr()),
        CredentialBlobSize: blob.len() as u32,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: PWSTR(user_w.as_mut_ptr()),
        ..Default::default()
    };
    unsafe { CredWriteW(&cred, 0).is_ok() }
}

fn read_secret_raw(target: &str) -> Option<String> {
    let target_h = HSTRING::from(target);
    let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        if CredReadW(&target_h, CRED_TYPE_GENERIC, Some(0), &mut ptr).is_err() || ptr.is_null() {
            return None;
        }
        let c = &*ptr;
        let out = if !c.CredentialBlob.is_null() && c.CredentialBlobSize > 0 {
            let slice = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize);
            String::from_utf8(slice.to_vec()).ok()
        } else {
            None
        };
        CredFree(ptr as *const _);
        out.filter(|s| !s.is_empty())
    }
}

pub fn read_secret(target: &str) -> Option<String> {
    if let Some(v) = read_secret_raw(target) {
        return Some(v);
    }
    if let Some(old) = legacy_target(target) {
        if let Some(v) = read_secret_raw(&old) {
            let _ = write_secret(target, &v);
            return Some(v);
        }
    }
    None
}

pub fn delete_secret(target: &str) -> bool {
    let target_h = HSTRING::from(target);
    let ok = unsafe { CredDeleteW(&target_h, CRED_TYPE_GENERIC, Some(0)).is_ok() };
    if let Some(old) = legacy_target(target) {
        let old_h = HSTRING::from(old);
        unsafe {
            let _ = CredDeleteW(&old_h, CRED_TYPE_GENERIC, Some(0));
        }
    }
    ok
}

pub fn mask_secret(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return String::new();
    }
    if chars.len() <= 8 {
        return "\u{2022}".repeat(chars.len());
    }
    let head: String = chars[..4].iter().collect();
    let tail: String = chars[chars.len() - 4..].iter().collect();
    format!("{head}\u{2022}\u{2022}\u{2022}\u{2022}{tail}")
}
