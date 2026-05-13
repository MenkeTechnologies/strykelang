// algebraic topology, knot theory, lie algebras, representation theory.

/// Euler characteristic of a CW-complex from face counts (alternating sum)
fn builtin_euler_char_complex(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let s: f64 = v.iter().enumerate()
        .map(|(i, x)| if i % 2 == 0 { x.to_number() } else { -x.to_number() })
        .sum();
    Ok(StrykeValue::float(s))
}

/// β₀ — number of connected components for a graph adjacency vector
fn builtin_betti_zero(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let edges = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((n - edges).max(1)))
}

/// β₁ — first Betti number from V, E, F (V - E + F → β₁ = 1 - χ for surfaces)
fn builtin_betti_one(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = i1(args);
    let e = args.get(1).map(|x| x.to_number() as i64).unwrap_or(0);
    let f = args.get(2).map(|x| x.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(e - v - f + 2))
}

/// β₂ — typically 1 for closed oriented surfaces / 0 otherwise
fn builtin_betti_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let chi = i1(args);
    Ok(StrykeValue::integer(if chi <= 2 { 1 } else { 0 }))
}

/// Genus of a closed orientable surface from χ: g = (2 - χ) / 2
fn builtin_genus_surface(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let chi = f1(args);
    Ok(StrykeValue::float((2.0 - chi) / 2.0))
}

/// First Chern number for 2D vector bundle: c₁ = ∫ F / (2π)
fn builtin_chern_first_2d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let int_f = f1(args);
    Ok(StrykeValue::float(int_f / (2.0 * std::f64::consts::PI)))
}

/// Arithmetic genus of plane curve degree d: pa = (d-1)(d-2)/2
fn builtin_genus_curve_arith(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d = i1(args);
    Ok(StrykeValue::integer((d - 1) * (d - 2) / 2))
}

/// Geometric genus = arithmetic - δ (count of singularity contributions)
fn builtin_genus_curve_geo(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pa = i1(args);
    let delta = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((pa - delta).max(0)))
}

/// Hodge diamond entry h^{p,q} for projective space (PⁿC): δ_{p,q} for p,q ≤ n
fn builtin_hodge_diamond_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = i1(args);
    let q = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let n = args.get(2).map(|v| v.to_number() as i64).unwrap_or(p);
    Ok(StrykeValue::integer(if p == q && p <= n && p >= 0 { 1 } else { 0 }))
}

/// Poincaré duality test: bₖ(M) == bₙ₋ₖ(M) for closed oriented n-manifold
fn builtin_poincare_duality(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bk = f1(args);
    let bnk = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if (bk - bnk).abs() < 1e-9 { 1 } else { 0 }))
}

/// π₁(lens space L(p,q)) ≅ Z/p — return order
fn builtin_fundamental_group_zn(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = i1(args);
    Ok(StrykeValue::integer(p.abs().max(1)))
}

/// Rank of the kth homology group from boundary map ranks
fn builtin_homology_rank(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dim_ck = f1(args);
    let rank_dk = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let rank_dk1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((dim_ck - rank_dk - rank_dk1).max(0.0)))
}

/// Cohomology rank — same as homology over field by universal coefficient
fn builtin_cohomology_rank(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_homology_rank(args)
}

/// πₖ(Sⁿ): canonical small cases (only the obvious ones)
fn builtin_homotopy_group_sphere_pi(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    if k < n { return Ok(StrykeValue::integer(0)); }
    if k == n { return Ok(StrykeValue::integer(1)); }  // Z
    if k == 3 && n == 2 { return Ok(StrykeValue::integer(1)); }  // Hopf
    Ok(StrykeValue::integer(-1))
}

/// Mapping class group of torus is SL(2, ℤ); return its dimension as group (∞ → -1 sentinel)
fn builtin_mapping_class_torus(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::integer(if n == 2 { -1 } else { 1 }))
}

/// Linking number of two disjoint oriented loops (signed crossings / 2)
fn builtin_linking_number_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let signed_crossings = f1(args);
    Ok(StrykeValue::float(signed_crossings / 2.0))
}

/// Writhe of a polygonal knot (sum of signed crossings)
fn builtin_writhe_polygon(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let s: f64 = v.iter().map(|x| x.to_number()).sum();
    Ok(StrykeValue::float(s))
}

/// Torsion coefficient T(n) = order of torsion part for Z/n
fn builtin_torsion_coefficient(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::integer(n.abs()))
}

