// Batch 32 — quantum mechanics deep: density matrices, channels, entanglement, decoherence.

// Pure state |ψ⟩⟨ψ| from amplitudes
fn builtin_pure_state_density(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let amps: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = amps.len();
    let rows: Vec<PerlValue> = (0..n).map(|i| {
        PerlValue::array((0..n).map(|j| PerlValue::float(amps[i] * amps[j])).collect())
    }).collect();
    Ok(PerlValue::array(rows))
}

// Trace of square matrix

// Purity Tr(ρ²)
fn builtin_purity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rho = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = rho.len();
    let mut sum = 0.0;
    for i in 0..n {
        for j in 0..n.min(rho[i].len()) {
            sum += rho[i][j] * rho[j][i];
        }
    }
    Ok(PerlValue::float(sum))
}

// Von Neumann entropy from eigenvalues

// Linear entropy = 1 - Tr(ρ²)
fn builtin_linear_entropy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let purity = builtin_purity(args)?.to_number();
    Ok(PerlValue::float(1.0 - purity))
}

// Renyi entropy of order α from eigenvalues

// Quantum mutual information I = S(A) + S(B) - S(AB)
fn builtin_quantum_mutual_info(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s_a = f1(args);
    let s_b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let s_ab = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(s_a + s_b - s_ab))
}

// Concurrence (2-qubit pure state amplitudes a, b, c, d)

// Entanglement entropy from concurrence (Wootters)
fn builtin_eof_from_concurrence(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c = f1(args).clamp(0.0, 1.0);
    let h = (1.0 + (1.0 - c * c).max(0.0).sqrt()) / 2.0;
    let h = h.clamp(1e-15, 1.0 - 1e-15);
    let s = -h * h.log2() - (1.0 - h) * (1.0 - h).log2();
    Ok(PerlValue::float(s))
}

// Bell state amplitude pattern (ψ+, ψ-, φ+, φ-) by index
fn builtin_bell_state_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let idx = i1(args).rem_euclid(4);
    let s = (0.5_f64).sqrt();
    let amps = match idx {
        0 => vec![s, 0.0, 0.0, s],     // φ+
        1 => vec![s, 0.0, 0.0, -s],    // φ-
        2 => vec![0.0, s, s, 0.0],     // ψ+
        _ => vec![0.0, s, -s, 0.0],    // ψ-
    };
    Ok(PerlValue::array(amps.into_iter().map(PerlValue::float).collect()))
}

// CHSH operator expectation (a, a', b, b' all in [-1,1])
fn builtin_chsh_expectation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e_ab = f1(args);
    let e_abp = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let e_apb = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let e_apbp = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(e_ab + e_abp + e_apb - e_apbp))
}

// Tsirelson bound 2√2
fn builtin_tsirelson_bound() -> PerlResult<PerlValue> {
    Ok(PerlValue::float(2.0 * (2.0_f64).sqrt()))
}

// Pauli matrix elements σ_X, σ_Y, σ_Z (one per index 1..4 = I, X, Y, Z; Y returns real-imag interleaved)
fn builtin_pauli_real_part(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let idx = i1(args);
    let m = match idx {
        1 => vec![vec![0.0, 1.0], vec![1.0, 0.0]],     // X
        2 => vec![vec![0.0, 0.0], vec![0.0, 0.0]],     // Y real part
        3 => vec![vec![1.0, 0.0], vec![0.0, -1.0]],    // Z
        _ => vec![vec![1.0, 0.0], vec![0.0, 1.0]],     // I
    };
    Ok(PerlValue::array(m.into_iter().map(|r|
        PerlValue::array(r.into_iter().map(PerlValue::float).collect())
    ).collect()))
}

// Pauli σ_Y imaginary part
fn builtin_pauli_y_imag() -> PerlResult<PerlValue> {
    let m = vec![vec![0.0, -1.0], vec![1.0, 0.0]];
    Ok(PerlValue::array(m.into_iter().map(|r|
        PerlValue::array(r.into_iter().map(PerlValue::float).collect())
    ).collect()))
}

// Bloch vector → density matrix (real part only since Y component imag)
fn builtin_bloch_to_density_real(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rx = f1(args);
    let _ry = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let rz = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let rho = vec![
        vec![0.5 * (1.0 + rz), 0.5 * rx],
        vec![0.5 * rx, 0.5 * (1.0 - rz)],
    ];
    Ok(PerlValue::array(rho.into_iter().map(|r|
        PerlValue::array(r.into_iter().map(PerlValue::float).collect())
    ).collect()))
}

