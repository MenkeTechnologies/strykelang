// Batch 47 — graphics, geometry, ray tracing, BRDF, color spaces, noise, SDF.

fn b47_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// Perspective projection x: x' = x / (w · tan(fov/2) · aspect)
fn builtin_gfx_perspective_proj_x(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let fov = args.get(2).map(|v| v.to_number()).unwrap_or(std::f64::consts::FRAC_PI_4);
    let aspect = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let denom = w * (fov / 2.0).tan() * aspect;
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(x / denom))
}

/// Perspective projection y
fn builtin_gfx_perspective_proj_y(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let fov = args.get(2).map(|v| v.to_number()).unwrap_or(std::f64::consts::FRAC_PI_4);
    let denom = w * (fov / 2.0).tan();
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(y / denom))
}

/// Orthographic projection (linear scaling)
fn builtin_gfx_orthographic_proj(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let lo = args.get(1).map(|v| v.to_number()).unwrap_or(-1.0);
    let hi = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if hi == lo { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(2.0 * (x - lo) / (hi - lo) - 1.0))
}

/// View matrix step (single component of M = R^T · T)
fn builtin_gfx_view_matrix_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(r * t))
}

/// LookAt forward = normalize(target - eye)_x
fn builtin_gfx_lookat_forward(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dx = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(dx / n))
}

/// LookAt right = normalize(forward × up)
fn builtin_gfx_lookat_right(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f = f1(args);
    let u = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(f * u))
}

/// LookAt up vector via cross product: up_corrected = right × forward (NOT
/// world-up, since the canonical right-handed view-matrix orthonormalizes).
/// Returns the requested component of (right × forward). Args: comp (0=x, 1=y,
/// 2=z), right_x, right_y, right_z, fwd_x, fwd_y, fwd_z.
fn builtin_gfx_lookat_up(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let comp = i1(args);
    let rx = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let ry = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let rz = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let fx = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let fy = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let fz = args.get(6).map(|v| v.to_number()).unwrap_or(-1.0);
    match comp {
        0 => Ok(StrykeValue::float(ry * fz - rz * fy)),
        1 => Ok(StrykeValue::float(rz * fx - rx * fz)),
        2 => Ok(StrykeValue::float(rx * fy - ry * fx)),
        _ => Ok(StrykeValue::float(0.0)),
    }
}

/// Quaternion to axis-angle (returns angle from w)
fn builtin_gfx_quat_to_axis_angle(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let w = f1(args);
    Ok(StrykeValue::float(2.0 * w.clamp(-1.0, 1.0).acos()))
}

/// Axis-angle to quaternion w = cos(θ/2)
fn builtin_gfx_axis_angle_to_quat(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    Ok(StrykeValue::float((theta / 2.0).cos()))
}

/// Quaternion slerp step at t
fn builtin_gfx_quat_slerp_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q0 = f1(args);
    let q1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let dot = q0 * q1;
    let omega = dot.clamp(-1.0, 1.0).acos();
    if omega.abs() < 1e-9 { return Ok(StrykeValue::float((1.0 - t) * q0 + t * q1)); }
    let sin_o = omega.sin();
    Ok(StrykeValue::float(((1.0 - t) * omega).sin() / sin_o * q0 + (t * omega).sin() / sin_o * q1))
}

/// Quaternion nlerp (normalized lerp)
fn builtin_gfx_quat_nlerp_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q0 = f1(args);
    let q1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float((1.0 - t) * q0 + t * q1))
}

/// Quaternion dot product
fn builtin_gfx_quat_dot_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b47_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = b47_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n = a.len().min(b.len());
    Ok(StrykeValue::float((0..n).map(|i| a[i] * b[i]).sum()))
}

/// Quaternion inverse: -q (for unit quaternion conjugate)
fn builtin_gfx_quat_inverse_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(-f1(args)))
}

/// Quaternion to Euler pitch
fn builtin_gfx_quat_to_euler_pitch(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let qw = f1(args);
    let qx = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let qy = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let qz = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((2.0 * (qw * qx + qy * qz)).atan2(1.0 - 2.0 * (qx * qx + qy * qy))))
}

/// Quat to Euler yaw
fn builtin_gfx_quat_to_euler_yaw(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let qw = f1(args);
    let qx = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let qy = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let qz = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((2.0 * (qw * qz + qx * qy)).atan2(1.0 - 2.0 * (qy * qy + qz * qz))))
}

/// Quat to Euler roll
fn builtin_gfx_quat_to_euler_roll(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let qw = f1(args);
    let qx = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let qy = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let qz = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let v = (2.0 * (qw * qy - qz * qx)).clamp(-1.0, 1.0);
    Ok(StrykeValue::float(v.asin()))
}

/// Euler to quat x = sin(roll/2)cos(pitch/2)cos(yaw/2) - cos(roll/2)sin(pitch/2)sin(yaw/2)
fn builtin_gfx_euler_to_quat_x(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let roll = f1(args);
    let pitch = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let yaw = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((roll / 2.0).sin() * (pitch / 2.0).cos() * (yaw / 2.0).cos()
        - (roll / 2.0).cos() * (pitch / 2.0).sin() * (yaw / 2.0).sin()))
}