/// Volume of standard n-simplex of side 1: √(n+1) / (n! 2^(n/2))
fn builtin_simplex_volume_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0) as f64;
    let mut fact = 1.0;
    for i in 1..=(n as i64) { fact *= i as f64; }
    Ok(StrykeValue::float(((n + 1.0).sqrt()) / (fact * 2f64.powf(n / 2.0))))
}

/// Simplicial volume bound (Gromov norm proxy ‖M‖ for surfaces)
fn builtin_simplicial_volume(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g = f1(args);
    Ok(StrykeValue::float(((4.0 * g - 4.0).max(0.0)) * 1.0))
}

/// Number of simplices in a nerve complex with k cover sets, all intersecting
fn builtin_nerve_complex_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = i1(args);
    if k <= 0 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer((1_i64 << k.min(62)) - 1))
}

/// Čech 0th cohomology Ȟ⁰(U; F) = ker δ⁰: count of connected components from cover
/// adjacency. Args: array of overlap-set sizes per cover element. Components =
/// covers - max-spanning-overlaps (forest count).
fn builtin_cech_zero_cohomology(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let overlaps = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = overlaps.len() as i64;
    if n == 0 { return Ok(StrykeValue::integer(0)); }
    let edges: i64 = overlaps.iter().map(|x| x.to_number() as i64).sum::<i64>() / 2;
    Ok(StrykeValue::integer((n - edges).max(1)))
}

/// de Rham H⁰(M; ℝ) = ℝ^{components(M)}. Compute components from adjacency-pair list.
fn builtin_de_rham_zero(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pairs = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(pairs.len() as i64 + 1);
    let mut parent: Vec<i64> = (0..n).collect();
    fn find(p: &mut [i64], i: i64) -> i64 {
        let mut r = i; while p[r as usize] != r { r = p[r as usize]; }
        let mut c = i; while p[c as usize] != c { let nx = p[c as usize]; p[c as usize] = r; c = nx; } r
    }
    let v: Vec<i64> = pairs.iter().map(|x| x.to_number() as i64).collect();
    for ch in v.chunks(2) {
        if ch.len() == 2 && ch[0] >= 0 && ch[0] < n && ch[1] >= 0 && ch[1] < n {
            let r0 = find(&mut parent, ch[0]); let r1 = find(&mut parent, ch[1]);
            if r0 != r1 { parent[r0 as usize] = r1; }
        }
    }
    let mut roots = std::collections::HashSet::new();
    for i in 0..n { roots.insert(find(&mut parent, i)); }
    Ok(StrykeValue::float(roots.len() as f64))
}

/// Poincaré polynomial Σ bₖ tᵏ evaluated at t
fn builtin_poincare_polynomial_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let b = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let s: f64 = b.iter().enumerate().map(|(k, x)| x.to_number() * t.powi(k as i32)).sum();
    Ok(StrykeValue::float(s))
}

/// Helme-Guizon-Rong chromatic homology Hⁱ,ⱼ(G) of a graph G categorifies the
/// chromatic polynomial: Σ (−1)ⁱ qʲ rank Hⁱ,ⱼ = chromatic_polynomial(G; q) (the
/// graded Euler characteristic). Total rank is bounded below by the sum of
/// |coefficients| of the chromatic polynomial. Args: array of chromatic-poly
/// coefficients [c₀, c₁, ..., c_n].
fn builtin_chromatic_homology_rank(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let coefs = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let s: i64 = coefs.iter().map(|x| (x.to_number().abs()) as i64).sum();
    Ok(StrykeValue::integer(s))
}

/// Khovanov q-grading shift on a state with n+ positive and n- negative crossings
fn builtin_khovanov_q_grading(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_plus = i1(args);
    let n_minus = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(n_plus - 2 * n_minus))
}

/// Hochschild cohomology HH⁰(A) = Z(A): center of an algebra given its multiplication
/// table on a basis. Args: flat n×n structure constants matrix; returns dim of Z(A) =
/// dim ker[a, ·] for the universal element computed as nullity of [eᵢ, eⱼ] commutator
/// table. Approximation: count basis elements that commute with every other basis vector.
fn builtin_hochschild_zero(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = (v.len() as f64).sqrt() as usize;
    if n == 0 || n * n != v.len() { return Ok(StrykeValue::float(1.0)); }
    let m: Vec<f64> = v.iter().map(|x| x.to_number()).collect();
    let mut central = 0_usize;
    for i in 0..n {
        let mut commutes = true;
        for j in 0..n {
            if (m[i * n + j] - m[j * n + i]).abs() > 1e-9 { commutes = false; break; }
        }
        if commutes { central += 1; }
    }
    Ok(StrykeValue::float(central.max(1) as f64))
}

