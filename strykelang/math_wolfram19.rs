// Batch 19 — numerical integration deep, ODE solvers, root finding extras.

// Composite Boole's rule (Newton-Cotes 4 panels)
fn builtin_boole_rule(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(3).map(|v| v.to_number() as usize).unwrap_or(8).max(4);
    let n = n - (n % 4);
    let h = (b - a) / n as f64;
    let mut sum = 0.0;
    for i in (0..n).step_by(4) {
        let x0 = a + i as f64 * h;
        let y0 = call_user_1(interp, &f, x0, line)?;
        let y1 = call_user_1(interp, &f, x0 + h, line)?;
        let y2 = call_user_1(interp, &f, x0 + 2.0 * h, line)?;
        let y3 = call_user_1(interp, &f, x0 + 3.0 * h, line)?;
        let y4 = call_user_1(interp, &f, x0 + 4.0 * h, line)?;
        sum += (2.0 * h / 45.0) * (7.0 * y0 + 32.0 * y1 + 12.0 * y2 + 32.0 * y3 + 7.0 * y4);
    }
    Ok(PerlValue::float(sum))
}

// Gauss-Legendre 5 point
fn builtin_gauss_legendre_5(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(-1.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let nodes = [
        -0.9061798459386640,
        -0.5384693101056831,
        0.0,
        0.5384693101056831,
        0.9061798459386640,
    ];
    let weights = [
        0.2369268850561891,
        0.4786286704993665,
        0.5688888888888889,
        0.4786286704993665,
        0.2369268850561891,
    ];
    let half = (b - a) / 2.0;
    let mid = (b + a) / 2.0;
    let mut sum = 0.0;
    for i in 0..5 {
        let x = mid + half * nodes[i];
        sum += weights[i] * call_user_1(interp, &f, x, line)?;
    }
    Ok(PerlValue::float(half * sum))
}

// Gauss-Kronrod 7-15 (returns [estimate, error])
fn builtin_gauss_kronrod_15(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(-1.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    // 7-point Gauss-Legendre nodes/weights
    let g_nodes = [
        -0.9491079123427585,
        -0.7415311855993945,
        -0.4058451513773972,
        0.0,
        0.4058451513773972,
        0.7415311855993945,
        0.9491079123427585,
    ];
    let g_weights = [
        0.1294849661688697,
        0.2797053914892767,
        0.3818300505051189,
        0.4179591836734694,
        0.3818300505051189,
        0.2797053914892767,
        0.1294849661688697,
    ];
    // 15-point Kronrod nodes/weights (simplified)
    let k_nodes = [
        -0.9914553711208126,
        -0.9491079123427585,
        -0.8648644233597691,
        -0.7415311855993945,
        -0.5860872354676911,
        -0.4058451513773972,
        -0.2077849550078985,
        0.0,
        0.2077849550078985,
        0.4058451513773972,
        0.5860872354676911,
        0.7415311855993945,
        0.8648644233597691,
        0.9491079123427585,
        0.9914553711208126,
    ];
    let k_weights = [
        0.0229353220105292,
        0.0630920926299786,
        0.1047900103222502,
        0.1406532597155259,
        0.1690047266392679,
        0.1903505780647854,
        0.2044329400752989,
        0.2094821410847278,
        0.2044329400752989,
        0.1903505780647854,
        0.1690047266392679,
        0.1406532597155259,
        0.1047900103222502,
        0.0630920926299786,
        0.0229353220105292,
    ];
    let half = (b - a) / 2.0;
    let mid = (b + a) / 2.0;
    let mut g_sum = 0.0;
    for i in 0..7 {
        g_sum += g_weights[i] * call_user_1(interp, &f, mid + half * g_nodes[i], line)?;
    }
    let mut k_sum = 0.0;
    for i in 0..15 {
        k_sum += k_weights[i] * call_user_1(interp, &f, mid + half * k_nodes[i], line)?;
    }
    let est = half * k_sum;
    let err = (half * (k_sum - g_sum)).abs();
    Ok(PerlValue::array(vec![PerlValue::float(est), PerlValue::float(err)]))
}

// Romberg integration (Richardson extrapolation on trapezoid)
fn builtin_romberg_b19(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let max_n = args.get(3).map(|v| v.to_number() as usize).unwrap_or(8).max(2);
    let mut r = vec![vec![0.0; max_n]; max_n];
    let h = b - a;
    let fa = call_user_1(interp, &f, a, line)?;
    let fb = call_user_1(interp, &f, b, line)?;
    r[0][0] = 0.5 * h * (fa + fb);
    for k in 1..max_n {
        let mut step_sum = 0.0;
        let pts = 1 << (k - 1);
        let h_k = h / (pts as f64 * 2.0);
        for i in 0..pts {
            let x = a + (2 * i + 1) as f64 * h_k;
            step_sum += call_user_1(interp, &f, x, line)?;
        }
        r[k][0] = 0.5 * r[k - 1][0] + h_k * step_sum;
        for j in 1..=k {
            let p = 4_f64.powi(j as i32);
            r[k][j] = (p * r[k][j - 1] - r[k - 1][j - 1]) / (p - 1.0);
        }
    }
    Ok(PerlValue::float(r[max_n - 1][max_n - 1]))
}

// Adaptive Simpson
fn builtin_adaptive_simpson_b19(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-8);
    fn simpson_quad(
        interp: &mut VMHelper,
        f: &PerlValue,
        a: f64,
        b: f64,
        line: usize,
    ) -> PerlResult<f64> {
        let m = 0.5 * (a + b);
        let fa = call_user_1(interp, f, a, line)?;
        let fb = call_user_1(interp, f, b, line)?;
        let fm = call_user_1(interp, f, m, line)?;
        Ok(((b - a) / 6.0) * (fa + 4.0 * fm + fb))
    }
    fn recur(
        interp: &mut VMHelper,
        f: &PerlValue,
        a: f64,
        b: f64,
        tol: f64,
        whole: f64,
        depth: usize,
        line: usize,
    ) -> PerlResult<f64> {
        let m = 0.5 * (a + b);
        let left = simpson_quad(interp, f, a, m, line)?;
        let right = simpson_quad(interp, f, m, b, line)?;
        if depth == 0 || (left + right - whole).abs() < 15.0 * tol {
            return Ok(left + right + (left + right - whole) / 15.0);
        }
        let l = recur(interp, f, a, m, tol / 2.0, left, depth - 1, line)?;
        let r = recur(interp, f, m, b, tol / 2.0, right, depth - 1, line)?;
        Ok(l + r)
    }
    let whole = simpson_quad(interp, &f, a, b, line)?;
    Ok(PerlValue::float(recur(interp, &f, a, b, tol, whole, 30, line)?))
}

// Double exponential (tanh-sinh) quadrature for [-1,1]
fn builtin_tanh_sinh_quad_b19(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(-1.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(3).map(|v| v.to_number() as usize).unwrap_or(40).max(10);
    let h = 6.0 / n as f64;
    let half = (b - a) / 2.0;
    let mid = (b + a) / 2.0;
    let mut sum = 0.0;
    for k in -(n as isize)..=(n as isize) {
        let t = k as f64 * h;
        let phi = (std::f64::consts::FRAC_PI_2 * t.sinh()).tanh();
        let dphi = std::f64::consts::FRAC_PI_2 * t.cosh()
            / (std::f64::consts::FRAC_PI_2 * t.sinh()).cosh().powi(2);
        let x = mid + half * phi;
        sum += dphi * call_user_1(interp, &f, x, line)?;
    }
    Ok(PerlValue::float(half * h * sum))
}

// Midpoint rule
fn builtin_midpoint_rule(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(3).map(|v| v.to_number() as usize).unwrap_or(100).max(1);
    let h = (b - a) / n as f64;
    let mut sum = 0.0;
    for i in 0..n {
        let x = a + (i as f64 + 0.5) * h;
        sum += call_user_1(interp, &f, x, line)?;
    }
    Ok(PerlValue::float(h * sum))
}

// Adams-Bashforth 4-step explicit ODE
fn builtin_adams_bashforth_4(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let y0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let t_end = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(4).map(|v| v.to_number() as usize).unwrap_or(100).max(4);
    let h = (t_end - t0) / n as f64;
    let mut ts = vec![t0];
    let mut ys = vec![y0];
    let f_call = |interp: &mut VMHelper, t: f64, y: f64| -> PerlResult<f64> {
        let sub = f
            .as_code_ref()
            .ok_or_else(|| PerlError::runtime("expected code ref", line))?;
        let r = exec_to_perl_result(
            interp.call_sub(
                &sub,
                vec![PerlValue::float(t), PerlValue::float(y)],
                WantarrayCtx::Scalar,
                line,
            ),
            "callback",
            line,
        )?;
        Ok(r.to_number())
    };
    // Bootstrap with RK4 for first 3 steps
    for _ in 0..3 {
        let t = *ts.last().unwrap();
        let y = *ys.last().unwrap();
        let k1 = f_call(interp, t, y)?;
        let k2 = f_call(interp, t + h / 2.0, y + h * k1 / 2.0)?;
        let k3 = f_call(interp, t + h / 2.0, y + h * k2 / 2.0)?;
        let k4 = f_call(interp, t + h, y + h * k3)?;
        ys.push(y + h * (k1 + 2.0 * k2 + 2.0 * k3 + k4) / 6.0);
        ts.push(t + h);
    }
    let mut fs: Vec<f64> = Vec::new();
    for i in 0..ts.len() {
        fs.push(f_call(interp, ts[i], ys[i])?);
    }
    for k in 3..n {
        let y = ys[k]
            + h * (55.0 * fs[k] - 59.0 * fs[k - 1] + 37.0 * fs[k - 2] - 9.0 * fs[k - 3]) / 24.0;
        let t = ts[k] + h;
        ys.push(y);
        ts.push(t);
        fs.push(f_call(interp, t, y)?);
    }
    Ok(PerlValue::array(ys.into_iter().map(PerlValue::float).collect()))
}

// Heun's method (improved Euler)
fn builtin_heun_method(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let y0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let t_end = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(4).map(|v| v.to_number() as usize).unwrap_or(100).max(1);
    let h = (t_end - t0) / n as f64;
    let mut ys = vec![y0];
    let mut t = t0;
    let mut y = y0;
    let f_call = |interp: &mut VMHelper, t: f64, y: f64| -> PerlResult<f64> {
        let sub = f
            .as_code_ref()
            .ok_or_else(|| PerlError::runtime("expected code ref", line))?;
        let r = exec_to_perl_result(
            interp.call_sub(
                &sub,
                vec![PerlValue::float(t), PerlValue::float(y)],
                WantarrayCtx::Scalar,
                line,
            ),
            "callback",
            line,
        )?;
        Ok(r.to_number())
    };
    for _ in 0..n {
        let k1 = f_call(interp, t, y)?;
        let y_pred = y + h * k1;
        let k2 = f_call(interp, t + h, y_pred)?;
        y += h * 0.5 * (k1 + k2);
        t += h;
        ys.push(y);
    }
    Ok(PerlValue::array(ys.into_iter().map(PerlValue::float).collect()))
}

// RK45 Cash-Karp adaptive (returns final y)
fn builtin_rk45_cash_karp(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let t_end = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let mut h = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    let tol = args.get(5).map(|v| v.to_number()).unwrap_or(1e-6);
    let f_call = |interp: &mut VMHelper, t: f64, y: f64| -> PerlResult<f64> {
        let sub = f
            .as_code_ref()
            .ok_or_else(|| PerlError::runtime("expected code ref", line))?;
        let r = exec_to_perl_result(
            interp.call_sub(
                &sub,
                vec![PerlValue::float(t), PerlValue::float(y)],
                WantarrayCtx::Scalar,
                line,
            ),
            "callback",
            line,
        )?;
        Ok(r.to_number())
    };
    let mut iter = 0;
    while t < t_end && iter < 10000 {
        iter += 1;
        let k1 = h * f_call(interp, t, y)?;
        let k2 = h * f_call(interp, t + h / 5.0, y + k1 / 5.0)?;
        let k3 = h * f_call(interp, t + 3.0 * h / 10.0, y + 3.0 * k1 / 40.0 + 9.0 * k2 / 40.0)?;
        let k4 = h * f_call(
            interp,
            t + 3.0 * h / 5.0,
            y + 3.0 * k1 / 10.0 - 9.0 * k2 / 10.0 + 6.0 * k3 / 5.0,
        )?;
        let k5 = h * f_call(
            interp,
            t + h,
            y - 11.0 * k1 / 54.0 + 5.0 * k2 / 2.0 - 70.0 * k3 / 27.0 + 35.0 * k4 / 27.0,
        )?;
        let k6 = h * f_call(
            interp,
            t + 7.0 * h / 8.0,
            y + 1631.0 * k1 / 55296.0
                + 175.0 * k2 / 512.0
                + 575.0 * k3 / 13824.0
                + 44275.0 * k4 / 110592.0
                + 253.0 * k5 / 4096.0,
        )?;
        let y4 = y + 37.0 * k1 / 378.0 + 250.0 * k3 / 621.0 + 125.0 * k4 / 594.0 + 512.0 * k6 / 1771.0;
        let y5 = y + 2825.0 * k1 / 27648.0
            + 18575.0 * k3 / 48384.0
            + 13525.0 * k4 / 55296.0
            + 277.0 * k5 / 14336.0
            + k6 / 4.0;
        let err = (y5 - y4).abs();
        if err < tol {
            t += h;
            y = y5;
        }
        let s = if err > 0.0 { 0.84 * (tol / err).powf(0.25) } else { 2.0 };
        h *= s.clamp(0.1, 4.0);
        if t + h > t_end {
            h = t_end - t;
        }
    }
    Ok(PerlValue::float(y))
}

// Milne's predictor-corrector
fn builtin_milne_pc(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let y0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let t_end = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(4).map(|v| v.to_number() as usize).unwrap_or(100).max(4);
    let h = (t_end - t0) / n as f64;
    let f_call = |interp: &mut VMHelper, t: f64, y: f64| -> PerlResult<f64> {
        let sub = f
            .as_code_ref()
            .ok_or_else(|| PerlError::runtime("expected code ref", line))?;
        let r = exec_to_perl_result(
            interp.call_sub(
                &sub,
                vec![PerlValue::float(t), PerlValue::float(y)],
                WantarrayCtx::Scalar,
                line,
            ),
            "callback",
            line,
        )?;
        Ok(r.to_number())
    };
    let mut ys = vec![y0];
    let mut t = t0;
    let mut y = y0;
    for _ in 0..3 {
        let k1 = f_call(interp, t, y)?;
        let k2 = f_call(interp, t + h / 2.0, y + h * k1 / 2.0)?;
        let k3 = f_call(interp, t + h / 2.0, y + h * k2 / 2.0)?;
        let k4 = f_call(interp, t + h, y + h * k3)?;
        y += h * (k1 + 2.0 * k2 + 2.0 * k3 + k4) / 6.0;
        t += h;
        ys.push(y);
    }
    let mut fs: Vec<f64> = Vec::new();
    for i in 0..ys.len() {
        fs.push(f_call(interp, t0 + i as f64 * h, ys[i])?);
    }
    for k in 3..n {
        let y_pred = ys[k - 3] + 4.0 * h * (2.0 * fs[k] - fs[k - 1] + 2.0 * fs[k - 2]) / 3.0;
        let f_pred = f_call(interp, t + h, y_pred)?;
        let y_corr = ys[k - 1] + h * (fs[k - 1] + 4.0 * fs[k] + f_pred) / 3.0;
        ys.push(y_corr);
        fs.push(f_call(interp, t + h, y_corr)?);
        t += h;
    }
    Ok(PerlValue::array(ys.into_iter().map(PerlValue::float).collect()))
}

// Bulirsch-Stoer (simplified — modified midpoint + Richardson)
fn builtin_modified_midpoint_ode(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let y0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let h_total = args.get(3).map(|v| v.to_number()).unwrap_or(0.1);
    let n = args.get(4).map(|v| v.to_number() as usize).unwrap_or(20).max(2);
    let f_call = |interp: &mut VMHelper, t: f64, y: f64| -> PerlResult<f64> {
        let sub = f
            .as_code_ref()
            .ok_or_else(|| PerlError::runtime("expected code ref", line))?;
        let r = exec_to_perl_result(
            interp.call_sub(
                &sub,
                vec![PerlValue::float(t), PerlValue::float(y)],
                WantarrayCtx::Scalar,
                line,
            ),
            "callback",
            line,
        )?;
        Ok(r.to_number())
    };
    let h = h_total / n as f64;
    let mut z0 = y0;
    let mut z1 = y0 + h * f_call(interp, t0, y0)?;
    let mut t = t0 + h;
    for _ in 1..n {
        let z2 = z0 + 2.0 * h * f_call(interp, t, z1)?;
        z0 = z1;
        z1 = z2;
        t += h;
    }
    let yh = 0.5 * (z1 + z0 + h * f_call(interp, t, z1)?);
    Ok(PerlValue::float(yh))
}

// Backward Euler (implicit, 1 Newton step approximation)
fn builtin_backward_euler(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let y0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let t_end = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(4).map(|v| v.to_number() as usize).unwrap_or(100).max(1);
    let h = (t_end - t0) / n as f64;
    let f_call = |interp: &mut VMHelper, t: f64, y: f64| -> PerlResult<f64> {
        let sub = f
            .as_code_ref()
            .ok_or_else(|| PerlError::runtime("expected code ref", line))?;
        let r = exec_to_perl_result(
            interp.call_sub(
                &sub,
                vec![PerlValue::float(t), PerlValue::float(y)],
                WantarrayCtx::Scalar,
                line,
            ),
            "callback",
            line,
        )?;
        Ok(r.to_number())
    };
    let mut ys = vec![y0];
    let mut t = t0;
    let mut y = y0;
    for _ in 0..n {
        let mut y_next = y + h * f_call(interp, t + h, y)?;
        for _ in 0..5 {
            let g = y_next - y - h * f_call(interp, t + h, y_next)?;
            let dg = 1.0 - h * (f_call(interp, t + h, y_next + 1e-7)? - f_call(interp, t + h, y_next)?) / 1e-7;
            if dg.abs() < 1e-12 { break; }
            y_next -= g / dg;
        }
        y = y_next;
        t += h;
        ys.push(y);
    }
    Ok(PerlValue::array(ys.into_iter().map(PerlValue::float).collect()))
}

// Trapezoidal rule for ODE (Crank-Nicolson 1-D)
fn builtin_crank_nicolson_ode(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let y0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let t_end = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(4).map(|v| v.to_number() as usize).unwrap_or(100).max(1);
    let h = (t_end - t0) / n as f64;
    let f_call = |interp: &mut VMHelper, t: f64, y: f64| -> PerlResult<f64> {
        let sub = f
            .as_code_ref()
            .ok_or_else(|| PerlError::runtime("expected code ref", line))?;
        let r = exec_to_perl_result(
            interp.call_sub(
                &sub,
                vec![PerlValue::float(t), PerlValue::float(y)],
                WantarrayCtx::Scalar,
                line,
            ),
            "callback",
            line,
        )?;
        Ok(r.to_number())
    };
    let mut ys = vec![y0];
    let mut t = t0;
    let mut y = y0;
    for _ in 0..n {
        let f_n = f_call(interp, t, y)?;
        let mut y_next = y + h * f_n;
        for _ in 0..3 {
            let f_n1 = f_call(interp, t + h, y_next)?;
            y_next = y + 0.5 * h * (f_n + f_n1);
        }
        y = y_next;
        t += h;
        ys.push(y);
    }
    Ok(PerlValue::array(ys.into_iter().map(PerlValue::float).collect()))
}

// Brent's method root finding
fn builtin_brent_root(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-10);
    let mut fa = call_user_1(interp, &f, a, line)?;
    let mut fb = call_user_1(interp, &f, b, line)?;
    if fa * fb > 0.0 {
        return Ok(PerlValue::float(f64::NAN));
    }
    if fa.abs() < fb.abs() {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut fa, &mut fb);
    }
    let mut c = a;
    let mut fc = fa;
    let mut mflag = true;
    let mut d = 0.0;
    let mut iter = 0;
    while fb.abs() > tol && (b - a).abs() > tol && iter < 100 {
        iter += 1;
        let s = if fa != fc && fb != fc {
            a * fb * fc / ((fa - fb) * (fa - fc))
                + b * fa * fc / ((fb - fa) * (fb - fc))
                + c * fa * fb / ((fc - fa) * (fc - fb))
        } else {
            b - fb * (b - a) / (fb - fa)
        };
        let cond1 = !((s - (3.0 * a + b) / 4.0) * (s - b) < 0.0);
        let cond2 = mflag && (s - b).abs() >= (b - c).abs() / 2.0;
        let cond3 = !mflag && (s - b).abs() >= (c - d).abs() / 2.0;
        let s = if cond1 || cond2 || cond3 {
            mflag = true;
            (a + b) / 2.0
        } else {
            mflag = false;
            s
        };
        let fs = call_user_1(interp, &f, s, line)?;
        d = c;
        c = b;
        fc = fb;
        if fa * fs < 0.0 {
            b = s;
            fb = fs;
        } else {
            a = s;
            fa = fs;
        }
        if fa.abs() < fb.abs() {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut fa, &mut fb);
        }
    }
    Ok(PerlValue::float(b))
}

