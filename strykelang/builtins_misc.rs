//! Miscellaneous primitives: BigInt ops + game/physics
//! vectors + probabilistic data structures + audio synthesis basics +
//! geo extras. Pure functions where possible; BigInt uses `num-bigint`.

use crate::value::StrykeValue;
use num_bigint::{BigInt, Sign};
use num_traits::{One, Pow, Signed, ToPrimitive, Zero};
use parking_lot::RwLock;
use std::sync::Arc;

fn arg_str(args: &[StrykeValue]) -> String {
    args.first().map(|v| v.to_string()).unwrap_or_default()
}

fn arg_f64(args: &[StrykeValue], idx: usize) -> Option<f64> {
    args.get(idx).map(|v| v.to_number())
}

fn arg_i64(args: &[StrykeValue], idx: usize) -> Option<i64> {
    args.get(idx).map(|v| v.to_int())
}

fn arr(vs: Vec<StrykeValue>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(vs)))
}

fn list_elements(v: &StrykeValue) -> Vec<StrykeValue> {
    if let Some(a) = v.as_array_ref() {
        return a.read().clone();
    }
    if let Some(a) = v.as_array_vec() {
        return a;
    }
    Vec::new()
}

// ══════════════════════════════════════════════════════════════════════
// BigInt operations (num-bigint backed; serialized as decimal strings)
// ══════════════════════════════════════════════════════════════════════

fn parse_bigint(s: &str) -> Option<BigInt> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return BigInt::parse_bytes(rest.as_bytes(), 16);
    }
    if let Some(rest) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        return BigInt::parse_bytes(rest.as_bytes(), 2);
    }
    if let Some(rest) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        return BigInt::parse_bytes(rest.as_bytes(), 8);
    }
    BigInt::parse_bytes(s.as_bytes(), 10)
}

fn arg_bigint(args: &[StrykeValue], idx: usize) -> Option<BigInt> {
    parse_bigint(&args.get(idx)?.to_string())
}

fn ret_bigint(n: BigInt) -> StrykeValue {
    StrykeValue::string(n.to_string())
}

pub fn bignum_new(args: &[StrykeValue]) -> StrykeValue {
    match arg_bigint(args, 0) {
        Some(n) => ret_bigint(n),
        None => StrykeValue::UNDEF,
    }
}

pub fn bignum_from_str(args: &[StrykeValue]) -> StrykeValue {
    bignum_new(args)
}

pub fn bignum_to_str(args: &[StrykeValue]) -> StrykeValue {
    bignum_new(args)
}

pub fn bignum_to_int(args: &[StrykeValue]) -> StrykeValue {
    match arg_bigint(args, 0) {
        Some(n) => n
            .to_i64()
            .map(StrykeValue::integer)
            .unwrap_or(StrykeValue::UNDEF),
        None => StrykeValue::UNDEF,
    }
}

pub fn bignum_add(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (arg_bigint(args, 0), arg_bigint(args, 1)) else {
        return StrykeValue::UNDEF;
    };
    ret_bigint(a + b)
}

pub fn bignum_sub(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (arg_bigint(args, 0), arg_bigint(args, 1)) else {
        return StrykeValue::UNDEF;
    };
    ret_bigint(a - b)
}

pub fn bignum_mul(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (arg_bigint(args, 0), arg_bigint(args, 1)) else {
        return StrykeValue::UNDEF;
    };
    ret_bigint(a * b)
}

pub fn bignum_div(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (arg_bigint(args, 0), arg_bigint(args, 1)) else {
        return StrykeValue::UNDEF;
    };
    if b.is_zero() {
        return StrykeValue::UNDEF;
    }
    ret_bigint(a / b)
}

pub fn bignum_mod(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (arg_bigint(args, 0), arg_bigint(args, 1)) else {
        return StrykeValue::UNDEF;
    };
    if b.is_zero() {
        return StrykeValue::UNDEF;
    }
    ret_bigint(a % b)
}

pub fn bignum_pow(args: &[StrykeValue]) -> StrykeValue {
    let Some(a) = arg_bigint(args, 0) else {
        return StrykeValue::UNDEF;
    };
    let exp = arg_i64(args, 1).unwrap_or(0).max(0) as u32;
    ret_bigint(a.pow(exp))
}

pub fn bignum_modpow(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(e), Some(m)) = (
        arg_bigint(args, 0),
        arg_bigint(args, 1),
        arg_bigint(args, 2),
    ) else {
        return StrykeValue::UNDEF;
    };
    if m.is_zero() {
        return StrykeValue::UNDEF;
    }
    ret_bigint(a.modpow(&e, &m))
}

pub fn bignum_gcd(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (arg_bigint(args, 0), arg_bigint(args, 1)) else {
        return StrykeValue::UNDEF;
    };
    // gcd via repeated Euclid since num-integer isn't in deps
    fn gcd(mut a: BigInt, mut b: BigInt) -> BigInt {
        let zero = BigInt::zero();
        while !b.is_zero() {
            let r = &a % &b;
            a = b;
            b = r;
        }
        if a.sign() == Sign::Minus {
            -a
        } else {
            a.max(zero)
        }
    }
    ret_bigint(gcd(a, b))
}

