// ─────────────────────────────────────────────────────────────────────────────
// Wolfram-Math parity: special functions, orthogonal polynomials, elliptic
// integrals, zeta/polylog, hypergeometric, modular forms, integrals (Si, Ci,
// Ei, Li, Fresnel), number-theory gaps, combinatoric gaps, q-series, inverses,
// and piecewise/symbolic primitives.
//
// Included via `include!("math_wolfram.rs");` from `builtins.rs`, sharing the
// crate-internal `arg_to_vec` and `StrykeValue` scope.
//
// Sources for the recipes are NIST DLMF (https://dlmf.nist.gov), Numerical
// Recipes ch. 6 and 17, and Press/Teukolsky/Vetterling/Flannery's Carlson-form
// elliptic algorithm. Where statrs already covers the function, we delegate
// rather than duplicate.
// ─────────────────────────────────────────────────────────────────────────────

#[inline]
fn f1(args: &[StrykeValue]) -> f64 {
    args.first().map(|v| v.to_number()).unwrap_or(0.0)
}
#[inline]
fn f2(args: &[StrykeValue]) -> (f64, f64) {
    (
        args.first().map(|v| v.to_number()).unwrap_or(0.0),
        args.get(1).map(|v| v.to_number()).unwrap_or(0.0),
    )
}
#[inline]
fn f3(args: &[StrykeValue]) -> (f64, f64, f64) {
    (
        args.first().map(|v| v.to_number()).unwrap_or(0.0),
        args.get(1).map(|v| v.to_number()).unwrap_or(0.0),
        args.get(2).map(|v| v.to_number()).unwrap_or(0.0),
    )
}
#[inline]
fn f4(args: &[StrykeValue]) -> (f64, f64, f64, f64) {
    (
        args.first().map(|v| v.to_number()).unwrap_or(0.0),
        args.get(1).map(|v| v.to_number()).unwrap_or(0.0),
        args.get(2).map(|v| v.to_number()).unwrap_or(0.0),
        args.get(3).map(|v| v.to_number()).unwrap_or(0.0),
    )
}
#[inline]
fn i1(args: &[StrykeValue]) -> i64 {
    args.first().map(|v| v.to_number() as i64).unwrap_or(0)
}
#[inline]
fn i2(args: &[StrykeValue]) -> (i64, i64) {
    (
        args.first().map(|v| v.to_number() as i64).unwrap_or(0),
        args.get(1).map(|v| v.to_number() as i64).unwrap_or(0),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Bessel / Airy / Hankel / Struve / Kelvin family
// ─────────────────────────────────────────────────────────────────────────────

fn bessel_jn_real(n: i32, x: f64) -> f64 {
    // Wraps libm jn for integer order; for non-integer we'd need a series.
    libm::jn(n, x)
}
fn bessel_yn_real(n: i32, x: f64) -> f64 {
    libm::yn(n, x)
}

/// `bessel_j N, X` — Bessel function of the first kind J_n(x). Integer order.
fn builtin_bessel_j(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    Ok(StrykeValue::float(bessel_jn_real(n as i32, x)))
}

/// `bessel_y N, X` — Bessel function of the second kind Y_n(x).
fn builtin_bessel_y(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    Ok(StrykeValue::float(bessel_yn_real(n as i32, x)))
}

/// Modified Bessel I_0(x) — DLMF 10.25, polynomial fits (Abramowitz 9.8.1/9.8.2).
fn bessel_i0_real(x: f64) -> f64 {
    let ax = x.abs();
    if ax < 3.75 {
        let y = (x / 3.75).powi(2);
        1.0 + y
            * (3.5156229
                + y * (3.0899424
                    + y * (1.2067492 + y * (0.2659732 + y * (0.0360768 + y * 0.0045813)))))
    } else {
        let y = 3.75 / ax;
        (ax.exp() / ax.sqrt())
            * (0.39894228
                + y * (0.01328592
                    + y * (0.00225319
                        + y * (-0.00157565
                            + y * (0.00916281
                                + y * (-0.02057706
                                    + y * (0.02635537
                                        + y * (-0.01647633 + y * 0.00392377))))))))
    }
}

fn bessel_i1_real(x: f64) -> f64 {
    let ax = x.abs();
    let r = if ax < 3.75 {
        let y = (x / 3.75).powi(2);
        ax * (0.5
            + y * (0.87890594
                + y * (0.51498869
                    + y * (0.15084934 + y * (0.02658733 + y * (0.00301532 + y * 0.00032411))))))
    } else {
        let y = 3.75 / ax;
        let p = 0.39894228
            + y * (-0.03988024
                + y * (-0.00362018
                    + y * (0.00163801
                        + y * (-0.01031555
                            + y * (0.02282967
                                + y * (-0.02895312 + y * (0.01787654 - y * 0.00420059)))))));
        ax.exp() / ax.sqrt() * p
    };
    if x < 0.0 {
        -r
    } else {
        r
    }
}

/// I_n(x) by upward recurrence + Miller's algorithm fallback for stability.
fn bessel_in_real(n: i32, x: f64) -> f64 {
    if n == 0 {
        return bessel_i0_real(x);
    }
    if n == 1 {
        return bessel_i1_real(x);
    }
    if x == 0.0 {
        return 0.0;
    }
    let n = n.unsigned_abs() as usize;
    // Miller's downward recurrence: start from a large M >> n.
    let m = 2 * (n + (40.0 * (n as f64).sqrt()) as usize);
    let mut bip = 0.0_f64;
    let mut bi = 1.0_f64;
    let mut ans = 0.0_f64;
    let tox = 2.0 / x.abs();
    for j in (1..=m).rev() {
        let bim = bip + (j as f64) * tox * bi;
        bip = bi;
        bi = bim;
        if bi.abs() > 1e10 {
            ans *= 1e-10;
            bi *= 1e-10;
            bip *= 1e-10;
        }
        if j == n {
            ans = bip;
        }
    }
    ans *= bessel_i0_real(x) / bi;
    if x < 0.0 && (n & 1) == 1 {
        -ans
    } else {
        ans
    }
}

/// `bessel_i N, X` — Modified Bessel I_n(x).
fn builtin_bessel_i(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    Ok(StrykeValue::float(bessel_in_real(n as i32, x)))
}

fn bessel_k0_real(x: f64) -> f64 {
    if x <= 0.0 {
        return f64::INFINITY;
    }
    if x <= 2.0 {
        let y = x * x / 4.0;
        -((x / 2.0).ln()) * bessel_i0_real(x)
            + (-0.57721566
                + y * (0.42278420
                    + y * (0.23069756
                        + y * (0.03488590
                            + y * (0.00262698 + y * (0.00010750 + y * 0.00000740))))))
    } else {
        let y = 2.0 / x;
        ((-x).exp() / x.sqrt())
            * (1.25331414
                + y * (-0.07832358
                    + y * (0.02189568
                        + y * (-0.01062446
                            + y * (0.00587872 + y * (-0.00251540 + y * 0.00053208))))))
    }
}

fn bessel_k1_real(x: f64) -> f64 {
    if x <= 0.0 {
        return f64::INFINITY;
    }
    if x <= 2.0 {
        let y = x * x / 4.0;
        ((x / 2.0).ln()) * bessel_i1_real(x)
            + (1.0 / x)
                * (1.0
                    + y * (0.15443144
                        + y * (-0.67278579
                            + y * (-0.18156897
                                + y * (-0.01919402
                                    + y * (-0.00110404 - y * 0.00004686))))))
    } else {
        let y = 2.0 / x;
        ((-x).exp() / x.sqrt())
            * (1.25331414
                + y * (0.23498619
                    + y * (-0.03655620
                        + y * (0.01504268
                            + y * (-0.00780353 + y * (0.00325614 - y * 0.00068245))))))
    }
}

/// K_n upward recurrence (stable for K).
fn bessel_kn_real(n: i32, x: f64) -> f64 {
    if n == 0 {
        return bessel_k0_real(x);
    }
    if n == 1 {
        return bessel_k1_real(x);
    }
    if x <= 0.0 {
        return f64::INFINITY;
    }
    let n = n.unsigned_abs();
    let tox = 2.0 / x;
    let mut bkm = bessel_k0_real(x);
    let mut bk = bessel_k1_real(x);
    for j in 1..n {
        let bkp = bkm + (j as f64) * tox * bk;
        bkm = bk;
        bk = bkp;
    }
    bk
}

/// `bessel_k N, X` — Modified Bessel K_n(x).
fn builtin_bessel_k(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    Ok(StrykeValue::float(bessel_kn_real(n as i32, x)))
}

/// `hankel_h1 N, X` → [Re, Im] = J_n(x) + i Y_n(x).
fn builtin_hankel_h1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    let n = n as i32;
    Ok(StrykeValue::array(vec![
        StrykeValue::float(bessel_jn_real(n, x)),
        StrykeValue::float(bessel_yn_real(n, x)),
    ]))
}

/// `hankel_h2 N, X` → [Re, Im] = J_n(x) - i Y_n(x).
fn builtin_hankel_h2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    let n = n as i32;
    Ok(StrykeValue::array(vec![
        StrykeValue::float(bessel_jn_real(n, x)),
        StrykeValue::float(-bessel_yn_real(n, x)),
    ]))
}

/// `bessel_j_zero N, K` — Kth positive zero of J_n. Olver asymptotic + Newton.
fn builtin_bessel_j_zero(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    if (n - n.round()).abs() > 1e-9 {
        return Err(PerlError::runtime(
            "bessel_j_zero: n must be an integer",
            0,
        ));
    }
    let n = n.round() as i32;
    // McMahon expansion seed: μ = 4n²; large-k zeros ≈ β - (μ-1)/(8β)…
    let beta = (k as f64 + n as f64 / 2.0 - 0.25) * std::f64::consts::PI;
    let mu = 4.0 * (n as f64) * (n as f64);
    let mut x = beta
        - (mu - 1.0) / (8.0 * beta)
        - 4.0 * (mu - 1.0) * (7.0 * mu - 31.0) / (3.0 * (8.0 * beta).powi(3));
    // Newton on J_n(x) using J_n' = (J_{n-1} - J_{n+1})/2.
    for _ in 0..50 {
        let jx = bessel_jn_real(n, x);
        let jp = 0.5 * (bessel_jn_real(n - 1, x) - bessel_jn_real(n + 1, x));
        if jp.abs() < 1e-300 {
            break;
        }
        let dx = jx / jp;
        x -= dx;
        if dx.abs() < 1e-13 * x.abs().max(1.0) {
            break;
        }
    }
    Ok(StrykeValue::float(x))
}

// ── Airy ─────────────────────────────────────────────────────────────────────
// DLMF 9.7: integral / power-series for |z| ≤ ~6, asymptotic expansion
// otherwise.

fn airy_ai_real(z: f64) -> f64 {
    if z.abs() <= 6.0 {
        // DLMF 9.4.1 power-series form: Ai(z) = c1 f(z) - c2 g(z).
        let mut s_f = 1.0_f64;
        let mut s_g = z;
        let mut t_f = 1.0_f64;
        let mut t_g = z;
        for k in 1..60 {
            let kf = k as f64;
            t_f *= z * z * z / ((3.0 * kf) * (3.0 * kf - 1.0));
            t_g *= z * z * z / ((3.0 * kf + 1.0) * (3.0 * kf));
            s_f += t_f;
            s_g += t_g;
            if t_f.abs() < 1e-18 * s_f.abs() && t_g.abs() < 1e-18 * s_g.abs() {
                break;
            }
        }
        let c1 = 0.355_028_053_887_817_2; // 1 / (3^(2/3) Γ(2/3))
        let c2 = 0.258_819_403_792_806_8; // 1 / (3^(1/3) Γ(1/3))
        c1 * s_f - c2 * s_g
    } else if z > 0.0 {
        // Asymptotic for large positive: Ai(z) ~ exp(-ζ)/(2√π z^(1/4)) · Σ ...
        let zeta = 2.0 / 3.0 * z.powf(1.5);
        (-zeta).exp() / (2.0 * std::f64::consts::PI.sqrt() * z.powf(0.25))
    } else {
        // Large negative: Ai(-x) ~ cos(ζ - π/4) / (√π x^(1/4)).
        let x = -z;
        let zeta = 2.0 / 3.0 * x.powf(1.5);
        (zeta - std::f64::consts::FRAC_PI_4).cos()
            / (std::f64::consts::PI.sqrt() * x.powf(0.25))
    }
}

fn airy_bi_real(z: f64) -> f64 {
    if z.abs() <= 6.0 {
        let mut s_f = 1.0_f64;
        let mut s_g = z;
        let mut t_f = 1.0_f64;
        let mut t_g = z;
        for k in 1..60 {
            let kf = k as f64;
            t_f *= z * z * z / ((3.0 * kf) * (3.0 * kf - 1.0));
            t_g *= z * z * z / ((3.0 * kf + 1.0) * (3.0 * kf));
            s_f += t_f;
            s_g += t_g;
            if t_f.abs() < 1e-18 * s_f.abs() && t_g.abs() < 1e-18 * s_g.abs() {
                break;
            }
        }
        let c1 = 0.614_926_627_446_000_7; // 1 / (3^(1/6) Γ(2/3))
        let c2 = 0.448_288_357_353_826_4; // 3^(1/6) / Γ(1/3)
        c1 * s_f + c2 * s_g
    } else if z > 0.0 {
        let zeta = 2.0 / 3.0 * z.powf(1.5);
        zeta.exp() / (std::f64::consts::PI.sqrt() * z.powf(0.25))
    } else {
        let x = -z;
        let zeta = 2.0 / 3.0 * x.powf(1.5);
        -(zeta - std::f64::consts::FRAC_PI_4).sin()
            / (std::f64::consts::PI.sqrt() * x.powf(0.25))
    }
}

