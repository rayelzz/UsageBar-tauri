use crate::overlay::Overlay;
use crate::prefs;
use tauri::{AppHandle, Manager, WebviewWindow};

const CARD_WINDOWS: [&str; 2] = ["tip", "update"];
const CARD_RADIUS: f64 = 14.0;

fn look_blur(app: &AppHandle) -> f64 {
    app.try_state::<Overlay>()
        .and_then(|state| state.prefs.lock().ok().map(|p| p.bar_blur))
        .unwrap_or_else(|| prefs::load().bar_blur)
}

fn native_radius(blur: f64) -> i32 {
    prefs::normalize_bar_blur(blur) as i32
}

/// 只更新条。tip / update 要等 JS 把窗口收成卡片后再 `set_window_blur`。
pub fn apply_from_app(app: &AppHandle) {
    let blur = look_blur(app);
    if let Some(bar) = app.get_webview_window("bar") {
        apply_window(&bar, blur);
    }
    if blur <= 0.0 {
        for label in CARD_WINDOWS {
            if let Some(win) = app.get_webview_window(label) {
                apply_window(&win, 0.0);
                #[cfg(target_os = "macos")]
                macos::set_card_corners(&win, false);
            }
        }
    }
}

pub fn apply_window(window: &WebviewWindow, blur: f64) {
    let _ = window.set_background_color(Some(tauri::window::Color(0, 0, 0, 0)));
    let radius = if blur <= 0.0 { 0 } else { native_radius(blur) };
    #[cfg(target_os = "macos")]
    macos::set_cgs_blur(window, radius);
    #[cfg(not(target_os = "macos"))]
    let _ = radius;
}

pub fn set_window_blur(app: &AppHandle, window: &str, blur: f64) {
    let radius_blur = if blur <= 0.0 {
        0.0
    } else {
        look_blur(app).max(prefs::normalize_bar_blur(blur))
    };
    let Some(win) = app.get_webview_window(window) else {
        return;
    };
    apply_window(&win, radius_blur);
    if CARD_WINDOWS.contains(&window) {
        #[cfg(target_os = "macos")]
        macos::set_card_corners(&win, radius_blur > 0.0);
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::CARD_RADIUS;
    use objc2::{msg_send, sel};
    use objc2::runtime::AnyObject;
    use std::ffi::{c_void, CStr};
    use std::sync::OnceLock;
    use tauri::WebviewWindow;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGSMainConnectionID() -> i32;
        fn CGSSetWindowBackgroundBlurRadius(connection: i32, window_id: i32, radius: i32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const c_void);
    }

    extern "C" {
        fn dlopen(path: *const i8, mode: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const i8) -> *mut c_void;
    }

    const RTLD_LAZY: i32 = 1;

    type TxCreate = unsafe extern "C" fn(i32) -> *mut c_void;
    type TxCommit = unsafe extern "C" fn(*mut c_void, i32) -> i32;
    type TxSetRadius = unsafe extern "C" fn(*mut c_void, u32, f64);
    type TxClearRadius = unsafe extern "C" fn(*mut c_void, u32);

    struct Sls {
        create: TxCreate,
        commit: TxCommit,
        set: TxSetRadius,
        set_sys: Option<TxSetRadius>,
        clear: Option<TxClearRadius>,
        clear_sys: Option<TxClearRadius>,
    }

    fn sls() -> Option<&'static Sls> {
        static SLS: OnceLock<Option<Sls>> = OnceLock::new();
        SLS.get_or_init(|| unsafe {
            let handle = dlopen(
                c"/System/Library/PrivateFrameworks/SkyLight.framework/SkyLight".as_ptr(),
                RTLD_LAZY,
            );
            if handle.is_null() {
                return None;
            }
            let create = load(handle, c"SLSTransactionCreate")?;
            let commit = load(handle, c"SLSTransactionCommit")?;
            let set = load(handle, c"SLSTransactionSetWindowCornerRadius")?;
            Some(Sls {
                create,
                commit,
                set,
                set_sys: load(handle, c"SLSTransactionSetWindowSystemCornerRadius"),
                clear: load(handle, c"SLSTransactionClearWindowCornerRadius"),
                clear_sys: load(handle, c"SLSTransactionClearWindowSystemCornerRadius"),
            })
        })
        .as_ref()
    }

    unsafe fn load<T: Copy>(handle: *mut c_void, name: &CStr) -> Option<T> {
        let symbol = dlsym(handle, name.as_ptr());
        if symbol.is_null() {
            None
        } else {
            Some(std::mem::transmute_copy(&symbol))
        }
    }

    pub fn set_cgs_blur(window: &WebviewWindow, radius: i32) {
        let Some((conn, wid)) = connection(window) else {
            return;
        };
        unsafe {
            let _ = CGSSetWindowBackgroundBlurRadius(conn, wid, radius);
        }
    }

    /// 只改 WindowServer 圆角，不改窗口外形区域，避免再复制出一块面板。
    pub fn set_card_corners(window: &WebviewWindow, rounded: bool) {
        let radius = if rounded { CARD_RADIUS } else { 0.0 };
        set_ns_corner_radius(window, radius);
        let Some((conn, wid)) = connection(window) else {
            return;
        };
        set_sls_corner_radius(conn, wid, radius);
    }

    fn set_sls_corner_radius(conn: i32, wid: i32, radius: f64) {
        let Some(sls) = sls() else {
            return;
        };
        unsafe {
            let txn = (sls.create)(conn);
            if txn.is_null() {
                return;
            }
            if radius <= 0.0 {
                if let Some(clear) = sls.clear {
                    clear(txn, wid as u32);
                } else {
                    (sls.set)(txn, wid as u32, 0.0);
                }
                if let Some(clear_sys) = sls.clear_sys {
                    clear_sys(txn, wid as u32);
                }
            } else {
                (sls.set)(txn, wid as u32, radius);
                if let Some(set_sys) = sls.set_sys {
                    set_sys(txn, wid as u32, radius);
                }
            }
            let _ = (sls.commit)(txn, 0);
            CFRelease(txn);
        }
    }

    fn set_ns_corner_radius(window: &WebviewWindow, radius: f64) {
        let Ok(ptr) = window.ns_window() else {
            return;
        };
        if ptr.is_null() {
            return;
        }
        let ns_window = ptr as *mut AnyObject;
        unsafe {
            let set_corner = sel!(_setCornerRadius:);
            if msg_send![ns_window, respondsToSelector: set_corner] {
                let _: () = msg_send![ns_window, _setCornerRadius: radius];
            }
            let set_effective = sel!(_setEffectiveCornerRadius:);
            if msg_send![ns_window, respondsToSelector: set_effective] {
                let _: () = msg_send![ns_window, _setEffectiveCornerRadius: radius];
            }
            let update_mask = sel!(_updateCornerMask);
            if msg_send![ns_window, respondsToSelector: update_mask] {
                let _: () = msg_send![ns_window, _updateCornerMask];
            }
            let _: () = msg_send![ns_window, invalidateShadow];
        }
    }

    fn connection(window: &WebviewWindow) -> Option<(i32, i32)> {
        let ptr = window.ns_window().ok()?;
        if ptr.is_null() {
            return None;
        }
        let ns_window = ptr as *mut AnyObject;
        unsafe {
            let window_number: isize = msg_send![ns_window, windowNumber];
            if window_number <= 0 {
                return None;
            }
            Some((CGSMainConnectionID(), window_number as i32))
        }
    }

}
