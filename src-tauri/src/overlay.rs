use crate::prefs::{self, Prefs};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager};

const SNAP: f64 = 32.0;
const SNAP_KEEP: f64 = 48.0;
const SNAP_SWITCH: f64 = 16.0;
const BAR_THICK: f64 = 46.0;
const BAR_PAD: f64 = 44.0;
const BAR_SLOT: f64 = 48.0;
const BAR_H_THICK: f64 = 40.0;
const BAR_H_BASE: f64 = 24.0;
const BAR_H_SLOT: f64 = 59.0;
const ICONS_THICK: f64 = 26.0;
const ICONS_BASE: f64 = 8.0;
const ICONS_SLOT: f64 = 21.0;
/// 圆环样式末尾追加的设置区长度：0.0.9 条身原样结束，其后留空放悬空齿轮舱。
const GEAR_ALONG: f64 = 22.0;
/// 浮动模式胶囊下方的预留（胶囊无贴边弧，舱体整体更靠下）。
const GEAR_ALONG_FLOAT: f64 = 40.0;
/// 齿轮舱横向圆心：距条内边 GEAR_CARVE（= 半径 14 + 缝隙 4.5）。
const GEAR_CARVE: f64 = 18.5;
/// 齿轮舱圆心距窗口末端的距离。
const GEAR_END: f64 = 19.0;
const GEAR_HIT: f64 = 30.0;

pub static JS_LEFT: AtomicBool = AtomicBool::new(false);
pub static MENU_OPEN: AtomicBool = AtomicBool::new(false);
/// 菜单卡片逻辑坐标 [x, y, w, h]，用于判断点击是否落在面板上。
pub static MENU_CARD: Mutex<Option<[f64; 4]>> = Mutex::new(None);

pub fn set_menu_card(card: Option<[f64; 4]>) {
    if let Ok(mut slot) = MENU_CARD.lock() {
        *slot = card;
    }
}


pub struct Overlay {
    pub prefs: Mutex<Prefs>,
    placed: AtomicBool,
    drag: Mutex<Option<Drag>>,
    ignoring: AtomicBool,
    last_size: Mutex<(u32, u32)>,
    monitors: Mutex<(Instant, Vec<Screen>)>,
    frame: Mutex<Option<CachedFrame>>,
    hovered: Mutex<Option<String>>,
    tip_hot: Mutex<bool>,
    bar_over: AtomicBool,
    was_left: AtomicBool,
    press_on_gear: AtomicBool,
}

struct CachedFrame {
    scale: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    at: Instant,
}

struct Drag {
    start_mx: f64,
    start_my: f64,
    start_x: f64,
    start_y: f64,
    start_w: f64,
    start_h: f64,
    start_edge: String,
    started: bool,
}

#[derive(Clone)]
struct Screen {
    name: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    wx: f64,
    wy: f64,
    ww: f64,
    wh: f64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LayoutPayload {
    edge: String,
    along: f64,
    floating_x: f64,
    floating_y: f64,
    last_x: f64,
    last_y: f64,
    screen_name: String,
    dragging: bool,
}

impl Overlay {
    pub fn new(prefs: Prefs) -> Self {
        Self {
            prefs: Mutex::new(prefs),
            placed: AtomicBool::new(false),
            drag: Mutex::new(None),
            ignoring: AtomicBool::new(false),
            last_size: Mutex::new((0, 0)),
            monitors: Mutex::new((Instant::now() - Duration::from_secs(10), Vec::new())),
            frame: Mutex::new(None),
            hovered: Mutex::new(None),
            tip_hot: Mutex::new(false),
            bar_over: AtomicBool::new(false),
            was_left: AtomicBool::new(false),
            press_on_gear: AtomicBool::new(false),
        }
    }

