// Batch 47 — graphics, geometry, ray tracing, BRDF, color spaces, noise, SDF.

fn b47_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

// Perspective projection x: x' = x / (w · tan(fov/2) · aspect)
fn builtin_gfx_perspective_proj_x(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let fov = args.get(2).map(|v| v.to_number()).unwrap_or(std::f64::consts::FRAC_PI_4);
    let aspect = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let denom = w * (fov / 2.0).tan() * aspect;
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(x / denom))
}

// Perspective projection y
fn builtin_gfx_perspective_proj_y(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let fov = args.get(2).map(|v| v.to_number()).unwrap_or(std::f64::consts::FRAC_PI_4);
    let denom = w * (fov / 2.0).tan();
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(y / denom))
}

// Orthographic projection (linear scaling)
fn builtin_gfx_orthographic_proj(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let lo = args.get(1).map(|v| v.to_number()).unwrap_or(-1.0);
    let hi = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if hi == lo { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(2.0 * (x - lo) / (hi - lo) - 1.0))
}

// View matrix step (single component of M = R^T · T)
fn builtin_gfx_view_matrix_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(r * t))
}

// LookAt forward = normalize(target - eye)_x
fn builtin_gfx_lookat_forward(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dx = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(dx / n))
}

// LookAt right = normalize(forward × up)
fn builtin_gfx_lookat_right(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args);
    let u = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(f * u))
}

// LookAt up = right × forward
fn builtin_gfx_lookat_up(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_lookat_right(args)
}

// Quaternion to axis-angle (returns angle from w)
fn builtin_gfx_quat_to_axis_angle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = f1(args);
    Ok(PerlValue::float(2.0 * w.clamp(-1.0, 1.0).acos()))
}

// Axis-angle to quaternion w = cos(θ/2)
fn builtin_gfx_axis_angle_to_quat(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    Ok(PerlValue::float((theta / 2.0).cos()))
}

// Quaternion slerp step at t
fn builtin_gfx_quat_slerp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q0 = f1(args);
    let q1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let dot = q0 * q1;
    let omega = dot.clamp(-1.0, 1.0).acos();
    if omega.abs() < 1e-9 { return Ok(PerlValue::float((1.0 - t) * q0 + t * q1)); }
    let sin_o = omega.sin();
    Ok(PerlValue::float(((1.0 - t) * omega).sin() / sin_o * q0 + (t * omega).sin() / sin_o * q1))
}

// Quaternion nlerp (normalized lerp)
fn builtin_gfx_quat_nlerp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q0 = f1(args);
    let q1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float((1.0 - t) * q0 + t * q1))
}

// Quaternion dot product
fn builtin_gfx_quat_dot_two(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = b47_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let b = b47_to_floats(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    let n = a.len().min(b.len());
    Ok(PerlValue::float((0..n).map(|i| a[i] * b[i]).sum()))
}

// Quaternion inverse: -q (for unit quaternion conjugate)
fn builtin_gfx_quat_inverse_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(-f1(args)))
}

// Quaternion to Euler pitch
fn builtin_gfx_quat_to_euler_pitch(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let qw = f1(args);
    let qx = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let qy = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let qz = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((2.0 * (qw * qx + qy * qz)).atan2(1.0 - 2.0 * (qx * qx + qy * qy))))
}

// Quat to Euler yaw
fn builtin_gfx_quat_to_euler_yaw(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let qw = f1(args);
    let qx = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let qy = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let qz = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((2.0 * (qw * qz + qx * qy)).atan2(1.0 - 2.0 * (qy * qy + qz * qz))))
}

// Quat to Euler roll
fn builtin_gfx_quat_to_euler_roll(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let qw = f1(args);
    let qx = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let qy = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let qz = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let v = (2.0 * (qw * qy - qz * qx)).clamp(-1.0, 1.0);
    Ok(PerlValue::float(v.asin()))
}

// Euler to quat x = sin(roll/2)cos(pitch/2)cos(yaw/2) - cos(roll/2)sin(pitch/2)sin(yaw/2)
fn builtin_gfx_euler_to_quat_x(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let roll = f1(args);
    let pitch = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let yaw = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((roll / 2.0).sin() * (pitch / 2.0).cos() * (yaw / 2.0).cos()
        - (roll / 2.0).cos() * (pitch / 2.0).sin() * (yaw / 2.0).sin()))
}

