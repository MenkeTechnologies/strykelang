// Batch 26 — cryptography deep: hash mixers, KDFs, PRNGs, ciphers, primality.

// FNV-1a 32-bit
fn builtin_fnv1a_32(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut h = 2166136261_u32;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    Ok(PerlValue::integer(h as i64))
}
// FNV-1a 64-bit
fn builtin_fnv1a_64(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut h = 14695981039346656037_u64;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    Ok(PerlValue::integer(h as i64))
}
// DJB2 hash
fn builtin_djb2_hash_b26(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut h = 5381_u32;
    for b in s.bytes() {
        h = h.wrapping_mul(33).wrapping_add(b as u32);
    }
    Ok(PerlValue::integer(h as i64))
}
// SDBM hash
fn builtin_sdbm_hash(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut h = 0_u32;
    for b in s.bytes() {
        h = (b as u32).wrapping_add(h.wrapping_shl(6)).wrapping_add(h.wrapping_shl(16)).wrapping_sub(h);
    }
    Ok(PerlValue::integer(h as i64))
}
// MurmurHash3 x86_32 (one-shot)
#[allow(dead_code)]
fn builtin_murmur3_32(args: &[PerlValue]) -> PerlResult<PerlValue> {
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
    Ok(PerlValue::integer(h1 as i64))
}

// xxHash32 (one-shot, simplified)
#[allow(dead_code)]
fn builtin_xxhash32(args: &[PerlValue]) -> PerlResult<PerlValue> {
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
    Ok(PerlValue::integer(h32 as i64))
}

// SipHash24 (simplified one-shot, 64-bit)
fn builtin_siphash24(args: &[PerlValue]) -> PerlResult<PerlValue> {
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
    Ok(PerlValue::integer((v0 ^ v1 ^ v2 ^ v3) as i64))
}

// PBKDF2-HMAC-SHA1 (simplified — single iteration of XOR + concat)
fn builtin_pbkdf2_hmac_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pw = args.first().map(|v| v.to_string()).unwrap_or_default();
    let salt = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let iters = args.get(2).map(|v| v.to_number() as usize).unwrap_or(1);
    let mut h = 5381_u64;
    for b in pw.bytes().chain(salt.bytes()) {
        h = h.wrapping_mul(33).wrapping_add(b as u64);
    }
    for _ in 0..iters {
        h = h.wrapping_mul(0x100000001b3).rotate_left(7);
    }
    Ok(PerlValue::integer(h as i64))
}

// Scrypt salsa20/8 word mixer (single round)
fn builtin_scrypt_round(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<u32> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number() as u32).collect();
    if xs.len() < 16 { return Ok(PerlValue::array(xs.into_iter().map(|v| PerlValue::integer(v as i64)).collect())); }
    let mut x = [0_u32; 16];
    x.copy_from_slice(&xs[..16]);
    for _ in 0..4 {
        x[4] ^= x[0].wrapping_add(x[12]).rotate_left(7);
        x[8] ^= x[4].wrapping_add(x[0]).rotate_left(9);
        x[12] ^= x[8].wrapping_add(x[4]).rotate_left(13);
        x[0] ^= x[12].wrapping_add(x[8]).rotate_left(18);
    }
    Ok(PerlValue::array(x.iter().map(|&v| PerlValue::integer(v as i64)).collect()))
}

// Bcrypt-style cost (just iterations)
fn builtin_bcrypt_cost_iters(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cost = i1(args).clamp(4, 31) as u32;
    Ok(PerlValue::integer(1_i64 << cost))
}

// Argon2 memory mixer (placeholder — single XOR over blocks)
fn builtin_argon2_block_mix(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let xs: Vec<u64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number() as u64).collect();
    let ys: Vec<u64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number() as u64).collect();
    let len = xs.len().max(ys.len());
    let out: Vec<PerlValue> = (0..len).map(|i| {
        let a = *xs.get(i).unwrap_or(&0);
        let b = *ys.get(i).unwrap_or(&0);
        PerlValue::integer((a ^ b) as i64)
    }).collect();
    Ok(PerlValue::array(out))
}