    pub fn clear_hover(&self) {
        if let Ok(mut g) = self.hovered.lock() {
            *g = None;
        }
        if let Ok(mut g) = self.tip_hot.lock() {
            *g = false;
        }
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::Builder::new()
        .name("usagebar-input".into())
        .spawn(move || {
            let pending = Arc::new(AtomicBool::new(false));
            loop {
                std::thread::sleep(Duration::from_millis(16));
                if pending.swap(true, Ordering::AcqRel) {
                    continue;
                }
                let pending2 = pending.clone();
                let handle = app.clone();
                if app
                    .run_on_main_thread(move || {
                        if let Some(state) = handle.try_state::<Overlay>() {
                            if !state.placed.load(Ordering::Relaxed) {
                                let _ = place(&handle);
                            }
                        }
                        tick(&handle);
                        pending2.store(false, Ordering::Release);
                    })
                    .is_err()
                {
                    pending.store(false, Ordering::Release);
                }
            }
        })
        .ok();
}

fn tick(app: &AppHandle) {
    let Some(state) = app.try_state::<Overlay>() else {
        return;
    };
    let state: &Overlay = &*state;
    let Some(bar) = app.get_webview_window("bar") else {
        return;
    };
    let prefs = state.prefs.lock().ok().map(|g| g.clone()).unwrap_or_default();
    let Some((scale, px, py, pw, ph)) = window_frame(&bar, state, false) else {
        return;
    };
    let cursor = match bar.cursor_position() {
        Ok(c) => c,
        Err(_) => return,
    };
    let left = left_mouse_down();
    let over = cursor.x >= px - 4.0
        && cursor.x <= px + pw + 4.0
        && cursor.y >= py - 4.0
        && cursor.y <= py + ph + 4.0;

    let dragging = state
        .drag
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|d| d.started))
        .unwrap_or(false);

    let menu_open = MENU_OPEN.load(Ordering::Relaxed);
    if !left || menu_open {
        let ignore = prefs.click_through && !over && !dragging && !menu_open;
        set_ignore(&state, &bar, ignore);
    }

    emit_bar_over(app, state, over);

    let on_gear = over
        && over_gear(
            &prefs.edge,
            is_icons(&prefs),
            scale,
            px,
            py,
            pw,
            ph,
            cursor.x,
            cursor.y,
        );
    let pressed = left && !state.was_left.swap(left, Ordering::AcqRel);
    if !left {
        state.press_on_gear.store(false, Ordering::Release);
    } else if pressed && on_gear {
        state.press_on_gear.store(true, Ordering::Release);
    }

    // 菜单打开时：点齿轮和菜单卡片以外即关闭。
    if pressed && menu_open && !on_gear && !over_menu_card(scale, cursor.x, cursor.y) {
        let _ = app.emit("usagebar-menu", "close");
    }

    if !prefs.locked
        && left
        && over
        && !on_gear
        && !state.press_on_gear.load(Ordering::Relaxed)
        && !menu_open
    {
        let mut drag = state.drag.lock().unwrap_or_else(|e| e.into_inner());
        if drag.is_none() {
            let (scale, px, py, pw, ph) =
                window_frame(&bar, state, true).unwrap_or((scale, px, py, pw, ph));
            *drag = Some(Drag {
                start_mx: cursor.x,
                start_my: cursor.y,
                start_x: px / scale,
                start_y: py / scale,
                start_w: pw / scale,
                start_h: ph / scale,
                start_edge: prefs.edge.clone(),
                started: false,
            });
        }
    }

    if left {
        if apply_drag(app, &state, &bar, &prefs, cursor.x, cursor.y, scale) {
            emit_hover(app, state, None);
            return;
        }
    } else if state.drag.lock().ok().map(|g| g.is_some()).unwrap_or(false) {
        finish_drag(app, &state, &bar, cursor.x, cursor.y, scale);
    }

    update_hover(
        app,
        state,
        &prefs,
        over,
        dragging,
        scale,
        px,
        py,
        pw,
        ph,
        cursor.x,
        cursor.y,
    );
}