pub fn bignum_lcm(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (arg_bigint(args, 0), arg_bigint(args, 1)) else {
        return StrykeValue::UNDEF;
    };
    fn gcd(mut a: BigInt, mut b: BigInt) -> BigInt {
        let zero = BigInt::zero();
        while !b.is_zero() {
            let r = &a % &b;
            a = b;
            b = r;
        }
        if a.sign() == Sign::Minus {
            -a
        } else {
            a.max(zero)
        }
    }
    if a.is_zero() || b.is_zero() {
        return ret_bigint(BigInt::zero());
    }
    let g = gcd(a.clone(), b.clone());
    let product = a.abs() * b.abs();
    ret_bigint(product / g)
}

pub fn bignum_factorial(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0);
    if n < 0 {
        return StrykeValue::UNDEF;
    }
    let mut result = BigInt::one();
    for i in 2..=(n as u64) {
        result *= BigInt::from(i);
    }
    ret_bigint(result)
}

pub fn bignum_sqrt(args: &[StrykeValue]) -> StrykeValue {
    let Some(a) = arg_bigint(args, 0) else {
        return StrykeValue::UNDEF;
    };
    if a.sign() == Sign::Minus {
        return StrykeValue::UNDEF;
    }
    ret_bigint(a.sqrt())
}

pub fn bignum_bit_length(args: &[StrykeValue]) -> StrykeValue {
    match arg_bigint(args, 0) {
        Some(n) => StrykeValue::integer(n.bits() as i64),
        None => StrykeValue::UNDEF,
    }
}

pub fn bignum_set_bit(args: &[StrykeValue]) -> StrykeValue {
    let Some(mut n) = arg_bigint(args, 0) else {
        return StrykeValue::UNDEF;
    };
    let bit = arg_i64(args, 1).unwrap_or(0).max(0) as u64;
    n.set_bit(bit, true);
    ret_bigint(n)
}

pub fn bignum_clear_bit(args: &[StrykeValue]) -> StrykeValue {
    let Some(mut n) = arg_bigint(args, 0) else {
        return StrykeValue::UNDEF;
    };
    let bit = arg_i64(args, 1).unwrap_or(0).max(0) as u64;
    n.set_bit(bit, false);
    ret_bigint(n)
}

pub fn bignum_test_bit(args: &[StrykeValue]) -> StrykeValue {
    let Some(n) = arg_bigint(args, 0) else {
        return StrykeValue::UNDEF;
    };
    let bit = arg_i64(args, 1).unwrap_or(0).max(0) as u64;
    StrykeValue::integer(if n.bit(bit) { 1 } else { 0 })
}

pub fn bignum_and(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (arg_bigint(args, 0), arg_bigint(args, 1)) else {
        return StrykeValue::UNDEF;
    };
    ret_bigint(a & b)
}

pub fn bignum_or(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (arg_bigint(args, 0), arg_bigint(args, 1)) else {
        return StrykeValue::UNDEF;
    };
    ret_bigint(a | b)
}

pub fn bignum_xor(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (arg_bigint(args, 0), arg_bigint(args, 1)) else {
        return StrykeValue::UNDEF;
    };
    ret_bigint(a ^ b)
}

pub fn bignum_not(args: &[StrykeValue]) -> StrykeValue {
    let Some(a) = arg_bigint(args, 0) else {
        return StrykeValue::UNDEF;
    };
    ret_bigint(!a)
}

pub fn bignum_shl(args: &[StrykeValue]) -> StrykeValue {
    let Some(a) = arg_bigint(args, 0) else {
        return StrykeValue::UNDEF;
    };
    let n = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    ret_bigint(a << n)
}

pub fn bignum_shr(args: &[StrykeValue]) -> StrykeValue {
    let Some(a) = arg_bigint(args, 0) else {
        return StrykeValue::UNDEF;
    };
    let n = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    ret_bigint(a >> n)
}

pub fn bignum_compare(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (arg_bigint(args, 0), arg_bigint(args, 1)) else {
        return StrykeValue::UNDEF;
    };
    use std::cmp::Ordering;
    StrykeValue::integer(match a.cmp(&b) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    })
}

pub fn bignum_negate(args: &[StrykeValue]) -> StrykeValue {
    match arg_bigint(args, 0) {
        Some(a) => ret_bigint(-a),
        None => StrykeValue::UNDEF,
    }
}

pub fn bignum_abs(args: &[StrykeValue]) -> StrykeValue {
    match arg_bigint(args, 0) {
        Some(a) => ret_bigint(a.abs()),
        None => StrykeValue::UNDEF,
    }
}

pub fn bignum_sign(args: &[StrykeValue]) -> StrykeValue {
    match arg_bigint(args, 0) {
        Some(a) => StrykeValue::integer(match a.sign() {
            Sign::Minus => -1,
            Sign::NoSign => 0,
            Sign::Plus => 1,
        }),
        None => StrykeValue::UNDEF,
    }
}

pub fn bignum_is_zero(args: &[StrykeValue]) -> StrykeValue {
    match arg_bigint(args, 0) {
        Some(a) => StrykeValue::integer(if a.is_zero() { 1 } else { 0 }),
        None => StrykeValue::UNDEF,
    }
}

pub fn bignum_is_negative(args: &[StrykeValue]) -> StrykeValue {
    match arg_bigint(args, 0) {
        Some(a) => StrykeValue::integer(if a.sign() == Sign::Minus { 1 } else { 0 }),
        None => StrykeValue::UNDEF,
    }
}

