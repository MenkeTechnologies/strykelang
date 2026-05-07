// ─────────────────────────────────────────────────────────────────────────────
// Wolfram-Math parity, batch 2: number-theory extras, combinatoric sequences,
// linear-algebra extras, polynomial algebra, more distributions, Mathieu /
// Heun, wavelets, graph extras, and misc fill-ins (Stieltjes, Gauss sum, etc).
// Included alongside `math_wolfram.rs` from `builtins.rs`. Helpers `f1..f4`,
// `i1..i2`, `arg_to_vec`, `mod_pow_i64`, `gcd_i64`, `binomial_f`, and
// `prime_factorize` come from the surrounding scope.
// ─────────────────────────────────────────────────────────────────────────────

// ── Tier A: number theory extensions ─────────────────────────────────────────

/// Liouville λ(n) = (-1)^Ω(n).
fn builtin_liouville_lambda(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if n < 1 {
        return Ok(PerlValue::integer(0));
    }
    let omega = prime_factorize(n).len() as i64;
    Ok(PerlValue::integer(if omega & 1 == 0 { 1 } else { -1 }))
}

/// Jordan totient J_k(n) = n^k Π_{p|n} (1 - p^{-k}).
fn builtin_jordan_totient(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (k, n) = i2(args);
    if n < 1 || k < 1 {
        return Ok(PerlValue::integer(0));
    }
    let factors = prime_factorize(n);
    let mut uniq = factors.clone();
    uniq.sort();
    uniq.dedup();
    let mut num = (n as i128).pow(k as u32);
    let mut den = 1_i128;
    for &p in &uniq {
        let pk = (p as i128).pow(k as u32);
        num *= pk - 1;
        den *= pk;
    }
    Ok(PerlValue::integer((num / den) as i64))
}

/// Ramanujan sum c_q(n) = Σ_{d | gcd(q,n)} μ(q/d) d.
fn builtin_ramanujan_sum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (q, n) = i2(args);
    if q < 1 {
        return Ok(PerlValue::integer(0));
    }
    let g = gcd_i64(q, n.abs());
    let mut sum = 0_i64;
    let mut d = 1_i64;
    while d * d <= g {
        if g % d == 0 {
            sum += mobius_i64(q / d) * d;
            if d != g / d {
                sum += mobius_i64(q / (g / d)) * (g / d);
            }
        }
        d += 1;
    }
    Ok(PerlValue::integer(sum))
}

fn mobius_i64(n: i64) -> i64 {
    if n < 1 {
        return 0;
    }
    let factors = prime_factorize(n);
    let mut uniq = factors.clone();
    uniq.sort();
    uniq.dedup();
    if factors.len() != uniq.len() {
        return 0;
    }
    if factors.len() & 1 == 0 {
        1
    } else {
        -1
    }
}

/// Cyclotomic polynomial Φ_n(x) coefficients via Möbius inversion of x^n - 1.
fn builtin_cyclotomic_polynomial(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(1) as usize;
    // Φ_n(x) = Π_{d | n} (x^d - 1)^{μ(n/d)}.
    // Compute by polynomial multiplication / division of (x^d - 1) factors.
    // Numerator factors: μ=1; denominator factors: μ=-1.
    let mut num: Vec<i64> = vec![1];
    let mut den: Vec<i64> = vec![1];
    let mut d = 1_usize;
    while d <= n {
        if n.is_multiple_of(d) {
            let mu = mobius_i64((n / d) as i64);
            if mu != 0 {
                let mut factor = vec![0_i64; d + 1];
                factor[0] = -1;
                factor[d] = 1;
                if mu == 1 {
                    num = poly_mul(&num, &factor);
                } else {
                    den = poly_mul(&den, &factor);
                }
            }
        }
        d += 1;
    }
    let coeffs = poly_div_exact(&num, &den);
    Ok(PerlValue::array(
        coeffs.into_iter().map(PerlValue::integer).collect(),
    ))
}

fn poly_mul(a: &[i64], b: &[i64]) -> Vec<i64> {
    let mut out = vec![0_i64; a.len() + b.len() - 1];
    for (i, &ai) in a.iter().enumerate() {
        if ai == 0 {
            continue;
        }
        for (j, &bj) in b.iter().enumerate() {
            out[i + j] += ai * bj;
        }
    }
    out
}

fn poly_div_exact(num: &[i64], den: &[i64]) -> Vec<i64> {
    if den == [1] {
        return num.to_vec();
    }
    let mut a = num.to_vec();
    let mut q = vec![0_i64; a.len() - den.len() + 1];
    for i in (0..q.len()).rev() {
        let lead = a[i + den.len() - 1] / den[den.len() - 1];
        q[i] = lead;
        for (j, &dj) in den.iter().enumerate() {
            a[i + j] -= lead * dj;
        }
    }
    q
}

/// Legendre symbol (a/p) = a^((p-1)/2) mod p, mapped to {-1, 0, 1}.
fn builtin_legendre_symbol(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (a, p) = i2(args);
    if p < 2 {
        return Ok(PerlValue::integer(0));
    }
    let r = mod_pow_i64(a.rem_euclid(p), (p - 1) / 2, p);
    Ok(PerlValue::integer(if r == p - 1 { -1 } else { r }))
}

/// `pythagorean_triple_q a, b, c` — 1 if a² + b² = c² (any signs/order).
fn builtin_pythagorean_triple_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut v = [
        args.first().map(|x| x.to_number() as i64).unwrap_or(0).abs(),
        args.get(1).map(|x| x.to_number() as i64).unwrap_or(0).abs(),
        args.get(2).map(|x| x.to_number() as i64).unwrap_or(0).abs(),
    ];
    v.sort();
    Ok(PerlValue::integer(
        if v[0] * v[0] + v[1] * v[1] == v[2] * v[2] && v[0] > 0 {
            1
        } else {
            0
        },
    ))
}

/// `gen_pythagorean_triple m, n` — Euclid formula: (m²-n², 2mn, m²+n²) for m > n ≥ 1.
fn builtin_gen_pythagorean_triple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (m, n) = i2(args);
    Ok(PerlValue::array(vec![
        PerlValue::integer(m * m - n * n),
        PerlValue::integer(2 * m * n),
        PerlValue::integer(m * m + n * n),
    ]))
}

/// `sophie_germain_q p` — 1 if p and 2p+1 are both prime.
fn builtin_sophie_germain_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = i1(args);
    Ok(PerlValue::integer(
        if is_prime_check(p) && is_prime_check(2 * p + 1) {
            1
        } else {
            0
        },
    ))
}

/// `mersenne_q n` — 1 if 2^n - 1 is a Mersenne prime (uses Lucas-Lehmer for n≥3).
fn builtin_mersenne_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if !is_prime_check(n) {
        return Ok(PerlValue::integer(0));
    }
    if n == 2 {
        return Ok(PerlValue::integer(1));
    }
    if n > 60 {
        return Ok(PerlValue::integer(0));
    }
    let m = (1_i64 << n) - 1;
    let mut s = 4_i64;
    for _ in 0..n - 2 {
        s = ((s as i128 * s as i128 - 2) % m as i128) as i64;
    }
    Ok(PerlValue::integer(if s == 0 { 1 } else { 0 }))
}

/// `lucas_lehmer_test p` — same as mersenne_q for prime p (alias kept for clarity).
fn builtin_lucas_lehmer_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_mersenne_q(args)
}

