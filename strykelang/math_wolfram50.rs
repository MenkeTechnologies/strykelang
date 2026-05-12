// Batch 50 — OS internals: schedulers, I/O, memory, power, control groups.

/// Priority aging step: prio_eff = prio_static + age_factor·age
fn builtin_os_priority_aging_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let age = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let factor = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(p - factor * age))
}

/// MLFQ demote step (move down one level)
fn builtin_os_mlfq_demote_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let level = i1(args);
    let max_level = args.get(1).map(|v| v.to_number() as i64).unwrap_or(7);
    Ok(StrykeValue::integer((level + 1).min(max_level)))
}

/// MLFQ priority boost: every S ms, all jobs jump to topmost queue. Returns
/// new level given current level, time since last boost, boost interval S.
fn builtin_os_mlfq_promote_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let level = i1(args);
    let elapsed = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let s = args.get(2).map(|v| v.to_number()).unwrap_or(1000.0);
    if elapsed >= s { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(level.max(0)))
}

/// Round-robin quantum (ms scaled by priority)
fn builtin_os_round_robin_quantum(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prio = f1(args);
    let base = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    Ok(StrykeValue::float(base * (1.0 + prio / 40.0)))
}

/// Linux CFS vruntime increment
fn builtin_os_completely_fair_vruntime(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let vruntime = f1(args);
    let delta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let weight = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if weight == 0.0 { return Ok(StrykeValue::float(vruntime)); }
    Ok(StrykeValue::float(vruntime + delta / weight))
}

/// Lottery scheduler ticket count
fn builtin_os_lottery_ticket_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().map(|x| x.to_number()).sum()))
}

/// Stride scheduler pass step: pass += stride
fn builtin_os_stride_pass_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pass = f1(args);
    let stride = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(pass + stride))
}

/// EEVDF eligibility (vruntime > virtual_eligible)
fn builtin_os_eevdf_eligible(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_runtime = f1(args);
    let v_eligible = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if v_runtime >= v_eligible { 1 } else { 0 }))
}

/// CFS load balance step
fn builtin_os_cfs_load_balance_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let busy = f1(args);
    let idle = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((busy - idle) / 2.0))
}

/// EAS energy estimate
fn builtin_os_eas_energy_estimate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_static = f1(args);
    let p_dynamic = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let load = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(p_static + p_dynamic * load))
}

/// SMT threading share (per logical CPU)
fn builtin_os_smt_threading_share(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    if n == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(1.0 / n))
}

/// NUMA node distance: 10 (same), 20 (cross)
fn builtin_os_numa_node_distance(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if a == b { 10 } else { 20 }))
}

/// CPU affinity score (matching mask bit count)
fn builtin_os_cpu_affinity_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mask = i1(args) as u64;
    Ok(StrykeValue::integer(mask.count_ones() as i64))
}

/// Thread migration cost (cache miss penalty)
fn builtin_os_thread_migration_cost(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cache_size = f1(args);
    Ok(StrykeValue::float(cache_size * 0.001))
}

/// Load average decay (1, 5, 15-min EWMAs): newL = e^(-1/N) · oldL + (1 - e^(-1/N)) · n
fn builtin_os_load_average_decay(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let old_l = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let tau = args.get(2).map(|v| v.to_number()).unwrap_or(60.0);
    if tau == 0.0 { return Ok(StrykeValue::float(n)); }
    let alpha = (-1.0 / tau).exp();
    Ok(StrykeValue::float(alpha * old_l + (1.0 - alpha) * n))
}

/// Runqueue depth: count of runnable tasks given (running, waiting_io, sleeping).
/// Per Linux kernel: nr_running = TASK_RUNNING - blocked. Args: total tasks
/// array of states (0=running, 1=ready, 2=waiting_io, 3=sleeping).
fn builtin_os_runqueue_depth(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let states = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let depth = states.iter().filter(|s| {
        let v = s.to_number() as i64;
        v == 0 || v == 1
    }).count();
    Ok(StrykeValue::integer(depth as i64))
}

/// Deadline I/O scheduler check
fn builtin_os_io_scheduler_deadline(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let deadline = f1(args);
    let now = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(if now >= deadline { 1 } else { 0 }))
}

/// CFQ scheduler virtual disk-time slice: vdisktime = service_received / weight.
/// Pick queue with min vdisktime. Args: service_received, ioprio_weight.
fn builtin_os_io_scheduler_cfq_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let service = f1(args);
    let weight = args.get(1).map(|v| v.to_number()).unwrap_or(500.0).max(1.0);
    Ok(StrykeValue::float(service / weight))
}

