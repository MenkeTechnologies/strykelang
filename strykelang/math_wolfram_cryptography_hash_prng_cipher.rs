// cryptography deep: hash mixers, KDFs, PRNGs, ciphers, primality.

// FNV-1a 32-bit
fn builtin_fnv1a_32(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut h = 2166136261_u32;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    Ok(StrykeValue::integer(h as i64))
}
// FNV-1a 64-bit
fn builtin_fnv1a_64(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut h = 14695981039346656037_u64;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    Ok(StrykeValue::integer(h as i64))
}
// DJB2 hash
// SDBM hash
fn builtin_sdbm_hash(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut h = 0_u32;
    for b in s.bytes() {
        h = (b as u32).wrapping_add(h.wrapping_shl(6)).wrapping_add(h.wrapping_shl(16)).wrapping_sub(h);
    }
    Ok(StrykeValue::integer(h as i64))
}
// MurmurHash3 x86_32 (one-shot)
#[allow(dead_code)]
fn builtin_murmur3_32(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let seed = args.get(1).map(|v| v.to_number() as u32).unwrap_or(0);
    let bytes = s.as_bytes();
    let len = bytes.len();
    let nblocks = len / 4;
    let mut h1 = seed;
    let c1: u32 = 0xcc9e2d51;
    let c2: u32 = 0x1b873593;
    for i in 0..nblocks {
        let p = i * 4;
        let mut k1 = u32::from_le_bytes([bytes[p], bytes[p+1], bytes[p+2], bytes[p+3]]);
        k1 = k1.wrapping_mul(c1);
        k1 = k1.rotate_left(15);
        k1 = k1.wrapping_mul(c2);
        h1 ^= k1;
        h1 = h1.rotate_left(13);
        h1 = h1.wrapping_mul(5).wrapping_add(0xe6546b64);
    }
    let tail = &bytes[nblocks * 4..];
    let mut k1 = 0_u32;
    if tail.len() >= 3 { k1 ^= (tail[2] as u32) << 16; }
    if tail.len() >= 2 { k1 ^= (tail[1] as u32) << 8; }
    if !tail.is_empty() {
        k1 ^= tail[0] as u32;
        k1 = k1.wrapping_mul(c1);
        k1 = k1.rotate_left(15);
        k1 = k1.wrapping_mul(c2);
        h1 ^= k1;
    }
    h1 ^= len as u32;
    h1 ^= h1 >> 16;
    h1 = h1.wrapping_mul(0x85ebca6b);
    h1 ^= h1 >> 13;
    h1 = h1.wrapping_mul(0xc2b2ae35);
    h1 ^= h1 >> 16;
    Ok(StrykeValue::integer(h1 as i64))
}

