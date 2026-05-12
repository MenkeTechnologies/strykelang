//! Geometric primitives, complex numbers, color-space extras, and
//! trig extras. Pure functions, no external crates.
//!
//! Complex numbers are represented as `{ re, im }` hashrefs. Points
//! are 2-element arrays `[x, y]`; 3D points are `[x, y, z]`.
//! Polygons are arrayrefs of points.

use crate::value::StrykeValue;

fn arg_f64(args: &[StrykeValue], idx: usize) -> Option<f64> {
    args.get(idx).map(|v| v.to_number())
}

fn point_xy(v: &StrykeValue) -> Option<(f64, f64)> {
    let arr = v.as_array_ref()?;
    let g = arr.read();
    if g.len() < 2 {
        return None;
    }
    Some((g[0].to_number(), g[1].to_number()))
}

// ══════════════════════════════════════════════════════════════════════
// Complex numbers
// ══════════════════════════════════════════════════════════════════════

fn mk_complex(re: f64, im: f64) -> StrykeValue {
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::sync::Arc;
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("re".to_string(), StrykeValue::float(re));
    h.insert("im".to_string(), StrykeValue::float(im));
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

fn unpack_complex(v: &StrykeValue) -> Option<(f64, f64)> {
    let h = v.as_hash_ref()?;
    let g = h.read();
    let re = g.get("re").map(|x| x.to_number())?;
    let im = g.get("im").map(|x| x.to_number())?;
    Some((re, im))
}

/// `complex_new(RE, IM)` — construct `{ re, im }`.
pub fn complex_new(args: &[StrykeValue]) -> StrykeValue {
    mk_complex(arg_f64(args, 0).unwrap_or(0.0), arg_f64(args, 1).unwrap_or(0.0))
}

/// `complex_real(C)` — real part.
pub fn complex_real(args: &[StrykeValue]) -> StrykeValue {
    match args.first().and_then(unpack_complex) {
        Some((re, _)) => StrykeValue::float(re),
        None => StrykeValue::UNDEF,
    }
}

/// `complex_imag(C)` — imaginary part.
pub fn complex_imag(args: &[StrykeValue]) -> StrykeValue {
    match args.first().and_then(unpack_complex) {
        Some((_, im)) => StrykeValue::float(im),
        None => StrykeValue::UNDEF,
    }
}

/// `complex_polar(C)` — `{ r, theta }` polar form.
pub fn complex_polar(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some((re, im)) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("r".to_string(), StrykeValue::float((re * re + im * im).sqrt()));
    h.insert("theta".to_string(), StrykeValue::float(im.atan2(re)));
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

/// `complex_from_polar(R, THETA)` — polar → rectangular.
pub fn complex_from_polar(args: &[StrykeValue]) -> StrykeValue {
    let r = arg_f64(args, 0).unwrap_or(0.0);
    let theta = arg_f64(args, 1).unwrap_or(0.0);
    mk_complex(r * theta.cos(), r * theta.sin())
}

/// `complex_magnitude(C)` / `complex_abs(C)` — `|z|`.
pub fn complex_magnitude(args: &[StrykeValue]) -> StrykeValue {
    match args.first().and_then(unpack_complex) {
        Some((re, im)) => StrykeValue::float((re * re + im * im).sqrt()),
        None => StrykeValue::UNDEF,
    }
}

pub fn complex_abs(args: &[StrykeValue]) -> StrykeValue {
    complex_magnitude(args)
}

/// `complex_phase(C)` / `complex_angle(C)` — arg(z) in radians.
pub fn complex_phase(args: &[StrykeValue]) -> StrykeValue {
    match args.first().and_then(unpack_complex) {
        Some((re, im)) => StrykeValue::float(im.atan2(re)),
        None => StrykeValue::UNDEF,
    }
}

pub fn complex_angle(args: &[StrykeValue]) -> StrykeValue {
    complex_phase(args)
}

/// `complex_conjugate(C)` — `{ re, -im }`.
pub fn complex_conjugate(args: &[StrykeValue]) -> StrykeValue {
    match args.first().and_then(unpack_complex) {
        Some((re, im)) => mk_complex(re, -im),
        None => StrykeValue::UNDEF,
    }
}

/// `complex_add(A, B)` — `(a.re + b.re) + (a.im + b.im)i`.
pub fn complex_add(args: &[StrykeValue]) -> StrykeValue {
    let Some(a) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    let Some(c) = args.get(1).and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    mk_complex(a.0 + c.0, a.1 + c.1)
}

pub fn complex_sub(args: &[StrykeValue]) -> StrykeValue {
    let Some(a) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    let Some(c) = args.get(1).and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    mk_complex(a.0 - c.0, a.1 - c.1)
}

pub fn complex_mul(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, b)) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    let Some((c, d)) = args.get(1).and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    mk_complex(a * c - b * d, a * d + b * c)
}

pub fn complex_div(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, b)) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    let Some((c, d)) = args.get(1).and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    let den = c * c + d * d;
    if den == 0.0 {
        return StrykeValue::UNDEF;
    }
    mk_complex((a * c + b * d) / den, (b * c - a * d) / den)
}

pub fn complex_pow(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, b)) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    let n = arg_f64(args, 1).unwrap_or(1.0);
    let r = (a * a + b * b).sqrt();
    let theta = b.atan2(a);
    let new_r = r.powf(n);
    let new_theta = theta * n;
    mk_complex(new_r * new_theta.cos(), new_r * new_theta.sin())
}

pub fn complex_sqrt(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, b)) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    let r = (a * a + b * b).sqrt();
    let new_r = r.sqrt();
    let theta = b.atan2(a) / 2.0;
    mk_complex(new_r * theta.cos(), new_r * theta.sin())
}

pub fn complex_exp(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, b)) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    let ex = a.exp();
    mk_complex(ex * b.cos(), ex * b.sin())
}

pub fn complex_log(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, b)) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    mk_complex((a * a + b * b).sqrt().ln(), b.atan2(a))
}

pub fn complex_sin(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, b)) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    mk_complex(a.sin() * b.cosh(), a.cos() * b.sinh())
}

pub fn complex_cos(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, b)) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    mk_complex(a.cos() * b.cosh(), -a.sin() * b.sinh())
}

pub fn complex_tan(args: &[StrykeValue]) -> StrykeValue {
    let s = complex_sin(args);
    let c = complex_cos(args);
    complex_div(&[s, c])
}

