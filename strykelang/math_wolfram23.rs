// Batch 23 — electromagnetism, optics, special relativity, waves, plasma.

const C_LIGHT: f64 = 2.99792458e8;
const EPS_0: f64 = 8.854187817e-12;
const MU_0: f64 = 1.25663706212e-6;
const E_CHARGE: f64 = 1.602176634e-19;

// Coulomb force
fn builtin_coulomb_force_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q1 = f1(args);
    let q2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    let k = 1.0 / (4.0 * std::f64::consts::PI * EPS_0);
    Ok(PerlValue::float(k * q1 * q2 / (r * r)))
}
// Electric field magnitude E = kq/r²
fn builtin_efield_point(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    let k = 1.0 / (4.0 * std::f64::consts::PI * EPS_0);
    Ok(PerlValue::float(k * q / (r * r)))
}
// Electric potential V = kq/r
fn builtin_epotential_point(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    let k = 1.0 / (4.0 * std::f64::consts::PI * EPS_0);
    Ok(PerlValue::float(k * q / r))
}
// Capacitance parallel plate C = ε₀εrA/d
fn builtin_capacitance_parallel_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let area = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let er = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if d == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(EPS_0 * er * area / d))
}
// Energy stored in capacitor U = ½CV²
fn builtin_capacitor_energy_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * c * v * v))
}
// Capacitor charge Q=CV
fn builtin_capacitor_charge(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(c * v))
}

// Ohm's law V = IR
fn builtin_ohm_voltage(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(i * r))
}
// Power = VI
fn builtin_power_vi(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(v * i))
}
// Power dissipation = I²R
fn builtin_power_i2r(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(i * i * r))
}
// Resistance series sum
fn builtin_resistance_series_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rs = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let sum: f64 = rs.iter().map(|v| v.to_number()).sum();
    Ok(PerlValue::float(sum))
}
// Resistance parallel
fn builtin_resistance_parallel_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rs = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let sum: f64 = rs.iter().filter_map(|v| {
        let r = v.to_number();
        if r == 0.0 { None } else { Some(1.0 / r) }
    }).sum();
    if sum == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / sum))
}
// Capacitance series
fn builtin_capacitance_series_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cs = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let sum: f64 = cs.iter().filter_map(|v| {
        let c = v.to_number();
        if c == 0.0 { None } else { Some(1.0 / c) }
    }).sum();
    if sum == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / sum))
}
// Capacitance parallel sum
fn builtin_capacitance_parallel_sum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cs = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let sum: f64 = cs.iter().map(|v| v.to_number()).sum();
    Ok(PerlValue::float(sum))
}

// Magnetic field straight wire B = μ₀I/(2πr)
fn builtin_bfield_wire(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(MU_0 * i / (2.0 * std::f64::consts::PI * r)))
}
// Solenoid B = μ₀nI
fn builtin_bfield_solenoid(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(MU_0 * n * i))
}
// Lorentz force F = qE + qv×B (magnitude only, perpendicular case)
fn builtin_lorentz_force_mag(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(q * v * b))
}
// Cyclotron frequency f = qB/(2πm)
fn builtin_cyclotron_frequency_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let m = args.get(2).map(|v| v.to_number()).unwrap_or(9.10938356e-31);
    if m == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(q.abs() * b / (2.0 * std::f64::consts::PI * m)))
}
// Larmor radius r = mv/(qB)
fn builtin_larmor_radius_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if q * b == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(m * v / (q.abs() * b)))
}
// Faraday induced EMF ε = -dΦ/dt (just magnitude)
fn builtin_faraday_emf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let dphi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if dt == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(-n * dphi / dt))
}
// Inductor energy U = ½LI²
fn builtin_inductor_energy_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = f1(args);
    let i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * l * i * i))
}
// LC frequency f = 1/(2π√(LC))
fn builtin_lc_frequency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if l * c <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / (2.0 * std::f64::consts::PI * (l * c).sqrt())))
}
// LC angular frequency ω = 1/√(LC)
fn builtin_lc_omega(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if l * c <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / (l * c).sqrt()))
}
// RC time constant τ = RC
fn builtin_rc_tau(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(r * c))
}
// RL time constant τ = L/R
fn builtin_rl_tau(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(l / r))
}

// Poynting vector magnitude (free space): S = E²/(μ₀c)
fn builtin_poynting_magnitude(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e = f1(args);
    Ok(PerlValue::float(e * e / (MU_0 * C_LIGHT)))
}
// Intensity from amplitude in vacuum: I = ½ε₀cE₀²
fn builtin_em_intensity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e0 = f1(args);
    Ok(PerlValue::float(0.5 * EPS_0 * C_LIGHT * e0 * e0))
}
// Radiation pressure p = I/c
fn builtin_radiation_pressure(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = f1(args);
    Ok(PerlValue::float(i / C_LIGHT))
}
// EM wavelength λ = c/f
fn builtin_em_wavelength(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args);
    if f == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(C_LIGHT / f))
}
// EM frequency f = c/λ
fn builtin_em_frequency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    if lambda == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(C_LIGHT / lambda))
}

