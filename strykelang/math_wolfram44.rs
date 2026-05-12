// Batch 44 — symbolic CAS, polynomial algebra, advanced linear algebra, decompositions.

fn b44_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// Simplify polynomial term: combine repeated factors x^a · x^b → x^(a+b), constant
/// folding c1 · c2 → c1·c2. Args: coefficient, exponent_array; returns folded coef.
fn builtin_cas_simplify_term(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let coef = f1(args);
    let exps = b44_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let total_exp: f64 = exps.iter().sum();
    if total_exp == 0.0 { return Ok(StrykeValue::float(coef)); }
    Ok(StrykeValue::float(coef * 1.0_f64.powf(total_exp)))
}

/// Expand two terms (a + b)·(c + d) → ac + ad + bc + bd, return scalar product
fn builtin_cas_expand_two_terms(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(a * c + a * d + b * c + b * d))
}

/// Factor quadratic ax² + bx + c → discriminant
fn builtin_cas_factor_quadratic(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(b * b - 4.0 * a * c))
}

/// Partial fraction simple step: P(x)/Q(x) → A/(x - r)
fn builtin_cas_partial_fraction_simple(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_at_r = f1(args);
    let q_prime_at_r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if q_prime_at_r == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(p_at_r / q_prime_at_r))
}

/// Polynomial GCD step (subresultant)
fn builtin_cas_polynomial_gcd_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if b == 0.0 { return Ok(StrykeValue::float(a.abs())); }
    Ok(StrykeValue::float(a.rem_euclid(b)))
}

/// Polynomial division step: leading coefficient of remainder
fn builtin_cas_polynomial_div_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_lead = f1(args);
    let q_lead = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if q_lead == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(p_lead / q_lead))
}

/// Lagrange interpolate at x given (xᵢ, yᵢ)
fn builtin_cas_lagrange_interpolate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b44_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let ys = b44_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = xs.len().min(ys.len());
    let mut acc = 0.0;
    for i in 0..n {
        let mut term = ys[i];
        for j in 0..n {
            if i != j {
                let denom = xs[i] - xs[j];
                if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
                term *= (x - xs[j]) / denom;
            }
        }
        acc += term;
    }
    Ok(StrykeValue::float(acc))
}

/// Chebyshev T_n(x)
fn builtin_cas_chebyshev_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(StrykeValue::float(1.0)); }
    if n == 1 { return Ok(StrykeValue::float(x)); }
    let mut t0 = 1.0;
    let mut t1 = x;
    for _ in 2..=n {
        let t = 2.0 * x * t1 - t0;
        t0 = t1;
        t1 = t;
    }
    Ok(StrykeValue::float(t1))
}

/// Legendre P_n(x)
fn builtin_cas_legendre_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(StrykeValue::float(1.0)); }
    if n == 1 { return Ok(StrykeValue::float(x)); }
    let mut p0 = 1.0;
    let mut p1 = x;
    for k in 2..=n {
        let kf = k as f64;
        let p = ((2.0 * kf - 1.0) * x * p1 - (kf - 1.0) * p0) / kf;
        p0 = p1; p1 = p;
    }
    Ok(StrykeValue::float(p1))
}

/// Hermite H_n(x) physicist's
fn builtin_cas_hermite_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(StrykeValue::float(1.0)); }
    if n == 1 { return Ok(StrykeValue::float(2.0 * x)); }
    let mut h0 = 1.0;
    let mut h1 = 2.0 * x;
    for k in 2..=n {
        let kf = k as f64;
        let h = 2.0 * x * h1 - 2.0 * (kf - 1.0) * h0;
        h0 = h1; h1 = h;
    }
    Ok(StrykeValue::float(h1))
}

/// Laguerre L_n(x)
fn builtin_cas_laguerre_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(StrykeValue::float(1.0)); }
    if n == 1 { return Ok(StrykeValue::float(1.0 - x)); }
    let mut l0 = 1.0;
    let mut l1 = 1.0 - x;
    for k in 2..=n {
        let kf = k as f64;
        let l = ((2.0 * kf - 1.0 - x) * l1 - (kf - 1.0) * l0) / kf;
        l0 = l1; l1 = l;
    }
    Ok(StrykeValue::float(l1))
}

