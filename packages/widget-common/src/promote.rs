use std::io;
use std::thread;
use std::time::Duration;

const ATTEMPTS: u32 = 16;
const RETRY_DELAY: Duration = Duration::from_secs(2);

/// Promotes this exe's tray icon once, on a background thread; every failure is ignored.
pub fn spawn_promotion(app_name: &'static str) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = thread::Builder::new()
        .name("tray-promotion".into())
        .spawn(move || {
            promote(
                &windows::WindowsRegistry,
                app_name,
                &exe.to_string_lossy(),
                thread::sleep,
            )
        });
}

trait Registry {
    /// `(subkey, ExecutablePath)` of every `NotifyIconSettings` entry.
    fn tray_entries(&self) -> io::Result<Vec<(String, String)>>;
    fn known_folder(&self, id: u128) -> Option<String>;
    fn promote(&self, subkey: &str) -> io::Result<()>;
    fn marker(&self, app_name: &str) -> Option<String>;
    fn save_marker(&self, app_name: &str, exe: &str) -> io::Result<()>;
}

fn promote(reg: &impl Registry, app_name: &str, exe: &str, mut wait: impl FnMut(Duration)) {
    let Some(target) = canonical(reg, exe) else {
        return;
    };
    if reg
        .marker(app_name)
        .is_some_and(|marker| canonical(reg, &marker).as_deref() == Some(&target))
    {
        return;
    }
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            wait(RETRY_DELAY);
        }
        if try_promote(reg, app_name, exe, &target) {
            return;
        }
    }
}

/// Returns `true` once the tray entry exists, whatever happens next.
fn try_promote(reg: &impl Registry, app_name: &str, exe: &str, target: &str) -> bool {
    let Ok(entries) = reg.tray_entries() else {
        return false;
    };
    let keys: Vec<&String> = entries
        .iter()
        .filter(|(_, path)| canonical(reg, path).as_deref() == Some(target))
        .map(|(key, _)| key)
        .collect();
    if keys.is_empty() {
        return false;
    }
    let mut promoted = false;
    for key in keys {
        promoted |= reg.promote(key).is_ok();
    }
    if promoted {
        let _ = reg.save_marker(app_name, exe);
    }
    true
}

