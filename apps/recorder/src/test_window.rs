//! A controlled animated window for capture tests; never records the user's desktop or audio.
use std::sync::{mpsc, Once};
use std::thread::{self, JoinHandle};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, InvalidateRect, PAINTSTRUCT,
};
use windows::Win32::UI::WindowsAndMessaging::*;

pub struct TestWindow {
    pub hwnd: usize,
    thread: Option<JoinHandle<()>>,
}

impl TestWindow {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = thread::spawn(move || unsafe {
            static REGISTER: Once = Once::new();
            REGISTER.call_once(|| {
                let class = WNDCLASSW {
                    lpfnWndProc: Some(window_proc),
                    lpszClassName: w!("EncoreControlledCaptureTest"),
                    ..Default::default()
                };
                assert_ne!(RegisterClassW(&class), 0);
            });
            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE,
                w!("EncoreControlledCaptureTest"),
                w!("Encore capture test"),
                WS_OVERLAPPEDWINDOW,
                20,
                20,
                640,
                360,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetTimer(Some(hwnd), 1, 8, None);
            tx.send(hwnd.0 as usize).unwrap();
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        let hwnd = rx.recv_timeout(std::time::Duration::from_secs(5)).expect("test window startup");
        Self { hwnd, thread: Some(thread) }
    }

    pub fn handle(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }
}

impl Drop for TestWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = PostMessageW(Some(self.handle()), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            let n = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } + 1;
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, n);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            unsafe {
                let dc = BeginPaint(hwnd, &mut paint);
                let n = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as u32;
                let brush = CreateSolidBrush(COLORREF((n.wrapping_mul(7919)) & 0x00ffffff));
                FillRect(dc, &paint.rcPaint, brush);
                let _ = DeleteObject(brush.into());
                let _ = EndPaint(hwnd, &paint);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