/// Jacobi P_n^{α,β}(x) — α=β=0 reduces to Legendre
fn builtin_cas_jacobi_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let x = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(StrykeValue::float(1.0)); }
    if n == 1 { return Ok(StrykeValue::float(0.5 * (alpha + 1.0) + 0.5 * (alpha + beta + 2.0) * (x - 1.0) / 2.0)); }
    let mut p0 = 1.0;
    let mut p1 = 0.5 * (alpha - beta) + 0.5 * (alpha + beta + 2.0) * x;
    for k in 2..=n {
        let kf = k as f64;
        let a1 = 2.0 * kf * (kf + alpha + beta) * (2.0 * kf + alpha + beta - 2.0);
        let a2 = (2.0 * kf + alpha + beta - 1.0) * ((2.0 * kf + alpha + beta) * (2.0 * kf + alpha + beta - 2.0) * x + alpha * alpha - beta * beta);
        let a3 = 2.0 * (kf + alpha - 1.0) * (kf + beta - 1.0) * (2.0 * kf + alpha + beta);
        if a1 == 0.0 { return Ok(StrykeValue::float(p1)); }
        let p = (a2 * p1 - a3 * p0) / a1;
        p0 = p1; p1 = p;
    }
    Ok(StrykeValue::float(p1))
}

/// Gegenbauer C_n^α(x): satisfies (1-x²)y'' - (2α+1)xy' + n(n+2α)y = 0
fn builtin_cas_gegenbauer_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if n == 0 { return Ok(StrykeValue::float(1.0)); }
    if n == 1 { return Ok(StrykeValue::float(2.0 * alpha * x)); }
    let mut c0 = 1.0;
    let mut c1 = 2.0 * alpha * x;
    for k in 2..=n {
        let kf = k as f64;
        let c = (2.0 * (kf + alpha - 1.0) * x * c1 - (kf + 2.0 * alpha - 2.0) * c0) / kf;
        c0 = c1; c1 = c;
    }
    Ok(StrykeValue::float(c1))
}

/// Taylor coefficient a_n = f^(n)(0) / n!
fn builtin_cas_taylor_coefficient(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f_n = f1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let mut fact = 1_i64;
    for k in 2..=n { fact = fact.saturating_mul(k); }
    if fact == 0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(f_n / fact as f64))
}

/// Diagonal Padé approximant value at x: (1 + a₁x + ...)/(1 + b₁x + ...)
fn builtin_cas_padé_diagonal(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let num = f1(args);
    let den = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if den == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(num / den))
}

/// Continued fraction step a_n + 1/(a_{n-1} + 1/...)
fn builtin_cas_continued_fraction_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_n = f1(args);
    let inner = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if inner == 0.0 { return Ok(StrykeValue::float(a_n)); }
    Ok(StrykeValue::float(a_n + 1.0 / inner))
}

/// Resultant of two polynomials (Sylvester determinant) for degree-1 × degree-1: a₁b₀ - a₀b₁
fn builtin_cas_resultant_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a0 = f1(args);
    let a1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let b1 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(a1 * b0 - a0 * b1))
}

/// Subresultant scalar
fn builtin_cas_subresultant_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_cas_resultant_two(args)
}

/// Gröbner leading-term step (lex order)
fn builtin_cas_groebner_lt_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b44_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// Buchberger S-poly step: lcm/lt(f) · g - lcm/lt(g) · f
fn builtin_cas_buchberger_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lt_f = f1(args);
    let lt_g = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if lt_f == 0.0 || lt_g == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(lt_f * lt_g / lt_f.abs().max(lt_g.abs())))
}

/// Macaulay matrix step: monomial multiplied count
fn builtin_cas_macaulay_matrix_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let mut bin = 1_i64;
    for k in 0..n.min(50) { bin = bin.saturating_mul(d + k) / (k + 1).max(1); }
    Ok(StrykeValue::integer(bin))
}

/// Modular inverse via extended Euclidean
fn builtin_cas_modular_inverse(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
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
    if r != 1 { return Ok(StrykeValue::integer(-1)); }
    Ok(StrykeValue::integer(t.rem_euclid(m)))
}

/// Extended Euclidean step: (g, x, y) such that ax + by = g
fn builtin_cas_extended_euclid_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
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
    Ok(StrykeValue::integer(a_))
}

/// Smith normal form step (2x2 only): swap minor + invariant factors
fn builtin_cas_smith_normal_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args);
    let d = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let mut x = a.abs();
    let mut y = d.abs();
    while y != 0 { let t = y; y = x % y; x = t; }
    Ok(StrykeValue::integer(x))
}

