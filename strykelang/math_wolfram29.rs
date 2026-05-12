// Batch 29 — special functions extra: hypergeometric, Mathieu, Whittaker, Kelvin, etc.

// 2F1 hypergeometric series ₂F₁(a,b;c;z) for |z|<1
fn builtin_hyper2f1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let z = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    if z.abs() >= 1.0 { return Ok(StrykeValue::float(f64::NAN)); }
    let mut sum = 1.0_f64;
    let mut term = 1.0_f64;
    for n in 0..200 {
        let denom = (c + n as f64) * (n as f64 + 1.0);
        if denom == 0.0 { break; }
        term *= (a + n as f64) * (b + n as f64) * z / denom;
        sum += term;
        if term.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(sum))
}

// 1F1 confluent hypergeometric (Kummer) series M(a;b;z)
fn builtin_hyper1f1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let z = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let mut sum = 1.0_f64;
    let mut term = 1.0_f64;
    for n in 0..500 {
        let denom = (b + n as f64) * (n as f64 + 1.0);
        if denom == 0.0 { break; }
        term *= (a + n as f64) * z / denom;
        sum += term;
        if term.abs() < 1e-15 * sum.abs() { break; }
    }
    Ok(StrykeValue::float(sum))
}

// 0F1 (limit confluent hypergeometric) — useful for Bessel-related
fn builtin_hyper0f1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let b = f1(args);
    let z = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut sum = 1.0_f64;
    let mut term = 1.0_f64;
    for n in 0..500 {
        let denom = (b + n as f64) * (n as f64 + 1.0);
        if denom == 0.0 { break; }
        term *= z / denom;
        sum += term;
        if term.abs() < 1e-15 * sum.abs() { break; }
    }
    Ok(StrykeValue::float(sum))
}

// Pochhammer (rising factorial) (a)_n
fn builtin_pochhammer(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let mut p = 1.0_f64;
    for k in 0..n {
        p *= a + k as f64;
    }
    Ok(StrykeValue::float(p))
}

// Falling factorial

// Mathieu (Floquet) cosine ce_n(z, q) — first-order series approximation
fn builtin_mathieu_ce0(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(1.0 + 0.5 * q * (2.0 * z).cos()))
}
fn builtin_mathieu_se1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(z.sin() + (q / 8.0) * (3.0 * z).sin()))
}

// Parabolic cylinder D_n(x) for n=0,1
fn builtin_parabolic_d0(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float((-x * x / 4.0).exp()))
}
fn builtin_parabolic_d1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(x * (-x * x / 4.0).exp()))
}

// Whittaker M(a, b, z) (in terms of 1F1): M_{k,μ}(z) = z^{μ+1/2} e^{-z/2} M(μ-k+1/2, 2μ+1, z)
fn builtin_whittaker_m(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let z = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let m_val = {
        // 1F1(μ - k + 1/2; 2μ + 1; z)
        let a = mu - k + 0.5;
        let b = 2.0 * mu + 1.0;
        let mut sum = 1.0_f64;
        let mut term = 1.0_f64;
        for n in 0..500 {
            let denom = (b + n as f64) * (n as f64 + 1.0);
            if denom == 0.0 { break; }
            term *= (a + n as f64) * z / denom;
            sum += term;
            if term.abs() < 1e-15 * sum.abs() { break; }
        }
        sum
    };
    Ok(StrykeValue::float(z.powf(mu + 0.5) * (-z / 2.0).exp() * m_val))
}

// Struve function H₀(x) — series form
fn builtin_struve_h0(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let pi = std::f64::consts::PI;
    let mut sum = 0.0;
    let mut term: f64 = 2.0 * x / pi;
    for k in 0..50 {
        sum += term;
        let denom = ((2 * k + 3) * (2 * k + 3)) as f64;
        term *= -(x * x) / denom;
        if term.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(sum))
}
fn builtin_struve_h1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let pi = std::f64::consts::PI;
    let mut sum = 0.0;
    let mut term: f64 = 2.0 * x * x / (3.0 * pi);
    for k in 0..50 {
        sum += term;
        let denom = ((2 * k + 3) * (2 * k + 5)) as f64;
        term *= -(x * x) / denom;
        if term.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(sum))
}

