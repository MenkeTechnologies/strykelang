//! Computer vision / image kernels, information retrieval,
//! distance metrics, Bayesian updates, RL primitives, color spaces,
//! window functions, trie / Fenwick / union-find, network extras.

use crate::value::StrykeValue;
use parking_lot::RwLock;
use std::sync::Arc;
use std::collections::HashMap;

fn arg_f64(args: &[StrykeValue], idx: usize) -> Option<f64> {
    args.get(idx).map(|v| v.to_number())
}

fn arg_i64(args: &[StrykeValue], idx: usize) -> Option<i64> {
    args.get(idx).map(|v| v.to_int())
}

fn arg_str(args: &[StrykeValue], idx: usize) -> Option<String> {
    args.get(idx).map(|v| v.as_str_or_empty())
}

fn as_vec_f64(v: &StrykeValue) -> Vec<f64> {
    if let Some(a) = v.as_array_ref() {
        return a.read().iter().map(|x| x.to_number()).collect();
    }
    if let Some(a) = v.as_array_vec() {
        return a.iter().map(|x| x.to_number()).collect();
    }
    Vec::new()
}

fn as_vec_sv(v: &StrykeValue) -> Vec<StrykeValue> {
    if let Some(a) = v.as_array_ref() {
        return a.read().clone();
    }
    if let Some(a) = v.as_array_vec() {
        return a.to_vec();
    }
    Vec::new()
}

fn as_matrix(v: &StrykeValue) -> Vec<Vec<f64>> {
    as_vec_sv(v).iter().map(as_vec_f64).collect()
}

fn arr_sv(v: Vec<StrykeValue>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(v)))
}

fn arr_f64(v: Vec<f64>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(
        v.into_iter().map(StrykeValue::float).collect(),
    )))
}

fn matrix_to_sv(m: &[Vec<f64>]) -> StrykeValue {
    arr_sv(m.iter().map(|r| arr_f64(r.clone())).collect())
}

fn make_hash(pairs: Vec<(&str, StrykeValue)>) -> StrykeValue {
    use indexmap::IndexMap;
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    for (k, v) in pairs {
        h.insert(k.to_string(), v);
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

// ══════════════════════════════════════════════════════════════════════
// Computer vision / image kernels
// ══════════════════════════════════════════════════════════════════════

/// True mathematical 2D convolution: kernel flipped along both axes before
/// the windowed multiply-add. For non-flipping cross-correlation (the common
/// image-processing operation) use `correlate2d`.
pub fn conv2d_apply(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let kernel = args.get(1).map(as_matrix).unwrap_or_default();
    if img.is_empty() || kernel.is_empty() {
        return matrix_to_sv(&[]);
    }
    let kh = kernel.len();
    let kw = kernel[0].len();
    let pad_y = kh / 2;
    let pad_x = kw / 2;
    let h = img.len();
    let w = img[0].len();
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            let mut sum = 0.0;
            for ki in 0..kh {
                for kj in 0..kw {
                    // Convolution flips the kernel: index (kh-1-ki, kw-1-kj).
                    let yy = i as isize + (kh - 1 - ki) as isize - pad_y as isize;
                    let xx = j as isize + (kw - 1 - kj) as isize - pad_x as isize;
                    if yy >= 0 && yy < h as isize && xx >= 0 && xx < w as isize {
                        sum += img[yy as usize][xx as usize] * kernel[ki][kj];
                    }
                }
            }
            out[i][j] = sum;
        }
    }
    matrix_to_sv(&out)
}

pub fn conv1d_apply(args: &[StrykeValue]) -> StrykeValue {
    let signal = args.first().map(as_vec_f64).unwrap_or_default();
    let kernel = args.get(1).map(as_vec_f64).unwrap_or_default();
    if signal.is_empty() || kernel.is_empty() {
        return arr_f64(vec![]);
    }
    let n = signal.len();
    let k = kernel.len();
    let pad = k / 2;
    let mut out = vec![0.0_f64; n];
    for i in 0..n {
        for ki in 0..k {
            let idx = i as isize + ki as isize - pad as isize;
            if idx >= 0 && idx < n as isize {
                out[i] += signal[idx as usize] * kernel[ki];
            }
        }
    }
    arr_f64(out)
}

/// 2D cross-correlation: no kernel flip. `out[i,j] = Σ img[i+ki,j+kj] · k[ki,kj]`.
/// This is what most image-processing code calls "convolution" (e.g. OpenCV
/// `filter2D`). For mathematical convolution use `conv2d_apply`.
pub fn correlate2d(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let kernel = args.get(1).map(as_matrix).unwrap_or_default();
    if img.is_empty() || kernel.is_empty() {
        return matrix_to_sv(&[]);
    }
    let kh = kernel.len();
    let kw = kernel[0].len();
    let pad_y = kh / 2;
    let pad_x = kw / 2;
    let h = img.len();
    let w = img[0].len();
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            let mut sum = 0.0;
            for ki in 0..kh {
                for kj in 0..kw {
                    let yy = i as isize + ki as isize - pad_y as isize;
                    let xx = j as isize + kj as isize - pad_x as isize;
                    if yy >= 0 && yy < h as isize && xx >= 0 && xx < w as isize {
                        sum += img[yy as usize][xx as usize] * kernel[ki][kj];
                    }
                }
            }
            out[i][j] = sum;
        }
    }
    matrix_to_sv(&out)
}

pub fn gaussian_kernel(args: &[StrykeValue]) -> StrykeValue {
    let size = arg_i64(args, 0).unwrap_or(5).max(3) as usize | 1;
    let sigma = arg_f64(args, 1).unwrap_or(1.0).max(1e-9);
    let half = (size / 2) as f64;
    let mut k = vec![vec![0.0_f64; size]; size];
    let mut sum = 0.0;
    for i in 0..size {
        for j in 0..size {
            let dx = i as f64 - half;
            let dy = j as f64 - half;
            let v = (-(dx * dx + dy * dy) / (2.0 * sigma * sigma)).exp();
            k[i][j] = v;
            sum += v;
        }
    }
    for row in &mut k {
        for v in row {
            *v /= sum;
        }
    }
    matrix_to_sv(&k)
}

pub fn sobel_x_kernel(_args: &[StrykeValue]) -> StrykeValue {
    matrix_to_sv(&[
        vec![-1.0, 0.0, 1.0],
        vec![-2.0, 0.0, 2.0],
        vec![-1.0, 0.0, 1.0],
    ])
}

pub fn sobel_y_kernel(_args: &[StrykeValue]) -> StrykeValue {
    matrix_to_sv(&[
        vec![-1.0, -2.0, -1.0],
        vec![0.0, 0.0, 0.0],
        vec![1.0, 2.0, 1.0],
    ])
}

pub fn laplacian_kernel(_args: &[StrykeValue]) -> StrykeValue {
    matrix_to_sv(&[
        vec![0.0, 1.0, 0.0],
        vec![1.0, -4.0, 1.0],
        vec![0.0, 1.0, 0.0],
    ])
}

pub fn gradient_magnitude_2d(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    if img.is_empty() {
        return matrix_to_sv(&[]);
    }
    let h = img.len();
    let w = img[0].len();
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 1..h - 1 {
        for j in 1..w - 1 {
            let gx = -img[i - 1][j - 1] + img[i - 1][j + 1] - 2.0 * img[i][j - 1] + 2.0 * img[i][j + 1] - img[i + 1][j - 1] + img[i + 1][j + 1];
            let gy = -img[i - 1][j - 1] - 2.0 * img[i - 1][j] - img[i - 1][j + 1] + img[i + 1][j - 1] + 2.0 * img[i + 1][j] + img[i + 1][j + 1];
            out[i][j] = (gx * gx + gy * gy).sqrt();
        }
    }
    matrix_to_sv(&out)
}

pub fn non_max_suppression(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    if img.is_empty() {
        return matrix_to_sv(&[]);
    }
    let h = img.len();
    let w = img[0].len();
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 1..h - 1 {
        for j in 1..w - 1 {
            let v = img[i][j];
            let mut max_around = f64::NEG_INFINITY;
            for di in -1..=1 {
                for dj in -1..=1 {
                    if di == 0 && dj == 0 {
                        continue;
                    }
                    let n = img[(i as isize + di) as usize][(j as isize + dj) as usize];
                    if n > max_around {
                        max_around = n;
                    }
                }
            }
            if v >= max_around {
                out[i][j] = v;
            }
        }
    }
    matrix_to_sv(&out)
}

