// Batch 39 — tensor calculus, GR, differential geometry, black holes, gravitational waves.

fn b39_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// Contraction T^a_a = Σ T^i_i (trace of two-index tensor as flat list)
fn builtin_tensor_contract_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b39_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = (v.len() as f64).sqrt() as usize;
    let s: f64 = (0..n).map(|i| v[i * n + i]).sum();
    Ok(StrykeValue::float(s))
}

/// Outer product two scalars (dyadic) returns u·v
fn builtin_tensor_outer_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let u = f1(args);
    let v = args.get(1).map(|x| x.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(u * v))
}

/// Trace at a specific index (alias to contract_two for 2-index tensor)
fn builtin_tensor_trace_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_tensor_contract_two(args)
}

/// Symmetrize T_(ab) = (T_ab + T_ba) / 2
fn builtin_tensor_symmetrize_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((a + b) / 2.0))
}

/// Antisymmetrize T_[ab] = (T_ab - T_ba) / 2
fn builtin_tensor_antisymmetrize_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((a - b) / 2.0))
}

/// Levi-Civita ε_{ijk} (3D)
fn builtin_levi_civita_three(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let k = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let p = (j - i) * (k - i) * (k - j);
    let s = if p > 0 { 1 } else if p < 0 { -1 } else { 0 };
    Ok(StrykeValue::integer(s))
}

/// Levi-Civita ε_{ijkl} (4D, sign of permutation)
fn builtin_levi_civita_four(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v: Vec<i64> = (0..4)
        .map(|k| args.get(k).map(|x| x.to_number() as i64).unwrap_or(k as i64))
        .collect();
    let mut sign = 1_i64;
    let mut a = v.clone();
    for i in 0..4 {
        for j in (i + 1)..4 {
            if a[i] > a[j] { a.swap(i, j); sign = -sign; }
        }
    }
    if a == [0, 1, 2, 3] { Ok(StrykeValue::integer(sign)) } else { Ok(StrykeValue::integer(0)) }
}

/// Kronecker delta in 3D: δ^i_j
fn builtin_kronecker_three(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if i == j && (0..3).contains(&i) { 1 } else { 0 }))
}

/// Kronecker delta in 4D
fn builtin_kronecker_four(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if i == j && (0..4).contains(&i) { 1 } else { 0 }))
}

/// Minkowski metric η_{μν} step: diag(-1,1,1,1)
fn builtin_metric_minkowski_eta_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mu = i1(args);
    let nu = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if mu != nu { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(if mu == 0 { -1 } else { 1 }))
}

/// Schwarzschild metric component g_{tt} = -(1 - 2M/r)
fn builtin_metric_schwarzschild_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    if r <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-(1.0 - 2.0 * m / r)))
}

/// Kerr metric tt component (slow-rotation limit) g_{tt} ≈ -(1 - 2Mr/Σ), Σ = r²
fn builtin_metric_kerr_step_simple(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    let a = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args.get(3).map(|v| v.to_number()).unwrap_or(std::f64::consts::PI / 2.0);
    let sigma = r * r + (a * theta.cos()).powi(2);
    if sigma == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-(1.0 - 2.0 * m * r / sigma)))
}

/// FRW lapse depends on time gauge:
///   gauge 0 (cosmic / synchronous time t):  N(t) = 1
///   gauge 1 (conformal time η, ds² = a² (−dη² + dx²)):  N(η) = a(η)
///   gauge 2 (e-fold N as time):  N = 1/H(N)
/// Args: gauge_id, scale_factor a, Hubble H.
fn builtin_metric_frw_lapse(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let gauge = i1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let h = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    match gauge {
        0 => Ok(StrykeValue::float(1.0)),
        1 => Ok(StrykeValue::float(a)),
        2 => Ok(StrykeValue::float(if h == 0.0 { f64::INFINITY } else { 1.0 / h })),
        _ => Ok(StrykeValue::float(1.0)),
    }
}

