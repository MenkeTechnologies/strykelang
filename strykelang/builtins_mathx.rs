//! Math / number theory / random extras (Phase 1, batch 6).
//!
//! Pure functions over i64/u64 + a handful of `rand` / `statrs`-backed
//! distribution samplers. Naming follows the audited proposal.

use crate::value::StrykeValue;

#[allow(dead_code)]
fn arg_str(args: &[StrykeValue]) -> String {
    args.first().map(|v| v.to_string()).unwrap_or_default()
}

fn arg_i64(args: &[StrykeValue], idx: usize) -> Option<i64> {
    args.get(idx).map(|v| v.to_int())
}

fn arg_u64(args: &[StrykeValue], idx: usize) -> Option<u64> {
    args.get(idx).map(|v| v.to_int().max(0) as u64)
}

fn arg_f64(args: &[StrykeValue], idx: usize) -> Option<f64> {
    args.get(idx).map(|v| v.to_number())
}

// ══════════════════════════════════════════════════════════════════════
// Number theory
// ══════════════════════════════════════════════════════════════════════

/// `extended_gcd(A, B)` — `(gcd, x, y)` with `A*x + B*y == gcd`.
/// Returns `[gcd, x, y]` as an arrayref.
pub fn extended_gcd(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use std::sync::Arc;
    let (a, b) = (
        arg_i64(args, 0).unwrap_or(0),
        arg_i64(args, 1).unwrap_or(0),
    );
    fn egcd(a: i64, b: i64) -> (i64, i64, i64) {
        if b == 0 {
            (a.abs(), if a < 0 { -1 } else { 1 }, 0)
        } else {
            let (g, x1, y1) = egcd(b, a.rem_euclid(b));
            (g, y1, x1 - (a.div_euclid(b)) * y1)
        }
    }
    let (g, x, y) = egcd(a, b);
    let arr = vec![
        StrykeValue::integer(g),
        StrykeValue::integer(x),
        StrykeValue::integer(y),
    ];
    StrykeValue::array_ref(Arc::new(RwLock::new(arr)))
}

/// `modinverse(A, M)` — modular multiplicative inverse of A mod M, or
/// undef if `gcd(A, M) != 1`.
pub fn modinverse(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_i64(args, 0).unwrap_or(0);
    let m = arg_i64(args, 1).unwrap_or(0);
    if m == 0 {
        return StrykeValue::UNDEF;
    }
    fn egcd(a: i64, b: i64) -> (i64, i64, i64) {
        if b == 0 {
            (a.abs(), if a < 0 { -1 } else { 1 }, 0)
        } else {
            let (g, x1, y1) = egcd(b, a.rem_euclid(b));
            (g, y1, x1 - (a.div_euclid(b)) * y1)
        }
    }
    let (g, x, _) = egcd(a, m);
    if g != 1 {
        return StrykeValue::UNDEF;
    }
    StrykeValue::integer(x.rem_euclid(m))
}

/// `modpow(BASE, EXP, MOD)` — fast `base^exp mod m`. Handles large exponents.
pub fn modpow(args: &[StrykeValue]) -> StrykeValue {
    let base = arg_i64(args, 0).unwrap_or(0);
    let exp = arg_i64(args, 1).unwrap_or(0);
    let m = arg_i64(args, 2).unwrap_or(0);
    if m == 0 {
        return StrykeValue::UNDEF;
    }
    if exp < 0 {
        return StrykeValue::UNDEF;
    }
    let mut result: i128 = 1;
    let mut base = (base as i128).rem_euclid(m as i128);
    let mut exp = exp as u64;
    let m128 = m as i128;
    while exp > 0 {
        if exp & 1 == 1 {
            result = (result * base).rem_euclid(m128);
        }
        exp >>= 1;
        base = (base * base).rem_euclid(m128);
    }
    StrykeValue::integer(result as i64)
}

