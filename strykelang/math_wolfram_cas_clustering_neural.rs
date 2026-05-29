// ─────────────────────────────────────────────────────────────────────────────
// Cross-language parity — additional Julia / R / Haskell / OCaml /
// scipy / SciPy.special / sklearn / statsmodels / OpenCV staples not yet in
// stryke. Layout: CAS-lite, more quadrature, more optimization, more
// distributions, clustering & dimensionality reduction, neural-net primitives,
// time-series advanced, image processing, spatial/geographic, integer
// sequences, graph metrics, 3-D geometry, classical iterative solvers,
// algebraic/crypto helpers, classical physics and chemistry. Included after
// `math_wolfram_autodiff_ode_glm.rs`.
// ─────────────────────────────────────────────────────────────────────────────

// ── 1. CAS-lite (real coefficients) ──────────────────────────────────────────

/// `factor_quadratic A, B, C` — return real roots of a x² + b x + c = 0.
fn builtin_factor_quadratic(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (a, b, c) = f3(args);
    if a.abs() < 1e-15 {
        if b.abs() < 1e-15 {
            return Ok(StrykeValue::array(vec![]));
        }
        return Ok(StrykeValue::array(vec![StrykeValue::float(-c / b)]));
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let s = disc.sqrt();
    Ok(StrykeValue::array(vec![
        StrykeValue::float((-b - s) / (2.0 * a)),
        StrykeValue::float((-b + s) / (2.0 * a)),
    ]))
}

/// `complete_square A, B, C` → `[h, k]` for a(x-h)² + k.
fn builtin_complete_square(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (a, b, c) = f3(args);
    if a.abs() < 1e-15 {
        return Err(StrykeError::runtime("complete_square: a must be non-zero", 0));
    }
    let h = -b / (2.0 * a);
    let k = c - b * b / (4.0 * a);
    Ok(StrykeValue::array(vec![StrykeValue::float(h), StrykeValue::float(k)]))
}

/// Heaviside-cover-up partial-fraction decomposition. Inputs: numerator
/// coefficients (low-to-high), distinct real roots of the denominator. Returns
/// residue at each root: r_i = num(root_i) / Π_{j≠i}(root_i - root_j).
fn builtin_partial_fraction_simple(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let num: Vec<f64> = poly_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let roots: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut out = Vec::with_capacity(roots.len());
    for (i, &r_i) in roots.iter().enumerate() {
        // num(r_i)
        let mut acc = 0.0_f64;
        for &c in num.iter().rev() {
            acc = acc * r_i + c;
        }
        // Π_{j≠i}(r_i - r_j)
        let mut denom = 1.0_f64;
        for (j, &r_j) in roots.iter().enumerate() {
            if i != j {
                denom *= r_i - r_j;
            }
        }
        out.push(StrykeValue::float(acc / denom));
    }
    Ok(StrykeValue::array(out))
}

// ── 2. More quadrature (callbacks) ───────────────────────────────────────────

fn cheb_t_nodes_weights(n: usize) -> (Vec<f64>, Vec<f64>) {
    // Chebyshev-Gauss with weight 1/√(1-x²): x_k = cos((2k-1)π/(2n)), w = π/n.
    let mut xs = vec![0.0_f64; n];
    let ws = vec![std::f64::consts::PI / n as f64; n];
    for k in 0..n {
        xs[k] = ((2 * k + 1) as f64 * std::f64::consts::PI / (2 * n) as f64).cos();
    }
    (xs, ws)
}

/// `gauss_chebyshev_quad` — Gauss chebyshev quad. Returns a float.
fn builtin_gauss_chebyshev_quad(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(16).max(2);
    let (xs, ws) = cheb_t_nodes_weights(n);
    let mut sum = 0.0_f64;
    for (xi, wi) in xs.iter().zip(ws.iter()) {
        sum += wi * call_user_1(interp, &f, *xi, line)?;
    }
    Ok(StrykeValue::float(sum))
}

fn gauss_hermite_nodes_weights(n: usize) -> (Vec<f64>, Vec<f64>) {
    // Eigenvalues of the Jacobi matrix with off-diagonal a_{i,i+1} = √((i+1)/2).
    let mut t = vec![vec![0.0_f64; n]; n];
    for i in 0..n - 1 {
        let v = ((i as f64 + 1.0) / 2.0).sqrt();
        t[i][i + 1] = v;
        t[i + 1][i] = v;
    }
    // Diagonalise (Jacobi rotation). We then have x_i = eigenvalues, w_i = √π · v_i,1².
    let mut v = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        v[i][i] = 1.0;
    }
    // Jacobi sweep (similar to elsewhere in this crate).
    for _ in 0..120 {
        let mut max_off = 0.0_f64;
        let (mut p, mut q) = (0_usize, 1_usize);
        for i in 0..n {
            for j in i + 1..n {
                if t[i][j].abs() > max_off {
                    max_off = t[i][j].abs();
                    p = i;
                    q = j;
                }
            }
        }
        if max_off < 1e-13 {
            break;
        }
        let theta = (t[q][q] - t[p][p]) / (2.0 * t[p][q]);
        let tt = if theta >= 0.0 {
            1.0 / (theta + (1.0 + theta * theta).sqrt())
        } else {
            1.0 / (theta - (1.0 + theta * theta).sqrt())
        };
        let c = 1.0 / (1.0 + tt * tt).sqrt();
        let s = tt * c;
        let app = t[p][p];
        let aqq = t[q][q];
        let apq = t[p][q];
        t[p][p] = c * c * app - 2.0 * s * c * apq + s * s * aqq;
        t[q][q] = s * s * app + 2.0 * s * c * apq + c * c * aqq;
        t[p][q] = 0.0;
        t[q][p] = 0.0;
        for i in 0..n {
            if i != p && i != q {
                let aip = t[i][p];
                let aiq = t[i][q];
                t[i][p] = c * aip - s * aiq;
                t[p][i] = t[i][p];
                t[i][q] = s * aip + c * aiq;
                t[q][i] = t[i][q];
            }
        }
        for i in 0..n {
            let vip = v[i][p];
            let viq = v[i][q];
            v[i][p] = c * vip - s * viq;
            v[i][q] = s * vip + c * viq;
        }
    }
    let mut pairs: Vec<(f64, f64)> = (0..n)
        .map(|i| (t[i][i], std::f64::consts::PI.sqrt() * v[0][i] * v[0][i]))
        .collect();
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let xs: Vec<f64> = pairs.iter().map(|(x, _)| *x).collect();
    let ws: Vec<f64> = pairs.iter().map(|(_, w)| *w).collect();
    (xs, ws)
}

/// `gauss_hermite_quad` — Gauss hermite quad. Returns a float.
fn builtin_gauss_hermite_quad(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(20).max(2);
    let (xs, ws) = gauss_hermite_nodes_weights(n);
    let mut sum = 0.0_f64;
    for (xi, wi) in xs.iter().zip(ws.iter()) {
        sum += wi * call_user_1(interp, &f, *xi, line)?;
    }
    Ok(StrykeValue::float(sum))
}

fn gauss_laguerre_nodes_weights(n: usize) -> (Vec<f64>, Vec<f64>) {
    // Tridiag with diag 2k+1, off-diag k+1 for k = 0..n-1.
    let mut t = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        t[i][i] = (2 * i + 1) as f64;
        if i + 1 < n {
            let v = i as f64 + 1.0;
            t[i][i + 1] = v;
            t[i + 1][i] = v;
        }
    }
    let mut v = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        v[i][i] = 1.0;
    }
    for _ in 0..120 {
        let mut max_off = 0.0_f64;
        let (mut p, mut q) = (0_usize, 1_usize);
        for i in 0..n {
            for j in i + 1..n {
                if t[i][j].abs() > max_off {
                    max_off = t[i][j].abs();
                    p = i;
                    q = j;
                }
            }
        }
        if max_off < 1e-13 {
            break;
        }
        let theta = (t[q][q] - t[p][p]) / (2.0 * t[p][q]);
        let tt = if theta >= 0.0 {
            1.0 / (theta + (1.0 + theta * theta).sqrt())
        } else {
            1.0 / (theta - (1.0 + theta * theta).sqrt())
        };
        let c = 1.0 / (1.0 + tt * tt).sqrt();
        let s = tt * c;
        let app = t[p][p];
        let aqq = t[q][q];
        let apq = t[p][q];
        t[p][p] = c * c * app - 2.0 * s * c * apq + s * s * aqq;
        t[q][q] = s * s * app + 2.0 * s * c * apq + c * c * aqq;
        t[p][q] = 0.0;
        t[q][p] = 0.0;
        for i in 0..n {
            if i != p && i != q {
                let aip = t[i][p];
                let aiq = t[i][q];
                t[i][p] = c * aip - s * aiq;
                t[p][i] = t[i][p];
                t[i][q] = s * aip + c * aiq;
                t[q][i] = t[i][q];
            }
        }
        for i in 0..n {
            let vip = v[i][p];
            let viq = v[i][q];
            v[i][p] = c * vip - s * viq;
            v[i][q] = s * vip + c * viq;
        }
    }
    let mut pairs: Vec<(f64, f64)> = (0..n).map(|i| (t[i][i], v[0][i] * v[0][i])).collect();
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let xs: Vec<f64> = pairs.iter().map(|(x, _)| *x).collect();
    let ws: Vec<f64> = pairs.iter().map(|(_, w)| *w).collect();
    (xs, ws)
}

/// `gauss_laguerre_quad` — Gauss laguerre quad. Returns a float.
fn builtin_gauss_laguerre_quad(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(20).max(2);
    let (xs, ws) = gauss_laguerre_nodes_weights(n);
    let mut sum = 0.0_f64;
    for (xi, wi) in xs.iter().zip(ws.iter()) {
        sum += wi * call_user_1(interp, &f, *xi, line)?;
    }
    Ok(StrykeValue::float(sum))
}

/// Clenshaw-Curtis quadrature on [-1, 1] (then scaled to [a, b]).
fn builtin_clenshaw_curtis_quad(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(3).map(|v| v.to_number() as usize).unwrap_or(64).max(2);
    let half = 0.5 * (b - a);
    let mid = 0.5 * (a + b);
    let mut sum = 0.0_f64;
    for k in 0..=n {
        let theta = std::f64::consts::PI * k as f64 / n as f64;
        let xk = mid + half * theta.cos();
        let mut w = 1.0 / (n as f64);
        let mut s = 0.0_f64;
        for j in 1..=n / 2 {
            let denom = 4.0 * j as f64 * j as f64 - 1.0;
            s += (2.0 * j as f64 * theta).cos() / denom;
        }
        w -= 2.0 / n as f64 * s;
        if k == 0 || k == n {
            w *= 0.5;
        }
        sum += 2.0 * w * call_user_1(interp, &f, xk, line)?;
    }
    Ok(StrykeValue::float(half * sum))
}

