use crate::overlay::Overlay;
use crate::prefs::{self, Prefs};
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

fn current_prefs(app: &AppHandle) -> Prefs {
    app.try_state::<Overlay>()
        .and_then(|state| state.prefs.lock().ok().map(|p| p.clone()))
        .unwrap_or_else(prefs::load)
}

/// 只更新条。tip / update 要等 JS 把窗口收成卡片后再 `set_window_blur`。
pub fn apply_from_app(app: &AppHandle) {
    let blur = look_blur(app);
    if let Some(bar) = app.get_webview_window("bar") {
        apply_bar(&bar, &current_prefs(app));
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

pub fn apply_bar(bar: &WebviewWindow, prefs: &Prefs) {
    #[cfg(target_os = "macos")]
    {
        apply_window(bar, 0.0);
        macos::sync_bar_frost(bar, prefs);
    }
    #[cfg(not(target_os = "macos"))]
    {
        apply_window(bar, prefs.bar_blur);
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
    if window == "bar" {
        if let Some(bar) = app.get_webview_window("bar") {
            apply_bar(&bar, &current_prefs(app));
        }
        return;
    }
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
    use crate::dock_shape;
    use crate::prefs::Prefs;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, sel, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSBackingStoreType, NSColor, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
        NSWindowOrderingMode, NSWindowStyleMask,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use std::ffi::{c_void, CStr};
    use std::sync::{Mutex, OnceLock};
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

    #[derive(Clone, PartialEq)]
    struct FrostKey {
        parent: isize,
        edge: String,
        w: i32,
        h: i32,
        blur: i32,
    }

    struct FrostWins {
        body: Retained<NSWindow>,
        pod: Retained<NSWindow>,
        key: Option<FrostKey>,
    }

    unsafe impl Send for FrostWins {}
    unsafe impl Sync for FrostWins {}

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

    fn frost_slot() -> &'static Mutex<Option<FrostWins>> {
        static SLOT: OnceLock<Mutex<Option<FrostWins>>> = OnceLock::new();
        SLOT.get_or_init(|| Mutex::new(None))
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
        let Some(ns) = bar_ns(window) else {
            return;
        };
        set_ns_corner_radius(ns, radius);
        let Some((conn, wid)) = connection_ns(ns) else {
            return;
        };
        set_sls_corner_radius(conn, wid, radius);
    }

    pub fn sync_bar_frost(bar: &WebviewWindow, prefs: &Prefs) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(parent) = bar_ns(bar) else {
            return;
        };
        set_cgs_blur_ns(parent, 0);
        set_ns_corner_radius(parent, 0.0);
        if let Some((conn, wid)) = connection_ns(parent) {
            set_sls_corner_radius(conn, wid, 0.0);
        }
        let blur = prefs.bar_blur;
        if blur <= 0.0 {
            hide_frost();
            return;
        }
        let frame = parent.frame();
        let w = frame.size.width;
        let h = frame.size.height;
        if w < 2.0 || h < 2.0 {
            hide_frost();
            return;
        }
        let mut slot = match frost_slot().lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        if slot.is_none() {
            *slot = Some(FrostWins {
                body: make_frost(mtm),
                pod: make_frost(mtm),
                key: None,
            });
        }
        let Some(frost) = slot.as_mut() else {
            return;
        };
        detach_child(&frost.body);
        detach_child(&frost.pod);
        style_like_parent(parent, &frost.body);
        style_like_parent(parent, &frost.pod);

        let radius = native_blur(blur);
        let (body_frame, corner) = body_frost_frame(frame, &prefs.edge);
        let (pod_frame, pod_r) = pod_frost_frame(frame, &prefs.edge);
        frost.body.setFrame_display(body_frame, false);
        frost.pod.setFrame_display(pod_frame, false);

        frost.body.orderFrontRegardless();
        frost.pod.orderFrontRegardless();
        let parent_num = parent.windowNumber();
        if parent_num > 0 {
            frost
                .body
                .orderWindow_relativeTo(NSWindowOrderingMode::Below, parent_num);
            frost
                .pod
                .orderWindow_relativeTo(NSWindowOrderingMode::Below, parent_num);
        }
        set_cgs_blur_ns(&frost.body, radius);
        set_cgs_blur_ns(&frost.pod, radius);
        set_ns_corner_radius(&frost.body, corner);
        if let Some((conn, wid)) = connection_ns(&frost.body) {
            set_sls_corner_radius(conn, wid, corner);
        }
            set_ns_corner_radius(&frost.pod, pod_r);
            if let Some((conn, wid)) = connection_ns(&frost.pod) {
                set_sls_corner_radius(conn, wid, pod_r);
            }
        frost.key = Some(FrostKey {
            parent: parent_num,
            edge: prefs.edge.clone(),
            w: w.round() as i32,
            h: h.round() as i32,
            blur: radius,
        });
        set_cgs_blur_ns(parent, 0);
    }

    fn hide_frost() {
        let mut slot = match frost_slot().lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        let Some(frost) = slot.as_mut() else {
            return;
        };
        set_cgs_blur_ns(&frost.body, 0);
        set_cgs_blur_ns(&frost.pod, 0);
        set_ns_corner_radius(&frost.body, 0.0);
        set_ns_corner_radius(&frost.pod, 0.0);
        if let Some((conn, wid)) = connection_ns(&frost.body) {
            set_sls_corner_radius(conn, wid, 0.0);
        }
        if let Some((conn, wid)) = connection_ns(&frost.pod) {
            set_sls_corner_radius(conn, wid, 0.0);
        }
        frost.body.orderOut(None);
        frost.pod.orderOut(None);
        frost.key = None;
    }

    fn make_frost(mtm: MainThreadMarker) -> Retained<NSWindow> {
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(8.0, 8.0));
        let win = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        win.setOpaque(false);
        win.setHasShadow(false);
        win.setIgnoresMouseEvents(true);
        unsafe {
            win.setReleasedWhenClosed(false);
        }
        win.setBackgroundColor(Some(&NSColor::colorWithWhite_alpha(0.0, 0.01)));
        win.setHidesOnDeactivate(false);
        win.setCanHide(false);
        win.setAnimationBehavior(NSWindowAnimationBehavior::None);
        win.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        win
    }

    fn detach_child(window: &NSWindow) {
        if let Some(parent) = window.parentWindow() {
            parent.removeChildWindow(window);
        }
    }

    fn style_like_parent(parent: &NSWindow, child: &NSWindow) {
        child.setLevel(parent.level());
        child.setCollectionBehavior(
            parent.collectionBehavior()
                | NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
    }

    fn body_frost_frame(bar: NSRect, edge: &str) -> (NSRect, f64) {
        let w = bar.size.width;
        let h = bar.size.height;
        let box_ = dock_shape::frost_body_box(w, h, edge);
        let wall = box_.radius + dock_shape::FROST_WALL_EXTRA;
        let mut frame = NSRect::new(
            NSPoint::new(bar.origin.x + box_.x, bar.origin.y + (h - box_.y - box_.h)),
            NSSize::new(box_.w, box_.h),
        );
        match edge {
            "right" => frame.size.width += wall,
            "left" => {
                frame.origin.x -= wall;
                frame.size.width += wall;
            }
            "top" => frame.size.height += wall,
            "bottom" => {
                frame.origin.y -= wall;
                frame.size.height += wall;
            }
            _ => {}
        }
        let radius = box_.radius.min(frame.size.width.min(frame.size.height) / 2.0);
        (frame, radius)
    }

    fn pod_frost_frame(bar: NSRect, edge: &str) -> (NSRect, f64) {
        let w = bar.size.width;
        let h = bar.size.height;
        let (cx, cy) = dock_shape::gear_center(w, h, edge);
        let r = dock_shape::frost_pod_radius();
        (
            NSRect::new(
                NSPoint::new(bar.origin.x + cx - r, bar.origin.y + (h - cy) - r),
                NSSize::new(r * 2.0, r * 2.0),
            ),
            r,
        )
    }

    fn native_blur(blur: f64) -> i32 {
        crate::prefs::normalize_bar_blur(blur) as i32
    }

    fn set_cgs_blur_ns(window: &NSWindow, radius: i32) {
        let Some((conn, wid)) = connection_ns(window) else {
            return;
        };
        unsafe {
            let _ = CGSSetWindowBackgroundBlurRadius(conn, wid, radius);
        }
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

    fn set_ns_corner_radius(window: &NSWindow, radius: f64) {
        let ns_window = window as *const NSWindow as *mut AnyObject;
        if ns_window.is_null() {
            return;
        }
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

    fn bar_ns(window: &WebviewWindow) -> Option<&NSWindow> {
        let ptr = window.ns_window().ok()?;
        if ptr.is_null() {
            return None;
        }
        Some(unsafe { &*(ptr as *const NSWindow) })
    }

    fn connection(window: &WebviewWindow) -> Option<(i32, i32)> {
        connection_ns(bar_ns(window)?)
    }

    fn connection_ns(window: &NSWindow) -> Option<(i32, i32)> {
        let window_number = window.windowNumber();
        if window_number <= 0 {
            return None;
        }
        Some((unsafe { CGSMainConnectionID() }, window_number as i32))
    }
}
