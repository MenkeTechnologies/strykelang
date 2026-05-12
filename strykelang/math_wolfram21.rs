// Batch 21 — chemistry: kinetics, equilibrium, gas laws, electrochem, thermo.

const R_GAS: f64 = 8.31446261815324; // J/(mol·K)
const F_FARADAY: f64 = 96485.33212;  // C/mol
const N_AVOGADRO: f64 = 6.02214076e23;

// pH from H+ concentration
fn builtin_ph_from_h(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = f1(args).max(1e-30);
    Ok(StrykeValue::float(-h.log10()))
}
// pOH from OH-
fn builtin_poh_from_oh(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let oh = f1(args).max(1e-30);
    Ok(StrykeValue::float(-oh.log10()))
}
// pKa
fn builtin_pka_from_ka(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ka = f1(args).max(1e-30);
    Ok(StrykeValue::float(-ka.log10()))
}
// Henderson-Hasselbalch: pH = pKa + log([A-]/[HA])
// Henderson base form (pOH from pKb)
fn builtin_henderson_base(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pkb = f1(args);
    let bh_plus = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if b <= 0.0 || bh_plus <= 0.0 { return Ok(StrykeValue::float(pkb)); }
    Ok(StrykeValue::float(pkb + (bh_plus / b).log10()))
}

// Arrhenius rate constant k = A * exp(-Ea/RT)
fn builtin_arrhenius_k(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let ea = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    Ok(StrykeValue::float(a * (-ea / (R_GAS * t)).exp()))
}
// Eyring equation k = (k_B*T/h) * exp(-ΔG‡/RT)
fn builtin_eyring_k(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dg = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(298.15);
    let kb = 1.380649e-23;
    let h = 6.62607015e-34;
    Ok(StrykeValue::float(kb * t / h * (-dg / (R_GAS * t)).exp()))
}

// First order rate: ln([A]/[A0]) = -kt
fn builtin_first_order_concentration(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a0 = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(a0 * (-k * t).exp()))
}
// First order half-life
fn builtin_first_order_half_life(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = f1(args).max(1e-30);
    Ok(StrykeValue::float(2.0_f64.ln() / k))
}
// Second order: 1/[A] - 1/[A0] = kt
fn builtin_second_order_concentration(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a0 = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let inv = 1.0 / a0 + k * t;
    if inv == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(1.0 / inv))
}
// Second order half-life: 1/(k*A0)
fn builtin_second_order_half_life(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = f1(args);
    let a0 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if k * a0 == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(1.0 / (k * a0)))
}
// Zero order: [A] = [A0] - kt
fn builtin_zero_order_concentration(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a0 = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float((a0 - k * t).max(0.0)))
}

// Michaelis-Menten v = Vmax*[S]/(Km+[S])
// Lineweaver-Burk inverse

// Ideal gas n = PV/RT
fn builtin_ideal_gas_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    Ok(StrykeValue::float(p * v / (R_GAS * t)))
}
// Van der Waals (P+a*n^2/V^2)(V-nb) = nRT — return predicted pressure
// Redlich-Kwong P
fn builtin_redlich_kwong_p(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    let a = args.get(3).map(|v| v.to_number()).unwrap_or(0.1);
    let b = args.get(4).map(|v| v.to_number()).unwrap_or(0.001);
    if v - n * b <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(n * R_GAS * t / (v - n * b)
        - a / (t.sqrt() * v * (v + n * b))))
}
// Compressibility factor Z = PV/(nRT)
fn builtin_compressibility_z(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(298.15);
    if n * R_GAS * t == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(p * v / (n * R_GAS * t)))
}

// Daltons partial pressure: P_i = x_i * P
// Mole fraction n_i / sum(n)

// Equilibrium Kc from rates
fn builtin_kc_from_rates(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kf = f1(args);
    let kr = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if kr == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(kf / kr))
}
// Kp from Kc: Kp = Kc * (RT)^Δn
fn builtin_kp_from_kc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kc = f1(args);
    let dn = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    Ok(StrykeValue::float(kc * (R_GAS * t).powf(dn)))
}
// Reaction quotient Q (same form as Kc, calculated from current concs)
fn builtin_reaction_quotient(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prods = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let reacts = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let prod_nu = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let react_nu = arg_to_vec(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF));
    let mut num = 1.0;
    for (i, c) in prods.iter().enumerate() {
        let nu = prod_nu.get(i).map(|v| v.to_number()).unwrap_or(1.0);
        num *= c.to_number().powf(nu);
    }
    let mut den = 1.0;
    for (i, c) in reacts.iter().enumerate() {
        let nu = react_nu.get(i).map(|v| v.to_number()).unwrap_or(1.0);
        den *= c.to_number().powf(nu);
    }
    if den == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(num / den))
}
// Le Chatelier shift direction (+1 forward, -1 reverse, 0 at eq)
fn builtin_le_chatelier_dir(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let dir = if (q - k).abs() < 1e-12 { 0 } else if q < k { 1 } else { -1 };
    Ok(StrykeValue::integer(dir))
}

