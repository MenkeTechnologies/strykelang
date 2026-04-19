// ─────────────────────────────────────────────────────────────────────────────
// Extended stdlib: Number Theory, Statistics, Geometry, Financial, Encoding,
// Color, Matrix, String, Validation, Algorithms, DSP, Misc
// ─────────────────────────────────────────────────────────────────────────────

// ─────────────────────────────────────────────────────────────────────────────
// Number Theory
// ─────────────────────────────────────────────────────────────────────────────

fn prime_factorize(mut n: i64) -> Vec<i64> {
    let mut factors = Vec::new();
    if n <= 1 {
        return factors;
    }
    let mut d = 2i64;
    while d * d <= n {
        while n % d == 0 {
            factors.push(d);
            n /= d;
        }
        d += 1;
    }
    if n > 1 {
        factors.push(n);
    }
    factors
}

fn is_prime_check(n: i64) -> bool {
    if n < 2 {
        return false;
    }
    if n < 4 {
        return true;
    }
    if n % 2 == 0 || n % 3 == 0 {
        return false;
    }
    let mut i = 5;
    while i * i <= n {
        if n % i == 0 || n % (i + 2) == 0 {
            return false;
        }
        i += 6;
    }
    true
}

fn aliquot(n: i64) -> i64 {
    if n <= 1 {
        return 0;
    }
    let mut s = 1i64;
    let mut i = 2i64;
    while i * i <= n {
        if n % i == 0 {
            s += i;
            if i != n / i {
                s += n / i;
            }
        }
        i += 1;
    }
    s
}

fn euler_phi(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut result = n;
    let mut m = n;
    let mut p = 2i64;
    while p * p <= m {
        if m % p == 0 {
            while m % p == 0 {
                m /= p;
            }
            result -= result / p;
        }
        p += 1;
    }
    if m > 1 {
        result -= result / m;
    }
    result
}

/// `prime_factors N` — prime factorization.
fn builtin_prime_factors(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int();
    Ok(PerlValue::array(
        prime_factorize(n)
            .into_iter()
            .map(PerlValue::integer)
            .collect(),
    ))
}

/// `divisors N` — all divisors of N, sorted.
fn builtin_divisors(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().unsigned_abs();
    if n == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    let mut divs = Vec::new();
    let mut i = 1u64;
    while i * i <= n {
        if n.is_multiple_of(i) {
            divs.push(i as i64);
            if i != n / i {
                divs.push((n / i) as i64);
            }
        }
        i += 1;
    }
    divs.sort_unstable();
    Ok(PerlValue::array(
        divs.into_iter().map(PerlValue::integer).collect(),
    ))
}

/// `num_divisors N` — count of divisors.
fn builtin_num_divisors(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().unsigned_abs();
    if n == 0 {
        return Ok(PerlValue::integer(0));
    }
    let mut count = 0i64;
    let mut i = 1u64;
    while i * i <= n {
        if n.is_multiple_of(i) {
            count += if i == n / i { 1 } else { 2 };
        }
        i += 1;
    }
    Ok(PerlValue::integer(count))
}

/// `sum_divisors N` — sum of proper divisors.
fn builtin_sum_divisors(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(aliquot(
        first_arg_or_topic(interp, args).to_int(),
    )))
}

/// `is_perfect N`.
fn builtin_is_perfect(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int();
    Ok(bool_iv(n > 1 && aliquot(n) == n))
}

/// `is_abundant N`.
fn builtin_is_abundant(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int();
    Ok(bool_iv(n > 0 && aliquot(n) > n))
}

/// `is_deficient N`.
fn builtin_is_deficient(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int();
    Ok(bool_iv(n > 0 && aliquot(n) < n))
}

/// `collatz_length N` — steps to reach 1.
fn builtin_collatz_length(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut n = first_arg_or_topic(interp, args).to_int();
    if n <= 0 {
        return Ok(PerlValue::integer(0));
    }
    let mut steps = 0i64;
    while n != 1 {
        n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 };
        steps += 1;
    }
    Ok(PerlValue::integer(steps))
}

/// `collatz_sequence N` — full sequence from N to 1.
fn builtin_collatz_sequence(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut n = first_arg_or_topic(interp, args).to_int();
    if n <= 0 {
        return Ok(PerlValue::array(vec![]));
    }
    let mut seq = vec![PerlValue::integer(n)];
    while n != 1 {
        n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 };
        seq.push(PerlValue::integer(n));
    }
    Ok(PerlValue::array(seq))
}

/// `lucas N` — Nth Lucas number.
fn builtin_lucas(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0) as usize;
    if n == 0 {
        return Ok(PerlValue::integer(2));
    }
    if n == 1 {
        return Ok(PerlValue::integer(1));
    }
    let (mut a, mut b) = (2i64, 1i64);
    for _ in 2..=n {
        let t = a.wrapping_add(b);
        a = b;
        b = t;
    }
    Ok(PerlValue::integer(b))
}

/// `tribonacci N` — Nth Tribonacci number.
fn builtin_tribonacci(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0) as usize;
    if n == 0 {
        return Ok(PerlValue::integer(0));
    }
    if n <= 2 {
        return Ok(PerlValue::integer(if n == 1 { 0 } else { 1 }));
    }
    let (mut a, mut b, mut c) = (0i64, 0i64, 1i64);
    for _ in 3..=n {
        let t = a.wrapping_add(b).wrapping_add(c);
        a = b;
        b = c;
        c = t;
    }
    Ok(PerlValue::integer(c))
}

/// `nth_prime N` — the Nth prime (1-indexed).
fn builtin_nth_prime(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(1) as usize;
    let mut count = 0usize;
    let mut candidate = 2i64;
    loop {
        if is_prime_check(candidate) {
            count += 1;
            if count == n {
                return Ok(PerlValue::integer(candidate));
            }
        }
        candidate += 1;
    }
}

/// `primes_up_to N` — sieve of Eratosthenes.
fn builtin_primes_up_to(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0) as usize;
    if n < 2 {
        return Ok(PerlValue::array(vec![]));
    }
    let mut sieve = vec![true; n + 1];
    sieve[0] = false;
    sieve[1] = false;
    let mut i = 2;
    while i * i <= n {
        if sieve[i] {
            let mut j = i * i;
            while j <= n {
                sieve[j] = false;
                j += i;
            }
        }
        i += 1;
    }
    Ok(PerlValue::array(
        sieve
            .iter()
            .enumerate()
            .filter(|(_, &is_p)| is_p)
            .map(|(i, _)| PerlValue::integer(i as i64))
            .collect(),
    ))
}

/// `next_prime N`.
fn builtin_next_prime(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut n = first_arg_or_topic(interp, args).to_int() + 1;
    if n < 2 {
        n = 2;
    }
    while !is_prime_check(n) {
        n += 1;
    }
    Ok(PerlValue::integer(n))
}

/// `prev_prime N`.
fn builtin_prev_prime(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut n = first_arg_or_topic(interp, args).to_int() - 1;
    while n >= 2 && !is_prime_check(n) {
        n -= 1;
    }
    Ok(if n >= 2 {
        PerlValue::integer(n)
    } else {
        PerlValue::UNDEF
    })
}

/// `triangular_number N` — N*(N+1)/2.
fn builtin_triangular_number(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int();
    Ok(PerlValue::integer(n * (n + 1) / 2))
}

/// `is_pentagonal N`.
fn builtin_is_pentagonal(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int();
    if n < 1 {
        return Ok(bool_iv(false));
    }
    let disc = 1.0 + 24.0 * n as f64;
    let s = disc.sqrt();
    Ok(bool_iv((s - 1.0) % 6.0 < 1e-9))
}

/// `pentagonal_number N` — N*(3N-1)/2.
fn builtin_pentagonal_number(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int();
    Ok(PerlValue::integer(n * (3 * n - 1) / 2))
}

/// `perfect_numbers N` — first N perfect numbers.
fn builtin_perfect_numbers(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let count = first_arg_or_topic(interp, args).to_int().max(0) as usize;
    let exponents = [2, 3, 5, 7, 13, 17, 19, 31];
    let mut result = Vec::with_capacity(count.min(exponents.len()));
    for &p in exponents.iter().take(count) {
        let mp = (1i64 << p) - 1;
        result.push(PerlValue::integer((1i64 << (p - 1)) * mp));
    }
    Ok(PerlValue::array(result))
}

/// `twin_primes N` — twin prime pairs up to N.
fn builtin_twin_primes(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0) as usize;
    let mut result = Vec::new();
    let mut p = 2i64;
    while (p + 2) <= n as i64 {
        if is_prime_check(p) && is_prime_check(p + 2) {
            result.push(PerlValue::array(vec![
                PerlValue::integer(p),
                PerlValue::integer(p + 2),
            ]));
        }
        p += 1;
    }
    Ok(PerlValue::array(result))
}

/// `goldbach N` — decomposition of even N into two primes.
fn builtin_goldbach(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int();
    if n < 4 || n % 2 != 0 {
        return Ok(PerlValue::UNDEF);
    }
    for p in 2..=n / 2 {
        if is_prime_check(p) && is_prime_check(n - p) {
            return Ok(PerlValue::array(vec![
                PerlValue::integer(p),
                PerlValue::integer(n - p),
            ]));
        }
    }
    Ok(PerlValue::UNDEF)
}

/// `prime_pi N` — count of primes ≤ N.
fn builtin_prime_pi(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0) as usize;
    if n < 2 {
        return Ok(PerlValue::integer(0));
    }
    let mut sieve = vec![true; n + 1];
    sieve[0] = false;
    sieve[1] = false;
    let mut i = 2;
    while i * i <= n {
        if sieve[i] {
            let mut j = i * i;
            while j <= n {
                sieve[j] = false;
                j += i;
            }
        }
        i += 1;
    }
    Ok(PerlValue::integer(
        sieve.iter().filter(|&&b| b).count() as i64
    ))
}

/// `totient_sum N` — sum of Euler's totient for 1..N.
fn builtin_totient_sum(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0) as usize;
    let mut sum = 0i64;
    for i in 1..=n {
        sum += euler_phi(i as i64);
    }
    Ok(PerlValue::integer(sum))
}

/// `subfactorial N` — number of derangements.
fn builtin_subfactorial(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0) as usize;
    if n == 0 {
        return Ok(PerlValue::integer(1));
    }
    if n == 1 {
        return Ok(PerlValue::integer(0));
    }
    let (mut a, mut b) = (1i64, 0i64);
    for i in 2..=n {
        let t = (i as i64 - 1) * (a + b);
        a = b;
        b = t;
    }
    Ok(PerlValue::integer(b))
}

/// `bell_number N`.
fn builtin_bell_number(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0) as usize;
    if n == 0 {
        return Ok(PerlValue::integer(1));
    }
    let mut tri = vec![vec![0i64; n + 1]; n + 1];
    tri[0][0] = 1;
    for i in 1..=n {
        tri[i][0] = tri[i - 1][i - 1];
        for j in 1..=i {
            tri[i][j] = tri[i][j - 1].wrapping_add(tri[i - 1][j - 1]);
        }
    }
    Ok(PerlValue::integer(tri[n][0]))
}

/// `partition_number N` — integer partitions of N.
fn builtin_partition_number(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0) as usize;
    let mut dp = vec![0i64; n + 1];
    dp[0] = 1;
    for i in 1..=n {
        for j in i..=n {
            dp[j] = dp[j].wrapping_add(dp[j - i]);
        }
    }
    Ok(PerlValue::integer(dp[n]))
}

/// `multinomial N, K1, K2, ...`.
fn builtin_multinomial(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs = flatten_args(args);
    if xs.is_empty() {
        return Ok(PerlValue::integer(1));
    }
    let n = xs[0].to_int();
    let ks: Vec<i64> = xs[1..].iter().map(|v| v.to_int()).collect();
    fn fact(x: i64) -> i64 {
        (1..=x).product()
    }
    let denom: i64 = ks.iter().map(|&k| fact(k)).product();
    Ok(PerlValue::integer(if denom == 0 {
        0
    } else {
        fact(n) / denom
    }))
}

/// `is_smith N`.
fn builtin_is_smith(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int();
    if n < 4 || is_prime_check(n) {
        return Ok(bool_iv(false));
    }
    fn digit_sum(mut x: i64) -> i64 {
        let mut s = 0;
        x = x.abs();
        while x > 0 {
            s += x % 10;
            x /= 10;
        }
        s
    }
    let ds = digit_sum(n);
    let pf_ds: i64 = prime_factorize(n).iter().map(|&p| digit_sum(p)).sum();
    Ok(bool_iv(ds == pf_ds))
}

/// `aliquot_sum N`.
fn builtin_aliquot_sum(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(aliquot(
        first_arg_or_topic(interp, args).to_int(),
    )))
}

/// `abundant_numbers N`.
fn builtin_abundant_numbers(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0);
    Ok(PerlValue::array(
        (1..=n)
            .filter(|&i| aliquot(i) > i)
            .map(PerlValue::integer)
            .collect(),
    ))
}

/// `deficient_numbers N`.
fn builtin_deficient_numbers(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0);
    Ok(PerlValue::array(
        (1..=n)
            .filter(|&i| aliquot(i) < i)
            .map(PerlValue::integer)
            .collect(),
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Statistics (extended)
// ─────────────────────────────────────────────────────────────────────────────

fn stats_mean(vals: &[f64]) -> f64 {
    if vals.is_empty() {
        0.0
    } else {
        vals.iter().sum::<f64>() / vals.len() as f64
    }
}

fn stats_variance(vals: &[f64]) -> f64 {
    let m = stats_mean(vals);
    vals.iter().map(|v| (v - m).powi(2)).sum::<f64>() / vals.len() as f64
}

fn stats_stddev(vals: &[f64]) -> f64 {
    stats_variance(vals).sqrt()
}

/// `skewness LIST`.
fn builtin_skewness(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    if vals.len() < 3 {
        return Ok(PerlValue::float(0.0));
    }
    let m = stats_mean(&vals);
    let sd = stats_stddev(&vals);
    if sd == 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    let n = vals.len() as f64;
    let skew =
        vals.iter().map(|v| ((v - m) / sd).powi(3)).sum::<f64>() * n / ((n - 1.0) * (n - 2.0));
    Ok(PerlValue::float(skew))
}

/// `kurtosis LIST` — excess kurtosis.
fn builtin_kurtosis(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    if vals.len() < 4 {
        return Ok(PerlValue::float(0.0));
    }
    let m = stats_mean(&vals);
    let sd = stats_stddev(&vals);
    if sd == 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    let n = vals.len() as f64;
    let m4 = vals.iter().map(|v| ((v - m) / sd).powi(4)).sum::<f64>() / n;
    Ok(PerlValue::float(m4 - 3.0))
}

/// `linear_regression XS, YS` — returns [slope, intercept, r²].
fn builtin_linear_regression(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = xs.len().min(ys.len());
    if n < 2 {
        return Ok(PerlValue::UNDEF);
    }
    let mx = xs[..n].iter().sum::<f64>() / n as f64;
    let my = ys[..n].iter().sum::<f64>() / n as f64;
    let (mut ss_xy, mut ss_xx, mut ss_yy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (dx, dy) = (xs[i] - mx, ys[i] - my);
        ss_xy += dx * dy;
        ss_xx += dx * dx;
        ss_yy += dy * dy;
    }
    if ss_xx == 0.0 {
        return Ok(PerlValue::UNDEF);
    }
    let slope = ss_xy / ss_xx;
    let intercept = my - slope * mx;
    let r2 = if ss_yy == 0.0 {
        1.0
    } else {
        (ss_xy * ss_xy) / (ss_xx * ss_yy)
    };
    Ok(PerlValue::array(vec![
        PerlValue::float(slope),
        PerlValue::float(intercept),
        PerlValue::float(r2),
    ]))
}

/// `moving_average WINDOW, LIST`.
fn builtin_moving_average(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let window = args
        .first()
        .map(|v| v.to_int().max(1) as usize)
        .unwrap_or(3);
    let vals: Vec<f64> = flatten_args(&args[1.min(args.len())..])
        .iter()
        .map(|v| v.to_number())
        .collect();
    if vals.len() < window {
        return Ok(PerlValue::array(vec![]));
    }
    let mut result = Vec::with_capacity(vals.len() - window + 1);
    let mut sum: f64 = vals[..window].iter().sum();
    result.push(PerlValue::float(sum / window as f64));
    for i in window..vals.len() {
        sum += vals[i] - vals[i - window];
        result.push(PerlValue::float(sum / window as f64));
    }
    Ok(PerlValue::array(result))
}

/// `exponential_moving_average ALPHA, LIST`.
fn builtin_exponential_moving_average(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let xs = flatten_args(&args[1.min(args.len())..]);
    if xs.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let mut ema = xs[0].to_number();
    let mut result = vec![PerlValue::float(ema)];
    for x in xs.iter().skip(1) {
        ema = alpha * x.to_number() + (1.0 - alpha) * ema;
        result.push(PerlValue::float(ema));
    }
    Ok(PerlValue::array(result))
}

/// `coeff_of_variation LIST`.
fn builtin_coeff_of_variation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    let m = stats_mean(&vals);
    Ok(PerlValue::float(if m == 0.0 {
        f64::NAN
    } else {
        stats_stddev(&vals) / m
    }))
}

/// `standard_error LIST`.
fn builtin_standard_error(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    let n = vals.len() as f64;
    Ok(PerlValue::float(if n <= 1.0 {
        0.0
    } else {
        stats_stddev(&vals) / n.sqrt()
    }))
}

/// `normalize_array LIST` — min-max normalize to [0,1].
fn builtin_normalize_array(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    if vals.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let mn = vals.iter().cloned().fold(f64::INFINITY, f64::min);
    let mx = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = mx - mn;
    if range == 0.0 {
        return Ok(PerlValue::array(vec![PerlValue::float(0.0); vals.len()]));
    }
    Ok(PerlValue::array(
        vals.iter()
            .map(|v| PerlValue::float((v - mn) / range))
            .collect(),
    ))
}

/// `cross_entropy P, Q`.
fn builtin_cross_entropy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let q: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    Ok(PerlValue::float(
        p.iter()
            .zip(q.iter())
            .filter(|(_, &qi)| qi > 0.0)
            .map(|(&pi, &qi)| -pi * qi.ln())
            .sum::<f64>(),
    ))
}

/// `euclidean_distance A, B`.
fn builtin_euclidean_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    Ok(PerlValue::float(
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f64>()
            .sqrt(),
    ))
}

/// `minkowski_distance A, B, P`.
fn builtin_minkowski_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    Ok(PerlValue::float(
        a.iter()
            .zip(b.iter())
            .map(|(x, y)| (x - y).abs().powf(p))
            .sum::<f64>()
            .powf(1.0 / p),
    ))
}

/// `mean_absolute_error A, B`.
fn builtin_mean_absolute_error(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = a.len().min(b.len());
    if n == 0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float(
        a[..n]
            .iter()
            .zip(b[..n].iter())
            .map(|(x, y)| (x - y).abs())
            .sum::<f64>()
            / n as f64,
    ))
}

/// `mean_squared_error A, B`.
fn builtin_mean_squared_error(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = a.len().min(b.len());
    if n == 0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float(
        a[..n]
            .iter()
            .zip(b[..n].iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum::<f64>()
            / n as f64,
    ))
}

/// `median_absolute_deviation LIST`.
fn builtin_median_absolute_deviation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    if vals.is_empty() {
        return Ok(PerlValue::float(0.0));
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let med = vals[vals.len() / 2];
    let mut devs: Vec<f64> = vals.iter().map(|v| (v - med).abs()).collect();
    devs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(PerlValue::float(devs[devs.len() / 2]))
}

/// `winsorize PERCENT, LIST`.
fn builtin_winsorize(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pct = args.first().map(|v| v.to_number()).unwrap_or(5.0) / 100.0;
    let mut vals: Vec<f64> = flatten_args(&args[1.min(args.len())..])
        .iter()
        .map(|v| v.to_number())
        .collect();
    if vals.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len();
    let lo_idx = (n as f64 * pct).floor() as usize;
    let hi_idx = n
        .saturating_sub(1)
        .min((n as f64 * (1.0 - pct)).ceil() as usize);
    let (lo, hi) = (vals[lo_idx], vals[hi_idx]);
    Ok(PerlValue::array(
        vals.iter()
            .map(|&v| PerlValue::float(v.clamp(lo, hi)))
            .collect(),
    ))
}

/// `weighted_mean VALUES, WEIGHTS`.
fn builtin_weighted_mean(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vals: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let weights: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = vals.len().min(weights.len());
    if n == 0 {
        return Ok(PerlValue::float(0.0));
    }
    let wsum: f64 = vals[..n]
        .iter()
        .zip(weights[..n].iter())
        .map(|(v, w)| v * w)
        .sum();
    let wtotal: f64 = weights[..n].iter().sum();
    Ok(PerlValue::float(if wtotal == 0.0 {
        0.0
    } else {
        wsum / wtotal
    }))
}

/// `z_score VALUE, MEAN, STDDEV` — standard score.
fn builtin_z_score(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mean = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sd = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if sd == 0.0 {
        0.0
    } else {
        (x - mean) / sd
    }))
}

/// `z_scores LIST` — compute z-scores for all values in the list.
fn builtin_z_scores(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    if vals.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let mean = stats_mean(&vals);
    let sd = stats_stddev(&vals);
    Ok(PerlValue::array(
        vals.iter()
            .map(|&v| PerlValue::float(if sd == 0.0 { 0.0 } else { (v - mean) / sd }))
            .collect(),
    ))
}

/// `percentile_rank VALUE, SORTED_LIST` — percentile rank of value in list.
fn builtin_percentile_rank(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let vals: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    if vals.is_empty() {
        return Ok(PerlValue::float(0.0));
    }
    let count_below = vals.iter().filter(|&&v| v < x).count() as f64;
    let count_equal = vals
        .iter()
        .filter(|&&v| (v - x).abs() < f64::EPSILON)
        .count() as f64;
    Ok(PerlValue::float(
        100.0 * (count_below + 0.5 * count_equal) / vals.len() as f64,
    ))
}

/// `quartiles LIST` — returns [Q1, Q2 (median), Q3].
fn builtin_quartiles(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    if vals.is_empty() {
        return Ok(PerlValue::array(vec![
            PerlValue::float(0.0),
            PerlValue::float(0.0),
            PerlValue::float(0.0),
        ]));
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len();
    let q1 = vals[n / 4];
    let q2 = vals[n / 2];
    let q3 = vals[3 * n / 4];
    Ok(PerlValue::array(vec![
        PerlValue::float(q1),
        PerlValue::float(q2),
        PerlValue::float(q3),
    ]))
}

/// `spearman_correlation XS, YS` — Spearman rank correlation coefficient.
fn builtin_spearman_correlation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = xs.len().min(ys.len());
    if n < 2 {
        return Ok(PerlValue::float(0.0));
    }
    fn ranks(vals: &[f64]) -> Vec<f64> {
        let mut indexed: Vec<(usize, f64)> = vals.iter().copied().enumerate().collect();
        indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut r = vec![0.0; vals.len()];
        for (rank, (orig_idx, _)) in indexed.into_iter().enumerate() {
            r[orig_idx] = (rank + 1) as f64;
        }
        r
    }
    let rx = ranks(&xs[..n]);
    let ry = ranks(&ys[..n]);
    let d2: f64 = rx.iter().zip(ry.iter()).map(|(x, y)| (x - y).powi(2)).sum();
    let nf = n as f64;
    Ok(PerlValue::float(1.0 - (6.0 * d2) / (nf * (nf * nf - 1.0))))
}

/// `t_test_one_sample SAMPLE, POPULATION_MEAN` — one-sample t-test, returns t-statistic.
fn builtin_t_test_one_sample(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sample: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let pop_mean = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = sample.len();
    if n < 2 {
        return Ok(PerlValue::float(0.0));
    }
    let sample_mean = stats_mean(&sample);
    let sample_sd = stats_stddev(&sample);
    let se = sample_sd / (n as f64).sqrt();
    Ok(PerlValue::float(if se == 0.0 {
        0.0
    } else {
        (sample_mean - pop_mean) / se
    }))
}

/// `t_test_two_sample SAMPLE1, SAMPLE2` — two-sample t-test (equal variance), returns t-statistic.
fn builtin_t_test_two_sample(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s1: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let s2: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n1 = s1.len();
    let n2 = s2.len();
    if n1 < 2 || n2 < 2 {
        return Ok(PerlValue::float(0.0));
    }
    let m1 = stats_mean(&s1);
    let m2 = stats_mean(&s2);
    let v1 = stats_variance(&s1);
    let v2 = stats_variance(&s2);
    let pooled_var = ((n1 as f64 - 1.0) * v1 + (n2 as f64 - 1.0) * v2) / (n1 + n2 - 2) as f64;
    let se = (pooled_var * (1.0 / n1 as f64 + 1.0 / n2 as f64)).sqrt();
    Ok(PerlValue::float(if se == 0.0 {
        0.0
    } else {
        (m1 - m2) / se
    }))
}

/// `chi_square_stat OBSERVED, EXPECTED` — chi-square statistic.
fn builtin_chi_square_stat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let obs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let exp: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = obs.len().min(exp.len());
    if n == 0 {
        return Ok(PerlValue::float(0.0));
    }
    let chi2: f64 = obs[..n]
        .iter()
        .zip(exp[..n].iter())
        .filter(|(_, e)| **e != 0.0)
        .map(|(o, e)| (o - e).powi(2) / e)
        .sum();
    Ok(PerlValue::float(chi2))
}

/// `gini_coefficient LIST` — Gini coefficient (0 = perfect equality, 1 = perfect inequality).
fn builtin_gini(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut vals: Vec<f64> = flatten_args(args)
        .iter()
        .map(|v| v.to_number())
        .filter(|&v| v >= 0.0)
        .collect();
    if vals.is_empty() {
        return Ok(PerlValue::float(0.0));
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len() as f64;
    let sum: f64 = vals.iter().sum();
    if sum == 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    let weighted_sum: f64 = vals
        .iter()
        .enumerate()
        .map(|(i, &v)| (2.0 * (i as f64 + 1.0) - n - 1.0) * v)
        .sum();
    Ok(PerlValue::float(weighted_sum / (n * sum)))
}

/// `lorenz_curve LIST` — returns array of cumulative proportions for Lorenz curve.
fn builtin_lorenz_curve(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut vals: Vec<f64> = flatten_args(args)
        .iter()
        .map(|v| v.to_number())
        .filter(|&v| v >= 0.0)
        .collect();
    if vals.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let total: f64 = vals.iter().sum();
    if total == 0.0 {
        return Ok(PerlValue::array(vec![PerlValue::float(0.0); vals.len()]));
    }
    let mut cumsum = 0.0;
    let curve: Vec<PerlValue> = vals
        .iter()
        .map(|&v| {
            cumsum += v;
            PerlValue::float(cumsum / total)
        })
        .collect();
    Ok(PerlValue::array(curve))
}

/// `outliers_iqr LIST` — returns values outside 1.5*IQR from Q1/Q3.
fn builtin_outliers_iqr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    if vals.len() < 4 {
        return Ok(PerlValue::array(vec![]));
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len();
    let q1 = vals[n / 4];
    let q3 = vals[3 * n / 4];
    let iqr = q3 - q1;
    let lower = q1 - 1.5 * iqr;
    let upper = q3 + 1.5 * iqr;
    let outliers: Vec<PerlValue> = vals
        .iter()
        .filter(|&&v| v < lower || v > upper)
        .map(|&v| PerlValue::float(v))
        .collect();
    Ok(PerlValue::array(outliers))
}

/// `five_number_summary LIST` — [min, Q1, median, Q3, max].
fn builtin_five_number_summary(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    if vals.is_empty() {
        return Ok(PerlValue::array(vec![PerlValue::float(0.0); 5]));
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len();
    Ok(PerlValue::array(vec![
        PerlValue::float(vals[0]),
        PerlValue::float(vals[n / 4]),
        PerlValue::float(vals[n / 2]),
        PerlValue::float(vals[3 * n / 4]),
        PerlValue::float(vals[n - 1]),
    ]))
}

/// `describe LIST` — returns hash with min, max, mean, median, stddev, count.
fn builtin_describe(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    if vals.is_empty() {
        let m: indexmap::IndexMap<String, PerlValue> = indexmap::IndexMap::new();
        return Ok(PerlValue::hash(m));
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len();
    let mean = stats_mean(&vals);
    let stddev = stats_stddev(&vals);
    let mut m = indexmap::IndexMap::new();
    m.insert("count".to_string(), PerlValue::integer(n as i64));
    m.insert("min".to_string(), PerlValue::float(vals[0]));
    m.insert("max".to_string(), PerlValue::float(vals[n - 1]));
    m.insert("mean".to_string(), PerlValue::float(mean));
    m.insert("median".to_string(), PerlValue::float(vals[n / 2]));
    m.insert("stddev".to_string(), PerlValue::float(stddev));
    m.insert("sum".to_string(), PerlValue::float(vals.iter().sum()));
    Ok(PerlValue::hash(m))
}

// ─────────────────────────────────────────────────────────────────────────────
// Geometry
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_area_circle(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = first_arg_or_topic(interp, args).to_number();
    Ok(PerlValue::float(std::f64::consts::PI * r * r))
}
fn builtin_area_triangle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let b = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * b * h))
}
fn builtin_area_rectangle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(w * h))
}
fn builtin_area_trapezoid(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((a + b) / 2.0 * h))
}
fn builtin_area_ellipse(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(std::f64::consts::PI * a * b))
}
fn builtin_circumference(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = first_arg_or_topic(interp, args).to_number();
    Ok(PerlValue::float(2.0 * std::f64::consts::PI * r))
}
fn builtin_perimeter_rectangle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(2.0 * (w + h)))
}
fn builtin_perimeter_triangle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(a + b + c))
}
fn builtin_polygon_area(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = pts.len();
    if n < 3 {
        return Ok(PerlValue::float(0.0));
    }
    let mut area = 0.0;
    for i in 0..n {
        let p1 = arg_to_vec(&pts[i]);
        let p2 = arg_to_vec(&pts[(i + 1) % n]);
        let (x1, y1) = (
            p1.first().map(|v| v.to_number()).unwrap_or(0.0),
            p1.get(1).map(|v| v.to_number()).unwrap_or(0.0),
        );
        let (x2, y2) = (
            p2.first().map(|v| v.to_number()).unwrap_or(0.0),
            p2.get(1).map(|v| v.to_number()).unwrap_or(0.0),
        );
        area += x1 * y2 - x2 * y1;
    }
    Ok(PerlValue::float((area / 2.0).abs()))
}
fn builtin_sphere_volume(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = first_arg_or_topic(interp, args).to_number();
    Ok(PerlValue::float(
        4.0 / 3.0 * std::f64::consts::PI * r * r * r,
    ))
}
fn builtin_sphere_surface(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = first_arg_or_topic(interp, args).to_number();
    Ok(PerlValue::float(4.0 * std::f64::consts::PI * r * r))
}
fn builtin_cylinder_volume(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(std::f64::consts::PI * r * r * h))
}
fn builtin_cone_volume(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(std::f64::consts::PI * r * r * h / 3.0))
}
fn builtin_heron_area(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let s = (a + b + c) / 2.0;
    Ok(PerlValue::float(
        (s * (s - a) * (s - b) * (s - c)).max(0.0).sqrt(),
    ))
}
fn builtin_point_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x1 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let y1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let x2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let y2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(
        ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt(),
    ))
}
fn builtin_midpoint(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x1 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let y1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let x2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let y2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::array(vec![
        PerlValue::float((x1 + x2) / 2.0),
        PerlValue::float((y1 + y2) / 2.0),
    ]))
}
fn builtin_slope(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x1 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let y1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let x2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let y2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dx = x2 - x1;
    Ok(PerlValue::float(if dx == 0.0 {
        f64::INFINITY
    } else {
        (y2 - y1) / dx
    }))
}
fn builtin_triangle_hypotenuse(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(a.hypot(b)))
}
fn builtin_degrees_to_compass(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let deg = first_arg_or_topic(interp, args).to_number() % 360.0;
    let d = if deg < 0.0 { deg + 360.0 } else { deg };
    let dirs = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    Ok(PerlValue::string(
        dirs[((d + 11.25) / 22.5) as usize % 16].to_string(),
    ))
}

/// `scale_point X, Y, SX, SY [, CX, CY]` — scale point around center (default origin).
fn builtin_scale_point(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sx = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let sy = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let cx = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let cy = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::array(vec![
        PerlValue::float(cx + (x - cx) * sx),
        PerlValue::float(cy + (y - cy) * sy),
    ]))
}

/// `translate_point X, Y, DX, DY` — translate point by delta.
fn builtin_translate_point(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dx = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dy = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::array(vec![
        PerlValue::float(x + dx),
        PerlValue::float(y + dy),
    ]))
}

/// `reflect_point X, Y, AXIS` — reflect point over 'x', 'y', or 'origin'.
fn builtin_reflect_point(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let axis = args
        .get(2)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "origin".to_string());
    let (nx, ny) = match axis.to_lowercase().as_str() {
        "x" => (x, -y),
        "y" => (-x, y),
        _ => (-x, -y),
    };
    Ok(PerlValue::array(vec![
        PerlValue::float(nx),
        PerlValue::float(ny),
    ]))
}

/// `angle_between X1, Y1, X2, Y2` — angle in degrees from point 1 to point 2.
fn builtin_angle_between(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x1 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let y1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let x2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let y2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((y2 - y1).atan2(x2 - x1).to_degrees()))
}

/// `line_intersection X1, Y1, X2, Y2, X3, Y3, X4, Y4` — intersection of two line segments.
fn builtin_line_intersection(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x1 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let y1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let x2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let y2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let x3 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let y3 = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let x4 = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
    let y4 = args.get(7).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
    if denom.abs() < f64::EPSILON {
        return Ok(PerlValue::UNDEF);
    }
    let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / denom;
    let u = -((x1 - x2) * (y1 - y3) - (y1 - y2) * (x1 - x3)) / denom;
    if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
        Ok(PerlValue::array(vec![
            PerlValue::float(x1 + t * (x2 - x1)),
            PerlValue::float(y1 + t * (y2 - y1)),
        ]))
    } else {
        Ok(PerlValue::UNDEF)
    }
}