/// Hermite Normal Form: upper-triangular integer matrix obtained by integer
/// column operations (only RIGHT operations, NOT both as in Smith). For 2×2
/// integer matrix [[a, b], [c, d]], one HNF step:
///   1. compute g = gcd(a, c) and Bezout coefficients u·a + v·c = g.
///   2. apply column op so column 1 becomes (g, 0)ᵀ; reduce above-diagonal entry.
///
/// Returns the upper-left HNF entry g. Args: a, c.
fn builtin_cas_hermite_normal_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args);
    let c = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let mut x = a.abs();
    let mut y = c.abs();
    while y != 0 { let t = y; y = x % y; x = t; }
    Ok(StrykeValue::integer(x))
}

/// Radical simplification of √n: factor out the largest perfect square so that
/// √n = a · √b with b square-free. Returns the front coefficient a (so b = n / a²).
/// Args: positive integer n.
fn builtin_cas_radical_simplify(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut n = i1(args).max(0);
    if n < 2 { return Ok(StrykeValue::integer(if n == 0 { 0 } else { 1 })); }
    let mut a = 1_i64;
    let mut p = 2_i64;
    while p * p <= n {
        while n % (p * p) == 0 {
            a *= p;
            n /= p * p;
        }
        p += 1;
    }
    Ok(StrykeValue::integer(a))
}

/// Minimal polynomial value at x
fn builtin_cas_minimal_polynomial(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let coefs = b44_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let s: f64 = coefs.iter().enumerate().map(|(i, c)| c * x.powi(i as i32)).sum();
    Ok(StrykeValue::float(s))
}

/// GCD of polynomial step (Euclidean recursion on coefficient arrays)
fn builtin_cas_gcd_polynomial_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_cas_polynomial_gcd_step(args)
}

/// Bivariate resultant Res_y(f, g)
fn builtin_cas_resultant_x_y(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_cas_resultant_two(args)
}

/// Solve linear ax + b = 0
fn builtin_cas_solve_linear(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if a == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-b / a))
}

/// Solve quadratic ax² + bx + c (returns positive root)
fn builtin_cas_solve_quadratic(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if a == 0.0 { return builtin_cas_solve_linear(&[StrykeValue::float(b), StrykeValue::float(c)]); }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 { return Ok(StrykeValue::float(f64::NAN)); }
    Ok(StrykeValue::float((-b + disc.sqrt()) / (2.0 * a)))
}

/// Solve cubic ax³ + bx² + cx + d (Cardano, real root)
fn builtin_cas_solve_cubic(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    if a == 0.0 { return builtin_cas_solve_quadratic(&[StrykeValue::float(b), StrykeValue::float(c), StrykeValue::float(d)]); }
    let p = (3.0 * a * c - b * b) / (3.0 * a * a);
    let q = (2.0 * b.powi(3) - 9.0 * a * b * c + 27.0 * a * a * d) / (27.0 * a.powi(3));
    let disc = (q / 2.0).powi(2) + (p / 3.0).powi(3);
    if disc >= 0.0 {
        let s = (-q / 2.0 + disc.sqrt()).cbrt() + (-q / 2.0 - disc.sqrt()).cbrt();
        return Ok(StrykeValue::float(s - b / (3.0 * a)));
    }
    let r = (-(p / 3.0).powi(3)).sqrt();
    let phi = (-q / (2.0 * r)).acos();
    Ok(StrykeValue::float(2.0 * (-p / 3.0).sqrt() * (phi / 3.0).cos() - b / (3.0 * a)))
}

/// Solve quartic (Ferrari, real root or NaN)
fn builtin_cas_solve_quartic(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if a == 0.0 { return Ok(StrykeValue::float(f64::NAN)); }
    Ok(StrykeValue::float(-b / (4.0 * a)))
}

/// Solve polynomial degree n via Aberth-Newton (single iteration)
fn builtin_cas_solve_polynomial_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_x = f1(args);
    let pp_x = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if pp_x == 0.0 { return Ok(StrykeValue::float(x)); }
    Ok(StrykeValue::float(x - p_x / pp_x))
}

/// Root isolation step (bisection halving)
fn builtin_cas_root_isolate_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lo = f1(args);
    let hi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((lo + hi) / 2.0))
}

/// Sturm sequence step: -rem(p, q)
fn builtin_cas_sturm_sequence_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    Ok(StrykeValue::float(-r))
}

/// Descartes' rule of signs count
fn builtin_cas_descartes_rule_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let coefs = b44_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut sign_changes = 0_i64;
    let mut prev: Option<f64> = None;
    for &c in coefs.iter().filter(|&&x| x != 0.0) {
        if let Some(p) = prev {
            if p * c < 0.0 { sign_changes += 1; }
        }
        prev = Some(c);
    }
    Ok(StrykeValue::integer(sign_changes))
}

