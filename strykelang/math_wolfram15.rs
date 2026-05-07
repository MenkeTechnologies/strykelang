// Batch 15 — astronomy/celestial mechanics, quantum gates and channels.

fn builtin_kepler_hyperbolic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args); let e = args.get(1).map(|v| v.to_number()).unwrap_or(1.5);
    let mut h = m.signum() * (2.0 * m.abs() / e + 1.6).ln();
    for _ in 0..50 {
        let f = e * h.sinh() - h - m;
        let fp = e * h.cosh() - 1.0;
        let dh = f / fp;
        h -= dh;
        if dh.abs() < 1e-13 { break; }
    }
    Ok(PerlValue::float(h))
}
fn builtin_hohmann_dv1(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r1 = f1(args); let r2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(2).map(|v| v.to_number()).unwrap_or(3.986e14);
    let v1 = (mu / r1).sqrt();
    let v_t1 = (mu * (2.0 / r1 - 2.0 / (r1 + r2))).sqrt();
    Ok(PerlValue::float(v_t1 - v1))
}
fn builtin_hohmann_dv2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r1 = f1(args); let r2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(2).map(|v| v.to_number()).unwrap_or(3.986e14);
    let v2 = (mu / r2).sqrt();
    let v_t2 = (mu * (2.0 / r2 - 2.0 / (r1 + r2))).sqrt();
    Ok(PerlValue::float(v2 - v_t2))
}
fn builtin_hohmann_total(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dv1 = builtin_hohmann_dv1(args)?.to_number();
    let dv2 = builtin_hohmann_dv2(args)?.to_number();
    Ok(PerlValue::float(dv1.abs() + dv2.abs()))
}
fn builtin_bielliptic_total(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r1 = f1(args); let r2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r_b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(3).map(|v| v.to_number()).unwrap_or(3.986e14);
    let v1 = (mu / r1).sqrt();
    let v_a = (mu * (2.0 / r1 - 2.0 / (r1 + r_b))).sqrt();
    let v_b1 = (mu * (2.0 / r_b - 2.0 / (r1 + r_b))).sqrt();
    let v_b2 = (mu * (2.0 / r_b - 2.0 / (r2 + r_b))).sqrt();
    let v_c = (mu * (2.0 / r2 - 2.0 / (r2 + r_b))).sqrt();
    let v2 = (mu / r2).sqrt();
    Ok(PerlValue::float((v_a - v1).abs() + (v_b2 - v_b1).abs() + (v2 - v_c).abs()))
}
fn builtin_lambert_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r1 = f1(args); let r2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let tof = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(3).map(|v| v.to_number()).unwrap_or(3.986e14);
    let a = (r1 + r2) / 2.0;
    let v = (mu * (2.0 / r1 - 1.0 / a)).max(0.0).sqrt();
    let _ = tof;
    Ok(PerlValue::float(v))
}
fn builtin_horizon_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = f1(args); let r = args.get(1).map(|v| v.to_number()).unwrap_or(6371000.0);
    Ok(PerlValue::float((2.0 * r * h + h * h).sqrt()))
}
fn builtin_solar_zenith_angle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lat = f1(args).to_radians();
    let dec = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let h = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    Ok(PerlValue::float((lat.sin() * dec.sin() + lat.cos() * dec.cos() * h.cos()).acos().to_degrees()))
}
fn builtin_air_mass_kasten(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let zen = f1(args).to_radians();
    Ok(PerlValue::float(1.0 / (zen.cos() + 0.50572 * (96.07995 - zen.to_degrees()).powf(-1.6364))))
}
fn builtin_solar_constant(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1361.0))
}
fn builtin_julian_centuries_j2000(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let jd = f1(args);
    Ok(PerlValue::float((jd - 2451545.0) / 36525.0))
}
fn builtin_mean_solar_longitude(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    Ok(PerlValue::float((280.46646 + 36000.76983 * t + 0.0003032 * t * t).rem_euclid(360.0)))
}
fn builtin_mean_solar_anomaly(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    Ok(PerlValue::float(357.52911 + 35999.05029 * t - 0.0001537 * t * t))
}
fn builtin_lst_to_solar(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lst = f1(args); let lon = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((lst - lon / 15.0).rem_euclid(24.0)))
}
fn builtin_ra_dec_to_az_alt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ra = f1(args).to_radians();
    let dec = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lat = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lst = args.get(3).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let h = lst - ra;
    let alt = (lat.sin() * dec.sin() + lat.cos() * dec.cos() * h.cos()).asin();
    let az = (-h.sin()).atan2(lat.cos() * dec.tan() - lat.sin() * h.cos());
    Ok(PerlValue::array(vec![
        PerlValue::float(az.to_degrees().rem_euclid(360.0)),
        PerlValue::float(alt.to_degrees()),
    ]))
}
fn builtin_ecliptic_to_equatorial(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lambda = f1(args).to_radians();
    let beta = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let eps = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(0.4090928);
    let ra = (lambda.sin() * eps.cos() - beta.tan() * eps.sin()).atan2(lambda.cos());
    let dec = (beta.sin() * eps.cos() + beta.cos() * eps.sin() * lambda.sin()).asin();
    Ok(PerlValue::array(vec![PerlValue::float(ra.to_degrees().rem_euclid(360.0)), PerlValue::float(dec.to_degrees())]))
}
fn builtin_equatorial_to_galactic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ra = f1(args).to_radians(); let dec = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let alpha_g = 192.85948_f64.to_radians();
    let delta_g = 27.12825_f64.to_radians();
    let l_n = 122.93192_f64.to_radians();
    let b = (delta_g.sin() * dec.sin() + delta_g.cos() * dec.cos() * (ra - alpha_g).cos()).asin();
    let l = l_n - ((dec.cos() * (ra - alpha_g).sin())
        .atan2(delta_g.cos() * dec.sin() - delta_g.sin() * dec.cos() * (ra - alpha_g).cos()));
    Ok(PerlValue::array(vec![PerlValue::float(l.to_degrees().rem_euclid(360.0)), PerlValue::float(b.to_degrees())]))
}
fn builtin_orbital_eccentricity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ra = f1(args); let rp = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((ra - rp) / (ra + rp).max(1e-30)))
}
fn builtin_semi_major_axis(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ra = f1(args); let rp = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((ra + rp) / 2.0))
}
fn builtin_specific_orbital_energy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args); let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(2).map(|v| v.to_number()).unwrap_or(3.986e14);
    Ok(PerlValue::float(0.5 * v * v - mu / r.max(1e-30)))
}
fn builtin_specific_angular_momentum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args); let v = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(std::f64::consts::FRAC_PI_2);
    Ok(PerlValue::float(r * v * theta.sin()))
}

