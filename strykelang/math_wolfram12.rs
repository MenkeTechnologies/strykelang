// Batch 12 — physics/chemistry/biology/astronomy/engineering long tail.

// ── Physics / classical ─────────────────────────────────────────────────────

fn builtin_kinetic_energy_b12(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args); let v = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * m * v * v))
}
fn builtin_relativistic_kinetic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args); let v = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let c = 2.997_924_58e8_f64;
    let g = 1.0 / (1.0 - (v / c).powi(2)).sqrt();
    Ok(PerlValue::float((g - 1.0) * m * c * c))
}
fn builtin_lorentz_factor_v(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args); let c = 2.997_924_58e8_f64;
    Ok(PerlValue::float(1.0 / (1.0 - (v / c).powi(2)).sqrt()))
}
fn builtin_doppler_relativistic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args); let v = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let c = 2.997_924_58e8_f64;
    Ok(PerlValue::float(f * ((1.0 + v / c) / (1.0 - v / c)).sqrt()))
}
fn builtin_drag_force_quadratic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cd = f1(args); let rho = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let a = args.get(2).map(|x| x.to_number()).unwrap_or(0.0); let v = args.get(3).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * cd * rho * a * v * v))
}
fn builtin_terminal_velocity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args); let g = args.get(1).map(|x| x.to_number()).unwrap_or(9.81);
    let cd = args.get(2).map(|x| x.to_number()).unwrap_or(1.0);
    let rho = args.get(3).map(|x| x.to_number()).unwrap_or(1.225);
    let a = args.get(4).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float((2.0 * m * g / (cd * rho * a)).sqrt()))
}
fn builtin_carnot_efficiency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let tc = f1(args); let th = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(1.0 - tc / th))
}
fn builtin_otto_efficiency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args); let g = args.get(1).map(|x| x.to_number()).unwrap_or(1.4);
    Ok(PerlValue::float(1.0 - 1.0 / r.powf(g - 1.0)))
}
fn builtin_brayton_efficiency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args); let g = args.get(1).map(|x| x.to_number()).unwrap_or(1.4);
    Ok(PerlValue::float(1.0 - 1.0 / r.powf((g - 1.0) / g)))
}
fn builtin_diesel_efficiency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args); let cutoff = args.get(1).map(|x| x.to_number()).unwrap_or(2.0);
    let g = args.get(2).map(|x| x.to_number()).unwrap_or(1.4);
    Ok(PerlValue::float(1.0 - (1.0 / r.powf(g - 1.0)) * (cutoff.powf(g) - 1.0) / (g * (cutoff - 1.0))))
}
fn builtin_specific_heat_const_v(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dof = f1(args); let r = 8.314462618_f64;
    Ok(PerlValue::float(0.5 * dof * r))
}
fn builtin_speed_of_sound_ideal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = f1(args); let r = args.get(1).map(|x| x.to_number()).unwrap_or(287.0);
    let t = args.get(2).map(|x| x.to_number()).unwrap_or(298.15);
    Ok(PerlValue::float((g * r * t).sqrt()))
}
fn builtin_kepler_period_au(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_au = f1(args);
    Ok(PerlValue::float(a_au.powf(1.5))) // years for solar mass
}
fn builtin_synodic_period(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p1 = f1(args); let p2 = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    if (p1 - p2).abs() < 1e-30 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float((p1 * p2).abs() / (p1 - p2).abs()))
}
fn builtin_hill_radius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args); let m = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let m_star = args.get(2).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(a * (m / (3.0 * m_star)).powf(1.0 / 3.0)))
}
fn builtin_jeans_length(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cs = f1(args); let g = args.get(1).map(|x| x.to_number()).unwrap_or(6.674e-11);
    let rho = args.get(2).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(cs * (std::f64::consts::PI / (g * rho)).sqrt()))
}
fn builtin_chandrasekhar_mass(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.4))  // solar masses
}
fn builtin_eddington_luminosity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m_kg = f1(args);
    Ok(PerlValue::float(1.26e31 * m_kg / 1.989e30))
}
fn builtin_schwarzschild_radius_m(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args); let g = 6.674e-11_f64; let c = 2.997_924_58e8_f64;
    Ok(PerlValue::float(2.0 * g * m / (c * c)))
}
fn builtin_gravity_at_radius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args); let r = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(6.674e-11 * m / (r * r)))
}
fn builtin_gravitational_pe(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m1 = f1(args); let m2 = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let r = args.get(2).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(-6.674e-11 * m1 * m2 / r))
}
fn builtin_orbital_velocity_circular_b12(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args); let r = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float((6.674e-11 * m / r).sqrt()))
}
fn builtin_freefall_time(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = f1(args); let g = args.get(1).map(|x| x.to_number()).unwrap_or(9.81);
    Ok(PerlValue::float((2.0 * h / g).sqrt()))
}
fn builtin_pendulum_freq(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = f1(args); let g = args.get(1).map(|x| x.to_number()).unwrap_or(9.81);
    Ok(PerlValue::float(1.0 / (2.0 * std::f64::consts::PI) * (g / l).sqrt()))
}
fn builtin_spring_period(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args); let k = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(2.0 * std::f64::consts::PI * (m / k).sqrt()))
}
fn builtin_centripetal_accel(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args); let r = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(v * v / r))
}
fn builtin_lens_focal_length(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let do_ = f1(args); let di = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(1.0 / (1.0 / do_ + 1.0 / di)))
}
fn builtin_magnification_lens_b12(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let do_ = f1(args); let di = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(-di / do_))
}