// Ridders' root method
fn builtin_ridders_root(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-10);
    let mut fa = call_user_1(interp, &f, a, line)?;
    let mut fb = call_user_1(interp, &f, b, line)?;
    if fa * fb > 0.0 {
        return Ok(PerlValue::float(f64::NAN));
    }
    for _ in 0..100 {
        let m = 0.5 * (a + b);
        let fm = call_user_1(interp, &f, m, line)?;
        let s = (fm * fm - fa * fb).sqrt();
        if s == 0.0 {
            return Ok(PerlValue::float(m));
        }
        let sign = if fa - fb > 0.0 { 1.0 } else { -1.0 };
        let x = m + (m - a) * sign * fm / s;
        let fx = call_user_1(interp, &f, x, line)?;
        if fx.abs() < tol {
            return Ok(PerlValue::float(x));
        }
        if fm * fx < 0.0 {
            a = m;
            fa = fm;
            b = x;
            fb = fx;
        } else if fa * fx < 0.0 {
            b = x;
            fb = fx;
        } else {
            a = x;
            fa = fx;
        }
        if (b - a).abs() < tol {
            return Ok(PerlValue::float(0.5 * (a + b)));
        }
    }
    Ok(PerlValue::float(0.5 * (a + b)))
}

// Anderson acceleration step (1-D Aitken-like)
fn builtin_anderson_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x0, x1, x2) = f3(args);
    let denom = x2 - 2.0 * x1 + x0;
    if denom.abs() < 1e-15 {
        return Ok(PerlValue::float(x2));
    }
    Ok(PerlValue::float(x0 - (x1 - x0).powi(2) / denom))
}