pub fn complex_sinh(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, b)) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    mk_complex(a.sinh() * b.cos(), a.cosh() * b.sin())
}

pub fn complex_cosh(args: &[StrykeValue]) -> StrykeValue {
    let Some((a, b)) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    mk_complex(a.cosh() * b.cos(), a.sinh() * b.sin())
}

pub fn complex_tanh(args: &[StrykeValue]) -> StrykeValue {
    let s = complex_sinh(args);
    let c = complex_cosh(args);
    complex_div(&[s, c])
}

pub fn complex_equal(args: &[StrykeValue]) -> StrykeValue {
    let Some(a) = args.first().and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    let Some(c) = args.get(1).and_then(unpack_complex) else {
        return StrykeValue::UNDEF;
    };
    let eps = arg_f64(args, 2).unwrap_or(1e-12);
    let eq = (a.0 - c.0).abs() < eps && (a.1 - c.1).abs() < eps;
    StrykeValue::integer(if eq { 1 } else { 0 })
}

// ══════════════════════════════════════════════════════════════════════
// Geometry — points, lines, polygons
// ══════════════════════════════════════════════════════════════════════

/// `point_angle(P1, P2)` — bearing (radians) from P1 to P2.
pub fn point_angle(args: &[StrykeValue]) -> StrykeValue {
    let Some((x1, y1)) = args.first().and_then(point_xy) else {
        return StrykeValue::UNDEF;
    };
    let Some((x2, y2)) = args.get(1).and_then(point_xy) else {
        return StrykeValue::UNDEF;
    };
    StrykeValue::float((y2 - y1).atan2(x2 - x1))
}

/// `line_intersect(L1, L2)` — intersection point of two infinite lines
/// given as `[P, Q]`. Returns `[x, y]` or undef if parallel.
pub fn line_intersect(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let unpack_line = |v: &StrykeValue| -> Option<((f64, f64), (f64, f64))> {
        let arr = v.as_array_ref()?;
        let g = arr.read();
        if g.len() < 2 {
            return None;
        }
        Some((point_xy(&g[0])?, point_xy(&g[1])?))
    };
    let Some((p1, p2)) = args.first().and_then(unpack_line) else {
        return StrykeValue::UNDEF;
    };
    let Some((p3, p4)) = args.get(1).and_then(unpack_line) else {
        return StrykeValue::UNDEF;
    };
    let den = (p1.0 - p2.0) * (p3.1 - p4.1) - (p1.1 - p2.1) * (p3.0 - p4.0);
    if den.abs() < 1e-12 {
        return StrykeValue::UNDEF;
    }
    let t = ((p1.0 - p3.0) * (p3.1 - p4.1) - (p1.1 - p3.1) * (p3.0 - p4.0)) / den;
    let x = p1.0 + t * (p2.0 - p1.0);
    let y = p1.1 + t * (p2.1 - p1.1);
    StrykeValue::array_ref(Arc::new(RwLock::new(vec![
        StrykeValue::float(x),
        StrykeValue::float(y),
    ])))
}

/// `line_segment_intersect(S1, S2)` — same as `line_intersect` but
/// returns undef if the intersection point is outside either segment.
pub fn line_segment_intersect(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let unpack_seg = |v: &StrykeValue| -> Option<((f64, f64), (f64, f64))> {
        let arr = v.as_array_ref()?;
        let g = arr.read();
        if g.len() < 2 {
            return None;
        }
        Some((point_xy(&g[0])?, point_xy(&g[1])?))
    };
    let Some((p1, p2)) = args.first().and_then(unpack_seg) else {
        return StrykeValue::UNDEF;
    };
    let Some((p3, p4)) = args.get(1).and_then(unpack_seg) else {
        return StrykeValue::UNDEF;
    };
    let den = (p1.0 - p2.0) * (p3.1 - p4.1) - (p1.1 - p2.1) * (p3.0 - p4.0);
    if den.abs() < 1e-12 {
        return StrykeValue::UNDEF;
    }
    let t = ((p1.0 - p3.0) * (p3.1 - p4.1) - (p1.1 - p3.1) * (p3.0 - p4.0)) / den;
    let u = -((p1.0 - p2.0) * (p1.1 - p3.1) - (p1.1 - p2.1) * (p1.0 - p3.0)) / den;
    if !(0.0..=1.0).contains(&t) || !(0.0..=1.0).contains(&u) {
        return StrykeValue::UNDEF;
    }
    let x = p1.0 + t * (p2.0 - p1.0);
    let y = p1.1 + t * (p2.1 - p1.1);
    StrykeValue::array_ref(Arc::new(RwLock::new(vec![
        StrykeValue::float(x),
        StrykeValue::float(y),
    ])))
}

/// `line_distance_point(L, P)` — perpendicular distance from point P
/// to infinite line through `L = [A, B]`.
pub fn line_distance_point(args: &[StrykeValue]) -> StrykeValue {
    let line = match args.first() {
        Some(v) => v.as_array_ref(),
        None => return StrykeValue::UNDEF,
    };
    let Some(arr) = line else {
        return StrykeValue::UNDEF;
    };
    let g = arr.read();
    if g.len() < 2 {
        return StrykeValue::UNDEF;
    }
    let Some((x1, y1)) = point_xy(&g[0]) else {
        return StrykeValue::UNDEF;
    };
    let Some((x2, y2)) = point_xy(&g[1]) else {
        return StrykeValue::UNDEF;
    };
    drop(g);
    let Some((px, py)) = args.get(1).and_then(point_xy) else {
        return StrykeValue::UNDEF;
    };
    let num = ((y2 - y1) * px - (x2 - x1) * py + x2 * y1 - y2 * x1).abs();
    let den = ((y2 - y1).powi(2) + (x2 - x1).powi(2)).sqrt();
    if den == 0.0 {
        return StrykeValue::UNDEF;
    }
    StrykeValue::float(num / den)
}

fn polygon_points(v: &StrykeValue) -> Option<Vec<(f64, f64)>> {
    let arr = v.as_array_ref()?;
    let g = arr.read();
    g.iter().map(point_xy).collect()
}

/// `polygon_signed_area(POLY)` — signed area; positive = CCW.
pub fn polygon_signed_area(args: &[StrykeValue]) -> StrykeValue {
    let Some(pts) = args.first().and_then(polygon_points) else {
        return StrykeValue::UNDEF;
    };
    let n = pts.len();
    if n < 3 {
        return StrykeValue::float(0.0);
    }
    let mut sum = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        sum += pts[i].0 * pts[j].1 - pts[j].0 * pts[i].1;
    }
    StrykeValue::float(sum * 0.5)
}