/// `modular_sqrt(A, P)` — Tonelli–Shanks for `x^2 ≡ a (mod p)` with
/// odd prime p. Returns undef if no solution. Returns the smaller root.
pub fn modular_sqrt(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_i64(args, 0).unwrap_or(0).rem_euclid(arg_i64(args, 1).unwrap_or(1));
    let p = arg_i64(args, 1).unwrap_or(0);
    if p < 2 {
        return StrykeValue::UNDEF;
    }
    if p == 2 {
        return StrykeValue::integer(a.rem_euclid(2));
    }
    // Euler's criterion to check non-residue
    let crit_args = [
        StrykeValue::integer(a),
        StrykeValue::integer((p - 1) / 2),
        StrykeValue::integer(p),
    ];
    let crit = modpow(&crit_args).to_int();
    if crit == p - 1 {
        return StrykeValue::UNDEF;
    }
    if p % 4 == 3 {
        // Simple case: x = a^((p+1)/4) mod p
        let r_args = [
            StrykeValue::integer(a),
            StrykeValue::integer((p + 1) / 4),
            StrykeValue::integer(p),
        ];
        return modpow(&r_args);
    }
    // General Tonelli–Shanks
    let mut q = p - 1;
    let mut s: i64 = 0;
    while q % 2 == 0 {
        q /= 2;
        s += 1;
    }
    // Find a quadratic non-residue z
    let mut z: i64 = 2;
    while modpow(&[
        StrykeValue::integer(z),
        StrykeValue::integer((p - 1) / 2),
        StrykeValue::integer(p),
    ])
    .to_int()
        != p - 1
    {
        z += 1;
        if z >= p {
            return StrykeValue::UNDEF;
        }
    }
    let mut m: i64 = s;
    let mut c = modpow(&[
        StrykeValue::integer(z),
        StrykeValue::integer(q),
        StrykeValue::integer(p),
    ])
    .to_int();
    let mut t = modpow(&[
        StrykeValue::integer(a),
        StrykeValue::integer(q),
        StrykeValue::integer(p),
    ])
    .to_int();
    let mut r = modpow(&[
        StrykeValue::integer(a),
        StrykeValue::integer((q + 1) / 2),
        StrykeValue::integer(p),
    ])
    .to_int();
    loop {
        if t == 1 {
            return StrykeValue::integer(r.min(p - r));
        }
        let mut i: i64 = 0;
        let mut tmp = t;
        while tmp != 1 && i < m {
            tmp = (tmp as i128 * tmp as i128).rem_euclid(p as i128) as i64;
            i += 1;
        }
        if i == m {
            return StrykeValue::UNDEF;
        }
        let exp = 1i64 << (m - i - 1);
        let bigb = modpow(&[
            StrykeValue::integer(c),
            StrykeValue::integer(exp),
            StrykeValue::integer(p),
        ])
        .to_int();
        m = i;
        c = ((bigb as i128 * bigb as i128).rem_euclid(p as i128)) as i64;
        t = ((t as i128 * c as i128).rem_euclid(p as i128)) as i64;
        r = ((r as i128 * bigb as i128).rem_euclid(p as i128)) as i64;
    }
}

/// `stirling_1(N, K)` — unsigned Stirling number of the first kind.
pub fn stirling_1(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0) as usize;
    let k = arg_u64(args, 1).unwrap_or(0) as usize;
    if n > 40 || k > 40 {
        return StrykeValue::UNDEF;
    }
    let mut dp = vec![vec![0i64; n + 1]; n + 1];
    dp[0][0] = 1;
    for i in 1..=n {
        for j in 1..=i {
            dp[i][j] = dp[i - 1][j - 1] + (i as i64 - 1).wrapping_mul(dp[i - 1][j]);
        }
    }
    if k <= n {
        StrykeValue::integer(dp[n][k].abs())
    } else {
        StrykeValue::integer(0)
    }
}

/// `stirling_2(N, K)` — Stirling number of the second kind: number of
/// partitions of a set of size N into exactly K non-empty subsets.
pub fn stirling_2(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0) as usize;
    let k = arg_u64(args, 1).unwrap_or(0) as usize;
    if n > 30 || k > 30 {
        return StrykeValue::UNDEF;
    }
    let mut dp = vec![vec![0i64; k + 1]; n + 1];
    dp[0][0] = 1;
    for i in 1..=n {
        for j in 1..=k.min(i) {
            dp[i][j] = (j as i64) * dp[i - 1][j] + dp[i - 1][j - 1];
        }
    }
    if k <= n {
        StrykeValue::integer(dp[n][k])
    } else {
        StrykeValue::integer(0)
    }
}

/// `catalan_number(N)` — Nth Catalan number `C(2n,n)/(n+1)`.
pub fn catalan_number(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0) as u128;
    if n > 33 {
        return StrykeValue::UNDEF; // overflows i64
    }
    let mut c: u128 = 1;
    for i in 0..n {
        c = c * (2 * n - i) / (i + 1);
    }
    c /= n + 1;
    StrykeValue::integer(c as i64)
}

