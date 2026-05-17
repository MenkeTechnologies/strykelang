// ─────────────────────────────────────────────────────────────────────────────
// Wolfram-Math parity: vector calculus, optimization, numerical
// integration, LA extras, polynomial helpers, quaternions/3D rotations,
// information theory, quantum primitives, stat mech, optics, astrodynamics,
// time series, graph centrality, random samplers for new distributions, 2-D
// convex hull, line geometry. Included from `builtins.rs` after
// `math_wolfram_number_theory_combinatorics.rs`. Helpers (`f1..f4`, `i1`, `i2`, `arg_to_vec`,
// `mat_mul`, `matrix_from_value`, `matrix_to_value`, `matrix_det_f64`,
// `gcd_i64`, `prime_factorize`, `binomial_f`) come from earlier modules.
// ─────────────────────────────────────────────────────────────────────────────

// `VMHelper`, `WantarrayCtx`, `exec_to_perl_result` are already in scope via
// `builtins.rs` (this file is `include!`'d).

// ── Helpers for callable args ────────────────────────────────────────────────

fn call_user_n(
    interp: &mut VMHelper,
    f: &StrykeValue,
    xs: Vec<f64>,
    line: usize,
) -> StrykeResult<f64> {
    let sub = f
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("expected code ref", line))?;
    // Multi-D: pass a single arrayref so the callback receives one reference
    // (`fn($pt) { my @x = @$pt }`) instead of a flattened arg list.
    let arr = Arc::new(RwLock::new(
        xs.into_iter().map(StrykeValue::float).collect::<Vec<_>>(),
    ));
    let args = vec![StrykeValue::array_ref(arr)];
    let r = exec_to_perl_result(
        interp.call_sub(&sub, args, WantarrayCtx::Scalar, line),
        "callback",
        line,
    )?;
    Ok(r.to_number())
}

/// 1-D scalar-callback variant for integrators / quadrature where users
/// expect `sub { $_[0] ** 2 }`.
fn call_user_1(
    interp: &mut VMHelper,
    f: &StrykeValue,
    x: f64,
    line: usize,
) -> StrykeResult<f64> {
    let sub = f
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("expected code ref", line))?;
    let r = exec_to_perl_result(
        interp.call_sub(&sub, vec![StrykeValue::float(x)], WantarrayCtx::Scalar, line),
        "callback",
        line,
    )?;
    Ok(r.to_number())
}

fn call_user_vec(
    interp: &mut VMHelper,
    f: &StrykeValue,
    xs: &[f64],
    line: usize,
) -> StrykeResult<Vec<f64>> {
    let sub = f
        .as_code_ref()
        .ok_or_else(|| StrykeError::runtime("expected code ref", line))?;
    let arr = Arc::new(RwLock::new(
        xs.iter().copied().map(StrykeValue::float).collect::<Vec<_>>(),
    ));
    let args = vec![StrykeValue::array_ref(arr)];
    let r = exec_to_perl_result(
        interp.call_sub(&sub, args, WantarrayCtx::Scalar, line),
        "callback",
        line,
    )?;
    Ok(arg_to_vec(&r).iter().map(|v| v.to_number()).collect())
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Vector calculus — central differences with adaptive step
// ─────────────────────────────────────────────────────────────────────────────

fn step(x: f64) -> f64 {
    let s = (x.abs() * 1e-6).max(1e-7);
    if x + s == x {
        1e-7
    } else {
        s
    }
}

/// `numerical_gradient F, POINT [, H]` — ∇f at the given point. F takes a
/// vector and returns a scalar. Returns the gradient as a flat array.
fn builtin_numerical_gradient(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let mut p: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let h_override = args.get(2).map(|v| v.to_number());
    let mut grad = vec![0.0_f64; p.len()];
    for i in 0..p.len() {
        let h = h_override.unwrap_or_else(|| step(p[i]));
        let original = p[i];
        p[i] = original + h;
        let f_plus = call_user_n(interp, &f, p.clone(), line)?;
        p[i] = original - h;
        let f_minus = call_user_n(interp, &f, p.clone(), line)?;
        p[i] = original;
        grad[i] = (f_plus - f_minus) / (2.0 * h);
    }
    Ok(StrykeValue::array(grad.into_iter().map(StrykeValue::float).collect()))
}

/// `numerical_jacobian F, POINT [, H]` — Jacobian of vector-valued F.
fn builtin_numerical_jacobian(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let mut p: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let f_at = call_user_vec(interp, &f, &p, line)?;
    let m = f_at.len();
    let n = p.len();
    let mut j = vec![vec![0.0_f64; n]; m];
    for col in 0..n {
        let h = step(p[col]);
        let original = p[col];
        p[col] = original + h;
        let fp = call_user_vec(interp, &f, &p, line)?;
        p[col] = original - h;
        let fm = call_user_vec(interp, &f, &p, line)?;
        p[col] = original;
        for row in 0..m {
            j[row][col] = (fp[row] - fm[row]) / (2.0 * h);
        }
    }
    Ok(matrix_to_value(&j))
}

/// `numerical_hessian F, POINT [, H]` — Hessian of scalar F.
fn builtin_numerical_hessian(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let mut p: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = p.len();
    let mut h = vec![vec![0.0_f64; n]; n];
    let f0 = call_user_n(interp, &f, p.clone(), line)?;
    for i in 0..n {
        for j in 0..n {
            let hi = step(p[i]);
            let hj = step(p[j]);
            if i == j {
                let oi = p[i];
                p[i] = oi + hi;
                let fp = call_user_n(interp, &f, p.clone(), line)?;
                p[i] = oi - hi;
                let fm = call_user_n(interp, &f, p.clone(), line)?;
                p[i] = oi;
                h[i][j] = (fp - 2.0 * f0 + fm) / (hi * hi);
            } else {
                let (oi, oj) = (p[i], p[j]);
                p[i] = oi + hi;
                p[j] = oj + hj;
                let fpp = call_user_n(interp, &f, p.clone(), line)?;
                p[i] = oi + hi;
                p[j] = oj - hj;
                let fpm = call_user_n(interp, &f, p.clone(), line)?;
                p[i] = oi - hi;
                p[j] = oj + hj;
                let fmp = call_user_n(interp, &f, p.clone(), line)?;
                p[i] = oi - hi;
                p[j] = oj - hj;
                let fmm = call_user_n(interp, &f, p.clone(), line)?;
                p[i] = oi;
                p[j] = oj;
                h[i][j] = (fpp - fpm - fmp + fmm) / (4.0 * hi * hj);
            }
        }
    }
    Ok(matrix_to_value(&h))
}

/// `numerical_divergence F, POINT [, H]` — div of vector field F.
fn builtin_numerical_divergence(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let mut p: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut sum = 0.0_f64;
    for i in 0..p.len() {
        let h = step(p[i]);
        let oi = p[i];
        p[i] = oi + h;
        let fp = call_user_vec(interp, &f, &p, line)?;
        p[i] = oi - h;
        let fm = call_user_vec(interp, &f, &p, line)?;
        p[i] = oi;
        if i < fp.len() {
            sum += (fp[i] - fm[i]) / (2.0 * h);
        }
    }
    Ok(StrykeValue::float(sum))
}

/// `numerical_curl F, POINT [, H]` — 3D curl of F. Returns 3-vector.
fn builtin_numerical_curl(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let mut p: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    if p.len() != 3 {
        return Err(StrykeError::runtime(
            "numerical_curl: requires 3-D point",
            line,
        ));
    }
    let mut partial = |k: usize, comp: usize| -> StrykeResult<f64> {
        let h = step(p[k]);
        let ok = p[k];
        p[k] = ok + h;
        let fp = call_user_vec(interp, &f, &p, line)?;
        p[k] = ok - h;
        let fm = call_user_vec(interp, &f, &p, line)?;
        p[k] = ok;
        Ok((fp[comp] - fm[comp]) / (2.0 * h))
    };
    let dy_fz = partial(1, 2)?;
    let dz_fy = partial(2, 1)?;
    let dz_fx = partial(2, 0)?;
    let dx_fz = partial(0, 2)?;
    let dx_fy = partial(0, 1)?;
    let dy_fx = partial(1, 0)?;
    Ok(StrykeValue::array(vec![
        StrykeValue::float(dy_fz - dz_fy),
        StrykeValue::float(dz_fx - dx_fz),
        StrykeValue::float(dx_fy - dy_fx),
    ]))
}

/// `numerical_laplacian F, POINT [, H]` — ∇²f via second derivatives.
fn builtin_numerical_laplacian(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let mut p: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let f0 = call_user_n(interp, &f, p.clone(), line)?;
    let mut sum = 0.0_f64;
    for i in 0..p.len() {
        let h = step(p[i]);
        let oi = p[i];
        p[i] = oi + h;
        let fp = call_user_n(interp, &f, p.clone(), line)?;
        p[i] = oi - h;
        let fm = call_user_n(interp, &f, p.clone(), line)?;
        p[i] = oi;
        sum += (fp - 2.0 * f0 + fm) / (h * h);
    }
    Ok(StrykeValue::float(sum))
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Optimization
// ─────────────────────────────────────────────────────────────────────────────

/// Nelder-Mead simplex on F(vector). Returns [x*, f(x*)].
fn builtin_nelder_mead(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let x0: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let max_iter = args.get(2).map(|v| v.to_number() as usize).unwrap_or(2000);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-10);
    let n = x0.len();
    if n == 0 {
        return Err(StrykeError::runtime("nelder_mead: empty start", line));
    }
    // Build initial simplex via Spendley/Pearson scheme.
    let mut simplex: Vec<Vec<f64>> = Vec::with_capacity(n + 1);
    simplex.push(x0.clone());
    for i in 0..n {
        let mut v = x0.clone();
        v[i] += if v[i].abs() > 1e-9 {
            0.05 * v[i]
        } else {
            0.00025
        };
        simplex.push(v);
    }
    let mut fx: Vec<f64> = Vec::with_capacity(n + 1);
    for v in &simplex {
        fx.push(call_user_n(interp, &f, v.clone(), line)?);
    }
    let (alpha, gamma_e, rho, sigma) = (1.0_f64, 2.0_f64, 0.5_f64, 0.5_f64);
    for _ in 0..max_iter {
        // Sort vertices by f value.
        let mut idx: Vec<usize> = (0..=n).collect();
        idx.sort_by(|a, b| fx[*a].partial_cmp(&fx[*b]).unwrap_or(std::cmp::Ordering::Equal));
        let order: Vec<Vec<f64>> = idx.iter().map(|&i| simplex[i].clone()).collect();
        let order_f: Vec<f64> = idx.iter().map(|&i| fx[i]).collect();
        simplex = order;
        fx = order_f;
        // Convergence check.
        let spread = fx[n] - fx[0];
        if spread.abs() < tol {
            break;
        }
        // Centroid of all but worst.
        let mut centroid = vec![0.0_f64; n];
        for v in simplex.iter().take(n) {
            for j in 0..n {
                centroid[j] += v[j];
            }
        }
        for c in centroid.iter_mut() {
            *c /= n as f64;
        }
        // Reflection.
        let xr: Vec<f64> = (0..n)
            .map(|j| centroid[j] + alpha * (centroid[j] - simplex[n][j]))
            .collect();
        let fr = call_user_n(interp, &f, xr.clone(), line)?;
        if fr < fx[0] {
            // Expansion.
            let xe: Vec<f64> = (0..n)
                .map(|j| centroid[j] + gamma_e * (xr[j] - centroid[j]))
                .collect();
            let fe = call_user_n(interp, &f, xe.clone(), line)?;
            if fe < fr {
                simplex[n] = xe;
                fx[n] = fe;
            } else {
                simplex[n] = xr;
                fx[n] = fr;
            }
            continue;
        }
        if fr < fx[n - 1] {
            simplex[n] = xr;
            fx[n] = fr;
            continue;
        }
        // Contraction.
        let xc: Vec<f64> = (0..n)
            .map(|j| centroid[j] + rho * (simplex[n][j] - centroid[j]))
            .collect();
        let fc = call_user_n(interp, &f, xc.clone(), line)?;
        if fc < fx[n] {
            simplex[n] = xc;
            fx[n] = fc;
            continue;
        }
        // Shrink.
        for i in 1..=n {
            for j in 0..n {
                simplex[i][j] = simplex[0][j] + sigma * (simplex[i][j] - simplex[0][j]);
            }
            fx[i] = call_user_n(interp, &f, simplex[i].clone(), line)?;
        }
    }
    let mut idx: Vec<usize> = (0..=n).collect();
    idx.sort_by(|a, b| fx[*a].partial_cmp(&fx[*b]).unwrap_or(std::cmp::Ordering::Equal));
    Ok(StrykeValue::array(vec![
        StrykeValue::array(
            simplex[idx[0]]
                .iter()
                .copied()
                .map(StrykeValue::float)
                .collect(),
        ),
        StrykeValue::float(fx[idx[0]]),
    ]))
}

/// `gradient_descent F, GRAD, X0 [, STEP, ITERS]` — simple fixed-step descent.
fn builtin_gradient_descent(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let g = args.get(1).cloned().unwrap_or(StrykeValue::UNDEF);
    let mut x: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let step_sz = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let iters = args.get(4).map(|v| v.to_number() as usize).unwrap_or(1000);
    for _ in 0..iters {
        let grad = call_user_vec(interp, &g, &x, line)?;
        for (xi, gi) in x.iter_mut().zip(grad.iter()) {
            *xi -= step_sz * gi;
        }
    }
    let fx = call_user_n(interp, &f, x.clone(), line)?;
    Ok(StrykeValue::array(vec![
        StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::float(fx),
    ]))
}

/// BFGS quasi-Newton minimisation. Args: F, GRAD, X0 [, MAX_ITER, TOL].
fn builtin_bfgs_minimize(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let g = args.get(1).cloned().unwrap_or(StrykeValue::UNDEF);
    let mut x: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let max_iter = args.get(3).map(|v| v.to_number() as usize).unwrap_or(200);
    let tol = args.get(4).map(|v| v.to_number()).unwrap_or(1e-8);
    let n = x.len();
    let mut h = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        h[i][i] = 1.0;
    }
    let mut grad = call_user_vec(interp, &g, &x, line)?;
    for _ in 0..max_iter {
        if grad.iter().map(|v| v * v).sum::<f64>().sqrt() < tol {
            break;
        }
        // Search direction p = -H grad.
        let p: Vec<f64> = (0..n)
            .map(|i| -(0..n).map(|j| h[i][j] * grad[j]).sum::<f64>())
            .collect();
        // Backtracking line search (Armijo).
        let f0 = call_user_n(interp, &f, x.clone(), line)?;
        let g0_dot_p: f64 = grad.iter().zip(p.iter()).map(|(a, b)| a * b).sum();
        let mut alpha = 1.0_f64;
        for _ in 0..30 {
            let candidate: Vec<f64> = x.iter().zip(p.iter()).map(|(xi, pi)| xi + alpha * pi).collect();
            let fc = call_user_n(interp, &f, candidate, line)?;
            if fc <= f0 + 1e-4 * alpha * g0_dot_p {
                break;
            }
            alpha *= 0.5;
        }
        let s: Vec<f64> = p.iter().map(|pi| alpha * pi).collect();
        let x_new: Vec<f64> = x.iter().zip(s.iter()).map(|(xi, si)| xi + si).collect();
        let g_new = call_user_vec(interp, &g, &x_new, line)?;
        let y: Vec<f64> = g_new.iter().zip(grad.iter()).map(|(a, b)| a - b).collect();
        let sy: f64 = s.iter().zip(y.iter()).map(|(a, b)| a * b).sum();
        if sy.abs() > 1e-12 {
            // BFGS update H_{k+1} = (I - ρ s y^T) H (I - ρ y s^T) + ρ s s^T, ρ = 1/sy.
            let rho = 1.0 / sy;
            let hy: Vec<f64> = (0..n)
                .map(|i| (0..n).map(|j| h[i][j] * y[j]).sum::<f64>())
                .collect();
            let yhy: f64 = y.iter().zip(hy.iter()).map(|(a, b)| a * b).sum();
            for i in 0..n {
                for j in 0..n {
                    h[i][j] += (rho + rho * rho * yhy) * s[i] * s[j]
                        - rho * (s[i] * hy[j] + hy[i] * s[j]);
                }
            }
        }
        x = x_new;
        grad = g_new;
    }
    let fx = call_user_n(interp, &f, x.clone(), line)?;
    Ok(StrykeValue::array(vec![
        StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()),
        StrykeValue::float(fx),
    ]))
}

