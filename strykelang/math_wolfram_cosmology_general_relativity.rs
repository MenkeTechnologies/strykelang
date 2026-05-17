// cosmology, general relativity, FLRW universe, black holes.

const C_LIGHT_B31: f64 = 2.99792458e8;
const G_NEWTON_B31: f64 = 6.67430e-11;
const H_PLANCK_B31: f64 = 6.62607015e-34;
const KB_B31: f64 = 1.380649e-23;
const PARSEC_M: f64 = 3.0857e16;
const SOLAR_MASS_KG: f64 = 1.98892e30;

// Hubble parameter H(z) for flat ΛCDM
fn builtin_hubble_lcdm(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = f1(args);
    let h0 = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    let omega_m = args.get(2).map(|v| v.to_number()).unwrap_or(0.315);
    let omega_l = args.get(3).map(|v| v.to_number()).unwrap_or(0.685);
    Ok(StrykeValue::float(h0 * (omega_m * (1.0 + z).powi(3) + omega_l).sqrt()))
}

// Hubble time t_H = 1/H0 in seconds (H0 in km/s/Mpc → SI conversion)
fn builtin_hubble_time(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let h0_kmsmpc = f1(args);
    let h0_si = h0_kmsmpc * 1000.0 / (1e6 * PARSEC_M);
    if h0_si == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(1.0 / h0_si))
}

// Hubble distance D_H = c / H0 in meters
fn builtin_hubble_distance_si(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let h0_kmsmpc = f1(args);
    let h0_si = h0_kmsmpc * 1000.0 / (1e6 * PARSEC_M);
    if h0_si == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(C_LIGHT_B31 / h0_si))
}

// Critical density ρ_c (kg/m³) from H0 in km/s/Mpc
fn builtin_critical_density_si(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let h0_kmsmpc = f1(args);
    let h0_si = h0_kmsmpc * 1000.0 / (1e6 * PARSEC_M);
    let pi = std::f64::consts::PI;
    Ok(StrykeValue::float(3.0 * h0_si * h0_si / (8.0 * pi * G_NEWTON_B31)))
}

// Comoving distance D_C(z) — trapezoidal integration of c/H(z')
fn builtin_comoving_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = f1(args);
    let h0 = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    let omega_m = args.get(2).map(|v| v.to_number()).unwrap_or(0.315);
    let omega_l = args.get(3).map(|v| v.to_number()).unwrap_or(0.685);
    let steps = 1000;
    let dz = z / steps as f64;
    let mut sum = 0.0;
    for i in 0..steps {
        let z_a = i as f64 * dz;
        let z_b = (i + 1) as f64 * dz;
        let h_a = h0 * (omega_m * (1.0 + z_a).powi(3) + omega_l).sqrt();
        let h_b = h0 * (omega_m * (1.0 + z_b).powi(3) + omega_l).sqrt();
        sum += 0.5 * dz * (1.0 / h_a + 1.0 / h_b);
    }
    let c_kms = 299792.458;
    Ok(StrykeValue::float(c_kms * sum))
}

// Luminosity distance D_L = (1+z) * D_C

// Angular diameter distance D_A = D_C / (1+z)
fn builtin_angular_diameter_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = f1(args);
    let dc = builtin_comoving_distance(args)?.to_number();
    Ok(StrykeValue::float(dc / (1.0 + z)))
}

// Lookback time t_L(z) — integral of 1/((1+z')H(z'))
fn builtin_lookback_time(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = f1(args);
    let h0 = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    let omega_m = args.get(2).map(|v| v.to_number()).unwrap_or(0.315);
    let omega_l = args.get(3).map(|v| v.to_number()).unwrap_or(0.685);
    let steps = 1000;
    let dz = z / steps as f64;
    let mut sum = 0.0;
    for i in 0..steps {
        let z_mid = (i as f64 + 0.5) * dz;
        let h = h0 * (omega_m * (1.0 + z_mid).powi(3) + omega_l).sqrt();
        sum += dz / ((1.0 + z_mid) * h);
    }
    let h0_si = h0 * 1000.0 / (1e6 * PARSEC_M);
    Ok(StrykeValue::float(sum / h0_si * h0))
}

