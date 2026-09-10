//! Bar silhouette shared with `src/main.js` (`dockPath`, `gearCenter`, `radii`).
//! Keep these constants and formulas in lockstep with the frontend.
#![allow(dead_code)]

pub const BAR_THICK: f64 = 46.0;
pub const BAR_PAD: f64 = 44.0;
pub const BAR_SLOT: f64 = 48.0;
pub const BAR_H_THICK: f64 = 40.0;
pub const BAR_H_BASE: f64 = 24.0;
pub const BAR_H_SLOT: f64 = 59.0;
pub const GEAR_R: f64 = 14.0;
pub const GEAR_CARVE: f64 = 18.5;
pub const GEAR_END: f64 = 19.0;
pub const GEAR_ALONG: f64 = 22.0;
pub const GEAR_ALONG_FLOAT: f64 = 40.0;
const K: f64 = 0.5523;
const S: f64 = 0.72;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Move(f64, f64),
    Line(f64, f64),
    Cubic {
        c1x: f64,
        c1y: f64,
        c2x: f64,
        c2y: f64,
        x: f64,
        y: f64,
    },
    Close,
}

pub fn gear_along(edge: &str) -> f64 {
    if edge == "floating" {
        GEAR_ALONG_FLOAT
    } else {
        GEAR_ALONG
    }
}

pub fn is_vertical(edge: &str) -> bool {
    matches!(edge, "left" | "right" | "floating")
}

pub fn radii(w: f64, h: f64, along: f64) -> (f64, f64) {
    let base = w.min(h);
    let mut fillet = base * 0.35;
    let mut corner = base * 0.42;
    let limit = along / 2.0;
    let total = fillet + corner;
    if total > limit && total > 0.0 {
        let s = limit / total;
        fillet *= s;
        corner *= s;
    }
    (fillet, corner)
}

pub fn bar_size(edge: &str, slots: usize) -> (f64, f64) {
    let n = slots as f64;
    let g = gear_along(edge);
    if is_vertical(edge) {
        (BAR_THICK, BAR_PAD + BAR_SLOT * n + g)
    } else {
        (BAR_H_BASE + BAR_H_SLOT * n + g, BAR_H_THICK)
    }
}

pub fn gear_center(w: f64, h: f64, edge: &str) -> (f64, f64) {
    let cross = GEAR_CARVE;
    match edge {
        "right" => (cross, h - GEAR_END),
        "left" => (w - cross, h - GEAR_END),
        "top" => (w - GEAR_END, h - cross),
        "bottom" => (w - GEAR_END, cross),
        _ => (w / 2.0, h - GEAR_END),
    }
}

/// Inset so the rounded frost slab stays inside `dockPath` (inner top/bottom at `fillet`).
pub const FROST_PAD: f64 = 1.0;
pub const FROST_WALL_EXTRA: f64 = 2.0;
pub const FROST_POD_INSET: f64 = 1.0;

#[derive(Clone, Copy, Debug)]
pub struct FrostBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub radius: f64,
}

/// SVG / top-left box for the bar-body frost window, before wall overhang.
pub fn frost_body_box(w: f64, h: f64, edge: &str) -> FrostBox {
    let g = gear_along(edge);
    let vertical = is_vertical(edge);
    let bw = if vertical { w } else { w - g };
    let bh = if vertical { h - g } else { h };
    let along = if vertical { bh } else { bw };
    let (fillet, corner) = if edge == "floating" {
        (0.0, bw.min(bh) / 2.0)
    } else {
        radii(bw, bh, along)
    };
    let pad = FROST_PAD;
    let inset = fillet;
    let (x, y, fw, fh) = match edge {
        "right" => (
            pad,
            inset + pad,
            (bw - pad).max(1.0),
            (bh - 2.0 * inset - 2.0 * pad).max(1.0),
        ),
        "left" => (
            0.0,
            inset + pad,
            (bw - pad).max(1.0),
            (bh - 2.0 * inset - 2.0 * pad).max(1.0),
        ),
        "top" => (
            inset + pad,
            0.0,
            (bw - 2.0 * inset - 2.0 * pad).max(1.0),
            (bh - pad).max(1.0),
        ),
        "bottom" => (
            inset + pad,
            pad,
            (bw - 2.0 * inset - 2.0 * pad).max(1.0),
            (bh - pad).max(1.0),
        ),
        _ => (
            pad,
            inset + pad,
            (bw - 2.0 * pad).max(1.0),
            (bh - 2.0 * inset - 2.0 * pad).max(1.0),
        ),
    };
    let radius = corner.min(fw.min(fh) / 2.0);
    FrostBox {
        x,
        y,
        w: fw,
        h: fh,
        radius,
    }
}

