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

/// Windows 11 hides new tray icons in the `^` overflow. Mark Lull's own
/// entry as shown on the taskbar. Explorer creates the entry shortly after
/// the icon first appears, so this returns false until it exists.
#[cfg(windows)]
pub fn promote_tray_icon() -> bool {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_DWORD, RRF_RT_REG_SZ, RegCloseKey,
        RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW,
    };
    use windows::core::{PCWSTR, PWSTR, w};

    let Some(exe) = std::env::current_exe().ok().map(|p| p.to_string_lossy().to_lowercase()) else {
        return false;
    };
    let mut found = false;
    unsafe {
        let mut root = HKEY::default();
        let key = w!(r"Control Panel\NotifyIconSettings");
        if RegOpenKeyExW(HKEY_CURRENT_USER, key, None, KEY_READ | KEY_WRITE, &mut root).is_err() {
            return false;
        }
        for i in 0.. {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            let r = RegEnumKeyExW(root, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None);
            if r.is_err() {
                break;
            }
            let sub = PCWSTR(name.as_ptr());
            let mut buf = [0u16; 1024];
            let mut size = (buf.len() * 2) as u32;
            let r = RegGetValueW(
                root,
                sub,
                w!("ExecutablePath"),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut size),
            );
            if r.is_err() {
                continue;
            }
            let len = (size as usize / 2).saturating_sub(1).min(buf.len());
            if String::from_utf16_lossy(&buf[..len]).to_lowercase() == exe {
                let one: u32 = 1;
                let r = RegSetKeyValueW(root, sub, w!("IsPromoted"), REG_DWORD.0, Some((&one as *const u32).cast()), 4);
                found |= r.is_ok();
            }
        }
        let _ = RegCloseKey(root);
    }
    found
}