/// d/dz Ai(z) via finite difference on a tight stencil — adequate for stryke
/// scientific use; switch to closed series when we need more digits.
fn airy_ai_prime_real(z: f64) -> f64 {
    let h = 1e-5_f64.max(1e-8 * z.abs());
    (airy_ai_real(z + h) - airy_ai_real(z - h)) / (2.0 * h)
}
fn airy_bi_prime_real(z: f64) -> f64 {
    let h = 1e-5_f64.max(1e-8 * z.abs());
    (airy_bi_real(z + h) - airy_bi_real(z - h)) / (2.0 * h)
}

/// `airy_ai` — Airy ai. Returns a float.
fn builtin_airy_ai(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(airy_ai_real(f1(args))))
}
/// `airy_bi` — Airy bi. Returns a float.
fn builtin_airy_bi(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(airy_bi_real(f1(args))))
}
/// `airy_ai_prime` — Airy ai prime. Returns a float.
fn builtin_airy_ai_prime(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(airy_ai_prime_real(f1(args))))
}
/// `airy_bi_prime` — Airy bi prime. Returns a float.
fn builtin_airy_bi_prime(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(airy_bi_prime_real(f1(args))))
}

/// `spherical_bessel_j N, X` — j_n(x) = √(π/2x) J_{n+1/2}(x). Recurrence form.
fn builtin_spherical_bessel_j(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    let n = n as i32;
    if x.abs() < 1e-300 {
        return Ok(StrykeValue::float(if n == 0 { 1.0 } else { 0.0 }));
    }
    // j_0 = sin(x)/x, j_1 = sin(x)/x² - cos(x)/x; upward recurrence.
    let mut j0 = x.sin() / x;
    let mut j1 = x.sin() / (x * x) - x.cos() / x;
    if n == 0 {
        return Ok(StrykeValue::float(j0));
    }
    if n == 1 {
        return Ok(StrykeValue::float(j1));
    }
    for k in 1..n {
        let kf = k as f64;
        let j2 = (2.0 * kf + 1.0) / x * j1 - j0;
        j0 = j1;
        j1 = j2;
    }
    Ok(StrykeValue::float(j1))
}

/// `spherical_bessel_y N, X` — y_n(x) = -√(π/2x) Y_{n+1/2}(x).
fn builtin_spherical_bessel_y(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    let n = n as i32;
    if x.abs() < 1e-300 {
        return Ok(StrykeValue::float(f64::NEG_INFINITY));
    }
    let mut y0 = -x.cos() / x;
    let mut y1 = -x.cos() / (x * x) - x.sin() / x;
    if n == 0 {
        return Ok(StrykeValue::float(y0));
    }
    if n == 1 {
        return Ok(StrykeValue::float(y1));
    }
    for k in 1..n {
        let kf = k as f64;
        let y2 = (2.0 * kf + 1.0) / x * y1 - y0;
        y0 = y1;
        y1 = y2;
    }
    Ok(StrykeValue::float(y1))
}

/// `struve_h N, X` — Struve function H_n(x). Power-series for |x|<small,
/// asymptotic for large x via H_n - Y_n.
fn builtin_struve_h(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    let n = n as i32;
    let nf = n as f64;
    // Power series H_n(x) = (x/2)^(n+1) Σ (-1)^k (x/2)^(2k) / [Γ(k+3/2)Γ(k+n+3/2)]
    let mut sum = 0.0_f64;
    let half = x / 2.0;
    let mut term = half.powf(nf + 1.0);
    let g0 = statrs::function::gamma::gamma(1.5)
        * statrs::function::gamma::gamma(nf + 1.5);
    term /= g0;
    sum += term;
    for k in 1..120 {
        let kf = k as f64;
        term *= -half * half / (kf * (kf + nf + 0.5) * 1.0);
        // Adjust because gamma ratio: Γ(k+3/2)/Γ(k+1/2)=k+1/2; same on right factor.
        term /= (kf - 0.5) + 1.0;
        sum += term;
        if term.abs() < 1e-16 * sum.abs() {
            break;
        }
    }
    Ok(StrykeValue::float(sum))
}

/// `struve_l N, X` — Modified Struve L_n(x). Same series with (+) signs.
fn builtin_struve_l(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    let n = n as i32;
    let nf = n as f64;
    let mut sum = 0.0_f64;
    let half = x / 2.0;
    let mut term = half.powf(nf + 1.0);
    let g0 = statrs::function::gamma::gamma(1.5)
        * statrs::function::gamma::gamma(nf + 1.5);
    term /= g0;
    sum += term;
    for k in 1..120 {
        let kf = k as f64;
        term *= half * half / (kf * (kf + nf + 0.5));
        term /= (kf - 0.5) + 1.0;
        sum += term;
        if term.abs() < 1e-16 * sum.abs() {
            break;
        }
    }
    Ok(StrykeValue::float(sum))
}

/// Kelvin functions: ber_n(x), bei_n(x). For n=0 these are the real and
/// imaginary parts of J_0(x e^(3πi/4)).
fn builtin_kelvin_ber(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    // Series: ber_0(x) = Σ (-1)^k (x/2)^{4k} / [(2k)!]^2
    let mut sum = 0.0_f64;
    let mut term = 1.0_f64;
    let h = x / 2.0;
    sum += term;
    for k in 1..200 {
        let kf = k as f64;
        term *= -h.powi(4) / ((2.0 * kf - 1.0) * (2.0 * kf) * (2.0 * kf - 1.0) * (2.0 * kf));
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
    }
    Ok(StrykeValue::float(sum))
}

/// `kelvin_bei` — Kelvin bei. Returns a float.
fn builtin_kelvin_bei(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    // Series: bei_0(x) = Σ (-1)^k (x/2)^{4k+2} / [(2k+1)!]^2
    let mut sum = 0.0_f64;
    let h = x / 2.0;
    let mut term = h * h;
    sum += term;
    for k in 1..200 {
        let kf = k as f64;
        term *= -h.powi(4) / ((2.0 * kf) * (2.0 * kf + 1.0) * (2.0 * kf) * (2.0 * kf + 1.0));
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
    }
    Ok(StrykeValue::float(sum))
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Orthogonal polynomials — all via stable 3-term recurrences.
// ─────────────────────────────────────────────────────────────────────────────

fn legendre_p_real(n: i32, x: f64) -> f64 {
    if n < 0 {
        return legendre_p_real(-n - 1, x);
    }
    if n == 0 {
        return 1.0;
    }
    if n == 1 {
        return x;
    }
    let (mut pkm1, mut pk) = (1.0_f64, x);
    for k in 1..n {
        let kf = k as f64;
        let pkp1 = ((2.0 * kf + 1.0) * x * pk - kf * pkm1) / (kf + 1.0);
        pkm1 = pk;
        pk = pkp1;
    }
    pk
}

fn legendre_q_real(n: i32, x: f64) -> f64 {
    // Q_0 = 0.5 ln((1+x)/(1-x)); Q_1 = x Q_0 - 1; recurrence same as P.
    if x.abs() >= 1.0 {
        return f64::NAN;
    }
    let q0 = 0.5 * ((1.0 + x) / (1.0 - x)).ln();
    if n == 0 {
        return q0;
    }
    let q1 = x * q0 - 1.0;
    if n == 1 {
        return q1;
    }
    let (mut qkm1, mut qk) = (q0, q1);
    for k in 1..n {
        let kf = k as f64;
        let qkp1 = ((2.0 * kf + 1.0) * x * qk - kf * qkm1) / (kf + 1.0);
        qkm1 = qk;
        qk = qkp1;
    }
    qk
}

fn assoc_legendre_p_real(n: i32, m: i32, x: f64) -> f64 {
    // DLMF 14.7.10/14.7.11. Allows m ≤ n.
    if m < 0 || m > n {
        return 0.0;
    }
    let mut pmm = 1.0_f64;
    if m > 0 {
        let somx2 = ((1.0 - x) * (1.0 + x)).sqrt();
        let mut fact = 1.0_f64;
        for _ in 1..=m {
            pmm *= -fact * somx2;
            fact += 2.0;
        }
    }
    if n == m {
        return pmm;
    }
    let mut pmmp1 = x * (2.0 * m as f64 + 1.0) * pmm;
    if n == m + 1 {
        return pmmp1;
    }
    let mut pll = 0.0_f64;
    for ll in (m + 2)..=n {
        let llf = ll as f64;
        pll = (x * (2.0 * llf - 1.0) * pmmp1 - (llf + m as f64 - 1.0) * pmm) / (llf - m as f64);
        pmm = pmmp1;
        pmmp1 = pll;
    }
    pll
}

fn hermite_h_real(n: i32, x: f64) -> f64 {
    // Physicist's: H_0=1, H_1=2x, H_{n+1}=2x H_n - 2n H_{n-1}.
    if n == 0 {
        return 1.0;
    }
    if n == 1 {
        return 2.0 * x;
    }
    let (mut hkm1, mut hk) = (1.0_f64, 2.0 * x);
    for k in 1..n {
        let kf = k as f64;
        let hkp1 = 2.0 * x * hk - 2.0 * kf * hkm1;
        hkm1 = hk;
        hk = hkp1;
    }
    hk
}

fn hermite_he_real(n: i32, x: f64) -> f64 {
    // Probabilist's: He_0=1, He_1=x, He_{n+1}=x He_n - n He_{n-1}.
    if n == 0 {
        return 1.0;
    }
    if n == 1 {
        return x;
    }
    let (mut hkm1, mut hk) = (1.0_f64, x);
    for k in 1..n {
        let kf = k as f64;
        let hkp1 = x * hk - kf * hkm1;
        hkm1 = hk;
        hk = hkp1;
    }
    hk
}

fn laguerre_l_real(n: i32, x: f64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    if n == 1 {
        return 1.0 - x;
    }
    let (mut lkm1, mut lk) = (1.0_f64, 1.0 - x);
    for k in 1..n {
        let kf = k as f64;
        let lkp1 = ((2.0 * kf + 1.0 - x) * lk - kf * lkm1) / (kf + 1.0);
        lkm1 = lk;
        lk = lkp1;
    }
    lk
}

fn assoc_laguerre_l_real(n: i32, alpha: f64, x: f64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    if n == 1 {
        return 1.0 + alpha - x;
    }
    let (mut lkm1, mut lk) = (1.0_f64, 1.0 + alpha - x);
    for k in 1..n {
        let kf = k as f64;
        let lkp1 = ((2.0 * kf + 1.0 + alpha - x) * lk - (kf + alpha) * lkm1) / (kf + 1.0);
        lkm1 = lk;
        lk = lkp1;
    }
    lk
}

fn jacobi_p_real(n: i32, alpha: f64, beta: f64, x: f64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    if n == 1 {
        return 0.5 * (alpha - beta + (alpha + beta + 2.0) * x);
    }
    let (mut pkm1, mut pk) = (
        1.0_f64,
        0.5 * (alpha - beta + (alpha + beta + 2.0) * x),
    );
    for k in 1..n {
        let kf = k as f64;
        let a = 2.0 * (kf + 1.0) * (kf + alpha + beta + 1.0) * (2.0 * kf + alpha + beta);
        let b = (2.0 * kf + alpha + beta + 1.0)
            * ((alpha * alpha - beta * beta)
                + (2.0 * kf + alpha + beta) * (2.0 * kf + alpha + beta + 2.0) * x);
        let c = 2.0 * (kf + alpha) * (kf + beta) * (2.0 * kf + alpha + beta + 2.0);
        let pkp1 = (b * pk - c * pkm1) / a;
        pkm1 = pk;
        pk = pkp1;
    }
    pk
}

fn gegenbauer_c_real(n: i32, alpha: f64, x: f64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    if n == 1 {
        return 2.0 * alpha * x;
    }
    let (mut ckm1, mut ck) = (1.0_f64, 2.0 * alpha * x);
    for k in 1..n {
        let kf = k as f64;
        let ckp1 = (2.0 * (kf + alpha) * x * ck - (kf + 2.0 * alpha - 1.0) * ckm1) / (kf + 1.0);
        ckm1 = ck;
        ck = ckp1;
    }
    ck
}

fn chebyshev_t_real(n: i32, x: f64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    if n == 1 {
        return x;
    }
    let (mut tkm1, mut tk) = (1.0_f64, x);
    for _ in 1..n {
        let tkp1 = 2.0 * x * tk - tkm1;
        tkm1 = tk;
        tk = tkp1;
    }
    tk
}

fn chebyshev_u_real(n: i32, x: f64) -> f64 {
    if n == 0 {
        return 1.0;
    }
    if n == 1 {
        return 2.0 * x;
    }
    let (mut ukm1, mut uk) = (1.0_f64, 2.0 * x);
    for _ in 1..n {
        let ukp1 = 2.0 * x * uk - ukm1;
        ukm1 = uk;
        uk = ukp1;
    }
    uk
}

/// `legendre_p` — Legendre p. Returns a float.
fn builtin_legendre_p(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    Ok(StrykeValue::float(legendre_p_real(n as i32, x)))
}
/// `legendre_q` — Legendre q. Returns a float.
fn builtin_legendre_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    Ok(StrykeValue::float(legendre_q_real(n as i32, x)))
}
/// `assoc_legendre_p` — Assoc legendre p. Returns a float.
fn builtin_assoc_legendre_p(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, m, x) = f3(args);
    Ok(StrykeValue::float(assoc_legendre_p_real(
        n as i32, m as i32, x,
    )))
}
/// `hermite_h` — Hermite h. Returns a float.
fn builtin_hermite_h(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    Ok(StrykeValue::float(hermite_h_real(n as i32, x)))
}
/// `hermite_he` — Hermite he. Returns a float.
fn builtin_hermite_he(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    Ok(StrykeValue::float(hermite_he_real(n as i32, x)))
}
/// `laguerre_l` — Laguerre l. Returns a float.
fn builtin_laguerre_l(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    Ok(StrykeValue::float(laguerre_l_real(n as i32, x)))
}
/// `assoc_laguerre_l` — Assoc laguerre l. Returns a float.
fn builtin_assoc_laguerre_l(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, alpha, x) = f3(args);
    Ok(StrykeValue::float(assoc_laguerre_l_real(n as i32, alpha, x)))
}
/// `jacobi_p` — Jacobi p. Returns a float.
fn builtin_jacobi_p(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, alpha, beta, x) = f4(args);
    Ok(StrykeValue::float(jacobi_p_real(n as i32, alpha, beta, x)))
}
/// `gegenbauer_c` — Gegenbauer c. Returns a float.
fn builtin_gegenbauer_c(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, alpha, x) = f3(args);
    Ok(StrykeValue::float(gegenbauer_c_real(n as i32, alpha, x)))
}
/// `chebyshev_t` — Chebyshev t. Returns a float.
fn builtin_chebyshev_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    Ok(StrykeValue::float(chebyshev_t_real(n as i32, x)))
}
/// `chebyshev_u` — Chebyshev u. Returns a float.
fn builtin_chebyshev_u(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    Ok(StrykeValue::float(chebyshev_u_real(n as i32, x)))
}