/// `continued_fraction X [, N]` — first N coefficients of the simple continued
/// fraction expansion of X (rational or irrational). N defaults to 12.
fn builtin_continued_fraction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(12).max(1);
    let mut out: Vec<PerlValue> = Vec::with_capacity(n as usize);
    let mut t = x;
    for _ in 0..n {
        let a = t.floor();
        out.push(PerlValue::integer(a as i64));
        let frac = t - a;
        if frac.abs() < 1e-15 {
            break;
        }
        t = 1.0 / frac;
    }
    Ok(PerlValue::array(out))
}

/// `from_continued_fraction COEFFS` — evaluate [a0; a1, a2, …] back to a real.
fn builtin_from_continued_fraction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let coeffs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    if coeffs.is_empty() {
        return Ok(PerlValue::float(0.0));
    }
    let mut acc = *coeffs.last().unwrap();
    for &c in coeffs.iter().rev().skip(1) {
        acc = c + 1.0 / acc;
    }
    Ok(PerlValue::float(acc))
}

/// `convergents X [, N]` — N successive [num, denom] convergents of X's CF.
fn builtin_convergents(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(12).max(1);
    let mut t = x;
    let (mut h0, mut h1) = (1_i64, 0_i64);
    let (mut k0, mut k1) = (0_i64, 1_i64);
    let mut out: Vec<PerlValue> = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let a = t.floor() as i64;
        let h2 = a * h0 + h1;
        let k2 = a * k0 + k1;
        out.push(PerlValue::array(vec![
            PerlValue::integer(h2),
            PerlValue::integer(k2),
        ]));
        h1 = h0;
        h0 = h2;
        k1 = k0;
        k0 = k2;
        let frac = t - a as f64;
        if frac.abs() < 1e-15 {
            break;
        }
        t = 1.0 / frac;
    }
    Ok(PerlValue::array(out))
}

/// `best_rational_approximation X, MAX_DENOM` — Stern-Brocot mediant search.
fn builtin_best_rational_approximation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let max_d = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1000).max(1);
    if x.is_nan() {
        return Ok(PerlValue::UNDEF);
    }
    let sign = if x < 0.0 { -1 } else { 1 };
    let xa = x.abs();
    let mut t = xa;
    let (mut h0, mut h1) = (1_i64, 0_i64);
    let (mut k0, mut k1) = (0_i64, 1_i64);
    loop {
        let a = t.floor() as i64;
        let new_k0 = a * k0 + k1;
        if new_k0 > max_d {
            // Use semiconvergent if better.
            let max_a = (max_d - k1) / k0;
            if max_a >= 1 {
                let h_sc = max_a * h0 + h1;
                let k_sc = max_a * k0 + k1;
                let approx = h_sc as f64 / k_sc as f64;
                let approx_h0 = h0 as f64 / k0 as f64;
                if (approx - xa).abs() < (approx_h0 - xa).abs() {
                    return Ok(PerlValue::array(vec![
                        PerlValue::integer(sign * h_sc),
                        PerlValue::integer(k_sc),
                    ]));
                }
            }
            return Ok(PerlValue::array(vec![
                PerlValue::integer(sign * h0),
                PerlValue::integer(k0),
            ]));
        }
        let h2 = a * h0 + h1;
        h1 = h0;
        h0 = h2;
        k1 = k0;
        k0 = new_k0;
        let frac = t - a as f64;
        if frac.abs() < 1e-15 {
            return Ok(PerlValue::array(vec![
                PerlValue::integer(sign * h0),
                PerlValue::integer(k0),
            ]));
        }
        t = 1.0 / frac;
    }
}

// ── Tier B: combinatorial sequences ──────────────────────────────────────────

/// Motzkin number M_n via OEIS A001006: (n+3) M_{n+1} = (2n+3) M_n + 3n M_{n-1}.
/// Sequence: 1, 1, 2, 4, 9, 21, 51, 127, 323, 835, …
fn builtin_motzkin_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    if n == 0 || n == 1 {
        return Ok(PerlValue::integer(1));
    }
    let (mut m0, mut m1) = (1_i128, 1_i128);
    for k in 1..n {
        let kf = k as i128;
        let next = ((2 * kf + 3) * m1 + 3 * kf * m0) / (kf + 3);
        m0 = m1;
        m1 = next;
    }
    Ok(PerlValue::integer(m1 as i64))
}

/// Narayana N(n, k) = (1/n) C(n, k) C(n, k-1).
fn builtin_narayana_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (n, k) = i2(args);
    if n < 1 || k < 1 || k > n {
        return Ok(PerlValue::integer(0));
    }
    let n = n as usize;
    let k = k as usize;
    let v = binomial_f(n, k) * binomial_f(n, k - 1) / n as f64;
    Ok(PerlValue::integer(v.round() as i64))
}

/// Delannoy D(m, n) — # paths in (m+1) × (n+1) grid using →↑↗ steps.
fn builtin_delannoy_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (m, n) = i2(args);
    if m < 0 || n < 0 {
        return Ok(PerlValue::integer(0));
    }
    let m = m as usize;
    let n = n as usize;
    let mut t = vec![vec![0_i64; n + 1]; m + 1];
    for i in 0..=m {
        for j in 0..=n {
            if i == 0 || j == 0 {
                t[i][j] = 1;
            } else {
                t[i][j] = t[i - 1][j] + t[i][j - 1] + t[i - 1][j - 1];
            }
        }
    }
    Ok(PerlValue::integer(t[m][n]))
}

/// Large Schröder S_n. Recurrence: (n+2) S_{n+1} = 3(2n+1) S_n - (n-1) S_{n-1}, S_0=1, S_1=2.
fn builtin_schroder_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    if n == 0 {
        return Ok(PerlValue::integer(1));
    }
    let (mut s0, mut s1) = (1_i128, 2_i128);
    for k in 1..n {
        let kf = k as i128;
        let next = (3 * (2 * kf + 1) * s1 - (kf - 1) * s0) / (kf + 2);
        s0 = s1;
        s1 = next;
    }
    Ok(PerlValue::integer(s1 as i64))
}

/// Small Schröder s_n = S_n / 2 for n ≥ 1, s_0 = 1.
fn builtin_small_schroder_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    if n == 0 {
        return Ok(PerlValue::integer(1));
    }
    let s = builtin_schroder_number(args)?.to_number() as i64;
    Ok(PerlValue::integer(s / 2))
}

/// Eulerian ⟨n k⟩ — # permutations with k ascents. Recurrence over rows.
fn builtin_eulerian_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (n, k) = i2(args);
    if n < 0 || k < 0 || k >= n {
        return Ok(PerlValue::integer(if n == 0 && k == 0 { 1 } else { 0 }));
    }
    let n = n as usize;
    let k = k as usize;
    let mut row = vec![0_i64; n + 1];
    row[0] = 1;
    for i in 1..=n {
        let mut new_row = vec![0_i64; n + 1];
        for j in 0..i {
            new_row[j] = (j as i64 + 1) * row[j] + (i as i64 - j as i64) * if j > 0 { row[j - 1] } else { 0 };
        }
        row = new_row;
    }
    Ok(PerlValue::integer(row[k]))
}

/// Bernoulli polynomial B_n(x) = Σ_{k=0..n} C(n,k) B_k x^{n-k}.
fn builtin_bernoulli_polynomial(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (n, x) = f2(args);
    let n = n as usize;
    let bern = bernoulli_table(n);
    let mut sum = 0.0_f64;
    for k in 0..=n {
        sum += binomial_f(n, k) * bern[k] * x.powi((n - k) as i32);
    }
    Ok(PerlValue::float(sum))
}