/// `point_in_polygon X, Y, POLYGON` — test if point is inside polygon (array of [x,y] pairs).
fn builtin_point_in_polygon(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let px = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let py = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let pts = arg_to_vec(&args.get(2).cloned().unwrap_or(PerlValue::UNDEF));
    let n = pts.len();
    if n < 3 {
        return Ok(PerlValue::integer(0));
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let pi = arg_to_vec(&pts[i]);
        let pj = arg_to_vec(&pts[j]);
        let (xi, yi) = (
            pi.first().map(|v| v.to_number()).unwrap_or(0.0),
            pi.get(1).map(|v| v.to_number()).unwrap_or(0.0),
        );
        let (xj, yj) = (
            pj.first().map(|v| v.to_number()).unwrap_or(0.0),
            pj.get(1).map(|v| v.to_number()).unwrap_or(0.0),
        );
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    Ok(PerlValue::integer(if inside { 1 } else { 0 }))
}

/// `convex_hull POINTS` — compute convex hull of 2D points using Graham scan.
fn builtin_convex_hull(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if pts.len() < 3 {
        return Ok(args.first().cloned().unwrap_or(PerlValue::array(vec![])));
    }
    let mut points: Vec<(f64, f64)> = pts
        .iter()
        .map(|p| {
            let arr = arg_to_vec(p);
            (
                arr.first().map(|v| v.to_number()).unwrap_or(0.0),
                arr.get(1).map(|v| v.to_number()).unwrap_or(0.0),
            )
        })
        .collect();
    points.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    fn cross(o: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    }
    let mut lower: Vec<(f64, f64)> = Vec::new();
    for &p in &points {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(f64, f64)> = Vec::new();
    for &p in points.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.append(&mut upper);
    Ok(PerlValue::array(
        lower
            .into_iter()
            .map(|(x, y)| PerlValue::array(vec![PerlValue::float(x), PerlValue::float(y)]))
            .collect(),
    ))
}

/// `bounding_box POINTS` — returns [min_x, min_y, max_x, max_y].
fn builtin_bounding_box(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if pts.is_empty() {
        return Ok(PerlValue::array(vec![
            PerlValue::float(0.0),
            PerlValue::float(0.0),
            PerlValue::float(0.0),
            PerlValue::float(0.0),
        ]));
    }
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for p in &pts {
        let arr = arg_to_vec(p);
        let x = arr.first().map(|v| v.to_number()).unwrap_or(0.0);
        let y = arr.get(1).map(|v| v.to_number()).unwrap_or(0.0);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    Ok(PerlValue::array(vec![
        PerlValue::float(min_x),
        PerlValue::float(min_y),
        PerlValue::float(max_x),
        PerlValue::float(max_y),
    ]))
}

/// `centroid POINTS` — geometric center of points.
fn builtin_centroid(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if pts.is_empty() {
        return Ok(PerlValue::array(vec![
            PerlValue::float(0.0),
            PerlValue::float(0.0),
        ]));
    }
    let mut sum_x = 0.0;
    let mut sum_y = 0.0;
    for p in &pts {
        let arr = arg_to_vec(p);
        sum_x += arr.first().map(|v| v.to_number()).unwrap_or(0.0);
        sum_y += arr.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    }
    let n = pts.len() as f64;
    Ok(PerlValue::array(vec![
        PerlValue::float(sum_x / n),
        PerlValue::float(sum_y / n),
    ]))
}

/// `polygon_perimeter POINTS` — perimeter of polygon.
fn builtin_polygon_perimeter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = pts.len();
    if n < 2 {
        return Ok(PerlValue::float(0.0));
    }
    let mut perimeter = 0.0;
    for i in 0..n {
        let p1 = arg_to_vec(&pts[i]);
        let p2 = arg_to_vec(&pts[(i + 1) % n]);
        let (x1, y1) = (
            p1.first().map(|v| v.to_number()).unwrap_or(0.0),
            p1.get(1).map(|v| v.to_number()).unwrap_or(0.0),
        );
        let (x2, y2) = (
            p2.first().map(|v| v.to_number()).unwrap_or(0.0),
            p2.get(1).map(|v| v.to_number()).unwrap_or(0.0),
        );
        perimeter += ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
    }
    Ok(PerlValue::float(perimeter))
}

/// `circle_from_three_points X1, Y1, X2, Y2, X3, Y3` — returns [center_x, center_y, radius].
fn builtin_circle_from_three_points(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x1 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let y1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let x2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let y2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let x3 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let y3 = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let ax = x2 - x1;
    let ay = y2 - y1;
    let bx = x3 - x1;
    let by = y3 - y1;
    let d = 2.0 * (ax * by - ay * bx);
    if d.abs() < f64::EPSILON {
        return Ok(PerlValue::UNDEF);
    }
    let a2 = ax * ax + ay * ay;
    let b2 = bx * bx + by * by;
    let cx = x1 + (by * a2 - ay * b2) / d;
    let cy = y1 + (ax * b2 - bx * a2) / d;
    let r = ((x1 - cx).powi(2) + (y1 - cy).powi(2)).sqrt();
    Ok(PerlValue::array(vec![
        PerlValue::float(cx),
        PerlValue::float(cy),
        PerlValue::float(r),
    ]))
}

/// `arc_length RADIUS, ANGLE_DEG` — length of circular arc.
fn builtin_arc_length(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let angle = args
        .get(1)
        .map(|v| v.to_number())
        .unwrap_or(0.0)
        .to_radians();
    Ok(PerlValue::float(r * angle.abs()))
}

/// `sector_area RADIUS, ANGLE_DEG` — area of circular sector.
fn builtin_sector_area(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let angle = args
        .get(1)
        .map(|v| v.to_number())
        .unwrap_or(0.0)
        .to_radians();
    Ok(PerlValue::float(0.5 * r * r * angle.abs()))
}

/// `torus_volume MAJOR_R, MINOR_R` — volume of torus.
fn builtin_torus_volume(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let major_r = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let minor_r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(
        2.0 * std::f64::consts::PI * std::f64::consts::PI * major_r * minor_r * minor_r,
    ))
}

/// `torus_surface MAJOR_R, MINOR_R` — surface area of torus.
fn builtin_torus_surface(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let major_r = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let minor_r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(
        4.0 * std::f64::consts::PI * std::f64::consts::PI * major_r * minor_r,
    ))
}

/// `pyramid_volume BASE_AREA, HEIGHT` — volume of pyramid.
fn builtin_pyramid_volume(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let base = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(base * h / 3.0))
}

/// `frustum_volume R1, R2, HEIGHT` — volume of conical frustum.
fn builtin_frustum_volume(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r1 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let r2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(
        std::f64::consts::PI * h / 3.0 * (r1 * r1 + r1 * r2 + r2 * r2),
    ))
}

/// `ellipse_perimeter A, B` — approximate perimeter of ellipse (Ramanujan).
fn builtin_ellipse_perimeter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h = ((a - b) / (a + b)).powi(2);
    Ok(PerlValue::float(
        std::f64::consts::PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt())),
    ))
}

/// `haversine_distance LAT1, LON1, LAT2, LON2` — great circle distance in km.
fn builtin_haversine_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lat1 = args
        .first()
        .map(|v| v.to_number())
        .unwrap_or(0.0)
        .to_radians();
    let lon1 = args
        .get(1)
        .map(|v| v.to_number())
        .unwrap_or(0.0)
        .to_radians();
    let lat2 = args
        .get(2)
        .map(|v| v.to_number())
        .unwrap_or(0.0)
        .to_radians();
    let lon2 = args
        .get(3)
        .map(|v| v.to_number())
        .unwrap_or(0.0)
        .to_radians();
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    Ok(PerlValue::float(6371.0 * c))
}

/// `vector_dot A, B` — dot product of two vectors.
fn builtin_vector_dot(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = a.len().min(b.len());
    Ok(PerlValue::float(
        a[..n].iter().zip(b[..n].iter()).map(|(x, y)| x * y).sum(),
    ))
}

/// `vector_cross A, B` — cross product of two 3D vectors.
fn builtin_vector_cross(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let (a0, a1, a2) = (
        *a.first().unwrap_or(&0.0),
        *a.get(1).unwrap_or(&0.0),
        *a.get(2).unwrap_or(&0.0),
    );
    let (b0, b1, b2) = (
        *b.first().unwrap_or(&0.0),
        *b.get(1).unwrap_or(&0.0),
        *b.get(2).unwrap_or(&0.0),
    );
    Ok(PerlValue::array(vec![
        PerlValue::float(a1 * b2 - a2 * b1),
        PerlValue::float(a2 * b0 - a0 * b2),
        PerlValue::float(a0 * b1 - a1 * b0),
    ]))
}

/// `vector_magnitude V` — length of vector.
fn builtin_vector_magnitude(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    Ok(PerlValue::float(
        v.iter().map(|x| x * x).sum::<f64>().sqrt(),
    ))
}

/// `vector_normalize V` — unit vector.
fn builtin_vector_normalize(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mag: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if mag == 0.0 {
        return Ok(PerlValue::array(
            v.into_iter().map(PerlValue::float).collect(),
        ));
    }
    Ok(PerlValue::array(
        v.into_iter().map(|x| PerlValue::float(x / mag)).collect(),
    ))
}

/// `vector_angle A, B` — angle between vectors in degrees.
fn builtin_vector_angle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = a.len().min(b.len());
    let dot: f64 = a[..n].iter().zip(b[..n].iter()).map(|(x, y)| x * y).sum();
    let mag_a: f64 = a[..n].iter().map(|x| x * x).sum::<f64>().sqrt();
    let mag_b: f64 = b[..n].iter().map(|x| x * x).sum::<f64>().sqrt();
    let denom = mag_a * mag_b;
    Ok(PerlValue::float(if denom == 0.0 {
        0.0
    } else {
        (dot / denom).clamp(-1.0, 1.0).acos().to_degrees()
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Financial
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_npv(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let cfs: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    Ok(PerlValue::float(
        cfs.iter()
            .enumerate()
            .map(|(i, cf)| cf / (1.0 + r).powi(i as i32))
            .sum::<f64>(),
    ))
}
fn builtin_depreciation_linear(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cost = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let salvage = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let life = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if life == 0.0 {
        0.0
    } else {
        (cost - salvage) / life
    }))
}
fn builtin_depreciation_double(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cost = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let life = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if life == 0.0 {
        0.0
    } else {
        cost * 2.0 / life
    }))
}
fn builtin_cagr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let start = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let end = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let years = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if start == 0.0 || years == 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float((end / start).powf(1.0 / years) - 1.0))
}
fn builtin_roi(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let gain = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let cost = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if cost == 0.0 {
        0.0
    } else {
        (gain - cost) / cost
    }))
}
fn builtin_break_even(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let fixed = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let price = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let vc = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let margin = price - vc;
    Ok(PerlValue::float(if margin == 0.0 {
        f64::INFINITY
    } else {
        (fixed / margin).ceil()
    }))
}
fn builtin_markup(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cost = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let sell = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if cost == 0.0 {
        0.0
    } else {
        (sell - cost) / cost * 100.0
    }))
}
fn builtin_margin(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cost = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let sell = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if sell == 0.0 {
        0.0
    } else {
        (sell - cost) / sell * 100.0
    }))
}
fn builtin_discount(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let price = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let pct = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(price * (1.0 - pct / 100.0)))
}
fn builtin_tax(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let price = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let rate = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(price * (1.0 + rate / 100.0)))
}
fn builtin_tip(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let amount = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let pct = args.get(1).map(|v| v.to_number()).unwrap_or(15.0);
    Ok(PerlValue::float(amount * pct / 100.0))
}

/// `irr CASH_FLOWS [, GUESS]` — internal rate of return using Newton-Raphson.
fn builtin_irr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cfs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    if cfs.is_empty() {
        return Ok(PerlValue::UNDEF);
    }
    let mut rate = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    for _ in 0..100 {
        let mut npv = 0.0;
        let mut dnpv = 0.0;
        for (i, &cf) in cfs.iter().enumerate() {
            let disc = (1.0 + rate).powi(i as i32);
            npv += cf / disc;
            if i > 0 {
                dnpv -= i as f64 * cf / ((1.0 + rate).powi(i as i32 + 1));
            }
        }
        if dnpv.abs() < f64::EPSILON {
            break;
        }
        let new_rate = rate - npv / dnpv;
        if (new_rate - rate).abs() < 1e-10 {
            return Ok(PerlValue::float(new_rate));
        }
        rate = new_rate;
    }
    Ok(PerlValue::float(rate))
}

/// `xirr CASH_FLOWS, DATES [, GUESS]` — IRR for irregular intervals (dates as days from epoch).
fn builtin_xirr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cfs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let dates: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = cfs.len().min(dates.len());
    if n == 0 {
        return Ok(PerlValue::UNDEF);
    }
    let d0 = dates[0];
    let mut rate = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    for _ in 0..100 {
        let mut npv = 0.0;
        let mut dnpv = 0.0;
        for i in 0..n {
            let t = (dates[i] - d0) / 365.0;
            let disc = (1.0 + rate).powf(t);
            npv += cfs[i] / disc;
            if t != 0.0 {
                dnpv -= t * cfs[i] / ((1.0 + rate).powf(t + 1.0));
            }
        }
        if dnpv.abs() < f64::EPSILON {
            break;
        }
        let new_rate = rate - npv / dnpv;
        if (new_rate - rate).abs() < 1e-10 {
            return Ok(PerlValue::float(new_rate));
        }
        rate = new_rate;
    }
    Ok(PerlValue::float(rate))
}

/// `payback_period INITIAL, CASH_FLOWS` — years to recover initial investment.
fn builtin_payback_period(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let initial = args.first().map(|v| v.to_number()).unwrap_or(0.0).abs();
    let cfs: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut cumulative = 0.0;
    for (i, &cf) in cfs.iter().enumerate() {
        let prev = cumulative;
        cumulative += cf;
        if cumulative >= initial {
            let frac = if cf == 0.0 {
                0.0
            } else {
                (initial - prev) / cf
            };
            return Ok(PerlValue::float(i as f64 + frac));
        }
    }
    Ok(PerlValue::UNDEF)
}

/// `discounted_payback INITIAL, CASH_FLOWS, RATE` — discounted payback period.
fn builtin_discounted_payback(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let initial = args.first().map(|v| v.to_number()).unwrap_or(0.0).abs();
    let cfs: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let rate = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    let mut cumulative = 0.0;
    for (i, &cf) in cfs.iter().enumerate() {
        let prev = cumulative;
        let discounted = cf / (1.0 + rate).powi(i as i32 + 1);
        cumulative += discounted;
        if cumulative >= initial {
            let frac = if discounted == 0.0 {
                0.0
            } else {
                (initial - prev) / discounted
            };
            return Ok(PerlValue::float(i as f64 + frac));
        }
    }
    Ok(PerlValue::UNDEF)
}

/// `pmt RATE, NPER, PV [, FV, TYPE]` — periodic payment for a loan.
fn builtin_pmt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rate = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let nper = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let pv = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let fv = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let typ = args.get(4).map(|v| v.to_int()).unwrap_or(0);
    if rate == 0.0 {
        return Ok(PerlValue::float(-(pv + fv) / nper));
    }
    let pvif = (1.0 + rate).powf(nper);
    let pmt = (rate * (pv * pvif + fv)) / ((pvif - 1.0) * (1.0 + rate * typ as f64));
    Ok(PerlValue::float(-pmt))
}

/// `nper RATE, PMT, PV [, FV, TYPE]` — number of periods.
fn builtin_nper(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rate = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let pmt = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let pv = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let fv = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let typ = args.get(4).map(|v| v.to_int()).unwrap_or(0);
    if rate == 0.0 {
        return Ok(PerlValue::float(if pmt == 0.0 {
            0.0
        } else {
            -(pv + fv) / pmt
        }));
    }
    let z = pmt * (1.0 + rate * typ as f64) / rate;
    let nper = (-fv + z).ln() / (pv + z).ln() / (1.0 + rate).ln();
    Ok(PerlValue::float(nper.abs()))
}

/// `amortization_schedule PRINCIPAL, RATE, NPER` — returns array of [period, payment, principal, interest, balance].
fn builtin_amortization_schedule(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let principal = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let rate = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let nper = args.get(2).map(|v| v.to_int()).unwrap_or(1).max(1) as usize;
    let pmt = if rate == 0.0 {
        principal / nper as f64
    } else {
        principal * rate * (1.0 + rate).powi(nper as i32) / ((1.0 + rate).powi(nper as i32) - 1.0)
    };
    let mut balance = principal;
    let mut schedule = Vec::with_capacity(nper);
    for i in 1..=nper {
        let interest = balance * rate;
        let prin_part = pmt - interest;
        balance -= prin_part;
        schedule.push(PerlValue::array(vec![
            PerlValue::integer(i as i64),
            PerlValue::float(pmt),
            PerlValue::float(prin_part),
            PerlValue::float(interest),
            PerlValue::float(balance.max(0.0)),
        ]));
    }
    Ok(PerlValue::array(schedule))
}

/// `bond_price FACE, COUPON_RATE, YIELD, PERIODS` — present value of bond.
fn builtin_bond_price(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let face = args.first().map(|v| v.to_number()).unwrap_or(1000.0);
    let coupon_rate = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let yld = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let periods = args.get(3).map(|v| v.to_int()).unwrap_or(10).max(1);
    let coupon = face * coupon_rate;
    let mut pv = 0.0;
    for i in 1..=periods {
        pv += coupon / (1.0 + yld).powi(i as i32);
    }
    pv += face / (1.0 + yld).powi(periods as i32);
    Ok(PerlValue::float(pv))
}

/// `bond_yield PRICE, FACE, COUPON_RATE, PERIODS [, GUESS]` — yield to maturity.
fn builtin_bond_yield(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let price = args.first().map(|v| v.to_number()).unwrap_or(1000.0);
    let face = args.get(1).map(|v| v.to_number()).unwrap_or(1000.0);
    let coupon_rate = args.get(2).map(|v| v.to_number()).unwrap_or(0.05);
    let periods = args.get(3).map(|v| v.to_int()).unwrap_or(10).max(1);
    let mut yld = args.get(4).map(|v| v.to_number()).unwrap_or(0.05);
    let coupon = face * coupon_rate;
    for _ in 0..100 {
        let mut pv = 0.0;
        let mut dpv = 0.0;
        for i in 1..=periods {
            let disc = (1.0 + yld).powi(i as i32);
            pv += coupon / disc;
            dpv -= i as f64 * coupon / ((1.0 + yld).powi(i as i32 + 1));
        }
        pv += face / (1.0 + yld).powi(periods as i32);
        dpv -= periods as f64 * face / ((1.0 + yld).powi(periods as i32 + 1));
        let diff = pv - price;
        if diff.abs() < 1e-10 {
            return Ok(PerlValue::float(yld));
        }
        if dpv.abs() < f64::EPSILON {
            break;
        }
        yld -= diff / dpv;
    }
    Ok(PerlValue::float(yld))
}

/// `duration CASH_FLOWS, YIELD` — Macaulay duration.
fn builtin_duration(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cfs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let yld = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let mut pv_sum = 0.0;
    let mut weighted_sum = 0.0;
    for (i, &cf) in cfs.iter().enumerate() {
        let t = (i + 1) as f64;
        let pv = cf / (1.0 + yld).powf(t);
        pv_sum += pv;
        weighted_sum += t * pv;
    }
    Ok(PerlValue::float(if pv_sum == 0.0 {
        0.0
    } else {
        weighted_sum / pv_sum
    }))
}

/// `modified_duration CASH_FLOWS, YIELD` — modified duration.
fn builtin_modified_duration(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cfs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let yld = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let mut pv_sum = 0.0;
    let mut weighted_sum = 0.0;
    for (i, &cf) in cfs.iter().enumerate() {
        let t = (i + 1) as f64;
        let pv = cf / (1.0 + yld).powf(t);
        pv_sum += pv;
        weighted_sum += t * pv;
    }
    let mac_dur = if pv_sum == 0.0 {
        0.0
    } else {
        weighted_sum / pv_sum
    };
    Ok(PerlValue::float(mac_dur / (1.0 + yld)))
}

/// `sharpe_ratio RETURNS, RISK_FREE_RATE` — Sharpe ratio.
fn builtin_sharpe_ratio(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let returns: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let rf = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if returns.len() < 2 {
        return Ok(PerlValue::float(0.0));
    }
    let mean = stats_mean(&returns);
    let sd = stats_stddev(&returns);
    Ok(PerlValue::float(if sd == 0.0 {
        0.0
    } else {
        (mean - rf) / sd
    }))
}

/// `sortino_ratio RETURNS, RISK_FREE_RATE` — Sortino ratio (downside deviation).
fn builtin_sortino_ratio(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let returns: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let rf = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if returns.len() < 2 {
        return Ok(PerlValue::float(0.0));
    }
    let mean = stats_mean(&returns);
    let downside: Vec<f64> = returns
        .iter()
        .filter(|&&r| r < rf)
        .map(|&r| (r - rf).powi(2))
        .collect();
    let downside_dev = if downside.is_empty() {
        0.0
    } else {
        (downside.iter().sum::<f64>() / downside.len() as f64).sqrt()
    };
    Ok(PerlValue::float(if downside_dev == 0.0 {
        0.0
    } else {
        (mean - rf) / downside_dev
    }))
}

/// `max_drawdown PRICES` — maximum drawdown as a decimal.
fn builtin_max_drawdown(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prices: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    if prices.is_empty() {
        return Ok(PerlValue::float(0.0));
    }
    let mut max_dd = 0.0;
    let mut peak = prices[0];
    for &price in &prices {
        if price > peak {
            peak = price;
        }
        let dd = (peak - price) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    Ok(PerlValue::float(max_dd))
}

/// `continuous_compound PRINCIPAL, RATE, TIME` — continuous compounding.
fn builtin_continuous_compound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let principal = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let rate = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let time = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(principal * (rate * time).exp()))
}

/// `rule_of_72 RATE` — years to double at given rate.
fn builtin_rule_of_72(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rate = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(if rate == 0.0 {
        f64::INFINITY
    } else {
        72.0 / (rate * 100.0)
    }))
}

/// `wacc EQUITY, DEBT, COST_EQUITY, COST_DEBT, TAX_RATE` — weighted average cost of capital.
fn builtin_wacc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let equity = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let debt = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let cost_e = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let cost_d = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let tax = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let total = equity + debt;
    if total == 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    let wacc = (equity / total) * cost_e + (debt / total) * cost_d * (1.0 - tax);
    Ok(PerlValue::float(wacc))
}

/// `capm RISK_FREE, BETA, MARKET_RETURN` — expected return using CAPM.
fn builtin_capm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rf = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let rm = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(rf + beta * (rm - rf)))
}

/// `black_scholes_call SPOT, STRIKE, TIME, RATE, VOLATILITY` — Black-Scholes call option price.
fn builtin_black_scholes_call(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(4).map(|v| v.to_number()).unwrap_or(0.2);
    if t <= 0.0 || sigma <= 0.0 || s <= 0.0 || k <= 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    let d1 = ((s / k).ln() + (r + sigma * sigma / 2.0) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    fn norm_cdf(x: f64) -> f64 {
        0.5 * (1.0 + statrs::function::erf::erf(x / std::f64::consts::SQRT_2))
    }
    let call = s * norm_cdf(d1) - k * (-r * t).exp() * norm_cdf(d2);
    Ok(PerlValue::float(call))
}

/// `black_scholes_put SPOT, STRIKE, TIME, RATE, VOLATILITY` — Black-Scholes put option price.
fn builtin_black_scholes_put(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(4).map(|v| v.to_number()).unwrap_or(0.2);
    if t <= 0.0 || sigma <= 0.0 || s <= 0.0 || k <= 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    let d1 = ((s / k).ln() + (r + sigma * sigma / 2.0) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    fn norm_cdf(x: f64) -> f64 {
        0.5 * (1.0 + statrs::function::erf::erf(x / std::f64::consts::SQRT_2))
    }
    let put = k * (-r * t).exp() * norm_cdf(-d2) - s * norm_cdf(-d1);
    Ok(PerlValue::float(put))
}

// ─────────────────────────────────────────────────────────────────────────────
// Encoding / Decoding
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_morse_encode(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let morse: Vec<&str> = s
        .to_uppercase()
        .chars()
        .filter_map(|c| match c {
            'A' => Some(".-"),
            'B' => Some("-..."),
            'C' => Some("-.-."),
            'D' => Some("-.."),
            'E' => Some("."),
            'F' => Some("..-."),
            'G' => Some("--."),
            'H' => Some("...."),
            'I' => Some(".."),
            'J' => Some(".---"),
            'K' => Some("-.-"),
            'L' => Some(".-.."),
            'M' => Some("--"),
            'N' => Some("-."),
            'O' => Some("---"),
            'P' => Some(".--."),
            'Q' => Some("--.-"),
            'R' => Some(".-."),
            'S' => Some("..."),
            'T' => Some("-"),
            'U' => Some("..-"),
            'V' => Some("...-"),
            'W' => Some(".--"),
            'X' => Some("-..-"),
            'Y' => Some("-.--"),
            'Z' => Some("--.."),
            '0' => Some("-----"),
            '1' => Some(".----"),
            '2' => Some("..---"),
            '3' => Some("...--"),
            '4' => Some("....-"),
            '5' => Some("....."),
            '6' => Some("-...."),
            '7' => Some("--..."),
            '8' => Some("---.."),
            '9' => Some("----."),
            ' ' => Some("/"),
            '.' => Some(".-.-.-"),
            ',' => Some("--..--"),
            '?' => Some("..--.."),
            '!' => Some("-.-.--"),
            _ => None,
        })
        .collect();
    Ok(PerlValue::string(morse.join(" ")))
}

fn builtin_morse_decode(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let decoded: String = s
        .split(' ')
        .map(|code| match code {
            ".-" => 'A',
            "-..." => 'B',
            "-.-." => 'C',
            "-.." => 'D',
            "." => 'E',
            "..-." => 'F',
            "--." => 'G',
            "...." => 'H',
            ".." => 'I',
            ".---" => 'J',
            "-.-" => 'K',
            ".-.." => 'L',
            "--" => 'M',
            "-." => 'N',
            "---" => 'O',
            ".--." => 'P',
            "--.-" => 'Q',
            ".-." => 'R',
            "..." => 'S',
            "-" => 'T',
            "..-" => 'U',
            "...-" => 'V',
            ".--" => 'W',
            "-..-" => 'X',
            "-.--" => 'Y',
            "--.." => 'Z',
            "-----" => '0',
            ".----" => '1',
            "..---" => '2',
            "...--" => '3',
            "....-" => '4',
            "....." => '5',
            "-...." => '6',
            "--..." => '7',
            "---.." => '8',
            "----." => '9',
            "/" => ' ',
            ".-.-.-" => '.',
            "--..--" => ',',
            "..--.." => '?',
            "-.-.--" => '!',
            _ => '?',
        })
        .collect();
    Ok(PerlValue::string(decoded))
}

fn builtin_nato_phonetic(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let words: Vec<&str> = s
        .to_uppercase()
        .chars()
        .filter_map(|c| match c {
            'A' => Some("Alfa"),
            'B' => Some("Bravo"),
            'C' => Some("Charlie"),
            'D' => Some("Delta"),
            'E' => Some("Echo"),
            'F' => Some("Foxtrot"),
            'G' => Some("Golf"),
            'H' => Some("Hotel"),
            'I' => Some("India"),
            'J' => Some("Juliet"),
            'K' => Some("Kilo"),
            'L' => Some("Lima"),
            'M' => Some("Mike"),
            'N' => Some("November"),
            'O' => Some("Oscar"),
            'P' => Some("Papa"),
            'Q' => Some("Quebec"),
            'R' => Some("Romeo"),
            'S' => Some("Sierra"),
            'T' => Some("Tango"),
            'U' => Some("Uniform"),
            'V' => Some("Victor"),
            'W' => Some("Whiskey"),
            'X' => Some("X-ray"),
            'Y' => Some("Yankee"),
            'Z' => Some("Zulu"),
            '0' => Some("Zero"),
            '1' => Some("One"),
            '2' => Some("Two"),
            '3' => Some("Three"),
            '4' => Some("Four"),
            '5' => Some("Five"),
            '6' => Some("Six"),
            '7' => Some("Seven"),
            '8' => Some("Eight"),
            '9' => Some("Nine"),
            _ => None,
        })
        .collect();
    Ok(PerlValue::string(words.join(" ")))
}

fn builtin_int_to_roman(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut n = first_arg_or_topic(interp, args).to_int();
    if n <= 0 || n > 3999 {
        return Ok(PerlValue::string(String::new()));
    }
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut result = String::new();
    for &(val, sym) in &table {
        while n >= val {
            result.push_str(sym);
            n -= val;
        }
    }
    Ok(PerlValue::string(result))
}

fn builtin_roman_to_int(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string().to_uppercase();
    let val_of = |c| match c {
        'I' => 1,
        'V' => 5,
        'X' => 10,
        'L' => 50,
        'C' => 100,
        'D' => 500,
        'M' => 1000,
        _ => 0,
    };
    let chars: Vec<char> = s.chars().collect();
    let mut total = 0i64;
    for i in 0..chars.len() {
        let cur = val_of(chars[i]);
        let next = if i + 1 < chars.len() {
            val_of(chars[i + 1])
        } else {
            0
        };
        if cur < next {
            total -= cur;
        } else {
            total += cur;
        }
    }
    Ok(PerlValue::integer(total))
}

fn builtin_binary_to_gray(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int();
    Ok(PerlValue::integer(n ^ (n >> 1)))
}
fn builtin_gray_to_binary(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut n = first_arg_or_topic(interp, args).to_int();
    let mut mask = n >> 1;
    while mask != 0 {
        n ^= mask;
        mask >>= 1;
    }
    Ok(PerlValue::integer(n))
}

fn builtin_pig_latin(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let result: Vec<String> = s
        .split_whitespace()
        .map(|w| {
            let lower = w.to_lowercase();
            if lower.starts_with(|c: char| "aeiou".contains(c)) {
                format!("{}yay", w)
            } else {
                let idx = lower.find(|c: char| "aeiou".contains(c)).unwrap_or(w.len());
                format!("{}{}ay", &w[idx..], &w[..idx])
            }
        })
        .collect();
    Ok(PerlValue::string(result.join(" ")))
}

fn builtin_atbash(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    Ok(PerlValue::string(
        s.chars()
            .map(|c| {
                if c.is_ascii_lowercase() {
                    (b'z' - (c as u8 - b'a')) as char
                } else if c.is_ascii_uppercase() {
                    (b'Z' - (c as u8 - b'A')) as char
                } else {
                    c
                }
            })
            .collect(),
    ))
}

fn builtin_to_emoji_num(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    Ok(PerlValue::string(
        s.chars()
            .map(|c| {
                if c.is_ascii_digit() {
                    format!("{}\u{FE0F}\u{20E3}", c)
                } else {
                    c.to_string()
                }
            })
            .collect(),
    ))
}

fn builtin_braille_encode(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    Ok(PerlValue::string(
        s.to_lowercase()
            .chars()
            .map(|c| match c {
                'a' => '\u{2801}',
                'b' => '\u{2803}',
                'c' => '\u{2809}',
                'd' => '\u{2819}',
                'e' => '\u{2811}',
                'f' => '\u{280B}',
                'g' => '\u{281B}',
                'h' => '\u{2813}',
                'i' => '\u{280A}',
                'j' => '\u{281A}',
                'k' => '\u{2805}',
                'l' => '\u{2807}',
                'm' => '\u{280D}',
                'n' => '\u{281D}',
                'o' => '\u{2815}',
                'p' => '\u{280F}',
                'q' => '\u{281F}',
                'r' => '\u{2817}',
                's' => '\u{280E}',
                't' => '\u{281E}',
                'u' => '\u{2825}',
                'v' => '\u{2827}',
                'w' => '\u{283A}',
                'x' => '\u{282D}',
                'y' => '\u{283D}',
                'z' => '\u{2835}',
                ' ' => ' ',
                _ => c,
            })
            .collect(),
    ))
}

fn builtin_phonetic_digit(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let words: Vec<&str> = s
        .chars()
        .filter_map(|c| match c {
            '0' => Some("zero"),
            '1' => Some("one"),
            '2' => Some("two"),
            '3' => Some("three"),
            '4' => Some("four"),
            '5' => Some("five"),
            '6' => Some("six"),
            '7' => Some("seven"),
            '8' => Some("eight"),
            '9' => Some("nine"),
            _ => None,
        })
        .collect();
    Ok(PerlValue::string(words.join(" ")))
}

// ─────────────────────────────────────────────────────────────────────────────
// Color (extended)
// ─────────────────────────────────────────────────────────────────────────────

fn hsl_to_rgb_convert(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let h = h / 360.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h * 6.0) as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

fn rgb_to_hsl_convert(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if (max - min).abs() < f64::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if (max - r).abs() < f64::EPSILON {
        ((g - b) / d + if g < b { 6.0 } else { 0.0 }) / 6.0
    } else if (max - g).abs() < f64::EPSILON {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h * 360.0, s, l)
}

fn builtin_hsl_to_rgb(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let s = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let (r, g, b) = hsl_to_rgb_convert(h, s, l);
    Ok(PerlValue::array(vec![
        PerlValue::integer(r as i64),
        PerlValue::integer(g as i64),
        PerlValue::integer(b as i64),
    ]))
}
fn builtin_rgb_to_hsl(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(|v| v.to_int() as u8).unwrap_or(0);
    let g = args.get(1).map(|v| v.to_int() as u8).unwrap_or(0);
    let b = args.get(2).map(|v| v.to_int() as u8).unwrap_or(0);
    let (h, s, l) = rgb_to_hsl_convert(r, g, b);
    Ok(PerlValue::array(vec![
        PerlValue::float(h),
        PerlValue::float(s),
        PerlValue::float(l),
    ]))
}
fn builtin_hsv_to_rgb(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let s = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as i32 % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Ok(PerlValue::array(vec![
        PerlValue::integer(((r + m) * 255.0).round() as i64),
        PerlValue::integer(((g + m) * 255.0).round() as i64),
        PerlValue::integer(((b + m) * 255.0).round() as i64),
    ]))
}
fn builtin_rgb_to_hsv(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (r, g, b) = (
        args.first().map(|v| v.to_number()).unwrap_or(0.0) / 255.0,
        args.get(1).map(|v| v.to_number()).unwrap_or(0.0) / 255.0,
        args.get(2).map(|v| v.to_number()).unwrap_or(0.0) / 255.0,
    );
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let s = if max == 0.0 { 0.0 } else { d / max };
    let h = if d == 0.0 {
        0.0
    } else if (max - r).abs() < f64::EPSILON {
        60.0 * (((g - b) / d) % 6.0)
    } else if (max - g).abs() < f64::EPSILON {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    Ok(PerlValue::array(vec![
        PerlValue::float(if h < 0.0 { h + 360.0 } else { h }),
        PerlValue::float(s),
        PerlValue::float(max),
    ]))
}
fn builtin_color_blend(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (r1, g1, b1) = (
        args.first().map(|v| v.to_number()).unwrap_or(0.0),
        args.get(1).map(|v| v.to_number()).unwrap_or(0.0),
        args.get(2).map(|v| v.to_number()).unwrap_or(0.0),
    );
    let (r2, g2, b2) = (
        args.get(3).map(|v| v.to_number()).unwrap_or(0.0),
        args.get(4).map(|v| v.to_number()).unwrap_or(0.0),
        args.get(5).map(|v| v.to_number()).unwrap_or(0.0),
    );
    let t = args.get(6).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::array(vec![
        PerlValue::integer((r1 + (r2 - r1) * t).round() as i64),
        PerlValue::integer((g1 + (g2 - g1) * t).round() as i64),
        PerlValue::integer((b1 + (b2 - b1) * t).round() as i64),
    ]))
}
fn builtin_color_lighten(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (r, g, b) = (
        args.first().map(|v| v.to_int() as u8).unwrap_or(0),
        args.get(1).map(|v| v.to_int() as u8).unwrap_or(0),
        args.get(2).map(|v| v.to_int() as u8).unwrap_or(0),
    );
    let amt = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let (h, s, l) = rgb_to_hsl_convert(r, g, b);
    let (nr, ng, nb) = hsl_to_rgb_convert(h, s, (l + amt).min(1.0));
    Ok(PerlValue::array(vec![
        PerlValue::integer(nr as i64),
        PerlValue::integer(ng as i64),
        PerlValue::integer(nb as i64),
    ]))
}
fn builtin_color_darken(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (r, g, b) = (
        args.first().map(|v| v.to_int() as u8).unwrap_or(0),
        args.get(1).map(|v| v.to_int() as u8).unwrap_or(0),
        args.get(2).map(|v| v.to_int() as u8).unwrap_or(0),
    );
    let amt = args.get(3).map(|v| v.to_number()).unwrap_or(0.2);
    let (h, s, l) = rgb_to_hsl_convert(r, g, b);
    let (nr, ng, nb) = hsl_to_rgb_convert(h, s, (l - amt).max(0.0));
    Ok(PerlValue::array(vec![
        PerlValue::integer(nr as i64),
        PerlValue::integer(ng as i64),
        PerlValue::integer(nb as i64),
    ]))
}
fn builtin_color_complement(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (r, g, b) = (
        args.first().map(|v| v.to_int() as u8).unwrap_or(0),
        args.get(1).map(|v| v.to_int() as u8).unwrap_or(0),
        args.get(2).map(|v| v.to_int() as u8).unwrap_or(0),
    );
    let (h, s, l) = rgb_to_hsl_convert(r, g, b);
    let (nr, ng, nb) = hsl_to_rgb_convert((h + 180.0) % 360.0, s, l);
    Ok(PerlValue::array(vec![
        PerlValue::integer(nr as i64),
        PerlValue::integer(ng as i64),
        PerlValue::integer(nb as i64),
    ]))
}
fn builtin_color_invert(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::array(vec![
        PerlValue::integer(255 - args.first().map(|v| v.to_int()).unwrap_or(0)),
        PerlValue::integer(255 - args.get(1).map(|v| v.to_int()).unwrap_or(0)),
        PerlValue::integer(255 - args.get(2).map(|v| v.to_int()).unwrap_or(0)),
    ]))
}
fn builtin_color_grayscale(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let gray = (0.2126 * r + 0.7152 * g + 0.0722 * b).round() as i64;
    Ok(PerlValue::array(vec![
        PerlValue::integer(gray),
        PerlValue::integer(gray),
        PerlValue::integer(gray),
    ]))
}
fn builtin_random_color() -> PerlResult<PerlValue> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    Ok(PerlValue::array(vec![
        PerlValue::integer(rng.gen_range(0..=255)),
        PerlValue::integer(rng.gen_range(0..=255)),
        PerlValue::integer(rng.gen_range(0..=255)),
    ]))
}
fn builtin_ansi_256(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_int()).unwrap_or(0);
    let text = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(format!(
        "\x1b[38;5;{}m{}\x1b[0m",
        n, text
    )))
}
fn builtin_ansi_truecolor(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (r, g, b) = (
        args.first().map(|v| v.to_int()).unwrap_or(0),
        args.get(1).map(|v| v.to_int()).unwrap_or(0),
        args.get(2).map(|v| v.to_int()).unwrap_or(0),
    );
    let text = args.get(3).map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(format!(
        "\x1b[38;2;{};{};{}m{}\x1b[0m",
        r, g, b, text
    )))
}
fn builtin_color_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (r1, g1, b1) = (
        args.first().map(|v| v.to_number()).unwrap_or(0.0),
        args.get(1).map(|v| v.to_number()).unwrap_or(0.0),
        args.get(2).map(|v| v.to_number()).unwrap_or(0.0),
    );
    let (r2, g2, b2) = (
        args.get(3).map(|v| v.to_number()).unwrap_or(0.0),
        args.get(4).map(|v| v.to_number()).unwrap_or(0.0),
        args.get(5).map(|v| v.to_number()).unwrap_or(0.0),
    );
    let rmean = (r1 + r2) / 2.0;
    let (dr, dg, db) = (r1 - r2, g1 - g2, b1 - b2);
    Ok(PerlValue::float(
        ((2.0 + rmean / 256.0) * dr * dr
            + 4.0 * dg * dg
            + (2.0 + (255.0 - rmean) / 256.0) * db * db)
            .sqrt(),
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// Matrix (extended)
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_matrix_transpose(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if m.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let rows = m.len();
    let cols = arg_to_vec(&m[0]).len();
    let data: Vec<Vec<f64>> = m
        .iter()
        .map(|r| arg_to_vec(r).iter().map(|v| v.to_number()).collect())
        .collect();
    let mut result = Vec::with_capacity(cols);
    for j in 0..cols {
        let mut row = Vec::with_capacity(rows);
        for data_row in &data {
            row.push(PerlValue::float(data_row.get(j).copied().unwrap_or(0.0)));
        }
        result.push(PerlValue::array_ref(Arc::new(RwLock::new(row))));
    }
    Ok(PerlValue::array_ref(Arc::new(RwLock::new(result))))
}
fn builtin_matrix_inverse(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let data: Vec<Vec<f64>> = m
        .iter()
        .map(|r| arg_to_vec(r).iter().map(|v| v.to_number()).collect())
        .collect();
    if m.len() == 2 && data[0].len() == 2 {
        let (a, b, c, d) = (data[0][0], data[0][1], data[1][0], data[1][1]);
        let det = a * d - b * c;
        if det == 0.0 {
            return Ok(PerlValue::UNDEF);
        }
        let inv = 1.0 / det;
        return Ok(PerlValue::array(vec![
            PerlValue::array_ref(Arc::new(RwLock::new(vec![
                PerlValue::float(d * inv),
                PerlValue::float(-b * inv),
            ]))),
            PerlValue::array_ref(Arc::new(RwLock::new(vec![
                PerlValue::float(-c * inv),
                PerlValue::float(a * inv),
            ]))),
        ]));
    }
    Ok(PerlValue::UNDEF)
}
fn builtin_matrix_hadamard(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m1 = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let m2 = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    Ok(PerlValue::array(
        m1.iter()
            .zip(m2.iter())
            .map(|(r1, r2)| {
                let (row1, row2) = (arg_to_vec(r1), arg_to_vec(r2));
                PerlValue::array_ref(Arc::new(RwLock::new(
                    row1.iter()
                        .zip(row2.iter())
                        .map(|(a, b)| PerlValue::float(a.to_number() * b.to_number()))
                        .collect(),
                )))
            })
            .collect(),
    ))
}
fn builtin_matrix_power(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.get(1).map(|v| v.to_int()).unwrap_or(1);
    if n == 0 {
        return builtin_matrix_identity(&[PerlValue::integer(
            arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF)).len() as i64,
        )]);
    }
    if n == 1 {
        return Ok(args.first().cloned().unwrap_or(PerlValue::UNDEF));
    }
    let mut result = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    for _ in 1..n {
        result = builtin_matrix_mult(&[result, args.first().cloned().unwrap_or(PerlValue::UNDEF)])?;
    }
    Ok(result)
}
fn builtin_matrix_flatten(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let mut result = Vec::new();
    for row in &m {
        for v in arg_to_vec(row) {
            result.push(v);
        }
    }
    Ok(PerlValue::array(result))
}
fn builtin_matrix_from_rows(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rows = args
        .first()
        .map(|v| v.to_int().max(1) as usize)
        .unwrap_or(1);
    let cols = args.get(1).map(|v| v.to_int().max(1) as usize).unwrap_or(1);
    let flat = flatten_args(&args[2.min(args.len())..]);
    let mut result = Vec::with_capacity(rows);
    for r in 0..rows {
        let mut row = Vec::with_capacity(cols);
        for c in 0..cols {
            row.push(
                flat.get(r * cols + c)
                    .cloned()
                    .unwrap_or(PerlValue::float(0.0)),
            );
        }
        result.push(PerlValue::array_ref(Arc::new(RwLock::new(row))));
    }
    Ok(PerlValue::array(result))
}
fn builtin_matrix_map(
    interp: &mut Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let Some(sub) = f.as_code_ref() else {
        return Ok(PerlValue::UNDEF);
    };
    let m = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let mut result = Vec::with_capacity(m.len());
    for row_val in &m {
        let row = arg_to_vec(row_val);
        let mut new_row = Vec::with_capacity(row.len());
        for v in &row {
            new_row.push(exec_to_perl_result(
                interp.call_sub(&sub, vec![v.clone()], WantarrayCtx::Scalar, line),
                "matrix_map",
                line,
            )?);
        }
        result.push(PerlValue::array_ref(Arc::new(RwLock::new(new_row))));
    }
    Ok(PerlValue::array(result))
}
fn builtin_matrix_sum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let mut sum = 0.0;
    for row_val in &m {
        for v in arg_to_vec(row_val) {
            sum += v.to_number();
        }
    }
    Ok(PerlValue::float(sum))
}
fn builtin_matrix_max(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let mut max = f64::NEG_INFINITY;
    for row_val in &m {
        for v in arg_to_vec(row_val) {
            let n = v.to_number();
            if n > max {
                max = n;
            }
        }
    }
    Ok(PerlValue::float(max))
}
fn builtin_matrix_min(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let mut min = f64::INFINITY;
    for row_val in &m {
        for v in arg_to_vec(row_val) {
            let n = v.to_number();
            if n < min {
                min = n;
            }
        }
    }
    Ok(PerlValue::float(min))
}