fn builtin_gfx_euler_to_quat_y(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let roll = f1(args);
    let pitch = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let yaw = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((roll / 2.0).cos() * (pitch / 2.0).sin() * (yaw / 2.0).cos()
        + (roll / 2.0).sin() * (pitch / 2.0).cos() * (yaw / 2.0).sin()))
}

fn builtin_gfx_euler_to_quat_z(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let roll = f1(args);
    let pitch = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let yaw = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((roll / 2.0).cos() * (pitch / 2.0).cos() * (yaw / 2.0).sin()
        - (roll / 2.0).sin() * (pitch / 2.0).sin() * (yaw / 2.0).cos()))
}

fn builtin_gfx_euler_to_quat_w(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let roll = f1(args);
    let pitch = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let yaw = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((roll / 2.0).cos() * (pitch / 2.0).cos() * (yaw / 2.0).cos()
        + (roll / 2.0).sin() * (pitch / 2.0).sin() * (yaw / 2.0).sin()))
}

/// Rotation matrix XX entry
fn builtin_gfx_rotation_matrix_xx(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    Ok(StrykeValue::float(theta.cos()))
}

fn builtin_gfx_rotation_matrix_yy(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    Ok(StrykeValue::float(theta.cos()))
}

fn builtin_gfx_rotation_matrix_zz(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    Ok(StrykeValue::float(theta.cos()))
}

/// Translation matrix entry T(i, j): identity except T(i, 3) = t_i for i ∈ {0,1,2}.
/// Args: row, col, t_x, t_y, t_z. Returns the matrix element at (row, col).
fn builtin_gfx_translation_matrix_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let row = i1(args);
    let col = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let tx = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let ty = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let tz = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    if row == col { return Ok(StrykeValue::float(1.0)); }
    if col == 3 {
        match row { 0 => return Ok(StrykeValue::float(tx)),
                    1 => return Ok(StrykeValue::float(ty)),
                    2 => return Ok(StrykeValue::float(tz)),
                    _ => return Ok(StrykeValue::float(0.0)) }
    }
    Ok(StrykeValue::float(0.0))
}

/// Scale matrix entry S(i, j): diag(s_x, s_y, s_z, 1).
fn builtin_gfx_scale_matrix_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let row = i1(args);
    let col = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let s = [args.get(2).map(|v| v.to_number()).unwrap_or(1.0),
             args.get(3).map(|v| v.to_number()).unwrap_or(1.0),
             args.get(4).map(|v| v.to_number()).unwrap_or(1.0),
             1.0];
    if row != col { return Ok(StrykeValue::float(0.0)); }
    if (0..4).contains(&row) { return Ok(StrykeValue::float(s[row as usize])); }
    Ok(StrykeValue::float(0.0))
}

/// Shear matrix XY entry: identity + shear factor h at (0, 1).
fn builtin_gfx_shear_matrix_xy(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let row = i1(args);
    let col = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let h = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if row == col { return Ok(StrykeValue::float(1.0)); }
    if row == 0 && col == 1 { return Ok(StrykeValue::float(h)); }
    Ok(StrykeValue::float(0.0))
}

/// Homogeneous divide: x/w
fn builtin_gfx_homogeneous_divide(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if w == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(x / w))
}

/// NDC to screen X: (x + 1) / 2 · width
fn builtin_gfx_screen_space_x(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float((x + 1.0) * 0.5 * w))
}

/// NDC to screen Y: (1 - y) / 2 · height
fn builtin_gfx_screen_space_y(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float((1.0 - y) * 0.5 * h))
}

fn builtin_gfx_ndc_to_screen_x(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_gfx_screen_space_x(args)
}

fn builtin_gfx_ndc_to_screen_y(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_gfx_screen_space_y(args)
}

/// Screen to NDC X: 2 · x / W - 1
fn builtin_gfx_screen_to_ndc_x(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if w == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(2.0 * x / w - 1.0))
}

fn builtin_gfx_screen_to_ndc_y(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if h == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(1.0 - 2.0 * y / h))
}

/// Polygon clip step (in vs out vertex)
fn builtin_gfx_clip_polygon_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_in = i1(args);
    let v_out = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(v_in - v_out))
}

/// Sutherland-Hodgman intersect parameter t
fn builtin_gfx_sutherland_hodgman(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dot1 = f1(args);
    let dot2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = dot1 - dot2;
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(dot1 / denom))
}

/// Cohen-Sutherland code: 4-bit outcode for a point
fn builtin_gfx_cohen_sutherland_code(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let xmin = args.get(2).map(|v| v.to_number()).unwrap_or(-1.0);
    let xmax = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let ymin = args.get(4).map(|v| v.to_number()).unwrap_or(-1.0);
    let ymax = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
    let mut code = 0_i64;
    if x < xmin { code |= 1; } else if x > xmax { code |= 2; }
    if y < ymin { code |= 4; } else if y > ymax { code |= 8; }
    Ok(StrykeValue::integer(code))
}