// Lambert W principal branch (Halley iteration)
fn builtin_lambert_w0(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x < -1.0 / std::f64::consts::E { return Ok(StrykeValue::float(f64::NAN)); }
    let mut w: f64 = if x < 1.0 { x * (1.0 - x + 2.0 * x * x) } else { x.ln() - x.ln().ln() };
    for _ in 0..50 {
        let ew = w.exp();
        let f = w * ew - x;
        let fp = ew * (1.0 + w);
        let fpp = ew * (2.0 + w);
        let dw = f / (fp - f * fpp / (2.0 * fp));
        let w_new = w - dw;
        if (w_new - w).abs() < 1e-12 { return Ok(StrykeValue::float(w_new)); }
        w = w_new;
    }
    Ok(StrykeValue::float(w))
}

// Wright omega ω(z) = W(e^z) (principal)
fn builtin_wright_omega(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    builtin_lambert_w0(&[StrykeValue::float(z.exp())])
}

// Sinhc(x) = sinh(x)/x
fn builtin_sinhc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x.abs() < 1e-9 { return Ok(StrykeValue::float(1.0)); }
    Ok(StrykeValue::float(x.sinh() / x))
}
// Coshc(x) = cosh(x)/x for x≠0 (only defined as scaled cosh / use cosh-1/x²)
fn builtin_cosh_minus1_over_x2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x.abs() < 1e-9 { return Ok(StrykeValue::float(0.5)); }
    Ok(StrykeValue::float((x.cosh() - 1.0) / (x * x)))
}

// Sici (sine integral) Si(x) — series for moderate |x|
fn builtin_sine_integral_si(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let mut sum = 0.0;
    let mut term = x;
    for n in 0..200 {
        sum += term / (2.0 * n as f64 + 1.0);
        term *= -(x * x) / ((2 * n + 2) * (2 * n + 3)) as f64;
        if term.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(sum))
}

// Cosine integral Ci(x) — for x>0
fn builtin_cosine_integral_ci(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x <= 0.0 { return Ok(StrykeValue::float(f64::NAN)); }
    let euler_gamma = 0.5772156649015329;
    let mut sum = 0.0;
    let mut term = -(x * x) / 4.0;
    sum += term;
    for n in 1..100 {
        let next = -term * x * x / (((2 * n + 1) * (2 * n + 2)) as f64);
        let contrib = next / (2.0 * (n + 1) as f64);
        sum += contrib;
        term = next;
        if contrib.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(euler_gamma + x.ln() + sum))
}

// Exponential integral E1(x) for x>0 (series for small, asymptotic for large)
fn builtin_exp_integral_e1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    let euler_gamma = 0.5772156649015329;
    if x < 1.0 {
        let mut sum: f64 = 0.0;
        let mut term: f64 = -x;
        sum += term;
        for n in 2..100 {
            term *= -x * (n - 1) as f64 / (n * n) as f64;
            let new_term = term;
            sum += new_term;
            if new_term.abs() < 1e-15 { break; }
        }
        Ok(StrykeValue::float(-euler_gamma - x.ln() - sum))
    } else {
        // Asymptotic
        let mut sum: f64 = 1.0;
        let mut term: f64 = 1.0;
        for n in 1..50 {
            term *= -(n as f64) / x;
            if term.abs() > sum.abs() * 0.5 { break; }
            sum += term;
        }
        Ok(StrykeValue::float((-x).exp() * sum / x))
    }
}

// Sin² integral / Fresnel S(x) — series

// Fresnel C(x)

// Dawson function D(x) = e^{-x²} ∫₀ˣ e^{t²} dt — series for small x
fn builtin_dawson_function(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x.abs() < 4.0 {
        let mut sum = 0.0;
        let mut term = x;
        for n in 0..200 {
            sum += term / (2.0 * n as f64 + 1.0);
            term *= -(x * x) / (n as f64 + 1.0);
            if term.abs() < 1e-15 { break; }
        }
        Ok(StrykeValue::float((-x * x).exp() * sum))
    } else {
        // Asymptotic
        let mut sum = 1.0_f64;
        let mut term = 1.0_f64;
        for n in 1..50 {
            term *= (2 * n - 1) as f64 / (2.0 * x * x);
            if term.abs() > sum.abs() * 0.5 { break; }
            sum += term;
        }
        Ok(StrykeValue::float(0.5 * sum / x))
    }
}