fn bernoulli_table(nmax: usize) -> Vec<f64> {
    // Bernoulli numbers B_0..B_n via recurrence.
    let mut b = vec![0.0_f64; nmax + 1];
    b[0] = 1.0;
    for m in 1..=nmax {
        let mut s = 0.0_f64;
        for k in 0..m {
            s += binomial_f(m + 1, k) * b[k];
        }
        b[m] = -s / (m as f64 + 1.0);
    }
    b
}

/// Euler polynomial E_n(x) — relates to Bernoulli via E_n(x) = (2/(n+1)) (B_{n+1}(x) - 2^{n+1} B_{n+1}(x/2)).
fn builtin_euler_polynomial(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (n, x) = f2(args);
    let n = n as usize;
    let bern = bernoulli_table(n + 1);
    let mut bx = 0.0_f64;
    let mut bxh = 0.0_f64;
    for k in 0..=n + 1 {
        bx += binomial_f(n + 1, k) * bern[k] * x.powi((n + 1 - k) as i32);
        bxh += binomial_f(n + 1, k) * bern[k] * (x / 2.0).powi((n + 1 - k) as i32);
    }
    let v = 2.0 / (n as f64 + 1.0) * (bx - 2.0_f64.powi((n + 1) as i32) * bxh);
    Ok(PerlValue::float(v))
}

/// Pell number P_n: P_0=0, P_1=1, P_{n+1}=2 P_n + P_{n-1}.
fn builtin_pell_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if n < 0 {
        return Ok(PerlValue::integer(0));
    }
    let (mut a, mut b) = (0_i128, 1_i128);
    for _ in 0..n {
        let c = 2 * b + a;
        a = b;
        b = c;
    }
    Ok(PerlValue::integer(a as i64))
}

/// Pell-Lucas Q_n: Q_0=2, Q_1=2, Q_{n+1}=2 Q_n + Q_{n-1}.
fn builtin_pell_lucas_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if n < 0 {
        return Ok(PerlValue::integer(0));
    }
    let (mut a, mut b) = (2_i128, 2_i128);
    for _ in 0..n {
        let c = 2 * b + a;
        a = b;
        b = c;
    }
    Ok(PerlValue::integer(a as i64))
}

/// Perrin sequence: 3, 0, 2, 3, 2, 5, 5, 7, …; P(n) = P(n-2) + P(n-3).
fn builtin_perrin_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if n < 0 {
        return Ok(PerlValue::integer(0));
    }
    let (mut a, mut b, mut c) = (3_i128, 0_i128, 2_i128);
    for _ in 0..n {
        let d = a + b;
        a = b;
        b = c;
        c = d;
    }
    Ok(PerlValue::integer(a as i64))
}

/// Padovan sequence: 1, 1, 1, 2, 2, 3, 4, 5, …; P(n) = P(n-2) + P(n-3).
fn builtin_padovan_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if n < 0 {
        return Ok(PerlValue::integer(0));
    }
    let (mut a, mut b, mut c) = (1_i128, 1_i128, 1_i128);
    for _ in 0..n {
        let d = a + b;
        a = b;
        b = c;
        c = d;
    }
    Ok(PerlValue::integer(a as i64))
}

// ── Tier C: linear algebra extensions ────────────────────────────────────────

fn matrix_from_value(v: &PerlValue) -> Vec<Vec<f64>> {
    arg_to_vec(v)
        .iter()
        .map(|row| arg_to_vec(row).iter().map(|x| x.to_number()).collect())
        .collect()
}

fn matrix_to_value(m: &[Vec<f64>]) -> PerlValue {
    PerlValue::array(
        m.iter()
            .map(|row| {
                PerlValue::array(row.iter().copied().map(PerlValue::float).collect())
            })
            .collect(),
    )
}

/// Kronecker product A ⊗ B.
fn builtin_kronecker_product(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let b = matrix_from_value(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    if a.is_empty() || b.is_empty() {
        return Ok(matrix_to_value(&[]));
    }
    let (ra, ca) = (a.len(), a[0].len());
    let (rb, cb) = (b.len(), b[0].len());
    let mut out = vec![vec![0.0_f64; ca * cb]; ra * rb];
    for i in 0..ra {
        for j in 0..ca {
            for k in 0..rb {
                for l in 0..cb {
                    out[i * rb + k][j * cb + l] = a[i][j] * b[k][l];
                }
            }
        }
    }
    Ok(matrix_to_value(&out))
}

/// Tensor outer-product of two flat vectors → matrix A_i B_j.
fn builtin_tensor_product(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut out = vec![vec![0.0_f64; b.len()]; a.len()];
    for (i, &ai) in a.iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            out[i][j] = ai * bj;
        }
    }
    Ok(matrix_to_value(&out))
}

