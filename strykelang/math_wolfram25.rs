// Batch 25 — signal processing deep: windows, IIR/FIR designs, biquads, transforms.

// Hamming window
fn builtin_hamming_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(1) as usize;
    let pi = std::f64::consts::PI;
    let out: Vec<PerlValue> = (0..n).map(|i| {
        PerlValue::float(0.54 - 0.46 * (2.0 * pi * i as f64 / (n - 1).max(1) as f64).cos())
    }).collect();
    Ok(PerlValue::array(out))
}
// Hann window
fn builtin_hann_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(1) as usize;
    let pi = std::f64::consts::PI;
    let out: Vec<PerlValue> = (0..n).map(|i| {
        PerlValue::float(0.5 - 0.5 * (2.0 * pi * i as f64 / (n - 1).max(1) as f64).cos())
    }).collect();
    Ok(PerlValue::array(out))
}
// Blackman window
fn builtin_blackman_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(1) as usize;
    let pi = std::f64::consts::PI;
    let out: Vec<PerlValue> = (0..n).map(|i| {
        let x = 2.0 * pi * i as f64 / (n - 1).max(1) as f64;
        PerlValue::float(0.42 - 0.5 * x.cos() + 0.08 * (2.0 * x).cos())
    }).collect();
    Ok(PerlValue::array(out))
}
// Blackman-Harris window
fn builtin_blackman_harris_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(1) as usize;
    let pi = std::f64::consts::PI;
    let a0 = 0.35875; let a1 = 0.48829; let a2 = 0.14128; let a3 = 0.01168;
    let out: Vec<PerlValue> = (0..n).map(|i| {
        let x = 2.0 * pi * i as f64 / (n - 1).max(1) as f64;
        PerlValue::float(a0 - a1 * x.cos() + a2 * (2.0 * x).cos() - a3 * (3.0 * x).cos())
    }).collect();
    Ok(PerlValue::array(out))
}
// Bartlett (triangular) window
fn builtin_bartlett_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(1) as usize;
    let nm1 = (n as f64 - 1.0).max(1.0);
    let out: Vec<PerlValue> = (0..n).map(|i| {
        PerlValue::float(1.0 - ((i as f64 - nm1 / 2.0).abs()) / (nm1 / 2.0))
    }).collect();
    Ok(PerlValue::array(out))
}
// Welch window
fn builtin_welch_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(1) as usize;
    let nm1 = (n as f64 - 1.0).max(1.0);
    let out: Vec<PerlValue> = (0..n).map(|i| {
        let arg = (i as f64 - nm1 / 2.0) / (nm1 / 2.0);
        PerlValue::float(1.0 - arg * arg)
    }).collect();
    Ok(PerlValue::array(out))
}
// Kaiser window (β param)
fn builtin_kaiser_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(64).max(1);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(8.6);
    fn i0(x: f64) -> f64 {
        let mut sum = 1.0;
        let mut term = 1.0;
        for k in 1..50 {
            term *= (x / 2.0).powi(2) / (k as f64 * k as f64);
            sum += term;
            if term.abs() < 1e-15 { break; }
        }
        sum
    }
    let denom = i0(beta);
    let nm1 = (n as f64 - 1.0).max(1.0);
    let out: Vec<PerlValue> = (0..n).map(|i| {
        let r = 2.0 * i as f64 / nm1 - 1.0;
        PerlValue::float(i0(beta * (1.0 - r * r).max(0.0).sqrt()) / denom)
    }).collect();
    Ok(PerlValue::array(out))
}
// Tukey (cosine-tapered) window with α
fn builtin_tukey_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(64).max(1);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    let pi = std::f64::consts::PI;
    let nm1 = (n as f64 - 1.0).max(1.0);
    let out: Vec<PerlValue> = (0..n).map(|i| {
        let x = i as f64 / nm1;
        if x < alpha / 2.0 {
            PerlValue::float(0.5 * (1.0 + (2.0 * pi * x / alpha - pi).cos()))
        } else if x <= 1.0 - alpha / 2.0 {
            PerlValue::float(1.0)
        } else {
            PerlValue::float(0.5 * (1.0 + (2.0 * pi * x / alpha - 2.0 * pi / alpha + pi).cos()))
        }
    }).collect();
    Ok(PerlValue::array(out))
}
// Gaussian window
fn builtin_gaussian_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(64).max(1);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(0.4);
    let nm1_2 = (n as f64 - 1.0) / 2.0;
    let out: Vec<PerlValue> = (0..n).map(|i| {
        let x = (i as f64 - nm1_2) / (sigma * nm1_2);
        PerlValue::float((-0.5 * x * x).exp())
    }).collect();
    Ok(PerlValue::array(out))
}