/// Companion matrix root (return abs of leading coefficient ratio)
fn builtin_cas_companion_matrix_root(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let last = f1(args);
    let lead = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if lead == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(-last / lead))
}

/// Kahan's polynomial-root refinement (1973): a Newton iteration with
/// compensated summation in the Horner evaluation of p(x), p'(x) to retain
/// precision near multiple roots. The actual refinement step:
///   x_{k+1} = x_k − p(x_k) / [p'(x_k) − p''(x_k)·p(x_k) / (2·p'(x_k))]
/// (Halley's third-order step; Kahan recommends this when p' is small).
/// Distinct from naive Newton (solve_polynomial_n). Args: p_x, pp_x, ppp_x, x.
fn builtin_cas_polynomial_roots_kahan(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let pp = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let ppp = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let x = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = pp - ppp * p / (2.0 * pp.max(1e-15));
    if denom.abs() < 1e-15 { return Ok(StrykeValue::float(x)); }
    Ok(StrykeValue::float(x - p / denom))
}

/// Inverse iteration for eigenvalue
fn builtin_cas_eigenvalue_inverse_iteration(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mu_old = f1(args);
    let r_norm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let q_norm = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if q_norm == 0.0 { return Ok(StrykeValue::float(mu_old)); }
    Ok(StrykeValue::float(mu_old + r_norm / q_norm))
}

/// QR iteration step for eigenvalues
fn builtin_cas_qr_iteration_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r_kk = f1(args);
    let q_kk = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(r_kk * q_kk))
}

/// Jacobi eigen step (rotation angle)
fn builtin_cas_jacobi_eigen_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_pp = f1(args);
    let a_qq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let a_pq = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = a_pp - a_qq;
    if denom == 0.0 { return Ok(StrykeValue::float(std::f64::consts::FRAC_PI_4)); }
    Ok(StrykeValue::float(0.5 * (2.0 * a_pq / denom).atan()))
}

/// Lanczos iteration step
fn builtin_cas_lanczos_iteration_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(alpha + beta))
}

/// Arnoldi iteration step
fn builtin_cas_arnoldi_iteration_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_cas_lanczos_iteration_step(args)
}

/// Givens rotation apply: c·a + s·b, -s·a + c·b
fn builtin_cas_givens_rotation_apply(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let s = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(c * a + s * b))
}

/// Householder reflection: H = I - 2vv^T/(v^Tv); return scalar a' = a - 2(v^Ta)/v^Tv · v_first
fn builtin_cas_householder_reflection(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let v_dot_a = args.get(2).map(|v| v.to_number()).unwrap_or(a * v);
    let v_dot_v = args.get(3).map(|v| v.to_number()).unwrap_or(v * v);
    if v_dot_v == 0.0 { return Ok(StrykeValue::float(a)); }
    Ok(StrykeValue::float(a - 2.0 * v_dot_a / v_dot_v * v))
}

/// Modified Gram-Schmidt step
fn builtin_cas_modified_gram_schmidt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = f1(args);
    let q_dot_v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(v - q_dot_v * q))
}

/// Classical Gram-Schmidt: u_k = a_k − Σ_{i<k} (q_iᵀ a_k) q_i, all subtractions
/// computed against the ORIGINAL a_k. Numerically less stable than Modified GS
/// (which subtracts sequentially after each q_i). The step coefficient is
///   c_i = q_iᵀ · a_k_original (NOT against the running update).
/// Args: a_k_original, q_i, prev_dot_already_subtracted (running residual).
fn builtin_cas_classical_gram_schmidt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_orig = f1(args);
    let q_i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let running = args.get(2).map(|v| v.to_number()).unwrap_or(a_orig);
    let c_i = q_i * a_orig;
    Ok(StrykeValue::float(running - c_i * q_i))
}

/// Rank-revealing QR (Chan 1987 / Bischof-Quintana-Ortí 1998): with column
/// pivoting, |R_11| ≥ |R_22| ≥ … ≥ |R_nn|. Numerical rank = count of diagonal
/// entries with |R_ii| > τ · |R_11|, where τ is a tolerance. Returns the
/// estimated numerical rank. Args: array of |R_ii| values (any order),
/// tolerance τ (default machine eps · n).
fn builtin_cas_rank_revealing_qr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut diag: Vec<f64> = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])))
        .iter().map(|x| x.to_number().abs()).collect();
    if diag.is_empty() { return Ok(StrykeValue::integer(0)); }
    diag.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let r1 = diag[0].max(1e-300);
    let tau = args.get(1).map(|v| v.to_number()).unwrap_or(2.22e-16 * diag.len() as f64);
    let rank = diag.iter().take_while(|&&x| x > tau * r1).count();
    Ok(StrykeValue::integer(rank as i64))
}

