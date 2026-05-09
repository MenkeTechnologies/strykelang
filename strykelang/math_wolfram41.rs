// Batch 41 — combinatorial optimization, graph algorithms, scheduling, packing.

fn b41_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// TSP lower bound from MST weight
fn builtin_tsp_lower_bound_mst(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mst_weight = f1(args);
    Ok(PerlValue::float(mst_weight))
}

/// Held-Karp step (memoized DP cell)
fn builtin_tsp_held_karp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prev = f1(args);
    let edge_w = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(prev + edge_w))
}

/// Christofides ratio bound 3/2 · OPT
fn builtin_christofides_ratio_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let opt = f1(args);
    Ok(PerlValue::float(1.5 * opt))
}

/// 2-opt swap delta: change in tour length when reversing (i+1..=j)
fn builtin_two_opt_swap_delta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d_ab = f1(args);
    let d_cd = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let d_ac = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d_bd = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(d_ac + d_bd - d_ab - d_cd))
}

/// Or-opt delta (move segment of size k)
fn builtin_or_opt_delta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let removed = f1(args);
    let added = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(added - removed))
}

/// 3-opt delta: best of 7 reconnection options
fn builtin_three_opt_delta(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Lin-Kernighan step: improvement gain
fn builtin_lin_kernighan_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = f1(args);
    let g_next = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(g.max(g_next)))
}

/// Nearest neighbor tour step: pick min-edge unvisited
fn builtin_nearest_neighbor_tour_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut best = (0_usize, f64::INFINITY);
    for (i, &x) in v.iter().enumerate() {
        if x > 0.0 && x < best.1 { best = (i, x); }
    }
    Ok(PerlValue::integer(best.0 as i64))
}

/// Greedy edge tour (Kruskal-style on edges)
fn builtin_greedy_edge_tour(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let edges = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(edges.iter().sum()))
}

/// Nearest insertion step (smallest-distance insertion)
fn builtin_nearest_insertion_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Farthest insertion step
fn builtin_farthest_insertion_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// Cheapest-insertion TSP heuristic (Rosenkrantz et al. 1977): for each candidate
/// vertex v ∉ tour, evaluate the smallest insertion cost over all tour edges
/// (i, j): Δ(v, i, j) = d(i, v) + d(v, j) − d(i, j). Pick the (v*, i*, j*)
/// minimizing Δ. Differs from nearest-insertion (which picks v minimizing
/// distance to existing tour). Args: array of insertion-Δ values across
/// candidate (v, i, j) triples; returns the minimum.
fn builtin_cheapest_insertion_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    if v.is_empty() { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(v.iter().map(|x| x.to_number()).fold(f64::INFINITY, f64::min)))
}

/// Ford-Fulkerson augment step: augment by min residual along path
fn builtin_max_flow_ford_fulkerson_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let path_capacities = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if path_capacities.is_empty() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(path_capacities.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Edmonds-Karp step (BFS shortest augmenting path)
fn builtin_edmonds_karp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_max_flow_ford_fulkerson_step(args)
}

/// Dinic **blocking-phase** throughput: sum of flow pushed along each augmenting
/// path in one blocking DFS (caller supplies nonnegative per-path flow amounts).
fn builtin_dinic_blocking_flow(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if v.is_empty() {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float(v.iter().filter(|x| **x >= 0.0).sum()))
}

/// Push–relabel **push** at `u` toward `v`: excess left at `u` after pushing
/// `min(excess(u), residual(u,v))`. Args: excess at `u`, residual capacity `c_f(u,v)`.
fn builtin_push_relabel_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let excess = f1(args);
    let res_cap = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(0.0);
    let push = excess.min(res_cap);
    Ok(PerlValue::float(excess - push))
}

/// Boykov–Kolmogorov augmentation bookkeeping: new cumulative flow value given
/// path bottleneck, prior total flow, and optional capacity used for **orphan**
/// repair (default `0`). Returns `prev_flow + max(0, bottleneck - orphan_cost)`.
fn builtin_boykov_kolmogorov_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bottleneck = f1(args);
    let prev_flow = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let orphan_cost = args.get(2).map(|v| v.to_number()).unwrap_or(0.0).max(0.0);
    Ok(PerlValue::float(prev_flow + (bottleneck - orphan_cost).max(0.0)))
}

