// database internals, distributed systems, consensus, sketches.

fn b48_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// B-tree split: median key index = (n - 1) / 2
fn builtin_db_b_tree_split(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::integer((n - 1) / 2))
}

/// B-tree merge: combined node fill
fn builtin_db_b_tree_merge(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(a + b))
}

/// LSM compaction step (size ratio T)
fn builtin_db_lsm_compaction_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let level_size = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    Ok(StrykeValue::float(level_size * t))
}

/// Skiplist height pick (geometric distribution)
fn builtin_db_skiplist_height_pick(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    if r <= 0.0 || p <= 0.0 || p >= 1.0 { return Ok(StrykeValue::integer(1)); }
    Ok(StrykeValue::integer((r.ln() / p.ln()).floor() as i64 + 1))
}

/// Bloom filter bit index from hash
fn builtin_db_bloom_filter_bit_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = i1(args) as u64;
    let m = args.get(1).map(|v| v.to_number() as u64).unwrap_or(1024);
    if m == 0 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer((h % m) as i64))
}

/// Cuckoo filter fingerprint (8 LSB)
fn builtin_db_cuckoo_filter_fingerprint(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = i1(args) as u64;
    Ok(StrykeValue::integer((h & 0xff) as i64))
}

/// Quotient filter canonical slot
fn builtin_db_quotient_filter_canonical(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = i1(args) as u64;
    let q = args.get(1).map(|v| v.to_number() as u64).unwrap_or(8);
    Ok(StrykeValue::integer((h >> (64 - q)) as i64))
}

/// Count-min sketch bin index
fn builtin_db_count_min_sketch_bin(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = i1(args) as u64;
    let w = args.get(1).map(|v| v.to_number() as u64).unwrap_or(2048);
    if w == 0 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer((h % w) as i64))
}

/// HyperLogLog register max value (rho function)
fn builtin_db_hyperloglog_register_max(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = i1(args) as u64;
    Ok(StrykeValue::integer(h.leading_zeros() as i64 + 1))
}

/// MinHash value: min(h(x_i)) over set
fn builtin_db_min_hash_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b48_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// SimHash bit index
fn builtin_db_simhash_bit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = f1(args);
    Ok(StrykeValue::integer(if v >= 0.0 { 1 } else { 0 }))
}

/// Consistent hash bucket
fn builtin_db_consistent_hash_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let key_hash = i1(args) as u64;
    let n = args.get(1).map(|v| v.to_number() as u64).unwrap_or(1).max(1);
    Ok(StrykeValue::integer((key_hash % n) as i64))
}

/// Rendezvous (HRW) hash score
fn builtin_db_rendezvous_hash_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let key = f1(args);
    let server = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((key * server).abs().sin()))
}

/// Jump-consistent hash (Lamping & Veach)
fn builtin_db_jump_hash_bucket(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut key = i1(args) as u64;
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let mut b = -1_i64;
    let mut j = 0_i64;
    while j < n {
        b = j;
        key = key.wrapping_mul(2_862_933_555_777_941_757).wrapping_add(1);
        j = (((b + 1) as f64) * ((1u64 << 31) as f64) / ((key >> 33) as f64 + 1.0)) as i64;
    }
    Ok(StrykeValue::integer(b))
}

/// Maglev hash step
fn builtin_db_maglev_hash_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let offset = i1(args) as u64;
    let skip = args.get(1).map(|v| v.to_number() as u64).unwrap_or(1).max(1);
    let m = args.get(2).map(|v| v.to_number() as u64).unwrap_or(101);
    if m == 0 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer((offset.wrapping_add(skip) % m) as i64))
}

/// LRU eviction age = now - last_access
fn builtin_db_lru_cache_eviction_age(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let now = f1(args);
    let last = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(now - last))
}

/// LFU cache decay: freq · γ^t
fn builtin_db_lfu_cache_decay(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let freq = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.95);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(freq * gamma.powf(t)))
}

/// ARC cache score (LRU + LFU split)
fn builtin_db_arc_cache_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lru = f1(args);
    let lfu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(p * lru + (1.0 - p) * lfu))
}

/// Clock cache hand position
fn builtin_db_clock_cache_hand(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    Ok(StrykeValue::integer((h + 1) % n))
}

