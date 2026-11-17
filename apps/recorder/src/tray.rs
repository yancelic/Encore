//! Win32 system tray icon and context menu for `encore-recorder`.
use std::path::Path;
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use encore_common::ipc;
use encore_common::tray_icon::{render_tray_bgra_premul, TrayBadge, ICON_DIM};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject, HBITMAP};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW, ShellExecuteW, NOTIFY_ICON_MESSAGE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
    ICONINFO, MF_GRAYED, MF_SEPARATOR, MF_STRING, SW_SHOWNORMAL, SetForegroundWindow,
    TPM_BOTTOMALIGN, TPM_LEFTALIGN, TrackPopupMenu, WM_APP, WM_LBUTTONDBLCLK, WM_RBUTTONUP,
    HICON, KillTimer, SetTimer,
};
use windows::core::{HSTRING, PCWSTR, w};

pub const WM_TRAY_CALLBACK: u32 = WM_APP + 1;
pub const RETRY_TIMER: usize = 1;
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
    visible: bool,
    added: bool,
    #[cfg(test)]
    notify_override: Option<fn(NOTIFY_ICON_MESSAGE, &NOTIFYICONDATAW) -> bool>,
}

impl TrayIcon {
    pub fn new(hwnd: HWND, tooltip: &str, badge: TrayBadge, visible: bool) -> Self {
        let mut s = Self {
            hwnd,
            icon: None,
            badge,
            visible,
            added: false,
            #[cfg(test)]
            notify_override: None,
        };
        if visible {
            s.add_icon(tooltip, badge);
        }
        s
    }

    fn notify(&self, message: NOTIFY_ICON_MESSAGE, data: &NOTIFYICONDATAW) -> bool {
        #[cfg(test)]
        if let Some(notify) = self.notify_override {
            return notify(message, data);
        }
        unsafe { Shell_NotifyIconW(message, data).as_bool() }
    }

    fn update_retry_timer(&self) {
        unsafe {
            if self.visible && !self.added {
                // Login can start the recorder before Explorer has a notification area.
                // Retry only while missing; normal recording does not poll the shell.
                SetTimer(Some(self.hwnd), RETRY_TIMER, 2_000, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), RETRY_TIMER);
            }
        }
    }

    fn add_icon(&mut self, tooltip: &str, badge: TrayBadge) {
        if self.added || !self.visible {
            return;
        }
        let Some(icon) = create_encore_icon(badge) else {
            self.update_retry_timer();
            return;
        };
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: 1,
            uFlags: NIF_MESSAGE | NIF_TIP | NIF_ICON,
            uCallbackMessage: WM_TRAY_CALLBACK,
            hIcon: icon,
            szTip: encode_tip(tooltip),
            ..Default::default()
        };
        let added = self.notify(NIM_ADD, &nid);
        if let Some(old) = self.icon.replace(icon) {
            unsafe { let _ = DestroyIcon(old); }
        }
        self.badge = badge;
        self.added = added;
        self.update_retry_timer();
    }

    pub fn set_visible(&mut self, visible: bool, tooltip: &str, badge: TrayBadge) {
        self.visible = visible;
        if visible {
            self.set_state(tooltip, badge);
        } else {
            if self.added {
                let nid = NOTIFYICONDATAW {
                    cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                    hWnd: self.hwnd,
                    uID: 1,
                    ..Default::default()
                };
                self.notify(NIM_DELETE, &nid);
            }
            self.added = false;
            if let Some(ic) = self.icon.take() {
                unsafe { let _ = DestroyIcon(ic); }
            }
            self.update_retry_timer();
        }
    }

    pub fn retry(&mut self, tooltip: &str, badge: TrayBadge) {
        if !self.added {
            self.add_icon(tooltip, badge);
        }
    }

    pub fn taskbar_created(&mut self, tooltip: &str, badge: TrayBadge) {
        // Explorer discarded every registered icon, even if NIM_ADD succeeded before.
        self.added = false;
        self.add_icon(tooltip, badge);
    }

    pub fn set_state(&mut self, tooltip: &str, badge: TrayBadge) {
        if !self.visible {
            return;
        }
        if !self.added {
            self.add_icon(tooltip, badge);
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
        let modified = self.notify(NIM_MODIFY, &nid);
        if let Some(ic) = new_icon {
            if let Some(old) = self.icon.replace(ic) {
                unsafe { let _ = DestroyIcon(old); }
            }
            self.badge = badge;
        }
        if !modified {
            self.added = false;
            self.add_icon(tooltip, badge);
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
            let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("Quit Encore"));

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
        unsafe { let _ = KillTimer(Some(self.hwnd), RETRY_TIMER); }
        if self.added {
            let nid = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: 1,
                ..Default::default()
            };
            self.notify(NIM_DELETE, &nid);
        }
        if let Some(ic) = self.icon.take() {
            unsafe {
                let _ = DestroyIcon(ic);
            }
        }
    }
}

