// Batch 77 — PIL/OpenCV image processing: kernels, filters, morphology,
// histogram, edges, features, transforms.

fn b77_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// `image_resize` — bilinear new pixel = weighted average of 4 source pixels.
fn builtin_image_resize(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p00 = f1(args);
    let p01 = args.get(1).map(|v| v.to_number()).unwrap_or(p00);
    let p10 = args.get(2).map(|v| v.to_number()).unwrap_or(p00);
    let p11 = args.get(3).map(|v| v.to_number()).unwrap_or(p00);
    let dx = args.get(4).map(|v| v.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    let dy = args.get(5).map(|v| v.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    Ok(StrykeValue::float(p00 * (1.0 - dx) * (1.0 - dy)
        + p01 * dx * (1.0 - dy)
        + p10 * (1.0 - dx) * dy
        + p11 * dx * dy))
}

/// `image_grayscale` — luminance Y = 0.299 R + 0.587 G + 0.114 B (Rec. 601).
fn builtin_image_grayscale(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(0.299 * r + 0.587 * g + 0.114 * b))
}

/// `image_threshold` — binary threshold: 1 if ≥ T else 0.
fn builtin_image_threshold(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(127.0);
    Ok(StrykeValue::integer(if p >= t { 1 } else { 0 }))
}

/// `image_blur_gaussian` — 1-D Gaussian kernel coef at offset k for σ.
fn builtin_image_blur_gaussian(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = f1(args);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let coef = (-(k * k) / (2.0 * sigma * sigma)).exp()
        / (sigma * (2.0 * std::f64::consts::PI).sqrt());
    Ok(StrykeValue::float(coef))
}

/// `image_blur_box` — box-blur kernel value: 1 / (2k+1).
fn builtin_image_blur_box(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = i1(args).max(0) as f64;
    Ok(StrykeValue::float(1.0 / (2.0 * k + 1.0)))
}

/// `image_sharpen` — apply unsharp mask: I + α (I − blur).
fn builtin_image_sharpen(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = f1(args);
    let blur = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(i + alpha * (i - blur)))
}

/// `image_edge_canny` — Canny non-max-suppression: keep magnitude if local max.
fn builtin_image_edge_canny(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mag = f1(args);
    let n_left = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n_right = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let lo = args.get(3).map(|v| v.to_number()).unwrap_or(50.0);
    let hi = args.get(4).map(|v| v.to_number()).unwrap_or(150.0);
    let suppressed = if mag >= n_left && mag >= n_right { mag } else { 0.0 };
    if suppressed >= hi { Ok(StrykeValue::integer(2)) }
    else if suppressed >= lo { Ok(StrykeValue::integer(1)) }
    else { Ok(StrykeValue::integer(0)) }
}

/// `image_edge_sobel` — Sobel Gx kernel = [[−1,0,1],[−2,0,2],[−1,0,1]];
/// returns gradient magnitude √(Gx² + Gy²).
fn builtin_image_edge_sobel(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let gx = f1(args);
    let gy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((gx * gx + gy * gy).sqrt()))
}

/// `image_edge_laplacian` — Laplacian = ∇²I = Σ_n (I_n − 4 I).
fn builtin_image_edge_laplacian(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let s = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let e = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let w = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(n + s + e + w - 4.0 * p))
}

/// `image_dilate` — morphological dilation: max in window.
fn builtin_image_dilate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b77_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// `image_erode` — morphological erosion: min in window.
fn builtin_image_erode(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b77_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// `image_morphology_open` — erosion (local min) followed by dilation (local
/// max). Args: array of pixel windows: window 1 = inner-erode neighbourhood
/// for the centre pixel; we apply min then return that single eroded value
/// (caller dilates over the eroded image in a second pass).
fn builtin_image_morphology_open(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let inner = b77_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let outer = args.get(1).map(b77_to_floats).unwrap_or_default();
    let eroded = inner.iter().cloned().fold(f64::INFINITY, f64::min);
    let mut dil = eroded;
    for &p in &outer { if p < dil { dil = p; } }
    let final_max = outer.iter().cloned().fold(eroded, f64::max);
    let _ = dil;
    Ok(StrykeValue::float(final_max))
}

/// `image_morphology_close` — dilation followed by erosion: max of window's
/// max-pool, then min over outer windows.
fn builtin_image_morphology_close(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let inner = b77_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let outer = args.get(1).map(b77_to_floats).unwrap_or_default();
    let dilated = inner.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let final_min = outer.iter().cloned().fold(dilated, f64::min);
    Ok(StrykeValue::float(final_min))
}

/// `image_histogram` — counts at given bin (0..255).
fn builtin_image_histogram(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pixels = b77_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let bin = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(pixels.iter().filter(|&&p| p as i64 == bin).count() as i64))
}

/// `image_equalize` — histogram equalization: cdf-based intensity remap.
fn builtin_image_equalize(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cdf_p = f1(args);
    let cdf_min = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let l = args.get(3).map(|v| v.to_number()).unwrap_or(255.0);
    Ok(StrykeValue::float(((cdf_p - cdf_min) / (n - cdf_min).max(1e-15)) * (l - 1.0)))
}