// Gibbs free energy change: ΔG = ΔH - TΔS
// ΔG° = -RT ln K
fn builtin_dg_from_k(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = f1(args).max(1e-30);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(298.15);
    Ok(StrykeValue::float(-R_GAS * t * k.ln()))
}
// K from ΔG°: K = exp(-ΔG°/RT)
fn builtin_k_from_dg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dg = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(298.15);
    Ok(StrykeValue::float((-dg / (R_GAS * t)).exp()))
}
// Van't Hoff: ln(K2/K1) = -ΔH/R * (1/T2 - 1/T1)
fn builtin_vant_hoff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k1 = f1(args);
    let dh = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t1 = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    let t2 = args.get(3).map(|v| v.to_number()).unwrap_or(310.0);
    Ok(StrykeValue::float(k1 * (-dh / R_GAS * (1.0 / t2 - 1.0 / t1)).exp()))
}
// Clausius-Clapeyron
fn builtin_clausius_clapeyron(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p1 = f1(args);
    let dh_vap = args.get(1).map(|v| v.to_number()).unwrap_or(40000.0);
    let t1 = args.get(2).map(|v| v.to_number()).unwrap_or(373.15);
    let t2 = args.get(3).map(|v| v.to_number()).unwrap_or(298.15);
    Ok(StrykeValue::float(p1 * (-dh_vap / R_GAS * (1.0 / t2 - 1.0 / t1)).exp()))
}
// Antoine equation log10(P) = A - B/(C+T)
fn builtin_antoine_p(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(298.15);
    if c + t == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(10_f64.powf(a - b / (c + t))))
}

// Nernst equation E = E° - (RT/nF) ln Q
// EMF from half-cell potentials
fn builtin_emf_from_half_cells(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cathode = f1(args);
    let anode = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(cathode - anode))
}
// Faraday: m = (Q*M)/(n*F)
fn builtin_faraday_mass_deposited(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if n == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(q * m / (n * F_FARADAY)))
}

// Beer-Lambert law A = ε * c * l
// Transmittance T = 10^(-A)
fn builtin_transmittance(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    Ok(StrykeValue::float(10_f64.powf(-a)))
}
// Solubility product Ksp from concentrations
fn builtin_ksp_from_concs(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cs = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let nus = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let mut prod = 1.0;
    for (i, c) in cs.iter().enumerate() {
        let nu = nus.get(i).map(|v| v.to_number()).unwrap_or(1.0);
        prod *= c.to_number().powf(nu);
    }
    Ok(StrykeValue::float(prod))
}
// Ionic strength I = 0.5 * sum(c_i * z_i^2)
// Debye-Hückel limiting law log γ = -A z^2 sqrt(I)
fn builtin_debye_huckel(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    let ionic = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(0.0);
    let a = args.get(2).map(|v| v.to_number()).unwrap_or(0.509);
    Ok(StrykeValue::float(10_f64.powf(-a * z * z * ionic.sqrt())))
}

// Heat capacity at constant pressure for ideal monatomic Cp = (5/2)R
fn builtin_cp_monatomic_ideal() -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(2.5 * R_GAS))
}
// Cv monatomic Cv = (3/2)R
fn builtin_cv_monatomic_ideal() -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(1.5 * R_GAS))
}
// Heat q = mcΔT
fn builtin_heat_capacity_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(4184.0);
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(m * c * dt))
}
// Calorimeter ΔT
fn builtin_calorimeter_dt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(4184.0);
    if m * c == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(q / (m * c)))
}
// Enthalpy of formation sum
fn builtin_enthalpy_reaction(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let products = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let reactants = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let p_sum: f64 = products.iter().map(|v| v.to_number()).sum();
    let r_sum: f64 = reactants.iter().map(|v| v.to_number()).sum();
    Ok(StrykeValue::float(p_sum - r_sum))
}

// Avogadro: number of particles N = n * NA
fn builtin_avogadro_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    Ok(StrykeValue::float(n * N_AVOGADRO))
}
// Mole from mass and molar mass
fn builtin_moles_from_mass(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let mm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if mm == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(m / mm))
}
// Molarity = moles/volume
// Molality = moles/kg solvent
// Dilution: c1*v1 = c2*v2 — solve for v2
fn builtin_dilution_v2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c1 = f1(args);
    let v1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let c2 = args.get(2).map(|v| v.to_number()).unwrap_or(c1);
    if c2 == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(c1 * v1 / c2))
}

// Raoult's law: P_solution = x_solvent * P°_solvent
fn builtin_raoult_law(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let p_pure = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(x * p_pure))
}
// Boiling point elevation ΔTb = Kb * m
fn builtin_bp_elevation(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kb = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let i = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(i * kb * m))
}
// Freezing point depression ΔTf = -Kf * m
fn builtin_fp_depression(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kf = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let i = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(-i * kf * m))
}
// Osmotic pressure π = MRT
fn builtin_osmotic_pressure(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let molarity = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(298.15);
    let i = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(i * molarity * R_GAS * t))
}

// Rydberg wavelength: 1/λ = R_∞ * Z² * (1/n1² - 1/n2²)
fn builtin_rydberg_lambda(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    let n1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n2 = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    let r_inf = 1.0973731568160e7;
    let inv_lambda = r_inf * z * z * (1.0 / (n1 * n1) - 1.0 / (n2 * n2));
    if inv_lambda == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(1.0 / inv_lambda))
}
// Bohr radius for level n = n²·a₀
fn builtin_bohr_radius_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    Ok(StrykeValue::float(n * n * 5.29177210903e-11))
}
// Bohr energy: E_n = -13.6/n² eV
fn builtin_bohr_energy_ev(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    if n == 0.0 { return Ok(StrykeValue::float(f64::NEG_INFINITY)); }
    Ok(StrykeValue::float(-13.605693122994 / (n * n)))
}
// Photon energy E = hf
fn builtin_photon_energy_freq(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f = f1(args);
    Ok(StrykeValue::float(6.62607015e-34 * f))
}
// Photon wavelength to energy: E = hc/λ
fn builtin_photon_energy_lambda(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    if lambda == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(6.62607015e-34 * 2.998e8 / lambda))
}
// de Broglie wavelength λ = h/p
fn builtin_de_broglie(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    if p == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(6.62607015e-34 / p))
}