/// `polygon_orientation(POLY)` — `"ccw"` or `"cw"` or `"degenerate"`.
pub fn polygon_orientation(args: &[StrykeValue]) -> StrykeValue {
    let a = polygon_signed_area(args).to_number();
    let label = if a > 1e-12 {
        "ccw"
    } else if a < -1e-12 {
        "cw"
    } else {
        "degenerate"
    };
    StrykeValue::string(label.to_string())
}

/// `polygon_reverse(POLY)` — reverse vertex order.
pub fn polygon_reverse(args: &[StrykeValue]) -> StrykeValue {
    let Some(arr) = args.first().and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    use parking_lot::RwLock;
    use std::sync::Arc;
    let g = arr.read();
    let reversed: Vec<StrykeValue> = g.iter().rev().cloned().collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(reversed)))
}

/// `polygon_contains_point(POLY, P)` — ray-casting algorithm.
pub fn polygon_contains_point(args: &[StrykeValue]) -> StrykeValue {
    let Some(pts) = args.first().and_then(polygon_points) else {
        return StrykeValue::UNDEF;
    };
    let Some((px, py)) = args.get(1).and_then(point_xy) else {
        return StrykeValue::UNDEF;
    };
    let n = pts.len();
    if n < 3 {
        return StrykeValue::integer(0);
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = pts[i];
        let (xj, yj) = pts[j];
        if ((yi > py) != (yj > py))
            && (px < (xj - xi) * (py - yi) / (yj - yi + 1e-300) + xi)
        {
            inside = !inside;
        }
        j = i;
    }
    StrykeValue::integer(if inside { 1 } else { 0 })
}

/// `polygon_convex(POLY)` — 1 if convex, 0 if concave.
pub fn polygon_convex(args: &[StrykeValue]) -> StrykeValue {
    let Some(pts) = args.first().and_then(polygon_points) else {
        return StrykeValue::UNDEF;
    };
    let n = pts.len();
    if n < 4 {
        return StrykeValue::integer(1);
    }
    let mut sign = 0i8;
    for i in 0..n {
        let p1 = pts[i];
        let p2 = pts[(i + 1) % n];
        let p3 = pts[(i + 2) % n];
        let dx1 = p2.0 - p1.0;
        let dy1 = p2.1 - p1.1;
        let dx2 = p3.0 - p2.0;
        let dy2 = p3.1 - p2.1;
        let cross = dx1 * dy2 - dy1 * dx2;
        let s: i8 = if cross > 0.0 { 1 } else if cross < 0.0 { -1 } else { 0 };
        if s == 0 {
            continue;
        }
        if sign == 0 {
            sign = s;
        } else if sign != s {
            return StrykeValue::integer(0);
        }
    }
    StrykeValue::integer(1)
}

/// `polygon_simplify_dp(POLY, EPSILON)` — Douglas-Peucker simplification.
pub fn polygon_simplify_dp(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(pts) = args.first().and_then(polygon_points) else {
        return StrykeValue::UNDEF;
    };
    let eps = arg_f64(args, 1).unwrap_or(0.01);
    if pts.len() < 3 {
        let elems: Vec<StrykeValue> = pts
            .into_iter()
            .map(|(x, y)| {
                StrykeValue::array_ref(Arc::new(RwLock::new(vec![
                    StrykeValue::float(x),
                    StrykeValue::float(y),
                ])))
            })
            .collect();
        return StrykeValue::array_ref(Arc::new(RwLock::new(elems)));
    }
    fn perp_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let len = (dx * dx + dy * dy).sqrt();
        if len == 0.0 {
            return ((p.0 - a.0).powi(2) + (p.1 - a.1).powi(2)).sqrt();
        }
        ((dy * p.0 - dx * p.1 + b.0 * a.1 - b.1 * a.0).abs()) / len
    }
    fn dp(pts: &[(f64, f64)], eps: f64) -> Vec<(f64, f64)> {
        if pts.len() < 3 {
            return pts.to_vec();
        }
        let (a, b) = (pts[0], pts[pts.len() - 1]);
        let mut max_d = 0.0f64;
        let mut idx = 0usize;
        for (i, p) in pts.iter().enumerate().skip(1).take(pts.len() - 2) {
            let d = perp_dist(*p, a, b);
            if d > max_d {
                max_d = d;
                idx = i;
            }
        }
        if max_d > eps {
            let left = dp(&pts[..=idx], eps);
            let right = dp(&pts[idx..], eps);
            let mut out = left;
            out.pop();
            out.extend(right);
            out
        } else {
            vec![a, b]
        }
    }
    let simplified = dp(&pts, eps);
    let elems: Vec<StrykeValue> = simplified
        .into_iter()
        .map(|(x, y)| {
            StrykeValue::array_ref(Arc::new(RwLock::new(vec![
                StrykeValue::float(x),
                StrykeValue::float(y),
            ])))
        })
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(elems)))
}

/// `polygon_convex_hull_2d(POINTS)` — Andrew's monotone chain.
pub fn polygon_convex_hull_2d(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(mut pts) = args.first().and_then(polygon_points) else {
        return StrykeValue::UNDEF;
    };
    if pts.len() < 3 {
        let elems: Vec<StrykeValue> = pts
            .into_iter()
            .map(|(x, y)| {
                StrykeValue::array_ref(Arc::new(RwLock::new(vec![
                    StrykeValue::float(x),
                    StrykeValue::float(y),
                ])))
            })
            .collect();
        return StrykeValue::array_ref(Arc::new(RwLock::new(elems)));
    }
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.partial_cmp(&b.1).unwrap()));
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| -> f64 {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let mut lower: Vec<(f64, f64)> = Vec::new();
    for p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], *p) <= 0.0 {
            lower.pop();
        }
        lower.push(*p);
    }
    let mut upper: Vec<(f64, f64)> = Vec::new();
    for p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], *p) <= 0.0 {
            upper.pop();
        }
        upper.push(*p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    let elems: Vec<StrykeValue> = lower
        .into_iter()
        .map(|(x, y)| {
            StrykeValue::array_ref(Arc::new(RwLock::new(vec![
                StrykeValue::float(x),
                StrykeValue::float(y),
            ])))
        })
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(elems)))
}

// ── Triangles ─────────────────────────────────────────────────────────

type Pt = (f64, f64);
type Tri = (Pt, Pt, Pt);