// Hilbert transform via DFT (returns analytic signal magnitude — simplified)
fn builtin_hilbert_envelope(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len();
    let mut x_re = xs.clone();
    let mut x_im = vec![0.0; n];
    let pi = std::f64::consts::PI;
    let mut re_f = vec![0.0; n];
    let mut im_f = vec![0.0; n];
    for k in 0..n {
        for t in 0..n {
            let theta = -2.0 * pi * k as f64 * t as f64 / n as f64;
            re_f[k] += x_re[t] * theta.cos();
            im_f[k] += x_re[t] * theta.sin();
        }
    }
    for k in 0..n {
        let mult = if k == 0 || (n % 2 == 0 && k == n / 2) { 1.0 }
                   else if k < n / 2 { 2.0 } else { 0.0 };
        re_f[k] *= mult;
        im_f[k] *= mult;
    }
    for t in 0..n {
        let mut sr = 0.0; let mut si = 0.0;
        for k in 0..n {
            let theta = 2.0 * pi * k as f64 * t as f64 / n as f64;
            sr += re_f[k] * theta.cos() - im_f[k] * theta.sin();
            si += re_f[k] * theta.sin() + im_f[k] * theta.cos();
        }
        x_re[t] = sr / n as f64;
        x_im[t] = si / n as f64;
    }
    let env: Vec<PerlValue> = (0..n).map(|i| PerlValue::float((x_re[i].powi(2) + x_im[i].powi(2)).sqrt())).collect();
    Ok(PerlValue::array(env))
}

// Goertzel single-frequency power
fn builtin_goertzel_b25(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let target_freq = args.get(1).map(|v| v.to_number()).unwrap_or(440.0);
    let sample_rate = args.get(2).map(|v| v.to_number()).unwrap_or(44100.0);
    let n = xs.len();
    let k = (0.5 + (n as f64 * target_freq) / sample_rate).floor();
    let omega = 2.0 * std::f64::consts::PI * k / n as f64;
    let cos_w = omega.cos();
    let coeff = 2.0 * cos_w;
    let mut q1 = 0.0_f64;
    let mut q2 = 0.0_f64;
    for &x in &xs {
        let q0 = coeff * q1 - q2 + x;
        q2 = q1;
        q1 = q0;
    }
    Ok(PerlValue::float(q1 * q1 + q2 * q2 - q1 * q2 * coeff))
}

// Biquad filter: process one sample, given state
// Returns [y, x1, x2, y1, y2]
fn builtin_biquad_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
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
    let y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
    Ok(PerlValue::array(vec![
        PerlValue::float(y), PerlValue::float(x), PerlValue::float(x1),
        PerlValue::float(y), PerlValue::float(y1),
    ]))
}

// Lowpass biquad design (RBJ cookbook)
fn builtin_biquad_lowpass_coeffs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let fc = f1(args);
    let fs = args.get(1).map(|v| v.to_number()).unwrap_or(44100.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(0.707);
    let omega = 2.0 * std::f64::consts::PI * fc / fs;
    let cos_w = omega.cos();
    let alpha = omega.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    let b0 = (1.0 - cos_w) / 2.0 / a0;
    let b1 = (1.0 - cos_w) / a0;
    let b2 = (1.0 - cos_w) / 2.0 / a0;
    let a1 = -2.0 * cos_w / a0;
    let a2 = (1.0 - alpha) / a0;
    Ok(PerlValue::array(vec![
        PerlValue::float(b0), PerlValue::float(b1), PerlValue::float(b2),
        PerlValue::float(a1), PerlValue::float(a2),
    ]))
}

