// cryptanalysis, number theory deep, factorization, modular arithmetic.

// Modular exponentiation a^b mod n (i64-safe via u128)

// Modular inverse (extended Euclidean)

// Carmichael function λ(n) for prime powers (simplified)

// Quadratic residue test (Legendre symbol)

// Jacobi symbol (a/n)

// Tonelli-Shanks square root mod p (prime)

// Multiplicative order of a mod n

// Discrete log baby-step giant-step (m steps, n = order)
fn builtin_bsgs_discrete_log(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g = i1(args);
    let h = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let p = args.get(2).map(|v| v.to_number() as i64).unwrap_or(2);
    if p <= 1 { return Ok(StrykeValue::integer(-1)); }
    let m = ((p as f64).sqrt() as i64) + 1;
    let mut table: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    let mut e = 1_i64;
    for j in 0..m {
        table.entry(e).or_insert(j);
        e = (e * g) % p;
    }
    fn pow_mod(mut b: i128, mut e: i128, m: i128) -> i128 {
        let mut r = 1_i128; b = b.rem_euclid(m);
        while e > 0 { if e & 1 == 1 { r = r * b % m; } e >>= 1; b = b * b % m; } r
    }
    let factor = pow_mod(g as i128, ((p - 2) * m) as i128, p as i128) as i64;
    let mut gamma = h;
    for i in 0..m {
        if let Some(&j) = table.get(&gamma) {
            return Ok(StrykeValue::integer(i * m + j));
        }
        gamma = (gamma * factor).rem_euclid(p);
    }
    Ok(StrykeValue::integer(-1))
}

// Pollard rho factorization

// Pollard p-1 factorization (B-smoothness)
fn builtin_pollard_p_minus_1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(100);
    if n <= 3 { return Ok(StrykeValue::integer(n)); }
    let mut a = 2_i128;
    let n128 = n as i128;
    fn gcd(a: i128, b: i128) -> i128 { if b == 0 { a.abs() } else { gcd(b, a % b) } }
    for j in 2..=b {
        a = pow_mod_helper(a, j as i128, n128);
        let g = gcd(a - 1, n128);
        if g > 1 && g < n128 { return Ok(StrykeValue::integer(g as i64)); }
    }
    Ok(StrykeValue::integer(0))
}
fn pow_mod_helper(mut base: i128, mut exp: i128, m: i128) -> i128 {
    let mut r = 1_i128; base = base.rem_euclid(m);
    while exp > 0 { if exp & 1 == 1 { r = r * base % m; } exp >>= 1; base = base * base % m; } r
}

// Fermat factorization (slow, for n = pq with p,q close)
fn builtin_fermat_factor(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n <= 0 { return Ok(StrykeValue::integer(0)); }
    let mut a = (n as f64).sqrt().ceil() as i64;
    let limit = a + 100000;
    while a <= limit {
        let b_sq = a * a - n;
        if b_sq >= 0 {
            let b = (b_sq as f64).sqrt() as i64;
            if b * b == b_sq {
                return Ok(StrykeValue::integer(a - b));
            }
        }
        a += 1;
    }
    Ok(StrykeValue::integer(0))
}

// Trial division smallest prime factor
fn builtin_trial_smallest_factor(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n <= 1 { return Ok(StrykeValue::integer(n)); }
    if n % 2 == 0 { return Ok(StrykeValue::integer(2)); }
    let mut p = 3_i64;
    while p * p <= n {
        if n % p == 0 { return Ok(StrykeValue::integer(p)); }
        p += 2;
    }
    Ok(StrykeValue::integer(n))
}

// Sum of divisors σ(n)

// Number of divisors d(n)

// Möbius function μ(n)

// Mertens function M(n) = Σ μ(k)
fn builtin_mertens(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(1);
    let mut total = 0_i64;
    for k in 1..=n {
        let mut mu = 0_i64;
        let mut m = k; let mut prime_count = 0_i64; let mut squarefree = true;
        let mut p = 2_i64;
        while p * p <= m {
            if m % p == 0 {
                m /= p;
                if m % p == 0 { squarefree = false; break; }
                prime_count += 1;
            }
            p += 1;
        }
        if squarefree {
            if m > 1 { prime_count += 1; }
            mu = if prime_count % 2 == 0 { 1 } else { -1 };
            if k == 1 { mu = 1; }
        }
        total += mu;
    }
    Ok(StrykeValue::integer(total))
}