pub fn otsu_threshold(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let mut hist = vec![0_u64; 256];
    for row in &img {
        for &v in row {
            let bin = v.clamp(0.0, 255.0) as usize;
            hist[bin] += 1;
        }
    }
    let total: u64 = hist.iter().sum();
    if total == 0 {
        return StrykeValue::float(128.0);
    }
    let sum_total: f64 = hist.iter().enumerate().map(|(i, &c)| i as f64 * c as f64).sum();
    let mut sum_back = 0.0_f64;
    let mut w_back = 0_u64;
    let mut max_var = 0.0_f64;
    let mut best = 0_usize;
    for t in 0..256 {
        w_back += hist[t];
        if w_back == 0 {
            continue;
        }
        let w_fore = total - w_back;
        if w_fore == 0 {
            break;
        }
        sum_back += t as f64 * hist[t] as f64;
        let m_back = sum_back / w_back as f64;
        let m_fore = (sum_total - sum_back) / w_fore as f64;
        let var_between = w_back as f64 * w_fore as f64 * (m_back - m_fore).powi(2);
        if var_between > max_var {
            max_var = var_between;
            best = t;
        }
    }
    StrykeValue::float(best as f64)
}

pub fn adaptive_threshold(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let window = arg_i64(args, 1).unwrap_or(11).max(3) as usize | 1;
    let c = arg_f64(args, 2).unwrap_or(2.0);
    if img.is_empty() {
        return matrix_to_sv(&[]);
    }
    let h = img.len();
    let w = img[0].len();
    let half = window / 2;
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            let mut sum = 0.0;
            let mut count = 0;
            for di in 0..window {
                for dj in 0..window {
                    let yy = i as isize + di as isize - half as isize;
                    let xx = j as isize + dj as isize - half as isize;
                    if yy >= 0 && yy < h as isize && xx >= 0 && xx < w as isize {
                        sum += img[yy as usize][xx as usize];
                        count += 1;
                    }
                }
            }
            let mean = if count > 0 { sum / count as f64 } else { 0.0 };
            out[i][j] = if img[i][j] > mean - c { 255.0 } else { 0.0 };
        }
    }
    matrix_to_sv(&out)
}

pub fn canny_edges_simple(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let low = arg_f64(args, 1).unwrap_or(50.0);
    let high = arg_f64(args, 2).unwrap_or(150.0);
    let grad = as_matrix(&gradient_magnitude_2d(&[matrix_to_sv(&img)]));
    let h = grad.len();
    let mut out = grad.clone();
    for i in 0..h {
        for j in 0..out[i].len() {
            out[i][j] = if grad[i][j] >= high {
                255.0
            } else if grad[i][j] >= low {
                128.0
            } else {
                0.0
            };
        }
    }
    matrix_to_sv(&out)
}

/// Harris corner response: `det(M) − k·trace(M)²` where `M` is the
/// 2×2 structure tensor `Σ_w [[Iₓ², IₓIᵧ], [IₓIᵧ, Iᵧ²]]` summed over a
/// 3×3 window. Without the windowed sum `det(M)` is identically zero
/// per pixel and the response would not distinguish corners from edges.
pub fn harris_response(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let k = arg_f64(args, 1).unwrap_or(0.04);
    if img.is_empty() {
        return matrix_to_sv(&[]);
    }
    let h = img.len();
    let w = img[0].len();
    if h < 3 || w < 3 {
        return matrix_to_sv(&vec![vec![0.0_f64; w]; h]);
    }
    let mut ix = vec![vec![0.0_f64; w]; h];
    let mut iy = vec![vec![0.0_f64; w]; h];
    for i in 1..h - 1 {
        for j in 1..w - 1 {
            ix[i][j] = (img[i][j + 1] - img[i][j - 1]) / 2.0;
            iy[i][j] = (img[i + 1][j] - img[i - 1][j]) / 2.0;
        }
    }
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 2..h - 2 {
        for j in 2..w - 2 {
            let mut sxx = 0.0_f64;
            let mut syy = 0.0_f64;
            let mut sxy = 0.0_f64;
            for di in -1..=1_isize {
                for dj in -1..=1_isize {
                    let y = (i as isize + di) as usize;
                    let x = (j as isize + dj) as usize;
                    let dx = ix[y][x];
                    let dy = iy[y][x];
                    sxx += dx * dx;
                    syy += dy * dy;
                    sxy += dx * dy;
                }
            }
            let det = sxx * syy - sxy * sxy;
            let trace = sxx + syy;
            out[i][j] = det - k * trace * trace;
        }
    }
    matrix_to_sv(&out)
}

pub fn integral_image(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    if img.is_empty() {
        return matrix_to_sv(&[]);
    }
    let h = img.len();
    let w = img[0].len();
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            out[i][j] = img[i][j]
                + if i > 0 { out[i - 1][j] } else { 0.0 }
                + if j > 0 { out[i][j - 1] } else { 0.0 }
                - if i > 0 && j > 0 { out[i - 1][j - 1] } else { 0.0 };
        }
    }
    matrix_to_sv(&out)
}

pub fn sliding_dot_product(args: &[StrykeValue]) -> StrykeValue {
    let signal = args.first().map(as_vec_f64).unwrap_or_default();
    let kernel = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = signal.len();
    let k = kernel.len();
    if n < k || k == 0 {
        return arr_f64(vec![]);
    }
    let mut out = Vec::with_capacity(n - k + 1);
    for i in 0..=n - k {
        let dot: f64 = (0..k).map(|j| signal[i + j] * kernel[j]).sum();
        out.push(dot);
    }
    arr_f64(out)
}

// ══════════════════════════════════════════════════════════════════════
// Information retrieval / distance metrics
// ══════════════════════════════════════════════════════════════════════

pub fn tfidf_compute(args: &[StrykeValue]) -> StrykeValue {
    // Args: docs (array of token arrays). Returns matrix [doc][term_idx] of TF-IDF.
    let docs_v = args.first().map(as_vec_sv).unwrap_or_default();
    let docs: Vec<Vec<String>> = docs_v
        .iter()
        .map(|d| as_vec_sv(d).iter().map(|x| x.as_str_or_empty()).collect())
        .collect();
    let n_docs = docs.len();
    if n_docs == 0 {
        return make_hash(vec![("vocab", arr_sv(vec![])), ("matrix", matrix_to_sv(&[]))]);
    }
    let mut vocab: Vec<String> = docs.iter().flat_map(|d| d.iter().cloned()).collect();
    vocab.sort();
    vocab.dedup();
    let term_idx: HashMap<String, usize> = vocab.iter().enumerate().map(|(i, t)| (t.clone(), i)).collect();
    let mut df = vec![0_u64; vocab.len()];
    for doc in &docs {
        let mut seen = std::collections::HashSet::new();
        for t in doc {
            if let Some(&i) = term_idx.get(t) {
                if seen.insert(i) {
                    df[i] += 1;
                }
            }
        }
    }
    let mut matrix = vec![vec![0.0_f64; vocab.len()]; n_docs];
    for (d, doc) in docs.iter().enumerate() {
        let n_terms = doc.len() as f64;
        if n_terms == 0.0 {
            continue;
        }
        let mut tf = HashMap::<usize, u64>::new();
        for t in doc {
            if let Some(&i) = term_idx.get(t) {
                *tf.entry(i).or_insert(0) += 1;
            }
        }
        for (i, count) in tf {
            let tf_val = count as f64 / n_terms;
            let idf = ((1.0 + n_docs as f64) / (1.0 + df[i] as f64)).ln() + 1.0;
            matrix[d][i] = tf_val * idf;
        }
    }
    make_hash(vec![
        ("vocab", arr_sv(vocab.into_iter().map(StrykeValue::string).collect())),
        ("matrix", matrix_to_sv(&matrix)),
    ])
}

pub fn bm25_score(args: &[StrykeValue]) -> StrykeValue {
    let query: Vec<String> = args.first().map(as_vec_sv).unwrap_or_default().iter().map(|x| x.as_str_or_empty()).collect();
    let doc: Vec<String> = args.get(1).map(as_vec_sv).unwrap_or_default().iter().map(|x| x.as_str_or_empty()).collect();
    let avg_dl = arg_f64(args, 2).unwrap_or(0.0).max(1e-9);
    let n_total = arg_f64(args, 3).unwrap_or(1.0).max(1.0);
    let df_v: Vec<f64> = args.get(4).map(as_vec_f64).unwrap_or_default();
    let k1 = arg_f64(args, 5).unwrap_or(1.5);
    let b = arg_f64(args, 6).unwrap_or(0.75);
    let dl = doc.len() as f64;
    let mut score = 0.0;
    for (i, term) in query.iter().enumerate() {
        let tf = doc.iter().filter(|t| *t == term).count() as f64;
        if tf == 0.0 {
            continue;
        }
        let df = df_v.get(i).copied().unwrap_or(1.0).max(1.0);
        let idf = ((n_total - df + 0.5) / (df + 0.5) + 1.0).ln();
        let num = tf * (k1 + 1.0);
        let denom = tf + k1 * (1.0 - b + b * dl / avg_dl);
        score += idf * num / denom;
    }
    StrykeValue::float(score)
}