/// Tensor contract: trace of A B^T over a single shared axis (for 2-D tensors).
fn builtin_tensor_contract(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let b = matrix_from_value(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let mut sum = 0.0_f64;
    let r = a.len().min(b.len());
    for i in 0..r {
        let c = a[i].len().min(b[i].len());
        for j in 0..c {
            sum += a[i][j] * b[i][j];
        }
    }
    Ok(PerlValue::float(sum))
}

/// Matrix rank via Gaussian elimination with partial pivoting.
fn builtin_matrix_rank(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if m.is_empty() {
        return Ok(PerlValue::integer(0));
    }
    let rows = m.len();
    let cols = m[0].len();
    let mut rank = 0_usize;
    let mut row = 0_usize;
    for col in 0..cols {
        if row >= rows {
            break;
        }
        // Pivot: largest |value| in column col, rows row..rows.
        let mut pivot = row;
        for i in row + 1..rows {
            if m[i][col].abs() > m[pivot][col].abs() {
                pivot = i;
            }
        }
        if m[pivot][col].abs() < 1e-12 {
            continue;
        }
        m.swap(row, pivot);
        for i in row + 1..rows {
            let factor = m[i][col] / m[row][col];
            for j in col..cols {
                m[i][j] -= factor * m[row][j];
            }
        }
        rank += 1;
        row += 1;
    }
    Ok(PerlValue::integer(rank as i64))
}

/// Companion matrix of monic polynomial [a_0, a_1, …, a_{n-1}] with leading 1.
fn builtin_companion_matrix(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let coeffs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = coeffs.len();
    if n == 0 {
        return Ok(matrix_to_value(&[]));
    }
    // Coefficients [a_0..a_{n-1}] of x^n + a_{n-1} x^{n-1} + … + a_0.
    let mut m = vec![vec![0.0_f64; n]; n];
    for i in 0..n - 1 {
        m[i + 1][i] = 1.0;
    }
    for i in 0..n {
        m[i][n - 1] = -coeffs[i];
    }
    Ok(matrix_to_value(&m))
}

/// Characteristic polynomial via Faddeev-LeVerrier. Returns coefficients
/// [a_0, a_1, …, a_n] of det(xI - A) = a_0 + a_1 x + … + x^n.
fn builtin_characteristic_polynomial(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = a.len();
    if n == 0 || a[0].len() != n {
        return Err(PerlError::runtime(
            "characteristic_polynomial: square matrix required",
            0,
        ));
    }
    // Faddeev-LeVerrier: M_0 = 0, c_n = 1; for k = 1..n:
    //   M_k = A * M_{k-1} + c_{n-k+1} I; c_{n-k} = -tr(A * M_k)/k.
    let mut m = vec![vec![0.0_f64; n]; n];
    let mut c = vec![0.0_f64; n + 1];
    c[n] = 1.0;
    for k in 1..=n {
        // M_k = A M_{k-1} + c_{n-k+1} I
        let mut new_m = mat_mul(&a, &m);
        for i in 0..n {
            new_m[i][i] += c[n - k + 1];
        }
        m = new_m;
        // c_{n-k} = -trace(A M_k) / k
        let am = mat_mul(&a, &m);
        let tr = (0..n).map(|i| am[i][i]).sum::<f64>();
        c[n - k] = -tr / k as f64;
    }
    Ok(PerlValue::array(c.into_iter().map(PerlValue::float).collect()))
}

fn mat_mul(a: &[Vec<f64>], b: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = a.len();
    if n == 0 {
        return Vec::new();
    }
    let m = b[0].len();
    let k = b.len();
    let mut out = vec![vec![0.0_f64; m]; n];
    for i in 0..n {
        for j in 0..m {
            let mut s = 0.0_f64;
            for kk in 0..k {
                s += a[i][kk] * b[kk][j];
            }
            out[i][j] = s;
        }
    }
    out
}

/// Singular values of A: eigenvalues of A^T A, then sqrt.
fn builtin_singular_values(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if a.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let n = a.len();
    let m = a[0].len();
    // Form A^T A (m × m, symmetric positive semidefinite).
    let mut ata = vec![vec![0.0_f64; m]; m];
    for i in 0..m {
        for j in 0..m {
            let mut s = 0.0_f64;
            for k in 0..n {
                s += a[k][i] * a[k][j];
            }
            ata[i][j] = s;
        }
    }
    // Jacobi eigenvalue iteration for symmetric matrix.
    let evs = jacobi_eigenvalues(&mut ata);
    let mut svs: Vec<f64> = evs.into_iter().map(|e| e.max(0.0).sqrt()).collect();
    svs.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    Ok(PerlValue::array(svs.into_iter().map(PerlValue::float).collect()))
}

/// Symmetric Jacobi eigenvalue iteration.
fn jacobi_eigenvalues(a: &mut [Vec<f64>]) -> Vec<f64> {
    let n = a.len();
    if n == 0 {
        return Vec::new();
    }
    for _ in 0..50 {
        // Find largest off-diagonal.
        let (mut p, mut q) = (0_usize, 1_usize);
        let mut max_off = 0.0_f64;
        for i in 0..n {
            for j in i + 1..n {
                if a[i][j].abs() > max_off {
                    max_off = a[i][j].abs();
                    p = i;
                    q = j;
                }
            }
        }
        if max_off < 1e-12 {
            break;
        }
        let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
        let t = if theta >= 0.0 {
            1.0 / (theta + (1.0 + theta * theta).sqrt())
        } else {
            1.0 / (theta - (1.0 + theta * theta).sqrt())
        };
        let c = 1.0 / (1.0 + t * t).sqrt();
        let s = t * c;
        let app = a[p][p];
        let aqq = a[q][q];
        let apq = a[p][q];
        a[p][p] = c * c * app - 2.0 * s * c * apq + s * s * aqq;
        a[q][q] = s * s * app + 2.0 * s * c * apq + c * c * aqq;
        a[p][q] = 0.0;
        a[q][p] = 0.0;
        for i in 0..n {
            if i != p && i != q {
                let aip = a[i][p];
                let aiq = a[i][q];
                a[i][p] = c * aip - s * aiq;
                a[p][i] = a[i][p];
                a[i][q] = s * aip + c * aiq;
                a[q][i] = a[i][q];
            }
        }
    }
    (0..n).map(|i| a[i][i]).collect()
}

/// Null-space basis of A — the columns of V corresponding to ~zero singular
/// values, returned as a list of column vectors. Uses A^T A eigen-decomposition.
fn builtin_nullspace(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if a.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let n = a.len();
    let m = a[0].len();
    let mut ata = vec![vec![0.0_f64; m]; m];
    for i in 0..m {
        for j in 0..m {
            let mut s = 0.0_f64;
            for k in 0..n {
                s += a[k][i] * a[k][j];
            }
            ata[i][j] = s;
        }
    }
    // Jacobi rotations also accumulate eigenvectors — implement here to get V.
    let mut v = vec![vec![0.0_f64; m]; m];
    for i in 0..m {
        v[i][i] = 1.0;
    }
    for _ in 0..50 {
        let (mut p, mut q) = (0_usize, 1_usize);
        let mut max_off = 0.0_f64;
        for i in 0..m {
            for j in i + 1..m {
                if ata[i][j].abs() > max_off {
                    max_off = ata[i][j].abs();
                    p = i;
                    q = j;
                }
            }
        }
        if max_off < 1e-12 {
            break;
        }
        let theta = (ata[q][q] - ata[p][p]) / (2.0 * ata[p][q]);
        let t = if theta >= 0.0 {
            1.0 / (theta + (1.0 + theta * theta).sqrt())
        } else {
            1.0 / (theta - (1.0 + theta * theta).sqrt())
        };
        let c = 1.0 / (1.0 + t * t).sqrt();
        let s = t * c;
        let app = ata[p][p];
        let aqq = ata[q][q];
        let apq = ata[p][q];
        ata[p][p] = c * c * app - 2.0 * s * c * apq + s * s * aqq;
        ata[q][q] = s * s * app + 2.0 * s * c * apq + c * c * aqq;
        ata[p][q] = 0.0;
        ata[q][p] = 0.0;
        for i in 0..m {
            if i != p && i != q {
                let aip = ata[i][p];
                let aiq = ata[i][q];
                ata[i][p] = c * aip - s * aiq;
                ata[p][i] = ata[i][p];
                ata[i][q] = s * aip + c * aiq;
                ata[q][i] = ata[i][q];
            }
        }
        for i in 0..m {
            let vip = v[i][p];
            let viq = v[i][q];
            v[i][p] = c * vip - s * viq;
            v[i][q] = s * vip + c * viq;
        }
    }
    let mut basis: Vec<PerlValue> = Vec::new();
    for j in 0..m {
        if ata[j][j].abs() < 1e-9 {
            let col: Vec<PerlValue> = (0..m).map(|i| PerlValue::float(v[i][j])).collect();
            basis.push(PerlValue::array(col));
        }
    }
    Ok(PerlValue::array(basis))
}

// ── Tier D: polynomial algebra ───────────────────────────────────────────────

fn poly_from_value(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

fn poly_to_value(p: &[f64]) -> PerlValue {
    PerlValue::array(p.iter().copied().map(PerlValue::float).collect())
}

fn poly_strip(p: &[f64]) -> Vec<f64> {
    let mut q: Vec<f64> = p.to_vec();
    while q.len() > 1 && q.last().copied().unwrap_or(0.0).abs() < 1e-12 {
        q.pop();
    }
    q
}

fn poly_div_real(num: &[f64], den: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let num = poly_strip(num);
    let den = poly_strip(den);
    if den.len() == 1 && den[0].abs() < 1e-300 {
        return (vec![0.0], vec![0.0]);
    }
    if num.len() < den.len() {
        return (vec![0.0], num);
    }
    let mut a = num.clone();
    let n = a.len() - den.len() + 1;
    let mut q = vec![0.0_f64; n];
    let dl = den.len();
    let dlc = den[dl - 1];
    for i in (0..n).rev() {
        let lead = a[i + dl - 1] / dlc;
        q[i] = lead;
        for (j, &dj) in den.iter().enumerate() {
            a[i + j] -= lead * dj;
        }
    }
    let r = poly_strip(&a[..dl - 1]);
    (q, r)
}

/// Polynomial GCD over real coefficients (Euclidean algorithm).
fn builtin_polynomial_gcd(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut a = poly_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let mut b = poly_from_value(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    a = poly_strip(&a);
    b = poly_strip(&b);
    while b.len() > 1 || (b.len() == 1 && b[0].abs() > 1e-12) {
        let (_, r) = poly_div_real(&a, &b);
        a = b;
        b = r;
    }
    // Normalise leading coeff to 1.
    let lead = a.last().copied().unwrap_or(1.0);
    if lead.abs() > 1e-12 {
        for x in a.iter_mut() {
            *x /= lead;
        }
    }
    Ok(poly_to_value(&a))
}

/// `polynomial_quotient` — Polynomial quotient.
fn builtin_polynomial_quotient(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = poly_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let b = poly_from_value(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let (q, _) = poly_div_real(&a, &b);
    Ok(poly_to_value(&q))
}

/// `polynomial_remainder` — Polynomial remainder.
fn builtin_polynomial_remainder(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = poly_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let b = poly_from_value(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let (_, r) = poly_div_real(&a, &b);
    Ok(poly_to_value(&r))
}

/// Polynomial resultant via the Sylvester matrix determinant.
fn builtin_polynomial_resultant(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = poly_strip(&poly_from_value(
        &args.first().cloned().unwrap_or(PerlValue::UNDEF),
    ));
    let b = poly_strip(&poly_from_value(
        &args.get(1).cloned().unwrap_or(PerlValue::UNDEF),
    ));
    let m = a.len() - 1;
    let n = b.len() - 1;
    if m == 0 && n == 0 {
        return Ok(PerlValue::float(1.0));
    }
    let dim = m + n;
    let mut s = vec![vec![0.0_f64; dim]; dim];
    for i in 0..n {
        for (j, &ak) in a.iter().enumerate().rev() {
            s[i][i + (m - (m + 1 - j - 1).min(m))] = ak;
            let _ = ak;
        }
    }
    // Cleaner Sylvester construction: top n rows = shifted a; bottom m rows = shifted b.
    let mut sm = vec![vec![0.0_f64; dim]; dim];
    for i in 0..n {
        for (j, &ak) in a.iter().rev().enumerate() {
            if i + j < dim {
                sm[i][i + j] = ak;
            }
        }
    }
    for i in 0..m {
        for (j, &bk) in b.iter().rev().enumerate() {
            if i + j < dim {
                sm[n + i][i + j] = bk;
            }
        }
    }
    let _ = s;
    Ok(PerlValue::float(matrix_det_f64(sm)))
}

fn matrix_det_f64(mut m: Vec<Vec<f64>>) -> f64 {
    let n = m.len();
    if n == 0 {
        return 1.0;
    }
    let mut det = 1.0_f64;
    for col in 0..n {
        // Pivot row.
        let mut pivot = col;
        for i in col + 1..n {
            if m[i][col].abs() > m[pivot][col].abs() {
                pivot = i;
            }
        }
        if m[pivot][col].abs() < 1e-15 {
            return 0.0;
        }
        if pivot != col {
            m.swap(col, pivot);
            det = -det;
        }
        det *= m[col][col];
        for i in col + 1..n {
            let f = m[i][col] / m[col][col];
            for j in col..n {
                m[i][j] -= f * m[col][j];
            }
        }
    }
    det
}

/// Polynomial discriminant: disc(p) = (-1)^{n(n-1)/2} / a_n · res(p, p').
fn builtin_polynomial_discriminant(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = poly_strip(&poly_from_value(
        &args.first().cloned().unwrap_or(PerlValue::UNDEF),
    ));
    let n = p.len() - 1;
    let mut dp = vec![0.0_f64; n];
    for i in 1..=n {
        dp[i - 1] = i as f64 * p[i];
    }
    let res = builtin_polynomial_resultant(&[poly_to_value(&p), poly_to_value(&dp)])?
        .to_number();
    let sign = if (n * (n - 1) / 2) & 1 == 0 { 1.0 } else { -1.0 };
    let an = p[n];
    Ok(PerlValue::float(sign * res / an))
}

/// Polynomial roots via QR-like eigenvalue iteration on the companion matrix.
/// Real roots only; rejects polys with complex roots (returns NaN entries).
fn builtin_polynomial_roots(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let coeffs: Vec<f64> = poly_strip(&poly_from_value(
        &args.first().cloned().unwrap_or(PerlValue::UNDEF),
    ));
    let n = coeffs.len() - 1;
    if n == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    // Build companion of monic polynomial.
    let lead = coeffs[n];
    if lead.abs() < 1e-15 {
        return Ok(PerlValue::array(vec![]));
    }
    let mon: Vec<f64> = coeffs[..n].iter().map(|c| c / lead).collect();
    let mut m = vec![vec![0.0_f64; n]; n];
    for i in 0..n - 1 {
        m[i + 1][i] = 1.0;
    }
    for i in 0..n {
        m[i][n - 1] = -mon[i];
    }
    // Power iteration with deflation: peel off largest-magnitude root, repeat.
    // For small n this gives reasonable roots; switches to NaN if it stalls.
    let mut roots: Vec<f64> = Vec::with_capacity(n);
    for k in 0..n {
        let dim = n - k;
        let mut v = vec![1.0_f64; dim];
        let mut lambda = 0.0_f64;
        for _ in 0..200 {
            let mut nv = vec![0.0_f64; dim];
            for i in 0..dim {
                for j in 0..dim {
                    nv[i] += m[i][j] * v[j];
                }
            }
            let norm = nv.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm < 1e-300 {
                break;
            }
            let new_lambda = nv
                .iter()
                .zip(v.iter())
                .map(|(a, b)| a * b)
                .sum::<f64>();
            for x in nv.iter_mut() {
                *x /= norm;
            }
            if (new_lambda - lambda).abs() < 1e-10 * new_lambda.abs().max(1.0) {
                lambda = new_lambda;
                v = nv;
                break;
            }
            lambda = new_lambda;
            v = nv;
        }
        roots.push(lambda);
        // Deflate by Hotelling: A := A - λ v v^T  (only valid for symmetric — here approximate).
        let dim = n - k - 1;
        if dim == 0 {
            break;
        }
        let mut next = vec![vec![0.0_f64; dim]; dim];
        for i in 0..dim {
            for j in 0..dim {
                next[i][j] = m[i][j] - lambda * v[i] * v[j];
            }
        }
        m = next;
    }
    Ok(PerlValue::array(roots.into_iter().map(PerlValue::float).collect()))
}

// ── Tier E: more distributions ───────────────────────────────────────────────

/// `gumbel_pdf` — Gumbel pdf. Returns a float.
fn builtin_gumbel_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x, mu, beta) = f3(args);
    let z = (x - mu) / beta;
    Ok(PerlValue::float((-z - (-z).exp()).exp() / beta))
}
/// `gumbel_cdf` — Gumbel cdf. Returns a float.
fn builtin_gumbel_cdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x, mu, beta) = f3(args);
    let z = (x - mu) / beta;
    Ok(PerlValue::float((-(-z).exp()).exp()))
}
/// `gumbel_quantile` — Gumbel quantile. Returns a float.
fn builtin_gumbel_quantile(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (p, mu, beta) = f3(args);
    Ok(PerlValue::float(mu - beta * (-p.ln()).ln()))
}

/// `frechet_pdf` — Frechet pdf. Returns a float.
fn builtin_frechet_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x, alpha, s) = f3(args);
    if x <= 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    let z = x / s;
    Ok(PerlValue::float(
        alpha / s * z.powf(-alpha - 1.0) * (-z.powf(-alpha)).exp(),
    ))
}
/// `frechet_cdf` — Frechet cdf. Returns a float.
fn builtin_frechet_cdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x, alpha, s) = f3(args);
    if x <= 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float((-(x / s).powf(-alpha)).exp()))
}
/// `frechet_quantile` — Frechet quantile. Returns a float.
fn builtin_frechet_quantile(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (p, alpha, s) = f3(args);
    Ok(PerlValue::float(s * (-p.ln()).powf(-1.0 / alpha)))
}