/// Conjugate gradient on symmetric positive-definite Ax = b. No callback needed.
fn builtin_conjugate_gradient(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = b.len();
    let mut x = vec![0.0_f64; n];
    let mut r = b.clone();
    let mut p = r.clone();
    let mut rr: f64 = r.iter().map(|v| v * v).sum();
    for _ in 0..n.max(50) {
        let ap: Vec<f64> = (0..n)
            .map(|i| (0..n).map(|j| a[i][j] * p[j]).sum::<f64>())
            .collect();
        let pap: f64 = p.iter().zip(ap.iter()).map(|(a, b)| a * b).sum();
        if pap.abs() < 1e-15 {
            break;
        }
        let alpha = rr / pap;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        let rr_new: f64 = r.iter().map(|v| v * v).sum();
        if rr_new.sqrt() < 1e-12 {
            break;
        }
        let beta = rr_new / rr;
        for i in 0..n {
            p[i] = r[i] + beta * p[i];
        }
        rr = rr_new;
    }
    Ok(StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()))
}

/// Linear least squares (normal equations): solves A^T A x = A^T b.
fn builtin_least_squares(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let m = a.len();
    if m == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let n = a[0].len();
    let mut ata = vec![vec![0.0_f64; n]; n];
    let mut atb = vec![0.0_f64; n];
    for i in 0..m {
        for j in 0..n {
            for k in 0..n {
                ata[j][k] += a[i][j] * a[i][k];
            }
            atb[j] += a[i][j] * b[i];
        }
    }
    let x = solve_linear(&ata, &atb);
    Ok(StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()))
}

fn solve_linear(a_in: &[Vec<f64>], b_in: &[f64]) -> Vec<f64> {
    let n = a_in.len();
    let mut a: Vec<Vec<f64>> = a_in.to_vec();
    let mut b = b_in.to_vec();
    for col in 0..n {
        let mut pivot = col;
        for i in col + 1..n {
            if a[i][col].abs() > a[pivot][col].abs() {
                pivot = i;
            }
        }
        a.swap(col, pivot);
        b.swap(col, pivot);
        if a[col][col].abs() < 1e-15 {
            return vec![f64::NAN; n];
        }
        for i in col + 1..n {
            let f = a[i][col] / a[col][col];
            for j in col..n {
                a[i][j] -= f * a[col][j];
            }
            b[i] -= f * b[col];
        }
    }
    let mut x = vec![0.0_f64; n];
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in i + 1..n {
            s -= a[i][j] * x[j];
        }
        x[i] = s / a[i][i];
    }
    x
}

/// Levenberg-Marquardt for nonlinear LSQ. Args: F (returns residuals vec),
/// X0, MAX_ITER (default 100), TOL (default 1e-8).
fn builtin_levenberg_marquardt(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let mut x: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let max_iter = args.get(2).map(|v| v.to_number() as usize).unwrap_or(100);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-8);
    let n = x.len();
    let mut lambda = 1e-3_f64;
    for _ in 0..max_iter {
        let r = call_user_vec(interp, &f, &x, line)?;
        let m = r.len();
        // Build Jacobian via finite differences.
        let mut j = vec![vec![0.0_f64; n]; m];
        for col in 0..n {
            let h = step(x[col]);
            let original = x[col];
            x[col] = original + h;
            let rp = call_user_vec(interp, &f, &x, line)?;
            x[col] = original - h;
            let rm = call_user_vec(interp, &f, &x, line)?;
            x[col] = original;
            for row in 0..m {
                j[row][col] = (rp[row] - rm[row]) / (2.0 * h);
            }
        }
        // J^T J + λ diag.
        let mut jtj = vec![vec![0.0_f64; n]; n];
        let mut jtr = vec![0.0_f64; n];
        for i in 0..m {
            for a in 0..n {
                for b in 0..n {
                    jtj[a][b] += j[i][a] * j[i][b];
                }
                jtr[a] += j[i][a] * r[i];
            }
        }
        for i in 0..n {
            jtj[i][i] += lambda * jtj[i][i].max(1e-12);
        }
        let dx = solve_linear(&jtj, &jtr);
        if dx.iter().any(|v| v.is_nan()) {
            break;
        }
        let x_new: Vec<f64> = x.iter().zip(dx.iter()).map(|(xi, d)| xi - d).collect();
        let r_new = call_user_vec(interp, &f, &x_new, line)?;
        let cost = r.iter().map(|v| v * v).sum::<f64>();
        let cost_new = r_new.iter().map(|v| v * v).sum::<f64>();
        if cost_new < cost {
            x = x_new;
            lambda *= 0.5;
            if dx.iter().map(|v| v * v).sum::<f64>().sqrt() < tol {
                break;
            }
        } else {
            lambda *= 2.0;
        }
    }
    Ok(StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()))
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Numerical integration with callbacks
// ─────────────────────────────────────────────────────────────────────────────