// HKDF expand step (one block)
fn builtin_hkdf_expand_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prk = args.first().map(|v| v.to_string()).unwrap_or_default();
    let info = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let counter = args.get(2).map(|v| v.to_number() as u8).unwrap_or(1);
    let mut h = 0_u64;
    for b in prk.bytes().chain(info.bytes()).chain(std::iter::once(counter)) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    Ok(PerlValue::integer(h as i64))
}

// Linear feedback shift register (Galois LFSR) step
fn builtin_lfsr_galois_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let state = i1(args) as u64;
    let mask = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0xb400);
    let next = if state & 1 != 0 { (state >> 1) ^ mask } else { state >> 1 };
    Ok(PerlValue::integer(next as i64))
}

// Mersenne Twister mt19937 next (32 bits) — single-step from seeded state
fn builtin_mt19937_temper(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut y = i1(args) as u32;
    y ^= y >> 11;
    y ^= (y << 7) & 0x9d2c5680;
    y ^= (y << 15) & 0xefc60000;
    y ^= y >> 18;
    Ok(PerlValue::integer(y as i64))
}

// xorshift64
fn builtin_xorshift64(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut x = i1(args) as u64;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    Ok(PerlValue::integer(x as i64))
}
// xorshift32
fn builtin_xorshift32(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut x = i1(args) as u32;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    Ok(PerlValue::integer(x as i64))
}
// PCG32 step
fn builtin_pcg32_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let state = i1(args) as u64;
    let inc = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0xda3e39cb94b95bdb);
    let new_state = state.wrapping_mul(6364136223846793005).wrapping_add(inc | 1);
    let xorshifted = (((new_state >> 18) ^ new_state) >> 27) as u32;
    let rot = (new_state >> 59) as u32;
    let out = xorshifted.rotate_right(rot);
    Ok(PerlValue::array(vec![PerlValue::integer(out as i64), PerlValue::integer(new_state as i64)]))
}

// LCG step (numerical recipes constants)
fn builtin_lcg_numrec_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = i1(args) as u64;
    Ok(PerlValue::integer((s.wrapping_mul(1664525).wrapping_add(1013904223) & 0xffffffff) as i64))
}

// SplitMix64 step
fn builtin_splitmix64_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = i1(args) as u64;
    let mut z = s.wrapping_add(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    let out = z ^ (z >> 31);
    Ok(PerlValue::integer(out as i64))
}

// Wyhash mix
fn builtin_wyhash_mix(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = i1(args) as u64;
    let b = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    let r = (a as u128).wrapping_mul(b as u128);
    let lo = r as u64;
    let hi = (r >> 64) as u64;
    Ok(PerlValue::integer((lo ^ hi) as i64))
}

// CRC-32 (poly 0xedb88320)
fn builtin_crc32_b26(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut crc = 0xffffffff_u32;
    for b in s.bytes() {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb88320 } else { crc >> 1 };
        }
    }
    Ok(PerlValue::integer((crc ^ 0xffffffff) as i64))
}