/// NOOP scheduler: FIFO merge of adjacent sectors. Returns merged-request count
/// from a sorted sector list (run-length compression).
fn builtin_os_io_scheduler_noop_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut sectors = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    sectors.sort_by(|a, b| a.to_number().partial_cmp(&b.to_number()).unwrap_or(std::cmp::Ordering::Equal));
    if sectors.is_empty() { return Ok(StrykeValue::integer(0)); }
    let mut merged = 1_i64;
    for w in sectors.windows(2) {
        if (w[1].to_number() - w[0].to_number()).abs() > 1.0 { merged += 1; }
    }
    Ok(StrykeValue::integer(merged))
}

/// BFQ I/O scheduler step (budget)
fn builtin_os_io_scheduler_bfq_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let budget = f1(args);
    let consumed = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((budget - consumed).max(0.0)))
}

/// Kyber scheduler latency budget. Adjusts queue depth to meet target latency:
///   new_depth = old_depth · (target_lat / observed_p99)
/// (Multiplicative AIMD-style controller; clamped 1..256.)
fn builtin_os_io_scheduler_kyber_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let depth = f1(args);
    let target = args.get(1).map(|v| v.to_number()).unwrap_or(2.0);
    let observed = args.get(2).map(|v| v.to_number()).unwrap_or(target).max(1e-6);
    let new_depth = depth * (target / observed);
    Ok(StrykeValue::integer(new_depth.clamp(1.0, 256.0).round() as i64))
}

/// MQ-deadline scheduler step
fn builtin_os_io_scheduler_mq_deadline(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_os_io_scheduler_deadline(args)
}

/// Anticipation window (anticipatory I/O)
fn builtin_os_anticipation_window(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let last_read_time = f1(args);
    let dt = args.get(1).map(|v| v.to_number()).unwrap_or(0.001);
    Ok(StrykeValue::float(last_read_time + dt))
}

/// Elevator (SCAN) step
fn builtin_os_elevator_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pos = f1(args);
    let direction = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(pos + direction))
}

/// Disk seek time (linear in distance)
fn builtin_os_disk_seek_time(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dist = f1(args);
    let speed = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if speed == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(dist / speed))
}

/// Disk rotational latency (avg = 1/(2·rpm/60))
fn builtin_os_disk_rotational_lat(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rpm = f1(args);
    if rpm <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(60.0 / (2.0 * rpm)))
}

/// Disk transfer time = bytes / bandwidth
fn builtin_os_disk_transfer_time(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = f1(args);
    let bw = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if bw == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(bytes / bw))
}

/// Pre-fetch window: ramp from min to max as sequential-access streak grows.
/// Linux readahead doubles window per hit until ra_pages cap. Args: streak, ra_max.
fn builtin_os_pre_fetch_window(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let streak = i1(args).max(0) as u32;
    let ra_max = args.get(1).map(|v| v.to_number() as i64).unwrap_or(128).max(1);
    let win = (1_i64 << streak.min(20)).min(ra_max);
    Ok(StrykeValue::integer(win))
}

/// Buffer cache pages
fn builtin_os_buffer_cache_pages(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = f1(args);
    let page_size = args.get(1).map(|v| v.to_number()).unwrap_or(4096.0);
    if page_size == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(bytes / page_size))
}

/// Dirty page threshold
fn builtin_os_dirty_page_threshold(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mem = f1(args);
    let pct = args.get(1).map(|v| v.to_number()).unwrap_or(0.2);
    Ok(StrykeValue::float(mem * pct))
}

/// Writeback step
fn builtin_os_writeback_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dirty = f1(args);
    let rate = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float((dirty - rate).max(0.0)))
}

/// Swappiness factor (linux: 0..200)
fn builtin_os_swappiness_factor(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(f1(args).clamp(0.0, 200.0)))
}

/// kswapd wake-up threshold: zone_low_wmark = min_free_kbytes · zone_managed /
/// total_managed. Returns whether free_pages < low_wmark (1=wake kswapd).
fn builtin_os_kswapd_wake_threshold(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let free_pages = f1(args);
    let min_free_kb = args.get(1).map(|v| v.to_number()).unwrap_or(11_584.0);
    let zone_managed = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let total_managed = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let low_wmark = (min_free_kb / 4.0) * (zone_managed / total_managed) * 1.5;
    Ok(StrykeValue::integer(if free_pages < low_wmark { 1 } else { 0 }))
}