/// Romberg integration on [a, b] with k levels (default 5).
fn builtin_romberg(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let levels = args.get(3).map(|v| v.to_number() as usize).unwrap_or(5);
    let mut r = vec![vec![0.0_f64; levels]; levels];
    r[0][0] = 0.5 * (b - a) * (call_user_1(interp, &f, a, line)? + call_user_1(interp, &f, b, line)?);
    for i in 1..levels {
        let two_pow = 1_usize << i;
        let h = (b - a) / two_pow as f64;
        let mut sum = 0.0_f64;
        for k in 1..=(two_pow / 2) {
            sum += call_user_1(interp, &f, a + (2 * k - 1) as f64 * h, line)?;
        }
        r[i][0] = 0.5 * r[i - 1][0] + h * sum;
        for j in 1..=i {
            r[i][j] = (4.0_f64.powi(j as i32) * r[i][j - 1] - r[i - 1][j - 1])
                / (4.0_f64.powi(j as i32) - 1.0);
        }
    }
    Ok(StrykeValue::float(r[levels - 1][levels - 1]))
}

/// Gauss-Legendre quadrature on [a, b] with N points (5..40 supported via
/// Newton-on-Legendre node finder).
fn builtin_gauss_legendre_quad(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(3).map(|v| v.to_number() as usize).unwrap_or(10);
    let (xs, ws) = gauss_legendre_nodes(n);
    let half = 0.5 * (b - a);
    let mid = 0.5 * (a + b);
    let mut sum = 0.0_f64;
    for (xi, wi) in xs.iter().zip(ws.iter()) {
        let pt = mid + half * xi;
        sum += wi * call_user_1(interp, &f, pt, line)?;
    }
    Ok(StrykeValue::float(half * sum))
}

fn gauss_legendre_nodes(n: usize) -> (Vec<f64>, Vec<f64>) {
    // Newton iteration on Legendre P_n. Tricomi's initial guess.
    let mut xs = vec![0.0_f64; n];
    let mut ws = vec![0.0_f64; n];
    let m = n.div_ceil(2);
    for i in 0..m {
        let mut z = (std::f64::consts::PI * (i as f64 + 0.75) / (n as f64 + 0.5)).cos();
        loop {
            let (mut p1, mut p2) = (1.0_f64, 0.0_f64);
            for j in 1..=n {
                let p3 = p2;
                p2 = p1;
                p1 = ((2.0 * j as f64 - 1.0) * z * p2 - (j as f64 - 1.0) * p3) / j as f64;
            }
            let pp = n as f64 * (z * p1 - p2) / (z * z - 1.0);
            let z_new = z - p1 / pp;
            if (z_new - z).abs() < 1e-15 {
                z = z_new;
                xs[i] = -z;
                xs[n - 1 - i] = z;
                ws[i] = 2.0 / ((1.0 - z * z) * pp * pp);
                ws[n - 1 - i] = ws[i];
                break;
            }
            z = z_new;
        }
    }
    (xs, ws)
}

/// Monte Carlo integration on [a, b] with N samples.
fn builtin_monte_carlo_integrate(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args
        .get(3)
        .map(|v| v.to_number() as usize)
        .unwrap_or(10_000);
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let mut sum = 0.0_f64;
    for _ in 0..n {
        let u: f64 = rng.gen();
        let x = a + (b - a) * u;
        sum += call_user_1(interp, &f, x, line)?;
    }
    Ok(StrykeValue::float((b - a) * sum / n as f64))
}

/// Adaptive Simpson on [a, b] with tolerance TOL.
fn builtin_adaptive_simpson(
    interp: &mut VMHelper,
    args: &[StrykeValue],
    line: usize,
) -> StrykeResult<StrykeValue> {
    let f = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-10);
    let max_depth = args.get(4).map(|v| v.to_number() as i32).unwrap_or(20);
    fn simpson(fa: f64, fb: f64, fm: f64, h: f64) -> f64 {
        h / 6.0 * (fa + 4.0 * fm + fb)
    }
    #[allow(clippy::too_many_arguments)]
    fn recurse(
        interp: &mut VMHelper,
        f: &StrykeValue,
        a: f64,
        b: f64,
        tol: f64,
        whole: f64,
        fa: f64,
        fb: f64,
        fm: f64,
        depth: i32,
        line: usize,
    ) -> StrykeResult<f64> {
        let m = 0.5 * (a + b);
        let lm = 0.5 * (a + m);
        let rm = 0.5 * (m + b);
        let flm = call_user_1(interp, f, lm, line)?;
        let frm = call_user_1(interp, f, rm, line)?;
        let left = simpson(fa, fm, flm, m - a);
        let right = simpson(fm, fb, frm, b - m);
        let s2 = left + right;
        if depth <= 0 || (s2 - whole).abs() <= 15.0 * tol {
            return Ok(s2 + (s2 - whole) / 15.0);
        }
        Ok(
            recurse(interp, f, a, m, tol / 2.0, left, fa, fm, flm, depth - 1, line)?
                + recurse(interp, f, m, b, tol / 2.0, right, fm, fb, frm, depth - 1, line)?,
        )
    }
    let fa = call_user_1(interp, &f, a, line)?;
    let fb = call_user_1(interp, &f, b, line)?;
    let m = 0.5 * (a + b);
    let fm = call_user_1(interp, &f, m, line)?;
    let whole = simpson(fa, fb, fm, b - a);
    Ok(StrykeValue::float(recurse(
        interp, &f, a, b, tol, whole, fa, fb, fm, max_depth, line,
    )?))
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Linear-algebra extras (no callbacks)
// ─────────────────────────────────────────────────────────────────────────────

/// `lu_decompose A` → [L, U, P] where P A = L U, P stored as a permutation
/// vector of row indices.
fn builtin_lu_decompose(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = a.len();
    if n == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let mut u = a;
    let mut l = vec![vec![0.0_f64; n]; n];
    let mut p: Vec<usize> = (0..n).collect();
    for k in 0..n {
        // Pivot.
        let mut pivot = k;
        for i in k + 1..n {
            if u[i][k].abs() > u[pivot][k].abs() {
                pivot = i;
            }
        }
        if pivot != k {
            u.swap(k, pivot);
            l.swap(k, pivot);
            p.swap(k, pivot);
        }
        for i in k + 1..n {
            if u[k][k].abs() < 1e-15 {
                continue;
            }
            l[i][k] = u[i][k] / u[k][k];
            for j in k..n {
                u[i][j] -= l[i][k] * u[k][j];
            }
        }
    }
    for i in 0..n {
        l[i][i] = 1.0;
    }
    let lv = matrix_to_value(&l);
    let uv = matrix_to_value(&u);
    let pv = StrykeValue::array(p.into_iter().map(|i| StrykeValue::integer(i as i64)).collect());
    Ok(StrykeValue::array(vec![lv, uv, pv]))
}

/// `qr_decompose A` → [Q, R] using Gram-Schmidt.
fn builtin_qr_decompose(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let m = a.len();
    if m == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let n = a[0].len();
    let mut q = vec![vec![0.0_f64; n]; m];
    let mut r = vec![vec![0.0_f64; n]; n];
    for j in 0..n {
        let mut v: Vec<f64> = (0..m).map(|i| a[i][j]).collect();
        for i in 0..j {
            let mut dot = 0.0_f64;
            for k in 0..m {
                dot += q[k][i] * a[k][j];
            }
            r[i][j] = dot;
            for k in 0..m {
                v[k] -= dot * q[k][i];
            }
        }
        let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        r[j][j] = norm;
        if norm > 1e-15 {
            for k in 0..m {
                q[k][j] = v[k] / norm;
            }
        }
    }
    Ok(StrykeValue::array(vec![matrix_to_value(&q), matrix_to_value(&r)]))
}

/// `householder_reflector V` → matrix H = I - 2 v v^T / (v · v). Useful to
/// build orthogonal transformations.
fn builtin_householder_reflector(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|x| x.to_number())
        .collect();
    let n = v.len();
    let mut vv = 0.0_f64;
    for &vi in &v {
        vv += vi * vi;
    }
    let mut h = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        h[i][i] = 1.0;
        if vv > 0.0 {
            for j in 0..n {
                h[i][j] -= 2.0 * v[i] * v[j] / vv;
            }
        }
    }
    Ok(matrix_to_value(&h))
}

/// `givens_rotation A, B` → [c, s] such that [c -s; s c] · [a; b] = [r; 0].
fn builtin_givens_rotation(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r = (a * a + b * b).sqrt();
    let (c, s) = if r > 0.0 { (a / r, b / r) } else { (1.0, 0.0) };
    Ok(StrykeValue::array(vec![StrykeValue::float(c), StrykeValue::float(s)]))
}

/// Forward substitution for lower-triangular L · x = b.
fn builtin_forward_substitute(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let l = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = b.len();
    let mut x = vec![0.0_f64; n];
    for i in 0..n {
        let mut s = b[i];
        for j in 0..i {
            s -= l[i][j] * x[j];
        }
        x[i] = s / l[i][i];
    }
    Ok(StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()))
}

/// Back substitution for upper-triangular U · x = b.
fn builtin_back_substitute(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let u = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = b.len();
    let mut x = vec![0.0_f64; n];
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in i + 1..n {
            s -= u[i][j] * x[j];
        }
        x[i] = s / u[i][i];
    }
    Ok(StrykeValue::array(x.into_iter().map(StrykeValue::float).collect()))
}