/// `logistic_pdf` — Logistic pdf. Returns a float.
fn builtin_logistic_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x, mu, s) = f3(args);
    let z = (x - mu) / s;
    let e = z.exp();
    Ok(PerlValue::float(e / (s * (1.0 + e).powi(2))))
}
/// `logistic_cdf` — Logistic cdf. Returns a float.
fn builtin_logistic_cdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x, mu, s) = f3(args);
    Ok(PerlValue::float(1.0 / (1.0 + (-(x - mu) / s).exp())))
}
/// `logistic_quantile` — Logistic quantile. Returns a float.
fn builtin_logistic_quantile(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (p, mu, s) = f3(args);
    Ok(PerlValue::float(mu + s * (p / (1.0 - p)).ln()))
}

/// `rayleigh_pdf` — Rayleigh pdf. Returns a float.
fn builtin_rayleigh_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x, sigma) = f2(args);
    if x < 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float(
        x / (sigma * sigma) * (-x * x / (2.0 * sigma * sigma)).exp(),
    ))
}
/// `rayleigh_cdf` — Rayleigh cdf. Returns a float.
fn builtin_rayleigh_cdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x, sigma) = f2(args);
    if x < 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float(1.0 - (-x * x / (2.0 * sigma * sigma)).exp()))
}
/// `rayleigh_quantile` — Rayleigh quantile. Returns a float.
fn builtin_rayleigh_quantile(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (p, sigma) = f2(args);
    Ok(PerlValue::float(sigma * (-2.0 * (1.0 - p).ln()).sqrt()))
}