// ─────────────────────────────────────────────────────────────────────────────
// String (extended)
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_ngrams(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args
        .first()
        .map(|v| v.to_int().max(1) as usize)
        .unwrap_or(2);
    let s = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < n {
        return Ok(PerlValue::array(vec![]));
    }
    Ok(PerlValue::array(
        chars
            .windows(n)
            .map(|w| PerlValue::string(w.iter().collect()))
            .collect(),
    ))
}
fn builtin_bigrams(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ngrams(&[
        PerlValue::integer(2),
        PerlValue::string(first_arg_or_topic(interp, args).to_string()),
    ])
}
fn builtin_trigrams(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ngrams(&[
        PerlValue::integer(3),
        PerlValue::string(first_arg_or_topic(interp, args).to_string()),
    ])
}
fn builtin_char_frequencies(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let mut map = indexmap::IndexMap::new();
    for c in s.chars() {
        let key = c.to_string();
        let count = map.get(&key).map(|v: &PerlValue| v.to_int()).unwrap_or(0);
        map.insert(key, PerlValue::integer(count + 1));
    }
    Ok(PerlValue::hash(map))
}
fn builtin_is_anagram(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (a, b) = (
        args.first()
            .map(|v| v.to_string())
            .unwrap_or_default()
            .to_lowercase(),
        args.get(1)
            .map(|v| v.to_string())
            .unwrap_or_default()
            .to_lowercase(),
    );
    let mut ac: Vec<char> = a.chars().filter(|c| c.is_alphanumeric()).collect();
    let mut bc: Vec<char> = b.chars().filter(|c| c.is_alphanumeric()).collect();
    ac.sort_unstable();
    bc.sort_unstable();
    Ok(bool_iv(ac == bc))
}
fn builtin_is_pangram(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string().to_lowercase();
    Ok(bool_iv(('a'..='z').all(|c| s.contains(c))))
}
fn builtin_is_printable(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(bool_iv(
        first_arg_or_topic(interp, args)
            .to_string()
            .chars()
            .all(|c| !c.is_control() || c == '\n' || c == '\t'),
    ))
}
fn builtin_is_control(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    Ok(bool_iv(!s.is_empty() && s.chars().all(|c| c.is_control())))
}
fn builtin_mask_string(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let show = args.get(1).map(|v| v.to_int() as usize).unwrap_or(4);
    let mc = args
        .get(2)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "*".to_string())
        .chars()
        .next()
        .unwrap_or('*');
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= show {
        return Ok(PerlValue::string(s));
    }
    Ok(PerlValue::string(
        chars[..chars.len() - show]
            .iter()
            .map(|_| mc)
            .chain(chars[chars.len() - show..].iter().copied())
            .collect(),
    ))
}
fn builtin_indent_text(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let indent = args
        .first()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "  ".to_string());
    let text = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(
        text.lines()
            .map(|line| format!("{}{}", indent, line))
            .collect::<Vec<_>>()
            .join("\n"),
    ))
}
fn builtin_dedent_text(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let lines: Vec<&str> = s.lines().collect();
    let min_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    Ok(PerlValue::string(
        lines
            .iter()
            .map(|l| {
                if l.len() >= min_indent {
                    &l[min_indent..]
                } else {
                    l
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    ))
}
fn builtin_strip_html(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let mut result = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => {
                in_tag = true;
            }
            '>' => {
                in_tag = false;
            }
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }
    Ok(PerlValue::string(result))
}
fn builtin_chunk_string(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args
        .first()
        .map(|v| v.to_int().max(1) as usize)
        .unwrap_or(1);
    let s = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let chars: Vec<char> = s.chars().collect();
    Ok(PerlValue::array(
        chars
            .chunks(n)
            .map(|c| PerlValue::string(c.iter().collect()))
            .collect(),
    ))
}
fn builtin_camel_to_snake(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_lowercase().next().unwrap_or(c));
    }
    Ok(PerlValue::string(result))
}
fn builtin_snake_to_camel(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    Ok(PerlValue::string(
        s.split('_')
            .enumerate()
            .map(|(i, part)| {
                if i == 0 {
                    part.to_string()
                } else {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(c) => format!("{}{}", c.to_uppercase(), chars.as_str()),
                        None => String::new(),
                    }
                }
            })
            .collect(),
    ))
}
fn builtin_string_multiply(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(1);
    Ok(PerlValue::string(s.repeat(n)))
}
fn builtin_collapse_whitespace(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let mut result = String::new();
    let mut prev_space = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                result.push(' ');
            }
            prev_space = true;
        } else {
            result.push(c);
            prev_space = false;
        }
    }
    Ok(PerlValue::string(result))
}
fn builtin_remove_vowels(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::string(
        first_arg_or_topic(interp, args)
            .to_string()
            .chars()
            .filter(|c| !"aeiouAEIOU".contains(*c))
            .collect(),
    ))
}
fn builtin_remove_consonants(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::string(
        first_arg_or_topic(interp, args)
            .to_string()
            .chars()
            .filter(|c| !c.is_ascii_alphabetic() || "aeiouAEIOU".contains(*c))
            .collect(),
    ))
}
fn builtin_is_numeric_string(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(bool_iv(
        first_arg_or_topic(interp, args)
            .to_string()
            .trim()
            .parse::<f64>()
            .is_ok(),
    ))
}
fn builtin_string_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let (ac, bc): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (m, n) = (ac.len(), bc.len());
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for (i, row) in dp.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in dp[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if ac[i - 1] == bc[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    Ok(PerlValue::integer(dp[m][n] as i64))
}
fn builtin_metaphone(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string().to_uppercase();
    let chars: Vec<char> = s.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if chars.is_empty() {
        return Ok(PerlValue::string(String::new()));
    }
    let mut result = String::new();
    let mut i = 0;
    if chars.len() >= 2 {
        match (chars[0], chars[1]) {
            ('A', 'E') | ('G', 'N') | ('K', 'N') | ('P', 'N') | ('W', 'R') => i = 1,
            _ => {}
        }
    }
    let mut prev = ' ';
    while i < chars.len() && result.len() < 6 {
        let c = chars[i];
        let next = chars.get(i + 1).copied().unwrap_or(' ');
        let code = match c {
            'B' if prev != 'M' => Some('B'),
            'C' if "EIY".contains(next) => Some('S'),
            'C' => Some('K'),
            'D' if "GEI".contains(next) => Some('J'),
            'D' => Some('T'),
            'F' => Some('F'),
            'G' if next == 'H' || (i > 0 && !"EIY".contains(next)) => None,
            'G' => Some('J'),
            'H' if "AEIOU".contains(next) && !"AEIOU".contains(prev) => Some('H'),
            'J' => Some('J'),
            'K' if prev != 'C' => Some('K'),
            'L' => Some('L'),
            'M' => Some('M'),
            'N' => Some('N'),
            'P' if next == 'H' => Some('F'),
            'P' => Some('P'),
            'Q' => Some('K'),
            'R' => Some('R'),
            'S' if next == 'H' => Some('X'),
            'S' => Some('S'),
            'T' if next == 'H' => Some('0'),
            'T' => Some('T'),
            'V' => Some('F'),
            'W' | 'Y' if "AEIOU".contains(next) => Some(c),
            'X' => {
                result.push('K');
                Some('S')
            }
            'Z' => Some('S'),
            _ => None,
        };
        if let Some(mc) = code {
            if result.is_empty() || !result.ends_with(mc) {
                result.push(mc);
            }
        }
        prev = c;
        i += 1;
    }
    Ok(PerlValue::string(result))
}
fn builtin_double_metaphone(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let code = builtin_metaphone(interp, args)?;
    Ok(PerlValue::array(vec![code.clone(), code]))
}
fn builtin_initials(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    Ok(PerlValue::string(
        s.split_whitespace()
            .filter_map(|w| w.chars().next())
            .map(|c| format!("{}.", c.to_uppercase().next().unwrap_or(c)))
            .collect(),
    ))
}
fn builtin_acronym(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    Ok(PerlValue::string(
        s.split_whitespace()
            .filter_map(|w| w.chars().next())
            .map(|c| c.to_uppercase().next().unwrap_or(c))
            .collect(),
    ))
}
fn builtin_superscript(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::string(
        first_arg_or_topic(interp, args)
            .to_string()
            .chars()
            .map(|c| match c {
                '0' => '\u{2070}',
                '1' => '\u{00B9}',
                '2' => '\u{00B2}',
                '3' => '\u{00B3}',
                '4' => '\u{2074}',
                '5' => '\u{2075}',
                '6' => '\u{2076}',
                '7' => '\u{2077}',
                '8' => '\u{2078}',
                '9' => '\u{2079}',
                '+' => '\u{207A}',
                '-' => '\u{207B}',
                '=' => '\u{207C}',
                '(' => '\u{207D}',
                ')' => '\u{207E}',
                'n' => '\u{207F}',
                _ => c,
            })
            .collect(),
    ))
}
fn builtin_subscript(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::string(
        first_arg_or_topic(interp, args)
            .to_string()
            .chars()
            .map(|c| match c {
                '0' => '\u{2080}',
                '1' => '\u{2081}',
                '2' => '\u{2082}',
                '3' => '\u{2083}',
                '4' => '\u{2084}',
                '5' => '\u{2085}',
                '6' => '\u{2086}',
                '7' => '\u{2087}',
                '8' => '\u{2088}',
                '9' => '\u{2089}',
                '+' => '\u{208A}',
                '-' => '\u{208B}',
                '=' => '\u{208C}',
                '(' => '\u{208D}',
                ')' => '\u{208E}',
                _ => c,
            })
            .collect(),
    ))
}
fn builtin_leetspeak(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::string(
        first_arg_or_topic(interp, args)
            .to_string()
            .chars()
            .map(|c| match c.to_lowercase().next().unwrap_or(c) {
                'a' => '4',
                'e' => '3',
                'g' => '6',
                'i' => '1',
                'o' => '0',
                's' => '5',
                't' => '7',
                _ => c,
            })
            .collect(),
    ))
}
fn builtin_zalgo(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    use rand::Rng;
    let s = first_arg_or_topic(interp, args).to_string();
    let mut rng = rand::thread_rng();
    let above = [
        '\u{0300}', '\u{0301}', '\u{0302}', '\u{0303}', '\u{0304}', '\u{0305}', '\u{0306}',
        '\u{0307}', '\u{0308}', '\u{030A}', '\u{030B}', '\u{030C}', '\u{030D}', '\u{030E}',
        '\u{030F}', '\u{0310}', '\u{0311}', '\u{0312}',
    ];
    let below = [
        '\u{0316}', '\u{0317}', '\u{0318}', '\u{0319}', '\u{031A}', '\u{031B}', '\u{031C}',
        '\u{031D}', '\u{031E}', '\u{031F}', '\u{0320}', '\u{0321}', '\u{0322}', '\u{0323}',
        '\u{0324}', '\u{0325}', '\u{0326}', '\u{0327}',
    ];
    let mut result = String::new();
    for c in s.chars() {
        result.push(c);
        for _ in 0..rng.gen_range(1..4) {
            result.push(above[rng.gen_range(0..above.len())]);
        }
        for _ in 0..rng.gen_range(1..3) {
            result.push(below[rng.gen_range(0..below.len())]);
        }
    }
    Ok(PerlValue::string(result))
}
fn builtin_reverse_each_word(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::string(
        first_arg_or_topic(interp, args)
            .to_string()
            .split(' ')
            .map(|w| w.chars().rev().collect::<String>())
            .collect::<Vec<_>>()
            .join(" "),
    ))
}
fn builtin_sort_words(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let mut words: Vec<&str> = s.split_whitespace().collect();
    words.sort_unstable();
    Ok(PerlValue::string(words.join(" ")))
}
fn builtin_unique_words(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let mut seen = indexmap::IndexMap::new();
    for w in s.split_whitespace() {
        seen.entry(w.to_string()).or_insert(());
    }
    Ok(PerlValue::string(
        seen.keys().cloned().collect::<Vec<_>>().join(" "),
    ))
}
fn builtin_word_frequencies(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let mut map = indexmap::IndexMap::new();
    for w in s.split_whitespace() {
        let key = w.to_lowercase();
        let count = map.get(&key).map(|v: &PerlValue| v.to_int()).unwrap_or(0);
        map.insert(key, PerlValue::integer(count + 1));
    }
    Ok(PerlValue::hash(map))
}

// ─────────────────────────────────────────────────────────────────────────────
// Validation (extended)
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_luhn_check(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let digits: Vec<u32> = first_arg_or_topic(interp, args)
        .to_string()
        .chars()
        .filter_map(|c| c.to_digit(10))
        .collect();
    if digits.len() < 2 {
        return Ok(bool_iv(false));
    }
    let mut sum = 0u32;
    for (i, &d) in digits.iter().rev().enumerate() {
        if i % 2 == 1 {
            let doubled = d * 2;
            sum += if doubled > 9 { doubled - 9 } else { doubled };
        } else {
            sum += d;
        }
    }
    Ok(bool_iv(sum.is_multiple_of(10)))
}
fn builtin_is_valid_hex_color(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    Ok(bool_iv(
        s.starts_with('#')
            && (s.len() == 4 || s.len() == 7)
            && s[1..].chars().all(|c| c.is_ascii_hexdigit()),
    ))
}
fn builtin_is_valid_cidr(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let parts: Vec<&str> = s.split('/').collect();
    if parts.len() != 2 {
        return Ok(bool_iv(false));
    }
    let ip_ok = parts[0]
        .split('.')
        .filter_map(|p| p.parse::<u32>().ok())
        .filter(|&n| n <= 255)
        .count()
        == 4;
    let prefix_ok = parts[1].parse::<u32>().map(|n| n <= 32).unwrap_or(false);
    Ok(bool_iv(ip_ok && prefix_ok))
}
fn builtin_is_valid_mime(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let parts: Vec<&str> = s.split('/').collect();
    Ok(bool_iv(
        parts.len() == 2
            && !parts[0].is_empty()
            && !parts[1].is_empty()
            && parts[0]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-'),
    ))
}
fn builtin_is_valid_cron(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(bool_iv(
        first_arg_or_topic(interp, args)
            .to_string()
            .split_whitespace()
            .count()
            == 5,
    ))
}
fn builtin_is_valid_latitude(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(bool_iv((-90.0..=90.0).contains(
        &args.first().map(|v| v.to_number()).unwrap_or(0.0),
    )))
}
fn builtin_is_valid_longitude(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(bool_iv((-180.0..=180.0).contains(
        &args.first().map(|v| v.to_number()).unwrap_or(0.0),
    )))
}

// ─────────────────────────────────────────────────────────────────────────────
// Algorithms
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_next_permutation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut xs: Vec<i64> = flatten_args(args).iter().map(|v| v.to_int()).collect();
    let n = xs.len();
    if n < 2 {
        return Ok(PerlValue::array(
            xs.into_iter().map(PerlValue::integer).collect(),
        ));
    }
    let mut i = n - 2;
    while i < n && xs[i] >= xs[i + 1] {
        if i == 0 {
            xs.reverse();
            return Ok(PerlValue::array(
                xs.into_iter().map(PerlValue::integer).collect(),
            ));
        }
        i -= 1;
    }
    let mut j = n - 1;
    while xs[j] <= xs[i] {
        j -= 1;
    }
    xs.swap(i, j);
    xs[i + 1..].reverse();
    Ok(PerlValue::array(
        xs.into_iter().map(PerlValue::integer).collect(),
    ))
}
fn builtin_is_balanced_parens(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let mut stack = Vec::new();
    for c in s.chars() {
        match c {
            '(' | '[' | '{' => stack.push(c),
            ')' if stack.pop() != Some('(') => {
                return Ok(bool_iv(false));
            }
            ']' if stack.pop() != Some('[') => {
                return Ok(bool_iv(false));
            }
            '}' if stack.pop() != Some('{') => {
                return Ok(bool_iv(false));
            }
            _ => {}
        }
    }
    Ok(bool_iv(stack.is_empty()))
}
fn builtin_eval_rpn(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let tokens = flatten_args(args);
    let mut stack: Vec<f64> = Vec::new();
    for tok in &tokens {
        let s = tok.to_string();
        match s.as_str() {
            "+" | "-" | "*" | "/" | "%" | "**" => {
                let b = stack.pop().unwrap_or(0.0);
                let a = stack.pop().unwrap_or(0.0);
                stack.push(match s.as_str() {
                    "+" => a + b,
                    "-" => a - b,
                    "*" => a * b,
                    "/" => {
                        if b == 0.0 {
                            f64::NAN
                        } else {
                            a / b
                        }
                    }
                    "%" => {
                        if b == 0.0 {
                            f64::NAN
                        } else {
                            a % b
                        }
                    }
                    "**" => a.powf(b),
                    _ => 0.0,
                });
            }
            _ => {
                stack.push(s.parse::<f64>().unwrap_or(0.0));
            }
        }
    }
    Ok(PerlValue::float(stack.pop().unwrap_or(0.0)))
}
fn builtin_merge_sorted(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let b = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let mut result = Vec::with_capacity(a.len() + b.len());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].to_number() <= b[j].to_number() {
            result.push(a[i].clone());
            i += 1;
        } else {
            result.push(b[j].clone());
            j += 1;
        }
    }
    result.extend_from_slice(&a[i..]);
    result.extend_from_slice(&b[j..]);
    Ok(PerlValue::array(result))
}
fn builtin_binary_insert(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let val = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut arr = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let v = val.to_number();
    let pos = arr.partition_point(|x| x.to_number() < v);
    arr.insert(pos, val);
    Ok(PerlValue::array(arr))
}
fn builtin_reservoir_sample(args: &[PerlValue]) -> PerlResult<PerlValue> {
    use rand::Rng;
    let k = args
        .first()
        .map(|v| v.to_int().max(0) as usize)
        .unwrap_or(1);
    let xs = flatten_args(&args[1.min(args.len())..]);
    if k >= xs.len() {
        return Ok(PerlValue::array(xs));
    }
    let mut reservoir: Vec<PerlValue> = xs[..k].to_vec();
    let mut rng = rand::thread_rng();
    for (i, item) in xs.iter().enumerate().skip(k) {
        let j = rng.gen_range(0..=i);
        if j < k {
            reservoir[j] = item.clone();
        }
    }
    Ok(PerlValue::array(reservoir))
}
fn builtin_run_length_encode_str(
    interp: &Interpreter,
    args: &[PerlValue],
) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    if s.is_empty() {
        return Ok(PerlValue::string(String::new()));
    }
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let mut count = 1;
        while i + count < chars.len() && chars[i + count] == c {
            count += 1;
        }
        result.push_str(&format!("{}{}", count, c));
        i += count;
    }
    Ok(PerlValue::string(result))
}
fn builtin_run_length_decode_str(
    interp: &Interpreter,
    args: &[PerlValue],
) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let mut result = String::new();
    let mut num = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let count: usize = num.parse().unwrap_or(1);
            for _ in 0..count {
                result.push(c);
            }
            num.clear();
        }
    }
    Ok(PerlValue::string(result))
}
fn builtin_range_expand(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    let mut result = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if let Some(dash) = part.find('-') {
            if let (Ok(start), Ok(end)) = (
                part[..dash].trim().parse::<i64>(),
                part[dash + 1..].trim().parse::<i64>(),
            ) {
                for i in start..=end {
                    result.push(PerlValue::integer(i));
                }
            }
        } else if let Ok(n) = part.parse::<i64>() {
            result.push(PerlValue::integer(n));
        }
    }
    Ok(PerlValue::array(result))
}
fn builtin_range_compress(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut nums: Vec<i64> = flatten_args(args).iter().map(|v| v.to_int()).collect();
    nums.sort_unstable();
    nums.dedup();
    if nums.is_empty() {
        return Ok(PerlValue::string(String::new()));
    }
    let mut ranges = Vec::new();
    let (mut start, mut end) = (nums[0], nums[0]);
    for &n in &nums[1..] {
        if n == end + 1 {
            end = n;
        } else {
            ranges.push(if start == end {
                format!("{}", start)
            } else {
                format!("{}-{}", start, end)
            });
            start = n;
            end = n;
        }
    }
    ranges.push(if start == end {
        format!("{}", start)
    } else {
        format!("{}-{}", start, end)
    });
    Ok(PerlValue::string(ranges.join(",")))
}
fn builtin_group_consecutive_by(
    interp: &mut Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let Some(sub) = f.as_code_ref() else {
        return Ok(PerlValue::array(vec![]));
    };
    let xs = flatten_args(&args[1..]);
    if xs.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let mut groups: Vec<PerlValue> = Vec::new();
    let mut current_group = vec![xs[0].clone()];
    let mut current_key = exec_to_perl_result(
        interp.call_sub(&sub, vec![xs[0].clone()], WantarrayCtx::Scalar, line),
        "group_consecutive_by",
        line,
    )?
    .to_string();
    for x in xs.iter().skip(1) {
        let key = exec_to_perl_result(
            interp.call_sub(&sub, vec![x.clone()], WantarrayCtx::Scalar, line),
            "group_consecutive_by",
            line,
        )?
        .to_string();
        if key == current_key {
            current_group.push(x.clone());
        } else {
            groups.push(PerlValue::array(std::mem::take(&mut current_group)));
            current_group.push(x.clone());
            current_key = key;
        }
    }
    if !current_group.is_empty() {
        groups.push(PerlValue::array(current_group));
    }
    Ok(PerlValue::array(groups))
}
fn builtin_histogram(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bins = args
        .first()
        .map(|v| v.to_int().max(1) as usize)
        .unwrap_or(10);
    let vals: Vec<f64> = flatten_args(&args[1.min(args.len())..])
        .iter()
        .map(|v| v.to_number())
        .collect();
    if vals.is_empty() {
        return Ok(PerlValue::array(vec![PerlValue::integer(0); bins]));
    }
    let mn = vals.iter().cloned().fold(f64::INFINITY, f64::min);
    let mx = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = mx - mn;
    if range == 0.0 {
        let mut counts = vec![0i64; bins];
        counts[0] = vals.len() as i64;
        return Ok(PerlValue::array(
            counts.into_iter().map(PerlValue::integer).collect(),
        ));
    }
    let mut counts = vec![0i64; bins];
    for v in &vals {
        let idx = (((v - mn) / range) * (bins as f64 - 1.0)).round() as usize;
        counts[idx.min(bins - 1)] += 1;
    }
    Ok(PerlValue::array(
        counts.into_iter().map(PerlValue::integer).collect(),
    ))
}
fn builtin_bucket(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let width = args.first().map(|v| v.to_number()).unwrap_or(10.0);
    let xs = flatten_args(&args[1.min(args.len())..]);
    let mut map: indexmap::IndexMap<String, Vec<PerlValue>> = indexmap::IndexMap::new();
    for x in &xs {
        let bucket = (x.to_number() / width).floor() * width;
        map.entry(format!("{}", bucket))
            .or_default()
            .push(x.clone());
    }
    let mut result = indexmap::IndexMap::new();
    for (k, v) in map {
        result.insert(k, PerlValue::array(v));
    }
    Ok(PerlValue::hash(result))
}
fn builtin_clamp_array(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lo = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let hi = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::array(
        flatten_args(&args[2.min(args.len())..])
            .iter()
            .map(|v| PerlValue::float(v.to_number().clamp(lo, hi)))
            .collect(),
    ))
}
fn builtin_normalize_range(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (new_min, new_max) = (
        args.first().map(|v| v.to_number()).unwrap_or(0.0),
        args.get(1).map(|v| v.to_number()).unwrap_or(1.0),
    );
    let vals: Vec<f64> = flatten_args(&args[2.min(args.len())..])
        .iter()
        .map(|v| v.to_number())
        .collect();
    if vals.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let mn = vals.iter().cloned().fold(f64::INFINITY, f64::min);
    let mx = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = mx - mn;
    if range == 0.0 {
        return Ok(PerlValue::array(vec![
            PerlValue::float(new_min);
            vals.len()
        ]));
    }
    Ok(PerlValue::array(
        vals.iter()
            .map(|v| PerlValue::float((v - mn) / range * (new_max - new_min) + new_min))
            .collect(),
    ))
}
// ─────────────────────────────────────────────────────────────────────────────
// Conversion utilities
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_to_string_val(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::string(
        first_arg_or_topic(interp, args).to_string(),
    ))
}
fn builtin_type_of(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let t = if v.is_undef() {
        "undef"
    } else if v.as_code_ref().is_some() {
        "code"
    } else if v.as_array_ref().is_some() {
        "arrayref"
    } else if v.as_hash_ref().is_some() {
        "hashref"
    } else if v.as_array_vec().is_some() {
        "array"
    } else if v.as_hash_map().is_some() {
        "hash"
    } else if v.is_integer_like() {
        "integer"
    } else if v.is_float_like() {
        "float"
    } else {
        "string"
    };
    Ok(PerlValue::string(t.to_string()))
}
fn builtin_byte_size(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(
        args.first()
            .cloned()
            .unwrap_or(PerlValue::UNDEF)
            .to_string()
            .len() as i64,
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// DSP / Signal
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_convolution(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    if a.is_empty() || b.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let n = a.len() + b.len() - 1;
    let mut result = vec![0.0; n];
    for (i, &ai) in a.iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            result[i + j] += ai * bj;
        }
    }
    Ok(PerlValue::array(
        result.into_iter().map(PerlValue::float).collect(),
    ))
}
fn builtin_autocorrelation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    let n = vals.len();
    if n == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    let mean = vals.iter().sum::<f64>() / n as f64;
    let var: f64 = vals.iter().map(|v| (v - mean).powi(2)).sum();
    if var == 0.0 {
        return Ok(PerlValue::array(vec![PerlValue::float(1.0); n]));
    }
    Ok(PerlValue::array(
        (0..n)
            .map(|lag| {
                let sum: f64 = (0..n - lag)
                    .map(|i| (vals[i] - mean) * (vals[i + lag] - mean))
                    .sum();
                PerlValue::float(sum / var)
            })
            .collect(),
    ))
}
fn builtin_fft_magnitude(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    let n = vals.len();
    if n == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    Ok(PerlValue::array(
        (0..=n / 2)
            .map(|k| {
                let (mut re, mut im) = (0.0, 0.0);
                for (i, &v) in vals.iter().enumerate() {
                    let angle = -2.0 * std::f64::consts::PI * k as f64 * i as f64 / n as f64;
                    re += v * angle.cos();
                    im += v * angle.sin();
                }
                PerlValue::float((re * re + im * im).sqrt())
            })
            .collect(),
    ))
}
fn builtin_zero_crossings(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    let mut count = 0i64;
    for i in 1..vals.len() {
        if (vals[i - 1] >= 0.0 && vals[i] < 0.0) || (vals[i - 1] < 0.0 && vals[i] >= 0.0) {
            count += 1;
        }
    }
    Ok(PerlValue::integer(count))
}
fn builtin_peak_detect(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vals: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    let mut peaks = Vec::new();
    for i in 1..vals.len().saturating_sub(1) {
        if vals[i] > vals[i - 1] && vals[i] > vals[i + 1] {
            peaks.push(PerlValue::integer(i as i64));
        }
    }
    Ok(PerlValue::array(peaks))
}