/// Tanh-sinh / double-exponential quadrature for finite [a, b].
fn builtin_tanh_sinh_quad(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let levels = args.get(3).map(|v| v.to_number() as i64).unwrap_or(6).max(2);
    let half = 0.5 * (b - a);
    let mid = 0.5 * (a + b);
    let mut sum = 0.0_f64;
    let h = std::f64::consts::PI / 2.0_f64.powi(levels as i32);
    let limit = (3.0 / h) as i64;
    for k in -limit..=limit {
        let t = k as f64 * h;
        let phi = (std::f64::consts::FRAC_PI_2 * t.sinh()).tanh();
        let dphi = (std::f64::consts::FRAC_PI_2 * t.sinh()).cosh().recip().powi(2)
            * (std::f64::consts::FRAC_PI_2 * t.cosh());
        if dphi.abs() < 1e-300 {
            continue;
        }
        let x = mid + half * phi;
        if x <= a || x >= b {
            continue;
        }
        sum += dphi * call_user_1(interp, &f, x, line)?;
    }
    Ok(StrykeValue::float(half * h * sum))
}

/// Cartesian-product Gauss-Legendre over [ax, bx] × [ay, by] with N nodes.
fn builtin_gauss_legendre_2d(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let ax = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let bx = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let ay = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let by = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(5).map(|v| v.to_number() as usize).unwrap_or(8).max(2);
    let (xs, ws) = gauss_legendre_nodes(n);
    let hx = 0.5 * (bx - ax);
    let mx = 0.5 * (ax + bx);
    let hy = 0.5 * (by - ay);
    let my = 0.5 * (ay + by);
    let sub = f
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("gauss_legendre_2d: code ref", line))?;
    let mut sum = 0.0_f64;
    for (xi, wxi) in xs.iter().zip(ws.iter()) {
        for (yj, wyj) in xs.iter().zip(ws.iter()) {
            let x = mx + hx * xi;
            let y = my + hy * yj;
            let r = exec_to_perl_result(
                interp.call_sub(
                    &sub,
                    vec![StrykeValue::float(x), StrykeValue::float(y)],
                    WantarrayCtx::Scalar,
                    line,
                ),
                "callback",
                line,
            )?;
            sum += wxi * wyj * r.to_number();
        }
    }
    Ok(StrykeValue::float(hx * hy * sum))
}

/// 2-D uniform Monte Carlo over a rectangular domain.
fn builtin_monte_carlo_2d(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let ax = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let bx = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let ay = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let by = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(5).map(|v| v.to_number() as usize).unwrap_or(10_000);
    let sub = f
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("monte_carlo_2d: code ref", line))?;
    let mut sum = 0.0_f64;
    let mut rng = rand::thread_rng();
    for _ in 0..n {
        let u: f64 = rng.gen();
        let v: f64 = rng.gen();
        let x = ax + (bx - ax) * u;
        let y = ay + (by - ay) * v;
        let r = exec_to_perl_result(
            interp.call_sub(
                &sub,
                vec![StrykeValue::float(x), StrykeValue::float(y)],
                WantarrayCtx::Scalar,
                line,
            ),
            "callback",
            line,
        )?;
        sum += r.to_number();
    }
    Ok(StrykeValue::float((bx - ax) * (by - ay) * sum / n as f64))
}

// ── 3. More optimization ─────────────────────────────────────────────────────

/// Simulated annealing with vector-valued state. Args: F (returns scalar),
/// X0, T0 (initial temperature), COOL (factor < 1), ITERS, STEP_SIZE.
fn builtin_simulated_annealing(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let x0: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let cool = args.get(3).map(|v| v.to_number()).unwrap_or(0.999);
    let iters = args.get(4).map(|v| v.to_number() as usize).unwrap_or(5000);
    let step_size = args.get(5).map(|v| v.to_number()).unwrap_or(0.5);
    let mut x = x0;
    let mut fx = call_user_n(interp, &f, x.clone(), line)?;
    let mut best_x = x.clone();
    let mut best_f = fx;
    let mut rng = rand::thread_rng();
    for _ in 0..iters {
        let mut cand = x.clone();
        for v in cand.iter_mut() {
            *v += step_size * (rng.gen_range(-1.0..1.0_f64));
        }
        let fc = call_user_n(interp, &f, cand.clone(), line)?;
        let delta = fc - fx;
        let u: f64 = rng.gen_range(1e-300..1.0);
        if delta < 0.0 || u < (-delta / t).exp() {
            x = cand;
            fx = fc;
            if fx < best_f {
                best_f = fx;
                best_x = x.clone();
            }
        }
        t *= cool;
        if t < 1e-10 {
            break;
        }
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::array(best_x.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::float(best_f),
    ]))
}

/// Two-phase simplex for `max cᵀ x  s.t. A x ≤ b, x ≥ 0` (b ≥ 0). Args: c, A, b.
/// Returns `[x*, value]`. Returns NaN-filled vector if infeasible/unbounded.
fn builtin_simplex_lp(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let c: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let a = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let b: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let m = a.len();
    let n = c.len();
    if m == 0 || n == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    // Tableau: [m × (n + m + 1)]; last col = b. Add slack for each constraint.
    let cols = n + m + 1;
    let mut t = vec![vec![0.0_f64; cols]; m + 1];
    for i in 0..m {
        for j in 0..n {
            t[i][j] = a[i][j];
        }
        t[i][n + i] = 1.0;
        t[i][cols - 1] = b[i];
    }
    for j in 0..n {
        t[m][j] = -c[j];
    }
    // Pivot.
    for _ in 0..200 {
        // Pick entering column = most-negative reduced cost.
        let mut piv_col = 0_usize;
        let mut min_v = 0.0_f64;
        for j in 0..cols - 1 {
            if t[m][j] < min_v - 1e-12 {
                min_v = t[m][j];
                piv_col = j;
            }
        }
        if min_v >= -1e-12 {
            break;
        }
        // Bland-style row pick: smallest positive ratio.
        let mut piv_row = usize::MAX;
        let mut min_ratio = f64::INFINITY;
        for i in 0..m {
            if t[i][piv_col] > 1e-12 {
                let r = t[i][cols - 1] / t[i][piv_col];
                if r < min_ratio {
                    min_ratio = r;
                    piv_row = i;
                }
            }
        }
        if piv_row == usize::MAX {
            return Ok(StrykeValue::array(vec![
                StrykeValue::float(f64::INFINITY),
                StrykeValue::float(f64::INFINITY),
            ]));
        }
        let pivot = t[piv_row][piv_col];
        for j in 0..cols {
            t[piv_row][j] /= pivot;
        }
        for i in 0..=m {
            if i == piv_row {
                continue;
            }
            let factor = t[i][piv_col];
            if factor.abs() < 1e-15 {
                continue;
            }
            for j in 0..cols {
                t[i][j] -= factor * t[piv_row][j];
            }
        }
    }
    // Read off solution: column j is basic if exactly one entry equals 1 and the rest 0.
    let mut x_out = vec![0.0_f64; n];
    for j in 0..n {
        let mut one_row = usize::MAX;
        let mut ok = true;
        for i in 0..m {
            if (t[i][j] - 1.0).abs() < 1e-9 {
                if one_row != usize::MAX {
                    ok = false;
                    break;
                }
                one_row = i;
            } else if t[i][j].abs() > 1e-9 {
                ok = false;
                break;
            }
        }
        if ok && one_row != usize::MAX {
            x_out[j] = t[one_row][cols - 1];
        }
    }
    let value = t[m][cols - 1];
    Ok(StrykeValue::array(vec![
        StrykeValue::array(x_out.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::float(value),
    ]))
}

/// Particle-swarm optimisation (constriction-coefficient form).
fn builtin_particle_swarm(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let bounds = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let swarm_size = args.get(2).map(|v| v.to_number() as usize).unwrap_or(30);
    let iters = args.get(3).map(|v| v.to_number() as usize).unwrap_or(200);
    let dim = bounds.len();
    let chi = 0.7298_f64;
    let phi = 2.05_f64;
    let mut rng = rand::thread_rng();
    let mut pos = vec![vec![0.0_f64; dim]; swarm_size];
    let mut vel = vec![vec![0.0_f64; dim]; swarm_size];
    let mut p_best = vec![vec![0.0_f64; dim]; swarm_size];
    let mut p_best_f = vec![f64::INFINITY; swarm_size];
    let mut g_best = vec![0.0_f64; dim];
    let mut g_best_f = f64::INFINITY;
    for i in 0..swarm_size {
        for d in 0..dim {
            let lo = bounds[d][0];
            let hi = bounds[d][1];
            pos[i][d] = rng.gen_range(lo..hi);
            vel[i][d] = rng.gen_range(-(hi - lo)..(hi - lo)) * 0.1;
        }
        let fx = call_user_n(interp, &f, pos[i].clone(), line)?;
        p_best[i] = pos[i].clone();
        p_best_f[i] = fx;
        if fx < g_best_f {
            g_best_f = fx;
            g_best = pos[i].clone();
        }
    }
    for _ in 0..iters {
        for i in 0..swarm_size {
            for d in 0..dim {
                let r1: f64 = rng.gen();
                let r2: f64 = rng.gen();
                vel[i][d] = chi
                    * (vel[i][d]
                        + phi * r1 * (p_best[i][d] - pos[i][d])
                        + phi * r2 * (g_best[d] - pos[i][d]));
                pos[i][d] = (pos[i][d] + vel[i][d]).clamp(bounds[d][0], bounds[d][1]);
            }
            let fx = call_user_n(interp, &f, pos[i].clone(), line)?;
            if fx < p_best_f[i] {
                p_best_f[i] = fx;
                p_best[i] = pos[i].clone();
                if fx < g_best_f {
                    g_best_f = fx;
                    g_best = pos[i].clone();
                }
            }
        }
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::array(g_best.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::float(g_best_f),
    ]))
}

// ── 4. More distributions ────────────────────────────────────────────────────

/// Generalized Extreme Value PDF.
fn builtin_gev_pdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let xi = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let s = (x - mu) / sigma;
    if xi.abs() < 1e-12 {
        return Ok(StrykeValue::float((-s - (-s).exp()).exp() / sigma));
    }
    if 1.0 + xi * s <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    let t = (1.0 + xi * s).powf(-1.0 / xi);
    Ok(StrykeValue::float(
        t.powf(xi + 1.0) * (-t).exp() / sigma,
    ))
}

/// `gev_cdf` — Gev cdf. Returns a float.
fn builtin_gev_cdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let xi = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let s = (x - mu) / sigma;
    if xi.abs() < 1e-12 {
        return Ok(StrykeValue::float((-(-s).exp()).exp()));
    }
    if 1.0 + xi * s <= 0.0 {
        return Ok(StrykeValue::float(if xi > 0.0 { 0.0 } else { 1.0 }));
    }
    Ok(StrykeValue::float((-(1.0 + xi * s).powf(-1.0 / xi)).exp()))
}