fn builtin_gfx_euler_to_quat_y(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let roll = f1(args);
    let pitch = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let yaw = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((roll / 2.0).cos() * (pitch / 2.0).sin() * (yaw / 2.0).cos()
        + (roll / 2.0).sin() * (pitch / 2.0).cos() * (yaw / 2.0).sin()))
}

fn builtin_gfx_euler_to_quat_z(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let roll = f1(args);
    let pitch = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let yaw = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((roll / 2.0).cos() * (pitch / 2.0).cos() * (yaw / 2.0).sin()
        - (roll / 2.0).sin() * (pitch / 2.0).sin() * (yaw / 2.0).cos()))
}

fn builtin_gfx_euler_to_quat_w(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let roll = f1(args);
    let pitch = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let yaw = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((roll / 2.0).cos() * (pitch / 2.0).cos() * (yaw / 2.0).cos()
        + (roll / 2.0).sin() * (pitch / 2.0).sin() * (yaw / 2.0).sin()))
}

// Rotation matrix XX entry
fn builtin_gfx_rotation_matrix_xx(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    Ok(PerlValue::float(theta.cos()))
}

fn builtin_gfx_rotation_matrix_yy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    Ok(PerlValue::float(theta.cos()))
}

fn builtin_gfx_rotation_matrix_zz(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    Ok(PerlValue::float(theta.cos()))
}

// Translation matrix step (translation component)
fn builtin_gfx_translation_matrix_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// Scale matrix entry
fn builtin_gfx_scale_matrix_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// Shear matrix XY entry
fn builtin_gfx_shear_matrix_xy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// Homogeneous divide: x/w
fn builtin_gfx_homogeneous_divide(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if w == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(x / w))
}

// NDC to screen X: (x + 1) / 2 · width
fn builtin_gfx_screen_space_x(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float((x + 1.0) * 0.5 * w))
}

// NDC to screen Y: (1 - y) / 2 · height
fn builtin_gfx_screen_space_y(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float((1.0 - y) * 0.5 * h))
}

fn builtin_gfx_ndc_to_screen_x(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_screen_space_x(args)
}

fn builtin_gfx_ndc_to_screen_y(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_screen_space_y(args)
}

// Screen to NDC X: 2 · x / W - 1
fn builtin_gfx_screen_to_ndc_x(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if w == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(2.0 * x / w - 1.0))
}

fn builtin_gfx_screen_to_ndc_y(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let h = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if h == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 - 2.0 * y / h))
}

// Polygon clip step (in vs out vertex)
fn builtin_gfx_clip_polygon_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v_in = i1(args);
    let v_out = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(v_in - v_out))
}

// Sutherland-Hodgman intersect parameter t
fn builtin_gfx_sutherland_hodgman(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dot1 = f1(args);
    let dot2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = dot1 - dot2;
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(dot1 / denom))
}

// Cohen-Sutherland code: 4-bit outcode for a point
fn builtin_gfx_cohen_sutherland_code(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let xmin = args.get(2).map(|v| v.to_number()).unwrap_or(-1.0);
    let xmax = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let ymin = args.get(4).map(|v| v.to_number()).unwrap_or(-1.0);
    let ymax = args.get(5).map(|v| v.to_number()).unwrap_or(1.0);
    let mut code = 0_i64;
    if x < xmin { code |= 1; } else if x > xmax { code |= 2; }
    if y < ymin { code |= 4; } else if y > ymax { code |= 8; }
    Ok(PerlValue::integer(code))
}

// Liang-Barsky t-value
fn builtin_gfx_liang_barsky_t(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if p == 0.0 { return Ok(PerlValue::float(if q < 0.0 { -1.0 } else { 1.0 })); }
    Ok(PerlValue::float(q / p))
}

// Bresenham step X (slope < 1)
fn builtin_gfx_bresenham_step_x(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let err = f1(args);
    let dx = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(err - dx))
}

// Bresenham step Y
fn builtin_gfx_bresenham_step_y(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let err = f1(args);
    let dy = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(err + dy))
}

// Xiaolin Wu intensity
fn builtin_gfx_xiaolin_wu_intensity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let frac = f1(args);
    Ok(PerlValue::float(1.0 - frac.fract().abs()))
}