/// Hessenberg reduction via Householder (real square).
fn builtin_hessenberg_reduce(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = a.len();
    if n < 3 {
        return Ok(matrix_to_value(&a));
    }
    for k in 0..n - 2 {
        // Build Householder for column k below the sub-diagonal.
        let mut x = vec![0.0_f64; n - k - 1];
        for i in 0..n - k - 1 {
            x[i] = a[k + 1 + i][k];
        }
        let alpha = -x[0].signum() * x.iter().map(|v| v * v).sum::<f64>().sqrt();
        let mut v = x.clone();
        v[0] -= alpha;
        let vv: f64 = v.iter().map(|x| x * x).sum();
        if vv < 1e-30 {
            continue;
        }
        // A := H A H, where H acts on rows/cols k+1..n.
        // Apply on left: rows.
        for j in k..n {
            let mut s = 0.0_f64;
            for i in 0..v.len() {
                s += v[i] * a[k + 1 + i][j];
            }
            for i in 0..v.len() {
                a[k + 1 + i][j] -= 2.0 * s / vv * v[i];
            }
        }
        // Apply on right: cols.
        for i in 0..n {
            let mut s = 0.0_f64;
            for j in 0..v.len() {
                s += v[j] * a[i][k + 1 + j];
            }
            for j in 0..v.len() {
                a[i][k + 1 + j] -= 2.0 * s / vv * v[j];
            }
        }
    }
    Ok(matrix_to_value(&a))
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Polynomial helpers
// ─────────────────────────────────────────────────────────────────────────────

/// `poly_derivative` — Poly derivative. Returns a float.
fn builtin_poly_derivative(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p: Vec<f64> = poly_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    if p.len() <= 1 {
        return Ok(StrykeValue::array(vec![StrykeValue::float(0.0)]));
    }
    let dp: Vec<f64> = (1..p.len()).map(|i| i as f64 * p[i]).collect();
    Ok(poly_to_value(&dp))
}

/// `poly_integrate` — Poly integrate.
fn builtin_poly_integrate(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p: Vec<f64> = poly_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut out = vec![c];
    for (i, coef) in p.iter().enumerate() {
        out.push(coef / (i as f64 + 1.0));
    }
    Ok(poly_to_value(&out))
}

/// `poly_compose P, Q` → P(Q(x)) as polynomial coefficients.
fn builtin_poly_compose(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p: Vec<f64> = poly_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let q: Vec<f64> = poly_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let mut result = vec![0.0_f64];
    let mut q_pow: Vec<f64> = vec![1.0];
    for &c in &p {
        for (i, qp) in q_pow.iter().enumerate() {
            if i >= result.len() {
                result.push(0.0);
            }
            result[i] += c * qp;
        }
        // q_pow := q_pow * q
        let mut new_qp = vec![0.0_f64; q_pow.len() + q.len() - 1];
        for (i, &a) in q_pow.iter().enumerate() {
            for (j, &b) in q.iter().enumerate() {
                new_qp[i + j] += a * b;
            }
        }
        q_pow = new_qp;
    }
    Ok(poly_to_value(&result))
}

/// Horner evaluation: poly_eval_horner [a_0..a_n], x.
fn builtin_poly_eval_horner(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p: Vec<f64> = poly_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut acc = 0.0_f64;
    for &c in p.iter().rev() {
        acc = acc * x + c;
    }
    Ok(StrykeValue::float(acc))
}

/// `pade_approximant TAYLOR_COEFFS, M, N` — [m/n] Padé from Taylor expansion.
/// Returns [num_coeffs, den_coeffs] with den[0] = 1.
fn builtin_pade_approximant(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let c: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let m = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    if c.len() < m + n + 1 {
        return Err(StrykeError::runtime(
            "pade_approximant: need at least m+n+1 Taylor coefficients",
            0,
        ));
    }
    // Solve linear system for q_1..q_n: Σ_{j=1..n} c_{m+1-j+i} q_j = -c_{m+1+i} for i=0..n-1.
    let mut a_mat = vec![vec![0.0_f64; n]; n];
    let mut rhs = vec![0.0_f64; n];
    for i in 0..n {
        for j in 0..n {
            let idx = m + i + 1 - (j + 1);
            a_mat[i][j] = if idx < c.len() { c[idx] } else { 0.0 };
        }
        rhs[i] = -c[m + i + 1];
    }
    let q_tail = if n > 0 { solve_linear(&a_mat, &rhs) } else { vec![] };
    let mut q = vec![1.0];
    q.extend(q_tail);
    let mut p_coeffs = vec![0.0_f64; m + 1];
    for i in 0..=m {
        let mut s = c[i];
        for (j, &qj) in q.iter().enumerate().skip(1) {
            if i >= j {
                s += qj * c[i - j];
            }
        }
        p_coeffs[i] = s;
    }
    Ok(StrykeValue::array(vec![
        poly_to_value(&p_coeffs),
        poly_to_value(&q),
    ]))
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. Quaternions and 3D rotations
// ─────────────────────────────────────────────────────────────────────────────

fn quat_from_value(v: &StrykeValue) -> [f64; 4] {
    let xs: Vec<f64> = arg_to_vec(v).iter().map(|x| x.to_number()).collect();
    [
        xs.first().copied().unwrap_or(1.0),
        xs.get(1).copied().unwrap_or(0.0),
        xs.get(2).copied().unwrap_or(0.0),
        xs.get(3).copied().unwrap_or(0.0),
    ]
}
fn quat_to_value(q: [f64; 4]) -> StrykeValue {
    StrykeValue::array(q.iter().copied().map(StrykeValue::float).collect())
}

/// Hamilton product of two quaternions (w, x, y, z).
fn builtin_quat_mul(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = quat_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b = quat_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    Ok(quat_to_value([
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]))
}

/// `quat_conj` — Quat conj.
fn builtin_quat_conj(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let q = quat_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    Ok(quat_to_value([q[0], -q[1], -q[2], -q[3]]))
}

/// `quat_norm` — Quat norm. Returns a float.
fn builtin_quat_norm(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let q = quat_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    Ok(StrykeValue::float(
        (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt(),
    ))
}

/// `quat_inv` — Quat inv.
fn builtin_quat_inv(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let q = quat_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n2 = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    if n2 < 1e-30 {
        return Err(StrykeError::runtime("quat_inv: zero quaternion", 0));
    }
    Ok(quat_to_value([q[0] / n2, -q[1] / n2, -q[2] / n2, -q[3] / n2]))
}

/// `quat_from_axis_angle` — Quat from axis angle.
fn builtin_quat_from_axis_angle(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let axis: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = (axis.iter().map(|x| x * x).sum::<f64>()).sqrt().max(1e-15);
    let s = (theta / 2.0).sin();
    Ok(quat_to_value([
        (theta / 2.0).cos(),
        axis.first().copied().unwrap_or(0.0) / n * s,
        axis.get(1).copied().unwrap_or(0.0) / n * s,
        axis.get(2).copied().unwrap_or(0.0) / n * s,
    ]))
}

/// `quat_to_axis_angle` — Quat to axis angle. Returns a float.
fn builtin_quat_to_axis_angle(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let q = quat_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let theta = 2.0 * q[0].clamp(-1.0, 1.0).acos();
    let s = (1.0 - q[0] * q[0]).sqrt().max(1e-15);
    let axis = [q[1] / s, q[2] / s, q[3] / s];
    Ok(StrykeValue::array(vec![
        StrykeValue::array(axis.iter().copied().map(StrykeValue::float).collect()),
        StrykeValue::float(theta),
    ]))
}

/// `quat_to_matrix` — Quat to matrix.
fn builtin_quat_to_matrix(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let q = quat_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let (w, x, y, z) = (q[0], q[1], q[2], q[3]);
    let m = vec![
        vec![
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        vec![
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        vec![
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ];
    Ok(matrix_to_value(&m))
}

/// `quat_from_matrix` — Quat from matrix.
fn builtin_quat_from_matrix(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    if m.len() < 3 || m[0].len() < 3 {
        return Err(StrykeError::runtime("quat_from_matrix: 3×3 matrix required", 0));
    }
    let tr = m[0][0] + m[1][1] + m[2][2];
    let (w, x, y, z);
    if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        w = 0.25 * s;
        x = (m[2][1] - m[1][2]) / s;
        y = (m[0][2] - m[2][0]) / s;
        z = (m[1][0] - m[0][1]) / s;
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        w = (m[2][1] - m[1][2]) / s;
        x = 0.25 * s;
        y = (m[0][1] + m[1][0]) / s;
        z = (m[0][2] + m[2][0]) / s;
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        w = (m[0][2] - m[2][0]) / s;
        x = (m[0][1] + m[1][0]) / s;
        y = 0.25 * s;
        z = (m[1][2] + m[2][1]) / s;
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        w = (m[1][0] - m[0][1]) / s;
        x = (m[0][2] + m[2][0]) / s;
        y = (m[1][2] + m[2][1]) / s;
        z = 0.25 * s;
    }
    Ok(quat_to_value([w, x, y, z]))
}

/// Spherical-linear interpolation between two quaternions.
fn builtin_quat_slerp(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut a = quat_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let mut b = quat_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let mut dot = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
    if dot < 0.0 {
        for v in b.iter_mut() {
            *v = -*v;
        }
        dot = -dot;
    }
    if dot > 0.9995 {
        let r = [
            a[0] + t * (b[0] - a[0]),
            a[1] + t * (b[1] - a[1]),
            a[2] + t * (b[2] - a[2]),
            a[3] + t * (b[3] - a[3]),
        ];
        let n = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2] + r[3] * r[3]).sqrt();
        return Ok(quat_to_value([r[0] / n, r[1] / n, r[2] / n, r[3] / n]));
    }
    let theta_0 = dot.clamp(-1.0, 1.0).acos();
    let theta = theta_0 * t;
    let sin_theta_0 = theta_0.sin();
    let sin_theta = theta.sin();
    let s1 = (theta_0 - theta).sin() / sin_theta_0;
    let s2 = sin_theta / sin_theta_0;
    a[0] = s1 * a[0] + s2 * b[0];
    a[1] = s1 * a[1] + s2 * b[1];
    a[2] = s1 * a[2] + s2 * b[2];
    a[3] = s1 * a[3] + s2 * b[3];
    Ok(quat_to_value(a))
}

/// `euler_zyx_to_matrix YAW, PITCH, ROLL` — Z-Y-X intrinsic Euler rotation.
fn builtin_euler_zyx_to_matrix(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let yaw = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let pitch = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let roll = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let (cy, sy) = (yaw.cos(), yaw.sin());
    let (cp, sp) = (pitch.cos(), pitch.sin());
    let (cr, sr) = (roll.cos(), roll.sin());
    let m = vec![
        vec![cy * cp, cy * sp * sr - sy * cr, cy * sp * cr + sy * sr],
        vec![sy * cp, sy * sp * sr + cy * cr, sy * sp * cr - cy * sr],
        vec![-sp, cp * sr, cp * cr],
    ];
    Ok(matrix_to_value(&m))
}

/// `matrix_to_euler_zyx M` — extract (yaw, pitch, roll). Returns [yaw, pitch, roll].
fn builtin_matrix_to_euler_zyx(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    if m.len() < 3 || m[0].len() < 3 {
        return Err(StrykeError::runtime(
            "matrix_to_euler_zyx: 3×3 matrix required",
            0,
        ));
    }
    let pitch = (-m[2][0]).clamp(-1.0, 1.0).asin();
    let (yaw, roll) = if pitch.cos().abs() > 1e-9 {
        (m[1][0].atan2(m[0][0]), m[2][1].atan2(m[2][2]))
    } else {
        ((-m[0][1]).atan2(m[1][1]), 0.0)
    };
    Ok(StrykeValue::array(vec![
        StrykeValue::float(yaw),
        StrykeValue::float(pitch),
        StrykeValue::float(roll),
    ]))
}

/// Rotate a 3D vector by a quaternion: q v q⁻¹.
fn builtin_rotate_3d_vec(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let q = quat_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let v: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|x| x.to_number())
        .collect();
    if v.len() < 3 {
        return Err(StrykeError::runtime("rotate_3d_vec: 3-vector required", 0));
    }
    let qv = [0.0, v[0], v[1], v[2]];
    // r = q * qv * q*
    let qa = [q[0], q[1], q[2], q[3]];
    let qc = [q[0], -q[1], -q[2], -q[3]];
    let mul = |a: [f64; 4], b: [f64; 4]| -> [f64; 4] {
        [
            a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
            a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
            a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
            a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
        ]
    };
    let r = mul(mul(qa, qv), qc);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(r[1]),
        StrykeValue::float(r[2]),
        StrykeValue::float(r[3]),
    ]))
}