// Bloch vector magnitude r = sqrt(rx²+ry²+rz²); pure state iff r=1
fn builtin_bloch_purity_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rx = f1(args);
    let ry = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let rz = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((rx * rx + ry * ry + rz * rz).sqrt()))
}

// Fidelity for pure states |⟨ψ|φ⟩|² (real amplitudes)
fn builtin_fidelity_pure_real(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let psi: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let phi: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let dot: f64 = psi.iter().zip(phi.iter()).map(|(a, b)| a * b).sum();
    Ok(PerlValue::float(dot * dot))
}

// Quantum coherence (l1 norm) for diagonal-real density matrix
fn builtin_l1_coherence(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rho = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = rho.len();
    let mut sum = 0.0;
    for i in 0..n {
        for j in 0..n.min(rho[i].len()) {
            if i != j { sum += rho[i][j].abs(); }
        }
    }
    Ok(PerlValue::float(sum))
}

// Relative entropy of coherence S(ρ_diag) - S(ρ)
fn builtin_relative_entropy_coherence(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s_diag = f1(args);
    let s_rho = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(s_diag - s_rho))
}

// Kraus operator action on state vector — single Kraus op K|ψ⟩
fn builtin_kraus_apply(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let psi: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = k.len();
    let mut out = vec![0.0; n];
    for i in 0..n {
        for j in 0..k[i].len().min(psi.len()) {
            out[i] += k[i][j] * psi[j];
        }
    }
    Ok(PerlValue::array(out.into_iter().map(PerlValue::float).collect()))
}

// Bit-flip channel probability
fn builtin_bit_flip_prob(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args).clamp(0.0, 1.0);
    Ok(PerlValue::array(vec![PerlValue::float(1.0 - p), PerlValue::float(p)]))
}

// Phase-flip channel ρ → (1−p)ρ + p Z ρ Z. On the Bloch vector r = (x, y, z),
// this damps the off-diagonal coherences but leaves Z invariant:
// r' = ((1−2p)·x, (1−2p)·y, z). Returns the new Bloch components for input p.
// Distinct from bit-flip (which preserves X and damps Y, Z).
fn builtin_phase_flip_prob(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args).clamp(0.0, 1.0);
    let f = 1.0 - 2.0 * p;
    Ok(PerlValue::array(vec![PerlValue::float(f), PerlValue::float(f), PerlValue::float(1.0)]))
}

// Depolarizing channel ρ → (1-p)ρ + p I/2
fn builtin_depolarizing_density_2x2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rho = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).clamp(0.0, 1.0);
    if rho.len() < 2 || rho[0].len() < 2 { return Ok(PerlValue::array(vec![])); }
    let new_rho = vec![
        vec![(1.0 - p) * rho[0][0] + p * 0.5, (1.0 - p) * rho[0][1]],
        vec![(1.0 - p) * rho[1][0], (1.0 - p) * rho[1][1] + p * 0.5],
    ];
    Ok(PerlValue::array(new_rho.into_iter().map(|r|
        PerlValue::array(r.into_iter().map(PerlValue::float).collect())
    ).collect()))
}

// Amplitude damping action on |1⟩ → (1-γ)|1⟩
fn builtin_amplitude_damping_excited(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let gamma = f1(args).clamp(0.0, 1.0);
    Ok(PerlValue::float(1.0 - gamma))
}

// Quantum Fisher information for parameter θ from |ψ_θ⟩, |ψ'_θ⟩ (dot products)
fn builtin_quantum_fisher_info(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let psi_dot_psi = f1(args);
    let dpsi_dot_dpsi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let psi_dot_dpsi = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if psi_dot_psi == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(4.0 * (dpsi_dot_dpsi - psi_dot_dpsi * psi_dot_dpsi / psi_dot_psi)))
}

// Cramer-Rao bound from QFI
fn builtin_cramer_rao_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let qfi = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if qfi * n == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / (n * qfi)))
}

// Squeezing parameter dB from variance ratio
fn builtin_squeezing_db(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let var_ratio = f1(args);
    if var_ratio <= 0.0 { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    Ok(PerlValue::float(10.0 * var_ratio.log10()))
}

// Heisenberg uncertainty product Δx Δp ≥ ℏ/2
fn builtin_heisenberg_min(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let hbar = args.first().map(|v| v.to_number()).unwrap_or(1.054571817e-34);
    Ok(PerlValue::float(hbar / 2.0))
}

// Coherent state |α|² mean photon number
fn builtin_coherent_mean_photons(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha_re = f1(args);
    let alpha_im = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(alpha_re * alpha_re + alpha_im * alpha_im))
}

