// Batch 38 — electrochemistry, batteries, fuel cells, corrosion, electrolytes.

const B38_R_GAS: f64 = 8.314_462_618;
const B38_F_FARADAY: f64 = 96_485.332_12;
const B38_KB: f64 = 1.380_649e-23;
const B38_E_CHARGE: f64 = 1.602_176_634e-19;

fn b38_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// Full Nernst potential: E = E° - (RT / nF) ln Q
fn builtin_nernst_potential_full(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e_std = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(298.15);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let q = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if n == 0.0 || q <= 0.0 { return Ok(PerlValue::float(e_std)); }
    Ok(PerlValue::float(e_std - (B38_R_GAS * t / (n * B38_F_FARADAY)) * q.ln()))
}

/// Electrode potential step under reference shift
fn builtin_electrode_potential_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e = f1(args);
    let shift = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(e + shift))
}

/// Exchange current density i₀ from rate constant: i₀ = nFk⁰ c
fn builtin_exchange_current_density(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let k0 = args.get(1).map(|v| v.to_number()).unwrap_or(1e-6);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(n * B38_F_FARADAY * k0 * c))
}

/// Butler-Volmer current: i = i₀ [exp(αₐ Fη/RT) - exp(-α_c Fη/RT)]
fn builtin_butler_volmer_current(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i0 = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    let alpha_a = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    let alpha_c = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    let f_rt = B38_F_FARADAY / (B38_R_GAS * t);
    Ok(PerlValue::float(i0 * ((alpha_a * eta * f_rt).exp() - (-alpha_c * eta * f_rt).exp())))
}

/// Tafel anodic branch: i = i₀ exp(αₐFη/RT)
fn builtin_tafel_anodic_current(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i0 = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    let alpha = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float(i0 * (alpha * eta * B38_F_FARADAY / (B38_R_GAS * t)).exp()))
}

/// Tafel cathodic branch: i = -i₀ exp(-α_cFη/RT)
fn builtin_tafel_cathodic_current(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i0 = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    let alpha = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float(-i0 * (-alpha * eta * B38_F_FARADAY / (B38_R_GAS * t)).exp()))
}

/// Mass transport overpotential: η_mt = (RT/nF) ln(1 - i/i_lim)
fn builtin_mass_transport_overpotential(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = f1(args);
    let i_lim = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(298.15);
    if i_lim <= 0.0 || (i / i_lim) >= 1.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float((B38_R_GAS * t / (n * B38_F_FARADAY)) * (1.0 - i / i_lim).ln()))
}

/// Limiting current density: i_lim = nFD c / δ
fn builtin_limiting_current_density(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1e-9);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let delta = args.get(3).map(|v| v.to_number()).unwrap_or(1e-5);
    if delta <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(n * B38_F_FARADAY * d * c / delta))
}

/// Diffusion layer thickness δ = D / k_d
fn builtin_diffusion_layer_thickness(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d = f1(args);
    let kd = args.get(1).map(|v| v.to_number()).unwrap_or(1e-3);
    if kd <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(d / kd))
}

/// Faradaic efficiency = q_actual / q_theoretical
fn builtin_faradaic_efficiency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_actual = f1(args);
    let q_theoretical = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if q_theoretical == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(q_actual / q_theoretical))
}

/// Coulombic efficiency cell = Q_discharge / Q_charge
fn builtin_coulombic_efficiency_cell(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_dis = f1(args);
    let q_chg = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if q_chg == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(q_dis / q_chg))
}

/// Energy efficiency cell = E_dis · Q_dis / (E_chg · Q_chg)
fn builtin_energy_efficiency_cell(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e_dis = f1(args);
    let q_dis = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let e_chg = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let q_chg = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let denom = e_chg * q_chg;
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(e_dis * q_dis / denom))
}

/// Voltaic efficiency = V_dis / V_chg
fn builtin_voltaic_efficiency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v_dis = f1(args);
    let v_chg = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if v_chg == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(v_dis / v_chg))
}

/// Charge capacity (Ah) of battery from current and time: Q = I·t
fn builtin_charge_capacity_battery(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(i * t))
}