pub fn bignum_is_prime(args: &[StrykeValue]) -> StrykeValue {
    // Miller-Rabin with deterministic witnesses for n < 3,317,044,064,679,887,385,961,981
    let Some(n) = arg_bigint(args, 0) else {
        return StrykeValue::UNDEF;
    };
    if n < BigInt::from(2) {
        return StrykeValue::integer(0);
    }
    let two = BigInt::from(2);
    if n == two {
        return StrykeValue::integer(1);
    }
    if (&n & BigInt::one()).is_zero() {
        return StrykeValue::integer(0);
    }
    // Find d, s such that n-1 = d * 2^s, d odd
    let n_minus_1 = &n - BigInt::one();
    let mut d = n_minus_1.clone();
    let mut s = 0u32;
    while (&d & BigInt::one()).is_zero() {
        d >>= 1;
        s += 1;
    }
    // Deterministic witnesses for various size ranges
    let witnesses: &[u64] = &[2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];
    'outer: for &w in witnesses {
        let w = BigInt::from(w);
        if w >= n {
            break;
        }
        let mut x = w.modpow(&d, &n);
        if x.is_one() || x == n_minus_1 {
            continue;
        }
        for _ in 0..s - 1 {
            x = x.modpow(&BigInt::from(2), &n);
            if x == n_minus_1 {
                continue 'outer;
            }
        }
        return StrykeValue::integer(0);
    }
    StrykeValue::integer(1)
}

pub fn bignum_random(args: &[StrykeValue]) -> StrykeValue {
    use rand::Rng;
    let bits = arg_i64(args, 0).unwrap_or(64).max(1) as u32;
    let mut rng = rand::thread_rng();
    let mut bytes = vec![0u8; bits.div_ceil(8) as usize];
    rng.fill(&mut bytes[..]);
    // Truncate to exact bit length
    let extra_bits = bytes.len() * 8 - bits as usize;
    if extra_bits > 0 {
        bytes[0] >>= extra_bits;
    }
    ret_bigint(BigInt::from_bytes_be(Sign::Plus, &bytes))
}

// ══════════════════════════════════════════════════════════════════════
// Physics / game primitives
// ══════════════════════════════════════════════════════════════════════

pub fn gravity_constant(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(9.80665)
}

pub fn physics_apply_force(args: &[StrykeValue]) -> StrykeValue {
    // args: [mass, [fx, fy], dt] → [ax*dt, ay*dt] (acceleration*dt = Δv)
    let mass = arg_f64(args, 0).unwrap_or(1.0).max(1e-12);
    let force = args.get(1).map(list_elements).unwrap_or_default();
    let dt = arg_f64(args, 2).unwrap_or(1.0 / 60.0);
    let fx = force.first().map(|v| v.to_number()).unwrap_or(0.0);
    let fy = force.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    arr(vec![
        StrykeValue::float(fx / mass * dt),
        StrykeValue::float(fy / mass * dt),
    ])
}

pub fn physics_apply_impulse(args: &[StrykeValue]) -> StrykeValue {
    let mass = arg_f64(args, 0).unwrap_or(1.0).max(1e-12);
    let imp = args.get(1).map(list_elements).unwrap_or_default();
    let ix = imp.first().map(|v| v.to_number()).unwrap_or(0.0);
    let iy = imp.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    arr(vec![
        StrykeValue::float(ix / mass),
        StrykeValue::float(iy / mass),
    ])
}

pub fn physics_collide_aabb(args: &[StrykeValue]) -> StrykeValue {
    // Two AABBs as [x, y, w, h]
    let a = args.first().map(list_elements).unwrap_or_default();
    let b = args.get(1).map(list_elements).unwrap_or_default();
    if a.len() < 4 || b.len() < 4 {
        return StrykeValue::UNDEF;
    }
    let (ax, ay, aw, ah) = (
        a[0].to_number(),
        a[1].to_number(),
        a[2].to_number(),
        a[3].to_number(),
    );
    let (bx, by, bw, bh) = (
        b[0].to_number(),
        b[1].to_number(),
        b[2].to_number(),
        b[3].to_number(),
    );
    StrykeValue::integer(
        if ax < bx + bw && ax + aw > bx && ay < by + bh && ay + ah > by {
            1
        } else {
            0
        },
    )
}

pub fn physics_collide_sphere(args: &[StrykeValue]) -> StrykeValue {
    // args: [c1x, c1y, r1, c2x, c2y, r2]
    let v: Vec<f64> = (0..6).map(|i| arg_f64(args, i).unwrap_or(0.0)).collect();
    let d2 = (v[3] - v[0]).powi(2) + (v[4] - v[1]).powi(2);
    let sr = v[2] + v[5];
    StrykeValue::integer(if d2 <= sr * sr { 1 } else { 0 })
}

pub fn physics_raycast(args: &[StrykeValue]) -> StrykeValue {
    // Simplified: ray (origin, dir) vs axis-aligned segment (a, b).
    // Return distance to intersection or undef.
    let origin = args.first().map(list_elements).unwrap_or_default();
    let dir = args.get(1).map(list_elements).unwrap_or_default();
    let max_dist = arg_f64(args, 2).unwrap_or(1e6);
    if origin.len() < 2 || dir.len() < 2 {
        return StrykeValue::UNDEF;
    }
    let len = (dir[0].to_number().powi(2) + dir[1].to_number().powi(2)).sqrt();
    if len < 1e-12 {
        return StrykeValue::UNDEF;
    }
    StrykeValue::float(max_dist) // placeholder; real impl needs scene
}