/// `lucas_n(N)` — Nth Lucas number (companion to Fibonacci).
/// L(0)=2, L(1)=1, L(n)=L(n-1)+L(n-2).
pub fn lucas_n(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0);
    if n > 88 {
        return StrykeValue::UNDEF; // overflows i64
    }
    let mut a: i64 = 2;
    let mut b: i64 = 1;
    for _ in 0..n {
        let c = a + b;
        a = b;
        b = c;
    }
    StrykeValue::integer(a)
}

/// `prime_count_below(N)` — count of primes < N. Sieve-based.
pub fn prime_count_below(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0) as usize;
    if n < 2 {
        return StrykeValue::integer(0);
    }
    let mut sieve = vec![true; n];
    sieve[0] = false;
    if n > 1 {
        sieve[1] = false;
    }
    let mut i = 2;
    while i * i < n {
        if sieve[i] {
            let mut j = i * i;
            while j < n {
                sieve[j] = false;
                j += i;
            }
        }
        i += 1;
    }
    StrykeValue::integer(sieve.iter().filter(|&&x| x).count() as i64)
}

/// Divisors of N (excluding N itself for proper-divisor-based fns).
fn divisors_of(n: u64) -> Vec<u64> {
    if n == 0 {
        return vec![];
    }
    let mut out = Vec::new();
    let mut i = 1u64;
    while i * i <= n {
        if n.is_multiple_of(i) {
            out.push(i);
            if i != n / i {
                out.push(n / i);
            }
        }
        i += 1;
    }
    out.sort();
    out
}

/// `divisor_count(N)` — number of divisors of N (including 1 and N).
pub fn divisor_count(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0);
    StrykeValue::integer(divisors_of(n).len() as i64)
}

/// `divisor_sum(N)` — sum of all divisors of N.
pub fn divisor_sum(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0);
    let sum: u64 = divisors_of(n).iter().sum();
    StrykeValue::integer(sum as i64)
}

/// `sigma_divisors(N, K)` — sigma function: sum of K-th powers of divisors.
pub fn sigma_divisors(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0);
    let k = arg_u64(args, 1).unwrap_or(1);
    let sum: u128 = divisors_of(n).iter().map(|d| (*d as u128).pow(k as u32)).sum();
    if sum > i64::MAX as u128 {
        return StrykeValue::UNDEF;
    }
    StrykeValue::integer(sum as i64)
}

/// `sum_digits(N, BASE?)` — sum of decimal (or base-N) digits.
pub fn sum_digits(args: &[StrykeValue]) -> StrykeValue {
    let mut n = arg_i64(args, 0).unwrap_or(0).unsigned_abs();
    let base = arg_u64(args, 1).unwrap_or(10).max(2);
    let mut sum = 0u64;
    while n > 0 {
        sum += n % base;
        n /= base;
    }
    StrykeValue::integer(sum as i64)
}

/// `product_digits(N, BASE?)` — product of decimal (or base-N) digits.
pub fn product_digits(args: &[StrykeValue]) -> StrykeValue {
    let mut n = arg_i64(args, 0).unwrap_or(0).unsigned_abs();
    let base = arg_u64(args, 1).unwrap_or(10).max(2);
    if n == 0 {
        return StrykeValue::integer(0);
    }
    let mut prod: u128 = 1;
    while n > 0 {
        prod = prod.saturating_mul((n % base) as u128);
        n /= base;
    }
    if prod > i64::MAX as u128 {
        return StrykeValue::UNDEF;
    }
    StrykeValue::integer(prod as i64)
}

/// `collatz_steps(N)` — number of steps to reach 1 under the Collatz map.
/// Returns undef for N <= 0.
pub fn collatz_steps(args: &[StrykeValue]) -> StrykeValue {
    let mut n = match arg_i64(args, 0) {
        Some(v) if v > 0 => v as u128,
        _ => return StrykeValue::UNDEF,
    };
    let mut steps = 0i64;
    while n > 1 {
        if n & 1 == 0 {
            n /= 2;
        } else {
            n = 3 * n + 1;
        }
        steps += 1;
        if steps > 1_000_000 {
            return StrykeValue::UNDEF;
        }
    }
    StrykeValue::integer(steps)
}