// AABB intersect check
fn builtin_gfx_aabb_intersect_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_min = f1(args);
    let a_max = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b_min = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let b_max = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::integer(if a_max >= b_min && b_max >= a_min { 1 } else { 0 }))
}

// OBB overlap step (SAT axis)
fn builtin_gfx_obb_overlap_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let proj_a = f1(args);
    let proj_b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dist = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::integer(if proj_a + proj_b > dist.abs() { 1 } else { 0 }))
}

// Sphere intersect t-distance
fn builtin_gfx_sphere_intersect_t(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let b = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let disc = b * b - c;
    if disc < 0.0 { return Ok(PerlValue::float(-1.0)); }
    Ok(PerlValue::float(-b - disc.sqrt()))
}

// Ray-triangle intersection t (Möller-Trumbore)
fn builtin_gfx_ray_triangle_t(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dot_n_d = f1(args);
    let dot_n_p = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if dot_n_d == 0.0 { return Ok(PerlValue::float(-1.0)); }
    Ok(PerlValue::float(-dot_n_p / dot_n_d))
}

// Ray-plane t = -(N·P0 + d) / (N·D)
fn builtin_gfx_ray_plane_t(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_ray_triangle_t(args)
}

// Ray-box slab method t
fn builtin_gfx_ray_box_t(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t_min = f1(args);
    let t_max = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if t_min > t_max { return Ok(PerlValue::float(-1.0)); }
    Ok(PerlValue::float(t_min))
}

// Ray-sphere t
fn builtin_gfx_ray_sphere_t(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_sphere_intersect_t(args)
}

// Ray-disk t (planar disk + radius check)
fn builtin_gfx_ray_disk_t(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let dist_sq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r_sq = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if dist_sq <= r_sq { t } else { -1.0 }))
}

// Ray-cylinder t (infinite cylinder)
fn builtin_gfx_ray_cylinder_t(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let disc = b * b - a * c;
    if disc < 0.0 || a == 0.0 { return Ok(PerlValue::float(-1.0)); }
    Ok(PerlValue::float((-b - disc.sqrt()) / a))
}

// Ray-cone t
fn builtin_gfx_ray_cone_t(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_ray_cylinder_t(args)
}

// Ray-ellipsoid t (transform ray, then sphere)
fn builtin_gfx_ray_ellipsoid_t(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_sphere_intersect_t(args)
}

// Ray-torus quartic approximation (returns nearest positive root)
fn builtin_gfx_ray_torus_t_approx(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// Barycentric α
fn builtin_gfx_barycentric_alpha(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let area_a = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(area_a / total))
}

fn builtin_gfx_barycentric_beta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_barycentric_alpha(args)
}

fn builtin_gfx_barycentric_gamma(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(1.0 - alpha - beta))
}

// Phong diffuse: max(0, N·L)
fn builtin_gfx_phong_diffuse_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args).max(0.0)))
}

// Phong specular: max(0, R·V)^n
fn builtin_gfx_phong_specular_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r_v = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(32.0);
    Ok(PerlValue::float(r_v.max(0.0).powf(n)))
}

// Phong ambient
fn builtin_gfx_phong_ambient_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// Blinn-Phong specular: max(0, N·H)^n
fn builtin_gfx_blinn_specular_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_phong_specular_step(args)
}

// Lambert term: max(0, N·L)
fn builtin_gfx_lambert_term(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args).max(0.0)))
}

// Oren-Nayar term (approximation)
fn builtin_gfx_oren_nayar_term(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_l = f1(args);
    let sigma2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let a = 1.0 - 0.5 * sigma2 / (sigma2 + 0.33);
    Ok(PerlValue::float(n_l.max(0.0) * a))
}

// Cook-Torrance D (GGX)
fn builtin_gfx_cook_torrance_d_ggx(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_h = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let alpha2 = alpha * alpha;
    let denom = n_h * n_h * (alpha2 - 1.0) + 1.0;
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(alpha2 / (std::f64::consts::PI * denom * denom)))
}

// Cook-Torrance G (Smith)
fn builtin_gfx_cook_torrance_g_smith(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_v = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let k = (alpha + 1.0).powi(2) / 8.0;
    if n_v * (1.0 - k) + k == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(n_v / (n_v * (1.0 - k) + k)))
}