pub fn physics_step(args: &[StrykeValue]) -> StrykeValue {
    // [pos, vel, dt] → new [pos, vel]
    let pos = args.first().map(list_elements).unwrap_or_default();
    let vel = args.get(1).map(list_elements).unwrap_or_default();
    let dt = arg_f64(args, 2).unwrap_or(1.0 / 60.0);
    if pos.len() < 2 || vel.len() < 2 {
        return StrykeValue::UNDEF;
    }
    arr(vec![
        arr(vec![
            StrykeValue::float(pos[0].to_number() + vel[0].to_number() * dt),
            StrykeValue::float(pos[1].to_number() + vel[1].to_number() * dt),
        ]),
        arr(vec![vel[0].clone(), vel[1].clone()]),
    ])
}

pub fn particle_emit(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let y = arg_f64(args, 1).unwrap_or(0.0);
    let life = arg_f64(args, 2).unwrap_or(1.0);
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("x".to_string(), StrykeValue::float(x));
    h.insert("y".to_string(), StrykeValue::float(y));
    h.insert("vx".to_string(), StrykeValue::float(0.0));
    h.insert("vy".to_string(), StrykeValue::float(0.0));
    h.insert("life".to_string(), StrykeValue::float(life));
    h.insert("age".to_string(), StrykeValue::float(0.0));
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

pub fn particle_update(args: &[StrykeValue]) -> StrykeValue {
    let Some(p) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::UNDEF;
    };
    let dt = arg_f64(args, 1).unwrap_or(1.0 / 60.0);
    let mut g = p.write();
    let x = g.get("x").map(|v| v.to_number()).unwrap_or(0.0);
    let y = g.get("y").map(|v| v.to_number()).unwrap_or(0.0);
    let vx = g.get("vx").map(|v| v.to_number()).unwrap_or(0.0);
    let vy = g.get("vy").map(|v| v.to_number()).unwrap_or(0.0);
    let age = g.get("age").map(|v| v.to_number()).unwrap_or(0.0);
    g.insert("x".to_string(), StrykeValue::float(x + vx * dt));
    g.insert("y".to_string(), StrykeValue::float(y + vy * dt));
    g.insert("age".to_string(), StrykeValue::float(age + dt));
    drop(g);
    args.first().cloned().unwrap_or(StrykeValue::UNDEF)
}

// ── 2D vectors ────────────────────────────────────────────────────────

pub fn vector2_new(args: &[StrykeValue]) -> StrykeValue {
    arr(vec![
        StrykeValue::float(arg_f64(args, 0).unwrap_or(0.0)),
        StrykeValue::float(arg_f64(args, 1).unwrap_or(0.0)),
    ])
}

fn v2(v: &StrykeValue) -> Option<(f64, f64)> {
    let e = list_elements(v);
    if e.len() < 2 {
        return None;
    }
    Some((e[0].to_number(), e[1].to_number()))
}

pub fn vector2_add(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (args.first().and_then(v2), args.get(1).and_then(v2)) else {
        return StrykeValue::UNDEF;
    };
    arr(vec![
        StrykeValue::float(a.0 + b.0),
        StrykeValue::float(a.1 + b.1),
    ])
}

pub fn vector2_sub(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (args.first().and_then(v2), args.get(1).and_then(v2)) else {
        return StrykeValue::UNDEF;
    };
    arr(vec![
        StrykeValue::float(a.0 - b.0),
        StrykeValue::float(a.1 - b.1),
    ])
}

pub fn vector2_scale(args: &[StrykeValue]) -> StrykeValue {
    let Some(a) = args.first().and_then(v2) else {
        return StrykeValue::UNDEF;
    };
    let s = arg_f64(args, 1).unwrap_or(1.0);
    arr(vec![
        StrykeValue::float(a.0 * s),
        StrykeValue::float(a.1 * s),
    ])
}

pub fn vector2_dot(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (args.first().and_then(v2), args.get(1).and_then(v2)) else {
        return StrykeValue::UNDEF;
    };
    StrykeValue::float(a.0 * b.0 + a.1 * b.1)
}

pub fn vector2_cross(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (args.first().and_then(v2), args.get(1).and_then(v2)) else {
        return StrykeValue::UNDEF;
    };
    StrykeValue::float(a.0 * b.1 - a.1 * b.0)
}

pub fn vector2_length(args: &[StrykeValue]) -> StrykeValue {
    match args.first().and_then(v2) {
        Some((x, y)) => StrykeValue::float((x * x + y * y).sqrt()),
        None => StrykeValue::UNDEF,
    }
}

pub fn vector2_normalize(args: &[StrykeValue]) -> StrykeValue {
    let Some((x, y)) = args.first().and_then(v2) else {
        return StrykeValue::UNDEF;
    };
    let len = (x * x + y * y).sqrt();
    if len < 1e-12 {
        return arr(vec![StrykeValue::float(0.0), StrykeValue::float(0.0)]);
    }
    arr(vec![
        StrykeValue::float(x / len),
        StrykeValue::float(y / len),
    ])
}