pub fn cosine_sim_sparse(args: &[StrykeValue]) -> StrykeValue {
    // Cosine similarity over sparse `[[index, value], ...]` representations.
    // Computes the dot product over shared indices and the L2 norms of each
    // sparse vector independently.
    let parse_sparse = |v: &StrykeValue| -> Vec<(i64, f64)> {
        as_vec_sv(v)
            .iter()
            .map(|pair| {
                let xs = as_vec_sv(pair);
                let idx = xs.first().map(|x| x.to_int()).unwrap_or(0);
                let val = xs.get(1).map(|x| x.to_number()).unwrap_or(0.0);
                (idx, val)
            })
            .collect()
    };
    let a = parse_sparse(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = parse_sparse(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    if a.is_empty() || b.is_empty() {
        return StrykeValue::float(0.0);
    }
    let mut b_map: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
    for &(i, v) in &b {
        *b_map.entry(i).or_insert(0.0) += v;
    }
    let dot: f64 = a.iter().map(|&(i, v)| v * b_map.get(&i).copied().unwrap_or(0.0)).sum();
    let na: f64 = a.iter().map(|&(_, v)| v * v).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|&(_, v)| v * v).sum::<f64>().sqrt();
    if na < 1e-12 || nb < 1e-12 {
        return StrykeValue::float(0.0);
    }
    StrykeValue::float(dot / (na * nb))
}

pub fn jaccard_sim(args: &[StrykeValue]) -> StrykeValue {
    let a: Vec<String> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF)).iter().map(|x| x.as_str_or_empty()).collect();
    let b: Vec<String> = as_vec_sv(args.get(1).unwrap_or(&StrykeValue::UNDEF)).iter().map(|x| x.as_str_or_empty()).collect();
    let sa: std::collections::HashSet<String> = a.into_iter().collect();
    let sb: std::collections::HashSet<String> = b.into_iter().collect();
    let inter = sa.intersection(&sb).count();
    let uni = sa.union(&sb).count();
    if uni == 0 {
        return StrykeValue::float(0.0);
    }
    StrykeValue::float(inter as f64 / uni as f64)
}

pub fn overlap_coeff(args: &[StrykeValue]) -> StrykeValue {
    let a: Vec<String> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF)).iter().map(|x| x.as_str_or_empty()).collect();
    let b: Vec<String> = as_vec_sv(args.get(1).unwrap_or(&StrykeValue::UNDEF)).iter().map(|x| x.as_str_or_empty()).collect();
    let sa: std::collections::HashSet<String> = a.into_iter().collect();
    let sb: std::collections::HashSet<String> = b.into_iter().collect();
    let inter = sa.intersection(&sb).count();
    let m = sa.len().min(sb.len());
    if m == 0 {
        return StrykeValue::float(0.0);
    }
    StrykeValue::float(inter as f64 / m as f64)
}

pub fn dice_coeff(args: &[StrykeValue]) -> StrykeValue {
    let a: Vec<String> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF)).iter().map(|x| x.as_str_or_empty()).collect();
    let b: Vec<String> = as_vec_sv(args.get(1).unwrap_or(&StrykeValue::UNDEF)).iter().map(|x| x.as_str_or_empty()).collect();
    let sa: std::collections::HashSet<String> = a.into_iter().collect();
    let sb: std::collections::HashSet<String> = b.into_iter().collect();
    let inter = sa.intersection(&sb).count();
    let total = sa.len() + sb.len();
    if total == 0 {
        return StrykeValue::float(0.0);
    }
    StrykeValue::float(2.0 * inter as f64 / total as f64)
}

pub fn tanimoto_coeff(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec_f64).unwrap_or_default();
    let b = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = a.len().min(b.len());
    let dot: f64 = (0..n).map(|i| a[i] * b[i]).sum();
    let na: f64 = a.iter().map(|x| x * x).sum();
    let nb: f64 = b.iter().map(|x| x * x).sum();
    let denom = na + nb - dot;
    if denom.abs() < 1e-12 {
        return StrykeValue::float(0.0);
    }
    StrykeValue::float(dot / denom)
}

pub fn tversky_index(args: &[StrykeValue]) -> StrykeValue {
    let a: std::collections::HashSet<String> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF)).iter().map(|x| x.as_str_or_empty()).collect();
    let b: std::collections::HashSet<String> = as_vec_sv(args.get(1).unwrap_or(&StrykeValue::UNDEF)).iter().map(|x| x.as_str_or_empty()).collect();
    let alpha = arg_f64(args, 2).unwrap_or(1.0);
    let beta = arg_f64(args, 3).unwrap_or(1.0);
    let inter = a.intersection(&b).count() as f64;
    let a_minus_b = a.difference(&b).count() as f64;
    let b_minus_a = b.difference(&a).count() as f64;
    let denom = inter + alpha * a_minus_b + beta * b_minus_a;
    if denom.abs() < 1e-12 {
        return StrykeValue::float(0.0);
    }
    StrykeValue::float(inter / denom)
}

pub fn manhattan_norm(args: &[StrykeValue]) -> StrykeValue {
    let v = args.first().map(as_vec_f64).unwrap_or_default();
    StrykeValue::float(v.iter().map(|x| x.abs()).sum())
}

pub fn chebyshev_norm(args: &[StrykeValue]) -> StrykeValue {
    let v = args.first().map(as_vec_f64).unwrap_or_default();
    StrykeValue::float(v.iter().map(|x| x.abs()).fold(0.0_f64, f64::max))
}

pub fn minkowski_norm(args: &[StrykeValue]) -> StrykeValue {
    let v = args.first().map(as_vec_f64).unwrap_or_default();
    let p = arg_f64(args, 1).unwrap_or(2.0);
    let s: f64 = v.iter().map(|x| x.abs().powf(p)).sum();
    StrykeValue::float(s.powf(1.0 / p))
}

pub fn mahalanobis_sq(args: &[StrykeValue]) -> StrykeValue {
    let x = args.first().map(as_vec_f64).unwrap_or_default();
    let mean = args.get(1).map(as_vec_f64).unwrap_or_default();
    let cov_inv = args.get(2).map(as_matrix).unwrap_or_default();
    let n = x.len();
    if n == 0 || mean.len() != n || cov_inv.len() != n {
        return StrykeValue::float(0.0);
    }
    let diff: Vec<f64> = (0..n).map(|i| x[i] - mean[i]).collect();
    let mut sum = 0.0;
    for i in 0..n {
        for j in 0..n {
            sum += diff[i] * cov_inv[i].get(j).copied().unwrap_or(0.0) * diff[j];
        }
    }
    StrykeValue::float(sum)
}

pub fn canberra_dist(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec_f64).unwrap_or_default();
    let b = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = a.len().min(b.len());
    let sum: f64 = (0..n)
        .filter(|&i| a[i].abs() + b[i].abs() > 0.0)
        .map(|i| (a[i] - b[i]).abs() / (a[i].abs() + b[i].abs()))
        .sum();
    StrykeValue::float(sum)
}

pub fn braycurtis_dist(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec_f64).unwrap_or_default();
    let b = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = a.len().min(b.len());
    let num: f64 = (0..n).map(|i| (a[i] - b[i]).abs()).sum();
    let denom: f64 = (0..n).map(|i| (a[i] + b[i]).abs()).sum();
    if denom < 1e-12 {
        return StrykeValue::float(0.0);
    }
    StrykeValue::float(num / denom)
}

pub fn earth_mover_1d(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec_f64).unwrap_or_default();
    let b = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = a.len().min(b.len());
    let mut sum_a = 0.0_f64;
    let mut sum_b = 0.0_f64;
    let mut work = 0.0_f64;
    for i in 0..n {
        sum_a += a[i];
        sum_b += b[i];
        work += (sum_a - sum_b).abs();
    }
    StrykeValue::float(work)
}

// ══════════════════════════════════════════════════════════════════════
// Bayesian inference
// ══════════════════════════════════════════════════════════════════════

pub fn bayesian_beta_update(args: &[StrykeValue]) -> StrykeValue {
    let alpha = arg_f64(args, 0).unwrap_or(1.0);
    let beta = arg_f64(args, 1).unwrap_or(1.0);
    let successes = arg_f64(args, 2).unwrap_or(0.0);
    let failures = arg_f64(args, 3).unwrap_or(0.0);
    make_hash(vec![
        ("alpha", StrykeValue::float(alpha + successes)),
        ("beta", StrykeValue::float(beta + failures)),
    ])
}

