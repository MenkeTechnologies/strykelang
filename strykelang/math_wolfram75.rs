// Batch 75 — NetworkX graph algorithms: shortest paths, MSTs, flows, cuts,
// centralities, communities, traversals, isomorphism.

fn b75_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

fn b75_to_ints(v: &PerlValue) -> Vec<i64> {
    arg_to_vec(v).iter().map(|x| x.to_number() as i64).collect()
}

// ───── shortest paths ─────

/// Dijkstra relaxation step: returns new tentative dist d[u] + w(u,v) if smaller.
fn builtin_dijkstra_relax(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d_u = f1(args);
    let w_uv = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(0.0);
    let d_v = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    let cand = d_u + w_uv;
    Ok(PerlValue::float(cand.min(d_v)))
}

/// Bellman-Ford relaxation: same form as Dijkstra but allows negative weights.
fn builtin_bellman_ford_relax(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d_u = f1(args);
    let w_uv = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let d_v = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(PerlValue::float((d_u + w_uv).min(d_v)))
}

/// Floyd-Warshall update: d[i][j] = min(d[i][j], d[i][k] + d[k][j]).
fn builtin_floyd_warshall_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d_ij = f1(args);
    let d_ik = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    let d_kj = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(PerlValue::float(d_ij.min(d_ik + d_kj)))
}

/// Johnson all-pairs reweighting: w'(u,v) = w(u,v) + h(u) − h(v).
fn builtin_johnson_reweight(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w_uv = f1(args);
    let h_u = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h_v = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(w_uv + h_u - h_v))
}

/// A* expansion: f(n) = g(n) + h(n).
fn builtin_astar_search(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g_n = f1(args);
    let h_n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(0.0);
    Ok(PerlValue::float(g_n + h_n))
}

/// Bidirectional Dijkstra meeting condition: best path crosses meet vertex if
/// d_f[v] + d_b[v] is minimum across all settled v.
fn builtin_bidirectional_dijkstra(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d_f = f1(args);
    let d_b = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(PerlValue::float(d_f + d_b))
}

/// Yen's k-shortest paths step: deviation cost from spur node.
fn builtin_yen_k_shortest(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let root_cost = f1(args);
    let spur_cost = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(root_cost + spur_cost))
}

/// IDA* iterative deepening A* threshold update.
fn builtin_ida_star(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cur_threshold = f1(args);
    let min_exceeded = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(PerlValue::float(cur_threshold.max(min_exceeded)))
}

// ───── traversals ─────

/// BFS visit count given branching factor b and depth d.
fn builtin_bfs_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let b = f1(args).max(0.0);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(0.0) as i32;
    if (b - 1.0).abs() < 1e-9 { return Ok(PerlValue::float(d as f64 + 1.0)); }
    Ok(PerlValue::float((b.powi(d + 1) - 1.0) / (b - 1.0)))
}

/// DFS post-order: returns 1 if node has no unvisited children remaining.
fn builtin_dfs_postorder_done(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let unvisited = i1(args);
    Ok(PerlValue::integer(if unvisited <= 0 { 1 } else { 0 }))
}

/// Topological sort (Kahn): returns 1 if in-degree zero (ready to emit).
fn builtin_topo_kahn_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let in_degree = i1(args);
    Ok(PerlValue::integer(if in_degree == 0 { 1 } else { 0 }))
}

/// Tarjan SCC step: lowlink = min(lowlink, child_lowlink).
fn builtin_tarjan_scc_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lowlink = f1(args);
    let child = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(PerlValue::float(lowlink.min(child)))
}

/// Kosaraju 2nd-pass: emit SCC label for vertex v given reverse-graph reach
/// bits. Args: vertex index, bit-vector of reverse-reachability from current
/// root. Returns 1 if v belongs to current SCC.
fn builtin_kosaraju_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v_pos = i1(args).max(0) as usize;
    let reach = b75_to_ints(args.get(1).unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::integer(reach.get(v_pos).copied().unwrap_or(0).clamp(0, 1)))
}

// ───── MSTs ─────

/// Kruskal step: union-find merge if find(u) ≠ find(v).
fn builtin_kruskal_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let root_u = i1(args);
    let root_v = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if root_u != root_v { 1 } else { 0 }))
}

/// Prim's relaxation: update key[v] = min(key[v], w(u,v)) if v ∉ MST.
fn builtin_prim_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let key_v = f1(args);
    let w_uv = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(PerlValue::float(key_v.min(w_uv)))
}

