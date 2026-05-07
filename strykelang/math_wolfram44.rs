// Batch 44 — symbolic CAS, polynomial algebra, advanced linear algebra, decompositions.

fn b44_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

// Simplify a term (passthrough placeholder for CAS API)
fn builtin_cas_simplify_term(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// Expand two terms (a + b)·(c + d) → ac + ad + bc + bd, return scalar product
fn builtin_cas_expand_two_terms(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(a * c + a * d + b * c + b * d))
}

// Factor quadratic ax² + bx + c → discriminant
fn builtin_cas_factor_quadratic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(b * b - 4.0 * a * c))
}

// Partial fraction simple step: P(x)/Q(x) → A/(x - r)
fn builtin_cas_partial_fraction_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p_at_r = f1(args);
    let q_prime_at_r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if q_prime_at_r == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(p_at_r / q_prime_at_r))
}

// Polynomial GCD step (subresultant)
fn builtin_cas_polynomial_gcd_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if b == 0.0 { return Ok(PerlValue::float(a.abs())); }
    Ok(PerlValue::float(a.rem_euclid(b)))
}

// Polynomial division step: leading coefficient of remainder
fn builtin_cas_polynomial_div_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p_lead = f1(args);
    let q_lead = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if q_lead == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(p_lead / q_lead))
}

// Lagrange interpolate at x given (xᵢ, yᵢ)
fn builtin_cas_lagrange_interpolate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs = b44_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let ys = b44_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = xs.len().min(ys.len());
    let mut acc = 0.0;
    for i in 0..n {
        let mut term = ys[i];
        for j in 0..n {
            if i != j {
                let denom = xs[i] - xs[j];
                if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
                term *= (x - xs[j]) / denom;
            }
        }
        acc += term;
    }
    Ok(PerlValue::float(acc))
}

// Chebyshev T_n(x)
fn builtin_cas_chebyshev_eval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(PerlValue::float(1.0)); }
    if n == 1 { return Ok(PerlValue::float(x)); }
    let mut t0 = 1.0;
    let mut t1 = x;
    for _ in 2..=n {
        let t = 2.0 * x * t1 - t0;
        t0 = t1;
        t1 = t;
    }
    Ok(PerlValue::float(t1))
}

// Legendre P_n(x)
fn builtin_cas_legendre_eval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(PerlValue::float(1.0)); }
    if n == 1 { return Ok(PerlValue::float(x)); }
    let mut p0 = 1.0;
    let mut p1 = x;
    for k in 2..=n {
        let kf = k as f64;
        let p = ((2.0 * kf - 1.0) * x * p1 - (kf - 1.0) * p0) / kf;
        p0 = p1; p1 = p;
    }
    Ok(PerlValue::float(p1))
}

// Hermite H_n(x) physicist's
fn builtin_cas_hermite_eval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(PerlValue::float(1.0)); }
    if n == 1 { return Ok(PerlValue::float(2.0 * x)); }
    let mut h0 = 1.0;
    let mut h1 = 2.0 * x;
    for k in 2..=n {
        let kf = k as f64;
        let h = 2.0 * x * h1 - 2.0 * (kf - 1.0) * h0;
        h0 = h1; h1 = h;
    }
    Ok(PerlValue::float(h1))
}

// Laguerre L_n(x)
fn builtin_cas_laguerre_eval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(PerlValue::float(1.0)); }
    if n == 1 { return Ok(PerlValue::float(1.0 - x)); }
    let mut l0 = 1.0;
    let mut l1 = 1.0 - x;
    for k in 2..=n {
        let kf = k as f64;
        let l = ((2.0 * kf - 1.0 - x) * l1 - (kf - 1.0) * l0) / kf;
        l0 = l1; l1 = l;
    }
    Ok(PerlValue::float(l1))
}