/// `hyperoperation(N, A, B)` — Knuth-style hyperoperation. N=0 is
/// successor, 1 is addition, 2 is multiplication, 3 is exponentiation,
/// 4 is tetration. Bounded to prevent runaway recursion.
pub fn hyperoperation(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0);
    let a = arg_u64(args, 1).unwrap_or(0);
    let bv = arg_u64(args, 2).unwrap_or(0);
    fn hyp(n: u64, a: u64, b: u64) -> Option<u128> {
        match n {
            0 => Some(b as u128 + 1),
            1 => Some(a as u128 + b as u128),
            2 => Some(a as u128 * b as u128),
            3 => {
                if b == 0 {
                    Some(1)
                } else {
                    (a as u128).checked_pow(b as u32)
                }
            }
            4 => {
                // Tetration: a^^b. Bounded heavily.
                if b == 0 {
                    return Some(1);
                }
                if a == 0 {
                    return Some(if b & 1 == 0 { 1 } else { 0 });
                }
                if a == 1 {
                    return Some(1);
                }
                if b > 4 {
                    return None;
                }
                let mut r: u128 = 1;
                for _ in 0..b {
                    r = (a as u128).checked_pow(r as u32)?;
                }
                Some(r)
            }
            _ => None,
        }
    }
    match hyp(n, a, bv) {
        Some(v) if v <= i64::MAX as u128 => StrykeValue::integer(v as i64),
        _ => StrykeValue::UNDEF,
    }
}

/// `busy_beaver(N)` — known busy-beaver values for N=1..=4. Returns
/// undef for unknown N (it's an undecidable function in general).
pub fn busy_beaver(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0);
    let v: i64 = match n {
        1 => 1,
        2 => 4,
        3 => 6,
        4 => 13,
        5 => 4098, // not proven exact but accepted lower bound for typed BB(5)
        _ => return StrykeValue::UNDEF,
    };
    StrykeValue::integer(v)
}

/// `quadratic_residue(A, P)` — 1 if A is a quadratic residue mod prime P,
/// 0 otherwise, undef on bad input.
pub fn quadratic_residue(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_i64(args, 0).unwrap_or(0);
    let p = arg_i64(args, 1).unwrap_or(0);
    if p < 2 {
        return StrykeValue::UNDEF;
    }
    let r = a.rem_euclid(p);
    if r == 0 {
        return StrykeValue::integer(1);
    }
    let crit = modpow(&[
        StrykeValue::integer(r),
        StrykeValue::integer((p - 1) / 2),
        StrykeValue::integer(p),
    ])
    .to_int();
    StrykeValue::integer(if crit == 1 { 1 } else { 0 })
}

/// `is_quadratic_residue(A, P)` — boolean form of `quadratic_residue`.
pub fn is_quadratic_residue(args: &[StrykeValue]) -> StrykeValue {
    quadratic_residue(args)
}

/// `discrete_log(BASE, TARGET, MOD)` — baby-step giant-step.
/// Find x such that `base^x ≡ target (mod m)`. Undef if no solution.
pub fn discrete_log(args: &[StrykeValue]) -> StrykeValue {
    use std::collections::HashMap;
    let base = arg_i64(args, 0).unwrap_or(0);
    let target = arg_i64(args, 1).unwrap_or(0).rem_euclid(arg_i64(args, 2).unwrap_or(1));
    let m = arg_i64(args, 2).unwrap_or(0);
    if m < 2 {
        return StrykeValue::UNDEF;
    }
    let n = (m as f64).sqrt().ceil() as i64;
    if n > 1_000_000 {
        return StrykeValue::UNDEF;
    }
    let mut table: HashMap<i64, i64> = HashMap::new();
    let mut val: i64 = target.rem_euclid(m);
    for j in 0..=n {
        table.insert(val, j);
        val = ((val as i128 * base as i128).rem_euclid(m as i128)) as i64;
    }
    let factor = modpow(&[
        StrykeValue::integer(base),
        StrykeValue::integer(n),
        StrykeValue::integer(m),
    ])
    .to_int();
    let mut cur: i64 = 1;
    for i in 0..=n {
        if let Some(j) = table.get(&cur) {
            let x = i * n - j;
            if x >= 0 {
                return StrykeValue::integer(x);
            }
        }
        cur = ((cur as i128 * factor as i128).rem_euclid(m as i128)) as i64;
    }
    StrykeValue::UNDEF
}

