// Batch 55 — astronomy / astrometry. Reference: Jean Meeus, "Astronomical
// Algorithms" 2nd ed., and IAU 2006/2000 conventions for precession/nutation.

const B55_J2000: f64 = 2_451_545.0; // JD of 2000-01-01 12h TT
const B55_AU_KM: f64 = 149_597_870.7;

fn b55_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// Julian Day from proleptic Gregorian (Y, M, D, hour). Meeus §7.
fn builtin_julian_day(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let hour = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let (yy, mm) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let a = yy.div_euclid(100);
    let b = 2 - a + a.div_euclid(4);
    let day_frac = d + hour / 24.0;
    let jd = (365.25 * (yy + 4716) as f64).floor()
        + (30.6001 * (mm + 1) as f64).floor()
        + day_frac + b as f64 - 1524.5;
    Ok(StrykeValue::float(jd))
}

/// Inverse: JD → (Y*10000 + M*100 + D, fractional day).
fn builtin_jd_to_calendar(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let jd = f1(args) + 0.5;
    let z = jd.floor() as i64;
    let f = jd - z as f64;
    let alpha = ((z as f64 - 1867216.25) / 36524.25).floor() as i64;
    let big_a = if z < 2299161 { z } else { z + 1 + alpha - alpha / 4 };
    let big_b = big_a + 1524;
    let big_c = ((big_b as f64 - 122.1) / 365.25).floor() as i64;
    let big_d = (365.25 * big_c as f64).floor() as i64;
    let big_e = ((big_b - big_d) as f64 / 30.6001).floor() as i64;
    let day = (big_b - big_d) as f64 - (30.6001 * big_e as f64).floor() + f;
    let month = if big_e < 14 { big_e - 1 } else { big_e - 13 };
    let year = if month > 2 { big_c - 4716 } else { big_c - 4715 };
    Ok(StrykeValue::float(year as f64 * 10000.0 + month as f64 * 100.0 + day))
}

/// TT → TDB: a tiny periodic correction. Approximate Fairhead-Bretagnon series,
/// keeping the leading sinusoidal term at amplitude 1.658e-3 s.
fn builtin_tt_to_tdb(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let jd_tt = f1(args);
    let g = (357.53 + 0.985_600_28 * (jd_tt - B55_J2000)).to_radians();
    let tt_minus_tdb = 0.001_658 * g.sin() + 0.000_014 * (2.0 * g).sin();
    Ok(StrykeValue::float(jd_tt + tt_minus_tdb / 86400.0))
}

/// Equatorial → horizontal: given (RA hours, Dec deg, latitude deg, LST hours),
/// return azimuth deg, altitude deg packed as az*1000 + alt.
fn builtin_ra_dec_to_alt_az(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ra = f1(args);
    let dec = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lat = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let lst = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let ha = (lst - ra) * 15.0;
    let ha_r = ha.to_radians();
    let dec_r = dec.to_radians();
    let lat_r = lat.to_radians();
    let alt = (lat_r.sin() * dec_r.sin() + lat_r.cos() * dec_r.cos() * ha_r.cos()).asin();
    let az = (-ha_r.sin()).atan2(lat_r.cos() * dec_r.tan() - lat_r.sin() * ha_r.cos());
    let az_deg = (az.to_degrees() + 360.0).rem_euclid(360.0);
    let alt_deg = alt.to_degrees();
    Ok(StrykeValue::float(az_deg * 1000.0 + alt_deg))
}

/// Horizontal → equatorial inverse.
fn builtin_alt_az_to_ra_dec(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let az = f1(args).to_radians();
    let alt = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lat = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lst = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dec = (lat.sin() * alt.sin() + lat.cos() * alt.cos() * az.cos()).asin();
    let ha = (-az.sin() * alt.cos()).atan2(lat.cos() * alt.sin() - lat.sin() * alt.cos() * az.cos());
    let ra = (lst * 15.0 - ha.to_degrees() + 360.0).rem_euclid(360.0) / 15.0;
    Ok(StrykeValue::float(ra * 1000.0 + dec.to_degrees()))
}

/// IAU 2006 precession (Capitaine et al.) angle ξ_A for date as polynomial in T
/// (centuries since J2000). Returns ξ_A in arcseconds.
fn builtin_precession_iau2006(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let jd = f1(args);
    let t = (jd - B55_J2000) / 36525.0;
    let xi_a = 2.650545 + t * (2306.083227 + t * (0.298_849_9 + t *
        (0.018_018_28 + t * (-0.000_005_971 + t * -0.000_000_316_5))));
    Ok(StrykeValue::float(xi_a))
}

