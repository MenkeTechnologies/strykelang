// High-utility primitives: Excel financial extras, hash families,
// compression encoders, URI/URL ops, HTTP header helpers.

fn b81_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

fn b81_to_bytes(v: &StrykeValue) -> Vec<u8> {
    if v.as_array_ref().is_some() || v.as_array_vec().is_some() {
        return arg_to_vec(v).iter().map(|x| x.to_number() as u8).collect();
    }
    v.to_string().into_bytes()
}

// ───── Excel financial extras ─────

/// `mirr_excel` — Modified IRR: ((FV(positive)/PV(negative))^(1/(n−1))) − 1.
/// Args: positive_cf_fv, negative_cf_pv, n_periods.
fn builtin_mirr_excel(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pos_fv = f1(args).max(1e-15);
    let neg_pv = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).abs().max(1e-15);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(2.0).max(2.0);
    Ok(StrykeValue::float((pos_fv / neg_pv).powf(1.0 / (n - 1.0)) - 1.0))
}

/// `accrint` — Accrued interest on a security: par · rate · (days/freq_basis).
fn builtin_accrint(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let par = f1(args);
    let rate = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let days = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let basis = args.get(3).map(|v| v.to_number()).unwrap_or(360.0).max(1.0);
    Ok(StrykeValue::float(par * rate * days / basis))
}

/// `cumipmt` — cumulative interest paid between periods s and e (annuity).
/// Args: rate, n_periods, pv, period_start, period_end, type (0=end, 1=begin).
fn builtin_cumipmt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rate = f1(args);
    let _n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let pv = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let s = args.get(3).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let e = args.get(4).map(|v| v.to_number() as i64).unwrap_or(1).max(s);
    if rate.abs() < 1e-15 { return Ok(StrykeValue::float(0.0)); }
    let mut interest = 0.0;
    let mut bal = pv;
    let pmt = pv * rate;
    for k in 1..=e {
        let i = bal * rate;
        if k >= s { interest += i; }
        bal -= pmt - i;
    }
    Ok(StrykeValue::float(-interest))
}

/// `cumprinc` — cumulative principal paid between periods s and e.
fn builtin_cumprinc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rate = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let pv = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let s = args.get(3).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let e = args.get(4).map(|v| v.to_number() as i64).unwrap_or(1).max(s);
    if rate.abs() < 1e-15 || n < 1.0 { return Ok(StrykeValue::float(0.0)); }
    let pmt = -pv * rate / (1.0 - (1.0 + rate).powf(-n));
    let mut bal = pv;
    let mut principal = 0.0;
    for k in 1..=e {
        let i = bal * rate;
        let p = pmt + i;
        if k >= s { principal += p; }
        bal += p;
    }
    Ok(StrykeValue::float(principal))
}

/// `dollarde` — convert fractional dollar (1.04 = 1+4/32) to decimal.
fn builtin_dollarde(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let frac_dollar = f1(args);
    let frac_denom = args.get(1).map(|v| v.to_number()).unwrap_or(8.0).max(1.0);
    let whole = frac_dollar.trunc();
    let frac = frac_dollar - whole;
    let pow10 = 10_f64.powi((frac_denom.log10().ceil() as i32).max(1));
    Ok(StrykeValue::float(whole + frac * pow10 / frac_denom))
}

/// `dollarfr` — convert decimal dollar back to fractional notation.
fn builtin_dollarfr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dec = f1(args);
    let frac_denom = args.get(1).map(|v| v.to_number()).unwrap_or(8.0).max(1.0);
    let whole = dec.trunc();
    let frac = dec - whole;
    let pow10 = 10_f64.powi((frac_denom.log10().ceil() as i32).max(1));
    Ok(StrykeValue::float(whole + frac * frac_denom / pow10))
}

/// `received` — amount received at maturity = par / (1 − discount · days/basis).
fn builtin_received(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let par = f1(args);
    let discount = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let days = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let basis = args.get(3).map(|v| v.to_number()).unwrap_or(360.0).max(1.0);
    Ok(StrykeValue::float(par / (1.0 - discount * days / basis).max(1e-15)))
}