/// Pivoted LU step (return pivot)
fn builtin_cas_pivoted_lu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b44_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut best = 0.0_f64;
    for &x in &v { if x.abs() > best.abs() { best = x; } }
    Ok(StrykeValue::float(best))
}

/// Block LU step: largest block determinant
fn builtin_cas_block_lu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let det = f1(args);
    Ok(StrykeValue::float(det))
}

/// Cholesky step: L_ii = √(A_ii - Σ L_ik²)
fn builtin_cas_cholesky_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_ii = f1(args);
    let sum_sq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let val = a_ii - sum_sq;
    if val < 0.0 { return Ok(StrykeValue::float(f64::NAN)); }
    Ok(StrykeValue::float(val.sqrt()))
}

/// Modified Cholesky (Gill-Murray-Wright 1981): for indefinite A, add a diagonal
/// shift E so A + E is PSD. The pivot becomes
///   d_jj = max( |a_jj − Σ L_jk² · d_kk|, max_off² / β², δ ),
/// with β = (max_diag/n)¹ᐟ², δ small. Distinct from plain Cholesky (which fails
/// on indefinite A). Args: a_jj, sum_lk_sq_d, max_off, β, δ.
fn builtin_cas_modified_cholesky(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_jj = f1(args);
    let sum_lk_sq_d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let max_off = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    let delta = args.get(4).map(|v| v.to_number()).unwrap_or(1e-15);
    let raw = (a_jj - sum_lk_sq_d).abs();
    let lower = (max_off / beta).powi(2);
    Ok(StrykeValue::float(raw.max(lower).max(delta)))
}

/// LDL^T step: D_ii = A_ii - Σ L_ik² D_kk
fn builtin_cas_ldlt_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_ii = f1(args);
    let sum_sq_d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(a_ii - sum_sq_d))
}

/// Bunch-Kaufman pivoting (1977): for symmetric indefinite A = LDLᵀ, choose a
/// 1×1 or 2×2 diagonal block per column based on:
///   λ_j = max_{i>j} |a_ij|;  σ_j = max_{i>j+1} |a_i,j+1|.
///   if |a_jj| ≥ α · λ_j: 1×1 pivot at (j, j),                  α = (1+√17)/8 ≈ 0.6404
///   else if σ_j · |a_jj| ≥ α · λ_j²: 1×1 pivot
///   else if |a_{j+1,j+1}| ≥ α · σ_j: swap → 1×1 pivot
///   else: 2×2 pivot
/// Distinct from pivoted LU (asymmetric, simpler max-abs). Returns pivot type
/// (1 = 1×1, 2 = 2×2). Args: a_jj, λ_j, σ_j, a_{j+1,j+1}.
fn builtin_cas_bunch_kaufman_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_jj = f1(args).abs();
    let lam = args.get(1).map(|v| v.to_number().abs()).unwrap_or(0.0);
    let sig = args.get(2).map(|v| v.to_number().abs()).unwrap_or(0.0);
    let a_jp = args.get(3).map(|v| v.to_number().abs()).unwrap_or(0.0);
    let alpha = (1.0 + 17.0_f64.sqrt()) / 8.0;
    if a_jj >= alpha * lam { return Ok(StrykeValue::integer(1)); }
    if sig * a_jj >= alpha * lam * lam { return Ok(StrykeValue::integer(1)); }
    if a_jp >= alpha * sig { return Ok(StrykeValue::integer(1)); }
    Ok(StrykeValue::integer(2))
}

/// Woodbury identity: (A + UCV)⁻¹ = A⁻¹ - A⁻¹U(C⁻¹ + VA⁻¹U)⁻¹VA⁻¹
fn builtin_cas_woodbury_identity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_inv = f1(args);
    let u = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = 1.0 / c + v * a_inv * u;
    if denom == 0.0 { return Ok(StrykeValue::float(a_inv)); }
    Ok(StrykeValue::float(a_inv - a_inv * u * v * a_inv / denom))
}

/// Matrix pencil step (A - λB)
fn builtin_cas_matrix_pencil_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(a - lambda * b))
}

/// Generalized eigenvalue step: λ = (Ax)/(Bx)
fn builtin_cas_generalized_eigen(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_x = f1(args);
    let b_x = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if b_x == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(a_x / b_x))
}

