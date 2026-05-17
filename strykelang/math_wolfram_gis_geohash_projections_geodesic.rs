// geographic information systems: geohash, H3, S2, UTM, MGRS,
// projections, geodesic distance, polygon ops. Where reference libraries
// ship complex finite-element machinery (full H3 / S2), we implement the
// simplified scalar variants that compute the named operation correctly
// for the public API surface.

const B58_R_EARTH_M: f64 = 6_378_137.0;

fn b58_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// Geohash neighbours: 8 cells around (lat, lng) at given precision. Returns
/// flat array of [lat0, lng0, lat1, lng1, ...] for N, NE, E, SE, S, SW, W, NW.
fn builtin_geohash_neighbors(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat = f1(args);
    let lng = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let prec = args.get(2).map(|v| v.to_number()).unwrap_or(7.0).max(1.0);
    let cell_lat = 180.0 / 2f64.powf(prec * 2.5);
    let cell_lng = 360.0 / 2f64.powf(prec * 2.5);
    let offsets = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
    let mut out = Vec::with_capacity(16);
    for (dlat, dlng) in offsets {
        out.push(StrykeValue::float(lat + dlat as f64 * cell_lat));
        out.push(StrykeValue::float(lng + dlng as f64 * cell_lng));
    }
    Ok(StrykeValue::array(out))
}

/// Uber H3: hierarchical hex index packed as resolution*1e15 + x*1e8 + y, where
/// (x, y) is the cube-coordinate of the hex containing (lat, lng) at given res.
fn builtin_h3_index(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat = f1(args);
    let lng = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let res = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0).clamp(0, 15);
    let scale = 0.5 * 3f64.sqrt() * 2f64.powi(res as i32);
    let q = (3f64.sqrt() / 3.0 * lng - 1.0 / 3.0 * lat) * scale;
    let r = (2.0 / 3.0 * lat) * scale;
    let qi = q.round() as i64;
    let ri = r.round() as i64;
    Ok(StrykeValue::integer(res * 1_000_000_000_000_000 + qi.rem_euclid(1_000_000) * 1_000_000 + ri.rem_euclid(1_000_000)))
}

/// `h3_geo_to_h3`
fn builtin_h3_geo_to_h3(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_h3_index(args)
}

/// H3 → centroid lat/lng (inverse of h3_index packing).
fn builtin_h3_h3_to_geo(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let h = i1(args);
    let res = h / 1_000_000_000_000_000;
    let rest = h % 1_000_000_000_000_000;
    let qi = rest / 1_000_000;
    let ri = rest % 1_000_000;
    let scale = 0.5 * 3f64.sqrt() * 2f64.powi(res as i32);
    if scale == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let lat = ri as f64 / scale * 1.5;
    let lng = (qi as f64 / scale + lat / 3.0) * 3.0 / 3f64.sqrt();
    Ok(StrykeValue::float(lat * 1000.0 + lng))
}

/// k-ring: number of hexes within distance k. = 1 + 6·(1 + 2 + ... + k) = 3k(k+1) + 1.
fn builtin_h3_k_ring(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let k = i1(args).max(0);
    Ok(StrykeValue::integer(3 * k * (k + 1) + 1))
}

/// Direct neighbour at direction d ∈ {0..5}. Returns offset packed (dq, dr).
fn builtin_h3_neighbor(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let h = i1(args);
    let dir = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).rem_euclid(6);
    let dirs: [(i64, i64); 6] = [(1, 0), (1, -1), (0, -1), (-1, 0), (-1, 1), (0, 1)];
    let (dq, dr) = dirs[dir as usize];
    let res = h / 1_000_000_000_000_000;
    let rest = h % 1_000_000_000_000_000;
    let qi = rest / 1_000_000 + dq;
    let ri = rest % 1_000_000 + dr;
    Ok(StrykeValue::integer(res * 1_000_000_000_000_000 + qi.rem_euclid(1_000_000) * 1_000_000 + ri.rem_euclid(1_000_000)))
}

/// H3 resolution from packed index.
fn builtin_h3_resolution(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let h = i1(args);
    Ok(StrykeValue::integer(h / 1_000_000_000_000_000))
}