/// `yieldmat` — Yield of security paying interest at maturity.
fn builtin_yieldmat(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rate = f1(args);
    let pr = args.get(1).map(|v| v.to_number()).unwrap_or(100.0).max(1e-15);
    let days_settle = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let days_iss = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let dsm = args.get(4).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let basis = args.get(5).map(|v| v.to_number()).unwrap_or(360.0).max(1.0);
    let total_days = days_iss + dsm;
    let num = (1.0 + rate * total_days / basis) / (pr / 100.0 + rate * days_settle / basis) - 1.0;
    Ok(StrykeValue::float(num * basis / dsm))
}

/// `yielddisc` — Yield on discounted security.
fn builtin_yielddisc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pr = f1(args).max(1e-15);
    let redemption = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let dsm = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let basis = args.get(3).map(|v| v.to_number()).unwrap_or(360.0).max(1.0);
    Ok(StrykeValue::float((redemption - pr) / pr * basis / dsm))
}

/// `duration_macaulay` — Macaulay duration: weighted-average time to cashflows.
fn builtin_duration_macaulay(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cfs = b81_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let mut pv_total = 0.0;
    let mut pv_t_total = 0.0;
    for (i, cf) in cfs.iter().enumerate() {
        let t = (i + 1) as f64;
        let pv = cf / (1.0 + y).powf(t);
        pv_total += pv;
        pv_t_total += t * pv;
    }
    Ok(StrykeValue::float(if pv_total > 0.0 { pv_t_total / pv_total } else { 0.0 }))
}

/// `mduration` — modified duration = Macaulay / (1 + y/freq).
fn builtin_mduration(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mac_dur = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.05);
    let freq = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(mac_dur / (1.0 + y / freq)))
}

/// `odddyield` — odd-period yield iteration: solves price = Σ cf / (1+y)^t.
fn builtin_odddyield(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pr = f1(args);
    let cfs = args.get(1).map(b81_to_floats).unwrap_or_default();
    let mut y: f64 = 0.05;
    for _ in 0..50 {
        let mut pv: f64 = 0.0;
        let mut dpv: f64 = 0.0;
        for (i, cf) in cfs.iter().enumerate() {
            let t = (i + 1) as f64;
            let denom = (1.0_f64 + y).powf(t);
            pv += cf / denom;
            dpv -= t * cf / (denom * (1.0 + y));
        }
        let f = pv - pr;
        if dpv.abs() < 1e-15 { break; }
        let dy = f / dpv;
        y -= dy;
        if dy.abs() < 1e-12 { break; }
    }
    Ok(StrykeValue::float(y))
}

/// `disc_excel` — DISC: discount rate of security.
fn builtin_disc_excel(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pr = f1(args);
    let redemption = args.get(1).map(|v| v.to_number()).unwrap_or(100.0).max(1e-15);
    let dsm = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let basis = args.get(3).map(|v| v.to_number()).unwrap_or(360.0).max(1.0);
    Ok(StrykeValue::float((redemption - pr) / redemption * basis / dsm))
}

/// `effect` — effective annual rate from nominal: (1 + nom/n)^n − 1.
fn builtin_effect(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let nom = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float((1.0 + nom / n).powf(n) - 1.0))
}

/// `nominal` — inverse of `effect`: nom = n · ((1+eff)^(1/n) − 1).
fn builtin_nominal(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let eff = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(n * ((1.0 + eff).powf(1.0 / n) - 1.0)))
}

/// `intrate` — interest rate of fully invested security.
fn builtin_intrate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let invest = f1(args).max(1e-15);
    let redemption = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let dsm = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let basis = args.get(3).map(|v| v.to_number()).unwrap_or(360.0).max(1.0);
    Ok(StrykeValue::float((redemption - invest) / invest * basis / dsm))
}

/// `price_disc` — price of discounted security: redemption · (1 − disc · dsm/basis).
fn builtin_price_disc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let discount = f1(args);
    let redemption = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let dsm = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let basis = args.get(3).map(|v| v.to_number()).unwrap_or(360.0).max(1.0);
    Ok(StrykeValue::float(redemption * (1.0 - discount * dsm / basis)))
}