/// Christoffel symbols of the first kind: Γ_{abc} = ½(∂g_ab + ∂g_ac - ∂g_bc)
fn builtin_christoffel_first_kind_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dg_ab = f1(args);
    let dg_ac = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dg_bc = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(0.5 * (dg_ab + dg_ac - dg_bc)))
}

/// Christoffel of the second kind: Γ^a_{bc} = g^{ad} Γ_{dbc}
fn builtin_christoffel_second_kind_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g_inv = f1(args);
    let gamma_first = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(g_inv * gamma_first))
}

/// Riemann tensor R^a_{bcd} = ∂_c Γ^a_{bd} - ∂_d Γ^a_{bc} + Γ^a_{ce} Γ^e_{bd} - Γ^a_{de} Γ^e_{bc}
/// Args: ∂cΓbd, ∂dΓbc, ΓceΓebd_sum, ΓdeΓebc_sum
fn builtin_riemann_tensor_step_zero(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dc_g_bd = f1(args);
    let dd_g_bc = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let gce_gebd = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let gde_gebc = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(dc_g_bd - dd_g_bc + gce_gebd - gde_gebc))
}

/// Riemann normal-coordinate form: R_{abcd} ≈ -1/3 (g_ac g_bd - g_ad g_bc) K
fn builtin_riemann_curvature_normal_form(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = f1(args);
    Ok(StrykeValue::float(-k / 3.0))
}

/// Ricci R_{ab} = R^c_{acb}: contraction of Riemann over upper index = c. Sum diagonal.
fn builtin_ricci_tensor_step_zero(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r_components = b39_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(r_components.iter().sum()))
}

/// Scalar curvature R step from Ricci trace
fn builtin_scalar_curvature_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b39_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().sum()))
}

/// Einstein tensor G_{ab} = R_{ab} - ½ g_{ab} R
fn builtin_einstein_tensor_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r_ab = f1(args);
    let g_ab = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(r_ab - 0.5 * g_ab * r))
}

/// Weyl tensor C_{abcd} = R_{abcd} - (n-2)⁻¹·(g_{a[c}R_{d]b} - g_{b[c}R_{d]a})
///                        + 2/((n-1)(n-2))·R·g_{a[c}g_{d]b}
fn builtin_weyl_tensor_step_zero(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let riemann = f1(args);
    let ricci_term = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let scalar_term = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(3).map(|v| v.to_number()).unwrap_or(4.0);
    if n <= 2.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(riemann - ricci_term / (n - 2.0) + 2.0 * scalar_term / ((n - 1.0) * (n - 2.0))))
}

/// Schouten tensor S = (R_{ab} - R/(2(n-1)) g_{ab}) / (n-2)
fn builtin_schouten_tensor_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r_ab = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(3).map(|v| v.to_number()).unwrap_or(4.0);
    if n <= 2.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((r_ab - r * g / (2.0 * (n - 1.0))) / (n - 2.0)))
}

/// Geodesic equation step: d²x^a/dτ² + Γ^a_{bc} (dx^b/dτ)(dx^c/dτ) = 0
/// Returns -Γ^a_{bc} · u^b · u^c (the acceleration component).
fn builtin_geodesic_equation_step_zero(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let gamma = f1(args);
    let u_b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let u_c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(-gamma * u_b * u_c))
}

/// Parallel transport step: V'^a + Γ^a_{bc} V^b dx^c/dτ = 0
fn builtin_parallel_transport_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_a = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v_b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dxc = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(v_a - gamma * v_b * dxc))
}

/// Covariant derivative ∇_μ V^a = ∂_μ V^a + Γ^a_{μb} V^b
fn builtin_covariant_derivative_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dva = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v_b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(dva + gamma * v_b))
}

/// Christoffel symbol normalization: g^ad Γ_{dbc}
fn builtin_christoffel_symbol_normalize(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_christoffel_second_kind_step(args)
}