/// `gev_sample` — Gev sample. Returns a float.
fn builtin_gev_sample(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let mu = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let xi = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let mut rng = rand::thread_rng();
    let u: f64 = rng.gen_range(1e-300..1.0);
    if xi.abs() < 1e-12 {
        return Ok(StrykeValue::float(mu - sigma * (-u.ln()).ln()));
    }
    Ok(StrykeValue::float(
        mu + sigma * ((-u.ln()).powf(-xi) - 1.0) / xi,
    ))
}

/// Generalized Pareto PDF.
fn builtin_gen_pareto_pdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let xi = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let z = (x - mu) / sigma;
    if z < 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    if xi.abs() < 1e-12 {
        return Ok(StrykeValue::float((-z).exp() / sigma));
    }
    if 1.0 + xi * z <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    Ok(StrykeValue::float(
        (1.0 + xi * z).powf(-(1.0 / xi + 1.0)) / sigma,
    ))
}

/// `gen_pareto_cdf` — Gen pareto cdf. Returns a float.
fn builtin_gen_pareto_cdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let xi = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let z = (x - mu) / sigma;
    if z < 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    if xi.abs() < 1e-12 {
        return Ok(StrykeValue::float(1.0 - (-z).exp()));
    }
    Ok(StrykeValue::float(1.0 - (1.0 + xi * z).powf(-1.0 / xi)))
}

/// `gen_pareto_sample` — Gen pareto sample. Returns a float.
fn builtin_gen_pareto_sample(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let mu = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let xi = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let mut rng = rand::thread_rng();
    let u: f64 = rng.gen_range(1e-300..1.0);
    if xi.abs() < 1e-12 {
        return Ok(StrykeValue::float(mu - sigma * (1.0 - u).ln()));
    }
    Ok(StrykeValue::float(
        mu + sigma * ((1.0 - u).powf(-xi) - 1.0) / xi,
    ))
}

/// Skew-normal PDF (Azzalini).
fn builtin_skew_normal_pdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let xi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let omega = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let alpha = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let z = (x - xi) / omega;
    use statrs::function::erf::erf;
    let phi = (-z * z / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let big_phi = 0.5 * (1.0 + erf(alpha * z / std::f64::consts::SQRT_2));
    Ok(StrykeValue::float(2.0 / omega * phi * big_phi))
}

/// `skew_normal_cdf` — Skew normal cdf. Returns a float.
fn builtin_skew_normal_cdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    // Owen's T uses series expansion; here we integrate the PDF numerically.
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let xi = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let omega = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let alpha = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let lower = xi - 8.0 * omega;
    let n = 1024_usize;
    let h = (x - lower) / n as f64;
    let mut sum = 0.0_f64;
    for i in 0..=n {
        let t = lower + i as f64 * h;
        let v = builtin_skew_normal_pdf(&[
            StrykeValue::float(t),
            StrykeValue::float(xi),
            StrykeValue::float(omega),
            StrykeValue::float(alpha),
        ])?
        .to_number();
        let w = if i == 0 || i == n { 0.5 } else { 1.0 };
        sum += w * v;
    }
    Ok(StrykeValue::float(sum * h))
}

/// Mixture-of-Gaussians PDF: weights × means × stds.
fn builtin_mixture_normal_pdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let weights: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let means: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let stds: Vec<f64> = arg_to_vec(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut sum = 0.0_f64;
    for ((w, m), s) in weights.iter().zip(means.iter()).zip(stds.iter()) {
        let z = (x - m) / s;
        sum += w * (-z * z / 2.0).exp() / (s * (2.0 * std::f64::consts::PI).sqrt());
    }
    Ok(StrykeValue::float(sum))
}

/// Categorical (multinoulli) sample. Args: probability vector. Returns index.
fn builtin_categorical_sample(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let probs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let total: f64 = probs.iter().sum();
    let mut rng = rand::thread_rng();
    let mut u: f64 = rng.gen::<f64>() * total;
    for (i, &p) in probs.iter().enumerate() {
        u -= p;
        if u <= 0.0 {
            return Ok(StrykeValue::integer(i as i64));
        }
    }
    Ok(StrykeValue::integer(probs.len() as i64 - 1))
}

/// Multinomial PMF. Args: counts, probs.
fn builtin_multinomial_pmf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let counts: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let probs: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    use statrs::function::gamma::ln_gamma;
    let n: i64 = counts.iter().sum();
    let mut log_p = ln_gamma(n as f64 + 1.0);
    for (c, p) in counts.iter().zip(probs.iter()) {
        log_p -= ln_gamma(*c as f64 + 1.0);
        if *p > 0.0 {
            log_p += *c as f64 * p.ln();
        }
    }
    Ok(StrykeValue::float(log_p.exp()))
}

/// Multinomial sample. Args: n, probs. Returns counts vector.
fn builtin_multinomial_sample(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let n = args.first().map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let probs: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let total: f64 = probs.iter().sum();
    let mut counts = vec![0_i64; probs.len()];
    let mut rng = rand::thread_rng();
    for _ in 0..n {
        let mut u: f64 = rng.gen::<f64>() * total;
        for (i, &p) in probs.iter().enumerate() {
            u -= p;
            if u <= 0.0 {
                counts[i] += 1;
                break;
            }
        }
    }
    Ok(StrykeValue::array(counts.into_iter().map(StrykeValue::integer).collect()))
}

/// `truncated_normal_pdf` — Truncated normal pdf. Returns a float.
fn builtin_truncated_normal_pdf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let lo = args.get(3).map(|v| v.to_number()).unwrap_or(f64::NEG_INFINITY);
    let hi = args.get(4).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    if x < lo || x > hi {
        return Ok(StrykeValue::float(0.0));
    }
    use statrs::function::erf::erf;
    let cdf = |z: f64| 0.5 * (1.0 + erf(z / std::f64::consts::SQRT_2));
    let z = (x - mu) / sigma;
    let phi = (-z * z / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let z_lo = (lo - mu) / sigma;
    let z_hi = (hi - mu) / sigma;
    Ok(StrykeValue::float(
        phi / (sigma * (cdf(z_hi) - cdf(z_lo))),
    ))
}

/// `truncated_normal_sample` — Truncated normal sample. Returns a float.
fn builtin_truncated_normal_sample(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let mu = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let lo = args.get(2).map(|v| v.to_number()).unwrap_or(f64::NEG_INFINITY);
    let hi = args.get(3).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    let mut rng = rand::thread_rng();
    for _ in 0..1000 {
        let u1: f64 = rng.gen_range(1e-300..1.0);
        let u2: f64 = rng.gen();
        let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
        let x = mu + sigma * z;
        if x >= lo && x <= hi {
            return Ok(StrykeValue::float(x));
        }
    }
    Ok(StrykeValue::float(mu))
}

// ── 5. Clustering / dimensionality reduction ─────────────────────────────────

/// DBSCAN with squared Euclidean ε. Returns label per point (-1 = noise).
fn builtin_dbscan(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pts = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let eps = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let min_pts = args.get(2).map(|v| v.to_number() as usize).unwrap_or(5);
    let n = pts.len();
    let mut labels = vec![-2_i64; n];
    let mut cluster = 0_i64;
    let dist2 = |a: &[f64], b: &[f64]| {
        a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum::<f64>()
    };
    // sklearn convention: ε-neighbourhood includes the query point itself when
    // counting against `min_pts`.
    let neighbors = |i: usize| -> Vec<usize> {
        (0..n).filter(|&j| dist2(&pts[i], &pts[j]) <= eps * eps).collect()
    };
    for i in 0..n {
        if labels[i] != -2 {
            continue;
        }
        let nbrs = neighbors(i);
        if nbrs.len() < min_pts {
            labels[i] = -1;
            continue;
        }
        labels[i] = cluster;
        let mut stack = nbrs;
        while let Some(j) = stack.pop() {
            if labels[j] == -1 {
                labels[j] = cluster;
            }
            if labels[j] != -2 {
                continue;
            }
            labels[j] = cluster;
            let nb = neighbors(j);
            if nb.len() >= min_pts {
                stack.extend(nb);
            }
        }
        cluster += 1;
    }
    Ok(StrykeValue::array(labels.into_iter().map(StrykeValue::integer).collect()))
}

/// 1-D Gaussian-mixture EM. Args: data, k, max_iter. Returns [pi, mu, sigma].
fn builtin_gmm_em_1d(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let data: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2).max(1);
    let max_iter = args.get(2).map(|v| v.to_number() as usize).unwrap_or(100);
    let n = data.len();
    if n == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let mean = data.iter().sum::<f64>() / n as f64;
    let std = (data.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64).sqrt();
    let mut pi = vec![1.0 / k as f64; k];
    let mut mu = (0..k)
        .map(|j| mean + std * (j as f64 - (k as f64 - 1.0) / 2.0))
        .collect::<Vec<_>>();
    let mut sigma = vec![std.max(1e-3); k];
    let pdf = |x: f64, m: f64, s: f64| {
        let z = (x - m) / s;
        (-0.5 * z * z).exp() / (s * (2.0 * std::f64::consts::PI).sqrt())
    };
    for _ in 0..max_iter {
        // E-step.
        let mut resp = vec![vec![0.0_f64; k]; n];
        for i in 0..n {
            let mut total = 0.0_f64;
            for j in 0..k {
                resp[i][j] = pi[j] * pdf(data[i], mu[j], sigma[j]);
                total += resp[i][j];
            }
            if total > 0.0 {
                for j in 0..k {
                    resp[i][j] /= total;
                }
            }
        }
        // M-step.
        for j in 0..k {
            let nk: f64 = resp.iter().map(|r| r[j]).sum();
            if nk < 1e-12 {
                continue;
            }
            mu[j] = (0..n).map(|i| resp[i][j] * data[i]).sum::<f64>() / nk;
            sigma[j] = ((0..n)
                .map(|i| resp[i][j] * (data[i] - mu[j]).powi(2))
                .sum::<f64>()
                / nk)
                .sqrt()
                .max(1e-6);
            pi[j] = nk / n as f64;
        }
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::array(pi.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::array(mu.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::array(sigma.into_iter().map(StrykeValue::float).collect()),
    ]))
}

