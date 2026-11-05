//! The few window features winit does not expose.

use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

/// Sets whole-window opacity (0..1).
pub fn set_opacity(w: &Window, alpha: f32) {
    let Ok(handle) = w.window_handle() else { return };
    match handle.as_raw() {
        #[cfg(windows)]
        RawWindowHandle::Win32(h) => unsafe {
            use windows::Win32::Foundation::{COLORREF, HWND};
            use windows::Win32::UI::WindowsAndMessaging::{
                GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes,
                SetWindowLongPtrW, WS_EX_LAYERED,
            };
            let hwnd = HWND(h.hwnd.get() as *mut _);
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if ex & WS_EX_LAYERED.0 as isize == 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as isize);
            }
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), (alpha * 255.0) as u8, LWA_ALPHA);
        },
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(h) => unsafe {
            use objc2::msg_send;
            use objc2::runtime::AnyObject;
            let view = &*(h.ns_view.as_ptr() as *const AnyObject);
            let win: *const AnyObject = msg_send![view, window];
            if let Some(win) = win.as_ref() {
                let _: () = msg_send![win, setAlphaValue: alpha as f64];
            }
        },
        _ => {}
    }
}

/// macOS: keep the on-screen timer on every Space, including full-screen apps.
pub fn show_on_all_spaces(w: &Window) {
    #[cfg(target_os = "macos")]
    if let Ok(handle) = w.window_handle() {
        if let RawWindowHandle::AppKit(h) = handle.as_raw() {
            unsafe {
                use objc2::msg_send;
                use objc2::runtime::AnyObject;
                const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
                const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
                let view = &*(h.ns_view.as_ptr() as *const AnyObject);
                let win: *const AnyObject = msg_send![view, window];
                if let Some(win) = win.as_ref() {
                    let behavior = CAN_JOIN_ALL_SPACES | FULL_SCREEN_AUXILIARY;
                    let _: () = msg_send![win, setCollectionBehavior: behavior];
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = w;
}