/// Ricci identity [∇_a, ∇_b] V^c = R^c_{dab} V^d → linear contribution
fn builtin_ricci_identity_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let v_d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(r * v_d))
}

/// First Bianchi identity check: R_{abcd} + R_{acdb} + R_{adbc} = 0
fn builtin_bianchi_first_identity_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r1 = f1(args);
    let r2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r3 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if (r1 + r2 + r3).abs() < 1e-9 { 1 } else { 0 }))
}

/// Second (differential) Bianchi identity:
///   ∇_a R^d_{ebc} + ∇_b R^d_{eca} + ∇_c R^d_{eab} = 0.
/// Differs from the first Bianchi (algebraic, on lower indices). Verifies the
/// cyclic sum of three covariant-derivative components vanishes.
/// Args: ∇_a R, ∇_b R (cyclic permutation), ∇_c R.
fn builtin_bianchi_second_identity_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let nabla_a = f1(args);
    let nabla_b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let nabla_c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if (nabla_a + nabla_b + nabla_c).abs() < 1e-9 { 1 } else { 0 }))
}

/// Killing equation step ∇_a ξ_b + ∇_b ξ_a = 0 — return value
fn builtin_killing_vector_lie_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let nab = f1(args);
    let nba = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(nab + nba))
}

/// Lie derivative scalar: L_X f = X^a ∂_a f
fn builtin_lie_derivative_scalar_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x_a = f1(args);
    let df = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(x_a * df))
}

/// Lie derivative vector: L_X V^a = X^b ∂_b V^a - V^b ∂_b X^a
fn builtin_lie_derivative_vector_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x_b = f1(args);
    let dv = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v_b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dx = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(x_b * dv - v_b * dx))
}

/// Exterior derivative of one-form ω: dω_{ab} = ∂_a ω_b - ∂_b ω_a
fn builtin_exterior_derivative_one_form(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dao_b = f1(args);
    let dbo_a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(dao_b - dbo_a))
}

/// Hodge star on a 1-form in flat 3D: *ω_i = ε_{ijk} ω^k δ_{j(i+1)}
fn builtin_hodge_star_one_form(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let w1 = f1(args);
    let w2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let w3 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(w1 + w2 + w3))
}

/// Codifferential on a k-form in an n-dim Riemannian manifold:
///   δ = (−1)^{n(k+1) + 1} · ∗ d ∗.
/// Sign depends on (n, k). Caller supplies the value of (∗ d ∗ ω); this fn
/// applies the correct sign. Args: ∗d∗ω value, dimension n, form degree k.
fn builtin_codifferential_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = f1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(4);
    let k = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let exp = n * (k + 1) + 1;
    let sign = if exp.rem_euclid(2) == 0 { 1.0 } else { -1.0 };
    Ok(StrykeValue::float(sign * v))
}

/// Laplace-de Rham operator Δ = dδ + δd
fn builtin_laplace_de_rham_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dd = f1(args);
    let dd2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(dd + dd2))
}

/// Riemannian volume form sqrt(|det g|) dⁿx
fn builtin_volume_form_riemannian(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let det_g = f1(args);
    Ok(StrykeValue::float(det_g.abs().sqrt()))
}

/// Hodge inner product ⟨α, β⟩ = ∫ α ∧ *β
fn builtin_hodge_inner_product_one(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(a * b))
}

/// Sectional curvature K(σ) for two-plane σ given Riemann tensor scalar
fn builtin_sectional_curvature_two_plane(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r_xyxy = f1(args);
    let denom = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(r_xyxy / denom))
}

/// Gauss-Codazzi step: tangent component of R
fn builtin_gauss_codazzi_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r_intrinsic = f1(args);
    let k_term = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(r_intrinsic + k_term))
}

/// Mainardi-Codazzi step: ∇_X k(Y, Z) = ∇_Y k(X, Z)
fn builtin_mainardi_codazzi_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lhs = f1(args);
    let rhs = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(lhs - rhs))
}

