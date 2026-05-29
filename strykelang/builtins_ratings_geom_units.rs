//! Ratings/tournaments, image morphology, computational geometry 2D,
//! crypto primitives, physics constants, case conversions, photography,
//! unit conversions.

use crate::value::StrykeValue;
use parking_lot::RwLock;
use std::sync::Arc;

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
// Ratings / tournaments
// ══════════════════════════════════════════════════════════════════════
/// `glicko_rd_update` — see implementation.

pub fn glicko_rd_update(args: &[StrykeValue]) -> StrykeValue {
    let rd = arg_f64(args, 0).unwrap_or(350.0);
    let c = arg_f64(args, 1).unwrap_or(34.6);
    let t = arg_f64(args, 2).unwrap_or(1.0);
    let new_rd = (rd * rd + c * c * t).sqrt().min(350.0);
    StrykeValue::float(new_rd)
}
/// `glicko_volatility` — see implementation.

pub fn glicko_volatility(args: &[StrykeValue]) -> StrykeValue {
    // Approximate Glicko-2 volatility update — single iteration.
    let sigma = arg_f64(args, 0).unwrap_or(0.06);
    let delta = arg_f64(args, 1).unwrap_or(0.0);
    let phi = arg_f64(args, 2).unwrap_or(0.5);
    let v = arg_f64(args, 3).unwrap_or(1.0);
    let tau = arg_f64(args, 4).unwrap_or(0.5);
    let a = (sigma * sigma).ln();
    let f = |x: f64| {
        let ex = x.exp();
        let num = ex * (delta * delta - phi * phi - v - ex);
        let denom = 2.0 * (phi * phi + v + ex).powi(2);
        num / denom - (x - a) / (tau * tau)
    };
    let mut x_a = a;
    let mut x_b = if delta * delta > phi * phi + v {
        (delta * delta - phi * phi - v).ln()
    } else {
        let mut k = 1.0;
        while f(a - k * tau) < 0.0 {
            k += 1.0;
        }
        a - k * tau
    };
    let mut fa = f(x_a);
    let mut fb = f(x_b);
    for _ in 0..100 {
        let x_c = x_a + (x_a - x_b) * fa / (fb - fa);
        let fc = f(x_c);
        if fc * fb <= 0.0 {
            x_a = x_b;
            fa = fb;
        } else {
            fa /= 2.0;
        }
        x_b = x_c;
        fb = fc;
        if (x_b - x_a).abs() < 1e-6 {
            break;
        }
    }
    StrykeValue::float((x_a / 2.0).exp())
}
/// `trueskill_simple` — see implementation.

pub fn trueskill_simple(args: &[StrykeValue]) -> StrykeValue {
    // 1v1 TrueSkill update via the Gaussian belief-propagation formulas
    // (v/w functions). Multi-team / team-vs-team is not handled.
    let mu_w = arg_f64(args, 0).unwrap_or(25.0);
    let sigma_w = arg_f64(args, 1).unwrap_or(25.0 / 3.0);
    let mu_l = arg_f64(args, 2).unwrap_or(25.0);
    let sigma_l = arg_f64(args, 3).unwrap_or(25.0 / 3.0);
    let beta = 25.0 / 6.0;
    let c2 = 2.0 * beta * beta + sigma_w * sigma_w + sigma_l * sigma_l;
    let c = c2.sqrt();
    use statrs::distribution::{ContinuousCDF, Normal};
    let n = Normal::new(0.0, 1.0).unwrap();
    let t = (mu_w - mu_l) / c;
    let v_fn = |t: f64| {
        let pdf = (-0.5 * t * t).exp() / (2.0 * std::f64::consts::PI).sqrt();
        let cdf = n.cdf(t);
        if cdf > 1e-12 {
            pdf / cdf
        } else {
            -t
        }
    };
    let w_fn = |t: f64| {
        let v = v_fn(t);
        v * (v + t)
    };
    let v = v_fn(t);
    let w = w_fn(t);
    let new_mu_w = mu_w + (sigma_w * sigma_w / c) * v;
    let new_mu_l = mu_l - (sigma_l * sigma_l / c) * v;
    let new_sigma_w = (sigma_w * sigma_w * (1.0 - sigma_w * sigma_w / c2 * w)).sqrt();
    let new_sigma_l = (sigma_l * sigma_l * (1.0 - sigma_l * sigma_l / c2 * w)).sqrt();
    make_hash(vec![
        ("winner_mu", StrykeValue::float(new_mu_w)),
        ("winner_sigma", StrykeValue::float(new_sigma_w)),
        ("loser_mu", StrykeValue::float(new_mu_l)),
        ("loser_sigma", StrykeValue::float(new_sigma_l)),
    ])
}
/// `pagerank_tournament` — see implementation.

pub fn pagerank_tournament(args: &[StrykeValue]) -> StrykeValue {
    // Treat wins matrix as transition probabilities, compute PageRank-style scores.
    let wins = args.first().map(as_matrix).unwrap_or_default();
    let damp = arg_f64(args, 1).unwrap_or(0.85);
    let iters = arg_i64(args, 2).unwrap_or(50).max(1) as usize;
    let n = wins.len();
    if n == 0 {
        return arr_f64(vec![]);
    }
    let row_sums: Vec<f64> = wins
        .iter()
        .map(|r| r.iter().sum::<f64>().max(1e-12))
        .collect();
    let mut pr = vec![1.0 / n as f64; n];
    for _ in 0..iters {
        let mut next = vec![(1.0 - damp) / n as f64; n];
        for i in 0..n {
            for j in 0..n {
                next[j] += damp * pr[i] * wins[i].get(j).copied().unwrap_or(0.0) / row_sums[i];
            }
        }
        pr = next;
    }
    arr_f64(pr)
}
/// `swiss_pairing` — see implementation.

pub fn swiss_pairing(args: &[StrykeValue]) -> StrykeValue {
    // Pair players sorted by score using greedy swiss (no rematches).
    let scores = args.first().map(as_vec_f64).unwrap_or_default();
    let played: Vec<Vec<f64>> = args.get(1).map(as_matrix).unwrap_or_default();
    let n = scores.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        scores[b]
            .partial_cmp(&scores[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut paired = vec![false; n];
    let mut out = Vec::new();
    for i in 0..n {
        if paired[order[i]] {
            continue;
        }
        let mut found = false;
        for j in i + 1..n {
            let a = order[i];
            let b = order[j];
            if paired[b] {
                continue;
            }
            let has_played = played.get(a).and_then(|r| r.get(b)).copied().unwrap_or(0.0) > 0.0;
            if has_played {
                continue;
            }
            out.push(arr_sv(vec![
                StrykeValue::integer(a as i64),
                StrykeValue::integer(b as i64),
            ]));
            paired[a] = true;
            paired[b] = true;
            found = true;
            break;
        }
        if !found {
            paired[order[i]] = true;
        }
    }
    arr_sv(out)
}
/// `arpad_predict` — see implementation.

pub fn arpad_predict(args: &[StrykeValue]) -> StrykeValue {
    let r_a = arg_f64(args, 0).unwrap_or(1500.0);
    let r_b = arg_f64(args, 1).unwrap_or(1500.0);
    StrykeValue::float(1.0 / (1.0 + 10f64.powf((r_b - r_a) / 400.0)))
}
/// `tournament_score` — see implementation.

pub fn tournament_score(args: &[StrykeValue]) -> StrykeValue {
    let wins = arg_f64(args, 0).unwrap_or(0.0);
    let draws = arg_f64(args, 1).unwrap_or(0.0);
    let losses = arg_f64(args, 2).unwrap_or(0.0);
    let _ = losses;
    StrykeValue::float(wins + 0.5 * draws)
}
/// `ranking_kendall_tau` — see implementation.

pub fn ranking_kendall_tau(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec_f64).unwrap_or_default();
    let b = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = a.len().min(b.len());
    if n < 2 {
        return StrykeValue::float(0.0);
    }
    let mut concordant = 0_i64;
    let mut discordant = 0_i64;
    for i in 0..n - 1 {
        for j in i + 1..n {
            let sign_a = (a[j] - a[i]).signum();
            let sign_b = (b[j] - b[i]).signum();
            if sign_a == sign_b && sign_a != 0.0 {
                concordant += 1;
            } else if sign_a == -sign_b && sign_a != 0.0 {
                discordant += 1;
            }
        }
    }
    let total = (n * (n - 1) / 2) as f64;
    StrykeValue::float((concordant - discordant) as f64 / total)
}
/// `ranking_spearman_rho` — see implementation.

pub fn ranking_spearman_rho(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec_f64).unwrap_or_default();
    let b = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = a.len().min(b.len());
    if n < 2 {
        return StrykeValue::float(0.0);
    }
    fn rank(xs: &[f64]) -> Vec<f64> {
        let mut idx: Vec<usize> = (0..xs.len()).collect();
        idx.sort_by(|&i, &j| {
            xs[i]
                .partial_cmp(&xs[j])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mut r = vec![0.0; xs.len()];
        for (rank, &i) in idx.iter().enumerate() {
            r[i] = (rank + 1) as f64;
        }
        r
    }
    let ra = rank(&a);
    let rb = rank(&b);
    let mean_a = ra.iter().sum::<f64>() / n as f64;
    let mean_b = rb.iter().sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut da = 0.0;
    let mut db = 0.0;
    for i in 0..n {
        num += (ra[i] - mean_a) * (rb[i] - mean_b);
        da += (ra[i] - mean_a).powi(2);
        db += (rb[i] - mean_b).powi(2);
    }
    let denom = (da * db).sqrt();
    if denom < 1e-12 {
        return StrykeValue::float(0.0);
    }
    StrykeValue::float(num / denom)
}
/// `ranking_average` — see implementation.

pub fn ranking_average(args: &[StrykeValue]) -> StrykeValue {
    // Average rankings across multiple lists (Borda count style).
    let lists: Vec<Vec<f64>> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(as_vec_f64)
        .collect();
    if lists.is_empty() {
        return arr_f64(vec![]);
    }
    let n = lists[0].len();
    let mut sums = vec![0.0; n];
    for list in &lists {
        for (i, v) in list.iter().take(n).enumerate() {
            sums[i] += v;
        }
    }
    let avg: Vec<f64> = sums.iter().map(|s| s / lists.len() as f64).collect();
    arr_f64(avg)
}

// ══════════════════════════════════════════════════════════════════════
// Image morphology
// ══════════════════════════════════════════════════════════════════════

fn morph_op(img: &[Vec<f64>], size: usize, dilate: bool) -> Vec<Vec<f64>> {
    if img.is_empty() {
        return Vec::new();
    }
    let h = img.len();
    let w = img[0].len();
    let half = size / 2;
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            let mut acc = if dilate {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
            for di in 0..size {
                for dj in 0..size {
                    let y = i as isize + di as isize - half as isize;
                    let x = j as isize + dj as isize - half as isize;
                    let v = if y >= 0 && y < h as isize && x >= 0 && x < w as isize {
                        img[y as usize][x as usize]
                    } else {
                        0.0
                    };
                    if dilate {
                        acc = acc.max(v);
                    } else {
                        acc = acc.min(v);
                    }
                }
            }
            out[i][j] = if acc.is_finite() { acc } else { 0.0 };
        }
    }
    out
}
/// `erosion_2d` — see implementation.

pub fn erosion_2d(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let size = arg_i64(args, 1).unwrap_or(3).max(1) as usize;
    matrix_to_sv(&morph_op(&img, size, false))
}
/// `dilation_2d` — see implementation.

pub fn dilation_2d(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let size = arg_i64(args, 1).unwrap_or(3).max(1) as usize;
    matrix_to_sv(&morph_op(&img, size, true))
}
/// `opening_2d` — see implementation.

pub fn opening_2d(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let size = arg_i64(args, 1).unwrap_or(3).max(1) as usize;
    matrix_to_sv(&morph_op(&morph_op(&img, size, false), size, true))
}
/// `closing_2d` — see implementation.

pub fn closing_2d(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let size = arg_i64(args, 1).unwrap_or(3).max(1) as usize;
    matrix_to_sv(&morph_op(&morph_op(&img, size, true), size, false))
}
/// `morphological_gradient` — see implementation.

pub fn morphological_gradient(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let size = arg_i64(args, 1).unwrap_or(3).max(1) as usize;
    let d = morph_op(&img, size, true);
    let e = morph_op(&img, size, false);
    let h = img.len();
    let w = img.first().map(|r| r.len()).unwrap_or(0);
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            out[i][j] = d[i][j] - e[i][j];
        }
    }
    matrix_to_sv(&out)
}
/// `top_hat_transform` — see implementation.