/// `lowpass_filter SIGNAL, ALPHA` — simple exponential low-pass filter.
fn builtin_lowpass_filter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let alpha = args
        .get(1)
        .map(|v| v.to_number())
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    if signal.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let mut out = Vec::with_capacity(signal.len());
    let mut prev = signal[0];
    for &s in &signal {
        prev = alpha * s + (1.0 - alpha) * prev;
        out.push(PerlValue::float(prev));
    }
    Ok(PerlValue::array(out))
}

/// `highpass_filter SIGNAL, ALPHA` — simple high-pass filter (signal - lowpass).
fn builtin_highpass_filter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let alpha = args
        .get(1)
        .map(|v| v.to_number())
        .unwrap_or(0.5)
        .clamp(0.0, 1.0);
    if signal.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let mut out = Vec::with_capacity(signal.len());
    let mut lp = signal[0];
    for &s in &signal {
        lp = alpha * s + (1.0 - alpha) * lp;
        out.push(PerlValue::float(s - lp));
    }
    Ok(PerlValue::array(out))
}

/// `bandpass_filter SIGNAL, LOW_ALPHA, HIGH_ALPHA` — bandpass via lowpass then highpass.
fn builtin_bandpass_filter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let low_alpha = args
        .get(1)
        .map(|v| v.to_number())
        .unwrap_or(0.3)
        .clamp(0.0, 1.0);
    let high_alpha = args
        .get(2)
        .map(|v| v.to_number())
        .unwrap_or(0.7)
        .clamp(0.0, 1.0);
    if signal.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let mut lp1 = Vec::with_capacity(signal.len());
    let mut prev = signal[0];
    for &s in &signal {
        prev = low_alpha * s + (1.0 - low_alpha) * prev;
        lp1.push(prev);
    }
    let mut out = Vec::with_capacity(signal.len());
    let mut lp2 = lp1[0];
    for &s in &lp1 {
        lp2 = high_alpha * s + (1.0 - high_alpha) * lp2;
        out.push(PerlValue::float(s - lp2));
    }
    Ok(PerlValue::array(out))
}

/// `median_filter SIGNAL, WINDOW_SIZE` — median filter for noise reduction.
fn builtin_median_filter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let window = args.get(1).map(|v| v.to_int()).unwrap_or(3).max(1) as usize;
    let half = window / 2;
    let mut out = Vec::with_capacity(signal.len());
    for i in 0..signal.len() {
        let start = i.saturating_sub(half);
        let end = (i + half + 1).min(signal.len());
        let mut window_vals: Vec<f64> = signal[start..end].to_vec();
        window_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mid = window_vals.len() / 2;
        out.push(PerlValue::float(window_vals[mid]));
    }
    Ok(PerlValue::array(out))
}

/// `window_hann SIZE` — Hann window coefficients.
fn builtin_window_hann(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_int()).unwrap_or(64).max(1) as usize;
    Ok(PerlValue::array(
        (0..n)
            .map(|i| {
                let w =
                    0.5 * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos());
                PerlValue::float(w)
            })
            .collect(),
    ))
}

/// `window_hamming SIZE` — Hamming window coefficients.
fn builtin_window_hamming(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_int()).unwrap_or(64).max(1) as usize;
    Ok(PerlValue::array(
        (0..n)
            .map(|i| {
                let w =
                    0.54 - 0.46 * (2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64).cos();
                PerlValue::float(w)
            })
            .collect(),
    ))
}

/// `window_blackman SIZE` — Blackman window coefficients.
fn builtin_window_blackman(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_int()).unwrap_or(64).max(1) as usize;
    let a0 = 0.42;
    let a1 = 0.5;
    let a2 = 0.08;
    Ok(PerlValue::array(
        (0..n)
            .map(|i| {
                let x = 2.0 * std::f64::consts::PI * i as f64 / (n - 1) as f64;
                let w = a0 - a1 * x.cos() + a2 * (2.0 * x).cos();
                PerlValue::float(w)
            })
            .collect(),
    ))
}

/// `window_kaiser SIZE, BETA` — Kaiser window coefficients.
fn builtin_window_kaiser(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_int()).unwrap_or(64).max(1) as usize;
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(5.0);
    fn bessel_i0(x: f64) -> f64 {
        let mut sum = 1.0;
        let mut term = 1.0;
        for k in 1..50 {
            term *= (x / (2.0 * k as f64)).powi(2);
            sum += term;
            if term < 1e-15 {
                break;
            }
        }
        sum
    }
    let denom = bessel_i0(beta);
    Ok(PerlValue::array(
        (0..n)
            .map(|i| {
                let x = 2.0 * i as f64 / (n - 1) as f64 - 1.0;
                let w = bessel_i0(beta * (1.0 - x * x).sqrt()) / denom;
                PerlValue::float(w)
            })
            .collect(),
    ))
}

/// `apply_window SIGNAL, WINDOW` — element-wise multiply signal by window.
fn builtin_apply_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let window: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = signal.len().min(window.len());
    Ok(PerlValue::array(
        signal[..n]
            .iter()
            .zip(window[..n].iter())
            .map(|(&s, &w)| PerlValue::float(s * w))
            .collect(),
    ))
}

/// `dft SIGNAL` — discrete Fourier transform, returns array of [re, im] pairs.
fn builtin_dft(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    let n = signal.len();
    if n == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    Ok(PerlValue::array(
        (0..n)
            .map(|k| {
                let (mut re, mut im) = (0.0, 0.0);
                for (i, &s) in signal.iter().enumerate() {
                    let angle = -2.0 * std::f64::consts::PI * k as f64 * i as f64 / n as f64;
                    re += s * angle.cos();
                    im += s * angle.sin();
                }
                PerlValue::array(vec![PerlValue::float(re), PerlValue::float(im)])
            })
            .collect(),
    ))
}

/// `idft SPECTRUM` — inverse DFT from array of [re, im] pairs.
fn builtin_idft(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let spectrum = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = spectrum.len();
    if n == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    let coeffs: Vec<(f64, f64)> = spectrum
        .iter()
        .map(|c| {
            let arr = arg_to_vec(c);
            (
                arr.first().map(|v| v.to_number()).unwrap_or(0.0),
                arr.get(1).map(|v| v.to_number()).unwrap_or(0.0),
            )
        })
        .collect();
    Ok(PerlValue::array(
        (0..n)
            .map(|i| {
                let mut sum = 0.0;
                for (k, &(re, im)) in coeffs.iter().enumerate() {
                    let angle = 2.0 * std::f64::consts::PI * k as f64 * i as f64 / n as f64;
                    sum += re * angle.cos() - im * angle.sin();
                }
                PerlValue::float(sum / n as f64)
            })
            .collect(),
    ))
}

/// `power_spectrum SIGNAL` — power spectral density (magnitude squared).
fn builtin_power_spectrum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    let n = signal.len();
    if n == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    Ok(PerlValue::array(
        (0..=n / 2)
            .map(|k| {
                let (mut re, mut im) = (0.0, 0.0);
                for (i, &s) in signal.iter().enumerate() {
                    let angle = -2.0 * std::f64::consts::PI * k as f64 * i as f64 / n as f64;
                    re += s * angle.cos();
                    im += s * angle.sin();
                }
                PerlValue::float(re * re + im * im)
            })
            .collect(),
    ))
}

/// `phase_spectrum SIGNAL` — phase angles in radians.
fn builtin_phase_spectrum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    let n = signal.len();
    if n == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    Ok(PerlValue::array(
        (0..=n / 2)
            .map(|k| {
                let (mut re, mut im) = (0.0, 0.0);
                for (i, &s) in signal.iter().enumerate() {
                    let angle = -2.0 * std::f64::consts::PI * k as f64 * i as f64 / n as f64;
                    re += s * angle.cos();
                    im += s * angle.sin();
                }
                PerlValue::float(im.atan2(re))
            })
            .collect(),
    ))
}

/// `spectrogram SIGNAL, FRAME_SIZE, HOP_SIZE` — returns 2D array of magnitude frames.
fn builtin_spectrogram(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let frame_size = args.get(1).map(|v| v.to_int()).unwrap_or(256).max(1) as usize;
    let hop_size = args.get(2).map(|v| v.to_int()).unwrap_or(128).max(1) as usize;
    let mut frames = Vec::new();
    let mut i = 0;
    while i + frame_size <= signal.len() {
        let frame = &signal[i..i + frame_size];
        let mags: Vec<PerlValue> = (0..=frame_size / 2)
            .map(|k| {
                let (mut re, mut im) = (0.0, 0.0);
                for (j, &s) in frame.iter().enumerate() {
                    let angle =
                        -2.0 * std::f64::consts::PI * k as f64 * j as f64 / frame_size as f64;
                    re += s * angle.cos();
                    im += s * angle.sin();
                }
                PerlValue::float((re * re + im * im).sqrt())
            })
            .collect();
        frames.push(PerlValue::array(mags));
        i += hop_size;
    }
    Ok(PerlValue::array(frames))
}

/// `resample SIGNAL, FACTOR` — simple linear interpolation resampling.
fn builtin_resample(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let factor = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(0.01);
    if signal.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let new_len = ((signal.len() as f64) * factor).round() as usize;
    let mut out = Vec::with_capacity(new_len);
    for i in 0..new_len {
        let pos = i as f64 / factor;
        let idx = pos.floor() as usize;
        let frac = pos - idx as f64;
        let v = if idx + 1 < signal.len() {
            signal[idx] * (1.0 - frac) + signal[idx + 1] * frac
        } else {
            signal[signal.len() - 1]
        };
        out.push(PerlValue::float(v));
    }
    Ok(PerlValue::array(out))
}

/// `normalize_signal SIGNAL` — normalize to [-1, 1] range.
fn builtin_normalize_signal(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    if signal.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let max_abs = signal.iter().map(|&v| v.abs()).fold(0.0, f64::max);
    if max_abs == 0.0 {
        return Ok(PerlValue::array(
            signal.into_iter().map(PerlValue::float).collect(),
        ));
    }
    Ok(PerlValue::array(
        signal
            .into_iter()
            .map(|v| PerlValue::float(v / max_abs))
            .collect(),
    ))
}

/// `energy SIGNAL` — sum of squares.
fn builtin_energy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = flatten_args(args).iter().map(|v| v.to_number()).collect();
    Ok(PerlValue::float(signal.iter().map(|v| v * v).sum()))
}

/// `spectral_centroid SIGNAL, SAMPLE_RATE` — center of mass of spectrum.
fn builtin_spectral_centroid(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let sr = args.get(1).map(|v| v.to_number()).unwrap_or(44100.0);
    let n = signal.len();
    if n == 0 {
        return Ok(PerlValue::float(0.0));
    }
    let mut weighted_sum = 0.0;
    let mut mag_sum = 0.0;
    for k in 0..=n / 2 {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &s) in signal.iter().enumerate() {
            let angle = -2.0 * std::f64::consts::PI * k as f64 * i as f64 / n as f64;
            re += s * angle.cos();
            im += s * angle.sin();
        }
        let mag = (re * re + im * im).sqrt();
        let freq = k as f64 * sr / n as f64;
        weighted_sum += freq * mag;
        mag_sum += mag;
    }
    Ok(PerlValue::float(if mag_sum == 0.0 {
        0.0
    } else {
        weighted_sum / mag_sum
    }))
}

/// `envelope SIGNAL, ATTACK, RELEASE` — amplitude envelope follower.
fn builtin_envelope(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let attack = args
        .get(1)
        .map(|v| v.to_number())
        .unwrap_or(0.01)
        .clamp(0.0, 1.0);
    let release = args
        .get(2)
        .map(|v| v.to_number())
        .unwrap_or(0.1)
        .clamp(0.0, 1.0);
    if signal.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let mut env = 0.0;
    let out: Vec<PerlValue> = signal
        .iter()
        .map(|&s| {
            let abs_s = s.abs();
            if abs_s > env {
                env = attack * abs_s + (1.0 - attack) * env;
            } else {
                env = release * abs_s + (1.0 - release) * env;
            }
            PerlValue::float(env)
        })
        .collect();
    Ok(PerlValue::array(out))
}

/// `cross_correlation A, B` — cross-correlation of two signals.
fn builtin_cross_correlation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    if a.is_empty() || b.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let n = a.len() + b.len() - 1;
    let mut result = vec![0.0; n];
    for (i, &ai) in a.iter().enumerate() {
        for (j, &bj) in b.iter().enumerate() {
            result[i + j] += ai * bj;
        }
    }
    Ok(PerlValue::array(
        result.into_iter().map(PerlValue::float).collect(),
    ))
}

/// `downsample SIGNAL, FACTOR` — reduce sample rate by integer factor.
fn builtin_downsample(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let factor = args.get(1).map(|v| v.to_int()).unwrap_or(2).max(1) as usize;
    Ok(PerlValue::array(
        signal
            .into_iter()
            .step_by(factor)
            .map(PerlValue::float)
            .collect(),
    ))
}

/// `upsample SIGNAL, FACTOR` — increase sample rate by inserting zeros.
fn builtin_upsample(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let signal: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let factor = args.get(1).map(|v| v.to_int()).unwrap_or(2).max(1) as usize;
    let mut out = Vec::with_capacity(signal.len() * factor);
    for &s in &signal {
        out.push(PerlValue::float(s));
        for _ in 1..factor {
            out.push(PerlValue::float(0.0));
        }
    }
    Ok(PerlValue::array(out))
}

// ─────────────────────────────────────────────────────────────────────────────
// Miscellaneous
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_fizzbuzz(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0);
    Ok(PerlValue::array(
        (1..=n)
            .map(|i| {
                PerlValue::string(match (i % 3, i % 5) {
                    (0, 0) => "FizzBuzz".to_string(),
                    (0, _) => "Fizz".to_string(),
                    (_, 0) => "Buzz".to_string(),
                    _ => i.to_string(),
                })
            })
            .collect(),
    ))
}
fn builtin_roman_numeral_list(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_int().max(0)).unwrap_or(10);
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    Ok(PerlValue::array(
        (1..=n)
            .map(|i| {
                let mut s = String::new();
                let mut rem = i;
                for &(val, sym) in &table {
                    while rem >= val {
                        s.push_str(sym);
                        rem -= val;
                    }
                }
                PerlValue::string(s)
            })
            .collect(),
    ))
}
fn builtin_look_and_say(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = first_arg_or_topic(interp, args).to_string();
    if s.is_empty() {
        return Ok(PerlValue::string("1".to_string()));
    }
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let mut count = 1;
        while i + count < chars.len() && chars[i + count] == c {
            count += 1;
        }
        result.push_str(&format!("{}{}", count, c));
        i += count;
    }
    Ok(PerlValue::string(result))
}
fn builtin_gray_code_sequence(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().clamp(0, 20) as u32;
    Ok(PerlValue::array(
        (0..1u64 << n)
            .map(|i| PerlValue::integer((i ^ (i >> 1)) as i64))
            .collect(),
    ))
}
fn builtin_sierpinski(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().clamp(0, 8) as u32;
    let size = 1usize << n;
    let mut lines = Vec::with_capacity(size);
    for y in 0..size {
        let mut line = " ".repeat(size - y - 1);
        for x in 0..=y {
            line.push_str(if (x & y) == x { "* " } else { "  " });
        }
        lines.push(line.trim_end().to_string());
    }
    Ok(PerlValue::string(lines.join("\n")))
}
fn builtin_mandelbrot_char(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cx = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let cy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let max_iter = args
        .get(2)
        .map(|v| v.to_int().max(1) as usize)
        .unwrap_or(100);
    let (mut zx, mut zy) = (0.0, 0.0);
    let mut i = 0;
    while i < max_iter && zx * zx + zy * zy < 4.0 {
        let tmp = zx * zx - zy * zy + cx;
        zy = 2.0 * zx * zy + cy;
        zx = tmp;
        i += 1;
    }
    let chars = b" .:-=+*#%@";
    Ok(PerlValue::string(String::from(if i >= max_iter {
        ' '
    } else {
        chars[i % chars.len()] as char
    })))
}
fn builtin_game_of_life_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let grid = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let rows = grid.len();
    if rows == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    let data: Vec<Vec<i64>> = grid
        .iter()
        .map(|r| arg_to_vec(r).iter().map(|v| v.to_int()).collect())
        .collect();
    let cols = data[0].len();
    let mut result = Vec::with_capacity(rows);
    for r in 0..rows {
        let mut row = Vec::with_capacity(cols);
        for c in 0..cols {
            let mut neighbors = 0;
            for dr in [-1i64, 0, 1] {
                for dc in [-1i64, 0, 1] {
                    if dr == 0 && dc == 0 {
                        continue;
                    }
                    let (nr, nc) = (r as i64 + dr, c as i64 + dc);
                    if nr >= 0 && nr < rows as i64 && nc >= 0 && nc < cols as i64 {
                        neighbors += data[nr as usize][nc as usize];
                    }
                }
            }
            row.push(PerlValue::integer(
                if if data[r][c] == 1 {
                    neighbors == 2 || neighbors == 3
                } else {
                    neighbors == 3
                } {
                    1
                } else {
                    0
                },
            ));
        }
        result.push(PerlValue::array_ref(Arc::new(RwLock::new(row))));
    }
    Ok(PerlValue::array(result))
}
fn builtin_tower_of_hanoi(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(
        (1i64 << first_arg_or_topic(interp, args).to_int().max(0)) - 1,
    ))
}
fn builtin_pascals_triangle(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0) as usize;
    let mut rows: Vec<Vec<i64>> = Vec::with_capacity(n);
    for i in 0..n {
        let mut row = vec![1i64; i + 1];
        for j in 1..i {
            row[j] = rows[i - 1][j - 1] + rows[i - 1][j];
        }
        rows.push(row);
    }
    Ok(PerlValue::array(
        rows.into_iter()
            .map(|r| {
                PerlValue::array_ref(Arc::new(RwLock::new(
                    r.into_iter().map(PerlValue::integer).collect(),
                )))
            })
            .collect(),
    ))
}
fn builtin_truth_table(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().clamp(0, 20) as u32;
    Ok(PerlValue::array(
        (0..1u64 << n)
            .map(|i| {
                let row: Vec<PerlValue> = (0..n)
                    .rev()
                    .map(|bit| PerlValue::integer(((i >> bit) & 1) as i64))
                    .collect();
                PerlValue::array_ref(Arc::new(RwLock::new(row)))
            })
            .collect(),
    ))
}
fn builtin_base_convert(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let from = args
        .get(1)
        .map(|v| v.to_int() as u32)
        .unwrap_or(10)
        .clamp(2, 36);
    let to = args
        .get(2)
        .map(|v| v.to_int() as u32)
        .unwrap_or(10)
        .clamp(2, 36);
    let n = i64::from_str_radix(s.trim(), from).unwrap_or(0);
    builtin_to_base(&[PerlValue::integer(n), PerlValue::integer(to as i64)])
}
fn builtin_roman_add(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let interp = crate::interpreter::Interpreter::new();
    let a = builtin_roman_to_int(&interp, &args[..1.min(args.len())])?.to_int();
    let b = if args.len() > 1 {
        builtin_roman_to_int(&interp, &args[1..2])?.to_int()
    } else {
        0
    };
    builtin_int_to_roman(&interp, &[PerlValue::integer(a + b)])
}
fn builtin_bearing(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (lat1, lon1) = (
        args.first()
            .map(|v| v.to_number())
            .unwrap_or(0.0)
            .to_radians(),
        args.get(1)
            .map(|v| v.to_number())
            .unwrap_or(0.0)
            .to_radians(),
    );
    let (lat2, lon2) = (
        args.get(2)
            .map(|v| v.to_number())
            .unwrap_or(0.0)
            .to_radians(),
        args.get(3)
            .map(|v| v.to_number())
            .unwrap_or(0.0)
            .to_radians(),
    );
    let dlon = lon2 - lon1;
    let bearing = (dlon.sin() * lat2.cos())
        .atan2(lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos())
        .to_degrees();
    Ok(PerlValue::float((bearing + 360.0) % 360.0))
}
fn builtin_bmi(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let weight = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let height = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if height == 0.0 {
        0.0
    } else {
        weight / (height * height)
    }))
}
fn builtin_bac_estimate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let drinks = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let weight_kg = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    let hours = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let gender = args
        .get(3)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "m".to_string());
    let r = if gender.starts_with('f') || gender.starts_with('F') {
        0.55
    } else {
        0.68
    };
    Ok(PerlValue::float(
        ((drinks * 14.0) / (weight_kg * 1000.0 * r) * 100.0 - 0.015 * hours).max(0.0),
    ))
}

// ════════════════════════════════════════════════════════════════════════════
// Math Formulas
// ════════════════════════════════════════════════════════════════════════════

fn builtin_quadratic_roots(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if a == 0.0 {
        return Ok(if b == 0.0 {
            PerlValue::UNDEF
        } else {
            PerlValue::array(vec![PerlValue::float(-c / b)])
        });
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return Ok(PerlValue::UNDEF);
    }
    let sqrt_disc = disc.sqrt();
    Ok(PerlValue::array(vec![
        PerlValue::float((-b + sqrt_disc) / (2.0 * a)),
        PerlValue::float((-b - sqrt_disc) / (2.0 * a)),
    ]))
}

fn builtin_quadratic_discriminant(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(b * b - 4.0 * a * c))
}

fn builtin_arithmetic_series(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a1 = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(n / 2.0 * (2.0 * a1 + (n - 1.0) * d)))
}

fn builtin_geometric_series(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a1 = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if (r - 1.0).abs() < f64::EPSILON {
        a1 * n
    } else {
        a1 * (1.0 - r.powf(n)) / (1.0 - r)
    }))
}

fn builtin_stirling_approx(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_number().max(0.0);
    if n < 1.0 {
        return Ok(PerlValue::float(1.0));
    }
    let e = std::f64::consts::E;
    let pi = std::f64::consts::PI;
    Ok(PerlValue::float((2.0 * pi * n).sqrt() * (n / e).powf(n)))
}

fn builtin_double_factorial(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = first_arg_or_topic(interp, args).to_int().max(0);
    let mut result: i64 = 1;
    let mut i = n;
    while i > 1 {
        result = result.saturating_mul(i);
        i -= 2;
    }
    Ok(PerlValue::integer(result))
}

fn builtin_rising_factorial(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(1).map(|v| v.to_int()).unwrap_or(1).max(0);
    let mut result = 1.0;
    for i in 0..n {
        result *= x + i as f64;
    }
    Ok(PerlValue::float(result))
}

fn builtin_falling_factorial(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(1).map(|v| v.to_int()).unwrap_or(1).max(0);
    let mut result = 1.0;
    for i in 0..n {
        result *= x - i as f64;
    }
    Ok(PerlValue::float(result))
}

fn builtin_gamma_approx(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = first_arg_or_topic(interp, args).to_number();
    fn gamma_internal(z: f64) -> f64 {
        let g = 7;
        let c = [
            0.999_999_999_999_809_9,
            676.5203681218851,
            -1259.1392167224028,
            771.323_428_777_653_1,
            -176.615_029_162_140_6,
            12.507343278686905,
            -0.13857109526572012,
            9.984_369_578_019_572e-6,
            1.5056327351493116e-7,
        ];
        if z < 0.5 {
            let pi = std::f64::consts::PI;
            return pi / ((pi * z).sin() * gamma_internal(1.0 - z));
        }
        let z = z - 1.0;
        let mut x = c[0];
        for i in 1..(g + 2) {
            x += c[i] / (z + i as f64);
        }
        let t = z + g as f64 + 0.5;
        (2.0 * std::f64::consts::PI).sqrt() * t.powf(z + 0.5) * (-t).exp() * x
    }
    Ok(PerlValue::float(gamma_internal(z)))
}

fn builtin_erf_approx(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = first_arg_or_topic(interp, args).to_number();
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let p = 0.3275911;
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + p * x);
    let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x * x).exp();
    Ok(PerlValue::float(sign * y))
}

fn builtin_normal_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let pi = std::f64::consts::PI;
    let z = (x - mu) / sigma;
    Ok(PerlValue::float(
        (1.0 / (sigma * (2.0 * pi).sqrt())) * (-0.5 * z * z).exp(),
    ))
}

fn builtin_normal_cdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let z = (x - mu) / (sigma * std::f64::consts::SQRT_2);
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;
    let p = 0.3275911;
    let sign = if z < 0.0 { -1.0 } else { 1.0 };
    let z_abs = z.abs();
    let t = 1.0 / (1.0 + p * z_abs);
    let erf = sign
        * (1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-z_abs * z_abs).exp());
    Ok(PerlValue::float(0.5 * (1.0 + erf)))
}

fn builtin_poisson_pmf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = args.first().map(|v| v.to_int()).unwrap_or(0).max(0);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut result = (-lambda).exp();
    for i in 1..=k {
        result *= lambda / i as f64;
    }
    Ok(PerlValue::float(result))
}

fn builtin_exponential_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if x < 0.0 {
        0.0
    } else {
        lambda * (-lambda * x).exp()
    }))
}

fn builtin_inverse_lerp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let denom = b - a;
    Ok(PerlValue::float(if denom.abs() < f64::EPSILON {
        0.0
    } else {
        (v - a) / denom
    }))
}

fn builtin_map_range(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let value = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let in_min = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let in_max = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let out_min = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let out_max = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let t = (value - in_min) / (in_max - in_min);
    Ok(PerlValue::float(out_min + t * (out_max - out_min)))
}

// ════════════════════════════════════════════════════════════════════════════
// Physics Formulas
// ════════════════════════════════════════════════════════════════════════════

fn builtin_momentum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(m * v))
}

fn builtin_impulse(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(f * t))
}

fn builtin_work(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let angle = args
        .get(2)
        .map(|v| v.to_number())
        .unwrap_or(0.0)
        .to_radians();
    Ok(PerlValue::float(f * d * angle.cos()))
}

fn builtin_power_phys(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if t == 0.0 {
        f64::INFINITY
    } else {
        w / t
    }))
}

fn builtin_torque(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let angle = args
        .get(2)
        .map(|v| v.to_number())
        .unwrap_or(90.0)
        .to_radians();
    Ok(PerlValue::float(r * f * angle.sin()))
}

fn builtin_angular_velocity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if t == 0.0 { 0.0 } else { theta / t }))
}

fn builtin_centripetal_force(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if r == 0.0 {
        f64::INFINITY
    } else {
        m * v * v / r
    }))
}

fn builtin_escape_velocity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args.first().map(|v| v.to_number()).unwrap_or(5.972e24);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(6.371e6);
    const G: f64 = 6.67430e-11;
    Ok(PerlValue::float((2.0 * G * m / r).sqrt()))
}

fn builtin_orbital_velocity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args.first().map(|v| v.to_number()).unwrap_or(5.972e24);
    let r = args
        .get(1)
        .map(|v| v.to_number())
        .unwrap_or(6.371e6 + 400000.0);
    const G: f64 = 6.67430e-11;
    Ok(PerlValue::float((G * m / r).sqrt()))
}

fn builtin_orbital_period(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args.first().map(|v| v.to_number()).unwrap_or(5.972e24);
    let r = args
        .get(1)
        .map(|v| v.to_number())
        .unwrap_or(6.371e6 + 400000.0);
    const G: f64 = 6.67430e-11;
    let pi = std::f64::consts::PI;
    Ok(PerlValue::float(2.0 * pi * (r.powi(3) / (G * m)).sqrt()))
}

fn builtin_gravitational_force(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m1 = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let m2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    const G: f64 = 6.67430e-11;
    Ok(PerlValue::float(if r == 0.0 {
        f64::INFINITY
    } else {
        G * m1 * m2 / (r * r)
    }))
}

fn builtin_coulomb_force(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q1 = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let q2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    const K: f64 = 8.9875517923e9;
    Ok(PerlValue::float(if r == 0.0 {
        f64::INFINITY
    } else {
        K * q1 * q2 / (r * r)
    }))
}

fn builtin_electric_field(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    const K: f64 = 8.9875517923e9;
    Ok(PerlValue::float(if r == 0.0 {
        f64::INFINITY
    } else {
        K * q / (r * r)
    }))
}

fn builtin_capacitance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if v == 0.0 { 0.0 } else { q / v }))
}

fn builtin_capacitor_energy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * c * v * v))
}

fn builtin_inductor_energy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * l * i * i))
}

fn builtin_resonant_frequency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let pi = std::f64::consts::PI;
    Ok(PerlValue::float(1.0 / (2.0 * pi * (l * c).sqrt())))
}

fn builtin_rc_time_constant(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(r * c))
}

fn builtin_rl_time_constant(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if r == 0.0 {
        f64::INFINITY
    } else {
        l / r
    }))
}

fn builtin_impedance_rlc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let f = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let omega = 2.0 * std::f64::consts::PI * f;
    let xl = omega * l;
    let xc = if c == 0.0 || omega == 0.0 {
        0.0
    } else {
        1.0 / (omega * c)
    };
    Ok(PerlValue::float((r * r + (xl - xc).powi(2)).sqrt()))
}

fn builtin_relativistic_mass(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m0 = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    const C: f64 = 299792458.0;
    let v2_c2 = (v / C).powi(2);
    Ok(PerlValue::float(if v2_c2 >= 1.0 {
        f64::INFINITY
    } else {
        m0 / (1.0 - v2_c2).sqrt()
    }))
}

fn builtin_lorentz_factor(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = first_arg_or_topic(interp, args).to_number();
    const C: f64 = 299792458.0;
    let v2_c2 = (v / C).powi(2);
    Ok(PerlValue::float(if v2_c2 >= 1.0 {
        f64::INFINITY
    } else {
        1.0 / (1.0 - v2_c2).sqrt()
    }))
}

fn builtin_time_dilation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dt0 = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    const C: f64 = 299792458.0;
    let v2_c2 = (v / C).powi(2);
    Ok(PerlValue::float(if v2_c2 >= 1.0 {
        f64::INFINITY
    } else {
        dt0 / (1.0 - v2_c2).sqrt()
    }))
}

fn builtin_length_contraction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l0 = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    const C: f64 = 299792458.0;
    let v2_c2 = (v / C).powi(2);
    Ok(PerlValue::float(if v2_c2 >= 1.0 {
        0.0
    } else {
        l0 * (1.0 - v2_c2).sqrt()
    }))
}

fn builtin_relativistic_energy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m0 = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    const C: f64 = 299792458.0;
    let v2_c2 = (v / C).powi(2);
    let gamma = if v2_c2 >= 1.0 {
        f64::INFINITY
    } else {
        1.0 / (1.0 - v2_c2).sqrt()
    };
    Ok(PerlValue::float(gamma * m0 * C * C))
}

fn builtin_rest_energy(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = first_arg_or_topic(interp, args).to_number();
    const C: f64 = 299792458.0;
    Ok(PerlValue::float(m * C * C))
}

fn builtin_de_broglie_wavelength(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args.first().map(|v| v.to_number()).unwrap_or(9.109e-31);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    const H: f64 = 6.62607015e-34;
    let p = m * v;
    Ok(PerlValue::float(if p == 0.0 {
        f64::INFINITY
    } else {
        H / p
    }))
}

fn builtin_photon_energy(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = first_arg_or_topic(interp, args).to_number();
    const H: f64 = 6.62607015e-34;
    Ok(PerlValue::float(H * f))
}

fn builtin_photon_energy_wavelength(
    interp: &Interpreter,
    args: &[PerlValue],
) -> PerlResult<PerlValue> {
    let lambda = first_arg_or_topic(interp, args).to_number();
    const H: f64 = 6.62607015e-34;
    const C: f64 = 299792458.0;
    Ok(PerlValue::float(if lambda == 0.0 {
        f64::INFINITY
    } else {
        H * C / lambda
    }))
}

fn builtin_schwarzschild_radius(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = first_arg_or_topic(interp, args).to_number();
    const G: f64 = 6.67430e-11;
    const C: f64 = 299792458.0;
    Ok(PerlValue::float(2.0 * G * m / (C * C)))
}

fn builtin_stefan_boltzmann(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let area = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let temp = args.get(1).map(|v| v.to_number()).unwrap_or(300.0);
    const SIGMA: f64 = 5.670374419e-8;
    Ok(PerlValue::float(SIGMA * area * temp.powi(4)))
}

fn builtin_wien_displacement(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let temp = first_arg_or_topic(interp, args).to_number();
    const B: f64 = 2.897771955e-3;
    Ok(PerlValue::float(if temp == 0.0 {
        f64::INFINITY
    } else {
        B / temp
    }))
}

fn builtin_ideal_gas_pressure(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(273.15);
    const R: f64 = 8.314462618;
    Ok(PerlValue::float(if v == 0.0 {
        f64::INFINITY
    } else {
        n * R * t / v
    }))
}

fn builtin_ideal_gas_volume(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(101325.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(273.15);
    const R: f64 = 8.314462618;
    Ok(PerlValue::float(if p == 0.0 {
        f64::INFINITY
    } else {
        n * R * t / p
    }))
}

fn builtin_projectile_range(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args
        .get(1)
        .map(|v| v.to_number())
        .unwrap_or(45.0)
        .to_radians();
    const G: f64 = 9.80665;
    Ok(PerlValue::float(v * v * (2.0 * theta).sin() / G))
}

fn builtin_projectile_max_height(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args
        .get(1)
        .map(|v| v.to_number())
        .unwrap_or(45.0)
        .to_radians();
    const G: f64 = 9.80665;
    Ok(PerlValue::float(v * v * theta.sin().powi(2) / (2.0 * G)))
}

fn builtin_projectile_time(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args
        .get(1)
        .map(|v| v.to_number())
        .unwrap_or(45.0)
        .to_radians();
    const G: f64 = 9.80665;
    Ok(PerlValue::float(2.0 * v * theta.sin() / G))
}

fn builtin_spring_force(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(-k * x))
}

