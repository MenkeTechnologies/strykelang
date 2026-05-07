// Batch 28 — geometry / topology / mesh / spatial.

// Triangle area (Heron)
fn builtin_triangle_area_heron(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let s = 0.5 * (a + b + c);
    let prod = (s - a) * (s - b) * (s - c) * s;
    if prod < 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float(prod.sqrt()))
}

// Triangle area from 3 points (2D)
fn builtin_triangle_area_pts(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if pts.len() < 3 { return Ok(PerlValue::float(0.0)); }
    let p0 = arg_to_vec(&pts[0]);
    let p1 = arg_to_vec(&pts[1]);
    let p2 = arg_to_vec(&pts[2]);
    let x0 = p0[0].to_number(); let y0 = p0[1].to_number();
    let x1 = p1[0].to_number(); let y1 = p1[1].to_number();
    let x2 = p2[0].to_number(); let y2 = p2[1].to_number();
    Ok(PerlValue::float(((x1 - x0) * (y2 - y0) - (x2 - x0) * (y1 - y0)).abs() / 2.0))
}

// Centroid of polygon (2D)
fn builtin_polygon_centroid_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = pts.len();
    if n == 0 { return Ok(PerlValue::array(vec![])); }
    let mut cx = 0.0; let mut cy = 0.0; let mut a = 0.0;
    for i in 0..n {
        let p = arg_to_vec(&pts[i]);
        let q = arg_to_vec(&pts[(i + 1) % n]);
        let xi = p[0].to_number(); let yi = p[1].to_number();
        let xj = q[0].to_number(); let yj = q[1].to_number();
        let cross = xi * yj - xj * yi;
        a += cross;
        cx += (xi + xj) * cross;
        cy += (yi + yj) * cross;
    }
    a *= 0.5;
    if a == 0.0 { return Ok(PerlValue::array(vec![PerlValue::float(0.0), PerlValue::float(0.0)])); }
    Ok(PerlValue::array(vec![
        PerlValue::float(cx / (6.0 * a)),
        PerlValue::float(cy / (6.0 * a)),
    ]))
}

// Triangle inradius
fn builtin_triangle_inradius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let s = 0.5 * (a + b + c);
    let area_sq = s * (s - a) * (s - b) * (s - c);
    if area_sq <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(area_sq.sqrt() / s))
}
// Triangle circumradius
fn builtin_triangle_circumradius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let s = 0.5 * (a + b + c);
    let area_sq = s * (s - a) * (s - b) * (s - c);
    if area_sq <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(a * b * c / (4.0 * area_sq.sqrt())))
}

// Regular n-gon area
fn builtin_regular_ngon_area(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let side = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let pi = std::f64::consts::PI;
    if n < 3.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(0.25 * n * side * side / (pi / n).tan()))
}
// Regular n-gon inradius
fn builtin_regular_ngon_inradius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let side = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let pi = std::f64::consts::PI;
    if n < 3.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(side / (2.0 * (pi / n).tan())))
}
// Regular n-gon circumradius
fn builtin_regular_ngon_circumradius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let side = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let pi = std::f64::consts::PI;
    if n < 3.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(side / (2.0 * (pi / n).sin())))
}

// Sphere volume
fn builtin_sphere_volume_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    Ok(PerlValue::float(4.0 / 3.0 * std::f64::consts::PI * r * r * r))
}
// Sphere surface area
fn builtin_sphere_surface_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    Ok(PerlValue::float(4.0 * std::f64::consts::PI * r * r))
}
// n-ball volume (general)
fn builtin_n_ball_volume(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args) as usize;
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let pi = std::f64::consts::PI;
    fn gamma_int_half(k: usize) -> f64 {
        let pi_local = std::f64::consts::PI;
        if k == 0 { 1.0 }
        else if k % 2 == 0 { (1..k/2).map(|i| i as f64).product::<f64>() }
        else {
            let m = (k - 1) / 2;
            (1..=m).map(|i| (2.0 * i as f64 - 1.0) / 2.0).product::<f64>() * pi_local.sqrt()
        }
    }
    Ok(PerlValue::float(pi.powf(n as f64 / 2.0) / gamma_int_half(n + 2) * r.powi(n as i32)))
}
// Cylinder volume
fn builtin_cylinder_volume_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(std::f64::consts::PI * r * r * h))
}
// Cylinder surface
fn builtin_cylinder_surface(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(2.0 * std::f64::consts::PI * r * (r + h)))
}
// Cone volume
fn builtin_cone_volume_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(std::f64::consts::PI * r * r * h / 3.0))
}
// Cone surface
fn builtin_cone_surface(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let l = (r * r + h * h).sqrt();
    Ok(PerlValue::float(std::f64::consts::PI * r * (r + l)))
}
// Torus volume
fn builtin_torus_volume_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r_major = f1(args);
    let r_minor = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let pi = std::f64::consts::PI;
    Ok(PerlValue::float(2.0 * pi * pi * r_major * r_minor * r_minor))
}
// Torus surface
fn builtin_torus_surface_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r_major = f1(args);
    let r_minor = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let pi = std::f64::consts::PI;
    Ok(PerlValue::float(4.0 * pi * pi * r_major * r_minor))
}
// Ellipsoid volume
fn builtin_ellipsoid_volume(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(4.0 / 3.0 * std::f64::consts::PI * a * b * c))
}
// Ellipsoid surface (Knud Thomsen approximation, p=1.6075)
fn builtin_ellipsoid_surface_approx(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let p = 1.6075;
    let inner = ((a * b).powf(p) + (a * c).powf(p) + (b * c).powf(p)) / 3.0;
    Ok(PerlValue::float(4.0 * std::f64::consts::PI * inner.powf(1.0 / p)))
}