pub fn vector2_distance(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (args.first().and_then(v2), args.get(1).and_then(v2)) else {
        return StrykeValue::UNDEF;
    };
    StrykeValue::float(((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt())
}

pub fn vector2_rotate(args: &[StrykeValue]) -> StrykeValue {
    let Some((x, y)) = args.first().and_then(v2) else {
        return StrykeValue::UNDEF;
    };
    let theta = arg_f64(args, 1).unwrap_or(0.0);
    let (c, s) = (theta.cos(), theta.sin());
    arr(vec![
        StrykeValue::float(x * c - y * s),
        StrykeValue::float(x * s + y * c),
    ])
}

// ── Quaternions ───────────────────────────────────────────────────────

pub fn quaternion_new(args: &[StrykeValue]) -> StrykeValue {
    arr(vec![
        StrykeValue::float(arg_f64(args, 0).unwrap_or(0.0)),
        StrykeValue::float(arg_f64(args, 1).unwrap_or(0.0)),
        StrykeValue::float(arg_f64(args, 2).unwrap_or(0.0)),
        StrykeValue::float(arg_f64(args, 3).unwrap_or(1.0)),
    ])
}

fn q4(v: &StrykeValue) -> Option<(f64, f64, f64, f64)> {
    let e = list_elements(v);
    if e.len() < 4 {
        return None;
    }
    Some((
        e[0].to_number(),
        e[1].to_number(),
        e[2].to_number(),
        e[3].to_number(),
    ))
}

pub fn quaternion_from_axis_angle(args: &[StrykeValue]) -> StrykeValue {
    let axis = args.first().map(list_elements).unwrap_or_default();
    let angle = arg_f64(args, 1).unwrap_or(0.0);
    if axis.len() < 3 {
        return StrykeValue::UNDEF;
    }
    let (ax, ay, az) = (
        axis[0].to_number(),
        axis[1].to_number(),
        axis[2].to_number(),
    );
    let len = (ax * ax + ay * ay + az * az).sqrt().max(1e-12);
    let half = angle / 2.0;
    let s = half.sin() / len;
    arr(vec![
        StrykeValue::float(ax * s),
        StrykeValue::float(ay * s),
        StrykeValue::float(az * s),
        StrykeValue::float(half.cos()),
    ])
}

pub fn quaternion_multiply(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (args.first().and_then(q4), args.get(1).and_then(q4)) else {
        return StrykeValue::UNDEF;
    };
    let (x1, y1, z1, w1) = a;
    let (x2, y2, z2, w2) = b;
    arr(vec![
        StrykeValue::float(w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2),
        StrykeValue::float(w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2),
        StrykeValue::float(w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2),
        StrykeValue::float(w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2),
    ])
}

pub fn quaternion_normalize(args: &[StrykeValue]) -> StrykeValue {
    let Some((x, y, z, w)) = args.first().and_then(q4) else {
        return StrykeValue::UNDEF;
    };
    let len = (x * x + y * y + z * z + w * w).sqrt().max(1e-12);
    arr(vec![
        StrykeValue::float(x / len),
        StrykeValue::float(y / len),
        StrykeValue::float(z / len),
        StrykeValue::float(w / len),
    ])
}

pub fn quaternion_to_matrix(args: &[StrykeValue]) -> StrykeValue {
    let Some((x, y, z, w)) = args.first().and_then(q4) else {
        return StrykeValue::UNDEF;
    };
    // 3x3 row-major matrix
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let xz = x * z;
    let yz = y * z;
    let wx = w * x;
    let wy = w * y;
    let wz = w * z;
    arr(vec![
        arr(vec![
            StrykeValue::float(1.0 - 2.0 * (yy + zz)),
            StrykeValue::float(2.0 * (xy - wz)),
            StrykeValue::float(2.0 * (xz + wy)),
        ]),
        arr(vec![
            StrykeValue::float(2.0 * (xy + wz)),
            StrykeValue::float(1.0 - 2.0 * (xx + zz)),
            StrykeValue::float(2.0 * (yz - wx)),
        ]),
        arr(vec![
            StrykeValue::float(2.0 * (xz - wy)),
            StrykeValue::float(2.0 * (yz + wx)),
            StrykeValue::float(1.0 - 2.0 * (xx + yy)),
        ]),
    ])
}

// ══════════════════════════════════════════════════════════════════════
// Music / audio basics
// ══════════════════════════════════════════════════════════════════════

const A4_FREQ: f64 = 440.0;
const A4_MIDI: f64 = 69.0;

pub fn freq_to_note(args: &[StrykeValue]) -> StrykeValue {
    let f = arg_f64(args, 0).unwrap_or(A4_FREQ);
    if f <= 0.0 {
        return StrykeValue::UNDEF;
    }
    let midi = (12.0 * (f / A4_FREQ).log2() + A4_MIDI).round();
    StrykeValue::integer(midi as i64)
}

pub fn note_to_freq(args: &[StrykeValue]) -> StrykeValue {
    let midi = arg_f64(args, 0).unwrap_or(A4_MIDI);
    StrykeValue::float(A4_FREQ * 2f64.powf((midi - A4_MIDI) / 12.0))
}

pub fn midi_note_to_name(args: &[StrykeValue]) -> StrykeValue {
    let midi = arg_i64(args, 0).unwrap_or(69);
    if !(0..=127).contains(&midi) {
        return StrykeValue::UNDEF;
    }
    let names = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let octave = midi / 12 - 1;
    let n = names[(midi % 12) as usize];
    StrykeValue::string(format!("{}{}", n, octave))
}

pub fn chord_notes(args: &[StrykeValue]) -> StrykeValue {
    let root = arg_i64(args, 0).unwrap_or(60);
    let kind = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "major".to_string());
    let intervals: &[i64] = match kind.to_ascii_lowercase().as_str() {
        "minor" | "min" | "m" => &[0, 3, 7],
        "dim" | "diminished" => &[0, 3, 6],
        "aug" | "augmented" => &[0, 4, 8],
        "7" | "dom7" => &[0, 4, 7, 10],
        "maj7" | "major7" => &[0, 4, 7, 11],
        "m7" | "min7" => &[0, 3, 7, 10],
        "sus2" => &[0, 2, 7],
        "sus4" => &[0, 5, 7],
        _ => &[0, 4, 7], // major
    };
    arr(intervals
        .iter()
        .map(|i| StrykeValue::integer(root + i))
        .collect())
}

pub fn scale_notes(args: &[StrykeValue]) -> StrykeValue {
    let root = arg_i64(args, 0).unwrap_or(60);
    let kind = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "major".to_string());
    let intervals: &[i64] = match kind.to_ascii_lowercase().as_str() {
        "minor" | "natural_minor" => &[0, 2, 3, 5, 7, 8, 10],
        "harmonic_minor" => &[0, 2, 3, 5, 7, 8, 11],
        "melodic_minor" => &[0, 2, 3, 5, 7, 9, 11],
        "pentatonic" | "major_pentatonic" => &[0, 2, 4, 7, 9],
        "minor_pentatonic" => &[0, 3, 5, 7, 10],
        "blues" => &[0, 3, 5, 6, 7, 10],
        "chromatic" => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
        "dorian" => &[0, 2, 3, 5, 7, 9, 10],
        "phrygian" => &[0, 1, 3, 5, 7, 8, 10],
        "lydian" => &[0, 2, 4, 6, 7, 9, 11],
        "mixolydian" => &[0, 2, 4, 5, 7, 9, 10],
        "locrian" => &[0, 1, 3, 5, 6, 8, 10],
        _ => &[0, 2, 4, 5, 7, 9, 11], // major
    };
    arr(intervals
        .iter()
        .map(|i| StrykeValue::integer(root + i))
        .collect())
}