/// TinyLFU admission score
fn builtin_db_tinylfu_admit_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let new_freq = f1(args);
    let victim_freq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if new_freq > victim_freq { 1 } else { 0 }))
}

/// W-TinyLFU frequency estimate (CMS lookup)
fn builtin_db_w_tinylfu_freq(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b48_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Buffer pool score (clock-pro / 2Q proxy)
fn builtin_db_buffer_pool_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let recency = f1(args);
    let freq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(recency * freq))
}

/// Query plan cost step (Selinger style: cardinality × per-row cost)
fn builtin_db_query_plan_cost_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let card = f1(args);
    let cost_per_row = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(card * cost_per_row))
}

/// Join selectivity = 1 / max(distinct_a, distinct_b)
fn builtin_db_join_selectivity_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d_a = f1(args);
    let d_b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let max_d = d_a.max(d_b).max(1.0);
    Ok(StrykeValue::float(1.0 / max_d))
}

/// Index seek cost: log(N) base B
fn builtin_db_index_seek_cost(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    if b <= 1.0 || n <= 0.0 { return Ok(StrykeValue::float(1.0)); }
    Ok(StrykeValue::float(n.ln() / b.ln()))
}

/// Sequential scan cost: pages_read · seq_page_cost + tuples · cpu_tuple_cost
/// (Postgres-style cost model). Args: rows, page_size, seq_page_cost, cpu_tuple_cost.
fn builtin_db_seq_scan_cost(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rows = f1(args);
    let page_size = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let seq_page = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let cpu_tuple = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let pages = (rows / page_size).ceil();
    Ok(StrykeValue::float(pages * seq_page + rows * cpu_tuple))
}

/// Index scan cost: log N + matches
fn builtin_db_index_scan_cost(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let matches = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(n.max(1.0).ln() + matches))
}

/// Sort cost estimate: N log N
fn builtin_db_sort_cost_estimate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    if n <= 1.0 { return Ok(StrykeValue::float(n)); }
    Ok(StrykeValue::float(n * n.ln()))
}

/// Hash join cost: |A| + |B|
fn builtin_db_hash_join_cost(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(a + b))
}

/// Merge join cost: requires both inputs sorted on join key. Includes external-
/// sort cost when not pre-sorted: cost = sort(A) + sort(B) + |A| + |B|, where
/// sort(N) = N·log₂(N/M)·2  (two-pass external merge with M memory blocks).
/// Args: |A|, |B|, M (memory blocks), sorted_a (0/1), sorted_b (0/1).
fn builtin_db_merge_join_cost(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m = args.get(2).map(|v| v.to_number()).unwrap_or(64.0).max(2.0);
    let sorted_a = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let sorted_b = args.get(4).map(|v| v.to_number() as i64).unwrap_or(0);
    let sort = |n: f64| if n <= 1.0 { 0.0 } else { 2.0 * n * (n / m).log2().max(1.0) };
    let s_a = if sorted_a != 0 { 0.0 } else { sort(a) };
    let s_b = if sorted_b != 0 { 0.0 } else { sort(b) };
    Ok(StrykeValue::float(s_a + s_b + a + b))
}

/// Nested loop cost: |A| * |B|
fn builtin_db_nested_loop_cost(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(a * b))
}

/// Query cardinality estimate from selectivity
fn builtin_db_query_cardinality(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let sel = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(n * sel))
}

/// Histogram bucket index from value, lower bound, bucket width
fn builtin_db_histogram_bucket_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = f1(args);
    let lo = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let w = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if w == 0.0 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(((v - lo) / w).floor() as i64))
}

/// Quantile estimate at p99 from sorted samples
fn builtin_db_quantile_estimate_p99(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut v = b48_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((v.len() - 1) as f64 * 0.99) as usize;
    Ok(StrykeValue::float(v[idx]))
}

/// t-digest centroid update (single)
fn builtin_db_t_digest_centroid(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cur_mean = f1(args);
    let cur_count = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let new = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if cur_count == 0.0 { return Ok(StrykeValue::float(new)); }
    Ok(StrykeValue::float((cur_mean * cur_count + new) / (cur_count + 1.0)))
}

/// KLL sketch quantile
fn builtin_db_kll_quantile_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let v = b48_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let mut sorted = v.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((sorted.len() - 1) as f64 * p.clamp(0.0, 1.0)) as usize;
    Ok(StrykeValue::float(sorted[idx]))
}

