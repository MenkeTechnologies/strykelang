// Batch 34 — ODE advanced: BDF, Gear, Rosenbrock, IMEX, stiff solvers, predictor-corrector.

// BDF1 (implicit Euler) step (1 Newton iteration approximation)
fn builtin_bdf1_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * f_y))
}

// BDF2 step (2-step backward differentiation formula, explicit form approx)
fn builtin_bdf2_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y_n = f1(args);
    let y_nm1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_np1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float((4.0 / 3.0) * y_n - (1.0 / 3.0) * y_nm1 + (2.0 / 3.0) * dt * f_np1))
}

// BDF3 step
fn builtin_bdf3_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y_n = f1(args);
    let y_nm1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let y_nm2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let f_np1 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float((18.0 / 11.0) * y_n - (9.0 / 11.0) * y_nm1 + (2.0 / 11.0) * y_nm2
        + (6.0 / 11.0) * dt * f_np1))
}

// BDF4 step
fn builtin_bdf4_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y_n = f1(args);
    let y_nm1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let y_nm2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let y_nm3 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let f_np1 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(5).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float((48.0 / 25.0) * y_n - (36.0 / 25.0) * y_nm1
        + (16.0 / 25.0) * y_nm2 - (3.0 / 25.0) * y_nm3 + (12.0 / 25.0) * dt * f_np1))
}

// BDF5 step
fn builtin_bdf5_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y_n = f1(args);
    let y_nm1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let y_nm2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let y_nm3 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let y_nm4 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let f_np1 = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(6).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float((300.0 / 137.0) * y_n - (300.0 / 137.0) * y_nm1
        + (200.0 / 137.0) * y_nm2 - (75.0 / 137.0) * y_nm3 + (12.0 / 137.0) * y_nm4
        + (60.0 / 137.0) * dt * f_np1))
}

// BDF6 step
fn builtin_bdf6_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y_n = f1(args);
    let y_nm1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let y_nm2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let y_nm3 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let y_nm4 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let y_nm5 = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let f_np1 = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(7).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float((360.0 / 147.0) * y_n - (450.0 / 147.0) * y_nm1
        + (400.0 / 147.0) * y_nm2 - (225.0 / 147.0) * y_nm3 + (72.0 / 147.0) * y_nm4
        - (10.0 / 147.0) * y_nm5 + (60.0 / 147.0) * dt * f_np1))
}

// Adams-Bashforth 1 (explicit Euler)
fn builtin_ab1_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * f_n))
}

// Adams-Bashforth 2
fn builtin_ab2_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_nm1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * (1.5 * f_n - 0.5 * f_nm1)))
}

// Adams-Bashforth 3
fn builtin_ab3_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_nm1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let f_nm2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * ((23.0 / 12.0) * f_n - (16.0 / 12.0) * f_nm1 + (5.0 / 12.0) * f_nm2)))
}

// Adams-Moulton 2 (implicit trapezoidal)
fn builtin_am2_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_np1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * 0.5 * (f_n + f_np1)))
}

// Adams-Moulton 3
fn builtin_am3_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_np1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_n = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let f_nm1 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * ((5.0 / 12.0) * f_np1 + (8.0 / 12.0) * f_n - (1.0 / 12.0) * f_nm1)))
}

// Adams-Moulton 4
fn builtin_am4_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_np1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_n = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let f_nm1 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let f_nm2 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(5).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * ((9.0 / 24.0) * f_np1 + (19.0 / 24.0) * f_n
        - (5.0 / 24.0) * f_nm1 + (1.0 / 24.0) * f_nm2)))
}

// Rosenbrock-Wanner ROS2 stage (linearly implicit, single stage approx)
fn builtin_ros2_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let j = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let gamma = 1.0 + 1.0 / (2.0_f64).sqrt();
    let denom = 1.0 - dt * gamma * j;
    if denom == 0.0 { return Ok(PerlValue::float(y + dt * f_y)); }
    let k1 = f_y / denom;
    Ok(PerlValue::float(y + dt * k1))
}

// IMEX Euler (split f = f_E + f_I, explicit + implicit)
fn builtin_imex_euler_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_e = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_i = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * (f_e + f_i)))
}