// von Mangoldt Λ(n)

// Liouville λ(n)
fn builtin_liouville(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut n = i1(args).max(1);
    let mut prime_count = 0_i64;
    let mut p = 2_i64;
    while p * p <= n {
        while n % p == 0 { n /= p; prime_count += 1; }
        p += 1;
    }
    if n > 1 { prime_count += 1; }
    Ok(StrykeValue::integer(if prime_count % 2 == 0 { 1 } else { -1 }))
}

// Squarefree predicate

// Smooth number check (B-smooth)
fn builtin_is_b_smooth(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut n = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(10);
    if n <= 0 { return Ok(StrykeValue::integer(0)); }
    let mut p = 2_i64;
    while p <= b {
        while n % p == 0 { n /= p; }
        p += 1;
    }
    Ok(StrykeValue::integer(if n == 1 { 1 } else { 0 }))
}

// Primorial p_n# = product of first n primes
fn builtin_primorial_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let mut count = 0_i64;
    let mut prod = 1_i128;
    let mut p = 2_i64;
    fn is_prime(n: i64) -> bool {
        if n < 2 { return false; }
        if n % 2 == 0 { return n == 2; }
        let mut i = 3_i64;
        while i * i <= n {
            if n % i == 0 { return false; }
            i += 2;
        }
        true
    }
    while count < n {
        if is_prime(p) {
            prod *= p as i128;
            count += 1;
        }
        p += 1;
    }
    Ok(StrykeValue::integer(prod as i64))
}

// Catalan's pseudoprime base 2 test
fn builtin_pseudoprime_base2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n < 4 { return Ok(StrykeValue::integer(0)); }
    fn pow_mod(mut b: i128, mut e: i128, m: i128) -> i128 {
        let mut r = 1_i128; b = b.rem_euclid(m);
        while e > 0 { if e & 1 == 1 { r = r * b % m; } e >>= 1; b = b * b % m; } r
    }
    Ok(StrykeValue::integer(if pow_mod(2, (n - 1) as i128, n as i128) == 1 { 1 } else { 0 }))
}

// Strong pseudoprime test for base a
fn builtin_strong_pseudoprime(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let a = args.get(1).map(|v| v.to_number() as i64).unwrap_or(2);
    if n < 3 || n % 2 == 0 { return Ok(StrykeValue::integer(0)); }
    let n128 = n as i128;
    let mut d = (n - 1) as i128;
    let mut r = 0_u32;
    while d % 2 == 0 { d /= 2; r += 1; }
    fn pow_mod(mut b: i128, mut e: i128, m: i128) -> i128 {
        let mut r = 1_i128; b = b.rem_euclid(m);
        while e > 0 { if e & 1 == 1 { r = r * b % m; } e >>= 1; b = b * b % m; } r
    }
    let mut x = pow_mod(a as i128, d, n128);
    if x == 1 || x == n128 - 1 { return Ok(StrykeValue::integer(1)); }
    for _ in 0..r - 1 {
        x = x * x % n128;
        if x == n128 - 1 { return Ok(StrykeValue::integer(1)); }
    }
    Ok(StrykeValue::integer(0))
}

// Carmichael number test

// Lucas-Lehmer test for Mersenne primes (p prime → 2^p-1)