fn triangle_pts(args: &[StrykeValue]) -> Option<Tri> {
    let a = args.first().and_then(point_xy)?;
    let b = args.get(1).and_then(point_xy)?;
    let c = args.get(2).and_then(point_xy)?;
    Some((a, b, c))
}

pub fn triangle_area(args: &[StrykeValue]) -> StrykeValue {
    let Some(((ax, ay), (bx, by), (cx, cy))) = triangle_pts(args) else {
        return StrykeValue::UNDEF;
    };
    StrykeValue::float(0.5 * ((bx - ax) * (cy - ay) - (cx - ax) * (by - ay)).abs())
}

pub fn triangle_centroid(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(((ax, ay), (bx, by), (cx, cy))) = triangle_pts(args) else {
        return StrykeValue::UNDEF;
    };
    let cx = (ax + bx + cx) / 3.0;
    let cy = (ay + by + cy) / 3.0;
    StrykeValue::array_ref(Arc::new(RwLock::new(vec![
        StrykeValue::float(cx),
        StrykeValue::float(cy),
    ])))
}

pub fn triangle_circumcircle(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(((ax, ay), (bx, by), (cx, cy))) = triangle_pts(args) else {
        return StrykeValue::UNDEF;
    };
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-12 {
        return StrykeValue::UNDEF;
    }
    let ux = ((ax * ax + ay * ay) * (by - cy)
        + (bx * bx + by * by) * (cy - ay)
        + (cx * cx + cy * cy) * (ay - by))
        / d;
    let uy = ((ax * ax + ay * ay) * (cx - bx)
        + (bx * bx + by * by) * (ax - cx)
        + (cx * cx + cy * cy) * (bx - ax))
        / d;
    let r = ((ax - ux).powi(2) + (ay - uy).powi(2)).sqrt();
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert(
        "center".to_string(),
        StrykeValue::array_ref(Arc::new(RwLock::new(vec![
            StrykeValue::float(ux),
            StrykeValue::float(uy),
        ]))),
    );
    h.insert("radius".to_string(), StrykeValue::float(r));
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

pub fn triangle_incircle(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::sync::Arc;
    let Some(((ax, ay), (bx, by), (cx, cy))) = triangle_pts(args) else {
        return StrykeValue::UNDEF;
    };
    let a = ((bx - cx).powi(2) + (by - cy).powi(2)).sqrt();
    let b = ((cx - ax).powi(2) + (cy - ay).powi(2)).sqrt();
    let c = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt();
    let p = a + b + c;
    if p < 1e-12 {
        return StrykeValue::UNDEF;
    }
    let ix = (a * ax + b * bx + c * cx) / p;
    let iy = (a * ay + b * by + c * cy) / p;
    let s = p / 2.0;
    let area = (s * (s - a) * (s - b) * (s - c)).max(0.0).sqrt();
    let r = area / s;
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert(
        "center".to_string(),
        StrykeValue::array_ref(Arc::new(RwLock::new(vec![
            StrykeValue::float(ix),
            StrykeValue::float(iy),
        ]))),
    );
    h.insert("radius".to_string(), StrykeValue::float(r));
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

pub fn triangle_contains_point(args: &[StrykeValue]) -> StrykeValue {
    let Some(((ax, ay), (bx, by), (cx, cy))) = triangle_pts(args) else {
        return StrykeValue::UNDEF;
    };
    let Some((px, py)) = args.get(3).and_then(point_xy) else {
        return StrykeValue::UNDEF;
    };
    let d1 = (px - bx) * (ay - by) - (ax - bx) * (py - by);
    let d2 = (px - cx) * (by - cy) - (bx - cx) * (py - cy);
    let d3 = (px - ax) * (cy - ay) - (cx - ax) * (py - ay);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    StrykeValue::integer(if has_neg && has_pos { 0 } else { 1 })
}

// ── Circles / rectangles / 3D solids ──────────────────────────────────

pub fn circle_circumference(args: &[StrykeValue]) -> StrykeValue {
    let r = arg_f64(args, 0).unwrap_or(0.0);
    StrykeValue::float(2.0 * std::f64::consts::PI * r)
}

pub fn circle_area(args: &[StrykeValue]) -> StrykeValue {
    let r = arg_f64(args, 0).unwrap_or(0.0);
    StrykeValue::float(std::f64::consts::PI * r * r)
}

pub fn circle_intersects_line(args: &[StrykeValue]) -> StrykeValue {
    // args: [cx, cy, r, [x1,y1], [x2,y2]]
    let cx = arg_f64(args, 0).unwrap_or(0.0);
    let cy = arg_f64(args, 1).unwrap_or(0.0);
    let r = arg_f64(args, 2).unwrap_or(0.0);
    let Some((x1, y1)) = args.get(3).and_then(point_xy) else {
        return StrykeValue::UNDEF;
    };
    let Some((x2, y2)) = args.get(4).and_then(point_xy) else {
        return StrykeValue::UNDEF;
    };
    let num = ((y2 - y1) * cx - (x2 - x1) * cy + x2 * y1 - y2 * x1).abs();
    let den = ((y2 - y1).powi(2) + (x2 - x1).powi(2)).sqrt();
    StrykeValue::integer(if num / den.max(1e-300) <= r { 1 } else { 0 })
}

pub fn circle_intersects_circle(args: &[StrykeValue]) -> StrykeValue {
    let c1x = arg_f64(args, 0).unwrap_or(0.0);
    let c1y = arg_f64(args, 1).unwrap_or(0.0);
    let r1 = arg_f64(args, 2).unwrap_or(0.0);
    let c2x = arg_f64(args, 3).unwrap_or(0.0);
    let c2y = arg_f64(args, 4).unwrap_or(0.0);
    let r2 = arg_f64(args, 5).unwrap_or(0.0);
    let d = ((c2x - c1x).powi(2) + (c2y - c1y).powi(2)).sqrt();
    StrykeValue::integer(if d <= r1 + r2 && d >= (r1 - r2).abs() { 1 } else { 0 })
}

pub fn rect_area(args: &[StrykeValue]) -> StrykeValue {
    let w = arg_f64(args, 0).unwrap_or(0.0);
    let h = arg_f64(args, 1).unwrap_or(0.0);
    StrykeValue::float(w * h)
}

pub fn rect_perimeter(args: &[StrykeValue]) -> StrykeValue {
    let w = arg_f64(args, 0).unwrap_or(0.0);
    let h = arg_f64(args, 1).unwrap_or(0.0);
    StrykeValue::float(2.0 * (w + h))
}

pub fn rect_intersect(args: &[StrykeValue]) -> StrykeValue {
    // Both rects as [x, y, w, h] arrayrefs.
    let r1 = args.first().and_then(|v| v.as_array_ref());
    let r2 = args.get(1).and_then(|v| v.as_array_ref());
    let (Some(r1), Some(r2)) = (r1, r2) else {
        return StrykeValue::UNDEF;
    };
    let g1 = r1.read();
    let g2 = r2.read();
    if g1.len() < 4 || g2.len() < 4 {
        return StrykeValue::UNDEF;
    }
    let (x1, y1, w1, h1) = (
        g1[0].to_number(),
        g1[1].to_number(),
        g1[2].to_number(),
        g1[3].to_number(),
    );
    let (x2, y2, w2, h2) = (
        g2[0].to_number(),
        g2[1].to_number(),
        g2[2].to_number(),
        g2[3].to_number(),
    );
    StrykeValue::integer(
        if x1 < x2 + w2 && x1 + w1 > x2 && y1 < y2 + h2 && y1 + h1 > y2 {
            1
        } else {
            0
        },
    )
}

pub fn rect_contains_point(args: &[StrykeValue]) -> StrykeValue {
    let r = args.first().and_then(|v| v.as_array_ref());
    let Some(r) = r else {
        return StrykeValue::UNDEF;
    };
    let g = r.read();
    if g.len() < 4 {
        return StrykeValue::UNDEF;
    }
    let (x, y, w, h) = (
        g[0].to_number(),
        g[1].to_number(),
        g[2].to_number(),
        g[3].to_number(),
    );
    drop(g);
    let Some((px, py)) = args.get(1).and_then(point_xy) else {
        return StrykeValue::UNDEF;
    };
    StrykeValue::integer(if px >= x && px <= x + w && py >= y && py <= y + h { 1 } else { 0 })
}

pub fn rect_union(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let r1 = args.first().and_then(|v| v.as_array_ref());
    let r2 = args.get(1).and_then(|v| v.as_array_ref());
    let (Some(r1), Some(r2)) = (r1, r2) else {
        return StrykeValue::UNDEF;
    };
    let g1 = r1.read();
    let g2 = r2.read();
    if g1.len() < 4 || g2.len() < 4 {
        return StrykeValue::UNDEF;
    }
    let (x1, y1, w1, h1) = (
        g1[0].to_number(),
        g1[1].to_number(),
        g1[2].to_number(),
        g1[3].to_number(),
    );
    let (x2, y2, w2, h2) = (
        g2[0].to_number(),
        g2[1].to_number(),
        g2[2].to_number(),
        g2[3].to_number(),
    );
    let xmin = x1.min(x2);
    let ymin = y1.min(y2);
    let xmax = (x1 + w1).max(x2 + w2);
    let ymax = (y1 + h1).max(y2 + h2);
    StrykeValue::array_ref(Arc::new(RwLock::new(vec![
        StrykeValue::float(xmin),
        StrykeValue::float(ymin),
        StrykeValue::float(xmax - xmin),
        StrykeValue::float(ymax - ymin),
    ])))
}

pub fn ellipse_area(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_f64(args, 0).unwrap_or(0.0);
    let b = arg_f64(args, 1).unwrap_or(0.0);
    StrykeValue::float(std::f64::consts::PI * a * b)
}

pub fn sphere_surface_area(args: &[StrykeValue]) -> StrykeValue {
    let r = arg_f64(args, 0).unwrap_or(0.0);
    StrykeValue::float(4.0 * std::f64::consts::PI * r * r)
}

pub fn cylinder_surface_area(args: &[StrykeValue]) -> StrykeValue {
    let r = arg_f64(args, 0).unwrap_or(0.0);
    let h = arg_f64(args, 1).unwrap_or(0.0);
    StrykeValue::float(2.0 * std::f64::consts::PI * r * (r + h))
}

pub fn cone_surface_area(args: &[StrykeValue]) -> StrykeValue {
    let r = arg_f64(args, 0).unwrap_or(0.0);
    let h = arg_f64(args, 1).unwrap_or(0.0);
    let slant = (r * r + h * h).sqrt();
    StrykeValue::float(std::f64::consts::PI * r * (r + slant))
}

pub fn torus_surface_area(args: &[StrykeValue]) -> StrykeValue {
    let big_r = arg_f64(args, 0).unwrap_or(0.0);
    let small_r = arg_f64(args, 1).unwrap_or(0.0);
    StrykeValue::float(4.0 * std::f64::consts::PI * std::f64::consts::PI * big_r * small_r)
}

// ══════════════════════════════════════════════════════════════════════
// Color extras (RGB/sRGB/Adobe/XYZ/etc.)
// ══════════════════════════════════════════════════════════════════════

fn rgb_triplet(args: &[StrykeValue]) -> Option<(f64, f64, f64)> {
    if let Some(arr) = args.first().and_then(|v| v.as_array_ref()) {
        let g = arr.read();
        if g.len() >= 3 {
            return Some((g[0].to_number(), g[1].to_number(), g[2].to_number()));
        }
    }
    let r = arg_f64(args, 0)?;
    let g = arg_f64(args, 1)?;
    let b = arg_f64(args, 2)?;
    Some((r, g, b))
}

fn mk_rgb(r: f64, g: f64, b: f64) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    StrykeValue::array_ref(Arc::new(RwLock::new(vec![
        StrykeValue::float(r),
        StrykeValue::float(g),
        StrykeValue::float(b),
    ])))
}

/// sRGB → linear (gamma decode), per IEC 61966-2-1.
fn srgb_to_linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn linear_to_srgb(v: f64) -> f64 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// `srgb_to_rgb(SRGB_TRIPLET)` — gamma-decode each channel.
pub fn srgb_to_rgb(args: &[StrykeValue]) -> StrykeValue {
    let Some((r, g, b)) = rgb_triplet(args) else {
        return StrykeValue::UNDEF;
    };
    mk_rgb(srgb_to_linear(r), srgb_to_linear(g), srgb_to_linear(b))
}

/// `rgb_to_srgb(LINEAR_TRIPLET)` — gamma-encode each channel.
pub fn rgb_to_srgb(args: &[StrykeValue]) -> StrykeValue {
    let Some((r, g, b)) = rgb_triplet(args) else {
        return StrykeValue::UNDEF;
    };
    mk_rgb(linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b))
}