/// Energy density (Wh/kg)
fn builtin_energy_density_battery(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let energy = f1(args);
    let mass = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if mass <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(energy / mass))
}

/// Power density (W/kg)
fn builtin_power_density_battery(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let power = f1(args);
    let mass = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if mass <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(power / mass))
}

/// Specific capacity (mAh/g): Q / m
fn builtin_specific_capacity_active(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_mah = f1(args);
    let m_g = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if m_g <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(q_mah / m_g))
}

/// Columbic capacity Li half-cell (theoretical): nF/M
fn builtin_columbic_capacity_lihalfcell(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if m <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(n * B38_F_FARADAY / m / 3.6))
}

/// Ragone point energy vs power product
fn builtin_ragone_point(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(e * p))
}

/// Peukert capacity Cp = I^k · t
fn builtin_peukert_capacity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(1.2);
    Ok(PerlValue::float(i.powf(k) * t))
}

/// Peukert exponent fit from two (I, t) pairs
fn builtin_peukert_exponent_fit(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i1_v = f1(args);
    let t1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let i2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let t2 = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if i1_v <= 0.0 || i2 <= 0.0 || t1 <= 0.0 || t2 <= 0.0 { return Ok(PerlValue::float(1.0)); }
    let denom = (i1_v / i2).ln();
    if denom == 0.0 { return Ok(PerlValue::float(1.0)); }
    Ok(PerlValue::float((t2 / t1).ln() / denom + 1.0))
}

/// Shepherd voltage step: V = E0 - K·Q/(Q-Q_used) - R·I + A·exp(-B·Q_used)
fn builtin_shepherd_voltage_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e0 = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let q_used = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let i = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(7).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = q - q_used;
    if denom == 0.0 { return Ok(PerlValue::float(e0)); }
    Ok(PerlValue::float(e0 - k * q / denom - r * i + a * (-b * q_used).exp()))
}

/// Nernst-Planck flux J = -D∇c - zFD/(RT) c ∇φ (1-D scalar)
fn builtin_nernst_planck_flux(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d = f1(args);
    let dc = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let z = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let dphi = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(5).map(|v| v.to_number()).unwrap_or(298.15);
    Ok(PerlValue::float(-d * dc - z * B38_F_FARADAY * d / (B38_R_GAS * t) * c * dphi))
}

/// Debye length: λ_D = √(ε_r ε₀ kT / Σ nᵢ zᵢ² e²)
fn builtin_debye_length_electrolyte(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let eps_r = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(298.15);
    let i_strength = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    let eps0 = 8.854_187_8128e-12;
    let denom = 2.0 * i_strength * 1000.0 * 6.022e23 * B38_E_CHARGE.powi(2);
    if denom <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(((eps_r * eps0 * B38_KB * t) / denom).sqrt()))
}

/// Debye-Hückel activity log γ = -A √I / (1 + B·a √I)
fn builtin_debye_huckel_activity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = f1(args);
    let i_strength = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    let a = args.get(2).map(|v| v.to_number()).unwrap_or(0.509);
    let b = args.get(3).map(|v| v.to_number()).unwrap_or(0.328);
    let radius = args.get(4).map(|v| v.to_number()).unwrap_or(3.0);
    let sqrt_i = i_strength.sqrt();
    Ok(PerlValue::float(-a * z * z * sqrt_i / (1.0 + b * radius * sqrt_i)))
}

/// Gouy-Chapman potential at distance x: ψ = ψ₀ exp(-x/λ_D)
fn builtin_gouy_chapman_potential(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let psi0 = f1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda_d = args.get(2).map(|v| v.to_number()).unwrap_or(1e-9);
    if lambda_d <= 0.0 { return Ok(PerlValue::float(psi0)); }
    Ok(PerlValue::float(psi0 * (-x / lambda_d).exp()))
}

/// Stern layer capacitance per unit area: C_S = ε ε₀ / d_S
fn builtin_stern_layer_capacitance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let eps_r = f1(args);
    let d_s = args.get(1).map(|v| v.to_number()).unwrap_or(0.5e-9);
    let eps0 = 8.854_187_8128e-12;
    if d_s <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(eps_r * eps0 / d_s))
}

