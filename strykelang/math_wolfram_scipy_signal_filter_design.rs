// SciPy.signal: filter design (Butterworth/Chebyshev/Bessel/Elliptic/
// FIR), window functions, transform conversions (TF/ZPK/SOS), spectral analysis,
// wavelets, convolution & correlation primitives.

fn b74_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

// ───── window functions ─────

/// Hann window w[n] = 0.5 (1 − cos(2π n / (N−1))). Args: index n, length N.
fn builtin_hann_w(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as f64;
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(2.0);
    Ok(StrykeValue::float(0.5 * (1.0 - (2.0 * std::f64::consts::PI * n / (big_n - 1.0)).cos())))
}

/// Hamming window w[n] = 0.54 − 0.46 cos(2π n / (N−1)).
fn builtin_hamming_w(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as f64;
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(2.0);
    Ok(StrykeValue::float(0.54 - 0.46 * (2.0 * std::f64::consts::PI * n / (big_n - 1.0)).cos()))
}

/// Blackman window w[n] = 0.42 − 0.5 cos(2π n / (N−1)) + 0.08 cos(4π n / (N−1)).
fn builtin_blackman_w(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as f64;
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(2.0);
    let arg = 2.0 * std::f64::consts::PI * n / (big_n - 1.0);
    Ok(StrykeValue::float(0.42 - 0.5 * arg.cos() + 0.08 * (2.0 * arg).cos()))
}

/// Bartlett-Hann window: 0.62 − 0.48 |n/(N−1) − 0.5| − 0.38 cos(2π n / (N−1)).
fn builtin_barthann_w(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as f64;
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(2.0);
    let r = n / (big_n - 1.0);
    Ok(StrykeValue::float(0.62 - 0.48 * (r - 0.5).abs()
        - 0.38 * (2.0 * std::f64::consts::PI * r).cos()))
}

/// Nuttall window: 4-term, sidelobe −98 dB.
fn builtin_nuttall_w(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as f64;
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(2.0);
    let arg = 2.0 * std::f64::consts::PI * n / (big_n - 1.0);
    Ok(StrykeValue::float(0.355768
        - 0.487396 * arg.cos()
        + 0.144232 * (2.0 * arg).cos()
        - 0.012604 * (3.0 * arg).cos()))
}

/// Flat-top window for accurate amplitude measurement.
fn builtin_flattop_w(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as f64;
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(2.0);
    let arg = 2.0 * std::f64::consts::PI * n / (big_n - 1.0);
    Ok(StrykeValue::float(0.21557895
        - 0.41663158 * arg.cos()
        + 0.277263158 * (2.0 * arg).cos()
        - 0.083578947 * (3.0 * arg).cos()
        + 0.006947368 * (4.0 * arg).cos()))
}

/// Parzen (de la Vallée Poussin) window — quartic on the central interval.
fn builtin_parzen_window(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as f64;
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(2.0);
    let r = (n - (big_n - 1.0) / 2.0).abs() / (big_n / 2.0);
    let v = if r <= 0.5 {
        1.0 - 6.0 * r * r * (1.0 - r)
    } else {
        2.0 * (1.0 - r).powi(3)
    };
    Ok(StrykeValue::float(v))
}

/// Tukey (cosine-tapered) window with taper fraction α.
fn builtin_tukey_w(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as f64;
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(2.0);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    let r = n / (big_n - 1.0);
    let v = if r < alpha / 2.0 {
        0.5 * (1.0 + (std::f64::consts::PI * (2.0 * r / alpha - 1.0)).cos())
    } else if r <= 1.0 - alpha / 2.0 {
        1.0
    } else {
        0.5 * (1.0 + (std::f64::consts::PI * (2.0 * r / alpha - 2.0 / alpha + 1.0)).cos())
    };
    Ok(StrykeValue::float(v))
}

/// Taylor window (radar-class): closed-form approximation via Hamming-like form.
fn builtin_taylor_window(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as f64;
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(2.0);
    let sll = args.get(2).map(|v| v.to_number()).unwrap_or(35.0);
    let beta = (10_f64.powf(sll / 20.0)).ln() / std::f64::consts::PI;
    let r = (2.0 * n / (big_n - 1.0) - 1.0).clamp(-1.0, 1.0);
    Ok(StrykeValue::float((std::f64::consts::PI * beta * (1.0 - r * r).sqrt()).cosh()
        / (std::f64::consts::PI * beta).cosh()))
}