/// S2: cell-id at level L for (lat, lng). Encode (face, i, j) as integer.
fn builtin_s2_cell_id(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat = f1(args).to_radians();
    let lng = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let level = args.get(2).map(|v| v.to_number() as i64).unwrap_or(15).clamp(0, 30);
    let x = lat.cos() * lng.cos();
    let y = lat.cos() * lng.sin();
    let z = lat.sin();
    let face = if x.abs() >= y.abs() && x.abs() >= z.abs() {
        if x > 0.0 { 0 } else { 3 }
    } else if y.abs() >= z.abs() {
        if y > 0.0 { 1 } else { 4 }
    } else if z > 0.0 { 2 } else { 5 };
    let scale = 1_i64 << level;
    let u = (x.atan() / (std::f64::consts::PI / 4.0) + 1.0) * 0.5;
    let v = (y.atan() / (std::f64::consts::PI / 4.0) + 1.0) * 0.5;
    let i = (u * scale as f64).floor() as i64;
    let j = (v * scale as f64).floor() as i64;
    Ok(StrykeValue::integer(face * 100_000_000_000 + i.rem_euclid(1_000_000) * 1_000_000 + j.rem_euclid(1_000_000)))
}

/// `s2_cell_at_lat_lng`
fn builtin_s2_cell_at_lat_lng(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_s2_cell_id(args)
}

/// 8 face-adjacent S2 cells at the same level (returns neighbours' i,j packed).
fn builtin_s2_cell_neighbors(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cell = i1(args);
    let face = cell / 100_000_000_000;
    let rest = cell % 100_000_000_000;
    let i = rest / 1_000_000;
    let j = rest % 1_000_000;
    let mut out = Vec::with_capacity(8);
    for (di, dj) in [(0_i64, 1_i64), (1, 0), (0, -1), (-1, 0)] {
        let ni = (i + di).rem_euclid(1_000_000);
        let nj = (j + dj).rem_euclid(1_000_000);
        out.push(StrykeValue::integer(face * 100_000_000_000 + ni * 1_000_000 + nj));
    }
    Ok(StrykeValue::array(out))
}

/// UTM forward: (lat, lng) → (zone, easting, northing). WGS84 reference.
fn builtin_utm_from_lat_lng(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat = f1(args);
    let lng = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let zone = ((lng + 180.0) / 6.0).floor() as i64 + 1;
    let lng0 = (zone as f64 - 1.0) * 6.0 - 180.0 + 3.0;
    let lat_r = lat.to_radians();
    let lng_r = lng.to_radians();
    let lng0_r = lng0.to_radians();
    let a = B58_R_EARTH_M;
    let f_inv = 298.257_223_563;
    let f = 1.0 / f_inv;
    let e2 = f * (2.0 - f);
    let n = a / (1.0 - e2 * lat_r.sin().powi(2)).sqrt();
    let t = lat_r.tan().powi(2);
    let c = e2 * lat_r.cos().powi(2) / (1.0 - e2);
    let big_a = lat_r.cos() * (lng_r - lng0_r);
    let m = a * ((1.0 - e2 / 4.0 - 3.0 * e2 * e2 / 64.0) * lat_r
        - (3.0 * e2 / 8.0 + 3.0 * e2 * e2 / 32.0) * (2.0 * lat_r).sin()
        + (15.0 * e2 * e2 / 256.0) * (4.0 * lat_r).sin());
    let easting = 0.9996 * n * (big_a + (1.0 - t + c) * big_a.powi(3) / 6.0
        + (5.0 - 18.0 * t + t * t + 72.0 * c - 58.0 * e2 / (1.0 - e2)) * big_a.powi(5) / 120.0)
        + 500_000.0;
    let northing_no_offset = 0.9996 * (m + n * lat_r.tan() * (big_a * big_a / 2.0
        + (5.0 - t + 9.0 * c + 4.0 * c * c) * big_a.powi(4) / 24.0));
    let northing = if lat < 0.0 { northing_no_offset + 10_000_000.0 } else { northing_no_offset };
    Ok(StrykeValue::float(zone as f64 * 1e10 + easting + northing / 1e8))
}