pub fn frost_pod_radius() -> f64 {
    (GEAR_R - FROST_POD_INSET).max(1.0)
}

pub fn dock_ops(w: f64, h: f64, edge: &str) -> Vec<Op> {
    if edge == "floating" {
        return floating_ops(w, h);
    }
    let vertical = edge == "left" || edge == "right";
    let g = gear_along(edge);
    let bw = if vertical { w } else { w - g };
    let bh = if vertical { h - g } else { h };
    let along = if vertical { bh } else { bw };
    let (f, r) = radii(bw, bh, along);
    let pad = 1.0;
    let a = S;
    let x = |t: f64| if edge == "right" { w - t } else { t };
    let yh = |t: f64| if edge == "top" { t } else { h - t };
    if vertical {
        vec![
            Op::Move(x(-pad), 0.0),
            Op::Line(x(0.0), 0.0),
            Op::Cubic {
                c1x: x(0.0),
                c1y: a * f,
                c2x: x((1.0 - a) * f),
                c2y: f,
                x: x(f),
                y: f,
            },
            Op::Line(x(w - r), f),
            Op::Cubic {
                c1x: x(w - r + K * r),
                c1y: f,
                c2x: x(w),
                c2y: f + r - K * r,
                x: x(w),
                y: f + r,
            },
            Op::Line(x(w), bh - f - r),
            Op::Cubic {
                c1x: x(w),
                c1y: bh - f - r + K * r,
                c2x: x(w - r + K * r),
                c2y: bh - f,
                x: x(w - r),
                y: bh - f,
            },
            Op::Line(x(f), bh - f),
            Op::Cubic {
                c1x: x((1.0 - a) * f),
                c1y: bh - f,
                c2x: x(0.0),
                c2y: bh - a * f,
                x: x(0.0),
                y: bh,
            },
            Op::Line(x(-pad), bh),
            Op::Close,
        ]
    } else {
        vec![
            Op::Move(0.0, yh(-pad)),
            Op::Line(0.0, yh(0.0)),
            Op::Cubic {
                c1x: a * f,
                c1y: yh(0.0),
                c2x: f,
                c2y: yh((1.0 - a) * f),
                x: f,
                y: yh(f),
            },
            Op::Line(f, yh(h - r)),
            Op::Cubic {
                c1x: f,
                c1y: yh(h - r + K * r),
                c2x: f + r - K * r,
                c2y: yh(h),
                x: f + r,
                y: yh(h),
            },
            Op::Line(bw - f - r, yh(h)),
            Op::Cubic {
                c1x: bw - f - r + K * r,
                c1y: yh(h),
                c2x: bw - f,
                c2y: yh(h - r + K * r),
                x: bw - f,
                y: yh(h - r),
            },
            Op::Line(bw - f, yh(f)),
            Op::Cubic {
                c1x: bw - f,
                c1y: yh((1.0 - a) * f),
                c2x: bw - a * f,
                c2y: yh(0.0),
                x: bw,
                y: yh(0.0),
            },
            Op::Line(bw, yh(-pad)),
            Op::Close,
        ]
    }
}

fn floating_ops(w: f64, h: f64) -> Vec<Op> {
    let hh = h - gear_along("floating");
    let r = w.min(hh) / 2.0;
    vec![
        Op::Move(r, 0.0),
        Op::Line(w - r, 0.0),
        Op::Cubic {
            c1x: w - r + K * r,
            c1y: 0.0,
            c2x: w,
            c2y: r - K * r,
            x: w,
            y: r,
        },
        Op::Line(w, hh - r),
        Op::Cubic {
            c1x: w,
            c1y: hh - r + K * r,
            c2x: w - r + K * r,
            c2y: hh,
            x: w - r,
            y: hh,
        },
        Op::Line(r, hh),
        Op::Cubic {
            c1x: r - K * r,
            c1y: hh,
            c2x: 0.0,
            c2y: hh - r + K * r,
            x: 0.0,
            y: hh - r,
        },
        Op::Line(0.0, r),
        Op::Cubic {
            c1x: 0.0,
            c1y: r - K * r,
            c2x: r - K * r,
            c2y: 0.0,
            x: r,
            y: 0.0,
        },
        Op::Close,
    ]
}

