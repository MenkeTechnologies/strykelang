// Batch 40 — information theory, coding, signal processing, RL, divergences.

fn b40_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

// Conditional entropy H(Y|X) = H(X,Y) - H(X)
fn builtin_conditional_entropy_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h_xy = f1(args);
    let h_x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(h_xy - h_x))
}

// Joint entropy H(X,Y) = -Σ p(x,y) log p(x,y)
fn builtin_joint_entropy_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let h: f64 = p.iter().filter(|&&x| x > 0.0).map(|&x| -x * x.log2()).sum();
    Ok(PerlValue::float(h))
}

// KL divergence D(P||Q) = Σ p log(p/q)
fn builtin_relative_entropy_kl(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let q = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = p.len().min(q.len());
    let kl: f64 = (0..n).filter(|&i| p[i] > 0.0 && q[i] > 0.0).map(|i| p[i] * (p[i] / q[i]).log2()).sum();
    Ok(PerlValue::float(kl))
}

// Mutual information I(X;Y) = H(X) + H(Y) - H(X,Y)
fn builtin_mutual_information_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h_x = f1(args);
    let h_y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h_xy = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(h_x + h_y - h_xy))
}

// Chain rule for entropy H(X,Y) = H(X) + H(Y|X)
fn builtin_chain_rule_entropy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h_x = f1(args);
    let h_yx = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(h_x + h_yx))
}

// Fano's inequality bound: H(P_e) + P_e log(|X| - 1) ≥ H(X|Y)
fn builtin_fano_inequality_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h_xy = f1(args);
    let card_x = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    if card_x <= 1.0 { return Ok(PerlValue::float(0.0)); }
    let lhs = (card_x - 1.0).log2();
    if lhs == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(h_xy / (1.0 + lhs)))
}

// Data processing inequality: I(X;Y) ≥ I(X;Z) when Markov
fn builtin_data_processing_inequality(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i_xy = f1(args);
    let i_xz = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::integer(if i_xy >= i_xz { 1 } else { 0 }))
}

// Arithmetic coding interval update for a single symbol
fn builtin_arithmetic_coding_interval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lo = f1(args);
    let hi = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let cum_lo = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let cum_hi = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let new_lo = lo + (hi - lo) * cum_lo;
    let new_hi = lo + (hi - lo) * cum_hi;
    Ok(PerlValue::float(new_hi - new_lo))
}

// Range coding step: same math as arithmetic coding, scaled to integer range
fn builtin_range_coding_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let range = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float(range * p))
}

// Golomb-Rice code length for value n with parameter k: ⌊n/2^k⌋ + k + 1
fn builtin_golomb_rice_code(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let q = if k >= 0 { n >> k } else { n };
    Ok(PerlValue::integer(q + k + 1))
}

// Elias gamma code length: 2⌊log2(n)⌋ + 1
fn builtin_elias_gamma_code(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if n <= 0 { return Ok(PerlValue::integer(1)); }
    Ok(PerlValue::integer(2 * (n as f64).log2().floor() as i64 + 1))
}

// Elias delta code length: ⌊log2(⌊log2(n)⌋ + 1)⌋·2 + 1 + ⌊log2(n)⌋
fn builtin_elias_delta_code(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if n <= 0 { return Ok(PerlValue::integer(1)); }
    let log_n = (n as f64).log2().floor() as i64;
    let log_log = ((log_n + 1) as f64).log2().floor() as i64;
    Ok(PerlValue::integer(2 * log_log + 1 + log_n))
}

// Exp-Golomb code length: 2⌊log2(n+1)⌋ + 1
fn builtin_exp_golomb_code(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer(2 * ((n + 1) as f64).log2().floor() as i64 + 1))
}

// Zeckendorf-Fibonacci code length (approximate via golden ratio)
fn builtin_fibonacci_code(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let phi = (1.0 + 5f64.sqrt()) / 2.0;
    if n <= 0.0 { return Ok(PerlValue::integer(1)); }
    Ok(PerlValue::integer(((n.ln() / phi.ln()).ceil() + 1.0) as i64))
}