/// Weingarten map W(X) = -∇_X N
fn builtin_weingarten_map_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dn = f1(args);
    Ok(StrykeValue::float(-dn))
}

/// Shape operator eigenvalues — return mean of two principal curvatures
fn builtin_shape_operator_eig(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k1 = f1(args);
    let k2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((k1 + k2) / 2.0))
}

/// Mean curvature H = (k1 + k2) / 2
fn builtin_mean_curvature_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k1 = f1(args);
    let k2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((k1 + k2) / 2.0))
}

/// Gaussian curvature K = k1·k2
fn builtin_gaussian_curvature_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k1 = f1(args);
    let k2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(k1 * k2))
}

/// Extrinsic principal curvatures (max, mean form): return greater of two
fn builtin_extrinsic_principal_curv(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k1 = f1(args);
    let k2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(k1.max(k2)))
}

/// Intrinsic (Gauss) curvature K of a 2-surface from its first fundamental form
/// coefficients (E, F, G) and Brioschi formula's discriminant det = EG − F²:
///   K_intrinsic = K_ext only when extrinsic frame is normal-aligned. In general,
///   K = (1 / det(I)²) · [det(M₁) − det(M₂)] (Brioschi). For diagonal I (F=0,
///   constant E, G), reduces to K = (1/(2EG))·[−E_vv − G_uu] (e.g. surface of revolution).
/// Args: E, G, E_vv, G_uu.
fn builtin_intrinsic_principal_curv(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let e = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let e_vv = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let g_uu = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = 2.0 * e * g;
    if denom.abs() < 1e-12 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(-(e_vv + g_uu) / denom))
}

/// Geodesic curvature κ_g of a curve on a surface
fn builtin_geodesic_curvature_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kappa = f1(args);
    let kappa_n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((kappa * kappa - kappa_n * kappa_n).max(0.0).sqrt()))
}

/// Darboux frame step: rotation rate around tangent
fn builtin_darboux_frame_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kappa_g = f1(args);
    let kappa_n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(kappa_g + kappa_n))
}

/// Fermi normal coordinate metric step (linear in geodesic deviation)
fn builtin_fermi_normal_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let xx = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(1.0 - r * xx / 3.0))
}

/// Synge world function σ(x, x') = ½ d(x, x')²
fn builtin_synge_world_function(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d = f1(args);
    Ok(StrykeValue::float(0.5 * d * d))
}

/// Raychaudhuri equation step for expansion: dθ/dτ = -⅓θ² - σ² + ω² - R_{ab}u^au^b
fn builtin_raychaudhuri_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    let sigma2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let omega2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let r_uu = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(-theta * theta / 3.0 - sigma2 + omega2 - r_uu))
}

/// Expansion scalar θ = ∇_μ u^μ from velocity gradient ∂_μ u^μ + Γ^μ_{μν} u^ν
fn builtin_expansion_scalar_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let div_u = f1(args);
    let gamma_trace = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let u = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(div_u + gamma_trace * u))
}

/// Shear tensor σ_{ab} (norm)
fn builtin_shear_tensor_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b39_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().map(|x| x * x).sum::<f64>().sqrt()))
}

/// Twist (vorticity) tensor ω_{ab} = ½(∇_a u_b − ∇_b u_a)·h^a_c·h^b_d (the
/// antisymmetric part of the projected ∇u). Distinct from σ_{ab} (symmetric
/// trace-free, the SHEAR). The norm ω² = ½ ω_{ab}·ω^{ab}. Caller passes the
/// flat antisymmetric components in row-major order; we return ω² = ½·Σ ωᵢⱼ².
fn builtin_twist_tensor_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b39_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let s: f64 = v.iter().map(|x| x * x).sum();
    Ok(StrykeValue::float(0.5 * s))
}

/// Optical scalars: combine expansion, shear, twist
fn builtin_optical_scalars_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let omega = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(theta + sigma + omega))
}