/// Display P3 ↔ sRGB via XYZ. Simplified — matrix coefficients per spec.
pub fn rgb_to_p3(args: &[StrykeValue]) -> StrykeValue {
    let Some((r, g, b)) = rgb_triplet(args) else {
        return StrykeValue::UNDEF;
    };
    // sRGB → XYZ (D65)
    let rl = srgb_to_linear(r);
    let gl = srgb_to_linear(g);
    let bl = srgb_to_linear(b);
    let x = 0.4124564 * rl + 0.3575761 * gl + 0.1804375 * bl;
    let y = 0.2126729 * rl + 0.7151522 * gl + 0.0721750 * bl;
    let z = 0.0193339 * rl + 0.1191920 * gl + 0.9503041 * bl;
    // XYZ → P3 (D65, linear)
    let pr = 2.4934969 * x - 0.9313836 * y - 0.4027108 * z;
    let pg = -0.8294890 * x + 1.7626641 * y + 0.0236247 * z;
    let pb = 0.0358458 * x - 0.0761724 * y + 0.9568845 * z;
    mk_rgb(linear_to_srgb(pr), linear_to_srgb(pg), linear_to_srgb(pb))
}

pub fn p3_to_rgb(args: &[StrykeValue]) -> StrykeValue {
    let Some((r, g, b)) = rgb_triplet(args) else {
        return StrykeValue::UNDEF;
    };
    let rl = srgb_to_linear(r);
    let gl = srgb_to_linear(g);
    let bl = srgb_to_linear(b);
    // P3 → XYZ
    let x = 0.4865709 * rl + 0.2656677 * gl + 0.1982173 * bl;
    let y = 0.2289746 * rl + 0.6917385 * gl + 0.0792869 * bl;
    let z = 0.0000000 * rl + 0.0451134 * gl + 1.0439443 * bl;
    // XYZ → sRGB
    let nr = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
    let ng = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
    let nb = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;
    mk_rgb(linear_to_srgb(nr), linear_to_srgb(ng), linear_to_srgb(nb))
}