// Thermal state mean photons n̄ = 1/(exp(βħω)-1)
fn builtin_thermal_mean_photons(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let omega = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(300.0);
    let hbar = 1.054571817e-34;
    let kb = 1.380649e-23;
    if t <= 0.0 || omega == 0.0 { return Ok(PerlValue::float(0.0)); }
    let exp_arg = hbar * omega / (kb * t);
    if exp_arg > 700.0 { return Ok(PerlValue::float(0.0)); }
    let denom = exp_arg.exp() - 1.0;
    if denom == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / denom))
}

// Photon distribution Poisson (coherent) P(n)
fn builtin_poisson_photon_pmf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_bar = f1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let mut fact = 1.0_f64;
    for k in 1..=n { fact *= k as f64; }
    Ok(PerlValue::float(n_bar.powi(n as i32) * (-n_bar).exp() / fact))
}

// Bose-Einstein photon dist P(n) = n̄ⁿ/(1+n̄)^(n+1)
fn builtin_bose_einstein_pmf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_bar = f1(args);
    let n = args.get(1).map(|v| v.to_number() as i32).unwrap_or(0).max(0);
    Ok(PerlValue::float(n_bar.powi(n) / (1.0 + n_bar).powi(n + 1)))
}

// Mandel Q parameter Q = (Var(n) - n̄)/n̄
fn builtin_mandel_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let var_n = f1(args);
    let n_bar = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n_bar == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((var_n - n_bar) / n_bar))
}

// g²(0) second-order correlation
fn builtin_g2_zero(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mean_n2 = f1(args);
    let mean_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if mean_n == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((mean_n2 - mean_n) / (mean_n * mean_n)))
}

// Free-particle dispersion E = ℏ²k²/(2m)
fn builtin_free_particle_energy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(9.10938356e-31);
    let hbar = 1.054571817e-34;
    if m == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(hbar * hbar * k * k / (2.0 * m)))
}

// Square-well bound state E_n = n²ℏ²π²/(2mL²)
fn builtin_infinite_well_energy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(9.10938356e-31);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(1e-10);
    let hbar = 1.054571817e-34;
    let pi = std::f64::consts::PI;
    if m * l * l == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(n * n * hbar * hbar * pi * pi / (2.0 * m * l * l)))
}

// Harmonic oscillator energy E_n = ℏω(n+1/2)
fn builtin_harmonic_oscillator_energy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let omega = args.get(1).map(|v| v.to_number()).unwrap_or(1e15);
    let hbar = 1.054571817e-34;
    Ok(PerlValue::float(hbar * omega * (n + 0.5)))
}

// Hydrogen atom energy E_n = -13.6 eV / n²
fn builtin_hydrogen_energy_n(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    if n == 0.0 { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    Ok(PerlValue::float(-13.605693122994 / (n * n)))
}

// Reduced mass m_red = m1·m2/(m1+m2)

// Stark shift first order ΔE = -e·E·z (linear)
fn builtin_stark_shift_linear(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e_field = f1(args);
    let z_expectation = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let e_charge = 1.602176634e-19;
    Ok(PerlValue::float(-e_charge * e_field * z_expectation))
}

// Zeeman energy ΔE = μ_B g_J m_J B
fn builtin_zeeman_energy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g_j = f1(args);
    let m_j = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let mu_b = 9.2740100783e-24;
    Ok(PerlValue::float(mu_b * g_j * m_j * b))
}

// Larmor frequency ω_L = γ B
fn builtin_larmor_frequency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let gamma = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(gamma * b))
}

// Rabi frequency Ω = μ·E/ℏ
fn builtin_rabi_frequency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mu = f1(args);
    let e = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let hbar = 1.054571817e-34;
    Ok(PerlValue::float(mu * e / hbar))
}

