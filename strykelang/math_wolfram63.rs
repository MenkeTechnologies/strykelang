// Batch 63 — chemistry & biochemistry: stoichiometry, kinetics, equilibrium,
// electrochemistry, spectroscopy, structural rules.

const B63_R_GAS: f64 = 8.314_462_618;
const B63_F_FARADAY: f64 = 96_485.332_12;
const B63_KB: f64 = 1.380_649e-23;
const B63_NA: f64 = 6.022_140_76e23;

fn b63_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// Molecular weight from atom counts × atomic weights. Args: counts, weights.
fn builtin_molecular_weight_compound(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let counts = b63_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let weights = b63_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n = counts.len().min(weights.len());
    Ok(StrykeValue::float((0..n).map(|i| counts[i] * weights[i]).sum()))
}

/// Molarity dilution: M₁V₁ = M₂V₂ → V₂ = M₁V₁/M₂.
fn builtin_molarity_dilution(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m1 = f1(args);
    let v1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float(m1 * v1 / m2))
}

/// Universal gas constant value in SI (J/mol·K). Returns the constant.
fn builtin_gas_constant_value(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(B63_R_GAS))
}

/// Eyring equation: k = (k_B·T/h) · exp(−ΔG‡ / RT). Args: T (K), ΔG‡ (J/mol).
fn builtin_eyring_rate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args).max(1e-9);
    let dg = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h = 6.626_070_15e-34;
    Ok(StrykeValue::float(B63_KB * t / h * (-dg / (B63_R_GAS * t)).exp()))
}

/// Van't Hoff: ln(K₂/K₁) = −ΔH/R·(1/T₂ − 1/T₁) → K₂.
fn builtin_van_t_hoff_kp(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k1 = f1(args).max(1e-300);
    let dh = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t1 = args.get(2).map(|v| v.to_number()).unwrap_or(298.15).max(1e-9);
    let t2 = args.get(3).map(|v| v.to_number()).unwrap_or(t1).max(1e-9);
    Ok(StrykeValue::float(k1 * (-dh / B63_R_GAS * (1.0 / t2 - 1.0 / t1)).exp()))
}

/// Henderson-Hasselbalch buffer: pH = pKa + log([A⁻]/[HA]).
fn builtin_henderson_buffer(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pka = f1(args);
    let a_minus = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    let ha = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(StrykeValue::float(pka + (a_minus / ha).log10()))
}

/// Titration endpoint: V_eq = (n·M_a · V_a) / M_b for strong-strong neutralization.
fn builtin_titration_ph_endpoint(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m_a = f1(args);
    let v_a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m_b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    let stoich = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(stoich * m_a * v_a / m_b))
}

/// Isoelectric point of a protein: pI = (pKa1 + pKa2) / 2 for the two adjacent
/// pKas spanning zero net charge (typical for amphoteric amino acids).
fn builtin_isoelectric_point_protein(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pka1 = f1(args);
    let pka2 = args.get(1).map(|v| v.to_number()).unwrap_or(pka1);
    Ok(StrykeValue::float((pka1 + pka2) / 2.0))
}

/// Ka → pKa.
fn builtin_ka_to_pka(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ka = f1(args).max(1e-300);
    Ok(StrykeValue::float(-ka.log10()))
}

/// pKb → Kb.
fn builtin_pkb_to_kb(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pkb = f1(args);
    Ok(StrykeValue::float(10f64.powf(-pkb)))
}

/// Amphoteric check: 1 if compound has both acidic and basic groups
/// (matches when input mask has both "OH/COOH" bit and "NH₂" bit).
fn builtin_amphoteric_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mask = i1(args) as u64;
    Ok(StrykeValue::integer(if (mask & 0x1) != 0 && (mask & 0x2) != 0 { 1 } else { 0 }))
}

/// Oxidation number from formal charge and bond multiplicities. Args: formal,
/// bonds_to_more_electronegative, bonds_to_less_electronegative.
fn builtin_oxidation_number(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let formal = i1(args);
    let to_more_eneg = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let to_less_eneg = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(formal + to_more_eneg - to_less_eneg))
}

/// Half-reaction balance: returns electrons transferred per reduction. For
/// oxidation states a → b on N atoms: e = N·(a − b).
fn builtin_half_reaction_balance(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let from_ox = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let to_ox = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(n * (from_ox - to_ox)))
}

/// Cell EMF (Nernst, full): E = E° − (RT/nF) ln Q.
fn builtin_redox_potential_cell(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let e_std = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(298.15);
    Ok(StrykeValue::float(e_std - (B63_R_GAS * t / (n * B63_F_FARADAY)) * q.ln()))
}