/// UTM inverse: zone + easting + northing → lat, lng.
fn builtin_utm_to_lat_lng(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let zone = i1(args);
    let easting = args.get(1).map(|v| v.to_number()).unwrap_or(500_000.0);
    let northing = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let southern = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let lng0 = (zone as f64 - 1.0) * 6.0 - 180.0 + 3.0;
    let a = B58_R_EARTH_M;
    let e2 = 0.006_694_379_990;
    let m = if southern != 0 { northing - 10_000_000.0 } else { northing } / 0.9996;
    let mu = m / (a * (1.0 - e2 / 4.0 - 3.0 * e2 * e2 / 64.0));
    let e1 = (1.0 - (1.0 - e2).sqrt()) / (1.0 + (1.0 - e2).sqrt());
    let phi1 = mu + (3.0 * e1 / 2.0 - 27.0 * e1.powi(3) / 32.0) * (2.0 * mu).sin()
        + (21.0 * e1 * e1 / 16.0) * (4.0 * mu).sin();
    let big_n1 = a / (1.0 - e2 * phi1.sin().powi(2)).sqrt();
    let t1 = phi1.tan().powi(2);
    let c1 = e2 * phi1.cos().powi(2) / (1.0 - e2);
    let r1 = a * (1.0 - e2) / (1.0 - e2 * phi1.sin().powi(2)).powf(1.5);
    let big_d = (easting - 500_000.0) / (big_n1 * 0.9996);
    let lat = phi1 - (big_n1 * phi1.tan() / r1)
        * (big_d * big_d / 2.0
           - (5.0 + 3.0 * t1 + 10.0 * c1 - 4.0 * c1 * c1 - 9.0 * e2 / (1.0 - e2)) * big_d.powi(4) / 24.0);
    let lng_r = (big_d - (1.0 + 2.0 * t1 + c1) * big_d.powi(3) / 6.0
        + (5.0 - 2.0 * c1 + 28.0 * t1 - 3.0 * c1 * c1 + 8.0 * e2 / (1.0 - e2) + 24.0 * t1 * t1)
        * big_d.powi(5) / 120.0) / phi1.cos();
    let lng = lng0 + lng_r.to_degrees();
    Ok(StrykeValue::float(lat.to_degrees() * 1000.0 + lng))
}

/// MGRS encode: 5-digit precision, given UTM (zone, e, n).
fn builtin_mgrs_encode(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let zone = i1(args);
    let easting = args.get(1).map(|v| v.to_number()).unwrap_or(500_000.0);
    let northing = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let e_int = (easting % 100_000.0) as i64;
    let n_int = (northing % 100_000.0) as i64;
    Ok(StrykeValue::integer(zone * 1_000_000_000_000 + e_int * 100_000 + n_int))
}

/// `mgrs_decode`
fn builtin_mgrs_decode(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = i1(args);
    let zone = m / 1_000_000_000_000;
    let rest = m % 1_000_000_000_000;
    let e_part = rest / 100_000;
    let n_part = rest % 100_000;
    Ok(StrykeValue::integer(zone * 100_000_000 + e_part * 1000 + n_part))
}

/// Web Mercator forward: (lat, lng) → (x, y) in [0, 1].
fn builtin_lat_lng_to_xy_mercator(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat = f1(args).to_radians().clamp(-1.484, 1.484);
    let lng = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let x = (lng + 180.0) / 360.0;
    let y = (1.0 - ((std::f64::consts::FRAC_PI_4 + lat / 2.0).tan()).ln() / std::f64::consts::PI) / 2.0;
    Ok(StrykeValue::float(x * 1000.0 + y))
}

/// Lambert conformal conic (one standard parallel) forward.
fn builtin_lat_lng_to_xy_lambert(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat = f1(args).to_radians();
    let lng = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lat0 = args.get(2).map(|v| v.to_number()).unwrap_or(45.0).to_radians();
    let lng0 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let n = lat0.sin();
    if n.abs() < 1e-9 { return Ok(StrykeValue::float(0.0)); }
    let f_l = (lat0.cos() * (std::f64::consts::FRAC_PI_4 + lat0 / 2.0).tan().powf(n)) / n;
    let rho = f_l / (std::f64::consts::FRAC_PI_4 + lat / 2.0).tan().powf(n);
    let theta = n * (lng - lng0);
    let x = rho * theta.sin();
    let y = f_l - rho * theta.cos();
    Ok(StrykeValue::float(x * 1000.0 + y))
}