// ── Chemistry ───────────────────────────────────────────────────────────────

fn builtin_avogadros_number(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(6.022_140_76e23))
}
fn builtin_boltzmann_const(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.380_649e-23))
}
fn builtin_planck_const_h(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(6.626_070_15e-34))
}
fn builtin_gas_constant_r(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(8.314_462_618))
}
fn builtin_faraday_constant_b12(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(96485.332_12))
}
fn builtin_concentration_dilute(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c1 = f1(args); let v1 = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let v2 = args.get(2).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(c1 * v1 / v2))
}
fn builtin_partial_pressure(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mole_frac = f1(args); let total_p = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(mole_frac * total_p))
}
fn builtin_mole_fraction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_i = f1(args); let n_total = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(n_i / n_total))
}
fn builtin_molarity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mol = f1(args); let l = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(mol / l))
}
fn builtin_molality(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mol = f1(args); let kg = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(mol / kg))
}
fn builtin_normality(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_eq = f1(args); let l = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(n_eq / l))
}
fn builtin_ionic_strength(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let concs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let charges: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let s: f64 = concs.iter().zip(charges.iter()).map(|(c, z)| c * z * z).sum();
    Ok(PerlValue::float(0.5 * s))
}
fn builtin_buffer_capacity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c = f1(args); let pka = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let ph = args.get(2).map(|x| x.to_number()).unwrap_or(7.0);
    let r = 10.0_f64.powf(ph - pka);
    Ok(PerlValue::float(2.303 * c * r / (r + 1.0).powi(2)))
}
fn builtin_titration_volume(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let na = f1(args); let va = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let nb = args.get(2).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(na * va / nb))
}
fn builtin_atomic_radius_pm(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(53.0))  // Bohr radius pm
}
fn builtin_de_broglie_wavelength_kg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args); let v = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(6.626070e-34 / (m * v)))
}

// ── Biology / population ────────────────────────────────────────────────────