/// Connes' SBI long exact sequence:
///   ... → HC_{n-2} →ᴮ HH_n → HC_n →ˢ HC_{n-2} → HH_{n-1} → ...
/// For dimensions over a field this gives  dim HC_n = dim HH_n + dim HC_{n-2}
/// − dim ker(S: HC_n → HC_{n-2}) − dim coker(B: HC_{n-2} → HH_n). Args:
/// dim HH_n, dim HC_{n-2}, rank S, rank B.
fn builtin_cyclic_homology_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let hh_n = f1(args);
    let hc_nm2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let rank_s = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let rank_b = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((hh_n + hc_nm2 - rank_s - rank_b).max(0.0)))
}

/// Group cohomology dim Hⁿ(G; M): for cyclic group Z/m on trivial module M = Z,
/// H⁰ = M, Hⁿ = ker(N)/im(σ-1) for odd n, and coker for even n > 0.
/// Returns rank of cohomology in degree n given group order m.
fn builtin_group_cohomology_dim(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = i1(args).max(1);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if n == 0 { return Ok(StrykeValue::float(1.0)); }
    if m == 1 { return Ok(StrykeValue::float(0.0)); }
    if n % 2 == 0 { Ok(StrykeValue::float(1.0)) } else { Ok(StrykeValue::float(0.0)) }
}

/// Group homology dim Hₙ(G; ℤ): for finite cyclic group Z/m, Hₙ = ℤ/m for odd n,
/// and 0 for even n > 0; H₀ = ℤ. Return rank/order indicator.
fn builtin_group_homology_dim(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = i1(args).max(1);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if n == 0 { return Ok(StrykeValue::float(1.0)); }
    if m == 1 { return Ok(StrykeValue::float(0.0)); }
    if n % 2 == 1 { Ok(StrykeValue::float(m as f64)) } else { Ok(StrykeValue::float(0.0)) }
}

/// Abelianization quotient G/[G,G] for finite group of order n with commutator subgroup of order m
fn builtin_abelianization_quotient(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    if m == 0 { return Ok(StrykeValue::integer(n)); }
    Ok(StrykeValue::integer(n / m))
}

/// Lower bound on free group rank from generators - relations
fn builtin_free_group_rank_lower(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let gens = i1(args);
    let rels = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((gens - rels).max(0)))
}

/// Lower bound on nilpotency class — log₂ of group order minimum
fn builtin_nilpotency_class_lower(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    if n <= 1.0 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(n.log2().ceil() as i64))
}

/// Upper bound on solvable length log₂(log₂ n) + 1
fn builtin_solvable_length_upper(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    if n <= 2.0 { return Ok(StrykeValue::integer(1)); }
    Ok(StrykeValue::integer(n.log2().log2().ceil() as i64 + 1))
}

/// Schreier index theorem: rank of subgroup H of free group F_n of index k = k(n-1) + 1
fn builtin_schreier_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(StrykeValue::integer(k * (n - 1) + 1))
}

/// Todd genus evaluated at first Chern class (linear order: c1/2)
fn builtin_todd_genus_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c1 = f1(args);
    Ok(StrykeValue::float(c1 / 2.0))
}

/// Hirzebruch signature for 4k-manifold: ⟨L_k, [M]⟩ ≈ p₁/3 in dim 4
fn builtin_hirzebruch_signature(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p1 = f1(args);
    Ok(StrykeValue::float(p1 / 3.0))
}

/// Chern-Simons action level k SU(2) on S³: k/(4π²)·∫ tr(A∧dA + 2A³/3)
fn builtin_chern_simons_action(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = f1(args);
    let int_val = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(k * int_val / (4.0 * std::f64::consts::PI * std::f64::consts::PI)))
}

/// Gauss-Bonnet ∫ K dA = 2π χ(M)
fn builtin_gauss_bonnet_total(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let chi = f1(args);
    Ok(StrykeValue::float(2.0 * std::f64::consts::PI * chi))
}

/// Lower bound on Seifert genus from Alexander polynomial degree / 2
fn builtin_seifert_genus_lower(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let deg_alex = i1(args);
    Ok(StrykeValue::integer((deg_alex / 2).max(0)))
}