// Steffensen's method
fn builtin_steffensen_root(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let tol = args.get(2).map(|v| v.to_number()).unwrap_or(1e-10);
    for _ in 0..100 {
        let fx = call_user_1(interp, &f, x, line)?;
        if fx.abs() < tol {
            return Ok(PerlValue::float(x));
        }
        let fxx = call_user_1(interp, &f, x + fx, line)?;
        let denom = fxx - fx;
        if denom.abs() < 1e-15 {
            return Ok(PerlValue::float(x));
        }
        x -= fx * fx / denom;
    }
    Ok(PerlValue::float(x))
}

// Halley's method
fn builtin_halley_root(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let df = args.get(1).cloned().unwrap_or(PerlValue::UNDEF);
    let ddf = args.get(2).cloned().unwrap_or(PerlValue::UNDEF);
    let mut x = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let tol = args.get(4).map(|v| v.to_number()).unwrap_or(1e-10);
    for _ in 0..100 {
        let fx = call_user_1(interp, &f, x, line)?;
        if fx.abs() < tol {
            return Ok(PerlValue::float(x));
        }
        let dfx = call_user_1(interp, &df, x, line)?;
        let ddfx = call_user_1(interp, &ddf, x, line)?;
        let denom = 2.0 * dfx * dfx - fx * ddfx;
        if denom.abs() < 1e-15 {
            return Ok(PerlValue::float(x));
        }
        x -= 2.0 * fx * dfx / denom;
    }
    Ok(PerlValue::float(x))
}