fn builtin_logistic_growth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n0 = f1(args); let r = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let k = args.get(2).map(|x| x.to_number()).unwrap_or(1.0);
    let t = args.get(3).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(k / (1.0 + ((k - n0) / n0) * (-r * t).exp())))
}
fn builtin_lotka_volterra_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prey = f1(args); let pred = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let alpha = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    let beta = args.get(3).map(|x| x.to_number()).unwrap_or(0.0);
    let delta = args.get(4).map(|x| x.to_number()).unwrap_or(0.0);
    let gamma = args.get(5).map(|x| x.to_number()).unwrap_or(0.0);
    let dt = args.get(6).map(|x| x.to_number()).unwrap_or(0.01);
    let d_prey = alpha * prey - beta * prey * pred;
    let d_pred = delta * prey * pred - gamma * pred;
    Ok(PerlValue::array(vec![PerlValue::float(prey + dt * d_prey), PerlValue::float(pred + dt * d_pred)]))
}
fn builtin_michaelis_menten(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v_max = f1(args); let s = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let km = args.get(2).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(v_max * s / (km + s)))
}
fn builtin_hill_equation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v_max = f1(args); let s = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let k = args.get(2).map(|x| x.to_number()).unwrap_or(1.0).max(1e-30);
    let n = args.get(3).map(|x| x.to_number()).unwrap_or(1.0);
    let s_n = s.powf(n);
    Ok(PerlValue::float(v_max * s_n / (k.powf(n) + s_n)))
}
fn builtin_lineweaver_burk(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let inv_v = f1(args); let inv_s = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(inv_v + inv_s))
}
fn builtin_eadie_hofstee_y(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v_max = f1(args); let v_s = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(v_max - v_s))
}
fn builtin_arrhenius_temp_q10(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r1 = f1(args); let q10 = args.get(1).map(|x| x.to_number()).unwrap_or(2.0);
    let dt = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(r1 * q10.powf(dt / 10.0)))
}
fn builtin_body_surface_area_dubois(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h_cm = f1(args); let w_kg = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.007184 * h_cm.powf(0.725) * w_kg.powf(0.425)))
}
fn builtin_bmr_harris_benedict_male(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w_kg = f1(args); let h_cm = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let age = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(88.362 + 13.397 * w_kg + 4.799 * h_cm - 5.677 * age))
}
fn builtin_bmr_harris_benedict_female(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w_kg = f1(args); let h_cm = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let age = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(447.593 + 9.247 * w_kg + 3.098 * h_cm - 4.330 * age))
}
fn builtin_max_heart_rate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let age = f1(args);
    Ok(PerlValue::float(220.0 - age))
}
fn builtin_target_heart_rate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let age = f1(args); let resting = args.get(1).map(|x| x.to_number()).unwrap_or(60.0);
    let intensity = args.get(2).map(|x| x.to_number()).unwrap_or(0.7);
    let max_hr = 220.0 - age;
    Ok(PerlValue::float(resting + (max_hr - resting) * intensity))
}
fn builtin_vo2_max_estimate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let age = f1(args); let resting = args.get(1).map(|x| x.to_number()).unwrap_or(60.0);
    Ok(PerlValue::float(15.3 * (220.0 - age) / resting))
}
fn builtin_pulse_pressure(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sys = f1(args); let dia = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(sys - dia))
}
fn builtin_mean_arterial_pressure(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sys = f1(args); let dia = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(dia + (sys - dia) / 3.0))
}

// ── Geophysics / atmosphere ─────────────────────────────────────────────────