// xxHash32 (one-shot, simplified)
#[allow(dead_code)]
fn builtin_xxhash32(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let seed = args.get(1).map(|v| v.to_number() as u32).unwrap_or(0);
    let bytes = s.as_bytes();
    let p1: u32 = 0x9E3779B1;
    let p2: u32 = 0x85EBCA77;
    let p3: u32 = 0xC2B2AE3D;
    let p4: u32 = 0x27D4EB2F;
    let p5: u32 = 0x165667B1;
    let len = bytes.len();
    let mut h32 = if len >= 16 {
        let mut v1 = seed.wrapping_add(p1).wrapping_add(p2);
        let mut v2 = seed.wrapping_add(p2);
        let mut v3 = seed;
        let mut v4 = seed.wrapping_sub(p1);
        let mut p = 0;
        while p + 16 <= len {
            let r = |bs: &[u8], o: usize| u32::from_le_bytes([bs[o], bs[o+1], bs[o+2], bs[o+3]]);
            v1 = v1.wrapping_add(r(bytes, p).wrapping_mul(p2)).rotate_left(13).wrapping_mul(p1);
            v2 = v2.wrapping_add(r(bytes, p+4).wrapping_mul(p2)).rotate_left(13).wrapping_mul(p1);
            v3 = v3.wrapping_add(r(bytes, p+8).wrapping_mul(p2)).rotate_left(13).wrapping_mul(p1);
            v4 = v4.wrapping_add(r(bytes, p+12).wrapping_mul(p2)).rotate_left(13).wrapping_mul(p1);
            p += 16;
        }
        v1.rotate_left(1).wrapping_add(v2.rotate_left(7))
            .wrapping_add(v3.rotate_left(12)).wrapping_add(v4.rotate_left(18))
    } else {
        seed.wrapping_add(p5)
    };
    h32 = h32.wrapping_add(len as u32);
    let mut p = if len >= 16 { len & !15 } else { 0 };
    while p + 4 <= len {
        let v = u32::from_le_bytes([bytes[p], bytes[p+1], bytes[p+2], bytes[p+3]]);
        h32 = h32.wrapping_add(v.wrapping_mul(p3)).rotate_left(17).wrapping_mul(p4);
        p += 4;
    }
    while p < len {
        h32 = h32.wrapping_add((bytes[p] as u32).wrapping_mul(p5)).rotate_left(11).wrapping_mul(p1);
        p += 1;
    }
    h32 ^= h32 >> 15;
    h32 = h32.wrapping_mul(p2);
    h32 ^= h32 >> 13;
    h32 = h32.wrapping_mul(p3);
    h32 ^= h32 >> 16;
    Ok(StrykeValue::integer(h32 as i64))
}

// SipHash24 (simplified one-shot, 64-bit)
fn builtin_siphash24(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let k0 = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    let k1 = args.get(2).map(|v| v.to_number() as u64).unwrap_or(0);
    let bytes = s.as_bytes();
    let mut v0 = 0x736f6d6570736575_u64 ^ k0;
    let mut v1 = 0x646f72616e646f6d_u64 ^ k1;
    let mut v2 = 0x6c7967656e657261_u64 ^ k0;
    let mut v3 = 0x7465646279746573_u64 ^ k1;
    let sip_round = |v0: &mut u64, v1: &mut u64, v2: &mut u64, v3: &mut u64| {
        *v0 = v0.wrapping_add(*v1); *v1 = v1.rotate_left(13); *v1 ^= *v0; *v0 = v0.rotate_left(32);
        *v2 = v2.wrapping_add(*v3); *v3 = v3.rotate_left(16); *v3 ^= *v2;
        *v0 = v0.wrapping_add(*v3); *v3 = v3.rotate_left(21); *v3 ^= *v0;
        *v2 = v2.wrapping_add(*v1); *v1 = v1.rotate_left(17); *v1 ^= *v2; *v2 = v2.rotate_left(32);
    };
    let nblocks = bytes.len() / 8;
    for i in 0..nblocks {
        let p = i * 8;
        let m = u64::from_le_bytes([
            bytes[p], bytes[p+1], bytes[p+2], bytes[p+3],
            bytes[p+4], bytes[p+5], bytes[p+6], bytes[p+7],
        ]);
        v3 ^= m;
        sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
        sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
        v0 ^= m;
    }
    let mut last = (bytes.len() as u64 & 0xff) << 56;
    let tail = &bytes[nblocks * 8..];
    for (i, &b) in tail.iter().enumerate() {
        last |= (b as u64) << (i * 8);
    }
    v3 ^= last;
    sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
    sip_round(&mut v0, &mut v1, &mut v2, &mut v3);
    v0 ^= last;
    v2 ^= 0xff;
    for _ in 0..4 { sip_round(&mut v0, &mut v1, &mut v2, &mut v3); }
    Ok(StrykeValue::integer((v0 ^ v1 ^ v2 ^ v3) as i64))
}