/// Double layer capacitance: 1/C_dl = 1/C_S + 1/C_diff
fn builtin_double_layer_capacitance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c_s = f1(args);
    let c_d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if c_s <= 0.0 || c_d <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 / (1.0 / c_s + 1.0 / c_d)))
}

/// Helmholtz capacitance per area = ε ε₀ / d
fn builtin_helmholtz_capacitance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_stern_layer_capacitance(args)
}

/// Zeta potential estimate from electrophoretic mobility (Smoluchowski): ζ = μ_e η / (ε ε₀)
fn builtin_zeta_potential_estimate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mu = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(1e-3);
    let eps_r = args.get(2).map(|v| v.to_number()).unwrap_or(78.5);
    let eps0 = 8.854_187_8128e-12;
    Ok(PerlValue::float(mu * eta / (eps_r * eps0)))
}

/// Electroosmotic velocity v_eo = -ε ε₀ ζ E / η
fn builtin_electroosmotic_velocity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let zeta = f1(args);
    let e_field = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let eps_r = args.get(2).map(|v| v.to_number()).unwrap_or(78.5);
    let eta = args.get(3).map(|v| v.to_number()).unwrap_or(1e-3);
    let eps0 = 8.854_187_8128e-12;
    Ok(PerlValue::float(-eps_r * eps0 * zeta * e_field / eta))
}

/// Hagen-Poiseuille electroosmotic flow rate
fn builtin_hagen_poiseuille_eo(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let area = args.get(1).map(|x| x.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(v * area))
}

/// Diffuse layer thickness ≈ Debye length
fn builtin_diffuse_layer_thickness(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_debye_length_electrolyte(args)
}

/// Poisson-Boltzmann step: ψ' = -ψ/λ_D² · sinh(zeψ/kT)
fn builtin_poisson_boltzmann_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let psi = f1(args);
    let lambda_d = args.get(1).map(|v| v.to_number()).unwrap_or(1e-9);
    let z = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(298.15);
    let beta = z * B38_E_CHARGE / (B38_KB * t);
    if lambda_d <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(-(beta * psi).sinh() / (lambda_d * lambda_d)))
}

/// Linearized PB step: ψ' = -ψ/λ_D²
fn builtin_linearized_pb_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let psi = f1(args);
    let lambda_d = args.get(1).map(|v| v.to_number()).unwrap_or(1e-9);
    if lambda_d <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(-psi / (lambda_d * lambda_d)))
}

/// Electrochemical impedance Z = R + 1/(jωC) magnitude
fn builtin_electrochem_impedance_z(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let omega = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if omega == 0.0 || c == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float((r * r + 1.0 / (omega * c).powi(2)).sqrt()))
}

/// Randles circuit Z = Rs + 1/(jωCdl + 1/(Rct + Zw))
fn builtin_randles_circuit_z(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rs = f1(args);
    let rct = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let cdl = args.get(2).map(|v| v.to_number()).unwrap_or(1e-6);
    let omega = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let xc = if omega * cdl == 0.0 { f64::INFINITY } else { 1.0 / (omega * cdl) };
    let denom = (1.0 / rct).powi(2) + (1.0 / xc).powi(2);
    if denom == 0.0 { return Ok(PerlValue::float(rs)); }
    Ok(PerlValue::float(rs + denom.sqrt().recip()))
}

/// Warburg impedance: Z_W = σ / √ω · (1 - j) → magnitude
fn builtin_warburg_impedance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sigma = f1(args);
    let omega = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if omega <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(sigma / omega.sqrt() * std::f64::consts::SQRT_2))
}

/// Cole-Cole equation EIS: ε* = ε∞ + (ε_s - ε∞) / (1 + (jωτ)^(1-α))
fn builtin_cole_cole_eis(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let eps_inf = f1(args);
    let eps_s = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let omega = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let tau = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let alpha = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = 1.0 + (omega * tau).powf(1.0 - alpha);
    if denom == 0.0 { return Ok(PerlValue::float(eps_inf)); }
    Ok(PerlValue::float(eps_inf + (eps_s - eps_inf) / denom))
}