/// OOM score step (badness)
fn builtin_os_oom_score_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rss = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(1000.0 * rss / total))
}

/// LRU page replacement: choose oldest page to evict from access-time array.
/// Args: access timestamps array; returns index of oldest (min).
fn builtin_os_page_replacement_lru(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let times = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if times.is_empty() { return Ok(StrykeValue::integer(-1)); }
    let mut best = (0_i64, f64::INFINITY);
    for (i, t) in times.iter().enumerate() {
        let v = t.to_number();
        if v < best.1 { best = (i as i64, v); }
    }
    Ok(StrykeValue::integer(best.0))
}

/// Clock page replacement (returns next position)
fn builtin_os_page_replacement_clock(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pos = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    Ok(StrykeValue::integer((pos + 1) % n))
}

/// 2Q page replacement: maintain Am (hot) + A1in (probationary) + A1out (ghost).
/// Promote on hit in A1out, evict from A1in on miss. Args: hit_in_a1out (bool),
/// a1in_size, am_size, kin_size_limit. Returns target queue ID for new entry.
fn builtin_os_page_replacement_2q(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let hit_a1out = i1(args);
    let a1in = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let kin_limit = args.get(2).map(|v| v.to_number()).unwrap_or(64.0);
    if hit_a1out != 0 { return Ok(StrykeValue::integer(0)); }
    if a1in >= kin_limit { return Ok(StrykeValue::integer(2)); }
    Ok(StrykeValue::integer(1))
}

/// Working set W(t, τ) = pages referenced in window [t-τ, t]. Compute from
/// access trace + timestamp + window.
fn builtin_os_working_set_size(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let trace = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let now = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let tau = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let mut seen = std::collections::HashSet::new();
    for ch in trace.chunks(2) {
        if ch.len() < 2 { continue; }
        let pg = ch[0].to_number() as i64;
        let t = ch[1].to_number();
        if t >= now - tau && t <= now { seen.insert(pg); }
    }
    Ok(StrykeValue::integer(seen.len() as i64))
}

/// Thrashing threshold check
fn builtin_os_thrashing_threshold(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pf_rate = f1(args);
    let threshold = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    Ok(StrykeValue::integer(if pf_rate > threshold { 1 } else { 0 }))
}

/// Demand-paging cost: page_fault_service_time = (1 - p)·mem_access + p·fault_time
/// where p = page-fault rate. Args: p, mem_access_ns, fault_service_us.
fn builtin_os_demand_paging_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args).clamp(0.0, 1.0);
    let mem = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let fault = args.get(2).map(|v| v.to_number()).unwrap_or(8e6);
    Ok(StrykeValue::float((1.0 - p) * mem + p * fault))
}

/// Copy-on-write check
fn builtin_os_copy_on_write_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let writable = i1(args);
    let shared = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if shared != 0 && writable != 0 { 1 } else { 0 }))
}

/// Zero-page optimization (free if all-zero)
fn builtin_os_zero_page_optimization(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let nonzero = i1(args);
    Ok(StrykeValue::integer(if nonzero == 0 { 1 } else { 0 }))
}

/// Huge page threshold
fn builtin_os_huge_page_threshold(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pages = f1(args);
    let huge_size = args.get(1).map(|v| v.to_number()).unwrap_or(512.0);
    Ok(StrykeValue::integer(if pages >= huge_size { 1 } else { 0 }))
}

/// Transparent Huge Pages (THP) decision per /sys/kernel/mm/transparent_hugepage/enabled.
/// Three modes: always (collapse all eligible 2 MB-aligned VMAs), madvise (only
/// MADV_HUGEPAGE), never. Plus defrag policy (always/madvise/defer/never).
/// Returns: 0 = use 4 KB pages, 1 = collapse to 2 MB. Args: thp_mode (0/1/2),
/// vma_madvised (0/1), aligned_2mb (0/1), defrag_mode (0..3).
fn builtin_os_transparent_hugepage(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mode = i1(args);
    let madvised = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let aligned = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    if mode == 2 || aligned == 0 { return Ok(StrykeValue::integer(0)); }
    if mode == 0 { return Ok(StrykeValue::integer(1)); }
    Ok(StrykeValue::integer(if madvised != 0 { 1 } else { 0 }))
}