/// `image_clahe` — Contrast Limited Adaptive Histogram Equalization clip step.
fn builtin_image_clahe(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let count = f1(args);
    let clip = args.get(1).map(|v| v.to_number()).unwrap_or(40.0);
    Ok(StrykeValue::float(count.min(clip)))
}

/// `image_contrast` — linear contrast: out = α·(in − 128) + 128.
fn builtin_image_contrast(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(alpha * (p - 128.0) + 128.0))
}

/// `image_brightness` — additive brightness: out = in + β.
fn builtin_image_brightness(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(p + beta))
}

/// `image_gamma` — power-law: out = 255 · (in/255)^γ.
fn builtin_image_gamma(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args).max(0.0);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float(255.0 * (p / 255.0).powf(gamma)))
}

/// `image_invert` — out = 255 − in.
fn builtin_image_invert(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(255.0 - f1(args)))
}

/// `image_sepia` — sepia tone: out_r = 0.393R + 0.769G + 0.189B.
fn builtin_image_sepia(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((0.393 * r + 0.769 * g + 0.189 * b).min(255.0)))
}

/// `image_posterize` — quantize to N levels per channel.
fn builtin_image_posterize(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let levels = args.get(1).map(|v| v.to_number()).unwrap_or(8.0).max(1.0);
    let step = 255.0 / levels;
    Ok(StrykeValue::float((p / step).floor() * step))
}

/// `image_solarize` — invert pixels above threshold.
fn builtin_image_solarize(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(128.0);
    Ok(StrykeValue::float(if p > t { 255.0 - p } else { p }))
}

/// `convolve_2d` — element of 2-D convolution sum at one position.
fn builtin_convolve_2d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kernel = b77_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let pixels = args.get(1).map(b77_to_floats).unwrap_or_default();
    let n = kernel.len().min(pixels.len());
    let s: f64 = (0..n).map(|i| kernel[i] * pixels[i]).sum();
    Ok(StrykeValue::float(s))
}

/// `filter_median` — median of pixel window.
fn builtin_filter_median(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut v = b77_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = v.len() / 2;
    Ok(StrykeValue::float(if v.len() % 2 == 1 { v[mid] }
                        else { (v[mid - 1] + v[mid]) / 2.0 }))
}

/// `filter_bilateral` — bilateral filter weight for spatial / range distance.
fn builtin_filter_bilateral(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dx = f1(args);
    let dy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dp = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma_s = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let sigma_r = args.get(4).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let w = (-(dx * dx + dy * dy) / (2.0 * sigma_s * sigma_s)).exp()
        * (-(dp * dp) / (2.0 * sigma_r * sigma_r)).exp();
    Ok(StrykeValue::float(w))
}

/// `filter_nlmeans` — non-local means weight: exp(−|Patch_i − Patch_j|² / h²).
fn builtin_filter_nlmeans(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dist_sq = f1(args).max(0.0);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(10.0).max(1e-15);
    Ok(StrykeValue::float((-dist_sq / (h * h)).exp()))
}

/// `gabor_filter` — Gabor kernel value at (x, y) with frequency f, σ, θ.
fn builtin_gabor_filter(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    let sigma = args.get(3).map(|v| v.to_number()).unwrap_or(2.0).max(1e-15);
    let theta = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let xp = x * theta.cos() + y * theta.sin();
    let yp = -x * theta.sin() + y * theta.cos();
    Ok(StrykeValue::float(
        (-(xp * xp + yp * yp) / (2.0 * sigma * sigma)).exp()
            * (2.0 * std::f64::consts::PI * f * xp).cos(),
    ))
}

/// `hog_features` — HOG cell magnitude binning bin count.
fn builtin_hog_features(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mag = f1(args);
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n_bins = args.get(2).map(|v| v.to_number()).unwrap_or(9.0).max(1.0);
    let bin = (theta * n_bins / std::f64::consts::PI).floor()
        .rem_euclid(n_bins) as i64;
    Ok(StrykeValue::float(mag * bin as f64))
}

/// `harris_corners` — Harris response: det(M) − k · trace(M)².
fn builtin_harris_corners(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ixx = f1(args);
    let iyy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let ixy = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(3).map(|v| v.to_number()).unwrap_or(0.04);
    let det = ixx * iyy - ixy * ixy;
    let trace = ixx + iyy;
    Ok(StrykeValue::float(det - k * trace * trace))
}

/// `shi_tomasi_corners` — Shi-Tomasi minimum eigenvalue criterion.
fn builtin_shi_tomasi_corners(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ixx = f1(args);
    let iyy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let ixy = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let half_trace = (ixx + iyy) / 2.0;
    let disc = (((ixx - iyy) / 2.0).powi(2) + ixy * ixy).sqrt();
    Ok(StrykeValue::float(half_trace - disc))
}