// Tetrahedron volume from 4 points
fn builtin_tetrahedron_volume_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if pts.len() < 4 { return Ok(PerlValue::float(0.0)); }
    let p0 = arg_to_vec(&pts[0]);
    let p1 = arg_to_vec(&pts[1]);
    let p2 = arg_to_vec(&pts[2]);
    let p3 = arg_to_vec(&pts[3]);
    let v1 = [p1[0].to_number() - p0[0].to_number(), p1[1].to_number() - p0[1].to_number(), p1[2].to_number() - p0[2].to_number()];
    let v2 = [p2[0].to_number() - p0[0].to_number(), p2[1].to_number() - p0[1].to_number(), p2[2].to_number() - p0[2].to_number()];
    let v3 = [p3[0].to_number() - p0[0].to_number(), p3[1].to_number() - p0[1].to_number(), p3[2].to_number() - p0[2].to_number()];
    let cross = [
        v2[1] * v3[2] - v2[2] * v3[1],
        v2[2] * v3[0] - v2[0] * v3[2],
        v2[0] * v3[1] - v2[1] * v3[0],
    ];
    let det = v1[0] * cross[0] + v1[1] * cross[1] + v1[2] * cross[2];
    Ok(PerlValue::float(det.abs() / 6.0))
}

// Distance point to line (2D)
fn builtin_dist_point_line_2d(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let px = f1(args);
    let py = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = (a * a + b * b).sqrt();
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((a * px + b * py + c).abs() / denom))
}

// Distance point to plane (3D)
fn builtin_dist_point_plane_3d(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let px = f1(args);
    let py = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let pz = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = (a * a + b * b + c * c).sqrt();
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((a * px + b * py + c * pz + d).abs() / denom))
}

// Closest point on segment (2D) — returns [x, y]
fn builtin_closest_pt_segment_2d(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let px = f1(args);
    let py = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let ax = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let ay = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let bx = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let by = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let dx = bx - ax; let dy = by - ay;
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 { return Ok(PerlValue::array(vec![PerlValue::float(ax), PerlValue::float(ay)])); }
    let t = (((px - ax) * dx + (py - ay) * dy) / l2).clamp(0.0, 1.0);
    Ok(PerlValue::array(vec![
        PerlValue::float(ax + t * dx),
        PerlValue::float(ay + t * dy),
    ]))
}

// Bounding box from points
fn builtin_bbox_from_points(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if pts.is_empty() { return Ok(PerlValue::array(vec![])); }
    let mut mnx = f64::INFINITY; let mut mny = f64::INFINITY;
    let mut mxx = f64::NEG_INFINITY; let mut mxy = f64::NEG_INFINITY;
    for p in &pts {
        let v = arg_to_vec(p);
        let x = v[0].to_number();
        let y = v.get(1).map(|q| q.to_number()).unwrap_or(0.0);
        if x < mnx { mnx = x; }
        if y < mny { mny = y; }
        if x > mxx { mxx = x; }
        if y > mxy { mxy = y; }
    }
    Ok(PerlValue::array(vec![
        PerlValue::float(mnx), PerlValue::float(mny),
        PerlValue::float(mxx), PerlValue::float(mxy),
    ]))
}

// Manhattan distance
fn builtin_manhattan_distance_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let q = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let mut sum = 0.0;
    for i in 0..p.len().min(q.len()) {
        sum += (p[i].to_number() - q[i].to_number()).abs();
    }
    Ok(PerlValue::float(sum))
}

// Euclidean distance N-dim
fn builtin_euclidean_distance_nd(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let q = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let mut sum = 0.0;
    for i in 0..p.len().min(q.len()) {
        let d = p[i].to_number() - q[i].to_number();
        sum += d * d;
    }
    Ok(PerlValue::float(sum.sqrt()))
}