// ─────────────────────────────────────────────────────────────────────────────
// 7. Information theory
// ─────────────────────────────────────────────────────────────────────────────

/// `kl_divergence` — Kl divergence. Returns a float.
fn builtin_kl_divergence(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let q: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut sum = 0.0_f64;
    for i in 0..p.len().min(q.len()) {
        if p[i] > 0.0 && q[i] > 0.0 {
            sum += p[i] * (p[i] / q[i]).ln();
        }
    }
    Ok(StrykeValue::float(sum))
}

/// `js_divergence` — Js divergence. Returns a float.
fn builtin_js_divergence(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let q: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let m: Vec<f64> = p.iter().zip(q.iter()).map(|(a, b)| 0.5 * (a + b)).collect();
    let kl = |a: &[f64], b: &[f64]| -> f64 {
        let mut s = 0.0;
        for i in 0..a.len().min(b.len()) {
            if a[i] > 0.0 && b[i] > 0.0 {
                s += a[i] * (a[i] / b[i]).ln();
            }
        }
        s
    };
    Ok(StrykeValue::float(0.5 * (kl(&p, &m) + kl(&q, &m))))
}

/// `mutual_information JOINT` — I(X;Y) from joint probability matrix.
fn builtin_mutual_information(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let joint = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = joint.len();
    let m = if n == 0 { 0 } else { joint[0].len() };
    let mut px = vec![0.0_f64; n];
    let mut py = vec![0.0_f64; m];
    for i in 0..n {
        for j in 0..m {
            px[i] += joint[i][j];
            py[j] += joint[i][j];
        }
    }
    let mut mi = 0.0_f64;
    for i in 0..n {
        for j in 0..m {
            if joint[i][j] > 0.0 && px[i] > 0.0 && py[j] > 0.0 {
                mi += joint[i][j] * (joint[i][j] / (px[i] * py[j])).ln();
            }
        }
    }
    Ok(StrykeValue::float(mi))
}

/// `cross_entropy_arr` — Cross entropy arr. Returns a float.
fn builtin_cross_entropy_arr(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let q: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut sum = 0.0_f64;
    for i in 0..p.len().min(q.len()) {
        if q[i] > 0.0 {
            sum -= p[i] * q[i].ln();
        }
    }
    Ok(StrykeValue::float(sum))
}

/// `renyi_entropy` — Renyi entropy. Returns a float.
fn builtin_renyi_entropy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    if (alpha - 1.0).abs() < 1e-9 {
        // Shannon limit.
        let s = p
            .iter()
            .filter(|&&v| v > 0.0)
            .map(|v| -v * v.ln())
            .sum::<f64>();
        return Ok(StrykeValue::float(s));
    }
    let s: f64 = p.iter().filter(|&&v| v > 0.0).map(|v| v.powf(alpha)).sum();
    Ok(StrykeValue::float(s.ln() / (1.0 - alpha)))
}

/// `tsallis_entropy` — Tsallis entropy. Returns a float.
fn builtin_tsallis_entropy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    let s: f64 = p.iter().filter(|&&v| v > 0.0).map(|v| v.powf(q)).sum();
    Ok(StrykeValue::float((1.0 - s) / (q - 1.0)))
}

// ─────────────────────────────────────────────────────────────────────────────
// 8. Quantum mechanics primitives
// ─────────────────────────────────────────────────────────────────────────────

/// Pauli σ_x.
fn builtin_pauli_x(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(matrix_to_value(&[vec![0.0, 1.0], vec![1.0, 0.0]]))
}

/// Pauli σ_y returned as 4-element [a_re, a_im, b_re, b_im, …] flat layout
/// because StrykeValue::float is real. Encoded as 2×2 block `[[0, -i], [i, 0]]`
/// → [[[0,0], [0,-1]], [[0,1], [0,0]]] structure: row → col → [Re, Im].
fn builtin_pauli_y(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::array(vec![
        StrykeValue::array(vec![
            StrykeValue::array(vec![StrykeValue::float(0.0), StrykeValue::float(0.0)]),
            StrykeValue::array(vec![StrykeValue::float(0.0), StrykeValue::float(-1.0)]),
        ]),
        StrykeValue::array(vec![
            StrykeValue::array(vec![StrykeValue::float(0.0), StrykeValue::float(1.0)]),
            StrykeValue::array(vec![StrykeValue::float(0.0), StrykeValue::float(0.0)]),
        ]),
    ]))
}

/// Pauli σ_z.
fn builtin_pauli_z(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(matrix_to_value(&[vec![1.0, 0.0], vec![0.0, -1.0]]))
}

/// 2×2 identity (Pauli I).
fn builtin_pauli_id(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(matrix_to_value(&[vec![1.0, 0.0], vec![0.0, 1.0]]))
}

/// Outer product |ψ⟩⟨φ| of two real column vectors.
fn builtin_ket_bra(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let psi: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let phi: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut out = vec![vec![0.0_f64; phi.len()]; psi.len()];
    for (i, &a) in psi.iter().enumerate() {
        for (j, &b) in phi.iter().enumerate() {
            out[i][j] = a * b;
        }
    }
    Ok(matrix_to_value(&out))
}

/// Density matrix ρ = |ψ⟩⟨ψ| (real-valued |ψ⟩).
fn builtin_density_matrix(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_ket_bra(&[
        args.first().cloned().unwrap_or(StrykeValue::UNDEF),
        args.first().cloned().unwrap_or(StrykeValue::UNDEF),
    ])
}

/// `expectation_value OP, STATE` — ⟨ψ|O|ψ⟩.
fn builtin_expectation_value(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let op = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let psi: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = psi.len();
    let mut sum = 0.0_f64;
    for i in 0..n {
        for j in 0..n {
            sum += psi[i] * op[i][j] * psi[j];
        }
    }
    Ok(StrykeValue::float(sum))
}

/// `commutator A, B` — [A, B] = AB - BA.
fn builtin_commutator(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let ab = mat_mul(&a, &b);
    let ba = mat_mul(&b, &a);
    let mut out = vec![vec![0.0_f64; ab[0].len()]; ab.len()];
    for i in 0..ab.len() {
        for j in 0..ab[0].len() {
            out[i][j] = ab[i][j] - ba[i][j];
        }
    }
    Ok(matrix_to_value(&out))
}

/// `anticommutator A, B` — {A, B} = AB + BA.
fn builtin_anticommutator(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let ab = mat_mul(&a, &b);
    let ba = mat_mul(&b, &a);
    let mut out = vec![vec![0.0_f64; ab[0].len()]; ab.len()];
    for i in 0..ab.len() {
        for j in 0..ab[0].len() {
            out[i][j] = ab[i][j] + ba[i][j];
        }
    }
    Ok(matrix_to_value(&out))
}

/// `partial_trace RHO, DIMS, KEEP` — partial trace over subsystems not in KEEP.
/// DIMS is array of subsystem dimensions; KEEP is array of subsystem indices to keep.
/// Tensor product convention: ρ_total = ρ_0 ⊗ ρ_1 ⊗ … in the order given.
fn builtin_partial_trace(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let rho = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let dims: Vec<usize> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as usize)
        .collect();
    let keep_set: std::collections::HashSet<usize> =
        arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
            .iter()
            .map(|v| v.to_number() as usize)
            .collect();
    let total: usize = dims.iter().product();
    if rho.len() != total {
        return Err(StrykeError::runtime(
            "partial_trace: ρ size doesn't match Π dims",
            0,
        ));
    }
    // Multi-index helpers.
    let to_idx = |multi: &[usize]| -> usize {
        let mut s = 0_usize;
        let mut stride = 1_usize;
        for k in (0..multi.len()).rev() {
            s += stride * multi[k];
            stride *= dims[k];
        }
        s
    };
    let from_idx = |mut idx: usize| -> Vec<usize> {
        let mut multi = vec![0_usize; dims.len()];
        for k in (0..dims.len()).rev() {
            multi[k] = idx % dims[k];
            idx /= dims[k];
        }
        multi
    };
    let kept_indices: Vec<usize> = (0..dims.len()).filter(|i| keep_set.contains(i)).collect();
    let kept_dim: usize = kept_indices.iter().map(|&i| dims[i]).product();
    let traced_indices: Vec<usize> = (0..dims.len()).filter(|i| !keep_set.contains(i)).collect();
    let traced_dim: usize = traced_indices.iter().map(|&i| dims[i]).product();
    let mut out = vec![vec![0.0_f64; kept_dim]; kept_dim];
    for ki in 0..kept_dim {
        for kj in 0..kept_dim {
            let mut multi_i = vec![0_usize; dims.len()];
            let mut multi_j = vec![0_usize; dims.len()];
            // Decode kept-subsystem indices.
            let mut tmp_i = ki;
            let mut tmp_j = kj;
            for (rev_k, &k) in kept_indices.iter().enumerate().rev() {
                multi_i[k] = tmp_i % dims[k];
                tmp_i /= dims[k];
                multi_j[k] = tmp_j % dims[k];
                tmp_j /= dims[k];
                let _ = rev_k;
            }
            let mut s = 0.0_f64;
            for tt in 0..traced_dim {
                let mut tmp_t = tt;
                for &k in traced_indices.iter().rev() {
                    multi_i[k] = tmp_t % dims[k];
                    multi_j[k] = multi_i[k];
                    tmp_t /= dims[k];
                }
                s += rho[to_idx(&multi_i)][to_idx(&multi_j)];
            }
            out[ki][kj] = s;
            let _ = from_idx; // keep for clarity / future uses
        }
    }
    Ok(matrix_to_value(&out))
}