/// `order_modulo(A, M)` — multiplicative order of A modulo M.
pub fn order_modulo(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_i64(args, 0).unwrap_or(0);
    let m = arg_i64(args, 1).unwrap_or(0);
    if m < 2 {
        return StrykeValue::UNDEF;
    }
    let a = a.rem_euclid(m);
    if a == 0 {
        return StrykeValue::UNDEF;
    }
    let mut cur: i64 = 1;
    for k in 1..m {
        cur = ((cur as i128 * a as i128).rem_euclid(m as i128)) as i64;
        if cur == 1 {
            return StrykeValue::integer(k);
        }
    }
    StrykeValue::UNDEF
}

/// `square_free(N)` — 1 if N has no repeated prime factors.
pub fn square_free(args: &[StrykeValue]) -> StrykeValue {
    let mut n = arg_u64(args, 0).unwrap_or(0);
    if n == 0 {
        return StrykeValue::integer(0);
    }
    let mut p = 2u64;
    while p * p <= n {
        if n.is_multiple_of(p * p) {
            return StrykeValue::integer(0);
        }
        if n.is_multiple_of(p) {
            while n.is_multiple_of(p) {
                n /= p;
            }
        }
        p += 1;
    }
    StrykeValue::integer(1)
}

/// `perfect_number(N)` — 1 if N equals the sum of its proper divisors.
pub fn perfect_number(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0);
    if n <= 1 {
        return StrykeValue::integer(0);
    }
    let proper: u64 = divisors_of(n).iter().filter(|&&d| d != n).sum();
    StrykeValue::integer(if proper == n { 1 } else { 0 })
}

/// `abundant(N)` — 1 if sum of proper divisors > N.
pub fn abundant(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0);
    if n <= 1 {
        return StrykeValue::integer(0);
    }
    let proper: u64 = divisors_of(n).iter().filter(|&&d| d != n).sum();
    StrykeValue::integer(if proper > n { 1 } else { 0 })
}

/// `deficient(N)` — 1 if sum of proper divisors < N.
pub fn deficient(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_u64(args, 0).unwrap_or(0);
    if n <= 1 {
        return StrykeValue::integer(0);
    }
    let proper: u64 = divisors_of(n).iter().filter(|&&d| d != n).sum();
    StrykeValue::integer(if proper < n { 1 } else { 0 })
}

// ══════════════════════════════════════════════════════════════════════
// Random / sampling
// ══════════════════════════════════════════════════════════════════════

/// `random_bernoulli(P)` — 1 with probability P, 0 otherwise.
pub fn random_bernoulli(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let p = arg_f64(args, 0).unwrap_or(0.5).clamp(0.0, 1.0);
    let r: f64 = rand::thread_rng().gen();
    StrykeValue::integer(if r < p { 1 } else { 0 })
}