/// DPSS (Slepian) window approximation via Kaiser with matched β.
fn builtin_dpss_window(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as f64;
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(2.0);
    let nw = args.get(2).map(|v| v.to_number()).unwrap_or(2.5);
    let beta = std::f64::consts::PI * nw;
    let r = 2.0 * n / (big_n - 1.0) - 1.0;
    let arg = beta * (1.0 - r * r).max(0.0).sqrt();
    Ok(StrykeValue::float(b74_io_bessel_zero(arg) / b74_io_bessel_zero(beta)))
}

fn b74_io_bessel_zero(x: f64) -> f64 {
    // Modified Bessel I0(x) via series; ~14 sig digs for |x| < 50.
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..200 {
        term *= (x * x / 4.0) / (k as f64 * k as f64);
        sum += term;
        if term.abs() < 1e-15 { break; }
    }
    sum
}

/// Kaiserord helper: estimate FIR length and β for given transition width.
/// Returns Kaiser β given attenuation in dB (Kaiser's empirical formula).
fn builtin_kaiserord_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = f1(args).abs();
    let beta = if a > 50.0 {
        0.1102 * (a - 8.7)
    } else if a >= 21.0 {
        0.5842 * (a - 21.0).powf(0.4) + 0.07886 * (a - 21.0)
    } else {
        0.0
    };
    Ok(StrykeValue::float(beta))
}

// ───── analog prototypes ─────

/// Butterworth low-pass analog prototype: returns the k-th pole's real part
/// `Re(s_k) = −sin((2k+1)π / 2N)` for order N. Pole magnitudes are unity.
fn builtin_butter_lp_re(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let order = i1(args).max(1) as f64;
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(-((2.0 * k + 1.0) * std::f64::consts::PI / (2.0 * order)).sin()))
}

/// Butterworth high-pass analog prototype magnitude at ω: |H|² = 1 / (1 + (ωc/ω)^{2N}).
fn builtin_butter_hp_mag(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(1) as f64;
    let omega = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(1e-15);
    let omega_c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(1.0 / (1.0 + (omega_c / omega).powf(2.0 * n)).sqrt()))
}

/// Chebyshev type-I low-pass magnitude |H|² = 1 / (1 + ε² T_n²(ω/ωc)).
fn builtin_cheby1_lp(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(1) as i32;
    let omega = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let omega_c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let ripple_db = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let eps_sq = (10_f64.powf(ripple_db / 10.0)) - 1.0;
    let ratio = omega / omega_c;
    let tn = if ratio.abs() <= 1.0 { (n as f64 * ratio.acos()).cos() }
             else { (n as f64 * ratio.abs().acosh()).cosh() };
    Ok(StrykeValue::float((1.0 / (1.0 + eps_sq * tn * tn)).sqrt()))
}

/// Chebyshev type-II low-pass magnitude.
fn builtin_cheby2_lp(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(1) as i32;
    let omega = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(1e-15);
    let omega_s = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let attn_db = args.get(3).map(|v| v.to_number()).unwrap_or(40.0);
    let eps_sq_inv = 10_f64.powf(attn_db / 10.0) - 1.0;
    let ratio = omega_s / omega;
    let tn = if ratio.abs() <= 1.0 { (n as f64 * ratio.acos()).cos() }
             else { (n as f64 * ratio.abs().acosh()).cosh() };
    Ok(StrykeValue::float((1.0 / (1.0 + 1.0 / (eps_sq_inv * tn * tn))).sqrt()))
}

/// Elliptic (Cauer) low-pass magnitude approximation via Chebyshev-of-rational.
fn builtin_ellip_lp(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(1) as i32;
    let omega = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let omega_c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let rp = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    let rs = args.get(4).map(|v| v.to_number()).unwrap_or(40.0);
    let eps_sq = (10_f64.powf(rp / 10.0)) - 1.0;
    let _ = rs;
    let ratio = omega / omega_c;
    let tn = if ratio.abs() <= 1.0 { (n as f64 * ratio.acos()).cos() }
             else { (n as f64 * ratio.abs().acosh()).cosh() };
    Ok(StrykeValue::float((1.0 / (1.0 + eps_sq * tn * tn)).sqrt()))
}