// 1-d Schrödinger numerical step (Crank-Nicolson, explicit kinetic)
fn builtin_schrodinger_step_real(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let psi: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let v: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    let dx = args.get(3).map(|v| v.to_number()).unwrap_or(0.1);
    let n = psi.len();
    let mut out = vec![0.0; n];
    for i in 0..n {
        let lap = if i == 0 || i == n - 1 { 0.0 }
                  else { (psi[i + 1] - 2.0 * psi[i] + psi[i - 1]) / (dx * dx) };
        let v_i = *v.get(i).unwrap_or(&0.0);
        out[i] = psi[i] - dt * (-0.5 * lap + v_i * psi[i]);
    }
    Ok(PerlValue::array(out.into_iter().map(PerlValue::float).collect()))
}

// Probability density |ψ|² (real ψ)
fn builtin_probability_density(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let psi: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    Ok(PerlValue::array(psi.iter().map(|&p| PerlValue::float(p * p)).collect()))
}

// Norm of state ⟨ψ|ψ⟩ (real amps)
fn builtin_state_norm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let psi: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    Ok(PerlValue::float(psi.iter().map(|&p| p * p).sum::<f64>().sqrt()))
}

// Normalize state vector
fn builtin_state_normalize(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let psi: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n: f64 = psi.iter().map(|&p| p * p).sum::<f64>().sqrt().max(1e-15);
    Ok(PerlValue::array(psi.into_iter().map(|p| PerlValue::float(p / n)).collect()))
}

// Expectation value ⟨ψ|A|ψ⟩ (real)

// Variance ⟨A²⟩ - ⟨A⟩²
fn builtin_quantum_variance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mean_a = f1(args);
    let mean_a2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(mean_a2 - mean_a * mean_a))
}

// Spin-J Casimir ⟨J²⟩ = ℏ² j(j+1)
fn builtin_spin_casimir(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let j = f1(args);
    let hbar = 1.054571817e-34;
    Ok(PerlValue::float(hbar * hbar * j * (j + 1.0)))
}

// Clebsch-Gordan coefficient ⟨j₁ m₁; j₂ m₂ | j m⟩ via the Racah/closed-form
// formula:
//   ⟨j₁ m₁; j₂ m₂ | j m⟩ = δ_{m, m₁+m₂} · √((2j+1) Δ(j₁ j₂ j))
//   · √(∏ four (j±m)! ratios) · Σ_k (−1)^k / [k! (j₁+j₂−j−k)!
//   (j₁−m₁−k)! (j₂+m₂−k)! (j−j₂+m₁+k)! (j−j₁−m₂+k)!]
// Args: j1, m1, j2, m2, j, m (all in half-integer units doubled to integers
// by the caller, i.e. pass 2j₁ etc. — convention: integer doubled values).
fn builtin_cg_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let two_j1 = i1(args);
    let two_m1 = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let two_j2 = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let two_m2 = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let two_j  = args.get(4).map(|v| v.to_number() as i64).unwrap_or(two_j1 + two_j2);
    let two_m  = args.get(5).map(|v| v.to_number() as i64).unwrap_or(two_m1 + two_m2);
    if two_m1 + two_m2 != two_m { return Ok(PerlValue::float(0.0)); }
    if two_j < (two_j1 - two_j2).abs() || two_j > two_j1 + two_j2 { return Ok(PerlValue::float(0.0)); }
    if (two_j1 + two_j2 + two_j) % 2 != 0 { return Ok(PerlValue::float(0.0)); }
    fn fact(n: i64) -> f64 {
        if n < 0 { return f64::NAN; }
        let mut p = 1.0_f64;
        for k in 2..=n { p *= k as f64; }
        p
    }
    let ja = (two_j1 + two_j2 - two_j) / 2;
    let jb = (two_j1 - two_j2 + two_j) / 2;
    let jc = (-two_j1 + two_j2 + two_j) / 2;
    let jd = (two_j1 + two_j2 + two_j) / 2 + 1;
    if ja < 0 || jb < 0 || jc < 0 { return Ok(PerlValue::float(0.0)); }
    let triangle = fact(ja) * fact(jb) * fact(jc) / fact(jd);
    let m1p = (two_j1 + two_m1) / 2; let m1m = (two_j1 - two_m1) / 2;
    let m2p = (two_j2 + two_m2) / 2; let m2m = (two_j2 - two_m2) / 2;
    let mp  = (two_j  + two_m ) / 2; let mm  = (two_j  - two_m ) / 2;
    let prefac = ((two_j as f64 + 1.0) * triangle * fact(m1p) * fact(m1m)
        * fact(m2p) * fact(m2m) * fact(mp) * fact(mm)).max(0.0).sqrt();
    let k_lo = 0_i64.max((two_j2 - two_j - two_m1) / 2).max((two_j1 - two_j + two_m2) / 2);
    let k_hi = ja.min((two_j1 - two_m1) / 2).min((two_j2 + two_m2) / 2);
    let mut sum = 0.0_f64;
    for k in k_lo..=k_hi {
        let denom = fact(k) * fact(ja - k) * fact((two_j1 - two_m1) / 2 - k)
            * fact((two_j2 + two_m2) / 2 - k)
            * fact((two_j - two_j2 + two_m1) / 2 + k)
            * fact((two_j - two_j1 - two_m2) / 2 + k);
        let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
        if denom > 0.0 { sum += sign / denom; }
    }
    Ok(PerlValue::float(prefac * sum))
}