/// DD-sketch bin index from value with relative accuracy α
fn builtin_db_dd_sketch_bin(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    if v <= 0.0 || alpha == 0.0 { return Ok(StrykeValue::integer(0)); }
    let gamma = (1.0 + alpha) / (1.0 - alpha);
    Ok(StrykeValue::integer((v.ln() / gamma.ln()).ceil() as i64))
}

/// Reservoir sampling: index for new item
fn builtin_db_reservoir_sample_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::integer((r * i) as i64))
}

/// Chao estimator for population
fn builtin_db_chao_estimator_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_obs = f1(args);
    let f1_count = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f2_count = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if f2_count == 0.0 { return Ok(StrykeValue::float(n_obs)); }
    Ok(StrykeValue::float(n_obs + f1_count * f1_count / (2.0 * f2_count)))
}

/// Jaccard MinHash estimate: # matching / total
fn builtin_db_jaccard_minhash_estimate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let matches = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(matches / total))
}

/// Linear probabilistic counting: -m·ln(empty/m)
fn builtin_db_distinct_estimate_lpc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let empty = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if empty <= 0.0 { return Ok(StrykeValue::float(m)); }
    Ok(StrykeValue::float(-m * (empty / m).ln()))
}

/// HLL distinct estimate: αm² / Σ 2^(-r)
fn builtin_db_distinct_estimate_hll(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let z = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let alpha = 0.7213 / (1.0 + 1.079 / m);
    if z == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(alpha * m * m / z))
}

/// Throttle token step
fn builtin_db_throttle_token_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let tokens = f1(args);
    let rate = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(tokens + rate * dt))
}

/// Leaky bucket step
fn builtin_db_leaky_bucket_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let level = f1(args);
    let leak = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let dt = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float((level - leak * dt).max(0.0)))
}

/// Token bucket step (admit/reject)
fn builtin_db_token_bucket_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let tokens = f1(args);
    let cost = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::integer(if tokens >= cost { 1 } else { 0 }))
}

/// Circuit breaker state step (open if errors > threshold)
fn builtin_db_circuit_breaker_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let errors = f1(args);
    let threshold = args.get(1).map(|v| v.to_number()).unwrap_or(5.0);
    Ok(StrykeValue::integer(if errors > threshold { 1 } else { 0 }))
}

/// Two-phase commit step: prepare (1) or commit (2) or abort (0)
fn builtin_db_two_phase_commit_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let votes_yes = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::integer(if votes_yes >= total { 2 } else { 0 }))
}

/// Three-phase commit (Skeen 1982): adds a "prepared-to-commit" phase between
/// 2PC's vote and commit, eliminating the blocking on coordinator failure.
/// State machine returns next state given current and votes:
///   0 INIT, 1 WAIT (vote phase), 2 PRECOMMIT (all yes), 3 COMMIT, 4 ABORT.
/// Args: cur_state, votes_yes, total. Transitions: 0→1, 1→2 if all yes else 4,
/// 2→3 (after timeout-safe ack), any timeout in WAIT→4, in PRECOMMIT→3.
fn builtin_db_three_phase_commit_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cur = i1(args);
    let votes = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let total = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    match cur {
        0 => Ok(StrykeValue::integer(1)),
        1 => Ok(StrykeValue::integer(if votes >= total { 2 } else { 4 })),
        2 => Ok(StrykeValue::integer(3)),
        _ => Ok(StrykeValue::integer(cur)),
    }
}

/// Paxos propose ID (epoch concatenated to node id)
fn builtin_db_paxos_propose_id(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let epoch = i1(args);
    let node_id = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((epoch << 16) | (node_id & 0xffff)))
}

/// Raft term advance
fn builtin_db_raft_term_advance(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let term = i1(args);
    Ok(StrykeValue::integer(term + 1))
}

/// Raft log match check (consistency)
fn builtin_db_raft_log_match_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prev_term = i1(args);
    let leader_term = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if prev_term == leader_term { 1 } else { 0 }))
}