// Chebyshev distance
fn builtin_chebyshev_distance_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let q = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let mut max_d = 0.0_f64;
    for i in 0..p.len().min(q.len()) {
        let d = (p[i].to_number() - q[i].to_number()).abs();
        if d > max_d { max_d = d; }
    }
    Ok(PerlValue::float(max_d))
}

// Minkowski distance
fn builtin_minkowski_distance_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let q = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let pp = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    if pp <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let mut sum = 0.0;
    for i in 0..p.len().min(q.len()) {
        sum += (p[i].to_number() - q[i].to_number()).abs().powf(pp);
    }
    Ok(PerlValue::float(sum.powf(1.0 / pp)))
}

// Cosine distance
fn builtin_cosine_distance_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let q = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let mut dot = 0.0; let mut np = 0.0; let mut nq = 0.0;
    for i in 0..p.len().min(q.len()) {
        let pi = p[i].to_number();
        let qi = q[i].to_number();
        dot += pi * qi;
        np += pi * pi;
        nq += qi * qi;
    }
    let denom = (np * nq).sqrt();
    if denom == 0.0 { return Ok(PerlValue::float(1.0)); }
    Ok(PerlValue::float(1.0 - dot / denom))
}

// Hamming distance for strings
fn builtin_hamming_distance_str(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let count: i64 = a.chars().zip(b.chars()).filter(|(x, y)| x != y).count() as i64;
    let extra: i64 = (a.chars().count() as i64 - b.chars().count() as i64).abs();
    Ok(PerlValue::integer(count + extra))
}

// Sphere surface from circle great-circle distance (haversine)
fn builtin_haversine_distance_b28(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lat1 = f1(args).to_radians();
    let lon1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lat2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lon2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let r = args.get(4).map(|v| v.to_number()).unwrap_or(6371000.0);
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    Ok(PerlValue::float(r * c))
}

// Vincenty distance simplified (great circle using law of cosines)
fn builtin_great_circle_law_of_cos(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lat1 = f1(args).to_radians();
    let lon1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lat2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lon2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let r = args.get(4).map(|v| v.to_number()).unwrap_or(6371000.0);
    let c = (lat1.sin() * lat2.sin() + lat1.cos() * lat2.cos() * (lon2 - lon1).cos()).clamp(-1.0, 1.0);
    Ok(PerlValue::float(r * c.acos()))
}

// Bearing (initial)
fn builtin_initial_bearing(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lat1 = f1(args).to_radians();
    let lon1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lat2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lon2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let dlon = lon2 - lon1;
    let y = dlon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
    Ok(PerlValue::float(y.atan2(x).to_degrees().rem_euclid(360.0)))
}

// Midpoint great circle
fn builtin_midpoint_great_circle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lat1 = f1(args).to_radians();
    let lon1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lat2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lon2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let bx = lat2.cos() * (lon2 - lon1).cos();
    let by = lat2.cos() * (lon2 - lon1).sin();
    let lat3 = (lat1.sin() + lat2.sin()).atan2(((lat1.cos() + bx).powi(2) + by.powi(2)).sqrt());
    let lon3 = lon1 + by.atan2(lat1.cos() + bx);
    Ok(PerlValue::array(vec![
        PerlValue::float(lat3.to_degrees()),
        PerlValue::float(lon3.to_degrees()),
    ]))
}

// Polygon shoelace area (signed)
fn builtin_shoelace_area(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = pts.len();
    if n < 3 { return Ok(PerlValue::float(0.0)); }
    let mut sum = 0.0;
    for i in 0..n {
        let p = arg_to_vec(&pts[i]);
        let q = arg_to_vec(&pts[(i + 1) % n]);
        sum += p[0].to_number() * q[1].to_number() - q[0].to_number() * p[1].to_number();
    }
    Ok(PerlValue::float(sum / 2.0))
}

// Polygon is convex (assumes ccw)
fn builtin_polygon_is_convex(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = pts.len();
    if n < 4 { return Ok(PerlValue::integer(if n >= 3 { 1 } else { 0 })); }
    let mut sign = 0_i32;
    for i in 0..n {
        let p = arg_to_vec(&pts[i]);
        let q = arg_to_vec(&pts[(i + 1) % n]);
        let r = arg_to_vec(&pts[(i + 2) % n]);
        let cross = (q[0].to_number() - p[0].to_number()) * (r[1].to_number() - q[1].to_number())
                  - (q[1].to_number() - p[1].to_number()) * (r[0].to_number() - q[0].to_number());
        let s = if cross > 0.0 { 1 } else if cross < 0.0 { -1 } else { 0 };
        if s != 0 {
            if sign == 0 { sign = s; }
            else if sign != s { return Ok(PerlValue::integer(0)); }
        }
    }
    Ok(PerlValue::integer(1))
}

