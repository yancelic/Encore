//! Windows per-user Run-key autostart management (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).
//!
//! Uses the Win32 Registry API directly instead of spawning `reg.exe` so antivirus/EDR heuristics
//! do not flag hidden child-process execution on the `Run` persistence key.
use std::path::Path;
use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Prefix};

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR};

const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "EncoreRecorder";

struct RegKeyGuard(HKEY);

impl Drop for RegKeyGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

fn win32_err(context: &str, err: WIN32_ERROR) -> String {
    format!("{context} (Win32 error {})", err.0)
}

fn startup_command(exe_path: &Path) -> Result<Vec<u16>, String> {
    if !exe_path.is_absolute() {
        return Err("Startup executable must have an absolute path".into());
    }
    let path: Vec<u16> = exe_path.as_os_str().encode_wide().collect();
    // Tauri resource paths can carry the extended-length prefix. File APIs accept
    // it, but the Windows shell does not consistently support that syntax.
    let path = match exe_path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::VerbatimDisk(_) => path[4..].to_vec(),
            Prefix::VerbatimUNC(..) => [vec![b'\\' as u16; 2], path[8..].to_vec()].concat(),
            Prefix::Disk(_) | Prefix::UNC(..) => path,
            _ => return Err("Startup executable must use a drive or UNC path".into()),
        },
        _ => return Err("Startup executable must use a drive or UNC path".into()),
    };
    if path.iter().any(|&c| c < 32 || c == b'"' as u16) {
        return Err("Startup executable path contains invalid characters".into());
    }
    // Run values support command lines of at most 260 characters, including quotes.
    if path.len() + 2 > 260 {
        return Err("Startup executable path exceeds the Windows Run-key limit".into());
    }
    Ok([vec![b'"' as u16], path, vec![b'"' as u16, 0]].concat())
}

fn set_autostart_in_key(subkey: &str, value_name: &str, exe_path: &Path, enabled: bool) -> Result<(), String> {
    let command = if enabled { Some(startup_command(exe_path)?) } else { None };
    let subkey_h = HSTRING::from(subkey);
    let value_h = HSTRING::from(value_name);

    unsafe {
        let mut raw_key = HKEY::default();
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey_h.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut raw_key,
            None,
        );
        if status.is_err() {
            return Err(win32_err("RegCreateKeyExW failed", status));
        }
        let _guard = RegKeyGuard(raw_key);

        if let Some(wide) = command {
            let bytes = std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * std::mem::size_of::<u16>());
            let st = RegSetValueExW(raw_key, PCWSTR(value_h.as_ptr()), None, REG_SZ, Some(bytes));
            if st.is_err() {
                return Err(win32_err("RegSetValueExW failed", st));
            }
            Ok(())
        } else {
            let st = RegDeleteValueW(raw_key, PCWSTR(value_h.as_ptr()));
            if st.is_ok() || st == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                Err(win32_err("RegDeleteValueW failed", st))
            }
        }
    }
}

fn is_autostart_enabled_in_key(subkey: &str, value_name: &str) -> bool {
    let subkey_h = HSTRING::from(subkey);
    let value_h = HSTRING::from(value_name);

    unsafe {
        let mut raw_key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(subkey_h.as_ptr()), None, KEY_QUERY_VALUE, &mut raw_key).is_err() {
            return false;
        }
        let _guard = RegKeyGuard(raw_key);
        let mut data_len: u32 = 0;
        RegQueryValueExW(
            raw_key,
            PCWSTR(value_h.as_ptr()),
            None,
            None,
            None,
            Some(&mut data_len),
        )
        .is_ok()
            && data_len > 2
    }
}

/// Enable or disable launching `exe_path` on Windows sign-in.
pub fn set_autostart(exe_path: &Path, enabled: bool) -> Result<(), String> {
    set_autostart_in_key(RUN_SUBKEY, VALUE_NAME, exe_path, enabled)
}

/// Check whether the per-user autostart registry entry currently exists.
pub fn is_autostart_enabled() -> bool {
    is_autostart_enabled_in_key(RUN_SUBKEY, VALUE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Registry::RegDeleteTreeW;

    #[test]
    fn startup_paths_are_shell_compatible_and_quoted() {
        for (input, expected) in [
            (r"C:\Program Files\Encore\encore-recorder.exe", r#""C:\Program Files\Encore\encore-recorder.exe""#),
            (r"\\?\C:\Users\test\Encore\encore-recorder.exe", r#""C:\Users\test\Encore\encore-recorder.exe""#),
            (r"\\?\UNC\server\share\Encore Recorder.exe", r#""\\server\share\Encore Recorder.exe""#),
            (r"\\server\share\Encore Recorder.exe", r#""\\server\share\Encore Recorder.exe""#),
            (r"C:\Kullanıcı\Oyunlar\encore-recorder.exe", r#""C:\Kullanıcı\Oyunlar\encore-recorder.exe""#),
        ] {
            let wide = startup_command(Path::new(input)).unwrap();
            assert_eq!(wide.last(), Some(&0));
            assert_eq!(String::from_utf16(&wide[..wide.len() - 1]).unwrap(), expected);
        }
        for input in ["encore-recorder.exe", "C:\\bad\npath.exe", "C:\\bad\"path.exe"] {
            assert!(startup_command(Path::new(input)).is_err());
        }
        assert!(startup_command(Path::new(&format!("C:\\{}.exe", "a".repeat(260)))).is_err());
    }

    #[test]
    fn win32_registry_autostart_roundtrips_without_spawning_reg_exe() {
        let test_subkey = format!(r"Software\EncoreAutostartTest_{}", std::process::id());
        let val_name = "EncoreTestEntry";
        let dummy_exe = Path::new(r"C:\Program Files\Encore\encore-recorder.exe");

        assert!(!is_autostart_enabled_in_key(&test_subkey, val_name));

        set_autostart_in_key(&test_subkey, val_name, dummy_exe, true).expect("enable autostart");
        assert!(is_autostart_enabled_in_key(&test_subkey, val_name));

        set_autostart_in_key(&test_subkey, val_name, dummy_exe, false).expect("disable autostart");
        assert!(!is_autostart_enabled_in_key(&test_subkey, val_name));

        // Disabling a second time when already absent should succeed cleanly.
        set_autostart_in_key(&test_subkey, val_name, dummy_exe, false).expect("idempotent disable");

        unsafe {
            let subkey_h = HSTRING::from(test_subkey.as_str());
            let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(subkey_h.as_ptr()));
        }
    }
}