// AKS primality witness step: tests whether (X + a)^n ≡ X^n + a (mod X^r - 1, n)
// holds for given (n, r, a). Uses square-and-multiply over the ring R = ℤ_n[X]/(X^r-1).
// Returns 1 if the congruence holds (n passes the step), 0 if a counter-example.
fn builtin_aks_witness_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(2);
    let r = args.get(1).map(|v| v.to_number() as i64).unwrap_or(7).max(2);
    let a = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1).rem_euclid(n);
    let r_us = r as usize;
    let mut base = vec![0_i128; r_us];
    base[1] = 1;
    base[0] = a as i128;
    let n_mod = n as i128;
    fn mul_mod(p: &[i128], q: &[i128], r: usize, m: i128) -> Vec<i128> {
        let mut out = vec![0_i128; r];
        for i in 0..r {
            if p[i] == 0 { continue; }
            for j in 0..r {
                if q[j] == 0 { continue; }
                let dst = (i + j) % r;
                out[dst] = (out[dst] + p[i] * q[j]).rem_euclid(m);
            }
        }
        out
    }
    let mut result = vec![0_i128; r_us];
    result[0] = 1;
    let mut b = base.clone();
    let mut e = n;
    while e > 0 {
        if e & 1 == 1 { result = mul_mod(&result, &b, r_us, n_mod); }
        e >>= 1;
        if e > 0 { b = mul_mod(&b, &b, r_us, n_mod); }
    }
    let mut expected = vec![0_i128; r_us];
    expected[(n as usize) % r_us] = 1;
    expected[0] = (expected[0] + a as i128).rem_euclid(n_mod);
    Ok(StrykeValue::integer(if result == expected { 1 } else { 0 }))
}

// Quadratic sieve smoothness (return 1 if x² mod n is B-smooth)
fn builtin_qs_relation(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let b = args.get(2).map(|v| v.to_number() as i64).unwrap_or(10);
    if n <= 0 { return Ok(StrykeValue::integer(0)); }
    let mut y = (x * x).rem_euclid(n);
    let mut p = 2_i64;
    while p <= b {
        while y % p == 0 { y /= p; }
        p += 1;
    }
    Ok(StrykeValue::integer(if y == 1 { 1 } else { 0 }))
}

// Index calculus easy case (small group)
fn builtin_index_calculus_naive(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g = i1(args);
    let h = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let p = args.get(2).map(|v| v.to_number() as i64).unwrap_or(2);
    if p <= 1 { return Ok(StrykeValue::integer(-1)); }
    let mut cur = 1_i64;
    for k in 0..p {
        if cur == h.rem_euclid(p) { return Ok(StrykeValue::integer(k)); }
        cur = (cur * g).rem_euclid(p);
    }
    Ok(StrykeValue::integer(-1))
}

// LLL reduction one-pass (reduces 2x2 lattice basis vectors)
fn builtin_lll_2x2_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let b1: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let b2: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    if b1.len() < 2 || b2.len() < 2 {
        return Ok(StrykeValue::array(vec![StrykeValue::array(vec![]), StrykeValue::array(vec![])]));
    }
    let n1 = b1[0] * b1[0] + b1[1] * b1[1];
    if n1 == 0.0 { return Ok(StrykeValue::array(vec![StrykeValue::array(vec![]), StrykeValue::array(vec![])])); }
    let mu = (b1[0] * b2[0] + b1[1] * b2[1]) / n1;
    let mu_round = mu.round();
    let new_b2 = vec![b2[0] - mu_round * b1[0], b2[1] - mu_round * b1[1]];
    Ok(StrykeValue::array(vec![
        StrykeValue::array(b1.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::array(new_b2.into_iter().map(StrykeValue::float).collect()),
    ]))
}

// Coppersmith's theorem (1996): for a monic polynomial f(X) ∈ ℤ[X] of degree d
// reduced mod N, there is a polynomial-time algorithm (LLL-based) that finds
// every root x₀ ∈ ℤ with |x₀| < N^(1/d) − ε. This returns the exact bound N^(1/d).
fn builtin_coppersmith_bound(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let degree = args.get(1).map(|v| v.to_number()).unwrap_or(2.0).max(1.0);
    Ok(StrykeValue::float(n.powf(1.0 / degree)))
}

// Shor period-finding measurement probability for r | period
fn builtin_shor_period_prob(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if q == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((1.0 / r) * (q / r).floor() / q))
}

// RSA key exponent inverse e * d ≡ 1 mod φ(n)
fn builtin_rsa_d_from_e(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let e = i1(args);
    let phi_n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    fn ext_gcd(a: i64, b: i64) -> (i64, i64, i64) {
        if a == 0 { (b, 0, 1) }
        else { let (g, x1, y1) = ext_gcd(b % a, a); (g, y1 - (b / a) * x1, x1) }
    }
    let (g, x, _) = ext_gcd(e.rem_euclid(phi_n), phi_n);
    if g != 1 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(x.rem_euclid(phi_n)))
}