/// SVG `d` using the same interpolation as JS `dockPath`.
pub fn dock_path(w: f64, h: f64, edge: &str) -> String {
    if edge == "floating" {
        let hh = h - gear_along(edge);
        let r = w.min(hh) / 2.0;
        return format!(
            "M{},{} H{} A{},{} 0 0 1 {},{} V{} A{},{} 0 0 1 {},{} H{} A{},{} 0 0 1 {},{} V{} A{},{} 0 0 1 {},{} Z",
            js_num(r),
            js_num(0.0),
            js_num(w - r),
            js_num(r),
            js_num(r),
            js_num(w),
            js_num(r),
            js_num(hh - r),
            js_num(r),
            js_num(r),
            js_num(w - r),
            js_num(hh),
            js_num(r),
            js_num(r),
            js_num(r),
            js_num(0.0),
            js_num(hh - r),
            js_num(r),
            js_num(r),
            js_num(r),
            js_num(r),
            js_num(0.0),
        );
    }
    let ops = dock_ops(w, h, edge);
    let mut out = String::new();
    for op in ops {
        match op {
            Op::Move(x, y) => {
                out.push('M');
                out.push_str(&js_num(x));
                out.push(',');
                out.push_str(&js_num(y));
            }
            Op::Line(x, y) => {
                out.push_str(" L");
                out.push_str(&js_num(x));
                out.push(',');
                out.push_str(&js_num(y));
            }
            Op::Cubic {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                out.push_str(" C");
                out.push_str(&js_num(c1x));
                out.push(',');
                out.push_str(&js_num(c1y));
                out.push(' ');
                out.push_str(&js_num(c2x));
                out.push(',');
                out.push_str(&js_num(c2y));
                out.push(' ');
                out.push_str(&js_num(x));
                out.push(',');
                out.push_str(&js_num(y));
            }
            Op::Close => out.push_str(" Z"),
        }
    }
    out
}