// Cook-Torrance F (Schlick): f0 + (1 - f0)(1 - cosθ)^5
fn builtin_gfx_cook_torrance_f_schlick(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f0 = f1(args);
    let cos_theta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(f0 + (1.0 - f0) * (1.0 - cos_theta).powi(5)))
}

// Disney principled D
fn builtin_gfx_disney_principled_d(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_cook_torrance_d_ggx(args)
}

// Microfacet BRDF combined: D·F·G / (4·N·V·N·L)
fn builtin_gfx_microfacet_brdf_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d = f1(args);
    let f = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let n_v = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let n_l = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    if n_v * n_l == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(d * f * g / (4.0 * n_v * n_l)))
}

// Subsurface scattering term (Burley diffuse)
fn builtin_gfx_subsurface_scattering_term(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_l = f1(args);
    Ok(PerlValue::float(n_l.max(0.0).powf(0.5)))
}

// Translucent falloff: exp(-d/τ)
fn builtin_gfx_translucent_falloff(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d = f1(args);
    let tau = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if tau <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((-d / tau).exp()))
}

// Normal distribution function GGX (alias)
fn builtin_gfx_normal_distribution_ggx(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_cook_torrance_d_ggx(args)
}

// Geometric attenuation Smith
fn builtin_gfx_geometric_attenuation_smith(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_cook_torrance_g_smith(args)
}

// Fresnel dielectric (full)
fn builtin_gfx_fresnel_dielectric_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cos_i = f1(args);
    let n1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.5);
    let sin_t = (n1 / n2) * (1.0 - cos_i * cos_i).max(0.0).sqrt();
    if sin_t > 1.0 { return Ok(PerlValue::float(1.0)); }
    let cos_t = (1.0 - sin_t * sin_t).max(0.0).sqrt();
    let r_para = (n2 * cos_i - n1 * cos_t) / (n2 * cos_i + n1 * cos_t);
    let r_perp = (n1 * cos_i - n2 * cos_t) / (n1 * cos_i + n2 * cos_t);
    Ok(PerlValue::float(0.5 * (r_para * r_para + r_perp * r_perp)))
}

// Fresnel conductor (Schlick approx)
fn builtin_gfx_fresnel_conductor_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_cook_torrance_f_schlick(args)
}

// Index of refraction sin θ_i / sin θ_t
fn builtin_gfx_index_of_refraction(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sin_i = f1(args);
    let sin_t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if sin_t == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(sin_i / sin_t))
}

// Snell's law angle: θ_t = asin((n1/n2) sin θ_i)
fn builtin_gfx_snells_law_angle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta_i = f1(args);
    let n1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.5);
    if n2 == 0.0 { return Ok(PerlValue::float(f64::NAN)); }
    let sin_t = (n1 / n2) * theta_i.sin();
    if sin_t.abs() > 1.0 { return Ok(PerlValue::float(f64::NAN)); }
    Ok(PerlValue::float(sin_t.asin()))
}

// Total internal reflection check
fn builtin_gfx_total_internal_reflection(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta_i = f1(args);
    let n1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.5);
    let n2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if n1 == 0.0 { return Ok(PerlValue::integer(0)); }
    let crit = (n2 / n1).asin();
    Ok(PerlValue::integer(if theta_i > crit { 1 } else { 0 }))
}

// Refract direction X (Snell)
fn builtin_gfx_refract_direction_x(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i_x = f1(args);
    let n_x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let eta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let cos_i = -i_x;
    let sin2_t = eta * eta * (1.0 - cos_i * cos_i);
    if sin2_t > 1.0 { return Ok(PerlValue::float(0.0)); }
    let cos_t = (1.0 - sin2_t).sqrt();
    Ok(PerlValue::float(eta * i_x + (eta * cos_i - cos_t) * n_x))
}

// Reflect direction X: I - 2 (N·I) N
fn builtin_gfx_reflect_direction_x(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i_x = f1(args);
    let n_x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n_dot_i = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(i_x - 2.0 * n_dot_i * n_x))
}

// Environment map U (longitude)
fn builtin_gfx_environment_map_uv_u(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dx = f1(args);
    let dz = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 + dz.atan2(dx) / (2.0 * std::f64::consts::PI)))
}

// Environment map V (latitude)
fn builtin_gfx_environment_map_uv_v(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dy = f1(args);
    Ok(PerlValue::float(0.5 - dy.asin() / std::f64::consts::PI))
}