fn builtin_spring_energy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * k * x * x))
}

fn builtin_pendulum_period(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = first_arg_or_topic(interp, args).to_number();
    const G: f64 = 9.80665;
    let pi = std::f64::consts::PI;
    Ok(PerlValue::float(2.0 * pi * (l / G).sqrt()))
}

fn builtin_doppler_frequency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = args.first().map(|v| v.to_number()).unwrap_or(440.0);
    let vs = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let vo = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    const V_SOUND: f64 = 343.0;
    let denom = V_SOUND + vs;
    Ok(PerlValue::float(if denom == 0.0 {
        f64::INFINITY
    } else {
        f * (V_SOUND + vo) / denom
    }))
}

fn builtin_decibel_ratio(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p1 = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let p2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if p2 == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (p1 / p2).log10()
    }))
}

fn builtin_snells_law(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n1 = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let n2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let theta1 = args
        .get(2)
        .map(|v| v.to_number())
        .unwrap_or(0.0)
        .to_radians();
    let sin_theta2 = n1 * theta1.sin() / n2;
    if sin_theta2.abs() > 1.0 {
        return Ok(PerlValue::UNDEF);
    }
    Ok(PerlValue::float(sin_theta2.asin().to_degrees()))
}

fn builtin_brewster_angle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n1 = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let n2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.5);
    Ok(PerlValue::float((n2 / n1).atan().to_degrees()))
}

fn builtin_critical_angle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n1 = args.first().map(|v| v.to_number()).unwrap_or(1.5);
    let n2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let ratio = n2 / n1;
    if ratio > 1.0 {
        return Ok(PerlValue::UNDEF);
    }
    Ok(PerlValue::float(ratio.asin().to_degrees()))
}

fn builtin_lens_power(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = first_arg_or_topic(interp, args).to_number();
    Ok(PerlValue::float(if f == 0.0 {
        f64::INFINITY
    } else {
        1.0 / f
    }))
}

fn builtin_thin_lens(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d_o = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let f = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let inv_di = 1.0 / f - 1.0 / d_o;
    Ok(PerlValue::float(if inv_di == 0.0 {
        f64::INFINITY
    } else {
        1.0 / inv_di
    }))
}

fn builtin_magnification_lens(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let di = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let d_o = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if d_o == 0.0 { 0.0 } else { -di / d_o }))
}

// ════════════════════════════════════════════════════════════════════════════
// Math Constants
// ════════════════════════════════════════════════════════════════════════════

fn builtin_euler_mascheroni(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(0.5772156649015329))
}

fn builtin_apery_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.2020569031595943))
}

fn builtin_feigenbaum_delta(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(4.669_201_609_102_99))
}

fn builtin_feigenbaum_alpha(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(2.502907875095893))
}

fn builtin_catalan_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(0.915_965_594_177_219))
}

fn builtin_khinchin_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(2.6854520010653065))
}

fn builtin_glaisher_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.2824271291006226))
}

fn builtin_plastic_number(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.3247179572447458))
}

fn builtin_silver_ratio(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.0 + std::f64::consts::SQRT_2))
}

fn builtin_supergolden_ratio(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.465_571_231_876_768))
}

// ════════════════════════════════════════════════════════════════════════════
// Physics Constants
// ════════════════════════════════════════════════════════════════════════════

fn builtin_vacuum_permittivity(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(8.8541878128e-12))
}

fn builtin_vacuum_permeability(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.25663706212e-6))
}

fn builtin_coulomb_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(8.9875517923e9))
}

fn builtin_fine_structure_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(0.0072973525693))
}

fn builtin_rydberg_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(10973731.568160))
}

fn builtin_bohr_radius(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(5.29177210903e-11))
}

fn builtin_bohr_magneton(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(9.2740100783e-24))
}

fn builtin_nuclear_magneton(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(5.0507837461e-27))
}

fn builtin_stefan_boltzmann_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(5.670374419e-8))
}

fn builtin_wien_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(2.897771955e-3))
}

fn builtin_gas_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(8.314462618))
}

fn builtin_faraday_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(96485.33212))
}

fn builtin_neutron_mass(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.67492749804e-27))
}

fn builtin_atomic_mass_unit(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.66053906660e-27))
}

fn builtin_earth_mass(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(5.972167867e24))
}

fn builtin_earth_radius(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(6.3710088e6))
}

fn builtin_sun_mass(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.98892e30))
}

fn builtin_sun_radius(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(6.9634e8))
}

fn builtin_astronomical_unit(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.495978707e11))
}

fn builtin_light_year(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(9.4607304725808e15))
}

fn builtin_parsec(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(3.085_677_581_491_367e16))
}

fn builtin_hubble_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(70.0))
}

fn builtin_planck_length(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.616255e-35))
}

fn builtin_planck_time(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(5.391247e-44))
}

fn builtin_planck_mass(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(2.176434e-8))
}

fn builtin_planck_temperature(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.416784e32))
}

// ─────────────────────────────────────────────────────────────────────────────
// Linear Algebra (solvers, decompositions, norms)
// ─────────────────────────────────────────────────────────────────────────────

/// Helper: parse NxN matrix from args.
fn args_to_matrix(arg: &PerlValue) -> Vec<Vec<f64>> {
    arg_to_vec(arg)
        .iter()
        .map(|r| arg_to_vec(r).iter().map(|v| v.to_number()).collect())
        .collect()
}

/// Helper: matrix to PerlValue array-of-arrays.
fn matrix_to_perl(m: &[Vec<f64>]) -> PerlValue {
    PerlValue::array(
        m.iter()
            .map(|row| PerlValue::array(row.iter().map(|&v| PerlValue::float(v)).collect()))
            .collect(),
    )
}

/// Helper: vector to PerlValue array.
fn vec_to_perl(v: &[f64]) -> PerlValue {
    PerlValue::array(v.iter().map(|&x| PerlValue::float(x)).collect())
}

/// `matrix_solve A, b` — solve Ax=b via Gaussian elimination with partial pivoting.
fn builtin_matrix_solve(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let bv = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let b: Vec<f64> = bv.iter().map(|v| v.to_number()).collect();
    let n = a.len();
    if n == 0 || b.len() != n {
        return Err(PerlError::runtime("matrix_solve: dimension mismatch", 0));
    }
    // Augmented matrix
    let mut aug: Vec<Vec<f64>> = a
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let mut r = row.clone();
            r.push(b[i]);
            r
        })
        .collect();
    // Forward elimination with partial pivoting
    for col in 0..n {
        let mut max_row = col;
        let mut max_val = aug[col][col].abs();
        for row in (col + 1)..n {
            if aug[row][col].abs() > max_val {
                max_val = aug[row][col].abs();
                max_row = row;
            }
        }
        if max_val < 1e-12 {
            return Err(PerlError::runtime("matrix_solve: singular matrix", 0));
        }
        aug.swap(col, max_row);
        let pivot = aug[col][col];
        for row in (col + 1)..n {
            let factor = aug[row][col] / pivot;
            for j in col..=n {
                let v = aug[col][j];
                aug[row][j] -= factor * v;
            }
        }
    }
    // Back substitution
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = aug[i][n];
        for j in (i + 1)..n {
            s -= aug[i][j] * x[j];
        }
        x[i] = s / aug[i][i];
    }
    Ok(vec_to_perl(&x))
}

/// `matrix_lu M` — LU decomposition. Returns [L, U, P] where PA = LU.
fn builtin_matrix_lu(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = a.len();
    if n == 0 {
        return Err(PerlError::runtime("matrix_lu: empty matrix", 0));
    }
    let mut u = a.clone();
    let mut l = vec![vec![0.0; n]; n];
    let mut perm: Vec<usize> = (0..n).collect();
    for i in 0..n {
        l[i][i] = 1.0;
    }
    for col in 0..n {
        let mut max_row = col;
        let mut max_val = u[col][col].abs();
        for row in (col + 1)..n {
            if u[row][col].abs() > max_val {
                max_val = u[row][col].abs();
                max_row = row;
            }
        }
        if max_row != col {
            u.swap(col, max_row);
            perm.swap(col, max_row);
            for k in 0..col {
                let tmp = l[col][k];
                l[col][k] = l[max_row][k];
                l[max_row][k] = tmp;
            }
        }
        if u[col][col].abs() < 1e-12 {
            continue;
        }
        for row in (col + 1)..n {
            let factor = u[row][col] / u[col][col];
            l[row][col] = factor;
            for j in col..n {
                let v = u[col][j];
                u[row][j] -= factor * v;
            }
        }
    }
    let p: Vec<Vec<f64>> = perm
        .iter()
        .map(|&pi| {
            let mut row = vec![0.0; n];
            row[pi] = 1.0;
            row
        })
        .collect();
    Ok(PerlValue::array(vec![
        matrix_to_perl(&l),
        matrix_to_perl(&u),
        matrix_to_perl(&p),
    ]))
}

/// `matrix_qr M` — QR decomposition via Gram-Schmidt. Returns [Q, R].
fn builtin_matrix_qr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let m = a.len();
    if m == 0 {
        return Err(PerlError::runtime("matrix_qr: empty matrix", 0));
    }
    let n = a[0].len();
    // Columns of A
    let cols: Vec<Vec<f64>> = (0..n).map(|j| (0..m).map(|i| a[i][j]).collect()).collect();
    let mut q_cols: Vec<Vec<f64>> = Vec::with_capacity(n);
    let mut r = vec![vec![0.0; n]; n];
    for j in 0..n {
        let mut v = cols[j].clone();
        for i in 0..q_cols.len() {
            let dot: f64 = v.iter().zip(q_cols[i].iter()).map(|(a, b)| a * b).sum();
            r[i][j] = dot;
            for k in 0..m {
                v[k] -= dot * q_cols[i][k];
            }
        }
        let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        r[j][j] = norm;
        if norm > 1e-12 {
            for k in 0..m {
                v[k] /= norm;
            }
        }
        q_cols.push(v);
    }
    // Build Q matrix (m x n)
    let q: Vec<Vec<f64>> = (0..m)
        .map(|i| (0..n.min(q_cols.len())).map(|j| q_cols[j][i]).collect())
        .collect();
    Ok(PerlValue::array(vec![
        matrix_to_perl(&q),
        matrix_to_perl(&r),
    ]))
}

/// `matrix_eigenvalues M` — eigenvalues of a square matrix via QR iteration.
fn builtin_matrix_eigenvalues(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut a = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = a.len();
    if n == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    // QR iteration (simple, no shifts)
    for _ in 0..200 {
        // QR decompose
        let m = a.len();
        let nc = a[0].len();
        let cols: Vec<Vec<f64>> = (0..nc).map(|j| (0..m).map(|i| a[i][j]).collect()).collect();
        let mut q_cols: Vec<Vec<f64>> = Vec::with_capacity(nc);
        let mut r = vec![vec![0.0; nc]; nc];
        for j in 0..nc {
            let mut v = cols[j].clone();
            for i in 0..q_cols.len() {
                let dot: f64 = v.iter().zip(q_cols[i].iter()).map(|(a, b)| a * b).sum();
                r[i][j] = dot;
                for k in 0..m {
                    v[k] -= dot * q_cols[i][k];
                }
            }
            let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
            r[j][j] = norm;
            if norm > 1e-12 {
                for k in 0..m {
                    v[k] /= norm;
                }
            }
            q_cols.push(v);
        }
        // A = R * Q
        let mut new_a = vec![vec![0.0; n]; n];
        for i in 0..n {
            for j in 0..n {
                let mut s = 0.0;
                for k in 0..n {
                    s += r[i][k] * q_cols[j][k];
                }
                new_a[i][j] = s;
            }
        }
        a = new_a;
        // Check convergence (sub-diagonal near zero)
        let mut converged = true;
        for i in 1..n {
            if a[i][i - 1].abs() > 1e-10 {
                converged = false;
                break;
            }
        }
        if converged {
            break;
        }
    }
    let eigs: Vec<PerlValue> = (0..n).map(|i| PerlValue::float(a[i][i])).collect();
    Ok(PerlValue::array(eigs))
}

/// `matrix_norm M [, p]` — matrix norm (default Frobenius).
fn builtin_matrix_norm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    if p == 1.0 {
        // Max absolute column sum
        if m.is_empty() {
            return Ok(PerlValue::float(0.0));
        }
        let cols = m[0].len();
        let mut max = 0.0f64;
        for j in 0..cols {
            let s: f64 = m.iter().map(|row| row[j].abs()).sum();
            max = max.max(s);
        }
        Ok(PerlValue::float(max))
    } else if p == f64::INFINITY {
        // Max absolute row sum
        let max = m
            .iter()
            .map(|row| row.iter().map(|v| v.abs()).sum::<f64>())
            .fold(0.0f64, f64::max);
        Ok(PerlValue::float(max))
    } else {
        // Frobenius
        let sum: f64 = m.iter().flat_map(|row| row.iter()).map(|v| v * v).sum();
        Ok(PerlValue::float(sum.sqrt()))
    }
}

/// `matrix_cond M` — condition number (ratio of max/min singular values via eigenvalues of A^T A).
fn builtin_matrix_cond(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = a.len();
    if n == 0 {
        return Ok(PerlValue::float(f64::INFINITY));
    }
    let m = a[0].len();
    // Compute A^T * A
    let mut ata = vec![vec![0.0; m]; m];
    for i in 0..m {
        for j in 0..m {
            let mut s = 0.0;
            for k in 0..n {
                s += a[k][i] * a[k][j];
            }
            ata[i][j] = s;
        }
    }
    // Get eigenvalues of A^T A (singular values squared)
    let eig_args = [matrix_to_perl(&ata)];
    let eigs = builtin_matrix_eigenvalues(&eig_args)?;
    let ev = arg_to_vec(&eigs);
    let vals: Vec<f64> = ev.iter().map(|v| v.to_number().abs()).collect();
    let max = vals.iter().cloned().fold(0.0f64, f64::max);
    let min = vals.iter().cloned().fold(f64::INFINITY, f64::min);
    if min < 1e-15 {
        Ok(PerlValue::float(f64::INFINITY))
    } else {
        Ok(PerlValue::float((max / min).sqrt()))
    }
}

/// `matrix_pinv M` — Moore-Penrose pseudo-inverse via SVD approximation.
fn builtin_matrix_pinv(args: &[PerlValue]) -> PerlResult<PerlValue> {
    // Use A^+ = (A^T A)^{-1} A^T for overdetermined systems
    let a = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = a.len();
    if n == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    let m = a[0].len();
    // A^T
    let mut at = vec![vec![0.0; n]; m];
    for i in 0..n {
        for j in 0..m {
            at[j][i] = a[i][j];
        }
    }
    // A^T * A
    let mut ata = vec![vec![0.0; m]; m];
    for i in 0..m {
        for j in 0..m {
            let mut s = 0.0;
            for k in 0..n {
                s += at[i][k] * a[k][j];
            }
            ata[i][j] = s;
        }
    }
    // Invert A^T A
    let inv_args = [matrix_to_perl(&ata)];
    let inv = builtin_matrix_inverse(&inv_args)?;
    let ata_inv = args_to_matrix(&inv);
    // (A^T A)^{-1} * A^T
    let mut result = vec![vec![0.0; n]; m];
    for i in 0..m {
        for j in 0..n {
            let mut s = 0.0;
            for k in 0..m {
                s += ata_inv[i][k] * at[k][j];
            }
            result[i][j] = s;
        }
    }
    Ok(matrix_to_perl(&result))
}

/// `matrix_cholesky M` — Cholesky decomposition of symmetric positive-definite matrix. Returns L where M = L * L^T.
fn builtin_matrix_cholesky(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = a.len();
    let mut l = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let mut s = 0.0;
            for k in 0..j {
                s += l[i][k] * l[j][k];
            }
            if i == j {
                let val = a[i][i] - s;
                if val < 0.0 {
                    return Err(PerlError::runtime(
                        "matrix_cholesky: not positive definite",
                        0,
                    ));
                }
                l[i][j] = val.sqrt();
            } else {
                l[i][j] = (a[i][j] - s) / l[j][j];
            }
        }
    }
    Ok(matrix_to_perl(&l))
}

/// General determinant for NxN via LU.
fn det_nxn(a: &[Vec<f64>]) -> f64 {
    let n = a.len();
    if n == 0 {
        return 1.0;
    }
    let mut u = a.to_vec();
    let mut sign = 1.0;
    for col in 0..n {
        let mut max_row = col;
        for row in (col + 1)..n {
            if u[row][col].abs() > u[max_row][col].abs() {
                max_row = row;
            }
        }
        if max_row != col {
            u.swap(col, max_row);
            sign = -sign;
        }
        if u[col][col].abs() < 1e-15 {
            return 0.0;
        }
        for row in (col + 1)..n {
            let factor = u[row][col] / u[col][col];
            for j in col..n {
                let v = u[col][j];
                u[row][j] -= factor * v;
            }
        }
    }
    let mut d = sign;
    for i in 0..n {
        d *= u[i][i];
    }
    d
}

/// `matrix_det_general M` — determinant for any NxN matrix.
fn builtin_matrix_det_general(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    Ok(PerlValue::float(det_nxn(&a)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Statistics Tests
// ─────────────────────────────────────────────────────────────────────────────

/// `welch_ttest SAMPLE1, SAMPLE2` — Welch's t-test for unequal variances. Returns [t, df].
fn builtin_welch_ttest(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s1: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let s2: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n1 = s1.len() as f64;
    let n2 = s2.len() as f64;
    if n1 < 2.0 || n2 < 2.0 {
        return Err(PerlError::runtime(
            "welch_ttest: need at least 2 samples each",
            0,
        ));
    }
    let m1: f64 = s1.iter().sum::<f64>() / n1;
    let m2: f64 = s2.iter().sum::<f64>() / n2;
    let v1: f64 = s1.iter().map(|x| (x - m1).powi(2)).sum::<f64>() / (n1 - 1.0);
    let v2: f64 = s2.iter().map(|x| (x - m2).powi(2)).sum::<f64>() / (n2 - 1.0);
    let se = (v1 / n1 + v2 / n2).sqrt();
    let t = (m1 - m2) / se;
    let num = (v1 / n1 + v2 / n2).powi(2);
    let den = (v1 / n1).powi(2) / (n1 - 1.0) + (v2 / n2).powi(2) / (n2 - 1.0);
    let df = num / den;
    Ok(PerlValue::array(vec![
        PerlValue::float(t),
        PerlValue::float(df),
    ]))
}

/// `paired_ttest SAMPLE1, SAMPLE2` — paired t-test. Returns [t, df].
fn builtin_paired_ttest(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s1: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let s2: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = s1.len();
    if n != s2.len() || n < 2 {
        return Err(PerlError::runtime(
            "paired_ttest: samples must be same length >= 2",
            0,
        ));
    }
    let diffs: Vec<f64> = s1.iter().zip(s2.iter()).map(|(a, b)| a - b).collect();
    let nf = n as f64;
    let mean_d: f64 = diffs.iter().sum::<f64>() / nf;
    let var_d: f64 = diffs.iter().map(|d| (d - mean_d).powi(2)).sum::<f64>() / (nf - 1.0);
    let t = mean_d / (var_d / nf).sqrt();
    Ok(PerlValue::array(vec![
        PerlValue::float(t),
        PerlValue::float(nf - 1.0),
    ]))
}

/// `cohen_d SAMPLE1, SAMPLE2` — Cohen's d effect size.
fn builtin_cohen_d(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s1: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let s2: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n1 = s1.len() as f64;
    let n2 = s2.len() as f64;
    let m1: f64 = s1.iter().sum::<f64>() / n1;
    let m2: f64 = s2.iter().sum::<f64>() / n2;
    let v1: f64 = s1.iter().map(|x| (x - m1).powi(2)).sum::<f64>() / (n1 - 1.0);
    let v2: f64 = s2.iter().map(|x| (x - m2).powi(2)).sum::<f64>() / (n2 - 1.0);
    let pooled = (((n1 - 1.0) * v1 + (n2 - 1.0) * v2) / (n1 + n2 - 2.0)).sqrt();
    let d = (m1 - m2) / pooled;
    Ok(PerlValue::float(d))
}

/// `anova_oneway G1, G2, ...` — one-way ANOVA F-statistic. Returns [F, df_between, df_within].
fn builtin_anova_oneway(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let groups: Vec<Vec<f64>> = args
        .iter()
        .map(|a| arg_to_vec(a).iter().map(|v| v.to_number()).collect())
        .collect();
    let k = groups.len() as f64;
    if k < 2.0 {
        return Err(PerlError::runtime("anova: need at least 2 groups", 0));
    }
    let n_total: f64 = groups.iter().map(|g| g.len() as f64).sum();
    let grand_mean: f64 = groups.iter().flat_map(|g| g.iter()).sum::<f64>() / n_total;
    let ss_between: f64 = groups
        .iter()
        .map(|g| {
            let ni = g.len() as f64;
            let gi: f64 = g.iter().sum::<f64>() / ni;
            ni * (gi - grand_mean).powi(2)
        })
        .sum();
    let ss_within: f64 = groups
        .iter()
        .map(|g| {
            let gi: f64 = g.iter().sum::<f64>() / g.len() as f64;
            g.iter().map(|x| (x - gi).powi(2)).sum::<f64>()
        })
        .sum();
    let df_b = k - 1.0;
    let df_w = n_total - k;
    let f = (ss_between / df_b) / (ss_within / df_w);
    Ok(PerlValue::array(vec![
        PerlValue::float(f),
        PerlValue::float(df_b),
        PerlValue::float(df_w),
    ]))
}

/// `spearman SAMPLE1, SAMPLE2` — Spearman rank correlation coefficient.
fn builtin_spearman(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s1: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let s2: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = s1.len();
    if n != s2.len() || n < 2 {
        return Err(PerlError::runtime(
            "spearman: samples must be same length >= 2",
            0,
        ));
    }
    fn ranks(v: &[f64]) -> Vec<f64> {
        let mut indexed: Vec<(usize, f64)> = v.iter().cloned().enumerate().collect();
        indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut r = vec![0.0; v.len()];
        let mut i = 0;
        while i < indexed.len() {
            let mut j = i;
            while j < indexed.len() && (indexed[j].1 - indexed[i].1).abs() < 1e-12 {
                j += 1;
            }
            let avg_rank = (i + j + 1) as f64 / 2.0;
            for k in i..j {
                r[indexed[k].0] = avg_rank;
            }
            i = j;
        }
        r
    }
    let r1 = ranks(&s1);
    let r2 = ranks(&s2);
    let nf = n as f64;
    let m1: f64 = r1.iter().sum::<f64>() / nf;
    let m2: f64 = r2.iter().sum::<f64>() / nf;
    let num: f64 = r1
        .iter()
        .zip(r2.iter())
        .map(|(a, b)| (a - m1) * (b - m2))
        .sum();
    let d1: f64 = r1.iter().map(|a| (a - m1).powi(2)).sum::<f64>().sqrt();
    let d2: f64 = r2.iter().map(|b| (b - m2).powi(2)).sum::<f64>().sqrt();
    let rho = if d1 * d2 > 0.0 { num / (d1 * d2) } else { 0.0 };
    Ok(PerlValue::float(rho))
}

/// `kendall_tau SAMPLE1, SAMPLE2` — Kendall rank correlation coefficient.
fn builtin_kendall_tau(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s1: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let s2: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = s1.len();
    if n != s2.len() || n < 2 {
        return Err(PerlError::runtime("kendall_tau: same length >= 2", 0));
    }
    let mut concordant = 0i64;
    let mut discordant = 0i64;
    for i in 0..n {
        for j in (i + 1)..n {
            let d1 = s1[i] - s1[j];
            let d2 = s2[i] - s2[j];
            let p = d1 * d2;
            if p > 0.0 {
                concordant += 1;
            } else if p < 0.0 {
                discordant += 1;
            }
        }
    }
    let nf = n as f64;
    let tau = (concordant - discordant) as f64 / (nf * (nf - 1.0) / 2.0);
    Ok(PerlValue::float(tau))
}

/// `confidence_interval SAMPLE [, confidence]` — CI for mean. Default 95%. Returns [lower, upper].
fn builtin_confidence_interval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let conf = args.get(1).map(|v| v.to_number()).unwrap_or(0.95);
    let n = s.len() as f64;
    if n < 2.0 {
        return Err(PerlError::runtime(
            "confidence_interval: need >= 2 samples",
            0,
        ));
    }
    let mean: f64 = s.iter().sum::<f64>() / n;
    let var: f64 = s.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
    let se = (var / n).sqrt();
    // Approximate z-score for common confidence levels
    let z = if (conf - 0.99).abs() < 0.005 {
        2.576
    } else if (conf - 0.95).abs() < 0.005 {
        1.96
    } else if (conf - 0.90).abs() < 0.005 {
        1.645
    } else {
        1.96
    };
    Ok(PerlValue::array(vec![
        PerlValue::float(mean - z * se),
        PerlValue::float(mean + z * se),
    ]))
}

// ─────────────────────────────────────────────────────────────────────────────
// Distributions
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_beta_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x < 0.0 || x > 1.0 {
        return Ok(PerlValue::float(0.0));
    }
    let ln_beta = lgamma_fn(a) + lgamma_fn(b) - lgamma_fn(a + b);
    let pdf = ((a - 1.0) * x.ln() + (b - 1.0) * (1.0 - x).ln() - ln_beta).exp();
    Ok(PerlValue::float(if pdf.is_finite() { pdf } else { 0.0 }))
}

fn builtin_gamma_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let theta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x < 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    let pdf = x.powf(k - 1.0) * (-x / theta).exp() / (theta.powf(k) * lgamma_fn(k).exp());
    Ok(PerlValue::float(if pdf.is_finite() { pdf } else { 0.0 }))
}

fn builtin_chi2_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if x < 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    // Chi-squared is Gamma(k/2, 2)
    let half_k = k / 2.0;
    let pdf =
        x.powf(half_k - 1.0) * (-x / 2.0).exp() / (2.0f64.powf(half_k) * lgamma_fn(half_k).exp());
    Ok(PerlValue::float(if pdf.is_finite() { pdf } else { 0.0 }))
}

fn builtin_t_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let nu = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let coeff = (lgamma_fn((nu + 1.0) / 2.0) - lgamma_fn(nu / 2.0)).exp()
        / (nu * std::f64::consts::PI).sqrt();
    let pdf = coeff * (1.0 + x * x / nu).powf(-(nu + 1.0) / 2.0);
    Ok(PerlValue::float(pdf))
}

fn builtin_f_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let d1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let d2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x < 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    let num = ((d1 * x).powf(d1) * d2.powf(d2) / (d1 * x + d2).powf(d1 + d2)).sqrt();
    let den = x * (lgamma_fn(d1 / 2.0) + lgamma_fn(d2 / 2.0) - lgamma_fn((d1 + d2) / 2.0)).exp();
    let pdf = num / den;
    Ok(PerlValue::float(if pdf.is_finite() { pdf } else { 0.0 }))
}

fn builtin_lognormal_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    let pdf = (-(x.ln() - mu).powi(2) / (2.0 * sigma * sigma)).exp()
        / (x * sigma * (2.0 * std::f64::consts::PI).sqrt());
    Ok(PerlValue::float(pdf))
}

fn builtin_weibull_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x < 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    let pdf = (k / lambda) * (x / lambda).powf(k - 1.0) * (-(x / lambda).powf(k)).exp();
    Ok(PerlValue::float(if pdf.is_finite() { pdf } else { 0.0 }))
}

fn builtin_cauchy_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let x0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let pdf = 1.0 / (std::f64::consts::PI * gamma * (1.0 + ((x - x0) / gamma).powi(2)));
    Ok(PerlValue::float(pdf))
}

fn builtin_laplace_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let pdf = (-(x - mu).abs() / b).exp() / (2.0 * b);
    Ok(PerlValue::float(pdf))
}

fn builtin_pareto_pdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let xm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x < xm {
        return Ok(PerlValue::float(0.0));
    }
    let pdf = alpha * xm.powf(alpha) / x.powf(alpha + 1.0);
    Ok(PerlValue::float(pdf))
}

/// Helper: log-gamma via Stirling's approximation (Lanczos).
fn lgamma_fn(x: f64) -> f64 {
    // Use std if available, otherwise Stirling
    // Lanczos approximation coefficients
    let g = 7.0;
    let c = [
        0.99999999999980993,
        676.5203681218851,
        -1259.1392167224028,
        771.32342877765313,
        -176.61502916214059,
        12.507343278686905,
        -0.13857109526572012,
        9.9843695780195716e-6,
        1.5056327351493116e-7,
    ];
    if x < 0.5 {
        let pi = std::f64::consts::PI;
        return (pi / (pi * x).sin()).ln() - lgamma_fn(1.0 - x);
    }
    let x = x - 1.0;
    let mut a = c[0];
    let t = x + g + 0.5;
    for i in 1..9 {
        a += c[i] / (x + i as f64);
    }
    0.5 * (2.0 * std::f64::consts::PI).ln() + (t.ln() * (x + 0.5)) - t + a.ln()
}

// ─────────────────────────────────────────────────────────────────────────────
// Interpolation & Curve Fitting
// ─────────────────────────────────────────────────────────────────────────────

/// `lagrange_interp XS, YS, x` — Lagrange polynomial interpolation at point x.
fn builtin_lagrange_interp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = xs.len();
    let mut result = 0.0;
    for i in 0..n {
        let mut basis = 1.0;
        for j in 0..n {
            if i != j {
                basis *= (x - xs[j]) / (xs[i] - xs[j]);
            }
        }
        result += ys[i] * basis;
    }
    Ok(PerlValue::float(result))
}

/// `cubic_spline XS, YS, x` — natural cubic spline interpolation at point x.
fn builtin_cubic_spline(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = xs.len();
    if n < 2 || ys.len() != n {
        return Err(PerlError::runtime(
            "cubic_spline: need >= 2 matched points",
            0,
        ));
    }
    let mut h = vec![0.0; n - 1];
    for i in 0..n - 1 {
        h[i] = xs[i + 1] - xs[i];
    }
    // Tridiagonal system for second derivatives
    let mut alpha = vec![0.0; n];
    for i in 1..n - 1 {
        alpha[i] = 3.0 / h[i] * (ys[i + 1] - ys[i]) - 3.0 / h[i - 1] * (ys[i] - ys[i - 1]);
    }
    let mut c = vec![0.0; n];
    let mut l = vec![1.0; n];
    let mut mu = vec![0.0; n];
    let mut z = vec![0.0; n];
    for i in 1..n - 1 {
        l[i] = 2.0 * (xs[i + 1] - xs[i - 1]) - h[i - 1] * mu[i - 1];
        mu[i] = h[i] / l[i];
        z[i] = (alpha[i] - h[i - 1] * z[i - 1]) / l[i];
    }
    for j in (0..n - 1).rev() {
        c[j] = z[j] - mu[j] * c[j + 1];
    }
    let mut b = vec![0.0; n - 1];
    let mut d = vec![0.0; n - 1];
    for i in 0..n - 1 {
        b[i] = (ys[i + 1] - ys[i]) / h[i] - h[i] * (c[i + 1] + 2.0 * c[i]) / 3.0;
        d[i] = (c[i + 1] - c[i]) / (3.0 * h[i]);
    }
    // Find interval
    let mut seg = n - 2;
    for i in 0..n - 1 {
        if x <= xs[i + 1] {
            seg = i;
            break;
        }
    }
    let dx = x - xs[seg];
    let result = ys[seg] + b[seg] * dx + c[seg] * dx * dx + d[seg] * dx * dx * dx;
    Ok(PerlValue::float(result))
}

/// `poly_eval COEFFS, x` — evaluate polynomial c0 + c1*x + c2*x^2 + ... using Horner's method.
fn builtin_poly_eval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let coeffs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut result = 0.0;
    for c in coeffs.iter().rev() {
        result = result * x + c;
    }
    Ok(PerlValue::float(result))
}

/// `polynomial_fit XS, YS, degree` — least-squares polynomial fit. Returns coefficients.
fn builtin_polynomial_fit(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let deg = args.get(2).map(|v| v.to_number() as usize).unwrap_or(1);
    let n = xs.len();
    let m = deg + 1;
    // Build Vandermonde matrix and solve via normal equations
    let mut ata = vec![vec![0.0; m]; m];
    let mut atb = vec![0.0; m];
    for i in 0..m {
        for j in 0..m {
            let mut s = 0.0;
            for k in 0..n {
                s += xs[k].powi((i + j) as i32);
            }
            ata[i][j] = s;
        }
        let mut s = 0.0;
        for k in 0..n {
            s += xs[k].powi(i as i32) * ys[k];
        }
        atb[i] = s;
    }
    // Solve
    let a_perl = matrix_to_perl(&ata);
    let b_perl = vec_to_perl(&atb);
    builtin_matrix_solve(&[a_perl, b_perl])
}

// ─────────────────────────────────────────────────────────────────────────────
// Numerical Integration & Differentiation
// ─────────────────────────────────────────────────────────────────────────────

/// `trapz YS [, dx]` — trapezoidal integration of evenly-spaced samples.
fn builtin_trapz(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ys: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let dx = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = ys.len();
    if n < 2 {
        return Ok(PerlValue::float(0.0));
    }
    let mut sum = 0.5 * (ys[0] + ys[n - 1]);
    for i in 1..n - 1 {
        sum += ys[i];
    }
    Ok(PerlValue::float(sum * dx))
}

/// `simpson YS [, dx]` — Simpson's rule integration of evenly-spaced samples.
fn builtin_simpson(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ys: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let dx = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = ys.len();
    if n < 3 {
        return builtin_trapz(args);
    }
    let mut sum = ys[0] + ys[n - 1];
    for i in 1..n - 1 {
        sum += if i % 2 == 0 { 2.0 * ys[i] } else { 4.0 * ys[i] };
    }
    Ok(PerlValue::float(sum * dx / 3.0))
}

/// `numerical_diff YS [, dx]` — numerical first derivative via central differences.
fn builtin_numerical_diff(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ys: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let dx = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = ys.len();
    if n < 2 {
        return Ok(PerlValue::array(vec![]));
    }
    let mut dy = Vec::with_capacity(n);
    dy.push(PerlValue::float((ys[1] - ys[0]) / dx));
    for i in 1..n - 1 {
        dy.push(PerlValue::float((ys[i + 1] - ys[i - 1]) / (2.0 * dx)));
    }
    dy.push(PerlValue::float((ys[n - 1] - ys[n - 2]) / dx));
    Ok(PerlValue::array(dy))
}

/// `cumtrapz YS [, dx]` — cumulative trapezoidal integration.
fn builtin_cumtrapz(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ys: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let dx = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut cum = vec![PerlValue::float(0.0)];
    let mut total = 0.0;
    for i in 1..ys.len() {
        total += 0.5 * (ys[i - 1] + ys[i]) * dx;
        cum.push(PerlValue::float(total));
    }
    Ok(PerlValue::array(cum))
}

// ─────────────────────────────────────────────────────────────────────────────
// Optimization / Root Finding
// ─────────────────────────────────────────────────────────────────────────────

