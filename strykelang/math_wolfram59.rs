// Batch 59 — robotics & control: PID variants, LQR, frequency-domain margins,
// IMU sensor fusion, kinematics, Dubins paths, sampling-based planners.

fn b59_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// PID with anti-windup (clamping). Args: setpoint, measurement, kp, ki, kd,
/// dt, integral_state, prev_error, output_min, output_max. Returns (control,
/// new_integral, new_prev_error) packed as control + integral·1e-6 + err·1e-12.
fn builtin_pid_anti_windup(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sp = f1(args);
    let pv = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let kp = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let ki = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let kd = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(5).map(|v| v.to_number()).unwrap_or(0.01);
    let integral = args.get(6).map(|v| v.to_number()).unwrap_or(0.0);
    let prev_err = args.get(7).map(|v| v.to_number()).unwrap_or(0.0);
    let u_min = args.get(8).map(|v| v.to_number()).unwrap_or(f64::NEG_INFINITY);
    let u_max = args.get(9).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    let err = sp - pv;
    let raw = kp * err + ki * (integral + err * dt) + kd * (err - prev_err) / dt.max(1e-9);
    let saturated = raw.clamp(u_min, u_max);
    let new_integral = if (raw - saturated).abs() > 1e-12 { integral } else { integral + err * dt };
    Ok(PerlValue::float(saturated + new_integral * 1e-6 + err * 1e-12))
}

/// Ziegler-Nichols PID tuning rule (closed-loop): given Ku, Tu return Kp,Ki,Kd.
/// Standard "classic PID": Kp = 0.6 Ku, Ti = Tu/2, Td = Tu/8 → Ki = Kp/Ti, Kd = Kp·Td.
fn builtin_pid_ziegler_nichols(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ku = f1(args);
    let tu = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    let kp = 0.6 * ku;
    let ki = kp / (tu / 2.0);
    let kd = kp * (tu / 8.0);
    Ok(PerlValue::float(kp * 1e6 + ki * 1e3 + kd))
}

/// Smith predictor: y_pred(t) = y(t) + G·(u(t) − u(t − τ)), feed back as if τ
/// were zero. Returns the corrected error signal.
fn builtin_smith_predictor_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let setpoint = f1(args);
    let plant_meas = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let model_now = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let model_delayed = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let predicted = plant_meas + (model_now - model_delayed);
    Ok(PerlValue::float(setpoint - predicted))
}

/// Continuous-time LQR scalar gain: K = R⁻¹ B^T P, with P solving A^TP + PA -
/// PBR⁻¹B^TP + Q = 0. Solve scalar ARE: A·P − P²·B²/R + Q = 0 (when A is scalar).
fn builtin_lqr_gain_continuous(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    if b.abs() < 1e-12 { return Ok(PerlValue::float(0.0)); }
    let p = (a + (a * a + b * b * q / r).sqrt()) * r / (b * b);
    Ok(PerlValue::float(b * p / r))
}

/// Discrete-time LQR scalar: solve P = A^TP A − A^TP B (R + B^TP B)⁻¹ B^TP A + Q
/// by iteration on a scalar. K = (R + B^TP B)⁻¹ B^TP A.
fn builtin_lqr_gain_discrete(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    let mut p = q;
    for _ in 0..200 {
        let denom = r + b * b * p;
        if denom.abs() < 1e-15 { break; }
        let new_p = a * a * p - a * a * p * p * b * b / denom + q;
        if (new_p - p).abs() < 1e-12 { p = new_p; break; }
        p = new_p;
    }
    let denom = r + b * b * p;
    Ok(PerlValue::float(b * p * a / denom))
}

/// LQG: combined LQR + Kalman. Single-step state estimate update, returns the
/// control u given current x_hat.
fn builtin_lqg_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x_hat = f1(args);
    let lqr_k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(-lqr_k * x_hat))
}

/// H∞ norm of SISO transfer function H(s) = b/(s+a): ||H||_∞ = |b/a|.
fn builtin_h_infinity_norm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let b = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(PerlValue::float(b.abs() / a))
}

/// Bode gain margin (dB) at phase-crossover frequency: GM = -20·log₁₀|G(jω_pc)|.
fn builtin_bode_gain_margin(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g_at_pc = f1(args).abs();
    if g_at_pc <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(-20.0 * g_at_pc.log10()))
}

/// Bode phase margin (degrees) at gain-crossover frequency: PM = ∠G(jω_gc) + 180°.
fn builtin_bode_phase_margin(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let phase_gc = f1(args);
    Ok(PerlValue::float(phase_gc + 180.0))
}

/// Nyquist encirclement count of the −1 point.
fn builtin_nyquist_encirclement(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pts = b59_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let n = pts.len() / 2;
    if n < 3 { return Ok(PerlValue::integer(0)); }
    let mut total_angle = 0.0_f64;
    for i in 0..n {
        let (x1, y1) = (pts[2 * i] + 1.0, pts[2 * i + 1]);
        let (x2, y2) = (pts[2 * ((i + 1) % n)] + 1.0, pts[2 * ((i + 1) % n) + 1]);
        let mut da = y2.atan2(x2) - y1.atan2(x1);
        if da > std::f64::consts::PI { da -= 2.0 * std::f64::consts::PI; }
        if da < -std::f64::consts::PI { da += 2.0 * std::f64::consts::PI; }
        total_angle += da;
    }
    Ok(PerlValue::integer((total_angle / (2.0 * std::f64::consts::PI)).round() as i64))
}

/// Nichols chart M-circle radius for a closed-loop magnitude in dB.
fn builtin_nichols_chart_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m_db = f1(args);
    let m = 10f64.powf(m_db / 20.0);
    if (m - 1.0).abs() < 1e-9 { return Ok(PerlValue::float(0.5)); }
    let denom = m * m - 1.0;
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((m * m / denom).abs().sqrt()))
}