/// Stoer-Wagner global mincut step
fn builtin_mincut_stoer_wagner(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cuts = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if cuts.is_empty() { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(cuts.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Gomory-Hu tree query: for any pair (s, t) in V, the min s-t cut equals the
/// minimum edge weight on the unique s-t path in the GH tree T. Different from
/// Stoer-Wagner (which finds ONE global min cut). Args: array of edge weights
/// along the tree path from s to t. Returns the bottleneck.
fn builtin_gomory_hu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let weights = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    if weights.is_empty() { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(weights.iter().map(|x| x.to_number()).fold(f64::INFINITY, f64::min)))
}

/// Karger random contraction edge selection
fn builtin_karger_contract_edge(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let weights = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let total: f64 = weights.iter().sum();
    if total == 0.0 { return Ok(PerlValue::integer(0)); }
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5) * total;
    let mut acc = 0.0;
    for (i, &w) in weights.iter().enumerate() {
        acc += w;
        if acc >= r { return Ok(PerlValue::integer(i as i64)); }
    }
    Ok(PerlValue::integer((weights.len() as i64).max(1) - 1))
}

/// Karger min-cut probability bound: P[success] ≥ 1 / C(n, 2)
fn builtin_karger_min_cut_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    if n < 2.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(2.0 / (n * (n - 1.0))))
}

/// Bipartite maximum matching — same as `hopcroft_karp` (edges, `n_left`, `n_right`).
fn builtin_maximum_bipartite_matching(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_hopcroft_karp(args)
}

/// First Hopcroft–Karp **phase** (empty initial matching): number of matches added
/// in one BFS layering + blocking DFS sweep (vertex-disjoint shortest augmentations).
fn builtin_hopcroft_karp_phase(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let edges = parse_edges_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n_left = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let n_right = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let aug = hopcroft_karp_first_phase_augmentations(&edges, n_left, n_right);
    Ok(PerlValue::integer(aug as i64))
}

/// Cardinality after one **successful** augmentation in general (non-bipartite)
/// matching: size increases by exactly 1 when an augmenting path exists. Args: current
/// size `m`, non-zero second arg iff a path was found and used this step.
fn builtin_blossom_match_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    let aug = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let inc = if aug != 0.0 { 1.0 } else { 0.0 };
    Ok(PerlValue::float(m + inc))
}

/// Kuhn (Hungarian) weighted matching step
fn builtin_weighted_match_kuhn_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cost = f1(args);
    let row_min = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((cost - row_min).max(0.0)))
}

/// Hungarian method step (assignment problem reduction)
fn builtin_hungarian_method_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let row_min = m.iter().cloned().fold(f64::INFINITY, f64::min);
    Ok(PerlValue::float(m.iter().sum::<f64>() - row_min * m.len() as f64))
}

/// Jonker-Volgenant LAP solver (1987): shortest-augmenting-path with reduced
/// costs c̃_ij = c_ij − u_i − v_j. One step: find shortest aug-path from
/// unassigned row r₀ via Dijkstra over reduced costs, then dual-update
/// u_i ← u_i + (d_min − d_i), v_j ← v_j + (d_min − d_j) for visited rows/cols.
/// Distinct from Hungarian (J-V is O(n³) but with much smaller constants and a
/// shortest-path framing, not "find smallest uncovered then row-cover").
/// Args: c̃ array (length n), prev d_min, prev d_max.
fn builtin_ap_jonker_volgenant_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let prev_min = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if c.is_empty() { return Ok(PerlValue::float(prev_min)); }
    let d_min = c.iter().map(|x| x.to_number()).fold(f64::INFINITY, f64::min);
    Ok(PerlValue::float((d_min - prev_min).max(0.0)))
}