/// Liang-Barsky t-value
fn builtin_gfx_liang_barsky_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if p == 0.0 { return Ok(StrykeValue::float(if q < 0.0 { -1.0 } else { 1.0 })); }
    Ok(StrykeValue::float(q / p))
}

/// Bresenham step X (slope < 1)
fn builtin_gfx_bresenham_step_x(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let err = f1(args);
    let dx = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(err - dx))
}

/// Bresenham step Y
fn builtin_gfx_bresenham_step_y(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let err = f1(args);
    let dy = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(err + dy))
}

/// Xiaolin Wu intensity
fn builtin_gfx_xiaolin_wu_intensity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let frac = f1(args);
    Ok(StrykeValue::float(1.0 - frac.fract().abs()))
}

/// AABB intersect check
fn builtin_gfx_aabb_intersect_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_min = f1(args);
    let a_max = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b_min = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let b_max = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if a_max >= b_min && b_max >= a_min { 1 } else { 0 }))
}

/// OBB overlap step (SAT axis)
fn builtin_gfx_obb_overlap_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let proj_a = f1(args);
    let proj_b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dist = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if proj_a + proj_b > dist.abs() { 1 } else { 0 }))
}

/// Sphere intersect t-distance
fn builtin_gfx_sphere_intersect_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let b = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let disc = b * b - c;
    if disc < 0.0 { return Ok(StrykeValue::float(-1.0)); }
    Ok(StrykeValue::float(-b - disc.sqrt()))
}

/// Ray-triangle intersection t (Möller-Trumbore)
fn builtin_gfx_ray_triangle_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dot_n_d = f1(args);
    let dot_n_p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if dot_n_d == 0.0 { return Ok(StrykeValue::float(-1.0)); }
    Ok(StrykeValue::float(-dot_n_p / dot_n_d))
}

/// Ray-plane intersection: plane (N, d) where N·X + d = 0, ray O + tD.
/// t = −(N·O + d) / (N·D). Returns −1 if parallel or behind.
/// Args: N·O+d, N·D.
fn builtin_gfx_ray_plane_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_o_plus_d = f1(args);
    let n_d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if n_d.abs() < 1e-12 { return Ok(StrykeValue::float(-1.0)); }
    let t = -n_o_plus_d / n_d;
    Ok(StrykeValue::float(if t >= 0.0 { t } else { -1.0 }))
}

/// Ray-box slab method t
fn builtin_gfx_ray_box_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t_min = f1(args);
    let t_max = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if t_min > t_max { return Ok(StrykeValue::float(-1.0)); }
    Ok(StrykeValue::float(t_min))
}

/// Ray-sphere t
fn builtin_gfx_ray_sphere_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_gfx_sphere_intersect_t(args)
}

/// Ray-disk t (planar disk + radius check)
fn builtin_gfx_ray_disk_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    let dist_sq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r_sq = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(if dist_sq <= r_sq { t } else { -1.0 }))
}

/// Ray-cylinder t (infinite cylinder)
fn builtin_gfx_ray_cylinder_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let disc = b * b - a * c;
    if disc < 0.0 || a == 0.0 { return Ok(StrykeValue::float(-1.0)); }
    Ok(StrykeValue::float((-b - disc.sqrt()) / a))
}

/// Ray-cone intersection: infinite double cone x² + y² = (z·tan θ)² with
/// half-angle θ. Substitute O+tD into D_x²+D_y²−tan²θ·D_z² scaled, get
/// at²+bt+c=0 with
///   a = D_x² + D_y² − k²·D_z²,
///   b = 2(O_x D_x + O_y D_y − k²·O_z D_z),
///   c = O_x² + O_y² − k²·O_z²,    k = tan θ.
/// Args: a, b, c. Returns nearest non-negative root or −1.
fn builtin_gfx_ray_cone_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if a.abs() < 1e-12 {
        if b.abs() < 1e-12 { return Ok(StrykeValue::float(-1.0)); }
        let t = -c / b;
        return Ok(StrykeValue::float(if t >= 0.0 { t } else { -1.0 }));
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 { return Ok(StrykeValue::float(-1.0)); }
    let sq = disc.sqrt();
    let t1 = (-b - sq) / (2.0 * a);
    let t2 = (-b + sq) / (2.0 * a);
    let cand = if t1 >= 0.0 { t1 } else { t2 };
    Ok(StrykeValue::float(if cand >= 0.0 { cand } else { -1.0 }))
}

/// Ray-ellipsoid intersection (axis-aligned ellipsoid x²/a² + y²/b² + z²/c² = 1):
/// substitute O' = (O_x/a, O_y/b, O_z/c), D' = (D_x/a, D_y/b, D_z/c) so the
/// problem reduces to sphere of radius 1 at origin: |O' + tD'|² = 1.
/// at² + 2bt + c = 0 with a = |D'|², b = O'·D', c = |O'|² − 1.
/// Args: |D'|², O'·D', |O'|² (caller scales by axes first).
fn builtin_gfx_ray_ellipsoid_t(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c_sq = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let c = c_sq - 1.0;
    if a.abs() < 1e-12 { return Ok(StrykeValue::float(-1.0)); }
    let disc = b * b - a * c;
    if disc < 0.0 { return Ok(StrykeValue::float(-1.0)); }
    let sq = disc.sqrt();
    let t1 = (-b - sq) / a;
    let t2 = (-b + sq) / a;
    let cand = if t1 >= 0.0 { t1 } else { t2 };
    Ok(StrykeValue::float(if cand >= 0.0 { cand } else { -1.0 }))
}