/// Silhouette score of a clustering (mean over points).
fn builtin_silhouette_score(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pts = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let labels: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let n = pts.len();
    if n == 0 {
        return Ok(StrykeValue::float(0.0));
    }
    let dist = |a: &[f64], b: &[f64]| {
        a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum::<f64>().sqrt()
    };
    let mut s_total = 0.0_f64;
    let mut count = 0_usize;
    for i in 0..n {
        let mut a_i = 0.0_f64;
        let mut a_n = 0_usize;
        let mut b_table: std::collections::HashMap<i64, (f64, usize)> =
            std::collections::HashMap::new();
        for j in 0..n {
            if i == j {
                continue;
            }
            let d = dist(&pts[i], &pts[j]);
            if labels[j] == labels[i] {
                a_i += d;
                a_n += 1;
            } else {
                let entry = b_table.entry(labels[j]).or_insert((0.0, 0));
                entry.0 += d;
                entry.1 += 1;
            }
        }
        if a_n == 0 || b_table.is_empty() {
            continue;
        }
        let a = a_i / a_n as f64;
        let b = b_table
            .values()
            .map(|(s, n)| s / *n as f64)
            .fold(f64::INFINITY, f64::min);
        let s = (b - a) / a.max(b);
        s_total += s;
        count += 1;
    }
    Ok(StrykeValue::float(if count == 0 { 0.0 } else { s_total / count as f64 }))
}

/// Davies-Bouldin clustering index (lower is better).
fn builtin_davies_bouldin_index(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pts = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let labels: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let n = pts.len();
    if n == 0 || labels.is_empty() {
        return Ok(StrykeValue::float(0.0));
    }
    let mut clusters: std::collections::HashMap<i64, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, &l) in labels.iter().enumerate() {
        clusters.entry(l).or_default().push(i);
    }
    let centroids: Vec<(i64, Vec<f64>)> = clusters
        .iter()
        .map(|(l, idxs)| {
            let dim = pts[idxs[0]].len();
            let mut c = vec![0.0_f64; dim];
            for &i in idxs {
                for d in 0..dim {
                    c[d] += pts[i][d];
                }
            }
            for v in c.iter_mut() {
                *v /= idxs.len() as f64;
            }
            (*l, c)
        })
        .collect();
    let s_cluster: Vec<(i64, f64)> = clusters
        .iter()
        .map(|(l, idxs)| {
            let c = &centroids.iter().find(|(ll, _)| ll == l).unwrap().1;
            let s: f64 = idxs
                .iter()
                .map(|&i| {
                    pts[i].iter().zip(c.iter()).map(|(x, y)| (x - y).powi(2)).sum::<f64>().sqrt()
                })
                .sum::<f64>()
                / idxs.len() as f64;
            (*l, s)
        })
        .collect();
    let k = centroids.len();
    let mut sum_max = 0.0_f64;
    for i in 0..k {
        let mut max_r = 0.0_f64;
        for j in 0..k {
            if i == j {
                continue;
            }
            let dist: f64 = centroids[i]
                .1
                .iter()
                .zip(centroids[j].1.iter())
                .map(|(x, y)| (x - y).powi(2))
                .sum::<f64>()
                .sqrt();
            let s_i = s_cluster[i].1;
            let s_j = s_cluster[j].1;
            if dist > 0.0 {
                let r = (s_i + s_j) / dist;
                if r > max_r {
                    max_r = r;
                }
            }
        }
        sum_max += max_r;
    }
    Ok(StrykeValue::float(sum_max / k as f64))
}

/// Calinski-Harabasz index (variance ratio criterion, higher is better).
fn builtin_calinski_harabasz_index(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pts = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let labels: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let n = pts.len();
    if n == 0 {
        return Ok(StrykeValue::float(0.0));
    }
    let dim = pts[0].len();
    let global_mean = {
        let mut g = vec![0.0_f64; dim];
        for p in &pts {
            for d in 0..dim {
                g[d] += p[d];
            }
        }
        for v in g.iter_mut() {
            *v /= n as f64;
        }
        g
    };
    let mut clusters: std::collections::HashMap<i64, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, &l) in labels.iter().enumerate() {
        clusters.entry(l).or_default().push(i);
    }
    let k = clusters.len();
    if k <= 1 {
        return Ok(StrykeValue::float(0.0));
    }
    let mut bk = 0.0_f64;
    let mut wk = 0.0_f64;
    for idxs in clusters.values() {
        let mut c = vec![0.0_f64; dim];
        for &i in idxs {
            for d in 0..dim {
                c[d] += pts[i][d];
            }
        }
        for v in c.iter_mut() {
            *v /= idxs.len() as f64;
        }
        let dist_sq_to_g: f64 = c
            .iter()
            .zip(global_mean.iter())
            .map(|(x, y)| (x - y).powi(2))
            .sum();
        bk += idxs.len() as f64 * dist_sq_to_g;
        for &i in idxs {
            wk += pts[i]
                .iter()
                .zip(c.iter())
                .map(|(x, y)| (x - y).powi(2))
                .sum::<f64>();
        }
    }
    if wk < 1e-12 {
        return Ok(StrykeValue::float(f64::INFINITY));
    }
    Ok(StrykeValue::float(
        (bk * (n - k) as f64) / (wk * (k - 1) as f64),
    ))
}

/// Classical multi-dimensional scaling (PCoA) on a distance matrix → 2-D coords.
fn builtin_mds_2d(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let d = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = d.len();
    if n == 0 {
        return Ok(matrix_to_value(&[]));
    }
    // Squared distances.
    let d2: Vec<Vec<f64>> = d
        .iter()
        .map(|row| row.iter().map(|x| x * x).collect())
        .collect();
    let row_means: Vec<f64> = (0..n)
        .map(|i| d2[i].iter().sum::<f64>() / n as f64)
        .collect();
    let col_means: Vec<f64> = (0..n)
        .map(|j| (0..n).map(|i| d2[i][j]).sum::<f64>() / n as f64)
        .collect();
    let grand_mean: f64 = d2.iter().flatten().sum::<f64>() / (n * n) as f64;
    let mut b = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        for j in 0..n {
            b[i][j] = -0.5 * (d2[i][j] - row_means[i] - col_means[j] + grand_mean);
        }
    }
    let evs = jacobi_eigenvalues(&mut b);
    let mut idx: Vec<usize> = (0..n).collect();
    idx.sort_by(|&a, &b2| evs[b2].partial_cmp(&evs[a]).unwrap_or(std::cmp::Ordering::Equal));
    let pick: Vec<usize> = idx[..2.min(n)].to_vec();
    let mut coords = vec![vec![0.0_f64; pick.len()]; n];
    for (col, &k) in pick.iter().enumerate() {
        let scale = evs[k].max(0.0).sqrt();
        for i in 0..n {
            coords[i][col] = scale * b[i][k];
        }
    }
    Ok(matrix_to_value(&coords))
}

/// Mean-shift clustering on points (Gaussian kernel, given bandwidth h).
fn builtin_mean_shift(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pts = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-6);
    let max_iter = args.get(2).map(|v| v.to_number() as usize).unwrap_or(50);
    let n = pts.len();
    if n == 0 {
        return Ok(matrix_to_value(&[]));
    }
    let dim = pts[0].len();
    let mut shifted = pts.clone();
    for _ in 0..max_iter {
        let mut next = shifted.clone();
        for i in 0..n {
            let mut num = vec![0.0_f64; dim];
            let mut den = 0.0_f64;
            for j in 0..n {
                let d2: f64 = shifted[i]
                    .iter()
                    .zip(pts[j].iter())
                    .map(|(x, y)| (x - y).powi(2))
                    .sum();
                let w = (-d2 / (2.0 * h * h)).exp();
                den += w;
                for d in 0..dim {
                    num[d] += w * pts[j][d];
                }
            }
            if den > 1e-12 {
                for d in 0..dim {
                    next[i][d] = num[d] / den;
                }
            }
        }
        let mut shift = 0.0_f64;
        for i in 0..n {
            for d in 0..dim {
                shift += (next[i][d] - shifted[i][d]).powi(2);
            }
        }
        shifted = next;
        if shift.sqrt() < 1e-5 {
            break;
        }
    }
    Ok(matrix_to_value(&shifted))
}

// ── 6. Neural-net primitives ─────────────────────────────────────────────────

/// `batch_norm` — Batch norm. Returns a float.
fn builtin_batch_norm(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let eps = args.get(1).map(|v| v.to_number()).unwrap_or(1e-5);
    if xs.is_empty() {
        return Ok(StrykeValue::array(vec![]));
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64;
    let denom = (var + eps).sqrt();
    Ok(StrykeValue::array(
        xs.into_iter().map(|x| StrykeValue::float((x - mean) / denom)).collect(),
    ))
}

/// `layer_norm xs, group_size [, eps]` — per-row LayerNorm over a flat vector
/// of N rows × `group_size` features each. Differs from BatchNorm: each row is
/// normalized using its OWN mean/variance (over the feature axis only), not
/// pooled across the batch.
fn builtin_layer_norm(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let group_size = args.get(1).map(|v| v.to_number() as usize).unwrap_or(xs.len()).max(1);
    let eps = args.get(2).map(|v| v.to_number()).unwrap_or(1e-5);
    if xs.is_empty() { return Ok(StrykeValue::array(vec![])); }
    let mut out: Vec<StrykeValue> = Vec::with_capacity(xs.len());
    for chunk in xs.chunks(group_size) {
        let n = chunk.len() as f64;
        let mean = chunk.iter().sum::<f64>() / n;
        let var = chunk.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        let denom = (var + eps).sqrt();
        for &x in chunk { out.push(StrykeValue::float((x - mean) / denom)); }
    }
    Ok(StrykeValue::array(out))
}

/// Dropout mask (shape n) with prob p of being zero.
fn builtin_dropout_mask(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let mut rng = rand::thread_rng();
    let mask: Vec<StrykeValue> = (0..n)
        .map(|_| {
            let u: f64 = rng.gen();
            StrykeValue::float(if u < p { 0.0 } else { 1.0 / (1.0 - p) })
        })
        .collect();
    Ok(StrykeValue::array(mask))
}

/// `max_pool_1d` — Max pool 1d. Returns a float.
fn builtin_max_pool_1d(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let win = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2).max(1);
    let mut out = Vec::new();
    let mut i = 0;
    while i + win <= xs.len() {
        let mut m = f64::NEG_INFINITY;
        for k in 0..win {
            m = m.max(xs[i + k]);
        }
        out.push(StrykeValue::float(m));
        i += win;
    }
    Ok(StrykeValue::array(out))
}

/// `avg_pool_1d` — Avg pool 1d. Returns a float.
fn builtin_avg_pool_1d(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let win = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2).max(1);
    let mut out = Vec::new();
    let mut i = 0;
    while i + win <= xs.len() {
        let s: f64 = xs[i..i + win].iter().sum();
        out.push(StrykeValue::float(s / win as f64));
        i += win;
    }
    Ok(StrykeValue::array(out))
}