/// ZAB (ZooKeeper Atomic Broadcast) zxid: 64-bit identifier with high 32 bits =
/// epoch (leader generation) and low 32 bits = counter (proposal number within
/// epoch). Advance: on new epoch, counter resets to 0; within epoch, counter++.
/// Args: prev_zxid (i64), new_epoch (0 = stay, 1 = new election).
fn builtin_db_zab_epoch_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prev = i1(args) as u64;
    let new_epoch = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let epoch = (prev >> 32) as u32;
    let counter = (prev & 0xffff_ffff) as u32;
    let (next_e, next_c) = if new_epoch != 0 { (epoch + 1, 0) }
                           else { (epoch, counter.wrapping_add(1)) };
    Ok(StrykeValue::integer((((next_e as u64) << 32) | next_c as u64) as i64))
}

/// Chubby lease step (countdown)
fn builtin_db_chubby_lease_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lease = f1(args);
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float((lease - dt).max(0.0)))
}

/// Logical clock step: max(local, msg) + 1
fn builtin_db_logical_clock_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let local = i1(args);
    let msg = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(local.max(msg) + 1))
}

/// Lamport timestamp (alias)
fn builtin_db_lamport_timestamp(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_db_logical_clock_step(args)
}

/// Vector clock merge (max-wise)
fn builtin_db_vector_clock_merge(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(a.max(b)))
}

/// Hybrid logical clock step
fn builtin_db_hybrid_logical_clock(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let phys = f1(args);
    let logical = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(phys.max(logical) + 1e-9))
}

/// CRDT G-Counter merge: max
fn builtin_db_crdt_g_counter_merge(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(a.max(b)))
}

/// CRDT PN-Counter: P - N
fn builtin_db_crdt_pn_counter_merge(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(p - n))
}

/// CRDT LWW register merge: pick by timestamp
fn builtin_db_crdt_lww_register_merge(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_a = f1(args);
    let ts_a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v_b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let ts_b = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(if ts_a >= ts_b { v_a } else { v_b }))
}

/// CRDT Set OR-merge (union of (e, ts) pairs — return count)
fn builtin_db_crdt_set_or_merge(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(a + b))
}

/// Consensus quorum size = ⌊N/2⌋ + 1
fn builtin_db_consensus_quorum_size(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::integer(n / 2 + 1))
}

/// Replication lag step
fn builtin_db_replication_lag_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let primary = f1(args);
    let replica = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(primary - replica))
}

/// Number of partitions for n entities
fn builtin_db_partitions_for_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let entries_per_partition = args.get(1).map(|v| v.to_number()).unwrap_or(1_000_000.0);
    if entries_per_partition == 0.0 { return Ok(StrykeValue::integer(1)); }
    Ok(StrykeValue::integer((n / entries_per_partition).ceil() as i64))
}

/// Consistent hash lookup id
fn builtin_db_consistent_lookup_id(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_db_consistent_hash_index(args)
}

/// Chord finger index = (n + 2^i) mod 2^m
fn builtin_db_chord_finger_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let i = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let m = args.get(2).map(|v| v.to_number() as i64).unwrap_or(160);
    let two_pow_m = if m < 63 { 1_i64 << m } else { i64::MAX };
    let two_pow_i = if i < 63 { 1_i64 << i } else { i64::MAX };
    Ok(StrykeValue::integer((n + two_pow_i) % two_pow_m.max(1)))
}

/// Kademlia XOR distance
fn builtin_db_kademlia_xor_distance(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args) as u64;
    let b = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    Ok(StrykeValue::integer((a ^ b) as i64))
}

/// Pastry routing step (route to numerically closest)
fn builtin_db_pastry_routing_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let target = f1(args);
    let leaf = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((target - leaf).abs()))
}

/// DHT replication factor: ⌈N · target_durability_log / log_node_avail⌉ for required
/// availability. Args: n_nodes, target_p (eg .9999), node_avail (eg .99).
/// Solves N_replicas = log(1 - target_p) / log(1 - node_avail).
fn builtin_db_dht_replicate_factor(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let target = f1(args).clamp(0.0, 0.999_999);
    let node_avail = args.get(1).map(|v| v.to_number()).unwrap_or(0.99).clamp(0.001, 0.999);
    let n = (1.0 - target).ln() / (1.0 - node_avail).ln();
    Ok(StrykeValue::integer(n.ceil() as i64))
}

/// Partition failure check (majority alive?)
fn builtin_db_partition_failure_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let alive = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::integer(if alive * 2.0 > total { 1 } else { 0 }))
}