// Diffie-Hellman shared secret
fn builtin_dh_secret(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_mod_exp(args)
}

// ElGamal encryption pair (g^k, h * y^k)
fn builtin_elgamal_encrypt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g = i1(args);
    let h = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let y = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let k = args.get(3).map(|v| v.to_number() as i64).unwrap_or(1);
    let p = args.get(4).map(|v| v.to_number() as i64).unwrap_or(2);
    fn pow_mod(mut b: i128, mut e: i128, m: i128) -> i128 {
        let mut r = 1_i128; b = b.rem_euclid(m);
        while e > 0 { if e & 1 == 1 { r = r * b % m; } e >>= 1; b = b * b % m; } r
    }
    let c1 = pow_mod(g as i128, k as i128, p as i128) as i64;
    let c2 = ((h as i128 * pow_mod(y as i128, k as i128, p as i128)).rem_euclid(p as i128)) as i64;
    Ok(StrykeValue::array(vec![StrykeValue::integer(c1), StrykeValue::integer(c2)]))
}

// ECC point doubling on y² = x³ + ax + b (over GF(p))
fn builtin_ecc_point_double(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = i1(args);
    let y = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let a = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let p = args.get(3).map(|v| v.to_number() as i64).unwrap_or(2);
    if y == 0 || p <= 1 { return Ok(StrykeValue::array(vec![StrykeValue::integer(0), StrykeValue::integer(0)])); }
    fn ext_gcd(a: i64, b: i64) -> (i64, i64, i64) {
        if a == 0 { (b, 0, 1) }
        else { let (g, x1, y1) = ext_gcd(b % a, a); (g, y1 - (b / a) * x1, x1) }
    }
    let (_, inv_2y, _) = ext_gcd((2 * y).rem_euclid(p), p);
    let lambda = ((3 * x * x + a) * inv_2y).rem_euclid(p);
    let x3 = (lambda * lambda - 2 * x).rem_euclid(p);
    let y3 = (lambda * (x - x3) - y).rem_euclid(p);
    Ok(StrykeValue::array(vec![StrykeValue::integer(x3), StrykeValue::integer(y3)]))
}

// Continued fraction expansion of √n (first k terms)
fn builtin_continued_fraction_sqrt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(10);
    let a0 = (n as f64).sqrt() as i64;
    if a0 * a0 == n { return Ok(StrykeValue::array(vec![StrykeValue::integer(a0)])); }
    let mut out = vec![StrykeValue::integer(a0)];
    let mut m = 0_i64; let mut d = 1_i64; let mut a = a0;
    for _ in 0..k - 1 {
        m = d * a - m;
        d = (n - m * m) / d.max(1);
        if d == 0 { break; }
        a = (a0 + m) / d;
        out.push(StrykeValue::integer(a));
    }
    Ok(StrykeValue::array(out))
}

// Pell equation x² - n·y² = 1 fundamental solution (small n)
fn builtin_pell_fundamental(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n <= 0 { return Ok(StrykeValue::array(vec![StrykeValue::integer(1), StrykeValue::integer(0)])); }
    let sqrt_n = (n as f64).sqrt() as i64;
    if sqrt_n * sqrt_n == n { return Ok(StrykeValue::array(vec![StrykeValue::integer(1), StrykeValue::integer(0)])); }
    let mut h_prev = 1_i128; let mut h = sqrt_n as i128;
    let mut k_prev = 0_i128; let mut k = 1_i128;
    let mut m = 0_i128; let mut d = 1_i128; let mut a = sqrt_n as i128;
    let n128 = n as i128;
    for _ in 0..200 {
        if h * h - n128 * k * k == 1 { return Ok(StrykeValue::array(vec![StrykeValue::integer(h as i64), StrykeValue::integer(k as i64)])); }
        m = d * a - m;
        d = (n128 - m * m) / d.max(1);
        if d == 0 { break; }
        a = (sqrt_n as i128 + m) / d;
        let h_new = a * h + h_prev;
        let k_new = a * k + k_prev;
        h_prev = h; h = h_new;
        k_prev = k; k = k_new;
    }
    Ok(StrykeValue::array(vec![StrykeValue::integer(h as i64), StrykeValue::integer(k as i64)]))
}