// Age of universe at redshift z
fn builtin_age_at_z(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = f1(args);
    let h0 = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    let omega_m = args.get(2).map(|v| v.to_number()).unwrap_or(0.315);
    let omega_l = args.get(3).map(|v| v.to_number()).unwrap_or(0.685);
    let steps = 5000;
    let z_max = 100.0;
    let mut sum = 0.0;
    for i in 0..steps {
        let z_mid = z + (z_max - z) * (i as f64 + 0.5) / steps as f64;
        let h = h0 * (omega_m * (1.0 + z_mid).powi(3) + omega_l).sqrt();
        sum += (z_max - z) / steps as f64 / ((1.0 + z_mid) * h);
    }
    let h0_si = h0 * 1000.0 / (1e6 * PARSEC_M);
    Ok(StrykeValue::float(sum / h0_si * h0))
}

// Cosmic scale factor a(t) = 1/(1+z)
fn builtin_scale_factor(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = f1(args);
    Ok(StrykeValue::float(1.0 / (1.0 + z)))
}

// Redshift from scale factor z = 1/a - 1
fn builtin_redshift_from_a(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = f1(args);
    if a == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(1.0 / a - 1.0))
}

// Density parameter Ω(z) = Ω_0 (1+z)^3 / E(z)^2
fn builtin_omega_m_at_z(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = f1(args);
    let omega_m_0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.315);
    let omega_l_0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.685);
    let e_sq = omega_m_0 * (1.0 + z).powi(3) + omega_l_0;
    if e_sq == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(omega_m_0 * (1.0 + z).powi(3) / e_sq))
}

// Equation-of-state parameter w for ΛCDM (constant -1)
fn builtin_lcdm_eos() -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::float(-1.0))
}

// w(z) for CPL parametrization w(z) = w0 + wa·z/(1+z)
fn builtin_cpl_w(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = f1(args);
    let w0 = args.get(1).map(|v| v.to_number()).unwrap_or(-1.0);
    let wa = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(w0 + wa * z / (1.0 + z)))
}

// Friedmann II: deceleration q = ½ Ω_m(z) - Ω_Λ(z)
fn builtin_deceleration_q(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = f1(args);
    let omega_m_0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.315);
    let omega_l_0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.685);
    let e_sq = omega_m_0 * (1.0 + z).powi(3) + omega_l_0;
    let omega_m_z = omega_m_0 * (1.0 + z).powi(3) / e_sq;
    let omega_l_z = omega_l_0 / e_sq;
    Ok(StrykeValue::float(0.5 * omega_m_z - omega_l_z))
}

// Schwarzschild radius (alternate signature)
fn builtin_schwarzschild_radius_kg(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    Ok(StrykeValue::float(2.0 * G_NEWTON_B31 * m_kg / (C_LIGHT_B31 * C_LIGHT_B31)))
}

// Kerr ergosphere radius (equatorial, prograde)
fn builtin_kerr_ergosphere_eq(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let a_param = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let rs = 2.0 * G_NEWTON_B31 * m_kg / (C_LIGHT_B31 * C_LIGHT_B31);
    Ok(StrykeValue::float(rs / 2.0 + (rs * rs / 4.0 - a_param * a_param).max(0.0).sqrt()))
}

// Kerr horizon radius r+ = M + sqrt(M^2 - a^2)
fn builtin_kerr_horizon(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let a_param = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m_geom = G_NEWTON_B31 * m_kg / (C_LIGHT_B31 * C_LIGHT_B31);
    Ok(StrykeValue::float(m_geom + (m_geom * m_geom - a_param * a_param).max(0.0).sqrt()))
}

// Hawking temperature T = ℏc³/(8πGMk_B)

// Black hole entropy S = k_B A / (4 ℓ_P^2)
fn builtin_bh_entropy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let pi = std::f64::consts::PI;
    let rs = 2.0 * G_NEWTON_B31 * m_kg / (C_LIGHT_B31 * C_LIGHT_B31);
    let area = 4.0 * pi * rs * rs;
    let hbar = H_PLANCK_B31 / (2.0 * pi);
    let l_p_sq = G_NEWTON_B31 * hbar / C_LIGHT_B31.powi(3);
    Ok(StrykeValue::float(KB_B31 * area / (4.0 * l_p_sq)))
}

// Black hole evaporation timescale τ ≈ 5120πG²M³/(ℏc⁴)
fn builtin_bh_evaporation_time(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let pi = std::f64::consts::PI;
    let hbar = H_PLANCK_B31 / (2.0 * pi);
    Ok(StrykeValue::float(5120.0 * pi * G_NEWTON_B31.powi(2) * m_kg.powi(3)
        / (hbar * C_LIGHT_B31.powi(4))))
}