/// Wake an already-running library, otherwise launch the Encore UI executable
/// next to this binary, falling back to opening `clips_dir`.
pub fn launch_ui_or_folder(clips_dir: &Path) {
    if ipc::send_ui_command("show").unwrap_or(false) { return; }
    if let Ok(exe) = std::env::current_exe() {
        for base in exe.ancestors().skip(1).take(2) {
            for name in ["Encore.exe", "encore-ui.exe"] {
                let cand = base.join(name);
                if cand.exists() && Command::new(cand).creation_flags(0x0800_0000).stdin(Stdio::null())
                        .stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok() {
                    return;
                }
            }
        }
    }
    open_folder(clips_dir);
}

/// A library close never stops recording. An explicit native Quit requests both exits.
pub fn close_ui() { let _ = ipc::send_ui_command("quit"); }

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
    use std::cell::RefCell;
    use windows::Win32::UI::WindowsAndMessaging::DestroyWindow;

    #[derive(Default)]
    struct Shell {
        fail_add: bool,
        fail_modify: bool,
        calls: Vec<NOTIFY_ICON_MESSAGE>,
        tooltip: String,
    }

    thread_local! { static SHELL: RefCell<Shell> = RefCell::default(); }

    fn notify(message: NOTIFY_ICON_MESSAGE, data: &NOTIFYICONDATAW) -> bool {
        SHELL.with_borrow_mut(|shell| {
            shell.calls.push(message);
            let end = data.szTip.iter().position(|c| *c == 0).unwrap_or(data.szTip.len());
            shell.tooltip = String::from_utf16_lossy(&data.szTip[..end]);
            !(message == NIM_ADD && shell.fail_add || message == NIM_MODIFY && shell.fail_modify)
        })
    }

    #[test]
    fn tray_recovers_from_login_failure_and_explorer_restart_without_unhiding() {
        // Use a real hidden window/timer, but simulate the shell being unavailable
        // without restarting the user's Explorer or changing their notification area.
        let hwnd = crate::windows_runtime::create_message_window();
        let mut tray = TrayIcon::new(hwnd, "Ready", TrayBadge::Ready, false);
        tray.notify_override = Some(notify);
        SHELL.with_borrow_mut(|shell| *shell = Shell { fail_add: true, ..Default::default() });

        tray.set_visible(true, "Ready", TrayBadge::Ready);
        assert!(tray.visible && !tray.added);
        tray.retry("Recording", TrayBadge::Recording);
        assert!(!tray.added);
        SHELL.with_borrow_mut(|shell| shell.fail_add = false);
        tray.retry("Recording", TrayBadge::Recording);
        assert!(tray.added);
        assert_eq!(tray.badge, TrayBadge::Recording);
        SHELL.with_borrow(|shell| assert_eq!(shell.tooltip, "Recording"));
        let count = SHELL.with_borrow(|shell| shell.calls.len());
        tray.retry("Recording", TrayBadge::Recording);
        SHELL.with_borrow(|shell| assert_eq!(shell.calls.len(), count));

        tray.taskbar_created("Ready again", TrayBadge::Ready);
        assert!(tray.added);
        SHELL.with_borrow(|shell| {
            assert_eq!(shell.calls.last(), Some(&NIM_ADD));
            assert_eq!(shell.tooltip, "Ready again");
        });
        SHELL.with_borrow_mut(|shell| shell.fail_modify = true);
        tray.set_state("Changed", TrayBadge::Recording);
        assert!(tray.added);
        SHELL.with_borrow(|shell| assert!(shell.calls.ends_with(&[NIM_MODIFY, NIM_ADD])));

        // Hiding while a retry is pending must cancel recovery, including later broadcasts.
        SHELL.with_borrow_mut(|shell| shell.fail_add = true);
        tray.taskbar_created("Ready", TrayBadge::Ready);
        assert!(!tray.added);
        tray.set_visible(false, "Ready", TrayBadge::Ready);
        let count = SHELL.with_borrow(|shell| shell.calls.len());
        tray.taskbar_created("Ready", TrayBadge::Ready);
        tray.retry("Ready", TrayBadge::Ready);
        tray.set_state("Recording", TrayBadge::Recording);
        SHELL.with_borrow(|shell| assert_eq!(shell.calls.len(), count));
        assert!(!tray.added);
        drop(tray);
        unsafe { DestroyWindow(hwnd).unwrap(); }
    }

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