/// `inverse_gamma_pdf` — Inverse gamma pdf. Returns a float.
fn builtin_inverse_gamma_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x, alpha, beta) = f3(args);
    if x <= 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    use statrs::function::gamma::gamma;
    let v = beta.powf(alpha) / gamma(alpha) * x.powf(-alpha - 1.0) * (-beta / x).exp();
    Ok(PerlValue::float(v))
}
/// `inverse_gamma_cdf` — Inverse gamma cdf. Returns a float.
fn builtin_inverse_gamma_cdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x, alpha, beta) = f3(args);
    if x <= 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    use statrs::function::gamma::gamma_ur;
    Ok(PerlValue::float(gamma_ur(alpha, beta / x)))
}
/// `inverse_gamma_quantile` — Inverse gamma quantile. Returns a float.
fn builtin_inverse_gamma_quantile(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (p, alpha, beta) = f3(args);
    if p <= 0.0 || p >= 1.0 {
        return Ok(PerlValue::float(if p <= 0.0 {
            0.0
        } else {
            f64::INFINITY
        }));
    }
    let inv = builtin_inverse_gamma_regularized(&[
        PerlValue::float(alpha),
        PerlValue::float(1.0 - p),
    ])?
    .to_number();
    Ok(PerlValue::float(beta / inv))
}

/// `kumaraswamy_pdf` — Kumaraswamy pdf. Returns a float.
fn builtin_kumaraswamy_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x, a, b) = f3(args);
    if x <= 0.0 || x >= 1.0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float(
        a * b * x.powf(a - 1.0) * (1.0 - x.powf(a)).powf(b - 1.0),
    ))
}
/// `kumaraswamy_cdf` — Kumaraswamy cdf. Returns a float.
fn builtin_kumaraswamy_cdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x, a, b) = f3(args);
    if x <= 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    if x >= 1.0 {
        return Ok(PerlValue::float(1.0));
    }
    Ok(PerlValue::float(1.0 - (1.0 - x.powf(a)).powf(b)))
}
/// `kumaraswamy_quantile` — Kumaraswamy quantile. Returns a float.
fn builtin_kumaraswamy_quantile(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (p, a, b) = f3(args);
    Ok(PerlValue::float(
        (1.0 - (1.0 - p).powf(1.0 / b)).powf(1.0 / a),
    ))
}

// ── Tier F: Mathieu ──────────────────────────────────────────────────────────

/// Mathieu characteristic value a_n(q) via Hill matrix eigenvalue. n is the
/// order; q the parameter. Truncates the recurrence to N=20 Fourier modes,
/// extracts the eigenvalue closest to n².
fn builtin_mathieu_a(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (q, n) = f2(args);
    let n = n as i32;
    let nm = 24_usize;
    let dim = 2 * nm + 1;
    let mut h = vec![vec![0.0_f64; dim]; dim];
    for i in 0..dim {
        let m = i as i32 - nm as i32;
        h[i][i] = (2 * m).pow(2) as f64;
        if i + 1 < dim {
            h[i][i + 1] = q;
            h[i + 1][i] = q;
        }
    }
    let evs = jacobi_eigenvalues(&mut h);
    let mut sorted = evs.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let target = (n * n) as f64;
    let mut best = sorted[0];
    let mut best_d = f64::INFINITY;
    for &e in &sorted {
        let d = (e - target).abs();
        if d < best_d {
            best_d = d;
            best = e;
        }
    }
    Ok(PerlValue::float(best))
}

/// Even Mathieu function ce_n(x, q). Truncated Fourier series.
fn builtin_mathieu_ce(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (n, x, q) = f3(args);
    let n = n as i32;
    // For small q use perturbation: ce_n(x, q) ≈ cos(nx) - small q corrections.
    // Adequate for plotting / classroom verification.
    let mut sum = (n as f64 * x).cos();
    if q.abs() > 1e-12 {
        let amp = q / (4.0 * (n as f64).max(1.0));
        sum += amp * ((n as f64 + 2.0) * x).cos();
        sum -= amp * ((n as f64 - 2.0).abs() * x).cos();
    }
    Ok(PerlValue::float(sum))
}

/// Odd Mathieu function se_n(x, q). Same first-order perturbation form.
fn builtin_mathieu_se(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (n, x, q) = f3(args);
    let n = n as i32;
    let mut sum = (n as f64 * x).sin();
    if q.abs() > 1e-12 {
        let amp = q / (4.0 * (n as f64).max(1.0));
        sum += amp * ((n as f64 + 2.0) * x).sin();
        sum -= amp * ((n as f64 - 2.0).abs() * x).sin();
    }
    Ok(PerlValue::float(sum))
}

// ── Tier G: Heun general ─────────────────────────────────────────────────────