/// IAU 2000A nutation in longitude (truncated to leading 5 terms). Returns Δψ
/// in arcseconds. Full series has 1365 terms; this covers ≥99% of the amplitude.
fn builtin_nutation_iau2000a(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let jd = f1(args);
    let t = (jd - B55_J2000) / 36525.0;
    let omega = (125.044_555 - 1_934.136_261 * t).to_radians();
    let l = (357.527_723_3 + 35_999.050_34 * t).to_radians();
    let lp = (134.962_981_4 + 477_198.867_4 * t).to_radians();
    let f = (93.271_910 + 483_202.017_5 * t).to_radians();
    let d = (297.850_363 + 445_267.111_5 * t).to_radians();
    let dpsi = -17.20 * omega.sin()
        - 1.319 * (2.0 * (f - d + omega)).sin()
        - 0.227 * (2.0 * (f + omega)).sin()
        + 0.206 * (2.0 * omega).sin()
        + 0.143 * l.sin()
        + 0.071 * lp.sin();
    Ok(StrykeValue::float(dpsi))
}

/// Annual aberration: ⊿λ = -κ cos(O - λ) sec β + κ e cos(π - λ) sec β.
/// Simplified with circular-orbit approximation (drops eccentricity term).
/// κ = 20.49552 arcsec. Args: solar longitude O (deg), object ecliptic λ, β.
fn builtin_aberration_annual(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sun_lon = f1(args).to_radians();
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let kappa = 20.495_52;
    Ok(StrykeValue::float(-kappa * (sun_lon - lambda).cos() / beta.cos()))
}

/// Apply proper motion μ_α, μ_δ (mas/yr) over Δt years to (RA, Dec) in deg.
fn builtin_proper_motion_apply(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ra = f1(args);
    let dec = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mu_alpha_mas = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let mu_delta_mas = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dt_years = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let new_ra = ra + mu_alpha_mas * dt_years / (3_600_000.0 * dec.to_radians().cos());
    let new_dec = dec + mu_delta_mas * dt_years / 3_600_000.0;
    Ok(StrykeValue::float(new_ra * 1000.0 + new_dec))
}

/// Annual parallax shift: π·sin(λ_sun − λ) for object at distance d_pc.
/// Returns offset in arcseconds.
fn builtin_parallax_correction(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d_pc = f1(args);
    let lambda_diff_deg = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if d_pc <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    let parallax_arcsec = 1.0 / d_pc;
    Ok(StrykeValue::float(parallax_arcsec * lambda_diff_deg.to_radians().sin()))
}

/// Sun's geocentric ecliptic longitude (low precision, Meeus §25). Returns deg.
fn builtin_sun_position_low(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let jd = f1(args);
    let n = jd - B55_J2000;
    let l0 = (280.460 + 0.985_647_4 * n).rem_euclid(360.0);
    let g = (357.528 + 0.985_600_28 * n).to_radians();
    let lambda = l0 + 1.915 * g.sin() + 0.020 * (2.0 * g).sin();
    Ok(StrykeValue::float(lambda.rem_euclid(360.0)))
}

/// Earth-Sun distance in AU (Meeus §25 low precision).
fn builtin_sun_distance_au(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let jd = f1(args);
    let n = jd - B55_J2000;
    let g = (357.528 + 0.985_600_28 * n).to_radians();
    let r = 1.000_14 - 0.016_71 * g.cos() - 0.000_14 * (2.0 * g).cos();
    Ok(StrykeValue::float(r))
}

/// Moon's geocentric ecliptic longitude, low-precision (Meeus §47, leading
/// 4 terms only). Returns deg.
fn builtin_moon_position_low(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let jd = f1(args);
    let t = (jd - B55_J2000) / 36525.0;
    let lp = 218.316 + 481_267.881_3 * t;
    let m = (134.963 + 477_198.867_6 * t).to_radians();
    let mp = (357.527 + 35_999.050_3 * t).to_radians();
    let f = (93.272 + 483_202.017 * t).to_radians();
    let lambda = lp + 6.289 * m.sin() - 1.274 * (2.0 * f - m).sin()
        + 0.658 * (2.0 * f).sin() - 0.186 * mp.sin();
    Ok(StrykeValue::float(lambda.rem_euclid(360.0)))
}

/// Moon phase age in days (since new moon). Synodic month = 29.530589 days.
fn builtin_moon_phase_age(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let jd = f1(args);
    let known_new = 2_451_550.1; // 2000-01-06 New Moon
    Ok(StrykeValue::float(((jd - known_new) % 29.530_589 + 29.530_589) % 29.530_589))
}

/// Lunation index (count of new moons since 2000-01-06).
fn builtin_lunation_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let jd = f1(args);
    let known_new = 2_451_550.1;
    Ok(StrykeValue::integer(((jd - known_new) / 29.530_589).floor() as i64))
}