/// Borůvka phase: per component, pick lightest outgoing edge weight. Args:
/// flat array of edge weights for one component. Returns minimum weight.
fn builtin_boruvka_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let weights = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if weights.is_empty() { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(weights.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Reverse-delete step: remove heaviest edge whose deletion preserves connectivity.
fn builtin_reverse_delete_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let connected = i1(args);
    Ok(PerlValue::integer(if connected != 0 { 1 } else { 0 }))
}

// ───── max flow / min cut ─────

/// Ford-Fulkerson augmenting path step: residual capacity after sending Δ units.
fn builtin_ford_fulkerson_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cap = f1(args);
    let flow = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((cap - flow).max(0.0)))
}

/// Edmonds-Karp BFS-augmenting path bottleneck.
fn builtin_edmonds_karp_bfs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let path_caps = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(path_caps.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Dinic blocking-flow augment along a layered DFS path: actual flow pushed
/// equals the bottleneck residual capacity along the path. Args: array of
/// edge residual capacities along one s→t path. Returns flow pushed.
fn builtin_dinic_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let path_caps = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if path_caps.is_empty() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(path_caps.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Push-relabel relabel: h[u] = 1 + min(h[v]) over residual edges.
fn builtin_push_relabel_relabel(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let min_h = i1(args);
    Ok(PerlValue::integer(1 + min_h))
}

/// Stoer-Wagner min cut phase: cut weight = w(s, t) at last vertex addition.
fn builtin_stoer_wagner_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cut_weight = f1(args);
    Ok(PerlValue::float(cut_weight))
}

/// Karger random edge contraction: select edge index via deterministic LCG
/// from seed and edge count. Args: edge count, seed. Returns chosen edge index
/// for contraction (caller merges its endpoints).
fn builtin_karger_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_edges = i1(args).max(1);
    let seed = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0) as u64;
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    s ^= s >> 33;
    Ok(PerlValue::integer((s % n_edges as u64) as i64))
}

// ───── PageRank / HITS ─────

/// PageRank iteration: PR(v) = (1−d)/N + d Σ PR(u)/L(u) for u → v.
fn builtin_pagerank_iter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let inbound_sum = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.85).clamp(0.0, 1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(PerlValue::float((1.0 - d) / n + d * inbound_sum))
}

/// HITS authority update: a(v) = Σ h(u) over u → v.
fn builtin_hits_authority(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(v.iter().sum()))
}

/// HITS hub update: h(u) = Σ a(v) over outgoing edges u → v, then normalise
/// by L2 norm of the new h-vector. Args: outgoing-authority sum, l2_norm.
fn builtin_hits_hub(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let outbound_a_sum = f1(args);
    let l2_norm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float(outbound_a_sum / l2_norm))
}

/// Personalised PageRank: source vertex gets +(1-d) bias on top.
fn builtin_personalized_pagerank(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let inbound_sum = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.85);
    let is_source = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let bias = if is_source != 0 { 1.0 - d } else { 0.0 };
    Ok(PerlValue::float(bias + d * inbound_sum))
}

// ───── centralities ─────

/// Degree centrality: deg(v) / (n − 1).
fn builtin_centrality_degree(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let deg = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(2.0).max(2.0);
    Ok(PerlValue::float(deg / (n - 1.0)))
}

/// Closeness centrality: (n − 1) / Σ d(v, u).
fn builtin_centrality_closeness(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dist_sum = f1(args).max(1e-300);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(2.0).max(2.0);
    Ok(PerlValue::float((n - 1.0) / dist_sum))
}

/// Betweenness centrality contribution: σ_st(v) / σ_st (per pair).
fn builtin_centrality_betweenness(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sigma_st_v = f1(args);
    let sigma_st = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float(sigma_st_v / sigma_st))
}

/// Eigenvector centrality power-iteration step.
fn builtin_centrality_eigenvector(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let neighbour_sum = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(PerlValue::float(neighbour_sum / lambda))
}

/// Katz centrality: x = α A x + β.
fn builtin_centrality_katz(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let neighbour_sum = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(alpha * neighbour_sum + beta))
}

/// Harmonic centrality: Σ 1/d(v, u) over u ≠ v reachable from v.
fn builtin_harmonic_centrality(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dists = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let s: f64 = dists.iter().filter(|&&d| d > 0.0).map(|d| 1.0 / d).sum();
    Ok(PerlValue::float(s))
}

/// Load centrality (Newman 2001): unit-flow random walk that splits at every
/// vertex by out-degree. Args: number of unit packets passing through v,
/// total packets sourced, out-degree of v.
fn builtin_load_centrality(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let packets_through = f1(args);
    let total_packets = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    let out_degree = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(PerlValue::float((packets_through / total_packets) / out_degree))
}

// ───── clustering ─────

/// Local clustering coefficient: 2|E_N(v)| / (k_v (k_v − 1)).
fn builtin_clustering_coefficient(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let triangles = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if k < 2.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(2.0 * triangles / (k * (k - 1.0))))
}