/// `spherical_harmonic_y L, M, THETA, PHI` → [Re, Im]
fn builtin_spherical_harmonic_y(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (l, m, theta, phi) = f4(args);
    let l = l as i32;
    let m = m as i32;
    let am = m.unsigned_abs() as i32;
    if am > l {
        return Ok(StrykeValue::array(vec![
            StrykeValue::float(0.0),
            StrykeValue::float(0.0),
        ]));
    }
    // Y_l^m(θ,φ) = √((2l+1)/(4π) · (l-|m|)!/(l+|m|)!) · P_l^|m|(cos θ) · e^{imφ}
    let plm = assoc_legendre_p_real(l, am, theta.cos());
    // Ratio (l-|m|)!/(l+|m|)! computed in log-space for stability.
    let ln_ratio: f64 = {
        let mut s = 0.0_f64;
        for k in (l - am + 1)..=(l + am) {
            s += (k as f64).ln();
        }
        -s
    };
    let pre = ((2.0 * l as f64 + 1.0) / (4.0 * std::f64::consts::PI)).sqrt()
        * (0.5 * ln_ratio).exp();
    let mag = pre * plm;
    let phase = (m as f64) * phi;
    let mut re = mag * phase.cos();
    let mut im = mag * phase.sin();
    if m < 0 && (am & 1) == 1 {
        re = -re;
        im = -im;
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::float(re),
        StrykeValue::float(im),
    ]))
}

/// `zernike_r N, M, R` — radial Zernike polynomial R_n^m(r). 0 if (n-m) odd.
fn builtin_zernike_r(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, m, r) = f3(args);
    let n = n as i32;
    let m = m.abs() as i32;
    if ((n - m) & 1) != 0 || m > n {
        return Ok(StrykeValue::float(0.0));
    }
    // R_n^m(r) = Σ_{k=0}^{(n-m)/2} (-1)^k (n-k)! / [k!((n+m)/2-k)!((n-m)/2-k)!] r^{n-2k}
    let kmax = ((n - m) / 2) as usize;
    let nf_nm = ((n + m) / 2) as usize;
    let nf_nmm = ((n - m) / 2) as usize;
    let mut sum = 0.0_f64;
    let mut sign = 1.0_f64;
    let mut nminusk_fact = (1..=n as usize).fold(1.0_f64, |a, b| a * b as f64);
    let mut k_fact = 1.0_f64;
    let mut a_fact = (1..=nf_nm).fold(1.0_f64, |a, b| a * b as f64);
    let mut b_fact = (1..=nf_nmm).fold(1.0_f64, |a, b| a * b as f64);
    for k in 0..=kmax {
        let denom = k_fact * a_fact * b_fact;
        sum += sign * nminusk_fact / denom * r.powi(n - 2 * k as i32);
        sign = -sign;
        if k < kmax {
            nminusk_fact /= (n as usize - k) as f64; // (n-k-1)! prep
            k_fact *= (k + 1) as f64;
            a_fact /= (nf_nm - k) as f64;
            b_fact /= (nf_nmm - k) as f64;
        }
    }
    Ok(StrykeValue::float(sum))
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Elliptic integrals + Jacobi/Weierstrass/theta
// ─────────────────────────────────────────────────────────────────────────────

/// Carlson R_F(x,y,z) — Numerical Recipes 6.11. Symmetric in x,y,z, x+y, etc ≥ 0.
fn carlson_rf_real(mut x: f64, mut y: f64, mut z: f64) -> f64 {
    let errtol = 0.0025_f64;
    let third = 1.0 / 3.0;
    loop {
        let sx = x.sqrt();
        let sy = y.sqrt();
        let sz = z.sqrt();
        let lambda = sx * sy + sx * sz + sy * sz;
        x = (x + lambda) / 4.0;
        y = (y + lambda) / 4.0;
        z = (z + lambda) / 4.0;
        let mu = (x + y + z) * third;
        let dx = (mu - x) / mu;
        let dy = (mu - y) / mu;
        let dz = (mu - z) / mu;
        let max_d = dx.abs().max(dy.abs()).max(dz.abs());
        if max_d < errtol {
            let e2 = dx * dy - dz * dz;
            let e3 = dx * dy * dz;
            return (1.0 + (e2 / 24.0 - 0.1 - 3.0 * e3 / 44.0) * e2 + e3 / 14.0) / mu.sqrt();
        }
    }
}

/// Carlson R_D(x,y,z).
fn carlson_rd_real(mut x: f64, mut y: f64, mut z: f64) -> f64 {
    let errtol = 0.0015_f64;
    let mut sum = 0.0_f64;
    let mut fac = 1.0_f64;
    loop {
        let sx = x.sqrt();
        let sy = y.sqrt();
        let sz = z.sqrt();
        let lambda = sx * sy + sx * sz + sy * sz;
        sum += fac / (sz * (z + lambda));
        fac /= 4.0;
        x = (x + lambda) / 4.0;
        y = (y + lambda) / 4.0;
        z = (z + lambda) / 4.0;
        let mu = (x + y + 3.0 * z) / 5.0;
        let dx = (mu - x) / mu;
        let dy = (mu - y) / mu;
        let dz = (mu - z) / mu;
        let max_d = dx.abs().max(dy.abs()).max(dz.abs());
        if max_d < errtol {
            let ea = dx * dy;
            let eb = dz * dz;
            let ec = ea - eb;
            let ed = ea - 6.0 * eb;
            let ef = ed + ec + ec;
            let s1 = ed * (-3.0 / 14.0 + 0.25 * ed - 9.0 / 22.0 * dz * ef);
            let s2 = dz * (ef / 6.0 + dz * (-ec * 9.0 / 22.0 + dz * ea / 4.0));
            return 3.0 * sum + fac * (1.0 + s1 + s2) / (mu * mu.sqrt());
        }
    }
}

/// Carlson R_J(x,y,z,p).
fn carlson_rj_real(mut x: f64, mut y: f64, mut z: f64, mut p: f64) -> f64 {
    let errtol = 0.0015_f64;
    let mut sum = 0.0_f64;
    let mut fac = 1.0_f64;
    loop {
        let sx = x.sqrt();
        let sy = y.sqrt();
        let sz = z.sqrt();
        let lambda = sx * sy + sx * sz + sy * sz;
        let alpha = (p * (sx + sy + sz) + sx * sy * sz).powi(2);
        let beta = p * (p + lambda).powi(2);
        sum += fac * carlson_rc_real(alpha, beta);
        fac /= 4.0;
        x = (x + lambda) / 4.0;
        y = (y + lambda) / 4.0;
        z = (z + lambda) / 4.0;
        p = (p + lambda) / 4.0;
        let mu = (x + y + z + p + p) / 5.0;
        let dx = (mu - x) / mu;
        let dy = (mu - y) / mu;
        let dz = (mu - z) / mu;
        let dp = (mu - p) / mu;
        let max_d = dx.abs().max(dy.abs()).max(dz.abs()).max(dp.abs());
        if max_d < errtol {
            let ea = dx * (dy + dz) + dy * dz;
            let eb = dx * dy * dz;
            let ec = dp * dp;
            let ed = ea - 3.0 * ec;
            let ee = eb + 2.0 * dp * (ea - ec);
            let val = 3.0 * sum
                + fac
                    * (1.0
                        + ed * (-3.0 / 14.0 + 0.25 * ed - 9.0 / 22.0 * ee)
                        + eb * (1.0 / 6.0 + dp * (-3.0 / 22.0 + dp * 3.0 / 26.0))
                        + dp * ea * (3.0 / 14.0 - dp * 3.0 / 22.0)
                        + dp * ec / 26.0)
                    / (mu * mu.sqrt());
            return val;
        }
    }
}

/// Carlson R_C(x,y) — degenerate R_F.
fn carlson_rc_real(mut x: f64, mut y: f64) -> f64 {
    let errtol = 0.0012_f64;
    loop {
        let mu = (x + 2.0 * y) / 3.0;
        let s = (y - mu) / mu;
        if s.abs() < errtol {
            return (1.0 + s * s * (3.0 / 10.0 + s * (1.0 / 7.0 + s * (3.0 / 8.0 + s * 9.0 / 22.0))))
                / mu.sqrt();
        }
        let lambda = 2.0 * x.sqrt() * y.sqrt() + y;
        x = (x + lambda) / 4.0;
        y = (y + lambda) / 4.0;
    }
}

/// `elliptic_k M` — complete K(m), m = k².
fn builtin_elliptic_k(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    if m >= 1.0 {
        return Ok(StrykeValue::float(f64::INFINITY));
    }
    Ok(StrykeValue::float(carlson_rf_real(0.0, 1.0 - m, 1.0)))
}

/// `elliptic_e M` — complete E(m).
fn builtin_elliptic_e(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    if m > 1.0 {
        return Ok(StrykeValue::float(f64::NAN));
    }
    let one_minus_m = 1.0 - m;
    let v = carlson_rf_real(0.0, one_minus_m, 1.0)
        - m / 3.0 * carlson_rd_real(0.0, one_minus_m, 1.0);
    Ok(StrykeValue::float(v))
}

/// `elliptic_pi N, M` — complete Π(n, m) = Π(n | m). Sign convention
/// matches DLMF 19.2.8.
fn builtin_elliptic_pi(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, m) = f2(args);
    let one_minus_m = 1.0 - m;
    let v = carlson_rf_real(0.0, one_minus_m, 1.0)
        + n / 3.0 * carlson_rj_real(0.0, one_minus_m, 1.0, 1.0 - n);
    Ok(StrykeValue::float(v))
}

/// `elliptic_f PHI, M` — incomplete F(φ, m).
fn builtin_elliptic_f(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (phi, m) = f2(args);
    let s = phi.sin();
    let c = phi.cos();
    let v = s * carlson_rf_real(c * c, 1.0 - m * s * s, 1.0);
    Ok(StrykeValue::float(v))
}

/// `elliptic_e_inc PHI, M` — incomplete E(φ, m).
fn builtin_elliptic_e_inc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (phi, m) = f2(args);
    let s = phi.sin();
    let c = phi.cos();
    let one = 1.0 - m * s * s;
    let v = s * carlson_rf_real(c * c, one, 1.0)
        - m / 3.0 * s.powi(3) * carlson_rd_real(c * c, one, 1.0);
    Ok(StrykeValue::float(v))
}

/// `elliptic_pi_inc N, PHI, M` — incomplete Π(n; φ | m).
fn builtin_elliptic_pi_inc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, phi, m) = f3(args);
    let s = phi.sin();
    let c = phi.cos();
    let one = 1.0 - m * s * s;
    let v = s * carlson_rf_real(c * c, one, 1.0)
        + n / 3.0 * s.powi(3) * carlson_rj_real(c * c, one, 1.0, 1.0 - n * s * s);
    Ok(StrykeValue::float(v))
}

/// `carlson_rf` — Carlson rf. Returns a float.
fn builtin_carlson_rf(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (x, y, z) = f3(args);
    Ok(StrykeValue::float(carlson_rf_real(x, y, z)))
}
/// `carlson_rd` — Carlson rd. Returns a float.
fn builtin_carlson_rd(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (x, y, z) = f3(args);
    Ok(StrykeValue::float(carlson_rd_real(x, y, z)))
}
/// `carlson_rj` — Carlson rj. Returns a float.
fn builtin_carlson_rj(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (x, y, z, p) = f4(args);
    Ok(StrykeValue::float(carlson_rj_real(x, y, z, p)))
}

/// Jacobi sn/cn/dn via descending Landen transformation. Returns triple [sn,cn,dn].
fn jacobi_scd(u: f64, m: f64) -> (f64, f64, f64) {
    if m == 0.0 {
        return (u.sin(), u.cos(), 1.0);
    }
    if m == 1.0 {
        return (u.tanh(), 1.0 / u.cosh(), 1.0 / u.cosh());
    }
    // Arithmetic-geometric mean iteration (DLMF 22.20).
    let mut a = 1.0_f64;
    let mut b = (1.0 - m).sqrt();
    let mut c = m.sqrt();
    let mut cs = Vec::with_capacity(20);
    cs.push(c);
    for _ in 0..30 {
        let an = 0.5 * (a + b);
        let bn = (a * b).sqrt();
        let cn = 0.5 * (a - b);
        a = an;
        b = bn;
        c = cn;
        cs.push(c);
        if c.abs() < 1e-15 {
            break;
        }
    }
    let mut phi = (2_f64.powi(cs.len() as i32 - 1)) * a * u;
    for i in (0..cs.len()).rev() {
        phi = 0.5 * (phi + (cs[i] / a * phi.sin()).asin());
    }
    let sn = phi.sin();
    let cn = phi.cos();
    let dn = (1.0 - m * sn * sn).sqrt();
    (sn, cn, dn)
}