pub fn bayesian_normal_update(args: &[StrykeValue]) -> StrykeValue {
    let prior_mean = arg_f64(args, 0).unwrap_or(0.0);
    let prior_var = arg_f64(args, 1).unwrap_or(1.0).max(1e-12);
    let data_mean = arg_f64(args, 2).unwrap_or(0.0);
    let data_var = arg_f64(args, 3).unwrap_or(1.0).max(1e-12);
    let n = arg_f64(args, 4).unwrap_or(1.0).max(1.0);
    let post_var = 1.0 / (1.0 / prior_var + n / data_var);
    let post_mean = post_var * (prior_mean / prior_var + n * data_mean / data_var);
    make_hash(vec![
        ("mean", StrykeValue::float(post_mean)),
        ("variance", StrykeValue::float(post_var)),
    ])
}

pub fn bayes_factor(args: &[StrykeValue]) -> StrykeValue {
    let likelihood_h1 = arg_f64(args, 0).unwrap_or(1.0).max(1e-30);
    let likelihood_h0 = arg_f64(args, 1).unwrap_or(1.0).max(1e-30);
    StrykeValue::float(likelihood_h1 / likelihood_h0)
}

pub fn posterior_predictive_beta(args: &[StrykeValue]) -> StrykeValue {
    let alpha = arg_f64(args, 0).unwrap_or(1.0);
    let beta = arg_f64(args, 1).unwrap_or(1.0);
    StrykeValue::float(alpha / (alpha + beta))
}

pub fn posterior_predictive_normal(args: &[StrykeValue]) -> StrykeValue {
    let mean = arg_f64(args, 0).unwrap_or(0.0);
    let var = arg_f64(args, 1).unwrap_or(1.0);
    make_hash(vec![
        ("mean", StrykeValue::float(mean)),
        ("variance", StrykeValue::float(var)),
    ])
}

pub fn prior_jeffreys_uniform(_args: &[StrykeValue]) -> StrykeValue {
    make_hash(vec![("alpha", StrykeValue::float(0.5)), ("beta", StrykeValue::float(0.5))])
}

pub fn maximum_a_posteriori(args: &[StrykeValue]) -> StrykeValue {
    let posterior = args.first().map(as_vec_f64).unwrap_or_default();
    if posterior.is_empty() {
        return StrykeValue::integer(-1);
    }
    let (idx, _) = posterior
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap();
    StrykeValue::integer(idx as i64)
}

pub fn credible_interval_beta(args: &[StrykeValue]) -> StrykeValue {
    use statrs::distribution::{Beta, ContinuousCDF};
    let alpha = arg_f64(args, 0).unwrap_or(1.0).max(1e-9);
    let beta = arg_f64(args, 1).unwrap_or(1.0).max(1e-9);
    let level = arg_f64(args, 2).unwrap_or(0.95).clamp(0.0, 1.0);
    let lo_p = (1.0 - level) / 2.0;
    let hi_p = 1.0 - lo_p;
    match Beta::new(alpha, beta) {
        Ok(d) => arr_f64(vec![d.inverse_cdf(lo_p), d.inverse_cdf(hi_p)]),
        Err(_) => arr_f64(vec![0.0, 1.0]),
    }
}

pub fn credible_interval_normal(args: &[StrykeValue]) -> StrykeValue {
    use statrs::distribution::{ContinuousCDF, Normal};
    let mean = arg_f64(args, 0).unwrap_or(0.0);
    let sd = arg_f64(args, 1).unwrap_or(1.0).max(1e-9);
    let level = arg_f64(args, 2).unwrap_or(0.95).clamp(0.0, 1.0);
    let lo_p = (1.0 - level) / 2.0;
    let hi_p = 1.0 - lo_p;
    let n = Normal::new(mean, sd).unwrap();
    arr_f64(vec![n.inverse_cdf(lo_p), n.inverse_cdf(hi_p)])
}

// ══════════════════════════════════════════════════════════════════════
// Reinforcement learning
// ══════════════════════════════════════════════════════════════════════

pub fn qlearning_step(args: &[StrykeValue]) -> StrykeValue {
    let q = arg_f64(args, 0).unwrap_or(0.0);
    let alpha = arg_f64(args, 1).unwrap_or(0.1);
    let reward = arg_f64(args, 2).unwrap_or(0.0);
    let gamma = arg_f64(args, 3).unwrap_or(0.9);
    let max_next = arg_f64(args, 4).unwrap_or(0.0);
    StrykeValue::float(q + alpha * (reward + gamma * max_next - q))
}

pub fn sarsa_step(args: &[StrykeValue]) -> StrykeValue {
    let q = arg_f64(args, 0).unwrap_or(0.0);
    let alpha = arg_f64(args, 1).unwrap_or(0.1);
    let reward = arg_f64(args, 2).unwrap_or(0.0);
    let gamma = arg_f64(args, 3).unwrap_or(0.9);
    let q_next = arg_f64(args, 4).unwrap_or(0.0);
    StrykeValue::float(q + alpha * (reward + gamma * q_next - q))
}

pub fn epsilon_greedy_choose(args: &[StrykeValue]) -> StrykeValue {
    let values = args.first().map(as_vec_f64).unwrap_or_default();
    let epsilon = arg_f64(args, 1).unwrap_or(0.1).clamp(0.0, 1.0);
    let seed = arg_i64(args, 2).unwrap_or(0) as u64;
    if values.is_empty() {
        return StrykeValue::integer(-1);
    }
    let state = seed
        .wrapping_add(0x9E3779B97F4A7C15)
        .wrapping_mul(6364136223846793005);
    let r = (state >> 32) as f64 / u32::MAX as f64;
    let r2 = (state.wrapping_mul(2862933555777941757) >> 32) as f64 / u32::MAX as f64;
    if r < epsilon {
        let idx = (r2 * values.len() as f64) as usize;
        StrykeValue::integer(idx.min(values.len() - 1) as i64)
    } else {
        let (idx, _) = values
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();
        StrykeValue::integer(idx as i64)
    }
}

pub fn ucb1_choose(args: &[StrykeValue]) -> StrykeValue {
    let q_values = args.first().map(as_vec_f64).unwrap_or_default();
    let counts: Vec<f64> = args.get(1).map(as_vec_f64).unwrap_or_default();
    let total: f64 = counts.iter().sum::<f64>().max(1.0);
    let c = arg_f64(args, 2).unwrap_or(2.0_f64.sqrt());
    let mut best = 0_usize;
    let mut best_score = f64::NEG_INFINITY;
    for (i, &q) in q_values.iter().enumerate() {
        let n = counts.get(i).copied().unwrap_or(0.0);
        let score = if n == 0.0 {
            f64::INFINITY
        } else {
            q + c * (total.ln() / n).sqrt()
        };
        if score > best_score {
            best_score = score;
            best = i;
        }
    }
    StrykeValue::integer(best as i64)
}

pub fn thompson_beta_choose(args: &[StrykeValue]) -> StrykeValue {
    use statrs::distribution::{Beta, ContinuousCDF};
    let alphas = args.first().map(as_vec_f64).unwrap_or_default();
    let betas = args.get(1).map(as_vec_f64).unwrap_or_default();
    let seed = arg_i64(args, 2).unwrap_or(0) as u64;
    let n = alphas.len().min(betas.len());
    let mut state = seed.wrapping_add(0x9E3779B97F4A7C15);
    let mut best = 0_usize;
    let mut best_s = f64::NEG_INFINITY;
    for i in 0..n {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let u = (state >> 32) as f64 / u32::MAX as f64;
        let sample = match Beta::new(alphas[i].max(1e-9), betas[i].max(1e-9)) {
            Ok(d) => d.inverse_cdf(u.clamp(1e-9, 1.0 - 1e-9)),
            Err(_) => 0.0,
        };
        if sample > best_s {
            best_s = sample;
            best = i;
        }
    }
    StrykeValue::integer(best as i64)
}

pub fn softmax_choose(args: &[StrykeValue]) -> StrykeValue {
    let logits = args.first().map(as_vec_f64).unwrap_or_default();
    let temp = arg_f64(args, 1).unwrap_or(1.0).max(1e-9);
    let seed = arg_i64(args, 2).unwrap_or(0) as u64;
    if logits.is_empty() {
        return StrykeValue::integer(-1);
    }
    let max = logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = logits.iter().map(|x| ((x - max) / temp).exp()).collect();
    let sum: f64 = exps.iter().sum();
    let probs: Vec<f64> = exps.iter().map(|x| x / sum).collect();
    let state = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    let r = (state >> 32) as f64 / u32::MAX as f64;
    let mut cum = 0.0;
    for (i, p) in probs.iter().enumerate() {
        cum += p;
        if r < cum {
            return StrykeValue::integer(i as i64);
        }
    }
    StrykeValue::integer((probs.len() - 1) as i64)
}