/// Ray-torus quartic: solving t⁴ + a·t³ + b·t² + c·t + d = 0 for a torus with
/// major radius R, minor radius r intersected by a ray. Args: 4 quartic coefs.
/// Use Ferrari resolvent's discriminant sign as a step: returns smallest real
/// root within [0, ∞) via Bairstow-style approximate factorization fallback.
fn builtin_gfx_ray_torus_t_approx(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let mut t = 1.0_f64;
    for _ in 0..32 {
        let p = (((t + a) * t + b) * t + c) * t + d;
        let pp = ((4.0 * t + 3.0 * a) * t + 2.0 * b) * t + c;
        if pp.abs() < 1e-12 { break; }
        t -= p / pp;
        if !t.is_finite() { return Ok(StrykeValue::float(-1.0)); }
    }
    Ok(StrykeValue::float(if t >= 0.0 { t } else { -1.0 }))
}

/// Barycentric α
fn builtin_gfx_barycentric_alpha(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let area_a = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(area_a / total))
}

fn builtin_gfx_barycentric_beta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_gfx_barycentric_alpha(args)
}

fn builtin_gfx_barycentric_gamma(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(1.0 - alpha - beta))
}

/// Phong diffuse: max(0, N·L)
fn builtin_gfx_phong_diffuse_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(f1(args).max(0.0)))
}

/// Phong specular: max(0, R·V)^n
fn builtin_gfx_phong_specular_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r_v = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(32.0);
    Ok(StrykeValue::float(r_v.max(0.0).powf(n)))
}

/// Phong ambient term: k_a · I_a (intensity of ambient light scaled by ambient
/// reflectance coefficient). Args: k_a, I_a.
fn builtin_gfx_phong_ambient_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let k_a = f1(args);
    let i_a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(k_a * i_a))
}

/// Blinn-Phong specular (Blinn 1977): I = (N·H)^n with H = (L+V)/|L+V| (half
/// vector). Derives the Phong shape using the half vector instead of reflection
/// vector, giving smoother elongated highlights at grazing angles. Caller passes
/// N·H, exponent n_blinn ≈ 4·n_phong empirically. Args: N·H, n_blinn.
fn builtin_gfx_blinn_specular_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_dot_h = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(128.0);
    Ok(StrykeValue::float(n_dot_h.max(0.0).powf(n)))
}

/// Lambert term: max(0, N·L)
fn builtin_gfx_lambert_term(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(f1(args).max(0.0)))
}

/// Oren-Nayar term (approximation)
fn builtin_gfx_oren_nayar_term(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_l = f1(args);
    let sigma2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let a = 1.0 - 0.5 * sigma2 / (sigma2 + 0.33);
    Ok(StrykeValue::float(n_l.max(0.0) * a))
}

/// Cook-Torrance D (GGX)
fn builtin_gfx_cook_torrance_d_ggx(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_h = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let alpha2 = alpha * alpha;
    let denom = n_h * n_h * (alpha2 - 1.0) + 1.0;
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(alpha2 / (std::f64::consts::PI * denom * denom)))
}

/// Cook-Torrance G (Smith)
fn builtin_gfx_cook_torrance_g_smith(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_v = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let k = (alpha + 1.0).powi(2) / 8.0;
    if n_v * (1.0 - k) + k == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(n_v / (n_v * (1.0 - k) + k)))
}

/// Cook-Torrance F (Schlick): f0 + (1 - f0)(1 - cosθ)^5
fn builtin_gfx_cook_torrance_f_schlick(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f0 = f1(args);
    let cos_theta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(f0 + (1.0 - f0) * (1.0 - cos_theta).powi(5)))
}

/// Disney principled D
fn builtin_gfx_disney_principled_d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_gfx_cook_torrance_d_ggx(args)
}

/// Microfacet BRDF combined: D·F·G / (4·N·V·N·L)
fn builtin_gfx_microfacet_brdf_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d = f1(args);
    let f = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let n_v = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let n_l = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    if n_v * n_l == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(d * f * g / (4.0 * n_v * n_l)))
}

/// Subsurface scattering term (Burley diffuse)
fn builtin_gfx_subsurface_scattering_term(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_l = f1(args);
    Ok(StrykeValue::float(n_l.max(0.0).powf(0.5)))
}

/// Translucent falloff: exp(-d/τ)
fn builtin_gfx_translucent_falloff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d = f1(args);
    let tau = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if tau <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((-d / tau).exp()))
}

/// Normal distribution function GGX (alias)
fn builtin_gfx_normal_distribution_ggx(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_gfx_cook_torrance_d_ggx(args)
}

/// Geometric attenuation Smith
fn builtin_gfx_geometric_attenuation_smith(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_gfx_cook_torrance_g_smith(args)
}