pub fn top_hat_transform(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let size = arg_i64(args, 1).unwrap_or(3).max(1) as usize;
    let opened = morph_op(&morph_op(&img, size, false), size, true);
    let h = img.len();
    let w = img.first().map(|r| r.len()).unwrap_or(0);
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            out[i][j] = img[i][j] - opened[i][j];
        }
    }
    matrix_to_sv(&out)
}
/// `black_hat_transform` — see implementation.

pub fn black_hat_transform(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let size = arg_i64(args, 1).unwrap_or(3).max(1) as usize;
    let closed = morph_op(&morph_op(&img, size, true), size, false);
    let h = img.len();
    let w = img.first().map(|r| r.len()).unwrap_or(0);
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            out[i][j] = closed[i][j] - img[i][j];
        }
    }
    matrix_to_sv(&out)
}
/// `bilateral_filter_2d` — see implementation.

pub fn bilateral_filter_2d(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let size = arg_i64(args, 1).unwrap_or(5).max(3) as usize | 1;
    let sigma_s = arg_f64(args, 2).unwrap_or(2.0);
    let sigma_r = arg_f64(args, 3).unwrap_or(25.0);
    if img.is_empty() {
        return matrix_to_sv(&[]);
    }
    let h = img.len();
    let w = img[0].len();
    let half = size / 2;
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            let mut sum = 0.0_f64;
            let mut wsum = 0.0_f64;
            let ci = img[i][j];
            for di in 0..size {
                for dj in 0..size {
                    let y = i as isize + di as isize - half as isize;
                    let x = j as isize + dj as isize - half as isize;
                    if y >= 0 && y < h as isize && x >= 0 && x < w as isize {
                        let v = img[y as usize][x as usize];
                        let d_spatial = ((di as f64 - half as f64).powi(2)
                            + (dj as f64 - half as f64).powi(2))
                            / (2.0 * sigma_s * sigma_s);
                        let d_range = (v - ci).powi(2) / (2.0 * sigma_r * sigma_r);
                        let weight = (-d_spatial - d_range).exp();
                        sum += weight * v;
                        wsum += weight;
                    }
                }
            }
            out[i][j] = if wsum > 1e-12 { sum / wsum } else { ci };
        }
    }
    matrix_to_sv(&out)
}
/// `contour_find` — see implementation.

pub fn contour_find(args: &[StrykeValue]) -> StrykeValue {
    // Find boundary cells in a binary image.
    let img = args.first().map(as_matrix).unwrap_or_default();
    let thresh = arg_f64(args, 1).unwrap_or(128.0);
    let h = img.len();
    if h == 0 {
        return arr_sv(vec![]);
    }
    let w = img[0].len();
    let mut out = Vec::new();
    let dirs = [(-1isize, 0isize), (1, 0), (0, -1), (0, 1)];
    for i in 0..h {
        for j in 0..w {
            if img[i][j] >= thresh {
                let mut is_boundary = false;
                for (di, dj) in &dirs {
                    let y = i as isize + di;
                    let x = j as isize + dj;
                    if y < 0 || y >= h as isize || x < 0 || x >= w as isize {
                        is_boundary = true;
                        break;
                    }
                    if img[y as usize][x as usize] < thresh {
                        is_boundary = true;
                        break;
                    }
                }
                if is_boundary {
                    out.push(arr_sv(vec![
                        StrykeValue::integer(i as i64),
                        StrykeValue::integer(j as i64),
                    ]));
                }
            }
        }
    }
    arr_sv(out)
}
/// `contour_perimeter` — see implementation.

pub fn contour_perimeter(args: &[StrykeValue]) -> StrykeValue {
    let pts: Vec<(f64, f64)> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(|p| {
            let xs = as_vec_f64(p);
            (
                xs.first().copied().unwrap_or(0.0),
                xs.get(1).copied().unwrap_or(0.0),
            )
        })
        .collect();
    let n = pts.len();
    if n < 2 {
        return StrykeValue::float(0.0);
    }
    let mut perim = 0.0;
    for i in 0..n {
        let (x1, y1) = pts[i];
        let (x2, y2) = pts[(i + 1) % n];
        perim += ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
    }
    StrykeValue::float(perim)
}
/// `contour_area` — see implementation.

pub fn contour_area(args: &[StrykeValue]) -> StrykeValue {
    let pts: Vec<(f64, f64)> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(|p| {
            let xs = as_vec_f64(p);
            (
                xs.first().copied().unwrap_or(0.0),
                xs.get(1).copied().unwrap_or(0.0),
            )
        })
        .collect();
    let n = pts.len();
    if n < 3 {
        return StrykeValue::float(0.0);
    }
    let mut a = 0.0;
    for i in 0..n {
        let (x1, y1) = pts[i];
        let (x2, y2) = pts[(i + 1) % n];
        a += x1 * y2 - x2 * y1;
    }
    StrykeValue::float(a.abs() / 2.0)
}
/// `contour_centroid` — see implementation.

pub fn contour_centroid(args: &[StrykeValue]) -> StrykeValue {
    // Area centroid of the closed polygon defined by the contour points
    // (OpenCV M10/M00, M01/M00 semantics). Falls back to vertex mean when
    // the polygon is degenerate (zero signed area).
    let pts: Vec<(f64, f64)> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(|p| {
            let xs = as_vec_f64(p);
            (
                xs.first().copied().unwrap_or(0.0),
                xs.get(1).copied().unwrap_or(0.0),
            )
        })
        .collect();
    let n = pts.len();
    if n == 0 {
        return arr_f64(vec![0.0, 0.0]);
    }
    if n < 3 {
        let (sx, sy): (f64, f64) = pts
            .iter()
            .fold((0.0, 0.0), |acc, &(x, y)| (acc.0 + x, acc.1 + y));
        return arr_f64(vec![sx / n as f64, sy / n as f64]);
    }
    let mut a2 = 0.0_f64;
    let mut cx = 0.0_f64;
    let mut cy = 0.0_f64;
    for i in 0..n {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % n];
        let cross = x0 * y1 - x1 * y0;
        a2 += cross;
        cx += (x0 + x1) * cross;
        cy += (y0 + y1) * cross;
    }
    if a2.abs() < 1e-12 {
        let (sx, sy): (f64, f64) = pts
            .iter()
            .fold((0.0, 0.0), |acc, &(x, y)| (acc.0 + x, acc.1 + y));
        return arr_f64(vec![sx / n as f64, sy / n as f64]);
    }
    arr_f64(vec![cx / (3.0 * a2), cy / (3.0 * a2)])
}
/// `moment_image` — see implementation.

pub fn moment_image(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let p = arg_i64(args, 1).unwrap_or(0).max(0) as u32;
    let q = arg_i64(args, 2).unwrap_or(0).max(0) as u32;
    let h = img.len();
    if h == 0 {
        return StrykeValue::float(0.0);
    }
    let mut total = 0.0;
    for (i, row) in img.iter().enumerate() {
        for (j, &v) in row.iter().enumerate() {
            total += (i as f64).powi(p as i32) * (j as f64).powi(q as i32) * v;
        }
    }
    StrykeValue::float(total)
}
/// `hu_moments` — see implementation.

pub fn hu_moments(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let m = |p: u32, q: u32| {
        moment_image(&[
            matrix_to_sv(&img),
            StrykeValue::integer(p as i64),
            StrykeValue::integer(q as i64),
        ])
        .to_number()
    };
    let m00 = m(0, 0).max(1e-12);
    let m10 = m(1, 0);
    let m01 = m(0, 1);
    let xc = m10 / m00;
    let yc = m01 / m00;
    let mu = |p: u32, q: u32| -> f64 {
        let mut total = 0.0;
        for (i, row) in img.iter().enumerate() {
            for (j, &v) in row.iter().enumerate() {
                total += (i as f64 - xc).powi(p as i32) * (j as f64 - yc).powi(q as i32) * v;
            }
        }
        total
    };
    let mu20 = mu(2, 0);
    let mu02 = mu(0, 2);
    let mu11 = mu(1, 1);
    let mu30 = mu(3, 0);
    let mu03 = mu(0, 3);
    let mu21 = mu(2, 1);
    let mu12 = mu(1, 2);
    let scale = m00.powf(2.0);
    let n20 = mu20 / scale;
    let n02 = mu02 / scale;
    let n11 = mu11 / scale;
    let scale3 = m00.powf(2.5);
    let n30 = mu30 / scale3;
    let n03 = mu03 / scale3;
    let n21 = mu21 / scale3;
    let n12 = mu12 / scale3;
    let h1 = n20 + n02;
    let h2 = (n20 - n02).powi(2) + 4.0 * n11.powi(2);
    let h3 = (n30 - 3.0 * n12).powi(2) + (3.0 * n21 - n03).powi(2);
    let h4 = (n30 + n12).powi(2) + (n21 + n03).powi(2);
    let a = n30 + n12;
    let b_ = n21 + n03;
    let h5 = (n30 - 3.0 * n12) * a * (a * a - 3.0 * b_ * b_)
        + (3.0 * n21 - n03) * b_ * (3.0 * a * a - b_ * b_);
    let h6 = (n20 - n02) * (a * a - b_ * b_) + 4.0 * n11 * a * b_;
    let h7 = (3.0 * n21 - n03) * a * (a * a - 3.0 * b_ * b_)
        - (n30 - 3.0 * n12) * b_ * (3.0 * a * a - b_ * b_);
    arr_f64(vec![h1, h2, h3, h4, h5, h6, h7])
}
/// `zernike_radial` — see implementation.