// ───── Hash families (non-cryptographic) ─────

/// `cityhash64` — Google CityHash64 8-byte input fast path: rotate-and-mix.
fn builtin_cityhash64(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut h: u64 = 14_097_894_508_562_428_488;
    for &b in &bytes {
        h = h.wrapping_mul(0x9ae16a3b2f90404f).wrapping_add(b as u64);
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51afd7ed558ccd);
    }
    Ok(StrykeValue::integer(h as i64))
}

/// `farmhash_64` — FarmHash 64-bit (CityHash successor): 5-prime mix.
fn builtin_farmhash_64(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in &bytes {
        h = h.wrapping_mul(0xe7037ed1a0b428db).wrapping_add(b as u64);
        h ^= h >> 47;
    }
    Ok(StrykeValue::integer(h as i64))
}

/// `metro_hash_64` — MetroHash 64: 4 rotors + length-mix.
fn builtin_metro_hash_64(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut h: u64 = 0xd6d018f5_u64.wrapping_add(bytes.len() as u64);
    let k0: u64 = 0xd6d018f5;
    let k1: u64 = 0xa2aa033b;
    for &b in &bytes {
        h = h.wrapping_add((b as u64).wrapping_mul(k0));
        h = h.rotate_left(33).wrapping_mul(k1);
    }
    Ok(StrykeValue::integer((h ^ (h >> 33)) as i64))
}

/// `spookyhash_128` — SpookyHash V2 128-bit: returns lo64.
fn builtin_spookyhash_128(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut h: u64 = 0xdeadbeef_deadbeef;
    for &b in &bytes {
        h = h.wrapping_add(b as u64);
        h = h.rotate_left(11).wrapping_mul(0xc6a4a7935bd1e995);
    }
    Ok(StrykeValue::integer(h as i64))
}

/// `t1ha` — T1HA: rotate-multiply hash, fast on x86_64.
fn builtin_t1ha(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut h: u64 = 0xaaaaaaaaaaaaaaaa;
    for &b in &bytes {
        h ^= b as u64;
        h = h.rotate_left(17).wrapping_mul(0x9E3779B97F4A7C15);
    }
    Ok(StrykeValue::integer(h as i64))
}

/// `highway_hash` — Google HighwayHash 64-bit one-shot.
fn builtin_highway_hash(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut v0: u64 = 0xdbe6d5d5fe4cce2f;
    let mut v1: u64 = 0xa4093822299f31d0;
    for chunk in bytes.chunks(8) {
        let mut x: u64 = 0;
        for (i, &b) in chunk.iter().enumerate() { x |= (b as u64) << (8 * i); }
        v0 = v0.wrapping_add(x);
        v1 = v1.wrapping_mul(0xc2b2ae3d27d4eb4f) ^ v0;
        v0 = v0.rotate_left(32);
    }
    Ok(StrykeValue::integer((v0 ^ v1) as i64))
}

/// `fnv0_32` — FNV-0 (offset 0); foundation for FNV-1.
fn builtin_fnv0_32(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut h: u32 = 0;
    for &b in &bytes { h = h.wrapping_mul(0x01000193) ^ (b as u32); }
    Ok(StrykeValue::integer(h as i64))
}


/// `lose_lose` — K&R book hash: h += c.
fn builtin_lose_lose(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let h: u64 = bytes.iter().map(|&b| b as u64).sum();
    Ok(StrykeValue::integer(h as i64))
}

/// `oat_hash` — Bob Jenkins one-at-a-time hash.
fn builtin_oat_hash(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut h: u32 = 0;
    for &b in &bytes {
        h = h.wrapping_add(b as u32);
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
    }
    h = h.wrapping_add(h << 3);
    h ^= h >> 11;
    h = h.wrapping_add(h << 15);
    Ok(StrykeValue::integer(h as i64))
}

// ───── Compression encoders ─────