/// Alexander polynomial evaluated at t = 1 (always returns ±1 for knots)
fn builtin_alexander_polynomial_at_one(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::integer(if (i1(args) % 2).abs() == 0 { 1 } else { -1 }))
}

/// Jones polynomial at q = -1 gives (-1)^c · Δ(-1)
fn builtin_jones_polynomial_at_minus_one(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let determinant = i1(args);
    Ok(StrykeValue::integer(determinant.abs()))
}

/// Jones polynomial at q = i gives ±(√2)^(c-1)
fn builtin_jones_polynomial_at_i(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c = i1(args);
    Ok(StrykeValue::float(2f64.sqrt().powi((c - 1) as i32)))
}

/// HOMFLY polynomial evaluation at (l, m)
fn builtin_homfly_evaluation(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(l * l - m * m + 1.0))
}

/// Kauffman bracket evaluation
fn builtin_kauffman_bracket_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    Ok(StrykeValue::float(-(a * a) - 1.0 / (a * a)))
}

/// Cabling pair signature: (p, q) torus knot signature ≈ -(p-1)(q-1)
fn builtin_cabling_pair_signature(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = i1(args);
    let q = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(StrykeValue::integer(-(p - 1) * (q - 1)))
}

/// Determinant of a Seifert form 2x2 matrix [[a,b],[c,d]] returns ad - bc
fn builtin_seifert_form_2x2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(a * d - b * c))
}

/// Turaev's reformulation of Alexander polynomial — one step (∇(z) = -∇(z) under crossing change)
fn builtin_turaev_alexander_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    Ok(StrykeValue::float(z * z - 1.0))
}

/// V polynomial (Jones-style) evaluated at q
fn builtin_v_polynomial_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q = f1(args);
    Ok(StrykeValue::float(-q.powi(2) - q.powi(-2)))
}

/// Skein relation step for Jones polynomial: q V₊ - q⁻¹ V₋ = (q^(1/2) - q^(-1/2)) V₀
fn builtin_polynomial_jones_skein(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q = f1(args);
    let v_plus = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v_minus = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = q.sqrt() - 1.0 / q.sqrt();
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((q * v_plus - v_minus / q) / denom))
}

/// Number of cells in a Δ-complex with given f-vector
fn builtin_delta_complex_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().map(|x| x.to_number()).sum()))
}

/// Zeta function of a 2-element poset evaluated at s
fn builtin_poset_zeta_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = f1(args);
    Ok(StrykeValue::float(1.0 + s))
}

/// Möbius function of a 2-element poset
fn builtin_mobius_poset_two(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::integer(-1))
}

/// Möbius function for pair (a, b) in divisibility poset
fn builtin_mobius_function_pair(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    if a == b { return Ok(StrykeValue::integer(1)); }
    if b % a != 0 { return Ok(StrykeValue::integer(0)); }
    let mut q = b / a;
    let mut sign = 1_i64;
    let mut p = 2_i64;
    while p * p <= q {
        if q % p == 0 {
            if q % (p * p) == 0 { return Ok(StrykeValue::integer(0)); }
            q /= p;
            sign = -sign;
        } else {
            p += 1;
        }
    }
    if q > 1 { sign = -sign; }
    Ok(StrykeValue::integer(sign))
}

/// Möbius inversion step f(n) = Σ_{d|n} μ(n/d) g(d)
fn builtin_mobius_inversion_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mu_val = f1(args);
    let g_val = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(mu_val * g_val))
}

/// Dimension of incidence algebra of an n-element chain = n(n+1)/2
fn builtin_incidence_algebra_dim(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::integer(n * (n + 1) / 2))
}

/// Number of paths of length k in a quiver with adjacency matrix sum a (scalar approx)
fn builtin_quiver_path_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(a.powf(k)))
}

/// Representation dimension step: dim V ⊗ W = dim V · dim W
fn builtin_representation_dim_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = f1(args);
    let w = args.get(1).map(|x| x.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(v * w))
}

/// Order of Weyl group of Aₙ = (n+1)!
fn builtin_weyl_group_order(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let mut p = 1_i64;
    for k in 2..=(n + 1) { p = p.saturating_mul(k); }
    Ok(StrykeValue::integer(p))
}

/// Number of roots in root system Aₙ = n(n+1)
fn builtin_root_system_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::integer(n * (n + 1)))
}

/// Determinant of Cartan matrix A₂ = 3
fn builtin_cartan_determinant_a2(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::integer(3))
}