// Symplectic Euler (Hamiltonian systems)
fn builtin_symplectic_euler_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dh_dq = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dh_dp = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    let p_new = p - dt * dh_dq;
    let q_new = q + dt * dh_dp;
    Ok(PerlValue::array(vec![PerlValue::float(p_new), PerlValue::float(q_new)]))
}

// Leapfrog (Verlet) step for second-order ODE q'' = a(q,t)
fn builtin_leapfrog_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let a_new = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    let q_new = q + v * dt + 0.5 * a * dt * dt;
    let v_new = v + 0.5 * (a + a_new) * dt;
    Ok(PerlValue::array(vec![PerlValue::float(q_new), PerlValue::float(v_new)]))
}

// Stormer-Verlet position update (q_n+1 = 2q_n - q_n-1 + a dt²)
fn builtin_stormer_verlet_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_n = f1(args);
    let q_nm1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let a = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(2.0 * q_n - q_nm1 + a * dt * dt))
}

// RK4 ODE step (single sample)
fn builtin_rk4_single(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let k1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let k2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let k3 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let k4 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(5).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * (k1 + 2.0 * k2 + 2.0 * k3 + k4) / 6.0))
}

// Dormand-Prince RK45 simple update from k1..k7
fn builtin_dopri5_combine(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let ks: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    if ks.len() < 7 { return Ok(PerlValue::float(y)); }
    let b = [35.0/384.0, 0.0, 500.0/1113.0, 125.0/192.0, -2187.0/6784.0, 11.0/84.0, 0.0];
    let mut sum = 0.0;
    for i in 0..7 { sum += b[i] * ks[i]; }
    Ok(PerlValue::float(y + dt * sum))
}

// Fehlberg RK45 (Cash-Karp) error estimate from k1..k6
fn builtin_rkf45_error(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ks: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    if ks.len() < 6 { return Ok(PerlValue::float(0.0)); }
    let e = [1.0/360.0, 0.0, -128.0/4275.0, -2197.0/75240.0, 1.0/50.0, 2.0/55.0];
    let mut err = 0.0;
    for i in 0..6 { err += e[i] * ks[i]; }
    Ok(PerlValue::float(dt * err.abs()))
}

// Lobatto IIIA s=2 (trapezoidal — already implemented as am2_step)
fn builtin_lobatto_iiia_2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_am2_step(args)
}

// Lobatto IIIC s=3 update (1 stage approximation)
fn builtin_lobatto_iiic_3(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let f_c = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * (f_a / 6.0 + 2.0 * f_b / 3.0 + f_c / 6.0)))
}

// Gauss-Legendre 2-stage IRK update
fn builtin_gauss_irk_2_stage(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + 0.5 * dt * (f_a + f_b)))
}

// Magnus expansion 1st order
fn builtin_magnus_1st(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_t0 = f1(args);
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float((dt * a_t0).exp()))
}

// Local truncation error estimate for explicit Euler
fn builtin_euler_lte(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f_prime = f1(args);
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(0.5 * f_prime * dt * dt))
}

// LTE for trapezoidal rule
fn builtin_trapezoidal_lte(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f_double_prime = f1(args);
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(-(dt.powi(3)) / 12.0 * f_double_prime))
}

// Step-size adaptation factor (PI controller)
fn builtin_pi_step_size(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let err = f1(args).max(1e-12);
    let err_prev = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    let order = args.get(2).map(|v| v.to_number()).unwrap_or(4.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let alpha = 0.7 / order;
    let beta = 0.4 / order;
    let scale = (1.0 / err).powf(alpha) * (err_prev).powf(beta);
    Ok(PerlValue::float(dt * scale.clamp(0.1, 4.0)))
}

// Stiffness ratio estimate from eigenvalues
fn builtin_stiffness_ratio(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lam_max = f1(args).abs();
    let lam_min = args.get(1).map(|v| v.to_number().abs()).unwrap_or(1.0).max(1e-12);
    Ok(PerlValue::float(lam_max / lam_min))
}

// Spectral radius from eigenvalues
fn builtin_spectral_radius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let eigs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number().abs()).collect();
    Ok(PerlValue::float(eigs.iter().cloned().fold(0.0_f64, f64::max)))
}