// Snell's law n1·sinθ1 = n2·sinθ2 — return θ2
fn builtin_snell_theta2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n1 = f1(args);
    let theta1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n2 = args.get(2).map(|v| v.to_number()).unwrap_or(n1);
    if n2 == 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    let arg = n1 * theta1.sin() / n2;
    if !(-1.0..=1.0).contains(&arg) { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float(arg.asin()))
}
// Critical angle θc = asin(n2/n1)
fn builtin_critical_angle_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n1 = f1(args);
    let n2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n1 == 0.0 || n2 / n1 > 1.0 { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float((n2 / n1).asin()))
}
// Brewster angle θB = atan(n2/n1)
fn builtin_brewster_angle_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n1 = f1(args);
    let n2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n1 == 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float((n2 / n1).atan()))
}
// Refractive index from speeds: n = c/v
fn builtin_index_from_speed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    if v == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(C_LIGHT / v))
}
// Fresnel reflection (s-polarization, normal incidence): R = ((n1-n2)/(n1+n2))²
fn builtin_fresnel_reflection_normal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n1 = f1(args);
    let n2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n1 + n2 == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(((n1 - n2) / (n1 + n2)).powi(2)))
}
// Fresnel s reflection coefficient (amplitude)
fn builtin_fresnel_rs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n1 = f1(args);
    let n2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let theta_i = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let arg = n1 / n2 * theta_i.sin();
    if !(-1.0..=1.0).contains(&arg) { return Ok(PerlValue::float(1.0)); }
    let theta_t = arg.asin();
    let denom = n1 * theta_i.cos() + n2 * theta_t.cos();
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((n1 * theta_i.cos() - n2 * theta_t.cos()) / denom))
}
// Fresnel p reflection
fn builtin_fresnel_rp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n1 = f1(args);
    let n2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let theta_i = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let arg = n1 / n2 * theta_i.sin();
    if !(-1.0..=1.0).contains(&arg) { return Ok(PerlValue::float(1.0)); }
    let theta_t = arg.asin();
    let denom = n2 * theta_i.cos() + n1 * theta_t.cos();
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((n2 * theta_i.cos() - n1 * theta_t.cos()) / denom))
}
// Lensmaker's equation 1/f = (n-1)(1/R1 - 1/R2)
fn builtin_lensmaker(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let r1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if r1 == 0.0 || r2 == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    let inv_f = (n - 1.0) * (1.0 / r1 - 1.0 / r2);
    if inv_f == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / inv_f))
}
// Thin lens equation 1/f = 1/u + 1/v
fn builtin_thin_lens_v(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args);
    let u = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if f == 0.0 || u == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    let inv_v = 1.0 / f - 1.0 / u;
    if inv_v == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / inv_v))
}
// Mirror equation (same as thin lens with sign conventions)
fn builtin_mirror_equation_v(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_thin_lens_v(args)
}
// Magnification m = -v/u
fn builtin_lens_magnification(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let u = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if u == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(-v / u))
}
// Diffraction grating sin θm = mλ/d
fn builtin_diffraction_grating_angle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if d == 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    let arg = m * lambda / d;
    if !(-1.0..=1.0).contains(&arg) { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float(arg.asin()))
}
// Single slit min: a sin θ = mλ
fn builtin_single_slit_min(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if a == 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    let arg = m * lambda / a;
    if !(-1.0..=1.0).contains(&arg) { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float(arg.asin()))
}
// Rayleigh resolution criterion θ = 1.22λ/D
fn builtin_rayleigh_resolution(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if d == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.22 * lambda / d))
}

