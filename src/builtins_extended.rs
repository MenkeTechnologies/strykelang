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