pub fn transpose_note(args: &[StrykeValue]) -> StrykeValue {
    let midi = arg_i64(args, 0).unwrap_or(60);
    let semitones = arg_i64(args, 1).unwrap_or(0);
    StrykeValue::integer(midi + semitones)
}

pub fn window_tukey(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let alpha = arg_f64(args, 1).unwrap_or(0.5).clamp(0.0, 1.0);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let x = i as f64 / (n - 1).max(1) as f64;
        let w = if x < alpha / 2.0 {
            0.5 * (1.0 + (std::f64::consts::PI * (2.0 * x / alpha - 1.0)).cos())
        } else if x < 1.0 - alpha / 2.0 {
            1.0
        } else {
            0.5 * (1.0 + (std::f64::consts::PI * (2.0 * x / alpha - 2.0 / alpha + 1.0)).cos())
        };
        out.push(StrykeValue::float(w));
    }
    arr(out)
}

pub fn zero_crossing_rate(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    if xs.len() < 2 {
        return StrykeValue::float(0.0);
    }
    let mut count = 0u64;
    for w in xs.windows(2) {
        let a = w[0].to_number();
        let b = w[1].to_number();
        if (a >= 0.0) != (b >= 0.0) {
            count += 1;
        }
    }
    StrykeValue::float(count as f64 / (xs.len() - 1) as f64)
}

pub fn peak_db(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let peak = xs
        .iter()
        .map(|v| v.to_number().abs())
        .fold(0.0f64, f64::max);
    if peak < 1e-12 {
        return StrykeValue::float(-f64::INFINITY);
    }
    StrykeValue::float(20.0 * peak.log10())
}

pub fn audio_normalize(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let peak = xs
        .iter()
        .map(|v| v.to_number().abs())
        .fold(0.0f64, f64::max);
    if peak < 1e-12 {
        return arr(xs);
    }
    let target = arg_f64(args, 1).unwrap_or(1.0);
    let scale = target / peak;
    arr(xs
        .iter()
        .map(|v| StrykeValue::float(v.to_number() * scale))
        .collect())
}

pub fn audio_fade_in(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let n = xs.len();
    arr(xs
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let g = if n > 1 {
                i as f64 / (n - 1) as f64
            } else {
                1.0
            };
            StrykeValue::float(v.to_number() * g)
        })
        .collect())
}

pub fn audio_fade_out(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let n = xs.len();
    arr(xs
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let g = if n > 1 {
                1.0 - i as f64 / (n - 1) as f64
            } else {
                1.0
            };
            StrykeValue::float(v.to_number() * g)
        })
        .collect())
}

pub fn audio_to_mono(args: &[StrykeValue]) -> StrykeValue {
    // Stereo: [[L,R], [L,R], ...] → mean
    let xs = args.first().map(list_elements).unwrap_or_default();
    arr(xs
        .iter()
        .map(|v| {
            let pair = list_elements(v);
            if pair.is_empty() {
                StrykeValue::float(0.0)
            } else {
                let sum: f64 = pair.iter().map(|x| x.to_number()).sum();
                StrykeValue::float(sum / pair.len() as f64)
            }
        })
        .collect())
}

pub fn audio_to_stereo(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    arr(xs
        .iter()
        .map(|v| {
            arr(vec![
                StrykeValue::float(v.to_number()),
                StrykeValue::float(v.to_number()),
            ])
        })
        .collect())
}