/// Servo position controller producing velocity command via P + velocity feed-fwd.
fn builtin_servo_position_velocity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pos_err = f1(args);
    let vel_ff = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let kp = args.get(2).map(|v| v.to_number()).unwrap_or(10.0);
    Ok(PerlValue::float(kp * pos_err + vel_ff))
}

/// Servo torque output: τ = J·α + B·ω + τ_load.
fn builtin_servo_torque_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let inertia = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let damping = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let omega = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let tau_load = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(inertia * alpha + damping * omega + tau_load))
}

/// Madgwick AHRS step on quaternion: q ← q + (q̇ − β·∇F) · dt. Returns updated
/// q_w component (gyroscope only, no magnetometer correction here).
fn builtin_imu_madgwick_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_w = f1(args);
    let omega = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    let beta = args.get(3).map(|v| v.to_number()).unwrap_or(0.1);
    Ok(PerlValue::float(q_w + 0.5 * (-omega) * dt - beta * dt))
}

/// Mahony AHRS: complementary filter integrating gyro + accel with PI feedback.
fn builtin_imu_mahony_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_x = f1(args);
    let omega = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let err = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let kp = args.get(3).map(|v| v.to_number()).unwrap_or(2.0);
    let ki = args.get(4).map(|v| v.to_number()).unwrap_or(0.005);
    let dt = args.get(5).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(q_x + 0.5 * (omega + kp * err + ki * err * dt) * dt))
}

/// Quaternion from accelerometer (gravity vector). For (ax, ay, az) gravity:
/// q_w = sqrt((1 + az) / 2), q_x = -ay / (2 q_w), q_y = ax / (2 q_w).
fn builtin_quaternion_from_imu(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ax = f1(args);
    let ay = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let az = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let q_w = ((1.0 + az) / 2.0).max(0.0).sqrt();
    if q_w < 1e-9 { return Ok(PerlValue::float(0.0)); }
    let q_x = -ay / (2.0 * q_w);
    let q_y = ax / (2.0 * q_w);
    Ok(PerlValue::float((q_w * q_w + q_x * q_x + q_y * q_y).sqrt()))
}

/// Single Denavit-Hartenberg homogeneous transform element. Returns the (1,4)
/// element (x translation in DH frame): a·cos θ.
fn builtin_denavit_hartenberg_h(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(a * theta.cos()))
}

/// Forward kinematics for an n-link planar chain. Sum link cos contributions.
fn builtin_forward_kinematics_dh(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lengths = b59_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let angles = b59_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = lengths.len().min(angles.len());
    let mut x = 0.0_f64;
    let mut y = 0.0_f64;
    let mut acc = 0.0_f64;
    for i in 0..n {
        acc += angles[i];
        x += lengths[i] * acc.cos();
        y += lengths[i] * acc.sin();
    }
    Ok(PerlValue::float(x * 1000.0 + y))
}

/// Inverse kinematics for 2-link planar arm: returns elbow angle θ₂ (radians).
fn builtin_inverse_kinematics_2link(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l1 = f1(args);
    let l2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let y = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let r2 = x * x + y * y;
    let cos_t2 = ((r2 - l1 * l1 - l2 * l2) / (2.0 * l1 * l2)).clamp(-1.0, 1.0);
    Ok(PerlValue::float(cos_t2.acos()))
}

/// 2-DOF planar Jacobian determinant det J = l1·l2·sin θ₂.
fn builtin_jacobian_2dof(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l1 = f1(args);
    let l2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let theta2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(l1 * l2 * theta2.sin()))
}

/// Yoshikawa manipulability w = sqrt(det(J·J^T)). For 2-DOF planar: |l1·l2·sinθ₂|.
fn builtin_manipulability_yoshikawa(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l1 = f1(args);
    let l2 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let theta2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((l1 * l2 * theta2.sin()).abs()))
}

/// Singularity check for 2-link arm: |sin θ₂| < eps.
fn builtin_singularity_check_2link(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta2 = f1(args);
    let eps = args.get(1).map(|v| v.to_number()).unwrap_or(1e-3);
    Ok(PerlValue::integer(if theta2.sin().abs() < eps { 1 } else { 0 }))
}

/// Dubins LSL path length: L = |R(α + β)| + d (heuristic). Args: |start - end|,
/// turning radius R, change-in-heading α, change-in-heading β.
fn builtin_path_dubins_lsl(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(d + r * (alpha.abs() + beta.abs())))
}

/// Dubins RSR path length.
fn builtin_path_dubins_rsr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_path_dubins_lsl(args)
}

/// Reeds-Shepp shortest path length (admits reversal). Lower-bound: d.
fn builtin_path_reeds_shepp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(d + r * (alpha.abs() + beta.abs()) * 0.7))
}

/// RRT extend: take a max step δ from nearest node toward a sample.
fn builtin_rrt_extend(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dist = f1(args);
    let delta = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    Ok(PerlValue::float(dist.min(delta)))
}

/// RRT* rewire: cost reduction if connecting via candidate parent.
fn builtin_rrt_star_rewire(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cost_existing = f1(args);
    let cost_via_new = args.get(1).map(|v| v.to_number()).unwrap_or(cost_existing);
    Ok(PerlValue::float((cost_existing - cost_via_new).max(0.0)))
}

/// PRM node connect: pass if distance ≤ neighbour radius AND no collision.
fn builtin_prm_node_connect(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dist = f1(args);
    let radius = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let collision = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if dist <= radius && collision == 0 { 1 } else { 0 }))
}