// Householder method 3rd order
fn builtin_householder_root(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    builtin_halley_root(interp, args, line)
}

// Aberth-Ehrlich step (single root iteration for poly)
fn builtin_aberth_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let fs: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let dfs: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = xs.len();
    let mut out = xs.clone();
    for i in 0..n {
        if dfs.get(i).map_or(true, |&d| d == 0.0) { continue; }
        let mut sum = 0.0;
        for j in 0..n {
            if i != j {
                let d = xs[i] - xs[j];
                if d.abs() > 1e-15 {
                    sum += 1.0 / d;
                }
            }
        }
        let w = (fs[i] / dfs[i]) / (1.0 - (fs[i] / dfs[i]) * sum);
        out[i] = xs[i] - w;
    }
    Ok(PerlValue::array(out.into_iter().map(PerlValue::float).collect()))
}

// Muller's method
fn builtin_muller_root(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut x0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut x1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let mut x2 = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let tol = args.get(4).map(|v| v.to_number()).unwrap_or(1e-10);
    for _ in 0..100 {
        let f0 = call_user_1(interp, &f, x0, line)?;
        let f1 = call_user_1(interp, &f, x1, line)?;
        let f2 = call_user_1(interp, &f, x2, line)?;
        if f2.abs() < tol { return Ok(PerlValue::float(x2)); }
        let h0 = x1 - x0;
        let h1 = x2 - x1;
        if h0.abs() < 1e-15 || h1.abs() < 1e-15 { return Ok(PerlValue::float(x2)); }
        let d0 = (f1 - f0) / h0;
        let d1 = (f2 - f1) / h1;
        let a = (d1 - d0) / (h1 + h0);
        let b = a * h1 + d1;
        let c = f2;
        let disc = (b * b - 4.0 * a * c).max(0.0);
        let sqd = disc.sqrt();
        let denom = if (b + sqd).abs() > (b - sqd).abs() { b + sqd } else { b - sqd };
        if denom.abs() < 1e-15 { return Ok(PerlValue::float(x2)); }
        let dx = -2.0 * c / denom;
        x0 = x1; x1 = x2; x2 += dx;
    }
    Ok(PerlValue::float(x2))
}