fn builtin_dew_point_magnus(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args); let rh = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let a = 17.62_f64; let b = 243.12_f64;
    let alpha = (a * t / (b + t)) + (rh / 100.0).ln();
    Ok(PerlValue::float(b * alpha / (a - alpha)))
}
fn builtin_heat_index_celsius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t_c = f1(args); let rh = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let t_f = t_c * 9.0 / 5.0 + 32.0;
    let hi = -42.379 + 2.04901523 * t_f + 10.14333127 * rh - 0.22475541 * t_f * rh
           - 0.00683783 * t_f * t_f - 0.05481717 * rh * rh
           + 0.00122874 * t_f * t_f * rh + 0.00085282 * t_f * rh * rh
           - 0.00000199 * t_f * t_f * rh * rh;
    Ok(PerlValue::float((hi - 32.0) * 5.0 / 9.0))
}
fn builtin_wind_chill_celsius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t_c = f1(args); let v_kmh = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(13.12 + 0.6215 * t_c - 11.37 * v_kmh.powf(0.16) + 0.3965 * t_c * v_kmh.powf(0.16)))
}
fn builtin_pressure_altitude_m(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p_hpa = f1(args);
    Ok(PerlValue::float(44330.0 * (1.0 - (p_hpa / 1013.25).powf(0.1903))))
}
fn builtin_density_altitude_m(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pa = f1(args); let temp_c = args.get(1).map(|x| x.to_number()).unwrap_or(15.0);
    Ok(PerlValue::float(pa + 120.0 * (temp_c - 15.0)))
}
fn builtin_saturation_vapor_pressure(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t_c = f1(args);
    Ok(PerlValue::float(6.112 * (17.67 * t_c / (t_c + 243.5)).exp()))
}
fn builtin_humidex(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t_c = f1(args); let dew_c = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let e = 6.11 * (5417.7530 * (1.0 / 273.16 - 1.0 / (273.15 + dew_c))).exp();
    Ok(PerlValue::float(t_c + 0.5555 * (e - 10.0)))
}
fn builtin_universal_thermal_climate_index_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args); let v = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let rh = args.get(2).map(|x| x.to_number()).unwrap_or(50.0);
    Ok(PerlValue::float(t - 0.7 * v + 0.05 * (rh - 50.0)))
}

// ── Engineering ─────────────────────────────────────────────────────────────

fn builtin_resistance_parallel(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let s: f64 = rs.iter().filter(|&&r| r.abs() > 1e-30).map(|r| 1.0 / r).sum();
    Ok(PerlValue::float(if s.abs() < 1e-30 { f64::INFINITY } else { 1.0 / s }))
}
fn builtin_resistance_series(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    Ok(PerlValue::float(rs.iter().sum()))
}
fn builtin_capacitance_parallel(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    Ok(PerlValue::float(cs.iter().sum()))
}
fn builtin_capacitance_series(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let s: f64 = cs.iter().filter(|&&c| c.abs() > 1e-30).map(|c| 1.0 / c).sum();
    Ok(PerlValue::float(if s.abs() < 1e-30 { f64::INFINITY } else { 1.0 / s }))
}
fn builtin_inductance_parallel(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_resistance_parallel(args)
}
fn builtin_inductance_series(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_resistance_series(args)
}
fn builtin_voltage_divider(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vin = f1(args); let r1 = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let r2 = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(vin * r2 / (r1 + r2)))
}
fn builtin_current_divider(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i_in = f1(args); let r1 = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let r2 = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(i_in * r2 / (r1 + r2)))
}
fn builtin_lc_resonant(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = f1(args); let c = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(1.0 / (2.0 * std::f64::consts::PI * (l * c).sqrt())))
}
fn builtin_q_factor_rlc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = f1(args); let c = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let r = args.get(2).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float((l / c).sqrt() / r))
}
fn builtin_skin_depth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args); let mu = args.get(1).map(|x| x.to_number()).unwrap_or(1.256e-6);
    let sigma = args.get(2).map(|x| x.to_number()).unwrap_or(5.96e7).max(1e-30);
    Ok(PerlValue::float((1.0 / (std::f64::consts::PI * f * mu * sigma)).sqrt()))
}
fn builtin_wire_resistance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rho = f1(args); let l = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let a = args.get(2).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(rho * l / a))
}
fn builtin_motor_torque(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args); let omega = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(p / omega))
}
fn builtin_efficiency_ratio(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p_out = f1(args); let p_in = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(p_out / p_in))
}
fn builtin_dB_voltage(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v_out = f1(args); let v_in = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(20.0 * (v_out / v_in).log10()))
}
fn builtin_dB_power(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p_out = f1(args); let p_in = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(PerlValue::float(10.0 * (p_out / p_in).log10()))
}
