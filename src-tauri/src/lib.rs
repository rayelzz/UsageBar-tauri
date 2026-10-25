mod dock_shape;
mod frost;
mod i18n;
mod overlay;
mod prefs;
mod providers;
mod updater;
mod usage_state;

use overlay::Overlay;
use prefs::Prefs;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

static TRAY_GEN: AtomicU64 = AtomicU64::new(0);
static BAR_HIDDEN_BY_TRAY: AtomicBool = AtomicBool::new(false);
static TRAY_SUMMARY: Mutex<String> = Mutex::new(String::new());

#[tauri::command]
async fn fetch_usage(app: AppHandle) -> Vec<providers::ProviderSnapshot> {
    let ids = app
        .try_state::<Overlay>()
        .and_then(|state| state.prefs.lock().ok().map(|p| p.visible_providers.clone()))
        .unwrap_or_else(|| prefs::load().visible_providers);
    tauri::async_runtime::spawn_blocking(move || {
        let handle = app.clone();
        providers::fetch_selected_each(&ids, |snap| {
            let _ = handle.emit("usagebar-usage", snap);
        })
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
fn get_prefs(app: AppHandle) -> Prefs {
    if let Some(state) = app.try_state::<Overlay>() {
        if let Ok(prefs) = state.prefs.lock() {
            return prefs.clone();
        }
    }
    prefs::load()
}

#[tauri::command]
fn set_prefs(app: AppHandle, prefs: Prefs) {
    let incoming = prefs;
    let base = if let Some(state) = app.try_state::<Overlay>() {
        state.prefs.lock().ok().map(|g| g.clone())
    } else {
        None
    }
    .unwrap_or_else(prefs::load);
    let prefs = prefs::apply_incoming(base.clone(), incoming);
    if let Some(state) = app.try_state::<Overlay>() {
        if let Ok(mut slot) = state.prefs.lock() {
            *slot = prefs.clone();
        }
    }
    prefs::save(&prefs);
    frost::apply_from_app(&app);
    let _ = app.emit("usagebar-prefs", &prefs);
    if prefs::tray_needs_update(&base, &prefs) {
        apply_app_icon(&app, &prefs);
        schedule_tray(app, prefs);
    }
}

#[tauri::command]
fn set_visible_providers(app: AppHandle, ids: Vec<String>) {
    let ids = prefs::normalize_visible(&ids);
    let prefs = if let Some(state) = app.try_state::<Overlay>() {
        let Ok(mut slot) = state.prefs.lock() else {
            return;
        };
        if slot.visible_providers == ids {
            let current = slot.clone();
            drop(slot);
            let _ = app.emit("usagebar-prefs", &current);
            return;
        }
        slot.visible_providers = ids;
        slot.clone()
    } else {
        let mut prefs = prefs::load();
        prefs.visible_providers = ids;
        prefs
    };
    prefs::save(&prefs);
    let _ = app.emit("usagebar-prefs", &prefs);
}

#[tauri::command]
fn set_bar_look(app: AppHandle, opacity: f64, blur: f64) {
    let opacity = prefs::normalize_bar_opacity(opacity);
    let blur = prefs::normalize_bar_blur(blur);
    let prefs = if let Some(state) = app.try_state::<Overlay>() {
        let Ok(mut slot) = state.prefs.lock() else {
            return;
        };
        if (slot.bar_opacity - opacity).abs() < 0.0001 && (slot.bar_blur - blur).abs() < 0.0001 {
            frost::apply_from_app(&app);
            return;
        }
        slot.bar_opacity = opacity;
        slot.bar_blur = blur;
        slot.clone()
    } else {
        let mut prefs = prefs::load();
        prefs.bar_opacity = opacity;
        prefs.bar_blur = blur;
        prefs
    };
    prefs::save(&prefs);
    frost::apply_from_app(&app);
    let _ = app.emit("usagebar-prefs", &prefs);
}

#[tauri::command]
fn refresh_window_blur(app: AppHandle) {
    frost::apply_from_app(&app);
}

#[tauri::command]
fn set_window_blur(app: AppHandle, window: String, blur: f64) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        frost::set_window_blur(&handle, &window, blur);
    });
}