/// Cartan matrix B₂ entry (i, j)
fn builtin_cartan_matrix_b2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let m = [[2_i64, -2], [-1, 2]];
    if (0..2).contains(&i) && (0..2).contains(&j) {
        return Ok(StrykeValue::integer(m[i as usize][j as usize]));
    }
    Ok(StrykeValue::integer(0))
}

/// Killing form on su(2): B(X, Y) = 4 tr(XY) — return scalar 4xy
fn builtin_killing_form_su2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(4.0 * x * y))
}

/// Casimir eigenvalue for su(2) spin-j representation: j(j+1)
fn builtin_casimir_eigenvalue_su2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let j = f1(args);
    Ok(StrykeValue::float(j * (j + 1.0)))
}

/// Universal enveloping algebra dimension in degree ≤ k for sl₂: (k+1)(k+2)(k+3)/6
fn builtin_universal_enveloping_dim(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = i1(args);
    Ok(StrykeValue::integer((k + 1) * (k + 2) * (k + 3) / 6))
}

/// Verma module character step ch L_λ at q
fn builtin_verma_character_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lam = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    if (1.0 - q).abs() < 1e-12 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(q.powf(lam) / (1.0 - q)))
}

/// Plethystic substitution f[g] evaluated as f(g(x))
fn builtin_plethystic_substitution_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f_val = f1(args);
    let g_val = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(f_val * g_val))
}

/// Schur polynomial s_λ at single variable x: x^|λ|
fn builtin_schur_polynomial_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lam = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let size: f64 = lam.iter().map(|v| v.to_number()).sum();
    Ok(StrykeValue::float(x.powf(size)))
}

/// Hall inner product on symmetric functions ⟨pₙ, pₙ⟩ = n
fn builtin_hall_inner_product_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(n);
    if n == m { return Ok(StrykeValue::integer(n)); }
    Ok(StrykeValue::integer(0))
}

/// Size of plactic class containing word of length n with k distinct letters
fn builtin_plactic_class_size(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(StrykeValue::integer(k.pow(n.clamp(0, 20) as u32)))
}

/// RSK correspondence is a bijection S_n ↔ {(P, Q) standard Young tableaux of
/// the same shape λ ⊢ n}. Hence Σ_{λ ⊢ n} f_λ² = n! (sum-of-squared dimensions
/// identity). Returns n! exactly.
fn builtin_robinson_schensted_pair(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let mut f = 1_i64;
    for k in 2..=n { f = f.saturating_mul(k); }
    Ok(StrykeValue::integer(f))
}

/// Number of Yamanouchi words of given content
fn builtin_yamanouchi_word_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n: i64 = c.iter().map(|v| v.to_number() as i64).sum();
    let mut num = 1_i64;
    for k in 2..=n { num = num.saturating_mul(k); }
    let mut denom = 1_i64;
    for ci in &c {
        let mut f = 1_i64;
        for k in 2..=(ci.to_number() as i64) { f = f.saturating_mul(k); }
        denom = denom.saturating_mul(f.max(1));
    }
    if denom == 0 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(num / denom))
}

/// Size of RSK image for permutations of [n] = n!
fn builtin_rsk_size(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let mut f = 1_i64;
    for k in 2..=n { f = f.saturating_mul(k); }
    Ok(StrykeValue::integer(f))
}

/// Character of su(2) spin-j on rotation θ: sin((2j+1)θ/2) / sin(θ/2)
fn builtin_character_su2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let j = f1(args);
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let denom = (theta / 2.0).sin();
    if denom.abs() < 1e-12 { return Ok(StrykeValue::float(2.0 * j + 1.0)); }
    Ok(StrykeValue::float(((2.0 * j + 1.0) * theta / 2.0).sin() / denom))
}

/// Character of fundamental rep of SU(N) on diagonal element x
fn builtin_character_sun(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(n as f64 * x))
}

/// Quantum dimension of su(2) spin-j at q: [2j+1]_q = (q^(j+1/2) - q^-(j+1/2))/(q^(1/2) - q^-(1/2))
fn builtin_quantum_dimension_su2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let j = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(1.5);
    let two_j_plus_1 = 2.0 * j + 1.0;
    let denom = q.sqrt() - 1.0 / q.sqrt();
    if denom.abs() < 1e-12 { return Ok(StrykeValue::float(two_j_plus_1)); }
    Ok(StrykeValue::float((q.powf(two_j_plus_1 / 2.0) - q.powf(-two_j_plus_1 / 2.0)) / denom))
}