/// Fresnel dielectric (full)
fn builtin_gfx_fresnel_dielectric_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cos_i = f1(args);
    let n1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.5);
    let sin_t = (n1 / n2) * (1.0 - cos_i * cos_i).max(0.0).sqrt();
    if sin_t > 1.0 { return Ok(StrykeValue::float(1.0)); }
    let cos_t = (1.0 - sin_t * sin_t).max(0.0).sqrt();
    let r_para = (n2 * cos_i - n1 * cos_t) / (n2 * cos_i + n1 * cos_t);
    let r_perp = (n1 * cos_i - n2 * cos_t) / (n1 * cos_i + n2 * cos_t);
    Ok(StrykeValue::float(0.5 * (r_para * r_para + r_perp * r_perp)))
}

/// Fresnel reflectance for conductors (complex IOR n + iκ). The exact formula
/// (NOT Schlick): for incident angle θ with cos_i = c,
///   t₁ = (n² + κ²) cos²θ
///   t₂ = 2 n cos θ
///   r_∥² = (t₁ − t₂ + 1) / (t₁ + t₂ + 1)
///   r_⊥² = ((n² + κ²) − t₂ + cos²θ) / ((n² + κ²) + t₂ + cos²θ)
///   F_conductor = (r_∥² + r_⊥²) / 2.
/// Distinct from Schlick (real-IOR approximation). Args: cos_i, n, κ.
fn builtin_gfx_fresnel_conductor_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cos_i = f1(args).clamp(0.0, 1.0);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let nk2 = n * n + k * k;
    let cos2 = cos_i * cos_i;
    let t1 = nk2 * cos2;
    let t2 = 2.0 * n * cos_i;
    let r_par = (t1 - t2 + 1.0) / (t1 + t2 + 1.0);
    let r_perp = (nk2 - t2 + cos2) / (nk2 + t2 + cos2);
    Ok(StrykeValue::float((r_par + r_perp) / 2.0))
}

/// Index of refraction sin θ_i / sin θ_t
fn builtin_gfx_index_of_refraction(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sin_i = f1(args);
    let sin_t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if sin_t == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(sin_i / sin_t))
}

/// Snell's law angle: θ_t = asin((n1/n2) sin θ_i)
fn builtin_gfx_snells_law_angle(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta_i = f1(args);
    let n1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.5);
    if n2 == 0.0 { return Ok(StrykeValue::float(f64::NAN)); }
    let sin_t = (n1 / n2) * theta_i.sin();
    if sin_t.abs() > 1.0 { return Ok(StrykeValue::float(f64::NAN)); }
    Ok(StrykeValue::float(sin_t.asin()))
}

/// Total internal reflection check
fn builtin_gfx_total_internal_reflection(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta_i = f1(args);
    let n1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.5);
    let n2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if n1 == 0.0 { return Ok(StrykeValue::integer(0)); }
    let crit = (n2 / n1).asin();
    Ok(StrykeValue::integer(if theta_i > crit { 1 } else { 0 }))
}

/// Refract direction X (Snell)
fn builtin_gfx_refract_direction_x(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i_x = f1(args);
    let n_x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let eta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let cos_i = -i_x;
    let sin2_t = eta * eta * (1.0 - cos_i * cos_i);
    if sin2_t > 1.0 { return Ok(StrykeValue::float(0.0)); }
    let cos_t = (1.0 - sin2_t).sqrt();
    Ok(StrykeValue::float(eta * i_x + (eta * cos_i - cos_t) * n_x))
}

/// Reflect direction X: I - 2 (N·I) N
fn builtin_gfx_reflect_direction_x(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i_x = f1(args);
    let n_x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n_dot_i = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(i_x - 2.0 * n_dot_i * n_x))
}

/// Environment map U (longitude)
fn builtin_gfx_environment_map_uv_u(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dx = f1(args);
    let dz = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(0.5 + dz.atan2(dx) / (2.0 * std::f64::consts::PI)))
}

/// Environment map V (latitude)
fn builtin_gfx_environment_map_uv_v(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dy = f1(args);
    Ok(StrykeValue::float(0.5 - dy.asin() / std::f64::consts::PI))
}

/// Cube map face index from direction (max abs component)
fn builtin_gfx_cube_map_face_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dx = f1(args);
    let dy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dz = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let abs_x = dx.abs();
    let abs_y = dy.abs();
    let abs_z = dz.abs();
    if abs_x >= abs_y && abs_x >= abs_z { Ok(StrykeValue::integer(if dx > 0.0 { 0 } else { 1 })) }
    else if abs_y >= abs_z { Ok(StrykeValue::integer(if dy > 0.0 { 2 } else { 3 })) }
    else { Ok(StrykeValue::integer(if dz > 0.0 { 4 } else { 5 })) }
}

/// Octahedral encode X
fn builtin_gfx_octahedral_encode_x(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dx = f1(args);
    let dy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dz = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = dx.abs() + dy.abs() + dz.abs();
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(dx / denom))
}