/// von Neumann entropy S(ρ) = -tr(ρ log ρ) for a real symmetric density matrix.
fn builtin_von_neumann_entropy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut rho = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let evs = jacobi_eigenvalues(&mut rho);
    let mut s = 0.0_f64;
    for e in evs {
        if e > 1e-12 {
            s -= e * e.ln();
        }
    }
    Ok(StrykeValue::float(s))
}

// ─────────────────────────────────────────────────────────────────────────────
// 9. Statistical mechanics
// ─────────────────────────────────────────────────────────────────────────────

/// `bose_einstein` — Bose einstein. Returns a float.
fn builtin_bose_einstein(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (eps, mu, kt) = f3(args);
    let z = ((eps - mu) / kt).exp();
    if z <= 1.0 {
        return Ok(StrykeValue::float(f64::INFINITY));
    }
    Ok(StrykeValue::float(1.0 / (z - 1.0)))
}

/// `fermi_dirac` — Fermi dirac. Returns a float.
fn builtin_fermi_dirac(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (eps, mu, kt) = f3(args);
    Ok(StrykeValue::float(1.0 / (((eps - mu) / kt).exp() + 1.0)))
}

/// Maxwell-Boltzmann speed distribution PDF f(v) = 4π (m/2πkT)^{3/2} v² e^{-mv²/2kT}.
fn builtin_maxwell_boltzmann_speed(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v = args.first().map(|x| x.to_number()).unwrap_or(0.0);
    let m = args.get(1).map(|x| x.to_number()).unwrap_or(1.0);
    let kt = args.get(2).map(|x| x.to_number()).unwrap_or(1.0);
    let pre = 4.0 * std::f64::consts::PI * (m / (2.0 * std::f64::consts::PI * kt)).powf(1.5);
    Ok(StrykeValue::float(
        pre * v * v * (-m * v * v / (2.0 * kt)).exp(),
    ))
}

/// Classical canonical partition function Z = Σ exp(-E_i / kT).
fn builtin_partition_function(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let energies: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let kt = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let z: f64 = energies.iter().map(|e| (-e / kt).exp()).sum();
    Ok(StrykeValue::float(z))
}

/// Helmholtz free energy F = -kT ln Z.
fn builtin_helmholtz_free_energy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let kt = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(-kt * z.ln()))
}

/// Boltzmann factor exp(-E / kT).
fn builtin_boltzmann_factor(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (e, t) = f2(args);
    Ok(StrykeValue::float((-e / t).exp()))
}

/// Einstein heat capacity C_V(T) = 3 R x² e^x / (e^x - 1)², x = θ_E / T.
fn builtin_einstein_specific_heat(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let t = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let r_gas = 8.314462618_f64;
    let x = theta / t;
    let ex = x.exp();
    Ok(StrykeValue::float(
        3.0 * r_gas * x * x * ex / (ex - 1.0).powi(2),
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// 10. Optics
// ─────────────────────────────────────────────────────────────────────────────

fn fresnel_pair_real(theta_i: f64, n1: f64, n2: f64) -> (f64, f64, f64, f64) {
    // Returns (r_te, r_tm, t_te, t_tm) using cos via Snell's law.
    let cos_i = theta_i.cos();
    let sin2_t = (n1 / n2).powi(2) * (1.0 - cos_i * cos_i);
    if sin2_t > 1.0 {
        return (1.0, 1.0, 0.0, 0.0); // total internal reflection
    }
    let cos_t = (1.0 - sin2_t).sqrt();
    let r_te = (n1 * cos_i - n2 * cos_t) / (n1 * cos_i + n2 * cos_t);
    let r_tm = (n2 * cos_i - n1 * cos_t) / (n2 * cos_i + n1 * cos_t);
    let t_te = 2.0 * n1 * cos_i / (n1 * cos_i + n2 * cos_t);
    let t_tm = 2.0 * n1 * cos_i / (n2 * cos_i + n1 * cos_t);
    (r_te, r_tm, t_te, t_tm)
}

/// `fresnel_reflection_te` — Fresnel reflection te. Returns a float.
fn builtin_fresnel_reflection_te(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (theta, n1, n2) = f3(args);
    Ok(StrykeValue::float(fresnel_pair_real(theta, n1, n2).0))
}
/// `fresnel_reflection_tm` — Fresnel reflection tm. Returns a float.
fn builtin_fresnel_reflection_tm(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (theta, n1, n2) = f3(args);
    Ok(StrykeValue::float(fresnel_pair_real(theta, n1, n2).1))
}
/// `fresnel_transmission_te` — Fresnel transmission te. Returns a float.
fn builtin_fresnel_transmission_te(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (theta, n1, n2) = f3(args);
    Ok(StrykeValue::float(fresnel_pair_real(theta, n1, n2).2))
}
/// `fresnel_transmission_tm` — Fresnel transmission tm. Returns a float.
fn builtin_fresnel_transmission_tm(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (theta, n1, n2) = f3(args);
    Ok(StrykeValue::float(fresnel_pair_real(theta, n1, n2).3))
}

/// ABCD ray matrix for a thin lens of focal length f.
fn builtin_abcd_thin_lens(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let f = f1(args);
    if f.abs() < 1e-15 {
        return Err(StrykeError::runtime("abcd_thin_lens: zero focal length", 0));
    }
    Ok(matrix_to_value(&[vec![1.0, 0.0], vec![-1.0 / f, 1.0]]))
}

/// ABCD ray matrix for free-space propagation distance d.
fn builtin_abcd_free_space(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let d = f1(args);
    Ok(matrix_to_value(&[vec![1.0, d], vec![0.0, 1.0]]))
}

/// Gaussian beam q parameter at distance z given waist w_0 and wavelength λ.
/// Returns [Re(q), Im(q)] = [z, π w_0² / λ].
fn builtin_gaussian_beam_q(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let z = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let w0 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(z),
        StrykeValue::float(std::f64::consts::PI * w0 * w0 / lambda),
    ]))
}

// ─────────────────────────────────────────────────────────────────────────────
// 11. Astrodynamics
// ─────────────────────────────────────────────────────────────────────────────

/// Solve Kepler's equation M = E - e sin(E) for E via Newton iteration.
fn builtin_kepler_solve(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let e = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut e_anom = if e < 0.8 { m } else { std::f64::consts::PI };
    for _ in 0..50 {
        let f = e_anom - e * e_anom.sin() - m;
        let fp = 1.0 - e * e_anom.cos();
        let dx = f / fp;
        e_anom -= dx;
        if dx.abs() < 1e-13 {
            break;
        }
    }
    Ok(StrykeValue::float(e_anom))
}

/// `true_to_eccentric` — True to eccentric. Returns a float.
fn builtin_true_to_eccentric(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let nu = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let e = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let cos_e = (e + nu.cos()) / (1.0 + e * nu.cos());
    let sin_e = ((1.0 - e * e).sqrt() * nu.sin()) / (1.0 + e * nu.cos());
    Ok(StrykeValue::float(sin_e.atan2(cos_e)))
}

/// `eccentric_to_mean` — Eccentric to mean. Returns a float.
fn builtin_eccentric_to_mean(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let e_anom = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let e = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(e_anom - e * e_anom.sin()))
}

/// Julian date for civil (Y, M, D, h, m, s).
fn builtin_julian_date(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let y = args.first().map(|v| v.to_number() as i64).unwrap_or(2000);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let h = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let mn = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let s = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    // Meeus formula.
    let (yy, mm) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    let a = yy / 100;
    let b = 2 - a + a / 4;
    let jd = (365.25 * (yy + 4716) as f64).floor()
        + (30.6001 * (mm + 1) as f64).floor()
        + d
        + b as f64
        - 1524.5
        + (h + mn / 60.0 + s / 3600.0) / 24.0;
    Ok(StrykeValue::float(jd))
}

/// Convert Julian date to Gregorian (Y, M, D.dd).
fn builtin_jd_to_gregorian(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let jd = f1(args) + 0.5;
    let z = jd.floor() as i64;
    let f = jd - z as f64;
    let a = if z < 2_299_161 {
        z
    } else {
        let alpha = ((z as f64 - 1_867_216.25) / 36_524.25).floor() as i64;
        z + 1 + alpha - alpha / 4
    };
    let b = a + 1524;
    let c = ((b as f64 - 122.1) / 365.25).floor() as i64;
    let d = (365.25 * c as f64).floor() as i64;
    let e = ((b - d) as f64 / 30.6001).floor() as i64;
    let day = (b - d) as f64 - (30.6001 * e as f64).floor() + f;
    let month = if e < 14 { e - 1 } else { e - 13 };
    let year = if month > 2 { c - 4716 } else { c - 4715 };
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(year),
        StrykeValue::integer(month),
        StrykeValue::float(day),
    ]))
}

/// Greenwich Mean Sidereal Time at JD (radians).
fn builtin_sidereal_time_gmst(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let jd = f1(args);
    let t = (jd - 2_451_545.0) / 36_525.0;
    // Meeus 12.4
    let mut theta_deg = 280.460_618_37
        + 360.985_647_366_29 * (jd - 2_451_545.0)
        + 0.000_387_933 * t * t
        - t * t * t / 38_710_000.0;
    theta_deg = theta_deg.rem_euclid(360.0);
    Ok(StrykeValue::float(theta_deg.to_radians()))
}

/// Vis-viva equation: v² = μ (2/r - 1/a).
fn builtin_vis_viva(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let r = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((mu * (2.0 / r - 1.0 / a)).sqrt()))
}

/// Orbital period via Kepler's third law: T = 2π √(a³ / μ).
fn builtin_orbital_period_kepler(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(
        2.0 * std::f64::consts::PI * (a.powi(3) / mu).sqrt(),
    ))
}