/// Quantum integer [n]_q = (qⁿ - q⁻ⁿ) / (q - q⁻¹)
fn builtin_quantum_dimension_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    let denom = q - 1.0 / q;
    if denom.abs() < 1e-12 { return Ok(StrykeValue::float(n)); }
    Ok(StrykeValue::float((q.powf(n) - q.powf(-n)) / denom))
}

/// One step of fusion rule N^c_{ab} for SU(2)_k: 1 if |a-b| ≤ c ≤ min(a+b, 2k - a - b) and parity ok
fn builtin_fusion_rule_su2_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let c = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let k = args.get(3).map(|v| v.to_number() as i64).unwrap_or(1);
    let lo = (a - b).abs();
    let hi = (a + b).min(2 * k - a - b);
    let ok = c >= lo && c <= hi && (a + b + c) % 2 == 0;
    Ok(StrykeValue::integer(if ok { 1 } else { 0 }))
}

/// Modular S-matrix entry S_{a,b} for SU(2)_k
fn builtin_modular_data_s_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = k + 2.0;
    let pre = (2.0 / n).sqrt();
    Ok(StrykeValue::float(pre * ((a + 1.0) * (b + 1.0) * std::f64::consts::PI / n).sin()))
}

/// Modular T-matrix diagonal entry T_{a,a} = exp(2πi (h_a - c/24))
fn builtin_modular_data_t_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((2.0 * std::f64::consts::PI * (h - c / 24.0)).cos()))
}

/// Verlinde formula for SU(2)_k WZW conformal blocks on a genus-g surface with
/// no marked points (closed): dim V_g(SU(2)_k) =
///   ((k+2)/2)^{g-1} · Σ_{j=1}^{k+1} (sin(jπ/(k+2)))^{2−2g}.
/// Args: genus g, level k.
fn builtin_verlinde_count_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g = i1(args).max(0);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let n = (k + 2) as f64;
    let exponent = 2 - 2 * g;
    let mut s = 0.0_f64;
    for j in 1..=(k + 1) {
        let sj = (j as f64 * std::f64::consts::PI / n).sin();
        if sj.abs() < 1e-15 { continue; }
        s += sj.powi(exponent as i32);
    }
    Ok(StrykeValue::float((n / 2.0).powi((g - 1) as i32) * s))
}

/// Evaluate a quantum knot invariant given as Laurent polynomial in q.
/// Args: array of coefficients [c_min_pow, c_{min+1}, ...], min_pow, q.
/// Computes Σ c_k · q^(min_pow + k).
fn builtin_quantum_invariant_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let coefs = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let min_pow = args.get(1).map(|v| v.to_number() as i32).unwrap_or(0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let s: f64 = coefs.iter().enumerate()
        .map(|(k, c)| c.to_number() * q.powi(min_pow + k as i32))
        .sum();
    Ok(StrykeValue::float(s))
}

/// Number of basic operations in a binary operad with 2 generators = 2^n
fn builtin_operad_count_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0) as u32;
    Ok(StrykeValue::integer(1_i64 << n.min(62)))
}

/// Dimension of moduli space of genus-g curves with n marked points: 3g - 3 + n
fn builtin_moduli_dimension_curves(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(3 * g - 3 + n))
}

/// Hodge polynomial Σ h^{p,q} u^p v^q evaluated at (u, v) = (1, 1) returns Σ h^{p,q}
fn builtin_hodge_polynomial_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().map(|x| x.to_number()).sum()))
}

/// Mirror symmetry check: h^{p,q}(M) == h^{n-p,q}(M̃)
fn builtin_mirror_symmetry_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = f1(args);
    let h_mirror = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if (h - h_mirror).abs() < 1e-9 { 1 } else { 0 }))
}