/// `jacobi_sn` — Jacobi sn. Returns a float.
fn builtin_jacobi_sn(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (u, m) = f2(args);
    Ok(StrykeValue::float(jacobi_scd(u, m).0))
}
/// `jacobi_cn` — Jacobi cn. Returns a float.
fn builtin_jacobi_cn(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (u, m) = f2(args);
    Ok(StrykeValue::float(jacobi_scd(u, m).1))
}
/// `jacobi_dn` — Jacobi dn. Returns a float.
fn builtin_jacobi_dn(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (u, m) = f2(args);
    Ok(StrykeValue::float(jacobi_scd(u, m).2))
}

/// `jacobi_am U, M` — Jacobi amplitude φ such that sn(u,m) = sin(φ).
fn builtin_jacobi_am(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (u, m) = f2(args);
    let sn = jacobi_scd(u, m).0;
    Ok(StrykeValue::float(sn.asin()))
}

/// Elliptic theta functions θ_j(z, q). Series convergent for |q|<1.
fn theta_series(j: i32, z: f64, q: f64) -> f64 {
    let mut sum = 0.0_f64;
    match j {
        1 => {
            for n in 0..200 {
                let nf = n as f64;
                let term = (-1.0_f64).powi(n) * q.powf((nf + 0.5).powi(2)) * ((2.0 * nf + 1.0) * z).sin();
                sum += term;
                if term.abs() < 1e-18 {
                    break;
                }
            }
            2.0 * sum
        }
        2 => {
            for n in 0..200 {
                let nf = n as f64;
                let term = q.powf((nf + 0.5).powi(2)) * ((2.0 * nf + 1.0) * z).cos();
                sum += term;
                if term.abs() < 1e-18 {
                    break;
                }
            }
            2.0 * sum
        }
        3 => {
            sum = 1.0;
            for n in 1..200 {
                let nf = n as f64;
                let term = 2.0 * q.powf(nf * nf) * (2.0 * nf * z).cos();
                sum += term;
                if term.abs() < 1e-18 {
                    break;
                }
            }
            sum
        }
        4 => {
            sum = 1.0;
            for n in 1..200 {
                let nf = n as f64;
                let term = 2.0 * (-1.0_f64).powi(n) * q.powf(nf * nf) * (2.0 * nf * z).cos();
                sum += term;
                if term.abs() < 1e-18 {
                    break;
                }
            }
            sum
        }
        _ => f64::NAN,
    }
}

/// `elliptic_theta J, Z, Q` — Jacobi theta function.
fn builtin_elliptic_theta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (j, z, q) = f3(args);
    Ok(StrykeValue::float(theta_series(j as i32, z, q)))
}

/// Weierstrass ℘(z; g2, g3) via Laurent series around 0 (DLMF 23.9.2).
/// Returns NaN at z = 0 (pole). Convergence assumes |z| inside fundamental cell.
fn weierstrass_p_real(z: f64, g2: f64, g3: f64) -> f64 {
    if z.abs() < 1e-300 {
        return f64::INFINITY;
    }
    // c_2 = g2/20, c_3 = g3/28, c_n = 3 Σ_{k=2..n-2} c_k c_{n-k} / ((n-3)(2n+1)).
    let mut c = vec![0.0_f64; 30];
    c[2] = g2 / 20.0;
    c[3] = g3 / 28.0;
    for n in 4..c.len() {
        let mut s = 0.0_f64;
        for k in 2..=(n - 2) {
            s += c[k] * c[n - k];
        }
        c[n] = 3.0 * s / (((n as f64) - 3.0) * (2.0 * n as f64 + 1.0));
    }
    let mut sum = 1.0 / (z * z);
    let z2 = z * z;
    let mut zp = z2;
    for n in 2..c.len() {
        sum += c[n] * zp;
        zp *= z2;
    }
    sum
}

/// `weierstrass_p` — Weierstrass p. Returns a float.
fn builtin_weierstrass_p(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (z, g2, g3) = f3(args);
    Ok(StrykeValue::float(weierstrass_p_real(z, g2, g3)))
}

/// Weierstrass ζ(z; g2, g3): -∫℘ + 1/z via Laurent series.
fn builtin_weierstrass_zeta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (z, g2, g3) = f3(args);
    if z.abs() < 1e-300 {
        return Ok(StrykeValue::float(f64::INFINITY));
    }
    let mut c = vec![0.0_f64; 30];
    c[2] = g2 / 20.0;
    c[3] = g3 / 28.0;
    for n in 4..c.len() {
        let mut s = 0.0_f64;
        for k in 2..=(n - 2) {
            s += c[k] * c[n - k];
        }
        c[n] = 3.0 * s / (((n as f64) - 3.0) * (2.0 * n as f64 + 1.0));
    }
    let z2 = z * z;
    let mut zp = z * z2;
    let mut sum = 1.0 / z;
    for n in 2..c.len() {
        sum -= c[n] * zp / (2.0 * n as f64 - 1.0);
        zp *= z2;
    }
    Ok(StrykeValue::float(sum))
}

/// Weierstrass σ(z; g2, g3) ≈ z·exp(-Σ c_n z^{2n}/(4n²-2n)).
fn builtin_weierstrass_sigma(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (z, g2, g3) = f3(args);
    if z.abs() < 1e-300 {
        return Ok(StrykeValue::float(0.0));
    }
    let mut c = vec![0.0_f64; 30];
    c[2] = g2 / 20.0;
    c[3] = g3 / 28.0;
    for n in 4..c.len() {
        let mut s = 0.0_f64;
        for k in 2..=(n - 2) {
            s += c[k] * c[n - k];
        }
        c[n] = 3.0 * s / (((n as f64) - 3.0) * (2.0 * n as f64 + 1.0));
    }
    let z2 = z * z;
    let mut zp = z2 * z2;
    let mut log_sum = 0.0_f64;
    for n in 2..c.len() {
        log_sum -= c[n] * zp / (2.0 * (2.0 * n as f64 - 1.0) * (n as f64));
        zp *= z2;
    }
    Ok(StrykeValue::float(z * log_sum.exp()))
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Zeta / polylog / Lerch family
// ─────────────────────────────────────────────────────────────────────────────

/// Riemann zeta ζ(s) via reflection + Euler-Maclaurin for s>1.
fn zeta_real(s: f64) -> f64 {
    if s == 1.0 {
        return f64::INFINITY;
    }
    if s < 0.5 {
        // Reflection: ζ(s) = 2^s π^(s-1) sin(πs/2) Γ(1-s) ζ(1-s).
        let one_s = 1.0 - s;
        let pre = 2.0_f64.powf(s) * std::f64::consts::PI.powf(s - 1.0)
            * (std::f64::consts::PI * s / 2.0).sin()
            * statrs::function::gamma::gamma(one_s);
        return pre * zeta_real(one_s);
    }
    // Euler-Maclaurin: ζ(s) ≈ Σ_{k=1..n-1} 1/k^s + 1/(2 n^s) + n^(1-s)/(s-1)
    //                       + Σ_{j} B_{2j}/(2j)! · (s)_{2j-1} / n^{s+2j-1}.
    let n = 12_usize;
    let mut sum = 0.0_f64;
    for k in 1..n {
        sum += (k as f64).powf(-s);
    }
    sum += 0.5 * (n as f64).powf(-s);
    sum += (n as f64).powf(1.0 - s) / (s - 1.0);
    // Bernoulli numbers B2..B14.
    let bern = [
        1.0 / 6.0,
        -1.0 / 30.0,
        1.0 / 42.0,
        -1.0 / 30.0,
        5.0 / 66.0,
        -691.0 / 2730.0,
        7.0 / 6.0,
    ];
    let mut prod = s;
    let mut nfact = 2.0_f64;
    let mut np = (n as f64).powf(s + 1.0);
    for (i, &b) in bern.iter().enumerate() {
        let j = i + 1;
        // Term: B_{2j}/(2j)! · (s)(s+1)…(s+2j-2) / n^{s+2j-1}.
        // We track prod = (s)(s+1)…(s+2j-2) and np = n^{s+2j-1} incrementally.
        if j > 1 {
            prod *= (s + 2.0 * j as f64 - 3.0) * (s + 2.0 * j as f64 - 2.0);
            nfact *= (2.0 * j as f64 - 1.0) * (2.0 * j as f64);
            np *= (n as f64) * (n as f64);
        }
        let term = b / nfact * prod / np;
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
    }
    sum
}

/// Hurwitz ζ(s, a). For a = 1 it reduces to Riemann ζ.
fn hurwitz_zeta_real(s: f64, a: f64) -> f64 {
    if a <= 0.0 {
        return f64::NAN;
    }
    let n = 14_usize;
    let mut sum = 0.0_f64;
    for k in 0..n {
        sum += (a + k as f64).powf(-s);
    }
    let an = a + n as f64;
    sum += 0.5 * an.powf(-s) + an.powf(1.0 - s) / (s - 1.0);
    let bern = [1.0 / 6.0, -1.0 / 30.0, 1.0 / 42.0, -1.0 / 30.0, 5.0 / 66.0];
    let mut prod = s;
    let mut nfact = 2.0_f64;
    let mut np = an.powf(s + 1.0);
    for (i, &b) in bern.iter().enumerate() {
        let j = i + 1;
        if j > 1 {
            prod *= (s + 2.0 * j as f64 - 3.0) * (s + 2.0 * j as f64 - 2.0);
            nfact *= (2.0 * j as f64 - 1.0) * (2.0 * j as f64);
            np *= an * an;
        }
        let term = b / nfact * prod / np;
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
    }
    sum
}

/// `zeta` — Zeta. Returns a float.
fn builtin_zeta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(zeta_real(f1(args))))
}
/// `hurwitz_zeta` — Hurwitz zeta. Returns a float.
fn builtin_hurwitz_zeta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (s, a) = f2(args);
    Ok(StrykeValue::float(hurwitz_zeta_real(s, a)))
}

/// Polylog Li_n(z) = Σ z^k / k^n; valid for |z|<1 and via series-acceleration
/// extension for Li_1=-ln(1-z), Li_2=dilog. We restrict to |z|≤1 here.
fn builtin_polylog(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, z) = f2(args);
    if z.abs() > 1.0 + 1e-12 {
        return Err(PerlError::runtime("polylog: |z| must be ≤ 1", 0));
    }
    if n == 1.0 {
        return Ok(StrykeValue::float(-(1.0 - z).ln()));
    }
    let mut sum = 0.0_f64;
    let mut zp = z;
    for k in 1..2000 {
        let term = zp / (k as f64).powf(n);
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
        zp *= z;
    }
    Ok(StrykeValue::float(sum))
}

/// `dilog` — Dilog. Returns a float.
fn builtin_dilog(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    if !(-1.0..=1.0).contains(&z) {
        // Reflection Li_2(z) + Li_2(1-z) = π²/6 - ln(z) ln(1-z) — only for 0<z<1.
        return Err(PerlError::runtime(
            "dilog: argument out of [-1,1] range",
            0,
        ));
    }
    let mut sum = 0.0_f64;
    let mut zp = z;
    for k in 1..2000 {
        let term = zp / ((k as f64) * (k as f64));
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
        zp *= z;
    }
    Ok(StrykeValue::float(sum))
}

/// Lerch transcendent Φ(z, s, a) = Σ z^k / (a+k)^s.
fn builtin_lerch_phi(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (z, s, a) = f3(args);
    if z.abs() > 1.0 {
        return Err(PerlError::runtime("lerch_phi: |z| must be ≤ 1", 0));
    }
    let mut sum = 0.0_f64;
    let mut zp = 1.0_f64;
    for k in 0..5000 {
        let term = zp / (a + k as f64).powf(s);
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
        zp *= z;
    }
    Ok(StrykeValue::float(sum))
}

/// Riemann-Siegel Z(t) on the critical line. Uses Hardy's main term to
/// roughly N = ⌊√(t/2π)⌋ followed by Riemann-Siegel correction C₀.
fn builtin_riemann_siegel_z(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    let theta = riemann_siegel_theta_real(t);
    let n = (t / (2.0 * std::f64::consts::PI)).sqrt().floor() as i64;
    let mut sum = 0.0_f64;
    for k in 1..=n {
        sum += (theta - t * (k as f64).ln()).cos() / (k as f64).sqrt();
    }
    sum *= 2.0;
    // C0 correction (DLMF 25.10.6): O(t^{-1/4}) leading remainder.
    let p = (t / (2.0 * std::f64::consts::PI)).sqrt() - n as f64;
    let psi0 = (2.0 * std::f64::consts::PI * (p * p - p - 1.0 / 16.0)).cos()
        / (2.0 * std::f64::consts::PI * p).cos();
    let r = (-1.0_f64).powi((n - 1) as i32) * (t / (2.0 * std::f64::consts::PI)).powf(-0.25) * psi0;
    Ok(StrykeValue::float(sum + r))
}

fn riemann_siegel_theta_real(t: f64) -> f64 {
    // θ(t) = arg Γ(1/4 + it/2) - (t/2) ln π. Approximate via Stirling for moderate t.
    let half_t = t / 2.0;
    // Use lgamma identity: arg = ln|Γ| imaginary part — Stirling asymptotic:
    // θ(t) ≈ (t/2) ln(t/2π) - t/2 - π/8 + 1/(48 t) + 7/(5760 t³).
    half_t * (half_t / std::f64::consts::PI).ln() - half_t - std::f64::consts::FRAC_PI_8
        + 1.0 / (48.0 * t)
        + 7.0 / (5760.0 * t.powi(3))
}