// Special relativity Lorentz factor γ = 1/√(1 - v²/c²)
fn builtin_lorentz_gamma(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let beta_sq = (v / C_LIGHT).powi(2);
    if beta_sq >= 1.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / (1.0 - beta_sq).sqrt()))
}
// Time dilation Δt' = γΔt
fn builtin_time_dilation_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dt0 = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta_sq = (v / C_LIGHT).powi(2);
    if beta_sq >= 1.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(dt0 / (1.0 - beta_sq).sqrt()))
}
// Length contraction L = L0/γ
fn builtin_length_contraction_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l0 = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta_sq = (v / C_LIGHT).powi(2);
    if beta_sq >= 1.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(l0 * (1.0 - beta_sq).sqrt()))
}
// Relativistic momentum p = γmv
fn builtin_rel_momentum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta_sq = (v / C_LIGHT).powi(2);
    if beta_sq >= 1.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    let gamma = 1.0 / (1.0 - beta_sq).sqrt();
    Ok(PerlValue::float(gamma * m * v))
}
// Relativistic kinetic energy: KE = (γ-1)mc²
fn builtin_rel_ke(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta_sq = (v / C_LIGHT).powi(2);
    if beta_sq >= 1.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    let gamma = 1.0 / (1.0 - beta_sq).sqrt();
    Ok(PerlValue::float((gamma - 1.0) * m * C_LIGHT * C_LIGHT))
}
// Relativistic total energy E = γmc²
fn builtin_rel_total_energy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta_sq = (v / C_LIGHT).powi(2);
    if beta_sq >= 1.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    let gamma = 1.0 / (1.0 - beta_sq).sqrt();
    Ok(PerlValue::float(gamma * m * C_LIGHT * C_LIGHT))
}
// E² = (pc)² + (mc²)² — return E given p, m
fn builtin_rel_energy_pm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(((p * C_LIGHT).powi(2) + (m * C_LIGHT * C_LIGHT).powi(2)).sqrt()))
}
// Relativistic Doppler (longitudinal): f' = f √((1-β)/(1+β))
fn builtin_relativistic_doppler(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = v / C_LIGHT;
    if (1.0 + beta).abs() < 1e-30 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(f * ((1.0 - beta) / (1.0 + beta)).max(0.0).sqrt()))
}
// Relativistic velocity addition u' = (u-v)/(1 - uv/c²)
fn builtin_rel_velocity_add(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = 1.0 - u * v / (C_LIGHT * C_LIGHT);
    if denom == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float((u + v) / denom))
}
// Compton wavelength shift Δλ = (h/(m_e c))(1 - cos θ)
fn builtin_compton_shift_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    let lambda_c = 2.4263102367e-12;
    Ok(PerlValue::float(lambda_c * (1.0 - theta.cos())))
}
// Photon momentum p = h/λ
fn builtin_photon_momentum_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    if lambda == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(6.62607015e-34 / lambda))
}

// Wave on string speed v = √(T/μ)
fn builtin_wave_string_speed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if mu <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((t / mu).sqrt()))
}
// Sound speed in solid: v = √(Y/ρ)
fn builtin_sound_solid(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let rho = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if rho <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((y / rho).sqrt()))
}
// Sound speed in gas v = √(γRT/M)
fn builtin_sound_gas(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let gamma = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(298.15);
    let m = args.get(2).map(|v| v.to_number()).unwrap_or(0.029);
    let r = 8.31446261815324;
    if m <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((gamma * r * t / m).sqrt()))
}
// Doppler effect (classical sound)
fn builtin_doppler_classical(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args);
    let v_obs = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v_src = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let v_sound = args.get(3).map(|v| v.to_number()).unwrap_or(343.0);
    if v_sound - v_src == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(f * (v_sound + v_obs) / (v_sound - v_src)))
}
// Standing wave fundamental f1 = v/(2L)
fn builtin_standing_wave_fundamental(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if l == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(v / (2.0 * l)))
}
// Open pipe harmonic n: f_n = nv/(2L)
fn builtin_open_pipe_harmonic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(343.0);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if l == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(n * v / (2.0 * l)))
}
// Closed pipe harmonic odd n: f_n = nv/(4L)
fn builtin_closed_pipe_harmonic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(343.0);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if l == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(n * v / (4.0 * l)))
}
// dB sound level β = 10 log10(I/I0)
fn builtin_sound_db(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = f1(args).max(1e-30);
    let i0 = args.get(1).map(|v| v.to_number()).unwrap_or(1e-12);
    if i0 <= 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float(10.0 * (i / i0).log10()))
}

// Plasma frequency ωp = √(ne²/(ε₀m))
fn builtin_plasma_frequency_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(9.10938356e-31);
    if m <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float((n * E_CHARGE * E_CHARGE / (EPS_0 * m)).max(0.0).sqrt()))
}
// Debye length λD = √(ε₀kT/(n e²))
fn builtin_debye_length_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(300.0);
    let kb = 1.380649e-23;
    if n <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float((EPS_0 * kb * t / (n * E_CHARGE * E_CHARGE)).max(0.0).sqrt()))
}
// Alfvén speed vA = B/√(μ₀ρ)
fn builtin_alfven_speed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let b = f1(args);
    let rho = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if rho <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(b / (MU_0 * rho).sqrt()))
}
// Schwarzschild radius rs = 2GM/c²
fn builtin_schwarzschild_radius_b23(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let g = 6.674e-11;
    Ok(PerlValue::float(2.0 * g * m / (C_LIGHT * C_LIGHT)))
}
// Gravitational time dilation factor √(1 - 2GM/(rc²))
fn builtin_grav_time_dilation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let g = 6.674e-11;
    let factor = 1.0 - 2.0 * g * m / (r * C_LIGHT * C_LIGHT);
    if factor <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(factor.sqrt()))
}
// Gravitational redshift z = (1/√(1-2GM/rc²)) - 1
fn builtin_grav_redshift(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let g = 6.674e-11;
    let factor = 1.0 - 2.0 * g * m / (r * C_LIGHT * C_LIGHT);
    if factor <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / factor.sqrt() - 1.0))
}