// PBKDF2-HMAC-SHA1 per RFC 2898 §5.2 / RFC 8018: derive a 20-byte key from
// password + salt with `iters` iterations, returned as the i64 reading of the
// first 8 bytes (big-endian). Real construction:
//   T = U_1 XOR U_2 XOR ... XOR U_iter
//   U_1 = HMAC-SHA1(P, S || INT(1));  U_j = HMAC-SHA1(P, U_{j-1})
fn builtin_pbkdf2_hmac_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    use hmac::{Hmac, Mac};
    type HSha1 = Hmac<sha1::Sha1>;
    let pw = args.first().map(|v| v.to_string()).unwrap_or_default();
    let salt = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let iters = args.get(2).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    let mut mac = match <HSha1 as Mac>::new_from_slice(pw.as_bytes()) {
        Ok(m) => m,
        Err(_) => return Ok(StrykeValue::integer(0)),
    };
    mac.update(salt.as_bytes());
    mac.update(&[0, 0, 0, 1]);
    let mut u = mac.finalize().into_bytes().to_vec();
    let mut t = u.clone();
    for _ in 1..iters {
        let mut m = match <HSha1 as Mac>::new_from_slice(pw.as_bytes()) {
            Ok(m) => m,
            Err(_) => break,
        };
        m.update(&u);
        u = m.finalize().into_bytes().to_vec();
        for (a, b) in t.iter_mut().zip(u.iter()) { *a ^= *b; }
    }
    let mut acc = 0_u64;
    for &byte in t.iter().take(8) { acc = (acc << 8) | byte as u64; }
    Ok(StrykeValue::integer(acc as i64))
}

// Scrypt salsa20/8 word mixer (single round)
fn builtin_scrypt_round(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<u32> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number() as u32).collect();
    if xs.len() < 16 { return Ok(StrykeValue::array(xs.into_iter().map(|v| StrykeValue::integer(v as i64)).collect())); }
    let mut x = [0_u32; 16];
    x.copy_from_slice(&xs[..16]);
    for _ in 0..4 {
        x[4] ^= x[0].wrapping_add(x[12]).rotate_left(7);
        x[8] ^= x[4].wrapping_add(x[0]).rotate_left(9);
        x[12] ^= x[8].wrapping_add(x[4]).rotate_left(13);
        x[0] ^= x[12].wrapping_add(x[8]).rotate_left(18);
    }
    Ok(StrykeValue::array(x.iter().map(|&v| StrykeValue::integer(v as i64)).collect()))
}

// Bcrypt-style cost (just iterations)
fn builtin_bcrypt_cost_iters(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cost = i1(args).clamp(4, 31) as u32;
    Ok(StrykeValue::integer(1_i64 << cost))
}

// Argon2 G compression on two 1024-byte blocks per RFC 9106 §3.5: R = X ⊕ Y;
// apply Blake2b's permutation P to columns (8 GB rounds), then to rows; XOR R.
// We implement the canonical GB round-function (Blake2b's G with rotations
// 32, 24, 16, 63) over each 16-word group and treat the array as a single
// row pass (one full P invocation), which is the load-bearing primitive.
fn builtin_argon2_block_mix(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<u64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number() as u64).collect();
    let ys: Vec<u64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number() as u64).collect();
    let len = xs.len().max(ys.len());
    let mut r: Vec<u64> = (0..len).map(|i| xs.get(i).copied().unwrap_or(0)
        ^ ys.get(i).copied().unwrap_or(0)).collect();
    let z = r.clone();
    fn gb(v: &mut [u64], a: usize, b: usize, c: usize, d: usize) {
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(2u64.wrapping_mul(
            (v[a] as u32 as u64).wrapping_mul(v[b] as u32 as u64)));
        v[d] = (v[d] ^ v[a]).rotate_right(32);
        v[c] = v[c].wrapping_add(v[d]).wrapping_add(2u64.wrapping_mul(
            (v[c] as u32 as u64).wrapping_mul(v[d] as u32 as u64)));
        v[b] = (v[b] ^ v[c]).rotate_right(24);
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(2u64.wrapping_mul(
            (v[a] as u32 as u64).wrapping_mul(v[b] as u32 as u64)));
        v[d] = (v[d] ^ v[a]).rotate_right(16);
        v[c] = v[c].wrapping_add(v[d]).wrapping_add(2u64.wrapping_mul(
            (v[c] as u32 as u64).wrapping_mul(v[d] as u32 as u64)));
        v[b] = (v[b] ^ v[c]).rotate_right(63);
    }
    for chunk_start in (0..len).step_by(16) {
        if chunk_start + 16 > len { break; }
        let s = &mut r[chunk_start..chunk_start + 16];
        gb(s, 0, 4,  8, 12); gb(s, 1, 5,  9, 13);
        gb(s, 2, 6, 10, 14); gb(s, 3, 7, 11, 15);
        gb(s, 0, 5, 10, 15); gb(s, 1, 6, 11, 12);
        gb(s, 2, 7,  8, 13); gb(s, 3, 4,  9, 14);
    }
    let out: Vec<StrykeValue> = (0..len).map(|i| {
        StrykeValue::integer((r[i] ^ z[i]) as i64)
    }).collect();
    Ok(StrykeValue::array(out))
}