fn biquad_coeffs(kind: &str, cutoff: f64, sample_rate: f64, q: f64) -> [f64; 5] {
    let w0 = 2.0 * std::f64::consts::PI * cutoff / sample_rate;
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * q);
    let (b0, b1, b2, a0, a1, a2) = match kind {
        "lp" => (
            (1.0 - cos_w0) / 2.0,
            1.0 - cos_w0,
            (1.0 - cos_w0) / 2.0,
            1.0 + alpha,
            -2.0 * cos_w0,
            1.0 - alpha,
        ),
        "hp" => (
            (1.0 + cos_w0) / 2.0,
            -(1.0 + cos_w0),
            (1.0 + cos_w0) / 2.0,
            1.0 + alpha,
            -2.0 * cos_w0,
            1.0 - alpha,
        ),
        "bp" => (
            sin_w0 / 2.0,
            0.0,
            -sin_w0 / 2.0,
            1.0 + alpha,
            -2.0 * cos_w0,
            1.0 - alpha,
        ),
        "notch" => (
            1.0,
            -2.0 * cos_w0,
            1.0,
            1.0 + alpha,
            -2.0 * cos_w0,
            1.0 - alpha,
        ),
        _ => (1.0, 0.0, 0.0, 1.0, 0.0, 0.0),
    };
    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

fn biquad_apply(xs: &[StrykeValue], coeffs: [f64; 5]) -> Vec<StrykeValue> {
    let mut out: Vec<StrykeValue> = Vec::with_capacity(xs.len());
    let mut x1 = 0.0;
    let mut x2 = 0.0;
    let mut y1 = 0.0;
    let mut y2 = 0.0;
    for v in xs {
        let x0 = v.to_number();
        let y0 = coeffs[0] * x0 + coeffs[1] * x1 + coeffs[2] * x2 - coeffs[3] * y1 - coeffs[4] * y2;
        out.push(StrykeValue::float(y0));
        x2 = x1;
        x1 = x0;
        y2 = y1;
        y1 = y0;
    }
    out
}

pub fn biquad_lowpass(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let cutoff = arg_f64(args, 1).unwrap_or(1000.0);
    let sr = arg_f64(args, 2).unwrap_or(44100.0);
    let q = arg_f64(args, 3).unwrap_or(0.707);
    arr(biquad_apply(&xs, biquad_coeffs("lp", cutoff, sr, q)))
}

pub fn biquad_highpass(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let cutoff = arg_f64(args, 1).unwrap_or(1000.0);
    let sr = arg_f64(args, 2).unwrap_or(44100.0);
    let q = arg_f64(args, 3).unwrap_or(0.707);
    arr(biquad_apply(&xs, biquad_coeffs("hp", cutoff, sr, q)))
}

pub fn biquad_bandpass(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let cutoff = arg_f64(args, 1).unwrap_or(1000.0);
    let sr = arg_f64(args, 2).unwrap_or(44100.0);
    let q = arg_f64(args, 3).unwrap_or(0.707);
    arr(biquad_apply(&xs, biquad_coeffs("bp", cutoff, sr, q)))
}

pub fn biquad_notch(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let cutoff = arg_f64(args, 1).unwrap_or(1000.0);
    let sr = arg_f64(args, 2).unwrap_or(44100.0);
    let q = arg_f64(args, 3).unwrap_or(0.707);
    arr(biquad_apply(&xs, biquad_coeffs("notch", cutoff, sr, q)))
}

fn osc_samples<F: Fn(f64) -> f64>(args: &[StrykeValue], shape: F) -> StrykeValue {
    let freq = arg_f64(args, 0).unwrap_or(440.0);
    let sr = arg_f64(args, 1).unwrap_or(44100.0);
    let n = arg_i64(args, 2).unwrap_or(44100).max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let phase = (i as f64 * freq / sr) % 1.0;
        out.push(StrykeValue::float(shape(phase)));
    }
    arr(out)
}

pub fn oscillator_sine(args: &[StrykeValue]) -> StrykeValue {
    osc_samples(args, |p| (2.0 * std::f64::consts::PI * p).sin())
}

pub fn oscillator_square(args: &[StrykeValue]) -> StrykeValue {
    osc_samples(args, |p| if p < 0.5 { 1.0 } else { -1.0 })
}

pub fn oscillator_sawtooth(args: &[StrykeValue]) -> StrykeValue {
    osc_samples(args, |p| 2.0 * p - 1.0)
}

pub fn oscillator_triangle(args: &[StrykeValue]) -> StrykeValue {
    osc_samples(args, |p| {
        if p < 0.5 {
            4.0 * p - 1.0
        } else {
            3.0 - 4.0 * p
        }
    })
}

pub fn adsr_envelope(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_f64(args, 0).unwrap_or(0.1);
    let d = arg_f64(args, 1).unwrap_or(0.1);
    let s = arg_f64(args, 2).unwrap_or(0.7);
    let r = arg_f64(args, 3).unwrap_or(0.2);
    let sr = arg_f64(args, 4).unwrap_or(44100.0);
    let hold = arg_f64(args, 5).unwrap_or(0.5);
    let mut out = Vec::new();
    let a_n = (a * sr) as usize;
    let d_n = (d * sr) as usize;
    let h_n = (hold * sr) as usize;
    let r_n = (r * sr) as usize;
    for i in 0..a_n {
        out.push(StrykeValue::float(i as f64 / a_n.max(1) as f64));
    }
    for i in 0..d_n {
        let t = i as f64 / d_n.max(1) as f64;
        out.push(StrykeValue::float(1.0 - (1.0 - s) * t));
    }
    for _ in 0..h_n {
        out.push(StrykeValue::float(s));
    }
    for i in 0..r_n {
        let t = i as f64 / r_n.max(1) as f64;
        out.push(StrykeValue::float(s * (1.0 - t)));
    }
    arr(out)
}