// Convex hull jarvis march (gift wrapping, simplified — returns indices)
fn builtin_convex_hull_jarvis(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts: Vec<(f64, f64)> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| {
            let pp = arg_to_vec(v);
            (pp[0].to_number(), pp.get(1).map(|q| q.to_number()).unwrap_or(0.0))
        }).collect();
    let n = pts.len();
    if n < 3 { return Ok(PerlValue::array((0..n).map(|i| PerlValue::integer(i as i64)).collect())); }
    let mut leftmost = 0_usize;
    for i in 1..n {
        if pts[i].0 < pts[leftmost].0 { leftmost = i; }
    }
    let mut hull = vec![];
    let mut p = leftmost;
    loop {
        hull.push(p);
        let mut q = (p + 1) % n;
        for r in 0..n {
            let cross = (pts[q].0 - pts[p].0) * (pts[r].1 - pts[p].1)
                      - (pts[q].1 - pts[p].1) * (pts[r].0 - pts[p].0);
            if cross < 0.0 { q = r; }
        }
        p = q;
        if p == leftmost || hull.len() > n { break; }
    }
    Ok(PerlValue::array(hull.into_iter().map(|i| PerlValue::integer(i as i64)).collect()))
}

// Euler characteristic V - E + F
fn builtin_euler_characteristic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = i1(args);
    let e = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let f = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(v - e + f))
}

// Genus from Euler char (orientable): g = (2 - χ) / 2
fn builtin_genus_from_euler(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let chi = i1(args);
    Ok(PerlValue::integer((2 - chi) / 2))
}

// Spherical triangle area (excess formula)
fn builtin_spherical_triangle_area(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let pi = std::f64::consts::PI;
    Ok(PerlValue::float(r * r * (a + b + c - pi)))
}

// Polygon with holes area (Sum outer - Sum inner)
fn builtin_polygon_with_holes_area(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let outer_v = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let holes_v = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    fn shoelace(pts: &[PerlValue]) -> f64 {
        let n = pts.len();
        if n < 3 { return 0.0; }
        let mut s = 0.0;
        for i in 0..n {
            let p = arg_to_vec(&pts[i]);
            let q = arg_to_vec(&pts[(i + 1) % n]);
            s += p[0].to_number() * q[1].to_number() - q[0].to_number() * p[1].to_number();
        }
        (s / 2.0).abs()
    }
    let outer = shoelace(&outer_v);
    let inner: f64 = holes_v.iter().map(|h| shoelace(&arg_to_vec(h))).sum();
    Ok(PerlValue::float((outer - inner).max(0.0)))
}

// Pick's theorem: A = I + B/2 - 1
fn builtin_picks_theorem(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let interior = f1(args);
    let boundary = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(interior + boundary / 2.0 - 1.0))
}

// Centroid of N-D points
fn builtin_centroid_nd(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if pts.is_empty() { return Ok(PerlValue::array(vec![])); }
    let dim = arg_to_vec(&pts[0]).len();
    let mut sums = vec![0.0; dim];
    for p in &pts {
        let v = arg_to_vec(p);
        for i in 0..dim.min(v.len()) {
            sums[i] += v[i].to_number();
        }
    }
    let n = pts.len() as f64;
    Ok(PerlValue::array(sums.into_iter().map(|s| PerlValue::float(s / n)).collect()))
}

// Variance-covariance via centered points (returns flat row-major n×n)
fn builtin_covariance_matrix_pts(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if pts.is_empty() { return Ok(PerlValue::array(vec![])); }
    let dim = arg_to_vec(&pts[0]).len();
    let n = pts.len() as f64;
    let mut means = vec![0.0; dim];
    for p in &pts {
        let v = arg_to_vec(p);
        for i in 0..dim.min(v.len()) {
            means[i] += v[i].to_number();
        }
    }
    for m in &mut means { *m /= n; }
    let mut cov = vec![vec![0.0; dim]; dim];
    for p in &pts {
        let v = arg_to_vec(p);
        for i in 0..dim {
            for j in 0..dim {
                let vi = v.get(i).map(|q| q.to_number()).unwrap_or(0.0) - means[i];
                let vj = v.get(j).map(|q| q.to_number()).unwrap_or(0.0) - means[j];
                cov[i][j] += vi * vj;
            }
        }
    }
    let denom = (n - 1.0).max(1.0);
    let out: Vec<PerlValue> = cov.into_iter()
        .map(|row| PerlValue::array(row.into_iter().map(|x| PerlValue::float(x / denom)).collect()))
        .collect();
    Ok(PerlValue::array(out))
}

// Simplex volume (n+1 points in n-D, using Cayley-Menger determinant approximation for n=3)
fn builtin_simplex_volume_3d(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_tetrahedron_volume_b28(args)
}