// CRC-16 CCITT (poly 0x1021)
fn builtin_crc16_ccitt_b26(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut crc = 0xffff_u16;
    for b in s.bytes() {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    Ok(PerlValue::integer(crc as i64))
}

// Adler-32
fn builtin_adler32_b26(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut a = 1_u32;
    let mut b = 0_u32;
    for byte in s.bytes() {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    Ok(PerlValue::integer(((b << 16) | a) as i64))
}

// XOR cipher (returns string of XOR with single key byte)
fn builtin_xor_cipher_byte(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let key = args.get(1).map(|v| v.to_number() as u8).unwrap_or(0);
    let out: Vec<u8> = s.bytes().map(|b| b ^ key).collect();
    Ok(PerlValue::string(String::from_utf8_lossy(&out).into_owned()))
}

// Caesar cipher
fn builtin_caesar_b26(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let shift = (args.get(1).map(|v| v.to_number() as i64).unwrap_or(3).rem_euclid(26)) as u8;
    let out: String = s.chars().map(|c| {
        if c.is_ascii_uppercase() { ((c as u8 - b'A' + shift) % 26 + b'A') as char }
        else if c.is_ascii_lowercase() { ((c as u8 - b'a' + shift) % 26 + b'a') as char }
        else { c }
    }).collect();
    Ok(PerlValue::string(out))
}

// ROT13
fn builtin_rot13_b26(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let out: String = s.chars().map(|c| {
        if c.is_ascii_uppercase() { ((c as u8 - b'A' + 13) % 26 + b'A') as char }
        else if c.is_ascii_lowercase() { ((c as u8 - b'a' + 13) % 26 + b'a') as char }
        else { c }
    }).collect();
    Ok(PerlValue::string(out))
}

// Rail fence cipher encrypt
fn builtin_railfence_encrypt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let rails = args.get(1).map(|v| v.to_number() as usize).unwrap_or(3).max(1);
    if rails == 1 { return Ok(PerlValue::string(s)); }
    let mut fence: Vec<Vec<char>> = vec![vec![]; rails];
    let mut row = 0_isize;
    let mut dir: isize = 1;
    for c in s.chars() {
        fence[row as usize].push(c);
        row += dir;
        if row == 0 || row == rails as isize - 1 { dir = -dir; }
    }
    let out: String = fence.into_iter().flat_map(|v| v.into_iter()).collect();
    Ok(PerlValue::string(out))
}

// Beaufort cipher (key-based, modulo 26)
fn builtin_beaufort(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let key = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let kb: Vec<u8> = key.bytes().filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_uppercase() - b'A').collect();
    if kb.is_empty() { return Ok(PerlValue::string(s)); }
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
    Ok(PerlValue::string(out))
}

// Affine cipher: E(x) = (a*x + b) mod 26
fn builtin_affine_encrypt(args: &[PerlValue]) -> PerlResult<PerlValue> {
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
    Ok(PerlValue::string(out))
}

// Substitution cipher (key as 26-char string)
fn builtin_substitution_encrypt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let key = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let key_bytes = key.as_bytes();
    if key_bytes.len() < 26 { return Ok(PerlValue::string(s)); }
    let out: String = s.chars().map(|c| {
        if c.is_ascii_uppercase() { key_bytes[(c as u8 - b'A') as usize].to_ascii_uppercase() as char }
        else if c.is_ascii_lowercase() { key_bytes[(c as u8 - b'a') as usize].to_ascii_lowercase() as char }
        else { c }
    }).collect();
    Ok(PerlValue::string(out))
}

// Frequency analysis
fn builtin_letter_frequency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let mut counts = vec![0_i64; 26];
    let mut total = 0_i64;
    for c in s.chars().filter(|c| c.is_ascii_uppercase()) {
        counts[(c as usize) - 'A' as usize] += 1;
        total += 1;
    }
    if total == 0 { return Ok(PerlValue::array(counts.into_iter().map(PerlValue::integer).collect())); }
    let out: Vec<PerlValue> = counts.iter().map(|&c| PerlValue::float(c as f64 / total as f64)).collect();
    Ok(PerlValue::array(out))
}

// Chi-squared distance (English freq baseline)
fn builtin_english_chi2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let observed: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let english = [0.0817, 0.0149, 0.0278, 0.0425, 0.1270, 0.0223, 0.0202, 0.0609,
        0.0697, 0.0015, 0.0077, 0.0403, 0.0241, 0.0675, 0.0751, 0.0193,
        0.0010, 0.0599, 0.0633, 0.0906, 0.0276, 0.0098, 0.0236, 0.0015, 0.0197, 0.0007];
    let mut chi = 0.0;
    for i in 0..observed.len().min(26) {
        if english[i] == 0.0 { continue; }
        chi += (observed[i] - english[i]).powi(2) / english[i];
    }
    Ok(PerlValue::float(chi))
}

// Index of coincidence
fn builtin_index_of_coincidence(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let mut counts = vec![0_i64; 26];
    let mut total = 0_i64;
    for c in s.chars().filter(|c| c.is_ascii_uppercase()) {
        counts[(c as usize) - 'A' as usize] += 1;
        total += 1;
    }
    if total < 2 { return Ok(PerlValue::float(0.0)); }
    let num: f64 = counts.iter().map(|&c| (c * (c - 1)) as f64).sum();
    Ok(PerlValue::float(num / (total * (total - 1)) as f64))
}