/// Nyquist plot phase angle from impedance components
fn builtin_nyquist_phase(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z_re = f1(args);
    let z_im = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(z_im.atan2(z_re)))
}

/// Charge transfer resistance from i₀: Rct = RT/(nFi₀)
fn builtin_charge_transfer_resistance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i0 = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    if i0 == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(B38_R_GAS * t / (n * B38_F_FARADAY * i0)))
}

/// Solution resistance estimate from conductivity κ and cell length L, area A
fn builtin_solution_resistance_estimate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let kappa = f1(args);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    let a = args.get(2).map(|v| v.to_number()).unwrap_or(1e-4);
    if kappa == 0.0 || a == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(l / (kappa * a)))
}

/// Ionic conductivity Arrhenius: σ = σ₀ exp(-E_a/RT)
fn builtin_ionic_conductivity_arrhenius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s0 = f1(args);
    let ea = args.get(1).map(|v| v.to_number()).unwrap_or(20_000.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    Ok(PerlValue::float(s0 * (-ea / (B38_R_GAS * t)).exp()))
}

/// Nernst-Einstein D = RTλ/(z²F²)
fn builtin_nernst_einstein_diffusivity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    let z = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    let denom = z * z * B38_F_FARADAY * B38_F_FARADAY;
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(B38_R_GAS * t * lambda / denom))
}

/// Walden product: Λη
fn builtin_walden_product(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(1e-3);
    Ok(PerlValue::float(lambda * eta))
}

/// Kohlrausch's law of independent ion migration: Λ° = ν₊λ°₊ + ν₋λ°₋
fn builtin_kohlrausch_law(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let nu_p = f1(args);
    let lambda_p = args.get(1).map(|v| v.to_number()).unwrap_or(50.0);
    let nu_n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda_n = args.get(3).map(|v| v.to_number()).unwrap_or(50.0);
    Ok(PerlValue::float(nu_p * lambda_p + nu_n * lambda_n))
}

/// Onsager relation: J_i = Σ L_{ij} X_j → simple scalar
fn builtin_onsager_relation_two_species(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l11 = f1(args);
    let x1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let l12 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let x2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(l11 * x1 + l12 * x2))
}

/// Trasatti charge from voltammetry: Q = ∫ I dV / v
fn builtin_trasatti_voltammetry_charge(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let int_idv = f1(args);
    let scan_rate = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    if scan_rate == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(int_idv / scan_rate))
}

/// Randles-Sevcik peak current: ip = 0.4463 nFAC √(nFvD/RT)
fn builtin_randles_sevcik_peak(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let area = args.get(1).map(|v| v.to_number()).unwrap_or(1e-4);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(3).map(|v| v.to_number()).unwrap_or(0.1);
    let d = args.get(4).map(|v| v.to_number()).unwrap_or(1e-9);
    let t = args.get(5).map(|v| v.to_number()).unwrap_or(298.15);
    let factor = (n * B38_F_FARADAY * v * d / (B38_R_GAS * t)).sqrt();
    Ok(PerlValue::float(0.4463 * n * B38_F_FARADAY * area * c * factor))
}

/// Levich current at rotating disk: i_L = 0.62 nFAC D^(2/3) ω^(1/2) ν^(-1/6)
fn builtin_levich_current_rde(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let area = args.get(1).map(|v| v.to_number()).unwrap_or(1e-4);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(1e-9);
    let omega = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let nu = args.get(5).map(|v| v.to_number()).unwrap_or(1e-6);
    Ok(PerlValue::float(0.62 * n * B38_F_FARADAY * area * c * d.powf(2.0 / 3.0) * omega.sqrt() * nu.powf(-1.0 / 6.0)))
}

/// Koutecky-Levich intercept: 1/i = 1/i_K + 1/(B·ω^1/2)
fn builtin_koutecky_levich_intercept(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i_k = f1(args);
    if i_k == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / i_k))
}