// False position (regula falsi)
fn builtin_regula_falsi(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-10);
    let mut fa = call_user_1(interp, &f, a, line)?;
    let mut fb = call_user_1(interp, &f, b, line)?;
    for _ in 0..100 {
        let c = (a * fb - b * fa) / (fb - fa);
        let fc = call_user_1(interp, &f, c, line)?;
        if fc.abs() < tol { return Ok(PerlValue::float(c)); }
        if fa * fc < 0.0 { b = c; fb = fc; }
        else { a = c; fa = fc; }
    }
    Ok(PerlValue::float(a))
}

// Bisection method explicit
fn builtin_bisection_b19(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-10);
    let mut fa = call_user_1(interp, &f, a, line)?;
    let fb = call_user_1(interp, &f, b, line)?;
    if fa * fb > 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    while (b - a).abs() > tol {
        let c = 0.5 * (a + b);
        let fc = call_user_1(interp, &f, c, line)?;
        if fc.abs() < tol { return Ok(PerlValue::float(c)); }
        if fa * fc < 0.0 { b = c; }
        else { a = c; fa = fc; }
    }
    Ok(PerlValue::float(0.5 * (a + b)))
}

// Secant method explicit
fn builtin_secant_root(
    interp: &mut VMHelper,
    args: &[PerlValue],
    line: usize,
) -> PerlResult<PerlValue> {
    let f = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let mut x0 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut x1 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let tol = args.get(3).map(|v| v.to_number()).unwrap_or(1e-10);
    for _ in 0..100 {
        let f0 = call_user_1(interp, &f, x0, line)?;
        let f1 = call_user_1(interp, &f, x1, line)?;
        if f1.abs() < tol { return Ok(PerlValue::float(x1)); }
        let denom = f1 - f0;
        if denom.abs() < 1e-15 { return Ok(PerlValue::float(x1)); }
        let x2 = x1 - f1 * (x1 - x0) / denom;
        x0 = x1; x1 = x2;
    }
    Ok(PerlValue::float(x1))
}