// Shannon-Fano-Elias code length: ⌈-log2(p)⌉ + 1
fn builtin_shannon_fano_elias_code(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    if p <= 0.0 { return Ok(PerlValue::integer(0)); }
    Ok(PerlValue::integer(((-p.log2()).ceil()) as i64 + 1))
}

// Balanced Huffman tree step: total weight from merging two nodes
fn builtin_huffman_balanced_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w1 = f1(args);
    let w2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(w1 + w2))
}

// Decode interval for arithmetic decoding: locate target in cumulative table
fn builtin_arithmetic_decode_interval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let target = f1(args);
    let cum = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    for (i, &c) in cum.iter().enumerate() {
        if target < c { return Ok(PerlValue::integer(i as i64)); }
    }
    Ok(PerlValue::integer((cum.len() as i64).max(0) - 1))
}

// Range decode step: target = (target - low) / range  → next symbol fraction
fn builtin_range_decode_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let target = f1(args);
    let low = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let range = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if range == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((target - low) / range))
}

// Universal code length L(n) = log* n + log(log* n) + ...
fn builtin_universal_code_length(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let mut total = 1.0;
    let mut x = n;
    while x > 1.0 { total += x.log2(); x = x.log2(); }
    Ok(PerlValue::float(total))
}

// Lempel-Ziv complexity estimate
fn builtin_ziv_lempel_estimate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    if n <= 1.0 { return Ok(PerlValue::float(1.0)); }
    Ok(PerlValue::float(n / n.log2()))
}

// LZ77 best-match length
fn builtin_lz77_match_length(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let candidates = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(candidates.iter().cloned().fold(0.0_f64, f64::max)))
}

// LZ78 dictionary growth: |D_n| = |D_{n-1}| + 1 if new prefix
fn builtin_lz78_dictionary_growth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let unique = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(n + unique))
}

// LZW step: output code, add new entry
fn builtin_lzw_step_dict(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dict_size = i1(args);
    Ok(PerlValue::integer(dict_size + 1))
}

// PPM predict probability for symbol after context
fn builtin_ppm_predict_prob(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let count = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(count / total))
}

// Deflate Huffman literal table size
fn builtin_deflate_huffman_lit(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let _ = args;
    Ok(PerlValue::integer(288))
}

// Brotli distance code count
fn builtin_brotli_distance_code_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_postfix = i1(args);
    let n_direct = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(16 + n_direct + (48_i64 << n_postfix)))
}

// Zstandard window log size for level
fn builtin_zstd_window_size_log(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let level = i1(args);
    Ok(PerlValue::integer(15 + level.min(7).max(0)))
}

// MPEG quantization value: q · qmat[i,j]
fn builtin_mpeg_quant_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let coef = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(8.0);
    let q_mat = args.get(2).map(|v| v.to_number()).unwrap_or(16.0);
    if q == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((coef / (q * q_mat / 8.0)).round()))
}

// JPEG zig-zag scan index lookup (returns linear index for (i,j))
fn builtin_jpeg_zig_zag_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = i1(args).clamp(0, 7) as usize;
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).clamp(0, 7) as usize;
    let zig: [[i64; 8]; 8] = [
        [0, 1, 5, 6, 14, 15, 27, 28],
        [2, 4, 7, 13, 16, 26, 29, 42],
        [3, 8, 12, 17, 25, 30, 41, 43],
        [9, 11, 18, 24, 31, 40, 44, 53],
        [10, 19, 23, 32, 39, 45, 52, 54],
        [20, 22, 33, 38, 46, 51, 55, 60],
        [21, 34, 37, 47, 50, 56, 59, 61],
        [35, 36, 48, 49, 57, 58, 62, 63],
    ];
    Ok(PerlValue::integer(zig[i][j]))
}

// JPEG DCT 8x8 quantized value
fn builtin_jpeg_dct_8x8_quant(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let coef = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(16.0);
    if q == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((coef / q).round()))
}

// Hadamard-Walsh transform step (Hadamard ordering): hadamard(n)·signal vector
fn builtin_hadamard_walsh_transform_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let n = v.len();
    if !n.is_power_of_two() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(v.iter().sum::<f64>() / (n as f64).sqrt()))
}