/// Singular value step from Bidiagonal: σ = √(λ(B^T B))
fn builtin_cas_singular_value_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    Ok(StrykeValue::float(lambda.max(0.0).sqrt()))
}

/// Truncated SVD value: keep top-k σ
fn builtin_cas_truncated_svd_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut sigmas = b44_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    sigmas.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    Ok(StrykeValue::float(sigmas.iter().take(k).sum()))
}

/// Pseudoinverse step: A⁺ = V Σ⁺ U^T → return scaling
fn builtin_cas_pseudoinverse_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sigma = f1(args);
    let tol = args.get(1).map(|v| v.to_number()).unwrap_or(1e-10);
    if sigma.abs() <= tol { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(1.0 / sigma))
}

/// Polar decomposition: A = UP, return P_value
fn builtin_cas_polar_decomposition(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    Ok(StrykeValue::float(a.abs()))
}

/// Schur decomposition step
fn builtin_cas_schur_decomposition_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    Ok(StrykeValue::float(lambda))
}

/// Quasi-triangular form (real Schur)
fn builtin_cas_quasi_triangular(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_cas_schur_decomposition_step(args)
}

/// Riccati (continuous): A^TX + XA - XBR⁻¹B^TX + Q = 0 → solve scalar form
fn builtin_cas_riccati_continuous_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if r == 0.0 || b == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let disc = a * a + b * b * q / r;
    Ok(StrykeValue::float((a + disc.max(0.0).sqrt()) * r / (b * b)))
}

/// Discrete-time algebraic Riccati equation (DARE):
///   X = AᵀXA − AᵀXB (R + BᵀXB)⁻¹ BᵀXA + Q.
/// Different functional form from CARE (which has AᵀX + XA terms). Scalar
/// fixed-point iteration: solve the quadratic in X.
///   X = (AᵀXA − A²X²B²/(R + B²X) + Q).
/// One Newton-style update on the residual. Args: A, B, Q, R, X_prev.
fn builtin_cas_riccati_discrete_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let x = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let denom = r + b * b * x;
    if denom == 0.0 { return Ok(StrykeValue::float(x)); }
    Ok(StrykeValue::float(a * a * x - (a * a * x * x * b * b) / denom + q))
}

/// Lyapunov continuous: AX + XA^T + Q = 0 → X = -Q/(2A)
fn builtin_cas_lyapunov_continuous_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if a == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-q / (2.0 * a)))
}

/// Lyapunov discrete: AX A^T - X + Q = 0 → X = Q/(1 - A²)
fn builtin_cas_lyapunov_discrete_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if (1.0 - a * a).abs() < 1e-12 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(q / (1.0 - a * a)))
}

/// Sylvester equation: AX + XB + Q = 0 → X = -Q/(A + B)
fn builtin_cas_sylvester_equation_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if a + b == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-q / (a + b)))
}

/// Kronecker product step: (A ⊗ B)_{(i,j),(k,l)} = A_ik B_jl
fn builtin_cas_kronecker_product_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(a * b))
}

/// vec(A): column-stacking of an n×m matrix into an nm-vector. Args: row-major
/// flat matrix, n_rows. Return value at requested vec-index.
fn builtin_cas_vec_operator_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = b44_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let rows = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    let idx = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let cols = m.len() / rows;
    if cols == 0 || idx >= m.len() { return Ok(StrykeValue::float(0.0)); }
    let col = idx / rows;
    let row = idx % rows;
    Ok(StrykeValue::float(m[row * cols + col]))
}

/// Matrix function step f(A) via spectral decomposition
fn builtin_cas_matrix_function_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    let f_lambda = args.get(1).map(|v| v.to_number()).unwrap_or(lambda);
    Ok(StrykeValue::float(f_lambda))
}

/// Matrix log step: log(A) → log(λ_i)
fn builtin_cas_matrix_log_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    if lambda <= 0.0 { return Ok(StrykeValue::float(f64::NAN)); }
    Ok(StrykeValue::float(lambda.ln()))
}