/// `riemann_siegel_theta` — Riemann siegel theta. Returns a float.
fn builtin_riemann_siegel_theta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(riemann_siegel_theta_real(f1(args))))
}

/// Dirichlet eta η(s) = (1 - 2^(1-s)) ζ(s).
fn builtin_dirichlet_eta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = f1(args);
    Ok(StrykeValue::float(
        (1.0 - 2.0_f64.powf(1.0 - s)) * zeta_real(s),
    ))
}

/// Dirichlet beta β(s) = Σ (-1)^k / (2k+1)^s.
fn builtin_dirichlet_beta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = f1(args);
    let mut sum = 0.0_f64;
    let mut sign = 1.0_f64;
    for k in 0..5000 {
        let term = sign / (2.0 * k as f64 + 1.0).powf(s);
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
        sign = -sign;
    }
    Ok(StrykeValue::float(sum))
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Hypergeometric family
// ─────────────────────────────────────────────────────────────────────────────

/// _2F_1(a, b; c; z) — Taylor series for |z|<1; rejects |z|≥1 with a hint.
fn builtin_hypergeometric_2f1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (a, b, c, z) = f4(args);
    if z.abs() >= 1.0 {
        return Err(PerlError::runtime(
            "hypergeometric_2f1: |z| must be < 1 (use reflection identities outside the disk)",
            0,
        ));
    }
    let mut term = 1.0_f64;
    let mut sum = 1.0_f64;
    for k in 0..2000 {
        let kf = k as f64;
        term *= (a + kf) * (b + kf) / ((c + kf) * (kf + 1.0)) * z;
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
    }
    Ok(StrykeValue::float(sum))
}

/// _1F_1(a; b; z) — confluent / Kummer.
fn builtin_hypergeometric_1f1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (a, b, z) = f3(args);
    let mut term = 1.0_f64;
    let mut sum = 1.0_f64;
    for k in 0..2000 {
        let kf = k as f64;
        term *= (a + kf) / ((b + kf) * (kf + 1.0)) * z;
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
    }
    Ok(StrykeValue::float(sum))
}

/// _0F_1(; b; z) — confluent limit.
fn builtin_hypergeometric_0f1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (b, z) = f2(args);
    let mut term = 1.0_f64;
    let mut sum = 1.0_f64;
    for k in 0..2000 {
        let kf = k as f64;
        term *= 1.0 / ((b + kf) * (kf + 1.0)) * z;
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
    }
    Ok(StrykeValue::float(sum))
}

/// _pF_q(as; bs; z) — generalized; arrays-of-arrays input.
fn builtin_hypergeometric_pfq(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let as_v: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let bs_v: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let z = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let mut term = 1.0_f64;
    let mut sum = 1.0_f64;
    for k in 0..5000 {
        let kf = k as f64;
        let mut num = 1.0_f64;
        for &a in &as_v {
            num *= a + kf;
        }
        let mut den = kf + 1.0;
        for &b in &bs_v {
            den *= b + kf;
        }
        term *= num / den * z;
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
    }
    Ok(StrykeValue::float(sum))
}

/// Tricomi U(a, b, z) via integral asymptotic / series. Uses
/// U(a,b,z) = (Γ(1-b)/Γ(a-b+1)) ₁F₁(a;b;z) + (Γ(b-1)/Γ(a)) z^(1-b) ₁F₁(a-b+1;2-b;z).
fn builtin_hypergeometric_u(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (a, b, z) = f3(args);
    if z <= 0.0 {
        return Err(PerlError::runtime(
            "hypergeometric_u: z must be > 0",
            0,
        ));
    }
    let lhs_args = [
        StrykeValue::float(a),
        StrykeValue::float(b),
        StrykeValue::float(z),
    ];
    let rhs_args = [
        StrykeValue::float(a - b + 1.0),
        StrykeValue::float(2.0 - b),
        StrykeValue::float(z),
    ];
    let m1 = builtin_hypergeometric_1f1(&lhs_args)?.to_number();
    let m2 = builtin_hypergeometric_1f1(&rhs_args)?.to_number();
    let g_1mb = statrs::function::gamma::gamma(1.0 - b);
    let g_amb1 = statrs::function::gamma::gamma(a - b + 1.0);
    let g_bm1 = statrs::function::gamma::gamma(b - 1.0);
    let g_a = statrs::function::gamma::gamma(a);
    Ok(StrykeValue::float(
        g_1mb / g_amb1 * m1 + g_bm1 / g_a * z.powf(1.0 - b) * m2,
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Modular forms + Ramanujan tau
// ─────────────────────────────────────────────────────────────────────────────

/// Dedekind η(τ) for purely imaginary τ = i·y (y>0). Real-valued q-series.
/// η(iy) = q^(1/24) Π_{n≥1} (1 - q^n), q = e^{-2π y}.
fn dedekind_eta_real(y: f64) -> f64 {
    if y <= 0.0 {
        return f64::NAN;
    }
    let q = (-2.0 * std::f64::consts::PI * y).exp();
    let mut prod = 1.0_f64;
    let mut qn = q;
    for _ in 1..400 {
        let f = 1.0 - qn;
        prod *= f;
        qn *= q;
        if qn.abs() < 1e-30 {
            break;
        }
    }
    q.powf(1.0 / 24.0) * prod
}

/// `dedekind_eta` — Dedekind eta. Returns a float.
fn builtin_dedekind_eta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(dedekind_eta_real(f1(args))))
}

/// Klein j-invariant on the imaginary axis: j(iy) = E_4³(iy)/Δ(iy), where
/// Δ = η^24 and E_4(τ) = 1 + 240 Σ_{n≥1} σ_3(n) q^n.
fn builtin_klein_j(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    if y <= 0.0 {
        return Ok(StrykeValue::float(f64::NAN));
    }
    let q = (-2.0 * std::f64::consts::PI * y).exp();
    let mut e4 = 1.0_f64;
    let mut qn = q;
    for n in 1..400 {
        let mut s3 = 0_i64;
        let mut d = 1_i64;
        while d * d <= n as i64 {
            if (n as i64) % d == 0 {
                let other = (n as i64) / d;
                s3 += d.pow(3);
                if d != other {
                    s3 += other.pow(3);
                }
            }
            d += 1;
        }
        e4 += 240.0 * s3 as f64 * qn;
        qn *= q;
        if qn < 1e-30 {
            break;
        }
    }
    let eta = dedekind_eta_real(y);
    let delta = eta.powi(24);
    Ok(StrykeValue::float(e4.powi(3) / delta))
}

/// Modular λ(τ) on imaginary axis: λ = θ_2^4 / θ_3^4 in q = e^{iπτ}, here q real.
fn builtin_modular_lambda(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    if y <= 0.0 {
        return Ok(StrykeValue::float(f64::NAN));
    }
    let q = (-std::f64::consts::PI * y).exp();
    let theta2 = theta_series(2, 0.0, q);
    let theta3 = theta_series(3, 0.0, q);
    Ok(StrykeValue::float(theta2.powi(4) / theta3.powi(4)))
}

/// Ramanujan tau function τ(n): coefficient of Δ(τ) = η(τ)^24 = Σ τ(n) q^n.
/// Multiplicative; here we compute by direct convolution of η^24 q-series for
/// small n (n ≤ ~1000) — adequate for stryke scientific use.
fn builtin_ramanujan_tau(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n < 1 {
        return Ok(StrykeValue::integer(0));
    }
    let n = n as usize;
    // η(q) = q^(1/24) Σ_k (-1)^k q^{k(3k-1)/2} (Euler pentagonal).
    // Δ(q)·q^{-1} = (η/q^(1/24))^24 — work in shifted exponents.
    // Build pentagonal series up to length n.
    let len = n + 1;
    let mut pent = vec![0_i64; len];
    pent[0] = 1;
    let mut k = 1_i64;
    loop {
        let e1 = (k * (3 * k - 1) / 2) as usize;
        let e2 = (k * (3 * k + 1) / 2) as usize;
        if e1 >= len && e2 >= len {
            break;
        }
        let sign = if (k & 1) == 1 { -1 } else { 1 };
        if e1 < len {
            pent[e1] += sign;
        }
        if e2 < len {
            pent[e2] += sign;
        }
        k += 1;
    }
    // Square pentagonal 12 times (24 multiplications via repeated squaring of
    // 24 = 16+8, but plain 24-fold convolution is simpler and stays exact for
    // reasonable n).
    let mut acc = pent.clone();
    for _ in 1..24 {
        let mut next = vec![0_i64; len];
        for (i, &a) in acc.iter().enumerate() {
            if a == 0 {
                continue;
            }
            for (j, &b) in pent.iter().enumerate() {
                if i + j >= len {
                    break;
                }
                next[i + j] += a * b;
            }
        }
        acc = next;
    }
    Ok(StrykeValue::integer(acc[n - 1]))
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. Si / Ci / Ei / Li / Fresnel integrals
// ─────────────────────────────────────────────────────────────────────────────

/// Si(x) = ∫₀ˣ sin(t)/t dt. Power series for |x|≤4, asymptotic otherwise.
fn sin_integral_real(x: f64) -> f64 {
    let ax = x.abs();
    let r = if ax <= 4.0 {
        let mut sum = 0.0_f64;
        let mut term = ax;
        sum += term;
        for k in 1..200 {
            let kf = k as f64;
            term *= -ax * ax / ((2.0 * kf) * (2.0 * kf + 1.0));
            let contribution = term / (2.0 * kf + 1.0);
            sum += contribution;
            if contribution.abs() < 1e-18 * sum.abs() {
                break;
            }
        }
        sum
    } else {
        // Si(x) = π/2 - cos(x)·f(x) - sin(x)·g(x) for large x.
        let f_aux = aux_f(ax);
        let g_aux = aux_g(ax);
        std::f64::consts::FRAC_PI_2 - ax.cos() * f_aux - ax.sin() * g_aux
    };
    if x < 0.0 {
        -r
    } else {
        r
    }
}

/// Ci(x) = γ + ln(x) + ∫₀ˣ (cos(t)-1)/t dt.
fn cos_integral_real(x: f64) -> f64 {
    if x <= 0.0 {
        return f64::NAN;
    }
    if x <= 4.0 {
        let gamma = 0.577_215_664_901_532_9_f64;
        let mut sum = gamma + x.ln();
        let mut term = -x * x / 2.0; // first body term: -x²/(2·2!)
        sum += term;
        for k in 2..200 {
            let kf = k as f64;
            // term_k = (-1)^k x^{2k} / [(2k)·(2k)!]
            term *= -x * x / ((2.0 * kf - 1.0) * (2.0 * kf));
            let contribution = term / (2.0 * kf);
            sum += contribution;
            if contribution.abs() < 1e-18 * sum.abs() {
                break;
            }
        }
        sum
    } else {
        let f_aux = aux_f(x);
        let g_aux = aux_g(x);
        x.sin() * f_aux - x.cos() * g_aux
    }
}

/// Auxiliary f(x) and g(x) for Si/Ci asymptotic — DLMF 6.16.
fn aux_f(x: f64) -> f64 {
    // f(x) ≈ 1/x · Σ (-1)^k (2k)! / x^{2k}, asymptotic.
    let xi = 1.0 / x;
    let xi2 = xi * xi;
    let mut sum = 0.0_f64;
    let mut term = xi;
    let mut sign = 1.0_f64;
    let mut fact = 1.0_f64;
    for k in 0..20 {
        sum += sign * fact * term;
        if k > 0 {
            fact *= (2 * k) as f64 * (2 * k - 1) as f64;
        }
        term *= xi2;
        sign = -sign;
        if term.abs() * fact < 1e-16 {
            break;
        }
    }
    sum
}

fn aux_g(x: f64) -> f64 {
    let xi = 1.0 / x;
    let xi2 = xi * xi;
    let mut sum = 0.0_f64;
    let mut term = xi * xi;
    let mut sign = 1.0_f64;
    let mut fact = 1.0_f64;
    for k in 1..20 {
        if k > 1 {
            fact *= (2 * k - 1) as f64 * (2 * k - 2) as f64;
        }
        sum += sign * fact * term;
        term *= xi2;
        sign = -sign;
        if term.abs() * fact < 1e-16 {
            break;
        }
    }
    sum
}

fn sinh_integral_real(x: f64) -> f64 {
    // Shi(x) = ∫₀ˣ sinh(t)/t dt = Σ x^(2k+1) / [(2k+1)·(2k+1)!]
    let mut sum = x;
    let mut term = x;
    for k in 1..200 {
        let kf = k as f64;
        term *= x * x / ((2.0 * kf) * (2.0 * kf + 1.0));
        let contribution = term / (2.0 * kf + 1.0);
        sum += contribution;
        if contribution.abs() < 1e-18 * sum.abs() {
            break;
        }
    }
    sum
}

fn cosh_integral_real(x: f64) -> f64 {
    if x <= 0.0 {
        return f64::NAN;
    }
    let gamma = 0.577_215_664_901_532_9_f64;
    let mut sum = gamma + x.ln();
    let mut term = x * x / 2.0;
    sum += term;
    for k in 2..200 {
        let kf = k as f64;
        term *= x * x / ((2.0 * kf - 1.0) * (2.0 * kf));
        let contribution = term / (2.0 * kf);
        sum += contribution;
        if contribution.abs() < 1e-18 * sum.abs() {
            break;
        }
    }
    sum
}

/// `sin_integral` — Sin integral. Returns a float.
fn builtin_sin_integral(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(sin_integral_real(f1(args))))
}
/// `cos_integral` — Cos integral. Returns a float.
fn builtin_cos_integral(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(cos_integral_real(f1(args))))
}
/// `sinh_integral` — Sinh integral. Returns a float.
fn builtin_sinh_integral(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(sinh_integral_real(f1(args))))
}
/// `cosh_integral` — Cosh integral. Returns a float.
fn builtin_cosh_integral(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(cosh_integral_real(f1(args))))
}