// Highpass biquad design
fn builtin_biquad_highpass_coeffs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let fc = f1(args);
    let fs = args.get(1).map(|v| v.to_number()).unwrap_or(44100.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(0.707);
    let omega = 2.0 * std::f64::consts::PI * fc / fs;
    let cos_w = omega.cos();
    let alpha = omega.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    let b0 = (1.0 + cos_w) / 2.0 / a0;
    let b1 = -(1.0 + cos_w) / a0;
    let b2 = (1.0 + cos_w) / 2.0 / a0;
    let a1 = -2.0 * cos_w / a0;
    let a2 = (1.0 - alpha) / a0;
    Ok(PerlValue::array(vec![
        PerlValue::float(b0), PerlValue::float(b1), PerlValue::float(b2),
        PerlValue::float(a1), PerlValue::float(a2),
    ]))
}

// Bandpass biquad
fn builtin_biquad_bandpass_coeffs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let fc = f1(args);
    let fs = args.get(1).map(|v| v.to_number()).unwrap_or(44100.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(0.707);
    let omega = 2.0 * std::f64::consts::PI * fc / fs;
    let cos_w = omega.cos();
    let alpha = omega.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    let b0 = alpha / a0;
    let b1 = 0.0;
    let b2 = -alpha / a0;
    let a1 = -2.0 * cos_w / a0;
    let a2 = (1.0 - alpha) / a0;
    Ok(PerlValue::array(vec![
        PerlValue::float(b0), PerlValue::float(b1), PerlValue::float(b2),
        PerlValue::float(a1), PerlValue::float(a2),
    ]))
}

// Notch biquad
fn builtin_biquad_notch_coeffs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let fc = f1(args);
    let fs = args.get(1).map(|v| v.to_number()).unwrap_or(44100.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(10.0);
    let omega = 2.0 * std::f64::consts::PI * fc / fs;
    let cos_w = omega.cos();
    let alpha = omega.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    let b0 = 1.0 / a0;
    let b1 = -2.0 * cos_w / a0;
    let b2 = 1.0 / a0;
    let a1 = -2.0 * cos_w / a0;
    let a2 = (1.0 - alpha) / a0;
    Ok(PerlValue::array(vec![
        PerlValue::float(b0), PerlValue::float(b1), PerlValue::float(b2),
        PerlValue::float(a1), PerlValue::float(a2),
    ]))
}

// All-pass biquad
fn builtin_biquad_allpass_coeffs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let fc = f1(args);
    let fs = args.get(1).map(|v| v.to_number()).unwrap_or(44100.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(0.707);
    let omega = 2.0 * std::f64::consts::PI * fc / fs;
    let cos_w = omega.cos();
    let alpha = omega.sin() / (2.0 * q);
    let a0 = 1.0 + alpha;
    let b0 = (1.0 - alpha) / a0;
    let b1 = -2.0 * cos_w / a0;
    let b2 = (1.0 + alpha) / a0;
    let a1 = -2.0 * cos_w / a0;
    let a2 = (1.0 - alpha) / a0;
    Ok(PerlValue::array(vec![
        PerlValue::float(b0), PerlValue::float(b1), PerlValue::float(b2),
        PerlValue::float(a1), PerlValue::float(a2),
    ]))
}

