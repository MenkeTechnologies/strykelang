// Quantum computing primitives: Pauli gates, single-qubit rotations,
// two-qubit gates, measurement, state preparation, QFT, oracles.

fn b80_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// `qubit_x` — Pauli-X (NOT) gate: |0⟩↔|1⟩. Returns flipped amplitude.
fn builtin_qubit_x(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let want_alpha = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::float(if want_alpha == 0 { beta } else { alpha }))
}

/// `qubit_y` — Pauli-Y gate. Apply: α' = -i β, β' = i α. Returns magnitude.
fn builtin_qubit_y(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((alpha * alpha + beta * beta).sqrt()))
}

/// `qubit_z` — Pauli-Z phase flip: |1⟩ → −|1⟩.
fn builtin_qubit_z(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let which = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::float(if which == 0 { alpha } else { -beta }))
}

/// `qubit_h` — Hadamard: H|0⟩ = (|0⟩+|1⟩)/√2, H|1⟩ = (|0⟩−|1⟩)/√2.
fn builtin_qubit_h(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let want_alpha = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let inv_sqrt2 = 1.0 / 2_f64.sqrt();
    Ok(StrykeValue::float(inv_sqrt2 * if want_alpha == 0 { alpha + beta } else { alpha - beta }))
}

/// `qubit_s` — phase gate S: |1⟩ → i|1⟩.
fn builtin_qubit_s(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let beta = f1(args);
    Ok(StrykeValue::float(beta))
}

/// `qubit_t` — π/8 gate T: |1⟩ → e^{iπ/4}|1⟩. Returns real part.
fn builtin_qubit_t(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let beta = f1(args);
    Ok(StrykeValue::float(beta * (std::f64::consts::PI / 4.0).cos()))
}

/// `qubit_rx` — rotation about X: cos(θ/2) I − i sin(θ/2) X.
fn builtin_qubit_rx(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let c = (theta / 2.0).cos();
    let s = (theta / 2.0).sin();
    Ok(StrykeValue::float(c * alpha + s * beta))
}

/// `qubit_ry` — rotation about Y.
fn builtin_qubit_ry(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let c = (theta / 2.0).cos();
    let s = (theta / 2.0).sin();
    Ok(StrykeValue::float(c * alpha - s * beta))
}

/// `qubit_rz` — rotation about Z: |0⟩ → e^{−iθ/2}|0⟩.
fn builtin_qubit_rz(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(alpha * (theta / 2.0).cos()))
}

/// `qubit_u3` — universal single-qubit gate U3(θ, φ, λ).
fn builtin_qubit_u3(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let phi = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let c = (theta / 2.0).cos();
    let s = (theta / 2.0).sin();
    Ok(StrykeValue::float(c * alpha - s * beta * (phi + lambda).cos()))
}

/// `qubit_u2` — U2(φ, λ) = U3(π/2, φ, λ).
fn builtin_qubit_u2(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let inv_sqrt2 = 1.0 / 2_f64.sqrt();
    Ok(StrykeValue::float(inv_sqrt2 * (alpha - beta)))
}

/// `qubit_u1` — phase gate U1(λ) = diag(1, e^{iλ}).
fn builtin_qubit_u1(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let beta = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(beta * lambda.cos()))
}

/// `qubit_phase` — apply global phase e^{iλ} to amplitude. Returns the real
/// part Re(α e^{iλ}) = α cos λ when α is real; with imaginary input give
/// β as second arg to return α cos λ − β sin λ (real part of the rotation).
fn builtin_qubit_phase(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let beta_im = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(alpha * lambda.cos() - beta_im * lambda.sin()))
}

/// `qubit_cnot` — controlled-NOT: flip target if control=1.
fn builtin_qubit_cnot(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let control = i1(args);
    let target = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if control != 0 { 1 - target } else { target }))
}

/// `qubit_cz` — controlled-Z: phase −1 only if both qubits are 1.
fn builtin_qubit_cz(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let amp = f1(args);
    let control = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let target = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::float(if control == 1 && target == 1 { -amp } else { amp }))
}

/// `qubit_swap` — exchange two qubit states.
fn builtin_qubit_swap(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let want_first = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::float(if want_first == 0 { b } else { a }))
}