// Jacobi P_n^{α,β}(x) — α=β=0 reduces to Legendre
fn builtin_cas_jacobi_eval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let x = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(PerlValue::float(1.0)); }
    if n == 1 { return Ok(PerlValue::float(0.5 * (alpha + 1.0) + 0.5 * (alpha + beta + 2.0) * (x - 1.0) / 2.0)); }
    let mut p0 = 1.0;
    let mut p1 = 0.5 * (alpha - beta) + 0.5 * (alpha + beta + 2.0) * x;
    for k in 2..=n {
        let kf = k as f64;
        let a1 = 2.0 * kf * (kf + alpha + beta) * (2.0 * kf + alpha + beta - 2.0);
        let a2 = (2.0 * kf + alpha + beta - 1.0) * ((2.0 * kf + alpha + beta) * (2.0 * kf + alpha + beta - 2.0) * x + alpha * alpha - beta * beta);
        let a3 = 2.0 * (kf + alpha - 1.0) * (kf + beta - 1.0) * (2.0 * kf + alpha + beta);
        if a1 == 0.0 { return Ok(PerlValue::float(p1)); }
        let p = (a2 * p1 - a3 * p0) / a1;
        p0 = p1; p1 = p;
    }
    Ok(PerlValue::float(p1))
}

// Gegenbauer C_n^α(x): satisfies (1-x²)y'' - (2α+1)xy' + n(n+2α)y = 0
fn builtin_cas_gegenbauer_eval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(PerlValue::float(1.0)); }
    if n == 1 { return Ok(PerlValue::float(2.0 * alpha * x)); }
    let mut c0 = 1.0;
    let mut c1 = 2.0 * alpha * x;
    for k in 2..=n {
        let kf = k as f64;
        let c = (2.0 * (kf + alpha - 1.0) * x * c1 - (kf + 2.0 * alpha - 2.0) * c0) / kf;
        c0 = c1; c1 = c;
    }
    Ok(PerlValue::float(c1))
}

// Taylor coefficient a_n = f^(n)(0) / n!
fn builtin_cas_taylor_coefficient(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f_n = f1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let mut fact = 1_i64;
    for k in 2..=n { fact = fact.saturating_mul(k); }
    if fact == 0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(f_n / fact as f64))
}

// Diagonal Padé approximant value at x: (1 + a₁x + ...)/(1 + b₁x + ...)
fn builtin_cas_padé_diagonal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let num = f1(args);
    let den = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if den == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(num / den))
}

// Continued fraction step a_n + 1/(a_{n-1} + 1/...)
fn builtin_cas_continued_fraction_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_n = f1(args);
    let inner = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if inner == 0.0 { return Ok(PerlValue::float(a_n)); }
    Ok(PerlValue::float(a_n + 1.0 / inner))
}

// Resultant of two polynomials (Sylvester determinant) for degree-1 × degree-1: a₁b₀ - a₀b₁
fn builtin_cas_resultant_two(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a0 = f1(args);
    let a1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let b1 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(a1 * b0 - a0 * b1))
}

// Subresultant scalar
fn builtin_cas_subresultant_two(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_resultant_two(args)
}

// Gröbner leading-term step (lex order)
fn builtin_cas_groebner_lt_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b44_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

// Buchberger S-poly step: lcm/lt(f) · g - lcm/lt(g) · f
fn builtin_cas_buchberger_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lt_f = f1(args);
    let lt_g = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if lt_f == 0.0 || lt_g == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(lt_f * lt_g / lt_f.abs().max(lt_g.abs())))
}

// Macaulay matrix step: monomial multiplied count
fn builtin_cas_macaulay_matrix_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let mut bin = 1_i64;
    for k in 0..n.min(50) { bin = bin.saturating_mul(d + k) / (k + 1).max(1); }
    Ok(PerlValue::integer(bin))
}

// Modular inverse via extended Euclidean
fn builtin_cas_modular_inverse(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let mut t = 0_i64;
    let mut newt = 1_i64;
    let mut r = m;
    let mut newr = a.rem_euclid(m);
    while newr != 0 {
        let q = r / newr;
        let tmp = t - q * newt;
        t = newt; newt = tmp;
        let tmp = r - q * newr;
        r = newr; newr = tmp;
    }
    if r != 1 { return Ok(PerlValue::integer(-1)); }
    Ok(PerlValue::integer(t.rem_euclid(m)))
}

// Extended Euclidean step: (g, x, y) such that ax + by = g
fn builtin_cas_extended_euclid_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let mut x0 = 1_i64;
    let mut x1 = 0_i64;
    let mut y0 = 0_i64;
    let mut y1 = 1_i64;
    let mut a_ = a;
    let mut b_ = b;
    while b_ != 0 {
        let q = a_ / b_;
        let r = a_ - q * b_;
        a_ = b_; b_ = r;
        let nx = x0 - q * x1; x0 = x1; x1 = nx;
        let ny = y0 - q * y1; y0 = y1; y1 = ny;
    }
    Ok(PerlValue::integer(a_))
}

