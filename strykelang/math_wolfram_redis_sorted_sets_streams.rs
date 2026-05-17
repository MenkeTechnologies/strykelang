// Redis-flavour primitives: sorted sets, hashes, lists, sets,
// expiration, hyperloglog, geo, streams.

fn b71_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

// ───── sorted set ─────

/// ZADD key member score — returns 1 if newly added, 0 if updated.
fn builtin_zadd(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let existed = i1(args);
    Ok(StrykeValue::integer(if existed != 0 { 0 } else { 1 }))
}

/// ZREM — count removed.
fn builtin_zrem(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let removed = i1(args);
    Ok(StrykeValue::integer(removed.max(0)))
}

/// ZRANGEBYSCORE — count in [min, max] inclusive.
fn builtin_zrangebyscore(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let scores = b71_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let min = args.get(1).map(|v| v.to_number()).unwrap_or(f64::NEG_INFINITY);
    let max = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::integer(scores.iter().filter(|&&s| s >= min && s <= max).count() as i64))
}

/// ZRANK — rank (0-based ascending) or -1 if missing.
fn builtin_zrank(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut scores = b71_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let target = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    scores.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    match scores.iter().position(|&s| s == target) {
        Some(i) => Ok(StrykeValue::integer(i as i64)),
        None => Ok(StrykeValue::integer(-1)),
    }
}

/// ZREVRANK — descending rank.
fn builtin_zrevrank(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut scores = b71_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let target = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    scores.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    match scores.iter().position(|&s| s == target) {
        Some(i) => Ok(StrykeValue::integer(i as i64)),
        None => Ok(StrykeValue::integer(-1)),
    }
}

/// ZINCRBY — return new score.
fn builtin_zincrby(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cur = f1(args);
    let inc = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(cur + inc))
}

/// ZCARD — cardinality.
fn builtin_zcard(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let scores = b71_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(scores.len() as i64))
}

/// ZCOUNT — same as ZRANGEBYSCORE count.
fn builtin_zcount(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_zrangebyscore(args)
}

/// ZLEXCOUNT — count of members between two lex bounds (treat as ints here).
fn builtin_zlexcount(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let scores = b71_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let min = args.get(1).map(|v| v.to_number()).unwrap_or(f64::NEG_INFINITY);
    let max = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::integer(scores.iter().filter(|&&s| s >= min && s < max).count() as i64))
}

// ───── list ─────

/// LPUSH — new length.
fn builtin_lpush(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let len = i1(args);
    let added = (args.len() as i64 - 1).max(0);
    Ok(StrykeValue::integer(len + added))
}

/// RPUSH — new length.
fn builtin_rpush(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_lpush(args)
}

/// LRANGE — number of elements in [start, stop] inclusive after wrap.
fn builtin_lrange(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let len = i1(args).max(0);
    let start_raw = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let stop_raw = args.get(2).map(|v| v.to_number() as i64).unwrap_or(-1);
    let start = if start_raw < 0 { (len + start_raw).max(0) } else { start_raw.min(len) };
    let stop = if stop_raw < 0 { (len + stop_raw).max(-1) } else { stop_raw.min(len - 1) };
    if stop < start { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(stop - start + 1))
}

/// LREM — remove count of matching elements.
fn builtin_lrem(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let occurrences = i1(args).max(0);
    let limit = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if limit == 0 { occurrences } else { occurrences.min(limit.unsigned_abs() as i64) }))
}

// ───── hash ─────

/// HSET — 1 if new field, 0 if updated.
fn builtin_hset(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let existed = i1(args);
    Ok(StrykeValue::integer(if existed != 0 { 0 } else { 1 }))
}

/// HGET — return existence marker (1 if present).
fn builtin_hget(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let exists = i1(args);
    Ok(StrykeValue::integer(if exists != 0 { 1 } else { 0 }))
}

/// HGETALL — count of (k, v) pairs.
fn builtin_hgetall(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer((v.len() / 2) as i64))
}

/// HLEN — number of fields.
fn builtin_hlen(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_hgetall(args)
}

/// HKEYS — count.
fn builtin_hkeys(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_hgetall(args)
}

/// HVALS — count.
fn builtin_hvals(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_hgetall(args)
}

/// HMSET — number of fields written.
fn builtin_hmset(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer((v.len() / 2) as i64))
}

/// HINCRBY — new field value.
fn builtin_hincrby(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cur = i1(args);
    let inc = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(cur + inc))
}

// ───── set ─────

/// SADD — number of new members added.
fn builtin_sadd(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let already = i1(args);
    let candidates = (args.len() as i64 - 1).max(0);
    Ok(StrykeValue::integer((candidates - already).max(0)))
}

/// SREM — number removed.
fn builtin_srem(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let removed = i1(args);
    Ok(StrykeValue::integer(removed.max(0)))
}

/// SMEMBERS — cardinality.
fn builtin_smembers(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.len() as i64))
}