/// Peeling step for Ψ₄ (gravitational radiation): Ψ₄ ~ 1/r at infinity
fn builtin_peeling_step_psi4(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let psi4 = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(psi4 / r))
}

/// AdS metric step (radial part): -((1 + r²/L²)) dt² + dr²/(1 + r²/L²) + r² dΩ²
fn builtin_ads_metric_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if l == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-(1.0 + r * r / (l * l))))
}

/// de Sitter metric step
fn builtin_de_sitter_metric_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if l == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-(1.0 - r * r / (l * l))))
}

/// Warped product metric step: g = g_B + f(t)² g_F
fn builtin_warped_product_step_zero(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g_b = f1(args);
    let f_t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let g_f = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(g_b + f_t * f_t * g_f))
}

/// Kaluza-Klein step: total metric g_5D from g_4D + φ A_a A_b
fn builtin_kaluza_klein_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g4 = f1(args);
    let phi = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let a_a = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let a_b = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(g4 + phi * a_a * a_b))
}

/// Brans-Dicke action term ∫ φ R √-g
fn builtin_brans_dicke_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let phi = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(phi * r))
}

/// Horndeski Lagrangian density. Sub-Lagrangian L_i selected by index 2..5:
///   L₂ = G₂(φ, X)
///   L₃ = -G₃(φ, X) □φ
///   L₄ = G₄(φ, X) R + G₄,X · [(□φ)² - (∇_μ ∇_ν φ)²]
///   L₅ = G₅(φ, X) G^{μν} ∇_μ ∇_ν φ - (1/6) G₅,X · [(□φ)³ - 3 □φ (∇²φ)² + 2 (∇³φ)³]
/// Args: term index (2..5), G_i, scalar curvature R or G_μν·∇∇φ, box_phi, box²,
/// G_i,X, [box³, box·sq, cube].
fn builtin_horndeski_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let curv = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let box_phi = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let nabla_sq = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let g_x = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let box_cu = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
    let cube = args.get(7).map(|v| v.to_number()).unwrap_or(0.0);
    match i {
        2 => Ok(StrykeValue::float(g)),
        3 => Ok(StrykeValue::float(-g * box_phi)),
        4 => Ok(StrykeValue::float(g * curv + g_x * (box_phi * box_phi - nabla_sq))),
        5 => Ok(StrykeValue::float(g * curv - g_x / 6.0
            * (box_cu - 3.0 * box_phi * nabla_sq + 2.0 * cube))),
        _ => Ok(StrykeValue::float(g)),
    }
}

/// Einstein-dilaton step: R + ∇φ · ∇φ
fn builtin_einstein_dilaton_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let dphi2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(r + dphi2))
}

/// Gauss-Bonnet term G = R² - 4 R_{ab} R^{ab} + R_{abcd} R^{abcd} (vanishes in 2D
/// as a topological invariant, equals 4πχ for closed surfaces)
fn builtin_gauss_bonnet_term_2d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let r_ab_sq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let riem_sq = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(r * r - 4.0 * r_ab_sq + riem_sq))
}

/// Chern-Pontryagin term in 4D ∫ R_{abcd} *R^{abcd}
fn builtin_chern_pontryagin_4d_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r2 = f1(args);
    Ok(StrykeValue::float(r2))
}

/// ADM mass M_ADM = (1/16π) ∮ (∂_j h_{ij} - ∂_i h_{jj}) dS^i
fn builtin_adm_mass_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let surface_int = f1(args);
    Ok(StrykeValue::float(surface_int / (16.0 * std::f64::consts::PI)))
}

/// Komar mass M_K = -(1/8π) ∮ ∇^a ξ^b dS_{ab}
fn builtin_komar_mass_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let surface_int = f1(args);
    Ok(StrykeValue::float(-surface_int / (8.0 * std::f64::consts::PI)))
}

/// Bondi mass at null infinity
fn builtin_bondi_mass_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m_initial = f1(args);
    let news_norm = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(m_initial - news_norm))
}