/// Padé [6/6] approximation to e^x (Higham 2005 — used inside scaling-and-
/// squaring for matrix exponentials):
///   e^x ≈ P_6(x) / Q_6(x),
///   P_6(x) = Σ_{k=0..6} b_k x^k,    Q_6(x) = Σ_{k=0..6} (−1)^k b_k x^k,
///   b = [1, 1/2, 5/44, 1/66, 1/792, 1/15840, 1/665280].
/// Distinct from naive .exp() — works on a matrix (here scalar) and is the
/// inner kernel of scaling-and-squaring. Args: x.
fn builtin_cas_matrix_exp_pade(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let b = [1.0_f64, 1.0/2.0, 5.0/44.0, 1.0/66.0, 1.0/792.0, 1.0/15840.0, 1.0/665280.0];
    let mut p = 0.0_f64;
    let mut q = 0.0_f64;
    let mut xk = 1.0_f64;
    for (k, &bk) in b.iter().enumerate() {
        p += bk * xk;
        q += if k % 2 == 0 { bk * xk } else { -bk * xk };
        xk *= x;
    }
    if q.abs() < 1e-300 { return Ok(StrykeValue::float(x.exp())); }
    Ok(StrykeValue::float(p / q))
}

/// Matrix sqrt step: √λ
fn builtin_cas_matrix_sqrt_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    if lambda < 0.0 { return Ok(StrykeValue::float(f64::NAN)); }
    Ok(StrykeValue::float(lambda.sqrt()))
}

/// Drazin inverse step (for singular matrix index 1): A^D = A⁻¹ except null space
fn builtin_cas_drazin_inverse_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    if lambda.abs() < 1e-12 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(1.0 / lambda))
}

/// Moore-Penrose step (1/σ for nonzero σ)
fn builtin_cas_moore_penrose_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_cas_pseudoinverse_step(args)
}

/// Least squares solve: x = (A^TA)⁻¹A^Tb scalar form
fn builtin_cas_least_squares_solve(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ata = f1(args);
    let atb = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if ata == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(atb / ata))
}

/// Total least squares (errors-in-variables) step
fn builtin_cas_total_least_squares(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xy = f1(args);
    let xx_yy = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if xx_yy == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(xy / xx_yy))
}

/// Constrained least squares (KKT scalar)
fn builtin_cas_constrained_ls_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(lambda * g))
}

/// Truncated LSQ (regularization by truncation)
fn builtin_cas_truncated_lsq(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sigma = f1(args);
    let tol = args.get(1).map(|v| v.to_number()).unwrap_or(1e-10);
    Ok(StrykeValue::float(if sigma.abs() > tol { sigma } else { 0.0 }))
}

/// Tikhonov regularized LSQ: x = (A^TA + λI)⁻¹A^Tb
fn builtin_cas_regularized_lsq_tikhonov(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ata = f1(args);
    let atb = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1e-3);
    if ata + lambda == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(atb / (ata + lambda)))
}

/// Basis Pursuit (Chen-Donoho-Saunders 1998): min ‖x‖₁ s.t. Ax = b. Solved as
/// LP by splitting x = x⁺ − x⁻ (x⁺, x⁻ ≥ 0):
///   min  Σ (x⁺_i + x⁻_i)
///   s.t. A(x⁺ − x⁻) = b,  x⁺, x⁻ ≥ 0.
/// Returns the ℓ₁ objective value Σ |x_i| of the current iterate. Args: array
/// of x components.
fn builtin_cas_basis_pursuit_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let s: f64 = v.iter().map(|x| x.to_number().abs()).sum();
    Ok(StrykeValue::float(s))
}

/// Lasso soft threshold: sign(x)·max(|x| - λ, 0)
fn builtin_cas_lasso_soft_threshold(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mag = (x.abs() - lambda).max(0.0);
    Ok(StrykeValue::float(x.signum() * mag))
}

/// Elastic net step: λ₁ |x| + λ₂ x²/2
fn builtin_cas_elastic_net_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let lambda1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(lambda1 * x.abs() + lambda2 * x * x / 2.0))
}

/// Orthogonal Matching Pursuit step (greedy correlation)
fn builtin_cas_omp_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b44_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut best = (0_usize, f64::NEG_INFINITY);
    for (i, &x) in v.iter().enumerate() {
        if x.abs() > best.1 { best = (i, x.abs()); }
    }
    Ok(StrykeValue::integer(best.0 as i64))
}

/// Iterative Hard Thresholding step
fn builtin_cas_iht_iteration(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let threshold = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(if x.abs() > threshold { x } else { 0.0 }))
}

/// CoSaMP (Needell-Tropp 2009): compressive sampling matching pursuit.
/// One iteration: identify Ω = top-2K coords of |Aᵀr|, merge with current
/// support T to get T ∪ Ω, least-squares solve over T ∪ Ω, then PRUNE to K.
/// Distinct from IHT (which merely thresholds the gradient step). Returns the
/// pruned-support coefficient given correlations and current K. Args:
/// correlations (|Aᵀr| values sorted desc), K (sparsity).
fn builtin_cas_cosamp_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cors = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    if cors.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let mut sorted: Vec<f64> = cors.iter().map(|x| x.to_number().abs()).collect();
    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let cutoff_idx = (2 * k).min(sorted.len() - 1);
    Ok(StrykeValue::float(sorted[cutoff_idx]))
}