// Peak (parametric EQ) biquad
fn builtin_biquad_peak_coeffs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let fc = f1(args);
    let fs = args.get(1).map(|v| v.to_number()).unwrap_or(44100.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let gain_db = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let a = 10_f64.powf(gain_db / 40.0);
    let omega = 2.0 * std::f64::consts::PI * fc / fs;
    let cos_w = omega.cos();
    let alpha = omega.sin() / (2.0 * q);
    let a0 = 1.0 + alpha / a;
    let b0 = (1.0 + alpha * a) / a0;
    let b1 = (-2.0 * cos_w) / a0;
    let b2 = (1.0 - alpha * a) / a0;
    let a1 = (-2.0 * cos_w) / a0;
    let a2 = (1.0 - alpha / a) / a0;
    Ok(PerlValue::array(vec![
        PerlValue::float(b0), PerlValue::float(b1), PerlValue::float(b2),
        PerlValue::float(a1), PerlValue::float(a2),
    ]))
}

// Low shelf
fn builtin_biquad_lowshelf_coeffs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let fc = f1(args);
    let fs = args.get(1).map(|v| v.to_number()).unwrap_or(44100.0);
    let s = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let gain_db = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let a = 10_f64.powf(gain_db / 40.0);
    let omega = 2.0 * std::f64::consts::PI * fc / fs;
    let cos_w = omega.cos();
    let sin_w = omega.sin();
    let alpha = sin_w / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
    let beta_2 = 2.0 * a.sqrt() * alpha;
    let a0 = (a + 1.0) + (a - 1.0) * cos_w + beta_2;
    let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w + beta_2) / a0;
    let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w) / a0;
    let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w - beta_2) / a0;
    let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w) / a0;
    let a2 = ((a + 1.0) + (a - 1.0) * cos_w - beta_2) / a0;
    Ok(PerlValue::array(vec![
        PerlValue::float(b0), PerlValue::float(b1), PerlValue::float(b2),
        PerlValue::float(a1), PerlValue::float(a2),
    ]))
}

// High shelf
fn builtin_biquad_highshelf_coeffs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let fc = f1(args);
    let fs = args.get(1).map(|v| v.to_number()).unwrap_or(44100.0);
    let s = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let gain_db = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let a = 10_f64.powf(gain_db / 40.0);
    let omega = 2.0 * std::f64::consts::PI * fc / fs;
    let cos_w = omega.cos();
    let sin_w = omega.sin();
    let alpha = sin_w / 2.0 * ((a + 1.0 / a) * (1.0 / s - 1.0) + 2.0).sqrt();
    let beta_2 = 2.0 * a.sqrt() * alpha;
    let a0 = (a + 1.0) - (a - 1.0) * cos_w + beta_2;
    let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w + beta_2) / a0;
    let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w) / a0;
    let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w - beta_2) / a0;
    let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w) / a0;
    let a2 = ((a + 1.0) - (a - 1.0) * cos_w - beta_2) / a0;
    Ok(PerlValue::array(vec![
        PerlValue::float(b0), PerlValue::float(b1), PerlValue::float(b2),
        PerlValue::float(a1), PerlValue::float(a2),
    ]))
}

// Butterworth lowpass IIR cutoff prewarp (just compute warped freq)
fn builtin_butterworth_prewarp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let fc = f1(args);
    let fs = args.get(1).map(|v| v.to_number()).unwrap_or(44100.0);
    let omega = 2.0 * std::f64::consts::PI * fc / fs;
    Ok(PerlValue::float(2.0 * fs * (omega / 2.0).tan()))
}

// Butterworth filter order from spec: ceil(log10((10^(αs/10)-1)/(10^(αp/10)-1)) / (2 log10(ωs/ωp)))
fn builtin_butterworth_order(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha_p = f1(args);
    let alpha_s = args.get(1).map(|v| v.to_number()).unwrap_or(40.0);
    let omega_p = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let omega_s = args.get(3).map(|v| v.to_number()).unwrap_or(2.0);
    if omega_s <= omega_p { return Ok(PerlValue::integer(0)); }
    let num = (10_f64.powf(alpha_s / 10.0) - 1.0) / (10_f64.powf(alpha_p / 10.0) - 1.0);
    let den = 2.0 * (omega_s / omega_p).log10();
    if den == 0.0 { return Ok(PerlValue::integer(0)); }
    Ok(PerlValue::integer(num.log10().div_euclid(den).max(0.0).ceil() as i64))
}