// Inverse quadratic interpolation step
fn builtin_inverse_quad_interp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let (x0, x1, x2) = f3(args);
    let f0 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let f1 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let f2 = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let r = (f1 - f2) * (f0 - f2);
    let s = (f0 - f1) * (f1 - f2);
    let t = (f2 - f0) * (f0 - f1);
    if r * s * t == 0.0 { return Ok(PerlValue::float(x2)); }
    let next = x0 * f1 * f2 / r + x1 * f0 * f2 / s + x2 * f0 * f1 / t;
    Ok(PerlValue::float(next))
}

// Levenberg-Marquardt step (1-D)
fn builtin_lm_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let j = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    let denom = j * j + lambda;
    if denom.abs() < 1e-15 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(j * r / denom))
}

// Gradient descent step (1-D)
fn builtin_gradient_descent_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let grad = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lr = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(x - lr * grad))
}

// Adam optimizer step (returns updated [x, m, v])
fn builtin_adam_step_b19(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let v = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let lr = args.get(5).map(|v| v.to_number()).unwrap_or(0.001);
    let b1 = args.get(6).map(|v| v.to_number()).unwrap_or(0.9);
    let b2 = args.get(7).map(|v| v.to_number()).unwrap_or(0.999);
    let eps = 1e-8;
    let m1 = b1 * m + (1.0 - b1) * g;
    let v1 = b2 * v + (1.0 - b2) * g * g;
    let m_hat = m1 / (1.0 - b1.powf(t));
    let v_hat = v1 / (1.0 - b2.powf(t));
    let x1 = x - lr * m_hat / (v_hat.sqrt() + eps);
    Ok(PerlValue::array(vec![
        PerlValue::float(x1),
        PerlValue::float(m1),
        PerlValue::float(v1),
    ]))
}