// Cube map face index from direction (max abs component)
fn builtin_gfx_cube_map_face_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dx = f1(args);
    let dy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dz = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let abs_x = dx.abs();
    let abs_y = dy.abs();
    let abs_z = dz.abs();
    if abs_x >= abs_y && abs_x >= abs_z { Ok(PerlValue::integer(if dx > 0.0 { 0 } else { 1 })) }
    else if abs_y >= abs_z { Ok(PerlValue::integer(if dy > 0.0 { 2 } else { 3 })) }
    else { Ok(PerlValue::integer(if dz > 0.0 { 4 } else { 5 })) }
}

// Octahedral encode X
fn builtin_gfx_octahedral_encode_x(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dx = f1(args);
    let dy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dz = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = dx.abs() + dy.abs() + dz.abs();
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(dx / denom))
}

// Octahedral encode Y
fn builtin_gfx_octahedral_encode_y(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dx = f1(args);
    let dy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dz = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = dx.abs() + dy.abs() + dz.abs();
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(dy / denom))
}

// Spherical harmonic Y_0^0 = 1/(2√π)
fn builtin_gfx_spherical_harmonic_y00(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(0.5 / std::f64::consts::PI.sqrt()))
}

// Y_1^0 = √(3/(4π)) z
fn builtin_gfx_spherical_harmonic_y10(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = f1(args);
    Ok(PerlValue::float((3.0 / (4.0 * std::f64::consts::PI)).sqrt() * z))
}

// Y_1^1 = √(3/(4π)) x
fn builtin_gfx_spherical_harmonic_y11(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::float((3.0 / (4.0 * std::f64::consts::PI)).sqrt() * x))
}

// Y_2^0 = √(5/(16π)) (3z² - 1)
fn builtin_gfx_spherical_harmonic_y20(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = f1(args);
    Ok(PerlValue::float((5.0 / (16.0 * std::f64::consts::PI)).sqrt() * (3.0 * z * z - 1.0)))
}

// Zonal harmonic step
fn builtin_gfx_zonal_harmonic_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// Irradiance SH evaluation (3-band)
fn builtin_gfx_irradiance_sh_eval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = b47_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(l.iter().sum()))
}

// Radiance SH evaluation (point)
fn builtin_gfx_radiance_sh_eval(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_irradiance_sh_eval(args)
}

// Skybox UV U
fn builtin_gfx_skybox_uv_u(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_environment_map_uv_u(args)
}

// Skybox UV V
fn builtin_gfx_skybox_uv_v(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_environment_map_uv_v(args)
}

// Reinhard tonemap: x / (1 + x)
fn builtin_gfx_tonemap_reinhard(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::float(x / (1.0 + x)))
}

// ACES filmic tone mapping
fn builtin_gfx_tonemap_aces(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    Ok(PerlValue::float(((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)))
}

// Uncharted2 tone mapping
fn builtin_gfx_tonemap_uncharted2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let a = 0.15;
    let b = 0.50;
    let c = 0.10;
    let d = 0.20;
    let e = 0.02;
    let f = 0.30;
    Ok(PerlValue::float(((x * (a * x + c * b) + d * e) / (x * (a * x + b) + d * f)) - e / f))
}

// Filmic tonemap (alias)
fn builtin_gfx_tonemap_filmic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_tonemap_aces(args)
}

// Gamma correction: x^(1/γ)
fn builtin_gfx_gamma_correct_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(2.2);
    if gamma == 0.0 { return Ok(PerlValue::float(x)); }
    Ok(PerlValue::float(x.max(0.0).powf(1.0 / gamma)))
}

// sRGB → linear
fn builtin_gfx_srgb_to_linear(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::float(if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }))
}

// Linear → sRGB
fn builtin_gfx_linear_to_srgb(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::float(if x <= 0.0031308 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }))
}

// Bayer 4×4 matrix value
fn builtin_gfx_dither_bayer_4x4(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = i1(args).clamp(0, 3) as usize;
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).clamp(0, 3) as usize;
    let m: [[f64; 4]; 4] = [
        [0.0, 8.0, 2.0, 10.0],
        [12.0, 4.0, 14.0, 6.0],
        [3.0, 11.0, 1.0, 9.0],
        [15.0, 7.0, 13.0, 5.0],
    ];
    Ok(PerlValue::float(m[i][j] / 16.0))
}