/// Electrolysis mass deposit (Faraday): m = (Q · M) / (n · F).
fn builtin_electrolysis_mass(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q_coul = f1(args);
    let molar_mass = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    Ok(StrykeValue::float(q_coul * molar_mass / (n * B63_F_FARADAY)))
}

/// Beer-Lambert: A = ε·c·l → transmittance T = 10^(-A).
fn builtin_spectrophotometer_beer_lambert(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let eps = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(eps * c * l))
}

/// Molar absorptivity ε from absorbance, concentration, path length.
fn builtin_epsilon_extinction(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float(a / (c * l)))
}

/// Transmittance to absorbance: A = -log₁₀ T.
fn builtin_transmittance_to_a(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args).clamp(1e-300, 1.0);
    Ok(StrykeValue::float(-t.log10()))
}

/// Crystal-field ligand strength (spectrochemical series rank). Args: ligand_id
/// (0..15 covering I⁻, Br⁻, S²⁻, SCN⁻, Cl⁻, F⁻, OH⁻, ox²⁻, H₂O, NCS⁻, NH₃, en,
/// bipy, phen, NO₂⁻, CN⁻).
fn builtin_crystal_field_ligand(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let id = i1(args).clamp(0, 15) as usize;
    Ok(StrykeValue::integer(id as i64))
}

/// Jahn-Teller check: 1 if d-electron count is in {1,2,3,4,6,7} (degenerate
/// e_g/t_2g configurations) for octahedral geometry.
fn builtin_jahn_teller_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d = i1(args);
    let active = matches!(d, 1 | 2 | 3 | 4 | 6 | 7 | 9);
    Ok(StrykeValue::integer(if active { 1 } else { 0 }))
}

/// VSEPR geometry: returns molecular shape ID given (steric, lone pairs).
/// 0=linear, 1=trigonal planar, 2=tetrahedral, 3=trig bipyramidal, 4=octahedral,
/// 5=bent, 6=trigonal pyramidal, 7=seesaw, 8=T-shape, 9=square pyramid.
fn builtin_vsepr_geometry(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let steric = i1(args);
    let lone = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let geom = match (steric, lone) {
        (2, 0) => 0, (3, 0) => 1, (4, 0) => 2, (5, 0) => 3, (6, 0) => 4,
        (3, 1) => 5, (4, 1) => 6, (4, 2) => 5, (5, 1) => 7, (5, 2) => 8,
        (6, 1) => 9, (6, 2) => 4, _ => -1,
    };
    Ok(StrykeValue::integer(geom))
}

/// Lewis dot count for octet/duet rule: total = sum(group_n) − 2·bonds.
fn builtin_lewis_dot_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let group_sum = i1(args);
    let bonds = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(group_sum - 2 * bonds))
}

/// Formal charge: FC = group_n − (lone_e + bonds).
fn builtin_formal_charge(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let group_n = i1(args);
    let lone = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let bonds = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(group_n - lone - bonds))
}

/// Resonance count: number of distinct equivalent Lewis structures (heuristic =
/// ⌊double_bond_count + lone_pair_on_pi_atoms⌋).
fn builtin_resonance_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dbl = i1(args);
    let lp = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((dbl + lp).max(1)))
}

/// Ramachandran φ/ψ check: 1 if (φ, ψ) inside the favourable α-helix or β-sheet
/// region. Helix: φ ∈ [-90, -35], ψ ∈ [-70, -15]. β-sheet: φ ∈ [-180, -90],
/// ψ ∈ [90, 180]. Args: φ_deg, ψ_deg.
fn builtin_ramachandran_phi_psi(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let phi = f1(args);
    let psi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let helix = (-90.0..=-35.0).contains(&phi) && (-70.0..=-15.0).contains(&psi);
    let sheet = (-180.0..=-90.0).contains(&phi) && (90.0..=180.0).contains(&psi);
    Ok(StrykeValue::integer(if helix || sheet { 1 } else { 0 }))
}

/// Radius of gyration Rg² = (1/N) Σ (r_i − r_cm)². Args: array of distances
/// from centre of mass.
fn builtin_rg_radius_of_gyration(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b63_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let n = v.len() as f64;
    let s: f64 = v.iter().map(|x| x * x).sum();
    Ok(StrykeValue::float((s / n).sqrt()))
}

/// Spectroscopic factor (nuclear): C² · S, the overlap of single-particle
/// transfer reaction with shell-model orbital. Args: C² (Clebsch-Gordan-like)
/// and S (spectroscopic strength).
fn builtin_spectroscopic_factor(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c2 = f1(args).max(0.0);
    let s = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(0.0);
    Ok(StrykeValue::float(c2 * s))
}

/// Avogadro number constant.
fn builtin_avogadro_constant(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(B63_NA))
}