// Wigner 3-j upper bound (rough sanity)
fn builtin_wigner_3j_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let j = f1(args);
    Ok(PerlValue::float(1.0 / (2.0 * j + 1.0).max(1.0).sqrt()))
}

// Quantum harmonic oscillator wavefunction at x for n=0 (Gaussian)
fn builtin_qho_ground_state(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(9.10938356e-31);
    let omega = args.get(2).map(|v| v.to_number()).unwrap_or(1e15);
    let pi = std::f64::consts::PI;
    let hbar = 1.054571817e-34;
    let alpha = m * omega / hbar;
    Ok(PerlValue::float((alpha / pi).powf(0.25) * (-0.5 * alpha * x * x).exp()))
}

// Tunneling probability (rectangular barrier) T ≈ exp(-2 κ a)
fn builtin_tunneling_prob(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v0 = f1(args);
    let e = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(2).map(|v| v.to_number()).unwrap_or(1e-10);
    let m = args.get(3).map(|v| v.to_number()).unwrap_or(9.10938356e-31);
    let hbar = 1.054571817e-34;
    if v0 <= e { return Ok(PerlValue::float(1.0)); }
    let kappa = (2.0 * m * (v0 - e)).max(0.0).sqrt() / hbar;
    Ok(PerlValue::float((-2.0 * kappa * a).exp()))
}

// Gamow factor for Coulomb barrier penetration
fn builtin_gamow_factor(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z1 = f1(args);
    let z2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let e = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let m = args.get(3).map(|v| v.to_number()).unwrap_or(1.6726219e-27);
    let hbar = 1.054571817e-34;
    let e_charge = 1.602176634e-19;
    let eps_0 = 8.854187817e-12;
    let pi = std::f64::consts::PI;
    if e <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let prefactor = pi * z1 * z2 * e_charge * e_charge / (2.0 * pi * eps_0 * hbar);
    let inv_v = (m / (2.0 * e)).max(0.0).sqrt();
    Ok(PerlValue::float((-prefactor * inv_v).exp()))
}

// Compton wavelength λ_C = h/(mc)
fn builtin_compton_wavelength(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let h = 6.62607015e-34;
    let c = 2.99792458e8;
    if m == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(h / (m * c)))
}

// Uncertainty in position from momentum spread
fn builtin_uncertainty_position(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dp = f1(args);
    let hbar = 1.054571817e-34;
    if dp == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(hbar / (2.0 * dp)))
}

// Berry phase γ from solid angle Ω: γ = -Ω/2 (spin-1/2)
fn builtin_berry_phase_spin_half(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let omega = f1(args);
    Ok(PerlValue::float(-omega / 2.0))
}

// Quantum Zeno effect survival probability after N measurements
fn builtin_zeno_survival(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dt = f1(args);
    let n = args.get(1).map(|v| v.to_number() as i32).unwrap_or(10);
    let h = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let p_step = (1.0 - h * h * dt * dt / 2.0).max(0.0);
    Ok(PerlValue::float(p_step.powi(n)))
}

// Decoherence time T2 ≈ 1/Γ
fn builtin_decoherence_time(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let gamma = f1(args);
    if gamma == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / gamma))
}

// Ramsey fringe visibility V e^(-t/T2)
fn builtin_ramsey_visibility(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v0 = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if t2 <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(v0 * (-t / t2).exp()))
}

// Fermi golden rule transition rate
fn builtin_fermi_golden_rule(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let matrix_elem_sq = f1(args);
    let dos = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let pi = std::f64::consts::PI;
    let hbar = 1.054571817e-34;
    Ok(PerlValue::float(2.0 * pi / hbar * matrix_elem_sq * dos))
}

// de Broglie wavelength p = h/λ