pub fn rl_n_step_return(args: &[StrykeValue]) -> StrykeValue {
    let rewards = args.first().map(as_vec_f64).unwrap_or_default();
    let gamma = arg_f64(args, 1).unwrap_or(0.9);
    let mut g = 0.0;
    let mut factor = 1.0;
    for r in &rewards {
        g += factor * r;
        factor *= gamma;
    }
    StrykeValue::float(g)
}

pub fn rl_td_error(args: &[StrykeValue]) -> StrykeValue {
    let r = arg_f64(args, 0).unwrap_or(0.0);
    let v_next = arg_f64(args, 1).unwrap_or(0.0);
    let v = arg_f64(args, 2).unwrap_or(0.0);
    let gamma = arg_f64(args, 3).unwrap_or(0.9);
    StrykeValue::float(r + gamma * v_next - v)
}

pub fn rl_discount_returns(args: &[StrykeValue]) -> StrykeValue {
    let rewards = args.first().map(as_vec_f64).unwrap_or_default();
    let gamma = arg_f64(args, 1).unwrap_or(0.9);
    let n = rewards.len();
    let mut out = vec![0.0; n];
    let mut g = 0.0;
    for i in (0..n).rev() {
        g = rewards[i] + gamma * g;
        out[i] = g;
    }
    arr_f64(out)
}

// ══════════════════════════════════════════════════════════════════════
// Color spaces (LCH, OKLAB, OKLCH, CIEDE)
// ══════════════════════════════════════════════════════════════════════

fn rgb_to_lab_inner(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let to_linear = |c: f64| {
        let c = c / 255.0;
        if c > 0.04045 {
            ((c + 0.055) / 1.055).powf(2.4)
        } else {
            c / 12.92
        }
    };
    let r = to_linear(r);
    let g = to_linear(g);
    let b = to_linear(b);
    let x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) * 100.0;
    let y = (0.2126729 * r + 0.7151522 * g + 0.0721750 * b) * 100.0;
    let z = (0.0193339 * r + 0.1191920 * g + 0.9503041 * b) * 100.0;
    let xn = 95.047;
    let yn = 100.0;
    let zn = 108.883;
    let f = |t: f64| if t > 0.008856 { t.powf(1.0 / 3.0) } else { 7.787 * t + 16.0 / 116.0 };
    let fx = f(x / xn);
    let fy = f(y / yn);
    let fz = f(z / zn);
    let l = 116.0 * fy - 16.0;
    let a = 500.0 * (fx - fy);
    let b2 = 200.0 * (fy - fz);
    (l, a, b2)
}

pub fn rgb_to_lch(args: &[StrykeValue]) -> StrykeValue {
    let xs = as_vec_f64(args.first().unwrap_or(&StrykeValue::UNDEF));
    if xs.len() < 3 {
        return arr_f64(vec![]);
    }
    let (l, a, b) = rgb_to_lab_inner(xs[0], xs[1], xs[2]);
    let c = (a * a + b * b).sqrt();
    let mut h = b.atan2(a).to_degrees();
    if h < 0.0 {
        h += 360.0;
    }
    arr_f64(vec![l, c, h])
}

pub fn lch_to_rgb(args: &[StrykeValue]) -> StrykeValue {
    let xs = as_vec_f64(args.first().unwrap_or(&StrykeValue::UNDEF));
    if xs.len() < 3 {
        return arr_f64(vec![]);
    }
    let l = xs[0];
    let c = xs[1];
    let h = xs[2].to_radians();
    let a = c * h.cos();
    let b = c * h.sin();
    lab_to_rgb_inner(l, a, b)
}

fn lab_to_rgb_inner(l: f64, a: f64, b: f64) -> StrykeValue {
    let fy = (l + 16.0) / 116.0;
    let fx = a / 500.0 + fy;
    let fz = fy - b / 200.0;
    let fcube = |t: f64| if t.powi(3) > 0.008856 { t.powi(3) } else { (t - 16.0 / 116.0) / 7.787 };
    let xn = 95.047;
    let yn = 100.0;
    let zn = 108.883;
    let x = xn * fcube(fx) / 100.0;
    let y = yn * fcube(fy) / 100.0;
    let z = zn * fcube(fz) / 100.0;
    let r = 3.2404542 * x - 1.5371385 * y - 0.4985314 * z;
    let g = -0.9692660 * x + 1.8760108 * y + 0.0415560 * z;
    let bl = 0.0556434 * x - 0.2040259 * y + 1.0572252 * z;
    let to_srgb = |c: f64| {
        let c = if c > 0.0031308 {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        } else {
            12.92 * c
        };
        (c.clamp(0.0, 1.0) * 255.0).round()
    };
    arr_f64(vec![to_srgb(r), to_srgb(g), to_srgb(bl)])
}

pub fn rgb_to_oklch(args: &[StrykeValue]) -> StrykeValue {
    let xs = as_vec_f64(args.first().unwrap_or(&StrykeValue::UNDEF));
    if xs.len() < 3 {
        return arr_f64(vec![]);
    }
    let oklab = as_vec_f64(&rgb_to_oklab_internal(xs[0], xs[1], xs[2]));
    if oklab.len() < 3 {
        return arr_f64(vec![]);
    }
    let l = oklab[0];
    let a = oklab[1];
    let b = oklab[2];
    let c = (a * a + b * b).sqrt();
    let mut h = b.atan2(a).to_degrees();
    if h < 0.0 {
        h += 360.0;
    }
    arr_f64(vec![l, c, h])
}

fn rgb_to_oklab_internal(r: f64, g: f64, b: f64) -> StrykeValue {
    let to_linear = |c: f64| {
        let c = c / 255.0;
        if c > 0.04045 {
            ((c + 0.055) / 1.055).powf(2.4)
        } else {
            c / 12.92
        }
    };
    let r = to_linear(r);
    let g = to_linear(g);
    let b = to_linear(b);
    let l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
    let m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
    let s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();
    arr_f64(vec![
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    ])
}

pub fn oklch_to_rgb(args: &[StrykeValue]) -> StrykeValue {
    let xs = as_vec_f64(args.first().unwrap_or(&StrykeValue::UNDEF));
    if xs.len() < 3 {
        return arr_f64(vec![]);
    }
    let l = xs[0];
    let c = xs[1];
    let h = xs[2].to_radians();
    let a = c * h.cos();
    let b = c * h.sin();
    oklab_to_rgb_internal(l, a, b)
}

fn oklab_to_rgb_internal(l: f64, a: f64, b: f64) -> StrykeValue {
    let l_ = l + 0.3963377774 * a + 0.2158037573 * b;
    let m_ = l - 0.1055613458 * a - 0.0638541728 * b;
    let s_ = l - 0.0894841775 * a - 1.2914855480 * b;
    let l = l_.powi(3);
    let m = m_.powi(3);
    let s = s_.powi(3);
    let r = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
    let g = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
    let bl = -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s;
    let to_srgb = |c: f64| {
        let c = if c > 0.0031308 {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        } else {
            12.92 * c
        };
        (c.clamp(0.0, 1.0) * 255.0).round()
    };
    arr_f64(vec![to_srgb(r), to_srgb(g), to_srgb(bl)])
}