/// Brown-York quasilocal energy E_qlocal = (1/8π) ∮ (k - k₀) √σ d²x
fn builtin_brown_york_quasilocal(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k_diff = f1(args);
    let area_elem = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(k_diff * area_elem / (8.0 * std::f64::consts::PI)))
}

/// Isolated horizon charge
fn builtin_isolated_horizon_charge(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let area_int = f1(args);
    Ok(StrykeValue::float(area_int / (4.0 * std::f64::consts::PI)))
}

/// Trapped surface check: θ_+ < 0 AND θ_- < 0
fn builtin_trapped_surface_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta_plus = f1(args);
    let theta_minus = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if theta_plus < 0.0 && theta_minus < 0.0 { 1 } else { 0 }))
}

/// Apparent horizon step (θ_+ = 0)
fn builtin_apparent_horizon_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta_plus = f1(args);
    Ok(StrykeValue::integer(if theta_plus.abs() < 1e-9 { 1 } else { 0 }))
}

/// Event horizon check at r = 2M for Schwarzschild
fn builtin_event_horizon_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::integer(if (r - 2.0 * m).abs() < 1e-9 { 1 } else { 0 }))
}

/// Cosmological constant term Λ g_{ab}
fn builtin_cosmological_constant_term(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(lambda * g))
}

/// de Sitter radius L_dS = √(3/Λ)
fn builtin_de_sitter_radius_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    if lambda <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float((3.0 / lambda).sqrt()))
}

/// Anti-de Sitter radius L_AdS = √(-3/Λ)
fn builtin_anti_de_sitter_radius_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    if lambda >= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float((-3.0 / lambda).sqrt()))
}

/// Penrose diagram conformal factor sec(t) sec(r)
fn builtin_penrose_diagram_factor(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let ct = t.cos();
    let cr = r.cos();
    if ct == 0.0 || cr == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(1.0 / (ct * cr)))
}

/// Conformal compactification step: Ω = (1 + r²)⁻¹
fn builtin_conformal_compactification_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    Ok(StrykeValue::float(1.0 / (1.0 + r * r)))
}

/// Schwarzschild → Kruskal coordinate U = -e^(-u/4M), V = e^(v/4M)
fn builtin_schwarzschild_kruskal_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let u = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if m == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(-(-u / (4.0 * m)).exp()))
}

/// Gullstrand-Painlevé time T = t + 2√(2Mr) - 4M ln((√r + √(2M))/(√r - √(2M)))
fn builtin_gullstrand_painleve_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(10.0);
    if r <= 2.0 * m { return Ok(StrykeValue::float(t)); }
    let inner = ((r).sqrt() + (2.0 * m).sqrt()) / ((r).sqrt() - (2.0 * m).sqrt());
    Ok(StrykeValue::float(t + 2.0 * (2.0 * m * r).sqrt() - 4.0 * m * inner.ln()))
}

/// Kerr-Newman charge term q²/r²
fn builtin_kerr_newman_charge_term(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    if r == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(q * q / (r * r)))
}

/// Boyer-Lindquist coordinate change step: dt' = dt + (2Mr/Δ) dr
fn builtin_boyer_lindquist_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    let delta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if delta == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(2.0 * m * r / delta))
}

/// Hartle-Thorne metric (slow rotation) component g_tt
fn builtin_hartle_thorne_metric(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    let j = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if r <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-(1.0 - 2.0 * m / r) + j * j / r.powi(4)))
}

/// Oppenheimer-Volkoff equation step: dP/dr = -(ρ + P)(M(r) + 4πr³P) / (r² - 2rM(r))
fn builtin_oppenheimer_volkoff_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rho = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m_r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(10.0);
    let denom = r * r - 2.0 * r * m_r;
    if denom == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-(rho + p) * (m_r + 4.0 * std::f64::consts::PI * r.powi(3) * p) / denom))
}

/// Post-Newtonian correction step: GM/c²r
fn builtin_post_newtonian_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(m / r))
}