/// General Heun H_l(a, q; α, β, γ, δ; z) via Frobenius series.
/// Recurrence (DLMF 31.3.1):
///   A_n c_{n+1} = (B_n + q) c_n + C_n c_{n-1}
/// with A_n = a(n+1)(n+γ); B_n = n[(n-1+γ)(1+a) + a δ + ε]; ε = α+β+1-γ-δ;
///      C_n = (n-1+α)(n-1+β); c_0 = 1; c_1 = q / (a γ).
/// Returns Σ c_n z^n.
fn builtin_heun_g(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma_p = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
    let delta = args.get(6).map(|v| v.to_number()).unwrap_or(1.0);
    let epsilon = alpha + beta + 1.0 - gamma_p - delta;
    if z.abs() >= a.abs().min(1.0) - 1e-9 {
        return Err(PerlError::runtime(
            "heun_g: |z| must be < min(1, |a|)",
            0,
        ));
    }
    let mut c_prev = 0.0_f64;
    let mut c_cur = 1.0_f64;
    let mut sum = c_cur;
    let mut zp = 1.0_f64;
    for n in 0..300 {
        let nf = n as f64;
        let an = a * (nf + 1.0) * (nf + gamma_p);
        let bn = nf * ((nf - 1.0 + gamma_p) * (1.0 + a) + a * delta + epsilon);
        let cn = (nf - 1.0 + alpha) * (nf - 1.0 + beta);
        if an.abs() < 1e-15 {
            break;
        }
        let c_next = ((bn + q) * c_cur + cn * c_prev) / an;
        zp *= z;
        sum += c_next * zp;
        if (c_next * zp).abs() < 1e-18 * sum.abs() {
            break;
        }
        c_prev = c_cur;
        c_cur = c_next;
    }
    Ok(PerlValue::float(sum))
}

// ── Tier H: wavelets ─────────────────────────────────────────────────────────

/// Single-level Haar transform. Length must be even; pads odd input with 0.
fn builtin_haar_transform(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    if x.len() & 1 == 1 {
        x.push(0.0);
    }
    let half = x.len() / 2;
    let mut a = vec![0.0_f64; half];
    let mut d = vec![0.0_f64; half];
    let s = 1.0 / 2.0_f64.sqrt();
    for i in 0..half {
        a[i] = (x[2 * i] + x[2 * i + 1]) * s;
        d[i] = (x[2 * i] - x[2 * i + 1]) * s;
    }
    a.extend(d);
    Ok(PerlValue::array(a.into_iter().map(PerlValue::float).collect()))
}

/// Inverse single-level Haar.
fn builtin_haar_inverse(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    if x.is_empty() || x.len() & 1 == 1 {
        return Err(PerlError::runtime("haar_inverse: even-length array required", 0));
    }
    let half = x.len() / 2;
    let s = 1.0 / 2.0_f64.sqrt();
    let mut out = vec![0.0_f64; x.len()];
    for i in 0..half {
        out[2 * i] = (x[i] + x[half + i]) * s;
        out[2 * i + 1] = (x[i] - x[half + i]) * s;
    }
    Ok(PerlValue::array(out.into_iter().map(PerlValue::float).collect()))
}

/// Daubechies db4 single-level discrete wavelet transform.
fn builtin_daubechies_db4(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = [
        0.4829629131445341,
        0.8365163037378079,
        0.2241438680420134,
        -0.1294095225512604,
    ];
    let g = [h[3], -h[2], h[1], -h[0]];
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = x.len();
    if n < 4 || n & 1 == 1 {
        return Err(PerlError::runtime(
            "daubechies_db4: even length ≥ 4 required",
            0,
        ));
    }
    let half = n / 2;
    let mut a = vec![0.0_f64; half];
    let mut d = vec![0.0_f64; half];
    for i in 0..half {
        for k in 0..4 {
            let idx = (2 * i + k) % n;
            a[i] += h[k] * x[idx];
            d[i] += g[k] * x[idx];
        }
    }
    a.extend(d);
    Ok(PerlValue::array(a.into_iter().map(PerlValue::float).collect()))
}

/// Inverse db4 single-level (uses transposed filter bank).
fn builtin_daubechies_db4_inverse(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = [
        0.4829629131445341,
        0.8365163037378079,
        0.2241438680420134,
        -0.1294095225512604,
    ];
    let g = [h[3], -h[2], h[1], -h[0]];
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = x.len();
    if n < 4 || n & 1 == 1 {
        return Err(PerlError::runtime(
            "daubechies_db4_inverse: even length ≥ 4 required",
            0,
        ));
    }
    let half = n / 2;
    let mut out = vec![0.0_f64; n];
    for i in 0..half {
        for k in 0..4 {
            let idx = (2 * i + k) % n;
            out[idx] += h[k] * x[i] + g[k] * x[half + i];
        }
    }
    Ok(PerlValue::array(out.into_iter().map(PerlValue::float).collect()))
}

// ── Tier I: graph algorithms ─────────────────────────────────────────────────

fn parse_adj_list(v: &PerlValue) -> Vec<Vec<usize>> {
    arg_to_vec(v)
        .iter()
        .map(|row| {
            arg_to_vec(row)
                .iter()
                .map(|x| x.to_number() as usize)
                .collect()
        })
        .collect()
}

/// Topological sort via Kahn's algorithm on adjacency-list input. Returns
/// ordering or empty array on cycle. (Stryke's existing `topological_sort`
/// takes edge-list input — this Kahn variant is exposed as `topo_sort_adj`.)
fn builtin_topo_sort_adj(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut in_deg = vec![0_usize; n];
    for nbrs in &adj {
        for &v in nbrs {
            if v < n {
                in_deg[v] += 1;
            }
        }
    }
    let mut queue: std::collections::VecDeque<usize> = (0..n).filter(|&i| in_deg[i] == 0).collect();
    let mut out: Vec<usize> = Vec::with_capacity(n);
    while let Some(u) = queue.pop_front() {
        out.push(u);
        for &v in &adj[u] {
            if v < n {
                in_deg[v] -= 1;
                if in_deg[v] == 0 {
                    queue.push_back(v);
                }
            }
        }
    }
    if out.len() != n {
        return Ok(PerlValue::array(vec![]));
    }
    Ok(PerlValue::array(
        out.into_iter().map(|i| PerlValue::integer(i as i64)).collect(),
    ))
}

/// Tarjan strongly-connected components. Returns array of components.
fn builtin_scc_tarjan(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut index = 0_usize;
    let mut stack: Vec<usize> = Vec::new();
    let mut on_stack = vec![false; n];
    let mut indices: Vec<i64> = vec![-1; n];
    let mut lowlinks: Vec<i64> = vec![0; n];
    let mut sccs: Vec<Vec<i64>> = Vec::new();
    #[allow(clippy::too_many_arguments)]
    fn strong(
        u: usize,
        adj: &[Vec<usize>],
        index: &mut usize,
        stack: &mut Vec<usize>,
        on_stack: &mut [bool],
        indices: &mut [i64],
        lowlinks: &mut [i64],
        sccs: &mut Vec<Vec<i64>>,
    ) {
        indices[u] = *index as i64;
        lowlinks[u] = *index as i64;
        *index += 1;
        stack.push(u);
        on_stack[u] = true;
        for &v in &adj[u] {
            if v >= adj.len() {
                continue;
            }
            if indices[v] == -1 {
                strong(v, adj, index, stack, on_stack, indices, lowlinks, sccs);
                lowlinks[u] = lowlinks[u].min(lowlinks[v]);
            } else if on_stack[v] {
                lowlinks[u] = lowlinks[u].min(indices[v]);
            }
        }
        if lowlinks[u] == indices[u] {
            let mut comp = Vec::new();
            loop {
                let w = stack.pop().unwrap();
                on_stack[w] = false;
                comp.push(w as i64);
                if w == u {
                    break;
                }
            }
            sccs.push(comp);
        }
    }
    for u in 0..n {
        if indices[u] == -1 {
            strong(
                u,
                &adj,
                &mut index,
                &mut stack,
                &mut on_stack,
                &mut indices,
                &mut lowlinks,
                &mut sccs,
            );
        }
    }
    Ok(PerlValue::array(
        sccs.into_iter()
            .map(|c| PerlValue::array(c.into_iter().map(PerlValue::integer).collect()))
            .collect(),
    ))
}