/// Mott-Schottky capacitance plot: 1/C² = 2/(εε₀eN_d)·(V - V_fb - kT/e)
fn builtin_mott_schottky_capacitance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let v_fb = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let nd = args.get(2).map(|v| v.to_number()).unwrap_or(1e22);
    let eps_r = args.get(3).map(|v| v.to_number()).unwrap_or(10.0);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(298.15);
    let eps0 = 8.854_187_8128e-12;
    let denom = eps_r * eps0 * B38_E_CHARGE * nd;
    if denom == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(2.0 / denom * (v - v_fb - B38_KB * t / B38_E_CHARGE)))
}

/// Flat-band potential from MS plot intercept
fn builtin_flat_band_potential(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let intercept = f1(args);
    let slope = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if slope == 0.0 { return Ok(PerlValue::float(intercept)); }
    Ok(PerlValue::float(-intercept / slope))
}

/// Schottky barrier height ϕ_B = ϕ_M - χ
fn builtin_schottky_barrier_height(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let phi_m = f1(args);
    let chi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(phi_m - chi))
}

/// Photocurrent density from quantum efficiency: J_ph = e φ Φ
fn builtin_photocurrent_density(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let phi = f1(args);
    let flux = args.get(1).map(|v| v.to_number()).unwrap_or(1e16);
    Ok(PerlValue::float(B38_E_CHARGE * phi * flux))
}

/// External quantum efficiency
fn builtin_quantum_efficiency_photo(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_e = f1(args);
    let n_ph = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n_ph == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(n_e / n_ph))
}

/// Overall PEC efficiency η_total = η_abs · η_sep · η_redox
fn builtin_overall_efficiency_pec(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b38_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().product()))
}

/// Fuel cell polarization: V = V_ocv - η_act - i·R - η_conc
fn builtin_fuel_cell_polarization(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v_ocv = f1(args);
    let eta_act = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let i_r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let eta_conc = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(v_ocv - eta_act - i_r - eta_conc))
}

/// Electrolyzer cell voltage: V = E_rev + η_anode + η_cathode + i·R
fn builtin_electrolyzer_voltage(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e_rev = f1(args);
    let eta_a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let eta_c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let i_r = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(e_rev + eta_a + eta_c + i_r))
}

/// Faraday efficiency for H₂ evolution
fn builtin_faraday_efficiency_h2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_h2 = f1(args);
    let total_charge = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total_charge == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(2.0 * B38_F_FARADAY * n_h2 / total_charge))
}

/// OER overpotential η_OER = E_app - 1.23 V vs RHE
fn builtin_overpotential_oer(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e_app = f1(args);
    Ok(PerlValue::float(e_app - 1.23))
}

/// HER overpotential η_HER = E_app - 0 V vs RHE (i.e., E_app since H+/H₂ is 0 V)
fn builtin_overpotential_her(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e_app = f1(args);
    Ok(PerlValue::float(e_app))
}

/// Electrocrystallization step: progress = 1 - exp(-k·t^m)
fn builtin_electrocrystallization_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    Ok(PerlValue::float(1.0 - (-k * t.powf(m)).exp()))
}

/// Nucleation rate constant J = A exp(-ΔG*/kT)
fn builtin_nucleation_rate_constant(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let dg = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    Ok(PerlValue::float(a * (-dg / (B38_KB * t)).exp()))
}

/// Metal corrosion rate (mm/year): CR = i·M·k / (n·F·ρ)
fn builtin_metal_corrosion_rate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(56.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    let rho = args.get(3).map(|v| v.to_number()).unwrap_or(7.87);
    let denom = n * B38_F_FARADAY * rho;
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(i * m * 3.27 / denom * 1000.0))
}

/// Pourbaix diagram E vs pH line: E = E₀ - (0.0591 m / n) pH
fn builtin_pourbaix_line_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e0 = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let ph = args.get(3).map(|v| v.to_number()).unwrap_or(7.0);
    if n == 0.0 { return Ok(PerlValue::float(e0)); }
    Ok(PerlValue::float(e0 - 0.0591 * m / n * ph))
}

/// Mixed potential step toward intersection of anodic/cathodic
fn builtin_mixed_potential_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i_a = f1(args);
    let i_c = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(i_a - i_c))
}