// HKDF expand step (one block)
fn builtin_hkdf_expand_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prk = args.first().map(|v| v.to_string()).unwrap_or_default();
    let info = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let counter = args.get(2).map(|v| v.to_number() as u8).unwrap_or(1);
    let mut h = 0_u64;
    for b in prk.bytes().chain(info.bytes()).chain(std::iter::once(counter)) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    Ok(StrykeValue::integer(h as i64))
}

// Linear feedback shift register (Galois LFSR) step
fn builtin_lfsr_galois_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let state = i1(args) as u64;
    let mask = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0xb400);
    let next = if state & 1 != 0 { (state >> 1) ^ mask } else { state >> 1 };
    Ok(StrykeValue::integer(next as i64))
}

// Mersenne Twister mt19937 next (32 bits) — single-step from seeded state
fn builtin_mt19937_temper(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut y = i1(args) as u32;
    y ^= y >> 11;
    y ^= (y << 7) & 0x9d2c5680;
    y ^= (y << 15) & 0xefc60000;
    y ^= y >> 18;
    Ok(StrykeValue::integer(y as i64))
}

// xorshift64
fn builtin_xorshift64(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut x = i1(args) as u64;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    Ok(StrykeValue::integer(x as i64))
}
// xorshift32
fn builtin_xorshift32(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut x = i1(args) as u32;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    Ok(StrykeValue::integer(x as i64))
}
// PCG32 step
fn builtin_pcg32_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let state = i1(args) as u64;
    let inc = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0xda3e39cb94b95bdb);
    let new_state = state.wrapping_mul(6364136223846793005).wrapping_add(inc | 1);
    let xorshifted = (((new_state >> 18) ^ new_state) >> 27) as u32;
    let rot = (new_state >> 59) as u32;
    let out = xorshifted.rotate_right(rot);
    Ok(StrykeValue::array(vec![StrykeValue::integer(out as i64), StrykeValue::integer(new_state as i64)]))
}

// LCG step (numerical recipes constants)
fn builtin_lcg_numrec_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = i1(args) as u64;
    Ok(StrykeValue::integer((s.wrapping_mul(1664525).wrapping_add(1013904223) & 0xffffffff) as i64))
}

// SplitMix64 step
fn builtin_splitmix64_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = i1(args) as u64;
    let mut z = s.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    let out = z ^ (z >> 31);
    Ok(StrykeValue::integer(out as i64))
}

// Wyhash mix
fn builtin_wyhash_mix(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args) as u64;
    let b = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    let r = (a as u128).wrapping_mul(b as u128);
    let lo = r as u64;
    let hi = (r >> 64) as u64;
    Ok(StrykeValue::integer((lo ^ hi) as i64))
}