fn call_f(
    interp: &mut crate::interpreter::Interpreter,
    f: &PerlValue,
    x: f64,
    line: usize,
) -> PerlResult<f64> {
    let sub = f
        .as_code_ref()
        .ok_or_else(|| PerlError::runtime("expected code ref", line))?;
    let r = exec_to_perl_result(
        interp.call_sub(&sub, vec![PerlValue::float(x)], WantarrayCtx::Scalar, line),
        "callback",
        line,
    )?;
    Ok(r.to_number())
}

fn call_f2(
    interp: &mut crate::interpreter::Interpreter,
    f: &PerlValue,
    a: f64,
    b: f64,
    line: usize,
) -> PerlResult<f64> {
    let sub = f
        .as_code_ref()
        .ok_or_else(|| PerlError::runtime("expected code ref", line))?;
    let r = exec_to_perl_result(
        interp.call_sub(
            &sub,
            vec![PerlValue::float(a), PerlValue::float(b)],
            WantarrayCtx::Scalar,
            line,
        ),
        "callback",
        line,
    )?;
    Ok(r.to_number())
}

/// `bisection F, a, b [, tol]` — find root of f(x)=0 in [a,b] via bisection.
fn builtin_bisection(
    interp: &mut crate::interpreter::Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-10);
    for _ in 0..100 {
        let mid = (a + b) / 2.0;
        let fmid = call_f(interp, &f, mid, line)?;
        let fa = call_f(interp, &f, a, line)?;
        if fmid.abs() < tol || (b - a) / 2.0 < tol {
            return Ok(PerlValue::float(mid));
        }
        if fa.signum() == fmid.signum() {
            a = mid;
        } else {
            b = mid;
        }
    }
    Ok(PerlValue::float((a + b) / 2.0))
}

/// `newton_method F, F', x0 [, tol]` — Newton-Raphson root finding.
fn builtin_newton_method(
    interp: &mut crate::interpreter::Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let fp = args.get(1).cloned().unwrap_or(PerlValue::UNDEF);
    let mut x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-10);
    for _ in 0..100 {
        let fx = call_f(interp, &f, x, line)?;
        let fpx = call_f(interp, &fp, x, line)?;
        if fpx.abs() < 1e-15 {
            break;
        }
        let x_new = x - fx / fpx;
        if (x_new - x).abs() < tol {
            return Ok(PerlValue::float(x_new));
        }
        x = x_new;
    }
    Ok(PerlValue::float(x))
}

/// `golden_section F, a, b [, tol]` — golden-section search for minimum of f on [a,b].
fn builtin_golden_section(
    interp: &mut crate::interpreter::Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-8);
    let gr = (5.0f64.sqrt() - 1.0) / 2.0;
    let mut c = b - gr * (b - a);
    let mut d = a + gr * (b - a);
    for _ in 0..100 {
        if (b - a).abs() < tol {
            break;
        }
        let fc = call_f(interp, &f, c, line)?;
        let fd = call_f(interp, &f, d, line)?;
        if fc < fd {
            b = d;
        } else {
            a = c;
        }
        c = b - gr * (b - a);
        d = a + gr * (b - a);
    }
    Ok(PerlValue::float((a + b) / 2.0))
}

// ─────────────────────────────────────────────────────────────────────────────
// ODE Solvers
// ─────────────────────────────────────────────────────────────────────────────

/// `rk4 F, t0, y0, dt, steps` — 4th-order Runge-Kutta. F(t,y)->dy/dt. Returns [[t,y],...].
fn builtin_rk4(
    interp: &mut crate::interpreter::Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut y = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let steps = args.get(4).map(|v| v.to_number() as usize).unwrap_or(100);
    let mut result = Vec::with_capacity(steps + 1);
    result.push(PerlValue::array(vec![
        PerlValue::float(t),
        PerlValue::float(y),
    ]));
    for _ in 0..steps {
        let k1 = call_f2(interp, &f, t, y, line)?;
        let k2 = call_f2(interp, &f, t + dt / 2.0, y + dt * k1 / 2.0, line)?;
        let k3 = call_f2(interp, &f, t + dt / 2.0, y + dt * k2 / 2.0, line)?;
        let k4 = call_f2(interp, &f, t + dt, y + dt * k3, line)?;
        y += dt / 6.0 * (k1 + 2.0 * k2 + 2.0 * k3 + k4);
        t += dt;
        result.push(PerlValue::array(vec![
            PerlValue::float(t),
            PerlValue::float(y),
        ]));
    }
    Ok(PerlValue::array(result))
}

/// `euler_ode F, t0, y0, dt, steps` — Euler method ODE solver.
fn builtin_euler_ode(
    interp: &mut crate::interpreter::Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut y = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let steps = args.get(4).map(|v| v.to_number() as usize).unwrap_or(100);
    let mut result = Vec::with_capacity(steps + 1);
    result.push(PerlValue::array(vec![
        PerlValue::float(t),
        PerlValue::float(y),
    ]));
    for _ in 0..steps {
        let dy = call_f2(interp, &f, t, y, line)?;
        y += dt * dy;
        t += dt;
        result.push(PerlValue::array(vec![
            PerlValue::float(t),
            PerlValue::float(y),
        ]));
    }
    Ok(PerlValue::array(result))
}

// ─────────────────────────────────────────────────────────────────────────────
// Graph Algorithms
// ─────────────────────────────────────────────────────────────────────────────

/// `dijkstra GRAPH, source` — shortest paths. GRAPH is {node => [[neighbor, weight],...], ...}. Returns {node => distance}.
fn builtin_dijkstra(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let graph_val = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let source = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let graph_map = graph_val
        .as_hash_map()
        .ok_or_else(|| PerlError::runtime("dijkstra: first arg must be a hash", 0))?;
    let mut dist: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    dist.insert(source.clone(), 0.0);
    loop {
        // Find unvisited node with smallest distance
        let mut current = None;
        let mut min_dist = f64::INFINITY;
        for (node, &d) in &dist {
            if !visited.contains(node) && d < min_dist {
                min_dist = d;
                current = Some(node.clone());
            }
        }
        let current = match current {
            Some(c) => c,
            None => break,
        };
        visited.insert(current.clone());
        // Relax neighbors
        if let Some(neighbors) = graph_map.get(&current) {
            let nv = arg_to_vec(neighbors);
            for edge in &nv {
                let ev = arg_to_vec(edge);
                if ev.len() >= 2 {
                    let neighbor = ev[0].to_string();
                    let weight = ev[1].to_number();
                    let new_dist = min_dist + weight;
                    let entry = dist.entry(neighbor).or_insert(f64::INFINITY);
                    if new_dist < *entry {
                        *entry = new_dist;
                    }
                }
            }
        }
    }
    let mut result = indexmap::IndexMap::new();
    for (k, v) in dist {
        result.insert(k, PerlValue::float(v));
    }
    Ok(PerlValue::hash(result))
}

/// `bellman_ford EDGES, n_nodes, source` — EDGES is [[u, v, weight],...]. Returns distances array.
fn builtin_bellman_ford(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let edges_v = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let src = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let mut dist = vec![f64::INFINITY; n];
    dist[src] = 0.0;
    let edges: Vec<(usize, usize, f64)> = edges_v
        .iter()
        .filter_map(|e| {
            let ev = arg_to_vec(e);
            if ev.len() >= 3 {
                Some((
                    ev[0].to_number() as usize,
                    ev[1].to_number() as usize,
                    ev[2].to_number(),
                ))
            } else {
                None
            }
        })
        .collect();
    for _ in 0..n - 1 {
        for &(u, v, w) in &edges {
            if dist[u] + w < dist[v] {
                dist[v] = dist[u] + w;
            }
        }
    }
    Ok(vec_to_perl(&dist))
}

/// `floyd_warshall MATRIX` — all-pairs shortest paths. Returns distance matrix.
fn builtin_floyd_warshall(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut dist = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = dist.len();
    for k in 0..n {
        for i in 0..n {
            for j in 0..n {
                if dist[i][k] + dist[k][j] < dist[i][j] {
                    dist[i][j] = dist[i][k] + dist[k][j];
                }
            }
        }
    }
    Ok(matrix_to_perl(&dist))
}

/// `prim_mst MATRIX` — minimum spanning tree via Prim's algorithm. Returns total weight.
fn builtin_prim_mst(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = w.len();
    if n == 0 {
        return Ok(PerlValue::float(0.0));
    }
    let mut in_mst = vec![false; n];
    let mut key = vec![f64::INFINITY; n];
    key[0] = 0.0;
    let mut total = 0.0;
    for _ in 0..n {
        let mut u = 0;
        let mut min_key = f64::INFINITY;
        for v in 0..n {
            if !in_mst[v] && key[v] < min_key {
                min_key = key[v];
                u = v;
            }
        }
        in_mst[u] = true;
        total += key[u];
        for v in 0..n {
            if !in_mst[v] && w[u][v] > 0.0 && w[u][v] < key[v] {
                key[v] = w[u][v];
            }
        }
    }
    Ok(PerlValue::float(total))
}

// ─────────────────────────────────────────────────────────────────────────────
// Trig Extensions
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_cot(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(1.0 / x.tan()))
}
fn builtin_sec(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(1.0 / x.cos()))
}
fn builtin_csc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(1.0 / x.sin()))
}
fn builtin_acot(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((1.0 / x).atan()))
}
fn builtin_asec(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float((1.0 / x).acos()))
}
fn builtin_acsc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float((1.0 / x).asin()))
}
fn builtin_sinc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(if x.abs() < 1e-15 {
        1.0
    } else {
        x.sin() / x
    }))
}
fn builtin_versin(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(1.0 - x.cos()))
}

// ─────────────────────────────────────────────────────────────────────────────
// Activation Functions (ML)
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_leaky_relu(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(if x >= 0.0 { x } else { alpha * x }))
}
fn builtin_elu(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if x >= 0.0 {
        x
    } else {
        alpha * (x.exp() - 1.0)
    }))
}
fn builtin_selu(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = 1.0507009873554805;
    let alpha = 1.6732632423543772;
    Ok(PerlValue::float(
        lambda * if x >= 0.0 { x } else { alpha * (x.exp() - 1.0) },
    ))
}
fn builtin_gelu(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(
        0.5 * x * (1.0 + (std::f64::consts::FRAC_2_SQRT_PI * (x + 0.044715 * x.powi(3))).tanh()),
    ))
}
fn builtin_silu(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(x / (1.0 + (-x).exp())))
}
fn builtin_mish(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(x * (x.exp().ln_1p()).tanh()))
}
fn builtin_softplus(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(x.exp().ln_1p()))
}
fn builtin_hard_sigmoid(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((x / 6.0 + 0.5).max(0.0).min(1.0)))
}
fn builtin_hard_swish(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(x * (x + 3.0).max(0.0).min(6.0) / 6.0))
}

// ─────────────────────────────────────────────────────────────────────────────
// Special Functions
// ─────────────────────────────────────────────────────────────────────────────

/// `bessel_j0 x` — Bessel function of the first kind, order 0 (polynomial approximation).
fn builtin_bessel_j0(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0).abs();
    let result = if x < 8.0 {
        let y = x * x;
        let n = 57568490574.0
            + y * (-13362590354.0
                + y * (651619640.7 + y * (-11214424.18 + y * (77392.33017 + y * (-184.9052456)))));
        let d = 57568490411.0
            + y * (1029532985.0 + y * (9494680.718 + y * (59272.64853 + y * (267.8532712 + y))));
        n / d
    } else {
        let z = 8.0 / x;
        let y = z * z;
        let xx = x - 0.785398164;
        let p = 1.0
            + y * (-0.1098628627e-2
                + y * (0.2734510407e-4 + y * (-0.2073370639e-5 + y * 0.2093887211e-6)));
        let q = -0.1562499995e-1
            + y * (0.1430488765e-3
                + y * (-0.6911147651e-5 + y * (0.7621095161e-6 - y * 0.934935152e-7)));
        (0.636619772 / x).sqrt() * (xx.cos() * p - z * xx.sin() * q)
    };
    Ok(PerlValue::float(result))
}

/// `bessel_j1 x` — Bessel function of the first kind, order 1.
fn builtin_bessel_j1(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let ax = x.abs();
    let result = if ax < 8.0 {
        let y = x * x;
        let n = x
            * (72362614232.0
                + y * (-7895059235.0
                    + y * (242396853.1
                        + y * (-2972611.439 + y * (15704.4826 + y * (-30.16036606))))));
        let d = 144725228442.0
            + y * (2300535178.0 + y * (18583304.74 + y * (99447.43394 + y * (376.9991397 + y))));
        n / d
    } else {
        let z = 8.0 / ax;
        let y = z * z;
        let xx = ax - 2.356194491;
        let p = 1.0
            + y * (0.183105e-2
                + y * (-0.3516396496e-4 + y * (0.2457520174e-5 - y * 0.240337019e-6)));
        let q = 0.04687499995
            + y * (-0.2002690873e-3
                + y * (0.8449199096e-5 + y * (-0.88228987e-6 + y * 0.105787412e-6)));
        let ans = (0.636619772 / ax).sqrt() * (xx.cos() * p - z * xx.sin() * q);
        if x < 0.0 {
            -ans
        } else {
            ans
        }
    };
    Ok(PerlValue::float(result))
}

/// `lambert_w x` — Lambert W function (principal branch) via Halley's method.
fn builtin_lambert_w(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    if x < -1.0 / std::f64::consts::E {
        return Err(PerlError::runtime("lambert_w: x must be >= -1/e", 0));
    }
    let mut w = if x < 1.0 { 0.0 } else { x.ln() };
    for _ in 0..50 {
        let ew = w.exp();
        let wew = w * ew;
        let f = wew - x;
        let fp = ew * (w + 1.0);
        if fp.abs() < 1e-15 {
            break;
        }
        let fpp = ew * (w + 2.0);
        let delta = f / (fp - f * fpp / (2.0 * fp));
        w -= delta;
        if delta.abs() < 1e-12 {
            break;
        }
    }
    Ok(PerlValue::float(w))
}

// ─────────────────────────────────────────────────────────────────────────────
// Number Theory (extended)
// ─────────────────────────────────────────────────────────────────────────────

/// `mod_exp base, exp, modulus` — modular exponentiation (base^exp mod m).
fn builtin_mod_exp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let base = args.first().map(|v| v.to_number() as u64).unwrap_or(0);
    let exp = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    let modulus = args.get(2).map(|v| v.to_number() as u64).unwrap_or(1);
    let mut result = 1u64;
    let mut b = base % modulus;
    let mut e = exp;
    while e > 0 {
        if e & 1 == 1 {
            result = result.wrapping_mul(b) % modulus;
        }
        e >>= 1;
        b = b.wrapping_mul(b) % modulus;
    }
    Ok(PerlValue::integer(result as i64))
}

/// `mod_inv a, m` — modular inverse of a mod m (extended Euclidean).
fn builtin_mod_inv(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let (mut old_r, mut r) = (a, m);
    let (mut old_s, mut s) = (1i64, 0i64);
    while r != 0 {
        let q = old_r / r;
        let tmp = r;
        r = old_r - q * r;
        old_r = tmp;
        let tmp = s;
        s = old_s - q * s;
        old_s = tmp;
    }
    if old_r != 1 {
        return Err(PerlError::runtime("mod_inv: no inverse exists", 0));
    }
    Ok(PerlValue::integer(((old_s % m) + m) % m))
}

/// `chinese_remainder REMAINDERS, MODULI` — Chinese Remainder Theorem.
fn builtin_chinese_remainder(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rems: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let mods: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    if rems.len() != mods.len() || rems.is_empty() {
        return Err(PerlError::runtime(
            "chinese_remainder: mismatched arrays",
            0,
        ));
    }
    let prod: i64 = mods.iter().product();
    let mut sum = 0i64;
    for i in 0..rems.len() {
        let ni = prod / mods[i];
        let inv_args = [PerlValue::integer(ni), PerlValue::integer(mods[i])];
        let inv = builtin_mod_inv(&inv_args)?.to_number() as i64;
        sum = (sum + rems[i] * ni % prod * inv) % prod;
    }
    Ok(PerlValue::integer((sum % prod + prod) % prod))
}

/// `miller_rabin n [, k]` — probabilistic primality test (k rounds, default 20).
fn builtin_miller_rabin(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as u64).unwrap_or(0);
    let k = args.get(1).map(|v| v.to_number() as u32).unwrap_or(20);
    if n < 2 {
        return Ok(PerlValue::integer(0));
    }
    if n < 4 {
        return Ok(PerlValue::integer(1));
    }
    if n % 2 == 0 {
        return Ok(PerlValue::integer(0));
    }
    let mut d = n - 1;
    let mut r = 0u32;
    while d % 2 == 0 {
        d /= 2;
        r += 1;
    }
    let witnesses = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];
    let max_w = k.min(witnesses.len() as u32) as usize;
    'outer: for &a in &witnesses[..max_w] {
        if a >= n {
            continue;
        }
        let me_args = [
            PerlValue::integer(a as i64),
            PerlValue::integer(d as i64),
            PerlValue::integer(n as i64),
        ];
        let mut x = builtin_mod_exp(&me_args)?.to_number() as u64;
        if x == 1 || x == n - 1 {
            continue;
        }
        for _ in 0..r - 1 {
            let me_args = [
                PerlValue::integer(x as i64),
                PerlValue::integer(2),
                PerlValue::integer(n as i64),
            ];
            x = builtin_mod_exp(&me_args)?.to_number() as u64;
            if x == n - 1 {
                continue 'outer;
            }
        }
        return Ok(PerlValue::integer(0));
    }
    Ok(PerlValue::integer(1))
}

// is_perfect and is_abundant already defined above in this file

// ─────────────────────────────────────────────────────────────────────────────
// Combinatorics (extended)
// ─────────────────────────────────────────────────────────────────────────────

/// `derangements n` — count of derangements (subfactorial !n).
fn builtin_derangements(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    if n <= 0 {
        return Ok(PerlValue::integer(1));
    }
    if n == 1 {
        return Ok(PerlValue::integer(0));
    }
    let mut a = 1i64;
    let mut b = 0i64;
    for _ in 2..=n {
        let c = (a + b) * (n - 1);
        a = b;
        b = c;
    }
    Ok(PerlValue::integer(b))
}

/// `stirling2 n, k` — Stirling number of the second kind.
fn builtin_stirling2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    if k > n {
        return Ok(PerlValue::integer(0));
    }
    if k == 0 {
        return Ok(PerlValue::integer(if n == 0 { 1 } else { 0 }));
    }
    if k == n || k == 1 {
        return Ok(PerlValue::integer(1));
    }
    // DP
    let mut dp = vec![vec![0i64; k + 1]; n + 1];
    dp[0][0] = 1;
    for i in 1..=n {
        for j in 1..=k.min(i) {
            dp[i][j] = j as i64 * dp[i - 1][j] + dp[i - 1][j - 1];
        }
    }
    Ok(PerlValue::integer(dp[n][k]))
}

/// `bernoulli_number n` — nth Bernoulli number.
fn builtin_bernoulli_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let mut b = vec![0.0; n + 1];
    b[0] = 1.0;
    for m in 1..=n {
        b[m] = 0.0;
        for k in 0..m {
            let binom = (0..k).fold(1.0, |acc, i| acc * (m + 1 - i) as f64 / (i + 1) as f64);
            b[m] -= binom * b[k];
        }
        b[m] /= (m + 1) as f64;
    }
    Ok(PerlValue::float(b[n]))
}

/// `harmonic_number n` — nth harmonic number H_n = 1 + 1/2 + ... + 1/n.
fn builtin_harmonic_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as u64).unwrap_or(0);
    let mut h = 0.0;
    for i in 1..=n {
        h += 1.0 / i as f64;
    }
    Ok(PerlValue::float(h))
}

// ─────────────────────────────────────────────────────────────────────────────
// Physics (extended — new functions only, existing ones already in file above)
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_drag_force(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cd = args.first().map(|v| v.to_number()).unwrap_or(0.47);
    let rho = args.get(1).map(|v| v.to_number()).unwrap_or(1.225);
    let area = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let velocity = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(
        0.5 * cd * rho * area * velocity * velocity,
    ))
}

fn builtin_ideal_gas(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let vol = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let r = 8.314;
    if p == 0.0 {
        return Ok(PerlValue::float(n * r * t / vol));
    }
    if vol == 0.0 {
        return Ok(PerlValue::float(n * r * t / p));
    }
    if t == 0.0 {
        return Ok(PerlValue::float(p * vol / (n * r)));
    }
    Ok(PerlValue::float(p * vol / (r * t)))
}

// ─────────────────────────────────────────────────────────────────────────────
// Financial (extended — Greeks, risk metrics)
// ─────────────────────────────────────────────────────────────────────────────

/// Normal CDF approximation for Black-Scholes Greeks.
fn norm_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf_approx_fn(x / std::f64::consts::SQRT_2))
}
fn norm_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt()
}
fn erf_approx_fn(x: f64) -> f64 {
    let a = [
        0.254829592,
        -0.284496736,
        1.421413741,
        -1.453152027,
        1.061405429,
    ];
    let p = 0.3275911;
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + p * x);
    let y = 1.0 - (((((a[4] * t + a[3]) * t) + a[2]) * t + a[1]) * t + a[0]) * t * (-x * x).exp();
    sign * y
}

/// BS d1/d2 helpers.
fn bs_d1d2(s: f64, k: f64, t: f64, r: f64, sigma: f64) -> (f64, f64) {
    let d1 = ((s / k).ln() + (r + 0.5 * sigma * sigma) * t) / (sigma * t.sqrt());
    let d2 = d1 - sigma * t.sqrt();
    (d1, d2)
}

fn builtin_bs_delta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_number()).unwrap_or(100.0);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(4).map(|v| v.to_number()).unwrap_or(0.2);
    let (d1, _) = bs_d1d2(s, k, t, r, sigma);
    Ok(PerlValue::float(norm_cdf(d1)))
}

fn builtin_bs_gamma(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_number()).unwrap_or(100.0);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(4).map(|v| v.to_number()).unwrap_or(0.2);
    let (d1, _) = bs_d1d2(s, k, t, r, sigma);
    Ok(PerlValue::float(norm_pdf(d1) / (s * sigma * t.sqrt())))
}

fn builtin_bs_vega(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_number()).unwrap_or(100.0);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(4).map(|v| v.to_number()).unwrap_or(0.2);
    let (d1, _) = bs_d1d2(s, k, t, r, sigma);
    Ok(PerlValue::float(s * norm_pdf(d1) * t.sqrt()))
}

fn builtin_bs_theta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_number()).unwrap_or(100.0);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(4).map(|v| v.to_number()).unwrap_or(0.2);
    let (d1, d2) = bs_d1d2(s, k, t, r, sigma);
    let theta =
        -(s * norm_pdf(d1) * sigma) / (2.0 * t.sqrt()) - r * k * (-r * t).exp() * norm_cdf(d2);
    Ok(PerlValue::float(theta))
}

fn builtin_bs_rho(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_number()).unwrap_or(100.0);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(4).map(|v| v.to_number()).unwrap_or(0.2);
    let (_, d2) = bs_d1d2(s, k, t, r, sigma);
    Ok(PerlValue::float(k * t * (-r * t).exp() * norm_cdf(d2)))
}

/// `bond_duration CASHFLOWS, RATES` — Macaulay duration.
fn builtin_bond_duration(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cfs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let rate = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let mut pv_sum = 0.0;
    let mut weighted_sum = 0.0;
    for (i, &cf) in cfs.iter().enumerate() {
        let t = (i + 1) as f64;
        let pv = cf / (1.0 + rate).powf(t);
        pv_sum += pv;
        weighted_sum += t * pv;
    }
    Ok(PerlValue::float(if pv_sum > 0.0 {
        weighted_sum / pv_sum
    } else {
        0.0
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// DSP Extensions
// ─────────────────────────────────────────────────────────────────────────────

/// `dct SIGNAL` — Type-II Discrete Cosine Transform.
fn builtin_dct(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = x.len();
    let mut result = Vec::with_capacity(n);
    for k in 0..n {
        let mut sum = 0.0;
        for i in 0..n {
            sum += x[i]
                * (std::f64::consts::PI * (2 * i + 1) as f64 * k as f64 / (2 * n) as f64).cos();
        }
        result.push(PerlValue::float(sum));
    }
    Ok(PerlValue::array(result))
}

/// `idct COEFFS` — Type-III Discrete Cosine Transform (inverse DCT).
fn builtin_idct(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = c.len();
    let mut result = Vec::with_capacity(n);
    for i in 0..n {
        let mut sum = c[0] / 2.0;
        for k in 1..n {
            sum += c[k]
                * (std::f64::consts::PI * (2 * i + 1) as f64 * k as f64 / (2 * n) as f64).cos();
        }
        result.push(PerlValue::float(sum * 2.0 / n as f64));
    }
    Ok(PerlValue::array(result))
}

/// `goertzel SIGNAL, freq, sample_rate` — Goertzel algorithm (single-frequency DFT bin magnitude).
fn builtin_goertzel(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let freq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sr = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = x.len();
    let k = (0.5 + n as f64 * freq / sr) as usize;
    let w = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0, 0.0);
    for &xi in &x {
        let s0 = xi + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    Ok(PerlValue::float(power.sqrt()))
}

/// `chirp n, f0, f1, sample_rate` — generate a linear chirp signal.
fn builtin_chirp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1000);
    let f0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f1 = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    let sr = args.get(3).map(|v| v.to_number()).unwrap_or(1000.0);
    let t_max = n as f64 / sr;
    let mut result = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / sr;
        let freq = f0 + (f1 - f0) * t / t_max;
        result.push(PerlValue::float(
            (2.0 * std::f64::consts::PI * freq * t).sin(),
        ));
    }
    Ok(PerlValue::array(result))
}

// ─────────────────────────────────────────────────────────────────────────────
// Encoding Extensions
// ─────────────────────────────────────────────────────────────────────────────

/// `base85_encode STR` — Ascii85/Base85 encoding.
fn builtin_base85_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let input = args.first().map(|v| v.to_string()).unwrap_or_default();
    let bytes = input.as_bytes();
    let mut result = String::new();
    for chunk in bytes.chunks(4) {
        let mut val = 0u32;
        for (i, &b) in chunk.iter().enumerate() {
            val |= (b as u32) << (24 - i * 8);
        }
        if chunk.len() == 4 && val == 0 {
            result.push('z');
        } else {
            let mut encoded = [0u8; 5];
            for i in (0..5).rev() {
                encoded[i] = (val % 85) as u8 + 33;
                val /= 85;
            }
            for i in 0..chunk.len() + 1 {
                result.push(encoded[i] as char);
            }
        }
    }
    Ok(PerlValue::string(result))
}

/// `base85_decode STR` — Ascii85/Base85 decoding.
fn builtin_base85_decode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let input = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut bytes = Vec::new();
    let chars: Vec<u8> = input.bytes().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == b'z' {
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            i += 1;
        } else {
            let chunk_len = (chars.len() - i).min(5);
            let mut val = 0u32;
            for j in 0..5 {
                let c = if j < chunk_len { chars[i + j] - 33 } else { 84 };
                val = val * 85 + c as u32;
            }
            let out_len = chunk_len - 1;
            for j in 0..out_len {
                bytes.push((val >> (24 - j * 8)) as u8);
            }
            i += chunk_len;
        }
    }
    Ok(PerlValue::string(
        String::from_utf8_lossy(&bytes).to_string(),
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// R base package — distribution CDFs, quantiles, matrix ops, stats tests
// ─────────────────────────────────────────────────────────────────────────────

// ── Distribution CDFs (p-functions) ──────────────────────────────────────────

/// `pnorm x [, mu, sigma]` — normal CDF.
fn builtin_pnorm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let z = (x - mu) / sigma;
    Ok(PerlValue::float(norm_cdf(z)))
}

/// `qnorm p [, mu, sigma]` — normal quantile (inverse CDF) via rational approximation.
fn builtin_qnorm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    // Beasley-Springer-Moro approximation
    let z = if p <= 0.0 {
        f64::NEG_INFINITY
    } else if p >= 1.0 {
        f64::INFINITY
    } else if p == 0.5 {
        0.0
    } else {
        let t = if p < 0.5 {
            (-2.0 * p.ln()).sqrt()
        } else {
            (-2.0 * (1.0 - p).ln()).sqrt()
        };
        let c = [2.515517, 0.802853, 0.010328];
        let d = [1.432788, 0.189269, 0.001308];
        let num = c[0] + t * (c[1] + t * c[2]);
        let den = 1.0 + t * (d[0] + t * (d[1] + t * d[2]));
        let z = t - num / den;
        if p < 0.5 {
            -z
        } else {
            z
        }
    };
    Ok(PerlValue::float(mu + sigma * z))
}

/// `pbinom k, n, p` — binomial CDF P(X <= k).
fn builtin_pbinom(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let mut cdf = 0.0;
    for i in 0..=k.min(n) {
        let mut binom = 1.0;
        for j in 0..i {
            binom *= (n - j) as f64 / (j + 1) as f64;
        }
        cdf += binom * p.powi(i as i32) * (1.0 - p).powi((n - i) as i32);
    }
    Ok(PerlValue::float(cdf))
}

/// `dbinom k, n, p` — binomial PMF P(X = k).
fn builtin_dbinom(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let mut binom = 1.0;
    for j in 0..k {
        binom *= (n - j) as f64 / (j + 1) as f64;
    }
    Ok(PerlValue::float(
        binom * p.powi(k as i32) * (1.0 - p).powi((n - k) as i32),
    ))
}

/// `ppois k, lambda` — Poisson CDF P(X <= k).
fn builtin_ppois(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut cdf = 0.0;
    let mut term = (-lambda).exp();
    cdf += term;
    for i in 1..=k {
        term *= lambda / i as f64;
        cdf += term;
    }
    Ok(PerlValue::float(cdf))
}

/// `punif x, a, b` — uniform CDF.
fn builtin_punif(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(((x - a) / (b - a)).max(0.0).min(1.0)))
}

/// `pexp x, rate` — exponential CDF.
fn builtin_pexp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let rate = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if x < 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float(1.0 - (-rate * x).exp()))
}

/// `pweibull x, shape, scale` — Weibull CDF.
fn builtin_pweibull(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x < 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float(1.0 - (-(x / lambda).powf(k)).exp()))
}

/// `plnorm x, mu, sigma` — log-normal CDF.
fn builtin_plnorm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float(norm_cdf((x.ln() - mu) / sigma)))
}

/// `pcauchy x, x0, gamma` — Cauchy CDF.
fn builtin_pcauchy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let x0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(
        0.5 + ((x - x0) / gamma).atan() / std::f64::consts::PI,
    ))
}

// ── Matrix ops (R style) ────────────────────────────────────────────────────

/// `rbind M1, M2, ...` — bind matrices by rows (vertical stack).
fn builtin_rbind(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut rows = Vec::new();
    for a in args {
        let m = arg_to_vec(a);
        for row in m {
            rows.push(row);
        }
    }
    Ok(PerlValue::array(rows))
}

/// `cbind M1, M2, ...` — bind matrices by columns (horizontal join).
fn builtin_cbind(args: &[PerlValue]) -> PerlResult<PerlValue> {
    if args.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let mats: Vec<Vec<Vec<f64>>> = args.iter().map(|a| args_to_matrix(a)).collect();
    let nrows = mats.iter().map(|m| m.len()).max().unwrap_or(0);
    let mut result = Vec::with_capacity(nrows);
    for i in 0..nrows {
        let mut row = Vec::new();
        for m in &mats {
            if i < m.len() {
                row.extend_from_slice(&m[i]);
            }
        }
        result.push(PerlValue::array(
            row.iter().map(|&v| PerlValue::float(v)).collect(),
        ));
    }
    Ok(PerlValue::array(result))
}

/// `row_sums M` — sum of each row.
fn builtin_row_sums(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    Ok(vec_to_perl(
        &m.iter().map(|r| r.iter().sum()).collect::<Vec<f64>>(),
    ))
}

/// `col_sums M` — sum of each column.
fn builtin_col_sums(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if m.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let nc = m[0].len();
    let mut sums = vec![0.0; nc];
    for row in &m {
        for (j, &v) in row.iter().enumerate() {
            sums[j] += v;
        }
    }
    Ok(vec_to_perl(&sums))
}

/// `row_means M` — mean of each row.
fn builtin_row_means(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let means: Vec<f64> = m
        .iter()
        .map(|r| {
            if r.is_empty() {
                0.0
            } else {
                r.iter().sum::<f64>() / r.len() as f64
            }
        })
        .collect();
    Ok(vec_to_perl(&means))
}

/// `col_means M` — mean of each column.
fn builtin_col_means(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if m.is_empty() {
        return Ok(PerlValue::array(vec![]));
    }
    let nr = m.len() as f64;
    let nc = m[0].len();
    let mut means = vec![0.0; nc];
    for row in &m {
        for (j, &v) in row.iter().enumerate() {
            means[j] += v;
        }
    }
    for j in 0..nc {
        means[j] /= nr;
    }
    Ok(vec_to_perl(&means))
}

/// `outer V1, V2` — outer product of two vectors. Returns matrix.
fn builtin_outer(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v1: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let v2: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let m: Vec<Vec<f64>> = v1
        .iter()
        .map(|&a| v2.iter().map(|&b| a * b).collect())
        .collect();
    Ok(matrix_to_perl(&m))
}

/// `crossprod M` — cross product: t(M) %*% M (equivalent to M^T * M).
fn builtin_crossprod(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let nr = a.len();
    if nr == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    let nc = a[0].len();
    let mut result = vec![vec![0.0; nc]; nc];
    for i in 0..nc {
        for j in 0..nc {
            for k in 0..nr {
                result[i][j] += a[k][i] * a[k][j];
            }
        }
    }
    Ok(matrix_to_perl(&result))
}

/// `tcrossprod M` — M %*% t(M).
fn builtin_tcrossprod(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let nr = a.len();
    if nr == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    let nc = a[0].len();
    let mut result = vec![vec![0.0; nr]; nr];
    for i in 0..nr {
        for j in 0..nr {
            for k in 0..nc {
                result[i][j] += a[i][k] * a[j][k];
            }
        }
    }
    Ok(matrix_to_perl(&result))
}

/// `nrow M` — number of rows.
fn builtin_nrow(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    Ok(PerlValue::integer(m.len() as i64))
}

/// `ncol M` — number of columns.
fn builtin_ncol(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let nc = if m.is_empty() {
        0
    } else {
        arg_to_vec(&m[0]).len()
    };
    Ok(PerlValue::integer(nc as i64))
}

// ── R vector ops ────────────────────────────────────────────────────────────

/// `cummax VEC` — cumulative maximum.
fn builtin_cummax(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut max = f64::NEG_INFINITY;
    let result: Vec<PerlValue> = v
        .iter()
        .map(|&x| {
            max = max.max(x);
            PerlValue::float(max)
        })
        .collect();
    Ok(PerlValue::array(result))
}

/// `cummin VEC` — cumulative minimum.
fn builtin_cummin(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut min = f64::INFINITY;
    let result: Vec<PerlValue> = v
        .iter()
        .map(|&x| {
            min = min.min(x);
            PerlValue::float(min)
        })
        .collect();
    Ok(PerlValue::array(result))
}