/// Stable softmax: y_i = exp(x_i - max(x)) / Σ exp(x_j - max(x)).
fn builtin_attention_softmax(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    if xs.is_empty() {
        return Ok(StrykeValue::array(vec![]));
    }
    let mx = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = xs.iter().map(|x| (x - mx).exp()).collect();
    let total: f64 = exps.iter().sum();
    Ok(StrykeValue::array(
        exps.into_iter().map(|v| StrykeValue::float(v / total)).collect(),
    ))
}

/// Sinusoidal positional encoding of length `length` × `d_model`.
fn builtin_positional_encoding(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let length = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let d_model = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let mut m = vec![vec![0.0_f64; d_model]; length];
    for pos in 0..length {
        for i in 0..d_model {
            let div = 10000_f64.powf(2.0 * (i / 2) as f64 / d_model as f64);
            m[pos][i] = if i & 1 == 0 {
                (pos as f64 / div).sin()
            } else {
                (pos as f64 / div).cos()
            };
        }
    }
    Ok(matrix_to_value(&m))
}

/// Glorot (Xavier) uniform init.
fn builtin_glorot_init(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let fan_in = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let fan_out = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let limit = (6.0 / (fan_in + fan_out) as f64).sqrt();
    let mut rng = rand::thread_rng();
    let m: Vec<Vec<f64>> = (0..fan_in)
        .map(|_| (0..fan_out).map(|_| rng.gen_range(-limit..limit)).collect())
        .collect();
    Ok(matrix_to_value(&m))
}

/// He init (gaussian, std = √(2 / fan_in)).
fn builtin_he_init(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let fan_in = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let fan_out = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let std = (2.0 / fan_in as f64).sqrt();
    let mut rng = rand::thread_rng();
    let m: Vec<Vec<f64>> = (0..fan_in)
        .map(|_| {
            (0..fan_out)
                .map(|_| {
                    let u1: f64 = rng.gen_range(1e-300..1.0);
                    let u2: f64 = rng.gen();
                    std * (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
                })
                .collect()
        })
        .collect();
    Ok(matrix_to_value(&m))
}

/// Single Adam optimisation step. Args: param, grad, m, v, lr, beta1, beta2,
/// eps, t. Returns [param', m', v'].
#[allow(dead_code)]
fn builtin_adam_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let param: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let grad: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let m_old: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let v_old: Vec<f64> = arg_to_vec(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let lr = args.get(4).map(|v| v.to_number()).unwrap_or(0.001);
    let beta1 = args.get(5).map(|v| v.to_number()).unwrap_or(0.9);
    let beta2 = args.get(6).map(|v| v.to_number()).unwrap_or(0.999);
    let eps = args.get(7).map(|v| v.to_number()).unwrap_or(1e-8);
    let t = args.get(8).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let n = param.len();
    let mut m_new = vec![0.0_f64; n];
    let mut v_new = vec![0.0_f64; n];
    let mut p_new = vec![0.0_f64; n];
    for i in 0..n {
        let m_def = m_old.get(i).copied().unwrap_or(0.0);
        let v_def = v_old.get(i).copied().unwrap_or(0.0);
        m_new[i] = beta1 * m_def + (1.0 - beta1) * grad[i];
        v_new[i] = beta2 * v_def + (1.0 - beta2) * grad[i] * grad[i];
        let m_hat = m_new[i] / (1.0 - beta1.powf(t));
        let v_hat = v_new[i] / (1.0 - beta2.powf(t));
        p_new[i] = param[i] - lr * m_hat / (v_hat.sqrt() + eps);
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::array(p_new.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::array(m_new.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::array(v_new.into_iter().map(StrykeValue::float).collect()),
    ]))
}

/// Single RMSProp step. Args: param, grad, v_old, lr, decay, eps. Returns [param', v'].
#[allow(dead_code)]
fn builtin_rmsprop_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let param: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let grad: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let v_old: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let lr = args.get(3).map(|v| v.to_number()).unwrap_or(0.001);
    let decay = args.get(4).map(|v| v.to_number()).unwrap_or(0.9);
    let eps = args.get(5).map(|v| v.to_number()).unwrap_or(1e-8);
    let n = param.len();
    let mut v_new = vec![0.0_f64; n];
    let mut p_new = vec![0.0_f64; n];
    for i in 0..n {
        let v_def = v_old.get(i).copied().unwrap_or(0.0);
        v_new[i] = decay * v_def + (1.0 - decay) * grad[i] * grad[i];
        p_new[i] = param[i] - lr * grad[i] / (v_new[i].sqrt() + eps);
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::array(p_new.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::array(v_new.into_iter().map(StrykeValue::float).collect()),
    ]))
}

// ── 7. Time series advanced ──────────────────────────────────────────────────

/// `ewma` — Ewma. Returns a float.
fn builtin_ewma(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.3);
    let mut out = Vec::with_capacity(xs.len());
    let mut prev = 0.0_f64;
    for (i, &x) in xs.iter().enumerate() {
        if i == 0 {
            prev = x;
        } else {
            prev = alpha * x + (1.0 - alpha) * prev;
        }
        out.push(StrykeValue::float(prev));
    }
    Ok(StrykeValue::array(out))
}

/// `ccf` — Ccf. Returns a float.
fn builtin_ccf(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let ys: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let max_lag = args.get(2).map(|v| v.to_number() as usize).unwrap_or(10);
    let n = xs.len().min(ys.len());
    let mx = xs.iter().sum::<f64>() / n as f64;
    let my = ys.iter().sum::<f64>() / n as f64;
    let cx: Vec<f64> = xs.iter().map(|x| x - mx).collect();
    let cy: Vec<f64> = ys.iter().map(|y| y - my).collect();
    let dx: f64 = cx.iter().map(|v| v * v).sum::<f64>().sqrt();
    let dy: f64 = cy.iter().map(|v| v * v).sum::<f64>().sqrt();
    let mut out: Vec<StrykeValue> = Vec::with_capacity(2 * max_lag + 1);
    for lag in -(max_lag as i64)..=(max_lag as i64) {
        let mut s = 0.0_f64;
        for i in 0..n {
            let j = i as i64 + lag;
            if j >= 0 && (j as usize) < n {
                s += cx[i] * cy[j as usize];
            }
        }
        out.push(StrykeValue::float(s / (dx * dy)));
    }
    Ok(StrykeValue::array(out))
}

/// Periodogram (squared magnitude / N) at `m` Fourier bins.
fn builtin_periodogram(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = xs.len();
    let mut out = Vec::with_capacity(n / 2);
    for k in 0..n / 2 {
        let mut re = 0.0_f64;
        let mut im = 0.0_f64;
        for (i, &x) in xs.iter().enumerate() {
            let theta = 2.0 * std::f64::consts::PI * k as f64 * i as f64 / n as f64;
            re += x * theta.cos();
            im -= x * theta.sin();
        }
        out.push(StrykeValue::float((re * re + im * im) / n as f64));
    }
    Ok(StrykeValue::array(out))
}

/// Welch's PSD (segment averaging, Hann window).
fn builtin_welch_psd(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let seg_len = args.get(1).map(|v| v.to_number() as usize).unwrap_or(64).max(8);
    let overlap = args.get(2).map(|v| v.to_number() as usize).unwrap_or(seg_len / 2);
    let step = seg_len - overlap;
    let n = xs.len();
    let mut acc = vec![0.0_f64; seg_len / 2];
    let mut count = 0_usize;
    let mut start = 0_usize;
    while start + seg_len <= n {
        let win: Vec<f64> = (0..seg_len)
            .map(|i| {
                let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (seg_len as f64 - 1.0)).cos();
                xs[start + i] * w
            })
            .collect();
        let win_norm = win.iter().map(|w| w * w).sum::<f64>();
        if win_norm < 1e-12 {
            break;
        }
        for k in 0..seg_len / 2 {
            let mut re = 0.0_f64;
            let mut im = 0.0_f64;
            for (i, &x) in win.iter().enumerate() {
                let theta = 2.0 * std::f64::consts::PI * k as f64 * i as f64 / seg_len as f64;
                re += x * theta.cos();
                im -= x * theta.sin();
            }
            acc[k] += (re * re + im * im) / win_norm;
        }
        let _ = win.len();
        count += 1;
        start += step;
    }
    if count > 0 {
        for v in acc.iter_mut() {
            *v /= count as f64;
        }
    }
    Ok(StrykeValue::array(acc.into_iter().map(StrykeValue::float).collect()))
}

/// Build a lag-feature matrix for autoregressive learning.
fn builtin_lag_features(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let p = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    let n = xs.len();
    if n <= p {
        return Ok(matrix_to_value(&[]));
    }
    let mut m = vec![vec![0.0_f64; p]; n - p];
    for i in p..n {
        for j in 0..p {
            m[i - p][j] = xs[i - 1 - j];
        }
    }
    Ok(matrix_to_value(&m))
}

// ── 8. Image processing ──────────────────────────────────────────────────────

/// `median_filter_2d` — Median filter 2d.
fn builtin_median_filter_2d(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let img = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let radius = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let h = img.len();
    let w = if h == 0 { 0 } else { img[0].len() };
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            let mut window: Vec<f64> = Vec::new();
            for di in -radius..=radius {
                for dj in -radius..=radius {
                    let ii = i as i64 + di;
                    let jj = j as i64 + dj;
                    if ii >= 0 && (ii as usize) < h && jj >= 0 && (jj as usize) < w {
                        window.push(img[ii as usize][jj as usize]);
                    }
                }
            }
            window.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            out[i][j] = window[window.len() / 2];
        }
    }
    Ok(matrix_to_value(&out))
}

/// Otsu's threshold for grayscale 0..255.
fn builtin_threshold_otsu(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let img: Vec<u8> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .flat_map(|row| {
            arg_to_vec(row).into_iter().map(|v| v.to_number().clamp(0.0, 255.0) as u8)
        })
        .collect();
    let mut hist = [0_usize; 256];
    for &v in &img {
        hist[v as usize] += 1;
    }
    let total = img.len();
    let sum_total: f64 = (0..256).map(|i| i as f64 * hist[i] as f64).sum();
    let mut w_b = 0_usize;
    let mut sum_b = 0.0_f64;
    let mut max_var = 0.0_f64;
    let mut threshold = 0_u8;
    for t in 0..256 {
        w_b += hist[t];
        if w_b == 0 {
            continue;
        }
        let w_f = total - w_b;
        if w_f == 0 {
            break;
        }
        sum_b += t as f64 * hist[t] as f64;
        let mu_b = sum_b / w_b as f64;
        let mu_f = (sum_total - sum_b) / w_f as f64;
        let var = w_b as f64 * w_f as f64 * (mu_b - mu_f).powi(2);
        if var > max_var {
            max_var = var;
            threshold = t as u8;
        }
    }
    Ok(StrykeValue::integer(threshold as i64))
}