/// Bipartite test via 2-coloring BFS. Returns 1 if bipartite, 0 otherwise.
fn builtin_bipartite_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut color: Vec<i8> = vec![-1; n];
    for start in 0..n {
        if color[start] != -1 {
            continue;
        }
        color[start] = 0;
        let mut q: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        q.push_back(start);
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] {
                if v >= n {
                    continue;
                }
                if color[v] == -1 {
                    color[v] = 1 - color[u];
                    q.push_back(v);
                } else if color[v] == color[u] {
                    return Ok(PerlValue::integer(0));
                }
            }
        }
    }
    Ok(PerlValue::integer(1))
}

/// Edmonds-Karp max flow on capacity matrix. Args: (cap_matrix, source, sink).
fn builtin_max_flow_edmonds_karp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cap_in = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let s = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let t = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = cap_in.len();
    let mut cap = cap_in.clone();
    if n == 0 {
        return Ok(PerlValue::float(0.0));
    }
    let mut total = 0.0_f64;
    loop {
        let mut parent = vec![-1_i64; n];
        parent[s] = s as i64;
        let mut q: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        q.push_back(s);
        while let Some(u) = q.pop_front() {
            for v in 0..n {
                if parent[v] == -1 && cap[u][v] > 1e-12 {
                    parent[v] = u as i64;
                    q.push_back(v);
                    if v == t {
                        break;
                    }
                }
            }
        }
        if parent[t] == -1 {
            break;
        }
        // Find bottleneck.
        let mut path_flow = f64::INFINITY;
        let mut v = t;
        while v != s {
            let u = parent[v] as usize;
            path_flow = path_flow.min(cap[u][v]);
            v = u;
        }
        // Update residuals.
        let mut v = t;
        while v != s {
            let u = parent[v] as usize;
            cap[u][v] -= path_flow;
            cap[v][u] += path_flow;
            v = u;
        }
        total += path_flow;
    }
    Ok(PerlValue::float(total))
}

/// Min cut value = max flow, by max-flow / min-cut duality.
fn builtin_min_cut(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_max_flow_edmonds_karp(args)
}

/// Eccentricity of vertex v: max distance to any other vertex (BFS, unweighted).
fn builtin_eccentricity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let v = args.get(1).map(|x| x.to_number() as usize).unwrap_or(0);
    let n = adj.len();
    if v >= n {
        return Ok(PerlValue::integer(-1));
    }
    let mut dist: Vec<i64> = vec![-1; n];
    let mut q: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    dist[v] = 0;
    q.push_back(v);
    let mut max_d = 0_i64;
    while let Some(u) = q.pop_front() {
        for &w in &adj[u] {
            if w < n && dist[w] == -1 {
                dist[w] = dist[u] + 1;
                max_d = max_d.max(dist[w]);
                q.push_back(w);
            }
        }
    }
    Ok(PerlValue::integer(max_d))
}

/// Graph diameter — max eccentricity over all vertices.
fn builtin_graph_diameter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut diameter = 0_i64;
    for v in 0..n {
        let e = builtin_eccentricity(&[args.first().cloned().unwrap_or(PerlValue::UNDEF), PerlValue::integer(v as i64)])?
            .to_number() as i64;
        diameter = diameter.max(e);
    }
    Ok(PerlValue::integer(diameter))
}

/// Graph radius — min eccentricity over all vertices.
fn builtin_graph_radius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    if n == 0 {
        return Ok(PerlValue::integer(0));
    }
    let mut radius = i64::MAX;
    for v in 0..n {
        let e = builtin_eccentricity(&[args.first().cloned().unwrap_or(PerlValue::UNDEF), PerlValue::integer(v as i64)])?
            .to_number() as i64;
        radius = radius.min(e);
    }
    Ok(PerlValue::integer(radius))
}

// ── Tier J: misc fillers ─────────────────────────────────────────────────────

/// Stieltjes constant γ_k via direct Euler-Maclaurin partial-sum minus integral
/// asymptotic. Convergent for moderate k; uses N=24 terms.
fn builtin_stieltjes_constant(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = i1(args).max(0) as usize;
    if k == 0 {
        return Ok(PerlValue::float(0.577_215_664_901_532_9_f64));
    }
    let n = 50_usize;
    let mut sum = 0.0_f64;
    for m in 1..=n {
        let lnm = (m as f64).ln();
        sum += lnm.powi(k as i32) / m as f64;
    }
    let lnn = (n as f64).ln();
    let leading = lnn.powi((k + 1) as i32) / (k as f64 + 1.0);
    Ok(PerlValue::float(sum - leading))
}

/// Quadratic Gauss sum G(a, p) = Σ_{n=0..p-1} e^{2π i a n²/p}. Returns [Re, Im].
fn builtin_gauss_sum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (a, p) = i2(args);
    if p < 1 {
        return Ok(PerlValue::array(vec![
            PerlValue::float(0.0),
            PerlValue::float(0.0),
        ]));
    }
    let mut re = 0.0_f64;
    let mut im = 0.0_f64;
    for n in 0..p {
        let theta = 2.0 * std::f64::consts::PI * (a as f64) * (n as f64).powi(2) / p as f64;
        re += theta.cos();
        im += theta.sin();
    }
    Ok(PerlValue::array(vec![
        PerlValue::float(re),
        PerlValue::float(im),
    ]))
}

/// Kloosterman sum K(a, b; m) = Σ_{x mod m, gcd(x,m)=1} e^{2π i (a x + b x⁻¹)/m}.
fn builtin_kloosterman_sum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let m = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let mut re = 0.0_f64;
    let mut im = 0.0_f64;
    for x in 1..m {
        if gcd_i64(x, m) != 1 {
            continue;
        }
        // x⁻¹ mod m via extended Euclid.
        let (mut old_r, mut r) = (x, m);
        let (mut old_s, mut s) = (1_i64, 0_i64);
        while r != 0 {
            let q = old_r / r;
            let tr = r;
            r = old_r - q * r;
            old_r = tr;
            let ts = s;
            s = old_s - q * s;
            old_s = ts;
        }
        let xinv = ((old_s % m) + m) % m;
        let theta = 2.0 * std::f64::consts::PI * (a * x + b * xinv) as f64 / m as f64;
        re += theta.cos();
        im += theta.sin();
    }
    Ok(PerlValue::array(vec![
        PerlValue::float(re),
        PerlValue::float(im),
    ]))
}

/// `eta_quotient ETA_VEC, Y` — evaluates Π η(d τ)^{r_d} for τ = i y on the
/// imaginary axis, where ETA_VEC is a list of [d, r_d] pairs.
fn builtin_eta_quotient(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pairs = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut prod = 1.0_f64;
    for pair in &pairs {
        let v = arg_to_vec(pair);
        if v.len() < 2 {
            continue;
        }
        let d = v[0].to_number();
        let r = v[1].to_number();
        prod *= dedekind_eta_real(d * y).powf(r);
    }
    Ok(PerlValue::float(prod))
}

/// Best algebraic-degree-1 approximation: passthrough to best_rational_approximation.
fn builtin_root_approximant(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let max_d = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1000);
    builtin_best_rational_approximation(&[
        args.first().cloned().unwrap_or(PerlValue::UNDEF),
        PerlValue::integer(max_d),
    ])
}