// Karhunen-Loeve transform step: project on largest eigenvector
fn builtin_karhunen_loeve_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let phi = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let s: f64 = v.iter().zip(phi.iter()).map(|(a, b)| a * b).sum();
    Ok(PerlValue::float(s))
}

// Discrete Haar wavelet step: (a + b) / √2
fn builtin_discrete_haar_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((a + b) / std::f64::consts::SQRT_2))
}

// Daubechies-4 step (low-pass coefficients)
fn builtin_db4_wavelet_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let coef: [f64; 4] = [0.482962913, 0.836516304, 0.224143868, -0.129409523];
    let s: f64 = v.iter().take(4).zip(coef.iter()).map(|(x, c)| x * c).sum();
    Ok(PerlValue::float(s))
}

// Biorthogonal wavelet step (5/3 lifting)
fn builtin_biorthogonal_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(b - 0.5 * (a + c)))
}

// Beylkin wavelet step (length-18 Beylkin filter approximation)
fn builtin_beylkin_wavelet_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let h0 = 0.099305765374353;
    Ok(PerlValue::float(v.iter().map(|x| x * h0).sum()))
}

// Coiflet wavelet step (Coif1 length-6)
fn builtin_coiflet_wavelet_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let coef: [f64; 6] = [-0.072732619512854, 0.337897662457809, 0.852572020212255,
                          0.384864846864203, -0.072732619512854, -0.015655728135465];
    let s: f64 = v.iter().take(6).zip(coef.iter()).map(|(x, c)| x * c).sum();
    Ok(PerlValue::float(s))
}

// Mallat pyramid step: downsample by 2 after low-pass filter
fn builtin_mallat_pyramid_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let s: f64 = v.iter().step_by(2).sum();
    Ok(PerlValue::float(s / (v.len() as f64 / 2.0).max(1.0)))
}

// Soft thresholding: sign(x) max(|x| - λ, 0)
fn builtin_threshold_soft_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mag = (x.abs() - lambda).max(0.0);
    Ok(PerlValue::float(x.signum() * mag))
}

// Hard thresholding: x · 1[|x| > λ]
fn builtin_threshold_hard_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(if x.abs() > lambda { x } else { 0.0 }))
}

// Median filter window (sort window, pick middle)
fn builtin_median_filter_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut v = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if v.is_empty() { return Ok(PerlValue::float(0.0)); }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(PerlValue::float(v[v.len() / 2]))
}

// Mean filter window
fn builtin_mean_filter_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if v.is_empty() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(v.iter().sum::<f64>() / v.len() as f64))
}

// Gaussian filter window kernel value: exp(-(i² + j²) / (2σ²))
fn builtin_gaussian_filter_window(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = f1(args);
    let j = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if sigma == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((-(i * i + j * j) / (2.0 * sigma * sigma)).exp()))
}

// Unsharp mask: out = orig + λ (orig - blur)
fn builtin_unsharp_mask_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let orig = f1(args);
    let blur = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(orig + lambda * (orig - blur)))
}

// Sobel kernel value at (i, j) for x-direction
fn builtin_sobel_kernel_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let kx: [[i64; 3]; 3] = [[-1, 0, 1], [-2, 0, 2], [-1, 0, 1]];
    if (0..3).contains(&i) && (0..3).contains(&j) {
        return Ok(PerlValue::integer(kx[i as usize][j as usize]));
    }
    Ok(PerlValue::integer(0))
}

// Prewitt kernel value
fn builtin_prewitt_kernel_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let kx: [[i64; 3]; 3] = [[-1, 0, 1], [-1, 0, 1], [-1, 0, 1]];
    if (0..3).contains(&i) && (0..3).contains(&j) {
        return Ok(PerlValue::integer(kx[i as usize][j as usize]));
    }
    Ok(PerlValue::integer(0))
}

// Roberts cross kernel value
fn builtin_roberts_kernel_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let kx: [[i64; 2]; 2] = [[1, 0], [0, -1]];
    if (0..2).contains(&i) && (0..2).contains(&j) {
        return Ok(PerlValue::integer(kx[i as usize][j as usize]));
    }
    Ok(PerlValue::integer(0))
}