// Owen's T function approximation (4-term Patefield-Tandy)
fn builtin_owen_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if a == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let pi = std::f64::consts::PI;
    let mut sum = 0.0;
    let mut term = a;
    for n in 0..50 {
        let denom = 2.0 * n as f64 + 1.0;
        sum += term / denom;
        term *= -a * a;
        if term.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float((1.0 / (2.0 * pi)) * (-(h * h) / 2.0).exp() * sum))
}

// Spherical Bessel j₀
fn builtin_spherical_bessel_j0(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x.abs() < 1e-9 { return Ok(StrykeValue::float(1.0)); }
    Ok(StrykeValue::float(x.sin() / x))
}
// Spherical Bessel j₁
fn builtin_spherical_bessel_j1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x.abs() < 1e-9 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((x.sin() - x * x.cos()) / (x * x)))
}
// Spherical Bessel y₀
fn builtin_spherical_bessel_y0(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x == 0.0 { return Ok(StrykeValue::float(f64::NEG_INFINITY)); }
    Ok(StrykeValue::float(-x.cos() / x))
}
// Spherical Bessel y₁
fn builtin_spherical_bessel_y1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x == 0.0 { return Ok(StrykeValue::float(f64::NEG_INFINITY)); }
    Ok(StrykeValue::float(-(x.cos() / (x * x) + x.sin() / x)))
}

// Modified spherical Bessel i₀
fn builtin_mod_sph_bessel_i0(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x.abs() < 1e-9 { return Ok(StrykeValue::float(1.0)); }
    Ok(StrykeValue::float(x.sinh() / x))
}
fn builtin_mod_sph_bessel_i1(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x.abs() < 1e-9 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((x.cosh() - x.sinh() / x) / x))
}
fn builtin_mod_sph_bessel_k0(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    if x <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    let pi = std::f64::consts::PI;
    Ok(StrykeValue::float(pi / 2.0 * (-x).exp() / x))
}

// Coulomb wave function F_L (L=0, simplified)
fn builtin_coulomb_f0(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let eta = f1(args);
    let rho = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if rho == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let pi = std::f64::consts::PI;
    let amplitude = 2.0 * pi * eta / ((2.0 * pi * eta).exp() - 1.0).max(1e-30);
    Ok(StrykeValue::float(amplitude.sqrt() * (rho - eta * rho.ln().max(1e-30)).sin()))
}

// Polylogarithm Li₂ (dilogarithm) — series
fn builtin_polylog_li2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    if z.abs() >= 1.0 { return Ok(StrykeValue::float(f64::NAN)); }
    let mut sum = 0.0;
    let mut term = z;
    for k in 1..500 {
        sum += term / (k * k) as f64;
        term *= z;
        if term.abs() < 1e-18 { break; }
    }
    Ok(StrykeValue::float(sum))
}
// Polylog Li_n (general)
fn builtin_polylog_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args) as i32;
    let z = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if z.abs() >= 1.0 { return Ok(StrykeValue::float(f64::NAN)); }
    let mut sum = 0.0;
    let mut term = z;
    for k in 1..500 {
        sum += term / (k as f64).powi(n);
        term *= z;
        if term.abs() < 1e-18 { break; }
    }
    Ok(StrykeValue::float(sum))
}

// Hurwitz zeta ζ(s, a) — series for s > 1

// Dirichlet eta η(s) = (1 - 2^{1-s}) ζ(s) — direct alternating series

// Dirichlet beta β(s) = sum (-1)^n / (2n+1)^s

// Catalan number n-th approximation via beta function

// Apéry's constant ζ(3) numerical

// Inverse tangent integral Ti₂(x) = sum (-1)^k x^{2k+1}/(2k+1)^2
fn builtin_ti2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mut sum = 0.0;
    let mut term = x;
    for k in 0..500 {
        let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
        sum += sign * term / (2.0 * k as f64 + 1.0).powi(2);
        term *= x * x;
        if term.abs() < 1e-18 { break; }
    }
    Ok(StrykeValue::float(sum))
}