/// Assignment lower bound (LP relaxation)
fn builtin_assignment_lower_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let costs = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(costs.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Job shop makespan lower bound (max processing time per machine)
fn builtin_job_shop_makespan_lower(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let times = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(times.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// Flow shop Johnson rule step (m=2 machines)
fn builtin_flow_shop_johnson_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p1 = f1(args);
    let p2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(p1.min(p2)))
}

/// Parallel machine LPT (Longest Processing Time first)
fn builtin_parallel_machine_lpt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut p = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let m = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    p.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let mut load = vec![0.0; m];
    for x in p {
        let i = load.iter().enumerate().min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|(i, _)| i).unwrap_or(0);
        load[i] += x;
    }
    Ok(PerlValue::float(load.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// Parallel machine SPT (Shortest Processing Time first)
fn builtin_parallel_machine_spt(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut p = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    p.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut total_completion = 0.0;
    let mut acc = 0.0;
    for x in p { acc += x; total_completion += acc; }
    Ok(PerlValue::float(total_completion))
}

/// List scheduling step (Graham): assign to least-loaded machine
fn builtin_list_scheduling_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let loads = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(loads.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Graham 2-approximation bound: 2 - 1/m
fn builtin_graham_2approx_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    if m == 0.0 { return Ok(PerlValue::float(2.0)); }
    Ok(PerlValue::float(2.0 - 1.0 / m))
}

/// CHC (Coffman-Hopcroft-Cyrus) bound: makespan ≥ Σpᵢ/m
fn builtin_chc_bound_makespan(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let total = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if m == 0.0 { return Ok(PerlValue::float(total)); }
    Ok(PerlValue::float(total / m))
}

/// Bin packing first fit count
fn builtin_bin_packing_first_fit(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let items = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let cap = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut bins: Vec<f64> = Vec::new();
    for it in items {
        let mut placed = false;
        for b in bins.iter_mut() {
            if *b + it <= cap { *b += it; placed = true; break; }
        }
        if !placed { bins.push(it); }
    }
    Ok(PerlValue::integer(bins.len() as i64))
}

/// Best fit
fn builtin_bin_packing_best_fit(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let items = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let cap = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut bins: Vec<f64> = Vec::new();
    for it in items {
        let mut best = (usize::MAX, 0.0);
        for (i, b) in bins.iter().enumerate() {
            let leftover = cap - *b - it;
            if leftover >= 0.0 && (best.0 == usize::MAX || leftover < best.1) {
                best = (i, leftover);
            }
        }
        if best.0 == usize::MAX { bins.push(it); } else { bins[best.0] += it; }
    }
    Ok(PerlValue::integer(bins.len() as i64))
}

/// Next fit
fn builtin_bin_packing_next_fit(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let items = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let cap = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut bins = 1_i64;
    let mut current = 0.0;
    for it in items {
        if current + it > cap { bins += 1; current = it; } else { current += it; }
    }
    Ok(PerlValue::integer(bins))
}

/// Bin packing L1 lower bound = ⌈Σwᵢ / cap⌉
fn builtin_bin_packing_lower_bound_l1(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let items = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let cap = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if cap == 0.0 { return Ok(PerlValue::integer(0)); }
    Ok(PerlValue::integer((items.iter().sum::<f64>() / cap).ceil() as i64))
}

/// Multidim packing step (sum each dimension)
fn builtin_multidim_packing_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dims = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(dims.iter().sum()))
}

/// 0/1 knapsack DP value
fn builtin_knapsack_01_dp_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prev = f1(args);
    let take = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(prev.max(take)))
}

/// Unbounded knapsack DP step
fn builtin_knapsack_unbounded_dp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dp_w = f1(args);
    let val = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let dp_w_minus_w = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(dp_w.max(dp_w_minus_w + val)))
}

/// Fractional knapsack greedy step (value-density)
fn builtin_knapsack_fractional_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if w == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(v / w))
}

/// Branch & bound knapsack step (fractional bound)
fn builtin_knapsack_branch_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lb = f1(args);
    let added = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(lb + added))
}

/// Knapsack LP relaxation value
fn builtin_knapsack_lp_relaxation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().sum()))
}

/// Multi-knapsack step (greedy on min remaining capacity)
fn builtin_multi_knapsack_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let caps = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let item = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if let Some(min_idx) = caps.iter().enumerate().filter(|(_, &c)| c >= item)
        .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i) {
        return Ok(PerlValue::integer(min_idx as i64));
    }
    Ok(PerlValue::integer(-1))
}

/// Quadratic assignment step Σ f_ij d_{p(i)p(j)}
fn builtin_quadratic_assignment_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f_val = f1(args);
    let d_val = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(f_val * d_val))
}

/// QAP lower bound (Gilmore-Lawler)
fn builtin_qap_lower_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().sum::<f64>() / 2.0))
}

/// Graph coloring DSATUR step: pick vertex with max saturation
fn builtin_graph_coloring_dsatur_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let saturations = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut best = (0_usize, f64::NEG_INFINITY);
    for (i, &s) in saturations.iter().enumerate() {
        if s > best.1 { best = (i, s); }
    }
    Ok(PerlValue::integer(best.0 as i64))
}