/// KASAN shadow offset
fn builtin_os_kasan_shadow_offset(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let addr = i1(args) as u64;
    let offset = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0xdfff_e000_0000_0000);
    Ok(StrykeValue::integer((addr.wrapping_shr(3).wrapping_add(offset)) as i64))
}

/// KFENCE check (sample 1 in N allocations)
fn builtin_os_kfence_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::integer(if r * n < 1.0 { 1 } else { 0 }))
}

/// KFENCE alloc index
fn builtin_os_kfence_alloc_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(255);
    Ok(StrykeValue::integer(i % (n + 1).max(1)))
}

/// SLUB object size round (next power of 2)
fn builtin_os_slub_object_size_round(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(1) as u64;
    Ok(StrykeValue::integer(n.next_power_of_two() as i64))
}

/// Slab color offset (cache line distribution)
fn builtin_os_slab_color_offset(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let slot = i1(args);
    let line_size = args.get(1).map(|v| v.to_number() as i64).unwrap_or(64);
    Ok(StrykeValue::integer(slot * line_size))
}

/// Per-CPU SLAB cache batchcount: per Linux mm/slab.c, batch is min(limit, n/8)
/// for limit ≤ 32 then capped at limit. Args: object size, n_objects.
fn builtin_os_per_cpu_cache_size(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let obj_size = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let limit: f64 = if obj_size <= 256.0 { 120.0 }
                else if obj_size <= 1024.0 { 54.0 }
                else if obj_size <= 8192.0 { 27.0 }
                else if obj_size <= 32768.0 { 8.0 }
                else { 1.0 };
    Ok(StrykeValue::integer(limit.min(n / 8.0).max(1.0).round() as i64))
}

/// Buddy allocator order pick: ⌈log2(pages)⌉
fn builtin_os_buddy_order_pick(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pages = f1(args);
    if pages <= 0.0 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(pages.log2().ceil() as i64))
}

/// Memory compaction: count of migratable pages moved to coalesce free regions.
/// Returns updated isolated_pages = prev + freshly_migratable - rejected.
/// Args: prev_isolated, migratable, rejected.
fn builtin_os_compact_memory_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prev = i1(args);
    let migratable = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let rejected = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((prev + migratable - rejected).max(0)))
}

/// KVM VMCS field offset
fn builtin_os_kvm_vmcs_field_offset(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let field_id = i1(args);
    Ok(StrykeValue::integer(field_id * 8))
}

/// APIC IRQ priority class
fn builtin_os_apic_irq_priority(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let irq = i1(args);
    Ok(StrykeValue::integer(irq >> 4))
}

/// MSI-X vectors per device per PCIe spec: encoded in Table Size field as N-1
/// (max 2048). Each vector consumes 16 bytes in MSI-X table. Args: requested,
/// max_supported.
fn builtin_os_msi_x_vector_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let requested = i1(args).max(1);
    let max_supported = args.get(1).map(|v| v.to_number() as i64).unwrap_or(2048);
    Ok(StrykeValue::integer(requested.min(max_supported).min(2048)))
}

/// IOMMU domain mapping: 4-level page table walk index. Given IOVA, depth.
/// PTE_index_at_level = (iova >> (12 + 9·(depth - level))) & 0x1FF.
fn builtin_os_iommu_domain_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let iova = i1(args) as u64;
    let level = args.get(1).map(|v| v.to_number() as u32).unwrap_or(0);
    let max_level = args.get(2).map(|v| v.to_number() as u32).unwrap_or(3);
    let shift = 12 + 9 * (max_level - level);
    Ok(StrykeValue::integer(((iova >> shift) & 0x1ff) as i64))
}

/// PCI bus address
fn builtin_os_pci_bus_address(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bus = i1(args);
    let dev = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let func = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((bus << 8) | (dev << 3) | func))
}

/// ACPI state transition (S0..S5 cost)
fn builtin_os_acpi_state_transition(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let from = i1(args);
    let to = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((to - from).abs()))
}

/// CPUFreq governor step
fn builtin_os_cpufreq_governor_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let load = f1(args);
    let f_max = args.get(1).map(|v| v.to_number()).unwrap_or(3000.0);
    Ok(StrykeValue::float(load * f_max))
}

