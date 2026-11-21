use std::io;

pub struct Credential {
    pub user: String,
    pub blob: Vec<u8>,
}

pub fn password(
    read: impl Fn(&str) -> io::Result<Option<Credential>>,
    service: &str,
    user: &str,
) -> Option<String> {
    let credential = match read(service).ok()? {
        Some(credential) if user.is_empty() || credential.user == user => credential,
        _ => read(&format!("{user}@{service}")).ok()??,
    };
    decode_password(&credential.blob)
}

fn decode_password(blob: &[u8]) -> Option<String> {
    decode_utf16le(blob).or_else(|| std::str::from_utf8(blob).ok().map(str::to_owned))
}

fn decode_utf16le(blob: &[u8]) -> Option<String> {
    let blob = blob.strip_prefix(&[0xff, 0xfe]).unwrap_or(blob);
    let (words, remainder) = blob.as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    let words: Vec<_> = words.iter().copied().map(u16::from_le_bytes).collect();
    String::from_utf16(&words).ok()
}

#[cfg(windows)]
pub fn read(target: &str) -> io::Result<Option<Credential>> {
    use std::{ptr, slice};

    use windows_sys::Win32::Foundation::ERROR_NOT_FOUND;
    use windows_sys::Win32::Security::Credentials::{CRED_TYPE_GENERIC, CredFree, CredReadW};

    let target = widget_common::utils::encode_wide(target);
    let mut credential = ptr::null_mut();
    // CredReadW returns a single allocation owning its username and blob pointers.
    unsafe {
        if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) == 0 {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(ERROR_NOT_FOUND as i32) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let value = &*credential;
        let mut user = String::new();
        if !value.UserName.is_null() {
            let len = (0..).take_while(|&i| *value.UserName.add(i) != 0).count();
            user = String::from_utf16_lossy(slice::from_raw_parts(value.UserName, len));
        }
        let blob = if value.CredentialBlobSize == 0 || value.CredentialBlob.is_null() {
            Vec::new()
        } else {
            slice::from_raw_parts(value.CredentialBlob, value.CredentialBlobSize as usize).to_vec()
        };
        CredFree(credential.cast());
        Ok(Some(Credential { user, blob }))
    }
}

#[cfg(not(windows))]
pub fn read(_target: &str) -> io::Result<Option<Credential>> {
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn credential(user: &str, blob: &str) -> Credential {
        Credential {
            user: user.to_owned(),
            blob: blob.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        }
    }

    #[test]
    fn accepts_the_service_entry_when_the_username_matches() {
        let read = |target: &str| {
            assert_eq!(target, "Claude Code");
            Ok(Some(credential("default", "fictitious-🔑")))
        };
        assert_eq!(
            password(read, "Claude Code", "default").as_deref(),
            Some("fictitious-🔑")
        );
    }

    #[test]
    fn accepts_the_service_entry_for_an_empty_username() {
        let read = |target: &str| {
            assert_eq!(target, "Claude Code");
            Ok(Some(credential("other", "fixture")))
        };
        assert_eq!(
            password(read, "Claude Code", "").as_deref(),
            Some("fixture")
        );
    }

    #[test]
    fn falls_back_to_the_compound_target_on_absence_or_user_mismatch() {
        for present in [true, false] {
            let targets = RefCell::new(Vec::new());
            let read = |target: &str| {
                targets.borrow_mut().push(target.to_owned());
                Ok(match target {
                    "Claude Code" => present.then(|| credential("other", "different")),
                    "default@Claude Code" => Some(credential("other", "fixture")),
                    _ => None,
                })
            };
            assert_eq!(
                password(read, "Claude Code", "default").as_deref(),
                Some("fixture")
            );
            assert_eq!(targets.into_inner(), ["Claude Code", "default@Claude Code"]);
        }
    }

    #[test]
    fn falls_back_to_utf8_for_invalid_utf16() {
        assert_eq!(decode_password(b"a").as_deref(), Some("a"));
        assert_eq!(decode_password("clé!".as_bytes()).as_deref(), Some("clé!"));
        assert_eq!(decode_password(&[0, 0xd8]), None);
        assert_eq!(
            decode_password(&[0xff, 0xfe, 0x61, 0]).as_deref(),
            Some("a")
        );
    }
}