// Innermost stable circular orbit (ISCO, Schwarzschild) r = 6M
fn builtin_schwarzschild_isco(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let rs = 2.0 * G_NEWTON_B31 * m_kg / (C_LIGHT_B31 * C_LIGHT_B31);
    Ok(StrykeValue::float(3.0 * rs))
}

// Photon sphere r = 1.5 r_s for Schwarzschild
fn builtin_photon_sphere_radius(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let rs = 2.0 * G_NEWTON_B31 * m_kg / (C_LIGHT_B31 * C_LIGHT_B31);
    Ok(StrykeValue::float(1.5 * rs))
}

// Tidal force at distance r from mass M (at radial separation Δr)
fn builtin_tidal_force(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let dr = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let m_test = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(2.0 * G_NEWTON_B31 * m_kg * m_test * dr / r.powi(3)))
}

// Gravitational time dilation factor at radius r outside mass M
fn builtin_grav_dilation_factor(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let factor = 1.0 - 2.0 * G_NEWTON_B31 * m_kg / (r * C_LIGHT_B31 * C_LIGHT_B31);
    if factor <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(factor.sqrt()))
}

// Frame-dragging angular velocity Lense-Thirring at r outside Kerr
fn builtin_lense_thirring_omega(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let a_param = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(2.0 * G_NEWTON_B31 * m_kg * a_param / (C_LIGHT_B31 * r.powi(3))))
}

// Gravitational wave strain h ≈ G(M c²) / (r c⁴) (rough order-of-magnitude)
fn builtin_gw_strain_amplitude(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_chirp_kg = f1(args);
    let f_gw = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let dist_m = args.get(2).map(|v| v.to_number()).unwrap_or(1e22);
    if dist_m == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    let pi = std::f64::consts::PI;
    let gm_c3 = G_NEWTON_B31 * m_chirp_kg / C_LIGHT_B31.powi(3);
    Ok(StrykeValue::float(4.0 * (gm_c3 * pi * f_gw).powf(2.0 / 3.0) * gm_c3 * C_LIGHT_B31 / dist_m))
}

// Chirp mass (m1, m2 in kg) M_c = (m1 m2)^(3/5) / (m1+m2)^(1/5)
fn builtin_chirp_mass(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m1 = f1(args);
    let m2 = args.get(1).map(|v| v.to_number()).unwrap_or(m1);
    if m1 + m2 == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((m1 * m2).powf(3.0 / 5.0) / (m1 + m2).powf(1.0 / 5.0)))
}

// Gravitational binding energy of uniform sphere U = -3GM²/(5R)
fn builtin_grav_binding_energy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(StrykeValue::float(f64::NEG_INFINITY)); }
    Ok(StrykeValue::float(-3.0 * G_NEWTON_B31 * m * m / (5.0 * r)))
}

// Roche limit (rigid body, dense satellite) d ≈ R_p (2 ρ_p / ρ_s)^(1/3)
fn builtin_roche_limit_rigid(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r_planet = f1(args);
    let rho_planet = args.get(1).map(|v| v.to_number()).unwrap_or(5500.0);
    let rho_sat = args.get(2).map(|v| v.to_number()).unwrap_or(2500.0).max(1e-12);
    Ok(StrykeValue::float(r_planet * (2.0 * rho_planet / rho_sat).powf(1.0 / 3.0)))
}

// Roche limit fluid d ≈ 2.44 R_p (ρ_p / ρ_s)^(1/3)
fn builtin_roche_limit_fluid(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r_planet = f1(args);
    let rho_planet = args.get(1).map(|v| v.to_number()).unwrap_or(5500.0);
    let rho_sat = args.get(2).map(|v| v.to_number()).unwrap_or(2500.0).max(1e-12);
    Ok(StrykeValue::float(2.44 * r_planet * (rho_planet / rho_sat).powf(1.0 / 3.0)))
}

// Hill radius (spherical) r_H ≈ a (m / 3 M)^(1/3)

// Lagrangian L1 distance from secondary (Hill approximation)
fn builtin_lagrange_l1(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_hill_radius(args)
}

// Sphere of influence (Laplace) r ≈ a (m/M)^(2/5)
fn builtin_sphere_of_influence(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a_orbit = f1(args);
    let m_secondary = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let m_primary = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if m_primary <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(a_orbit * (m_secondary / m_primary).powf(2.0 / 5.0)))
}

// Synodic period 1/T = |1/T1 - 1/T2|