pub fn zernike_radial(args: &[StrykeValue]) -> StrykeValue {
    // Zernike polynomial R_n^m(rho) for n,m integers
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let m = arg_i64(args, 1).unwrap_or(0).unsigned_abs() as usize;
    let rho = arg_f64(args, 2).unwrap_or(0.0).clamp(0.0, 1.0);
    if !(n - m).is_multiple_of(2) || m > n {
        return StrykeValue::float(0.0);
    }
    let mut sum = 0.0;
    let max_k = (n - m) / 2;
    fn fact(n: usize) -> f64 {
        let mut r = 1.0;
        for i in 2..=n {
            r *= i as f64;
        }
        r
    }
    for k in 0..=max_k {
        let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
        let num = fact(n - k);
        let denom = fact(k) * fact((n + m) / 2 - k) * fact((n - m) / 2 - k);
        sum += sign * num / denom * rho.powi((n - 2 * k) as i32);
    }
    StrykeValue::float(sum)
}

/// Full Canny edge detector pipeline:
/// 1. Gaussian blur (5×5, σ=1.4)
/// 2. Sobel gradient magnitude + direction
/// 3. Non-maximum suppression along gradient direction
/// 4. Double-threshold classification (strong=255, weak=128)
pub fn canny_edges_full(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    let low = arg_f64(args, 1).unwrap_or(50.0);
    let high = arg_f64(args, 2).unwrap_or(150.0);
    if img.is_empty() {
        return matrix_to_sv(&[]);
    }
    let h = img.len();
    let w = img[0].len();
    // 1. Gaussian 5×5 blur (σ=1.4); symmetric kernel, no flip needed.
    let sigma = 1.4_f64;
    let mut g5 = [[0.0_f64; 5]; 5];
    let mut g_sum = 0.0_f64;
    for i in 0..5 {
        for j in 0..5 {
            let dx = i as f64 - 2.0;
            let dy = j as f64 - 2.0;
            g5[i][j] = (-(dx * dx + dy * dy) / (2.0 * sigma * sigma)).exp();
            g_sum += g5[i][j];
        }
    }
    for row in g5.iter_mut() {
        for v in row.iter_mut() {
            *v /= g_sum;
        }
    }
    let mut blurred = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            let mut acc = 0.0_f64;
            for ki in 0..5 {
                for kj in 0..5 {
                    let y = i as isize + ki as isize - 2;
                    let x = j as isize + kj as isize - 2;
                    if (0..h as isize).contains(&y) && (0..w as isize).contains(&x) {
                        acc += img[y as usize][x as usize] * g5[ki][kj];
                    }
                }
            }
            blurred[i][j] = acc;
        }
    }
    // 2. Sobel gradient on the blurred image.
    let mut mag = vec![vec![0.0_f64; w]; h];
    let mut ang = vec![vec![0.0_f64; w]; h];
    for i in 1..h - 1 {
        for j in 1..w - 1 {
            let gx = -blurred[i - 1][j - 1] + blurred[i - 1][j + 1] - 2.0 * blurred[i][j - 1]
                + 2.0 * blurred[i][j + 1]
                - blurred[i + 1][j - 1]
                + blurred[i + 1][j + 1];
            let gy = -blurred[i - 1][j - 1] - 2.0 * blurred[i - 1][j] - blurred[i - 1][j + 1]
                + blurred[i + 1][j - 1]
                + 2.0 * blurred[i + 1][j]
                + blurred[i + 1][j + 1];
            mag[i][j] = (gx * gx + gy * gy).sqrt();
            ang[i][j] = gy.atan2(gx).to_degrees().rem_euclid(180.0);
        }
    }
    // Non-max suppression
    let mut nms = vec![vec![0.0_f64; w]; h];
    for i in 1..h - 1 {
        for j in 1..w - 1 {
            let a = ang[i][j];
            let (n1, n2) = if !(22.5..157.5).contains(&a) {
                (mag[i][j - 1], mag[i][j + 1])
            } else if (22.5..67.5).contains(&a) {
                (mag[i - 1][j + 1], mag[i + 1][j - 1])
            } else if (67.5..112.5).contains(&a) {
                (mag[i - 1][j], mag[i + 1][j])
            } else {
                (mag[i - 1][j - 1], mag[i + 1][j + 1])
            };
            nms[i][j] = if mag[i][j] >= n1 && mag[i][j] >= n2 {
                mag[i][j]
            } else {
                0.0
            };
        }
    }
    // Hysteresis: classify into strong/weak/none, then flood-fill weak pixels
    // 8-connected to strong pixels into the strong set (kept = 255). Weak
    // pixels not reachable from any strong are discarded (set to 0).
    let mut label = vec![vec![0u8; w]; h]; // 0=none, 1=weak, 2=strong
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for i in 0..h {
        for j in 0..w {
            if nms[i][j] >= high {
                label[i][j] = 2;
                stack.push((i, j));
            } else if nms[i][j] >= low {
                label[i][j] = 1;
            }
        }
    }
    while let Some((i, j)) = stack.pop() {
        for di in -1..=1_isize {
            for dj in -1..=1_isize {
                if di == 0 && dj == 0 {
                    continue;
                }
                let ni = i as isize + di;
                let nj = j as isize + dj;
                if ni < 0 || nj < 0 || ni >= h as isize || nj >= w as isize {
                    continue;
                }
                let (ni, nj) = (ni as usize, nj as usize);
                if label[ni][nj] == 1 {
                    label[ni][nj] = 2;
                    stack.push((ni, nj));
                }
            }
        }
    }
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            if label[i][j] == 2 {
                out[i][j] = 255.0;
            }
        }
    }
    matrix_to_sv(&out)
}
/// `sobel_magnitude` — see implementation.

pub fn sobel_magnitude(args: &[StrykeValue]) -> StrykeValue {
    let img = args.first().map(as_matrix).unwrap_or_default();
    if img.is_empty() {
        return matrix_to_sv(&[]);
    }
    let h = img.len();
    let w = img[0].len();
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 1..h - 1 {
        for j in 1..w - 1 {
            let gx = -img[i - 1][j - 1] + img[i - 1][j + 1] - 2.0 * img[i][j - 1]
                + 2.0 * img[i][j + 1]
                - img[i + 1][j - 1]
                + img[i + 1][j + 1];
            let gy = -img[i - 1][j - 1] - 2.0 * img[i - 1][j] - img[i - 1][j + 1]
                + img[i + 1][j - 1]
                + 2.0 * img[i + 1][j]
                + img[i + 1][j + 1];
            out[i][j] = (gx * gx + gy * gy).sqrt();
        }
    }
    matrix_to_sv(&out)
}
/// `prewitt_x_kernel` — see implementation.

pub fn prewitt_x_kernel(_args: &[StrykeValue]) -> StrykeValue {
    matrix_to_sv(&[
        vec![-1.0, 0.0, 1.0],
        vec![-1.0, 0.0, 1.0],
        vec![-1.0, 0.0, 1.0],
    ])
}
/// `prewitt_y_kernel` — see implementation.

pub fn prewitt_y_kernel(_args: &[StrykeValue]) -> StrykeValue {
    matrix_to_sv(&[
        vec![-1.0, -1.0, -1.0],
        vec![0.0, 0.0, 0.0],
        vec![1.0, 1.0, 1.0],
    ])
}
/// `scharr_x_kernel` — see implementation.

pub fn scharr_x_kernel(_args: &[StrykeValue]) -> StrykeValue {
    matrix_to_sv(&[
        vec![-3.0, 0.0, 3.0],
        vec![-10.0, 0.0, 10.0],
        vec![-3.0, 0.0, 3.0],
    ])
}
/// `scharr_y_kernel` — see implementation.

pub fn scharr_y_kernel(_args: &[StrykeValue]) -> StrykeValue {
    matrix_to_sv(&[
        vec![-3.0, -10.0, -3.0],
        vec![0.0, 0.0, 0.0],
        vec![3.0, 10.0, 3.0],
    ])
}
/// `roberts_cross_kernel` — see implementation.

pub fn roberts_cross_kernel(_args: &[StrykeValue]) -> StrykeValue {
    matrix_to_sv(&[vec![1.0, 0.0], vec![0.0, -1.0]])
}

// ══════════════════════════════════════════════════════════════════════
// Computational geometry 2D
// ══════════════════════════════════════════════════════════════════════

fn point_xy_pair(v: &StrykeValue) -> (f64, f64) {
    let xs = as_vec_f64(v);
    (
        xs.first().copied().unwrap_or(0.0),
        xs.get(1).copied().unwrap_or(0.0),
    )
}

fn pts_pack(pts: &[(f64, f64)]) -> StrykeValue {
    arr_sv(pts.iter().map(|&(x, y)| arr_f64(vec![x, y])).collect())
}
/// `graham_scan_hull` — see implementation.

