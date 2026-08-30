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
async fn fetch_usage() -> Vec<providers::ProviderSnapshot> {
    tauri::async_runtime::spawn_blocking(providers::fetch_all)
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
    if let Some(state) = app.try_state::<Overlay>() {
        if let Ok(mut slot) = state.prefs.lock() {
            *slot = prefs.clone();
        }
    }
    prefs::save(&prefs);
    apply_tray(&app, &prefs);
    let _ = overlay::place(&app);
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
    let refresh = MenuItem::with_id(app, "refresh", "立即刷新", true, None::<&str>)?;
    let intervals = [
        (15u64, "15 秒"),
        (30, "30 秒"),
        (60, "60 秒"),
        (120, "2 分钟"),
        (300, "5 分钟"),
        (600, "10 分钟"),
        (0, "关闭"),
    ];
    let mut refresh_items = Vec::new();
    for (sec, title) in intervals {
        refresh_items.push(CheckMenuItem::with_id(
            app,
            format!("interval:{sec}"),
            title,
            true,
            prefs.refresh_interval == sec,
            None::<&str>,
        )?);
    }
    let refresh_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = refresh_items
        .iter()
        .map(|i| i as &dyn tauri::menu::IsMenuItem<tauri::Wry>)
        .collect();
    let refresh_sub = Submenu::with_id_and_items(app, "auto-refresh", "自动刷新", true, &refresh_refs)?;
    let lock = MenuItem::with_id(
        app,
        "lock",
        if prefs.locked {
            "解锁位置"
        } else {
            "锁定位置"
        },
        true,
        None::<&str>,
    )?;
    let click = CheckMenuItem::with_id(
        app,
        "click",
        "不阻挡下方点击",
        true,
        prefs.click_through,
        None::<&str>,
    )?;
    let left = CheckMenuItem::with_id(app, "snap:left", "贴左边", true, prefs.edge == "left", None::<&str>)?;
    let right = CheckMenuItem::with_id(app, "snap:right", "贴右边", true, prefs.edge == "right", None::<&str>)?;
    let top = CheckMenuItem::with_id(app, "snap:top", "贴上边", true, prefs.edge == "top", None::<&str>)?;
    let bottom = CheckMenuItem::with_id(
        app,
        "snap:bottom",
        "贴下边",
        true,
        prefs.edge == "bottom",
        None::<&str>,
    )?;
    let style_full = CheckMenuItem::with_id(
        app,
        "style:full",
        "圆环用量",
        true,
        prefs.display_style != "icons",
        None::<&str>,
    )?;
    let style_icons = CheckMenuItem::with_id(
        app,
        "style:icons",
        "透明图标",
        true,
        prefs.display_style == "icons",
        None::<&str>,
    )?;
    let style_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> = vec![&style_full, &style_icons];
    let style_sub = Submenu::with_id_and_items(app, "display-style", "显示样式", true, &style_refs)?;
    let login = CheckMenuItem::with_id(
        app,
        "login",
        "登录时打开",
        true,
        prefs.launch_at_login,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "退出 UsageBar", true, Some("q"))?;
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
            tray_rect
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