/// `sift_keypoints` — DoG response: difference of two Gaussian-blurred values.
fn builtin_sift_keypoints(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g_high = f1(args);
    let g_low = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(g_high - g_low))
}

/// `orb_keypoints` — FAST score: count of n contiguous bright/dark pixels.
fn builtin_orb_keypoints(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_bright = i1(args);
    let n_dark = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(n_bright.max(n_dark)))
}

/// `surf_keypoints` — SURF determinant of Hessian approx via box filters.
fn builtin_surf_keypoints(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dxx = f1(args);
    let dyy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dxy = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(dxx * dyy - 0.81 * dxy * dxy))
}

/// `template_match` — normalised cross-correlation: Σ(I−Ī)(T−T̄) / √(σ_I·σ_T).
fn builtin_template_match(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cov = f1(args);
    let var_i = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let var_t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float(cov / (var_i * var_t).sqrt()))
}

/// `face_detect_haar` — Haar feature: rectangle-difference sum.
fn builtin_face_detect_haar(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bright = f1(args);
    let dark = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(bright - dark))
}

/// `watershed_segment` — flooding fill water level threshold.
fn builtin_watershed_segment(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let elevations = b77_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let level = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(elevations.iter().filter(|&&e| e <= level).count() as i64))
}

/// `slic_superpixels` — k-means iteration step over (x, y, l*, a*, b*).
fn builtin_slic_superpixels(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dist_color = f1(args);
    let dist_space = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m = args.get(2).map(|v| v.to_number()).unwrap_or(10.0);
    let s = args.get(3).map(|v| v.to_number()).unwrap_or(10.0).max(1e-15);
    Ok(StrykeValue::float((dist_color * dist_color
        + (dist_space * dist_space) * (m / s).powi(2)).sqrt()))
}

/// `felzenszwalb_segment` — internal-difference threshold τ(C) = k / |C|.
fn builtin_felzenszwalb_segment(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k = f1(args);
    let c_size = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(k / c_size))
}

/// `graph_cut_segment` — energy E = Σ data(p) + λ Σ smooth(p,q).
fn builtin_graph_cut_segment(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let data_term = f1(args);
    let smooth_term = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(data_term + lambda * smooth_term))
}

/// `hough_lines` — accumulator vote at (ρ, θ).
fn builtin_hough_lines(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(x * theta.cos() + y * theta.sin()))
}

/// `hough_circles` — circle accumulator: (x − a)² + (y − b)² = r².
fn builtin_hough_circles(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(((x - a).powi(2) + (y - b).powi(2)).sqrt()))
}

/// `ransac_homography` — inlier count given consensus threshold.
fn builtin_ransac_homography(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let residuals = b77_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::integer(residuals.iter().filter(|&&r| r.abs() < t).count() as i64))
}

/// `optical_flow_lk` — Lucas-Kanade matrix solve: u = (A^T A)⁻¹ A^T b (det form).
fn builtin_optical_flow_lk(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ixx = f1(args);
    let iyy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let ixy = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let ixt = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let iyt = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let det = ixx * iyy - ixy * ixy;
    if det.abs() < 1e-15 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((iyy * (-ixt) - ixy * (-iyt)) / det))
}

/// `optical_flow_farneback` — quadratic polynomial expansion coefficient.
fn builtin_optical_flow_farneback(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(r * theta.cos()))
}

/// `corner_subpix` — subpixel refinement via paraboloid fit.
fn builtin_corner_subpix(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f0 = f1(args);
    let f1v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = 2.0 * (f0 - 2.0 * f1v + f2);
    if denom.abs() < 1e-15 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((f0 - f2) / denom))
}

/// `image_rotate` — rotate (x, y) about origin by angle θ, return new x.
fn builtin_image_rotate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(x * theta.cos() - y * theta.sin()))
}

/// `image_flip_h` — horizontal flip: x' = (W − 1) − x.
fn builtin_image_flip_h(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(w - 1.0 - x))
}

/// `image_flip_v` — vertical flip: y' = (H − 1) − y.
fn builtin_image_flip_v(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(h - 1.0 - y))
}

/// `image_emboss` — emboss kernel response: 2I − N − S − 128.
fn builtin_image_emboss(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let s = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(2.0 * i - n - s + 128.0))
}

/// `image_motion_blur` — convolve pixels with linear motion kernel along angle
/// θ. Returns weighted sum: each tap weight = (1 − |k − L/2| / (L/2)) / Z
/// (triangular kernel, Z = sum of weights). Args: pixels along motion line, L.
fn builtin_image_motion_blur(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pixels = b77_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let l = pixels.len() as f64;
    if l == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let mid = (l - 1.0) / 2.0;
    let span = mid.max(1e-15);
    let mut z = 0.0;
    let mut acc = 0.0;
    for (k, &p) in pixels.iter().enumerate() {
        let w = (1.0 - (k as f64 - mid).abs() / span).max(0.0);
        acc += w * p;
        z += w;
    }
    Ok(StrykeValue::float(acc / z.max(1e-15)))
}