/// Genus-0 Gromov-Witten invariant N_d of ℙ²: number of rational degree-d curves
/// through 3d-1 generic points. Kontsevich recursion:
///   N_d = Σ_{d₁+d₂=d, d₁,d₂≥1} N_{d₁}·N_{d₂}·d₁²·d₂·[d₂·C(3d-4, 3d₁-2) - d₁·C(3d-4, 3d₁-1)]
/// Initial values: N_1 = 1 (the line through 2 points). Args: degree d.
fn builtin_gromov_witten_invariant(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d = i1(args).max(1) as usize;
    let mut n: Vec<f64> = vec![0.0; d + 1];
    n[1] = 1.0;
    fn binom(n: i64, k: i64) -> f64 {
        if k < 0 || k > n { return 0.0; }
        let k = k.min(n - k);
        let mut r = 1.0_f64;
        for i in 0..k { r *= (n - i) as f64 / (i + 1) as f64; }
        r
    }
    for dd in 2..=d {
        let dd_i = dd as i64;
        let mut s = 0.0_f64;
        for d1 in 1..dd {
            let d2 = dd - d1;
            let d1_f = d1 as f64;
            let d2_f = d2 as f64;
            let term = n[d1] * n[d2] * d1_f * d1_f * d2_f
                * (d2_f * binom(3 * dd_i - 4, 3 * d1 as i64 - 2)
                   - d1_f * binom(3 * dd_i - 4, 3 * d1 as i64 - 1));
            s += term;
        }
        n[dd] = s;
    }
    Ok(StrykeValue::float(n[d]))
}

/// Donaldson series for a 4-manifold X of simple type (Witten conjecture):
///   D_X(α) = exp(Q(α)/2) · Σ_s SW(s) · exp(⟨s, α⟩)
/// Args: Q(α,α) intersection form value, then pairs of (basic class pairing ⟨s, α⟩,
/// SW value at s). Returns scalar D_X(α).
fn builtin_donaldson_invariant(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q = f1(args);
    let pairings = arg_to_vec(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let sw_vals = arg_to_vec(args.get(2).unwrap_or(&StrykeValue::array(vec![])));
    let n = pairings.len().min(sw_vals.len());
    let prefactor = (q / 2.0).exp();
    let sum: f64 = (0..n).map(|i| sw_vals[i].to_number() * pairings[i].to_number().exp()).sum();
    Ok(StrykeValue::float(prefactor * sum))
}

/// Seiberg-Witten invariant SW(s) for Kähler surface X of general type. For
/// canonical class K_X with c₁²(K) > 0: SW(±K) = ±1, all other spin-c are 0.
/// For elliptic surface E(n) (n ≥ 2): SW(s) = ±C(n-2, k) · Δ_E(t)|_{t=ξ} pattern.
/// Args: c1_squared, k_dot_c1 (pairing with canonical class), surface_type
/// (0=Kähler general type, 1=elliptic E(n), 2=other).
fn builtin_seiberg_witten_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c1_sq = f1(args);
    let k_pair = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let surf = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    if surf == 0 {
        if c1_sq <= 0.0 { return Ok(StrykeValue::float(0.0)); }
        if (k_pair - c1_sq).abs() < 1e-9 { return Ok(StrykeValue::float(1.0)); }
        if (k_pair + c1_sq).abs() < 1e-9 { return Ok(StrykeValue::float(-1.0)); }
        Ok(StrykeValue::float(0.0))
    } else if surf == 1 {
        let n = args.get(3).map(|v| v.to_number() as i64).unwrap_or(2).max(2);
        let k_idx = (k_pair as i64).abs();
        if k_idx > (n - 2).max(0) { return Ok(StrykeValue::float(0.0)); }
        let mut binom = 1.0_f64;
        for i in 0..k_idx { binom *= (n - 2 - i) as f64 / (i + 1) as f64; }
        Ok(StrykeValue::float(if k_pair >= 0.0 { binom } else { -binom }))
    } else {
        Ok(StrykeValue::float(0.0))
    }
}

/// Heegaard Floer hat-rank ĤF(Y) for closed 3-manifolds:
///   ĤF(S³) = ℤ (rank 1).
///   ĤF(L(p, q)) = ℤ^p (rank p, lens space).
///   ĤF(Σ_g × S¹) = (genus-related polynomial, rank = Σ C(2g, k)² ≈ 2^{2g}).
///   ĤF(#ⁿ S¹×S²) = (Λ*H¹) ⊗ ℤ — rank 2^n.
/// Args: manifold_type (0=S³, 1=lens L(p,q), 2=Σ_g×S¹, 3=#ⁿS¹×S²), parameter.
fn builtin_floer_homology_rank(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mtype = i1(args);
    let p = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    match mtype {
        0 => Ok(StrykeValue::integer(1)),
        1 => Ok(StrykeValue::integer(p)),
        2 => {
            let g = p;
            let mut s = 0_i64;
            for k in 0..=2 * g {
                let mut bin = 1_i64;
                for i in 0..k { bin = bin.saturating_mul(2 * g - i) / (i + 1); }
                s = s.saturating_add(bin * bin);
            }
            Ok(StrykeValue::integer(s))
        },
        3 => Ok(StrykeValue::integer(1_i64 << p.min(62))),
        _ => Ok(StrykeValue::integer(1)),
    }
}