// Sum of two squares representation (Gaussian integers)
fn builtin_sum_two_squares(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let limit = (n as f64).sqrt() as i64;
    for a in 0..=limit {
        let b_sq = n - a * a;
        if b_sq < 0 { break; }
        let b = (b_sq as f64).sqrt() as i64;
        if b * b == b_sq {
            return Ok(StrykeValue::array(vec![StrykeValue::integer(a), StrykeValue::integer(b)]));
        }
    }
    Ok(StrykeValue::integer(0))
}

// Minkowski's class-number bound for a number field K of degree n with
// discriminant d_K and r₂ complex places:
//   h(K) ≤ M_K = (n!/nⁿ) · (4/π)^{r₂} · √|d_K|.
// For an imaginary quadratic field K = ℚ(√−d), n = 2 and r₂ = 1, giving
//   M_K = (2/π) · √d.
// Args: |d|, degree n, number of complex places r₂.
fn builtin_class_number_bound(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d = f1(args).abs();
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(2.0).max(1.0);
    let r2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(0.0);
    if d <= 0.0 { return Ok(StrykeValue::integer(1)); }
    let mut fact = 1.0_f64;
    for k in 2..=(n as i64) { fact *= k as f64; }
    let factor = fact / n.powf(n);
    let pi_term = (4.0 / std::f64::consts::PI).powf(r2);
    let bound = factor * pi_term * d.sqrt();
    Ok(StrykeValue::integer(bound.ceil() as i64))
}

// Smith normal form reduction (1 step on 2x2 integer matrix)
fn builtin_smith_normal_2x2_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    if m.len() < 2 || m[0].len() < 2 { return Ok(StrykeValue::array(vec![])); }
    let a = m[0][0]; let b = m[0][1]; let c = m[1][0]; let d = m[1][1];
    let det = a * d - b * c;
    Ok(StrykeValue::array(vec![StrykeValue::float(1.0), StrykeValue::float(det)]))
}

// Stark unit (for heuristic class group computations)
fn builtin_regulator_naive(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d = f1(args);
    if d <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(d.ln()))
}

// Power-residue check x^(N-1) mod N for fixed base
fn builtin_power_residue_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    if n <= 1 { return Ok(StrykeValue::integer(0)); }
    fn pow_mod(mut b: i128, mut e: i128, m: i128) -> i128 {
        let mut r = 1_i128; b = b.rem_euclid(m);
        while e > 0 { if e & 1 == 1 { r = r * b % m; } e >>= 1; b = b * b % m; } r
    }
    Ok(StrykeValue::integer(if pow_mod(x as i128, (n - 1) as i128, n as i128) == 1 { 1 } else { 0 }))
}

// Wieferich-like prime test
fn builtin_wieferich_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = i1(args);
    if p <= 2 { return Ok(StrykeValue::integer(0)); }
    fn pow_mod(mut b: i128, mut e: i128, m: i128) -> i128 {
        let mut r = 1_i128; b = b.rem_euclid(m);
        while e > 0 { if e & 1 == 1 { r = r * b % m; } e >>= 1; b = b * b % m; } r
    }
    let p_sq = (p as i128) * (p as i128);
    Ok(StrykeValue::integer(if pow_mod(2, (p - 1) as i128, p_sq) == 1 { 1 } else { 0 }))
}

// Wilson's theorem ((p-1)! ≡ -1 mod p) test
fn builtin_wilson_test(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = i1(args);
    if p < 2 { return Ok(StrykeValue::integer(0)); }
    let mut fact = 1_i128;
    for i in 1..p { fact = (fact * i as i128).rem_euclid(p as i128); }
    Ok(StrykeValue::integer(if fact == (p - 1) as i128 { 1 } else { 0 }))
}