/// Orbital elements → state vector. Args: a, e, i (rad), Ω (rad), ω (rad),
/// ν (rad), μ. Returns [r_x, r_y, r_z, v_x, v_y, v_z] in the inertial frame.
fn builtin_orbital_elements_to_state(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let e = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let i = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let raan = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let arg_p = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let nu = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
    let p = a * (1.0 - e * e);
    let r = p / (1.0 + e * nu.cos());
    // Position in perifocal frame.
    let x_p = r * nu.cos();
    let y_p = r * nu.sin();
    // Velocity in perifocal.
    let h = (mu * p).sqrt();
    let vx_p = -mu / h * nu.sin();
    let vy_p = mu / h * (e + nu.cos());
    // Rotation matrix perifocal → inertial.
    let (cw, sw) = (arg_p.cos(), arg_p.sin());
    let (co, so) = (raan.cos(), raan.sin());
    let (ci, si) = (i.cos(), i.sin());
    let r11 = co * cw - so * sw * ci;
    let r12 = -co * sw - so * cw * ci;
    let r21 = so * cw + co * sw * ci;
    let r22 = -so * sw + co * cw * ci;
    let r31 = sw * si;
    let r32 = cw * si;
    let rx = r11 * x_p + r12 * y_p;
    let ry = r21 * x_p + r22 * y_p;
    let rz = r31 * x_p + r32 * y_p;
    let vx = r11 * vx_p + r12 * vy_p;
    let vy = r21 * vx_p + r22 * vy_p;
    let vz = r31 * vx_p + r32 * vy_p;
    Ok(StrykeValue::array(vec![
        StrykeValue::float(rx),
        StrykeValue::float(ry),
        StrykeValue::float(rz),
        StrykeValue::float(vx),
        StrykeValue::float(vy),
        StrykeValue::float(vz),
    ]))
}

// ─────────────────────────────────────────────────────────────────────────────
// 12. Time series
// ─────────────────────────────────────────────────────────────────────────────

/// Single Kalman update step. Args: x, P, F, H, Q, R, z. Returns [x_new, P_new].
fn builtin_kalman_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let x: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let p_in = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let f_mat = matrix_from_value(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let h = matrix_from_value(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF));
    let q = matrix_from_value(&args.get(4).cloned().unwrap_or(StrykeValue::UNDEF));
    let r = matrix_from_value(&args.get(5).cloned().unwrap_or(StrykeValue::UNDEF));
    let z: Vec<f64> = arg_to_vec(&args.get(6).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = x.len();
    let m = z.len();
    // Predict.
    let mut x_p = vec![0.0_f64; n];
    for i in 0..n {
        for j in 0..n {
            x_p[i] += f_mat[i][j] * x[j];
        }
    }
    let f_p = mat_mul(&f_mat, &p_in);
    let mut p_p = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        for j in 0..n {
            let mut s = 0.0_f64;
            for k in 0..n {
                s += f_p[i][k] * f_mat[j][k];
            }
            p_p[i][j] = s + q[i][j];
        }
    }
    // Innovation.
    let mut hx = vec![0.0_f64; m];
    for i in 0..m {
        for j in 0..n {
            hx[i] += h[i][j] * x_p[j];
        }
    }
    let y: Vec<f64> = z.iter().zip(hx.iter()).map(|(a, b)| a - b).collect();
    // S = H P_p H^T + R
    let hp = mat_mul(&h, &p_p);
    let mut s = vec![vec![0.0_f64; m]; m];
    for i in 0..m {
        for j in 0..m {
            let mut sum = 0.0_f64;
            for k in 0..n {
                sum += hp[i][k] * h[j][k];
            }
            s[i][j] = sum + r[i][j];
        }
    }
    // K = P_p H^T S^{-1}.
    // Solve S^T K^T = (P_p H^T)^T columnwise.
    let mut ph_t = vec![vec![0.0_f64; m]; n];
    for i in 0..n {
        for j in 0..m {
            let mut sum = 0.0_f64;
            for k in 0..n {
                sum += p_p[i][k] * h[j][k];
            }
            ph_t[i][j] = sum;
        }
    }
    let mut k_mat = vec![vec![0.0_f64; m]; n];
    for col in 0..m {
        let rhs: Vec<f64> = (0..m).map(|i| ph_t[col][i]).collect();
        let _ = rhs;
        let rhs2: Vec<f64> = (0..m).map(|i| s[col][i]).collect();
        let _ = rhs2;
        // Solve s · k = ph_t[*][col].
        let rhs3: Vec<f64> = (0..n).map(|i| ph_t[i][col]).collect();
        let _ = rhs3;
    }
    // Direct: solve for K row-wise: for each row i of n, solve s · k_i^T = ph_t[i].
    for i in 0..n {
        let rhs: Vec<f64> = (0..m).map(|j| ph_t[i][j]).collect();
        let sol = solve_linear(&s, &rhs);
        k_mat[i][..m].copy_from_slice(&sol[..m]);
    }
    // Update x = x_p + K y.
    let mut x_new = vec![0.0_f64; n];
    for i in 0..n {
        x_new[i] = x_p[i];
        for j in 0..m {
            x_new[i] += k_mat[i][j] * y[j];
        }
    }
    // P_new = (I - K H) P_p
    let mut kh = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        for j in 0..n {
            for kk in 0..m {
                kh[i][j] += k_mat[i][kk] * h[kk][j];
            }
        }
    }
    let mut imkh = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        for j in 0..n {
            imkh[i][j] = if i == j { 1.0 } else { 0.0 } - kh[i][j];
        }
    }
    let p_new = mat_mul(&imkh, &p_p);
    Ok(StrykeValue::array(vec![
        StrykeValue::array(x_new.into_iter().map(StrykeValue::float).collect()),
        matrix_to_value(&p_new),
    ]))
}

/// Exponential smoothing: y_t = α x_t + (1-α) y_{t-1}.
fn builtin_exponential_smoothing(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let mut out = Vec::with_capacity(xs.len());
    if xs.is_empty() {
        return Ok(StrykeValue::array(vec![]));
    }
    let mut y = xs[0];
    out.push(StrykeValue::float(y));
    for &x in xs.iter().skip(1) {
        y = alpha * x + (1.0 - alpha) * y;
        out.push(StrykeValue::float(y));
    }
    Ok(StrykeValue::array(out))
}

/// Holt-Winters additive: returns smoothed level series.
fn builtin_holt_winters(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.4);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.2);
    let gamma = args.get(3).map(|v| v.to_number()).unwrap_or(0.1);
    let period = args.get(4).map(|v| v.to_number() as usize).unwrap_or(12);
    let n = xs.len();
    if n < period * 2 {
        return Ok(StrykeValue::array(
            xs.into_iter().map(StrykeValue::float).collect(),
        ));
    }
    // Initialise.
    let initial_l: f64 = xs[..period].iter().sum::<f64>() / period as f64;
    let initial_t = (xs[period..2 * period].iter().sum::<f64>()
        - xs[..period].iter().sum::<f64>())
        / (period * period) as f64;
    let mut s = vec![0.0_f64; period];
    for i in 0..period {
        s[i] = xs[i] - initial_l;
    }
    let mut l = initial_l;
    let mut t = initial_t;
    let mut out = Vec::with_capacity(n);
    out.push(StrykeValue::float(l + s[0]));
    for i in 1..n {
        let s_idx = i % period;
        let prev_l = l;
        let prev_t = t;
        l = alpha * (xs[i] - s[s_idx]) + (1.0 - alpha) * (prev_l + prev_t);
        t = beta * (l - prev_l) + (1.0 - beta) * prev_t;
        s[s_idx] = gamma * (xs[i] - l) + (1.0 - gamma) * s[s_idx];
        out.push(StrykeValue::float(l + s[s_idx]));
    }
    Ok(StrykeValue::array(out))
}

/// Yule-Walker AR(p) coefficients via Levinson-Durbin recursion.
fn builtin_arma_yw_fit(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let p = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    let n = xs.len();
    if n < p + 1 {
        return Ok(StrykeValue::array(vec![]));
    }
    let mean: f64 = xs.iter().sum::<f64>() / n as f64;
    let centered: Vec<f64> = xs.iter().map(|v| v - mean).collect();
    let mut r = vec![0.0_f64; p + 1];
    for k in 0..=p {
        let mut s = 0.0_f64;
        for i in 0..n - k {
            s += centered[i] * centered[i + k];
        }
        r[k] = s / n as f64;
    }
    // Levinson-Durbin.
    let mut phi = vec![0.0_f64; p];
    let mut e = r[0];
    for i in 0..p {
        let mut k = r[i + 1];
        for j in 0..i {
            k -= phi[j] * r[i - j];
        }
        if e.abs() < 1e-15 {
            break;
        }
        k /= e;
        let phi_old = phi.clone();
        phi[i] = k;
        for j in 0..i {
            phi[j] = phi_old[j] - k * phi_old[i - 1 - j];
        }
        e *= 1.0 - k * k;
    }
    Ok(StrykeValue::array(phi.into_iter().map(StrykeValue::float).collect()))
}

// ─────────────────────────────────────────────────────────────────────────────
// 13. Graph centrality
// ─────────────────────────────────────────────────────────────────────────────

/// PageRank via power iteration.
fn builtin_pagerank(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let damping = args.get(1).map(|v| v.to_number()).unwrap_or(0.85);
    let iters = args.get(2).map(|v| v.to_number() as usize).unwrap_or(100);
    let n = adj.len();
    if n == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let mut rank = vec![1.0 / n as f64; n];
    for _ in 0..iters {
        let mut new_rank = vec![(1.0 - damping) / n as f64; n];
        for u in 0..n {
            let out_deg = adj[u].len().max(1);
            let share = damping * rank[u] / out_deg as f64;
            for &v in &adj[u] {
                if v < n {
                    new_rank[v] += share;
                }
            }
        }
        rank = new_rank;
    }
    Ok(StrykeValue::array(rank.into_iter().map(StrykeValue::float).collect()))
}