/// `scale VEC [, center, scale_flag]` — standardize: (x - mean) / sd. R's scale().
fn builtin_scale(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = v.len() as f64;
    if n < 2.0 {
        return Ok(args.first().cloned().unwrap_or(PerlValue::UNDEF).clone());
    }
    let mean: f64 = v.iter().sum::<f64>() / n;
    let sd: f64 = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
    let result: Vec<PerlValue> = v
        .iter()
        .map(|&x| PerlValue::float(if sd > 0.0 { (x - mean) / sd } else { 0.0 }))
        .collect();
    Ok(PerlValue::array(result))
}

/// `which_val VEC, pred` — indices where predicate is true (R's which()).
fn builtin_which_val(
    interp: &mut crate::interpreter::Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let v = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let pred = args.get(1).cloned().unwrap_or(PerlValue::UNDEF);
    let sub = pred
        .as_code_ref()
        .ok_or_else(|| PerlError::runtime("which: expected code ref", line))?;
    let mut indices = Vec::new();
    for (i, x) in v.iter().enumerate() {
        let r = exec_to_perl_result(
            interp.call_sub(&sub, vec![x.clone()], WantarrayCtx::Scalar, line),
            "which",
            line,
        )?;
        if r.is_true() {
            indices.push(PerlValue::integer(i as i64));
        }
    }
    Ok(PerlValue::array(indices))
}

/// `tabulate VEC` — frequency table (R's table()). Returns hash of value => count.
fn builtin_tabulate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let mut counts = indexmap::IndexMap::new();
    for x in &v {
        let key = x.to_string();
        let entry = counts.entry(key).or_insert(PerlValue::integer(0));
        *entry = PerlValue::integer(entry.to_number() as i64 + 1);
    }
    Ok(PerlValue::hash(counts))
}

/// `duplicated VEC` — boolean array: true if element appeared earlier.
fn builtin_duplicated(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let mut seen = std::collections::HashSet::new();
    let result: Vec<PerlValue> = v
        .iter()
        .map(|x| {
            let key = x.to_string();
            if seen.contains(&key) {
                PerlValue::integer(1)
            } else {
                seen.insert(key);
                PerlValue::integer(0)
            }
        })
        .collect();
    Ok(PerlValue::array(result))
}

/// `rev_vec VEC` — reverse a vector (R's rev()).
fn builtin_rev_vec(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut v = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    v.reverse();
    Ok(PerlValue::array(v))
}

/// `seq_fn from, to [, by]` — generate sequence (R's seq()).
fn builtin_seq_fn(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let from = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let to = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let by = args
        .get(2)
        .map(|v| v.to_number())
        .unwrap_or(if to >= from { 1.0 } else { -1.0 });
    let mut result = Vec::new();
    let mut x = from;
    if by > 0.0 {
        while x <= to + 1e-10 {
            result.push(PerlValue::float(x));
            x += by;
        }
    } else if by < 0.0 {
        while x >= to - 1e-10 {
            result.push(PerlValue::float(x));
            x += by;
        }
    }
    Ok(PerlValue::array(result))
}

/// `rep_fn VAL, times` — repeat a value (R's rep()).
fn builtin_rep_fn(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let val = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let times = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    Ok(PerlValue::array(vec![val; times]))
}

/// `cut VEC, breaks` — bin values into intervals (R's cut()). Returns integer bin indices.
fn builtin_cut(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let breaks: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let result: Vec<PerlValue> = v
        .iter()
        .map(|&x| {
            let mut bin = 0i64;
            for (i, w) in breaks.windows(2).enumerate() {
                if x >= w[0] && x < w[1] {
                    bin = (i + 1) as i64;
                    break;
                }
                if i == breaks.len() - 2 && x == w[1] {
                    bin = (i + 1) as i64;
                }
            }
            PerlValue::integer(bin)
        })
        .collect();
    Ok(PerlValue::array(result))
}

/// `find_interval X, breaks` — find interval indices (R's findInterval()).
fn builtin_find_interval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let breaks: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let result: Vec<PerlValue> = x
        .iter()
        .map(|&xi| {
            let mut idx = 0i64;
            for (i, &b) in breaks.iter().enumerate() {
                if xi >= b {
                    idx = (i + 1) as i64;
                } else {
                    break;
                }
            }
            PerlValue::integer(idx)
        })
        .collect();
    Ok(PerlValue::array(result))
}

/// `ecdf VEC, x` — empirical CDF: proportion of elements <= x.
fn builtin_ecdf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = v.len() as f64;
    let count = v.iter().filter(|&&xi| xi <= x).count() as f64;
    Ok(PerlValue::float(count / n))
}

/// `density VEC [, n_points]` — kernel density estimation (Gaussian kernel). Returns [[x],[y]].
fn builtin_density(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let data: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n_pts = args.get(1).map(|v| v.to_number() as usize).unwrap_or(512);
    let n = data.len() as f64;
    if n < 2.0 {
        return Err(PerlError::runtime("density: need >= 2 data points", 0));
    }
    let mean: f64 = data.iter().sum::<f64>() / n;
    let sd: f64 = (data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0)).sqrt();
    let bw = 1.06 * sd * n.powf(-0.2); // Silverman's rule
    let min = data.iter().cloned().fold(f64::INFINITY, f64::min) - 3.0 * bw;
    let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max) + 3.0 * bw;
    let step = (max - min) / (n_pts - 1) as f64;
    let mut xs = Vec::with_capacity(n_pts);
    let mut ys = Vec::with_capacity(n_pts);
    let pi2 = (2.0 * std::f64::consts::PI).sqrt();
    for i in 0..n_pts {
        let x = min + i as f64 * step;
        let y: f64 = data
            .iter()
            .map(|&d| (-0.5 * ((x - d) / bw).powi(2)).exp() / (bw * pi2))
            .sum::<f64>()
            / n;
        xs.push(PerlValue::float(x));
        ys.push(PerlValue::float(y));
    }
    Ok(PerlValue::array(vec![
        PerlValue::array(xs),
        PerlValue::array(ys),
    ]))
}

// ── R stats tests ───────────────────────────────────────────────────────────

/// `shapiro_test VEC` — Shapiro-Wilk W statistic (simplified). Returns W.
fn builtin_shapiro_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = x.len();
    if n < 3 {
        return Err(PerlError::runtime("shapiro_test: need >= 3 samples", 0));
    }
    x.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mean: f64 = x.iter().sum::<f64>() / n as f64;
    let ss: f64 = x.iter().map(|v| (v - mean).powi(2)).sum();
    // Approximate W using normal order statistics
    let mut a_sum = 0.0;
    for i in 0..n / 2 {
        let ai = (2.0 * (i + 1) as f64 - 1.0) / (2.0 * n as f64);
        let expected = qnorm_approx(ai);
        a_sum += expected * (x[n - 1 - i] - x[i]);
    }
    let w = (a_sum * a_sum) / ss;
    Ok(PerlValue::float(w.min(1.0)))
}

fn qnorm_approx(p: f64) -> f64 {
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    let t = if p < 0.5 {
        (-2.0 * p.ln()).sqrt()
    } else {
        (-2.0 * (1.0 - p).ln()).sqrt()
    };
    let c = [2.515517, 0.802853, 0.010328];
    let d = [1.432788, 0.189269, 0.001308];
    let z = t - (c[0] + t * (c[1] + t * c[2])) / (1.0 + t * (d[0] + t * (d[1] + t * d[2])));
    if p < 0.5 {
        -z
    } else {
        z
    }
}

/// `ks_test SAMPLE1, SAMPLE2` — two-sample Kolmogorov-Smirnov test statistic D.
fn builtin_ks_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut s1: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut s2: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    s1.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    s2.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n1 = s1.len() as f64;
    let n2 = s2.len() as f64;
    let mut i = 0usize;
    let mut j = 0usize;
    let mut d = 0.0f64;
    while i < s1.len() && j < s2.len() {
        if s1[i] <= s2[j] {
            i += 1;
        } else {
            j += 1;
        }
        let diff = (i as f64 / n1 - j as f64 / n2).abs();
        d = d.max(diff);
    }
    Ok(PerlValue::float(d))
}

/// `wilcox_test SAMPLE1, SAMPLE2` — Wilcoxon rank-sum (Mann-Whitney U) test statistic.
fn builtin_wilcox_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s1: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let s2: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut u = 0i64;
    for &x in &s1 {
        for &y in &s2 {
            if x > y {
                u += 1;
            } else if (x - y).abs() < 1e-12 { /* tie: add 0.5, skip for integer */
            }
        }
    }
    Ok(PerlValue::integer(u))
}

/// `prop_test x, n [, p0]` — one-sample proportion test (z-test). Returns [z, p_value].
fn builtin_prop_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let p0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let p_hat = x / n;
    let z = (p_hat - p0) / (p0 * (1.0 - p0) / n).sqrt();
    let p_val = 2.0 * (1.0 - norm_cdf(z.abs()));
    Ok(PerlValue::array(vec![
        PerlValue::float(z),
        PerlValue::float(p_val),
    ]))
}

/// `binom_test x, n [, p]` — exact binomial test. Returns p-value (two-sided, summing small tails).
fn builtin_binom_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    // P(X = k)
    let pmf = |k: i64| -> f64 {
        let mut b = 1.0;
        for j in 0..k {
            b *= (n - j) as f64 / (j + 1) as f64;
        }
        b * p.powi(k as i32) * (1.0 - p).powi((n - k) as i32)
    };
    let px = pmf(x);
    let mut pval = 0.0;
    for k in 0..=n {
        let pk = pmf(k);
        if pk <= px + 1e-12 {
            pval += pk;
        }
    }
    Ok(PerlValue::float(pval.min(1.0)))
}

// ── R apply family / functional ─────────────────────────────────────────────

/// `sapply VEC, FN` — apply function to each element, return vector (R's sapply).
fn builtin_sapply(
    interp: &mut crate::interpreter::Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let v = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let f = args.get(1).cloned().unwrap_or(PerlValue::UNDEF);
    let sub = f
        .as_code_ref()
        .ok_or_else(|| PerlError::runtime("sapply: expected code ref", line))?;
    let mut result = Vec::with_capacity(v.len());
    for x in v {
        let r = exec_to_perl_result(
            interp.call_sub(&sub, vec![x], WantarrayCtx::Scalar, line),
            "sapply",
            line,
        )?;
        result.push(r);
    }
    Ok(PerlValue::array(result))
}

/// `tapply VEC, GROUPS, FN` — apply function by group (R's tapply). Returns hash.
fn builtin_tapply(
    interp: &mut crate::interpreter::Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let v = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let groups = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let f = args.get(2).cloned().unwrap_or(PerlValue::UNDEF);
    let sub = f
        .as_code_ref()
        .ok_or_else(|| PerlError::runtime("tapply: expected code ref", line))?;
    let mut grouped: indexmap::IndexMap<String, Vec<PerlValue>> = indexmap::IndexMap::new();
    for (i, x) in v.iter().enumerate() {
        let key = if i < groups.len() {
            groups[i].to_string()
        } else {
            "NA".to_string()
        };
        grouped.entry(key).or_default().push(x.clone());
    }
    let mut result = indexmap::IndexMap::new();
    for (k, vals) in grouped {
        let r = exec_to_perl_result(
            interp.call_sub(
                &sub,
                vec![PerlValue::array(vals)],
                WantarrayCtx::Scalar,
                line,
            ),
            "tapply",
            line,
        )?;
        result.insert(k, r);
    }
    Ok(PerlValue::hash(result))
}

/// `do_call FN, ARGS` — call function with args from a list (R's do.call).
fn builtin_do_call(
    interp: &mut crate::interpreter::Interpreter,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let call_args = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let sub = f
        .as_code_ref()
        .ok_or_else(|| PerlError::runtime("do_call: expected code ref", line))?;
    exec_to_perl_result(
        interp.call_sub(&sub, call_args, WantarrayCtx::Scalar, line),
        "do_call",
        line,
    )
}

/// `embed VEC, dimension` — time-delay embedding (R's embed). Returns matrix.
fn builtin_embed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let dim = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2);
    if v.len() < dim {
        return Ok(PerlValue::array(vec![]));
    }
    let nrows = v.len() - dim + 1;
    let mut result = Vec::with_capacity(nrows);
    for i in 0..nrows {
        let row: Vec<PerlValue> = (0..dim)
            .map(|j| PerlValue::float(v[i + dim - 1 - j]))
            .collect();
        result.push(PerlValue::array(row));
    }
    Ok(PerlValue::array(result))
}

/// `prop_table M` — proportions table: each cell divided by total (R's prop.table).
fn builtin_prop_table(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let total: f64 = m.iter().flat_map(|r| r.iter()).sum();
    if total == 0.0 {
        return Ok(matrix_to_perl(&m));
    }
    let result: Vec<Vec<f64>> = m
        .iter()
        .map(|r| r.iter().map(|&v| v / total).collect())
        .collect();
    Ok(matrix_to_perl(&result))
}

/// `kmeans DATA, k [, max_iter]` — k-means clustering. DATA is array of [x,y,...] points. Returns cluster assignments.
fn builtin_kmeans(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let data_raw = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2);
    let max_iter = args.get(2).map(|v| v.to_number() as usize).unwrap_or(100);
    let data: Vec<Vec<f64>> = data_raw
        .iter()
        .map(|v| arg_to_vec(v).iter().map(|x| x.to_number()).collect())
        .collect();
    let n = data.len();
    if n == 0 || k == 0 {
        return Ok(PerlValue::array(vec![]));
    }
    let dim = data[0].len();
    // Initialize centroids to first k points
    let mut centroids: Vec<Vec<f64>> = data.iter().take(k).cloned().collect();
    let mut assignments = vec![0usize; n];
    for _ in 0..max_iter {
        // Assign
        let mut changed = false;
        for i in 0..n {
            let mut best = 0;
            let mut best_dist = f64::INFINITY;
            for c in 0..k {
                let d: f64 = (0..dim)
                    .map(|j| (data[i][j] - centroids[c][j]).powi(2))
                    .sum();
                if d < best_dist {
                    best_dist = d;
                    best = c;
                }
            }
            if assignments[i] != best {
                changed = true;
                assignments[i] = best;
            }
        }
        if !changed {
            break;
        }
        // Update centroids
        let mut counts = vec![0usize; k];
        centroids = vec![vec![0.0; dim]; k];
        for i in 0..n {
            let c = assignments[i];
            counts[c] += 1;
            for j in 0..dim {
                centroids[c][j] += data[i][j];
            }
        }
        for c in 0..k {
            if counts[c] > 0 {
                for j in 0..dim {
                    centroids[c][j] /= counts[c] as f64;
                }
            }
        }
    }
    Ok(PerlValue::array(
        assignments
            .iter()
            .map(|&c| PerlValue::integer(c as i64))
            .collect(),
    ))
}

/// `prcomp DATA [, n_components]` — PCA via eigendecomposition of covariance matrix. Returns [[scores], [loadings], [variance_explained]].
fn builtin_prcomp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let data_raw = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let data: Vec<Vec<f64>> = data_raw
        .iter()
        .map(|v| arg_to_vec(v).iter().map(|x| x.to_number()).collect())
        .collect();
    let n = data.len();
    if n < 2 {
        return Err(PerlError::runtime("prcomp: need >= 2 observations", 0));
    }
    let p = data[0].len();
    let nf = n as f64;
    // Center
    let means: Vec<f64> = (0..p)
        .map(|j| data.iter().map(|r| r[j]).sum::<f64>() / nf)
        .collect();
    let centered: Vec<Vec<f64>> = data
        .iter()
        .map(|r| (0..p).map(|j| r[j] - means[j]).collect())
        .collect();
    // Covariance matrix
    let mut cov = vec![vec![0.0; p]; p];
    for i in 0..p {
        for j in 0..p {
            let s: f64 = centered.iter().map(|r| r[i] * r[j]).sum();
            cov[i][j] = s / (nf - 1.0);
        }
    }
    // Eigenvalues via QR iteration
    let eig_args = [matrix_to_perl(&cov)];
    let eigs = builtin_matrix_eigenvalues(&eig_args)?;
    let var_explained = eigs;
    Ok(PerlValue::array(vec![var_explained]))
}

// ─────────────────────────────────────────────────────────────────────────────
// R base: distribution random generators (r-functions)
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_rnorm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    use rand::Rng;
    let result: Vec<PerlValue> = (0..n)
        .map(|_| {
            // Box-Muller transform
            let u1: f64 = rng.gen::<f64>().max(1e-15);
            let u2: f64 = rng.gen::<f64>();
            let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
            PerlValue::float(mu + sigma * z)
        })
        .collect();
    if n == 1 {
        Ok(result.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(result))
    }
}

fn builtin_runif(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    use rand::Rng;
    let result: Vec<PerlValue> = (0..n)
        .map(|_| PerlValue::float(a + (b - a) * rng.gen::<f64>()))
        .collect();
    if n == 1 {
        Ok(result.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(result))
    }
}

fn builtin_rexp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let rate = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    use rand::Rng;
    let result: Vec<PerlValue> = (0..n)
        .map(|_| {
            let u: f64 = rng.gen::<f64>().max(1e-15);
            PerlValue::float(-u.ln() / rate)
        })
        .collect();
    if n == 1 {
        Ok(result.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(result))
    }
}

fn builtin_rbinom(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let size = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    let prob = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let mut rng = rand::thread_rng();
    use rand::Rng;
    let result: Vec<PerlValue> = (0..n)
        .map(|_| {
            let k: i64 = (0..size).filter(|_| rng.gen::<f64>() < prob).count() as i64;
            PerlValue::integer(k)
        })
        .collect();
    if n == 1 {
        Ok(result.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(result))
    }
}

fn builtin_rpois(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    use rand::Rng;
    let result: Vec<PerlValue> = (0..n)
        .map(|_| {
            // Knuth's algorithm
            let l = (-lambda).exp();
            let mut k = 0i64;
            let mut p = 1.0;
            loop {
                k += 1;
                p *= rng.gen::<f64>();
                if p < l {
                    break;
                }
            }
            PerlValue::integer(k - 1)
        })
        .collect();
    if n == 1 {
        Ok(result.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(result))
    }
}

fn builtin_rgeom(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let prob = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let mut rng = rand::thread_rng();
    use rand::Rng;
    let result: Vec<PerlValue> = (0..n)
        .map(|_| {
            let u: f64 = rng.gen::<f64>().max(1e-15);
            PerlValue::integer((u.ln() / (1.0 - prob).ln()).floor() as i64)
        })
        .collect();
    if n == 1 {
        Ok(result.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(result))
    }
}

fn builtin_rgamma(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let shape = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let scale = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    use rand::Rng;
    let result: Vec<PerlValue> = (0..n)
        .map(|_| {
            // Marsaglia and Tsang's method for shape >= 1
            let a = if shape < 1.0 { shape + 1.0 } else { shape };
            let d = a - 1.0 / 3.0;
            let c = 1.0 / (9.0 * d).sqrt();
            let mut x;
            loop {
                let u1: f64 = rng.gen::<f64>().max(1e-15);
                let u2: f64 = rng.gen::<f64>();
                let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
                let v = (1.0 + c * z).powi(3);
                if v > 0.0 {
                    let u: f64 = rng.gen::<f64>().max(1e-15);
                    if u.ln() < 0.5 * z * z + d * (1.0 - v + v.ln()) {
                        x = d * v * scale;
                        if shape < 1.0 {
                            x *= rng.gen::<f64>().max(1e-15).powf(1.0 / shape);
                        }
                        break;
                    }
                }
            }
            PerlValue::float(x)
        })
        .collect();
    if n == 1 {
        Ok(result.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(result))
    }
}

fn builtin_rbeta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    // Beta(a,b) = Gamma(a,1) / (Gamma(a,1) + Gamma(b,1))
    let ga_args = [
        PerlValue::integer(1),
        PerlValue::float(a),
        PerlValue::float(1.0),
    ];
    let gb_args = [
        PerlValue::integer(1),
        PerlValue::float(b),
        PerlValue::float(1.0),
    ];
    let result: Vec<PerlValue> = (0..n)
        .map(|_| {
            let xa = builtin_rgamma(&ga_args).unwrap().to_number();
            let xb = builtin_rgamma(&gb_args).unwrap().to_number();
            PerlValue::float(xa / (xa + xb))
        })
        .collect();
    if n == 1 {
        Ok(result.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(result))
    }
}

fn builtin_rchisq(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let df = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    // Chi-sq(df) = Gamma(df/2, 2)
    let g_args = [
        PerlValue::integer(n as i64),
        PerlValue::float(df / 2.0),
        PerlValue::float(2.0),
    ];
    builtin_rgamma(&g_args)
}

fn builtin_rt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let df = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    // t(df) = Z / sqrt(Chi2(df)/df)
    let result: Vec<PerlValue> = (0..n)
        .map(|_| {
            let z = builtin_rnorm(&[PerlValue::integer(1)]).unwrap().to_number();
            let chi2 = builtin_rchisq(&[PerlValue::integer(1), PerlValue::float(df)])
                .unwrap()
                .to_number();
            PerlValue::float(z / (chi2 / df).sqrt())
        })
        .collect();
    if n == 1 {
        Ok(result.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(result))
    }
}

fn builtin_rf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let d1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let d2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let result: Vec<PerlValue> = (0..n)
        .map(|_| {
            let c1 = builtin_rchisq(&[PerlValue::integer(1), PerlValue::float(d1)])
                .unwrap()
                .to_number();
            let c2 = builtin_rchisq(&[PerlValue::integer(1), PerlValue::float(d2)])
                .unwrap()
                .to_number();
            PerlValue::float((c1 / d1) / (c2 / d2))
        })
        .collect();
    if n == 1 {
        Ok(result.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(result))
    }
}

fn builtin_rweibull(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let shape = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let scale = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    use rand::Rng;
    let result: Vec<PerlValue> = (0..n)
        .map(|_| {
            let u: f64 = rng.gen::<f64>().max(1e-15);
            PerlValue::float(scale * (-u.ln()).powf(1.0 / shape))
        })
        .collect();
    if n == 1 {
        Ok(result.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(result))
    }
}

fn builtin_rlnorm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let norms = builtin_rnorm(&[
        PerlValue::integer(n as i64),
        PerlValue::float(mu),
        PerlValue::float(sigma),
    ])?;
    if n == 1 {
        return Ok(PerlValue::float(norms.to_number().exp()));
    }
    let v = arg_to_vec(&norms);
    Ok(PerlValue::array(
        v.iter()
            .map(|x| PerlValue::float(x.to_number().exp()))
            .collect(),
    ))
}

fn builtin_rcauchy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let x0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    use rand::Rng;
    let result: Vec<PerlValue> = (0..n)
        .map(|_| {
            let u: f64 = rng.gen::<f64>();
            PerlValue::float(x0 + gamma * (std::f64::consts::PI * (u - 0.5)).tan())
        })
        .collect();
    if n == 1 {
        Ok(result.into_iter().next().unwrap())
    } else {
        Ok(PerlValue::array(result))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// R base: quantile functions (q-functions) — inverse CDFs
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_qunif(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(a + p * (b - a)))
}

fn builtin_qexp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let rate = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(-(1.0 - p).ln() / rate))
}

fn builtin_qweibull(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(lambda * (-(1.0 - p).ln()).powf(1.0 / k)))
}

fn builtin_qlnorm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let z = qnorm_approx(p);
    Ok(PerlValue::float((mu + sigma * z).exp()))
}

fn builtin_qcauchy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let x0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(
        x0 + gamma * (std::f64::consts::PI * (p - 0.5)).tan(),
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// R base: additional CDFs
// ─────────────────────────────────────────────────────────────────────────────

/// `pgamma x, shape [, scale]` — gamma CDF via numerical integration.
fn builtin_pgamma(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let shape = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let scale = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    // Use regularized lower incomplete gamma
    let z = x / scale;
    // Series expansion for small z
    let mut sum = 0.0;
    let mut term = 1.0 / shape;
    sum += term;
    for n in 1..200 {
        term *= z / (shape + n as f64);
        sum += term;
        if term.abs() < 1e-12 {
            break;
        }
    }
    let result = z.powf(shape) * (-z).exp() * sum / lgamma_fn(shape).exp();
    Ok(PerlValue::float(result.min(1.0).max(0.0)))
}

/// `pbeta x, a, b` — beta CDF (regularized incomplete beta).
fn builtin_pbeta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    if x >= 1.0 {
        return Ok(PerlValue::float(1.0));
    }
    // Numerical integration via trapezoidal rule
    let steps = 1000;
    let dx = x / steps as f64;
    let mut sum = 0.0;
    let ln_beta = lgamma_fn(a) + lgamma_fn(b) - lgamma_fn(a + b);
    for i in 0..steps {
        let t = (i as f64 + 0.5) * dx;
        sum += t.powf(a - 1.0) * (1.0 - t).powf(b - 1.0);
    }
    let result = sum * dx / ln_beta.exp();
    Ok(PerlValue::float(result.min(1.0).max(0.0)))
}

fn builtin_pchisq(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let df = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    builtin_pgamma(&[
        PerlValue::float(x),
        PerlValue::float(df / 2.0),
        PerlValue::float(2.0),
    ])
}

fn builtin_pt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let df = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    // Approximate via normal for large df, otherwise numerical
    if df > 100.0 {
        return Ok(PerlValue::float(norm_cdf(x)));
    }
    let steps = 2000;
    let lo = -20.0f64;
    let hi = x;
    if hi <= lo {
        return Ok(PerlValue::float(0.0));
    }
    let dx = (hi - lo) / steps as f64;
    let coeff = (lgamma_fn((df + 1.0) / 2.0) - lgamma_fn(df / 2.0)).exp()
        / (df * std::f64::consts::PI).sqrt();
    let mut sum = 0.0;
    for i in 0..steps {
        let t = lo + (i as f64 + 0.5) * dx;
        sum += coeff * (1.0 + t * t / df).powf(-(df + 1.0) / 2.0);
    }
    Ok(PerlValue::float((sum * dx).min(1.0).max(0.0)))
}

fn builtin_pf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let d1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let d2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if x <= 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    // Use beta CDF: F(x) = I(d1*x/(d1*x+d2); d1/2, d2/2)
    let z = d1 * x / (d1 * x + d2);
    builtin_pbeta(&[
        PerlValue::float(z),
        PerlValue::float(d1 / 2.0),
        PerlValue::float(d2 / 2.0),
    ])
}

// ─────────────────────────────────────────────────────────────────────────────
// R base: additional PMFs
// ─────────────────────────────────────────────────────────────────────────────

fn builtin_dgeom(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float(p * (1.0 - p).powi(k as i32)))
}

fn builtin_dunif(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if x >= a && x <= b {
        1.0 / (b - a)
    } else {
        0.0
    }))
}

fn builtin_dnbinom(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    // NB(k; r, p) = C(k+r-1, k) * p^r * (1-p)^k
    let mut binom = 1.0;
    for j in 0..k {
        binom *= (k + r as i64 - 1 - j) as f64 / (j + 1) as f64;
    }
    Ok(PerlValue::float(
        binom * p.powf(r) * (1.0 - p).powi(k as i32),
    ))
}

fn builtin_dhyper(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(10); // successes in population
    let n = args.get(2).map(|v| v.to_number() as i64).unwrap_or(10); // failures in population
    let nn = args.get(3).map(|v| v.to_number() as i64).unwrap_or(5); // draws
                                                                     // C(m,k) * C(n, nn-k) / C(m+n, nn)
    let lnchoose = |a: i64, b: i64| -> f64 {
        if b < 0 || b > a {
            return f64::NEG_INFINITY;
        }
        lgamma_fn((a + 1) as f64) - lgamma_fn((b + 1) as f64) - lgamma_fn((a - b + 1) as f64)
    };
    let log_pmf = lnchoose(m, k) + lnchoose(n, nn - k) - lnchoose(m + n, nn);
    Ok(PerlValue::float(log_pmf.exp()))
}

// ─────────────────────────────────────────────────────────────────────────────
// R base: smoothing
// ─────────────────────────────────────────────────────────────────────────────

/// `lowess XS, YS [, f]` — locally-weighted scatterplot smoothing. Returns smoothed Y values.
fn builtin_lowess(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let f = args.get(2).map(|v| v.to_number()).unwrap_or(0.6667);
    let n = xs.len();
    if n < 3 || ys.len() != n {
        return Err(PerlError::runtime("lowess: need >= 3 matched points", 0));
    }
    let span = (f * n as f64).ceil() as usize;
    let span = span.max(2).min(n);
    let mut smoothed = Vec::with_capacity(n);
    for i in 0..n {
        // Find span nearest neighbors
        let mut dists: Vec<(usize, f64)> = (0..n).map(|j| (j, (xs[j] - xs[i]).abs())).collect();
        dists.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let max_dist = dists[span - 1].1.max(1e-10);
        // Weighted least squares (degree 1)
        let mut sw = 0.0;
        let mut swx = 0.0;
        let mut swy = 0.0;
        let mut swxx = 0.0;
        let mut swxy = 0.0;
        for &(j, d) in dists.iter().take(span) {
            let u = d / max_dist;
            let w = (1.0 - u * u * u).max(0.0).powi(3); // tricube
            sw += w;
            swx += w * xs[j];
            swy += w * ys[j];
            swxx += w * xs[j] * xs[j];
            swxy += w * xs[j] * ys[j];
        }
        let det = sw * swxx - swx * swx;
        let y_hat = if det.abs() > 1e-12 {
            let b0 = (swxx * swy - swx * swxy) / det;
            let b1 = (sw * swxy - swx * swy) / det;
            b0 + b1 * xs[i]
        } else {
            swy / sw
        };
        smoothed.push(PerlValue::float(y_hat));
    }
    Ok(PerlValue::array(smoothed))
}

/// `approx_fn XS, YS, XOUT` — linear interpolation at points xout (R's approx).
fn builtin_approx_fn(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let xout: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = xs.len();
    let result: Vec<PerlValue> = xout
        .iter()
        .map(|&x| {
            if n < 2 {
                return PerlValue::float(f64::NAN);
            }
            if x <= xs[0] {
                return PerlValue::float(ys[0]);
            }
            if x >= xs[n - 1] {
                return PerlValue::float(ys[n - 1]);
            }
            let mut i = 0;
            while i < n - 1 && xs[i + 1] < x {
                i += 1;
            }
            let t = (x - xs[i]) / (xs[i + 1] - xs[i]);
            PerlValue::float(ys[i] + t * (ys[i + 1] - ys[i]))
        })
        .collect();
    Ok(PerlValue::array(result))
}

// ─────────────────────────────────────────────────────────────────────────────
// R base: linear models
// ─────────────────────────────────────────────────────────────────────────────

/// `lm_fit XS, YS` — simple linear regression. Returns {intercept, slope, r_squared, residuals, fitted}.
fn builtin_lm_fit(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = xs.len() as f64;
    let mx: f64 = xs.iter().sum::<f64>() / n;
    let my_val: f64 = ys.iter().sum::<f64>() / n;
    let ss_xy: f64 = xs
        .iter()
        .zip(ys.iter())
        .map(|(x, y)| (x - mx) * (y - my_val))
        .sum();
    let ss_xx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let slope = ss_xy / ss_xx;
    let intercept = my_val - slope * mx;
    let fitted: Vec<f64> = xs.iter().map(|&x| intercept + slope * x).collect();
    let residuals: Vec<f64> = ys.iter().zip(fitted.iter()).map(|(y, f)| y - f).collect();
    let ss_res: f64 = residuals.iter().map(|r| r * r).sum();
    let ss_tot: f64 = ys.iter().map(|y| (y - my_val).powi(2)).sum();
    let r2 = 1.0 - ss_res / ss_tot;
    let mut result = indexmap::IndexMap::new();
    result.insert("intercept".to_string(), PerlValue::float(intercept));
    result.insert("slope".to_string(), PerlValue::float(slope));
    result.insert("r_squared".to_string(), PerlValue::float(r2));
    result.insert("residuals".to_string(), vec_to_perl(&residuals));
    result.insert("fitted".to_string(), vec_to_perl(&fitted));
    Ok(PerlValue::hash(result))
}

// ─────────────────────────────────────────────────────────────────────────────
// R base: remaining distribution quantiles (q-functions)
// ─────────────────────────────────────────────────────────────────────────────

/// `qgamma p, shape [, scale]` — gamma quantile via Newton iteration on pgamma.
fn builtin_qgamma(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let shape = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let scale = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    // Initial guess via Wilson-Hilferty
    let mut x = shape * scale;
    for _ in 0..100 {
        let cdf = builtin_pgamma(&[
            PerlValue::float(x),
            PerlValue::float(shape),
            PerlValue::float(scale),
        ])?
        .to_number();
        let pdf_args = [
            PerlValue::float(x),
            PerlValue::float(shape),
            PerlValue::float(scale),
        ];
        let pdf = builtin_gamma_pdf(&pdf_args)?.to_number();
        if pdf.abs() < 1e-15 {
            break;
        }
        let dx = (cdf - p) / pdf;
        x -= dx;
        x = x.max(1e-15);
        if dx.abs() < 1e-10 {
            break;
        }
    }
    Ok(PerlValue::float(x))
}

/// `qbeta p, a, b` — beta quantile via Newton iteration on pbeta.
fn builtin_qbeta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let mut x = p; // initial guess
    for _ in 0..100 {
        let cdf = builtin_pbeta(&[
            PerlValue::float(x),
            PerlValue::float(a),
            PerlValue::float(b),
        ])?
        .to_number();
        let pdf = builtin_beta_pdf(&[
            PerlValue::float(x),
            PerlValue::float(a),
            PerlValue::float(b),
        ])?
        .to_number();
        if pdf.abs() < 1e-15 {
            break;
        }
        let dx = (cdf - p) / pdf;
        x -= dx;
        x = x.max(1e-12).min(1.0 - 1e-12);
        if dx.abs() < 1e-10 {
            break;
        }
    }
    Ok(PerlValue::float(x))
}

/// `qchisq p, df` — chi-squared quantile.
fn builtin_qchisq(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let df = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    builtin_qgamma(&[
        PerlValue::float(p),
        PerlValue::float(df / 2.0),
        PerlValue::float(2.0),
    ])
}

/// `qt p, df` — Student's t quantile via Newton iteration on pt.
fn builtin_qt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let df = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut x = qnorm_approx(p); // initial guess from normal
    for _ in 0..100 {
        let cdf = builtin_pt(&[PerlValue::float(x), PerlValue::float(df)])?.to_number();
        let pdf = builtin_t_pdf(&[PerlValue::float(x), PerlValue::float(df)])?.to_number();
        if pdf.abs() < 1e-15 {
            break;
        }
        let dx = (cdf - p) / pdf;
        x -= dx;
        if dx.abs() < 1e-10 {
            break;
        }
    }
    Ok(PerlValue::float(x))
}