fn update_hover(
    app: &AppHandle,
    state: &Overlay,
    prefs: &Prefs,
    over: bool,
    dragging: bool,
    scale: f64,
    px: f64,
    py: f64,
    pw: f64,
    ph: f64,
    mx: f64,
    my: f64,
) {
    if dragging || MENU_OPEN.load(Ordering::Relaxed) {
        emit_tip_hot(app, state, false);
        emit_hover(app, state, None);
        return;
    }
    let hot = over_tip(app, mx, my);
    emit_tip_hot(app, state, hot);
    if hot {
        return;
    }
    let next = hover_id(prefs, over, scale, px, py, pw, ph, mx, my);
    emit_hover(app, state, next);
}

fn hover_id(
    prefs: &Prefs,
    over: bool,
    scale: f64,
    px: f64,
    py: f64,
    pw: f64,
    ph: f64,
    mx: f64,
    my: f64,
) -> Option<String> {
    if !over {
        return None;
    }
    let local_x = (mx - px) / scale;
    let local_y = (my - py) / scale;
    let slots = prefs::display_slots(prefs);
    let count = slots.len().max(1) as f64;
    if is_icons(prefs) {
        let vertical = is_vertical(&prefs.edge);
        let along = if vertical { ph / scale } else { pw / scale };
        let coord = if vertical { local_y } else { local_x };
        if along <= 0.0 {
            return None;
        }
        let slot = along / count;
        let idx = (coord / slot).floor() as i32;
        let mid = (idx as f64 + 0.5) * slot;
        if idx >= 0 && (idx as usize) < slots.len() && (coord - mid).abs() < slot * 0.48 {
            return Some(slots[idx as usize].clone());
        }
        return None;
    }
    let vertical = is_vertical(&prefs.edge);
    let (inset_start, inset_end) = if vertical {
        (20.0, 20.0 + GEAR_ALONG)
    } else {
        (18.0, 12.0 + GEAR_ALONG)
    };
    let along = if vertical { ph / scale } else { pw / scale };
    let coord = if vertical { local_y } else { local_x };
    let inner = along - inset_start - inset_end;
    if inner <= 0.0 {
        return None;
    }
    let slot = inner / count;
    let idx = ((coord - inset_start) / slot).floor() as i32;
    let mid = inset_start + (idx as f64 + 0.5) * slot;
    if idx >= 0 && (idx as usize) < slots.len() && (coord - mid).abs() < slot * 0.48 {
        return Some(slots[idx as usize].clone());
    }
    None
}

fn over_menu_card(scale: f64, mx: f64, my: f64) -> bool {
    let card = match MENU_CARD.lock() {
        Ok(g) => *g,
        Err(e) => *e.into_inner(),
    };
    let Some([x, y, w, h]) = card else {
        return false;
    };
    let s = if scale > 0.0 { scale } else { 1.0 };
    mx >= x * s - 6.0
        && mx <= (x + w) * s + 6.0
        && my >= y * s - 6.0
        && my <= (y + h) * s + 6.0
}

fn over_tip(app: &AppHandle, mx: f64, my: f64) -> bool {
    let Some(tip) = app.get_webview_window("tip") else {
        return false;
    };
    if !tip.is_visible().unwrap_or(false) {
        return false;
    }
    let Ok(pos) = tip.outer_position() else {
        return false;
    };
    let Ok(size) = tip.outer_size() else {
        return false;
    };
    mx >= pos.x as f64 - 8.0
        && mx <= pos.x as f64 + size.width as f64 + 8.0
        && my >= pos.y as f64 - 8.0
        && my <= pos.y as f64 + size.height as f64 + 8.0
}

fn emit_tip_hot(app: &AppHandle, state: &Overlay, hot: bool) {
    let mut slot = match state.tip_hot.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    if *slot == hot {
        return;
    }
    *slot = hot;
    let _ = app.emit("usagebar-tip-hover", hot);
}

fn emit_bar_over(app: &AppHandle, state: &Overlay, over: bool) {
    if state.bar_over.swap(over, Ordering::AcqRel) != over {
        let _ = app.emit("usagebar-over", over);
    }
}