/// Shapiro time delay Δt = (2GM/c³) ln((r₁ + r₂ + d)/(r₁ + r₂ - d))
fn builtin_shapiro_delay_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let r1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let r2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    let denom = r1 + r2 - d;
    if denom <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(2.0 * m * ((r1 + r2 + d) / denom).ln()))
}

/// Mercury perihelion advance ω̇ = 6πGM / (c²a(1 - e²))
fn builtin_mercury_perihelion_advance(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let e = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = a * (1.0 - e * e);
    if denom == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(6.0 * std::f64::consts::PI * m / denom))
}

/// Quadrupole gravitational wave amplitude: h ~ G/(c⁴r) Q̈
fn builtin_gravitational_wave_quadrupole(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q_dd = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(q_dd / r))
}

/// Plus polarization amplitude h+
fn builtin_plus_polarization_amp(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h0 = f1(args);
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(h0 * (1.0 + theta.cos().powi(2)) / 2.0))
}

/// Cross polarization amplitude h×
fn builtin_cross_polarization_amp(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h0 = f1(args);
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(h0 * theta.cos()))
}

/// Chirp mass M_c = (m₁m₂)^(3/5) / (m₁+m₂)^(1/5)
fn builtin_chirp_mass_inspiral_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m1 = f1(args);
    let m2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let total = m1 + m2;
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((m1 * m2).powf(0.6) / total.powf(0.2)))
}

/// ISCO radius for Kerr: r_ISCO = M (3 + Z₂ - sign(a)√((3-Z₁)(3+Z₁+2Z₂)))
fn builtin_isco_radius_kerr_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if a.abs() > 1.0 { return Ok(StrykeValue::float(0.0)); }
    let z1 = 1.0 + (1.0 - a * a).cbrt() * ((1.0 + a).cbrt() + (1.0 - a).cbrt());
    let z2 = (3.0 * a * a + z1 * z1).sqrt();
    let sign_a = if a >= 0.0 { 1.0 } else { -1.0 };
    Ok(StrykeValue::float(m * (3.0 + z2 - sign_a * ((3.0 - z1) * (3.0 + z1 + 2.0 * z2)).sqrt())))
}

/// Spin-orbit coupling term in PN expansion: ξ_SO = (s/m²)·sin θ
fn builtin_spin_orbit_coupling_term(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let theta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if m == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(s * theta.sin() / (m * m)))
}

/// Spin-spin coupling
fn builtin_spin_spin_coupling_term(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s1 = f1(args);
    let s2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(s1 * s2))
}

/// Hawking area increase: dA / dt ≥ 0
fn builtin_hawking_area_increase(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let da_dt = f1(args);
    Ok(StrykeValue::integer(if da_dt >= 0.0 { 1 } else { 0 }))
}

/// Unruh temperature T = ℏ a / (2πck_B)
fn builtin_unruh_temperature_full(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let hbar = 1.054_571_817e-34;
    let c = 2.997_924_58e8;
    let kb = 1.380_649e-23;
    Ok(StrykeValue::float(hbar * a / (2.0 * std::f64::consts::PI * c * kb)))
}

/// Bekenstein entropy S = A / (4 ℓ_P²) with ℓ_P = 1
fn builtin_bekenstein_entropy_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let area = f1(args);
    Ok(StrykeValue::float(area / 4.0))
}

/// Holographic entanglement entropy S = (Area_minimal / 4G_N)
fn builtin_holographic_entanglement_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let area = f1(args);
    let g_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if g_n == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(area / (4.0 * g_n)))
}

/// Ryu-Takayanagi formula step
fn builtin_ryu_takayanagi_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_holographic_entanglement_step(args)
}

/// Swampland distance conjecture check: |Δφ| < d_c (returns 1 if inside)
fn builtin_swampland_distance_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dphi = f1(args);
    let d_c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::integer(if dphi.abs() < d_c { 1 } else { 0 }))
}