/// Intel P-state HWP target: HWP_REQUEST MSR 0x774 with min/max/desired/EPP.
/// Driver picks freq via:
///   target = clamp(busy% · max_perf · (1 − epp_factor), min_perf, max_perf)
/// where epp_factor depends on EPP byte (0..255, 0=performance, 255=power_save).
/// Args: busy_percent, max_perf, min_perf, epp (0..255).
fn builtin_os_intel_pstate_target(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let busy = f1(args).clamp(0.0, 1.0);
    let max_perf = args.get(1).map(|v| v.to_number()).unwrap_or(3000.0);
    let min_perf = args.get(2).map(|v| v.to_number()).unwrap_or(800.0);
    let epp = args.get(3).map(|v| v.to_number()).unwrap_or(128.0).clamp(0.0, 255.0);
    let epp_factor = epp / 1024.0;
    let raw = busy * max_perf * (1.0 - epp_factor);
    Ok(StrykeValue::float(raw.clamp(min_perf, max_perf)))
}

/// AMD P-state CPPC: CPPC_REQUEST MSR 0xc00102b3 with desired_perf, min_perf,
/// max_perf, energy_perf_preference (0..255). Differs from Intel in scaled
/// units: AMD uses lowest_freq..highest_freq normalized 0..255 (Capability
/// Performance Computing) instead of Intel's HWP perf scale.
///   target = scale_lerp(min_perf, max_perf, busy_percent · (1 − epp/255)).
fn builtin_os_amd_pstate_target(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let busy = f1(args).clamp(0.0, 1.0);
    let max_perf = args.get(1).map(|v| v.to_number()).unwrap_or(3000.0);
    let min_perf = args.get(2).map(|v| v.to_number()).unwrap_or(800.0);
    let epp = args.get(3).map(|v| v.to_number()).unwrap_or(128.0).clamp(0.0, 255.0);
    let alpha = busy * (1.0 - epp / 255.0);
    Ok(StrykeValue::float(min_perf + alpha * (max_perf - min_perf)))
}

/// Thermal zone trip point
fn builtin_os_thermal_zone_trip(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let temp = f1(args);
    let trip = args.get(1).map(|v| v.to_number()).unwrap_or(85.0);
    Ok(StrykeValue::integer(if temp >= trip { 1 } else { 0 }))
}

/// Throttle temperature (clamps frequency)
fn builtin_os_throttle_temperature(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f = f1(args);
    let throttle = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(f * throttle.clamp(0.0, 1.0)))
}

/// Battery capacity percent
fn builtin_os_battery_capacity_pct(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cur = f1(args);
    let max = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if max == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(100.0 * cur / max))
}

/// Powertop wakeup score = wakeups_per_second · power_per_wakeup_J. Higher is
/// worse. Args: wakeups, total_seconds, power_watts.
fn builtin_os_powertop_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let wakeups = f1(args);
    let secs = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-6);
    let p_watts = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(wakeups / secs * p_watts))
}

/// Idle state select (deepest below latency budget)
fn builtin_os_idle_state_select(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let predicted_idle = f1(args);
    let depths = arg_to_vec(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let mut best = -1_i64;
    for (i, d) in depths.iter().enumerate() {
        if d.to_number() <= predicted_idle { best = i as i64; }
    }
    Ok(StrykeValue::integer(best))
}

/// C-state residency percentage
fn builtin_os_c_state_residency(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let in_state = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(in_state / total))
}

/// P-state voltage at frequency f: V(f) = V_min + k·(f - f_min). Linear DVFS curve.
fn builtin_os_p_state_voltage(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f = f1(args);
    let f_min = args.get(1).map(|v| v.to_number()).unwrap_or(800.0);
    let v_min = args.get(2).map(|v| v.to_number()).unwrap_or(0.7);
    let k = args.get(3).map(|v| v.to_number()).unwrap_or(0.0004);
    Ok(StrykeValue::float(v_min + k * (f - f_min).max(0.0)))
}

/// DVFS step (power = α C V² f)
fn builtin_os_dvfs_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = f1(args);
    let f = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(c * v * v * f))
}

/// Voltage scaling power ratio: P_new/P_old = (V_new/V_old)² (per dynamic
/// power ∝ V²). Args: V_old, V_new.
fn builtin_os_voltage_scaling_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_old = f1(args);
    let v_new = args.get(1).map(|v| v.to_number()).unwrap_or(v_old);
    if v_old == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let r = v_new / v_old;
    Ok(StrykeValue::float(r * r))
}