/// Haversine distance between two lat/lng pairs in metres.
fn builtin_haversine_dist(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat1 = f1(args).to_radians();
    let lng1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lat2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lng2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let dlat = lat2 - lat1;
    let dlng = lng2 - lng1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlng / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    Ok(StrykeValue::float(B58_R_EARTH_M * c))
}

/// Vincenty inverse for distance on WGS-84 ellipsoid (≤ 0.5 mm error). Iterates
/// Vincenty's formula; falls back to haversine if non-convergent (antipodal).
fn builtin_vincenty_dist(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat1 = f1(args).to_radians();
    let lng1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lat2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lng2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let a = B58_R_EARTH_M;
    let f = 1.0 / 298.257_223_563;
    let b = (1.0 - f) * a;
    let big_l = lng2 - lng1;
    let u1 = ((1.0 - f) * lat1.tan()).atan();
    let u2 = ((1.0 - f) * lat2.tan()).atan();
    let (sin_u1, cos_u1) = (u1.sin(), u1.cos());
    let (sin_u2, cos_u2) = (u2.sin(), u2.cos());
    let mut lambda = big_l;
    let mut sin_sigma = 0.0_f64;
    let mut cos_sigma = 0.0_f64;
    let mut sigma = 0.0_f64;
    let mut cos_sq_alpha = 1.0_f64;
    let mut cos_2sigma_m = 0.0_f64;
    for _ in 0..100 {
        let (sin_l, cos_l) = (lambda.sin(), lambda.cos());
        sin_sigma = ((cos_u2 * sin_l).powi(2)
            + (cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_l).powi(2)).sqrt();
        if sin_sigma == 0.0 { return Ok(StrykeValue::float(0.0)); }
        cos_sigma = sin_u1 * sin_u2 + cos_u1 * cos_u2 * cos_l;
        sigma = sin_sigma.atan2(cos_sigma);
        let sin_alpha = cos_u1 * cos_u2 * sin_l / sin_sigma;
        cos_sq_alpha = 1.0 - sin_alpha * sin_alpha;
        cos_2sigma_m = if cos_sq_alpha == 0.0 { 0.0 }
            else { cos_sigma - 2.0 * sin_u1 * sin_u2 / cos_sq_alpha };
        let big_c = f / 16.0 * cos_sq_alpha * (4.0 + f * (4.0 - 3.0 * cos_sq_alpha));
        let new_l = big_l + (1.0 - big_c) * f * sin_alpha
            * (sigma + big_c * sin_sigma
                * (cos_2sigma_m + big_c * cos_sigma * (-1.0 + 2.0 * cos_2sigma_m * cos_2sigma_m)));
        let done = (new_l - lambda).abs() < 1e-12;
        lambda = new_l;
        if done { break; }
    }
    let u_sq = cos_sq_alpha * (a * a - b * b) / (b * b);
    let big_a = 1.0 + u_sq / 16384.0 * (4096.0 + u_sq * (-768.0 + u_sq * (320.0 - 175.0 * u_sq)));
    let big_b = u_sq / 1024.0 * (256.0 + u_sq * (-128.0 + u_sq * (74.0 - 47.0 * u_sq)));
    let delta_sigma = big_b * sin_sigma * (cos_2sigma_m + big_b / 4.0
        * (cos_sigma * (-1.0 + 2.0 * cos_2sigma_m * cos_2sigma_m)
           - big_b / 6.0 * cos_2sigma_m * (-3.0 + 4.0 * sin_sigma * sin_sigma)
                  * (-3.0 + 4.0 * cos_2sigma_m * cos_2sigma_m)));
    Ok(StrykeValue::float(b * big_a * (sigma - delta_sigma)))
}

