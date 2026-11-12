//! Login startup will be implemented with each native recorder backend.
use std::path::Path;

pub fn is_autostart_enabled() -> bool {
    false
}

pub fn set_autostart(_: &Path, enabled: bool) -> Result<(), String> {
    if enabled {
        Err("Login startup is not available in this platform preview yet.".into())
    } else {
        Ok(())
    }
}