/// JavaScript `Number#toString` for the finite values this geometry produces.
fn js_num(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n.is_infinite() {
        return if n.is_sign_positive() {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if n == 0.0 {
        return "0".into();
    }
    if n.fract() == 0.0 && n.abs() <= ((1i64 << 53) as f64) {
        return format!("{}", n as i64);
    }
    // V8 prints some binary leftovers (e.g. 16.099999999999998) instead of 16.1.
    let raw = format!("{n:.16}");
    let mut s = raw;
    if let Some(dot) = s.find('.') {
        while s.len() > dot + 1 && s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s == "-0" {
        "0".into()
    } else {
        s
    }
}

pub fn flip_ops_cocoa(ops: &[Op], h: f64) -> Vec<Op> {
    ops.iter()
        .map(|op| match *op {
            Op::Move(x, y) => Op::Move(x, h - y),
            Op::Line(x, y) => Op::Line(x, h - y),
            Op::Cubic {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => Op::Cubic {
                c1x,
                c1y: h - c1y,
                c2x,
                c2y: h - c2y,
                x,
                y: h - y,
            },
            Op::Close => Op::Close,
        })
        .collect()
}

/// Window-local rectangles (points, Cocoa Y-up) covering the D-body path.
pub fn body_region_rects(w: f64, h: f64, edge: &str, scale: f64) -> Vec<[f64; 4]> {
    let scale = scale.max(1.0);
    let ops = flip_ops_cocoa(&dock_ops(w, h, edge), h);
    let polys = flatten_ops(&ops);
    scanline_rects(&polys, w, h, scale)
}

fn flatten_ops(ops: &[Op]) -> Vec<Vec<(f64, f64)>> {
    let mut polys = Vec::new();
    let mut cur = Vec::new();
    let mut start = (0.0, 0.0);
    let mut pen = (0.0, 0.0);
    for op in ops {
        match *op {
            Op::Move(x, y) => {
                if cur.len() >= 2 {
                    polys.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                start = (x, y);
                pen = start;
                cur.push(pen);
            }
            Op::Line(x, y) => {
                pen = (x, y);
                cur.push(pen);
            }
            Op::Cubic {
                c1x,
                c1y,
                c2x,
                c2y,
                x,
                y,
            } => {
                flatten_cubic(pen, (c1x, c1y), (c2x, c2y), (x, y), 0.15, &mut cur);
                pen = (x, y);
            }
            Op::Close => {
                if cur.last() != Some(&start) {
                    cur.push(start);
                }
                if cur.len() >= 2 {
                    polys.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                pen = start;
            }
        }
    }
    if cur.len() >= 2 {
        polys.push(cur);
    }
    polys
}

fn flatten_cubic(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    p3: (f64, f64),
    tol: f64,
    out: &mut Vec<(f64, f64)>,
) {
    fn dist2(a: (f64, f64), b: (f64, f64)) -> f64 {
        let dx = a.0 - b.0;
        let dy = a.1 - b.1;
        dx * dx + dy * dy
    }
    fn split(
        p0: (f64, f64),
        p1: (f64, f64),
        p2: (f64, f64),
        p3: (f64, f64),
    ) -> [(f64, f64); 7] {
        let m01 = ((p0.0 + p1.0) * 0.5, (p0.1 + p1.1) * 0.5);
        let m12 = ((p1.0 + p2.0) * 0.5, (p1.1 + p2.1) * 0.5);
        let m23 = ((p2.0 + p3.0) * 0.5, (p2.1 + p3.1) * 0.5);
        let m012 = ((m01.0 + m12.0) * 0.5, (m01.1 + m12.1) * 0.5);
        let m123 = ((m12.0 + m23.0) * 0.5, (m12.1 + m23.1) * 0.5);
        let mid = ((m012.0 + m123.0) * 0.5, (m012.1 + m123.1) * 0.5);
        [p0, m01, m012, mid, m123, m23, p3]
    }
    let d1 = dist_to_seg(p1, p0, p3);
    let d2 = dist_to_seg(p2, p0, p3);
    if d1 <= tol && d2 <= tol && dist2(p0, p3) < 64.0 * 64.0 {
        out.push(p3);
        return;
    }
    if out.len() > 8000 {
        out.push(p3);
        return;
    }
    let s = split(p0, p1, p2, p3);
    flatten_cubic(s[0], s[1], s[2], s[3], tol, out);
    flatten_cubic(s[3], s[4], s[5], s[6], tol, out);
}

fn dist_to_seg(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let vx = b.0 - a.0;
    let vy = b.1 - a.1;
    let len2 = vx * vx + vy * vy;
    if len2 <= 1e-12 {
        return ((p.0 - a.0).hypot(p.1 - a.1)).abs();
    }
    let t = ((p.0 - a.0) * vx + (p.1 - a.1) * vy) / len2;
    let t = t.clamp(0.0, 1.0);
    let qx = a.0 + t * vx;
    let qy = a.1 + t * vy;
    (p.0 - qx).hypot(p.1 - qy)
}

fn scanline_rects(polys: &[Vec<(f64, f64)>], w: f64, h: f64, scale: f64) -> Vec<[f64; 4]> {
    let ph = ((h * scale).round() as i32).max(1);
    let mut rects: Vec<[f64; 4]> = Vec::new();
    let mut xs = Vec::new();
    for y in 0..ph {
        let y_mid = (y as f64 + 0.5) / scale;
        xs.clear();
        for poly in polys {
            let n = poly.len();
            if n < 2 {
                continue;
            }
            for i in 0..n - 1 {
                let (x0, y0) = poly[i];
                let (x1, y1) = poly[i + 1];
                if (y0 <= y_mid && y1 > y_mid) || (y1 <= y_mid && y0 > y_mid) {
                    let t = (y_mid - y0) / (y1 - y0);
                    xs.push(x0 + t * (x1 - x0));
                }
            }
        }
        if xs.len() < 2 {
            continue;
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut i = 0;
        while i + 1 < xs.len() {
            let x0 = xs[i].clamp(0.0, w);
            let x1 = xs[i + 1].clamp(0.0, w);
            if x1 - x0 >= 0.25 / scale {
                let rx = x0;
                let rw = x1 - x0;
                let ry = y as f64 / scale;
                let rh = 1.0 / scale;
                if let Some(last) = rects.last_mut() {
                    let same_x = (last[0] - rx).abs() < 0.02 && (last[2] - rw).abs() < 0.02;
                    let adj_y = (last[1] + last[3] - ry).abs() < 0.02;
                    if same_x && adj_y {
                        last[3] += rh;
                        i += 2;
                        continue;
                    }
                }
                rects.push([rx, ry, rw, rh]);
            }
            i += 2;
        }
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Case {
        n: usize,
        edge: &'static str,
        js: &'static str,
        cx: f64,
        cy: f64,
    }

    fn cases() -> [Case; 10] {
        [
            Case {
                n: 1,
                edge: "right",
                js: "M47,0 L46,0 C46,11.591999999999999 41.492,16.099999999999998 29.900000000000002,16.099999999999998 L19.32,16.099999999999998 C8.649563999999998,16.099999999999998 0,24.749564 0,35.42 L0,56.580000000000005 C0,67.25043600000001 8.649563999999998,75.9 19.32,75.9 L29.900000000000002,75.9 C41.492,75.9 46,80.408 46,92 L47,92 Z",
                cx: 18.5,
                cy: 95.0,
            },
            Case {
                n: 1,
                edge: "left",
                js: "M-1,0 L0,0 C0,11.591999999999999 4.508,16.099999999999998 16.099999999999998,16.099999999999998 L26.68,16.099999999999998 C37.350436,16.099999999999998 46,24.749564 46,35.42 L46,56.580000000000005 C46,67.25043600000001 37.350436,75.9 26.68,75.9 L16.099999999999998,75.9 C4.508,75.9 0,80.408 0,92 L-1,92 Z",
                cx: 27.5,
                cy: 95.0,
            },
            Case {
                n: 1,
                edge: "top",
                js: "M0,-1 L0,0 C10.08,0 14,3.9200000000000004 14,14 L14,23.2 C14,32.47864 21.52136,40 30.8,40 L52.2,40 C61.478640000000006,40 69,32.47864 69,23.2 L69,14 C69,3.9200000000000004 72.92,0 83,0 L83,-1 Z",
                cx: 86.0,
                cy: 21.5,
            },
            Case {
                n: 1,
                edge: "bottom",
                js: "M0,41 L0,40 C10.08,40 14,36.08 14,26 L14,16.8 C14,7.521360000000001 21.52136,0 30.8,0 L52.2,0 C61.478640000000006,0 69,7.521360000000001 69,16.8 L69,26 C69,36.08 72.92,40 83,40 L83,41 Z",
                cx: 86.0,
                cy: 18.5,
            },
            Case {
                n: 1,
                edge: "floating",
                js: "M23,0 H23 A23,23 0 0 1 46,23 V69 A23,23 0 0 1 23,92 H23 A23,23 0 0 1 0,69 V23 A23,23 0 0 1 23,0 Z",
                cx: 23.0,
                cy: 113.0,
            },
            Case {
                n: 4,
                edge: "right",
                js: "M47,0 L46,0 C46,11.591999999999999 41.492,16.099999999999998 29.900000000000002,16.099999999999998 L19.32,16.099999999999998 C8.649563999999998,16.099999999999998 0,24.749564 0,35.42 L0,200.58 C0,211.250436 8.649563999999998,219.9 19.32,219.9 L29.900000000000002,219.9 C41.492,219.9 46,224.40800000000002 46,236 L47,236 Z",
                cx: 18.5,
                cy: 239.0,
            },
            Case {
                n: 4,
                edge: "left",
                js: "M-1,0 L0,0 C0,11.591999999999999 4.508,16.099999999999998 16.099999999999998,16.099999999999998 L26.68,16.099999999999998 C37.350436,16.099999999999998 46,24.749564 46,35.42 L46,200.58 C46,211.250436 37.350436,219.9 26.68,219.9 L16.099999999999998,219.9 C4.508,219.9 0,224.40800000000002 0,236 L-1,236 Z",
                cx: 27.5,
                cy: 239.0,
            },
            Case {
                n: 4,
                edge: "top",
                js: "M0,-1 L0,0 C10.08,0 14,3.9200000000000004 14,14 L14,23.2 C14,32.47864 21.52136,40 30.8,40 L229.2,40 C238.47863999999998,40 246,32.47864 246,23.2 L246,14 C246,3.9200000000000004 249.92,0 260,0 L260,-1 Z",
                cx: 263.0,
                cy: 21.5,
            },
            Case {
                n: 4,
                edge: "bottom",
                js: "M0,41 L0,40 C10.08,40 14,36.08 14,26 L14,16.8 C14,7.521360000000001 21.52136,0 30.8,0 L229.2,0 C238.47863999999998,0 246,7.521360000000001 246,16.8 L246,26 C246,36.08 249.92,40 260,40 L260,41 Z",
                cx: 263.0,
                cy: 18.5,
            },
            Case {
                n: 4,
                edge: "floating",
                js: "M23,0 H23 A23,23 0 0 1 46,23 V213 A23,23 0 0 1 23,236 H23 A23,23 0 0 1 0,213 V23 A23,23 0 0 1 23,0 Z",
                cx: 23.0,
                cy: 257.0,
            },
        ]
    }

    fn parse_nums(d: &str) -> Vec<f64> {
        let mut nums = Vec::new();
        let mut buf = String::new();
        let mut chars = d.chars().peekable();
        while let Some(c) = chars.next() {
            let start = c == '-' || c == '+' || c.is_ascii_digit() || c == '.';
            if start {
                buf.clear();
                buf.push(c);
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_digit() || n == '.' || n == 'e' || n == 'E' {
                        buf.push(n);
                        chars.next();
                    } else if (n == '-' || n == '+') && buf.ends_with(['e', 'E']) {
                        buf.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if let Ok(v) = buf.parse::<f64>() {
                    nums.push(v);
                }
            }
        }
        nums
    }

    fn letters(d: &str) -> String {
        d.chars()
            .filter(|c| c.is_ascii_alphabetic())
            .collect()
    }

    #[test]
    fn dock_path_matches_js() {
        for case in cases() {
            let (w, h) = bar_size(case.edge, case.n);
            let rust = dock_path(w, h, case.edge);
            assert_eq!(letters(&rust), letters(case.js), "{} n={}", case.edge, case.n);
            let got = parse_nums(&rust);
            let want = parse_nums(case.js);
            assert_eq!(got.len(), want.len(), "{} n={} rust={rust}", case.edge, case.n);
            for (a, b) in got.iter().zip(want.iter()) {
                assert!(
                    (a - b).abs() <= 1e-9,
                    "{} n={} {a} vs {b}\nrust={rust}\njs={}",
                    case.edge,
                    case.n,
                    case.js
                );
            }
            let (cx, cy) = gear_center(w, h, case.edge);
            assert_eq!((cx, cy), (case.cx, case.cy), "gear {} n={}", case.edge, case.n);
        }
    }

    fn hit(rects: &[[f64; 4]], x: f64, y: f64) -> bool {
        rects
            .iter()
            .any(|r| x >= r[0] && x <= r[0] + r[2] && y >= r[1] && y <= r[1] + r[3])
    }

    #[test]
    fn right_body_region_leaves_gear_gap() {
        let (w, h) = bar_size("right", 4);
        let rects = body_region_rects(w, h, "right", 2.0);
        assert!(!rects.is_empty());
        let (gx, gy) = gear_center(w, h, "right");
        let below_body_svg = gy + 10.0;
        assert!(
            !hit(&rects, gx, h - below_body_svg),
            "area below the D-body should stay clear of the body region"
        );
        assert!(
            !hit(&rects, 2.0, h - 4.0),
            "square window corner outside the D-curve should stay clear"
        );
        assert!(
            hit(&rects, 23.0, h - 80.0),
            "D-body center should be covered"
        );
    }

    #[test]
    fn frost_body_box_stays_inside_d_inner() {
        let (w, h) = bar_size("right", 4);
        let g = gear_along("right");
        let bh = h - g;
        let (fillet, _) = radii(w, bh, bh);
        let box_ = frost_body_box(w, h, "right");
        assert!(box_.y + 1e-9 >= fillet);
        assert!(box_.y + box_.h <= bh - fillet + 1e-9);
        assert!(box_.x >= 0.0);
        assert!(box_.x + box_.w <= w + 1e-9);
        assert!(box_.radius > 0.0);
    }
}
