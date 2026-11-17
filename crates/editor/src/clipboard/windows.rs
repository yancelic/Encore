use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Shell::DROPFILES;

const CF_HDROP: u32 = 15;

/// Put a clip file on the Windows clipboard (`CF_HDROP`) so Ctrl+V pastes the MP4 directly into Discord/Explorer.
pub fn copy_file_to_clipboard(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("clip does not exist: {}", path.display()));
    }
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // Strip `\\?\` extended-length prefix so Explorer/Discord accept the drop path cleanly.
    let s = abs.to_string_lossy();
    let clean = s.strip_prefix(r"\\?\").unwrap_or(&s);
    let mut wide: Vec<u16> = OsStr::new(clean).encode_wide().collect();
    wide.push(0); // path null-terminator
    wide.push(0); // double-null list terminator

    let header_bytes = std::mem::size_of::<DROPFILES>();
    let total_bytes = header_bytes + wide.len() * 2;
    unsafe {
        let hmem = GlobalAlloc(GMEM_MOVEABLE, total_bytes).map_err(|e| format!("GlobalAlloc failed: {e}"))?;
        let ptr = GlobalLock(hmem) as *mut u8;
        if ptr.is_null() {
            return Err("GlobalLock returned null".into());
        }
        std::ptr::write_bytes(ptr, 0, total_bytes);
        let dropfiles = DROPFILES {
            pFiles: header_bytes as u32,
            pt: Default::default(),
            fNC: false.into(),
            fWide: true.into(),
        };
        std::ptr::write(ptr as *mut DROPFILES, dropfiles);
        std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr.add(header_bytes), wide.len() * 2);
        let _ = GlobalUnlock(hmem);

        OpenClipboard(Some(HWND::default())).map_err(|e| format!("OpenClipboard failed: {e}"))?;
        let res = (|| -> Result<(), String> {
            EmptyClipboard().map_err(|e| format!("EmptyClipboard failed: {e}"))?;
            SetClipboardData(CF_HDROP, Some(HANDLE(hmem.0)))
                .map_err(|e| format!("SetClipboardData failed: {e}"))?;
            Ok(())
        })();
        let _ = CloseClipboard();
        res
    }
}