/// Rasmussen s-invariant. For positive braid closure (incl. T(p, q) torus knots):
///   s(K) = -σ(K) (Rasmussen 2010 for positive knots; for T(p,q): s = (p-1)(q-1)).
/// Args: knot_type (0=positive braid use σ, 1=torus T(p,q)), σ (or p), q (if torus).
fn builtin_khovanov_rasmussen_s(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kind = i1(args);
    if kind == 1 {
        let p = args.get(1).map(|v| v.to_number() as i64).unwrap_or(2).max(2);
        let q = args.get(2).map(|v| v.to_number() as i64).unwrap_or(3).max(2);
        return Ok(StrykeValue::integer((p - 1) * (q - 1)));
    }
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(-sigma))
}

/// Ozsváth-Szabó τ invariant. For torus knot T(p, q): τ(T(p,q)) = (p-1)(q-1)/2.
/// For knots with τ = g₄(K) (e.g. positive knots), τ realizes the slice genus.
/// Args: knot_type (0=generic, 1=torus T(p,q)), and either g₄ or (p, q).
fn builtin_ozsvath_szabo_tau(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kind = i1(args);
    if kind == 1 {
        let p = args.get(1).map(|v| v.to_number() as i64).unwrap_or(2).max(2);
        let q = args.get(2).map(|v| v.to_number() as i64).unwrap_or(3).max(2);
        return Ok(StrykeValue::float(((p - 1) * (q - 1)) as f64 / 2.0));
    }
    let g4 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(g4))
}

/// Heegaard genus lower bound: g(M) ≥ rank(H₁(M)) (Heegaard splittings descend to π₁).
fn builtin_heegaard_genus_lower(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rank_h1 = i1(args).max(0);
    Ok(StrykeValue::integer(rank_h1))
}

/// Fintushel-Stern knot-surgery formula: SW(X_K) = SW(X) · Δ_K(t²)|_{t = exp(2t·c)}
/// expanded as Laurent polynomial in t. Compute Δ_K(t²) at parameter q from
/// symmetric Alexander polynomial coefficients [a_n, ..., a_1, a_0, a_1, ..., a_n].
/// Args: SW(X), array of Alexander coefficients (lower triangle), evaluation parameter q.
fn builtin_fintushel_stern_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sw_x = f1(args);
    let coefs = arg_to_vec(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let mut alex = 0.0_f64;
    for (k, c) in coefs.iter().enumerate() {
        let cv = c.to_number();
        let pw = q.powi(2 * k as i32);
        if k == 0 { alex += cv; } else { alex += cv * (pw + 1.0 / pw); }
    }
    Ok(StrykeValue::float(sw_x * alex))
}

/// Bauer-Furuta degree refinement: for 4-manifold X with b₁ = 0 and b₂⁺ = 1,
/// BF map gives a stable cohomotopy class refining SW. The refined "Bauer-Furuta
/// invariant" reduces to SW × ε where ε is a sign from the Furuta inequality
/// 10/8: b₂(X) ≥ (10/8)|σ(X)| + 2 (when X is spin closed, σ ≡ 0 mod 16). Args:
/// b₂(X), σ(X), spin (1 if spin else 0).
fn builtin_bauer_furuta_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let b2 = f1(args);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let spin = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    if spin == 1 {
        let lhs = b2;
        let rhs = (10.0 / 8.0) * sigma.abs() + 2.0;
        return Ok(StrykeValue::integer(if lhs >= rhs { 1 } else { 0 }));
    }
    Ok(StrykeValue::float(sigma / 16.0))
}

/// Geometric intersection number i(α, β) for curves on a surface
fn builtin_geometric_intersection_number(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = arg_to_vec(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let s: f64 = a.iter().zip(b.iter()).map(|(x, y)| (x.to_number() * y.to_number()).abs()).sum();
    Ok(StrykeValue::float(s))
}

/// Algebraic intersection number ⟨α, β⟩ (signed)
fn builtin_algebraic_intersection_number(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = arg_to_vec(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let s: f64 = a.iter().zip(b.iter()).map(|(x, y)| x.to_number() * y.to_number()).sum();
    Ok(StrykeValue::float(s))
}