// Heun-Euler embedded pair (RK12)
fn builtin_heun_euler_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let k1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let k2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * 0.5 * (k1 + k2)))
}

// Bogacki-Shampine RK23 update from k1..k4
fn builtin_bogacki_shampine_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let k1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let k2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let k3 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * (2.0 * k1 + 3.0 * k2 + 4.0 * k3) / 9.0))
}

// 4th order Verner update from 8 stages (b coefficients only, simplified)
fn builtin_verner_8_combine(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let ks: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    if ks.len() < 8 { return Ok(PerlValue::float(y)); }
    let b = [13.0/160.0, 0.0, 0.0, 2375.0/5984.0, 5.0/16.0, 12.0/85.0, 3.0/44.0, 0.0];
    let mut sum = 0.0;
    for i in 0..8 { sum += b[i] * ks[i]; }
    Ok(PerlValue::float(y + dt * sum))
}

// Generic Runge-Kutta combine: y + dt * (sum b_i k_i)
fn builtin_rk_combine(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let bs: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let ks: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let n = bs.len().min(ks.len());
    let mut sum = 0.0;
    for i in 0..n { sum += bs[i] * ks[i]; }
    Ok(PerlValue::float(y + dt * sum))
}

// Linear multistep coefficient — Adams-Bashforth k-step coefficients (sum b_j)
fn builtin_ab_coeff_sum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let coeffs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    Ok(PerlValue::float(coeffs.iter().sum::<f64>()))
}

// Newmark-beta β=1/4, γ=1/2 step (constant acceleration)
fn builtin_newmark_beta_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let a_n = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let a_np1 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    let beta = 0.25;
    let gamma = 0.5;
    let u_new = u + v * dt + 0.5 * dt * dt * ((1.0 - 2.0 * beta) * a_n + 2.0 * beta * a_np1);
    let v_new = v + dt * ((1.0 - gamma) * a_n + gamma * a_np1);
    Ok(PerlValue::array(vec![PerlValue::float(u_new), PerlValue::float(v_new)]))
}

// Wilson-θ step (θ=1.4 default)
fn builtin_wilson_theta_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let a_n = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let a_np1 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args.get(4).map(|v| v.to_number()).unwrap_or(1.4);
    let dt = args.get(5).map(|v| v.to_number()).unwrap_or(0.01);
    let tau = theta * dt;
    let u_new = u + v * tau + 0.5 * tau * tau * (a_n + (a_np1 - a_n) / 3.0);
    let v_new = v + tau * 0.5 * (a_n + a_np1);
    Ok(PerlValue::array(vec![PerlValue::float(u_new), PerlValue::float(v_new)]))
}

// Operator splitting Strang (2nd order)
fn builtin_strang_split(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt / 2.0 * f_a + dt * f_b + dt / 2.0 * f_a))
}

// Operator splitting Lie (1st order)
fn builtin_lie_split(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let f_a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + dt * (f_a + f_b)))
}

// Exponential Euler step y_{n+1} = e^(λdt) y_n + (e^(λdt)-1)/λ · g(y_n,t_n)
fn builtin_exp_euler_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(-1.0);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let exp_lt = (lambda * dt).exp();
    let phi1 = if lambda.abs() < 1e-12 { dt } else { (exp_lt - 1.0) / lambda };
    Ok(PerlValue::float(exp_lt * y + phi1 * g))
}

// Lawson-Hatch ETD (exponential time differencing) RK2
fn builtin_etd_rk2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(-1.0);
    let n_y = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n_a = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    let exp_lt = (lambda * dt).exp();
    let phi1 = if lambda.abs() < 1e-12 { dt } else { (exp_lt - 1.0) / lambda };
    let phi2 = if lambda.abs() < 1e-12 { dt * dt / 2.0 }
               else { (exp_lt - 1.0 - lambda * dt) / (lambda * lambda * dt) };
    Ok(PerlValue::float(exp_lt * y + phi1 * n_y + phi2 * (n_a - n_y)))
}