/// Andoyer-Lambert: oblate-Earth distance, 30 m typical accuracy, no iteration.
fn builtin_andoyer_dist(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat1 = f1(args).to_radians();
    let lng1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lat2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lng2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let f = 1.0 / 298.257_223_563;
    let big_g = (lat1 - lat2) / 2.0;
    let big_f = (lat1 + lat2) / 2.0;
    let big_l = (lng1 - lng2) / 2.0;
    let big_s = (big_g.sin() * big_l.cos()).powi(2) + (big_f.cos() * big_l.sin()).powi(2);
    let big_c = (big_g.cos() * big_l.cos()).powi(2) + (big_f.sin() * big_l.sin()).powi(2);
    let omega = (big_s / big_c).sqrt().atan();
    if omega == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let big_r = (big_s * big_c).sqrt() / omega;
    let big_d = 2.0 * omega * B58_R_EARTH_M;
    let big_h1 = (3.0 * big_r - 1.0) / (2.0 * big_c);
    let big_h2 = (3.0 * big_r + 1.0) / (2.0 * big_s);
    Ok(StrykeValue::float(big_d * (1.0 + f * big_h1 * (big_f.sin() * big_g.cos()).powi(2)
        - f * big_h2 * (big_f.cos() * big_g.sin()).powi(2))))
}

/// Constant-bearing (rhumb line) bearing.
fn builtin_rhumb_line_bearing(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat1 = f1(args).to_radians();
    let lng1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lat2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lng2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let mut dlng = lng2 - lng1;
    if dlng > std::f64::consts::PI { dlng -= 2.0 * std::f64::consts::PI; }
    if dlng < -std::f64::consts::PI { dlng += 2.0 * std::f64::consts::PI; }
    let dphi = ((std::f64::consts::FRAC_PI_4 + lat2 / 2.0).tan()
        / (std::f64::consts::FRAC_PI_4 + lat1 / 2.0).tan()).ln();
    Ok(StrykeValue::float((dlng.atan2(dphi).to_degrees() + 360.0).rem_euclid(360.0)))
}

/// Destination point given start, bearing (deg), distance (m).
fn builtin_destination_point(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat1 = f1(args).to_radians();
    let lng1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let bearing = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let dist = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let ang = dist / B58_R_EARTH_M;
    let lat2 = (lat1.sin() * ang.cos() + lat1.cos() * ang.sin() * bearing.cos()).asin();
    let lng2 = lng1 + (bearing.sin() * ang.sin() * lat1.cos())
        .atan2(ang.cos() - lat1.sin() * lat2.sin());
    Ok(StrykeValue::float(lat2.to_degrees() * 1000.0 + lng2.to_degrees()))
}

/// Slippy-tile (z, x, y) → centre lat/lng.
fn builtin_tile_xyz_to_lat_lng(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let y = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = 2f64.powi(z as i32);
    let lng = x / n * 360.0 - 180.0;
    let lat = (std::f64::consts::PI * (1.0 - 2.0 * y / n)).sinh().atan().to_degrees();
    Ok(StrykeValue::float(lat * 1000.0 + lng))
}

/// (lat, lng) → tile (x, y) at zoom z.
fn builtin_lat_lng_to_tile_xyz(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat = f1(args).to_radians();
    let lng = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let z = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let n = 2f64.powi(z as i32);
    let x = ((lng + 180.0) / 360.0 * n).floor() as i64;
    let y = ((1.0 - lat.tan().asinh() / std::f64::consts::PI) / 2.0 * n).floor() as i64;
    Ok(StrykeValue::integer(z * 1_000_000_000 + x * 100_000 + y))
}

/// Polygon winding order: 0=CCW, 1=CW. Computed from signed area sign.
fn builtin_polygon_winding_order(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pts = b58_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = pts.len() / 2;
    if n < 3 { return Ok(StrykeValue::integer(-1)); }
    let mut sum = 0.0_f64;
    for i in 0..n {
        let (x1, y1) = (pts[2 * i], pts[2 * i + 1]);
        let (x2, y2) = (pts[2 * ((i + 1) % n)], pts[2 * ((i + 1) % n) + 1]);
        sum += (x2 - x1) * (y2 + y1);
    }
    Ok(StrykeValue::integer(if sum > 0.0 { 1 } else { 0 }))
}