// FIR moving average (length L)
fn builtin_fir_moving_average(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let l = args.get(1).map(|v| v.to_number() as usize).unwrap_or(3).max(1);
    let n = xs.len();
    if n == 0 { return Ok(PerlValue::array(vec![])); }
    let mut out = vec![0.0; n];
    let mut sum = 0.0;
    for i in 0..n {
        sum += xs[i];
        if i >= l { sum -= xs[i - l]; }
        let div = (i + 1).min(l) as f64;
        out[i] = sum / div;
    }
    Ok(PerlValue::array(out.into_iter().map(PerlValue::float).collect()))
}

// FIR low-pass via windowed-sinc
fn builtin_fir_lowpass_design(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(31).max(1);
    let fc = args.get(1).map(|v| v.to_number()).unwrap_or(0.25);
    let pi = std::f64::consts::PI;
    let mid = (n as f64 - 1.0) / 2.0;
    let out: Vec<PerlValue> = (0..n).map(|i| {
        let t = i as f64 - mid;
        let h = if t == 0.0 { 2.0 * fc } else { (2.0 * pi * fc * t).sin() / (pi * t) };
        let w = 0.54 - 0.46 * (2.0 * pi * i as f64 / (n - 1).max(1) as f64).cos();
        PerlValue::float(h * w)
    }).collect();
    Ok(PerlValue::array(out))
}

// Convolve 1D
fn builtin_convolve_b25(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let h: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len();
    let m = h.len();
    let mut out = vec![0.0; n + m - 1];
    for i in 0..n {
        for j in 0..m {
            out[i + j] += xs[i] * h[j];
        }
    }
    Ok(PerlValue::array(out.into_iter().map(PerlValue::float).collect()))
}

// Crosscorrelation
fn builtin_xcorr_b25(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len();
    let m = ys.len();
    let len = n + m - 1;
    let mut out = vec![0.0; len];
    for k in 0..len {
        let lag = k as isize - (m - 1) as isize;
        let mut s = 0.0;
        for i in 0..n {
            let j = i as isize - lag;
            if j >= 0 && (j as usize) < m { s += xs[i] * ys[j as usize]; }
        }
        out[k] = s;
    }
    Ok(PerlValue::array(out.into_iter().map(PerlValue::float).collect()))
}

// Power spectral density via periodogram (DFT-based)
fn builtin_periodogram_b25(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len();
    let pi = std::f64::consts::PI;
    let mut psd = vec![0.0; n / 2 + 1];
    for k in 0..=n/2 {
        let mut re = 0.0; let mut im = 0.0;
        for t in 0..n {
            let theta = -2.0 * pi * k as f64 * t as f64 / n as f64;
            re += xs[t] * theta.cos();
            im += xs[t] * theta.sin();
        }
        psd[k] = (re * re + im * im) / n as f64;
    }
    Ok(PerlValue::array(psd.into_iter().map(PerlValue::float).collect()))
}

// Spectrogram (windowed periodogram)
fn builtin_spectrogram_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let win = args.get(1).map(|v| v.to_number() as usize).unwrap_or(64).max(1);
    let hop = args.get(2).map(|v| v.to_number() as usize).unwrap_or(win / 2).max(1);
    let n = xs.len();
    if n < win { return Ok(PerlValue::array(vec![])); }
    let pi = std::f64::consts::PI;
    let frames = (n - win) / hop + 1;
    let mut out: Vec<PerlValue> = Vec::with_capacity(frames);
    for f in 0..frames {
        let start = f * hop;
        let mut psd = vec![0.0; win / 2 + 1];
        for k in 0..=win/2 {
            let mut re = 0.0; let mut im = 0.0;
            for t in 0..win {
                let w = 0.5 - 0.5 * (2.0 * pi * t as f64 / (win - 1).max(1) as f64).cos();
                let v = xs[start + t] * w;
                let theta = -2.0 * pi * k as f64 * t as f64 / win as f64;
                re += v * theta.cos();
                im += v * theta.sin();
            }
            psd[k] = (re * re + im * im) / win as f64;
        }
        out.push(PerlValue::array(psd.into_iter().map(PerlValue::float).collect()));
    }
    Ok(PerlValue::array(out))
}