/// Welsh-Powell coloring step: order by descending degree
fn builtin_graph_coloring_welsh_powell(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let degrees = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut idx: Vec<usize> = (0..degrees.len()).collect();
    idx.sort_by(|a, b| degrees[*b].partial_cmp(&degrees[*a]).unwrap_or(std::cmp::Ordering::Equal));
    Ok(PerlValue::integer(idx.first().copied().unwrap_or(0) as i64))
}

/// Brooks' theorem bound: χ(G) ≤ Δ(G) for connected non-complete non-odd-cycle
fn builtin_graph_coloring_brooks_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let max_deg = f1(args);
    Ok(PerlValue::float(max_deg))
}

/// LP coloring bound (fractional chromatic)
fn builtin_graph_coloring_lp_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let omega = f1(args);
    Ok(PerlValue::float(omega))
}

/// Fractional chromatic lower bound: |V| / α(G)
fn builtin_fractional_chromatic_lower(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if alpha == 0.0 { return Ok(PerlValue::float(v)); }
    Ok(PerlValue::float(v / alpha))
}

/// List coloring (Erdős-Rubin-Taylor 1979): each vertex v has its own palette
/// L(v); proper coloring requires c(v) ∈ L(v) and c(u) ≠ c(v) for uv ∈ E.
/// One greedy step: pick smallest available color in L(v) excluding colors used
/// by already-colored neighbors. Args: my_palette, used_by_neighbors. Returns
/// chosen color (smallest free in palette \\ neighbor-set), -1 if no fit.
fn builtin_list_coloring_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let palette: Vec<i64> = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])))
        .iter().map(|x| x.to_number() as i64).collect();
    let used: std::collections::HashSet<i64> = arg_to_vec(args.get(1).unwrap_or(&PerlValue::array(vec![])))
        .iter().map(|x| x.to_number() as i64).collect();
    for &c in &palette { if !used.contains(&c) { return Ok(PerlValue::integer(c)); } }
    Ok(PerlValue::integer(-1))
}

/// Vizing edge coloring step: Δ(G) ≤ χ'(G) ≤ Δ(G) + 1
fn builtin_edge_coloring_vizing_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let max_deg = f1(args);
    Ok(PerlValue::float(max_deg + 1.0))
}

/// Clique number lower bound (greedy)
fn builtin_clique_number_lower(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let max_deg = f1(args);
    Ok(PerlValue::float(((1.0 + (1.0 + 8.0 * max_deg).sqrt()) / 2.0).floor()))
}

/// Independence number upper bound: α(G) ≤ n - χ(G) + 1
fn builtin_independence_number_upper(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let chi = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(n - chi + 1.0))
}

/// LP-rounded vertex cover
fn builtin_vertex_cover_lp_round(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::integer(if x >= 0.5 { 1 } else { 0 }))
}

/// Greedy dominating set step
fn builtin_dominating_set_greedy_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let degrees = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut best = (0_usize, f64::NEG_INFINITY);
    for (i, &d) in degrees.iter().enumerate() {
        if d > best.1 { best = (i, d); }
    }
    Ok(PerlValue::integer(best.0 as i64))
}

/// Dominating set LP bound: γ(G) ≤ ⌈n / (Δ+1)⌉
fn builtin_dominating_set_lp_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let delta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float((n / (delta + 1.0)).ceil()))
}

/// Greedy set cover step: pick set covering max uncovered
fn builtin_set_cover_greedy_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let coverage = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut best = (0_usize, f64::NEG_INFINITY);
    for (i, &c) in coverage.iter().enumerate() {
        if c > best.1 { best = (i, c); }
    }
    Ok(PerlValue::integer(best.0 as i64))
}

/// Set cover LP relaxation rounding (probabilistic)
fn builtin_set_cover_lp_round(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let log_n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::integer(if x * log_n >= 0.5 { 1 } else { 0 }))
}

/// Hitting set greedy
fn builtin_hitting_set_greedy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_set_cover_greedy_step(args)
}

/// Weighted set cover step (cost over coverage ratio)
fn builtin_weighted_set_cover_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cost = f1(args);
    let coverage = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if coverage == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(cost / coverage))
}