// Kasiski distance pattern (simplified: count 3-gram repeats)
fn builtin_kasiski_repeats(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let bytes: Vec<u8> = s.bytes().filter(|c| c.is_ascii_alphabetic()).collect();
    if bytes.len() < 6 { return Ok(PerlValue::integer(0)); }
    let mut count = 0_i64;
    for i in 0..bytes.len() - 3 {
        for j in (i + 3)..bytes.len() - 2 {
            if bytes[i..i+3] == bytes[j..j+3] { count += 1; }
        }
    }
    Ok(PerlValue::integer(count))
}

// AKS-like deterministic primality (simplified: Miller-Rabin with first 12 witnesses)
fn builtin_deterministic_prime(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if n < 2 { return Ok(PerlValue::integer(0)); }
    if n < 4 { return Ok(PerlValue::integer(1)); }
    if n % 2 == 0 { return Ok(PerlValue::integer(0)); }
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
        return Ok(PerlValue::integer(0));
    }
    Ok(PerlValue::integer(1))
}

// Pollard rho (simple)
#[allow(dead_code)]
fn builtin_pollard_rho(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    if n <= 1 { return Ok(PerlValue::integer(n)); }
    if n % 2 == 0 { return Ok(PerlValue::integer(2)); }
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
    if d == n { Ok(PerlValue::integer(0)) }
    else { Ok(PerlValue::integer(d)) }
}

// Diffie-Hellman shared key (simplified mod p)
fn builtin_dh_shared(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_pub = i1(args) as i128;
    let b_priv = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1) as i128;
    let p = args.get(2).map(|v| v.to_number() as i64).unwrap_or(23) as i128;
    if p <= 0 { return Ok(PerlValue::integer(0)); }
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
    Ok(PerlValue::integer(pow_mod(a_pub, b_priv, p) as i64))
}

// RSA encrypt simple
fn builtin_rsa_encrypt_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = i1(args) as i128;
    let e = args.get(1).map(|v| v.to_number() as i64).unwrap_or(65537) as i128;
    let n = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1) as i128;
    if n <= 0 { return Ok(PerlValue::integer(0)); }
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
    Ok(PerlValue::integer(pow_mod(m, e, n) as i64))
}

// PRNG quality: monobit test
fn builtin_monobit_test(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bits: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number() as i64).collect();
    let n = bits.len() as f64;
    if n == 0.0 { return Ok(PerlValue::float(1.0)); }
    let s: f64 = bits.iter().map(|&b| if b == 0 { -1.0 } else { 1.0 }).sum();
    let s_obs = s.abs() / n.sqrt();
    let p_value = libm::erfc(s_obs / std::f64::consts::SQRT_2);
    Ok(PerlValue::float(p_value))
}

// Runs test (NIST)
fn builtin_runs_test_b26(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bits: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number() as i64).collect();
    let n = bits.len();
    if n < 2 { return Ok(PerlValue::float(1.0)); }
    let pi: f64 = bits.iter().filter(|&&b| b != 0).count() as f64 / n as f64;
    if (pi - 0.5).abs() >= 2.0 / (n as f64).sqrt() { return Ok(PerlValue::float(0.0)); }
    let mut v = 1_usize;
    for i in 1..n {
        if bits[i] != bits[i - 1] { v += 1; }
    }
    let num = (v as f64 - 2.0 * n as f64 * pi * (1.0 - pi)).abs();
    let den = 2.0 * (2.0 * n as f64).sqrt() * pi * (1.0 - pi);
    if den == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(libm::erfc(num / den)))
}

// Approximate entropy (very simplified)
fn builtin_approximate_entropy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bits: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter().map(|v| v.to_number() as i64).collect();
    let m = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2);
    let n = bits.len();
    if n < m { return Ok(PerlValue::float(0.0)); }
    let mut counts: std::collections::HashMap<Vec<i64>, usize> = std::collections::HashMap::new();
    for i in 0..=n - m {
        let pat = bits[i..i+m].to_vec();
        *counts.entry(pat).or_insert(0) += 1;
    }
    let total = (n - m + 1) as f64;
    let h: f64 = counts.values().map(|&c| {
        let p = c as f64 / total;
        -p * p.ln()
    }).sum();
    Ok(PerlValue::float(h))
}