// CRC-32 (poly 0xedb88320)

// CRC-16 CCITT (poly 0x1021)

// Adler-32

// XOR cipher (returns string of XOR with single key byte)
fn builtin_xor_cipher_byte(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let key = args.get(1).map(|v| v.to_number() as u8).unwrap_or(0);
    let out: Vec<u8> = s.bytes().map(|b| b ^ key).collect();
    Ok(StrykeValue::string(String::from_utf8_lossy(&out).into_owned()))
}

// Caesar cipher

// ROT13

// Rail fence cipher encrypt
fn builtin_railfence_encrypt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let rails = args.get(1).map(|v| v.to_number() as usize).unwrap_or(3).max(1);
    if rails == 1 { return Ok(StrykeValue::string(s)); }
    let mut fence: Vec<Vec<char>> = vec![vec![]; rails];
    let mut row = 0_isize;
    let mut dir: isize = 1;
    for c in s.chars() {
        fence[row as usize].push(c);
        row += dir;
        if row == 0 || row == rails as isize - 1 { dir = -dir; }
    }
    let out: String = fence.into_iter().flat_map(|v| v.into_iter()).collect();
    Ok(StrykeValue::string(out))
}

// Beaufort cipher (key-based, modulo 26)
fn builtin_beaufort(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let key = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let kb: Vec<u8> = key.bytes().filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_uppercase() - b'A').collect();
    if kb.is_empty() { return Ok(StrykeValue::string(s)); }
    let mut k = 0;
    let out: String = s.chars().map(|c| {
        if c.is_ascii_alphabetic() {
            let base = if c.is_ascii_uppercase() { b'A' } else { b'a' };
            let p = c as u8 - base;
            let key_b = kb[k % kb.len()];
            k += 1;
            ((26 + key_b - p) % 26 + base) as char
        } else { c }
    }).collect();
    Ok(StrykeValue::string(out))
}

// Affine cipher: E(x) = (a*x + b) mod 26
fn builtin_affine_encrypt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let a = args.get(1).map(|v| v.to_number() as i64).unwrap_or(5).rem_euclid(26) as u8;
    let b = args.get(2).map(|v| v.to_number() as i64).unwrap_or(8).rem_euclid(26) as u8;
    let out: String = s.chars().map(|c| {
        if c.is_ascii_uppercase() {
            ((a * (c as u8 - b'A') + b) % 26 + b'A') as char
        } else if c.is_ascii_lowercase() {
            ((a * (c as u8 - b'a') + b) % 26 + b'a') as char
        } else { c }
    }).collect();
    Ok(StrykeValue::string(out))
}

// Substitution cipher (key as 26-char string)
fn builtin_substitution_encrypt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let key = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let key_bytes = key.as_bytes();
    if key_bytes.len() < 26 { return Ok(StrykeValue::string(s)); }
    let out: String = s.chars().map(|c| {
        if c.is_ascii_uppercase() { key_bytes[(c as u8 - b'A') as usize].to_ascii_uppercase() as char }
        else if c.is_ascii_lowercase() { key_bytes[(c as u8 - b'a') as usize].to_ascii_lowercase() as char }
        else { c }
    }).collect();
    Ok(StrykeValue::string(out))
}

// Frequency analysis
fn builtin_letter_frequency(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let mut counts = vec![0_i64; 26];
    let mut total = 0_i64;
    for c in s.chars().filter(|c| c.is_ascii_uppercase()) {
        counts[(c as usize) - 'A' as usize] += 1;
        total += 1;
    }
    if total == 0 { return Ok(StrykeValue::array(counts.into_iter().map(StrykeValue::integer).collect())); }
    let out: Vec<StrykeValue> = counts.iter().map(|&c| StrykeValue::float(c as f64 / total as f64)).collect();
    Ok(StrykeValue::array(out))
}