/// SINTER — |A ∩ B|.
fn builtin_sinter(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a: std::collections::HashSet<u64> =
        b71_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])))
            .iter().map(|x| x.to_bits()).collect();
    let b: std::collections::HashSet<u64> = args.get(1).map(b71_to_floats).unwrap_or_default()
        .iter().map(|x| x.to_bits()).collect();
    Ok(StrykeValue::integer(a.intersection(&b).count() as i64))
}

/// SUNION — |A ∪ B|.
fn builtin_sunion(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a: std::collections::HashSet<u64> =
        b71_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])))
            .iter().map(|x| x.to_bits()).collect();
    let b: std::collections::HashSet<u64> = args.get(1).map(b71_to_floats).unwrap_or_default()
        .iter().map(|x| x.to_bits()).collect();
    Ok(StrykeValue::integer(a.union(&b).count() as i64))
}

/// SDIFF — |A \ B|.
fn builtin_sdiff(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a: std::collections::HashSet<u64> =
        b71_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])))
            .iter().map(|x| x.to_bits()).collect();
    let b: std::collections::HashSet<u64> = args.get(1).map(b71_to_floats).unwrap_or_default()
        .iter().map(|x| x.to_bits()).collect();
    Ok(StrykeValue::integer(a.difference(&b).count() as i64))
}

/// SCARD — cardinality.
fn builtin_scard(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_smembers(args)
}

/// SISMEMBER — 1/0.
fn builtin_sismember(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = b71_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let needle = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if s.contains(&needle) { 1 } else { 0 }))
}

/// SPOP — random member's index (deterministic given seed). Args: cardinality, seed.
fn builtin_spop(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let card = i1(args).max(1);
    let seed = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0) as u64;
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    s ^= s >> 33;
    Ok(StrykeValue::integer((s % card as u64) as i64))
}

// ───── expiration / generic ─────

/// SETEX / SETNX — boolean "did set".
fn builtin_setex(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::integer(1))
}
/// `setnx`
fn builtin_setnx(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let exists = i1(args);
    Ok(StrykeValue::integer(if exists != 0 { 0 } else { 1 }))
}

/// EXPIRE — 1 if applied.
fn builtin_expire(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let exists = i1(args);
    Ok(StrykeValue::integer(if exists != 0 { 1 } else { 0 }))
}

/// TTL — remaining seconds (-2 missing, -1 no expire).
fn builtin_ttl(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let secs = i1(args);
    Ok(StrykeValue::integer(secs))
}
/// `pttl`
fn builtin_pttl(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let secs = i1(args);
    Ok(StrykeValue::integer(secs * 1000))
}

/// PERSIST — 1 if expiration removed.
fn builtin_persist(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let had_ttl = i1(args);
    Ok(StrykeValue::integer(if had_ttl != 0 { 1 } else { 0 }))
}

/// INCR / DECR / INCRBY / DECRBY.
fn builtin_incr(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::integer(i1(args) + 1))
}
/// `decr`
fn builtin_decr(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::integer(i1(args) - 1))
}
/// `incrby`
fn builtin_incrby(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cur = i1(args);
    let inc = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(cur + inc))
}
/// `decrby`
fn builtin_decrby(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cur = i1(args);
    let dec = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(cur - dec))
}

/// GETSET — return old value, set new.
fn builtin_getset(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let old = i1(args);
    Ok(StrykeValue::integer(old))
}

/// MSET — number of keys set.
fn builtin_mset(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer((v.len() / 2) as i64))
}

/// MGET — number returned (some may be missing → 0 here for simplicity).
fn builtin_mget(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.len() as i64))
}

/// RENAMENX — 1 if rename happened (target didn't exist).
fn builtin_renamenx(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let target_exists = i1(args);
    Ok(StrykeValue::integer(if target_exists != 0 { 0 } else { 1 }))
}

/// DBSIZE — total keys.
fn builtin_dbsize(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::integer(n.max(0)))
}

/// TYPE — numeric type id (string=1, list=2, set=3, zset=4, hash=5, stream=6).
fn builtin_type_redis(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let id = i1(args).clamp(0, 6);
    Ok(StrykeValue::integer(id))
}

/// EXISTS key — count of provided keys that exist.
fn builtin_exists_key(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.iter().filter(|x| x.to_number() != 0.0).count() as i64))
}

/// STRLEN — length of value.
fn builtin_strlen(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.len() as i64))
}

/// GETRANGE — substring length [start, end] inclusive.
fn builtin_getrange(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let len = i1(args).max(0);
    let start = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let end_raw = args.get(2).map(|v| v.to_number() as i64).unwrap_or(-1);
    let s = if start < 0 { (len + start).max(0) } else { start.min(len) };
    let e = if end_raw < 0 { (len + end_raw).max(-1) } else { end_raw.min(len - 1) };
    if e < s { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(e - s + 1))
}

/// SETRANGE — new length after writing at offset.
fn builtin_setrange(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cur = i1(args).max(0);
    let off = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    let write = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    Ok(StrykeValue::integer(cur.max(off + write)))
}

/// APPEND — new length after concatenation.
fn builtin_append_redis(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cur = i1(args);
    let add = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(cur + add))
}

// ───── bits ─────