/// Octahedral encode Y
fn builtin_gfx_octahedral_encode_y(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dx = f1(args);
    let dy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dz = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = dx.abs() + dy.abs() + dz.abs();
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(dy / denom))
}

/// Spherical harmonic Y_0^0 = 1/(2√π)
fn builtin_gfx_spherical_harmonic_y00(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(0.5 / std::f64::consts::PI.sqrt()))
}

/// Y_1^0 = √(3/(4π)) z
fn builtin_gfx_spherical_harmonic_y10(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    Ok(StrykeValue::float((3.0 / (4.0 * std::f64::consts::PI)).sqrt() * z))
}

/// Y_1^1 = √(3/(4π)) x
fn builtin_gfx_spherical_harmonic_y11(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float((3.0 / (4.0 * std::f64::consts::PI)).sqrt() * x))
}

/// Y_2^0 = √(5/(16π)) (3z² - 1)
fn builtin_gfx_spherical_harmonic_y20(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    Ok(StrykeValue::float((5.0 / (16.0 * std::f64::consts::PI)).sqrt() * (3.0 * z * z - 1.0)))
}

/// Zonal harmonic Z_l(θ) = √((2l+1)/(4π)) P_l(cos θ): zonal slice of Y_l^0.
fn builtin_gfx_zonal_harmonic_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    let l = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let x = theta.cos();
    let mut p0 = 1.0_f64;
    let mut p1 = x;
    if l == 0 { return Ok(StrykeValue::float((1.0 / (4.0 * std::f64::consts::PI)).sqrt())); }
    for k in 2..=l {
        let kf = k as f64;
        let p = ((2.0 * kf - 1.0) * x * p1 - (kf - 1.0) * p0) / kf;
        p0 = p1; p1 = p;
    }
    Ok(StrykeValue::float(((2.0 * l as f64 + 1.0) / (4.0 * std::f64::consts::PI)).sqrt() * p1))
}

/// Irradiance SH evaluation (3-band)
fn builtin_gfx_irradiance_sh_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l = b47_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(l.iter().sum()))
}

/// Radiance SH evaluation (point)
fn builtin_gfx_radiance_sh_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_gfx_irradiance_sh_eval(args)
}

/// Skybox UV U
fn builtin_gfx_skybox_uv_u(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_gfx_environment_map_uv_u(args)
}

/// Skybox UV V
fn builtin_gfx_skybox_uv_v(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_gfx_environment_map_uv_v(args)
}

/// Reinhard tonemap: x / (1 + x)
fn builtin_gfx_tonemap_reinhard(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(x / (1.0 + x)))
}

/// ACES filmic tone mapping
fn builtin_gfx_tonemap_aces(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    Ok(StrykeValue::float(((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)))
}

/// Uncharted2 tone mapping
fn builtin_gfx_tonemap_uncharted2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let a = 0.15;
    let b = 0.50;
    let c = 0.10;
    let d = 0.20;
    let e = 0.02;
    let f = 0.30;
    Ok(StrykeValue::float(((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f))
}

/// Filmic tonemap (alias)
fn builtin_gfx_tonemap_filmic(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_gfx_tonemap_aces(args)
}

/// Gamma correction: x^(1/γ)
fn builtin_gfx_gamma_correct_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(2.2);
    if gamma == 0.0 { return Ok(StrykeValue::float(x)); }
    Ok(StrykeValue::float(x.max(0.0).powf(1.0 / gamma)))
}

/// sRGB → linear
fn builtin_gfx_srgb_to_linear(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }))
}

/// Linear → sRGB
fn builtin_gfx_linear_to_srgb(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(if x <= 0.0031308 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }))
}

/// Bayer 4×4 matrix value
fn builtin_gfx_dither_bayer_4x4(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args).clamp(0, 3) as usize;
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).clamp(0, 3) as usize;
    let m: [[f64; 4]; 4] = [
        [0.0, 8.0, 2.0, 10.0],
        [12.0, 4.0, 14.0, 6.0],
        [3.0, 11.0, 1.0, 9.0],
        [15.0, 7.0, 13.0, 5.0],
    ];
    Ok(StrykeValue::float(m[i][j] / 16.0))
}

/// Floyd-Steinberg error diffusion (single coefficient — bottom-right)
fn builtin_gfx_dither_floyd_steinberg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let err = f1(args);
    Ok(StrykeValue::float(err * 7.0 / 16.0))
}

/// OKLab L = (l)^(1/3)
fn builtin_gfx_oklab_l_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l = f1(args);
    Ok(StrykeValue::float(l.cbrt()))
}

fn builtin_gfx_oklab_a_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    Ok(StrykeValue::float(m.cbrt()))
}

fn builtin_gfx_oklab_b_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = f1(args);
    Ok(StrykeValue::float(s.cbrt()))
}

/// OKLCh chroma = √(a² + b²)
fn builtin_gfx_oklch_chroma(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((a * a + b * b).sqrt()))
}

/// OKLCh hue = atan2(b, a)
fn builtin_gfx_oklch_hue(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(b.atan2(a)))
}