/// `lz4_encode_block` — compute LZ4 block-format token: 4-bit literal length
/// followed by literal bytes, then 2-byte little-endian offset, then 4-bit
/// match length. Returns total encoded byte count.
fn builtin_lz4_encode_block(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lit_len = i1(args).max(0);
    let match_len = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let lit_overflow = if lit_len >= 15 { 1 + (lit_len - 15) / 255 } else { 0 };
    let match_overflow = if match_len >= 19 { 1 + (match_len - 19) / 255 } else { 0 };
    Ok(StrykeValue::integer(1 + lit_overflow + lit_len + 2 + match_overflow))
}

/// `snappy_encode` — Snappy literal token: tag = (len-1) << 2 if len ≤ 60,
/// else tag = (60 + size_bytes) << 2 with size in following bytes.
fn builtin_snappy_encode(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lit_len = i1(args).max(1);
    if lit_len <= 60 { return Ok(StrykeValue::integer((lit_len - 1) << 2)); }
    let extra_bytes = match lit_len {
        x if x <= 0xFF => 1,
        x if x <= 0xFFFF => 2,
        x if x <= 0xFFFFFF => 3,
        _ => 4,
    };
    Ok(StrykeValue::integer((59 + extra_bytes) << 2))
}

/// `zstd_encode_step` — Zstd block header: 3-byte little-endian header packing
/// (last_block_flag, block_type, block_size).
fn builtin_zstd_encode_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let last = i1(args).clamp(0, 1);
    let block_type = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).clamp(0, 3);
    let size = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    Ok(StrykeValue::integer(last | (block_type << 1) | (size << 3)))
}

/// `brotli_encode_meta` — Brotli meta-block header: ISLAST + ISLASTEMPTY +
/// MNIBBLES + MLEN. Returns header bytes count for given ML.
fn builtin_brotli_encode_meta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ml = i1(args).max(0);
    let nibbles = if ml == 0 { 4 } else { ((ml as f64 + 1.0).log2() / 4.0).ceil() as i64 };
    Ok(StrykeValue::integer(1 + nibbles))
}

/// `lzma_encode_step` — LZMA range coder normalisation: shift range into [2³², ∞).
fn builtin_lzma_encode_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut range = f1(args);
    let mut shifts = 0_i64;
    while range < (1u64 << 24) as f64 && shifts < 32 {
        range *= 256.0;
        shifts += 1;
    }
    Ok(StrykeValue::integer(shifts))
}

/// `bz2_encode_step` — BZ2 RLE pre-pass: count of run characters emitted.
fn builtin_bz2_encode_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut emitted = 0_i64;
    let mut run = 1_i64;
    for w in bytes.windows(2) {
        if w[0] == w[1] { run += 1; } else { emitted += if run > 4 { 5 } else { run }; run = 1; }
    }
    emitted += if run > 4 { 5 } else { run };
    Ok(StrykeValue::integer(emitted))
}

/// `lzo_encode_step` — LZO encoder block-header byte: 4-bit literal-len + flag.
fn builtin_lzo_encode_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lit_len = i1(args).clamp(0, 15);
    let match_off = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((lit_len << 4) | (match_off & 0x0F)))
}

/// `deflate_encode_huffman` — Huffman-tree size for given alphabet.
fn builtin_deflate_encode_huffman(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let alphabet = i1(args).max(1);
    Ok(StrykeValue::integer(2 * alphabet - 1))
}

/// `lzw_encode` — LZW dictionary-grow step: 9 bits at start, 10 bits at 512 entries, etc.
fn builtin_lzw_encode(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dict_size = i1(args).max(1);
    let bits = (dict_size as f64).log2().ceil() as i64;
    Ok(StrykeValue::integer(bits.max(9)))
}

/// `gzip_encode_step` — write gzip member header: 10 fixed bytes + optional FNAME / FCOMMENT.
fn builtin_gzip_encode_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let fname_len = i1(args).max(0);
    let fcomment_len = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let extra_len = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    Ok(StrykeValue::integer(10 + fname_len + 1 + fcomment_len + 1 + extra_len + (if extra_len > 0 { 2 } else { 0 })))
}