// Schwarzschild radial velocity for falling object from rest at infinity
fn builtin_freefall_velocity_schwarzschild(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float((2.0 * G_NEWTON_B31 * m_kg / r).max(0.0).sqrt()))
}

// Einstein ring radius (point lens at distance D_l, source at D_s)
fn builtin_einstein_ring_radius(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let d_ls = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let d_l = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let d_s = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if d_l * d_s == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((4.0 * G_NEWTON_B31 * m_kg * d_ls / (C_LIGHT_B31.powi(2) * d_l * d_s)).max(0.0).sqrt()))
}

// Microlensing magnification for u = β/θ_E
fn builtin_microlensing_magnification(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let u = f1(args);
    if u == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float((u * u + 2.0) / (u * (u * u + 4.0).sqrt())))
}

// Cosmic distance modulus DM = 5 log10(D_L/10 pc) — D_L in meters
fn builtin_cosmic_distance_modulus_si(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let d_l_m = f1(args);
    if d_l_m <= 0.0 { return Ok(StrykeValue::float(f64::NEG_INFINITY)); }
    let d_l_pc = d_l_m / PARSEC_M;
    Ok(StrykeValue::float(5.0 * (d_l_pc / 10.0).log10()))
}

// CMB temperature today (default 2.725 K)
fn builtin_cmb_temperature() -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::float(2.725))
}

// CMB temperature at redshift z
fn builtin_cmb_temperature_at_z(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = f1(args);
    Ok(StrykeValue::float(2.725 * (1.0 + z)))
}

// Wien displacement law λ_max T = 2.898e-3 m·K

// Stefan-Boltzmann power per area σT⁴
fn builtin_stefan_boltzmann_si(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let t = f1(args);
    let sigma = 5.670374419e-8;
    Ok(StrykeValue::float(sigma * t.powi(4)))
}

// Planck spectral radiance B(λ,T)
fn builtin_planck_spectral_radiance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lambda = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(5800.0);
    if lambda <= 0.0 || t <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    let exp_arg = H_PLANCK_B31 * C_LIGHT_B31 / (lambda * KB_B31 * t);
    if exp_arg > 700.0 { return Ok(StrykeValue::float(0.0)); }
    let denom = exp_arg.exp() - 1.0;
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(2.0 * H_PLANCK_B31 * C_LIGHT_B31.powi(2) / lambda.powi(5) / denom))
}

// Schwarzschild metric coefficient g_tt = -(1 - 2M/r)
fn builtin_schwarzschild_g_tt(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(StrykeValue::float(f64::NEG_INFINITY)); }
    Ok(StrykeValue::float(-(1.0 - 2.0 * G_NEWTON_B31 * m_kg / (r * C_LIGHT_B31 * C_LIGHT_B31))))
}

// Schwarzschild g_rr = 1/(1-2M/r)
fn builtin_schwarzschild_g_rr(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let factor = 1.0 - 2.0 * G_NEWTON_B31 * m_kg / (r * C_LIGHT_B31 * C_LIGHT_B31);
    if factor == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(1.0 / factor))
}

// Riemann invariant Kretschmann scalar for Schwarzschild K = 48 M²/r⁶
fn builtin_kretschmann_schwarzschild(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let m_geom = G_NEWTON_B31 * m_kg / (C_LIGHT_B31 * C_LIGHT_B31);
    if r == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(48.0 * m_geom * m_geom / r.powi(6)))
}

// Newtonian Hill velocity at Hill radius (orbital velocity around secondary)
fn builtin_hill_velocity(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_secondary = f1(args);
    let r_hill = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r_hill == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float((G_NEWTON_B31 * m_secondary / r_hill).max(0.0).sqrt()))
}

// Vacuum energy density ρ_vac = Λ c²/(8πG)
fn builtin_vacuum_energy_density(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lambda_cosm = f1(args);
    let pi = std::f64::consts::PI;
    Ok(StrykeValue::float(lambda_cosm * C_LIGHT_B31 * C_LIGHT_B31 / (8.0 * pi * G_NEWTON_B31)))
}

// Sound horizon at recombination (rough fit)
fn builtin_sound_horizon_recomb(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let omega_b = f1(args);
    let omega_m = args.get(1).map(|v| v.to_number()).unwrap_or(0.315);
    let h0 = args.get(2).map(|v| v.to_number()).unwrap_or(70.0);
    let h_dimless = h0 / 100.0;
    Ok(StrykeValue::float(44.5 * (omega_m * h_dimless * h_dimless).powf(-0.25)
        * (omega_b * h_dimless * h_dimless / 0.0125_f64).powf(-0.125_f64)))
}