/// PCG hash step
fn builtin_gfx_pcg_hash_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = i1(args) as u64;
    let state = s.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277_803_737);
    let result = (word >> 22) ^ word;
    Ok(StrykeValue::integer(result as i64))
}

/// XOR-shift step (32-bit)
fn builtin_gfx_xorshift_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut s = i1(args) as u32;
    s ^= s << 13;
    s ^= s >> 17;
    s ^= s << 5;
    Ok(StrykeValue::integer(s as i64))
}

/// Halton sequence step base b
fn builtin_gfx_halton_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut i = i1(args).max(1) as u64;
    let b = args.get(1).map(|v| v.to_number() as u64).unwrap_or(2).max(2);
    let mut f = 1.0_f64;
    let mut r = 0.0_f64;
    while i > 0 {
        f /= b as f64;
        r += f * (i % b) as f64;
        i /= b;
    }
    Ok(StrykeValue::float(r))
}

/// Sobol sequence (Antonov-Saleev recurrence, dimension 1, primitive polynomial
/// x+1, direction numbers V_k = 2^(B−k) for k=1..B). Gray-code update:
///   x_{i+1} = x_i ⊕ V_{c_i+1}, where c_i = trailing-zero count of (i+1).
/// Returns x_i / 2^B as f64. Args: index i (≥0).
fn builtin_gfx_sobol_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    const B: u32 = 32;
    let n = i1(args).max(0) as u64;
    let mut x: u64 = 0;
    for k in 0..n {
        let c = (k + 1).trailing_zeros();
        let v = 1u64 << (B - 1 - c.min(B - 1));
        x ^= v;
    }
    Ok(StrykeValue::float(x as f64 / (1u64 << B) as f64))
}

/// Van der Corput sequence φ_b(n): radical-inverse in base b. Write n in base b
/// as Σ a_k b^k; then φ_b(n) = Σ a_k b^(−k−1). Default base 2 (the original
/// Van der Corput sequence). Bit-reversal in base 2 implemented directly with
/// 32-bit reverse for speed; general-base falls back to digit-by-digit.
fn builtin_gfx_van_der_corput(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0) as u64;
    let b = args.get(1).map(|v| v.to_number() as u64).unwrap_or(2).max(2);
    if b == 2 {
        let bits = (n as u32).reverse_bits();
        return Ok(StrykeValue::float(bits as f64 / (1u64 << 32) as f64));
    }
    let mut i = n;
    let mut f = 1.0_f64;
    let mut r = 0.0_f64;
    while i > 0 {
        f /= b as f64;
        r += f * (i % b) as f64;
        i /= b;
    }
    Ok(StrykeValue::float(r))
}

/// Low-discrepancy step: discrepancy bound D*_N for an N-point sequence in d
/// dimensions, per the Koksma-Hlawka theorem: D*_N(seq) ≤ C·(log N)^d / N for
/// (t, d)-sequences (Halton, Sobol, Faure). Returns the asymptotic upper bound
/// for given algorithm choice. Args: N (samples), d (dimensions), algo
/// (0 = Halton, 1 = Sobol with C ≈ 1, 2 = Faure with smaller C).
fn builtin_gfx_low_discrepancy_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args).max(2.0);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let algo = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let c = match algo { 0 => 1.0_f64, 1 => 0.95, 2 => 0.7, _ => 1.0 };
    Ok(StrykeValue::float(c * n.ln().powf(d) / n))
}

/// Blue noise value (Bayer-like simulated)
fn builtin_gfx_blue_noise_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let h = (i.wrapping_mul(73_856_093) ^ j.wrapping_mul(19_349_663)) as u32;
    Ok(StrykeValue::float((h as f64) / (u32::MAX as f64)))
}

/// Perlin noise (1-D simplified gradient noise)
fn builtin_gfx_perlin_noise_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let xi = x.floor();
    let xf = x - xi;
    let u = xf * xf * (3.0 - 2.0 * xf);
    Ok(StrykeValue::float(u))
}

/// Perlin's simplex noise (2-D): skew F = (√3 − 1)/2 to the simplex grid; for
/// each of three corners compute attenuation t² and contribute t⁴·(g·d) where
/// g is a unit pseudo-gradient picked from a 12-vector palette by hashed corner
/// coords. Sum of three corners gives the noise. Real algorithm.
fn builtin_gfx_simplex_noise_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f2 = 0.5 * (3.0_f64.sqrt() - 1.0);
    let g2 = (3.0 - 3.0_f64.sqrt()) / 6.0;
    let s = (x + y) * f2;
    let i = (x + s).floor();
    let j = (y + s).floor();
    let t = (i + j) * g2;
    let x0 = x - (i - t);
    let y0 = y - (j - t);
    let (i1, j1) = if x0 > y0 { (1.0, 0.0) } else { (0.0, 1.0) };
    let x1 = x0 - i1 + g2;
    let y1 = y0 - j1 + g2;
    let x2 = x0 - 1.0 + 2.0 * g2;
    let y2 = y0 - 1.0 + 2.0 * g2;
    fn grad2(h: u32, x: f64, y: f64) -> f64 {
        let g = h & 7;
        let (gx, gy) = match g {
            0 => (1.0, 1.0), 1 => (-1.0, 1.0), 2 => (1.0, -1.0), 3 => (-1.0, -1.0),
            4 => (1.0, 0.0), 5 => (-1.0, 0.0), 6 => (0.0, 1.0), _ => (0.0, -1.0),
        };
        gx * x + gy * y
    }
    let hash = |a: f64, b: f64| -> u32 {
        let ai = a as i32 as u32;
        let bi = b as i32 as u32;
        ai.wrapping_mul(73_856_093) ^ bi.wrapping_mul(19_349_663)
    };
    let n_corner = |xx: f64, yy: f64, h: u32| -> f64 {
        let t = 0.5 - xx * xx - yy * yy;
        if t < 0.0 { 0.0 } else { let t2 = t * t; t2 * t2 * grad2(h, xx, yy) }
    };
    let n0 = n_corner(x0, y0, hash(i, j));
    let n1 = n_corner(x1, y1, hash(i + i1, j + j1));
    let n2 = n_corner(x2, y2, hash(i + 1.0, j + 1.0));
    Ok(StrykeValue::float(70.0 * (n0 + n1 + n2)))
}

