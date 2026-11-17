//! Windows per-user Run-key autostart management (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`).
//!
//! Uses the Win32 Registry API directly instead of spawning `reg.exe` so antivirus/EDR heuristics
//! do not flag hidden child-process execution on the `Run` persistence key.
use std::path::Path;

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

fn set_autostart_in_key(subkey: &str, value_name: &str, exe_path: &Path, enabled: bool) -> Result<(), String> {
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

        if enabled {
            let quoted = format!("\"{}\"", exe_path.display());
            let mut wide: Vec<u16> = quoted.encode_utf16().collect();
            wide.push(0);
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
