mod i18n;
mod overlay;
mod prefs;
mod providers;

use overlay::Overlay;
use prefs::Prefs;
use serde::Serialize;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::MacosLauncher;

#[tauri::command]
async fn fetch_usage(app: AppHandle) -> Vec<providers::ProviderSnapshot> {
    let ids = app
        .try_state::<Overlay>()
        .and_then(|state| state.prefs.lock().ok().map(|p| p.visible_providers.clone()))
        .unwrap_or_else(|| prefs::load().visible_providers);
    tauri::async_runtime::spawn_blocking(move || providers::fetch_selected(&ids))
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
fn set_prefs(app: AppHandle, mut prefs: Prefs) {
    prefs.visible_providers = prefs::normalize_visible(&prefs.visible_providers);
    if let Some(state) = app.try_state::<Overlay>() {
        if let Ok(mut slot) = state.prefs.lock() {
            *slot = prefs.clone();
        }
    }
    prefs::save(&prefs);
    apply_tray(&app, &prefs);
    let _ = overlay::place(&app);
    let _ = app.emit("usagebar-prefs", &prefs);
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
fn set_menu_open(app: AppHandle, open: bool) {
    overlay::MENU_OPEN.store(open, std::sync::atomic::Ordering::Relaxed);
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
fn quit(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn open_settings(app: AppHandle) {
    let loc = app
        .try_state::<Overlay>()
        .and_then(|state| state.prefs.lock().ok().map(|p| p.locale.clone()))
        .unwrap_or_else(|| prefs::load().locale);
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.set_title(i18n::tools_window(&loc));
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
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

fn build_tray_menu(app: &AppHandle, prefs: &Prefs) -> tauri::Result<Menu<tauri::Wry>> {
    let loc = prefs.locale.as_str();
    let refresh = MenuItem::with_id(app, "refresh", i18n::refresh_now(loc), true, None::<&str>)?;
    let intervals = [15u64, 30, 60, 120, 300, 600, 0];
    let mut refresh_items = Vec::new();
    for sec in intervals {
        refresh_items.push(CheckMenuItem::with_id(
            app,
            format!("interval:{sec}"),
            i18n::interval_label(loc, sec),
            true,
            prefs.refresh_interval == sec,
            None::<&str>,
        )?);
    }
    let refresh_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = refresh_items
        .iter()
        .map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
        .collect();
    let refresh_sub =
        Submenu::with_id_and_items(app, "auto-refresh", i18n::auto_refresh(loc), true, &refresh_refs)?;
    let lock = MenuItem::with_id(
        app,
        "lock",
        i18n::lock_position(loc, prefs.locked),
        true,
        None::<&str>,
    )?;
    let click = CheckMenuItem::with_id(
        app,
        "click",
        i18n::click_through(loc),
        true,
        prefs.click_through,
        None::<&str>,
    )?;
    let left = CheckMenuItem::with_id(
        app,
        "snap:left",
        i18n::snap_left(loc),
        true,
        prefs.edge == "left",
        None::<&str>,
    )?;
    let right = CheckMenuItem::with_id(
        app,
        "snap:right",
        i18n::snap_right(loc),
        true,
        prefs.edge == "right",
        None::<&str>,
    )?;
    let top = CheckMenuItem::with_id(
        app,
        "snap:top",
        i18n::snap_top(loc),
        true,
        prefs.edge == "top",
        None::<&str>,
    )?;
    let bottom = CheckMenuItem::with_id(
        app,
        "snap:bottom",
        i18n::snap_bottom(loc),
        true,
        prefs.edge == "bottom",
        None::<&str>,
    )?;
    let style_full = CheckMenuItem::with_id(
        app,
        "style:full",
        i18n::ring_usage(loc),
        true,
        prefs.display_style != "icons",
        None::<&str>,
    )?;
    let style_icons = CheckMenuItem::with_id(
        app,
        "style:icons",
        i18n::transparent_icons(loc),
        true,
        prefs.display_style == "icons",
        None::<&str>,
    )?;
    let style_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = vec![&style_full, &style_icons];
    let style_sub =
        Submenu::with_id_and_items(app, "display-style", i18n::display_style(loc), true, &style_refs)?;
    let zh = i18n::is_zh(loc);
    let lang_en = CheckMenuItem::with_id(app, "locale:en", "English", true, !zh, None::<&str>)?;
    let lang_zh = CheckMenuItem::with_id(app, "locale:zh", "中文", true, zh, None::<&str>)?;
    let lang_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = vec![&lang_en, &lang_zh];
    let lang_sub = Submenu::with_id_and_items(app, "language", i18n::language(loc), true, &lang_refs)?;
    let tools = MenuItem::with_id(app, "tools", i18n::tools(loc), true, None::<&str>)?;
    let login = CheckMenuItem::with_id(
        app,
        "login",
        i18n::open_at_login(loc),
        true,
        prefs.launch_at_login,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", i18n::quit(loc), true, Some("q"))?;
    let sep = PredefinedMenuItem::separator(app)?;
    Menu::with_items(
        app,
        &[
            &refresh,
            &refresh_sub,
            &sep,
            &lock,
            &click,
            &sep,
            &left,
            &right,
            &top,
            &bottom,
            &style_sub,
            &lang_sub,
            &tools,
            &sep,
            &login,
            &sep,
            &quit,
        ],
    )
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
        let has_image = button.image().is_some();
        let _ = std::fs::write(
            "/tmp/ub-tray.txt",
            format!(
                "styled title-only has_image={has_image} subviews={}\n",
                subs.len()
            ),
        );
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
    if let Ok(menu) = build_tray_menu(app, prefs) {
        let _ = tray.set_menu(Some(menu));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .invoke_handler(tauri::generate_handler![
            fetch_usage,
            get_prefs,
            set_prefs,
            place_bar,
            set_pointer,
            set_menu_open,
            format_reset,
            quit,
            os_name,
            tray_rect,
            open_settings
        ])
        .setup(|app| {
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
            if let Some(settings) = app.get_webview_window("settings") {
                let hide = settings.clone();
                settings.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = hide.hide();
                    }
                });
            }

            let prefs = prefs::load();
            app.manage(Overlay::new(prefs.clone()));
            overlay::spawn(app.handle().clone());
            if app.tray_by_id("main").is_none() {
                let menu = build_tray_menu(app.handle(), &prefs)?;
                let mut tray = TrayIconBuilder::with_id("main")
                    .tooltip("UsageBar")
                    .menu(&menu)
                    .show_menu_on_left_click(true)
                    .on_menu_event(|app, event| {
                        let _ = app.emit("usagebar-menu", event.id().as_ref());
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
                #[cfg(target_os = "macos")]
                {
                    style_macos_status_item(&tray);
                }
            } else if let Some(tray) = app.tray_by_id("main") {
                if let Ok(menu) = build_tray_menu(app.handle(), &prefs) {
                    let _ = tray.set_menu(Some(menu));
                }
                tray.on_menu_event(|app, event| {
                    let _ = app.emit("usagebar-menu", event.id().as_ref());
                });
            }
            apply_tray(app.handle(), &prefs);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running UsageBar");
}