// Zero padding
fn builtin_zero_pad(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<PerlValue> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let target = args.get(1).map(|v| v.to_number() as usize).unwrap_or(xs.len());
    let mut out = xs.clone();
    while out.len() < target { out.push(PerlValue::float(0.0)); }
    Ok(PerlValue::array(out))
}

// Resample (nearest neighbor, ratio)
fn builtin_resample_nearest(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let ratio = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if ratio <= 0.0 || xs.is_empty() { return Ok(PerlValue::array(vec![])); }
    let new_len = ((xs.len() as f64) * ratio) as usize;
    let out: Vec<PerlValue> = (0..new_len).map(|i| {
        let src = ((i as f64) / ratio).round() as usize;
        PerlValue::float(*xs.get(src.min(xs.len() - 1)).unwrap_or(&0.0))
    }).collect();
    Ok(PerlValue::array(out))
}

// Resample linear
fn builtin_resample_linear(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let ratio = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if ratio <= 0.0 || xs.is_empty() { return Ok(PerlValue::array(vec![])); }
    let new_len = ((xs.len() as f64) * ratio) as usize;
    let out: Vec<PerlValue> = (0..new_len).map(|i| {
        let src = (i as f64) / ratio;
        let i0 = src.floor() as usize;
        let i1 = (i0 + 1).min(xs.len() - 1);
        let frac = src - i0 as f64;
        PerlValue::float(xs[i0] * (1.0 - frac) + xs[i1] * frac)
    }).collect();
    Ok(PerlValue::array(out))
}

// Quantize to N bits
fn builtin_quantize(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let bits = args.get(1).map(|v| v.to_number() as i32).unwrap_or(16).clamp(1, 31);
    let levels = (1_i64 << bits) as f64;
    let max_v = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max).max(1e-12);
    let out: Vec<PerlValue> = xs.iter().map(|&v| {
        PerlValue::float(((v / max_v) * levels).round() / levels * max_v)
    }).collect();
    Ok(PerlValue::array(out))
}

// Mu-law encode
fn builtin_mu_law_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(255.0);
    let s = x.signum();
    let abs_x = x.abs().min(1.0);
    Ok(PerlValue::float(s * (1.0 + mu * abs_x).ln() / (1.0 + mu).ln()))
}

// Mu-law decode
fn builtin_mu_law_decode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(255.0);
    let s = y.signum();
    let abs_y = y.abs();
    Ok(PerlValue::float(s * ((1.0 + mu).powf(abs_y) - 1.0) / mu))
}

// A-law encode
fn builtin_a_law_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(87.6);
    let s = x.signum();
    let abs_x = x.abs().min(1.0);
    let y = if abs_x < 1.0 / a {
        a * abs_x / (1.0 + a.ln())
    } else {
        (1.0 + (a * abs_x).ln()) / (1.0 + a.ln())
    };
    Ok(PerlValue::float(s * y))
}

// A-law decode
fn builtin_a_law_decode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(87.6);
    let s = y.signum();
    let abs_y = y.abs();
    let x = if abs_y < 1.0 / (1.0 + a.ln()) {
        abs_y * (1.0 + a.ln()) / a
    } else {
        ((abs_y * (1.0 + a.ln()) - 1.0).exp()) / a
    };
    Ok(PerlValue::float(s * x))
}

// Chirp signal sample (linear)
fn builtin_chirp_linear(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let f0 = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let f1_ = args.get(2).map(|v| v.to_number()).unwrap_or(1000.0);
    let dur = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let phase = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let pi = std::f64::consts::PI;
    let k = (f1_ - f0) / dur;
    Ok(PerlValue::float((2.0 * pi * (f0 * t + 0.5 * k * t * t) + phase).sin()))
}