fn schedule_tray(app: AppHandle, prefs: Prefs) {
    let gen = TRAY_GEN.fetch_add(1, Ordering::AcqRel) + 1;
    std::thread::spawn(move || {
        let handle = app.clone();
        let _ = handle.run_on_main_thread(move || {
            if TRAY_GEN.load(Ordering::Acquire) != gen {
                return;
            }
            apply_tray(&app, &prefs);
        });
    });
}

#[tauri::command]
fn place_bar(app: AppHandle) -> Option<overlay::BarFrame> {
    overlay::place(&app)
}

#[tauri::command]
fn set_pointer(left: bool) {
    overlay::JS_LEFT.store(left, std::sync::atomic::Ordering::Relaxed);
}

#[tauri::command]
fn set_update_card(card: Option<[f64; 4]>) {
    overlay::set_update_card(card);
}

#[tauri::command]
fn set_menu_open(app: AppHandle, open: bool, card: Option<[f64; 4]>) {
    overlay::MENU_OPEN.store(open, std::sync::atomic::Ordering::Relaxed);
    if !open {
        overlay::set_menu_card(None);
    } else if card.is_some() {
        overlay::set_menu_card(card);
    }
    if open {
        if let Some(state) = app.try_state::<Overlay>() {
            state.clear_hover();
        }
    }
}

#[tauri::command]
fn format_reset(ms: Option<i64>) -> Option<String> {
    providers::format_reset(ms)
}

#[tauri::command]
fn dismiss_reset_notice(id: String) {
    usage_state::dismiss(&id);
}

#[tauri::command]
fn dismiss_credit_notice(id: String) {
    usage_state::dismiss_credit(&id);
}

#[tauri::command]
fn app_version() -> String {
    updater::current_version()
}

#[tauri::command]
async fn check_update() -> Option<updater::UpdateInfo> {
    tauri::async_runtime::spawn_blocking(updater::check)
        .await
        .ok()
        .flatten()
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    updater::install(app).await
}