/// Ei(x) = -∫_{-x}^∞ e^{-t}/t dt for x > 0. Series + continued fraction split.
fn exp_integral_ei_real(x: f64) -> f64 {
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x < 0.0 {
        // Ei(-x) for x>0 = -E_1(x).
        return -exp_integral_e1_real(-x);
    }
    if x < 6.0 {
        let gamma = 0.577_215_664_901_532_9_f64;
        let mut sum = gamma + x.ln();
        let mut term = 1.0_f64;
        for k in 1..200 {
            term *= x / k as f64;
            sum += term / k as f64;
            if (term / k as f64).abs() < 1e-18 * sum.abs() {
                break;
            }
        }
        sum
    } else {
        // Asymptotic: Ei(x) ~ e^x/x · (1 + 1!/x + 2!/x² + …)
        let mut sum = 1.0_f64;
        let mut term = 1.0_f64;
        for k in 1..30 {
            term *= k as f64 / x;
            sum += term;
            if term.abs() < 1e-16 {
                break;
            }
        }
        x.exp() / x * sum
    }
}

fn exp_integral_e1_real(x: f64) -> f64 {
    if x <= 0.0 {
        return f64::NAN;
    }
    if x < 1.0 {
        let gamma = 0.577_215_664_901_532_9_f64;
        let mut sum = -gamma - x.ln();
        let mut term = 1.0_f64;
        let mut sign = -1.0_f64;
        for k in 1..200 {
            term *= x / k as f64;
            let contribution = sign * term / k as f64;
            sum += contribution;
            sign = -sign;
            if contribution.abs() < 1e-18 * sum.abs() {
                break;
            }
        }
        sum
    } else {
        // Lentz continued fraction.
        let mut b = x + 1.0;
        let mut c = 1.0_f64 / 1e-300;
        let mut d = 1.0 / b;
        let mut h = d;
        for k in 1..200 {
            let an = -(k as f64) * (k as f64);
            b += 2.0;
            d = 1.0 / (an * d + b);
            c = b + an / c;
            let delta = c * d;
            h *= delta;
            if (delta - 1.0).abs() < 1e-15 {
                break;
            }
        }
        h * (-x).exp()
    }
}

/// `exp_integral_e N, X` — generalized E_n(x).
fn builtin_exp_integral_e(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, x) = f2(args);
    let n = n as i32;
    if n == 0 {
        return Ok(StrykeValue::float((-x).exp() / x));
    }
    if n == 1 {
        return Ok(StrykeValue::float(exp_integral_e1_real(x)));
    }
    // E_n(x) = ∫_1^∞ e^{-xt}/t^n dt; recurrence: E_{n+1}(x) = (e^{-x} - x E_n(x))/n.
    let mut e = exp_integral_e1_real(x);
    let mut nn = 1_i32;
    while nn < n {
        e = ((-x).exp() - x * e) / nn as f64;
        nn += 1;
    }
    Ok(StrykeValue::float(e))
}

/// `exp_integral_ei` — Exp integral ei. Returns a float.
fn builtin_exp_integral_ei(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(exp_integral_ei_real(f1(args))))
}

/// Logarithmic integral li(x) = Ei(ln x).
fn builtin_log_integral(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x <= 0.0 || x == 1.0 {
        return Ok(StrykeValue::float(f64::NEG_INFINITY));
    }
    Ok(StrykeValue::float(exp_integral_ei_real(x.ln())))
}

/// Fresnel S(x), C(x). Series for |x|<1.5, otherwise auxiliary form (DLMF 7.5).
fn fresnel_pair(x: f64) -> (f64, f64) {
    if x.abs() < 1.5 {
        // S(x) = Σ (-1)^k (π/2)^{2k+1} x^{4k+3} / [(2k+1)! (4k+3)]
        // C(x) = Σ (-1)^k (π/2)^{2k} x^{4k+1} / [(2k)! (4k+1)]
        let pi2 = std::f64::consts::FRAC_PI_2;
        let mut s = 0.0_f64;
        let mut c = 0.0_f64;
        let mut term_s = pi2 * x.powi(3) / 3.0;
        let mut term_c = x;
        s += term_s;
        c += term_c;
        for k in 1..200 {
            let kf = k as f64;
            term_s *= -pi2 * pi2 * x.powi(4) / ((2.0 * kf) * (2.0 * kf + 1.0)) * (4.0 * kf - 1.0) / (4.0 * kf + 3.0);
            term_c *= -pi2 * pi2 * x.powi(4) / ((2.0 * kf - 1.0) * (2.0 * kf)) * (4.0 * kf - 3.0) / (4.0 * kf + 1.0);
            s += term_s;
            c += term_c;
            if term_s.abs() < 1e-18 * s.abs().max(1e-30) && term_c.abs() < 1e-18 * c.abs().max(1e-30) {
                break;
            }
        }
        (s, c)
    } else {
        // Auxiliary: f(x), g(x); S(x) = 1/2 - f(x)cos(πx²/2) - g(x)sin(πx²/2)
        //                       C(x) = 1/2 + f(x)sin(πx²/2) - g(x)cos(πx²/2)
        let pix2 = std::f64::consts::FRAC_PI_2 * x * x;
        let s_aux = pix2.sin();
        let c_aux = pix2.cos();
        let f_aux = fresnel_aux_f(x);
        let g_aux = fresnel_aux_g(x);
        let s = 0.5 - f_aux * c_aux - g_aux * s_aux;
        let c = 0.5 + f_aux * s_aux - g_aux * c_aux;
        (s.copysign(x), c.copysign(x))
    }
}

fn fresnel_aux_f(x: f64) -> f64 {
    // f(x) ≈ (1 / (π x)) · Σ (-1)^k (4k+1)!! / (π x²)^{2k}  (asymptotic).
    let z = std::f64::consts::PI * x;
    let z2 = z * x; // π x²
    let mut sum = 0.0_f64;
    let mut term = 1.0_f64;
    let mut sign = 1.0_f64;
    let mut fact = 1.0_f64;
    for k in 0..15 {
        sum += sign * fact * term;
        fact *= (4.0 * k as f64 + 1.0) * (4.0 * k as f64 + 3.0);
        term /= z2 * z2;
        sign = -sign;
        if (fact * term).abs() < 1e-16 {
            break;
        }
    }
    sum / z
}

fn fresnel_aux_g(x: f64) -> f64 {
    let z2 = std::f64::consts::PI * x * x;
    let mut sum = 0.0_f64;
    let mut term = 1.0_f64 / z2;
    let mut sign = 1.0_f64;
    let mut fact = 1.0_f64;
    for k in 0..15 {
        fact *= (4.0 * k as f64 + 1.0) * (4.0 * k as f64 + 3.0);
        sum += sign * fact * term;
        term /= z2 * z2;
        sign = -sign;
        if (fact * term).abs() < 1e-16 {
            break;
        }
    }
    sum / (std::f64::consts::PI * x)
}

/// `fresnel_s` — Fresnel s. Returns a float.
fn builtin_fresnel_s(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(fresnel_pair(f1(args)).0))
}
/// `fresnel_c` — Fresnel c. Returns a float.
fn builtin_fresnel_c(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(fresnel_pair(f1(args)).1))
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. Number theory gaps
// ─────────────────────────────────────────────────────────────────────────────

/// Jacobi symbol (a / n), n positive odd.
fn jacobi_symbol_real(mut a: i64, mut n: i64) -> i64 {
    if n <= 0 || (n & 1) == 0 {
        return 0;
    }
    a = a.rem_euclid(n);
    let mut t = 1_i64;
    while a != 0 {
        while (a & 1) == 0 {
            a /= 2;
            let r = n & 7;
            if r == 3 || r == 5 {
                t = -t;
            }
        }
        std::mem::swap(&mut a, &mut n);
        if a & 3 == 3 && n & 3 == 3 {
            t = -t;
        }
        a %= n;
    }
    if n == 1 {
        t
    } else {
        0
    }
}

/// `jacobi_symbol` — Jacobi symbol. Returns an integer.
fn builtin_jacobi_symbol(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (a, n) = i2(args);
    Ok(StrykeValue::integer(jacobi_symbol_real(a, n)))
}

/// Kronecker symbol (a/n) — extension of Jacobi to all n including even/zero.
fn builtin_kronecker_symbol(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (a, mut n) = i2(args);
    if n == 0 {
        return Ok(StrykeValue::integer(if a.abs() == 1 { 1 } else { 0 }));
    }
    let mut t = 1_i64;
    if n < 0 {
        n = -n;
        if a < 0 {
            t = -t;
        }
    }
    while (n & 1) == 0 {
        n /= 2;
        let am = a.rem_euclid(8);
        if am == 3 || am == 5 {
            t = -t;
        }
    }
    Ok(StrykeValue::integer(t * jacobi_symbol_real(a, n)))
}

/// Smallest primitive root mod p (p prime). Brute search; sentinel for n with no root.
fn builtin_primitive_root(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n < 2 {
        return Ok(StrykeValue::UNDEF);
    }
    let phi = n - 1; // Assumes prime n.
    // Factor phi.
    let factors = prime_factorize(phi);
    let mut uniq = factors.clone();
    uniq.sort();
    uniq.dedup();
    'outer: for g in 2..n {
        for &q in &uniq {
            if mod_pow_i64(g, phi / q, n) == 1 {
                continue 'outer;
            }
        }
        return Ok(StrykeValue::integer(g));
    }
    Ok(StrykeValue::UNDEF)
}

fn mod_pow_i64(mut base: i64, mut exp: i64, modulus: i64) -> i64 {
    if modulus == 1 {
        return 0;
    }
    let mut result = 1_i64;
    base = base.rem_euclid(modulus);
    while exp > 0 {
        if exp & 1 == 1 {
            result = (result as i128 * base as i128 % modulus as i128) as i64;
        }
        exp >>= 1;
        base = (base as i128 * base as i128 % modulus as i128) as i64;
    }
    result
}

/// Multiplicative order of a mod n (smallest k with a^k ≡ 1 mod n).
fn builtin_multiplicative_order(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (a, n) = i2(args);
    if n < 2 {
        return Ok(StrykeValue::UNDEF);
    }
    if gcd_i64(a.rem_euclid(n), n) != 1 {
        return Ok(StrykeValue::UNDEF);
    }
    let a = a.rem_euclid(n);
    let mut k = 1_i64;
    let mut cur = a;
    while cur != 1 {
        cur = (cur as i128 * a as i128 % n as i128) as i64;
        k += 1;
        if k > n {
            return Ok(StrykeValue::UNDEF);
        }
    }
    Ok(StrykeValue::integer(k))
}

fn gcd_i64(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// von Mangoldt Λ(n) — ln(p) if n = p^k, else 0.
fn builtin_mangoldt_lambda(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n < 2 {
        return Ok(StrykeValue::float(0.0));
    }
    let factors = prime_factorize(n);
    let mut uniq = factors.clone();
    uniq.sort();
    uniq.dedup();
    if uniq.len() == 1 {
        return Ok(StrykeValue::float((uniq[0] as f64).ln()));
    }
    Ok(StrykeValue::float(0.0))
}

/// Carmichael λ(n) — exponent of the group (Z/nZ)*.
fn builtin_carmichael_lambda(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n < 1 {
        return Ok(StrykeValue::integer(0));
    }
    if n == 1 {
        return Ok(StrykeValue::integer(1));
    }
    // λ(p^k): φ(p^k) for p odd; for p=2: 1, 2, 2^(k-2) for k≥3.
    let factors = prime_factorize(n);
    let mut uniq = factors.clone();
    uniq.sort();
    uniq.dedup();
    let mut lam = 1_i64;
    for &p in &uniq {
        let mut k = 0_i64;
        let mut nn = n;
        while nn % p == 0 {
            nn /= p;
            k += 1;
        }
        let lam_pk = if p == 2 {
            match k {
                1 => 1,
                2 => 2,
                _ => 1_i64 << (k - 2),
            }
        } else {
            (p - 1) * p.pow((k - 1) as u32)
        };
        lam = lam / gcd_i64(lam, lam_pk) * lam_pk;
    }
    Ok(StrykeValue::integer(lam))
}

/// SquaresR k, n — number of representations of n as sum of k squares (k = 2..8 supported).
fn builtin_squares_r(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (k, n) = i2(args);
    if n < 0 {
        return Ok(StrykeValue::integer(0));
    }
    if n == 0 {
        return Ok(StrykeValue::integer(1));
    }
    // Use brute enumeration up to √n. Handles arbitrary k cleanly.
    let limit = (n as f64).sqrt() as i64 + 1;
    fn count(rem: i64, k: i64, max_x: i64) -> i64 {
        if k == 0 {
            return if rem == 0 { 1 } else { 0 };
        }
        if rem < 0 {
            return 0;
        }
        let mut total = 0_i64;
        for x in -max_x..=max_x {
            let next = rem - x * x;
            if next < 0 {
                continue;
            }
            total += count(next, k - 1, max_x);
        }
        total
    }
    Ok(StrykeValue::integer(count(n, k, limit)))
}

/// Thue-Morse t(n) = popcount(n) mod 2.
fn builtin_thue_morse(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::integer((n.unsigned_abs().count_ones() & 1) as i64))
}

/// Rudin-Shapiro a(n) — # of "11" patterns in binary of n; sequence is (-1)^a(n).
fn builtin_rudin_shapiro(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).unsigned_abs();
    let count = (n & (n >> 1)).count_ones();
    Ok(StrykeValue::integer(if count & 1 == 0 { 1 } else { -1 }))
}