/// Histogram equalisation for grayscale 0..255.
fn builtin_histogram_equalize(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let img: Vec<Vec<u8>> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|row| {
            arg_to_vec(row)
                .iter()
                .map(|v| v.to_number().clamp(0.0, 255.0) as u8)
                .collect()
        })
        .collect();
    // Empty image → nothing to equalize. Without this guard the
    // `img[0].len()` access below OOB-panics.
    if img.is_empty() || img[0].is_empty() {
        return Ok(matrix_to_value(&[]));
    }
    let mut hist = [0_usize; 256];
    let mut total = 0_usize;
    for row in &img {
        for &v in row {
            hist[v as usize] += 1;
            total += 1;
        }
    }
    let mut cdf = [0_usize; 256];
    let mut acc = 0_usize;
    for i in 0..256 {
        acc += hist[i];
        cdf[i] = acc;
    }
    let cdf_min = cdf.iter().copied().find(|&v| v > 0).unwrap_or(0);
    let scale = 255.0 / (total - cdf_min).max(1) as f64;
    let mut out = vec![vec![0.0_f64; img[0].len()]; img.len()];
    for (i, row) in img.iter().enumerate() {
        for (j, &v) in row.iter().enumerate() {
            out[i][j] = ((cdf[v as usize] - cdf_min) as f64 * scale).round();
        }
    }
    Ok(matrix_to_value(&out))
}

/// `erode_2d` — Erode 2d.
fn builtin_erode_2d(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let img = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let radius = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let h = img.len();
    let w = if h == 0 { 0 } else { img[0].len() };
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            let mut m = f64::INFINITY;
            for di in -radius..=radius {
                for dj in -radius..=radius {
                    let ii = i as i64 + di;
                    let jj = j as i64 + dj;
                    if ii >= 0 && (ii as usize) < h && jj >= 0 && (jj as usize) < w {
                        m = m.min(img[ii as usize][jj as usize]);
                    }
                }
            }
            out[i][j] = m;
        }
    }
    Ok(matrix_to_value(&out))
}

/// `dilate_2d` — Dilate 2d.
fn builtin_dilate_2d(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let img = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let radius = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let h = img.len();
    let w = if h == 0 { 0 } else { img[0].len() };
    let mut out = vec![vec![0.0_f64; w]; h];
    for i in 0..h {
        for j in 0..w {
            let mut m = f64::NEG_INFINITY;
            for di in -radius..=radius {
                for dj in -radius..=radius {
                    let ii = i as i64 + di;
                    let jj = j as i64 + dj;
                    if ii >= 0 && (ii as usize) < h && jj >= 0 && (jj as usize) < w {
                        m = m.max(img[ii as usize][jj as usize]);
                    }
                }
            }
            out[i][j] = m;
        }
    }
    Ok(matrix_to_value(&out))
}

// ── 9. Loss functions ────────────────────────────────────────────────────────

/// `mse_loss` — Mse loss. Returns a float.
fn builtin_mse_loss(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (a, b) = vec_pair(args);
    let n = a.len().min(b.len()).max(1);
    let s: f64 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum();
    Ok(StrykeValue::float(s / n as f64))
}

/// `mae_loss` — Mae loss. Returns a float.
fn builtin_mae_loss(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (a, b) = vec_pair(args);
    let n = a.len().min(b.len()).max(1);
    let s: f64 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).abs()).sum();
    Ok(StrykeValue::float(s / n as f64))
}

/// `huber_loss` — Huber loss. Returns a float.
fn builtin_huber_loss(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (a, b) = vec_pair(args);
    let delta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = a.len().min(b.len()).max(1);
    let s: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| {
            let d = (x - y).abs();
            if d <= delta {
                0.5 * d * d
            } else {
                delta * (d - 0.5 * delta)
            }
        })
        .sum();
    Ok(StrykeValue::float(s / n as f64))
}

// ── 10. Spatial / geographic ─────────────────────────────────────────────────

/// Vincenty formula for geodesic distance (WGS-84 ellipsoid).
fn builtin_vincenty_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat1 = args.first().map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon1 = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lat2 = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon2 = args.get(3).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let a = 6378137.0_f64;
    let b = 6356752.314245_f64;
    let f = 1.0 / 298.257223563_f64;
    let l = lon2 - lon1;
    let u1 = ((1.0 - f) * lat1.tan()).atan();
    let u2 = ((1.0 - f) * lat2.tan()).atan();
    let sin_u1 = u1.sin();
    let cos_u1 = u1.cos();
    let sin_u2 = u2.sin();
    let cos_u2 = u2.cos();
    let mut lambda = l;
    for _ in 0..100 {
        let sin_l = lambda.sin();
        let cos_l = lambda.cos();
        let sin_sigma = ((cos_u2 * sin_l).powi(2)
            + (cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_l).powi(2))
        .sqrt();
        if sin_sigma < 1e-15 {
            return Ok(StrykeValue::float(0.0));
        }
        let cos_sigma = sin_u1 * sin_u2 + cos_u1 * cos_u2 * cos_l;
        let sigma = sin_sigma.atan2(cos_sigma);
        let sin_alpha = cos_u1 * cos_u2 * sin_l / sin_sigma;
        let cos_sq_alpha = 1.0 - sin_alpha * sin_alpha;
        let cos_2_sigma_m = if cos_sq_alpha == 0.0 {
            0.0
        } else {
            cos_sigma - 2.0 * sin_u1 * sin_u2 / cos_sq_alpha
        };
        let c = f / 16.0 * cos_sq_alpha * (4.0 + f * (4.0 - 3.0 * cos_sq_alpha));
        let lambda_new = l
            + (1.0 - c)
                * f
                * sin_alpha
                * (sigma
                    + c * sin_sigma
                        * (cos_2_sigma_m
                            + c * cos_sigma * (-1.0 + 2.0 * cos_2_sigma_m * cos_2_sigma_m)));
        if (lambda_new - lambda).abs() < 1e-12 {
            let u_sq = cos_sq_alpha * (a * a - b * b) / (b * b);
            let big_a = 1.0 + u_sq / 16384.0
                * (4096.0 + u_sq * (-768.0 + u_sq * (320.0 - 175.0 * u_sq)));
            let big_b = u_sq / 1024.0 * (256.0 + u_sq * (-128.0 + u_sq * (74.0 - 47.0 * u_sq)));
            let delta_sigma = big_b
                * sin_sigma
                * (cos_2_sigma_m
                    + big_b / 4.0
                        * (cos_sigma * (-1.0 + 2.0 * cos_2_sigma_m.powi(2))
                            - big_b / 6.0
                                * cos_2_sigma_m
                                * (-3.0 + 4.0 * sin_sigma.powi(2))
                                * (-3.0 + 4.0 * cos_2_sigma_m.powi(2))));
            return Ok(StrykeValue::float(b * big_a * (sigma - delta_sigma)));
        }
        lambda = lambda_new;
    }
    Ok(StrykeValue::float(f64::NAN))
}

/// `mercator_project` — Mercator project. Returns a float.
fn builtin_mercator_project(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let lon = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(6378137.0);
    let x = r * lon.to_radians();
    let y = r * (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0).tan().ln();
    Ok(StrykeValue::array(vec![StrykeValue::float(x), StrykeValue::float(y)]))
}

/// Destination given start lat/lon, bearing (deg), distance (m). Spherical earth.
fn builtin_destination_from_bearing(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat1 = args.first().map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon1 = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let bearing = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let distance = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(4).map(|v| v.to_number()).unwrap_or(6378137.0);
    let d = distance / r;
    let lat2 =
        (lat1.sin() * d.cos() + lat1.cos() * d.sin() * bearing.cos()).asin();
    let lon2 = lon1
        + (bearing.sin() * d.sin() * lat1.cos()).atan2(d.cos() - lat1.sin() * lat2.sin());
    Ok(StrykeValue::array(vec![
        StrykeValue::float(lat2.to_degrees()),
        StrykeValue::float(lon2.to_degrees()),
    ]))
}

// ── 11. Integer sequences ────────────────────────────────────────────────────

/// `recaman` — Recaman. Returns an integer.
fn builtin_recaman(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0) as usize;
    let mut seen: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let mut out: Vec<StrykeValue> = Vec::with_capacity(n + 1);
    let mut current = 0_i64;
    seen.insert(0);
    out.push(StrykeValue::integer(0));
    for k in 1..=n {
        let cand = current - k as i64;
        let next = if cand > 0 && !seen.contains(&cand) {
            cand
        } else {
            current + k as i64
        };
        seen.insert(next);
        out.push(StrykeValue::integer(next));
        current = next;
    }
    Ok(StrykeValue::array(out))
}

/// `sylvester` — Sylvester. Returns an integer.
fn builtin_sylvester(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0) as usize;
    let mut out: Vec<StrykeValue> = Vec::with_capacity(n + 1);
    let mut a: i128 = 2;
    out.push(StrykeValue::integer(2));
    for _ in 1..=n {
        a = a * a - a + 1;
        if a > i64::MAX as i128 {
            break;
        }
        out.push(StrykeValue::integer(a as i64));
    }
    Ok(StrykeValue::array(out))
}

/// `happy_q` — Happy q. Returns an integer.
fn builtin_happy_q(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut n = i1(args).abs();
    let mut seen = std::collections::HashSet::new();
    while n != 1 && !seen.contains(&n) {
        seen.insert(n);
        let mut s = 0_i64;
        let mut m = n;
        while m > 0 {
            s += (m % 10).pow(2);
            m /= 10;
        }
        n = s;
    }
    Ok(StrykeValue::integer(if n == 1 { 1 } else { 0 }))
}

/// `amicable_pair_q` — Amicable pair q. Returns an integer.
fn builtin_amicable_pair_q(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (a, b) = i2(args);
    let sum_proper = |x: i64| -> i64 {
        if x < 1 {
            return 0;
        }
        let mut s = 0_i64;
        let mut d = 1_i64;
        while d * d <= x {
            if x % d == 0 {
                if d != x {
                    s += d;
                }
                let other = x / d;
                if other != x && other != d {
                    s += other;
                }
            }
            d += 1;
        }
        s
    };
    Ok(StrykeValue::integer(
        if a > 1 && b > 1 && a != b && sum_proper(a) == b && sum_proper(b) == a {
            1
        } else {
            0
        },
    ))
}

/// `aliquot_sequence` — Aliquot sequence. Returns an integer.
fn builtin_aliquot_sequence(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut n = i1(args).max(1);
    let max_steps = args.get(1).map(|v| v.to_number() as usize).unwrap_or(50);
    let sum_proper = |x: i64| -> i64 {
        if x < 1 {
            return 0;
        }
        let mut s = 0_i64;
        let mut d = 1_i64;
        while d * d <= x {
            if x % d == 0 {
                if d != x {
                    s += d;
                }
                let other = x / d;
                if other != x && other != d {
                    s += other;
                }
            }
            d += 1;
        }
        s
    };
    let mut out: Vec<StrykeValue> = vec![StrykeValue::integer(n)];
    for _ in 0..max_steps {
        n = sum_proper(n);
        out.push(StrykeValue::integer(n));
        if n == 0 || n == 1 {
            break;
        }
    }
    Ok(StrykeValue::array(out))
}