// Laplacian kernel value
fn builtin_laplacian_kernel_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let k: [[i64; 3]; 3] = [[0, -1, 0], [-1, 4, -1], [0, -1, 0]];
    if (0..3).contains(&i) && (0..3).contains(&j) {
        return Ok(PerlValue::integer(k[i as usize][j as usize]));
    }
    Ok(PerlValue::integer(0))
}

// Canny threshold step (apply two thresholds T_low, T_high)
fn builtin_canny_threshold_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = f1(args);
    let t_lo = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let t_hi = args.get(2).map(|v| v.to_number()).unwrap_or(0.3);
    if g >= t_hi { Ok(PerlValue::integer(2)) }
    else if g >= t_lo { Ok(PerlValue::integer(1)) }
    else { Ok(PerlValue::integer(0)) }
}

// Hough accumulator step: increment cell (r, θ)
fn builtin_hough_accumulator_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prev = f1(args);
    let increment = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(prev + increment))
}

// RANSAC iteration count k = log(1-p) / log(1 - q^n)
fn builtin_ransac_iteration_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    let denom = (1.0 - q.powf(n)).ln();
    if denom == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float((1.0 - p).ln() / denom))
}

// Lucas-Kanade optical flow step: (Iₓ², Iₓ I_y, I_y², Iₓ I_t, I_y I_t)
fn builtin_optical_flow_lk_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ix = f1(args);
    let iy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let it = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let det = ix * ix * iy * iy - (ix * iy).powi(2);
    if det == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(-(iy * iy * ix * it - ix * iy * iy * it) / det))
}

// Horn-Schunck step: smoothness-weighted update
fn builtin_horn_schunck_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let avg_u = f1(args);
    let ix = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let p_val = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let denom = alpha * alpha + ix * ix;
    if denom == 0.0 { return Ok(PerlValue::float(avg_u)); }
    Ok(PerlValue::float(avg_u - ix * p_val / denom))
}

// Kalman filter predict state: x = Fx + Bu
fn builtin_kalman_predict_state(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let bu = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(f * x + bu))
}

// Kalman filter update state with Kalman gain
fn builtin_kalman_update_state(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x_pred = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let y = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let h_x = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(x_pred + k * (y - h_x)))
}

// Particle filter resample step (weight normalization sum)
fn builtin_particle_filter_resample(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let s: f64 = w.iter().sum();
    if s == 0.0 { return Ok(PerlValue::float(0.0)); }
    let n_eff: f64 = 1.0 / w.iter().map(|x| (x / s).powi(2)).sum::<f64>();
    Ok(PerlValue::float(n_eff))
}

// Unscented sigma point at index k
fn builtin_unscented_sigma_point(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mu = f1(args);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let kappa = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(mu + ((n + kappa).max(0.0) * sigma).sqrt()))
}

// Extended Kalman filter Jacobian step
fn builtin_ekf_jacobian_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dh_dx = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(dh_dx * p * dh_dx))
}

// MDP value iteration step: V(s) = max_a Σ_{s'} p(s'|s,a)(r + γV(s'))
fn builtin_markov_decision_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.99);
    let v_next = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(r + gamma * v_next))
}

// Bellman equation step
fn builtin_bellman_equation_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_markov_decision_value(args)
}

// Q-learning update: Q(s,a) ← Q(s,a) + α(r + γ max Q(s',a') - Q(s,a))
fn builtin_q_learning_update(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma = args.get(3).map(|v| v.to_number()).unwrap_or(0.99);
    let max_q_next = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(q + alpha * (r + gamma * max_q_next - q)))
}

// Policy iteration (Howard 1960): policy-evaluation step — solve V_π = R + γPV_π
// for fixed π using one expectation-Bellman update (NO max, follow current policy).
//   V_new(s) = Σ_{s'} p(s' | s, π(s)) · [r(s, π(s), s') + γ V_old(s')].
// Args: array of [prob_s', reward, V_old(s')] triples for the current action; γ.
fn builtin_policy_iteration_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let triples = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.99);
    let mut v = 0.0_f64;
    for ch in triples.chunks(3) {
        if ch.len() < 3 { continue; }
        v += ch[0] * (ch[1] + gamma * ch[2]);
    }
    Ok(PerlValue::float(v))
}