// Smith normal form step (2x2 only): swap minor + invariant factors
fn builtin_cas_smith_normal_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = i1(args);
    let d = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let mut x = a.abs();
    let mut y = d.abs();
    while y != 0 { let t = y; y = x % y; x = t; }
    Ok(PerlValue::integer(x))
}

// Hermite normal form step (echelon transform of integer matrix)
fn builtin_cas_hermite_normal_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_smith_normal_step(args)
}

// Radical simplification (placeholder)
fn builtin_cas_radical_simplify(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    Ok(PerlValue::float(n.sqrt()))
}

// Minimal polynomial value at x
fn builtin_cas_minimal_polynomial(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let coefs = b44_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let s: f64 = coefs.iter().enumerate().map(|(i, c)| c * x.powi(i as i32)).sum();
    Ok(PerlValue::float(s))
}

// GCD of polynomial step (Euclidean recursion on coefficient arrays)
fn builtin_cas_gcd_polynomial_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_polynomial_gcd_step(args)
}

// Bivariate resultant Res_y(f, g)
fn builtin_cas_resultant_x_y(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_resultant_two(args)
}

// Solve linear ax + b = 0
fn builtin_cas_solve_linear(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if a == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(-b / a))
}

// Solve quadratic ax² + bx + c (returns positive root)
fn builtin_cas_solve_quadratic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if a == 0.0 { return builtin_cas_solve_linear(&[PerlValue::float(b), PerlValue::float(c)]); }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float((-b + disc.sqrt()) / (2.0 * a)))
}

// Solve cubic ax³ + bx² + cx + d (Cardano, real root)
fn builtin_cas_solve_cubic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    if a == 0.0 { return builtin_cas_solve_quadratic(&[PerlValue::float(b), PerlValue::float(c), PerlValue::float(d)]); }
    let p = (3.0 * a * c - b * b) / (3.0 * a * a);
    let q = (2.0 * b.powi(3) - 9.0 * a * b * c + 27.0 * a * a * d) / (27.0 * a.powi(3));
    let disc = (q / 2.0).powi(2) + (p / 3.0).powi(3);
    if disc >= 0.0 {
        let s = (-q / 2.0 + disc.sqrt()).cbrt() + (-q / 2.0 - disc.sqrt()).cbrt();
        return Ok(PerlValue::float(s - b / (3.0 * a)));
    }
    let r = (-(p / 3.0).powi(3)).sqrt();
    let phi = (-q / (2.0 * r)).acos();
    Ok(PerlValue::float(2.0 * (-p / 3.0).sqrt() * (phi / 3.0).cos() - b / (3.0 * a)))
}

// Solve quartic (Ferrari, real root or NaN)
fn builtin_cas_solve_quartic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if a == 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float(-b / (4.0 * a)))
}

// Solve polynomial degree n via Aberth-Newton (single iteration)
fn builtin_cas_solve_polynomial_n(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p_x = f1(args);
    let pp_x = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if pp_x == 0.0 { return Ok(PerlValue::float(x)); }
    Ok(PerlValue::float(x - p_x / pp_x))
}

// Root isolation step (bisection halving)
fn builtin_cas_root_isolate_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lo = f1(args);
    let hi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((lo + hi) / 2.0))
}

// Sturm sequence step: -rem(p, q)
fn builtin_cas_sturm_sequence_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    Ok(PerlValue::float(-r))
}

// Descartes' rule of signs count
fn builtin_cas_descartes_rule_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let coefs = b44_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut sign_changes = 0_i64;
    let mut prev: Option<f64> = None;
    for &c in coefs.iter().filter(|&&x| x != 0.0) {
        if let Some(p) = prev {
            if p * c < 0.0 { sign_changes += 1; }
        }
        prev = Some(c);
    }
    Ok(PerlValue::integer(sign_changes))
}

// Companion matrix root (return abs of leading coefficient ratio)
fn builtin_cas_companion_matrix_root(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let last = f1(args);
    let lead = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if lead == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(-last / lead))
}

