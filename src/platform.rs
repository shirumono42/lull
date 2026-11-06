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

/// macOS major version, read from the system version file (no process spawn).
#[cfg(target_os = "macos")]
fn macos_major() -> u32 {
    let plist = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").unwrap_or_default();
    plist
        .split("<key>ProductVersion</key>")
        .nth(1)
        .and_then(|rest| rest.split("<string>").nth(1))
        .and_then(|v| v.split(['.', '<']).next())
        .and_then(|major| major.trim().parse().ok())
        .unwrap_or(15)
}

/// Corner radius (pt) macOS gives its own windows of each kind, measured with
/// `-[NSWindow _cornerRadius]`: standard titled windows are 12 pt on macOS 15
/// and 16 pt on macOS 26; HUD panels 10 pt and 15 pt.
#[cfg(not(target_os = "macos"))]
pub fn corner_radius(_hud: bool) -> f64 {
    0.0 // Windows 11 rounds borderless windows itself (see CornerPreference)
}

#[cfg(target_os = "macos")]
pub fn corner_radius(hud: bool) -> f64 {
    match (macos_major() >= 26, hud) {
        (true, false) => 16.0,
        (true, true) => 15.0,
        (false, false) => 12.0,
        (false, true) => 10.0,
    }
}

/// macOS: borderless windows get square corners. Clip the content to a
/// rounded rectangle with Apple's continuous ("squircle") corner curve and
/// let the system draw its shadow around that shape.
pub fn round_corners(w: &Window, radius: f64) {
    #[cfg(target_os = "macos")]
    if let Ok(handle) = w.window_handle() {
        if let RawWindowHandle::AppKit(h) = handle.as_raw() {
            unsafe {
                use objc2::runtime::{AnyObject, Bool};
                use objc2::{class, msg_send};
                let view = &*(h.ns_view.as_ptr() as *const AnyObject);
                let _: () = msg_send![view, setWantsLayer: Bool::YES];
                let layer: *const AnyObject = msg_send![view, layer];
                if let Some(layer) = layer.as_ref() {
                    let _: () = msg_send![layer, setCornerRadius: radius];
                    let _: () = msg_send![layer, setMasksToBounds: Bool::YES];
                    // kCACornerCurveContinuous (macOS 10.15+).
                    let curve: *const AnyObject =
                        msg_send![class!(NSString), stringWithUTF8String: c"continuous".as_ptr()];
                    let _: () = msg_send![layer, setCornerCurve: curve];
                }
                let win: *const AnyObject = msg_send![view, window];
                if let Some(win) = win.as_ref() {
                    let clear: *const AnyObject = msg_send![class!(NSColor), clearColor];
                    let _: () = msg_send![win, setOpaque: Bool::NO];
                    let _: () = msg_send![win, setBackgroundColor: clear];
                    let _: () = msg_send![win, setHasShadow: Bool::YES];
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (w, radius);
}

/// macOS computes a window's shadow from its content; recompute it after the
/// first frame so it follows the rounded corners.
pub fn refresh_shadow(w: &Window) {
    #[cfg(target_os = "macos")]
    if let Ok(handle) = w.window_handle() {
        if let RawWindowHandle::AppKit(h) = handle.as_raw() {
            unsafe {
                use objc2::msg_send;
                use objc2::runtime::AnyObject;
                let view = &*(h.ns_view.as_ptr() as *const AnyObject);
                let win: *const AnyObject = msg_send![view, window];
                if let Some(win) = win.as_ref() {
                    let _: () = msg_send![win, invalidateShadow];
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = w;
}