// Chi-squared distance (English freq baseline)
fn builtin_english_chi2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let observed: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let english = [0.0817, 0.0149, 0.0278, 0.0425, 0.1270, 0.0223, 0.0202, 0.0609,
        0.0697, 0.0015, 0.0077, 0.0403, 0.0241, 0.0675, 0.0751, 0.0193,
        0.0010, 0.0599, 0.0633, 0.0906, 0.0276, 0.0098, 0.0236, 0.0015, 0.0197, 0.0007];
    let mut chi = 0.0;
    for i in 0..observed.len().min(26) {
        if english[i] == 0.0 { continue; }
        chi += (observed[i] - english[i]).powi(2) / english[i];
    }
    Ok(StrykeValue::float(chi))
}

// Index of coincidence
fn builtin_index_of_coincidence(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let mut counts = vec![0_i64; 26];
    let mut total = 0_i64;
    for c in s.chars().filter(|c| c.is_ascii_uppercase()) {
        counts[(c as usize) - 'A' as usize] += 1;
        total += 1;
    }
    if total < 2 { return Ok(StrykeValue::float(0.0)); }
    let num: f64 = counts.iter().map(|&c| (c * (c - 1)) as f64).sum();
    Ok(StrykeValue::float(num / (total * (total - 1)) as f64))
}

// Kasiski distance pattern (simplified: count 3-gram repeats)
fn builtin_kasiski_repeats(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let bytes: Vec<u8> = s.bytes().filter(|c| c.is_ascii_alphabetic()).collect();
    if bytes.len() < 6 { return Ok(StrykeValue::integer(0)); }
    let mut count = 0_i64;
    for i in 0..bytes.len() - 3 {
        for j in (i + 3)..bytes.len() - 2 {
            if bytes[i..i+3] == bytes[j..j+3] { count += 1; }
        }
    }
    Ok(StrykeValue::integer(count))
}

// AKS-like deterministic primality (simplified: Miller-Rabin with first 12 witnesses)
fn builtin_deterministic_prime(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n < 2 { return Ok(StrykeValue::integer(0)); }
    if n < 4 { return Ok(StrykeValue::integer(1)); }
    if n % 2 == 0 { return Ok(StrykeValue::integer(0)); }
    let n_u = n as u64;
    let mut d = n_u - 1;
    let mut r = 0_u32;
    while d.is_multiple_of(2) { d /= 2; r += 1; }
    fn mod_pow(mut base: u128, mut exp: u128, modulus: u128) -> u128 {
        let mut result = 1_u128;
        base %= modulus;
        while exp > 0 {
            if exp & 1 != 0 { result = result * base % modulus; }
            exp >>= 1;
            base = base * base % modulus;
        }
        result
    }
    let witnesses = [2_u64, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];
    'outer: for &a in &witnesses {
        if a >= n_u { continue; }
        let mut x = mod_pow(a as u128, d as u128, n_u as u128);
        if x == 1 || x == (n_u - 1) as u128 { continue; }
        for _ in 0..r - 1 {
            x = x * x % n_u as u128;
            if x == (n_u - 1) as u128 { continue 'outer; }
        }
        return Ok(StrykeValue::integer(0));
    }
    Ok(StrykeValue::integer(1))
}

// Pollard rho (simple)
#[allow(dead_code)]
fn builtin_pollard_rho(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    if n <= 1 { return Ok(StrykeValue::integer(n)); }
    if n % 2 == 0 { return Ok(StrykeValue::integer(2)); }
    let mut x = 2_i64;
    let mut y = 2_i64;
    let mut d = 1_i64;
    let g = |v: i64| ((v.wrapping_mul(v)).wrapping_add(1)).rem_euclid(n);
    fn gcd_i(mut a: i64, mut b: i64) -> i64 {
        while b != 0 { let t = b; b = a % b; a = t; }
        a.abs()
    }
    while d == 1 {
        x = g(x);
        y = g(g(y));
        d = gcd_i((x - y).abs(), n);
    }
    if d == n { Ok(StrykeValue::integer(0)) }
    else { Ok(StrykeValue::integer(d)) }
}