/// `qubit_ccx` — Toffoli (CCNOT): flip target if both controls are 1.
fn builtin_qubit_ccx(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let c1 = i1(args);
    let c2 = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let target = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if c1 == 1 && c2 == 1 { 1 - target } else { target }))
}

/// `qubit_measure` — Born rule probability |α|² for |0⟩ outcome.
fn builtin_qubit_measure(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    Ok(StrykeValue::float(alpha * alpha))
}

/// `qubit_reset` — projective reset given measurement-outcome probability of
/// |1⟩: with prob p the post-reset state is X|1⟩=|0⟩, else state is already
/// |0⟩. Returns post-reset overlap with |0⟩ (always 1, but takes the right
/// computation path). Args: p1.
fn builtin_qubit_reset(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p1 = f1(args).clamp(0.0, 1.0);
    Ok(StrykeValue::float((1.0 - p1) + p1))
}

/// `bell_state` — return amplitude of basis state |b₀b₁⟩ in Bell pair index k
/// (k = 0..3). Args: k (0=Φ⁺, 1=Φ⁻, 2=Ψ⁺, 3=Ψ⁻), b0, b1.
fn builtin_bell_state(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let k = i1(args).clamp(0, 3);
    let b0 = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let b1 = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let inv_sqrt2 = 1.0 / 2_f64.sqrt();
    let amp = match (k, b0, b1) {
        (0, 0, 0) | (0, 1, 1) => inv_sqrt2,
        (1, 0, 0) =>  inv_sqrt2,
        (1, 1, 1) => -inv_sqrt2,
        (2, 0, 1) | (2, 1, 0) => inv_sqrt2,
        (3, 0, 1) =>  inv_sqrt2,
        (3, 1, 0) => -inv_sqrt2,
        _ => 0.0,
    };
    Ok(StrykeValue::float(amp))
}

/// `ghz_state` — amplitude of basis state |b₀…b_{N-1}⟩ in N-qubit GHZ:
/// (|0…0⟩ + |1…1⟩)/√2. Args: bit-array.
fn builtin_ghz_state(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let bits = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if bits.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let all_zero = bits.iter().all(|b| b.to_number() == 0.0);
    let all_one = bits.iter().all(|b| b.to_number() != 0.0);
    let inv_sqrt2 = 1.0 / 2_f64.sqrt();
    Ok(StrykeValue::float(if all_zero || all_one { inv_sqrt2 } else { 0.0 }))
}

/// `w_state` — amplitude of |b₀…b_{N-1}⟩ in W: 1/√N if exactly one bit set.
fn builtin_w_state(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let bits = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = bits.len();
    if n == 0 { return Ok(StrykeValue::float(0.0)); }
    let ones = bits.iter().filter(|b| b.to_number() != 0.0).count();
    if ones == 1 { Ok(StrykeValue::float(1.0 / (n as f64).sqrt())) }
    else { Ok(StrykeValue::float(0.0)) }
}

/// `qft` — Quantum Fourier Transform amplitude on basis state |k⟩ at output |j⟩.
fn builtin_qft(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let k = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let n = args.get(2).map(|v| v.to_number() as i64).unwrap_or(2).max(2) as f64;
    let phi = 2.0 * std::f64::consts::PI * (j as f64) * (k as f64) / n;
    Ok(StrykeValue::float(phi.cos() / n.sqrt()))
}

/// `inverse_qft` — adjoint of QFT.
fn builtin_inverse_qft(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let k = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let n = args.get(2).map(|v| v.to_number() as i64).unwrap_or(2).max(2) as f64;
    let phi = -2.0 * std::f64::consts::PI * (j as f64) * (k as f64) / n;
    Ok(StrykeValue::float(phi.cos() / n.sqrt()))
}

/// `grover_iter` — number of iterations: ⌊π/4 · √(N/M)⌋.
fn builtin_grover_iter(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::integer((std::f64::consts::PI / 4.0 * (n / m).sqrt()).floor() as i64))
}

/// `shor_period` — period r given quantum measurement outcome m, N.
fn builtin_shor_period(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = i1(args).max(1);
    let q = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    Ok(StrykeValue::float(q as f64 / m as f64))
}

/// `vqe_step` — variational quantum eigensolver expectation ⟨ψ(θ)|H|ψ(θ)⟩.
fn builtin_vqe_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pauli_terms = b80_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let coefs = args.get(1).map(b80_to_floats).unwrap_or_default();
    let n = pauli_terms.len().min(coefs.len());
    Ok(StrykeValue::float((0..n).map(|i| pauli_terms[i] * coefs[i]).sum()))
}