pub fn graham_scan_hull(args: &[StrykeValue]) -> StrykeValue {
    let mut pts: Vec<(f64, f64)> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(point_xy_pair)
        .collect();
    if pts.len() < 3 {
        return pts_pack(&pts);
    }
    let pivot = pts
        .iter()
        .min_by(|a, b| {
            a.1.partial_cmp(&b.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
        })
        .copied()
        .unwrap();
    pts.sort_by(|a, b| {
        let aa = (a.1 - pivot.1).atan2(a.0 - pivot.0);
        let bb = (b.1 - pivot.1).atan2(b.0 - pivot.0);
        aa.partial_cmp(&bb).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut hull: Vec<(f64, f64)> = Vec::new();
    for p in pts {
        while hull.len() >= 2 {
            let n = hull.len();
            let cross = (hull[n - 1].0 - hull[n - 2].0) * (p.1 - hull[n - 2].1)
                - (hull[n - 1].1 - hull[n - 2].1) * (p.0 - hull[n - 2].0);
            if cross <= 0.0 {
                hull.pop();
            } else {
                break;
            }
        }
        hull.push(p);
    }
    pts_pack(&hull)
}
/// `andrew_monotone_hull` — see implementation.

pub fn andrew_monotone_hull(args: &[StrykeValue]) -> StrykeValue {
    let mut pts: Vec<(f64, f64)> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(point_xy_pair)
        .collect();
    pts.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    });
    let n = pts.len();
    if n < 3 {
        return pts_pack(&pts);
    }
    let cross = |o: (f64, f64), a: (f64, f64), b: (f64, f64)| {
        (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
    };
    let mut lower: Vec<(f64, f64)> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<(f64, f64)> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    pts_pack(&lower)
}
/// `liang_barsky_clip` — see implementation.

pub fn liang_barsky_clip(args: &[StrykeValue]) -> StrykeValue {
    // Clip line segment against rectangle [xmin,ymin,xmax,ymax]
    let p1 = point_xy_pair(args.first().unwrap_or(&StrykeValue::UNDEF));
    let p2 = point_xy_pair(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let rect = as_vec_f64(args.get(2).unwrap_or(&StrykeValue::UNDEF));
    if rect.len() < 4 {
        return arr_sv(vec![]);
    }
    let xmin = rect[0];
    let ymin = rect[1];
    let xmax = rect[2];
    let ymax = rect[3];
    let dx = p2.0 - p1.0;
    let dy = p2.1 - p1.1;
    let p = [-dx, dx, -dy, dy];
    let q = [p1.0 - xmin, xmax - p1.0, p1.1 - ymin, ymax - p1.1];
    let mut u1 = 0.0_f64;
    let mut u2 = 1.0_f64;
    for k in 0..4 {
        if p[k].abs() < 1e-12 {
            if q[k] < 0.0 {
                return arr_sv(vec![]);
            }
        } else {
            let t = q[k] / p[k];
            if p[k] < 0.0 {
                if t > u2 {
                    return arr_sv(vec![]);
                }
                if t > u1 {
                    u1 = t;
                }
            } else {
                if t < u1 {
                    return arr_sv(vec![]);
                }
                if t < u2 {
                    u2 = t;
                }
            }
        }
    }
    let clipped_a = (p1.0 + u1 * dx, p1.1 + u1 * dy);
    let clipped_b = (p1.0 + u2 * dx, p1.1 + u2 * dy);
    arr_sv(vec![
        arr_f64(vec![clipped_a.0, clipped_a.1]),
        arr_f64(vec![clipped_b.0, clipped_b.1]),
    ])
}
/// `polygon_winding` — see implementation.

pub fn polygon_winding(args: &[StrykeValue]) -> StrykeValue {
    let pts: Vec<(f64, f64)> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(point_xy_pair)
        .collect();
    let n = pts.len();
    if n < 3 {
        return StrykeValue::integer(0);
    }
    let mut signed = 0.0_f64;
    for i in 0..n {
        let (x1, y1) = pts[i];
        let (x2, y2) = pts[(i + 1) % n];
        signed += (x2 - x1) * (y2 + y1);
    }
    StrykeValue::integer(if signed > 0.0 { -1 } else { 1 })
}
/// `polygon_simple_check` — see implementation.

pub fn polygon_simple_check(args: &[StrykeValue]) -> StrykeValue {
    // Returns 1 if polygon edges have no self-intersections
    let pts: Vec<(f64, f64)> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(point_xy_pair)
        .collect();
    let n = pts.len();
    if n < 4 {
        return StrykeValue::integer(1);
    }
    fn seg_intersect(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
        let ccw = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| -> f64 {
            (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
        };
        let d1 = ccw(c, d, a);
        let d2 = ccw(c, d, b);
        let d3 = ccw(a, b, c);
        let d4 = ccw(a, b, d);
        ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
            && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    }
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            if seg_intersect(pts[i], pts[(i + 1) % n], pts[j], pts[(j + 1) % n]) {
                return StrykeValue::integer(0);
            }
        }
    }
    StrykeValue::integer(1)
}
/// `polygon_self_intersects` — see implementation.

pub fn polygon_self_intersects(args: &[StrykeValue]) -> StrykeValue {
    let simple = polygon_simple_check(args).to_int();
    StrykeValue::integer(1 - simple)
}
/// `polygon_offset` — see implementation.

pub fn polygon_offset(args: &[StrykeValue]) -> StrykeValue {
    // Perpendicular-distance offset using the corner-bisector formula:
    // offset_vertex = vertex + d · (n1 + n2) / (1 + n1·n2)
    // where n1, n2 are the outward unit normals of the adjacent edges.
    // This gives an exact perpendicular offset d, not the naive d·bisector_dir
    // (which under-offsets non-flat corners).
    let pts: Vec<(f64, f64)> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(point_xy_pair)
        .collect();
    let d = arg_f64(args, 1).unwrap_or(1.0);
    let n = pts.len();
    if n < 3 {
        return pts_pack(&pts);
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let prev = pts[(i + n - 1) % n];
        let cur = pts[i];
        let next = pts[(i + 1) % n];
        // Outward normal of edge prev→cur (CCW polygon): (dy, -dx) / |e|.
        let e1 = (cur.0 - prev.0, cur.1 - prev.1);
        let e2 = (next.0 - cur.0, next.1 - cur.1);
        let l1 = (e1.0 * e1.0 + e1.1 * e1.1).sqrt().max(1e-12);
        let l2 = (e2.0 * e2.0 + e2.1 * e2.1).sqrt().max(1e-12);
        let n1 = (e1.1 / l1, -e1.0 / l1);
        let n2 = (e2.1 / l2, -e2.0 / l2);
        let dot = n1.0 * n2.0 + n1.1 * n2.1;
        let denom = 1.0 + dot;
        // Anti-spike clamp: if corner is near 180° reflex, fall back to bisector direction at distance d.
        if denom.abs() < 1e-9 {
            let bx = n1.0 + n2.0;
            let by = n1.1 + n2.1;
            let bl = (bx * bx + by * by).sqrt().max(1e-12);
            out.push((cur.0 + d * bx / bl, cur.1 + d * by / bl));
        } else {
            out.push((
                cur.0 + d * (n1.0 + n2.0) / denom,
                cur.1 + d * (n1.1 + n2.1) / denom,
            ));
        }
    }
    pts_pack(&out)
}
/// `polygon_shrink` — see implementation.

pub fn polygon_shrink(args: &[StrykeValue]) -> StrykeValue {
    let pts = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let d = -arg_f64(args, 1).unwrap_or(1.0);
    polygon_offset(&[pts, StrykeValue::float(d)])
}
/// `voronoi_cell_2d` — see implementation.

pub fn voronoi_cell_2d(args: &[StrykeValue]) -> StrykeValue {
    // For a given point + set of other points, compute its Voronoi cell as half-plane intersection
    let pt = point_xy_pair(args.first().unwrap_or(&StrykeValue::UNDEF));
    let others: Vec<(f64, f64)> = as_vec_sv(args.get(1).unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(point_xy_pair)
        .collect();
    let bbox = as_vec_f64(args.get(2).unwrap_or(&StrykeValue::UNDEF));
    let (xmin, ymin, xmax, ymax) = if bbox.len() >= 4 {
        (bbox[0], bbox[1], bbox[2], bbox[3])
    } else {
        (-1000.0, -1000.0, 1000.0, 1000.0)
    };
    let mut cell = vec![(xmin, ymin), (xmax, ymin), (xmax, ymax), (xmin, ymax)];
    for o in others {
        if (o.0 - pt.0).abs() < 1e-12 && (o.1 - pt.1).abs() < 1e-12 {
            continue;
        }
        let mx = (pt.0 + o.0) / 2.0;
        let my = (pt.1 + o.1) / 2.0;
        let dx = o.0 - pt.0;
        let dy = o.1 - pt.1;
        let mut new_cell = Vec::new();
        let n = cell.len();
        let inside = |p: (f64, f64)| dx * (p.0 - mx) + dy * (p.1 - my) <= 0.0;
        for i in 0..n {
            let cur = cell[i];
            let prev = cell[(i + n - 1) % n];
            let cur_in = inside(cur);
            let prev_in = inside(prev);
            if cur_in {
                if !prev_in {
                    let denom = dx * (cur.0 - prev.0) + dy * (cur.1 - prev.1);
                    if denom.abs() > 1e-12 {
                        let t = (dx * (mx - prev.0) + dy * (my - prev.1)) / denom;
                        new_cell
                            .push((prev.0 + t * (cur.0 - prev.0), prev.1 + t * (cur.1 - prev.1)));
                    }
                }
                new_cell.push(cur);
            } else if prev_in {
                let denom = dx * (cur.0 - prev.0) + dy * (cur.1 - prev.1);
                if denom.abs() > 1e-12 {
                    let t = (dx * (mx - prev.0) + dy * (my - prev.1)) / denom;
                    new_cell.push((prev.0 + t * (cur.0 - prev.0), prev.1 + t * (cur.1 - prev.1)));
                }
            }
        }
        cell = new_cell;
    }
    pts_pack(&cell)
}
/// `delaunay_triangulate_2d` — see implementation.

pub fn delaunay_triangulate_2d(args: &[StrykeValue]) -> StrykeValue {
    // Bowyer-Watson on small point sets
    let pts: Vec<(f64, f64)> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(point_xy_pair)
        .collect();
    if pts.len() < 3 {
        return arr_sv(vec![]);
    }
    let in_circle = |a: (f64, f64), b: (f64, f64), c: (f64, f64), p: (f64, f64)| -> bool {
        let ax = a.0 - p.0;
        let ay = a.1 - p.1;
        let bx = b.0 - p.0;
        let by = b.1 - p.1;
        let cx = c.0 - p.0;
        let cy = c.1 - p.1;
        let det = ax * (by * (cx * cx + cy * cy) - cy * (bx * bx + by * by))
            - ay * (bx * (cx * cx + cy * cy) - cx * (bx * bx + by * by))
            + (ax * ax + ay * ay) * (bx * cy - by * cx);
        det > 0.0
    };
    // Super-triangle bounding all points
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    for &(x, y) in &pts {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    let dx = max_x - min_x;
    let dy = max_y - min_y;
    let delta_max = dx.max(dy) * 10.0;
    let mid_x = (min_x + max_x) / 2.0;
    let mid_y = (min_y + max_y) / 2.0;
    let p1 = (mid_x - 20.0 * delta_max, mid_y - delta_max);
    let p2 = (mid_x, mid_y + 20.0 * delta_max);
    let p3 = (mid_x + 20.0 * delta_max, mid_y - delta_max);
    let n_orig = pts.len();
    let mut all_pts = pts.clone();
    all_pts.push(p1);
    all_pts.push(p2);
    all_pts.push(p3);
    let mut triangles: Vec<(usize, usize, usize)> = vec![(n_orig, n_orig + 1, n_orig + 2)];
    for i in 0..n_orig {
        let mut bad: Vec<usize> = Vec::new();
        for (idx, &tri) in triangles.iter().enumerate() {
            if in_circle(all_pts[tri.0], all_pts[tri.1], all_pts[tri.2], all_pts[i]) {
                bad.push(idx);
            }
        }
        let mut edges: Vec<(usize, usize)> = Vec::new();
        for &idx in &bad {
            let (a, b, c) = triangles[idx];
            edges.push((a.min(b), a.max(b)));
            edges.push((b.min(c), b.max(c)));
            edges.push((a.min(c), a.max(c)));
        }
        edges.sort();
        let mut unique_edges: Vec<(usize, usize)> = Vec::new();
        let mut j = 0;
        while j < edges.len() {
            if j + 1 < edges.len() && edges[j] == edges[j + 1] {
                j += 2;
            } else {
                unique_edges.push(edges[j]);
                j += 1;
            }
        }
        // remove bad triangles in reverse
        bad.sort_by(|a, b| b.cmp(a));
        for idx in bad {
            triangles.remove(idx);
        }
        for (a, b) in unique_edges {
            triangles.push((a, b, i));
        }
    }
    // Remove triangles touching super-triangle
    let result: Vec<StrykeValue> = triangles
        .into_iter()
        .filter(|t| t.0 < n_orig && t.1 < n_orig && t.2 < n_orig)
        .map(|t| {
            arr_sv(vec![
                StrykeValue::integer(t.0 as i64),
                StrykeValue::integer(t.1 as i64),
                StrykeValue::integer(t.2 as i64),
            ])
        })
        .collect();
    arr_sv(result)
}
/// `minkowski_sum_2d` — see implementation.

pub fn minkowski_sum_2d(args: &[StrykeValue]) -> StrykeValue {
    let a: Vec<(f64, f64)> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(point_xy_pair)
        .collect();
    let b: Vec<(f64, f64)> = as_vec_sv(args.get(1).unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(point_xy_pair)
        .collect();
    let mut out: Vec<(f64, f64)> = Vec::new();
    for &p in &a {
        for &q in &b {
            out.push((p.0 + q.0, p.1 + q.1));
        }
    }
    pts_pack(&out)
}

/// `convex_hull_3d(points) → array` — incremental 3D convex hull.
/// Returns the unique hull vertices as `[x, y, z]` triples.
/// Falls back gracefully for ≤4 input points (returns them all).
pub fn convex_hull_3d(args: &[StrykeValue]) -> StrykeValue {
    let pts: Vec<(f64, f64, f64)> = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF))
        .iter()
        .map(|p| {
            let xs = as_vec_f64(p);
            (
                xs.first().copied().unwrap_or(0.0),
                xs.get(1).copied().unwrap_or(0.0),
                xs.get(2).copied().unwrap_or(0.0),
            )
        })
        .collect();
    if pts.len() < 4 {
        return arr_sv(
            pts.into_iter()
                .map(|(x, y, z)| arr_f64(vec![x, y, z]))
                .collect(),
        );
    }
    let n = pts.len();
    // Each face is (a, b, c) — three indices into pts forming a CCW triangle
    // when viewed from outside the hull.
    type Face = (usize, usize, usize);
    let cross = |u: (f64, f64, f64), v: (f64, f64, f64)| {
        (
            u.1 * v.2 - u.2 * v.1,
            u.2 * v.0 - u.0 * v.2,
            u.0 * v.1 - u.1 * v.0,
        )
    };
    let sub = |a: (f64, f64, f64), b: (f64, f64, f64)| (a.0 - b.0, a.1 - b.1, a.2 - b.2);
    let dot = |u: (f64, f64, f64), v: (f64, f64, f64)| u.0 * v.0 + u.1 * v.1 + u.2 * v.2;
    let face_outward = |f: &Face, p: (f64, f64, f64)| -> f64 {
        let a = pts[f.0];
        let b = pts[f.1];
        let c = pts[f.2];
        let n = cross(sub(b, a), sub(c, a));
        dot(n, sub(p, a))
    };
    // Seed tetrahedron from the first four non-coplanar points.
    let mut seed: Option<Face> = None;
    let mut apex: Option<usize> = None;
    'outer: for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let a = pts[i];
                let b = pts[j];
                let c = pts[k];
                let n_vec = cross(sub(b, a), sub(c, a));
                let mag = (n_vec.0 * n_vec.0 + n_vec.1 * n_vec.1 + n_vec.2 * n_vec.2).sqrt();
                if mag < 1e-9 {
                    continue;
                }
                for l in 0..n {
                    if l == i || l == j || l == k {
                        continue;
                    }
                    let p = pts[l];
                    let d = dot(n_vec, sub(p, a));
                    if d.abs() > 1e-9 {
                        seed = Some(if d > 0.0 { (i, k, j) } else { (i, j, k) });
                        apex = Some(l);
                        break 'outer;
                    }
                }
            }
        }
    }
    let (seed, apex) = match (seed, apex) {
        (Some(s), Some(a)) => (s, a),
        _ => {
            // All points coplanar — return original points as a degenerate hull.
            return arr_sv(
                pts.into_iter()
                    .map(|(x, y, z)| arr_f64(vec![x, y, z]))
                    .collect(),
            );
        }
    };
    // Initial tetrahedron faces (4 triangles), all oriented to point outward
    // from the centroid of the four seed points.
    let (a, b, c) = seed;
    let d = apex;
    let centroid = (
        (pts[a].0 + pts[b].0 + pts[c].0 + pts[d].0) / 4.0,
        (pts[a].1 + pts[b].1 + pts[c].1 + pts[d].1) / 4.0,
        (pts[a].2 + pts[b].2 + pts[c].2 + pts[d].2) / 4.0,
    );
    let mut faces: Vec<Face> = vec![(a, b, c), (a, c, d), (a, d, b), (b, d, c)];
    for f in faces.iter_mut() {
        if face_outward(f, centroid) > 0.0 {
            std::mem::swap(&mut f.1, &mut f.2);
        }
    }
    let mut on_hull = vec![false; n];
    on_hull[a] = true;
    on_hull[b] = true;
    on_hull[c] = true;
    on_hull[d] = true;
    // Incrementally add each remaining point.
    for p_idx in 0..n {
        if on_hull[p_idx] {
            continue;
        }
        let p = pts[p_idx];
        // Find visible faces.
        let visible: Vec<usize> = (0..faces.len())
            .filter(|&i| face_outward(&faces[i], p) > 1e-9)
            .collect();
        if visible.is_empty() {
            continue;
        }
        // Find horizon edges (edges of visible faces not shared with another visible face).
        use std::collections::HashSet;
        let mut edge_count: std::collections::HashMap<(usize, usize), i32> =
            std::collections::HashMap::new();
        for &fi in &visible {
            let f = faces[fi];
            for (u, v) in [(f.0, f.1), (f.1, f.2), (f.2, f.0)] {
                let key = if u < v { (u, v) } else { (v, u) };
                *edge_count.entry(key).or_insert(0) += 1;
            }
        }
        let horizon: Vec<(usize, usize)> = edge_count
            .into_iter()
            .filter(|&(_, c)| c == 1)
            .map(|(e, _)| e)
            .collect();
        // Remove visible faces.
        let visible_set: HashSet<usize> = visible.into_iter().collect();
        faces = faces
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !visible_set.contains(i))
            .map(|(_, f)| f)
            .collect();
        // Add new faces from horizon to p_idx, oriented outward.
        for (u, v) in horizon {
            let mut new_face = (u, v, p_idx);
            if face_outward(&new_face, centroid) > 0.0 {
                std::mem::swap(&mut new_face.1, &mut new_face.2);
            }
            faces.push(new_face);
        }
        on_hull[p_idx] = true;
    }
    // Collect unique hull vertices in input order.
    let mut hull_idxs: Vec<usize> = (0..n).filter(|&i| on_hull[i]).collect();
    // Filter to vertices that actually appear in some face (incremental hull
    // may include points not on the final hull).
    let mut face_verts: HashSet<usize> = HashSet::new();
    for f in &faces {
        face_verts.insert(f.0);
        face_verts.insert(f.1);
        face_verts.insert(f.2);
    }
    hull_idxs.retain(|i| face_verts.contains(i));
    arr_sv(
        hull_idxs
            .into_iter()
            .map(|i| {
                let (x, y, z) = pts[i];
                arr_f64(vec![x, y, z])
            })
            .collect(),
    )
}