/// Byzantine quorum size = ⌊2N/3⌋ + 1
fn builtin_db_byzantine_quorum_size(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::integer(2 * n / 3 + 1))
}

/// PBFT view change step
fn builtin_db_pbft_view_change(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let view = i1(args);
    Ok(StrykeValue::integer(view + 1))
}

/// HoneyBadger BFT throughput estimate: B = batch_size · (1 - timeout_rate) /
/// epoch_duration. Args: batch, timeout_rate, epoch_duration_seconds.
fn builtin_db_honey_badger_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let batch = f1(args);
    let timeout_rate = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).clamp(0.0, 1.0);
    let epoch = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-9);
    Ok(StrykeValue::float(batch * (1.0 - timeout_rate) / epoch))
}

/// Avalanche query step (multi-round subsampling)
fn builtin_db_avalanche_query_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let yes = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(20.0);
    if k == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(yes / k))
}

/// Quorum intersection check (W + R > N)
fn builtin_db_quorum_intersection_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let w = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(3.0);
    Ok(StrykeValue::integer(if w + r > n { 1 } else { 0 }))
}

/// Anti-entropy step (gossip count)
fn builtin_db_anti_entropy_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(f1(args) + 1.0))
}

/// Merkle node hash combine
fn builtin_db_merkle_node_hash(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l = i1(args) as u64;
    let r = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    let combined = l.wrapping_mul(31).wrapping_add(r);
    Ok(StrykeValue::integer(combined as i64))
}

/// Merkle path verify (compares hashes match)
fn builtin_db_merkle_path_verify(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h_actual = i1(args);
    let h_expected = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if h_actual == h_expected { 1 } else { 0 }))
}

/// Gossip fanout step
fn builtin_db_gossip_fanout_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f = f1(args);
    Ok(StrykeValue::float(f.max(1.0)))
}

/// Anti-entropy pull step
fn builtin_db_anti_entropy_pull_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_db_anti_entropy_step(args)
}

/// Split-brain check
fn builtin_db_split_brain_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if a > 0.0 && b > 0.0 { 1 } else { 0 }))
}

/// Clock skew estimate
fn builtin_db_clock_skew_estimate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t1 = f1(args);
    let t2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((t2 - t1).abs()))
}

/// Freshness score (1 - lag/max_lag)
fn builtin_db_freshness_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lag = f1(args);
    let max_lag = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if max_lag == 0.0 { return Ok(StrykeValue::float(1.0)); }
    Ok(StrykeValue::float((1.0 - lag / max_lag).max(0.0)))
}

/// Read repair step (compare versions)
fn builtin_db_read_repair_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_local = f1(args);
    let v_remote = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(v_local.max(v_remote)))
}

/// Hinted handoff step
fn builtin_db_hinted_handoff_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let queue = f1(args);
    let drained = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((queue - drained).max(0.0)))
}

/// Compaction score
fn builtin_db_compaction_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let level_size = f1(args);
    let target = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if target == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(level_size / target))
}

/// Levelled compaction step
fn builtin_db_levelled_compaction_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_db_lsm_compaction_step(args)
}

/// Size-tiered compaction
fn builtin_db_size_tiered_compaction(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b48_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().sum()))
}

/// Universal compaction (RocksDB): trigger when file ratios meet ANY of the
/// three conditions: (1) #files ≥ level0_file_num_compaction_trigger,
/// (2) running_size_ratio = (Σ_{i≤N-1} size_i) / size_N ≤ size_ratio_threshold,
/// (3) trailing_size_ratio = size_N / total ≥ max_size_amplification_pct/100.
/// Returns trigger code (0 none, 1 # files, 2 size ratio, 3 amp). Args: nfiles,
/// f0_trigger, size_ratio, ratio_thresh, amp_pct, max_amp_pct.
fn builtin_db_universal_compaction_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let nfiles = i1(args);
    let f0_trig = args.get(1).map(|v| v.to_number() as i64).unwrap_or(4);
    let size_ratio = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let ratio_thresh = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let amp_pct = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let max_amp = args.get(5).map(|v| v.to_number()).unwrap_or(200.0);
    if nfiles >= f0_trig { return Ok(StrykeValue::integer(1)); }
    if size_ratio <= ratio_thresh { return Ok(StrykeValue::integer(2)); }
    if amp_pct >= max_amp { return Ok(StrykeValue::integer(3)); }
    Ok(StrykeValue::integer(0))
}