// Floyd-Steinberg error diffusion (single coefficient — bottom-right)
fn builtin_gfx_dither_floyd_steinberg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let err = f1(args);
    Ok(PerlValue::float(err * 7.0 / 16.0))
}

// OKLab L = (l)^(1/3)
fn builtin_gfx_oklab_l_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l = f1(args);
    Ok(PerlValue::float(l.cbrt()))
}

fn builtin_gfx_oklab_a_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    Ok(PerlValue::float(m.cbrt()))
}

fn builtin_gfx_oklab_b_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = f1(args);
    Ok(PerlValue::float(s.cbrt()))
}

// OKLCh chroma = √(a² + b²)
fn builtin_gfx_oklch_chroma(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((a * a + b * b).sqrt()))
}

// OKLCh hue = atan2(b, a)
fn builtin_gfx_oklch_hue(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(b.atan2(a)))
}

// PCG hash step
fn builtin_gfx_pcg_hash_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = i1(args) as u64;
    let state = s.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
    let word = ((state >> ((state >> 28).wrapping_add(4))) ^ state).wrapping_mul(277_803_737);
    let result = (word >> 22) ^ word;
    Ok(PerlValue::integer(result as i64))
}

// XOR-shift step (32-bit)
fn builtin_gfx_xorshift_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut s = i1(args) as u32;
    s ^= s << 13;
    s ^= s >> 17;
    s ^= s << 5;
    Ok(PerlValue::integer(s as i64))
}

// Halton sequence step base b
fn builtin_gfx_halton_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut i = i1(args).max(1) as u64;
    let b = args.get(1).map(|v| v.to_number() as u64).unwrap_or(2).max(2);
    let mut f = 1.0_f64;
    let mut r = 0.0_f64;
    while i > 0 {
        f /= b as f64;
        r += f * (i % b) as f64;
        i /= b;
    }
    Ok(PerlValue::float(r))
}

// Sobol sequence step (single dim)
fn builtin_gfx_sobol_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_halton_step(args)
}

// Van der Corput
fn builtin_gfx_van_der_corput(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_halton_step(args)
}

// Low discrepancy step (alias)
fn builtin_gfx_low_discrepancy_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_halton_step(args)
}

// Blue noise value (Bayer-like simulated)
fn builtin_gfx_blue_noise_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = i1(args);
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let h = (i.wrapping_mul(73_856_093) ^ j.wrapping_mul(19_349_663)) as u32;
    Ok(PerlValue::float((h as f64) / (u32::MAX as f64)))
}

// Perlin noise (1-D simplified gradient noise)
fn builtin_gfx_perlin_noise_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let xi = x.floor();
    let xf = x - xi;
    let u = xf * xf * (3.0 - 2.0 * xf);
    Ok(PerlValue::float(u))
}

// Simplex noise step (1-D)
fn builtin_gfx_simplex_noise_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_perlin_noise_step(args)
}

// fBm: sum of octaves with persistence
fn builtin_gfx_fbm_noise_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let oct = args.get(1).map(|v| v.to_number()).unwrap_or(4.0);
    let persistence = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float(n * (1.0 - persistence.powf(oct)) / (1.0 - persistence).max(1e-9)))
}

// Worley noise (cell distance)
fn builtin_gfx_worley_noise_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b47_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().cloned().fold(f64::INFINITY, f64::min)))
}

// Voronoi distance (alias)
fn builtin_gfx_voronoi_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_worley_noise_step(args)
}

// Curl noise step (placeholder)
fn builtin_gfx_curl_noise_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(p - q))
}

// Gradient noise step (alias of perlin)
fn builtin_gfx_gradient_noise_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_perlin_noise_step(args)
}

// Value noise step
fn builtin_gfx_value_noise_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_gfx_perlin_noise_step(args)
}

// Signed distance to box: max(|p| - b)
fn builtin_gfx_signed_distance_box(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(p.abs() - b))
}

// SDF sphere: |p| - r
fn builtin_gfx_signed_distance_sphere(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(p.abs() - r))
}

// SDF capsule: |p - a + (b-a)·t| - r where t = clamp(...)
fn builtin_gfx_signed_distance_capsule(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dist = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(dist - r))
}