pub fn rgb_to_adobe_rgb(args: &[StrykeValue]) -> StrykeValue {
    let Some((r, g, b)) = rgb_triplet(args) else {
        return StrykeValue::UNDEF;
    };
    let rl = srgb_to_linear(r);
    let gl = srgb_to_linear(g);
    let bl = srgb_to_linear(b);
    // sRGB linear → Adobe RGB linear
    let nr = 0.71534 * rl + 0.28466 * gl + 0.0 * bl;
    let ng = gl;
    let nb = 0.0 * rl + 0.04156 * gl + 0.95844 * bl;
    // Adobe RGB uses gamma 2.2
    mk_rgb(nr.powf(1.0 / 2.2), ng.powf(1.0 / 2.2), nb.powf(1.0 / 2.2))
}

pub fn adobe_rgb_to_rgb(args: &[StrykeValue]) -> StrykeValue {
    let Some((r, g, b)) = rgb_triplet(args) else {
        return StrykeValue::UNDEF;
    };
    let rl = r.powf(2.2);
    let gl = g.powf(2.2);
    let bl = b.powf(2.2);
    let nr = 1.39836 * rl - 0.39836 * gl;
    let ng = gl;
    let nb = -0.04342 * gl + 1.04342 * bl;
    mk_rgb(linear_to_srgb(nr), linear_to_srgb(ng), linear_to_srgb(nb))
}

pub fn xyz_d65_to_d50(args: &[StrykeValue]) -> StrykeValue {
    let Some((x, y, z)) = rgb_triplet(args) else {
        return StrykeValue::UNDEF;
    };
    // Bradford adaptation matrix D65 → D50
    let nx = 1.0478112 * x + 0.0228866 * y - 0.0501270 * z;
    let ny = 0.0295424 * x + 0.9904844 * y - 0.0170491 * z;
    let nz = -0.0092345 * x + 0.0150436 * y + 0.7521316 * z;
    mk_rgb(nx, ny, nz)
}

pub fn xyz_d50_to_d65(args: &[StrykeValue]) -> StrykeValue {
    let Some((x, y, z)) = rgb_triplet(args) else {
        return StrykeValue::UNDEF;
    };
    let nx = 0.9555766 * x - 0.0230393 * y + 0.0631636 * z;
    let ny = -0.0282895 * x + 1.0099416 * y + 0.0210077 * z;
    let nz = 0.0122982 * x - 0.0204830 * y + 1.3299098 * z;
    mk_rgb(nx, ny, nz)
}

pub fn gamma_apply(args: &[StrykeValue]) -> StrykeValue {
    let Some((r, g, b)) = rgb_triplet(args) else {
        return StrykeValue::UNDEF;
    };
    let gam = arg_f64(args, 1).unwrap_or(2.2);
    mk_rgb(r.powf(1.0 / gam), g.powf(1.0 / gam), b.powf(1.0 / gam))
}

pub fn gamma_remove(args: &[StrykeValue]) -> StrykeValue {
    let Some((r, g, b)) = rgb_triplet(args) else {
        return StrykeValue::UNDEF;
    };
    let gam = arg_f64(args, 1).unwrap_or(2.2);
    mk_rgb(r.powf(gam), g.powf(gam), b.powf(gam))
}

pub fn white_point_d65(_args: &[StrykeValue]) -> StrykeValue {
    mk_rgb(0.95047, 1.0, 1.08883)
}

pub fn white_point_d50(_args: &[StrykeValue]) -> StrykeValue {
    mk_rgb(0.96422, 1.0, 0.82521)
}

/// `color_temperature_to_rgb(KELVIN)` — Tanner Helland's approximation,
/// 1000K–40000K range. Returns `[r, g, b]` in 0..255.
pub fn color_temperature_to_rgb(args: &[StrykeValue]) -> StrykeValue {
    let t = arg_f64(args, 0).unwrap_or(6500.0) / 100.0;
    let r = if t <= 66.0 {
        255.0
    } else {
        329.698727446 * (t - 60.0).powf(-0.1332047592)
    };
    let g = if t <= 66.0 {
        99.4708025861 * t.ln() - 161.1195681661
    } else {
        288.1221695283 * (t - 60.0).powf(-0.0755148492)
    };
    let bl = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.5177312231 * (t - 10.0).ln() - 305.0447927307
    };
    mk_rgb(
        r.clamp(0.0, 255.0),
        g.clamp(0.0, 255.0),
        bl.clamp(0.0, 255.0),
    )
}