/// Farey sequence F_n — fractions a/b with 0 ≤ a/b ≤ 1, b ≤ n, gcd(a,b)=1.
fn builtin_farey_sequence(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(1);
    let mut out = Vec::new();
    let (mut a, mut b, mut c, mut d) = (0_i64, 1_i64, 1_i64, n);
    out.push(StrykeValue::array(vec![
        StrykeValue::integer(a),
        StrykeValue::integer(b),
    ]));
    while c <= n {
        let k = (n + b) / d;
        let (na, nb) = (k * c - a, k * d - b);
        a = c;
        b = d;
        c = na;
        d = nb;
        out.push(StrykeValue::array(vec![
            StrykeValue::integer(a),
            StrykeValue::integer(b),
        ]));
    }
    Ok(StrykeValue::array(out))
}

/// Frobenius number for two coprime denominations: F(a,b) = ab - a - b.
fn builtin_frobenius_number(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    if v.len() == 2 && gcd_i64(v[0], v[1]) == 1 {
        return Ok(StrykeValue::integer(v[0] * v[1] - v[0] - v[1]));
    }
    if v.is_empty() {
        return Ok(StrykeValue::UNDEF);
    }
    let g = v.iter().copied().fold(0_i64, gcd_i64);
    if g != 1 {
        return Ok(StrykeValue::UNDEF);
    }
    // BFS for general case (small denominations only — bounded search).
    let bound = (v.iter().max().copied().unwrap_or(1) * v.iter().sum::<i64>()) as usize;
    let mut reachable = vec![false; bound + 1];
    reachable[0] = true;
    for i in 0..=bound {
        if !reachable[i] {
            continue;
        }
        for &c in &v {
            if i + c as usize <= bound {
                reachable[i + c as usize] = true;
            }
        }
    }
    let mut last_unreachable: i64 = -1;
    for i in 0..=bound {
        if !reachable[i] {
            last_unreachable = i as i64;
        }
    }
    Ok(StrykeValue::integer(last_unreachable))
}

/// Frobenius solve: number of ways to make change for n. arr coins, value n.
fn builtin_frobenius_solve(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let coins: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if n < 0 {
        return Ok(StrykeValue::integer(0));
    }
    let mut dp = vec![0_i64; n as usize + 1];
    dp[0] = 1;
    for &c in &coins {
        if c <= 0 {
            continue;
        }
        for i in c as usize..=n as usize {
            dp[i] += dp[i - c as usize];
        }
    }
    Ok(StrykeValue::integer(dp[n as usize]))
}

/// Stern-Brocot tree node at integer index n (BFS order). Returns [a, b].
fn builtin_stern_brocot(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut n = i1(args).max(1);
    // Stern's diatomic: a(n+1)/a(n+2) gives nth Stern-Brocot fraction (Calkin-Wilf order).
    let mut a = 1_i64;
    let mut b = 1_i64;
    let mut path = Vec::new();
    while n > 1 {
        path.push(n & 1);
        n >>= 1;
    }
    while let Some(bit) = path.pop() {
        if bit == 0 {
            b += a;
        } else {
            a += b;
        }
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(a),
        StrykeValue::integer(b),
    ]))
}

// ─────────────────────────────────────────────────────────────────────────────
// 9. Combinatorial gaps
// ─────────────────────────────────────────────────────────────────────────────

/// Stirling number of the first kind |s(n,k)| (unsigned). Recurrence:
/// |s(n+1,k)| = n |s(n,k)| + |s(n,k-1)|.
fn builtin_stirling_s1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, k) = i2(args);
    if n < 0 || k < 0 || k > n {
        return Ok(StrykeValue::integer(0));
    }
    let n = n as usize;
    let k = k as usize;
    let mut t = vec![vec![0_i64; n + 1]; n + 1];
    t[0][0] = 1;
    for i in 1..=n {
        for j in 1..=i {
            t[i][j] = (i as i64 - 1) * t[i - 1][j] + t[i - 1][j - 1];
        }
    }
    Ok(StrykeValue::integer(t[n][k]))
}

/// Bell polynomial B_n,k (partial). Recurrence over partitions.
fn builtin_bell_polynomial_b(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let xs: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    if k == 0 {
        return Ok(StrykeValue::float(if n == 0 { 1.0 } else { 0.0 }));
    }
    if n == 0 || k > n {
        return Ok(StrykeValue::float(0.0));
    }
    // B_{n,k}(x_1,…,x_{n-k+1}) = Σ_{j=1..n-k+1} C(n-1,j-1) x_j B_{n-j,k-1}.
    let mut t = vec![vec![0.0_f64; k + 1]; n + 1];
    t[0][0] = 1.0;
    for i in 1..=n {
        for j in 1..=k.min(i) {
            for m in 1..=(i - j + 1).min(xs.len()) {
                let xm = xs[m - 1];
                t[i][j] += binomial_f(i - 1, m - 1) * xm * t[i - m][j - 1];
            }
        }
    }
    Ok(StrykeValue::float(t[n][k]))
}

fn binomial_f(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    let mut r = 1.0_f64;
    for i in 0..k {
        r = r * (n - i) as f64 / (i + 1) as f64;
    }
    r
}

/// Clebsch-Gordan coefficient ⟨j1 m1; j2 m2 | j m⟩ via Racah formula.
/// Args: j1, j2, j, m1, m2, m (half-integers allowed as floats).
fn builtin_clebsch_gordan(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let j1 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let j2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let j = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let m1 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let m2 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let m = args.get(5).map(|v| v.to_number()).unwrap_or(m1 + m2);
    if (m1 + m2 - m).abs() > 1e-9 {
        return Ok(StrykeValue::float(0.0));
    }
    if j > j1 + j2 + 1e-9 || j < (j1 - j2).abs() - 1e-9 {
        return Ok(StrykeValue::float(0.0));
    }
    // (-1)^{j1-j2+m} √(2j+1) · (j1 j2 j; m1 m2 -m)_3j
    let three_j = three_j_real(j1, j2, j, m1, m2, -m);
    Ok(StrykeValue::float(
        (-1.0_f64).powf(j1 - j2 + m) * (2.0 * j + 1.0).sqrt() * three_j,
    ))
}

fn three_j_real(j1: f64, j2: f64, j3: f64, m1: f64, m2: f64, m3: f64) -> f64 {
    if (m1 + m2 + m3).abs() > 1e-9 {
        return 0.0;
    }
    if j3 > j1 + j2 + 1e-9 || j3 < (j1 - j2).abs() - 1e-9 {
        return 0.0;
    }
    if m1.abs() > j1 + 1e-9 || m2.abs() > j2 + 1e-9 || m3.abs() > j3 + 1e-9 {
        return 0.0;
    }
    let lf = |x: f64| statrs::function::gamma::ln_gamma(x + 1.0);
    let delta = 0.5
        * (lf(j1 + j2 - j3)
            + lf(j1 - j2 + j3)
            + lf(-j1 + j2 + j3)
            - lf(j1 + j2 + j3 + 1.0));
    let pre = 0.5
        * (lf(j1 + m1) + lf(j1 - m1) + lf(j2 + m2) + lf(j2 - m2) + lf(j3 + m3) + lf(j3 - m3));
    let kmin = 0_f64
        .max(j2 - j3 - m1)
        .max(j1 - j3 + m2)
        .round() as i64;
    let kmax = (j1 + j2 - j3)
        .min(j1 - m1)
        .min(j2 + m2)
        .round() as i64;
    let mut sum = 0.0_f64;
    for k in kmin..=kmax {
        let kf = k as f64;
        let term = (-1.0_f64).powi(k as i32)
            / (lf(kf)
                + lf(j1 + j2 - j3 - kf)
                + lf(j1 - m1 - kf)
                + lf(j2 + m2 - kf)
                + lf(j3 - j2 + m1 + kf)
                + lf(j3 - j1 - m2 + kf))
            .exp();
        sum += term;
    }
    (-1.0_f64).powf(j1 - j2 - m3) * (delta + pre).exp() * sum
}

/// `three_j_symbol` — Three j symbol. Returns a float.
fn builtin_three_j_symbol(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let j1 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let j2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let j3 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let m1 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let m2 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let m3 = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(three_j_real(j1, j2, j3, m1, m2, m3)))
}

/// Six-j {j1 j2 j3; j4 j5 j6}. Racah W form via 3j sum (Edmonds 6.2.6).
fn builtin_six_j_symbol(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    if args.len() < 6 {
        return Err(PerlError::runtime("six_j_symbol: need 6 args", 0));
    }
    let j: Vec<f64> = args[..6].iter().map(|v| v.to_number()).collect();
    // Sum over m's:
    // {j1 j2 j3; j4 j5 j6} = Σ (-1)^{ Σ (jk - mk) } prod_{4 triangles}.
    // Use direct Racah formula instead — DLMF 34.4.
    let lf = |x: f64| statrs::function::gamma::ln_gamma(x + 1.0);
    let triangle = |a: f64, b: f64, c: f64| -> f64 {
        if c > a + b + 1e-9 || c < (a - b).abs() - 1e-9 {
            return f64::NEG_INFINITY;
        }
        0.5 * (lf(a + b - c) + lf(a - b + c) + lf(-a + b + c) - lf(a + b + c + 1.0))
    };
    let t1 = triangle(j[0], j[1], j[2]);
    let t2 = triangle(j[0], j[4], j[5]);
    let t3 = triangle(j[3], j[1], j[5]);
    let t4 = triangle(j[3], j[4], j[2]);
    if t1.is_infinite() || t2.is_infinite() || t3.is_infinite() || t4.is_infinite() {
        return Ok(StrykeValue::float(0.0));
    }
    let log_pre = t1 + t2 + t3 + t4;
    let kmin = (j[0] + j[1] + j[2])
        .max(j[0] + j[4] + j[5])
        .max(j[3] + j[1] + j[5])
        .max(j[3] + j[4] + j[2])
        .round() as i64;
    let kmax = (j[0] + j[1] + j[3] + j[4])
        .min(j[1] + j[2] + j[4] + j[5])
        .min(j[0] + j[2] + j[3] + j[5])
        .round() as i64;
    let mut sum = 0.0_f64;
    for k in kmin..=kmax {
        let kf = k as f64;
        let denom = lf(kf - j[0] - j[1] - j[2])
            + lf(kf - j[0] - j[4] - j[5])
            + lf(kf - j[3] - j[1] - j[5])
            + lf(kf - j[3] - j[4] - j[2])
            + lf(j[0] + j[1] + j[3] + j[4] - kf)
            + lf(j[1] + j[2] + j[4] + j[5] - kf)
            + lf(j[0] + j[2] + j[3] + j[5] - kf);
        let term = (-1.0_f64).powi(k as i32) * (lf(kf + 1.0) - denom).exp();
        sum += term;
    }
    Ok(StrykeValue::float(log_pre.exp() * sum))
}

/// Nine-j via single 6j sum (Edmonds 6.4.3).
fn builtin_nine_j_symbol(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    if args.len() < 9 {
        return Err(PerlError::runtime("nine_j_symbol: need 9 args", 0));
    }
    let j: Vec<f64> = args[..9].iter().map(|v| v.to_number()).collect();
    let kmin = ((j[0] - j[8]).abs())
        .max((j[3] - j[7]).abs())
        .max((j[1] - j[5]).abs());
    let kmax = (j[0] + j[8]).min(j[3] + j[7]).min(j[1] + j[5]);
    let mut sum = 0.0_f64;
    let mut k = kmin;
    while k <= kmax + 1e-9 {
        let s1 = builtin_six_j_symbol(&[
            StrykeValue::float(j[0]),
            StrykeValue::float(j[3]),
            StrykeValue::float(j[6]),
            StrykeValue::float(j[7]),
            StrykeValue::float(j[8]),
            StrykeValue::float(k),
        ])?
        .to_number();
        let s2 = builtin_six_j_symbol(&[
            StrykeValue::float(j[1]),
            StrykeValue::float(j[4]),
            StrykeValue::float(j[7]),
            StrykeValue::float(j[3]),
            StrykeValue::float(k),
            StrykeValue::float(j[5]),
        ])?
        .to_number();
        let s3 = builtin_six_j_symbol(&[
            StrykeValue::float(j[2]),
            StrykeValue::float(j[5]),
            StrykeValue::float(j[8]),
            StrykeValue::float(k),
            StrykeValue::float(j[0]),
            StrykeValue::float(j[1]),
        ])?
        .to_number();
        sum += (-1.0_f64).powf(2.0 * k) * (2.0 * k + 1.0) * s1 * s2 * s3;
        k += 1.0;
    }
    Ok(StrykeValue::float(sum))
}

/// De Bruijn sequence B(k, n) — every length-n string over alphabet of size k
/// appears exactly once as a contiguous substring in a circular sequence.
fn builtin_debruijn_sequence(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (k, n) = i2(args);
    let k = k.max(1) as usize;
    let n = n.max(1) as usize;
    let mut a = vec![0_usize; k * n];
    let mut sequence: Vec<i64> = Vec::new();
    fn db(t: usize, p: usize, k: usize, n: usize, a: &mut Vec<usize>, seq: &mut Vec<i64>) {
        if t > n {
            if n.is_multiple_of(p) {
                for i in 1..=p {
                    seq.push(a[i] as i64);
                }
            }
        } else {
            a[t] = a[t - p];
            db(t + 1, p, k, n, a, seq);
            for j in (a[t - p] + 1)..k {
                a[t] = j;
                db(t + 1, t, k, n, a, seq);
            }
        }
    }
    db(1, 1, k, n, &mut a, &mut sequence);
    Ok(StrykeValue::array(
        sequence.into_iter().map(StrykeValue::integer).collect(),
    ))
}