/// Triangle count via dot-product of neighbour bit vectors.
fn builtin_triangles_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let neighbours_a = b75_to_ints(args.first().unwrap_or(&PerlValue::array(vec![])));
    let neighbours_b = args.get(1).map(b75_to_ints).unwrap_or_default();
    let set_a: std::collections::HashSet<i64> = neighbours_a.into_iter().collect();
    let inter = neighbours_b.iter().filter(|&&n| set_a.contains(&n)).count();
    Ok(PerlValue::integer(inter as i64))
}

/// Transitivity (global clustering): 3·triangles / connected_triples.
fn builtin_transitivity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let triangles = f1(args);
    let triples = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float(3.0 * triangles / triples))
}

// ───── communities / partition ─────

/// Modularity contribution: (A_ij − k_i k_j / 2m) δ(c_i, c_j).
fn builtin_modularity_score(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a_ij = f1(args);
    let k_i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let k_j = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let m = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    let same = args.get(4).map(|v| v.to_number() as i64).unwrap_or(0);
    if same == 0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((a_ij - k_i * k_j / (2.0 * m)) / (2.0 * m)))
}

/// Louvain modularity gain ΔQ when moving v into community C.
fn builtin_louvain_gain(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sigma_in = f1(args);
    let sigma_tot = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let k_v_in = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let k_v = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let two_m = args.get(4).map(|v| v.to_number()).unwrap_or(2.0).max(1e-300);
    let term1 = (sigma_in + 2.0 * k_v_in) / two_m
        - ((sigma_tot + k_v) / two_m).powi(2);
    let term2 = sigma_in / two_m
        - (sigma_tot / two_m).powi(2)
        - (k_v / two_m).powi(2);
    Ok(PerlValue::float(term1 - term2))
}

/// Label propagation step: pick majority label among neighbours.
fn builtin_label_propagation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let labels = b75_to_ints(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut counts = std::collections::HashMap::<i64, i64>::new();
    let mut best = (labels.first().copied().unwrap_or(0), 0_i64);
    for &l in &labels {
        let c = counts.entry(l).or_insert(0);
        *c += 1;
        if *c > best.1 { best = (l, *c); }
    }
    Ok(PerlValue::integer(best.0))
}

/// Girvan-Newman edge-betweenness removal step.
fn builtin_girvan_newman(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let edge_betweenness = f1(args);
    let max_betweenness = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float(edge_betweenness / max_betweenness))
}

// ───── connectivity ─────

/// Articulation point check: low[v] ≥ disc[u] for parent u in DFS tree.
fn builtin_articulation_point(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let low_v = i1(args);
    let disc_u = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if low_v >= disc_u { 1 } else { 0 }))
}

/// Bridge edge check: low[v] > disc[u].
fn builtin_bridge_edge(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let low_v = i1(args);
    let disc_u = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if low_v > disc_u { 1 } else { 0 }))
}

/// Edge connectivity = min cut over all pairs (Whitney's theorem).
fn builtin_edge_connectivity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cut_weights = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(cut_weights.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Vertex connectivity κ(G): use Whitney bound κ(G) ≤ λ(G) ≤ δ(G); return
/// min over (min vertex-cut size, edge connectivity, min degree).
fn builtin_vertex_connectivity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let vertex_cuts = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let edge_conn = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    let min_deg = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    let v_min = vertex_cuts.iter().cloned().fold(f64::INFINITY, f64::min);
    Ok(PerlValue::float(v_min.min(edge_conn).min(min_deg)))
}

/// Biconnected components: count of articulation+1 (rough).
fn builtin_biconnected_components(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let articulations = i1(args).max(0);
    Ok(PerlValue::integer(articulations + 1))
}

/// Diameter: max shortest-path distance in graph.
fn builtin_gx_diameter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dists = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(dists.iter().cloned().fold(0.0_f64, f64::max)))
}

/// Radius: min eccentricity.
fn builtin_gx_radius(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let eccs = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(eccs.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Eccentricity: max d(v, u) over u.
fn builtin_gx_eccentricity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dists = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(dists.iter().cloned().fold(0.0_f64, f64::max)))
}

// ───── transitive closure / TSP / coloring ─────

/// Warshall transitive closure: r[i][j] |= r[i][k] & r[k][j].
fn builtin_warshall_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r_ij = i1(args);
    let r_ik = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let r_kj = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(r_ij | (r_ik & r_kj)))
}

/// TSP held-karp DP: g(S, j) = min over i ∈ S\{j} of g(S\{j}, i) + d(i, j).
fn builtin_tsp_held_karp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let dists = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let prev = args.get(1).map(b75_to_floats).unwrap_or_default();
    let n = dists.len().min(prev.len());
    let mut best = f64::INFINITY;
    for i in 0..n { best = best.min(prev[i] + dists[i]); }
    Ok(PerlValue::float(best))
}