/// Frequency scaling delay: each f-change has hardware-imposed transition latency
/// proportional to |Δf|/slew_rate. Args: f_old, f_new, slew_rate (MHz/μs).
fn builtin_os_frequency_scaling_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f_old = f1(args);
    let f_new = args.get(1).map(|v| v.to_number()).unwrap_or(f_old);
    let slew = args.get(2).map(|v| v.to_number()).unwrap_or(100.0).max(1.0);
    Ok(StrykeValue::float((f_new - f_old).abs() / slew))
}

/// inotify event count: events generated for an array of [mask] flags
/// (IN_MODIFY=2, IN_ATTRIB=4, IN_CLOSE_WRITE=8, IN_CREATE=256, IN_DELETE=512).
/// Returns popcount of OR'd mask = total event types being watched.
fn builtin_os_inotify_event_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mask = i1(args) as u64;
    Ok(StrykeValue::integer(mask.count_ones() as i64))
}

/// epoll_ctl total ops = ADD + MOD + DEL counts. Args: array [add, mod, del].
fn builtin_os_epoll_ctl_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.iter().map(|x| x.to_number() as i64).sum()))
}

/// io_uring SQE count = head - tail in submission queue ring. Args: head, tail, mask.
fn builtin_os_io_uring_sqe_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let head = i1(args) as u32;
    let tail = args.get(1).map(|v| v.to_number() as u32).unwrap_or(0);
    Ok(StrykeValue::integer(tail.wrapping_sub(head) as i64))
}

/// io_uring CQE count = tail - head in completion queue ring.
fn builtin_os_io_uring_cqe_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let head = i1(args) as u32;
    let tail = args.get(1).map(|v| v.to_number() as u32).unwrap_or(0);
    Ok(StrykeValue::integer(tail.wrapping_sub(head) as i64))
}

/// kqueue event count: kevent[] entries with EV_ADD or EV_RECEIPT bit set.
fn builtin_os_kqueue_event_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let flags = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let count = flags.iter().filter(|f| {
        let v = f.to_number() as u64;
        v & 0x0001 != 0 || v & 0x0040 != 0
    }).count();
    Ok(StrykeValue::integer(count as i64))
}

/// Journal size in bytes: per-file rotation enforces SystemMaxFileSize (default
/// 128 MB). Total = files · max_file. Args: file_count, max_file_mb.
fn builtin_os_systemd_journal_size(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let files = f1(args);
    let max_mb = args.get(1).map(|v| v.to_number()).unwrap_or(128.0);
    Ok(StrykeValue::float(files * max_mb * 1024.0 * 1024.0))
}

/// dmesg severity level (RFC 5424: 0=emerg..7=debug)
fn builtin_os_dmesg_severity_level(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::integer(i1(args).clamp(0, 7)))
}

/// Audit event priority: type ranges per linux/audit.h —
///   1100..1199 KERNEL, 1200..1299 USER, 1300..1399 LOGIN, 1400..1499 AVC,
///   1500..1599 INTEGRITY. Returns priority class 1..5 (kernel highest).
fn builtin_os_audit_event_priority(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = i1(args);
    if (1100..1200).contains(&t) { Ok(StrykeValue::integer(1)) }
    else if (1400..1500).contains(&t) { Ok(StrykeValue::integer(2)) }
    else if (1500..1600).contains(&t) { Ok(StrykeValue::integer(3)) }
    else if (1300..1400).contains(&t) { Ok(StrykeValue::integer(4)) }
    else { Ok(StrykeValue::integer(5)) }
}

/// AppArmor profile active: matches when active mode is enforce (1) or complain (2).
/// Args: mode_int (0=unconfined, 1=enforce, 2=complain).
fn builtin_os_apparmor_profile_active(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mode = i1(args);
    Ok(StrykeValue::integer(if mode == 1 || mode == 2 { 1 } else { 0 }))
}

/// SELinux context match
fn builtin_os_selinux_context_match(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = i1(args);
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if a == b { 1 } else { 0 }))
}

/// Smack label dominance check (Schaufler 2007): unlike SELinux's exact-context
/// match, Smack uses dominance with privileged labels (`_` floor allows anyone,
/// `^` hat is privileged-read, `*` star grants any-access, `?` question denies
/// unless explicitly allowed).
///
/// Returns 1 if subject_label dominates object_label per Smack rules.
/// Args: subj_label (encoded 0..3 for _,^,*,?), obj_label, exact_match (0/1).
fn builtin_os_smack_label_compare(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let subj = i1(args);
    let obj = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let exact = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    if exact != 0 { return Ok(StrykeValue::integer(if subj == obj { 1 } else { 0 })); }
    if obj == 0 || subj == 2 { return Ok(StrykeValue::integer(1)); }
    if subj == 3 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(if subj == obj || (subj == 1 && obj != 3) { 1 } else { 0 }))
}