/// `random_normal(MU, SIGMA)` — sample from N(μ, σ²) via Box-Muller.
pub fn random_normal(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let mu = arg_f64(args, 0).unwrap_or(0.0);
    let sigma = arg_f64(args, 1).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    let u1: f64 = rng.gen();
    let u2: f64 = rng.gen();
    let z = (-2.0 * u1.max(1e-300).ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
    StrykeValue::float(mu + sigma * z)
}

/// `random_lognormal(MU, SIGMA)` — sample from a lognormal distribution.
pub fn random_lognormal(args: &[StrykeValue]) -> StrykeValue {
    let z = random_normal(args).to_number();
    StrykeValue::float(z.exp())
}

/// `random_exponential(LAMBDA)` — sample from `Exp(λ)`.
pub fn random_exponential(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let lambda = arg_f64(args, 0).unwrap_or(1.0).max(1e-300);
    let u: f64 = rand::thread_rng().gen::<f64>().max(1e-300);
    StrykeValue::float(-u.ln() / lambda)
}

/// `random_poisson(LAMBDA)` — Knuth's algorithm for small λ; for large
/// λ uses normal approximation.
pub fn random_poisson(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let lambda = arg_f64(args, 0).unwrap_or(1.0).max(0.0);
    if lambda < 30.0 {
        let l = (-lambda).exp();
        let mut k: i64 = 0;
        let mut p: f64 = 1.0;
        let mut rng = rand::thread_rng();
        loop {
            k += 1;
            p *= rng.gen::<f64>();
            if p <= l {
                return StrykeValue::integer(k - 1);
            }
        }
    } else {
        // Normal approximation N(λ, λ)
        let z = random_normal(&[
            StrykeValue::float(lambda),
            StrykeValue::float(lambda.sqrt()),
        ])
        .to_number();
        StrykeValue::integer(z.round().max(0.0) as i64)
    }
}

/// `random_gamma(SHAPE, SCALE)` — Marsaglia-Tsang for shape >= 1;
/// Ahrens-Dieter for shape < 1.
pub fn random_gamma(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let shape = arg_f64(args, 0).unwrap_or(1.0).max(1e-12);
    let scale = arg_f64(args, 1).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    let alpha = if shape < 1.0 { shape + 1.0 } else { shape };
    let d = alpha - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    let sample = loop {
        let x = random_normal(&[StrykeValue::float(0.0), StrykeValue::float(1.0)]).to_number();
        let v = (1.0 + c * x).powi(3);
        if v <= 0.0 {
            continue;
        }
        let u: f64 = rng.gen();
        if u < 1.0 - 0.0331 * x.powi(4) {
            break d * v;
        }
        if u.ln() < 0.5 * x * x + d * (1.0 - v + v.ln()) {
            break d * v;
        }
    };
    let result = if shape < 1.0 {
        let u: f64 = rng.gen::<f64>().max(1e-300);
        sample * u.powf(1.0 / shape)
    } else {
        sample
    };
    StrykeValue::float(result * scale)
}

/// `random_beta(ALPHA, BETA)` — via two gamma samples.
pub fn random_beta(args: &[StrykeValue]) -> StrykeValue {
    let alpha = arg_f64(args, 0).unwrap_or(1.0);
    let beta = arg_f64(args, 1).unwrap_or(1.0);
    let x = random_gamma(&[StrykeValue::float(alpha), StrykeValue::float(1.0)]).to_number();
    let y = random_gamma(&[StrykeValue::float(beta), StrykeValue::float(1.0)]).to_number();
    StrykeValue::float(x / (x + y))
}

/// `random_alphanumeric(LEN)` — random `[A-Za-z0-9]` string.
pub fn random_alphanumeric(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let n = arg_u64(args, 0).unwrap_or(16) as usize;
    let charset: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    let s: String = (0..n)
        .map(|_| {
            let idx = rng.gen_range(0..charset.len());
            charset[idx] as char
        })
        .collect();
    StrykeValue::string(s)
}

/// `random_alphabetic(LEN)` — random `[A-Za-z]` string.
pub fn random_alphabetic(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let n = arg_u64(args, 0).unwrap_or(16) as usize;
    let charset: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::thread_rng();
    let s: String = (0..n)
        .map(|_| {
            let idx = rng.gen_range(0..charset.len());
            charset[idx] as char
        })
        .collect();
    StrykeValue::string(s)
}

/// `random_password(LEN, OPTS?)` — secure random password. OPTS is an
/// optional hashref of flags: `{ no_symbols => 1, no_upper => 1, ... }`.
/// Default: 20 chars, mixed case + digits + safe symbols.
pub fn random_password(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let n = arg_u64(args, 0).unwrap_or(20) as usize;
    let opts = args.get(1).and_then(|v| v.as_hash_ref());
    let has = |key: &str| -> bool {
        opts.as_ref()
            .map(|h| h.read().get(key).is_some_and(|v| v.is_true()))
            .unwrap_or(false)
    };
    let mut charset = String::from("abcdefghijklmnopqrstuvwxyz0123456789");
    if !has("no_upper") {
        charset.push_str("ABCDEFGHIJKLMNOPQRSTUVWXYZ");
    }
    if !has("no_symbols") {
        charset.push_str("!@#$%^&*-_=+");
    }
    let chars: Vec<char> = charset.chars().collect();
    let mut rng = rand::thread_rng();
    let s: String = (0..n)
        .map(|_| chars[rng.gen_range(0..chars.len())])
        .collect();
    StrykeValue::string(s)
}

/// `random_choices_weighted(\@items, \@weights, N)` — N samples with
/// replacement according to weights.
pub fn random_choices_weighted(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use rand::Rng;
    use std::sync::Arc;
    let Some(items) = args.first().and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let Some(weights) = args.get(1).and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let n = arg_u64(args, 2).unwrap_or(1) as usize;
    let g = items.read();
    let wg = weights.read();
    if g.len() != wg.len() || g.is_empty() {
        return StrykeValue::UNDEF;
    }
    let weights_f: Vec<f64> = wg.iter().map(|v| v.to_number().max(0.0)).collect();
    let total: f64 = weights_f.iter().sum();
    if total <= 0.0 {
        return StrykeValue::UNDEF;
    }
    let cum: Vec<f64> = {
        let mut c = Vec::with_capacity(weights_f.len());
        let mut acc = 0.0;
        for w in &weights_f {
            acc += w;
            c.push(acc);
        }
        c
    };
    let mut rng = rand::thread_rng();
    let out: Vec<StrykeValue> = (0..n)
        .map(|_| {
            let r: f64 = rng.gen::<f64>() * total;
            let idx = cum.iter().position(|&x| r < x).unwrap_or(g.len() - 1);
            g[idx].clone()
        })
        .collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(out)))
}