/// TSP nearest-neighbour heuristic step: pick min unvisited distance.
fn builtin_tsp_nn_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let unvisited = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(unvisited.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Christofides TSP upper bound: MST(G) + matching(odd-degree vertices)/2.
/// Args: MST cost, weight of minimum-weight perfect matching on the odd-degree
/// subgraph. Returns ≤ 1.5·OPT for metric TSP.
fn builtin_tsp_christofides(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mst_cost = f1(args);
    let matching_cost = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(mst_cost + matching_cost / 2.0))
}

/// Greedy graph colouring: pick smallest colour not used by neighbours.
fn builtin_graph_coloring_greedy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let used = b75_to_ints(args.first().unwrap_or(&PerlValue::array(vec![])));
    let used_set: std::collections::HashSet<i64> = used.into_iter().collect();
    let mut c = 0_i64;
    while used_set.contains(&c) { c += 1; }
    Ok(PerlValue::integer(c))
}

/// Welsh-Powell algorithm step: order vertices by descending degree.
fn builtin_welsh_powell(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let degrees = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if degrees.is_empty() { return Ok(PerlValue::integer(0)); }
    let max = degrees.iter().cloned().fold(0.0_f64, f64::max);
    Ok(PerlValue::integer(max as i64 + 1))
}

// ───── isomorphism / matching ─────

/// VF2 candidate consistency check: degree of u == degree of mapped v.
fn builtin_vf2_consistent(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let deg_u = i1(args);
    let deg_v = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if deg_u == deg_v { 1 } else { 0 }))
}

/// Subgraph isomorphism: smaller graph degree must be ≤ host degree.
fn builtin_subgraph_isomorphism(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let deg_pat = i1(args);
    let deg_host = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if deg_pat <= deg_host { 1 } else { 0 }))
}

/// Maximum bipartite matching (Hungarian) augmenting path step.
fn builtin_hungarian_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let matched = i1(args);
    Ok(PerlValue::integer(matched + 1))
}

/// Hopcroft-Karp BFS-layered phase: count of vertex-disjoint augmenting paths
/// found in this phase. Args: array of layer-0 free-left-vertex degrees;
/// returns sum of saturated paths (one per qualifying free vertex).
fn builtin_hopcroft_karp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let degs = b75_to_ints(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::integer(degs.iter().filter(|&&d| d > 0).count() as i64))
}

// ───── max clique / vertex cover ─────

/// Bron-Kerbosch maximum clique recursion: returns 1 if R is maximal.
fn builtin_bron_kerbosch(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p_size = i1(args);
    let x_size = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if p_size == 0 && x_size == 0 { 1 } else { 0 }))
}

/// Minimum vertex cover via König's theorem: n − max matching for bipartite.
fn builtin_min_vertex_cover(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let max_matching = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer((n - max_matching).max(0)))
}

/// Independent set size: n − vertex cover.
fn builtin_max_independent_set(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let vc = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer((n - vc).max(0)))
}

/// Dominating set greedy step: pick vertex covering most uncovered.
fn builtin_dominating_set_greedy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let uncovered = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    Ok(PerlValue::float(uncovered.iter().cloned().fold(0.0_f64, f64::max)))
}

/// Hamiltonian cycle DP: 2^n · n table state.
fn builtin_hamiltonian_path(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).clamp(1, 20);
    Ok(PerlValue::integer((1_i64 << n) * n))
}

// ───── flow / spanning ─────

/// Minimum Steiner tree heuristic (Mehlhorn): cost = MST(metric closure).
fn builtin_min_steiner_tree(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let metric_mst = f1(args);
    Ok(PerlValue::float(metric_mst))
}

/// k-shortest spanning trees: weight of k-th smallest spanning tree.
fn builtin_k_shortest_spanning(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut weights = b75_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    weights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(PerlValue::float(weights.get(k).copied().unwrap_or(f64::INFINITY)))
}

/// Random walk hitting probability: stationary π(v) = deg(v) / 2m.
fn builtin_random_walk_hitting(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let deg = f1(args);
    let two_m = args.get(1).map(|v| v.to_number()).unwrap_or(2.0).max(1e-300);
    Ok(PerlValue::float(deg / two_m))
}

/// SimRank similarity: s(a, b) = (C / (|I(a)| · |I(b)|)) · Σ s(I_i(a), I_j(b)).
fn builtin_simrank(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let inner_sum = f1(args);
    let i_a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let i_b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let c = args.get(3).map(|v| v.to_number()).unwrap_or(0.8);
    Ok(PerlValue::float(c * inner_sum / (i_a * i_b)))
}