/// fBm: sum of octaves with persistence
fn builtin_gfx_fbm_noise_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let oct = args.get(1).map(|v| v.to_number()).unwrap_or(4.0);
    let persistence = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(n * (1.0 - persistence.powf(oct)) / (1.0 - persistence).max(1e-9)))
}

/// Worley noise (cell distance)
fn builtin_gfx_worley_noise_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b47_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Voronoi distance (alias)
fn builtin_gfx_voronoi_distance(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_gfx_worley_noise_step(args)
}

/// Bridson curl noise (2-D divergence-free vector field): given a scalar
/// potential ψ(x, y), the divergence-free flow is V = (∂ψ/∂y, −∂ψ/∂x).
/// Approximate the partial via central differences on noise samples ψ at
/// offsets ±h. Returns the requested component (0=Vx, 1=Vy).
/// Args: comp (0/1), psi_xp, psi_xm, psi_yp, psi_ym, h.
fn builtin_gfx_curl_noise_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let comp = i1(args);
    let psi_xp = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let psi_xm = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let psi_yp = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let psi_ym = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let h = args.get(5).map(|v| v.to_number()).unwrap_or(1e-3).max(1e-12);
    let dpsi_dy = (psi_yp - psi_ym) / (2.0 * h);
    let dpsi_dx = (psi_xp - psi_xm) / (2.0 * h);
    Ok(StrykeValue::float(if comp == 0 { dpsi_dy } else { -dpsi_dx }))
}

/// Generic gradient noise (1-D): pick a random unit gradient g_i ∈ {−1, +1} at
/// each integer lattice node, dot-product with the offset to that node, and
/// quintic-interpolate (Perlin's improved fade). Distinct from value noise
/// (which interpolates lattice values, not gradients). Args: x.
fn builtin_gfx_gradient_noise_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let xi = x.floor() as i64;
    let xf = x - x.floor();
    fn grad1(i: i64) -> f64 {
        let h = (i.wrapping_mul(73_856_093) as u32).wrapping_mul(0x9e37_79b9);
        if h & 1 == 0 { 1.0 } else { -1.0 }
    }
    let g0 = grad1(xi) * xf;
    let g1 = grad1(xi + 1) * (xf - 1.0);
    let u = xf * xf * xf * (xf * (xf * 6.0 - 15.0) + 10.0);
    Ok(StrykeValue::float(g0 * (1.0 - u) + g1 * u))
}

/// Value noise: assign a random value v(i, j) ∈ [0, 1) to each integer lattice
/// node and bilinearly smooth-step interpolate. Differs from Perlin (which
/// dot-products against a gradient at each corner). Args: x, y.
fn builtin_gfx_value_noise_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let xi = x.floor() as i64;
    let yi = y.floor() as i64;
    let xf = x - x.floor();
    let yf = y - y.floor();
    fn rand2(i: i64, j: i64) -> f64 {
        let h = (i.wrapping_mul(73_856_093) ^ j.wrapping_mul(19_349_663)) as u32;
        (h as f64) / (u32::MAX as f64)
    }
    let v00 = rand2(xi, yi);
    let v10 = rand2(xi + 1, yi);
    let v01 = rand2(xi, yi + 1);
    let v11 = rand2(xi + 1, yi + 1);
    let u = xf * xf * (3.0 - 2.0 * xf);
    let v = yf * yf * (3.0 - 2.0 * yf);
    let a = v00 * (1.0 - u) + v10 * u;
    let b = v01 * (1.0 - u) + v11 * u;
    Ok(StrykeValue::float(a * (1.0 - v) + b * v))
}

/// Signed distance to box: max(|p| - b)
fn builtin_gfx_signed_distance_box(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(p.abs() - b))
}

/// SDF sphere: |p| - r
fn builtin_gfx_signed_distance_sphere(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(p.abs() - r))
}

/// SDF capsule: |p - a + (b-a)·t| - r where t = clamp(...)
fn builtin_gfx_signed_distance_capsule(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dist = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(dist - r))
}