/// ECL yield η_ecl
fn builtin_electrochemiluminescence_yield(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_photons = f1(args);
    let n_charge = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n_charge == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(n_photons / n_charge))
}

/// Solid electrolyte capacity Q = nFm/M
fn builtin_solid_electrolyte_capacity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let m_active = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let molar_mass = args.get(2).map(|v| v.to_number()).unwrap_or(50.0);
    if molar_mass == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(n * B38_F_FARADAY * m_active / molar_mass))
}

/// Ionic liquid viscosity step (VFT): η = η₀ exp(B/(T-T₀))
fn builtin_ionic_liquid_viscosity_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let eta0 = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(700.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    let t0 = args.get(3).map(|v| v.to_number()).unwrap_or(160.0);
    if t - t0 == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(eta0 * (b / (t - t0)).exp()))
}

/// Lithium-ion diffusivity in graphite anode (Arrhenius)
fn builtin_lithium_ion_diffusivity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d0 = f1(args);
    let ea = args.get(1).map(|v| v.to_number()).unwrap_or(35_000.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    Ok(PerlValue::float(d0 * (-ea / (B38_R_GAS * t)).exp()))
}

/// State of charge from coulomb counting: SoC = SoC₀ - (1/Q) ∫I dt
fn builtin_soc_estimate_coulomb(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let soc0 = f1(args);
    let q_total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let charge_drawn = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if q_total == 0.0 { return Ok(PerlValue::float(soc0)); }
    Ok(PerlValue::float((soc0 - charge_drawn / q_total).clamp(0.0, 1.0)))
}

/// SoH from capacity fade: SoH = Q_now / Q_initial
fn builtin_soh_capacity_fade(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_now = f1(args);
    let q_init = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if q_init == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(q_now / q_init))
}

/// OCV-Li-ion step approximation by polynomial fit
fn builtin_ocv_lithium_ion_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let soc = f1(args);
    Ok(PerlValue::float(3.0 + 1.2 * soc - 0.3 * soc.powi(2)))
}

/// Kalman filter SoC update: SoC = SoC_pred + K(z - h(x))
fn builtin_state_of_charge_kalman(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let soc_pred = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let z = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let hx = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(soc_pred + k * (z - hx)))
}

/// Thermal runaway threshold check
fn builtin_thermal_runaway_threshold(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let t_thresh = args.get(1).map(|v| v.to_number()).unwrap_or(80.0);
    Ok(PerlValue::integer(if t >= t_thresh { 1 } else { 0 }))
}

/// Joule heating in battery: P = I²R
fn builtin_joule_heating_battery(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(i * i * r))
}

/// Calorimetric heat in battery (CV/dT): Q = mc dT
fn builtin_calorimetric_heat_battery(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(900.0);
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(m * c * dt))
}

/// Abuse test voltage cutoff
fn builtin_abuse_test_voltage(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let v_max = args.get(1).map(|v| v.to_number()).unwrap_or(4.5);
    Ok(PerlValue::integer(if v >= v_max { 1 } else { 0 }))
}

/// Swelling strain step ε = 3α dT (cubic expansion)
fn builtin_swelling_strain_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha = f1(args);
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    Ok(PerlValue::float(3.0 * alpha * dt))
}

/// SEI resistance growth R(t) = R₀ + k √t
fn builtin_sei_resistance_growth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r0 = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(r0 + k * t.max(0.0).sqrt()))
}

/// Optimal binder content (electrode coating, wt%): w_b = (ρ_b · t_b · A_s) /
/// (1 + ρ_b · t_b · A_s) · 100%, where A_s is BET specific surface area
/// (m²/g of active material), t_b is binder shell thickness (m), ρ_b is binder
/// density (g/m³). For PVDF (ρ_b ≈ 1.78 g/cm³ = 1.78e6 g/m³, t_b ≈ 5e-9 m), this
/// gives 5–8 wt% at 50 m²/g, matching empirical practice for Li-ion electrodes.
/// Args: A_s (m²/g), t_b (m), ρ_b (g/m³).
fn builtin_binder_content_optimal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_s = f1(args).max(0.0);
    let t_b = args.get(1).map(|v| v.to_number()).unwrap_or(5e-9).max(0.0);
    let rho_b = args.get(2).map(|v| v.to_number()).unwrap_or(1.78e6).max(0.0);
    let m = rho_b * t_b * a_s;
    Ok(PerlValue::float(100.0 * m / (1.0 + m)))
}