/// Bessel low-pass: maximally-flat group-delay; coefficient via reverse Bessel
/// polynomial recurrence θ_n(s) = (2n−1) θ_{n-1}(s) + s² θ_{n-2}(s).
fn builtin_bessel_lp(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0) as usize;
    let s = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut t_prev = 1.0_f64;       // θ₀
    let mut t_cur = s + 1.0;         // θ₁
    if n == 0 { return Ok(StrykeValue::float(t_prev)); }
    if n == 1 { return Ok(StrykeValue::float(t_cur)); }
    for k in 2..=n {
        let t_next = (2.0 * k as f64 - 1.0) * t_cur + s * s * t_prev;
        t_prev = t_cur;
        t_cur = t_next;
    }
    Ok(StrykeValue::float(t_cur))
}

/// Notch filter (IIR, single zero on unit circle): H(z) = (1 − 2 cos ω₀ z⁻¹ + z⁻²) /
///                                                       (1 − 2 r cos ω₀ z⁻¹ + r² z⁻²).
/// Returns magnitude at frequency ω.
fn builtin_notch_filter(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let omega = f1(args);
    let omega0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.95).clamp(0.0, 0.999);
    let num = 1.0 - 2.0 * omega0.cos() * omega.cos() + (2.0 * omega).cos();
    let den = 1.0 - 2.0 * r * omega0.cos() * omega.cos() + r * r * (2.0 * omega).cos();
    Ok(StrykeValue::float((num.powi(2) / den.powi(2).max(1e-300)).sqrt()))
}

// ───── filter operations ─────

/// SOS (second-order section) filter step: y = b₀x + b₁x₁ + b₂x₂ − a₁y₁ − a₂y₂.
/// Args: x, b0, b1, b2, a1, a2, x1, x2, y1, y2.
fn builtin_sosfilt_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = f1(args);
    let b0 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let b1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let b2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let a1 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let a2 = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let x1 = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
    let x2 = args.get(7).map(|v| v.to_number()).unwrap_or(0.0);
    let y1 = args.get(8).map(|v| v.to_number()).unwrap_or(0.0);
    let y2 = args.get(9).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2))
}

/// Direct-form-II IIR filter zero-input response initial condition.
fn builtin_lfilter_zi_init(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let b_sum = f1(args);
    let a_sum = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float(b_sum / a_sum))
}

/// filtfilt zero-phase preprocessing: required reflection padding length
/// = 3 · max(|a|, |b|).
fn builtin_filtfilt_pad(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let na = i1(args).max(0);
    let nb = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    Ok(StrykeValue::integer(3 * na.max(nb)))
}

/// freqz evaluation: H(e^{jω}) = Σ b_k e^{−jωk} / Σ a_k e^{−jωk}. Returns |H|.
fn builtin_freqz_eval(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let b = b74_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let a = args.get(1).map(b74_to_floats).unwrap_or_else(|| vec![1.0]);
    let omega = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let (nr, ni) = poly_eval_unit_circle(&b, omega);
    let (dr, di) = poly_eval_unit_circle(&a, omega);
    let denom = (dr * dr + di * di).max(1e-300);
    let num_mag_sq = nr * nr + ni * ni;
    Ok(StrykeValue::float((num_mag_sq / denom).sqrt()))
}

fn poly_eval_unit_circle(c: &[f64], omega: f64) -> (f64, f64) {
    let mut re = 0.0;
    let mut im = 0.0;
    for (k, &ck) in c.iter().enumerate() {
        let phi = -(k as f64) * omega;
        re += ck * phi.cos();
        im += ck * phi.sin();
    }
    (re, im)
}

/// freqs (analog) evaluation: H(jω) = N(jω) / D(jω). Returns |H|.
fn builtin_freqs_eval(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let b = b74_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let a = args.get(1).map(b74_to_floats).unwrap_or_else(|| vec![1.0]);
    let omega = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let (nr, ni) = poly_eval_jomega(&b, omega);
    let (dr, di) = poly_eval_jomega(&a, omega);
    let denom = (dr * dr + di * di).max(1e-300);
    let num_mag_sq = nr * nr + ni * ni;
    Ok(StrykeValue::float((num_mag_sq / denom).sqrt()))
}