#[tauri::command]
fn open_release_page(app: AppHandle, url: Option<String>) {
    let target = url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .unwrap_or(updater::RELEASES_PAGE);
    let _ = app.opener().open_url(target, None::<&str>);
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn os_name() -> String {
    if cfg!(target_os = "macos") {
        "macos".into()
    } else if cfg!(target_os = "windows") {
        "windows".into()
    } else {
        "linux".into()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrayRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[tauri::command]
fn tray_rect(app: AppHandle) -> Option<TrayRect> {
    tray_rect_for(&app)
}

fn tray_rect_for(app: &AppHandle) -> Option<TrayRect> {
    let tray = app.tray_by_id("main")?;
    let rect = tray.rect().ok()??;
    let pos = rect.position.to_physical(1.0);
    let size = rect.size.to_physical(1.0);
    Some(TrayRect {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
    })
}

fn emit_tray_hover(app: &AppHandle, show: bool) {
    let visible = app
        .try_state::<Overlay>()
        .and_then(|state| {
            state
                .prefs
                .lock()
                .ok()
                .map(|prefs| prefs.app_icon == "menubar")
        })
        .unwrap_or(false);
    let payload = serde_json::json!({
        "show": show && visible,
        "rect": if show && visible { tray_rect_for(app) } else { None },
    });
    let _ = app.emit("usagebar-tray-hover", payload);
}

fn handle_tray_icon_event(app: &AppHandle, event: &TrayIconEvent) {
    match event {
        TrayIconEvent::Enter { .. } => emit_tray_hover(app, true),
        TrayIconEvent::Leave { .. } | TrayIconEvent::Click { .. } => emit_tray_hover(app, false),
        _ => {}
    }
}

fn build_tray_menu(app: &AppHandle, prefs: &Prefs) -> tauri::Result<Menu<tauri::Wry>> {
    let toggle = MenuItem::with_id(
        app,
        "toggle-bar",
        if BAR_HIDDEN_BY_TRAY.load(Ordering::Acquire) {
            i18n::show(prefs.locale.as_str())
        } else {
            i18n::hide(prefs.locale.as_str())
        },
        true,
        None::<&str>,
    )?;
    let refresh = MenuItem::with_id(
        app,
        "refresh",
        i18n::refresh(prefs.locale.as_str()),
        true,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(
        app,
        "settings",
        i18n::settings(prefs.locale.as_str()),
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        i18n::quit(prefs.locale.as_str()),
        true,
        Some("q"),
    )?;
    let summary = TRAY_SUMMARY
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();
    if summary.is_empty() {
        Menu::with_items(app, &[&toggle, &refresh, &settings, &separator, &quit])
    } else {
        let summary = MenuItem::with_id(
            app,
            "quota-summary",
            summary.replace('\n', "  ·  "),
            false,
            None::<&str>,
        )?;
        let summary_separator = PredefinedMenuItem::separator(app)?;
        Menu::with_items(
            app,
            &[
                &summary,
                &summary_separator,
                &toggle,
                &refresh,
                &settings,
                &separator,
                &quit,
            ],
        )
    }
}

#[tauri::command]
fn set_tray_summary(app: AppHandle, summary: String) {
    let summary = sanitize_tray_summary(&summary);
    let changed = if let Ok(mut cached) = TRAY_SUMMARY.lock() {
        if *cached == summary {
            false
        } else {
            *cached = summary.clone();
            true
        }
    } else {
        true
    };
    if !changed {
        return;
    }
    let prefs = get_prefs(app.clone());
    if let Some(tray) = app.tray_by_id("main") {
        let tooltip = if summary.is_empty() {
            "UsageBar"
        } else {
            &summary
        };
        let _ = tray.set_tooltip(Some(tooltip));
        if let Ok(menu) = build_tray_menu(&app, &prefs) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

fn sanitize_tray_summary(summary: &str) -> String {
    summary
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.chars()
                .filter(|c| !c.is_control())
                .take(160)
                .collect::<String>()
        })
        .take(prefs::SLOT_MAX)
        .collect::<Vec<_>>()
        .join("\n")
}

#[tauri::command]
fn set_visible_metrics(app: AppHandle, selections: std::collections::HashMap<String, Vec<String>>) {
    let selections = prefs::normalize_visible_metrics(&selections);
    let prefs = if let Some(state) = app.try_state::<Overlay>() {
        let Ok(mut slot) = state.prefs.lock() else {
            return;
        };
        if slot.visible_metrics == selections {
            let current = slot.clone();
            drop(slot);
            let _ = app.emit("usagebar-prefs", &current);
            return;
        }
        slot.visible_metrics = selections;
        slot.clone()
    } else {
        let mut prefs = prefs::load();
        prefs.visible_metrics = selections;
        prefs
    };
    prefs::save(&prefs);
    let _ = app.emit("usagebar-prefs", &prefs);
}

fn set_bar_visible(app: &AppHandle, visible: bool) {
    BAR_HIDDEN_BY_TRAY.store(!visible, Ordering::Release);
    if visible {
        reveal_bar(app);
    } else {
        overlay::MENU_OPEN.store(false, Ordering::Relaxed);
        overlay::set_menu_card(None);
        for label in ["bar", "tip", "update"] {
            if let Some(window) = app.get_webview_window(label) {
                let _ = window.hide();
            }
        }
    }
    let prefs = get_prefs(app.clone());
    apply_tray(app, &prefs);
}

fn handle_tray_menu_event(app: &AppHandle, id: &str) {
    match id {
        "toggle-bar" => {
            let show = BAR_HIDDEN_BY_TRAY.load(Ordering::Acquire);
            set_bar_visible(app, show);
        }
        "settings" => {
            set_bar_visible(app, true);
            let _ = app.emit("usagebar-menu", "settings");
        }
        _ => {
            let _ = app.emit("usagebar-menu", id);
        }
    }
}

fn apply_app_icon(app: &AppHandle, prefs: &Prefs) {
    let show_tray = prefs.app_icon == "menubar";
    if !show_tray {
        emit_tray_hover(app, false);
    }
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_visible(show_tray);
        #[cfg(target_os = "macos")]
        if show_tray {
            style_macos_status_item(&tray);
        }
    }
    #[cfg(target_os = "macos")]
    set_macos_dock_visible(app, prefs.app_icon == "dock");
    #[cfg(not(target_os = "macos"))]
    if let Some(bar) = app.get_webview_window("bar") {
        let _ = bar.set_skip_taskbar(prefs.app_icon != "dock");
    }
    reveal_bar(app);
}

fn reveal_bar(app: &AppHandle) {
    if BAR_HIDDEN_BY_TRAY.load(Ordering::Acquire) {
        return;
    }
    let Some(bar) = app.get_webview_window("bar") else {
        return;
    };
    let _ = bar.set_always_on_top(true);
    let _ = bar.show();
    #[cfg(target_os = "macos")]
    if let Ok(ptr) = bar.ns_window() {
        if !ptr.is_null() {
            unsafe {
                let win = ptr as *mut objc2::runtime::AnyObject;
                let _: () = objc2::msg_send![win, orderFrontRegardless];
            }
        }
    }
    frost::apply_from_app(app);
}

/// Hide/show the Dock icon without Tauri's 1s hide-after-show debounce.
/// UIElement 变换后必须把条再顶到前面，否则透明窗会停在窗口列表里却不画。
#[cfg(target_os = "macos")]
fn set_macos_dock_visible(_app: &AppHandle, visible: bool) {
    use objc2_app_kit::NSApplication;
    use objc2_foundation::MainThreadMarker;

    #[repr(C)]
    struct ProcessSerialNumber {
        high_long_of_psn: u32,
        low_long_of_psn: u32,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn TransformProcessType(psn: *const ProcessSerialNumber, transform_state: i32) -> i32;
    }

    const CURRENT_PROCESS: u32 = 2;
    const TO_FOREGROUND: i32 = 1;
    const TO_UI_ELEMENT: i32 = 4;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let ns_app = NSApplication::sharedApplication(mtm);
    if !visible {
        for window in ns_app.windows() {
            window.setCanHide(false);
        }
    }
    let psn = ProcessSerialNumber {
        high_long_of_psn: 0,
        low_long_of_psn: CURRENT_PROCESS,
    };
    unsafe {
        TransformProcessType(
            &psn,
            if visible {
                TO_FOREGROUND
            } else {
                TO_UI_ELEMENT
            },
        );
    }
}

#[cfg(target_os = "macos")]
fn style_macos_status_item(tray: &tauri::tray::TrayIcon) {
    let _ = tray.set_icon(None);
    let _ = tray.set_title(Some("UB"));
    let _ = tray.with_inner_tray_icon(|inner| {
        use objc2_app_kit::{NSCellImagePosition, NSFont, NSFontWeightSemibold};
        use objc2_foundation::{MainThreadMarker, NSString};
        let Some(item) = inner.ns_status_item() else {
            return;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(button) = item.button(mtm) else {
            return;
        };
        button.setImage(None);
        button.setImagePosition(NSCellImagePosition::NoImage);
        button.setTitle(&NSString::from_str("UB"));
        let font = NSFont::systemFontOfSize_weight(11.0, unsafe { NSFontWeightSemibold });
        button.setFont(Some(&font));
        button.setToolTip(Some(&NSString::from_str("UsageBar")));
        // tray-icon 会叠一层 wantsLayer 的 TrayTarget，未设 icon 时会留下白色方块
        let subs = button.subviews();
        for view in subs.iter() {
            view.setHidden(true);
        }
    });
}

pub(crate) fn apply_tray(app: &AppHandle, prefs: &Prefs) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    #[cfg(target_os = "macos")]
    {
        style_macos_status_item(&tray);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = tray.set_title(None::<&str>);
        if let Some(icon) = app.default_window_icon() {
            let _ = tray.set_icon(Some(icon.clone()));
        }
    }
    let _ = tray.set_show_menu_on_left_click(true);
    let summary = TRAY_SUMMARY
        .lock()
        .map(|value| value.clone())
        .unwrap_or_default();
    let _ = tray.set_tooltip(Some(if summary.is_empty() {
        "UsageBar"
    } else {
        &summary
    }));
    if let Ok(menu) = build_tray_menu(app, prefs) {
        let _ = tray.set_menu(Some(menu));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("UsageBar")
                .build(),
        )
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            fetch_usage,
            get_prefs,
            set_prefs,
            set_visible_providers,
            set_visible_metrics,
            set_tray_summary,
            set_bar_look,
            refresh_window_blur,
            set_window_blur,
            place_bar,
            set_pointer,
            set_menu_open,
            set_update_card,
            format_reset,
            dismiss_reset_notice,
            dismiss_credit_notice,
            app_version,
            check_update,
            install_update,
            open_release_page,
            quit,
            os_name,
            tray_rect
        ])
        .setup(|app| {
            // 应用整体为暗色设计，原生菜单 / 设置窗口统一走暗色外观。
            app.handle().set_theme(Some(tauri::Theme::Dark));
            let prefs = prefs::load();
            for (label, win) in app.webview_windows() {
                if label != "bar" && label != "tip" {
                    let _ = win.hide();
                }
            }
            if let Some(bar) = app.get_webview_window("bar") {
                let _ = bar.set_always_on_top(true);
                let _ = bar.set_skip_taskbar(true);
                let _ = bar.set_focusable(false);
                #[cfg(target_os = "macos")]
                {
                    let _ = bar.set_visible_on_all_workspaces(true);
                }
            }
            if let Some(tip) = app.get_webview_window("tip") {
                let _ = tip.set_always_on_top(true);
                let _ = tip.set_skip_taskbar(true);
                let _ = tip.set_ignore_cursor_events(true);
                let _ = tip.set_focusable(false);
                #[cfg(target_os = "macos")]
                {
                    let _ = tip.set_visible_on_all_workspaces(true);
                }
            }
            if let Some(update) = app.get_webview_window("update") {
                let _ = update.set_always_on_top(true);
                let _ = update.set_skip_taskbar(true);
                let _ = update.set_ignore_cursor_events(true);
                let _ = update.set_focusable(false);
                #[cfg(target_os = "macos")]
                {
                    let _ = update.set_visible_on_all_workspaces(true);
                }
            }
            app.manage(Overlay::new(prefs.clone()));
            if overlay::place(app.handle()).is_none() {
                if let Some(bar) = app.get_webview_window("bar") {
                    let _ = bar.show();
                }
            }
            overlay::spawn(app.handle().clone());
            if app.tray_by_id("main").is_none() {
                let menu = build_tray_menu(app.handle(), &prefs)?;
                let mut tray = TrayIconBuilder::with_id("main")
                    .tooltip("UsageBar")
                    .menu(&menu)
                    .show_menu_on_left_click(true)
                    .on_menu_event(|app, event| {
                        handle_tray_menu_event(app, event.id().as_ref());
                    })
                    .on_tray_icon_event(|tray, event| {
                        handle_tray_icon_event(tray.app_handle(), &event);
                    });
                #[cfg(target_os = "macos")]
                {
                    tray = tray.title("UB");
                }
                #[cfg(not(target_os = "macos"))]
                {
                    if let Some(icon) = app.default_window_icon() {
                        tray = tray.icon(icon.clone());
                    }
                }
                let tray = tray.build(app)?;
                #[cfg(not(target_os = "macos"))]
                let _ = &tray;
                #[cfg(target_os = "macos")]
                {
                    style_macos_status_item(&tray);
                }
            } else if let Some(tray) = app.tray_by_id("main") {
                if let Ok(menu) = build_tray_menu(app.handle(), &prefs) {
                    let _ = tray.set_menu(Some(menu));
                }
                tray.on_menu_event(|app, event| {
                    handle_tray_menu_event(app, event.id().as_ref());
                });
                tray.on_tray_icon_event(|tray, event| {
                    handle_tray_icon_event(tray.app_handle(), &event);
                });
            }
            apply_tray(app.handle(), &prefs);
            apply_app_icon(app.handle(), &prefs);
            frost::apply_from_app(app.handle());
            for label in ["bar", "tip", "update"] {
                if let Some(win) = app.get_webview_window(label) {
                    let handle = app.handle().clone();
                    let _ = win.on_window_event(move |event| {
                        if matches!(
                            event,
                            tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_)
                        ) {
                            frost::apply_from_app(&handle);
                        }
                    });
                }
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building UsageBar")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                let _ = app.emit("usagebar-reopen", ());
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (app, event);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::sanitize_tray_summary;

    #[test]
    fn tray_summary_drops_empty_control_text_and_extra_providers() {
        let raw = (0..14)
            .map(|index| format!("\nProvider {index}: 5h 90%\u{0}"))
            .collect::<String>();
        let clean = sanitize_tray_summary(&raw);
        assert_eq!(clean.lines().count(), crate::prefs::SLOT_MAX);
        assert!(!clean.contains('\0'));
        assert!(clean.lines().all(|line| line.len() <= 160));
    }
}