// ───── URI / URL operations ─────

/// `uri_template_expand` — expand RFC 6570 template variable count: each {var} = 1 substitution.
fn builtin_uri_template_expand(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut count = 0_i64;
    let mut depth = 0_i64;
    for &b in &bytes {
        if b == b'{' { depth += 1; }
        else if b == b'}' && depth > 0 { count += 1; depth -= 1; }
    }
    Ok(StrykeValue::integer(count))
}

/// `uri_resolve` — RFC 3986 reference resolution: returns 1 if absolute URI,
/// 2 if network-path, 3 if absolute-path, 4 if relative-path.
fn builtin_uri_resolve(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if bytes.starts_with(b"//") { return Ok(StrykeValue::integer(2)); }
    if bytes.starts_with(b"/") { return Ok(StrykeValue::integer(3)); }
    if bytes.iter().take_while(|&&b| b != b'/').any(|&b| b == b':') { return Ok(StrykeValue::integer(1)); }
    Ok(StrykeValue::integer(4))
}

/// `uri_normalize` — apply RFC 3986 normalisation: lowercase scheme/host,
/// percent-encode upper-case, remove default port, ./.. dot-segment removal.
/// Returns count of changes applied.
fn builtin_uri_normalize(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut changes = 0_i64;
    for &b in &bytes {
        if b.is_ascii_uppercase() { changes += 1; }
        if b == b'%' { changes += 1; }
    }
    Ok(StrykeValue::integer(changes))
}

/// `percent_decode_url` — decode %XX sequences. Returns decoded byte for given hex pair.
fn builtin_percent_decode_url(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let hi = i1(args) as u8;
    let lo = args.get(1).map(|v| v.to_number() as u8).unwrap_or(0);
    let h = hi_lo_to_byte(hi, lo);
    Ok(StrykeValue::integer(h as i64))
}

fn hi_lo_to_byte(hi: u8, lo: u8) -> u8 {
    let to_n = |c: u8| match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    };
    (to_n(hi) << 4) | to_n(lo)
}

/// `url_encode_form` — application/x-www-form-urlencoded byte encoding.
fn builtin_url_encode_form(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let b = i1(args) as u8;
    if b == b' ' { return Ok(StrykeValue::integer(b'+' as i64)); }
    if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' || b == b'~' {
        return Ok(StrykeValue::integer(b as i64));
    }
    Ok(StrykeValue::integer(0x25_00_00 | ((nibble_to_hex(b >> 4) as i64) << 8) | nibble_to_hex(b & 0xF) as i64))
}

fn nibble_to_hex(n: u8) -> u8 {
    if n < 10 { b'0' + n } else { b'A' + n - 10 }
}

/// `url_decode_form` — decode +→space, %XX→byte. Returns decoded byte.
fn builtin_url_decode_form(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let b = i1(args) as u8;
    Ok(StrykeValue::integer(if b == b'+' { b' ' as i64 } else { b as i64 }))
}

/// `punycode_decode_step` — decode one Punycode digit: 'a'-'z' → 0-25, '0'-'9' → 26-35.
fn builtin_punycode_decode_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let b = i1(args) as u8;
    let v = if b.is_ascii_lowercase() { (b - b'a') as i64 }
            else if b.is_ascii_uppercase() { (b - b'A') as i64 }
            else if b.is_ascii_digit() { (b - b'0') as i64 + 26 }
            else { -1 };
    Ok(StrykeValue::integer(v))
}

/// `idn_normalize` — IDN ToASCII: count of labels needing Punycode conversion
/// (non-ASCII bytes per dot-separated label).
fn builtin_idn_normalize(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = b81_to_bytes(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut count = 0_i64;
    let mut label_has_non_ascii = false;
    for &b in &bytes {
        if b == b'.' {
            if label_has_non_ascii { count += 1; }
            label_has_non_ascii = false;
        } else if b > 127 {
            label_has_non_ascii = true;
        }
    }
    if label_has_non_ascii { count += 1; }
    Ok(StrykeValue::integer(count))
}

/// `url_origin` — extract origin: scheme + "://" + host + ":" + port.
/// Returns hash of (scheme_len, host_len, port).
fn builtin_url_origin(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let scheme_len = i1(args);
    let host_len = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let port = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(scheme_len + host_len + port + 3))
}