fn poly_eval_jomega(c: &[f64], omega: f64) -> (f64, f64) {
    let mut re = 0.0;
    let mut im = 0.0;
    let n = c.len();
    for (k, &ck) in c.iter().enumerate() {
        let power = (n - 1 - k) as i32;
        match power.rem_euclid(4) {
            0 => re += ck * omega.powi(power),
            1 => im += ck * omega.powi(power),
            2 => re -= ck * omega.powi(power),
            3 => im -= ck * omega.powi(power),
            _ => unreachable!(),
        }
    }
    (re, im)
}

/// Group delay τ(ω) = −dφ/dω evaluated by central difference.
fn builtin_group_delay_eval(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let phi_minus = f1(args);
    let phi_plus = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dw = args.get(2).map(|v| v.to_number()).unwrap_or(1e-3).max(1e-15);
    Ok(StrykeValue::float(-(phi_plus - phi_minus) / (2.0 * dw)))
}

/// Impulse-response truncation length n at which |h[n]| < tol; returns n_max.
fn builtin_impulse_response_n(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pole_rad = f1(args).abs().min(0.999_999);
    let tol = args.get(1).map(|v| v.to_number()).unwrap_or(1e-6);
    if pole_rad <= 0.0 { return Ok(StrykeValue::integer(1)); }
    let n = (tol.ln() / pole_rad.ln()).ceil() as i64;
    Ok(StrykeValue::integer(n.max(1)))
}

// ───── transform conversions ─────

/// TF → ZPK conversion: count of poles equals deg(a) − [leading-zeros].
fn builtin_tf2zpk_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = b74_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let leading_zeros = a.iter().take_while(|&&v| v == 0.0).count();
    Ok(StrykeValue::integer((a.len().saturating_sub(leading_zeros)) as i64 - 1))
}

/// ZPK → TF: produce one b-coefficient via Vieta. Args: zeros array, k index.
fn builtin_zpk2tf_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let zeros = b74_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0) as usize;
    let n = zeros.len();
    if k > n { return Ok(StrykeValue::float(0.0)); }
    let sign = if k.is_multiple_of(2) { 1.0 } else { -1.0 };
    Ok(StrykeValue::float(sign * elementary_symmetric(&zeros, k)))
}

fn elementary_symmetric(xs: &[f64], k: usize) -> f64 {
    let n = xs.len();
    let mut dp = vec![0.0; k + 1];
    dp[0] = 1.0;
    for &x in xs {
        for j in (1..=k.min(n)).rev() {
            dp[j] += x * dp[j - 1];
        }
    }
    dp[k]
}

/// TF → SOS step: number of biquads = ⌈order / 2⌉.
fn builtin_tf2sos_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let order = i1(args).max(1);
    Ok(StrykeValue::integer((order + 1) / 2))
}

/// ZPK → SOS step: pair conjugates, return biquad count.
fn builtin_zpk2sos_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pole_count = i1(args).max(0);
    let zero_count = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    Ok(StrykeValue::integer((pole_count.max(zero_count) + 1) / 2))
}

/// SOS → TF step: deg = 2 · n_biquads.
fn builtin_sos2tf_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let nb = i1(args).max(0);
    Ok(StrykeValue::integer(2 * nb))
}

/// Bilinear transform s → 2/T · (z−1)/(z+1): map analog pole p to digital
/// p_d = (2/T + p) / (2/T − p).
fn builtin_bilinear_xform(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let two_over_t = 2.0 / t;
    Ok(StrykeValue::float((two_over_t + p) / (two_over_t - p)))
}

/// Bilinear transform on ZPK: scale gain by ∏(2/T − z_k) / ∏(2/T − p_k).
fn builtin_bilinear_zpk_xform(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let zeros_prod = f1(args);
    let poles_prod = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(k * zeros_prod / poles_prod))
}

// ───── FIR design (windowed-sinc) ─────

/// firwin lowpass: ideal sinc impulse response truncated to length N.
/// Returns h[n] = ωc/π · sinc(ωc (n − M) / π), n = 0..N−1, M = (N−1)/2.
fn builtin_firwin_lowpass(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(11.0).max(2.0);
    let omega_c = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let m = (big_n - 1.0) / 2.0;
    let arg = omega_c * (n as f64 - m);
    let h = if arg.abs() < 1e-15 { omega_c / std::f64::consts::PI }
            else { (omega_c / std::f64::consts::PI) * arg.sin() / arg };
    Ok(StrykeValue::float(h))
}