// Clausen Cl₂(θ) = sum sin(nθ)/n²
fn builtin_clausen_cl2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    let mut sum = 0.0;
    for n in 1..1000 {
        let term = (n as f64 * theta).sin() / (n as f64).powi(2);
        sum += term;
        if term.abs() < 1e-15 { break; }
    }
    Ok(StrykeValue::float(sum))
}

// Bose-Einstein integral g_s(x) = sum x^k/k^s (alias for polylog)
fn builtin_bose_einstein_g(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_polylog_n(args)
}

// Fermi-Dirac integral F_n(x) — numerical via simpson approx
fn builtin_fermi_dirac_int(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let upper = (x + 30.0).max(50.0);
    let steps = 1000_usize;
    let h = upper / steps as f64;
    let f = |t: f64| t.powf(n) / ((t - x).exp() + 1.0);
    let mut sum = 0.5 * (f(0.0) + f(upper));
    for i in 1..steps {
        sum += f(i as f64 * h);
    }
    Ok(StrykeValue::float(h * sum))
}

// Theta function ϑ_3(z, q) — first 10 terms
fn builtin_theta3(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut sum = 1.0;
    for n in 1..50 {
        let term = 2.0 * q.powi(n * n) * (2.0 * n as f64 * z).cos();
        sum += term;
        if term.abs() < 1e-18 { break; }
    }
    Ok(StrykeValue::float(sum))
}
fn builtin_theta2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut sum = 0.0;
    for n in 0..50 {
        let term = 2.0 * q.powi(((2 * n + 1) * (2 * n + 1)) / 4) * ((2.0 * n as f64 + 1.0) * z).cos();
        sum += term;
        if term.abs() < 1e-18 { break; }
    }
    Ok(StrykeValue::float(sum))
}

// Jacobi sn (small q expansion, simplified)
fn builtin_jacobi_sn_small_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let u = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(u.sin() + q * u.sin().powi(3)))
}
fn builtin_jacobi_cn_small_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let u = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(u.cos() - q * u.cos() * u.sin().powi(2)))
}
fn builtin_jacobi_dn_small_q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let u = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(1.0 - 0.5 * q * (1.0 - (2.0 * u).cos())))
}

// Riemann ξ(s) (with completed gamma factor — simplified)
fn builtin_riemann_xi(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = f1(args);
    if s <= 1.0 { return Ok(StrykeValue::float(f64::NAN)); }
    let pi = std::f64::consts::PI;
    let mut zeta = 0.0;
    for n in 1..10000 {
        zeta += 1.0 / (n as f64).powf(s);
    }
    Ok(StrykeValue::float(0.5 * s * (s - 1.0) * pi.powf(-s / 2.0) * libm::tgamma(s / 2.0) * zeta))
}

// Bessel J of arbitrary integer order n via series
fn builtin_bessel_jn_general(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number() as i32).unwrap_or(0);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n < 0 { return Ok(StrykeValue::float(f64::NAN)); }
    let mut sum = 0.0;
    let mut fact_n = 1.0_f64;
    for k in 1..=n { fact_n *= k as f64; }
    let mut term = (x / 2.0).powi(n) / fact_n;
    sum += term;
    for k in 1..200 {
        term *= -(x * x) / (4.0 * k as f64 * (k as f64 + n as f64));
        sum += term;
        if term.abs() < 1e-18 { break; }
    }
    Ok(StrykeValue::float(sum))
}

// Modified Bessel I_n via series
fn builtin_bessel_in_general(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number() as i32).unwrap_or(0);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n < 0 { return Ok(StrykeValue::float(f64::NAN)); }
    let mut sum = 0.0;
    let mut fact_n = 1.0_f64;
    for k in 1..=n { fact_n *= k as f64; }
    let mut term = (x / 2.0).powi(n) / fact_n;
    sum += term;
    for k in 1..200 {
        term *= (x * x) / (4.0 * k as f64 * (k as f64 + n as f64));
        sum += term;
        if term.abs() < 1e-18 { break; }
    }
    Ok(StrykeValue::float(sum))
}