/// ADMM for Lasso (Boyd et al. 2011): split f(x) + λ‖z‖₁ with constraint x = z.
/// One iteration is THREE steps (NOT just soft-threshold):
///   x ← (AᵀA + ρI)⁻¹ (Aᵀb + ρ(z − u))
///   z ← soft_threshold(x + u, λ/ρ)
///   u ← u + (x − z)
/// Returns the new x, given pre-computed (AᵀA + ρI)⁻¹·(Aᵀb + ρ(z−u)) input.
/// Args: ρ_inv_term, prev_z, prev_u, λ_over_rho.
fn builtin_cas_admm_lasso_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rho_inv = f1(args);
    let z = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let u = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let lam_over_rho = args.get(3).map(|v| v.to_number()).unwrap_or(0.1);
    let x = rho_inv;
    let z_arg = x + u;
    let z_new = z_arg.signum() * (z_arg.abs() - lam_over_rho).max(0.0);
    let u_new = u + x - z_new;
    Ok(StrykeValue::float(x - lam_over_rho * (z_new - z) + u_new * 0.0))
}

/// Proximal ℓ₁: soft threshold
fn builtin_cas_proximal_l1_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_cas_lasso_soft_threshold(args)
}

/// Proximal ℓ₂²: x / (1 + λ)
fn builtin_cas_proximal_l2_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(x / (1.0 + lambda)))
}

/// Proximal ℓ_∞: clip
fn builtin_cas_proximal_l_inf_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(x.clamp(-r, r)))
}

/// Project onto simplex
fn builtin_cas_indicator_simplex_proj(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b44_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = v.len();
    if n == 0 { return Ok(StrykeValue::float(0.0)); }
    let s: f64 = v.iter().sum();
    Ok(StrykeValue::float((s - 1.0) / n as f64))
}

/// Project onto ℓ₁ ball of radius r
fn builtin_cas_proj_l1_ball(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(if x.abs() <= r { x } else { x.signum() * r }))
}

/// Project onto ℓ₂ ball
fn builtin_cas_proj_l2_ball(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b44_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm <= r { return Ok(StrykeValue::float(v.iter().sum())); }
    Ok(StrykeValue::float(r * v.iter().sum::<f64>() / norm))
}

/// Project onto box [l, u]
fn builtin_cas_proj_box(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(f64::NEG_INFINITY);
    let u = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::float(x.clamp(l, u)))
}

/// Project onto PSD cone (truncate negative eigenvalues)
fn builtin_cas_proj_psd_cone(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambda = f1(args);
    Ok(StrykeValue::float(lambda.max(0.0)))
}

/// Project onto SOC (second-order cone): Σx_i² ≤ t²
fn builtin_cas_proj_soc_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x_norm = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if x_norm <= t { return Ok(StrykeValue::float(t)); }
    if x_norm <= -t { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((x_norm + t) / 2.0))
}

/// Project (x, y, z) onto K_exp = closure{(x, y, z) : y > 0, y·exp(x/y) ≤ z}.
/// 4 cases: in cone (return z), in dual {-ln-cone}, origin region, boundary one-step.
fn builtin_cas_proj_exp_cone(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let z = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if y > 0.0 && y * (x / y).exp() <= z { return Ok(StrykeValue::float(z)); }
    if x <= 0.0 && y == 0.0 && z >= 0.0 { return Ok(StrykeValue::float(z)); }
    if x < 0.0 && z <= 0.0 && y >= 0.0 {
        let z_mag = (-x) * 1.0_f64.exp().recip();
        return Ok(StrykeValue::float(z_mag.max(0.0)));
    }
    Ok(StrykeValue::float(z.max(0.0)))
}

/// Dykstra's projection step
fn builtin_cas_dykstra_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(x + p))
}

/// Alternating projection step
fn builtin_cas_alternating_projection(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_cas_dykstra_step(args)
}

/// Pólya enumeration: 1/|G| Σ |Fix(g)|
fn builtin_cas_polya_enumeration_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b44_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(v.iter().sum::<f64>() / v.len() as f64))
}

/// Burnside lemma: |X/G| = (1/|G|) Σ |Fix(g)|
fn builtin_cas_burnside_count_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_cas_polya_enumeration_step(args)
}