// Goldbach decomposition (find one p+q = n for even n)
fn builtin_goldbach_pair(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n < 4 || !n % 2 == 0 { return Ok(StrykeValue::integer(0)); }
    fn is_prime(n: i64) -> bool {
        if n < 2 { return false; }
        if n % 2 == 0 { return n == 2; }
        let mut i = 3; while i * i <= n { if n % i == 0 { return false; } i += 2; }
        true
    }
    for p in 2..=n / 2 {
        if is_prime(p) && is_prime(n - p) {
            return Ok(StrykeValue::array(vec![StrykeValue::integer(p), StrykeValue::integer(n - p)]));
        }
    }
    Ok(StrykeValue::integer(0))
}

// Frequency analysis distance from English (chi-squared)
fn builtin_english_likeness(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let mut counts = vec![0_f64; 26];
    let mut total = 0_f64;
    for c in s.chars().filter(|c| c.is_ascii_uppercase()) {
        counts[(c as usize) - 'A' as usize] += 1.0;
        total += 1.0;
    }
    if total == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    let english = [0.0817, 0.0149, 0.0278, 0.0425, 0.1270, 0.0223, 0.0202, 0.0609,
        0.0697, 0.0015, 0.0077, 0.0403, 0.0241, 0.0675, 0.0751, 0.0193,
        0.0010, 0.0599, 0.0633, 0.0906, 0.0276, 0.0098, 0.0236, 0.0015, 0.0197, 0.0007];
    let mut chi = 0.0;
    for i in 0..26 {
        if english[i] > 0.0 {
            let obs = counts[i] / total;
            chi += (obs - english[i]).powi(2) / english[i];
        }
    }
    Ok(StrykeValue::float(chi))
}

// XOR cipher break: best single-byte key by lowest English chi²
fn builtin_xor_break_singlebyte(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let bytes: Vec<u8> = s.bytes().collect();
    let english = [0.0817, 0.0149, 0.0278, 0.0425, 0.1270, 0.0223, 0.0202, 0.0609,
        0.0697, 0.0015, 0.0077, 0.0403, 0.0241, 0.0675, 0.0751, 0.0193,
        0.0010, 0.0599, 0.0633, 0.0906, 0.0276, 0.0098, 0.0236, 0.0015, 0.0197, 0.0007];
    let mut best_key = 0_u8;
    let mut best_chi = f64::INFINITY;
    for k in 0..=255_u8 {
        let mut counts = vec![0_f64; 26];
        let mut total = 0_f64;
        for &b in &bytes {
            let c = (b ^ k) as char;
            if c.is_ascii_alphabetic() {
                counts[c.to_ascii_uppercase() as usize - 'A' as usize] += 1.0;
                total += 1.0;
            }
        }
        if total == 0.0 { continue; }
        let mut chi = 0.0;
        for i in 0..26 {
            if english[i] > 0.0 {
                let obs = counts[i] / total;
                chi += (obs - english[i]).powi(2) / english[i];
            }
        }
        if chi < best_chi { best_chi = chi; best_key = k; }
    }
    Ok(StrykeValue::integer(best_key as i64))
}

// Hamming weight (popcount)

// Bit reverse 32

// Bit reverse 64
fn builtin_bit_reverse_64(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = i1(args) as u64;
    Ok(StrykeValue::integer(x.reverse_bits() as i64))
}

// Trailing zeros count

// Leading zeros count

// Galois field GF(2^8) multiply (AES-like)
fn builtin_gf256_multiply(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut a = i1(args) as u8;
    let mut b = args.get(1).map(|v| v.to_number() as u8).unwrap_or(0);
    let mut p = 0_u8;
    for _ in 0..8 {
        if b & 1 != 0 { p ^= a; }
        let high_bit = a & 0x80;
        a <<= 1;
        if high_bit != 0 { a ^= 0x1b; }
        b >>= 1;
    }
    Ok(StrykeValue::integer(p as i64))
}

// Hash combiner (boost-style)
fn builtin_hash_combine(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h1 = i1(args) as u64;
    let h2 = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    let combined = h1 ^ (h2.wrapping_add(0x9e3779b9).wrapping_add(h1 << 6).wrapping_add(h1 >> 2));
    Ok(StrykeValue::integer(combined as i64))
}