/// Magic constant of an n×n magic square: M(n) = n(n²+1)/2.
fn builtin_magic_constant(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::integer(n * (n * n + 1) / 2))
}

// ── 12. Graph link metrics ───────────────────────────────────────────────────

/// `clustering_coefficient_local` — Clustering coefficient local. Returns a float.
fn builtin_clustering_coefficient_local(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let sets: Vec<std::collections::HashSet<usize>> = adj
        .iter()
        .map(|nbrs| nbrs.iter().copied().collect())
        .collect();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let k = adj[i].len();
        if k < 2 {
            out.push(StrykeValue::float(0.0));
            continue;
        }
        let mut tri = 0_usize;
        for &u in &adj[i] {
            for &v in &adj[i] {
                if u < v && sets[u].contains(&v) {
                    tri += 1;
                }
            }
        }
        out.push(StrykeValue::float(2.0 * tri as f64 / (k as f64 * (k as f64 - 1.0))));
    }
    Ok(StrykeValue::array(out))
}

/// `clustering_coefficient_global` — Clustering coefficient global. Returns a float.
fn builtin_clustering_coefficient_global(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let local = builtin_clustering_coefficient_local(args)?;
    let arr = arg_to_vec(&local);
    let s: f64 = arr.iter().map(|v| v.to_number()).sum();
    Ok(StrykeValue::float(if arr.is_empty() {
        0.0
    } else {
        s / arr.len() as f64
    }))
}

/// `assortativity` — Assortativity. Returns a float.
fn builtin_assortativity(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let degrees: Vec<f64> = adj.iter().map(|n| n.len() as f64).collect();
    let mut sum_jk = 0.0_f64;
    let mut sum_jpk = 0.0_f64;
    let mut sum_j2pk2 = 0.0_f64;
    let mut m = 0_usize;
    for (i, nbrs) in adj.iter().enumerate() {
        for &j in nbrs {
            if j > i {
                sum_jk += degrees[i] * degrees[j];
                sum_jpk += 0.5 * (degrees[i] + degrees[j]);
                sum_j2pk2 += 0.5 * (degrees[i].powi(2) + degrees[j].powi(2));
                m += 1;
            }
        }
    }
    if m == 0 {
        return Ok(StrykeValue::float(0.0));
    }
    let m_f = m as f64;
    let num = sum_jk / m_f - (sum_jpk / m_f).powi(2);
    let den = sum_j2pk2 / m_f - (sum_jpk / m_f).powi(2);
    Ok(StrykeValue::float(if den < 1e-15 { 0.0 } else { num / den }))
}

/// `common_neighbors` — Common neighbors. Returns an integer.
fn builtin_common_neighbors(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let u = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let v = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    if u >= adj.len() || v >= adj.len() {
        return Ok(StrykeValue::integer(0));
    }
    let s_u: std::collections::HashSet<usize> = adj[u].iter().copied().collect();
    Ok(StrykeValue::integer(
        adj[v].iter().filter(|w| s_u.contains(w)).count() as i64,
    ))
}

/// `jaccard_neighbors` — Jaccard neighbors. Returns a float.
fn builtin_jaccard_neighbors(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let u = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let v = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    if u >= adj.len() || v >= adj.len() {
        return Ok(StrykeValue::float(0.0));
    }
    let s_u: std::collections::HashSet<usize> = adj[u].iter().copied().collect();
    let s_v: std::collections::HashSet<usize> = adj[v].iter().copied().collect();
    let inter = s_u.intersection(&s_v).count();
    let union = s_u.union(&s_v).count();
    Ok(StrykeValue::float(if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64
    }))
}

/// `adamic_adar` — Adamic adar. Returns a float.
fn builtin_adamic_adar(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let u = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let v = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    if u >= adj.len() || v >= adj.len() {
        return Ok(StrykeValue::float(0.0));
    }
    let s_u: std::collections::HashSet<usize> = adj[u].iter().copied().collect();
    let mut sum = 0.0_f64;
    for w in &adj[v] {
        if s_u.contains(w) {
            let deg = adj[*w].len() as f64;
            if deg > 1.0 {
                sum += 1.0 / deg.ln();
            }
        }
    }
    Ok(StrykeValue::float(sum))
}

/// `preferential_attachment_score` — Preferential attachment score. Returns an integer.
fn builtin_preferential_attachment_score(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let u = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let v = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    if u >= adj.len() || v >= adj.len() {
        return Ok(StrykeValue::integer(0));
    }
    Ok(StrykeValue::integer((adj[u].len() * adj[v].len()) as i64))
}

// ── 13. 3-D geometry ─────────────────────────────────────────────────────────

