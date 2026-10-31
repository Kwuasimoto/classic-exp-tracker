//! The user's Gemini API key, kept in Windows Credential Manager (as a generic credential of
//! this Windows account, on this PC), never in the database or a file. It's visible, and can
//! be deleted, under Control Panel > Credential Manager > Windows Credentials.

use windows::Win32::Foundation::ERROR_NOT_FOUND;
use windows::Win32::Security::Credentials::{
    CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW,
};
use windows::core::{HSTRING, PWSTR};

/// The credential's name in Credential Manager.
const TARGET: &str = "Classic EXP Tracker/Gemini API key";

#[derive(Debug, thiserror::Error)]
pub enum CredentialError {
    #[error("Windows Credential Manager: {0}")]
    Windows(#[from] windows::core::Error),
    #[error("the key is longer than Credential Manager allows")]
    TooLong,
    #[error("the saved key isn't text; save it again")]
    NotText,
}

/// Save `key`, replacing any saved before.
pub fn save(key: &str) -> Result<(), CredentialError> {
    let mut blob = key.as_bytes().to_vec();
    let size = u32::try_from(blob.len()).ok().filter(|&n| n <= CRED_MAX_CREDENTIAL_BLOB_SIZE).ok_or(CredentialError::TooLong)?;
    let mut target: Vec<u16> = TARGET.encode_utf16().chain(Some(0)).collect();
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: PWSTR(target.as_mut_ptr()),
        CredentialBlobSize: size,
        CredentialBlob: blob.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..Default::default()
    };
    // SAFETY: `credential` points into `target` and `blob`, which outlive the call; CredWriteW
    // copies what it needs.
    unsafe { CredWriteW(&credential, 0) }?;
    Ok(())
}

/// The saved key, if there is one.
pub fn load() -> Result<Option<String>, CredentialError> {
    let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: on success CredReadW points `credential` at a block it allocated, freed below.
    match unsafe { CredReadW(&HSTRING::from(TARGET), CRED_TYPE_GENERIC, None, &mut credential) } {
        Ok(()) => {}
        Err(e) if e.code() == ERROR_NOT_FOUND.to_hresult() => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    // SAFETY: CredReadW succeeded, so `credential` is valid until CredFree, and its blob is
    // `CredentialBlobSize` bytes (a null blob only when that's 0).
    let bytes = unsafe {
        let c = &*credential;
        let bytes = if c.CredentialBlobSize == 0 || c.CredentialBlob.is_null() {
            Vec::new()
        } else {
            std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec()
        };
        CredFree(credential.cast());
        bytes
    };
    String::from_utf8(bytes).map(Some).map_err(|_| CredentialError::NotText)
}

/// Delete the saved key; nothing saved is fine too.
pub fn delete() -> Result<(), CredentialError> {
    // SAFETY: plain call with a valid wide string.
    match unsafe { CredDeleteW(&HSTRING::from(TARGET), CRED_TYPE_GENERIC, None) } {
        Err(e) if e.code() != ERROR_NOT_FOUND.to_hresult() => Err(e.into()),
        _ => Ok(()),
    }
}