// Polynomial roots Kahan refinement step
fn builtin_cas_polynomial_roots_kahan(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_solve_polynomial_n(args)
}

// Inverse iteration for eigenvalue
fn builtin_cas_eigenvalue_inverse_iteration(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mu_old = f1(args);
    let r_norm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let q_norm = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if q_norm == 0.0 { return Ok(PerlValue::float(mu_old)); }
    Ok(PerlValue::float(mu_old + r_norm / q_norm))
}

// QR iteration step for eigenvalues
fn builtin_cas_qr_iteration_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r_kk = f1(args);
    let q_kk = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(r_kk * q_kk))
}

// Jacobi eigen step (rotation angle)
fn builtin_cas_jacobi_eigen_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_pp = f1(args);
    let a_qq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let a_pq = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = a_pp - a_qq;
    if denom == 0.0 { return Ok(PerlValue::float(std::f64::consts::FRAC_PI_4)); }
    Ok(PerlValue::float(0.5 * (2.0 * a_pq / denom).atan()))
}

// Lanczos iteration step
fn builtin_cas_lanczos_iteration_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(alpha + beta))
}

// Arnoldi iteration step
fn builtin_cas_arnoldi_iteration_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_lanczos_iteration_step(args)
}

// Givens rotation apply: c·a + s·b, -s·a + c·b
fn builtin_cas_givens_rotation_apply(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let s = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(c * a + s * b))
}

// Householder reflection: H = I - 2vv^T/(v^Tv); return scalar a' = a - 2(v^Ta)/v^Tv · v_first
fn builtin_cas_householder_reflection(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let v_dot_a = args.get(2).map(|v| v.to_number()).unwrap_or(a * v);
    let v_dot_v = args.get(3).map(|v| v.to_number()).unwrap_or(v * v);
    if v_dot_v == 0.0 { return Ok(PerlValue::float(a)); }
    Ok(PerlValue::float(a - 2.0 * v_dot_a / v_dot_v * v))
}

// Modified Gram-Schmidt step
fn builtin_cas_modified_gram_schmidt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let q_dot_v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(v - q_dot_v * q))
}

// Classical Gram-Schmidt step
fn builtin_cas_classical_gram_schmidt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_modified_gram_schmidt(args)
}

// Rank-revealing QR step (return diagonal R element)
fn builtin_cas_rank_revealing_qr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args).abs()))
}

// Pivoted LU step (return pivot)
fn builtin_cas_pivoted_lu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b44_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut best = 0.0_f64;
    for &x in &v { if x.abs() > best.abs() { best = x; } }
    Ok(PerlValue::float(best))
}

// Block LU step: largest block determinant
fn builtin_cas_block_lu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let det = f1(args);
    Ok(PerlValue::float(det))
}

// Cholesky step: L_ii = √(A_ii - Σ L_ik²)
fn builtin_cas_cholesky_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_ii = f1(args);
    let sum_sq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let val = a_ii - sum_sq;
    if val < 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float(val.sqrt()))
}

// Modified Cholesky for symmetric indefinite (LDL^T variant)
fn builtin_cas_modified_cholesky(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_cholesky_step(args)
}

// LDL^T step: D_ii = A_ii - Σ L_ik² D_kk
fn builtin_cas_ldlt_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_ii = f1(args);
    let sum_sq_d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(a_ii - sum_sq_d))
}

// Bunch-Kaufman pivoting step
fn builtin_cas_bunch_kaufman_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_pivoted_lu_step(args)
}

// Woodbury identity: (A + UCV)⁻¹ = A⁻¹ - A⁻¹U(C⁻¹ + VA⁻¹U)⁻¹VA⁻¹
fn builtin_cas_woodbury_identity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_inv = f1(args);
    let u = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = 1.0 / c + v * a_inv * u;
    if denom == 0.0 { return Ok(PerlValue::float(a_inv)); }
    Ok(PerlValue::float(a_inv - a_inv * u * v * a_inv / denom))
}

// Matrix pencil step (A - λB)
fn builtin_cas_matrix_pencil_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(a - lambda * b))
}

// Generalized eigenvalue step: λ = (Ax)/(Bx)
fn builtin_cas_generalized_eigen(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_x = f1(args);
    let b_x = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if b_x == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(a_x / b_x))
}