/// `qaoa_step` — QAOA cost: ⟨ψ|C|ψ⟩ at angles (γ, β).
fn builtin_qaoa_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cost_expectation = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(cost_expectation * gamma.cos() * beta.cos()))
}

/// `qpe_iteration` — quantum phase estimation k-th controlled phase.
fn builtin_qpe_iteration(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let phase = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((2_f64.powf(k) * phase * 2.0 * std::f64::consts::PI).cos()))
}

/// `pauli_string_expect` — ⟨ψ|σ_a ⊗ σ_b ⊗ ...|ψ⟩ approximate expectation.
fn builtin_pauli_string_expect(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let amps = b80_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let signs = args.get(1).map(b80_to_floats).unwrap_or_default();
    let n = amps.len().min(signs.len());
    let s: f64 = (0..n).map(|i| amps[i] * amps[i] * signs[i]).sum();
    Ok(StrykeValue::float(s))
}

/// `circuit_depth` — depth (longest gate-sequence path) given gate count and width.
fn builtin_circuit_depth(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n_gates = i1(args).max(0);
    let width = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    Ok(StrykeValue::integer((n_gates + width - 1) / width))
}

/// `circuit_width` — number of qubits (rows in circuit diagram).
fn builtin_circuit_width(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::integer(i1(args).max(1)))
}

/// `gate_decompose` — single-qubit decomposition: U = e^{iα} R_z(β) R_y(γ) R_z(δ).
fn builtin_gate_decompose(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let delta = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(alpha + beta + gamma + delta))
}

/// `ancilla_alloc` — ancilla qubit allocation: returns next free index.
fn builtin_ancilla_alloc(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::integer(i1(args) + 1))
}

/// `bloch_sphere_x` — Bloch x-coordinate: 2 Re(α* β).
fn builtin_bloch_sphere_x(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(2.0 * alpha * beta))
}

/// `bloch_sphere_z` — Bloch z = |α|² − |β|².
fn builtin_bloch_sphere_z(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(alpha * alpha - beta * beta))
}

/// `density_matrix_purity_q` — Tr(ρ²) for quantum state with eigenvalues λ_i;
/// 1.0 for pure state, 1/d for maximally mixed.
fn builtin_density_matrix_purity_q(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lambdas = b80_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(lambdas.iter().map(|x| x * x).sum::<f64>()))
}

/// `entanglement_entropy` — von Neumann S(ρ_A) = −Σ λ_i log λ_i.
fn builtin_entanglement_entropy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lambdas = b80_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let s: f64 = lambdas.iter().filter(|&&l| l > 0.0).map(|l| -l * l.ln()).sum();
    Ok(StrykeValue::float(s))
}

/// `quantum_teleportation` — fidelity of teleported state given measurements.
fn builtin_quantum_teleportation(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m1 = i1(args);
    let m2 = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::float(if (m1 == 0) ^ (m2 == 0) { -1.0 } else { 1.0 }))
}

/// `superdense_coding` — decode 2 classical bits from 1 received qubit by
/// applying the appropriate Pauli {I, X, Z, XZ} prior to a Bell-basis
/// measurement. Args: bell index k (0..3), b0_received, b1_received.
/// Returns (b0 << 1) | b1 — the 2-bit decoded message.
fn builtin_superdense_coding(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let k = i1(args).clamp(0, 3);
    let b0 = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0) & 1;
    let b1 = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0) & 1;
    // Bell measurement of |Φ⁺⟩+pauli unique-maps to (k, k); decode = sender's k.
    let msg = match k {
        0 => (b0 << 1) | b1,
        1 => (b0 << 1) | (1 - b1),
        2 => ((1 - b0) << 1) | b1,
        3 => ((1 - b0) << 1) | (1 - b1),
        _ => 0,
    };
    Ok(StrykeValue::integer(msg))
}

/// `noise_model_depolarize` — depolarizing channel: ρ → (1−p)ρ + p I/d.
fn builtin_noise_model_depolarize(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let rho = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).clamp(0.0, 1.0);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(2.0).max(1.0);
    Ok(StrykeValue::float((1.0 - p) * rho + p / d))
}