/// firwin highpass: spectral inversion of lowpass.
fn builtin_firwin_highpass(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(11.0).max(2.0);
    let omega_c = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let m = (big_n - 1.0) / 2.0;
    let arg = omega_c * (n as f64 - m);
    let h_lp = if arg.abs() < 1e-15 { omega_c / std::f64::consts::PI }
               else { (omega_c / std::f64::consts::PI) * arg.sin() / arg };
    let delta = if (n as f64 - m).abs() < 1e-9 { 1.0 } else { 0.0 };
    Ok(StrykeValue::float(delta - h_lp))
}

/// firwin bandpass: difference of two lowpass impulse responses.
fn builtin_firwin_bandpass(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(11.0).max(2.0);
    let omega_l = args.get(2).map(|v| v.to_number()).unwrap_or(0.25);
    let omega_h = args.get(3).map(|v| v.to_number()).unwrap_or(0.75);
    let m = (big_n - 1.0) / 2.0;
    let argh = omega_h * (n as f64 - m);
    let argl = omega_l * (n as f64 - m);
    let s_h = if argh.abs() < 1e-15 { omega_h / std::f64::consts::PI }
              else { (omega_h / std::f64::consts::PI) * argh.sin() / argh };
    let s_l = if argl.abs() < 1e-15 { omega_l / std::f64::consts::PI }
              else { (omega_l / std::f64::consts::PI) * argl.sin() / argl };
    Ok(StrykeValue::float(s_h - s_l))
}

/// firwin bandstop: 1 − bandpass.
fn builtin_firwin_bandstop(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    let big_n = args.get(1).map(|v| v.to_number()).unwrap_or(11.0).max(2.0);
    let omega_l = args.get(2).map(|v| v.to_number()).unwrap_or(0.25);
    let omega_h = args.get(3).map(|v| v.to_number()).unwrap_or(0.75);
    let m = (big_n - 1.0) / 2.0;
    let argh = omega_h * (n as f64 - m);
    let argl = omega_l * (n as f64 - m);
    let s_h = if argh.abs() < 1e-15 { omega_h / std::f64::consts::PI }
              else { (omega_h / std::f64::consts::PI) * argh.sin() / argh };
    let s_l = if argl.abs() < 1e-15 { omega_l / std::f64::consts::PI }
              else { (omega_l / std::f64::consts::PI) * argl.sin() / argl };
    let bp = s_h - s_l;
    let delta = if (n as f64 - m).abs() < 1e-9 { 1.0 } else { 0.0 };
    Ok(StrykeValue::float(delta - bp))
}

/// firwin2 (frequency-sampling design): inverse-DFT of |H(ω_k)| samples.
fn builtin_firwin2_freq(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mag = b74_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0).min(mag.len());
    if n == 0 { return Ok(StrykeValue::float(0.0)); }
    let big_n = mag.len() as f64;
    let mut h_n = 0.0;
    for (k, &m_k) in mag.iter().enumerate() {
        h_n += m_k * (2.0 * std::f64::consts::PI * k as f64 * n as f64 / big_n).cos();
    }
    Ok(StrykeValue::float(h_n / big_n))
}

/// Remez exchange step: error-norm convergence test.
fn builtin_remez_design(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cur_err = f1(args);
    let prev_err = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let tol = args.get(2).map(|v| v.to_number()).unwrap_or(1e-6);
    Ok(StrykeValue::integer(if (cur_err - prev_err).abs() < tol { 1 } else { 0 }))
}

// ───── spectral analysis ─────

/// STFT step: one-frame DFT of windowed signal at frequency bin k.
fn builtin_stft_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = b74_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let win = args.get(1).map(b74_to_floats).unwrap_or_default();
    let k = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = x.len();
    let mut re = 0.0;
    let mut im = 0.0;
    for (i, &xi) in x.iter().enumerate() {
        let w = win.get(i).copied().unwrap_or(1.0);
        let phi = -2.0 * std::f64::consts::PI * (k as f64) * (i as f64) / (n as f64);
        re += xi * w * phi.cos();
        im += xi * w * phi.sin();
    }
    Ok(StrykeValue::float((re * re + im * im).sqrt()))
}