// Value iteration (Bellman 1957): one optimality-Bellman update — max over actions.
//   V_{k+1}(s) = max_a Σ_{s'} p(s'|s,a)[r(s,a,s') + γ V_k(s')].
// Args: array of action-Q-values [Q_a₁, Q_a₂, ...] (caller computes E[r+γV] per action).
fn builtin_value_iteration_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let qs = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if qs.is_empty() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(qs.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

// SARSA update: Q(s,a) ← Q(s,a) + α(r + γQ(s',a') - Q(s,a))
fn builtin_sarsa_update(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma = args.get(3).map(|v| v.to_number()).unwrap_or(0.99);
    let q_next = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(q + alpha * (r + gamma * q_next - q)))
}

// Double Q-learning step
fn builtin_double_q_learning_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_a = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma = args.get(3).map(|v| v.to_number()).unwrap_or(0.99);
    let q_b_next = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(q_a + alpha * (r + gamma * q_b_next - q_a)))
}

// UCB1 action value: Q(a) + c√(ln N / n_a)
fn builtin_ucb1_action_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.41);
    let big_n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n_a = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    if n_a <= 0.0 || big_n <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(q + c * (big_n.ln() / n_a).sqrt()))
}

// Thompson sampling Beta posterior mean (α / (α+β))
fn builtin_thompson_sample_beta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let total = alpha + beta;
    if total == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(alpha / total))
}

// Boltzmann softmax action prob: exp(Q/τ) / Σ exp(Q/τ)
fn builtin_boltzmann_softmax_action(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let tau = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let idx = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    if tau == 0.0 || q.is_empty() || idx >= q.len() { return Ok(PerlValue::float(0.0)); }
    let max = q.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let denom: f64 = q.iter().map(|v| ((v - max) / tau).exp()).sum();
    Ok(PerlValue::float(((q[idx] - max) / tau).exp() / denom))
}

// Epsilon-greedy explore-exploit decision
fn builtin_explore_exploit_epsilon(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let eps = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::integer(if r < eps { 1 } else { 0 }))
}

// Monte Carlo returns step: G_t = R_{t+1} + γ G_{t+1}
fn builtin_montecarlo_returns_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.99);
    let g_next = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(r + gamma * g_next))
}

// TD(0) update: V(s) ← V(s) + α(r + γV(s') - V(s))
fn builtin_td_zero_update(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma = args.get(3).map(|v| v.to_number()).unwrap_or(0.99);
    let v_next = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(v + alpha * (r + gamma * v_next - v)))
}

// TD(λ) eligibility-weighted update
fn builtin_td_lambda_update(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let delta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let e = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(v + alpha * delta * e))
}

// Gradient TD: δ - α θ ∇φ
fn builtin_gradient_temporal_diff(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let delta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let phi = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(theta + alpha * delta * phi))
}

// Deep Q target: r + γ max_{a'} Q_target(s', a')
fn builtin_deep_q_target(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.99);
    let max_q_target = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(r + gamma * max_q_target))
}

// DDPG critic loss step: (target - Q)²
fn builtin_ddpg_critic_loss_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let target = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let diff = target - q;
    Ok(PerlValue::float(diff * diff))
}

// PPO clipped surrogate term: min(rA, clip(r, 1-ε, 1+ε)·A)
fn builtin_ppo_clip_term(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let eps = args.get(2).map(|v| v.to_number()).unwrap_or(0.2);
    let r_clipped = r.clamp(1.0 - eps, 1.0 + eps);
    Ok(PerlValue::float((r * a).min(r_clipped * a)))
}

// TRPO KL constraint
fn builtin_trpo_kl_constraint(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let kl = f1(args);
    let delta = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::integer(if kl <= delta { 1 } else { 0 }))
}

// A3C advantage step: A = R - V(s)
fn builtin_a3c_advantage_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(r - v))
}

// PPO advantage step
fn builtin_ppo_advantage_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_a3c_advantage_step(args)
}

// GAE λ-advantage step: A_t = δ_t + γλ A_{t+1}
fn builtin_gae_advantage_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let delta = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.99);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(0.95);
    let a_next = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(delta + gamma * lambda * a_next))
}

// Generalized advantage estimator (alias for GAE step)
fn builtin_generalized_advantage(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gae_advantage_step(args)
}