/// BITCOUNT — popcount over byte stream.
fn builtin_bitcount(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let bytes = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let total: u32 = bytes.iter().map(|b| (b.to_number() as u8).count_ones()).sum();
    Ok(StrykeValue::integer(total as i64))
}

/// BITOP — operation on equal-length byte vectors. op: 0=AND, 1=OR, 2=XOR, 3=NOT.
fn builtin_bitop(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let op = i1(args);
    let a = args.get(1).map(arg_to_vec).unwrap_or_default();
    let b = args.get(2).map(arg_to_vec).unwrap_or_default();
    let n = a.len().min(b.len()).max(a.len());
    let mut out = 0_i64;
    for i in 0..n {
        let av = a.get(i).map(|x| x.to_number() as u8).unwrap_or(0);
        let bv = b.get(i).map(|x| x.to_number() as u8).unwrap_or(0);
        let r = match op { 0 => av & bv, 1 => av | bv, 2 => av ^ bv, 3 => !av, _ => av };
        out += r as i64;
    }
    Ok(StrykeValue::integer(out))
}

/// BITPOS — position of first bit equal to `bit`, or -1.
fn builtin_bitpos(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let bytes = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let bit = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    for (i, byte) in bytes.iter().enumerate() {
        let b = byte.to_number() as u8;
        for k in 0..8 {
            let actual = (b >> (7 - k)) & 1;
            if actual as i64 == bit { return Ok(StrykeValue::integer((i * 8 + k) as i64)); }
        }
    }
    Ok(StrykeValue::integer(-1))
}

// ───── HyperLogLog (simplified register-based estimate) ─────

/// PFADD — 1 if estimate changed.
fn builtin_pfadd(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let novel = i1(args);
    Ok(StrykeValue::integer(if novel != 0 { 1 } else { 0 }))
}

/// PFCOUNT — α_m · m² / Σ 2^{-M[j]}; we use raw register sum.
fn builtin_pfcount(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let regs = b71_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let m = regs.len() as f64;
    if m == 0.0 { return Ok(StrykeValue::integer(0)); }
    let alpha = match m as usize {
        16 => 0.673,
        32 => 0.697,
        64 => 0.709,
        _ => 0.7213 / (1.0 + 1.079 / m),
    };
    let z: f64 = regs.iter().map(|r| 2_f64.powf(-r)).sum();
    let e = alpha * m * m / z.max(1e-300);
    Ok(StrykeValue::integer(e as i64))
}

// ───── geo ─────

/// GEOADD — number of new members.
fn builtin_geoadd(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let added = i1(args);
    Ok(StrykeValue::integer(added.max(0)))
}

/// GEODIST — Haversine in metres.
fn builtin_geodist(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat1 = f1(args).to_radians();
    let lon1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lat2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let lon2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0).to_radians();
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    Ok(StrykeValue::float(6_372_797.560856 * c))
}

/// GEOHASH — interleaved bits up to 52 (precision 11).
fn builtin_geohash(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lat = f1(args);
    let lon = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mut bits: u64 = 0;
    let mut lat_lo = -90.0;
    let mut lat_hi = 90.0;
    let mut lon_lo = -180.0;
    let mut lon_hi = 180.0;
    for i in 0..52 {
        let mid_lon = (lon_lo + lon_hi) / 2.0;
        let mid_lat = (lat_lo + lat_hi) / 2.0;
        if i % 2 == 0 {
            if lon >= mid_lon { bits |= 1 << (51 - i); lon_lo = mid_lon; } else { lon_hi = mid_lon; }
        } else if lat >= mid_lat { bits |= 1 << (51 - i); lat_lo = mid_lat; } else { lat_hi = mid_lat; }
    }
    Ok(StrykeValue::integer(bits as i64))
}

// ───── streams ─────

/// XADD — new entry id (monotone).
fn builtin_xadd(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let last_id = i1(args);
    Ok(StrykeValue::integer(last_id + 1))
}

/// XLEN — entry count.
fn builtin_xlen(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.len() as i64))
}

/// XRANGE — count of entries in [start, end].
fn builtin_xrange(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let ids = b71_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let start = args.get(1).map(|v| v.to_number()).unwrap_or(f64::NEG_INFINITY);
    let end = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::integer(ids.iter().filter(|&&x| x >= start && x <= end).count() as i64))
}

/// OBJECT ENCODING — id of encoding (raw=0, embstr=1, int=2, ziplist=3,
/// linkedlist=4, hashtable=5, intset=6, listpack=7, skiplist=8).
fn builtin_object_encoding(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let id = i1(args).clamp(0, 8);
    Ok(StrykeValue::integer(id))
}

/// DEBUG OBJECT — return refcount.
fn builtin_debug_object(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let refcount = i1(args).max(1);
    Ok(StrykeValue::integer(refcount))
}

/// CLUSTER SLOTS — slot index from CRC16(key) mod 16384.
fn builtin_cluster_slots(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let bytes = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut crc: u16 = 0;
    for byte in bytes {
        let b = byte.to_number() as u8;
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 { crc = (crc << 1) ^ 0x1021; } else { crc <<= 1; }
        }
    }
    Ok(StrykeValue::integer((crc % 16384) as i64))
}