// Quantum gates and channels
fn builtin_toffoli_gate(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut m = vec![vec![0.0_f64; 8]; 8];
    for i in 0..8 { m[i][i] = 1.0; }
    m[6][6] = 0.0; m[6][7] = 1.0; m[7][7] = 0.0; m[7][6] = 1.0;
    Ok(matrix_to_value(&m))
}
fn builtin_fredkin_gate(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut m = vec![vec![0.0_f64; 8]; 8];
    for i in 0..8 { m[i][i] = 1.0; }
    m[5][5] = 0.0; m[5][6] = 1.0; m[6][6] = 0.0; m[6][5] = 1.0;
    Ok(matrix_to_value(&m))
}
fn builtin_iswap_gate(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::array(vec![
        matrix_to_value(&[vec![1.0,0.0,0.0,0.0], vec![0.0,0.0,0.0,0.0], vec![0.0,0.0,0.0,0.0], vec![0.0,0.0,0.0,1.0]]),
        matrix_to_value(&[vec![0.0,0.0,0.0,0.0], vec![0.0,0.0,1.0,0.0], vec![0.0,1.0,0.0,0.0], vec![0.0,0.0,0.0,0.0]]),
    ]))
}
fn builtin_sqrt_swap_gate(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(matrix_to_value(&[
        vec![1.0, 0.0, 0.0, 0.0],
        vec![0.0, 0.5, 0.5, 0.0],
        vec![0.0, 0.5, 0.5, 0.0],
        vec![0.0, 0.0, 0.0, 1.0],
    ]))
}
fn builtin_rx_gate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    let c = (theta / 2.0).cos(); let s = (theta / 2.0).sin();
    Ok(PerlValue::array(vec![
        matrix_to_value(&[vec![c, 0.0], vec![0.0, c]]),
        matrix_to_value(&[vec![0.0, -s], vec![-s, 0.0]]),
    ]))
}
fn builtin_ry_gate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    let c = (theta / 2.0).cos(); let s = (theta / 2.0).sin();
    Ok(matrix_to_value(&[vec![c, -s], vec![s, c]]))
}
fn builtin_rz_gate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    let c = (theta / 2.0).cos(); let s = (theta / 2.0).sin();
    Ok(PerlValue::array(vec![
        matrix_to_value(&[vec![c, 0.0], vec![0.0, c]]),
        matrix_to_value(&[vec![-s, 0.0], vec![0.0, s]]),
    ]))
}
fn builtin_ghz_state_n(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(1) as usize;
    let s = 1.0 / 2.0_f64.sqrt();
    let dim = 1 << n;
    let mut state = vec![0.0_f64; dim];
    state[0] = s; state[dim - 1] = s;
    Ok(PerlValue::array(state.into_iter().map(PerlValue::float).collect()))
}
fn builtin_w_state_n(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(1) as usize;
    let dim = 1 << n;
    let amp = 1.0 / (n as f64).sqrt();
    let mut state = vec![0.0_f64; dim];
    for i in 0..n {
        state[1 << i] = amp;
    }
    Ok(PerlValue::array(state.into_iter().map(PerlValue::float).collect()))
}
fn builtin_depolarizing_channel(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rho = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = rho.len();
    let trace: f64 = (0..n).map(|i| rho[i][i]).sum();
    let mut out = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        for j in 0..n {
            out[i][j] = (1.0 - p) * rho[i][j];
        }
        out[i][i] += p * trace / n as f64;
    }
    Ok(matrix_to_value(&out))
}
fn builtin_dephasing_channel(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rho = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = rho.len();
    let mut out = rho.clone();
    for i in 0..n { for j in 0..n {
        if i != j { out[i][j] *= 1.0 - p; }
    }}
    Ok(matrix_to_value(&out))
}
fn builtin_amplitude_damping_channel(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rho = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if rho.len() != 2 { return Ok(matrix_to_value(&rho)); }
    let sg = (1.0 - g).sqrt();
    let mut out = vec![vec![0.0_f64; 2]; 2];
    out[0][0] = rho[0][0] + g * rho[1][1];
    out[0][1] = sg * rho[0][1];
    out[1][0] = sg * rho[1][0];
    out[1][1] = (1.0 - g) * rho[1][1];
    Ok(matrix_to_value(&out))
}
fn builtin_quantum_fidelity_pure(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    Ok(PerlValue::float(dot * dot))
}
fn builtin_trace_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rho = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let sigma = matrix_from_value(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let n = rho.len();
    let mut diff = vec![vec![0.0_f64; n]; n];
    for i in 0..n { for j in 0..n { diff[i][j] = rho[i][j] - sigma[i][j]; } }
    let evs = jacobi_eigenvalues(&mut diff.clone());
    let s: f64 = evs.iter().map(|x| x.abs()).sum();
    Ok(PerlValue::float(s / 2.0))
}
fn builtin_bell_inequality_chsh(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let e_ab = f1(args);
    let e_abp = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let e_apb = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let e_apbp = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((e_ab + e_abp + e_apb - e_apbp).abs()))
}
fn builtin_pauli_decomposition_2x2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    if m.len() != 2 { return Ok(PerlValue::array(vec![])); }
    let i_coef = (m[0][0] + m[1][1]) / 2.0;
    let x_coef = (m[0][1] + m[1][0]) / 2.0;
    let z_coef = (m[0][0] - m[1][1]) / 2.0;
    Ok(PerlValue::array(vec![
        PerlValue::float(i_coef), PerlValue::float(x_coef), PerlValue::float(z_coef),
    ]))
}
fn builtin_quantum_relative_entropy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut rho = matrix_from_value(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let evs_p = jacobi_eigenvalues(&mut rho);
    let mut sigma = matrix_from_value(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let evs_q = jacobi_eigenvalues(&mut sigma);
    let mut s = 0.0_f64;
    for (p, q) in evs_p.iter().zip(evs_q.iter()) {
        if *p > 1e-12 && *q > 1e-12 { s += p * (p / q).ln(); }
    }
    Ok(PerlValue::float(s))
}
fn builtin_qft_4_real(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = 4_usize;
    let mut re = vec![vec![0.0_f64; n]; n];
    let s = 0.5_f64;
    for j in 0..n { for k in 0..n {
        re[j][k] = s * (2.0 * std::f64::consts::PI * j as f64 * k as f64 / n as f64).cos();
    }}
    Ok(matrix_to_value(&re))
}
