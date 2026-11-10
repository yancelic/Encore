//! Win32 system tray icon and context menu for `encore-recorder`.
use std::io::Write;
use std::path::Path;

use encore_common::ipc::UI_WAKE_PIPE;
use encore_common::tray_icon::{render_tray_bgra_premul, TrayBadge, ICON_DIM};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject, HBITMAP};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
    ICONINFO, MF_GRAYED, MF_SEPARATOR, MF_STRING, SW_SHOWNORMAL, SetForegroundWindow,
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, TrackPopupMenu, WM_APP, WM_LBUTTONDBLCLK, WM_RBUTTONUP,
    HICON,
};
use windows::core::{HSTRING, PCWSTR, w};

pub const WM_TRAY_CALLBACK: u32 = WM_APP + 1;
pub const CMD_STATUS: usize = 1001;
pub const CMD_SAVE_CLIP: usize = 1002;
pub const CMD_OPEN_UI: usize = 1003;
pub const CMD_OPEN_FOLDER: usize = 1004;
pub const CMD_QUIT: usize = 1005;

fn encode_tip(text: &str) -> [u16; 128] {
    let mut tip = [0u16; 128];
    for (i, ch) in text.encode_utf16().take(127).enumerate() {
        tip[i] = ch;
    }
    tip
}

fn create_encore_icon(badge: TrayBadge) -> Option<HICON> {
    const W: i32 = ICON_DIM as i32;
    let bgra = render_tray_bgra_premul(badge);
    let mask = vec![0u8; (W * W / 8) as usize];
    unsafe {
        let color_bmp: HBITMAP = CreateBitmap(W, W, 1, 32, Some(bgra.as_ptr().cast()));
        let mask_bmp: HBITMAP = CreateBitmap(W, W, 1, 1, Some(mask.as_ptr().cast()));
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask_bmp,
            hbmColor: color_bmp,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color_bmp.into());
        let _ = DeleteObject(mask_bmp.into());
        icon
    }
}

pub struct TrayIcon {
    hwnd: HWND,
    icon: Option<HICON>,
    badge: TrayBadge,
    added: bool,
}

impl TrayIcon {
    pub fn new(hwnd: HWND, tooltip: &str, badge: TrayBadge, visible: bool) -> Self {
        let mut s = Self {
            hwnd,
            icon: None,
            badge,
            added: false,
        };
        if visible {
            s.add_icon(tooltip, badge);
        }
        s
    }

    fn add_icon(&mut self, tooltip: &str, badge: TrayBadge) {
        if self.added {
            return;
        }
        let icon = create_encore_icon(badge);
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_TIP | if icon.is_some() { NIF_ICON } else { Default::default() },
            uCallbackMessage: WM_TRAY_CALLBACK,
            hIcon: icon.unwrap_or_default(),
            szTip: encode_tip(tooltip),
            ..Default::default()
        };
        let added = unsafe { Shell_NotifyIconW(NIM_ADD, &nid).as_bool() };
        if let Some(old) = self.icon.replace(icon.unwrap_or_default()).filter(|_| icon.is_some()) {
            unsafe { let _ = DestroyIcon(old); }
        }
        if icon.is_none() {
            self.icon = None;
        }
        self.badge = badge;
        self.added = added;
    }

    pub fn set_visible(&mut self, visible: bool, tooltip: &str, badge: TrayBadge) {
        if visible {
            if !self.added {
                self.add_icon(tooltip, badge);
            } else {
                self.set_state(tooltip, badge);
            }
        } else if self.added {
            let nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: 1,
                ..Default::default()
            };
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            }
            self.added = false;
            if let Some(ic) = self.icon.take() {
                unsafe { let _ = DestroyIcon(ic); }
            }
        }
    }

    pub fn set_state(&mut self, tooltip: &str, badge: TrayBadge) {
        if !self.added {
            return;
        }
        let mut flags = NIF_TIP;
        let mut new_icon = None;
        if badge != self.badge || self.icon.is_none() {
            new_icon = create_encore_icon(badge);
            if new_icon.is_some() {
                flags |= NIF_ICON;
            }
        }
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: flags,
            hIcon: new_icon.or(self.icon).unwrap_or_default(),
            szTip: encode_tip(tooltip),
            ..Default::default()
        };
        unsafe {
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
        if let Some(ic) = new_icon {
            if let Some(old) = self.icon.replace(ic) {
                unsafe { let _ = DestroyIcon(old); }
            }
            self.badge = badge;
        }
    }

    pub fn handle_callback(&self, lparam: LPARAM, status_line: &str, hotkey: &str) -> Option<usize> {
        let msg = (lparam.0 as u32) & 0xffff;
        if msg == WM_LBUTTONDBLCLK {
            return Some(CMD_OPEN_UI);
        }
        if msg != WM_RBUTTONUP {
            return None;
        }
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return None };
            let status_h = HSTRING::from(status_line);
            let save_h = HSTRING::from(format!("Save Clip ({hotkey})"));
            let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, CMD_STATUS, PCWSTR(status_h.as_ptr()));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, CMD_SAVE_CLIP, PCWSTR(save_h.as_ptr()));
            let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN_UI, w!("Open Encore"));
            let _ = AppendMenuW(menu, MF_STRING, CMD_OPEN_FOLDER, w!("Open Clips Folder"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("Quit Encore Recorder"));

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(self.hwnd);
            let _ = TrackPopupMenu(menu, TPM_LEFTALIGN | TPM_BOTTOMALIGN, pt.x, pt.y, None, self.hwnd, None);
            let _ = DestroyMenu(menu);
        }
        None
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        if self.added {
            let nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: 1,
                ..Default::default()
            };
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            }
        }
        if let Some(ic) = self.icon.take() {
            unsafe {
                let _ = DestroyIcon(ic);
            }
        }
    }
}

/// Wake an already-running UI window if hidden in tray, otherwise launch the Encore UI executable
/// next to this binary, falling back to opening `clips_dir`.
pub fn launch_ui_or_folder(clips_dir: &Path) {
    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(UI_WAKE_PIPE) {
        if f.write_all(b"show\n").is_ok() {
            return;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        for base in exe.ancestors().skip(1).take(2) {
            for name in ["Encore.exe", "encore-ui.exe"] {
                let cand = base.join(name);
                if cand.exists() {
                    let h = HSTRING::from(cand.as_os_str());
                    unsafe {
                        let _ = ShellExecuteW(None, w!("open"), PCWSTR(h.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
                    }
                    return;
                }
            }
        }
    }
    open_folder(clips_dir);
}

pub fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    let h = HSTRING::from(path.as_os_str());
    unsafe {
        let _ = ShellExecuteW(None, w!("open"), PCWSTR(h.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

#[allow(dead_code)]
fn _unused_wparam(_: WPARAM) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn procedural_tray_icon_builds_cleanly() {
        for badge in [TrayBadge::Offline, TrayBadge::Ready, TrayBadge::Recording] {
            let ic = create_encore_icon(badge).expect("icon");
            unsafe {
                let _ = DestroyIcon(ic);
            }
        }
    }
}