// RMSprop step
fn builtin_rmsprop_step_b19(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let lr = args.get(3).map(|v| v.to_number()).unwrap_or(0.001);
    let rho = args.get(4).map(|v| v.to_number()).unwrap_or(0.9);
    let eps = 1e-8;
    let v1 = rho * v + (1.0 - rho) * g * g;
    let x1 = x - lr * g / (v1.sqrt() + eps);
    Ok(PerlValue::array(vec![PerlValue::float(x1), PerlValue::float(v1)]))
}

// Nesterov momentum step
fn builtin_nesterov_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let lr = args.get(3).map(|v| v.to_number()).unwrap_or(0.001);
    let mu = args.get(4).map(|v| v.to_number()).unwrap_or(0.9);
    let v1 = mu * v - lr * g;
    let x1 = x + mu * v1 - lr * g;
    Ok(PerlValue::array(vec![PerlValue::float(x1), PerlValue::float(v1)]))
}

// Adagrad step
fn builtin_adagrad_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let g_acc = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let lr = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let eps = 1e-8;
    let g_acc1 = g_acc + g * g;
    let x1 = x - lr * g / (g_acc1.sqrt() + eps);
    Ok(PerlValue::array(vec![PerlValue::float(x1), PerlValue::float(g_acc1)]))
}