/// Matroid greedy step (max-weight independent set)
fn builtin_matroid_greedy_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut weights = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    weights.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    Ok(PerlValue::float(weights.iter().sum()))
}

/// Matroid intersection step
fn builtin_matroid_intersection_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(a.min(b)))
}

/// Submodular greedy step: marginal gain
fn builtin_submodular_greedy_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f_with = f1(args);
    let f_without = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((f_with - f_without).max(0.0)))
}

/// Submodular curvature bound 1/(1 - e^(-c))
fn builtin_submodular_curvature_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c = f1(args);
    let denom = 1.0 - (-c).exp();
    if denom == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / denom))
}

/// Nemhauser-Wolsey 1-1/e bound for submodular max
fn builtin_nemhauser_wolsey_bound(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let opt = f1(args);
    Ok(PerlValue::float((1.0 - 1.0 / std::f64::consts::E) * opt))
}

/// LP relax round (floor below 0.5, ceil above)
fn builtin_lp_relax_round(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::integer(if x >= 0.5 { x.ceil() as i64 } else { x.floor() as i64 }))
}

/// Branch-and-bound step: prune if LB > best UB
fn builtin_branch_and_bound_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lb = f1(args);
    let ub = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(PerlValue::integer(if lb > ub { 0 } else { 1 }))
}

/// Cutting plane step (add violated inequality)
fn builtin_cutting_plane_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lhs = f1(args);
    let rhs = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(lhs - rhs))
}

/// Gomory cut step: x_i ≥ ⌈f_i⌉ - f_i (1 - x_i)
fn builtin_gomory_cut_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f_i = f1(args);
    Ok(PerlValue::float(f_i - f_i.floor()))
}

/// Chvátal-Gomory cut: ⌊a^T x / b⌋ ≤ ⌊c/b⌋
fn builtin_chvatal_gomory_cut(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lhs = f1(args);
    let denom = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if denom == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((lhs / denom).floor()))
}

/// MIP round up
fn builtin_mixed_integer_round_up(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(f1(args).ceil() as i64))
}

/// MIP round down
fn builtin_mixed_integer_round_down(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(f1(args).floor() as i64))
}

/// SOS constraint check (special-ordered set type 1: at most one nonzero)
fn builtin_sos_constraint_check(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let nonzero = v.iter().filter(|&&x| x != 0.0).count();
    Ok(PerlValue::integer(if nonzero <= 1 { 1 } else { 0 }))
}

/// Column generation step: reduced cost = c_j - π^T A_j
fn builtin_column_generation_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c_j = f1(args);
    let pi_a = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(c_j - pi_a))
}

/// Benders decomposition step (master + subproblem cut)
fn builtin_benders_decomposition_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let master = f1(args);
    let cut = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(master + cut))
}

/// Dantzig-Wolfe step (pricing problem reduced cost)
fn builtin_dantzig_wolfe_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_column_generation_step(args)
}

/// Lagrangian relaxation step: L(λ) = c^T x + λ^T (Ax - b)
fn builtin_lagrangian_relax_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c_x = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let slack = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(c_x + lambda * slack))
}

/// Lagrangian dual: max_λ min_x L(x, λ)
fn builtin_lagrangian_dual_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l_old = f1(args);
    let l_new = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(l_old.max(l_new)))
}

/// Subgradient step size 1/(k+1)
fn builtin_subgradient_step_size(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = f1(args);
    Ok(PerlValue::float(1.0 / (k + 1.0)))
}

/// Nonlinear dual step
fn builtin_nonlinear_dual_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_lagrangian_dual_step(args)
}

/// Augmented Lagrangian step: L_ρ = f + λ^T g + (ρ/2)|g|²
fn builtin_augmented_lagrangian_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f_val = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let rho = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(f_val + lambda * g + 0.5 * rho * g * g))
}

/// ADMM primal step: x ← argmin_x f(x) + (ρ/2)|Ax - z + u|²
fn builtin_admm_primal_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = f1(args);
    let u = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(z - u))
}

/// ADMM dual step: u ← u + Ax - z
fn builtin_admm_dual_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let u = f1(args);
    let ax = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let z = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(u + ax - z))
}

/// Proximal gradient step: x ← prox_{tg}(x - t∇f)
fn builtin_proximal_gradient_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let grad = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(x - t * grad))
}