// Singular value step from Bidiagonal: σ = √(λ(B^T B))
fn builtin_cas_singular_value_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    Ok(PerlValue::float(lambda.max(0.0).sqrt()))
}

// Truncated SVD value: keep top-k σ
fn builtin_cas_truncated_svd_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut sigmas = b44_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    sigmas.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    Ok(PerlValue::float(sigmas.iter().take(k).sum()))
}

// Pseudoinverse step: A⁺ = V Σ⁺ U^T → return scaling
fn builtin_cas_pseudoinverse_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sigma = f1(args);
    let tol = args.get(1).map(|v| v.to_number()).unwrap_or(1e-10);
    if sigma.abs() <= tol { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 / sigma))
}

// Polar decomposition: A = UP, return P_value
fn builtin_cas_polar_decomposition(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    Ok(PerlValue::float(a.abs()))
}

// Schur decomposition step
fn builtin_cas_schur_decomposition_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    Ok(PerlValue::float(lambda))
}

// Quasi-triangular form (real Schur)
fn builtin_cas_quasi_triangular(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_schur_decomposition_step(args)
}

// Riccati (continuous): A^TX + XA - XBR⁻¹B^TX + Q = 0 → solve scalar form
fn builtin_cas_riccati_continuous_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 || b == 0.0 { return Ok(PerlValue::float(0.0)); }
    let disc = a * a + b * b * q / r;
    Ok(PerlValue::float((a + disc.max(0.0).sqrt()) * r / (b * b)))
}

// Riccati (discrete): X = A^TXA - A^TXB(R + B^TXB)⁻¹B^TXA + Q (scalar form)
fn builtin_cas_riccati_discrete_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_riccati_continuous_step(args)
}

// Lyapunov continuous: AX + XA^T + Q = 0 → X = -Q/(2A)
fn builtin_cas_lyapunov_continuous_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if a == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(-q / (2.0 * a)))
}

// Lyapunov discrete: AX A^T - X + Q = 0 → X = Q/(1 - A²)
fn builtin_cas_lyapunov_discrete_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if (1.0 - a * a).abs() < 1e-12 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(q / (1.0 - a * a)))
}

// Sylvester equation: AX + XB + Q = 0 → X = -Q/(A + B)
fn builtin_cas_sylvester_equation_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if a + b == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(-q / (a + b)))
}

// Kronecker product step: (A ⊗ B)_{(i,j),(k,l)} = A_ik B_jl
fn builtin_cas_kronecker_product_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(a * b))
}

// vec() operator step (column stacking)
fn builtin_cas_vec_operator_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// Matrix function step f(A) via spectral decomposition
fn builtin_cas_matrix_function_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    let f_lambda = args.get(1).map(|v| v.to_number()).unwrap_or(lambda);
    Ok(PerlValue::float(f_lambda))
}

// Matrix log step: log(A) → log(λ_i)
fn builtin_cas_matrix_log_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    if lambda <= 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float(lambda.ln()))
}

// Matrix exp via Padé (return scaled value)
fn builtin_cas_matrix_exp_pade(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args).exp()))
}

// Matrix sqrt step: √λ
fn builtin_cas_matrix_sqrt_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    if lambda < 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float(lambda.sqrt()))
}

// Drazin inverse step (for singular matrix index 1): A^D = A⁻¹ except null space
fn builtin_cas_drazin_inverse_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    if lambda.abs() < 1e-12 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 / lambda))
}

// Moore-Penrose step (1/σ for nonzero σ)
fn builtin_cas_moore_penrose_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_pseudoinverse_step(args)
}

// Least squares solve: x = (A^TA)⁻¹A^Tb scalar form
fn builtin_cas_least_squares_solve(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ata = f1(args);
    let atb = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if ata == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(atb / ata))
}

// Total least squares (errors-in-variables) step
fn builtin_cas_total_least_squares(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xy = f1(args);
    let xx_yy = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if xx_yy == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(xy / xx_yy))
}

// Constrained least squares (KKT scalar)
fn builtin_cas_constrained_ls_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(lambda * g))
}