/// Eclipse magnitude estimate from sun/moon angular separation. Returns 0–1.
/// Args: separation_arcsec, sun_radius_arcsec, moon_radius_arcsec.
fn builtin_eclipse_magnitude(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sep = f1(args).abs();
    let r_sun = args.get(1).map(|v| v.to_number()).unwrap_or(960.0);
    let r_moon = args.get(2).map(|v| v.to_number()).unwrap_or(933.0);
    if sep >= r_sun + r_moon { return Ok(StrykeValue::float(0.0)); }
    if sep <= (r_sun - r_moon).abs() {
        return Ok(StrykeValue::float(if r_moon >= r_sun { 1.0 } else { r_moon / r_sun }));
    }
    let mag = (r_sun + r_moon - sep) / (2.0 * r_sun);
    Ok(StrykeValue::float(mag.clamp(0.0, 1.0)))
}

/// Saros cycle: one saros = 18.031 years ≈ 6585.32 days.
fn builtin_saros_cycle(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let jd = f1(args);
    let saros_days = 6_585.321_347;
    Ok(StrykeValue::integer(((jd - 2_018_999.0) / saros_days).floor() as i64))
}

/// Metonic cycle: 19 tropical years ≈ 235 lunations.
fn builtin_metonic_cycle(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let year = i1(args);
    Ok(StrykeValue::integer((year - 1).rem_euclid(19) + 1))
}

/// Kepler's 3rd law: T² = a³ / M_sun  → T_years = sqrt(a_AU³ / M_solar).
fn builtin_orbit_kepler3(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_au = f1(args).max(0.0);
    let m_sun = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float((a_au.powi(3) / m_sun).sqrt()))
}

/// Orbital period in years given semi-major axis in AU.
fn builtin_orbital_period_au(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_orbit_kepler3(args)
}

/// Eccentric anomaly E from M, e via Newton-Raphson on M = E - e sin E.
fn builtin_orbit_eccentric_anomaly(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let e = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).clamp(0.0, 0.999);
    let mut big_e = m;
    for _ in 0..20 {
        let f = big_e - e * big_e.sin() - m;
        let fp = 1.0 - e * big_e.cos();
        if fp.abs() < 1e-15 { break; }
        big_e -= f / fp;
    }
    Ok(StrykeValue::float(big_e))
}

/// Escape velocity: v = sqrt(2 G M / r). Args: M_kg, r_m.
fn builtin_escape_velocity_body(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    let big_g = 6.674_30e-11;
    Ok(StrykeValue::float((2.0 * big_g * m / r).sqrt()))
}

/// Hill sphere radius r_H = a(1-e)·(m/(3M))^(1/3). Args: a, e, m, M.
fn builtin_hill_sphere_radius(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let e = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let big_m = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float(a * (1.0 - e) * (m / (3.0 * big_m)).cbrt()))
}

/// Tisserand parameter T_J = a_J/a + 2·sqrt((a/a_J)(1-e²))·cos i.
fn builtin_tisserand_param(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let a_j = args.get(1).map(|v| v.to_number()).unwrap_or(5.2);
    let e = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let i_deg = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    if a <= 0.0 || a_j <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(a_j / a + 2.0 * ((a / a_j) * (1.0 - e * e)).sqrt() * i_deg.to_radians().cos()))
}

/// TLE mean motion (rev/day) → semi-major axis (km). a = (mu / (n·2π/86400)²)^(1/3).
fn builtin_tle_mean_motion(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_rev_day = f1(args).max(1e-9);
    let mu_km3_s2 = 398_600.441_8_f64;
    let n_rad_s = n_rev_day * 2.0 * std::f64::consts::PI / 86400.0;
    Ok(StrykeValue::float((mu_km3_s2 / (n_rad_s * n_rad_s)).cbrt()))
}

/// Single-step SGP4 mean-anomaly propagation: M(t) = M₀ + n·dt, where n is the
/// kozai mean motion. Args: M₀, n_rad_per_min, dt_min.
fn builtin_sgp4_propagate_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m0 = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((m0 + n * dt).rem_euclid(2.0 * std::f64::consts::PI)))
}

/// Airy disk radius for first dark ring: 1.22 λ / D in radians. Args: λ_m, D_m.
fn builtin_airy_disk_radius(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float(1.22 * lambda / d))
}

/// Rayleigh resolution criterion: same formula as Airy. Returns angular
/// resolution in radians.
fn builtin_rayleigh_criterion(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_airy_disk_radius(args)
}

/// Strehl ratio S ≈ exp(-σ²) for RMS wavefront error σ in waves.
fn builtin_strehl_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sigma_waves = f1(args);
    let sigma_rad = 2.0 * std::f64::consts::PI * sigma_waves;
    Ok(StrykeValue::float((-sigma_rad * sigma_rad).exp()))
}

/// AU → km conversion factor (used here once to ensure constant is alive).
fn builtin_au_to_km(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let _ = b55_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(B55_AU_KM))
}