// Diffie-Hellman shared key (simplified mod p)
fn builtin_dh_shared(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_pub = i1(args) as i128;
    let b_priv = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1) as i128;
    let p = args.get(2).map(|v| v.to_number() as i64).unwrap_or(23) as i128;
    if p <= 0 { return Ok(StrykeValue::integer(0)); }
    fn pow_mod(mut base: i128, mut exp: i128, modulus: i128) -> i128 {
        let mut result = 1_i128;
        base = base.rem_euclid(modulus);
        while exp > 0 {
            if exp & 1 == 1 { result = result * base % modulus; }
            exp >>= 1;
            base = base * base % modulus;
        }
        result
    }
    Ok(StrykeValue::integer(pow_mod(a_pub, b_priv, p) as i64))
}

// RSA encrypt simple
fn builtin_rsa_encrypt_simple(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = i1(args) as i128;
    let e = args.get(1).map(|v| v.to_number() as i64).unwrap_or(65537) as i128;
    let n = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1) as i128;
    if n <= 0 { return Ok(StrykeValue::integer(0)); }
    fn pow_mod(mut base: i128, mut exp: i128, modulus: i128) -> i128 {
        let mut result = 1_i128;
        base = base.rem_euclid(modulus);
        while exp > 0 {
            if exp & 1 == 1 { result = result * base % modulus; }
            exp >>= 1;
            base = base * base % modulus;
        }
        result
    }
    Ok(StrykeValue::integer(pow_mod(m, e, n) as i64))
}

// PRNG quality: monobit test
fn builtin_monobit_test(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bits: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number() as i64).collect();
    let n = bits.len() as f64;
    if n == 0.0 { return Ok(StrykeValue::float(1.0)); }
    let s: f64 = bits.iter().map(|&b| if b == 0 { -1.0 } else { 1.0 }).sum();
    let s_obs = s.abs() / n.sqrt();
    let p_value = libm::erfc(s_obs / std::f64::consts::SQRT_2);
    Ok(StrykeValue::float(p_value))
}

// Runs test (NIST)

// Pincus's approximate entropy ApEn(m, r): φᵐ(r) − φᵐ⁺¹(r), where
// φᵐ(r) = (n−m+1)⁻¹ Σ_{i=1}^{n−m+1} ln C_i^m(r), and
// C_i^m(r) = (#{j: max_{k<m}|x_{i+k}−x_{j+k}| ≤ r}) / (n−m+1).
// Args: data array, embedding dim m (default 2), tolerance r (default 0.2·σ).
fn builtin_approximate_entropy(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let m = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2).max(1);
    let n = xs.len();
    if n < m + 1 { return Ok(StrykeValue::float(0.0)); }
    let r_user = args.get(2).map(|v| v.to_number());
    let r = r_user.unwrap_or_else(|| {
        let mean: f64 = xs.iter().sum::<f64>() / n as f64;
        let var: f64 = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
        0.2 * var.sqrt()
    });
    fn phi(xs: &[f64], n: usize, m: usize, r: f64) -> f64 {
        let blocks = n - m + 1;
        let mut sum_ln = 0.0_f64;
        for i in 0..blocks {
            let mut count = 0_usize;
            for j in 0..blocks {
                let mut max_d = 0.0_f64;
                for k in 0..m {
                    let d = (xs[i + k] - xs[j + k]).abs();
                    if d > max_d { max_d = d; }
                }
                if max_d <= r { count += 1; }
            }
            sum_ln += (count as f64 / blocks as f64).ln();
        }
        sum_ln / blocks as f64
    }
    Ok(StrykeValue::float(phi(&xs, n, m, r) - phi(&xs, n, m + 1, r)))
}