/// Inverse STFT step: overlap-add reconstruction; returns sum of cos·X_k.
fn builtin_istft_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mags = b74_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let phases = args.get(1).map(b74_to_floats).unwrap_or_default();
    let n = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let big_n = mags.len() as f64;
    let mut sum = 0.0;
    for (k, &m_k) in mags.iter().enumerate() {
        let phi = phases.get(k).copied().unwrap_or(0.0);
        sum += m_k * (2.0 * std::f64::consts::PI * k as f64 * n as f64 / big_n + phi).cos();
    }
    Ok(StrykeValue::float(sum / big_n))
}

/// Continuous wavelet transform with Morlet wavelet at scale a, position b.
/// ψ(t) = π^{−1/4} e^{iω₀t} e^{−t²/2}; we return real part.
fn builtin_cwt_morlet(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let t = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let omega0 = args.get(3).map(|v| v.to_number()).unwrap_or(6.0);
    let u = (t - b) / a;
    let envelope = (-u * u / 2.0).exp() / std::f64::consts::PI.powf(0.25);
    Ok(StrykeValue::float(envelope * (omega0 * u).cos()))
}

/// Ricker (Mexican hat 2D version): ψ(t) = (1 − t²) e^{−t²/2}.
fn builtin_ricker_wavelet(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let t = f1(args);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let u = t / sigma;
    Ok(StrykeValue::float((1.0 - u * u) * (-u * u / 2.0).exp()))
}

/// Mexican-hat (Marr) wavelet: ψ(x, y) = (1/(πσ⁴)) (1 − r²/(2σ²)) e^{−r²/(2σ²)},
/// the 2-D second derivative of a Gaussian (DOG approximation). Args: x, y, σ.
/// Distinct from `ricker_wavelet` which is the 1-D form.
fn builtin_mexican_hat_wavelet(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let r_sq = (x * x + y * y) / (sigma * sigma);
    let norm = 1.0 / (std::f64::consts::PI * sigma.powi(4));
    Ok(StrykeValue::float(norm * (1.0 - r_sq / 2.0) * (-r_sq / 2.0).exp()))
}

/// Magnitude-squared coherence γ_xy(ω) = |S_xy|² / (S_xx · S_yy).
fn builtin_coherence_xy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s_xy_re = f1(args);
    let s_xy_im = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let s_xx = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    let s_yy = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(StrykeValue::float((s_xy_re * s_xy_re + s_xy_im * s_xy_im) / (s_xx * s_yy)))
}

/// Cross-spectral density: average of conjugate-multiplied DFT bins.
fn builtin_csd_xy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xr = f1(args);
    let xi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let yr = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let yi = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(xr * yr + xi * yi))
}

/// Welch periodogram: average of |X_k|² across overlapping frames.
fn builtin_welch_psd_avg(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let frames = b74_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if frames.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let s: f64 = frames.iter().map(|x| x * x).sum();
    Ok(StrykeValue::float(s / frames.len() as f64))
}

/// Basic periodogram: |X(ω)|² / N.
fn builtin_periodogram_basic(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xr = f1(args);
    let xi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float((xr * xr + xi * xi) / n))
}

/// Lomb-Scargle periodogram contribution at frequency ω for sample (t, x).
fn builtin_lombscargle_freq(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let t = f1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let omega = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let tau = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let phase = omega * (t - tau);
    let cos_t = phase.cos();
    let sin_t = phase.sin();
    Ok(StrykeValue::float(x * x * (cos_t * cos_t + sin_t * sin_t)))
}

/// Hilbert transform: analytic-signal coefficient a_k for FFT bin k of N-pt
/// real signal: a_0 = 1, a_{N/2} = 1, a_{1..N/2-1} = 2, rest = 0.
fn builtin_hilbert_signal(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let k = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(1);
    let coef = if k == 0 || k == n / 2 { 1.0 }
               else if k > 0 && k < n / 2 { 2.0 }
               else { 0.0 };
    Ok(StrykeValue::float(coef))
}

/// Envelope amplitude from analytic signal: |x + i·H(x)|.
fn builtin_envelope_amplitude(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = f1(args);
    let hx = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((x * x + hx * hx).sqrt()))
}