// Conjugate gradient β (Polak-Ribière)
fn builtin_cg_beta_pr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g_new = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let g_old = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    if g_old.is_empty() { return Ok(PerlValue::float(0.0)); }
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 0..g_new.len().min(g_old.len()) {
        let n = g_new[i].to_number();
        let o = g_old[i].to_number();
        num += n * (n - o);
        den += o * o;
    }
    if den.abs() < 1e-15 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(num / den))
}

// Conjugate gradient β (Fletcher-Reeves)
fn builtin_cg_beta_fr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g_new = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let g_old = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    if g_old.is_empty() { return Ok(PerlValue::float(0.0)); }
    let num: f64 = g_new.iter().map(|v| v.to_number().powi(2)).sum();
    let den: f64 = g_old.iter().map(|v| v.to_number().powi(2)).sum();
    if den.abs() < 1e-15 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(num / den))
}

// BFGS approximation update step (1-D Hessian update)
fn builtin_bfgs_h_update_1d(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = f1(args);
    let s = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let y = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if s.abs() < 1e-15 || y.abs() < 1e-15 { return Ok(PerlValue::float(h)); }
    let new_h = h - h * h * s * s / (s * h * s) + y * y / (y * s);
    Ok(PerlValue::float(new_h))
}

// Wolfe condition strong check
fn builtin_wolfe_strong_q(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f0 = f1(args);
    let f_new = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let g0 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let g_new = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let p = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
    let c1 = args.get(6).map(|v| v.to_number()).unwrap_or(1e-4);
    let c2 = args.get(7).map(|v| v.to_number()).unwrap_or(0.9);
    let armijo = f_new <= f0 + c1 * alpha * g0 * p;
    let curvature = (g_new * p).abs() <= c2 * (g0 * p).abs();
    Ok(PerlValue::integer(if armijo && curvature { 1 } else { 0 }))
}

// Trust region step (1-D dogleg)
fn builtin_dogleg_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = f1(args);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let delta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let p_b = -g / h;
    if p_b.abs() <= delta {
        return Ok(PerlValue::float(p_b));
    }
    let p_u = -g * g / (g * h * g);
    if p_u.abs() >= delta {
        return Ok(PerlValue::float(delta * (-g).signum()));
    }
    Ok(PerlValue::float(p_u + (delta - p_u.abs()) * (p_b - p_u).signum()))
}

// Nelder-Mead reflection simplex 1-D
fn builtin_nelder_mead_reflect(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let centroid = f1(args);
    let worst = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(centroid + alpha * (centroid - worst)))
}

// Nelder-Mead expansion
fn builtin_nelder_mead_expand(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let centroid = f1(args);
    let reflected = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    Ok(PerlValue::float(centroid + gamma * (reflected - centroid)))
}

// Nelder-Mead contraction
fn builtin_nelder_mead_contract(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let centroid = f1(args);
    let worst = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float(centroid + beta * (worst - centroid)))
}

// Simulated annealing accept probability
fn builtin_sa_accept_prob(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let de = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if de < 0.0 { return Ok(PerlValue::float(1.0)); }
    if t <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((-de / t).exp()))
}

// Boltzmann temperature schedule
fn builtin_sa_boltzmann_temp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t0 = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if k <= 0.0 { return Ok(PerlValue::float(t0)); }
    Ok(PerlValue::float(t0 / (1.0 + k).ln()))
}

// Cauchy temperature schedule
fn builtin_sa_cauchy_temp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t0 = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(t0 / (1.0 + k)))
}

// Geometric schedule
fn builtin_sa_geometric_temp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t0 = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.95);
    let k = args.get(2).map(|v| v.to_number() as i32).unwrap_or(1);
    Ok(PerlValue::float(t0 * alpha.powi(k)))
}

// Acceptance ratio target adapter
fn builtin_acceptance_target(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let actual = f1(args);
    let target = args.get(1).map(|v| v.to_number()).unwrap_or(0.234);
    let step = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if actual > target {
        Ok(PerlValue::float(step * 1.1))
    } else {
        Ok(PerlValue::float(step * 0.9))
    }
}