fn vec3(v: &StrykeValue) -> [f64; 3] {
    let xs = arg_to_vec(v);
    [
        xs.first().map(|x| x.to_number()).unwrap_or(0.0),
        xs.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        xs.get(2).map(|x| x.to_number()).unwrap_or(0.0),
    ]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm3(a: [f64; 3]) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// `triangle_3d_normal` — Triangle 3d normal. Returns a float.
fn builtin_triangle_3d_normal(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p1 = vec3(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let p2 = vec3(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let p3 = vec3(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let n = cross3(sub3(p2, p1), sub3(p3, p1));
    let l = norm3(n).max(1e-15);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(n[0] / l),
        StrykeValue::float(n[1] / l),
        StrykeValue::float(n[2] / l),
    ]))
}

/// `triangle_3d_area` — Triangle 3d area. Returns a float.
fn builtin_triangle_3d_area(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p1 = vec3(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let p2 = vec3(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let p3 = vec3(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    Ok(StrykeValue::float(0.5 * norm3(cross3(sub3(p2, p1), sub3(p3, p1)))))
}

/// `tetrahedron_volume` — Tetrahedron volume. Returns a float.
fn builtin_tetrahedron_volume(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = vec3(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b = vec3(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let c = vec3(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let d = vec3(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF));
    let ab = sub3(b, a);
    let ac = sub3(c, a);
    let ad = sub3(d, a);
    Ok(StrykeValue::float(dot3(ab, cross3(ac, ad)).abs() / 6.0))
}

/// `plane_from_3_points` — Plane from 3 points. Returns a float.
fn builtin_plane_from_3_points(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p1 = vec3(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let p2 = vec3(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let p3 = vec3(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let n = cross3(sub3(p2, p1), sub3(p3, p1));
    let d = -dot3(n, p1);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(n[0]),
        StrykeValue::float(n[1]),
        StrykeValue::float(n[2]),
        StrykeValue::float(d),
    ]))
}

/// `point_to_plane_distance` — Point to plane distance. Returns a float.
fn builtin_point_to_plane_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pt = vec3(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let plane = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let n = [
        plane.first().map(|x| x.to_number()).unwrap_or(0.0),
        plane.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        plane.get(2).map(|x| x.to_number()).unwrap_or(0.0),
    ];
    let d = plane.get(3).map(|x| x.to_number()).unwrap_or(0.0);
    let l = norm3(n).max(1e-15);
    Ok(StrykeValue::float((dot3(n, pt) + d).abs() / l))
}

/// Möller-Trumbore ray-triangle intersection.
fn builtin_ray_triangle_intersect(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let origin = vec3(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let dir = vec3(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let a = vec3(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let b = vec3(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF));
    let c = vec3(&args.get(4).cloned().unwrap_or(StrykeValue::UNDEF));
    let ab = sub3(b, a);
    let ac = sub3(c, a);
    let h = cross3(dir, ac);
    let det = dot3(ab, h);
    if det.abs() < 1e-12 {
        return Ok(StrykeValue::float(f64::NAN));
    }
    let inv_det = 1.0 / det;
    let s = sub3(origin, a);
    let u = inv_det * dot3(s, h);
    if !(0.0..=1.0).contains(&u) {
        return Ok(StrykeValue::float(f64::NAN));
    }
    let q = cross3(s, ab);
    let v = inv_det * dot3(dir, q);
    if v < 0.0 || u + v > 1.0 {
        return Ok(StrykeValue::float(f64::NAN));
    }
    let t = inv_det * dot3(ac, q);
    if t < 0.0 {
        return Ok(StrykeValue::float(f64::NAN));
    }
    Ok(StrykeValue::float(t))
}

/// Ray-sphere intersection — returns nearest non-negative t or NaN.
fn builtin_ray_sphere_intersect(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let origin = vec3(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let dir = vec3(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let center = vec3(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let radius = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let oc = sub3(origin, center);
    let b = dot3(oc, dir);
    let c = dot3(oc, oc) - radius * radius;
    let disc = b * b - dot3(dir, dir) * c;
    if disc < 0.0 {
        return Ok(StrykeValue::float(f64::NAN));
    }
    let sq = disc.sqrt();
    let aa = dot3(dir, dir);
    let t1 = (-b - sq) / aa;
    let t2 = (-b + sq) / aa;
    if t1 >= 0.0 {
        Ok(StrykeValue::float(t1))
    } else if t2 >= 0.0 {
        Ok(StrykeValue::float(t2))
    } else {
        Ok(StrykeValue::float(f64::NAN))
    }
}

/// AABB (axis-aligned bounding box) overlap test in 3D.
fn builtin_aabb_overlap(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a_min = vec3(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let a_max = vec3(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let b_min = vec3(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let b_max = vec3(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF));
    let overlap = (a_min[0] <= b_max[0] && a_max[0] >= b_min[0])
        && (a_min[1] <= b_max[1] && a_max[1] >= b_min[1])
        && (a_min[2] <= b_max[2] && a_max[2] >= b_min[2]);
    Ok(StrykeValue::integer(if overlap { 1 } else { 0 }))
}

// ── 14. Iterative numerical solvers ──────────────────────────────────────────

/// `gauss_seidel` — Gauss seidel. Returns a list.
fn builtin_gauss_seidel(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let max_iter = args.get(2).map(|v| v.to_number() as usize).unwrap_or(500);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-8);
    let n = b.len();
    let mut x = vec![0.0_f64; n];
    for _ in 0..max_iter {
        let mut max_d = 0.0_f64;
        for i in 0..n {
            let mut s = b[i];
            for j in 0..n {
                if i != j {
                    s -= a[i][j] * x[j];
                }
            }
            let new_x = s / a[i][i];
            max_d = max_d.max((new_x - x[i]).abs());
            x[i] = new_x;
        }
        if max_d < tol {
            break;
        }
    }
    Ok(StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()))
}

/// `jacobi_iteration` — Jacobi iteration. Returns a list.
fn builtin_jacobi_iteration(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let max_iter = args.get(2).map(|v| v.to_number() as usize).unwrap_or(500);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-8);
    let n = b.len();
    let mut x = vec![0.0_f64; n];
    let mut x_new = vec![0.0_f64; n];
    for _ in 0..max_iter {
        for i in 0..n {
            let mut s = b[i];
            for j in 0..n {
                if i != j {
                    s -= a[i][j] * x[j];
                }
            }
            x_new[i] = s / a[i][i];
        }
        let max_d = x.iter().zip(x_new.iter()).map(|(a, b)| (a - b).abs()).fold(0.0_f64, f64::max);
        x.copy_from_slice(&x_new);
        if max_d < tol {
            break;
        }
    }
    Ok(StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()))
}

/// `sor_solve` — Sor solve. Returns a list.
fn builtin_sor_solve(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let omega = args.get(2).map(|v| v.to_number()).unwrap_or(1.5);
    let max_iter = args.get(3).map(|v| v.to_number() as usize).unwrap_or(500);
    let tol = args.get(4).map(|v| v.to_number()).unwrap_or(1e-8);
    let n = b.len();
    let mut x = vec![0.0_f64; n];
    for _ in 0..max_iter {
        let mut max_d = 0.0_f64;
        for i in 0..n {
            let mut s = b[i];
            for j in 0..n {
                if i != j {
                    s -= a[i][j] * x[j];
                }
            }
            let new_x = (1.0 - omega) * x[i] + omega * s / a[i][i];
            max_d = max_d.max((new_x - x[i]).abs());
            x[i] = new_x;
        }
        if max_d < tol {
            break;
        }
    }
    Ok(StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()))
}

/// Thomas algorithm: solves tridiagonal A x = d. Args: a (sub), b (main), c (super), d.
fn builtin_thomas_tridiag_solve(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let c: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut d: Vec<f64> = arg_to_vec(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = b.len();
    for i in 1..n {
        let m = a[i - 1] / b[i - 1];
        b[i] -= m * c[i - 1];
        d[i] -= m * d[i - 1];
    }
    let mut x = vec![0.0_f64; n];
    if n > 0 {
        x[n - 1] = d[n - 1] / b[n - 1];
        for i in (0..n - 1).rev() {
            x[i] = (d[i] - c[i] * x[i + 1]) / b[i];
        }
    }
    Ok(StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()))
}

/// Richardson extrapolation: F(h, h/2, …) → improved estimate.
fn builtin_richardson_extrapolation(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let h0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let levels = args.get(2).map(|v| v.to_number() as usize).unwrap_or(5).max(1);
    let mut t = vec![vec![0.0_f64; levels]; levels];
    for i in 0..levels {
        let h = h0 / 2.0_f64.powi(i as i32);
        t[i][0] = call_user_1(interp, &f, h, line)?;
        for j in 1..=i {
            t[i][j] = (4.0_f64.powi(j as i32) * t[i][j - 1] - t[i - 1][j - 1])
                / (4.0_f64.powi(j as i32) - 1.0);
        }
    }
    Ok(StrykeValue::float(t[levels - 1][levels - 1]))
}

/// 5-point central difference: f'(x) ≈ [-f(x+2h) + 8 f(x+h) - 8 f(x-h) + f(x-2h)] / 12h.
fn builtin_finite_difference_5pt(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(2).map(|v| v.to_number()).unwrap_or(1e-3);
    let f1 = call_user_1(interp, &f, x + 2.0 * h, line)?;
    let f2 = call_user_1(interp, &f, x + h, line)?;
    let f3 = call_user_1(interp, &f, x - h, line)?;
    let f4 = call_user_1(interp, &f, x - 2.0 * h, line)?;
    Ok(StrykeValue::float((-f1 + 8.0 * f2 - 8.0 * f3 + f4) / (12.0 * h)))
}

// ── 15. Algebraic / cryptographic ────────────────────────────────────────────

/// Tonelli-Shanks: solves x² ≡ n (mod p) with p odd prime. Returns smaller root or -1.
fn builtin_tonelli_shanks_sqrt(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).rem_euclid(args.get(1).map(|v| v.to_number() as i64).unwrap_or(2));
    let p = args.get(1).map(|v| v.to_number() as i64).unwrap_or(2);
    if p < 2 {
        return Ok(StrykeValue::integer(-1));
    }
    if n == 0 {
        return Ok(StrykeValue::integer(0));
    }
    // Quadratic-residue test.
    if mod_pow_i64(n, (p - 1) / 2, p) != 1 {
        return Ok(StrykeValue::integer(-1));
    }
    if p % 4 == 3 {
        return Ok(StrykeValue::integer(mod_pow_i64(n, (p + 1) / 4, p)));
    }
    let mut q = p - 1;
    let mut s = 0_i64;
    while q & 1 == 0 {
        q /= 2;
        s += 1;
    }
    let mut z = 2_i64;
    while mod_pow_i64(z, (p - 1) / 2, p) != p - 1 {
        z += 1;
    }
    let mut m = s;
    let mut c = mod_pow_i64(z, q, p);
    let mut t = mod_pow_i64(n, q, p);
    let mut r = mod_pow_i64(n, (q + 1) / 2, p);
    loop {
        if t == 1 {
            return Ok(StrykeValue::integer(r.min(p - r)));
        }
        let mut i = 0_i64;
        let mut tmp = t;
        while tmp != 1 && i < m {
            tmp = (tmp as i128 * tmp as i128 % p as i128) as i64;
            i += 1;
        }
        if i == m {
            return Ok(StrykeValue::integer(-1));
        }
        let b = mod_pow_i64(c, 1_i64 << (m - i - 1), p);
        m = i;
        c = (b as i128 * b as i128 % p as i128) as i64;
        t = (t as i128 * c as i128 % p as i128) as i64;
        r = (r as i128 * b as i128 % p as i128) as i64;
    }
}

/// Baby-step Giant-step: finds smallest x with g^x ≡ h (mod p), or -1.
fn builtin_baby_step_giant_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let g = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let h = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let p = args.get(2).map(|v| v.to_number() as i64).unwrap_or(2).max(2);
    let m = ((p as f64).sqrt().ceil() as i64).max(1);
    let mut table: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
    let mut e = 1_i64;
    for j in 0..m {
        table.entry(e).or_insert(j);
        e = (e as i128 * g as i128 % p as i128) as i64;
    }
    let factor = mod_pow_i64(g, m * (p - 2), p);
    let mut y = h.rem_euclid(p);
    for i in 0..m {
        if let Some(&j) = table.get(&y) {
            return Ok(StrykeValue::integer(i * m + j));
        }
        y = (y as i128 * factor as i128 % p as i128) as i64;
    }
    Ok(StrykeValue::integer(-1))
}

/// Pollard's rho factorisation. Returns one non-trivial factor of n (>1).
fn builtin_pollard_rho_factor(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    if n < 2 {
        return Ok(StrykeValue::integer(1));
    }
    if n % 2 == 0 {
        return Ok(StrykeValue::integer(2));
    }
    let f = |x: i64, c: i64| -> i64 {
        ((x as i128 * x as i128 + c as i128) % n as i128) as i64
    };
    for c in 1..50 {
        let mut x = 2_i64;
        let mut y = 2_i64;
        let mut d = 1_i64;
        while d == 1 {
            x = f(x, c);
            y = f(f(y, c), c);
            d = gcd_i64((x - y).abs(), n);
        }
        if d != n {
            return Ok(StrykeValue::integer(d));
        }
    }
    Ok(StrykeValue::integer(n))
}

/// Modular LCM via gcd-based formula.
fn builtin_modular_lcm(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let mut acc = 1_i64;
    for &x in &xs {
        let g = gcd_i64(acc, x);
        if g == 0 {
            continue;
        }
        acc = acc / g * x;
    }
    Ok(StrykeValue::integer(acc))
}

/// Generalised Chinese Remainder over arbitrary (not coprime) moduli.
fn builtin_crt_general(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let m: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    if r.len() != m.len() || r.is_empty() {
        return Err(StrykeError::runtime("crt_general: mismatched arrays", 0));
    }
    let mut rr = r[0];
    let mut mm = m[0];
    for i in 1..r.len() {
        let g = gcd_i64(mm, m[i]);
        if (r[i] - rr).rem_euclid(g) != 0 {
            return Ok(StrykeValue::integer(-1));
        }
        let lcm = mm / g * m[i];
        let m1g = mm / g;
        let inv = {
            // inverse of m1g modulo m[i]/g
            let modulus = m[i] / g;
            let mut old_r = m1g;
            let mut r = modulus;
            let mut old_s = 1_i64;
            let mut s = 0_i64;
            while r != 0 {
                let q = old_r / r;
                let tmp = r;
                r = old_r - q * r;
                old_r = tmp;
                let tmp = s;
                s = old_s - q * s;
                old_s = tmp;
            }
            ((old_s % modulus) + modulus) % modulus
        };
        let diff = (r[i] - rr).rem_euclid(m[i]) / g;
        rr = (rr + mm * (diff * inv).rem_euclid(m[i] / g)) % lcm;
        mm = lcm;
    }
    Ok(StrykeValue::integer(((rr % mm) + mm) % mm))
}

// ── 16. Physics / chemistry ──────────────────────────────────────────────────

/// Van-der-Waals pressure: P = nRT/(V-nb) − a n²/V².
fn builtin_van_der_waals_p(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(298.0);
    let v = args.get(2).map(|v| v.to_number()).unwrap_or(0.0224);
    let a = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let r = 8.314462618_f64;
    Ok(StrykeValue::float(n * r * t / (v - n * b) - a * n * n / (v * v)))
}

/// Nernst equation: E = E0 - (RT/nF) ln(Q).
fn builtin_nernst_equation(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let e0 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    let q = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let r = 8.314462618_f64;
    let f = 96485.33212_f64;
    Ok(StrykeValue::float(e0 - r * t / (n * f) * q.ln()))
}

/// Arrhenius rate: k = A exp(-Ea / RT).
fn builtin_arrhenius_rate(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let ea = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(298.15);
    let r = 8.314462618_f64;
    Ok(StrykeValue::float(a * (-ea / (r * t)).exp()))
}

/// `reduced_mass` — Reduced mass. Returns a float.
fn builtin_reduced_mass(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m1 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let m2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if m1 + m2 <= 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    Ok(StrykeValue::float(m1 * m2 / (m1 + m2)))
}

/// `ph_to_concentration` — Ph to concentration. Returns a float.
fn builtin_ph_to_concentration(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let ph = args.first().map(|v| v.to_number()).unwrap_or(7.0);
    Ok(StrykeValue::float(10.0_f64.powf(-ph)))
}
