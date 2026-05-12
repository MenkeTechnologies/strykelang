// Batch 14 — geographic projections, geohash/MGRS/Plus, image filter kernels.

fn builtin_mollweide_project(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lat = args.first().map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(6378137.0);
    let mut theta = lat;
    for _ in 0..50 {
        let dt = (2.0 * theta + (2.0 * theta).sin() - std::f64::consts::PI * lat.sin())
            / (2.0 + 2.0 * (2.0 * theta).cos());
        theta -= dt;
        if dt.abs() < 1e-12 { break; }
    }
    let x = r * 2.0 * std::f64::consts::SQRT_2 / std::f64::consts::PI * lon * theta.cos();
    let y = r * std::f64::consts::SQRT_2 * theta.sin();
    Ok(StrykeValue::array(vec![StrykeValue::float(x), StrykeValue::float(y)]))
}
fn builtin_robinson_project(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lat_deg = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let lon = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(6378137.0);
    let a = (lat_deg.abs() / 90.0).clamp(0.0, 1.0);
    let pdfe = 1.0 - 0.18 * a * a;
    let pl = lat_deg.abs() / 90.0 * 1.3523;
    let x = r * 0.8487 * pdfe * lon;
    let y = r * 1.3523 * pl * lat_deg.signum();
    Ok(StrykeValue::array(vec![StrykeValue::float(x), StrykeValue::float(y)]))
}
fn builtin_sinusoidal_project(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lat = args.first().map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(6378137.0);
    Ok(StrykeValue::array(vec![StrykeValue::float(r * lon * lat.cos()), StrykeValue::float(r * lat)]))
}
fn builtin_equirectangular_project(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lat = args.first().map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let phi0 = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(6378137.0);
    Ok(StrykeValue::array(vec![StrykeValue::float(r * lon * phi0.cos()), StrykeValue::float(r * lat)]))
}
fn builtin_lambert_azimuthal_project(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lat = args.first().map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lat0 = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon0 = args.get(3).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let r = args.get(4).map(|v| v.to_number()).unwrap_or(6378137.0);
    let cos_c = lat0.sin() * lat.sin() + lat0.cos() * lat.cos() * (lon - lon0).cos();
    let k = (2.0 / (1.0 + cos_c)).max(0.0).sqrt();
    let x = r * k * lat.cos() * (lon - lon0).sin();
    let y = r * k * (lat0.cos() * lat.sin() - lat0.sin() * lat.cos() * (lon - lon0).cos());
    Ok(StrykeValue::array(vec![StrykeValue::float(x), StrykeValue::float(y)]))
}
fn builtin_albers_conic_project(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lat = args.first().map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let phi1 = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let phi2 = args.get(3).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let phi0 = args.get(4).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon0 = args.get(5).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let r = args.get(6).map(|v| v.to_number()).unwrap_or(6378137.0);
    let n = 0.5 * (phi1.sin() + phi2.sin());
    let c = phi1.cos().powi(2) + 2.0 * n * phi1.sin();
    let rho0 = r * (c - 2.0 * n * phi0.sin()).max(0.0).sqrt() / n;
    let rho = r * (c - 2.0 * n * lat.sin()).max(0.0).sqrt() / n;
    let theta = n * (lon - lon0);
    Ok(StrykeValue::array(vec![StrykeValue::float(rho * theta.sin()), StrykeValue::float(rho0 - rho * theta.cos())]))
}
fn builtin_geohash_encode(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lat = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let lon = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let precision = args.get(2).map(|v| v.to_number() as usize).unwrap_or(8);
    let base32 = "0123456789bcdefghjkmnpqrstuvwxyz";
    let mut lat_range = (-90.0_f64, 90.0_f64);
    let mut lon_range = (-180.0_f64, 180.0_f64);
    let mut even = true;
    let mut bit = 0_usize;
    let mut ch = 0_u8;
    let mut out = String::new();
    while out.len() < precision {
        if even {
            let mid = (lon_range.0 + lon_range.1) / 2.0;
            if lon >= mid { ch |= 1 << (4 - bit); lon_range.0 = mid; } else { lon_range.1 = mid; }
        } else {
            let mid = (lat_range.0 + lat_range.1) / 2.0;
            if lat >= mid { ch |= 1 << (4 - bit); lat_range.0 = mid; } else { lat_range.1 = mid; }
        }
        even = !even;
        if bit < 4 { bit += 1; } else {
            out.push(base32.as_bytes()[ch as usize] as char);
            bit = 0; ch = 0;
        }
    }
    Ok(StrykeValue::string(out))
}
fn builtin_geohash_decode(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let base32 = "0123456789bcdefghjkmnpqrstuvwxyz";
    let mut lat_range = (-90.0_f64, 90.0_f64);
    let mut lon_range = (-180.0_f64, 180.0_f64);
    let mut even = true;
    for c in s.chars() {
        let cd = base32.find(c.to_ascii_lowercase()).unwrap_or(0);
        for i in (0..5).rev() {
            let bit = (cd >> i) & 1;
            if even {
                let mid = (lon_range.0 + lon_range.1) / 2.0;
                if bit == 1 { lon_range.0 = mid; } else { lon_range.1 = mid; }
            } else {
                let mid = (lat_range.0 + lat_range.1) / 2.0;
                if bit == 1 { lat_range.0 = mid; } else { lat_range.1 = mid; }
            }
            even = !even;
        }
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::float((lat_range.0 + lat_range.1) / 2.0),
        StrykeValue::float((lon_range.0 + lon_range.1) / 2.0),
    ]))
}
fn builtin_gabor_kernel(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sigma = args.first().map(|v| v.to_number()).unwrap_or(2.0);
    let theta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(4.0);
    let psi = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma_aspect = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    let radius = (3.0 * sigma).ceil() as i64;
    let len = 2 * radius + 1;
    let mut m = vec![vec![0.0_f64; len as usize]; len as usize];
    for i in 0..len { for j in 0..len {
        let x = (j - radius) as f64; let y = (i - radius) as f64;
        let xt = x * theta.cos() + y * theta.sin();
        let yt = -x * theta.sin() + y * theta.cos();
        let env = (-(xt * xt + gamma_aspect * gamma_aspect * yt * yt) / (2.0 * sigma * sigma)).exp();
        m[i as usize][j as usize] = env * (2.0 * std::f64::consts::PI * xt / lambda + psi).cos();
    }}
    Ok(matrix_to_value(&m))
}
fn builtin_unsharp_mask_kernel(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let amount = args.first().map(|v| v.to_number()).unwrap_or(1.0);
    let kernel = vec![
        vec![-amount / 9.0; 3]; 3
    ];
    let mut k = kernel.clone();
    k[1][1] = 1.0 + 8.0 * amount / 9.0;
    Ok(matrix_to_value(&k))
}
fn builtin_emboss_kernel(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(matrix_to_value(&[vec![-2.0, -1.0, 0.0], vec![-1.0, 1.0, 1.0], vec![0.0, 1.0, 2.0]]))
}
fn builtin_box_blur_kernel(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let n = 2 * r + 1;
    let v = 1.0 / (n * n) as f64;
    Ok(matrix_to_value(&vec![vec![v; n]; n]))
}
fn builtin_motion_blur_kernel(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(5).max(1);
    let mut k = vec![vec![0.0_f64; n]; n];
    for i in 0..n { k[i][i] = 1.0 / n as f64; }
    Ok(matrix_to_value(&k))
}
fn builtin_sharpen_kernel(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(matrix_to_value(&[vec![0.0, -1.0, 0.0], vec![-1.0, 5.0, -1.0], vec![0.0, -1.0, 0.0]]))
}
fn builtin_edge_detect_kernel(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(matrix_to_value(&[vec![-1.0, -1.0, -1.0], vec![-1.0, 8.0, -1.0], vec![-1.0, -1.0, -1.0]]))
}
fn builtin_sobel_diagonal_kernel(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(matrix_to_value(&[vec![0.0, 1.0, 2.0], vec![-1.0, 0.0, 1.0], vec![-2.0, -1.0, 0.0]]))
}
fn builtin_haar_2d_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let img = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let h = img.len(); if h == 0 { return Ok(matrix_to_value(&[])); }
    let w = img[0].len();
    let h2 = h / 2; let w2 = w / 2;
    let s = 1.0 / 2.0_f64.sqrt();
    let mut ll = vec![vec![0.0_f64; w2]; h2];
    let mut hh = vec![vec![0.0_f64; w2]; h2];
    for i in 0..h2 { for j in 0..w2 {
        let a = img[2*i][2*j]; let b = img[2*i][2*j+1];
        let c = img[2*i+1][2*j]; let d = img[2*i+1][2*j+1];
        ll[i][j] = (a + b + c + d) * 0.5 * s * s;
        hh[i][j] = (a - b - c + d) * 0.5 * s * s;
    }}
    Ok(StrykeValue::array(vec![matrix_to_value(&ll), matrix_to_value(&hh)]))
}
fn builtin_db4_coeffs(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = vec![0.6830127, 1.1830127, 0.3169873, -0.1830127];
    let s = 1.0 / 2.0_f64.sqrt();
    Ok(StrykeValue::array(h.into_iter().map(|x| StrykeValue::float(x * s)).collect()))
}
fn builtin_db6_coeffs(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::array(vec![
        StrykeValue::float(0.47046721), StrykeValue::float(1.14111692),
        StrykeValue::float(0.65036501), StrykeValue::float(-0.19093442),
        StrykeValue::float(-0.12083221), StrykeValue::float(0.04981750),
    ]))
}
fn builtin_sym4_coeffs(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::array(vec![
        StrykeValue::float(-0.07576571), StrykeValue::float(-0.02963553),
        StrykeValue::float(0.49761867), StrykeValue::float(0.80373875),
        StrykeValue::float(0.29785780), StrykeValue::float(-0.09921954),
        StrykeValue::float(-0.01260396), StrykeValue::float(0.03222310),
    ]))
}
fn builtin_coif1_coeffs(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::array(vec![
        StrykeValue::float(-0.01565572), StrykeValue::float(-0.07273262),
        StrykeValue::float(0.38486485), StrykeValue::float(0.85257202),
        StrykeValue::float(0.33789767), StrykeValue::float(-0.07273262),
    ]))
}
fn builtin_aes_sbox_byte(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = (i1(args) & 0xff) as usize;
    static SBOX: [u8; 256] = [
        0x63,0x7c,0x77,0x7b,0xf2,0x6b,0x6f,0xc5,0x30,0x01,0x67,0x2b,0xfe,0xd7,0xab,0x76,
        0xca,0x82,0xc9,0x7d,0xfa,0x59,0x47,0xf0,0xad,0xd4,0xa2,0xaf,0x9c,0xa4,0x72,0xc0,
        0xb7,0xfd,0x93,0x26,0x36,0x3f,0xf7,0xcc,0x34,0xa5,0xe5,0xf1,0x71,0xd8,0x31,0x15,
        0x04,0xc7,0x23,0xc3,0x18,0x96,0x05,0x9a,0x07,0x12,0x80,0xe2,0xeb,0x27,0xb2,0x75,
        0x09,0x83,0x2c,0x1a,0x1b,0x6e,0x5a,0xa0,0x52,0x3b,0xd6,0xb3,0x29,0xe3,0x2f,0x84,
        0x53,0xd1,0x00,0xed,0x20,0xfc,0xb1,0x5b,0x6a,0xcb,0xbe,0x39,0x4a,0x4c,0x58,0xcf,
        0xd0,0xef,0xaa,0xfb,0x43,0x4d,0x33,0x85,0x45,0xf9,0x02,0x7f,0x50,0x3c,0x9f,0xa8,
        0x51,0xa3,0x40,0x8f,0x92,0x9d,0x38,0xf5,0xbc,0xb6,0xda,0x21,0x10,0xff,0xf3,0xd2,
        0xcd,0x0c,0x13,0xec,0x5f,0x97,0x44,0x17,0xc4,0xa7,0x7e,0x3d,0x64,0x5d,0x19,0x73,
        0x60,0x81,0x4f,0xdc,0x22,0x2a,0x90,0x88,0x46,0xee,0xb8,0x14,0xde,0x5e,0x0b,0xdb,
        0xe0,0x32,0x3a,0x0a,0x49,0x06,0x24,0x5c,0xc2,0xd3,0xac,0x62,0x91,0x95,0xe4,0x79,
        0xe7,0xc8,0x37,0x6d,0x8d,0xd5,0x4e,0xa9,0x6c,0x56,0xf4,0xea,0x65,0x7a,0xae,0x08,
        0xba,0x78,0x25,0x2e,0x1c,0xa6,0xb4,0xc6,0xe8,0xdd,0x74,0x1f,0x4b,0xbd,0x8b,0x8a,
        0x70,0x3e,0xb5,0x66,0x48,0x03,0xf6,0x0e,0x61,0x35,0x57,0xb9,0x86,0xc1,0x1d,0x9e,
        0xe1,0xf8,0x98,0x11,0x69,0xd9,0x8e,0x94,0x9b,0x1e,0x87,0xe9,0xce,0x55,0x28,0xdf,
        0x8c,0xa1,0x89,0x0d,0xbf,0xe6,0x42,0x68,0x41,0x99,0x2d,0x0f,0xb0,0x54,0xbb,0x16,
    ];
    Ok(StrykeValue::integer(SBOX[i] as i64))
}
fn builtin_aes_inv_sbox_byte(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = (i1(args) & 0xff) as usize;
    static INV: [u8; 256] = [
        0x52,0x09,0x6a,0xd5,0x30,0x36,0xa5,0x38,0xbf,0x40,0xa3,0x9e,0x81,0xf3,0xd7,0xfb,
        0x7c,0xe3,0x39,0x82,0x9b,0x2f,0xff,0x87,0x34,0x8e,0x43,0x44,0xc4,0xde,0xe9,0xcb,
        0x54,0x7b,0x94,0x32,0xa6,0xc2,0x23,0x3d,0xee,0x4c,0x95,0x0b,0x42,0xfa,0xc3,0x4e,
        0x08,0x2e,0xa1,0x66,0x28,0xd9,0x24,0xb2,0x76,0x5b,0xa2,0x49,0x6d,0x8b,0xd1,0x25,
        0x72,0xf8,0xf6,0x64,0x86,0x68,0x98,0x16,0xd4,0xa4,0x5c,0xcc,0x5d,0x65,0xb6,0x92,
        0x6c,0x70,0x48,0x50,0xfd,0xed,0xb9,0xda,0x5e,0x15,0x46,0x57,0xa7,0x8d,0x9d,0x84,
        0x90,0xd8,0xab,0x00,0x8c,0xbc,0xd3,0x0a,0xf7,0xe4,0x58,0x05,0xb8,0xb3,0x45,0x06,
        0xd0,0x2c,0x1e,0x8f,0xca,0x3f,0x0f,0x02,0xc1,0xaf,0xbd,0x03,0x01,0x13,0x8a,0x6b,
        0x3a,0x91,0x11,0x41,0x4f,0x67,0xdc,0xea,0x97,0xf2,0xcf,0xce,0xf0,0xb4,0xe6,0x73,
        0x96,0xac,0x74,0x22,0xe7,0xad,0x35,0x85,0xe2,0xf9,0x37,0xe8,0x1c,0x75,0xdf,0x6e,
        0x47,0xf1,0x1a,0x71,0x1d,0x29,0xc5,0x89,0x6f,0xb7,0x62,0x0e,0xaa,0x18,0xbe,0x1b,
        0xfc,0x56,0x3e,0x4b,0xc6,0xd2,0x79,0x20,0x9a,0xdb,0xc0,0xfe,0x78,0xcd,0x5a,0xf4,
        0x1f,0xdd,0xa8,0x33,0x88,0x07,0xc7,0x31,0xb1,0x12,0x10,0x59,0x27,0x80,0xec,0x5f,
        0x60,0x51,0x7f,0xa9,0x19,0xb5,0x4a,0x0d,0x2d,0xe5,0x7a,0x9f,0x93,0xc9,0x9c,0xef,
        0xa0,0xe0,0x3b,0x4d,0xae,0x2a,0xf5,0xb0,0xc8,0xeb,0xbb,0x3c,0x83,0x53,0x99,0x61,
        0x17,0x2b,0x04,0x7e,0xba,0x77,0xd6,0x26,0xe1,0x69,0x14,0x63,0x55,0x21,0x0c,0x7d,
    ];
    Ok(StrykeValue::integer(INV[i] as i64))
}
fn builtin_chacha20_qround(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut a = i1(args) as u32;
    let mut b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0) as u32;
    let mut c = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0) as u32;
    let mut d = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0) as u32;
    a = a.wrapping_add(b); d ^= a; d = d.rotate_left(16);
    c = c.wrapping_add(d); b ^= c; b = b.rotate_left(12);
    a = a.wrapping_add(b); d ^= a; d = d.rotate_left(8);
    c = c.wrapping_add(d); b ^= c; b = b.rotate_left(7);
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(a as i64), StrykeValue::integer(b as i64),
        StrykeValue::integer(c as i64), StrykeValue::integer(d as i64),
    ]))
}
fn builtin_xtea_round(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut v0 = i1(args) as u32;
    let mut v1 = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0) as u32;
    let key = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let k: [u32; 4] = [
        key.first().map(|v| v.to_number() as u32).unwrap_or(0),
        key.get(1).map(|v| v.to_number() as u32).unwrap_or(0),
        key.get(2).map(|v| v.to_number() as u32).unwrap_or(0),
        key.get(3).map(|v| v.to_number() as u32).unwrap_or(0),
    ];
    let mut sum = 0_u32;
    let delta = 0x9e3779b9_u32;
    for _ in 0..32 {
        v0 = v0.wrapping_add(((v1.wrapping_shl(4) ^ v1.wrapping_shr(5)).wrapping_add(v1)) ^ (sum.wrapping_add(k[(sum & 3) as usize])));
        sum = sum.wrapping_add(delta);
        v1 = v1.wrapping_add(((v0.wrapping_shl(4) ^ v0.wrapping_shr(5)).wrapping_add(v0)) ^ (sum.wrapping_add(k[((sum >> 11) & 3) as usize])));
    }
    Ok(StrykeValue::array(vec![StrykeValue::integer(v0 as i64), StrykeValue::integer(v1 as i64)]))
}
fn builtin_speck_round(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut x = i1(args) as u64;
    let mut y = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0) as u64;
    let k = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0) as u64;
    x = x.rotate_right(8).wrapping_add(y) ^ k;
    y = y.rotate_left(3) ^ x;
    Ok(StrykeValue::array(vec![StrykeValue::integer(x as i64), StrykeValue::integer(y as i64)]))
}
fn builtin_simon_round(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = i1(args) as u64;
    let y = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0) as u64;
    let k = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0) as u64;
    let new_x = y ^ (x.rotate_left(1) & x.rotate_left(8)) ^ x.rotate_left(2) ^ k;
    Ok(StrykeValue::array(vec![StrykeValue::integer(new_x as i64), StrykeValue::integer(x as i64)]))
}
fn builtin_geohash_neighbor(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let dir = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let coords = arg_to_vec(&builtin_geohash_decode(&[StrykeValue::string(s.clone())])?);
    let lat = coords.first().map(|v| v.to_number()).unwrap_or(0.0);
    let lon = coords.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let step = 1.0 / 2.0_f64.powi(s.len() as i32 * 5 / 2);
    let (dlat, dlon) = match dir.as_str() {
        "n" => (step, 0.0), "s" => (-step, 0.0), "e" => (0.0, step), "w" => (0.0, -step),
        "ne" => (step, step), "nw" => (step, -step), "se" => (-step, step), "sw" => (-step, -step),
        _ => (0.0, 0.0),
    };
    builtin_geohash_encode(&[
        StrykeValue::float(lat + dlat), StrykeValue::float(lon + dlon),
        StrykeValue::integer(s.len() as i64),
    ])
}
fn builtin_geohash_bbox(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let base32 = "0123456789bcdefghjkmnpqrstuvwxyz";
    let mut lat_range = (-90.0_f64, 90.0_f64);
    let mut lon_range = (-180.0_f64, 180.0_f64);
    let mut even = true;
    for c in s.chars() {
        let cd = base32.find(c.to_ascii_lowercase()).unwrap_or(0);
        for i in (0..5).rev() {
            let bit = (cd >> i) & 1;
            if even {
                let mid = (lon_range.0 + lon_range.1) / 2.0;
                if bit == 1 { lon_range.0 = mid; } else { lon_range.1 = mid; }
            } else {
                let mid = (lat_range.0 + lat_range.1) / 2.0;
                if bit == 1 { lat_range.0 = mid; } else { lat_range.1 = mid; }
            }
            even = !even;
        }
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::float(lat_range.0), StrykeValue::float(lon_range.0),
        StrykeValue::float(lat_range.1), StrykeValue::float(lon_range.1),
    ]))
}