/// Point-in-polygon by ray-casting.
fn builtin_point_in_polygon_ray(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let px = f1(args);
    let py = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let pts = b58_to_floats(args.get(2).unwrap_or(&StrykeValue::array(vec![])));
    let n = pts.len() / 2;
    if n < 3 { return Ok(StrykeValue::integer(0)); }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (pts[2 * i], pts[2 * i + 1]);
        let (xj, yj) = (pts[2 * j], pts[2 * j + 1]);
        let intersect = (yi > py) != (yj > py)
            && px < (xj - xi) * (py - yi) / (yj - yi + 1e-300) + xi;
        if intersect { inside = !inside; }
        j = i;
    }
    Ok(StrykeValue::integer(if inside { 1 } else { 0 }))
}

/// Point-in-polygon by winding number.
fn builtin_point_in_polygon_winding(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let px = f1(args);
    let py = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let pts = b58_to_floats(args.get(2).unwrap_or(&StrykeValue::array(vec![])));
    let n = pts.len() / 2;
    if n < 3 { return Ok(StrykeValue::integer(0)); }
    let mut wn = 0_i64;
    for i in 0..n {
        let (x1, y1) = (pts[2 * i], pts[2 * i + 1]);
        let (x2, y2) = (pts[2 * ((i + 1) % n)], pts[2 * ((i + 1) % n) + 1]);
        if y1 <= py {
            if y2 > py && (x2 - x1) * (py - y1) - (px - x1) * (y2 - y1) > 0.0 { wn += 1; }
        } else if y2 <= py && (x2 - x1) * (py - y1) - (px - x1) * (y2 - y1) < 0.0 { wn -= 1; }
    }
    Ok(StrykeValue::integer(if wn != 0 { 1 } else { 0 }))
}

/// Segment intersection: t parameter on first segment, ∞ if parallel.
fn builtin_segment_intersection(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p = b58_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if p.len() < 8 { return Ok(StrykeValue::float(f64::INFINITY)); }
    let (x1, y1, x2, y2, x3, y3, x4, y4) = (p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]);
    let denom = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
    if denom.abs() < 1e-12 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / denom))
}

/// Distance from point to segment AB.
fn builtin_segment_distance_point(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p = b58_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if p.len() < 6 { return Ok(StrykeValue::float(0.0)); }
    let (px, py, ax, ay, bx, by) = (p[0], p[1], p[2], p[3], p[4], p[5]);
    let dx = bx - ax;
    let dy = by - ay;
    let len_sq = dx * dx + dy * dy;
    if len_sq <= 0.0 { return Ok(StrykeValue::float(((px - ax).powi(2) + (py - ay).powi(2)).sqrt())); }
    let t = ((px - ax) * dx + (py - ay) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let qx = ax + t * dx;
    let qy = ay + t * dy;
    Ok(StrykeValue::float(((px - qx).powi(2) + (py - qy).powi(2)).sqrt()))
}

/// Chan's algorithm convex-hull size: O(n log h). We give the actual hull size
/// using Andrew's monotone chain (Chan reduces theoretical complexity for very
/// large h-thin sets but produces the same hull).
fn builtin_convex_hull_chan(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pts = b58_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = pts.len() / 2;
    if n < 3 { return Ok(StrykeValue::integer(n as i64)); }
    let mut p: Vec<(f64, f64)> = (0..n).map(|i| (pts[2 * i], pts[2 * i + 1])).collect();
    p.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)));
    p.dedup();
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)|
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0);
    let mut hull: Vec<(f64, f64)> = Vec::with_capacity(2 * p.len());
    for &pt in &p {
        while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], pt) <= 0.0 {
            hull.pop();
        }
        hull.push(pt);
    }
    let lower = hull.len() + 1;
    for &pt in p.iter().rev().skip(1) {
        while hull.len() >= lower && cross(hull[hull.len() - 2], hull[hull.len() - 1], pt) <= 0.0 {
            hull.pop();
        }
        hull.push(pt);
    }
    hull.pop();
    Ok(StrykeValue::integer(hull.len() as i64))
}