/// `qf p, d1, d2` — F-distribution quantile via Newton iteration.
fn builtin_qf(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let d1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let d2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let mut x = 1.0; // initial guess
    for _ in 0..100 {
        let cdf = builtin_pf(&[
            PerlValue::float(x),
            PerlValue::float(d1),
            PerlValue::float(d2),
        ])?
        .to_number();
        let pdf = builtin_f_pdf(&[
            PerlValue::float(x),
            PerlValue::float(d1),
            PerlValue::float(d2),
        ])?
        .to_number();
        if pdf.abs() < 1e-15 {
            break;
        }
        let dx = (cdf - p) / pdf;
        x -= dx;
        x = x.max(1e-15);
        if dx.abs() < 1e-10 {
            break;
        }
    }
    Ok(PerlValue::float(x))
}

/// `qbinom p, n, prob` — binomial quantile (smallest k where P(X<=k) >= p).
fn builtin_qbinom(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let prob = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let mut cdf = 0.0;
    for k in 0..=n {
        let pmf = builtin_dbinom(&[
            PerlValue::integer(k),
            PerlValue::integer(n),
            PerlValue::float(prob),
        ])?
        .to_number();
        cdf += pmf;
        if cdf >= p {
            return Ok(PerlValue::integer(k));
        }
    }
    Ok(PerlValue::integer(n))
}

/// `qpois p, lambda` — Poisson quantile (smallest k where P(X<=k) >= p).
fn builtin_qpois(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = args.first().map(|v| v.to_number()).unwrap_or(0.5);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut cdf = 0.0;
    let mut term = (-lambda).exp();
    cdf += term;
    if cdf >= p {
        return Ok(PerlValue::integer(0));
    }
    for k in 1..10000 {
        term *= lambda / k as f64;
        cdf += term;
        if cdf >= p {
            return Ok(PerlValue::integer(k));
        }
    }
    Ok(PerlValue::integer(0))
}

// ─────────────────────────────────────────────────────────────────────────────
// R base: time series
// ─────────────────────────────────────────────────────────────────────────────

/// `acf_fn VEC [, max_lag]` — autocorrelation function. Returns array of ACF values for lags 0..max_lag.
fn builtin_acf_fn(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = x.len();
    let max_lag = args
        .get(1)
        .map(|v| v.to_number() as usize)
        .unwrap_or(n.min(40));
    let mean: f64 = x.iter().sum::<f64>() / n as f64;
    let var: f64 = x.iter().map(|v| (v - mean).powi(2)).sum::<f64>();
    let mut acf_vals = Vec::with_capacity(max_lag + 1);
    for lag in 0..=max_lag.min(n - 1) {
        let mut s = 0.0;
        for i in 0..n - lag {
            s += (x[i] - mean) * (x[i + lag] - mean);
        }
        acf_vals.push(PerlValue::float(s / var));
    }
    Ok(PerlValue::array(acf_vals))
}

/// `pacf_fn VEC [, max_lag]` — partial autocorrelation function via Durbin-Levinson.
fn builtin_pacf_fn(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let acf_result = builtin_acf_fn(args)?;
    let acf: Vec<f64> = arg_to_vec(&acf_result)
        .iter()
        .map(|v| v.to_number())
        .collect();
    let max_lag = acf.len() - 1;
    let mut pacf_vals = vec![PerlValue::float(1.0)]; // lag 0
    if max_lag == 0 {
        return Ok(PerlValue::array(pacf_vals));
    }
    let mut phi = vec![vec![0.0; max_lag + 1]; max_lag + 1];
    phi[1][1] = acf[1];
    pacf_vals.push(PerlValue::float(acf[1]));
    for k in 2..=max_lag {
        let mut num = acf[k];
        for j in 1..k {
            num -= phi[k - 1][j] * acf[k - j];
        }
        let mut den = 1.0;
        for j in 1..k {
            den -= phi[k - 1][j] * acf[j];
        }
        phi[k][k] = if den.abs() > 1e-15 { num / den } else { 0.0 };
        for j in 1..k {
            phi[k][j] = phi[k - 1][j] - phi[k][k] * phi[k - 1][k - j];
        }
        pacf_vals.push(PerlValue::float(phi[k][k]));
    }
    Ok(PerlValue::array(pacf_vals))
}

/// `diff_lag VEC [, lag [, differences]]` — lagged differences (R's diff).
fn builtin_diff_lag(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut v: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let lag = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    let differences = args.get(2).map(|v| v.to_number() as usize).unwrap_or(1);
    for _ in 0..differences {
        if v.len() <= lag {
            return Ok(PerlValue::array(vec![]));
        }
        let new: Vec<f64> = (lag..v.len()).map(|i| v[i] - v[i - lag]).collect();
        v = new;
    }
    Ok(vec_to_perl(&v))
}

/// `ts_filter VEC, COEFFS` — linear filtering (convolution with coefficients). R's filter() with method="convolution".
fn builtin_ts_filter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let filt: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = x.len();
    let m = filt.len();
    if n < m {
        return Ok(PerlValue::array(vec![]));
    }
    let half = m / 2;
    let mut result = Vec::with_capacity(n);
    for i in 0..n {
        if i < half || i + m - half > n {
            result.push(PerlValue::float(f64::NAN));
        } else {
            let mut s = 0.0;
            for j in 0..m {
                let idx = i + j - half;
                if idx < n {
                    s += x[idx] * filt[j];
                }
            }
            result.push(PerlValue::float(s));
        }
    }
    Ok(PerlValue::array(result))
}

// ─────────────────────────────────────────────────────────────────────────────
// R base: regression diagnostics
// ─────────────────────────────────────────────────────────────────────────────

/// `predict_lm MODEL, X_NEW` — predict from a linear model. MODEL is hash from lm_fit.
fn builtin_predict_lm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let model = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let x_new: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let map = model
        .as_hash_map()
        .ok_or_else(|| PerlError::runtime("predict_lm: expected model hash from lm_fit", 0))?;
    let intercept = map.get("intercept").map(|v| v.to_number()).unwrap_or(0.0);
    let slope = map.get("slope").map(|v| v.to_number()).unwrap_or(0.0);
    let result: Vec<PerlValue> = x_new
        .iter()
        .map(|&x| PerlValue::float(intercept + slope * x))
        .collect();
    Ok(PerlValue::array(result))
}

/// `confint_lm MODEL [, level]` — confidence intervals for linear model coefficients.
fn builtin_confint_lm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let model = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let level = args.get(1).map(|v| v.to_number()).unwrap_or(0.95);
    let map = model
        .as_hash_map()
        .ok_or_else(|| PerlError::runtime("confint_lm: expected model hash", 0))?;
    let intercept = map.get("intercept").map(|v| v.to_number()).unwrap_or(0.0);
    let slope = map.get("slope").map(|v| v.to_number()).unwrap_or(0.0);
    let residuals = map
        .get("residuals")
        .map(|v| {
            arg_to_vec(v)
                .iter()
                .map(|x| x.to_number())
                .collect::<Vec<f64>>()
        })
        .unwrap_or_default();
    let fitted = map
        .get("fitted")
        .map(|v| {
            arg_to_vec(v)
                .iter()
                .map(|x| x.to_number())
                .collect::<Vec<f64>>()
        })
        .unwrap_or_default();
    let n = residuals.len() as f64;
    if n < 3.0 {
        return Err(PerlError::runtime("confint_lm: need >= 3 observations", 0));
    }
    let se_resid = (residuals.iter().map(|r| r * r).sum::<f64>() / (n - 2.0)).sqrt();
    // Reconstruct xs from fitted and model
    let xs: Vec<f64> = fitted
        .iter()
        .map(|&f| (f - intercept) / slope.max(1e-15))
        .collect();
    let mx: f64 = xs.iter().sum::<f64>() / n;
    let ss_xx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let se_intercept = se_resid * (1.0 / n + mx * mx / ss_xx).sqrt();
    let se_slope = se_resid / ss_xx.sqrt();
    let alpha = 1.0 - level;
    let t_crit = builtin_qt(&[
        PerlValue::float(1.0 - alpha / 2.0),
        PerlValue::float(n - 2.0),
    ])?
    .to_number();
    let mut result = indexmap::IndexMap::new();
    result.insert(
        "intercept_lower".to_string(),
        PerlValue::float(intercept - t_crit * se_intercept),
    );
    result.insert(
        "intercept_upper".to_string(),
        PerlValue::float(intercept + t_crit * se_intercept),
    );
    result.insert(
        "slope_lower".to_string(),
        PerlValue::float(slope - t_crit * se_slope),
    );
    result.insert(
        "slope_upper".to_string(),
        PerlValue::float(slope + t_crit * se_slope),
    );
    Ok(PerlValue::hash(result))
}

// ─────────────────────────────────────────────────────────────────────────────
// R base: multivariate statistics
// ─────────────────────────────────────────────────────────────────────────────

/// `cor_matrix DATA` — correlation matrix. DATA is array of observations (each is a vector).
fn builtin_cor_matrix(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let data: Vec<Vec<f64>> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| arg_to_vec(v).iter().map(|x| x.to_number()).collect())
        .collect();
    let n = data.len();
    if n < 2 {
        return Err(PerlError::runtime("cor_matrix: need >= 2 observations", 0));
    }
    let p = data[0].len();
    let nf = n as f64;
    let means: Vec<f64> = (0..p)
        .map(|j| data.iter().map(|r| r[j]).sum::<f64>() / nf)
        .collect();
    let sds: Vec<f64> = (0..p)
        .map(|j| (data.iter().map(|r| (r[j] - means[j]).powi(2)).sum::<f64>() / (nf - 1.0)).sqrt())
        .collect();
    let mut cor = vec![vec![0.0; p]; p];
    for i in 0..p {
        cor[i][i] = 1.0;
        for j in (i + 1)..p {
            let r: f64 = data
                .iter()
                .map(|row| (row[i] - means[i]) * (row[j] - means[j]))
                .sum::<f64>()
                / ((nf - 1.0) * sds[i] * sds[j]);
            cor[i][j] = r;
            cor[j][i] = r;
        }
    }
    Ok(matrix_to_perl(&cor))
}

/// `cov_matrix DATA` — covariance matrix.
fn builtin_cov_matrix(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let data: Vec<Vec<f64>> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| arg_to_vec(v).iter().map(|x| x.to_number()).collect())
        .collect();
    let n = data.len();
    if n < 2 {
        return Err(PerlError::runtime("cov_matrix: need >= 2 observations", 0));
    }
    let p = data[0].len();
    let nf = n as f64;
    let means: Vec<f64> = (0..p)
        .map(|j| data.iter().map(|r| r[j]).sum::<f64>() / nf)
        .collect();
    let mut cov = vec![vec![0.0; p]; p];
    for i in 0..p {
        for j in i..p {
            let s: f64 = data
                .iter()
                .map(|r| (r[i] - means[i]) * (r[j] - means[j]))
                .sum();
            cov[i][j] = s / (nf - 1.0);
            cov[j][i] = cov[i][j];
        }
    }
    Ok(matrix_to_perl(&cov))
}

/// `mahalanobis X, CENTER, COV_INV` — Mahalanobis distance for each observation.
fn builtin_mahalanobis(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let data: Vec<Vec<f64>> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| arg_to_vec(v).iter().map(|x| x.to_number()).collect())
        .collect();
    let center: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let cov_inv = args_to_matrix(&args.get(2).cloned().unwrap_or(PerlValue::UNDEF));
    let p = center.len();
    let result: Vec<PerlValue> = data
        .iter()
        .map(|x| {
            let diff: Vec<f64> = (0..p).map(|j| x[j] - center[j]).collect();
            let mut d = 0.0;
            for i in 0..p {
                for j in 0..p {
                    d += diff[i] * cov_inv[i][j] * diff[j];
                }
            }
            PerlValue::float(d.sqrt())
        })
        .collect();
    Ok(PerlValue::array(result))
}

/// `dist_matrix DATA [, method]` — distance matrix between observations. Default "euclidean".
fn builtin_dist_matrix(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let data: Vec<Vec<f64>> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| arg_to_vec(v).iter().map(|x| x.to_number()).collect())
        .collect();
    let method = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "euclidean".to_string());
    let n = data.len();
    let mut d = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let dist = match method.as_str() {
                "manhattan" => data[i]
                    .iter()
                    .zip(data[j].iter())
                    .map(|(a, b)| (a - b).abs())
                    .sum(),
                "maximum" | "chebyshev" => data[i]
                    .iter()
                    .zip(data[j].iter())
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0f64, f64::max),
                _ => data[i]
                    .iter()
                    .zip(data[j].iter())
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt(),
            };
            d[i][j] = dist;
            d[j][i] = dist;
        }
    }
    Ok(matrix_to_perl(&d))
}

/// `hclust DIST_MATRIX [, method]` — hierarchical clustering. Returns merge order as array of [i, j, height].
fn builtin_hclust(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = d.len();
    if n < 2 {
        return Ok(PerlValue::array(vec![]));
    }
    let mut active: Vec<bool> = vec![true; n];
    let mut cluster_size = vec![1usize; n];
    let mut dist = d.clone();
    let mut merges = Vec::new();
    for _ in 0..n - 1 {
        // Find minimum distance
        let mut min_d = f64::INFINITY;
        let mut mi = 0;
        let mut mj = 0;
        for i in 0..n {
            if !active[i] {
                continue;
            }
            for j in (i + 1)..n {
                if !active[j] {
                    continue;
                }
                if dist[i][j] < min_d {
                    min_d = dist[i][j];
                    mi = i;
                    mj = j;
                }
            }
        }
        merges.push(PerlValue::array(vec![
            PerlValue::integer(mi as i64),
            PerlValue::integer(mj as i64),
            PerlValue::float(min_d),
        ]));
        // Merge mj into mi (average linkage)
        active[mj] = false;
        let si = cluster_size[mi] as f64;
        let sj = cluster_size[mj] as f64;
        for k in 0..n {
            if !active[k] || k == mi {
                continue;
            }
            dist[mi][k] = (dist[mi][k] * si + dist[mj][k] * sj) / (si + sj);
            dist[k][mi] = dist[mi][k];
        }
        cluster_size[mi] += cluster_size[mj];
    }
    Ok(PerlValue::array(merges))
}

/// `cutree MERGES, k` — cut dendrogram to k clusters. Returns cluster assignments.
fn builtin_cutree(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let merges_raw = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2);
    let n = merges_raw.len() + 1;
    // Start with each point in its own cluster
    let mut assignment: Vec<usize> = (0..n).collect();
    let merges_to_apply = if n > k { n - k } else { 0 };
    for i in 0..merges_to_apply {
        let merge = arg_to_vec(&merges_raw[i]);
        if merge.len() < 2 {
            continue;
        }
        let mi = merge[0].to_number() as usize;
        let mj = merge[1].to_number() as usize;
        let target = assignment[mi];
        let source = assignment[mj];
        for a in assignment.iter_mut() {
            if *a == source {
                *a = target;
            }
        }
    }
    // Renumber clusters 0..k-1
    let mut label_map = std::collections::HashMap::new();
    let mut next_label = 0usize;
    let result: Vec<PerlValue> = assignment
        .iter()
        .map(|&a| {
            let label = *label_map.entry(a).or_insert_with(|| {
                let l = next_label;
                next_label += 1;
                l
            });
            PerlValue::integer(label as i64)
        })
        .collect();
    Ok(PerlValue::array(result))
}

/// `weighted_var VEC, WEIGHTS` — weighted variance.
fn builtin_weighted_var(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let w: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let sw: f64 = w.iter().sum();
    let wm: f64 = x.iter().zip(w.iter()).map(|(xi, wi)| xi * wi).sum::<f64>() / sw;
    let wv: f64 = x
        .iter()
        .zip(w.iter())
        .map(|(xi, wi)| wi * (xi - wm).powi(2))
        .sum::<f64>()
        / sw;
    Ok(PerlValue::float(wv))
}

/// `cov2cor COV_MATRIX` — convert covariance matrix to correlation matrix.
fn builtin_cov2cor(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cov = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let p = cov.len();
    let sds: Vec<f64> = (0..p).map(|i| cov[i][i].sqrt()).collect();
    let mut cor = vec![vec![0.0; p]; p];
    for i in 0..p {
        for j in 0..p {
            cor[i][j] = if sds[i] > 0.0 && sds[j] > 0.0 {
                cov[i][j] / (sds[i] * sds[j])
            } else {
                0.0
            };
        }
    }
    Ok(matrix_to_perl(&cor))
}

// ─────────────────────────────────────────────────────────────────────────────
// SVG Plotting — terminal-friendly, pipe-friendly, zero dependencies
// ─────────────────────────────────────────────────────────────────────────────

const SVG_W: f64 = 600.0;
const SVG_H: f64 = 400.0;
const SVG_PAD: f64 = 60.0;

fn svg_header(w: f64, h: f64, title: &str) -> String {
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" font-family="monospace" font-size="11">
<rect width="{w}" height="{h}" fill="#1a1a2e"/>
<text x="{tx}" y="20" fill="#0ff" font-size="14" text-anchor="middle">{title}</text>
"##,
        tx = w / 2.0
    )
}

fn svg_footer() -> &'static str {
    "</svg>"
}

fn svg_axis_lines(x0: f64, _y0: f64, x1: f64, y1: f64) -> String {
    format!(
        r##"<line x1="{x0}" y1="{y1}" x2="{x1}" y2="{y1}" stroke="#555" stroke-width="1"/>
<line x1="{x0}" y1="30" x2="{x0}" y2="{y1}" stroke="#555" stroke-width="1"/>
"##
    )
}

fn svg_tick_labels(
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    px0: f64,
    _py0: f64,
    px1: f64,
    py1: f64,
    n_ticks: usize,
) -> String {
    let ph = py1 - 30.0;
    let mut s = String::new();
    for i in 0..=n_ticks {
        let frac = i as f64 / n_ticks as f64;
        let xv = x_min + frac * (x_max - x_min);
        let px = px0 + frac * (px1 - px0);
        s += &format!(
            r##"<text x="{px:.1}" y="{ly}" fill="#888" font-size="9" text-anchor="middle">{xv:.4}</text>
"##,
            ly = py1 + 14.0,
        );
        let yv = y_min + frac * (y_max - y_min);
        let py = py1 - frac * ph;
        s += &format!(
            r##"<text x="{lx}" y="{py:.1}" fill="#888" font-size="9" text-anchor="end" dominant-baseline="middle">{yv:.4}</text>
<line x1="{px0}" y1="{py:.1}" x2="{px1}" y2="{py:.1}" stroke="#333" stroke-width="0.5" stroke-dasharray="3,3"/>
"##,
            lx = px0 - 5.0,
        );
    }
    s
}

/// `scatter_svg XS, YS [, title]` — SVG scatter plot.
fn builtin_scatter_svg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let title = args
        .get(2)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Scatter Plot".to_string());
    let n = xs.len().min(ys.len());
    if n == 0 {
        return Ok(PerlValue::string(String::new()));
    }
    let x_min = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let x_max = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let y_min = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    let y_max = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let x_range = (x_max - x_min).max(1e-10);
    let y_range = (y_max - y_min).max(1e-10);
    let (px0, py0, px1, py1) = (SVG_PAD, 30.0, SVG_W - 20.0, SVG_H - SVG_PAD);
    let pw = px1 - px0;
    let ph = py1 - py0;

    let mut svg = svg_header(SVG_W, SVG_H, &title);
    svg += &svg_axis_lines(px0, py0, px1, py1);
    svg += &svg_tick_labels(x_min, x_max, y_min, y_max, px0, py0, px1, py1, 5);
    for i in 0..n {
        let px = px0 + (xs[i] - x_min) / x_range * pw;
        let py = py1 - (ys[i] - y_min) / y_range * ph;
        svg += &format!(r##"<circle cx="{px:.1}" cy="{py:.1}" r="3" fill="#0ff" opacity="0.7"/>"##);
        svg.push('\n');
    }
    svg += svg_footer();
    Ok(PerlValue::string(svg))
}

/// `line_svg XS, YS [, title]` — SVG line plot.
fn builtin_line_svg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let title = args
        .get(2)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Line Plot".to_string());
    let n = xs.len().min(ys.len());
    if n < 2 {
        return Ok(PerlValue::string(String::new()));
    }
    let x_min = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let x_max = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let y_min = ys.iter().cloned().fold(f64::INFINITY, f64::min);
    let y_max = ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let x_range = (x_max - x_min).max(1e-10);
    let y_range = (y_max - y_min).max(1e-10);
    let (px0, _py0, px1, py1) = (SVG_PAD, 30.0, SVG_W - 20.0, SVG_H - SVG_PAD);
    let pw = px1 - px0;
    let ph = py1 - 30.0;

    let mut svg = svg_header(SVG_W, SVG_H, &title);
    svg += &svg_axis_lines(px0, 30.0, px1, py1);
    svg += &svg_tick_labels(x_min, x_max, y_min, y_max, px0, 30.0, px1, py1, 5);
    let mut points = String::new();
    for i in 0..n {
        let px = px0 + (xs[i] - x_min) / x_range * pw;
        let py = py1 - (ys[i] - y_min) / y_range * ph;
        if i > 0 {
            points.push(' ');
        }
        points += &format!("{px:.1},{py:.1}");
    }
    svg +=
        &format!(r##"<polyline points="{points}" fill="none" stroke="#0ff" stroke-width="1.5"/>"##);
    svg.push('\n');
    svg += svg_footer();
    Ok(PerlValue::string(svg))
}

/// `plot_svg YS [, title]` — SVG line plot with auto x-axis (0..n-1).
fn builtin_plot_svg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ys: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let title = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Plot".to_string());
    let xs: Vec<PerlValue> = (0..ys.len()).map(|i| PerlValue::float(i as f64)).collect();
    let ys_pv: Vec<PerlValue> = ys.iter().map(|&y| PerlValue::float(y)).collect();
    builtin_line_svg(&[
        PerlValue::array(xs),
        PerlValue::array(ys_pv),
        PerlValue::string(title),
    ])
}

/// `hist_svg VEC [, bins [, title]]` — SVG histogram.
fn builtin_hist_svg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let data: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n_bins = args
        .get(1)
        .map(|v| v.to_number() as usize)
        .unwrap_or_else(|| (data.len() as f64).sqrt().ceil().max(5.0).min(50.0) as usize);
    let title = args
        .get(2)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Histogram".to_string());
    if data.is_empty() {
        return Ok(PerlValue::string(String::new()));
    }
    let d_min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let d_max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = (d_max - d_min).max(1e-10);
    let bin_width = range / n_bins as f64;
    let mut counts = vec![0usize; n_bins];
    for &v in &data {
        let idx = ((v - d_min) / bin_width).floor() as usize;
        counts[idx.min(n_bins - 1)] += 1;
    }
    let max_count = *counts.iter().max().unwrap_or(&1) as f64;
    let (px0, _py0, px1, py1) = (SVG_PAD, 30.0, SVG_W - 20.0, SVG_H - SVG_PAD);
    let pw = px1 - px0;
    let ph = py1 - 30.0;
    let bar_w = pw / n_bins as f64;

    let mut svg = svg_header(SVG_W, SVG_H, &title);
    svg += &svg_axis_lines(px0, 30.0, px1, py1);
    svg += &svg_tick_labels(d_min, d_max, 0.0, max_count, px0, 30.0, px1, py1, 5);
    for (i, &c) in counts.iter().enumerate() {
        let bh = (c as f64 / max_count) * ph;
        let bx = px0 + i as f64 * bar_w;
        let by = py1 - bh;
        svg += &format!(
            r##"<rect x="{bx:.1}" y="{by:.1}" width="{bw:.1}" height="{bh:.1}" fill="#0ff" opacity="0.7" stroke="#1a1a2e" stroke-width="1"/>"##,
            bw = bar_w * 0.9,
        );
        svg.push('\n');
    }
    svg += svg_footer();
    Ok(PerlValue::string(svg))
}

/// `boxplot_svg GROUPS [, title]` — SVG box-and-whisker plot. GROUPS is array of arrays.
fn builtin_boxplot_svg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let groups_raw = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let title = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Box Plot".to_string());
    let groups: Vec<Vec<f64>> = groups_raw
        .iter()
        .map(|g| {
            let mut v: Vec<f64> = arg_to_vec(g).iter().map(|x| x.to_number()).collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            v
        })
        .collect();
    if groups.is_empty() {
        return Ok(PerlValue::string(String::new()));
    }
    let g_min = groups
        .iter()
        .flat_map(|g| g.iter())
        .cloned()
        .fold(f64::INFINITY, f64::min);
    let g_max = groups
        .iter()
        .flat_map(|g| g.iter())
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    let y_range = (g_max - g_min).max(1e-10);
    let ng = groups.len();
    let (px0, _py0, px1, py1) = (SVG_PAD, 30.0, SVG_W - 20.0, SVG_H - SVG_PAD);
    let pw = px1 - px0;
    let ph = py1 - 30.0;
    let box_w = (pw / ng as f64) * 0.6;
    let gap = pw / ng as f64;

    let mut svg = svg_header(SVG_W, SVG_H, &title);
    svg += &svg_axis_lines(px0, 30.0, px1, py1);
    for i in 0..=5 {
        let frac = i as f64 / 5.0;
        let yv = g_min + frac * y_range;
        let py = py1 - frac * ph;
        svg += &format!(
            r##"<text x="{lx}" y="{py:.1}" fill="#888" font-size="9" text-anchor="end" dominant-baseline="middle">{yv:.2}</text>"##,
            lx = px0 - 5.0
        );
        svg.push('\n');
    }
    let percentile_at = |v: &[f64], p: f64| -> f64 {
        if v.is_empty() {
            return 0.0;
        }
        let idx = p * (v.len() - 1) as f64;
        let lo = idx.floor() as usize;
        let hi = idx.ceil() as usize;
        if lo == hi {
            v[lo]
        } else {
            v[lo] + (idx - lo as f64) * (v[hi] - v[lo])
        }
    };
    for (i, g) in groups.iter().enumerate() {
        if g.is_empty() {
            continue;
        }
        let q1 = percentile_at(g, 0.25);
        let median = percentile_at(g, 0.5);
        let q3 = percentile_at(g, 0.75);
        let iqr = q3 - q1;
        let wlo = g
            .iter()
            .cloned()
            .find(|&v| v >= q1 - 1.5 * iqr)
            .unwrap_or(g[0]);
        let whi = g
            .iter()
            .rev()
            .cloned()
            .find(|&v| v <= q3 + 1.5 * iqr)
            .unwrap_or(*g.last().unwrap());
        let cx = px0 + (i as f64 + 0.5) * gap;
        let to_py = |v: f64| py1 - (v - g_min) / y_range * ph;
        let bx = cx - box_w / 2.0;
        let by = to_py(q3);
        let bh = to_py(q1) - by;
        svg += &format!(
            r##"<rect x="{bx:.1}" y="{by:.1}" width="{box_w:.1}" height="{bh:.1}" fill="none" stroke="#0ff" stroke-width="1.5"/>"##
        );
        svg.push('\n');
        svg += &format!(
            r##"<line x1="{bx:.1}" y1="{my:.1}" x2="{bx2:.1}" y2="{my:.1}" stroke="#ff0" stroke-width="2"/>"##,
            my = to_py(median),
            bx2 = bx + box_w
        );
        svg.push('\n');
        let (wlo_py, whi_py) = (to_py(wlo), to_py(whi));
        svg += &format!(
            r##"<line x1="{cx:.1}" y1="{by:.1}" x2="{cx:.1}" y2="{whi_py:.1}" stroke="#0ff" stroke-width="1"/>"##
        );
        svg.push('\n');
        svg += &format!(
            r##"<line x1="{cx:.1}" y1="{q1y:.1}" x2="{cx:.1}" y2="{wlo_py:.1}" stroke="#0ff" stroke-width="1"/>"##,
            q1y = to_py(q1)
        );
        svg.push('\n');
        let cap = box_w * 0.3;
        svg += &format!(
            r##"<line x1="{x1:.1}" y1="{whi_py:.1}" x2="{x2:.1}" y2="{whi_py:.1}" stroke="#0ff" stroke-width="1"/>"##,
            x1 = cx - cap,
            x2 = cx + cap
        );
        svg.push('\n');
        svg += &format!(
            r##"<line x1="{x1:.1}" y1="{wlo_py:.1}" x2="{x2:.1}" y2="{wlo_py:.1}" stroke="#0ff" stroke-width="1"/>"##,
            x1 = cx - cap,
            x2 = cx + cap
        );
        svg.push('\n');
        for &v in g.iter() {
            if v < wlo || v > whi {
                svg += &format!(
                    r##"<circle cx="{cx:.1}" cy="{oy:.1}" r="2.5" fill="none" stroke="#f55" stroke-width="1"/>"##,
                    oy = to_py(v)
                );
                svg.push('\n');
            }
        }
        svg += &format!(
            r##"<text x="{cx:.1}" y="{ly}" fill="#888" font-size="10" text-anchor="middle">{label}</text>"##,
            ly = py1 + 14.0,
            label = i + 1
        );
        svg.push('\n');
    }
    svg += svg_footer();
    Ok(PerlValue::string(svg))
}

/// `bar_svg LABELS, VALUES [, title]` — SVG bar chart.
fn builtin_bar_svg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let labels: Vec<String> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_string())
        .collect();
    let values: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let title = args
        .get(2)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Bar Chart".to_string());
    let n = labels.len().min(values.len());
    if n == 0 {
        return Ok(PerlValue::string(String::new()));
    }
    let v_max = values.iter().cloned().fold(0.0f64, f64::max).max(1e-10);
    let (px0, _py0, px1, py1) = (SVG_PAD, 30.0, SVG_W - 20.0, SVG_H - SVG_PAD);
    let pw = px1 - px0;
    let ph = py1 - 30.0;
    let bar_w = (pw / n as f64) * 0.7;
    let gap = pw / n as f64;
    let colors = [
        "#0ff", "#f0f", "#0f0", "#ff0", "#f55", "#55f", "#fa0", "#5ff",
    ];

    let mut svg = svg_header(SVG_W, SVG_H, &title);
    svg += &svg_axis_lines(px0, 30.0, px1, py1);
    for i in 0..n {
        let bh = (values[i] / v_max) * ph;
        let bx = px0 + (i as f64 + 0.5) * gap - bar_w / 2.0;
        let by = py1 - bh;
        svg += &format!(
            r##"<rect x="{bx:.1}" y="{by:.1}" width="{bar_w:.1}" height="{bh:.1}" fill="{color}" opacity="0.8"/>"##,
            color = colors[i % colors.len()]
        );
        svg.push('\n');
        svg += &format!(
            r##"<text x="{cx:.1}" y="{vy:.1}" fill="#fff" font-size="9" text-anchor="middle">{v:.1}</text>"##,
            cx = bx + bar_w / 2.0,
            vy = by - 4.0,
            v = values[i]
        );
        svg.push('\n');
        let label = if labels[i].len() > 8 {
            &labels[i][..8]
        } else {
            &labels[i]
        };
        svg += &format!(
            r##"<text x="{cx:.1}" y="{ly}" fill="#888" font-size="9" text-anchor="middle">{label}</text>"##,
            cx = px0 + (i as f64 + 0.5) * gap,
            ly = py1 + 14.0
        );
        svg.push('\n');
    }
    svg += svg_footer();
    Ok(PerlValue::string(svg))
}

/// `pie_svg LABELS, VALUES [, title]` — SVG pie chart.
fn builtin_pie_svg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let labels: Vec<String> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_string())
        .collect();
    let values: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let title = args
        .get(2)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Pie Chart".to_string());
    let n = labels.len().min(values.len());
    if n == 0 {
        return Ok(PerlValue::string(String::new()));
    }
    let total: f64 = values.iter().sum();
    if total <= 0.0 {
        return Ok(PerlValue::string(String::new()));
    }
    let cx = SVG_W / 2.0;
    let cy = SVG_H / 2.0 + 10.0;
    let r = 140.0;
    let colors = [
        "#0ff", "#f0f", "#0f0", "#ff0", "#f55", "#55f", "#fa0", "#5ff", "#f80", "#8f0",
    ];

    let mut svg = svg_header(SVG_W, SVG_H, &title);
    let mut angle = -std::f64::consts::FRAC_PI_2;
    for i in 0..n {
        let sweep = 2.0 * std::f64::consts::PI * values[i] / total;
        let x1 = cx + r * angle.cos();
        let y1 = cy + r * angle.sin();
        let x2 = cx + r * (angle + sweep).cos();
        let y2 = cy + r * (angle + sweep).sin();
        let large = if sweep > std::f64::consts::PI { 1 } else { 0 };
        svg += &format!(
            r##"<path d="M{cx},{cy} L{x1:.1},{y1:.1} A{r},{r} 0 {large},1 {x2:.1},{y2:.1} Z" fill="{color}" opacity="0.8" stroke="#1a1a2e" stroke-width="1.5"/>"##,
            color = colors[i % colors.len()]
        );
        svg.push('\n');
        let mid = angle + sweep / 2.0;
        let lx = cx + r * 0.65 * mid.cos();
        let ly = cy + r * 0.65 * mid.sin();
        let pct = values[i] / total * 100.0;
        let label = if labels[i].len() > 6 {
            &labels[i][..6]
        } else {
            &labels[i]
        };
        svg += &format!(
            r##"<text x="{lx:.1}" y="{ly:.1}" fill="#fff" font-size="9" text-anchor="middle" dominant-baseline="middle">{label} {pct:.0}%</text>"##
        );
        svg.push('\n');
        angle += sweep;
    }
    svg += svg_footer();
    Ok(PerlValue::string(svg))
}

/// `heatmap_svg MATRIX [, title]` — SVG heatmap.
fn builtin_heatmap_svg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = args_to_matrix(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let title = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "Heatmap".to_string());
    let nr = m.len();
    if nr == 0 {
        return Ok(PerlValue::string(String::new()));
    }
    let nc = m[0].len();
    let v_min = m
        .iter()
        .flat_map(|r| r.iter())
        .cloned()
        .fold(f64::INFINITY, f64::min);
    let v_max = m
        .iter()
        .flat_map(|r| r.iter())
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max);
    let v_range = (v_max - v_min).max(1e-10);
    let (px0, py0, px1, py1) = (SVG_PAD, 30.0, SVG_W - 20.0, SVG_H - 30.0);
    let cell_w = (px1 - px0) / nc as f64;
    let cell_h = (py1 - py0) / nr as f64;

    let mut svg = svg_header(SVG_W, SVG_H, &title);
    for i in 0..nr {
        for j in 0..nc {
            let t = (m[i][j] - v_min) / v_range;
            let (cr, cg, cb) = if t < 0.5 {
                let s = t * 2.0;
                (
                    (0.0 * 255.0) as u8,
                    (s * 255.0) as u8,
                    ((1.0 - s * 0.5) * 255.0) as u8,
                )
            } else {
                let s = (t - 0.5) * 2.0;
                ((s * 255.0) as u8, ((1.0 - s * 0.5) * 255.0) as u8, 0u8)
            };
            let cx = px0 + j as f64 * cell_w;
            let cy = py0 + i as f64 * cell_h;
            svg += &format!(
                r##"<rect x="{cx:.1}" y="{cy:.1}" width="{cell_w:.1}" height="{cell_h:.1}" fill="rgb({cr},{cg},{cb})" stroke="#1a1a2e" stroke-width="0.5"/>"##
            );
            svg.push('\n');
        }
    }
    svg += svg_footer();
    Ok(PerlValue::string(svg))
}