// Krogh DDE (delay) Euler step
fn builtin_dde_euler_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y_now = f1(args);
    let y_delayed = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let _ = y_delayed;
    Ok(PerlValue::float(y_now + dt * f))
}

// Stochastic Euler-Maruyama step
fn builtin_em_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let drift = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let diff = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dw = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(4).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + drift * dt + diff * dw))
}

// Milstein step (single Itô SDE)
fn builtin_milstein_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let drift = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let diff = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let diff_prime = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dw = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(5).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + drift * dt + diff * dw + 0.5 * diff * diff_prime * (dw * dw - dt)))
}

// Heun-Maruyama (predictor-corrector SDE)
fn builtin_heun_sde_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let drift_y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let drift_pred = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let diff_y = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let diff_pred = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let dw = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(6).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y + 0.5 * dt * (drift_y + drift_pred) + 0.5 * dw * (diff_y + diff_pred)))
}

// Stratonovich correction term ½σσ'
fn builtin_stratonovich_correction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sigma = f1(args);
    let sigma_prime = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * sigma * sigma_prime))
}

// Predictor-corrector single iteration
fn builtin_predictor_corrector(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y_pred = f1(args);
    let f_pred = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_old = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(y_pred + 0.5 * dt * (f_pred - f_old)))
}

// Numerical Jacobian column (finite difference)
fn builtin_numerical_jacobian_col(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f_x = f1(args);
    let f_x_plus = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(2).map(|v| v.to_number()).unwrap_or(1e-6);
    if h == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((f_x_plus - f_x) / h))
}

// Crank-Nicolson coefficient half
fn builtin_cn_coefficient() -> PerlResult<PerlValue> {
    Ok(PerlValue::float(0.5))
}

// Implicit-explicit splitting weight ϑ
fn builtin_imex_theta_split(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    let f_e = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_i = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(theta * f_i + (1.0 - theta) * f_e))
}

// Bulirsch-Stoer Richardson extrapolation single step
fn builtin_bulirsch_stoer_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t1 = f1(args);
    let t2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n1 = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    let n2 = args.get(3).map(|v| v.to_number()).unwrap_or(4.0);
    if n2 == n1 { return Ok(PerlValue::float(t1)); }
    Ok(PerlValue::float(t2 + (t2 - t1) / (n2 / n1 - 1.0)))
}

// CFL number (Courant) c·Δt/Δx
fn builtin_cfl_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c = f1(args);
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    let dx = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if dx == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(c * dt / dx))
}

// Von Neumann stability factor for explicit diffusion
fn builtin_diffusion_stability(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha = f1(args);
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    let dx = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if dx == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(alpha * dt / (dx * dx)))
}

// Lax-Friedrichs flux (1D conservation law)
fn builtin_lax_friedrichs_flux(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f_l = f1(args);
    let f_r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let u_l = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let u_r = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dx_dt = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(0.5 * (f_l + f_r) - 0.5 * dx_dt * (u_r - u_l)))
}

// Lax-Wendroff flux (linear advection)
fn builtin_lax_wendroff_flux(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u_l = f1(args);
    let u_r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let dx = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    if dx == 0.0 { return Ok(PerlValue::float(0.0)); }
    let nu = c * dt / dx;
    Ok(PerlValue::float(0.5 * c * (u_l + u_r) - 0.5 * c * nu * (u_r - u_l)))
}

// MUSCL slope limiter (van Leer)
fn builtin_van_leer_limiter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    if r <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(2.0 * r / (1.0 + r)))
}

// Minmod slope limiter
fn builtin_minmod_limiter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    Ok(PerlValue::float(0.0_f64.max(1.0_f64.min(r))))
}

// Superbee limiter
fn builtin_superbee_limiter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    if r <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((2.0_f64.min(r)).max(1.0_f64.min(2.0 * r))))
}

// MC limiter
fn builtin_mc_limiter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    if r <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let bound = (2.0_f64).min(2.0 * r).min((1.0 + r) / 2.0);
    Ok(PerlValue::float(bound.max(0.0)))
}