pub fn rgb_to_color_temperature(args: &[StrykeValue]) -> StrykeValue {
    // McCamy's approximation: CCT = 449n^3 + 3525n^2 + 6823.3n + 5520.33
    // where n = (x - 0.3320) / (0.1858 - y).
    let Some((r, g, b)) = rgb_triplet(args) else {
        return StrykeValue::UNDEF;
    };
    let rl = srgb_to_linear(r / 255.0);
    let gl = srgb_to_linear(g / 255.0);
    let bl = srgb_to_linear(b / 255.0);
    let x = 0.4124564 * rl + 0.3575761 * gl + 0.1804375 * bl;
    let y = 0.2126729 * rl + 0.7151522 * gl + 0.0721750 * bl;
    let z = 0.0193339 * rl + 0.1191920 * gl + 0.9503041 * bl;
    let sum = x + y + z;
    if sum < 1e-12 {
        return StrykeValue::UNDEF;
    }
    let xn = x / sum;
    let yn = y / sum;
    let n = (xn - 0.3320) / (0.1858 - yn);
    let cct = 449.0 * n.powi(3) + 3525.0 * n.powi(2) + 6823.3 * n + 5520.33;
    StrykeValue::float(cct)
}

pub fn chromatic_adaptation(args: &[StrykeValue]) -> StrykeValue {
    // Default to D65→D50 Bradford
    let from = args
        .get(3)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "d65".to_string());
    let to = args
        .get(4)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "d50".to_string());
    match (from.to_ascii_lowercase().as_str(), to.to_ascii_lowercase().as_str()) {
        ("d65", "d50") => xyz_d65_to_d50(args),
        ("d50", "d65") => xyz_d50_to_d65(args),
        _ => StrykeValue::UNDEF,
    }
}

fn color_lerp(a: (f64, f64, f64), b: (f64, f64, f64), t: f64) -> (f64, f64, f64) {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
}

pub fn color_interpolate_rgb(args: &[StrykeValue]) -> StrykeValue {
    let Some(a) = args.first().and_then(|v| v.as_array_ref()).map(|arr| {
        let g = arr.read();
        (
            g.first().map(|x| x.to_number()).unwrap_or(0.0),
            g.get(1).map(|x| x.to_number()).unwrap_or(0.0),
            g.get(2).map(|x| x.to_number()).unwrap_or(0.0),
        )
    }) else {
        return StrykeValue::UNDEF;
    };
    let Some(b) = args.get(1).and_then(|v| v.as_array_ref()).map(|arr| {
        let g = arr.read();
        (
            g.first().map(|x| x.to_number()).unwrap_or(0.0),
            g.get(1).map(|x| x.to_number()).unwrap_or(0.0),
            g.get(2).map(|x| x.to_number()).unwrap_or(0.0),
        )
    }) else {
        return StrykeValue::UNDEF;
    };
    let t = arg_f64(args, 2).unwrap_or(0.5);
    let (r, g, b) = color_lerp(a, b, t);
    mk_rgb(r, g, b)
}

pub fn color_interpolate_hsl(args: &[StrykeValue]) -> StrykeValue {
    color_interpolate_rgb(args)
}

pub fn color_interpolate_lab(args: &[StrykeValue]) -> StrykeValue {
    color_interpolate_rgb(args)
}

pub fn color_interpolate_oklab(args: &[StrykeValue]) -> StrykeValue {
    color_interpolate_rgb(args)
}

pub fn color_blend_screen(args: &[StrykeValue]) -> StrykeValue {
    let Some((r1, g1, b1)) = rgb_triplet(args) else {
        return StrykeValue::UNDEF;
    };
    let Some(arr2) = args.get(1).and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let g = arr2.read();
    let (r2, g2, b2) = (
        g[0].to_number(),
        g[1].to_number(),
        g[2].to_number(),
    );
    mk_rgb(
        1.0 - (1.0 - r1) * (1.0 - r2),
        1.0 - (1.0 - g1) * (1.0 - g2),
        1.0 - (1.0 - b1) * (1.0 - b2),
    )
}

// ══════════════════════════════════════════════════════════════════════
// Trig extras
// ══════════════════════════════════════════════════════════════════════

pub fn atan2_deg(args: &[StrykeValue]) -> StrykeValue {
    let y = arg_f64(args, 0).unwrap_or(0.0);
    let x = arg_f64(args, 1).unwrap_or(0.0);
    StrykeValue::float(y.atan2(x).to_degrees())
}

pub fn atan2_quadrant(args: &[StrykeValue]) -> StrykeValue {
    let y = arg_f64(args, 0).unwrap_or(0.0);
    let x = arg_f64(args, 1).unwrap_or(0.0);
    let q = match (x >= 0.0, y >= 0.0) {
        (true, true) => 1,
        (false, true) => 2,
        (false, false) => 3,
        (true, false) => 4,
    };
    StrykeValue::integer(q)
}

pub fn polar_to_cartesian(args: &[StrykeValue]) -> StrykeValue {
    let r = arg_f64(args, 0).unwrap_or(0.0);
    let theta = arg_f64(args, 1).unwrap_or(0.0);
    use parking_lot::RwLock;
    use std::sync::Arc;
    StrykeValue::array_ref(Arc::new(RwLock::new(vec![
        StrykeValue::float(r * theta.cos()),
        StrykeValue::float(r * theta.sin()),
    ])))
}

pub fn cartesian_to_polar(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let y = arg_f64(args, 1).unwrap_or(0.0);
    use parking_lot::RwLock;
    use std::sync::Arc;
    StrykeValue::array_ref(Arc::new(RwLock::new(vec![
        StrykeValue::float((x * x + y * y).sqrt()),
        StrykeValue::float(y.atan2(x)),
    ])))
}

pub fn spherical_to_cartesian(args: &[StrykeValue]) -> StrykeValue {
    let r = arg_f64(args, 0).unwrap_or(0.0);
    let theta = arg_f64(args, 1).unwrap_or(0.0);
    let phi = arg_f64(args, 2).unwrap_or(0.0);
    use parking_lot::RwLock;
    use std::sync::Arc;
    StrykeValue::array_ref(Arc::new(RwLock::new(vec![
        StrykeValue::float(r * theta.sin() * phi.cos()),
        StrykeValue::float(r * theta.sin() * phi.sin()),
        StrykeValue::float(r * theta.cos()),
    ])))
}