fn canonical(reg: &impl Registry, path: &str) -> Option<String> {
    let path = path.replace('/', "\\");
    let path = path.strip_prefix(r"\\?\").unwrap_or(&path);
    let resolved = match path.strip_prefix('{').and_then(|rest| rest.split_once('}')) {
        Some((guid, rest)) => {
            let folder = reg.known_folder(parse_guid(guid)?)?;
            if rest.starts_with('\\') {
                folder.trim_end_matches(['\\', '/']).to_owned() + rest
            } else {
                folder + rest
            }
        }
        None => path.to_owned(),
    };
    Some(resolved.to_lowercase())
}

fn parse_guid(text: &str) -> Option<u128> {
    let hex: String = text.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok()
}

mod windows {
    use crate::utils::encode_wide;
    use std::io;
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{ERROR_SUCCESS, WIN32_ERROR};
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE,
        REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegEnumKeyExW,
        RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };
    use windows_sys::Win32::UI::Shell::{KF_FLAG_DEFAULT, SHGetKnownFolderPath};
    use windows_sys::core::{GUID, PWSTR};

    const TRAY_KEY: &str = r"Control Panel\NotifyIconSettings";
    const MARKER_VALUE: &str = "PromotedExecutablePath";

    pub struct WindowsRegistry;

    impl super::Registry for WindowsRegistry {
        fn tray_entries(&self) -> io::Result<Vec<(String, String)>> {
            let root = Key::open(TRAY_KEY, KEY_READ)?;
            Ok(root
                .subkeys()
                .into_iter()
                .filter_map(|name| {
                    let path = Key::open(&format!(r"{TRAY_KEY}\{name}"), KEY_READ)
                        .and_then(|key| key.string("ExecutablePath"))
                        .ok()?;
                    Some((name, path))
                })
                .collect())
        }

        fn known_folder(&self, id: u128) -> Option<String> {
            let mut path: PWSTR = null_mut();
            let hr = unsafe {
                SHGetKnownFolderPath(
                    &GUID::from_u128(id),
                    KF_FLAG_DEFAULT as u32,
                    null_mut(),
                    &mut path,
                )
            };
            let folder = (hr >= 0 && !path.is_null()).then(|| unsafe {
                let len = (0..).take_while(|&i| *path.add(i) != 0).count();
                String::from_utf16_lossy(std::slice::from_raw_parts(path, len))
            });
            unsafe { CoTaskMemFree(path as _) };
            folder
        }

        fn promote(&self, subkey: &str) -> io::Result<()> {
            let bytes = 1u32.to_le_bytes();
            Key::open(&format!(r"{TRAY_KEY}\{subkey}"), KEY_SET_VALUE)?.set(
                "IsPromoted",
                REG_DWORD,
                &bytes,
            )
        }

        fn marker(&self, app_name: &str) -> Option<String> {
            Key::open(&format!(r"Software\{app_name}"), KEY_READ)
                .and_then(|key| key.string(MARKER_VALUE))
                .ok()
        }

        fn save_marker(&self, app_name: &str, exe: &str) -> io::Result<()> {
            let bytes: Vec<u8> = encode_wide(exe)
                .iter()
                .flat_map(|u| u.to_le_bytes())
                .collect();
            Key::create(&format!(r"Software\{app_name}"))?.set(MARKER_VALUE, REG_SZ, &bytes)
        }
    }

    struct Key(HKEY);

    impl Key {
        fn open(path: &str, access: REG_SAM_FLAGS) -> io::Result<Key> {
            let mut hkey = null_mut();
            check(unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    encode_wide(path).as_ptr(),
                    0,
                    access,
                    &mut hkey,
                )
            })?;
            Ok(Key(hkey))
        }

        fn create(path: &str) -> io::Result<Key> {
            let mut hkey = null_mut();
            check(unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    encode_wide(path).as_ptr(),
                    0,
                    null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_SET_VALUE,
                    null(),
                    &mut hkey,
                    null_mut(),
                )
            })?;
            Ok(Key(hkey))
        }

        fn subkeys(&self) -> Vec<String> {
            let mut names = Vec::new();
            let mut buf = [0u16; 256];
            for index in 0.. {
                let mut len = buf.len() as u32;
                let code = unsafe {
                    RegEnumKeyExW(
                        self.0,
                        index,
                        buf.as_mut_ptr(),
                        &mut len,
                        null(),
                        null_mut(),
                        null_mut(),
                        null_mut(),
                    )
                };
                match code {
                    ERROR_SUCCESS => names.push(String::from_utf16_lossy(&buf[..len as usize])),
                    _ => break,
                }
            }
            names
        }

        fn string(&self, name: &str) -> io::Result<String> {
            let name = encode_wide(name);
            let mut kind: REG_VALUE_TYPE = 0;
            let mut size = 0u32;
            check(unsafe {
                RegQueryValueExW(
                    self.0,
                    name.as_ptr(),
                    null(),
                    &mut kind,
                    null_mut(),
                    &mut size,
                )
            })?;
            if kind != REG_SZ {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let mut data = vec![0u16; (size as usize).div_ceil(2)];
            check(unsafe {
                RegQueryValueExW(
                    self.0,
                    name.as_ptr(),
                    null(),
                    &mut kind,
                    data.as_mut_ptr().cast(),
                    &mut size,
                )
            })?;
            data.truncate(size as usize / 2);
            let end = data.iter().position(|&u| u == 0).unwrap_or(data.len());
            Ok(String::from_utf16_lossy(&data[..end]))
        }

        fn set(&self, name: &str, kind: REG_VALUE_TYPE, bytes: &[u8]) -> io::Result<()> {
            check(unsafe {
                RegSetValueExW(
                    self.0,
                    encode_wide(name).as_ptr(),
                    0,
                    kind,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                )
            })
        }
    }

    impl Drop for Key {
        fn drop(&mut self) {
            unsafe { RegCloseKey(self.0) };
        }
    }

    fn check(code: WIN32_ERROR) -> io::Result<()> {
        match code {
            ERROR_SUCCESS => Ok(()),
            _ => Err(io::Error::from_raw_os_error(code as i32)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    use super::*;

    const APP: &str = "TestUsageWidget";
    const EXE: &str = r"C:\Users\Test\AppData\Local\TestUsageWidget\TestUsageWidget.exe";
    const LOCAL_APP_DATA: u128 = 0xF1B32785_6FBA_4FCF_9D55_7B8E7F157091;

    #[derive(Default)]
    struct FakeRegistry {
        entries: Vec<(String, String)>,
        hidden_for: Cell<u32>,
        folders: HashMap<u128, String>,
        marker: RefCell<Option<String>>,
        promoted: RefCell<Vec<String>>,
        fail_entries: bool,
        fail_promote: bool,
    }

    impl Registry for FakeRegistry {
        fn tray_entries(&self) -> io::Result<Vec<(String, String)>> {
            if self.fail_entries {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            if self.hidden_for.get() > 0 {
                self.hidden_for.set(self.hidden_for.get() - 1);
                return Ok(Vec::new());
            }
            Ok(self.entries.clone())
        }

        fn known_folder(&self, id: u128) -> Option<String> {
            self.folders.get(&id).cloned()
        }

        fn promote(&self, subkey: &str) -> io::Result<()> {
            if self.fail_promote {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            self.promoted.borrow_mut().push(subkey.to_owned());
            Ok(())
        }

        fn marker(&self, app_name: &str) -> Option<String> {
            assert_eq!(app_name, APP);
            self.marker.borrow().clone()
        }

        fn save_marker(&self, app_name: &str, exe: &str) -> io::Result<()> {
            assert_eq!(app_name, APP);
            *self.marker.borrow_mut() = Some(exe.to_owned());
            Ok(())
        }
    }

    fn registry(entries: &[(&str, &str)]) -> FakeRegistry {
        FakeRegistry {
            entries: entries
                .iter()
                .map(|(key, path)| (key.to_string(), path.to_string()))
                .collect(),
            folders: HashMap::from([(LOCAL_APP_DATA, r"C:\Users\Test\AppData\Local".into())]),
            ..Default::default()
        }
    }

    fn run(reg: &FakeRegistry, exe: &str) -> Vec<Duration> {
        let mut waits = Vec::new();
        promote(reg, APP, exe, |delay| waits.push(delay));
        waits
    }

    #[test]
    fn promotes_matching_entry_and_saves_marker() {
        let reg = registry(&[("1", r"C:\Windows\explorer.exe"), ("2", EXE)]);
        assert!(run(&reg, EXE).is_empty());
        assert_eq!(*reg.promoted.borrow(), ["2"]);
        assert_eq!(reg.marker.borrow().as_deref(), Some(EXE));
    }

    #[test]
    fn resolves_known_folder_guid_ignoring_case_and_separators() {
        let mut reg = registry(&[(
            "7",
            r"{F1B32785-6FBA-4FCF-9D55-7B8E7F157091}\TestUsageWidget\TestUsageWidget.exe",
        )]);
        reg.folders
            .get_mut(&LOCAL_APP_DATA)
            .unwrap()
            .push(char::from(92));
        run(
            &reg,
            r"\\?\c:/users/TEST/appdata/local/testusagewidget/TESTUSAGEWIDGET.EXE",
        );
        assert_eq!(*reg.promoted.borrow(), ["7"]);
    }

    #[test]
    fn unknown_guid_never_matches() {
        let reg = registry(&[(
            "7",
            r"{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}\TestUsageWidget\TestUsageWidget.exe",
        )]);
        run(&reg, EXE);
        assert!(reg.promoted.borrow().is_empty());
    }

    #[test]
    fn parses_guid_in_textual_order() {
        assert_eq!(
            parse_guid("F1B32785-6FBA-4FCF-9D55-7B8E7F157091"),
            Some(LOCAL_APP_DATA)
        );
        assert_eq!(parse_guid("not-a-guid"), None);
        assert_eq!(parse_guid("+1B32785-6FBA-4FCF-9D55-7B8E7F157091"), None);
    }

    #[test]
    fn marker_for_same_path_skips_promotion() {
        let reg = FakeRegistry {
            fail_entries: true,
            ..registry(&[("2", EXE)])
        };
        *reg.marker.borrow_mut() = Some(EXE.to_uppercase());
        assert!(run(&reg, EXE).is_empty());
        assert!(reg.promoted.borrow().is_empty());
    }

    #[test]
    fn marker_for_other_path_promotes_again() {
        let reg = registry(&[("2", EXE)]);
        *reg.marker.borrow_mut() = Some(r"D:\Old\TestUsageWidget.exe".into());
        run(&reg, EXE);
        assert_eq!(*reg.promoted.borrow(), ["2"]);
        assert_eq!(reg.marker.borrow().as_deref(), Some(EXE));
    }

    #[test]
    fn retries_until_entry_appears() {
        let reg = registry(&[("2", EXE)]);
        reg.hidden_for.set(3);
        assert_eq!(run(&reg, EXE), [RETRY_DELAY; 3]);
        assert_eq!(*reg.promoted.borrow(), ["2"]);
    }

    #[test]
    fn gives_up_after_about_thirty_seconds() {
        let reg = registry(&[("1", r"C:\Windows\explorer.exe")]);
        let waits = run(&reg, EXE);
        assert_eq!(waits.iter().sum::<Duration>(), Duration::from_secs(30));
        assert!(reg.promoted.borrow().is_empty());
        assert!(reg.marker.borrow().is_none());
    }

    #[test]
    fn registry_errors_are_silent() {
        let reg = FakeRegistry {
            fail_entries: true,
            ..registry(&[("2", EXE)])
        };
        assert_eq!(run(&reg, EXE).len() as u32, ATTEMPTS - 1);
        assert!(reg.promoted.borrow().is_empty());

        let reg = FakeRegistry {
            fail_promote: true,
            ..registry(&[("2", EXE)])
        };
        assert!(run(&reg, EXE).is_empty());
        assert!(reg.marker.borrow().is_none());
    }
}