use std::collections::HashSet;

// ══════════════════════════════════════════════════════════════════════
// Crypto primitives
// ══════════════════════════════════════════════════════════════════════
/// `rsa_modular_exp` — see implementation.

pub fn rsa_modular_exp(args: &[StrykeValue]) -> StrykeValue {
    use num_bigint::BigUint;
    let base = arg_str(args, 0).unwrap_or_default();
    let exp = arg_str(args, 1).unwrap_or_default();
    let modulus = arg_str(args, 2).unwrap_or_default();
    let b = match BigUint::parse_bytes(base.as_bytes(), 10) {
        Some(v) => v,
        None => return StrykeValue::UNDEF,
    };
    let e = match BigUint::parse_bytes(exp.as_bytes(), 10) {
        Some(v) => v,
        None => return StrykeValue::UNDEF,
    };
    let m = match BigUint::parse_bytes(modulus.as_bytes(), 10) {
        Some(v) => v,
        None => return StrykeValue::UNDEF,
    };
    StrykeValue::string(b.modpow(&e, &m).to_string())
}

/// Textbook RSA keypair from two prime hints. Validates that p and q are
/// both prime (trial division for the small primes this fits), that they
/// differ, and that `gcd(e, (p-1)(q-1)) = 1` so the modular inverse exists.
/// Returns `{n, e, d}` or UNDEF for invalid input.
///
/// Not cryptographically secure — uses i64 modulus, no padding (raw RSA),
/// and small primes. Educational use only.
pub fn rsa_keypair_simple(args: &[StrykeValue]) -> StrykeValue {
    fn is_prime(n: i64) -> bool {
        if n < 2 {
            return false;
        }
        if n < 4 {
            return true;
        }
        if n % 2 == 0 {
            return false;
        }
        let mut k = 3_i64;
        while k * k <= n {
            if n % k == 0 {
                return false;
            }
            k += 2;
        }
        true
    }
    fn ext_gcd(a: i128, b: i128) -> (i128, i128, i128) {
        if a == 0 {
            return (b, 0, 1);
        }
        let (g, x, y) = ext_gcd(b % a, a);
        (g, y - (b / a) * x, x)
    }
    let p_arg = arg_i64(args, 0).unwrap_or(61);
    let q_arg = arg_i64(args, 1).unwrap_or(53);
    if !is_prime(p_arg) || !is_prime(q_arg) || p_arg == q_arg {
        return StrykeValue::UNDEF;
    }
    let n = p_arg as i128 * q_arg as i128;
    let phi = (p_arg as i128 - 1) * (q_arg as i128 - 1);
    // Try e=65537 first (standard), fall back to 17 / 3 for very small phi.
    let candidates = [65537_i128, 17, 3];
    let mut chosen: Option<(i128, i128)> = None;
    for &e in &candidates {
        if e >= phi {
            continue;
        }
        let (g, x, _) = ext_gcd(e, phi);
        if g == 1 {
            let d = ((x % phi) + phi) % phi;
            chosen = Some((e, d));
            break;
        }
    }
    let (e, d) = match chosen {
        Some(pair) => pair,
        None => return StrykeValue::UNDEF,
    };
    make_hash(vec![
        ("n", StrykeValue::integer(n as i64)),
        ("e", StrykeValue::integer(e as i64)),
        ("d", StrykeValue::integer(d as i64)),
    ])
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        let pair = std::str::from_utf8(&bytes[i..i + 2]).ok()?;
        out.push(u8::from_str_radix(pair, 16).ok()?);
        i += 2;
    }
    Some(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Affine elliptic-curve point addition on the modular curve
/// `y² ≡ x³ + a·x + b (mod p)` using arbitrary-precision modular arithmetic.
/// Points are `[x, y]` decimal strings; identity is `[]`.
///
/// `ec_point_add(P, Q, a, p)` returns `P + Q` mod p, handling doubling
/// (P == Q) and the additive identity correctly.
pub fn ec_point_add(args: &[StrykeValue]) -> StrykeValue {
    use num_bigint::BigInt;
    use num_integer::Integer;
    use num_traits::{One, Zero};
    fn parse_pt(v: &StrykeValue) -> Option<(BigInt, BigInt)> {
        let xs = as_vec_sv(v);
        if xs.len() < 2 {
            return None;
        }
        let x = BigInt::parse_bytes(xs[0].as_str_or_empty().as_bytes(), 10)?;
        let y = BigInt::parse_bytes(xs[1].as_str_or_empty().as_bytes(), 10)?;
        Some((x, y))
    }
    fn modinv(a: &BigInt, m: &BigInt) -> Option<BigInt> {
        let er = a.extended_gcd(m);
        if !er.gcd.is_one() {
            return None;
        }
        Some(er.x.mod_floor(m))
    }
    let p1 = args.first().and_then(parse_pt);
    let p2 = args.get(1).and_then(parse_pt);
    let a = arg_str(args, 2)
        .and_then(|s| BigInt::parse_bytes(s.as_bytes(), 10))
        .unwrap_or_else(BigInt::zero);
    let prime = match arg_str(args, 3).and_then(|s| BigInt::parse_bytes(s.as_bytes(), 10)) {
        Some(p) => p,
        None => return arr_sv(vec![]),
    };
    let fmt_pt = |x: &BigInt, y: &BigInt| {
        arr_sv(vec![
            StrykeValue::string(x.to_string()),
            StrykeValue::string(y.to_string()),
        ])
    };
    let (p1, p2) = match (p1, p2) {
        (Some(p), Some(q)) => (p, q),
        (Some(p), None) => return fmt_pt(&p.0, &p.1),
        (None, Some(q)) => return fmt_pt(&q.0, &q.1),
        _ => return arr_sv(vec![]),
    };
    let (x1, y1) = p1;
    let (x2, y2) = p2;
    let x1m = x1.mod_floor(&prime);
    let x2m = x2.mod_floor(&prime);
    if x1m == x2m {
        if (&y1 + &y2).mod_floor(&prime).is_zero() {
            return arr_sv(vec![]);
        }
        // Doubling: λ = (3x² + a) / (2y)  (mod p)
        let num = (BigInt::from(3) * &x1 * &x1 + &a).mod_floor(&prime);
        let den = (BigInt::from(2) * &y1).mod_floor(&prime);
        let inv = match modinv(&den, &prime) {
            Some(i) => i,
            None => return arr_sv(vec![]),
        };
        let lam = (num * inv).mod_floor(&prime);
        let x3 = (&lam * &lam - BigInt::from(2) * &x1).mod_floor(&prime);
        let y3 = (&lam * (&x1 - &x3) - &y1).mod_floor(&prime);
        return fmt_pt(&x3, &y3);
    }
    // Addition: λ = (y2 - y1) / (x2 - x1)
    let num = (&y2 - &y1).mod_floor(&prime);
    let den = (&x2 - &x1).mod_floor(&prime);
    let inv = match modinv(&den, &prime) {
        Some(i) => i,
        None => return arr_sv(vec![]),
    };
    let lam = (num * inv).mod_floor(&prime);
    let x3 = (&lam * &lam - &x1 - &x2).mod_floor(&prime);
    let y3 = (&lam * (&x1 - &x3) - &y1).mod_floor(&prime);
    fmt_pt(&x3, &y3)
}

/// EC point doubling on `y² ≡ x³ + a·x + b (mod p)`. Equivalent to
/// `ec_point_add(P, P, a, p)`.
pub fn ec_point_double(args: &[StrykeValue]) -> StrykeValue {
    let p = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = args.get(1).cloned().unwrap_or(StrykeValue::UNDEF);
    let prime = args.get(2).cloned().unwrap_or(StrykeValue::UNDEF);
    ec_point_add(&[p.clone(), p, a, prime])
}

/// Real BIP-340 Schnorr signature over secp256k1.
/// `schnorr_sign(msg, private_key_hex_32) → hex_signature_64`.
pub fn schnorr_sign_simple(args: &[StrykeValue]) -> StrykeValue {
    use k256::schnorr::{signature::Signer, Signature, SigningKey};
    let msg = arg_str(args, 0).unwrap_or_default();
    let priv_hex = arg_str(args, 1).unwrap_or_default();
    let priv_bytes = match hex_decode(&priv_hex) {
        Some(b) if b.len() == 32 => b,
        _ => return StrykeValue::UNDEF,
    };
    let sk = match SigningKey::from_bytes(&priv_bytes) {
        Ok(s) => s,
        Err(_) => return StrykeValue::UNDEF,
    };
    let sig: Signature = sk.sign(msg.as_bytes());
    StrykeValue::string(hex_encode(&sig.to_bytes()))
}

/// Real BIP-340 Schnorr signature verification.
/// `schnorr_verify(msg, public_key_hex_32, signature_hex_64) → 0|1`.
pub fn schnorr_verify_simple(args: &[StrykeValue]) -> StrykeValue {
    use k256::schnorr::{signature::Verifier, Signature, VerifyingKey};
    let msg = arg_str(args, 0).unwrap_or_default();
    let pub_hex = arg_str(args, 1).unwrap_or_default();
    let sig_hex = arg_str(args, 2).unwrap_or_default();
    let pub_bytes = match hex_decode(&pub_hex) {
        Some(b) if b.len() == 32 => b,
        _ => return StrykeValue::integer(0),
    };
    let sig_bytes = match hex_decode(&sig_hex) {
        Some(b) if b.len() == 64 => b,
        _ => return StrykeValue::integer(0),
    };
    let vk = match VerifyingKey::from_bytes(&pub_bytes) {
        Ok(k) => k,
        Err(_) => return StrykeValue::integer(0),
    };
    let sig = match Signature::try_from(sig_bytes.as_slice()) {
        Ok(s) => s,
        Err(_) => return StrykeValue::integer(0),
    };
    StrykeValue::integer(i64::from(vk.verify(msg.as_bytes(), &sig).is_ok()))
}

/// Modular-exponentiation Diffie-Hellman: shared = `peer_public^our_private mod p`.
/// All three arguments are decimal-string BigUInts.
pub fn dh_compute_shared(args: &[StrykeValue]) -> StrykeValue {
    use num_bigint::BigUint;
    let private_key = arg_str(args, 0).unwrap_or_default();
    let public_other = arg_str(args, 1).unwrap_or_default();
    let modulus = arg_str(args, 2).unwrap_or_default();
    let priv_n = BigUint::parse_bytes(private_key.as_bytes(), 10).unwrap_or_default();
    let pub_n = BigUint::parse_bytes(public_other.as_bytes(), 10).unwrap_or_default();
    let mod_n = match BigUint::parse_bytes(modulus.as_bytes(), 10) {
        Some(v) if v > BigUint::from(1u32) => v,
        _ => return StrykeValue::UNDEF,
    };
    StrykeValue::string(pub_n.modpow(&priv_n, &mod_n).to_string())
}

/// Real Ed25519 keypair from a 32-byte hex seed. Returns `{private, public}`
/// as hex strings using `ed25519-dalek`.
pub fn ed25519_keypair_simple(args: &[StrykeValue]) -> StrykeValue {
    use ed25519_dalek::SigningKey;
    let seed_hex = arg_str(args, 0).unwrap_or_else(|| "0".repeat(64));
    let seed_bytes = match hex_decode(&seed_hex) {
        Some(b) if b.len() == 32 => b,
        _ => return StrykeValue::UNDEF,
    };
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&seed_bytes);
    let sk = SigningKey::from_bytes(&arr);
    let vk = sk.verifying_key();
    make_hash(vec![
        ("private", StrykeValue::string(hex_encode(sk.as_bytes()))),
        ("public", StrykeValue::string(hex_encode(vk.as_bytes()))),
    ])
}