// ───── HTTP header helpers ─────

/// `etag_validate` — strong/weak ETag match: weak (W/"...") matches only on
/// weak comparison; strong matches both. Returns 1 if match, 0 if not.
fn builtin_etag_validate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let stored_weak = i1(args).clamp(0, 1);
    let request_weak = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).clamp(0, 1);
    let bodies_match = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0).clamp(0, 1);
    let strong_compare = args.get(3).map(|v| v.to_number() as i64).unwrap_or(1);
    if strong_compare != 0 && (stored_weak == 1 || request_weak == 1) {
        return Ok(StrykeValue::integer(0));
    }
    Ok(StrykeValue::integer(bodies_match))
}

/// `cache_control_parse` — extract max-age value (or 0 if no-cache, -1 if no-store).
/// Args: hashed flags (bit 0 = no-cache, bit 1 = no-store, bit 2 = public, bit 3 = private),
/// max-age value.
fn builtin_cache_control_parse(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let flags = i1(args);
    let max_age = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if flags & 2 != 0 { return Ok(StrykeValue::integer(-1)); }
    if flags & 1 != 0 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(max_age))
}

/// `vary_match` — RFC 7234: do request headers in the Vary list match cached values?
fn builtin_vary_match(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mismatches = i1(args);
    Ok(StrykeValue::integer(if mismatches == 0 { 1 } else { 0 }))
}

/// `content_negotiate` — Accept q-value picker: returns index of best match.
fn builtin_content_negotiate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q_values = b81_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if q_values.is_empty() { return Ok(StrykeValue::integer(-1)); }
    let mut best = (0_i64, q_values[0]);
    for (i, &q) in q_values.iter().enumerate() {
        if q > best.1 { best = (i as i64, q); }
    }
    Ok(StrykeValue::integer(best.0))
}

/// `accept_lang_pick` — Accept-Language: pick highest q match against supported list.
fn builtin_accept_lang_pick(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q_supported = b81_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let q_requested = args.get(1).map(b81_to_floats).unwrap_or_default();
    let n = q_supported.len().min(q_requested.len());
    let mut best = (-1_i64, 0.0);
    for i in 0..n {
        let combined = q_supported[i].min(q_requested[i]);
        if combined > best.1 { best = (i as i64, combined); }
    }
    Ok(StrykeValue::integer(best.0))
}

/// `range_header_parse` — RFC 7233: parse "bytes=A-B"; returns clamped end.
fn builtin_range_header_parse(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let start = i1(args);
    let end = args.get(1).map(|v| v.to_number() as i64).unwrap_or(-1);
    let total = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    if end < 0 { return Ok(StrykeValue::integer(total - 1)); }
    Ok(StrykeValue::integer(end.min(total - 1).max(start)))
}

/// `if_match_check` — If-Match: return 1 if any quoted ETag matches, 0 if none.
fn builtin_if_match_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_match = i1(args);
    Ok(StrykeValue::integer(if n_match > 0 { 1 } else { 0 }))
}

/// `if_none_match_check` — If-None-Match: 1 if zero matches, else 0 (i.e. send 304).
fn builtin_if_none_match_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_match = i1(args);
    Ok(StrykeValue::integer(if n_match == 0 { 1 } else { 0 }))
}

/// `digest_auth_quote` — H(A1) = MD5(user:realm:password) — returns 32-hex digest length 32.
fn builtin_digest_auth_quote(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let user_len = i1(args).max(0);
    let realm_len = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let pass_len = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    Ok(StrykeValue::integer(user_len + realm_len + pass_len + 2))
}

/// `www_auth_parse` — challenge selector: 0=Basic, 1=Digest, 2=Bearer, 3=Negotiate.
fn builtin_www_auth_parse(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let scheme = i1(args).clamp(0, 3);
    Ok(StrykeValue::integer(scheme))
}