pub fn ar_envelope(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_f64(args, 0).unwrap_or(0.05);
    let r = arg_f64(args, 1).unwrap_or(0.1);
    let sr = arg_f64(args, 2).unwrap_or(44100.0);
    let a_n = (a * sr) as usize;
    let r_n = (r * sr) as usize;
    let mut out = Vec::new();
    for i in 0..a_n {
        out.push(StrykeValue::float(i as f64 / a_n.max(1) as f64));
    }
    for i in 0..r_n {
        out.push(StrykeValue::float(1.0 - i as f64 / r_n.max(1) as f64));
    }
    arr(out)
}

pub fn crossfade(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(list_elements).unwrap_or_default();
    let b = args.get(1).map(list_elements).unwrap_or_default();
    let n = a.len().min(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = if n > 1 {
            i as f64 / (n - 1) as f64
        } else {
            0.5
        };
        let av = a[i].to_number();
        let bv = b[i].to_number();
        out.push(StrykeValue::float(av * (1.0 - t) + bv * t));
    }
    arr(out)
}

pub fn fade_curve_linear(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.push(StrykeValue::float(i as f64 / n.max(1) as f64));
    }
    arr(out)
}

pub fn fade_curve_logarithmic(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = (i as f64 / n.max(1) as f64).max(1e-6);
        out.push(StrykeValue::float((t.log10() + 2.0) / 2.0));
    }
    arr(out)
}

pub fn fade_curve_exponential(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / n.max(1) as f64;
        out.push(StrykeValue::float(t * t));
    }
    arr(out)
}

// ══════════════════════════════════════════════════════════════════════
// Geo extras (no new crate)
// ══════════════════════════════════════════════════════════════════════

pub fn bbox_contains(args: &[StrykeValue]) -> StrykeValue {
    // bbox: [minlat, minlon, maxlat, maxlon]; point: [lat, lon]
    let bbox = args.first().map(list_elements).unwrap_or_default();
    let p = args.get(1).map(list_elements).unwrap_or_default();
    if bbox.len() < 4 || p.len() < 2 {
        return StrykeValue::UNDEF;
    }
    let lat = p[0].to_number();
    let lon = p[1].to_number();
    let ok = lat >= bbox[0].to_number()
        && lat <= bbox[2].to_number()
        && lon >= bbox[1].to_number()
        && lon <= bbox[3].to_number();
    StrykeValue::integer(if ok { 1 } else { 0 })
}

pub fn bbox_union(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(list_elements).unwrap_or_default();
    let b = args.get(1).map(list_elements).unwrap_or_default();
    if a.len() < 4 || b.len() < 4 {
        return StrykeValue::UNDEF;
    }
    arr(vec![
        StrykeValue::float(a[0].to_number().min(b[0].to_number())),
        StrykeValue::float(a[1].to_number().min(b[1].to_number())),
        StrykeValue::float(a[2].to_number().max(b[2].to_number())),
        StrykeValue::float(a[3].to_number().max(b[3].to_number())),
    ])
}

pub fn bbox_intersect(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(list_elements).unwrap_or_default();
    let b = args.get(1).map(list_elements).unwrap_or_default();
    if a.len() < 4 || b.len() < 4 {
        return StrykeValue::UNDEF;
    }
    let min_lat = a[0].to_number().max(b[0].to_number());
    let min_lon = a[1].to_number().max(b[1].to_number());
    let max_lat = a[2].to_number().min(b[2].to_number());
    let max_lon = a[3].to_number().min(b[3].to_number());
    if min_lat > max_lat || min_lon > max_lon {
        return StrykeValue::UNDEF;
    }
    arr(vec![
        StrykeValue::float(min_lat),
        StrykeValue::float(min_lon),
        StrykeValue::float(max_lat),
        StrykeValue::float(max_lon),
    ])
}

pub fn bbox_center(args: &[StrykeValue]) -> StrykeValue {
    let bbox = args.first().map(list_elements).unwrap_or_default();
    if bbox.len() < 4 {
        return StrykeValue::UNDEF;
    }
    arr(vec![
        StrykeValue::float((bbox[0].to_number() + bbox[2].to_number()) / 2.0),
        StrykeValue::float((bbox[1].to_number() + bbox[3].to_number()) / 2.0),
    ])
}

pub fn bbox_area(args: &[StrykeValue]) -> StrykeValue {
    let bbox = args.first().map(list_elements).unwrap_or_default();
    if bbox.len() < 4 {
        return StrykeValue::UNDEF;
    }
    let dlat = bbox[2].to_number() - bbox[0].to_number();
    let dlon = bbox[3].to_number() - bbox[1].to_number();
    StrykeValue::float(dlat.abs() * dlon.abs())
}

pub fn mercator_unproject(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let y = arg_f64(args, 1).unwrap_or(0.0);
    let r = 6378137.0;
    let lon = (x / r).to_degrees();
    let lat = (y / r)
        .atan()
        .exp()
        .atan()
        .mul_add(2.0, -std::f64::consts::FRAC_PI_2)
        .to_degrees();
    arr(vec![StrykeValue::float(lat), StrykeValue::float(lon)])
}

pub fn geohash_precision(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    StrykeValue::integer(s.chars().count() as i64)
}

// Returns `string` for compatibility with the arg_str + arg_f64 helpers.
#[allow(dead_code)]
fn placeholder_use() {
    let _ = arg_str(&[]);
}