// Information bottleneck step: I(X;T) - β I(T;Y) → minimize
fn builtin_information_bottleneck_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i_xt = f1(args);
    let i_ty = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(i_xt - beta * i_ty))
}

// Free energy principle: F = E[log q(z)] - E[log p(x, z)]
fn builtin_free_energy_principle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let log_q = f1(args);
    let log_p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(log_q - log_p))
}

// Fisher information metric (1-D, Cramér-Rao):  E[(∂ ln L/∂θ)²]
fn builtin_fisher_info_metric(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if g.is_empty() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(g.iter().map(|x| x * x).sum::<f64>() / g.len() as f64))
}

// Kullback-Jensen divergence (KL with reference midpoint)
fn builtin_kullback_jensen_div(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let q = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = p.len().min(q.len());
    let m: Vec<f64> = (0..n).map(|i| 0.5 * (p[i] + q[i])).collect();
    let kl_pm: f64 = (0..n).filter(|&i| p[i] > 0.0 && m[i] > 0.0).map(|i| p[i] * (p[i] / m[i]).log2()).sum();
    let kl_qm: f64 = (0..n).filter(|&i| q[i] > 0.0 && m[i] > 0.0).map(|i| q[i] * (q[i] / m[i]).log2()).sum();
    Ok(PerlValue::float(0.5 * (kl_pm + kl_qm)))
}

// Hellinger distance: √(½ Σ (√p - √q)²)
fn builtin_hellinger_distance_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let q = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = p.len().min(q.len());
    let s: f64 = (0..n).map(|i| (p[i].max(0.0).sqrt() - q[i].max(0.0).sqrt()).powi(2)).sum();
    Ok(PerlValue::float((0.5 * s).sqrt()))
}

// Total variation distance: ½ Σ |p - q|
fn builtin_total_variation_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let q = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = p.len().min(q.len());
    let s: f64 = (0..n).map(|i| (p[i] - q[i]).abs()).sum();
    Ok(PerlValue::float(0.5 * s))
}

// Bhattacharyya coefficient: Σ √(p·q)
fn builtin_bhattacharyya_coefficient(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let q = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = p.len().min(q.len());
    let s: f64 = (0..n).map(|i| (p[i].max(0.0) * q[i].max(0.0)).sqrt()).sum();
    Ok(PerlValue::float(s))
}

// Empirical Wasserstein distance (1-D, sorted)
fn builtin_wasserstein_dist_emp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut p = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut q = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    p.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    q.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = p.len().min(q.len());
    if n == 0 { return Ok(PerlValue::float(0.0)); }
    let s: f64 = (0..n).map(|i| (p[i] - q[i]).abs()).sum();
    Ok(PerlValue::float(s / n as f64))
}

// χ² metric: Σ (p - q)² / (p + q)
fn builtin_chisquare_metric(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let q = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = p.len().min(q.len());
    let s: f64 = (0..n).filter(|&i| p[i] + q[i] > 0.0).map(|i| (p[i] - q[i]).powi(2) / (p[i] + q[i])).sum();
    Ok(PerlValue::float(s))
}

// Hellinger kernel K_H(p, q) = exp(−H²(p, q)) = exp(2·(BC − 1)), the
// positive-definite Mercer kernel induced by the Hellinger embedding p ↦ √p.
// Distinct from the Bhattacharyya coefficient (which is BC = Σ √(pq) directly,
// without the exp wrap). Args: p, q.
fn builtin_hellinger_kernel(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let q = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = p.len().min(q.len());
    let bc: f64 = (0..n).map(|i| (p[i].max(0.0) * q[i].max(0.0)).sqrt()).sum();
    Ok(PerlValue::float((2.0 * (bc - 1.0)).exp()))
}

// Jensen-Shannon divergence (alias of kullback_jensen_div in 1-D form)
fn builtin_jensen_shannon_div(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_kullback_jensen_div(args)
}