/// Capability check (linux capabilities bitmask)
fn builtin_os_capability_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mask = i1(args) as u64;
    let cap = args.get(1).map(|v| v.to_number() as u64).unwrap_or(0);
    Ok(StrykeValue::integer(if mask & (1u64 << cap) != 0 { 1 } else { 0 }))
}

/// Seccomp filter step
fn builtin_os_seccomp_filter_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let action = i1(args);
    Ok(StrykeValue::integer(action))
}

/// Namespace isolation: count of unshared namespace types per linux clone flags
/// (CLONE_NEWUSER=0x10000000, CLONE_NEWNS=0x20000, CLONE_NEWPID=0x20000000,
/// CLONE_NEWNET=0x40000000, CLONE_NEWUTS=0x4000000, CLONE_NEWIPC=0x8000000,
/// CLONE_NEWCGROUP=0x2000000, CLONE_NEWTIME=0x80). Args: flags.
fn builtin_os_namespace_isolation(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let flags = i1(args) as u64;
    let ns_mask = 0x10000000u64 | 0x20000 | 0x20000000 | 0x40000000
                | 0x4000000 | 0x8000000 | 0x2000000 | 0x80;
    Ok(StrykeValue::integer((flags & ns_mask).count_ones() as i64))
}

/// cgroup v1 controller count from `/sys/fs/cgroup/<ctl>/`. The 12 standard v1
/// controllers: cpu, cpuacct, cpuset, blkio, devices, freezer, hugetlb,
/// memory, net_cls, net_prio, perf_event, pids. Args: bitmask of enabled.
fn builtin_os_cgroup_v1_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mask = i1(args) as u64;
    Ok(StrykeValue::integer((mask & 0xfff).count_ones() as i64))
}

/// cgroup v2 controller count: 7 standard v2 controllers (cpu, memory, io,
/// pids, cpuset, hugetlb, rdma). Args: bitmask.
fn builtin_os_cgroup_v2_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mask = i1(args) as u64;
    Ok(StrykeValue::integer((mask & 0x7f).count_ones() as i64))
}

/// pid_max value
fn builtin_os_pid_max_value(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::integer(4_194_304))
}

/// kernel.threads-max default: max(20, RAM_KB / (8 · THREAD_SIZE_KB)) per
/// fork.c. Args: RAM in KB, optional thread-stack size in KB.
fn builtin_os_thread_max_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ram_kb = f1(args);
    let stack_kb = args.get(1).map(|v| v.to_number()).unwrap_or(16.0).max(1.0);
    Ok(StrykeValue::integer((ram_kb / (8.0 * stack_kb)).max(20.0).floor() as i64))
}

/// fs.file-max default: NR_FILE_STAT pages → max(8192, RAM_pages / 10) per
/// kernel/sysctl.c. Args: RAM in pages.
fn builtin_os_file_max_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ram_pages = f1(args);
    Ok(StrykeValue::integer((ram_pages / 10.0).max(8192.0).floor() as i64))
}

/// Open files count: sum across [pid].nr_open arrays.
fn builtin_os_open_files_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.iter().map(|x| x.to_number() as i64).sum()))
}

/// Socket max: net.core.somaxconn defaults to 4096 since 5.4. Returns the
/// actual cap — somaxconn min(input, hard_max).
fn builtin_os_socket_max_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let want = i1(args);
    let hard_max = args.get(1).map(|v| v.to_number() as i64).unwrap_or(65535);
    Ok(StrykeValue::integer(want.min(hard_max).max(0)))
}

/// inotify watches per user max: fs.inotify.max_user_watches; default
/// max(8192, ram_pages / 32). Args: RAM in pages.
fn builtin_os_inotify_max_watches(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ram_pages = f1(args);
    Ok(StrykeValue::integer((ram_pages / 32.0).max(8192.0).floor() as i64))
}