fn emit_hover(app: &AppHandle, state: &Overlay, next: Option<String>) {
    let mut slot = match state.hovered.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    if *slot == next {
        return;
    }
    *slot = next.clone();
    let _ = app.emit("usagebar-hover", next);
}

fn apply_drag(
    app: &AppHandle,
    state: &Overlay,
    bar: &tauri::WebviewWindow,
    prefs: &Prefs,
    mx: f64,
    my: f64,
    scale: f64,
) -> bool {
    let mut drag = match state.drag.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };
    let Some(session) = drag.as_mut() else {
        return false;
    };
    let dx = mx - session.start_mx;
    let dy = my - session.start_my;
    if !session.started {
        if (dx * dx + dy * dy).sqrt() < 8.0 * scale {
            return false;
        }
        session.started = true;
        emit_layout(app, prefs, true);
    }
    let w = session.start_w;
    let h = session.start_h;
    let mut x = session.start_x + dx / scale;
    let mut y = session.start_y + dy / scale;
    let screens = screens(app, state);
    let Some(screen) = monitor_at(&screens, mx / scale, my / scale) else {
        return true;
    };
    x = clamp(x, screen.x - 8.0, screen.x + screen.w - w + 8.0);
    y = clamp(y, screen.y - 8.0, screen.y + screen.h - h + 8.0);
    let probe = nearest(x, y, w, h, &session.start_edge, &screen);
    if probe != "floating" && is_vertical(&probe) == is_vertical(&session.start_edge) {
        let along = if is_vertical(&probe) { y } else { x };
        let glued = glued_frame(&probe, w, h, along, &screen);
        set_frame(state, bar, glued.0, glued.1, glued.2, glued.3);
    } else {
        set_frame(state, bar, x, y, w, h);
    }
    true
}

fn finish_drag(
    app: &AppHandle,
    state: &Overlay,
    bar: &tauri::WebviewWindow,
    mx: f64,
    my: f64,
    scale: f64,
) {
    let session = {
        let mut drag = state.drag.lock().unwrap_or_else(|e| e.into_inner());
        drag.take()
    };
    let Some(session) = session else {
        return;
    };
    if !session.started {
        return;
    }
    let Ok(pos) = bar.outer_position() else {
        emit_layout(app, &state.prefs.lock().map(|g| g.clone()).unwrap_or_default(), false);
        return;
    };
    let screens = screens(app, state);
    let lx = pos.x as f64 / scale;
    let ly = pos.y as f64 / scale;
    let cx = mx / scale;
    let cy = my / scale;
    let screen = monitor_at(&screens, cx, cy)
        .or_else(|| monitor_at(&screens, lx, ly))
        .cloned();
    let Some(screen) = screen else {
        return;
    };
    let mut prefs = state.prefs.lock().map(|g| g.clone()).unwrap_or_default();
    prefs.screen_name = screen.name.clone();
    prefs.edge = nearest(
        lx,
        ly,
        session.start_w,
        session.start_h,
        &session.start_edge,
        &screen,
    );
    let (w, h) = bar_size(&prefs.edge, &prefs);
    if prefs.edge == "floating" {
        prefs.floating_x = lx;
        prefs.floating_y = ly;
        prefs.along = lx;
        prefs.last_x = lx;
        prefs.last_y = ly;
        set_frame(state, bar, lx, ly, w, h);
    } else {
        prefs.along = if is_vertical(&prefs.edge) { ly } else { lx };
        let glued = glued_frame(&prefs.edge, w, h, prefs.along, &screen);
        prefs.last_x = glued.0;
        prefs.last_y = glued.1;
        set_frame(state, bar, glued.0, glued.1, glued.2, glued.3);
    }
    if let Ok(mut g) = state.prefs.lock() {
        *g = prefs.clone();
    }
    prefs::save(&prefs);
    emit_layout(app, &prefs, false);
}