/// Nesterov accelerate step: y_k = x_k + β(x_k - x_{k-1})
fn builtin_nesterov_accelerate_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x_k = f1(args);
    let x_km1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.9);
    Ok(PerlValue::float(x_k + beta * (x_k - x_km1)))
}

/// FISTA (Beck & Teboulle 2009): x_{k+1} = prox_{t·g}(y_k − t·∇f(y_k));
/// y_{k+1} = x_{k+1} + ((t_k − 1)/t_{k+1})·(x_{k+1} − x_k); t_{k+1} = (1 + √(1+4t_k²))/2.
/// Combines Nesterov momentum with the proximal operator (NOT just Nesterov).
/// Args: y (extrapolated), grad_at_y, step t, prox_lambda. Returns x_{k+1} = soft_threshold(y - t∇f, t·λ).
fn builtin_fista_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let grad = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    let lambda = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let z = y - t * grad;
    let thresh = t * lambda;
    let mag = (z.abs() - thresh).max(0.0);
    Ok(PerlValue::float(z.signum() * mag))
}

/// ISTA: x_{k+1} = soft_threshold(x_k − t·∇f(x_k), t·λ) — proximal-gradient on
/// f(x) + λ‖x‖₁. Specifically uses the ℓ₁ prox = soft thresholding (NOT a generic
/// prox; that's the proximal_gradient_step entry point). Args: x, ∇f, t, λ.
fn builtin_ista_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let grad = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    let lambda = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let z = x - t * grad;
    let thresh = t * lambda;
    let mag = (z.abs() - thresh).max(0.0);
    Ok(PerlValue::float(z.signum() * mag))
}

/// Mirror descent step: x_{k+1} = ∇φ*(∇φ(x_k) - η g_k)
fn builtin_mirror_descent_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(x * (-eta * g).exp()))
}

/// Frank-Wolfe step γ_k = 2 / (k + 2)
fn builtin_frank_wolfe_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = f1(args);
    Ok(PerlValue::float(2.0 / (k + 2.0)))
}

/// Conditional gradient step (Frank-Wolfe alias)
fn builtin_conditional_gradient_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_frank_wolfe_step(args)
}

/// Randomized rounding for set cover (Chvátal / Vazirani): for c·ln n independent
/// trials, set x_S = 1 with probability x*_S in each round; final cover is union
/// of trials. P[element e uncovered after k=c·ln n rounds] ≤ (1 − ∏(1−x*_S))^k
/// ≤ e^{−k} = n^{−c}. Returns approximation guarantee factor c·ln n given LP
/// optimum and output ratio. Args: x_star (LP fractional value), n (universe size),
/// c (over-rounding constant default 2.0).
fn builtin_greedy_set_cover_round(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x_star = f1(args).clamp(0.0, 1.0);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(2.0).max(2.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    let rounds = c * n.ln();
    let p_cover = 1.0 - (1.0 - x_star).powf(rounds);
    Ok(PerlValue::float(p_cover))
}

/// Local search swap step: improvement
fn builtin_local_search_swap_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cur = f1(args);
    let cand = args.get(1).map(|v| v.to_number()).unwrap_or(cur);
    Ok(PerlValue::float(cand.max(cur)))
}

/// Tabu search move score: f(x') - tabu_penalty
fn builtin_tabu_search_move_score(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f_x = f1(args);
    let penalty = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(f_x - penalty))
}

/// Simulated annealing step: accept-prob = exp(-Δf / T)
fn builtin_simulated_annealing_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let delta = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if t == 0.0 { return Ok(PerlValue::integer(if delta < 0.0 { 1 } else { 0 })); }
    Ok(PerlValue::float((-delta / t).exp().min(1.0)))
}

/// Genetic crossover one-point (return crossover position)
fn builtin_genetic_crossover_one_point(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::integer((r * n) as i64))
}

/// Mutation bit flip prob
fn builtin_mutation_bit_flip_prob(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::integer(if r < p { 1 } else { 0 }))
}

/// Roulette-wheel select index from cumulative fitnesses
fn builtin_roulette_wheel_select_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cum = b41_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let total: f64 = cum.iter().sum();
    let target = r * total;
    let mut acc = 0.0;
    for (i, &c) in cum.iter().enumerate() {
        acc += c;
        if acc >= target { return Ok(PerlValue::integer(i as i64)); }
    }
    Ok(PerlValue::integer((cum.len() as i64).max(1) - 1))
}