/// OOM kill score per kernel/mm/oom_kill.c oom_badness(). Combines RSS+swap+pgtables
/// against total memory (1000 = full memory), then applies oom_score_adj as offset
/// in 0..1000 units. Kthreads / init / OOM_SCORE_ADJ_MIN immune. Returns final
/// kill score (0 = immune, else relative). Args: rss_pages, swap_pages, pgtable_pages,
/// total_pages, oom_score_adj, is_kthread (0/1), is_init (0/1).
fn builtin_os_oom_kill_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rss = f1(args);
    let swap = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let pgt = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let total = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let adj = args.get(4).map(|v| v.to_number() as i64).unwrap_or(0);
    let is_kthread = args.get(5).map(|v| v.to_number() as i64).unwrap_or(0);
    let is_init = args.get(6).map(|v| v.to_number() as i64).unwrap_or(0);
    if is_kthread != 0 || is_init != 0 || adj <= -1000 { return Ok(StrykeValue::integer(0)); }
    let mem_score = (1000.0 * (rss + swap + pgt) / total).round() as i64;
    let total_score = (mem_score + adj).clamp(1, 1000);
    Ok(StrykeValue::integer(total_score))
}

/// zswap compress ratio
fn builtin_os_zswap_compress_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let raw = f1(args);
    let compressed = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if compressed == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(raw / compressed))
}

/// zram effective compression ratio includes zsmalloc per-class slab fragmentation
/// (typical ~12% over-allocation) — distinct from zswap's zpool-managed compression.
/// effective = raw / (compressed · (1 + frag_overhead)). Args: raw, compressed,
/// frag_overhead (default 0.12 per zsmalloc avg fragmentation).
fn builtin_os_zram_compress_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let raw = f1(args);
    let compressed = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let frag = args.get(2).map(|v| v.to_number()).unwrap_or(0.12).max(0.0);
    let denom = compressed * (1.0 + frag);
    if denom <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(raw / denom))
}

/// Swap pressure score = swap_used / swap_total + (1 - free_ram / total_ram).
fn builtin_os_swap_pressure_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let swap_used = f1(args);
    let swap_total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let free_ram = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let total_ram = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(swap_used / swap_total + 1.0 - free_ram / total_ram))
}

/// PSI pressure stall step
fn builtin_os_pressure_stall_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let stall_time = f1(args);
    let total_time = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total_time == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(stall_time / total_time))
}

/// PSI avg10/avg60/avg300 per kernel/sched/psi.c: exponentially-weighted moving
/// average of the per-sample stall ratio with sampling interval Δt = 2 s and
/// time constants 10, 60, 300 s. The kernel uses fixed-point with FIXED_1=2048
/// and EXP_N = round(2048 · exp(−Δt/N)):
///   EXP_10 = 1677, EXP_60 = 1981, EXP_300 = 2034.
/// Update rule: new = (old · EXP + sample · (FIXED_1 − EXP)) / FIXED_1.
/// Args: prev_avg, sample (current stall ratio in [0,1]).
fn builtin_os_psi_avg10_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prev = f1(args);
    let sample = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let exp_n = 1677.0_f64 / 2048.0;
    Ok(StrykeValue::float(prev * exp_n + sample * (1.0 - exp_n)))
}

fn builtin_os_psi_avg60_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prev = f1(args);
    let sample = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let exp_n = 1981.0_f64 / 2048.0;
    Ok(StrykeValue::float(prev * exp_n + sample * (1.0 - exp_n)))
}

fn builtin_os_psi_avg300_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prev = f1(args);
    let sample = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let exp_n = 2034.0_f64 / 2048.0;
    Ok(StrykeValue::float(prev * exp_n + sample * (1.0 - exp_n)))
}

/// /proc/loadavg first column. Linux fixed-point: load = (active_tasks · CONST) ·
/// EXP_N / 2048, where CONST = 2048, EXP_N = ⌊2048·exp(-Δt/N·60)⌋. Args:
/// runnable_count (R + D states), prev_load_fp.
fn builtin_os_load_proc_avg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let active = f1(args);
    let prev = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let exp1 = (-5.0_f64 / 60.0).exp();
    Ok(StrykeValue::float(prev * exp1 + active * (1.0 - exp1)))
}

/// User CPU% load: user_jiffies / total_jiffies · 100.
fn builtin_os_load_user_avg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let user = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(100.0 * user / total))
}

/// I/O wait fraction: iowait_jiffies / total_jiffies · 100.
fn builtin_os_load_iowait_avg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let iowait = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(100.0 * iowait / total))
}