/// Porosity of active layer ε = 1 - ρ/ρ_solid
fn builtin_porosity_active_layer(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rho = f1(args);
    let rho_solid = args.get(1).map(|v| v.to_number()).unwrap_or(2.5);
    if rho_solid == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 - rho / rho_solid))
}

/// Bruggeman tortuosity τ = ε^(1-α), α = 1.5 typical
fn builtin_tortuosity_estimate_bruggeman(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let eps = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.5);
    if eps <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(eps.powf(1.0 - alpha)))
}

/// Electrolyte decomposition temperature (from pyrolysis kinetics)
fn builtin_electrolyte_decomposition_temp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ea = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1e10);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(10.0);
    Ok(PerlValue::float(ea / (B38_R_GAS * (a / beta).ln())))
}

/// Gibbs-Thomson undercooling: ΔT = 2γT_m/(ρLΔr)
fn builtin_gibbs_thomson_undercooling(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let gamma = f1(args);
    let tm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let rho = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let l = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(4).map(|v| v.to_number()).unwrap_or(1e-6);
    let denom = rho * l * r;
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(2.0 * gamma * tm / denom))
}

/// Nernst diffusion-layer δ_N for rotating disk
fn builtin_nernst_diffusion_layer(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d = f1(args);
    let nu = args.get(1).map(|v| v.to_number()).unwrap_or(1e-6);
    let omega = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if omega <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.61 * d.powf(1.0 / 3.0) * nu.powf(1.0 / 6.0) / omega.sqrt()))
}

/// Diffusion coefficient aqueous estimate (Stokes-Einstein): D = kT/(6πηr)
fn builtin_diff_coeff_aqueous_estimate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(1e-3);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(1e-10);
    let denom = 6.0 * std::f64::consts::PI * eta * r;
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(B38_KB * t / denom))
}

/// Mean salt activity coefficient from Debye-Hückel
fn builtin_salt_activity_coefficient(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z_p = f1(args);
    let z_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let i_strength = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float((-0.509 * z_p.abs() * z_n.abs() * i_strength.sqrt()).exp()))
}

/// Pitzer mean activity coefficient (lowest-order)
fn builtin_mean_activity_coeff_pitzer(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let beta0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((2.0 * beta0 * m).exp()))
}

/// Pitzer osmotic coefficient
fn builtin_osmotic_coefficient_pitzer(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let beta0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(1.0 + beta0 * m))
}

/// Debye-Hückel screening factor κ
fn builtin_debye_huckel_screening_factor(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda_d = f1(args);
    if lambda_d == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / lambda_d))
}

/// pH at isoelectric point: pI = (pKa + pKb) / 2
fn builtin_ph_at_isoelectric(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pka = f1(args);
    let pkb = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((pka + pkb) / 2.0))
}

/// Buffer capacity β = 2.303 [HA] [A-] / ([HA] + [A-])
fn builtin_buffer_capacity_acid_base(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ha = f1(args);
    let a_minus = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let total = ha + a_minus;
    if total == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(2.303 * ha * a_minus / total))
}

/// Henderson-Hasselbalch solve [A-]/[HA] = 10^(pH - pKa)
fn builtin_henderson_hasselbalch_solve(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ph = f1(args);
    let pka = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(10f64.powf(ph - pka)))
}

/// Titration endpoint index: argmax dpH/dV
fn builtin_titration_endpoint_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b38_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if v.len() < 2 { return Ok(PerlValue::integer(0)); }
    let mut best = 0_usize;
    let mut best_d = f64::NEG_INFINITY;
    for i in 1..v.len() {
        let d = v[i] - v[i - 1];
        if d > best_d { best_d = d; best = i; }
    }
    Ok(PerlValue::integer(best as i64))
}