/// Real Ed25519 signature over the message. Returns a 64-byte hex string.
pub fn ed25519_sign_simple(args: &[StrykeValue]) -> StrykeValue {
    use ed25519_dalek::{Signer, SigningKey};
    let msg = arg_str(args, 0).unwrap_or_default();
    let priv_hex = arg_str(args, 1).unwrap_or_default();
    let priv_bytes = match hex_decode(&priv_hex) {
        Some(b) if b.len() == 32 => b,
        _ => return StrykeValue::UNDEF,
    };
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&priv_bytes);
    let sk = SigningKey::from_bytes(&arr);
    let sig = sk.sign(msg.as_bytes());
    StrykeValue::string(hex_encode(&sig.to_bytes()))
}

/// Real Ed25519 verification of a 64-byte signature against a 32-byte public key.
pub fn ed25519_verify_simple(args: &[StrykeValue]) -> StrykeValue {
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};
    let msg = arg_str(args, 0).unwrap_or_default();
    let pub_hex = arg_str(args, 1).unwrap_or_default();
    let sig_hex = arg_str(args, 2).unwrap_or_default();
    let pub_bytes = match hex_decode(&pub_hex) {
        Some(b) if b.len() == 32 => b,
        _ => return StrykeValue::integer(0),
    };
    let sig_bytes = match hex_decode(&sig_hex) {
        Some(b) if b.len() == 64 => b,
        _ => return StrykeValue::integer(0),
    };
    let mut pub_arr = [0u8; 32];
    pub_arr.copy_from_slice(&pub_bytes);
    let vk = match VerifyingKey::from_bytes(&pub_arr) {
        Ok(k) => k,
        Err(_) => return StrykeValue::integer(0),
    };
    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let sig = Signature::from_bytes(&sig_arr);
    StrykeValue::integer(i64::from(vk.verify(msg.as_bytes(), &sig).is_ok()))
}

// ══════════════════════════════════════════════════════════════════════
// Physics constants
// ══════════════════════════════════════════════════════════════════════
/// `constants_planck` — see implementation.

pub fn constants_planck(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(6.626_070_15e-34)
}
/// `constants_planck_h` — see implementation.
pub fn constants_planck_h(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(6.626_070_15e-34)
}
/// `constants_planck_hbar` — see implementation.
pub fn constants_planck_hbar(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(1.054_571_817e-34)
}
/// `constants_speed_of_light` — see implementation.
pub fn constants_speed_of_light(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(299_792_458.0)
}
/// `constants_gravitational_g` — see implementation.
pub fn constants_gravitational_g(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(6.674_30e-11)
}
/// `constants_electron_charge` — see implementation.
pub fn constants_electron_charge(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(1.602_176_634e-19)
}
/// `constants_electron_mass` — see implementation.
pub fn constants_electron_mass(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(9.109_383_701_5e-31)
}
/// `constants_proton_mass` — see implementation.
pub fn constants_proton_mass(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(1.672_621_923_69e-27)
}
/// `constants_neutron_mass` — see implementation.
pub fn constants_neutron_mass(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(1.674_927_498_04e-27)
}
/// `constants_solar_mass` — see implementation.
pub fn constants_solar_mass(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(1.989e30)
}
/// `constants_solar_radius` — see implementation.
pub fn constants_solar_radius(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(6.957e8)
}
/// `constants_earth_mass` — see implementation.
pub fn constants_earth_mass(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(5.972e24)
}
/// `constants_earth_radius` — see implementation.
pub fn constants_earth_radius(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(6.371e6)
}
/// `constants_au_meters` — see implementation.
pub fn constants_au_meters(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(1.495_978_707e11)
}
/// `constants_parsec_meters` — see implementation.
pub fn constants_parsec_meters(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(3.085_677_581e16)
}
/// `constants_lightyear_meters` — see implementation.
pub fn constants_lightyear_meters(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(9.460_730_472_580_8e15)
}
/// `constants_avogadro_n` — see implementation.
pub fn constants_avogadro_n(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(6.022_140_76e23)
}
/// `constants_boltzmann_k` — see implementation.
pub fn constants_boltzmann_k(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(1.380_649e-23)
}
/// `constants_gas_r` — see implementation.
pub fn constants_gas_r(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(8.314_462_618)
}
/// `constants_faraday` — see implementation.
pub fn constants_faraday(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(96_485.332_12)
}
/// `constants_rydberg` — see implementation.
pub fn constants_rydberg(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(1.097_373_156_816_0e7)
}
/// `constants_bohr_radius` — see implementation.
pub fn constants_bohr_radius(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(5.291_772_109_03e-11)
}
/// `constants_stefan_boltzmann` — see implementation.
pub fn constants_stefan_boltzmann(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(5.670_374_419e-8)
}