fn window_frame(
    bar: &tauri::WebviewWindow,
    state: &Overlay,
    force: bool,
) -> Option<(f64, f64, f64, f64, f64)> {
    if !force {
        if let Ok(cache) = state.frame.lock() {
            if let Some(f) = cache.as_ref() {
                if f.at.elapsed() < Duration::from_millis(250) {
                    return Some((f.scale, f.x, f.y, f.w, f.h));
                }
            }
        }
    }
    let scale = bar.scale_factor().ok()?;
    let pos = bar.outer_position().ok()?;
    let size = bar.outer_size().ok()?;
    let frame = CachedFrame {
        scale,
        x: pos.x as f64,
        y: pos.y as f64,
        w: size.width as f64,
        h: size.height as f64,
        at: Instant::now(),
    };
    let out = (frame.scale, frame.x, frame.y, frame.w, frame.h);
    if let Ok(mut g) = state.frame.lock() {
        *g = Some(frame);
    }
    Some(out)
}

fn set_frame(state: &Overlay, bar: &tauri::WebviewWindow, x: f64, y: f64, w: f64, h: f64) {
    let _ = bar.set_min_size(Some(LogicalSize::new(1.0, 1.0)));
    let _ = bar.set_size(LogicalSize::new(w, h));
    let _ = bar.set_position(LogicalPosition::new(x, y));
    if let Ok(mut g) = state.last_size.lock() {
        *g = (w.round() as u32, h.round() as u32);
    }
    if let Ok(scale) = bar.scale_factor() {
        if let Ok(mut g) = state.frame.lock() {
            *g = Some(CachedFrame {
                scale,
                x: x * scale,
                y: y * scale,
                w: w * scale,
                h: h * scale,
                at: Instant::now(),
            });
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BarFrame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub fn place(app: &AppHandle) -> Option<BarFrame> {
    let state = app.try_state::<Overlay>()?;
    let state: &Overlay = &*state;
    let bar = app.get_webview_window("bar")?;
    let prefs = state.prefs.lock().ok()?.clone();
    let screens = screens(app, state);
    if screens.is_empty() {
        return None;
    }
    let screen = pick_screen(&screens, &prefs)?;
    let (w, h) = bar_size(&prefs.edge, &prefs);
    let mut along = prefs.along;
    if prefs::is_unset(along) {
        along = if !prefs::is_unset(prefs.last_x) && !prefs::is_unset(prefs.last_y) {
            if is_vertical(&prefs.edge) {
                prefs.last_y
            } else {
                prefs.last_x
            }
        } else if is_vertical(&prefs.edge) {
            screen.wy + screen.wh / 2.0 - h / 2.0
        } else {
            screen.wx + screen.ww / 2.0 - w / 2.0
        };
    }
    let (x, y, w, h) = if prefs.edge == "floating" {
        let mut x = if !prefs::is_unset(prefs.last_x) {
            prefs.last_x
        } else {
            prefs.floating_x
        };
        let mut y = if !prefs::is_unset(prefs.last_y) {
            prefs.last_y
        } else {
            prefs.floating_y
        };
        if x == 0.0 && y == 0.0 {
            x = screen.wx + screen.ww / 2.0 - w / 2.0;
            y = screen.wy + screen.wh / 2.0 - h / 2.0;
        }
        (
            clamp(x, screen.wx, screen.wx + screen.ww - w),
            clamp(y, screen.wy, screen.wy + screen.wh - h),
            w,
            h,
        )
    } else {
        glued_frame(&prefs.edge, w, h, along, &screen)
    };
    set_frame(state, &bar, x, y, w, h);
    let _ = bar.show();
    state.placed.store(true, Ordering::Release);
    let mut next = prefs;
    next.along = if is_vertical(&next.edge) { y } else { x };
    next.last_x = x;
    next.last_y = y;
    next.screen_name = screen.name;
    if let Ok(mut g) = state.prefs.lock() {
        *g = next.clone();
    }
    prefs::save(&next);
    Some(BarFrame { x, y, w, h })
}

fn pick_screen(screens: &[Screen], prefs: &Prefs) -> Option<Screen> {
    if !prefs.screen_name.is_empty() {
        if let Some(s) = screens.iter().find(|s| s.name == prefs.screen_name) {
            return Some(s.clone());
        }
    }
    if !prefs::is_unset(prefs.last_x) && !prefs::is_unset(prefs.last_y) {
        if let Some(s) = monitor_at(screens, prefs.last_x, prefs.last_y) {
            return Some(s.clone());
        }
    }
    screens.first().cloned()
}

fn set_ignore(state: &Overlay, bar: &tauri::WebviewWindow, ignore: bool) {
    if state.ignoring.swap(ignore, Ordering::AcqRel) != ignore {
        let _ = bar.set_ignore_cursor_events(ignore);
    }
}

fn emit_layout(app: &AppHandle, prefs: &Prefs, dragging: bool) {
    let payload = LayoutPayload {
        edge: prefs.edge.clone(),
        along: prefs.along,
        floating_x: prefs.floating_x,
        floating_y: prefs.floating_y,
        last_x: prefs.last_x,
        last_y: prefs.last_y,
        screen_name: prefs.screen_name.clone(),
        dragging,
    };
    let _ = app.emit("usagebar-layout", payload);
}

fn screens(app: &AppHandle, state: &Overlay) -> Vec<Screen> {
    if let Ok(cache) = state.monitors.lock() {
        if cache.0.elapsed() < Duration::from_millis(400) && !cache.1.is_empty() {
            return cache.1.clone();
        }
    }
    let list = app
        .available_monitors()
        .unwrap_or_default()
        .into_iter()
        .map(|m| {
            let s = m.scale_factor();
            let p = m.position();
            let size = m.size();
            let wa = m.work_area();
            Screen {
                name: m.name().cloned().unwrap_or_default(),
                x: p.x as f64 / s,
                y: p.y as f64 / s,
                w: size.width as f64 / s,
                h: size.height as f64 / s,
                wx: wa.position.x as f64 / s,
                wy: wa.position.y as f64 / s,
                ww: wa.size.width as f64 / s,
                wh: wa.size.height as f64 / s,
            }
        })
        .collect::<Vec<_>>();
    if let Ok(mut cache) = state.monitors.lock() {
        *cache = (Instant::now(), list.clone());
    }
    list
}

fn monitor_at(list: &[Screen], x: f64, y: f64) -> Option<&Screen> {
    list.iter()
        .find(|s| x >= s.x && x <= s.x + s.w && y >= s.y && y <= s.y + s.h)
        .or_else(|| list.first())
}

fn nearest(x: f64, y: f64, w: f64, h: f64, current: &str, screen: &Screen) -> String {
    let right = (x + w - (screen.x + screen.w)).abs();
    let left = (x - screen.x).abs();
    let top = (y - screen.wy).abs();
    let bottom = (y + h - (screen.wy + screen.wh)).abs();
    let pairs = [
        ("right", right),
        ("left", left),
        ("top", top),
        ("bottom", bottom),
    ];
    let best = pairs
        .iter()
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .copied()
        .unwrap_or(("floating", f64::MAX));
    if let Some(d) = pairs.iter().find(|p| p.0 == current).map(|p| p.1) {
        if d <= SNAP_KEEP {
            if best.0 != current && best.1 + SNAP_SWITCH < d && best.1 <= SNAP {
                return best.0.to_string();
            }
            return current.to_string();
        }
    }
    if best.1 <= SNAP {
        best.0.to_string()
    } else {
        "floating".into()
    }
}

fn glued_frame(edge: &str, w: f64, h: f64, along: f64, screen: &Screen) -> (f64, f64, f64, f64) {
    match edge {
        "right" => {
            let y = clamp(along, screen.wy, screen.wy + screen.wh - h);
            (screen.x + screen.w - w + 1.0, y, w, h)
        }
        "left" => {
            let y = clamp(along, screen.wy, screen.wy + screen.wh - h);
            (screen.x - 1.0, y, w, h)
        }
        "top" => {
            let x = clamp(along, screen.wx, screen.wx + screen.ww - w);
            (x, screen.wy, w, h)
        }
        "bottom" => {
            let x = clamp(along, screen.wx, screen.wx + screen.ww - w);
            (x, screen.wy + screen.wh - h, w, h)
        }
        _ => {
            let x = clamp(along, screen.wx, screen.wx + screen.ww - w);
            (x, screen.wy + 80.0, w, h)
        }
    }
}

fn is_icons(prefs: &Prefs) -> bool {
    prefs.display_style == "icons"
}

fn bar_size(edge: &str, prefs: &Prefs) -> (f64, f64) {
    let n = prefs::slot_count(prefs) as f64;
    if is_icons(prefs) {
        let along = ICONS_BASE + ICONS_SLOT * n;
        if is_vertical(edge) {
            (ICONS_THICK, along)
        } else {
            (along, ICONS_THICK)
        }
    } else {
        let g = if edge == "floating" {
            GEAR_ALONG_FLOAT
        } else {
            GEAR_ALONG
        };
        if is_vertical(edge) {
            (BAR_THICK, BAR_PAD + BAR_SLOT * n + g)
        } else {
            (BAR_H_BASE + BAR_H_SLOT * n + g, BAR_H_THICK)
        }
    }
}

fn gear_center(edge: &str, w: f64, h: f64) -> (f64, f64) {
    match edge {
        "right" => (GEAR_CARVE, h - GEAR_END),
        "left" => (w - GEAR_CARVE, h - GEAR_END),
        "top" => (w - GEAR_END, h - GEAR_CARVE),
        "bottom" => (w - GEAR_END, GEAR_CARVE),
        _ => (w / 2.0, h - GEAR_END),
    }
}

fn over_gear(
    edge: &str,
    icons: bool,
    scale: f64,
    px: f64,
    py: f64,
    pw: f64,
    ph: f64,
    mx: f64,
    my: f64,
) -> bool {
    if icons {
        return false;
    }
    let w = pw / scale;
    let h = ph / scale;
    let lx = (mx - px) / scale;
    let ly = (my - py) / scale;
    let (cx, cy) = gear_center(edge, w, h);
    let r = GEAR_HIT / 2.0;
    (lx - cx).abs() <= r && (ly - cy).abs() <= r
}

fn is_vertical(edge: &str) -> bool {
    matches!(edge, "left" | "right" | "floating")
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi.max(lo))
}

fn left_mouse_down() -> bool {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        platform_left()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        JS_LEFT.load(Ordering::Relaxed)
    }
}

#[cfg(target_os = "macos")]
fn platform_left() -> bool {
    objc2_app_kit::NSEvent::pressedMouseButtons() & 1 != 0
}

#[cfg(target_os = "windows")]
fn platform_left() -> bool {
    #[link(name = "user32")]
    extern "system" {
        fn GetAsyncKeyState(v_key: i32) -> i16;
    }
    unsafe { GetAsyncKeyState(0x01) < 0 }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_left() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(name: &str, x: f64, y: f64) -> Screen {
        Screen {
            name: name.into(),
            x,
            y,
            w: 1920.0,
            h: 1080.0,
            wx: x,
            wy: y,
            ww: 1920.0,
            wh: 1080.0,
        }
    }

    #[test]
    fn pick_screen_uses_saved_name() {
        let list = vec![screen("A", 0.0, 0.0), screen("B", 1920.0, 0.0)];
        let mut prefs = Prefs::default();
        prefs.screen_name = "B".into();
        prefs.last_x = 100.0;
        prefs.last_y = 100.0;
        assert_eq!(pick_screen(&list, &prefs).unwrap().name, "B");
    }

    #[test]
    fn pick_screen_falls_back_to_last_point_when_name_changes() {
        let list = vec![screen("Built-in", 0.0, 0.0), screen("HDMI", 1920.0, 0.0)];
        let mut prefs = Prefs::default();
        prefs.screen_name = "Monitor #8193".into();
        prefs.last_x = 2100.0;
        prefs.last_y = 200.0;
        assert_eq!(pick_screen(&list, &prefs).unwrap().name, "HDMI");
    }
}