/// `sample_weighted_unique(\@items, \@weights, K)` — K unique samples
/// (without replacement) drawn proportional to weights, using the
/// Efraimidis–Spirakis A-Res reservoir method.
pub fn sample_weighted_unique(args: &[StrykeValue]) -> StrykeValue {
    use parking_lot::RwLock;
    use rand::Rng;
    use std::sync::Arc;
    let Some(items) = args.first().and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let Some(weights) = args.get(1).and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let k = arg_u64(args, 2).unwrap_or(1) as usize;
    let g = items.read();
    let wg = weights.read();
    if g.len() != wg.len() {
        return StrykeValue::UNDEF;
    }
    let n = g.len();
    if k > n {
        return StrykeValue::UNDEF;
    }
    let mut rng = rand::thread_rng();
    let mut scored: Vec<(f64, usize)> = (0..n)
        .map(|i| {
            let w = wg[i].to_number().max(1e-300);
            let u: f64 = rng.gen::<f64>().max(1e-300);
            (u.powf(1.0 / w), i)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let out: Vec<StrykeValue> = scored.iter().take(k).map(|(_, i)| g[*i].clone()).collect();
    StrykeValue::array_ref(Arc::new(RwLock::new(out)))
}

/// `reservoir_sample_weighted(\@items, \@weights, K)` — alias of
/// `sample_weighted_unique` (A-Res is the standard reservoir-sampling
/// algorithm for weighted streams).
pub fn reservoir_sample_weighted(args: &[StrykeValue]) -> StrykeValue {
    sample_weighted_unique(args)
}

/// `seeded_rng(SEED)` — return a hashref representing a seeded PRNG
/// state. Use with `random_choice` etc. via `with_rng => ` option.
/// (Stryke's `rand` is currently process-global; this returns the seed
/// so callers can pass it to a deterministic sampling path.)
pub fn seeded_rng(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::sync::Arc;
    let seed = arg_u64(args, 0).unwrap_or(0);
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("seed".to_string(), StrykeValue::integer(seed as i64));
    h.insert(
        "kind".to_string(),
        StrykeValue::string("seeded_rng".to_string()),
    );
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

/// `save_random_state()` — capture process-level RNG snapshot.
/// Currently returns the system entropy snapshot timestamp; a future
/// rev should expose a real deterministic state.
pub fn save_random_state(_args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let seed: u64 = rng.gen();
    seeded_rng(&[StrykeValue::integer(seed as i64)])
}

/// `restore_random_state(\%state)` — no-op for now (the process-global
/// RNG can't be restored to an arbitrary state without crate changes).
/// Returns 1 on success (state seed accepted).
pub fn restore_random_state(args: &[StrykeValue]) -> StrykeValue {
    if args.first().and_then(|v| v.as_hash_ref()).is_some() {
        StrykeValue::integer(1)
    } else {
        StrykeValue::integer(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(i: i64) -> StrykeValue {
        StrykeValue::integer(i)
    }

    #[test]
    fn extended_gcd_basic() {
        // gcd(35, 15) = 5; 35*x + 15*y = 5
        let r = extended_gcd(&[n(35), n(15)]);
        let arr = r.as_array_ref().expect("array");
        let g = arr.read();
        assert_eq!(g[0].to_int(), 5);
        let x = g[1].to_int();
        let y = g[2].to_int();
        assert_eq!(35 * x + 15 * y, 5);
    }

    #[test]
    fn modinverse_correct() {
        // 3 * 7 ≡ 21 ≡ 1 (mod 10). So inv(3, 10) = 7.
        assert_eq!(modinverse(&[n(3), n(10)]).to_int(), 7);
        assert!(modinverse(&[n(2), n(10)]).is_undef()); // gcd != 1
    }

    #[test]
    fn modpow_basic() {
        assert_eq!(modpow(&[n(2), n(10), n(1000)]).to_int(), 24);
        assert_eq!(modpow(&[n(3), n(100), n(7)]).to_int(), 4);
    }

    #[test]
    fn stirling_numbers() {
        // s(4,2) unsigned = 11
        assert_eq!(stirling_1(&[n(4), n(2)]).to_int(), 11);
        // S(5,3) = 25
        assert_eq!(stirling_2(&[n(5), n(3)]).to_int(), 25);
    }

    #[test]
    fn catalan_numbers() {
        // C_0..C_5 = 1, 1, 2, 5, 14, 42
        assert_eq!(catalan_number(&[n(0)]).to_int(), 1);
        assert_eq!(catalan_number(&[n(4)]).to_int(), 14);
        assert_eq!(catalan_number(&[n(5)]).to_int(), 42);
    }

    #[test]
    fn lucas_basic() {
        // L_0=2, L_1=1, L_2=3, L_3=4, L_4=7
        assert_eq!(lucas_n(&[n(0)]).to_int(), 2);
        assert_eq!(lucas_n(&[n(1)]).to_int(), 1);
        assert_eq!(lucas_n(&[n(4)]).to_int(), 7);
    }

    #[test]
    fn divisor_functions() {
        // d(12) = 1,2,3,4,6,12 → 6 divisors, sum 28
        assert_eq!(divisor_count(&[n(12)]).to_int(), 6);
        assert_eq!(divisor_sum(&[n(12)]).to_int(), 28);
        assert_eq!(sigma_divisors(&[n(6), n(2)]).to_int(), 50); // 1+4+9+36
    }

    #[test]
    fn digit_functions() {
        assert_eq!(sum_digits(&[n(12345)]).to_int(), 15);
        assert_eq!(product_digits(&[n(1234)]).to_int(), 24);
        assert_eq!(sum_digits(&[n(0xff), n(16)]).to_int(), 30); // f+f=15+15
    }

    #[test]
    fn collatz_basic() {
        // 6: 6→3→10→5→16→8→4→2→1 = 8 steps
        assert_eq!(collatz_steps(&[n(6)]).to_int(), 8);
        // 27 takes 111 steps
        assert_eq!(collatz_steps(&[n(27)]).to_int(), 111);
    }

    #[test]
    fn perfect_abundant_deficient() {
        assert_eq!(perfect_number(&[n(28)]).to_int(), 1); // 28 = 1+2+4+7+14
        assert_eq!(perfect_number(&[n(6)]).to_int(), 1);
        assert_eq!(abundant(&[n(12)]).to_int(), 1); // 1+2+3+4+6=16 > 12
        assert_eq!(deficient(&[n(10)]).to_int(), 1); // 1+2+5=8 < 10
    }

    #[test]
    fn square_free_check() {
        assert_eq!(square_free(&[n(30)]).to_int(), 1); // 2*3*5
        assert_eq!(square_free(&[n(12)]).to_int(), 0); // 4*3, has 2^2
        assert_eq!(square_free(&[n(1)]).to_int(), 1);
    }

    #[test]
    fn quadratic_residue_check() {
        // 4 is QR mod 7 (since 2^2 = 4)
        assert_eq!(quadratic_residue(&[n(4), n(7)]).to_int(), 1);
        // 3 is NOT QR mod 7
        assert_eq!(quadratic_residue(&[n(3), n(7)]).to_int(), 0);
    }

    #[test]
    fn order_modulo_basic() {
        // order of 2 mod 7 = 3 (since 2^3 = 8 ≡ 1)
        assert_eq!(order_modulo(&[n(2), n(7)]).to_int(), 3);
    }

    #[test]
    fn random_alphanumeric_length() {
        let r = random_alphanumeric(&[n(20)]).to_string();
        assert_eq!(r.len(), 20);
        assert!(r.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn random_password_default() {
        let r = random_password(&[n(16)]).to_string();
        assert_eq!(r.len(), 16);
    }
}