pub fn ciede76_color_distance(args: &[StrykeValue]) -> StrykeValue {
    let a = as_vec_f64(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = as_vec_f64(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    if a.len() < 3 || b.len() < 3 {
        return StrykeValue::float(0.0);
    }
    let (l1, a1, b1) = rgb_to_lab_inner(a[0], a[1], a[2]);
    let (l2, a2, b2) = rgb_to_lab_inner(b[0], b[1], b[2]);
    StrykeValue::float(((l1 - l2).powi(2) + (a1 - a2).powi(2) + (b1 - b2).powi(2)).sqrt())
}

pub fn ciede94_color_distance(args: &[StrykeValue]) -> StrykeValue {
    let a = as_vec_f64(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = as_vec_f64(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    if a.len() < 3 || b.len() < 3 {
        return StrykeValue::float(0.0);
    }
    let (l1, a1, b1) = rgb_to_lab_inner(a[0], a[1], a[2]);
    let (l2, a2, b2) = rgb_to_lab_inner(b[0], b[1], b[2]);
    let c1 = (a1 * a1 + b1 * b1).sqrt();
    let c2 = (a2 * a2 + b2 * b2).sqrt();
    let dl = l1 - l2;
    let dc = c1 - c2;
    let da = a1 - a2;
    let db = b1 - b2;
    let dh2 = (da * da + db * db - dc * dc).max(0.0);
    let kl = 1.0;
    let k1 = 0.045;
    let k2 = 0.015;
    let sl = 1.0;
    let sc = 1.0 + k1 * c1;
    let sh = 1.0 + k2 * c1;
    let total = (dl / (kl * sl)).powi(2) + (dc / sc).powi(2) + dh2 / (sh * sh);
    StrykeValue::float(total.sqrt())
}

pub fn ciede2000_color_distance(args: &[StrykeValue]) -> StrykeValue {
    // Full CIEDE2000 ΔE color distance per Sharma/Wu/Dalal 2005.
    // Includes mean hue, T factor, R_T rotation, all weighting terms.
    let a = as_vec_f64(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = as_vec_f64(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    if a.len() < 3 || b.len() < 3 {
        return StrykeValue::float(0.0);
    }
    let (l1, a1, b1) = rgb_to_lab_inner(a[0], a[1], a[2]);
    let (l2, a2, b2) = rgb_to_lab_inner(b[0], b[1], b[2]);
    let c1 = (a1 * a1 + b1 * b1).sqrt();
    let c2 = (a2 * a2 + b2 * b2).sqrt();
    let c_bar = (c1 + c2) / 2.0;
    let pow7_c = c_bar.powi(7);
    let pow7_25 = 25f64.powi(7);
    let g = 0.5 * (1.0 - (pow7_c / (pow7_c + pow7_25)).sqrt());
    let a1p = (1.0 + g) * a1;
    let a2p = (1.0 + g) * a2;
    let c1p = (a1p * a1p + b1 * b1).sqrt();
    let c2p = (a2p * a2p + b2 * b2).sqrt();
    let h1p = if a1p == 0.0 && b1 == 0.0 { 0.0 } else { b1.atan2(a1p).to_degrees().rem_euclid(360.0) };
    let h2p = if a2p == 0.0 && b2 == 0.0 { 0.0 } else { b2.atan2(a2p).to_degrees().rem_euclid(360.0) };
    let dl = l2 - l1;
    let dcp = c2p - c1p;
    let dhp = if c1p * c2p == 0.0 {
        0.0
    } else if (h2p - h1p).abs() <= 180.0 {
        h2p - h1p
    } else if h2p - h1p > 180.0 {
        h2p - h1p - 360.0
    } else {
        h2p - h1p + 360.0
    };
    let dh_big = 2.0 * (c1p * c2p).sqrt() * (dhp.to_radians() / 2.0).sin();
    let l_bar = (l1 + l2) / 2.0;
    let c_bar_p = (c1p + c2p) / 2.0;
    let h_bar_p = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let t = 1.0
        - 0.17 * (h_bar_p - 30.0).to_radians().cos()
        + 0.24 * (2.0 * h_bar_p).to_radians().cos()
        + 0.32 * (3.0 * h_bar_p + 6.0).to_radians().cos()
        - 0.20 * (4.0 * h_bar_p - 63.0).to_radians().cos();
    let delta_theta_deg = 30.0 * (-((h_bar_p - 275.0) / 25.0).powi(2)).exp();
    let pow7_cbp = c_bar_p.powi(7);
    let r_c = 2.0 * (pow7_cbp / (pow7_cbp + pow7_25)).sqrt();
    let r_t = -(2.0 * delta_theta_deg).to_radians().sin() * r_c;
    let sl = 1.0 + 0.015 * (l_bar - 50.0).powi(2) / (20.0 + (l_bar - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * c_bar_p;
    let sh = 1.0 + 0.015 * c_bar_p * t;
    let tl = dl / sl;
    let tc = dcp / sc;
    let th = dh_big / sh;
    StrykeValue::float((tl * tl + tc * tc + th * th + r_t * tc * th).sqrt())
}

// ══════════════════════════════════════════════════════════════════════
// Window functions
// ══════════════════════════════════════════════════════════════════════

pub fn window_blackman_harris(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(64).max(1) as usize;
    let nf = (n - 1) as f64;
    arr_f64(
        (0..n)
            .map(|i| {
                let x = 2.0 * std::f64::consts::PI * i as f64 / nf;
                0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos()
            })
            .collect(),
    )
}

pub fn window_gaussian(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(64).max(1) as usize;
    let sigma = arg_f64(args, 1).unwrap_or(0.4);
    let half = (n - 1) as f64 / 2.0;
    arr_f64(
        (0..n)
            .map(|i| {
                let x = (i as f64 - half) / (sigma * half);
                (-0.5 * x * x).exp()
            })
            .collect(),
    )
}

pub fn window_flat_top(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(64).max(1) as usize;
    let nf = (n - 1) as f64;
    arr_f64(
        (0..n)
            .map(|i| {
                let x = 2.0 * std::f64::consts::PI * i as f64 / nf;
                0.21557895 - 0.41663158 * x.cos() + 0.277263158 * (2.0 * x).cos()
                    - 0.083578947 * (3.0 * x).cos()
                    + 0.006947368 * (4.0 * x).cos()
            })
            .collect(),
    )
}

pub fn window_bartlett(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(64).max(1) as usize;
    let nf = (n - 1) as f64;
    arr_f64(
        (0..n)
            .map(|i| 1.0 - ((i as f64 - nf / 2.0) / (nf / 2.0)).abs())
            .collect(),
    )
}

pub fn window_welch(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(64).max(1) as usize;
    let nf = (n - 1) as f64;
    arr_f64(
        (0..n)
            .map(|i| 1.0 - ((i as f64 - nf / 2.0) / (nf / 2.0)).powi(2))
            .collect(),
    )
}

// ══════════════════════════════════════════════════════════════════════
// Trie / Fenwick tree / Union-Find
// ══════════════════════════════════════════════════════════════════════

pub fn trie_new(_args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let h: IndexMap<String, StrykeValue> = IndexMap::new();
    make_hash(vec![
        ("end", StrykeValue::integer(0)),
        ("children", StrykeValue::hash_ref(Arc::new(RwLock::new(h)))),
    ])
}

pub fn trie_insert(args: &[StrykeValue]) -> StrykeValue {
    let trie = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let word = arg_str(args, 1).unwrap_or_default();
    if let Some(t) = trie.as_hash_ref() {
        let node = t.write();
        let mut cur_children = node.get("children").cloned();
        drop(node);
        let mut cur_ref = trie.clone();
        for c in word.chars() {
            let key = c.to_string();
            let children_v = cur_children.clone().unwrap_or(StrykeValue::UNDEF);
            if let Some(ch) = children_v.as_hash_ref() {
                let needs_create = !ch.read().contains_key(&key);
                if needs_create {
                    let new_node = trie_new(&[]);
                    ch.write().insert(key.clone(), new_node);
                }
                let child = ch.read().get(&key).cloned().unwrap_or(StrykeValue::UNDEF);
                cur_children = child.as_hash_ref().and_then(|c| c.read().get("children").cloned());
                cur_ref = child;
            }
        }
        if let Some(n) = cur_ref.as_hash_ref() {
            n.write().insert("end".to_string(), StrykeValue::integer(1));
        }
    }
    trie
}

pub fn trie_lookup(args: &[StrykeValue]) -> StrykeValue {
    let trie = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let word = arg_str(args, 1).unwrap_or_default();
    let mut cur = trie;
    for c in word.chars() {
        let key = c.to_string();
        let next = cur
            .as_hash_ref()
            .and_then(|n| n.read().get("children").cloned())
            .and_then(|ch| ch.as_hash_ref().and_then(|c| c.read().get(&key).cloned()));
        match next {
            Some(v) => cur = v,
            None => return StrykeValue::integer(0),
        }
    }
    let end = cur
        .as_hash_ref()
        .and_then(|n| n.read().get("end").map(|v| v.to_int()))
        .unwrap_or(0);
    StrykeValue::integer(end)
}

pub fn trie_prefix_search(args: &[StrykeValue]) -> StrykeValue {
    let trie = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let prefix = arg_str(args, 1).unwrap_or_default();
    let mut cur = trie;
    for c in prefix.chars() {
        let key = c.to_string();
        let next = cur
            .as_hash_ref()
            .and_then(|n| n.read().get("children").cloned())
            .and_then(|ch| ch.as_hash_ref().and_then(|c| c.read().get(&key).cloned()));
        match next {
            Some(v) => cur = v,
            None => return arr_sv(vec![]),
        }
    }
    let mut results = Vec::new();
    fn walk(node: &StrykeValue, path: String, results: &mut Vec<StrykeValue>) {
        if let Some(n) = node.as_hash_ref() {
            let n_read = n.read();
            let is_end = n_read.get("end").map(|v| v.to_int()).unwrap_or(0);
            if is_end == 1 {
                results.push(StrykeValue::string(path.clone()));
            }
            if let Some(ch) = n_read.get("children").cloned() {
                drop(n_read);
                if let Some(c) = ch.as_hash_ref() {
                    let entries: Vec<(String, StrykeValue)> =
                        c.read().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                    for (k, child) in entries {
                        walk(&child, format!("{path}{k}"), results);
                    }
                }
            }
        }
    }
    walk(&cur, prefix, &mut results);
    arr_sv(results)
}

pub fn trie_remove(args: &[StrykeValue]) -> StrykeValue {
    let trie = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let word = arg_str(args, 1).unwrap_or_default();
    let mut cur = trie.clone();
    for c in word.chars() {
        let key = c.to_string();
        let next = cur
            .as_hash_ref()
            .and_then(|n| n.read().get("children").cloned())
            .and_then(|ch| ch.as_hash_ref().and_then(|c| c.read().get(&key).cloned()));
        match next {
            Some(v) => cur = v,
            None => return trie,
        }
    }
    if let Some(n) = cur.as_hash_ref() {
        n.write().insert("end".to_string(), StrykeValue::integer(0));
    }
    trie
}

pub fn trie_keys(args: &[StrykeValue]) -> StrykeValue {
    trie_prefix_search(&[args.first().cloned().unwrap_or(StrykeValue::UNDEF), StrykeValue::string(String::new())])
}

pub fn trie_count(args: &[StrykeValue]) -> StrykeValue {
    let keys_v = trie_keys(args);
    StrykeValue::integer(as_vec_sv(&keys_v).len() as i64)
}

pub fn fenwick_new(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    arr_sv(vec![StrykeValue::integer(0); n + 1])
}

pub fn fenwick_update(args: &[StrykeValue]) -> StrykeValue {
    let tree = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let mut idx = arg_i64(args, 1).unwrap_or(0).max(1) as usize;
    let delta = arg_i64(args, 2).unwrap_or(0);
    if let Some(t) = tree.as_array_ref() {
        let n = t.read().len();
        while idx < n {
            let cur = t.read()[idx].to_int();
            t.write()[idx] = StrykeValue::integer(cur + delta);
            idx += idx & idx.wrapping_neg();
        }
    }
    tree
}

pub fn fenwick_query_prefix(args: &[StrykeValue]) -> StrykeValue {
    let tree = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let mut idx = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let mut sum = 0_i64;
    if let Some(t) = tree.as_array_ref() {
        let t_read = t.read();
        while idx > 0 {
            sum += t_read[idx].to_int();
            idx -= idx & idx.wrapping_neg();
        }
    }
    StrykeValue::integer(sum)
}

pub fn fenwick_query_range(args: &[StrykeValue]) -> StrykeValue {
    let tree = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let l = arg_i64(args, 1).unwrap_or(1).max(1);
    let r = arg_i64(args, 2).unwrap_or(1);
    let p_r = fenwick_query_prefix(&[tree.clone(), StrykeValue::integer(r)]).to_int();
    let p_l = fenwick_query_prefix(&[tree, StrykeValue::integer(l - 1)]).to_int();
    StrykeValue::integer(p_r - p_l)
}

pub fn union_find_new(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let parent: Vec<StrykeValue> = (0..n).map(|i| StrykeValue::integer(i as i64)).collect();
    let rank: Vec<StrykeValue> = (0..n).map(|_| StrykeValue::integer(0)).collect();
    make_hash(vec![("parent", arr_sv(parent)), ("rank", arr_sv(rank))])
}

fn uf_find(parent: &mut Vec<i64>, x: usize) -> usize {
    if parent[x] != x as i64 {
        let r = uf_find(parent, parent[x] as usize);
        parent[x] = r as i64;
    }
    parent[x] as usize
}

pub fn union_find_find(args: &[StrykeValue]) -> StrykeValue {
    let uf = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let x = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    if let Some(h) = uf.as_hash_ref() {
        let h = h.read();
        if let Some(p_sv) = h.get("parent") {
            let mut p: Vec<i64> = as_vec_sv(p_sv).iter().map(|v| v.to_int()).collect();
            if x < p.len() {
                let r = uf_find(&mut p, x);
                return StrykeValue::integer(r as i64);
            }
        }
    }
    StrykeValue::integer(-1)
}

pub fn union_find_union(args: &[StrykeValue]) -> StrykeValue {
    let uf = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let x = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let y = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    if let Some(h) = uf.as_hash_ref() {
        let p_sv = h.read().get("parent").cloned();
        let r_sv = h.read().get("rank").cloned();
        if let (Some(p_sv), Some(r_sv)) = (p_sv, r_sv) {
            if let (Some(p_ref), Some(r_ref)) = (p_sv.as_array_ref(), r_sv.as_array_ref()) {
                let mut p: Vec<i64> = p_ref.read().iter().map(|v| v.to_int()).collect();
                let mut r: Vec<i64> = r_ref.read().iter().map(|v| v.to_int()).collect();
                let rx = uf_find(&mut p, x);
                let ry = uf_find(&mut p, y);
                if rx != ry {
                    if r[rx] < r[ry] {
                        p[rx] = ry as i64;
                    } else if r[rx] > r[ry] {
                        p[ry] = rx as i64;
                    } else {
                        p[ry] = rx as i64;
                        r[rx] += 1;
                    }
                }
                *p_ref.write() = p.into_iter().map(StrykeValue::integer).collect();
                *r_ref.write() = r.into_iter().map(StrykeValue::integer).collect();
            }
        }
    }
    uf
}

pub fn union_find_components(args: &[StrykeValue]) -> StrykeValue {
    let uf = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    if let Some(h) = uf.as_hash_ref() {
        let p_sv = h.read().get("parent").cloned();
        if let Some(p_sv) = p_sv {
            let mut p: Vec<i64> = as_vec_sv(&p_sv).iter().map(|v| v.to_int()).collect();
            let n = p.len();
            let roots: std::collections::HashSet<usize> = (0..n).map(|i| uf_find(&mut p, i)).collect();
            return StrykeValue::integer(roots.len() as i64);
        }
    }
    StrykeValue::integer(0)
}

// ══════════════════════════════════════════════════════════════════════
// Network / IP extras
// ══════════════════════════════════════════════════════════════════════

pub fn ip_subnet_split(args: &[StrykeValue]) -> StrykeValue {
    let cidr = arg_str(args, 0).unwrap_or_default();
    let parts = cidr.split('/').collect::<Vec<_>>();
    if parts.len() != 2 {
        return arr_sv(vec![]);
    }
    let prefix: u32 = parts[1].parse().unwrap_or(32);
    let new_prefix = arg_i64(args, 1).unwrap_or((prefix + 1) as i64) as u32;
    if new_prefix <= prefix || new_prefix > 32 {
        return arr_sv(vec![]);
    }
    let ip_parts: Vec<u32> = parts[0].split('.').filter_map(|p| p.parse().ok()).collect();
    if ip_parts.len() != 4 {
        return arr_sv(vec![]);
    }
    let base = (ip_parts[0] << 24) | (ip_parts[1] << 16) | (ip_parts[2] << 8) | ip_parts[3];
    let count = 1u32 << (new_prefix - prefix);
    let block_size = 1u32 << (32 - new_prefix);
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let addr = base + i * block_size;
        out.push(StrykeValue::string(format!(
            "{}.{}.{}.{}/{}",
            (addr >> 24) & 0xFF,
            (addr >> 16) & 0xFF,
            (addr >> 8) & 0xFF,
            addr & 0xFF,
            new_prefix
        )));
    }
    arr_sv(out)
}

pub fn ipv6_global_unicast(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let first_bytes: String = s.chars().take(4).collect();
    let prefix: u16 = u16::from_str_radix(&first_bytes, 16).unwrap_or(0);
    // 2000::/3 is global unicast — first 3 bits are 001
    let is_global = (prefix >> 13) == 0b001;
    StrykeValue::integer(if is_global { 1 } else { 0 })
}

pub fn cidr_to_range(args: &[StrykeValue]) -> StrykeValue {
    let cidr = arg_str(args, 0).unwrap_or_default();
    let parts = cidr.split('/').collect::<Vec<_>>();
    if parts.len() != 2 {
        return arr_sv(vec![]);
    }
    let prefix: u32 = parts[1].parse().unwrap_or(32);
    let ip_parts: Vec<u32> = parts[0].split('.').filter_map(|p| p.parse().ok()).collect();
    if ip_parts.len() != 4 {
        return arr_sv(vec![]);
    }
    let base = (ip_parts[0] << 24) | (ip_parts[1] << 16) | (ip_parts[2] << 8) | ip_parts[3];
    let mask = if prefix == 0 { 0 } else { !0u32 << (32 - prefix) };
    let net = base & mask;
    let bcast = net | !mask;
    let fmt = |a: u32| format!("{}.{}.{}.{}", (a >> 24) & 0xFF, (a >> 16) & 0xFF, (a >> 8) & 0xFF, a & 0xFF);
    arr_sv(vec![StrykeValue::string(fmt(net)), StrykeValue::string(fmt(bcast))])
}

pub fn range_to_cidr(args: &[StrykeValue]) -> StrykeValue {
    let start = arg_str(args, 0).unwrap_or_default();
    let end = arg_str(args, 1).unwrap_or_default();
    let parse_ip = |s: &str| -> Option<u32> {
        let parts: Vec<u32> = s.split('.').filter_map(|p| p.parse().ok()).collect();
        if parts.len() != 4 {
            return None;
        }
        Some((parts[0] << 24) | (parts[1] << 16) | (parts[2] << 8) | parts[3])
    };
    let mut s = match parse_ip(&start) {
        Some(v) => v,
        None => return arr_sv(vec![]),
    };
    let e = match parse_ip(&end) {
        Some(v) => v,
        None => return arr_sv(vec![]),
    };
    let mut out = Vec::new();
    let fmt = |a: u32| format!("{}.{}.{}.{}", (a >> 24) & 0xFF, (a >> 16) & 0xFF, (a >> 8) & 0xFF, a & 0xFF);
    while s <= e {
        let mut max_size = 32 - if s == 0 { 32 } else { s.trailing_zeros() };
        let range_size = ((e - s + 1) as f64).log2().floor() as u32;
        if max_size > range_size {
            max_size = range_size;
        }
        let size = 32 - max_size;
        out.push(StrykeValue::string(format!("{}/{}", fmt(s), size)));
        let count = 1u32 << max_size;
        if let Some(n) = s.checked_add(count) {
            s = n;
            if s == 0 {
                break;
            }
        } else {
            break;
        }
    }
    arr_sv(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sv(x: f64) -> StrykeValue {
        StrykeValue::float(x)
    }
    fn sv_i(x: i64) -> StrykeValue {
        StrykeValue::integer(x)
    }
    fn sv_s(x: &str) -> StrykeValue {
        StrykeValue::string(x.to_string())
    }

    #[test]
    fn conv1d_identity() {
        let r = conv1d_apply(&[arr_f64(vec![1.0, 2.0, 3.0, 4.0]), arr_f64(vec![1.0])]);
        let xs = as_vec_f64(&r);
        assert_eq!(xs, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn gaussian_kernel_sums_to_one() {
        let g = gaussian_kernel(&[sv_i(5), sv(1.0)]);
        let m = as_matrix(&g);
        let total: f64 = m.iter().flat_map(|r| r.iter()).sum();
        assert!((total - 1.0).abs() < 1e-9);
    }

    #[test]
    fn integral_image_corner() {
        let img = matrix_to_sv(&[vec![1.0, 1.0, 1.0], vec![1.0, 1.0, 1.0], vec![1.0, 1.0, 1.0]]);
        let r = integral_image(&[img]);
        let m = as_matrix(&r);
        assert_eq!(m[2][2], 9.0);
    }

    #[test]
    fn jaccard_basic() {
        let a = arr_sv(vec![sv_s("a"), sv_s("b"), sv_s("c")]);
        let b = arr_sv(vec![sv_s("b"), sv_s("c"), sv_s("d")]);
        let r = jaccard_sim(&[a, b]).to_number();
        assert!((r - 0.5).abs() < 1e-9);
    }

    #[test]
    fn cosine_sim_basic() {
        let a = arr_sv(vec![arr_sv(vec![
            StrykeValue::integer(0),
            StrykeValue::float(1.0),
        ])]);
        let b = arr_sv(vec![arr_sv(vec![
            StrykeValue::integer(0),
            StrykeValue::float(1.0),
        ])]);
        let r = cosine_sim_sparse(&[a, b]).to_number();
        assert!((r - 1.0).abs() < 1e-9);
    }

    #[test]
    fn manhattan_norm_basic() {
        let r = manhattan_norm(&[arr_f64(vec![1.0, -2.0, 3.0])]).to_number();
        assert_eq!(r, 6.0);
    }

    #[test]
    fn bayesian_beta_update_basic() {
        let r = bayesian_beta_update(&[sv(1.0), sv(1.0), sv(5.0), sv(3.0)]);
        if let Some(h) = r.as_hash_ref() {
            let h = h.read();
            assert_eq!(h.get("alpha").unwrap().to_number(), 6.0);
            assert_eq!(h.get("beta").unwrap().to_number(), 4.0);
        }
    }

    #[test]
    fn qlearning_step_check() {
        // q + alpha * (r + gamma * max_next - q) = 0 + 0.1 * (10 + 0.9*5 - 0) = 1.45
        let r = qlearning_step(&[sv(0.0), sv(0.1), sv(10.0), sv(0.9), sv(5.0)]).to_number();
        assert!((r - 1.45).abs() < 1e-9);
    }

    #[test]
    fn ucb1_unvisited_first() {
        let q = arr_f64(vec![1.0, 2.0, 0.0]);
        let counts = arr_f64(vec![10.0, 10.0, 0.0]);
        let r = ucb1_choose(&[q, counts]).to_int();
        assert_eq!(r, 2);
    }

    #[test]
    fn window_bartlett_endpoints_zero() {
        let r = window_bartlett(&[sv_i(11)]);
        let xs = as_vec_f64(&r);
        assert_eq!(xs[0], 0.0);
        assert!((xs[5] - 1.0).abs() < 1e-9);
        assert_eq!(xs[10], 0.0);
    }

    #[test]
    fn trie_insert_lookup() {
        let t = trie_new(&[]);
        let t = trie_insert(&[t, sv_s("hello")]);
        let r = trie_lookup(&[t.clone(), sv_s("hello")]).to_int();
        assert_eq!(r, 1);
        let r2 = trie_lookup(&[t, sv_s("hellz")]).to_int();
        assert_eq!(r2, 0);
    }

    #[test]
    fn fenwick_basic() {
        let f = fenwick_new(&[sv_i(5)]);
        let f = fenwick_update(&[f, sv_i(1), sv_i(3)]);
        let f = fenwick_update(&[f, sv_i(3), sv_i(7)]);
        let r = fenwick_query_prefix(&[f.clone(), sv_i(3)]).to_int();
        assert_eq!(r, 10);
        let r2 = fenwick_query_range(&[f, sv_i(2), sv_i(3)]).to_int();
        assert_eq!(r2, 7);
    }

    #[test]
    fn union_find_basic() {
        let uf = union_find_new(&[sv_i(5)]);
        let uf = union_find_union(&[uf, sv_i(0), sv_i(1)]);
        let uf = union_find_union(&[uf, sv_i(2), sv_i(3)]);
        let comp = union_find_components(&[uf]).to_int();
        assert_eq!(comp, 3);
    }

    #[test]
    fn cidr_to_range_basic() {
        let r = cidr_to_range(&[sv_s("192.168.1.0/24")]);
        let xs = as_vec_sv(&r);
        assert_eq!(xs[0].as_str_or_empty(), "192.168.1.0");
        assert_eq!(xs[1].as_str_or_empty(), "192.168.1.255");
    }

    #[test]
    fn ip_subnet_split_basic() {
        let r = ip_subnet_split(&[sv_s("10.0.0.0/24"), sv_i(26)]);
        let xs = as_vec_sv(&r);
        assert_eq!(xs.len(), 4);
        assert_eq!(xs[0].as_str_or_empty(), "10.0.0.0/26");
        assert_eq!(xs[3].as_str_or_empty(), "10.0.0.192/26");
    }

    #[test]
    fn rgb_to_oklch_roundtrip() {
        let rgb = arr_f64(vec![128.0, 64.0, 200.0]);
        let oklch = rgb_to_oklch(&[rgb]);
        let back = oklch_to_rgb(&[oklch]);
        let xs = as_vec_f64(&back);
        assert!((xs[0] - 128.0).abs() < 2.0);
        assert!((xs[1] - 64.0).abs() < 2.0);
        assert!((xs[2] - 200.0).abs() < 2.0);
    }
}
