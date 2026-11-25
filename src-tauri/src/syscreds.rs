//! Secrets the configuration file must not contain.
//!
//! Windows Credential Manager is generic enough for this: a target name, a
//! blob, and the user's own profile protects it. Writing the WebDAV password
//! into `store.json` would put it in every configuration backup — including the
//! one this feature uploads — which is exactly the leak the feature exists to
//! avoid.
//!
//! Only Windows is implemented. The app itself is Windows-only today, and a
//! silent fallback to a plaintext file elsewhere would be worse than a sentence
//! saying the platform is not supported.

use crate::error::AppResult;

/// One target per secret, so a future second secret does not collide.
pub const WEBDAV_TARGET: &str = "X-Matrix/WebDAV";

/// The blob is UTF-16 including the terminator, which is what CredWriteW
/// expects. Keeping the conversion here (not inside the Win32 branch) makes it
/// testable on every platform, and the reading side never has to guess where
/// the string ends: `CredentialBlobSize` said so.
fn encode_units(secret: &str) -> Vec<u16> {
    secret.encode_utf16().chain(Some(0)).collect()
}

/// Decode exactly the units the credential reports.
///
/// `bytes.len()` is the recorded length, so a NUL inside the password (not
/// something a person can type, but a secret could be generated) is data, not a
/// terminator. The trailing NUL is the one the writer added and is dropped.
fn decode_bytes(bytes: &[u8]) -> String {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        units.push(u16::from_le_bytes([pair[0], pair[1]]));
    }
    String::from_utf16_lossy(&units)
        .trim_end_matches('\0')
        .to_string()
}

pub fn set_secret(target: &str, secret: &str) -> AppResult<()> {
    #[cfg(windows)]
    {
        windows_set(target, secret)
    }
    #[cfg(not(windows))]
    {
        let _ = (target, secret);
        unsupported()
    }
}

pub fn get_secret(target: &str) -> AppResult<Option<String>> {
    #[cfg(windows)]
    {
        windows_get(target)
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        unsupported()
    }
}

pub fn delete_secret(target: &str) -> AppResult<()> {
    #[cfg(windows)]
    {
        windows_delete(target)
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        unsupported()
    }
}

#[cfg(not(windows))]
fn unsupported<T>() -> AppResult<T> {
    Err(crate::error::AppError::Internal(
        "只有 Windows 实现了凭据管理器".to_string(),
    ))
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::io;
    use std::ptr;

    use windows_sys::Win32::Foundation::ERROR_NOT_FOUND;
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    use super::{decode_bytes, encode_units};
    use crate::error::{AppError, AppResult};

    /// `CredReadW` hands out a buffer the caller owns until `CredFree`. The
    /// guard is what makes every early return — including the error paths —
    /// free it exactly once.
    struct Credential(*mut CREDENTIALW);

    impl Drop for Credential {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the pointer came from CredReadW and has not been
                // freed: this Drop is the only owner.
                unsafe { CredFree(self.0 as *const c_void) };
            }
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    fn write_error(action: &str, error: io::Error) -> AppError {
        AppError::Io(format!("凭据管理器{action}失败：{error}"))
    }

    fn is_not_found(error: &io::Error) -> bool {
        error.raw_os_error() == Some(ERROR_NOT_FOUND as i32)
    }

    pub(super) fn windows_set(target: &str, secret: &str) -> AppResult<()> {
        let target = wide(target);
        let units = encode_units(secret);
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_ptr() as *mut u16,
            Comment: ptr::null_mut(),
            LastWritten: Default::default(),
            // Bytes, not units: the Win32 struct is explicitly a byte count.
            CredentialBlobSize: (units.len() * 2) as u32,
            CredentialBlob: units.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: ptr::null_mut(),
            TargetAlias: ptr::null_mut(),
            UserName: ptr::null_mut(),
        };
        // SAFETY: all pointers point at live locals for the duration of the
        // call, and the struct is fully initialised.
        let ok = unsafe { CredWriteW(&credential, 0) };
        if ok == 0 {
            return Err(write_error("写入", io::Error::last_os_error()));
        }
        Ok(())
    }

    pub(super) fn windows_get(target: &str) -> AppResult<Option<String>> {
        let target = wide(target);
        let mut raw: *mut CREDENTIALW = ptr::null_mut();
        // SAFETY: `raw` is a valid out-parameter and the target is NUL-terminated.
        let ok = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if is_not_found(&error) {
                return Ok(None);
            }
            return Err(write_error("读取", error));
        }
        let credential = Credential(raw);
        if credential.0.is_null() {
            return Ok(None);
        }
        // SAFETY: the guard owns a live CREDENTIALW for as long as the borrows
        // below are used, and both sizes come from that struct.
        let (size, blob) = unsafe {
            (
                (*credential.0).CredentialBlobSize as usize,
                (*credential.0).CredentialBlob,
            )
        };
        if blob.is_null() || size == 0 {
            return Ok(None);
        }
        // SAFETY: CredentialBlob points at CredentialBlobSize bytes owned by
        // the credential, which the guard keeps alive until after the copy.
        let bytes = unsafe { std::slice::from_raw_parts(blob, size) };
        Ok(Some(decode_bytes(bytes)))
    }

    pub(super) fn windows_delete(target: &str) -> AppResult<()> {
        let target = wide(target);
        // SAFETY: the target is NUL-terminated and lives for the call.
        let ok = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if ok == 0 {
            let error = io::Error::last_os_error();
            // Deleting something that is not there already achieved the goal;
            // that is what "clear the password" means on a fresh install.
            if is_not_found(&error) {
                return Ok(());
            }
            return Err(write_error("删除", error));
        }
        Ok(())
    }
}

#[cfg(windows)]
use win::{windows_delete, windows_get, windows_set};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_is_the_one_documented_in_the_settings_page() {
        assert_eq!(WEBDAV_TARGET, "X-Matrix/WebDAV");
    }

    #[test]
    fn a_secret_round_trips_through_the_blob() {
        let units = encode_units("p@ss 密码");
        let bytes: Vec<u8> = units.iter().flat_map(|unit| unit.to_le_bytes()).collect();
        assert_eq!(decode_bytes(&bytes), "p@ss 密码");
    }

    #[test]
    fn the_written_terminator_is_not_part_of_the_secret() {
        let bytes: Vec<u8> = "pw\0".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode_bytes(&bytes), "pw");
    }

    #[test]
    fn an_empty_blob_decodes_to_an_empty_string() {
        assert_eq!(decode_bytes(&[]), "");
    }

    #[cfg(not(windows))]
    #[test]
    fn the_non_windows_branch_says_the_platform_is_unsupported() {
        let error = get_secret(WEBDAV_TARGET).unwrap_err();
        assert!(matches!(error, crate::error::AppError::Internal(_)));
        assert!(set_secret(WEBDAV_TARGET, "x").is_err());
        assert!(delete_secret(WEBDAV_TARGET).is_err());
    }
}