// Rényi divergence: 1/(α-1) log Σ p^α q^(1-α)
fn builtin_renyi_divergence_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha = f1(args);
    let p = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let q = b40_to_floats(args.get(2).unwrap_or(&PerlValue::array(vec![])));
    if (alpha - 1.0).abs() < 1e-9 { return builtin_relative_entropy_kl(&[args.get(1).cloned().unwrap_or_default(), args.get(2).cloned().unwrap_or_default()]); }
    let n = p.len().min(q.len());
    let s: f64 = (0..n).filter(|&i| p[i] > 0.0 && q[i] > 0.0).map(|i| p[i].powf(alpha) * q[i].powf(1.0 - alpha)).sum();
    if s <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(s.ln() / (alpha - 1.0)))
}

// Amari α-divergence
fn builtin_amari_alpha_div(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha = f1(args);
    let p = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let q = b40_to_floats(args.get(2).unwrap_or(&PerlValue::array(vec![])));
    if (alpha.abs() - 1.0).abs() < 1e-9 { return builtin_relative_entropy_kl(&[args.get(1).cloned().unwrap_or_default(), args.get(2).cloned().unwrap_or_default()]); }
    let n = p.len().min(q.len());
    let s: f64 = (0..n).map(|i| p[i].powf((1.0 + alpha) / 2.0) * q[i].powf((1.0 - alpha) / 2.0)).sum();
    Ok(PerlValue::float((4.0 / (1.0 - alpha * alpha)) * (1.0 - s)))
}

// Csiszár ϕ-divergence: Σ q ϕ(p/q)
fn builtin_csiszar_phi_div(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let q = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = p.len().min(q.len());
    let s: f64 = (0..n).filter(|&i| q[i] > 0.0 && p[i] > 0.0).map(|i| q[i] * (p[i] / q[i]).ln()).sum();
    Ok(PerlValue::float(s))
}

// Sinkhorn iteration step: u ← a / (Kv), v ← b / (K^Tu)
fn builtin_sinkhorn_iteration_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let kv = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if kv == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(a / kv))
}

// Sliced p-Wasserstein (Rabin et al. 2011): project both empirical measures
// onto L random unit directions θ_l, compute 1-D Wasserstein per slice (sort
// + |F⁻¹(u) − G⁻¹(u)|), average. SW_p(μ, ν) = (1/L · Σ_l W_p(θ_l#μ, θ_l#ν)^p)^(1/p).
// Args: array of pre-projected slice distances [w₁, w₂, ..., w_L], p (default 1).
fn builtin_sliced_wasserstein(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let slices = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    if slices.is_empty() { return Ok(PerlValue::float(0.0)); }
    let avg: f64 = slices.iter().map(|w| w.abs().powf(p)).sum::<f64>() / slices.len() as f64;
    Ok(PerlValue::float(avg.powf(1.0 / p)))
}

// Gromov-Wasserstein step (linearized): Σ (p_ij - q_ij)²
fn builtin_gromov_wasserstein_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let q = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = p.len().min(q.len());
    let s: f64 = (0..n).map(|i| (p[i] - q[i]).powi(2)).sum();
    Ok(PerlValue::float(s))
}

// Spectral signature match: cosine similarity
fn builtin_spectral_signature_match(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let q = b40_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = p.len().min(q.len());
    let dot: f64 = (0..n).map(|i| p[i] * q[i]).sum();
    let np_norm: f64 = p.iter().take(n).map(|x| x * x).sum::<f64>().sqrt();
    let nq_norm: f64 = q.iter().take(n).map(|x| x * x).sum::<f64>().sqrt();
    if np_norm == 0.0 || nq_norm == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(dot / (np_norm * nq_norm)))
}

// MFCC coefficient step (DCT of log Mel)
fn builtin_mfcc_coeff_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let log_mel = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let len = log_mel.len();
    if len == 0 { return Ok(PerlValue::float(0.0)); }
    let s: f64 = (0..len).map(|k| log_mel[k] * ((std::f64::consts::PI * (n as f64) * (k as f64 + 0.5)) / len as f64).cos()).sum();
    Ok(PerlValue::float(s))
}

// Chroma feature step (12-bin pitch class)
fn builtin_chroma_feature_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b40_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let bin = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let s: f64 = v.iter().enumerate().filter(|(i, _)| i % 12 == bin).map(|(_, x)| *x).sum();
    Ok(PerlValue::float(s))
}