/// Write amplification
fn builtin_db_write_amplification(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes_written = f1(args);
    let bytes_user = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if bytes_user == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(bytes_written / bytes_user))
}

/// Read amplification (RA): logical-read bytes vs. physical-read bytes per
/// user query. For LSM with N levels and bloom-FP-rate p_bloom:
///   RA ≈ 1 + Σ_{l=1}^{N} p_bloom^{l−1}     (geometric series of false-positive lookups).
/// Args: levels N, bloom false-positive rate (default 0.01).
fn builtin_db_read_amplification(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let levels = i1(args).max(1);
    let p_bloom = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    let mut s = 1.0_f64;
    let mut term = 1.0_f64;
    for _ in 1..levels { term *= p_bloom; s += term; }
    Ok(StrykeValue::float(s))
}

/// Space amplification (SA): bytes-on-disk / bytes-of-live-data. For LSM with
/// growth factor T and N levels at full state:
///   SA ≈ T / (T − 1)     (geometric series of stale data per level).
/// Args: T (level multiplier, default 10).
fn builtin_db_space_amplification(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args).max(1.0001);
    Ok(StrykeValue::float(t / (t - 1.0)))
}

/// Block cache hit rate = hits / (hits + misses)
fn builtin_db_block_cache_hit_rate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let hits = f1(args);
    let misses = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let total = hits + misses;
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(hits / total))
}

/// Page cache eviction age
fn builtin_db_page_cache_eviction_age(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_db_lru_cache_eviction_age(args)
}

/// WAL fsync cost = base_seek + bytes / disk_bandwidth + fsync_constant.
/// Args: bytes, disk_bw_bytes_per_s, fsync_const_us.
fn builtin_db_wal_fsync_cost(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = f1(args);
    let bw = args.get(1).map(|v| v.to_number()).unwrap_or(500e6).max(1.0);
    let const_us = args.get(2).map(|v| v.to_number()).unwrap_or(50.0);
    Ok(StrykeValue::float(const_us + bytes / bw * 1e6))
}

/// Group commit batches: aggregate count of commits flushed in one fsync.
/// Throughput = commits_per_fsync · 1/fsync_latency. Args: in-flight, max_batch.
fn builtin_db_group_commit_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let inflight = i1(args);
    let max_batch = args.get(1).map(|v| v.to_number() as i64).unwrap_or(64);
    Ok(StrykeValue::integer(inflight.min(max_batch).max(1)))
}

/// Replica lag threshold check
fn builtin_db_replica_lag_threshold(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lag = f1(args);
    let threshold = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::integer(if lag > threshold { 1 } else { 0 }))
}

/// Synchronous commit check
fn builtin_db_synchronous_commit_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let acks = f1(args);
    let required = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::integer(if acks >= required { 1 } else { 0 }))
}

/// Async commit check (1 if leader-only ack)
fn builtin_db_async_commit_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let leader_ack = i1(args);
    Ok(StrykeValue::integer(leader_ack))
}

/// Eventual consistency check (just returns 1 if propagation begun)
fn builtin_db_eventual_consistency_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let propagating = i1(args);
    Ok(StrykeValue::integer(propagating))
}

/// Strong consistency check (W + R > N)
fn builtin_db_strong_consistency_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_db_quorum_intersection_check(args)
}

/// Linearizability (Herlihy & Wing 1990): every operation appears to take effect
/// at a single instant between its invocation and response, AND the order
/// respects real time. Stronger than serializability and strong consistency.
/// Per Wing-Gong / Gibbons-Korach NP-hardness: full check requires history
/// search. This step verifies the necessary local conditions: (1) every
/// completed op has a fixed linearization point in [inv, resp], (2) real-time
/// order is preserved, (3) reads return the most recent linearized write.
/// Args: rt_violations, point_violations, stale_reads. Returns 1 if all 0.
fn builtin_db_linearizability_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rt = i1(args);
    let pt = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let stale = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if rt == 0 && pt == 0 && stale == 0 { 1 } else { 0 }))
}

/// Causal consistency check
fn builtin_db_causal_consistency_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let happens_before = i1(args);
    Ok(StrykeValue::integer(happens_before))
}