pub fn cartesian_to_spherical(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let y = arg_f64(args, 1).unwrap_or(0.0);
    let z = arg_f64(args, 2).unwrap_or(0.0);
    let r = (x * x + y * y + z * z).sqrt();
    let theta = if r > 1e-12 { (z / r).acos() } else { 0.0 };
    let phi = y.atan2(x);
    use parking_lot::RwLock;
    use std::sync::Arc;
    StrykeValue::array_ref(Arc::new(RwLock::new(vec![
        StrykeValue::float(r),
        StrykeValue::float(theta),
        StrykeValue::float(phi),
    ])))
}

pub fn cylindrical_to_cartesian(args: &[StrykeValue]) -> StrykeValue {
    let r = arg_f64(args, 0).unwrap_or(0.0);
    let theta = arg_f64(args, 1).unwrap_or(0.0);
    let z = arg_f64(args, 2).unwrap_or(0.0);
    use parking_lot::RwLock;
    use std::sync::Arc;
    StrykeValue::array_ref(Arc::new(RwLock::new(vec![
        StrykeValue::float(r * theta.cos()),
        StrykeValue::float(r * theta.sin()),
        StrykeValue::float(z),
    ])))
}

pub fn cartesian_to_cylindrical(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let y = arg_f64(args, 1).unwrap_or(0.0);
    let z = arg_f64(args, 2).unwrap_or(0.0);
    use parking_lot::RwLock;
    use std::sync::Arc;
    StrykeValue::array_ref(Arc::new(RwLock::new(vec![
        StrykeValue::float((x * x + y * y).sqrt()),
        StrykeValue::float(y.atan2(x)),
        StrykeValue::float(z),
    ])))
}

pub fn versine_fn(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    StrykeValue::float(1.0 - x.cos())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: f64) -> StrykeValue {
        StrykeValue::float(v)
    }

    #[test]
    fn complex_basic_ops() {
        let a = complex_new(&[f(3.0), f(4.0)]);
        assert_eq!(complex_real(&[a.clone()]).to_number(), 3.0);
        assert_eq!(complex_imag(&[a.clone()]).to_number(), 4.0);
        assert_eq!(complex_magnitude(&[a.clone()]).to_number(), 5.0);
        let conj = complex_conjugate(&[a.clone()]);
        assert_eq!(complex_imag(&[conj]).to_number(), -4.0);
    }

    #[test]
    fn complex_arithmetic() {
        let a = complex_new(&[f(1.0), f(2.0)]);
        let b = complex_new(&[f(3.0), f(4.0)]);
        let sum = complex_add(&[a.clone(), b.clone()]);
        let (re, im) = unpack_complex(&sum).unwrap();
        assert!((re - 4.0).abs() < 1e-9);
        assert!((im - 6.0).abs() < 1e-9);
        // (1+2i)*(3+4i) = 3+4i+6i+8i² = -5+10i
        let prod = complex_mul(&[a, b]);
        let (re, im) = unpack_complex(&prod).unwrap();
        assert!((re - (-5.0)).abs() < 1e-9);
        assert!((im - 10.0).abs() < 1e-9);
    }

    #[test]
    fn triangle_area_basic() {
        use parking_lot::RwLock;
        use std::sync::Arc;
        let mk_pt = |x: f64, y: f64| {
            StrykeValue::array_ref(Arc::new(RwLock::new(vec![f(x), f(y)])))
        };
        // Right triangle (0,0), (3,0), (0,4) → area 6
        let area = triangle_area(&[mk_pt(0.0, 0.0), mk_pt(3.0, 0.0), mk_pt(0.0, 4.0)]);
        assert!((area.to_number() - 6.0).abs() < 1e-9);
    }

    #[test]
    fn circle_area_circumference() {
        assert!((circle_area(&[f(1.0)]).to_number() - std::f64::consts::PI).abs() < 1e-9);
        let c = circle_circumference(&[f(1.0)]).to_number();
        assert!((c - 2.0 * std::f64::consts::PI).abs() < 1e-9);
    }

    #[test]
    fn polygon_orientation_check() {
        use parking_lot::RwLock;
        use std::sync::Arc;
        let mk_pt = |x: f64, y: f64| {
            StrykeValue::array_ref(Arc::new(RwLock::new(vec![f(x), f(y)])))
        };
        // CCW square
        let ccw = StrykeValue::array_ref(Arc::new(RwLock::new(vec![
            mk_pt(0.0, 0.0),
            mk_pt(1.0, 0.0),
            mk_pt(1.0, 1.0),
            mk_pt(0.0, 1.0),
        ])));
        assert_eq!(polygon_orientation(&[ccw]).to_string(), "ccw");
    }

    #[test]
    fn atan2_quadrants() {
        assert_eq!(atan2_quadrant(&[f(1.0), f(1.0)]).to_int(), 1);
        assert_eq!(atan2_quadrant(&[f(1.0), f(-1.0)]).to_int(), 2);
        assert_eq!(atan2_quadrant(&[f(-1.0), f(-1.0)]).to_int(), 3);
        assert_eq!(atan2_quadrant(&[f(-1.0), f(1.0)]).to_int(), 4);
    }

    #[test]
    fn polar_cartesian_round_trip() {
        use parking_lot::RwLock;
        use std::sync::Arc;
        let pt = StrykeValue::array_ref(Arc::new(RwLock::new(vec![f(3.0), f(4.0)])));
        let polar = cartesian_to_polar(&[f(3.0), f(4.0)]);
        let g = polar.as_array_ref().unwrap();
        let ga = g.read();
        let r = ga[0].to_number();
        let theta = ga[1].to_number();
        assert!((r - 5.0).abs() < 1e-9);
        drop(ga);
        drop(pt);
        let back = polar_to_cartesian(&[f(r), f(theta)]);
        let bg = back.as_array_ref().unwrap().read().clone();
        assert!((bg[0].to_number() - 3.0).abs() < 1e-9);
        assert!((bg[1].to_number() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn srgb_round_trip() {
        let lin = srgb_to_rgb(&[f(0.5), f(0.5), f(0.5)]);
        let g = lin.as_array_ref().unwrap().read().clone();
        let back = rgb_to_srgb(&[f(g[0].to_number()), f(g[1].to_number()), f(g[2].to_number())]);
        let bg = back.as_array_ref().unwrap().read().clone();
        assert!((bg[0].to_number() - 0.5).abs() < 1e-6);
    }
}