// ══════════════════════════════════════════════════════════════════════
// Case conversions
// ══════════════════════════════════════════════════════════════════════

fn split_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut prev_upper = false;
    for c in s.chars() {
        if c == '_' || c == '-' || c == '.' || c == '/' || c == ' ' {
            if !buf.is_empty() {
                out.push(buf.clone());
                buf.clear();
            }
        } else if c.is_ascii_uppercase() && !buf.is_empty() && !prev_upper {
            out.push(buf.clone());
            buf.clear();
            buf.push(c);
        } else {
            buf.push(c);
        }
        prev_upper = c.is_ascii_uppercase();
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}
/// `case_pascal` — see implementation.

pub fn case_pascal(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let out: String = split_words(&s)
        .iter()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => {
                    first.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase()
                }
                None => String::new(),
            }
        })
        .collect();
    StrykeValue::string(out)
}
/// `case_constant` — see implementation.

pub fn case_constant(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::string(
        split_words(&s)
            .iter()
            .map(|w| w.to_ascii_uppercase())
            .collect::<Vec<_>>()
            .join("_"),
    )
}
/// `case_dot` — see implementation.

pub fn case_dot(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::string(
        split_words(&s)
            .iter()
            .map(|w| w.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("."),
    )
}
/// `case_train` — see implementation.

pub fn case_train(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::string(
        split_words(&s)
            .iter()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    Some(first) => {
                        first.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase()
                    }
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join("-"),
    )
}
/// `case_path` — see implementation.

pub fn case_path(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::string(
        split_words(&s)
            .iter()
            .map(|w| w.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("/"),
    )
}
/// `case_sentence` — see implementation.

pub fn case_sentence(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let words = split_words(&s);
    if words.is_empty() {
        return StrykeValue::string(String::new());
    }
    let mut iter = words.iter();
    let first = iter.next().unwrap();
    let mut out = String::new();
    let mut fc = first.chars();
    if let Some(c) = fc.next() {
        out.push(c.to_ascii_uppercase());
        out.push_str(&fc.as_str().to_ascii_lowercase());
    }
    for w in iter {
        out.push(' ');
        out.push_str(&w.to_ascii_lowercase());
    }
    StrykeValue::string(out)
}
/// `case_title_proper` — see implementation.

pub fn case_title_proper(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let small_words = [
        "a", "an", "the", "and", "or", "but", "for", "nor", "of", "in", "on", "at", "to", "by",
    ];
    let words = split_words(&s);
    let n = words.len();
    let out: Vec<String> = words
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let lower = w.to_ascii_lowercase();
            if i != 0 && i != n - 1 && small_words.contains(&lower.as_str()) {
                lower
            } else {
                let mut c = w.chars();
                match c.next() {
                    Some(first) => {
                        first.to_ascii_uppercase().to_string() + &c.as_str().to_ascii_lowercase()
                    }
                    None => String::new(),
                }
            }
        })
        .collect();
    StrykeValue::string(out.join(" "))
}
/// `case_alternating` — see implementation.

pub fn case_alternating(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let out: String = s
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if i % 2 == 0 {
                c.to_ascii_lowercase()
            } else {
                c.to_ascii_uppercase()
            }
        })
        .collect();
    StrykeValue::string(out)
}
/// `case_swap` — see implementation.

pub fn case_swap(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_uppercase() {
                c.to_ascii_lowercase()
            } else if c.is_ascii_lowercase() {
                c.to_ascii_uppercase()
            } else {
                c
            }
        })
        .collect();
    StrykeValue::string(out)
}

// ══════════════════════════════════════════════════════════════════════
// Photography
// ══════════════════════════════════════════════════════════════════════
/// `exposure_value` — see implementation.

pub fn exposure_value(args: &[StrykeValue]) -> StrykeValue {
    let aperture = arg_f64(args, 0).unwrap_or(8.0);
    let shutter = arg_f64(args, 1).unwrap_or(1.0 / 250.0);
    let iso = arg_f64(args, 2).unwrap_or(100.0);
    StrykeValue::float((aperture * aperture / shutter * 100.0 / iso).log2())
}
/// `hyperfocal_distance` — see implementation.

pub fn hyperfocal_distance(args: &[StrykeValue]) -> StrykeValue {
    let focal_mm = arg_f64(args, 0).unwrap_or(50.0);
    let aperture = arg_f64(args, 1).unwrap_or(8.0);
    let coc = arg_f64(args, 2).unwrap_or(0.03);
    StrykeValue::float(focal_mm * focal_mm / (aperture * coc) + focal_mm)
}
/// `depth_of_field_near` — see implementation.

pub fn depth_of_field_near(args: &[StrykeValue]) -> StrykeValue {
    let focal_mm = arg_f64(args, 0).unwrap_or(50.0);
    let aperture = arg_f64(args, 1).unwrap_or(8.0);
    let dist_mm = arg_f64(args, 2).unwrap_or(2000.0);
    let coc = arg_f64(args, 3).unwrap_or(0.03);
    let h = focal_mm * focal_mm / (aperture * coc);
    StrykeValue::float(dist_mm * h / (h + dist_mm - focal_mm))
}
/// `depth_of_field_far` — see implementation.

pub fn depth_of_field_far(args: &[StrykeValue]) -> StrykeValue {
    let focal_mm = arg_f64(args, 0).unwrap_or(50.0);
    let aperture = arg_f64(args, 1).unwrap_or(8.0);
    let dist_mm = arg_f64(args, 2).unwrap_or(2000.0);
    let coc = arg_f64(args, 3).unwrap_or(0.03);
    let h = focal_mm * focal_mm / (aperture * coc);
    if dist_mm >= h {
        return StrykeValue::float(f64::INFINITY);
    }
    StrykeValue::float(dist_mm * h / (h - (dist_mm - focal_mm)))
}
/// `field_of_view` — see implementation.

pub fn field_of_view(args: &[StrykeValue]) -> StrykeValue {
    let focal_mm = arg_f64(args, 0).unwrap_or(50.0);
    let sensor_dim_mm = arg_f64(args, 1).unwrap_or(36.0);
    StrykeValue::float(2.0 * (sensor_dim_mm / (2.0 * focal_mm)).atan().to_degrees())
}
/// `focal_length_35mm_equiv` — see implementation.

pub fn focal_length_35mm_equiv(args: &[StrykeValue]) -> StrykeValue {
    let focal_mm = arg_f64(args, 0).unwrap_or(50.0);
    let crop = arg_f64(args, 1).unwrap_or(1.0);
    StrykeValue::float(focal_mm * crop)
}
/// `crop_factor` — see implementation.

pub fn crop_factor(args: &[StrykeValue]) -> StrykeValue {
    let sensor_diag_mm = arg_f64(args, 0).unwrap_or(43.27);
    let ref_diag_mm = 43.27_f64;
    StrykeValue::float(ref_diag_mm / sensor_diag_mm)
}
/// `shutter_speed_reciprocal` — see implementation.

pub fn shutter_speed_reciprocal(args: &[StrykeValue]) -> StrykeValue {
    let focal_mm = arg_f64(args, 0).unwrap_or(50.0);
    let crop = arg_f64(args, 1).unwrap_or(1.0);
    StrykeValue::float(1.0 / (focal_mm * crop))
}
/// `sunny_16_rule` — see implementation.

pub fn sunny_16_rule(args: &[StrykeValue]) -> StrykeValue {
    let iso = arg_f64(args, 0).unwrap_or(100.0);
    make_hash(vec![
        ("aperture", StrykeValue::float(16.0)),
        ("shutter", StrykeValue::float(1.0 / iso)),
        ("iso", StrykeValue::float(iso)),
    ])
}
/// `aperture_stop_to_fnumber` — see implementation.

pub fn aperture_stop_to_fnumber(args: &[StrykeValue]) -> StrykeValue {
    let stops = arg_f64(args, 0).unwrap_or(0.0);
    StrykeValue::float(2f64.powf(stops / 2.0))
}

// ══════════════════════════════════════════════════════════════════════
// Unit conversions
// ══════════════════════════════════════════════════════════════════════
/// `unit_volume_us_to_metric` — see implementation.

pub fn unit_volume_us_to_metric(args: &[StrykeValue]) -> StrykeValue {
    let value = arg_f64(args, 0).unwrap_or(0.0);
    let unit = arg_str(args, 1).unwrap_or_default();
    let ml = match unit.to_lowercase().as_str() {
        "tsp" | "teaspoon" => value * 4.92892,
        "tbsp" | "tablespoon" => value * 14.7868,
        "fl_oz" | "fluid_ounce" => value * 29.5735,
        "cup" => value * 236.588,
        "pint" | "pt" => value * 473.176,
        "quart" | "qt" => value * 946.353,
        "gallon" | "gal" => value * 3785.41,
        _ => return StrykeValue::UNDEF,
    };
    StrykeValue::float(ml)
}
/// `unit_volume_metric_to_us` — see implementation.

pub fn unit_volume_metric_to_us(args: &[StrykeValue]) -> StrykeValue {
    let ml = arg_f64(args, 0).unwrap_or(0.0);
    let unit = arg_str(args, 1).unwrap_or_default();
    let value = match unit.to_lowercase().as_str() {
        "tsp" | "teaspoon" => ml / 4.92892,
        "tbsp" | "tablespoon" => ml / 14.7868,
        "fl_oz" | "fluid_ounce" => ml / 29.5735,
        "cup" => ml / 236.588,
        "pint" | "pt" => ml / 473.176,
        "quart" | "qt" => ml / 946.353,
        "gallon" | "gal" => ml / 3785.41,
        _ => return StrykeValue::UNDEF,
    };
    StrykeValue::float(value)
}
/// `unit_temperature` — see implementation.