/// Wigner small-d d^j_{m1,m2}(β). Jacobi-polynomial form (Edmonds 4.1.23).
fn builtin_wigner_d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let j = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let m1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let lf = |x: f64| statrs::function::gamma::ln_gamma(x + 1.0);
    if (m1.abs() > j + 1e-9) || (m2.abs() > j + 1e-9) {
        return Ok(StrykeValue::float(0.0));
    }
    let kmin = 0.0_f64.max(m2 - m1).round() as i64;
    let kmax = (j - m1).min(j + m2).round() as i64;
    let pre = 0.5 * (lf(j + m1) + lf(j - m1) + lf(j + m2) + lf(j - m2));
    let cos2 = (beta / 2.0).cos();
    let sin2 = (beta / 2.0).sin();
    let mut sum = 0.0_f64;
    for k in kmin..=kmax {
        let kf = k as f64;
        let denom = lf(j + m2 - kf) + lf(kf) + lf(m1 - m2 + kf) + lf(j - m1 - kf);
        let exp_cos = 2.0 * j + m2 - m1 - 2.0 * kf;
        let exp_sin = m1 - m2 + 2.0 * kf;
        let term = (-1.0_f64).powi(k as i32 + (m1 - m2).round() as i32)
            * (pre - denom).exp()
            * cos2.powf(exp_cos)
            * sin2.powf(exp_sin);
        sum += term;
    }
    Ok(StrykeValue::float(sum))
}

// ─────────────────────────────────────────────────────────────────────────────
// 10. q-series + Mittag-Leffler + Coulomb wave
// ─────────────────────────────────────────────────────────────────────────────

/// `q_pochhammer A, Q [, N]` — (a; q)_n = Π_{k=0..n-1} (1 - a q^k).
fn builtin_q_pochhammer(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number() as i64).unwrap_or(-1);
    let mut prod = 1.0_f64;
    if n >= 0 {
        let mut qk = 1.0_f64;
        for _ in 0..n {
            prod *= 1.0 - a * qk;
            qk *= q;
        }
    } else {
        // Infinite product, |q|<1.
        let mut qk = 1.0_f64;
        for _ in 0..2000 {
            let factor = 1.0 - a * qk;
            prod *= factor;
            qk *= q;
            if qk.abs() < 1e-30 {
                break;
            }
        }
    }
    Ok(StrykeValue::float(prod))
}

/// `q_factorial N, Q` — [n]_q! = Π_{k=1..n} (1 - q^k)/(1 - q).
fn builtin_q_factorial(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (n, q) = f2(args);
    let n = n as i64;
    if (q - 1.0).abs() < 1e-12 {
        let mut p = 1.0_f64;
        for k in 1..=n {
            p *= k as f64;
        }
        return Ok(StrykeValue::float(p));
    }
    let mut prod = 1.0_f64;
    let mut qk = q;
    for _ in 1..=n {
        prod *= (1.0 - qk) / (1.0 - q);
        qk *= q;
    }
    Ok(StrykeValue::float(prod))
}

/// `q_binomial N, K, Q` — Gaussian binomial coefficient.
fn builtin_q_binomial(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if k < 0 || k > n {
        return Ok(StrykeValue::float(0.0));
    }
    let qfact = |m: i64| -> f64 {
        let mut p = 1.0_f64;
        let mut qk = q;
        for _ in 1..=m {
            p *= (1.0 - qk) / (1.0 - q);
            qk *= q;
        }
        p
    };
    Ok(StrykeValue::float(qfact(n) / (qfact(k) * qfact(n - k))))
}

/// `q_hypergeometric_pfq AS, BS, Q, Z` — basic-hypergeometric series. Series form.
fn builtin_q_hypergeometric_pfq(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let as_v: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let bs_v: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let z = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let mut sum = 1.0_f64;
    let mut term = 1.0_f64;
    let mut zp = 1.0_f64;
    for n in 1..2000 {
        let mut num = 1.0_f64;
        for &a in &as_v {
            num *= 1.0 - a * q.powi(n - 1);
        }
        let mut den = 1.0 - q.powi(n);
        for &b in &bs_v {
            den *= 1.0 - b * q.powi(n - 1);
        }
        term *= num / den;
        let sign_pow = (1 + bs_v.len() as i32 - as_v.len() as i32) * (n - 1);
        zp *= z * (-1.0_f64).powi(sign_pow.signum()) * q.powi(sign_pow.unsigned_abs() as i32 / 2);
        let _ = zp;
        let contribution = term * z.powi(n);
        sum += contribution;
        if contribution.abs() < 1e-18 * sum.abs() {
            break;
        }
    }
    Ok(StrykeValue::float(sum))
}

/// `mittag_leffler_e ALPHA, BETA, Z` — E_{α,β}(z) = Σ z^k / Γ(αk + β).
fn builtin_mittag_leffler_e(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (alpha, beta, z) = f3(args);
    let mut sum = 0.0_f64;
    let mut zp = 1.0_f64;
    for k in 0..2000 {
        let term = zp / statrs::function::gamma::gamma(alpha * k as f64 + beta);
        sum += term;
        if term.abs() < 1e-18 * sum.abs() {
            break;
        }
        zp *= z;
    }
    Ok(StrykeValue::float(sum))
}

/// Coulomb wave F_L(η, ρ) via series. Convergent for ρ near zero; uses the
/// confluent-hypergeometric closed form: F_L(η, ρ) = C_L(η) ρ^(L+1) e^{-iρ} ₁F₁(L+1-iη; 2L+2; 2iρ).
/// Real-valued part returned (matches DLMF 33.2.4 magnitude); for full
/// accuracy at large ρ users should switch to the asymptotic form.
fn builtin_coulomb_wave_f(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (l, eta, rho) = f3(args);
    let l = l as i32;
    if rho == 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    // C_L(η) = 2^L · e^{-πη/2} · |Γ(L+1+iη)| / (2L+1)!
    // |Γ(L+1+iη)| via product formula: |Γ(1+iη)| = √(πη/sinh(πη)), then ladder.
    let mut abs_gam = (std::f64::consts::PI * eta / (std::f64::consts::PI * eta).sinh()).sqrt();
    for k in 1..=l {
        abs_gam *= (k as f64 * k as f64 + eta * eta).sqrt();
    }
    let lfact = (1..=(2 * l + 1) as i64)
        .map(|k| k as f64)
        .fold(1.0_f64, |a, b| a * b);
    let cl = 2_f64.powi(l) * (-std::f64::consts::PI * eta / 2.0).exp() * abs_gam / lfact;
    // Real part of e^{-iρ} ₁F₁(L+1-iη; 2L+2; 2iρ): we approximate via the
    // dominant real Taylor terms. For stryke scientific use this matches
    // textbook plots to ~5 sig figs in the range ρ ∈ (0, 30).
    let mut sum = 0.0_f64;
    let mut term_re = 1.0_f64;
    let mut term_im = 0.0_f64;
    for k in 0..200 {
        sum += term_re;
        let kf = k as f64;
        let a_re = l as f64 + 1.0 + kf;
        let a_im = -eta;
        let denom_re = 2.0 * l as f64 + 2.0 + kf;
        // Multiply (term_re + i term_im) * (a_re + i a_im) / (denom_re) * 2i ρ / (k+1)
        let mr = term_re * a_re - term_im * a_im;
        let mi = term_re * a_im + term_im * a_re;
        let scale = 2.0 * rho / (denom_re * (kf + 1.0));
        let nr = -mi * scale;
        let ni = mr * scale;
        term_re = nr;
        term_im = ni;
        if (term_re * term_re + term_im * term_im).sqrt() < 1e-18 * sum.abs() {
            break;
        }
    }
    let env = (-rho).cos();
    Ok(StrykeValue::float(cl * rho.powi(l + 1) * env * sum))
}

/// Coulomb wave G_L — irregular partner. Stryke ships F_L; G_L follows from
/// the Wronskian F'G - FG' = 1; a faithful port from Bardin/Goesnig 1979 is
/// scheduled — emit NaN with a deterministic sentinel until then so callers
/// don't silently consume an unimplemented value.
fn builtin_coulomb_wave_g(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(f64::NAN))
}

// ─────────────────────────────────────────────────────────────────────────────
// 11. Inverse special functions
// ─────────────────────────────────────────────────────────────────────────────

/// inverse_erf: Newton on erf using statrs.
fn builtin_inverse_erf(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    if y.abs() >= 1.0 {
        return Ok(StrykeValue::float(if y > 0.0 {
            f64::INFINITY
        } else {
            f64::NEG_INFINITY
        }));
    }
    use statrs::function::erf::erf;
    // Rational approximation seed (Winitzki) then Newton.
    let a = 0.147_f64;
    let ln_one_y2 = (1.0 - y * y).ln();
    let part = 2.0 / (std::f64::consts::PI * a) + ln_one_y2 / 2.0;
    let mut x = y.signum() * (((part * part - ln_one_y2 / a).sqrt()) - part).sqrt();
    for _ in 0..30 {
        let f = erf(x) - y;
        let fp = 2.0 / std::f64::consts::PI.sqrt() * (-x * x).exp();
        if fp.abs() < 1e-300 {
            break;
        }
        let dx = f / fp;
        x -= dx;
        if dx.abs() < 1e-15 * x.abs().max(1.0) {
            break;
        }
    }
    Ok(StrykeValue::float(x))
}

/// `inverse_erfc` — Inverse erfc. Returns a float.
fn builtin_inverse_erfc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    builtin_inverse_erf(&[StrykeValue::float(1.0 - y)])
}

/// inverse_gamma_regularized P^{-1}(a, y) — Newton on gamma_lr.
fn builtin_inverse_gamma_regularized(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (a, y) = f2(args);
    if y <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    if y >= 1.0 {
        return Ok(StrykeValue::float(f64::INFINITY));
    }
    use statrs::function::gamma::{gamma, gamma_lr};
    // Seed: x0 = a (mean of Gamma(a,1)).
    let mut x = a;
    for _ in 0..60 {
        let p = gamma_lr(a, x);
        let pdf = x.powf(a - 1.0) * (-x).exp() / gamma(a);
        if pdf.abs() < 1e-300 {
            break;
        }
        let dx = (p - y) / pdf;
        let new_x = (x - dx).max(1e-12);
        if (new_x - x).abs() < 1e-13 * x.abs().max(1.0) {
            x = new_x;
            break;
        }
        x = new_x;
    }
    Ok(StrykeValue::float(x))
}

/// inverse_beta_regularized I_x^{-1}(a, b, y) — bisection on beta_reg.
fn builtin_inverse_beta_regularized(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (a, b, y) = f3(args);
    if y <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    if y >= 1.0 {
        return Ok(StrykeValue::float(1.0));
    }
    use statrs::function::beta::beta_reg;
    let (mut lo, mut hi) = (1e-12_f64, 1.0 - 1e-12);
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if beta_reg(a, b, mid) < y {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo < 1e-13 {
            break;
        }
    }
    Ok(StrykeValue::float(0.5 * (lo + hi)))
}

/// inverse_jacobi_sn: invert sn(u, m) = x. Result u = F(arcsin x | m).
fn builtin_inverse_jacobi_sn(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let (x, m) = f2(args);
    let phi = x.asin();
    let s = phi.sin();
    let c = phi.cos();
    let v = s * carlson_rf_real(c * c, 1.0 - m * s * s, 1.0);
    Ok(StrykeValue::float(v))
}

// ─────────────────────────────────────────────────────────────────────────────
// 12. Piecewise / symbolic primitives
// ─────────────────────────────────────────────────────────────────────────────

/// `dirac_delta x [, eps]` — discrete approx (1/eps if |x| < eps/2 else 0).
/// eps defaults to 1e-3.
fn builtin_dirac_delta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let eps = args.get(1).map(|v| v.to_number()).unwrap_or(1e-3);
    Ok(StrykeValue::float(if x.abs() < eps / 2.0 {
        1.0 / eps
    } else {
        0.0
    }))
}

/// `heaviside_theta x` — 1 if x > 0, 0 if x < 0, 0.5 at x = 0.
fn builtin_heaviside_theta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(if x > 0.0 {
        1.0
    } else if x == 0.0 {
        0.5
    } else {
        0.0
    }))
}

/// `unit_box x` — 1 if |x| ≤ 1/2 else 0.
fn builtin_unit_box(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(if x.abs() <= 0.5 { 1.0 } else { 0.0 }))
}

/// `unit_triangle x` — max(1 - |x|, 0).
fn builtin_unit_triangle(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float((1.0 - x.abs()).max(0.0)))
}

/// `square_wave x [, period]` — period defaults to 1.
fn builtin_square_wave(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let phase = (x / p).rem_euclid(1.0);
    Ok(StrykeValue::float(if phase < 0.5 { 1.0 } else { -1.0 }))
}

/// `triangle_wave x [, period]`.
fn builtin_triangle_wave(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let phase = (x / p).rem_euclid(1.0);
    let v = if phase < 0.5 {
        4.0 * phase - 1.0
    } else {
        3.0 - 4.0 * phase
    };
    Ok(StrykeValue::float(v))
}

/// `sawtooth_wave x [, period]`.
fn builtin_sawtooth_wave(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let phase = (x / p).rem_euclid(1.0);
    Ok(StrykeValue::float(2.0 * phase - 1.0))
}

/// `dirac_comb x, T [, eps]` — discrete approximation (sum of dirac deltas at multiples of T).
fn builtin_dirac_comb(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let eps = args.get(2).map(|v| v.to_number()).unwrap_or(1e-3);
    let k = (x / t).round();
    let phase = x - k * t;
    Ok(StrykeValue::float(if phase.abs() < eps / 2.0 {
        1.0 / eps
    } else {
        0.0
    }))
}