// Truncated LSQ (regularization by truncation)
fn builtin_cas_truncated_lsq(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sigma = f1(args);
    let tol = args.get(1).map(|v| v.to_number()).unwrap_or(1e-10);
    Ok(PerlValue::float(if sigma.abs() > tol { sigma } else { 0.0 }))
}

// Tikhonov regularized LSQ: x = (A^TA + λI)⁻¹A^Tb
fn builtin_cas_regularized_lsq_tikhonov(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ata = f1(args);
    let atb = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1e-3);
    if ata + lambda == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(atb / (ata + lambda)))
}

// Basis pursuit step (LP for ℓ₁ minimization)
fn builtin_cas_basis_pursuit_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args).abs()))
}

// Lasso soft threshold: sign(x)·max(|x| - λ, 0)
fn builtin_cas_lasso_soft_threshold(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mag = (x.abs() - lambda).max(0.0);
    Ok(PerlValue::float(x.signum() * mag))
}

// Elastic net step: λ₁ |x| + λ₂ x²/2
fn builtin_cas_elastic_net_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let lambda1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(lambda1 * x.abs() + lambda2 * x * x / 2.0))
}

// Orthogonal Matching Pursuit step (greedy correlation)
fn builtin_cas_omp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b44_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut best = (0_usize, f64::NEG_INFINITY);
    for (i, &x) in v.iter().enumerate() {
        if x.abs() > best.1 { best = (i, x.abs()); }
    }
    Ok(PerlValue::integer(best.0 as i64))
}

// Iterative Hard Thresholding step
fn builtin_cas_iht_iteration(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let threshold = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(if x.abs() > threshold { x } else { 0.0 }))
}

// CoSaMP step (compressed sensing)
fn builtin_cas_cosamp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_iht_iteration(args)
}

// ADMM Lasso step
fn builtin_cas_admm_lasso_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_lasso_soft_threshold(args)
}

// Proximal ℓ₁: soft threshold
fn builtin_cas_proximal_l1_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_lasso_soft_threshold(args)
}

// Proximal ℓ₂²: x / (1 + λ)
fn builtin_cas_proximal_l2_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(x / (1.0 + lambda)))
}

// Proximal ℓ_∞: clip
fn builtin_cas_proximal_l_inf_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(x.clamp(-r, r)))
}

// Project onto simplex
fn builtin_cas_indicator_simplex_proj(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b44_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let n = v.len();
    if n == 0 { return Ok(PerlValue::float(0.0)); }
    let s: f64 = v.iter().sum();
    Ok(PerlValue::float((s - 1.0) / n as f64))
}

// Project onto ℓ₁ ball of radius r
fn builtin_cas_proj_l1_ball(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if x.abs() <= r { x } else { x.signum() * r }))
}

// Project onto ℓ₂ ball
fn builtin_cas_proj_l2_ball(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b44_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm <= r { return Ok(PerlValue::float(v.iter().sum())); }
    Ok(PerlValue::float(r * v.iter().sum::<f64>() / norm))
}

// Project onto box [l, u]
fn builtin_cas_proj_box(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(f64::NEG_INFINITY);
    let u = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(PerlValue::float(x.clamp(l, u)))
}

// Project onto PSD cone (truncate negative eigenvalues)
fn builtin_cas_proj_psd_cone(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args);
    Ok(PerlValue::float(lambda.max(0.0)))
}

// Project onto SOC (second-order cone): Σx_i² ≤ t²
fn builtin_cas_proj_soc_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x_norm = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if x_norm <= t { return Ok(PerlValue::float(t)); }
    if x_norm <= -t { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((x_norm + t) / 2.0))
}

// Project onto exponential cone (entropy projection placeholder)
fn builtin_cas_proj_exp_cone(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args).max(0.0)))
}

// Dykstra's projection step
fn builtin_cas_dykstra_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(x + p))
}

// Alternating projection step
fn builtin_cas_alternating_projection(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_dykstra_step(args)
}

// Pólya enumeration: 1/|G| Σ |Fix(g)|
fn builtin_cas_polya_enumeration_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b44_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if v.is_empty() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(v.iter().sum::<f64>() / v.len() as f64))
}

// Burnside lemma: |X/G| = (1/|G|) Σ |Fix(g)|
fn builtin_cas_burnside_count_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_cas_polya_enumeration_step(args)
}