pub fn unit_temperature(args: &[StrykeValue]) -> StrykeValue {
    let value = arg_f64(args, 0).unwrap_or(0.0);
    let from = arg_str(args, 1).unwrap_or_default().to_lowercase();
    let to = arg_str(args, 2).unwrap_or_default().to_lowercase();
    let kelvin = match from.as_str() {
        "k" | "kelvin" => value,
        "c" | "celsius" => value + 273.15,
        "f" | "fahrenheit" => (value - 32.0) * 5.0 / 9.0 + 273.15,
        "r" | "rankine" => value * 5.0 / 9.0,
        _ => return StrykeValue::UNDEF,
    };
    let result = match to.as_str() {
        "k" | "kelvin" => kelvin,
        "c" | "celsius" => kelvin - 273.15,
        "f" | "fahrenheit" => (kelvin - 273.15) * 9.0 / 5.0 + 32.0,
        "r" | "rankine" => kelvin * 9.0 / 5.0,
        _ => return StrykeValue::UNDEF,
    };
    StrykeValue::float(result)
}
/// `unit_pressure` — see implementation.

pub fn unit_pressure(args: &[StrykeValue]) -> StrykeValue {
    let value = arg_f64(args, 0).unwrap_or(0.0);
    let from = arg_str(args, 1).unwrap_or_default().to_lowercase();
    let to = arg_str(args, 2).unwrap_or_default().to_lowercase();
    let pa = match from.as_str() {
        "pa" => value,
        "kpa" => value * 1000.0,
        "mpa" => value * 1.0e6,
        "bar" => value * 1.0e5,
        "atm" => value * 101325.0,
        "torr" | "mmhg" => value * 133.322,
        "psi" => value * 6894.76,
        _ => return StrykeValue::UNDEF,
    };
    let result = match to.as_str() {
        "pa" => pa,
        "kpa" => pa / 1000.0,
        "mpa" => pa / 1.0e6,
        "bar" => pa / 1.0e5,
        "atm" => pa / 101325.0,
        "torr" | "mmhg" => pa / 133.322,
        "psi" => pa / 6894.76,
        _ => return StrykeValue::UNDEF,
    };
    StrykeValue::float(result)
}
/// `unit_energy` — see implementation.

pub fn unit_energy(args: &[StrykeValue]) -> StrykeValue {
    let value = arg_f64(args, 0).unwrap_or(0.0);
    let from = arg_str(args, 1).unwrap_or_default().to_lowercase();
    let to = arg_str(args, 2).unwrap_or_default().to_lowercase();
    let joules = match from.as_str() {
        "j" | "joule" => value,
        "kj" => value * 1000.0,
        "cal" => value * 4.184,
        "kcal" => value * 4184.0,
        "wh" => value * 3600.0,
        "kwh" => value * 3.6e6,
        "ev" => value * 1.602176634e-19,
        "btu" => value * 1055.06,
        _ => return StrykeValue::UNDEF,
    };
    let result = match to.as_str() {
        "j" | "joule" => joules,
        "kj" => joules / 1000.0,
        "cal" => joules / 4.184,
        "kcal" => joules / 4184.0,
        "wh" => joules / 3600.0,
        "kwh" => joules / 3.6e6,
        "ev" => joules / 1.602176634e-19,
        "btu" => joules / 1055.06,
        _ => return StrykeValue::UNDEF,
    };
    StrykeValue::float(result)
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
    fn glicko_rd_grows_with_inactivity() {
        let new = glicko_rd_update(&[sv(50.0), sv(34.6), sv(10.0)]).to_number();
        assert!(new > 50.0);
    }

    #[test]
    fn arpad_predict_equal() {
        let r = arpad_predict(&[sv(1500.0), sv(1500.0)]).to_number();
        assert!((r - 0.5).abs() < 1e-9);
    }

    #[test]
    fn arpad_predict_400_higher_is_10x() {
        // 400 elo difference = ~91% win expectancy
        let r = arpad_predict(&[sv(1900.0), sv(1500.0)]).to_number();
        assert!((r - 0.909).abs() < 0.01);
    }

    #[test]
    fn kendall_tau_perfect() {
        let a = arr_f64(vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        let b = arr_f64(vec![10.0, 20.0, 30.0, 40.0, 50.0]);
        let r = ranking_kendall_tau(&[a, b]).to_number();
        assert!((r - 1.0).abs() < 1e-9);
    }

    #[test]
    fn dilation_grows() {
        let img = matrix_to_sv(&[
            vec![0.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 0.0],
        ]);
        let r = dilation_2d(&[img, sv_i(3)]);
        let m = as_matrix(&r);
        for row in m.iter() {
            for v in row {
                assert_eq!(*v, 1.0);
            }
        }
    }

    #[test]
    fn erosion_shrinks() {
        let img = matrix_to_sv(&[
            vec![1.0, 1.0, 1.0],
            vec![1.0, 1.0, 1.0],
            vec![1.0, 1.0, 1.0],
        ]);
        let r = erosion_2d(&[img, sv_i(3)]);
        let m = as_matrix(&r);
        // Only center stays 1.0 (corners get min including out-of-bounds 0)
        assert_eq!(m[1][1], 1.0);
        assert_eq!(m[0][0], 0.0);
    }

    #[test]
    fn graham_hull_square() {
        let pts = arr_sv(vec![
            arr_f64(vec![0.0, 0.0]),
            arr_f64(vec![1.0, 0.0]),
            arr_f64(vec![1.0, 1.0]),
            arr_f64(vec![0.0, 1.0]),
            arr_f64(vec![0.5, 0.5]), // interior point
        ]);
        let r = graham_scan_hull(&[pts]);
        let hull_pts = as_vec_sv(&r);
        assert_eq!(hull_pts.len(), 4);
    }

    #[test]
    fn andrew_hull_square() {
        let pts = arr_sv(vec![
            arr_f64(vec![0.0, 0.0]),
            arr_f64(vec![1.0, 0.0]),
            arr_f64(vec![1.0, 1.0]),
            arr_f64(vec![0.0, 1.0]),
            arr_f64(vec![0.5, 0.5]),
        ]);
        let r = andrew_monotone_hull(&[pts]);
        let hull_pts = as_vec_sv(&r);
        assert_eq!(hull_pts.len(), 4);
    }

    #[test]
    fn contour_area_unit_square() {
        let pts = arr_sv(vec![
            arr_f64(vec![0.0, 0.0]),
            arr_f64(vec![1.0, 0.0]),
            arr_f64(vec![1.0, 1.0]),
            arr_f64(vec![0.0, 1.0]),
        ]);
        assert!((contour_area(&[pts]).to_number() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn rsa_modular_exp_basic() {
        // 2^10 mod 100 = 24
        let r = rsa_modular_exp(&[sv_s("2"), sv_s("10"), sv_s("100")]);
        assert_eq!(r.as_str_or_empty(), "24");
    }

    #[test]
    fn rsa_keypair_textbook() {
        // p=61 q=53 → n=3233, e=17, d=2753
        let r = rsa_keypair_simple(&[sv_i(61), sv_i(53)]);
        if let Some(h) = r.as_hash_ref() {
            let h = h.read();
            assert_eq!(h.get("n").unwrap().to_int(), 3233);
            assert_eq!(h.get("e").unwrap().to_int(), 17);
            assert_eq!(h.get("d").unwrap().to_int(), 2753);
        }
    }

    #[test]
    fn constants_speed_of_light_exact() {
        assert_eq!(constants_speed_of_light(&[]).to_number(), 299_792_458.0);
    }

    #[test]
    fn constants_avogadro_exact() {
        assert_eq!(constants_avogadro_n(&[]).to_number(), 6.022_140_76e23);
    }

    #[test]
    fn case_pascal_basic() {
        assert_eq!(
            case_pascal(&[sv_s("hello_world")]).as_str_or_empty(),
            "HelloWorld"
        );
        assert_eq!(
            case_pascal(&[sv_s("foo-bar-baz")]).as_str_or_empty(),
            "FooBarBaz"
        );
    }

    #[test]
    fn case_constant_basic() {
        assert_eq!(
            case_constant(&[sv_s("helloWorld")]).as_str_or_empty(),
            "HELLO_WORLD"
        );
    }

    #[test]
    fn case_swap_basic() {
        assert_eq!(
            case_swap(&[sv_s("Hello World")]).as_str_or_empty(),
            "hELLO wORLD"
        );
    }

    #[test]
    fn case_alternating_basic() {
        assert_eq!(
            case_alternating(&[sv_s("hello")]).as_str_or_empty(),
            "hElLo"
        );
    }

    #[test]
    fn exposure_value_zero() {
        // EV=0 corresponds to f/1 at 1s at ISO 100
        let r = exposure_value(&[sv(1.0), sv(1.0), sv(100.0)]).to_number();
        assert!(r.abs() < 1e-6);
    }

    #[test]
    fn sunny_16_iso100() {
        let r = sunny_16_rule(&[sv(100.0)]);
        if let Some(h) = r.as_hash_ref() {
            let h = h.read();
            assert_eq!(h.get("aperture").unwrap().to_number(), 16.0);
            assert!((h.get("shutter").unwrap().to_number() - 0.01).abs() < 1e-9);
        }
    }

    #[test]
    fn unit_temperature_celsius_to_fahrenheit() {
        let r = unit_temperature(&[sv(100.0), sv_s("c"), sv_s("f")]).to_number();
        assert!((r - 212.0).abs() < 1e-9);
    }

    #[test]
    fn unit_volume_cup_to_ml() {
        let r = unit_volume_us_to_metric(&[sv(1.0), sv_s("cup")]).to_number();
        assert!((r - 236.588).abs() < 1e-3);
    }

    #[test]
    fn unit_energy_kwh_to_joules() {
        let r = unit_energy(&[sv(1.0), sv_s("kwh"), sv_s("j")]).to_number();
        assert!((r - 3.6e6).abs() < 1.0);
    }

    #[test]
    fn polygon_winding_ccw() {
        // CCW square
        let pts = arr_sv(vec![
            arr_f64(vec![0.0, 0.0]),
            arr_f64(vec![1.0, 0.0]),
            arr_f64(vec![1.0, 1.0]),
            arr_f64(vec![0.0, 1.0]),
        ]);
        assert_eq!(polygon_winding(&[pts]).to_int(), 1);
    }

    #[test]
    fn liang_barsky_clip_inside() {
        let p1 = arr_f64(vec![0.5, 0.5]);
        let p2 = arr_f64(vec![0.8, 0.8]);
        let rect = arr_f64(vec![0.0, 0.0, 1.0, 1.0]);
        let r = liang_barsky_clip(&[p1, p2, rect]);
        let xs = as_vec_sv(&r);
        assert_eq!(xs.len(), 2);
    }

    #[test]
    fn liang_barsky_clip_outside() {
        let p1 = arr_f64(vec![2.0, 2.0]);
        let p2 = arr_f64(vec![3.0, 3.0]);
        let rect = arr_f64(vec![0.0, 0.0, 1.0, 1.0]);
        let r = liang_barsky_clip(&[p1, p2, rect]);
        let xs = as_vec_sv(&r);
        assert_eq!(xs.len(), 0);
    }
}