/// Brandes' algorithm — betweenness centrality (unweighted).
fn builtin_betweenness_centrality(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let mut cb = vec![0.0_f64; n];
    for s in 0..n {
        let mut stack: Vec<usize> = Vec::new();
        let mut pred: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut sigma = vec![0.0_f64; n];
        let mut dist: Vec<i64> = vec![-1; n];
        sigma[s] = 1.0;
        dist[s] = 0;
        let mut q: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        q.push_back(s);
        while let Some(v) = q.pop_front() {
            stack.push(v);
            for &w in &adj[v] {
                if w >= n {
                    continue;
                }
                if dist[w] < 0 {
                    dist[w] = dist[v] + 1;
                    q.push_back(w);
                }
                if dist[w] == dist[v] + 1 {
                    sigma[w] += sigma[v];
                    pred[w].push(v);
                }
            }
        }
        let mut delta = vec![0.0_f64; n];
        while let Some(w) = stack.pop() {
            for &v in &pred[w] {
                delta[v] += (sigma[v] / sigma[w]) * (1.0 + delta[w]);
            }
            if w != s {
                cb[w] += delta[w];
            }
        }
    }
    Ok(StrykeValue::array(cb.into_iter().map(StrykeValue::float).collect()))
}

/// Closeness centrality C(v) = (n-1) / Σ d(v, u). 0 if disconnected.
fn builtin_closeness_centrality(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let mut cc = vec![0.0_f64; n];
    for s in 0..n {
        let mut dist = vec![-1_i64; n];
        dist[s] = 0;
        let mut q: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
        q.push_back(s);
        let mut total = 0_i64;
        let mut reached = 0_i64;
        while let Some(v) = q.pop_front() {
            for &w in &adj[v] {
                if w < n && dist[w] < 0 {
                    dist[w] = dist[v] + 1;
                    total += dist[w];
                    reached += 1;
                    q.push_back(w);
                }
            }
        }
        if total > 0 {
            cc[s] = reached as f64 / total as f64;
        }
    }
    Ok(StrykeValue::array(cc.into_iter().map(StrykeValue::float).collect()))
}

/// Eigenvector centrality via power iteration on adjacency matrix.
fn builtin_eigenvector_centrality(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let mut v = vec![1.0_f64 / (n as f64).sqrt(); n];
    for _ in 0..100 {
        let mut nv = vec![0.0_f64; n];
        for u in 0..n {
            for &w in &adj[u] {
                if w < n {
                    nv[u] += v[w];
                }
            }
        }
        let norm = nv.iter().map(|x| x * x).sum::<f64>().sqrt();
        if norm < 1e-15 {
            break;
        }
        for x in nv.iter_mut() {
            *x /= norm;
        }
        v = nv;
    }
    Ok(StrykeValue::array(v.into_iter().map(StrykeValue::float).collect()))
}

/// `degree_centrality` — Degree centrality. Returns a float.
fn builtin_degree_centrality(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let denom = (n as f64 - 1.0).max(1.0);
    Ok(StrykeValue::array(
        adj.iter()
            .map(|nbrs| StrykeValue::float(nbrs.len() as f64 / denom))
            .collect(),
    ))
}

/// Triangle count per vertex (undirected interpretation).
fn builtin_triangle_count(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let sets: Vec<std::collections::HashSet<usize>> = adj
        .iter()
        .map(|nbrs| nbrs.iter().copied().collect())
        .collect();
    let mut counts = vec![0_i64; n];
    for u in 0..n {
        for &v in &adj[u] {
            if v <= u {
                continue;
            }
            for &w in &adj[v] {
                if w <= v {
                    continue;
                }
                if sets[u].contains(&w) {
                    counts[u] += 1;
                    counts[v] += 1;
                    counts[w] += 1;
                }
            }
        }
    }
    let _ = sets.len();
    Ok(StrykeValue::array(
        counts.into_iter().map(StrykeValue::integer).collect(),
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
// 14. Random samplers for new distributions
// ─────────────────────────────────────────────────────────────────────────────

/// `rgumbel` — Rgumbel. Returns a float.
fn builtin_rgumbel(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let mu = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    let u: f64 = rng.gen_range(1e-15..1.0);
    Ok(StrykeValue::float(mu - beta * (-u.ln()).ln()))
}

/// `rfrechet` — Rfrechet. Returns a float.
fn builtin_rfrechet(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let alpha = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let s = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    let u: f64 = rng.gen_range(1e-15..1.0);
    Ok(StrykeValue::float(s * (-u.ln()).powf(-1.0 / alpha)))
}

/// `rrayleigh` — Rrayleigh. Returns a float.
fn builtin_rrayleigh(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let sigma = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    let u: f64 = rng.gen_range(1e-15..1.0);
    Ok(StrykeValue::float(sigma * (-2.0 * u.ln()).sqrt()))
}

/// `rlogistic` — Rlogistic. Returns a float.
fn builtin_rlogistic(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let mu = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let s = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    let u: f64 = rng.gen_range(1e-15..(1.0 - 1e-15));
    Ok(StrykeValue::float(mu + s * (u / (1.0 - u)).ln()))
}

/// `rkumaraswamy` — Rkumaraswamy. Returns a float.
fn builtin_rkumaraswamy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    use rand::Rng;
    let a = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut rng = rand::thread_rng();
    let u: f64 = rng.gen_range(1e-15..(1.0 - 1e-15));
    Ok(StrykeValue::float(
        (1.0 - (1.0 - u).powf(1.0 / b)).powf(1.0 / a),
    ))
}

/// Sample a Gamma(α, scale=θ) variate via Marsaglia-Tsang (α ≥ 1) or by
/// raising `α + 1` and applying U^(1/α). Avoids the rand_distr dependency.
fn sample_gamma(alpha: f64, theta: f64) -> f64 {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    if alpha < 1.0 {
        let g = sample_gamma(alpha + 1.0, 1.0);
        let u: f64 = rng.gen_range(1e-300..1.0);
        return theta * g * u.powf(1.0 / alpha);
    }
    let d = alpha - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let x: f64 = {
            // Box-Muller standard normal.
            let u1: f64 = rng.gen_range(1e-300..1.0);
            let u2: f64 = rng.gen();
            (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
        };
        let v = (1.0 + c * x).powi(3);
        if v <= 0.0 {
            continue;
        }
        let u: f64 = rng.gen_range(1e-300..1.0);
        let xx = x * x;
        if u < 1.0 - 0.0331 * xx * xx {
            return theta * d * v;
        }
        if u.ln() < 0.5 * xx + d * (1.0 - v + v.ln()) {
            return theta * d * v;
        }
    }
}

/// `rinverse_gamma` — Rinverse gamma. Returns a float.
fn builtin_rinverse_gamma(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let alpha = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let s = sample_gamma(alpha, 1.0 / beta);
    if s.abs() < 1e-300 {
        return Ok(StrykeValue::float(f64::INFINITY));
    }
    Ok(StrykeValue::float(1.0 / s))
}

// ─────────────────────────────────────────────────────────────────────────────
// 15. 2-D convex hull + line geometry
// ─────────────────────────────────────────────────────────────────────────────

/// Graham-scan convex hull. Points: `[[x, y], …]`. Returns hull in CCW order.
fn builtin_graham_scan(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pts: Vec<(f64, f64)> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|p| {
            let v = arg_to_vec(p);
            (
                v.first().map(|x| x.to_number()).unwrap_or(0.0),
                v.get(1).map(|x| x.to_number()).unwrap_or(0.0),
            )
        })
        .collect();
    if pts.len() < 3 {
        return Ok(StrykeValue::array(
            pts.into_iter()
                .map(|(x, y)| StrykeValue::array(vec![StrykeValue::float(x), StrykeValue::float(y)]))
                .collect(),
        ));
    }
    let mut p = pts.clone();
    // Pivot = lowest-y (then lowest-x).
    let pivot_idx = (0..p.len())
        .min_by(|&a, &b| {
            p[a].1
                .partial_cmp(&p[b].1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(p[a].0.partial_cmp(&p[b].0).unwrap_or(std::cmp::Ordering::Equal))
        })
        .unwrap();
    p.swap(0, pivot_idx);
    let pivot = p[0];
    p[1..].sort_by(|a, b| {
        let cross = (a.0 - pivot.0) * (b.1 - pivot.1) - (a.1 - pivot.1) * (b.0 - pivot.0);
        if cross > 0.0 {
            std::cmp::Ordering::Less
        } else if cross < 0.0 {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    let mut stack: Vec<(f64, f64)> = Vec::new();
    for q in p {
        while stack.len() > 1 {
            let n = stack.len();
            let cross = (stack[n - 1].0 - stack[n - 2].0) * (q.1 - stack[n - 2].1)
                - (stack[n - 1].1 - stack[n - 2].1) * (q.0 - stack[n - 2].0);
            if cross <= 0.0 {
                stack.pop();
            } else {
                break;
            }
        }
        stack.push(q);
    }
    Ok(StrykeValue::array(
        stack
            .into_iter()
            .map(|(x, y)| StrykeValue::array(vec![StrykeValue::float(x), StrykeValue::float(y)]))
            .collect(),
    ))
}

/// Line-line intersection in 2D. Args: P1, P2, P3, P4. Returns intersection
/// `[x, y]` or undef when parallel.
fn builtin_line_line_intersect_2d(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let to_pair = |v: &StrykeValue| -> (f64, f64) {
        let xs = arg_to_vec(v);
        (
            xs.first().map(|x| x.to_number()).unwrap_or(0.0),
            xs.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        )
    };
    let p1 = to_pair(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let p2 = to_pair(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let p3 = to_pair(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let p4 = to_pair(&args.get(3).cloned().unwrap_or(StrykeValue::UNDEF));
    let denom = (p1.0 - p2.0) * (p3.1 - p4.1) - (p1.1 - p2.1) * (p3.0 - p4.0);
    if denom.abs() < 1e-15 {
        return Ok(StrykeValue::UNDEF);
    }
    let t = ((p1.0 - p3.0) * (p3.1 - p4.1) - (p1.1 - p3.1) * (p3.0 - p4.0)) / denom;
    Ok(StrykeValue::array(vec![
        StrykeValue::float(p1.0 + t * (p2.0 - p1.0)),
        StrykeValue::float(p1.1 + t * (p2.1 - p1.1)),
    ]))
}

/// Point-segment distance in 2D.
fn builtin_point_segment_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let to_pair = |v: &StrykeValue| -> (f64, f64) {
        let xs = arg_to_vec(v);
        (
            xs.first().map(|x| x.to_number()).unwrap_or(0.0),
            xs.get(1).map(|x| x.to_number()).unwrap_or(0.0),
        )
    };
    let p = to_pair(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let a = to_pair(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let b = to_pair(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-30 {
        return Ok(StrykeValue::float(
            ((p.0 - a.0).powi(2) + (p.1 - a.1).powi(2)).sqrt(),
        ));
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
    let qx = a.0 + t * dx;
    let qy = a.1 + t * dy;
    Ok(StrykeValue::float(
        ((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt(),
    ))
}