// BAO scale today (rough fit ~150 Mpc)
fn builtin_bao_scale_today() -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::float(150.0))
}

// Sigma8 power spectrum normalization (default value)
fn builtin_sigma8_default() -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::float(0.811))
}

// Gravitational lensing convergence κ = Σ / Σ_crit (dimensionless)
fn builtin_lensing_convergence(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let sigma = f1(args);
    let sigma_crit = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if sigma_crit == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(sigma / sigma_crit))
}

// Critical surface density Σ_crit
fn builtin_sigma_crit(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let d_s = f1(args);
    let d_l = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let d_ls = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let pi = std::f64::consts::PI;
    if d_l * d_ls == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(C_LIGHT_B31 * C_LIGHT_B31 * d_s / (4.0 * pi * G_NEWTON_B31 * d_l * d_ls)))
}

// Geodesic precession (Schwarzschild) Δφ per orbit
fn builtin_perihelion_precession(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let a_orbit = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let e = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let pi = std::f64::consts::PI;
    let denom = a_orbit * (1.0 - e * e) * C_LIGHT_B31 * C_LIGHT_B31;
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(6.0 * pi * G_NEWTON_B31 * m_kg / denom))
}

// Shapiro delay (one-way) for ray passing close to mass M with impact b
fn builtin_shapiro_delay(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let r1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let r2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if b <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(2.0 * G_NEWTON_B31 * m_kg / C_LIGHT_B31.powi(3)
        * (4.0 * r1 * r2 / (b * b)).max(1.0).ln()))
}

// Light deflection angle by point mass α = 4GM/(bc²)
fn builtin_light_deflection_angle(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if b == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(4.0 * G_NEWTON_B31 * m_kg / (b * C_LIGHT_B31 * C_LIGHT_B31)))
}

// Chandrasekhar mass limit (electron-degenerate)

// Tolman-Oppenheimer-Volkoff (TOV) limit (neutron star)
fn builtin_tov_mass_limit() -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::float(2.16 * SOLAR_MASS_KG))
}

// Eddington luminosity L_Edd = 4πGMm_p c / σ_T

// Stellar main-sequence lifetime (Sun-relative scaling) τ ≈ 10 Gyr (M/M_sun)^-2.5
fn builtin_main_sequence_lifetime(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_solar = f1(args);
    if m_solar <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(1e10 * m_solar.powf(-2.5)))
}

// Schwarzschild T(r) free-fall coordinate time integral (rough)
fn builtin_schwarzschild_freefall_time(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m_kg = f1(args);
    let r0 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if m_kg <= 0.0 || r0 <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    let pi = std::f64::consts::PI;
    Ok(StrykeValue::float(pi / (2.0_f64).sqrt() * r0.powf(1.5) / (G_NEWTON_B31 * m_kg).sqrt()))
}

// Friedmann eq: ρ_total at z given Ω's
fn builtin_friedmann_density_total(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = f1(args);
    let h0 = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    let omega_m = args.get(2).map(|v| v.to_number()).unwrap_or(0.315);
    let omega_l = args.get(3).map(|v| v.to_number()).unwrap_or(0.685);
    let h0_si = h0 * 1000.0 / (1e6 * PARSEC_M);
    let h_z = h0_si * (omega_m * (1.0 + z).powi(3) + omega_l).sqrt();
    let pi = std::f64::consts::PI;
    Ok(StrykeValue::float(3.0 * h_z * h_z / (8.0 * pi * G_NEWTON_B31)))
}

// Cosmological constant from Ω_Λ and H0
fn builtin_cosmological_constant(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let omega_l = f1(args);
    let h0_kmsmpc = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    let h0_si = h0_kmsmpc * 1000.0 / (1e6 * PARSEC_M);
    Ok(StrykeValue::float(3.0 * omega_l * h0_si * h0_si / (C_LIGHT_B31 * C_LIGHT_B31)))
}

// Planck length
fn builtin_planck_energy() -> StrykeResult<StrykeValue> {
    let pi = std::f64::consts::PI;
    let hbar = H_PLANCK_B31 / (2.0 * pi);
    let m_p = (hbar * C_LIGHT_B31 / G_NEWTON_B31).sqrt();
    Ok(StrykeValue::float(m_p * C_LIGHT_B31 * C_LIGHT_B31))
}