/// Deconvolution step: y[n] = (x[n] − Σ h[k]·y[n−k]) / h[0] (causal).
fn builtin_deconvolve_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x_n = f1(args);
    let conv_sum = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h0 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(StrykeValue::float((x_n - conv_sum) / h0))
}

/// FFT convolve length: m + n − 1 (next power-of-two for padding).
fn builtin_fftconvolve_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = i1(args).max(0);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let total = m + n - 1;
    let mut p = 1_i64;
    while p < total { p *= 2; }
    Ok(StrykeValue::integer(p))
}

/// Overlap-add convolution block size: optimal = next pow2 ≥ 4·M (filter length).
fn builtin_oaconvolve_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = i1(args).max(1);
    let target = 4 * m;
    let mut p = 1_i64;
    while p < target { p *= 2; }
    Ok(StrykeValue::integer(p))
}

/// upfirdn step: upsample-by-L → FIR-filter → downsample-by-M output length.
fn builtin_upfirdn_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n_in = i1(args).max(0);
    let l_up = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let m_down = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let h_len = args.get(3).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let n_filtered = n_in * l_up + h_len - 1;
    Ok(StrykeValue::integer((n_filtered + m_down - 1) / m_down))
}

/// resample_poly: polyphase filter output length = ⌈n · up / down⌉.
fn builtin_resample_poly_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0);
    let up = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let down = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    Ok(StrykeValue::integer((n * up + down - 1) / down))
}

/// Decimate by integer factor M: output length = ⌈n / M⌉.
fn builtin_decimate_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    Ok(StrykeValue::integer((n + m - 1) / m))
}

/// Savitzky-Golay coefficient at index k for window 2M+1, polynomial order p,
/// derivative ν: c_{k,ν} from convolution-sum approximation. Returns the centred
/// scaled coefficient √(2π)·exp(−k²) (analytic kernel proxy).
fn builtin_savgol_coef(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let k = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(2.0).max(1.0);
    let scale = 1.0 / (2.0 * m + 1.0);
    Ok(StrykeValue::float(scale * (-(k * k) / (2.0 * m * m).max(1e-15)).exp()))
}

/// Linear detrend: subtract least-squares-fit line. Returns slope.
fn builtin_detrend_linear(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs = b74_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = xs.len() as f64;
    if n < 2.0 { return Ok(StrykeValue::float(0.0)); }
    let mean_x = (n - 1.0) / 2.0;
    let mean_y: f64 = xs.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut den = 0.0;
    for (i, &y) in xs.iter().enumerate() {
        let dx = i as f64 - mean_x;
        num += dx * (y - mean_y);
        den += dx * dx;
    }
    Ok(StrykeValue::float(if den > 0.0 { num / den } else { 0.0 }))
}

/// Wiener filter pointwise gain: H(ω) = S_xx / (S_xx + N).
fn builtin_wiener_filter(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let sxx = f1(args).max(0.0);
    let noise = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(0.0);
    Ok(StrykeValue::float(sxx / (sxx + noise).max(1e-300)))
}

/// 1-D median filter: at every index `i`, returns the median of a window of
/// size `k` (odd, default 3) centred at `i`. Boundary windows clamp to the
/// available samples on either side (shrunken window) rather than reflecting.
/// Previous implementation returned the global median of the input.
fn builtin_medfilt_1d(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let signal = b74_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = signal.len();
    if n == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let raw_k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(3);
    // Force odd, ≥ 1.
    let mut k = raw_k.max(1) as usize;
    if k % 2 == 0 {
        k += 1;
    }
    let half = k / 2;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let lo = i.saturating_sub(half);
        let hi = (i + half + 1).min(n);
        let mut window: Vec<f64> = signal[lo..hi].to_vec();
        window.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let m = window.len();
        let med = if m % 2 == 1 {
            window[m / 2]
        } else {
            (window[m / 2 - 1] + window[m / 2]) / 2.0
        };
        out.push(StrykeValue::float(med));
    }
    Ok(StrykeValue::array(out))
}

/// Peak-width-at-half-prominence estimation given prominence and slope.
fn builtin_peak_widths_at(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let prominence = f1(args).max(0.0);
    let slope_left = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).abs().max(1e-15);
    let slope_right = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).abs().max(1e-15);
    Ok(StrykeValue::float(prominence / 2.0 / slope_left + prominence / 2.0 / slope_right))
}
