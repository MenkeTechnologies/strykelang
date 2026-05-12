// Batch 75 — NetworkX graph algorithms: shortest paths, MSTs, flows, cuts,
// centralities, communities, traversals, isomorphism.

fn b75_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

fn b75_to_ints(v: &StrykeValue) -> Vec<i64> {
    arg_to_vec(v).iter().map(|x| x.to_number() as i64).collect()
}

/// Tarjan/E-maxx style edge-stack biconnected components (vertex biconnected /
/// 2-vertex-connected blocks). `edges` are undirected simple pairs; multi-edges
/// are ignored after de-duplication.
fn b75_biconnected_component_count(mut n: usize, raw: &[(usize, usize)]) -> i64 {
    use std::collections::HashSet;
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for &(mut u, mut v) in raw {
        if u == v {
            continue;
        }
        if u > v {
            std::mem::swap(&mut u, &mut v);
        }
        if seen.insert((u, v)) {
            pairs.push((u, v));
            n = n.max(u + 1).max(v + 1);
        }
    }
    if n == 0 || pairs.is_empty() {
        return 0;
    }
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
    for (u, v) in pairs {
        adj[u].push(v);
        adj[v].push(u);
    }

    let mut tin: Vec<i32> = vec![-1; n];
    let mut low: Vec<i32> = vec![0; n];
    let mut visited = vec![false; n];
    let mut timer: i32 = 0;
    let mut st: Vec<(usize, usize)> = Vec::new();
    let mut bcc = 0_i64;

    #[allow(clippy::too_many_arguments)]
    fn dfs(
        v: usize,
        parent: i32,
        adj: &[Vec<usize>],
        tin: &mut [i32],
        low: &mut [i32],
        visited: &mut [bool],
        timer: &mut i32,
        st: &mut Vec<(usize, usize)>,
        bcc: &mut i64,
    ) {
        visited[v] = true;
        tin[v] = *timer;
        low[v] = *timer;
        *timer += 1;
        for &to in &adj[v] {
            if to as i32 == parent {
                continue;
            }
            if visited[to] {
                low[v] = low[v].min(tin[to]);
                if tin[to] < tin[v] {
                    st.push((v, to));
                }
            } else {
                st.push((v, to));
                dfs(
                    to,
                    v as i32,
                    adj,
                    tin,
                    low,
                    visited,
                    timer,
                    st,
                    bcc,
                );
                low[v] = low[v].min(low[to]);
                if low[to] >= tin[v] {
                    *bcc += 1;
                    loop {
                        let e = st.pop().expect("bcc pop matches push");
                        if e == (v, to) {
                            break;
                        }
                    }
                }
            }
        }
    }

    for start in 0..n {
        if !visited[start] {
            dfs(
                start,
                -1,
                &adj,
                &mut tin,
                &mut low,
                &mut visited,
                &mut timer,
                &mut st,
                &mut bcc,
            );
        }
    }
    bcc
}

// ───── shortest paths ─────

/// Dijkstra relaxation step: returns new tentative dist d[u] + w(u,v) if smaller.
fn builtin_dijkstra_relax(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d_u = f1(args);
    let w_uv = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(0.0);
    let d_v = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    let cand = d_u + w_uv;
    Ok(StrykeValue::float(cand.min(d_v)))
}

/// Bellman-Ford relaxation: same form as Dijkstra but allows negative weights.
fn builtin_bellman_ford_relax(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d_u = f1(args);
    let w_uv = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let d_v = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::float((d_u + w_uv).min(d_v)))
}

/// Floyd-Warshall update: d[i][j] = min(d[i][j], d[i][k] + d[k][j]).
fn builtin_floyd_warshall_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d_ij = f1(args);
    let d_ik = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    let d_kj = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::float(d_ij.min(d_ik + d_kj)))
}

/// Johnson all-pairs reweighting: w'(u,v) = w(u,v) + h(u) − h(v).
fn builtin_johnson_reweight(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let w_uv = f1(args);
    let h_u = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h_v = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(w_uv + h_u - h_v))
}

/// A* expansion: f(n) = g(n) + h(n).
fn builtin_astar_search(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g_n = f1(args);
    let h_n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(0.0);
    Ok(StrykeValue::float(g_n + h_n))
}

/// Bidirectional Dijkstra meeting condition: best path crosses meet vertex if
/// d_f[v] + d_b[v] is minimum across all settled v.
fn builtin_bidirectional_dijkstra(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d_f = f1(args);
    let d_b = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::float(d_f + d_b))
}

/// Yen's k-shortest paths step: deviation cost from spur node.
fn builtin_yen_k_shortest(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let root_cost = f1(args);
    let spur_cost = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(root_cost + spur_cost))
}

/// IDA* threshold update: next threshold = smallest f-value **strictly above**
/// the current bound among pruned nodes (`min_exceeded`); if none, pass
/// `inf` / omit second arg.
fn builtin_ida_star(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let _cur_threshold = f1(args);
    let min_exceeded = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::float(min_exceeded))
}

// ───── traversals ─────

/// BFS visit count given branching factor b and depth d.
fn builtin_bfs_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let b = f1(args).max(0.0);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(0.0) as i32;
    if (b - 1.0).abs() < 1e-9 { return Ok(StrykeValue::float(d as f64 + 1.0)); }
    Ok(StrykeValue::float((b.powi(d + 1) - 1.0) / (b - 1.0)))
}

/// DFS post-order: returns 1 if node has no unvisited children remaining.
fn builtin_dfs_postorder_done(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let unvisited = i1(args);
    Ok(StrykeValue::integer(if unvisited <= 0 { 1 } else { 0 }))
}

/// Topological sort (Kahn): returns 1 if in-degree zero (ready to emit).
fn builtin_topo_kahn_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let in_degree = i1(args);
    Ok(StrykeValue::integer(if in_degree == 0 { 1 } else { 0 }))
}

/// Tarjan SCC step: lowlink = min(lowlink, child_lowlink).
fn builtin_tarjan_scc_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lowlink = f1(args);
    let child = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::float(lowlink.min(child)))
}

/// Kosaraju 2nd-pass: emit SCC label for vertex v given reverse-graph reach
/// bits. Args: vertex index, bit-vector of reverse-reachability from current
/// root. Returns 1 if v belongs to current SCC.
fn builtin_kosaraju_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_pos = i1(args).max(0) as usize;
    let reach = b75_to_ints(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(reach.get(v_pos).copied().unwrap_or(0).clamp(0, 1)))
}

// ───── MSTs ─────

/// Kruskal step: union-find merge if find(u) ≠ find(v).
fn builtin_kruskal_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let root_u = i1(args);
    let root_v = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if root_u != root_v { 1 } else { 0 }))
}

/// Prim's relaxation: update key[v] = min(key[v], w(u,v)) if v ∉ MST.
fn builtin_prim_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let key_v = f1(args);
    let w_uv = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::float(key_v.min(w_uv)))
}

/// Borůvka phase: per component, pick lightest outgoing edge weight. Args:
/// flat array of edge weights for one component. Returns minimum weight.
fn builtin_boruvka_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let weights = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if weights.is_empty() { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(weights.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Reverse-delete MST helper: `still_connected` should be 1 if the graph
/// remains **connected** after deleting the candidate edge, 0 if it becomes
/// disconnected (then the edge is a bridge and must not be removed).
fn builtin_reverse_delete_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let still_connected = i1(args);
    Ok(StrykeValue::integer(if still_connected != 0 { 1 } else { 0 }))
}

// ───── max flow / min cut ─────

/// Ford-Fulkerson augmenting path step: residual capacity after sending Δ units.
fn builtin_ford_fulkerson_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cap = f1(args);
    let flow = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((cap - flow).max(0.0)))
}

/// Edmonds-Karp BFS-augmenting path bottleneck.
fn builtin_edmonds_karp_bfs(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let path_caps = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(path_caps.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Dinic blocking-flow augment along a layered DFS path: actual flow pushed
/// equals the bottleneck residual capacity along the path. Args: array of
/// edge residual capacities along one s→t path. Returns flow pushed.
fn builtin_dinic_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let path_caps = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if path_caps.is_empty() { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(path_caps.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Push–relabel relabel step (integer heights): `h[u] = 1 + min(h[v])` over
/// residual neighbors `v` with positive residual capacity. Pass neighbor
/// heights as a flat list; isolated `u` yields height 0 (no admissible edge).
fn builtin_push_relabel_relabel(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let neigh = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if neigh.is_empty() {
        return Ok(StrykeValue::integer(0));
    }
    let min_h = neigh
        .iter()
        .filter(|h| h.is_finite())
        .copied()
        .fold(f64::INFINITY, f64::min);
    if !min_h.is_finite() {
        return Ok(StrykeValue::integer(0));
    }
    Ok(StrykeValue::integer(1 + min_h.round() as i64))
}

/// Stoer–Wagner “last addition” phase weight: sum of edge weights from the
/// current cut candidate vertex to the rest of the working set `A`.
fn builtin_stoer_wagner_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ws = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(ws.iter().sum()))
}

/// Karger random edge contraction: select edge index via deterministic LCG
/// from seed and edge count. Args: edge count, seed. Returns chosen edge index
/// for contraction (caller merges its endpoints).
fn builtin_karger_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_edges = i1(args).max(1);
    let seed = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0) as u64;
    let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    s ^= s >> 33;
    Ok(StrykeValue::integer((s % n_edges as u64) as i64))
}

// ───── PageRank / HITS ─────

/// PageRank iteration: PR(v) = (1−d)/N + d Σ PR(u)/L(u) for u → v.
fn builtin_pagerank_iter(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let inbound_sum = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.85).clamp(0.0, 1.0);
    let n = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float((1.0 - d) / n + d * inbound_sum))
}

/// HITS authority update: a(v) = Σ h(u) over u → v.
fn builtin_hits_authority(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().sum()))
}

/// HITS hub update: h(u) = Σ a(v) over outgoing edges u → v, then normalise
/// by L2 norm of the new h-vector. Args: outgoing-authority sum, l2_norm.
fn builtin_hits_hub(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let outbound_a_sum = f1(args);
    let l2_norm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(StrykeValue::float(outbound_a_sum / l2_norm))
}

/// Personalised PageRank: source vertex gets +(1-d) bias on top.
fn builtin_personalized_pagerank(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let inbound_sum = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.85);
    let is_source = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let bias = if is_source != 0 { 1.0 - d } else { 0.0 };
    Ok(StrykeValue::float(bias + d * inbound_sum))
}

// ───── centralities ─────

/// Degree centrality: deg(v) / (n − 1).
fn builtin_centrality_degree(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let deg = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(2.0).max(2.0);
    Ok(StrykeValue::float(deg / (n - 1.0)))
}

/// Closeness centrality: (n − 1) / Σ d(v, u).
fn builtin_centrality_closeness(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dist_sum = f1(args).max(1e-300);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(2.0).max(2.0);
    Ok(StrykeValue::float((n - 1.0) / dist_sum))
}

/// Betweenness centrality contribution: σ_st(v) / σ_st (per pair).
fn builtin_centrality_betweenness(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sigma_st_v = f1(args);
    let sigma_st = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(StrykeValue::float(sigma_st_v / sigma_st))
}

/// Eigenvector centrality power-iteration step.
fn builtin_centrality_eigenvector(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let neighbour_sum = f1(args);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float(neighbour_sum / lambda))
}

/// Katz centrality: x = α A x + β.
fn builtin_centrality_katz(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let neighbour_sum = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(alpha * neighbour_sum + beta))
}

/// Harmonic centrality: Σ 1/d(v, u) over u ≠ v reachable from v.
fn builtin_harmonic_centrality(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dists = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let s: f64 = dists.iter().filter(|&&d| d > 0.0).map(|d| 1.0 / d).sum();
    Ok(StrykeValue::float(s))
}

/// Load centrality (Newman 2001): unit-flow random walk that splits at every
/// vertex by out-degree. Args: number of unit packets passing through v,
/// total packets sourced, out-degree of v.
fn builtin_load_centrality(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let packets_through = f1(args);
    let total_packets = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    let out_degree = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float((packets_through / total_packets) / out_degree))
}

// ───── clustering ─────

/// Local clustering coefficient: 2|E_N(v)| / (k_v (k_v − 1)).
fn builtin_clustering_coefficient(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let triangles = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if k < 2.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(2.0 * triangles / (k * (k - 1.0))))
}

/// Triangle count via dot-product of neighbour bit vectors.
fn builtin_triangles_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let neighbours_a = b75_to_ints(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let neighbours_b = args.get(1).map(b75_to_ints).unwrap_or_default();
    let set_a: std::collections::HashSet<i64> = neighbours_a.into_iter().collect();
    let inter = neighbours_b.iter().filter(|&&n| set_a.contains(&n)).count();
    Ok(StrykeValue::integer(inter as i64))
}

/// Transitivity (global clustering): 3·triangles / connected_triples.
fn builtin_transitivity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let triangles = f1(args);
    let triples = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(StrykeValue::float(3.0 * triangles / triples))
}

// ───── communities / partition ─────

/// Modularity contribution: (A_ij − k_i k_j / 2m) δ(c_i, c_j).
fn builtin_modularity_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a_ij = f1(args);
    let k_i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let k_j = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let m = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    let same = args.get(4).map(|v| v.to_number() as i64).unwrap_or(0);
    if same == 0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((a_ij - k_i * k_j / (2.0 * m)) / (2.0 * m)))
}

/// Louvain modularity gain ΔQ when moving v into community C.
fn builtin_louvain_gain(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
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
    Ok(StrykeValue::float(term1 - term2))
}

/// Label propagation step: pick majority label among neighbours.
fn builtin_label_propagation(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let labels = b75_to_ints(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut counts = std::collections::HashMap::<i64, i64>::new();
    let mut best = (labels.first().copied().unwrap_or(0), 0_i64);
    for &l in &labels {
        let c = counts.entry(l).or_insert(0);
        *c += 1;
        if *c > best.1 { best = (l, *c); }
    }
    Ok(StrykeValue::integer(best.0))
}

/// Girvan-Newman edge-betweenness removal step.
fn builtin_girvan_newman(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let edge_betweenness = f1(args);
    let max_betweenness = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(StrykeValue::float(edge_betweenness / max_betweenness))
}

// ───── connectivity ─────

/// Articulation point check: low[v] ≥ disc[u] for parent u in DFS tree.
fn builtin_articulation_point(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let low_v = i1(args);
    let disc_u = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if low_v >= disc_u { 1 } else { 0 }))
}

/// Bridge edge check: low[v] > disc[u].
fn builtin_bridge_edge(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let low_v = i1(args);
    let disc_u = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if low_v > disc_u { 1 } else { 0 }))
}

/// Edge connectivity = min cut over all pairs (Whitney's theorem).
fn builtin_edge_connectivity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cut_weights = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(cut_weights.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Vertex connectivity κ(G): use Whitney bound κ(G) ≤ λ(G) ≤ δ(G); return
/// min over (min vertex-cut size, edge connectivity, min degree).
fn builtin_vertex_connectivity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let vertex_cuts = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let edge_conn = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    let min_deg = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    let v_min = vertex_cuts.iter().cloned().fold(f64::INFINITY, f64::min);
    Ok(StrykeValue::float(v_min.min(edge_conn).min(min_deg)))
}

/// Biconnected components (2-vertex-connected blocks). Args: edge list as in
/// `hopcroft_karp` — array of `[u,v]` pairs; optional `n` = vertex count (else
/// inferred from max endpoint).
fn builtin_biconnected_components(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let edges = parse_edges_b24(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n_arg = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let mut n = n_arg;
    for &(u, v) in &edges {
        n = n.max(u + 1).max(v + 1);
    }
    Ok(StrykeValue::integer(b75_biconnected_component_count(n, &edges)))
}

/// Diameter: max shortest-path distance in graph.
fn builtin_gx_diameter(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dists = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(dists.iter().cloned().fold(0.0_f64, f64::max)))
}

/// Radius: min eccentricity.
fn builtin_gx_radius(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let eccs = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(eccs.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Eccentricity: max d(v, u) over u.
fn builtin_gx_eccentricity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dists = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(dists.iter().cloned().fold(0.0_f64, f64::max)))
}

// ───── transitive closure / TSP / coloring ─────

/// Warshall transitive closure: r[i][j] |= r[i][k] & r[k][j].
fn builtin_warshall_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r_ij = i1(args);
    let r_ik = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let r_kj = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(r_ij | (r_ik & r_kj)))
}

/// Held–Karp exact TSP on a **directed** graph: flat row-major `n×n` weight
/// matrix, optional scalar `n` (otherwise `√(len)`). Hard cap `n ≤ 20` for
/// bitmask DP.
fn builtin_tsp_held_karp(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let flat = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let inferred = {
        let len = flat.len();
        let r = (len as f64).sqrt().round() as usize;
        if r > 0 && r * r == len { r } else { 0 }
    };
    let n = if let Some(v) = args.get(1) {
        let nv = v.to_number() as usize;
        if nv > 0 && nv * nv <= flat.len() {
            nv
        } else {
            inferred
        }
    } else {
        inferred
    };
    if n == 0 || flat.len() < n * n {
        return Ok(StrykeValue::float(0.0));
    }
    if n > 20 {
        return Ok(StrykeValue::float(f64::INFINITY));
    }
    let mut dist = vec![vec![0.0_f64; n]; n];
    for i in 0..n {
        for j in 0..n {
            dist[i][j] = flat[i * n + j];
        }
    }
    const INF: f64 = f64::INFINITY;
    let full_mask = 1usize << n;
    let mut dp = vec![vec![INF; n]; full_mask];
    dp[1][0] = 0.0;
    for mask in 1..full_mask {
        for j in 0..n {
            if (mask & (1 << j)) == 0 {
                continue;
            }
            if mask == 1 && j == 0 {
                continue;
            }
            let pm = mask ^ (1 << j);
            if pm == 0 {
                continue;
            }
            let mut best = INF;
            for i in 0..n {
                if i == j || (pm & (1 << i)) == 0 {
                    continue;
                }
                let cand = dp[pm][i] + dist[i][j];
                if cand < best {
                    best = cand;
                }
            }
            dp[mask][j] = best;
        }
    }
    let all = full_mask - 1;
    let mut ans = INF;
    for j in 1..n {
        if dp[all][j] < INF {
            let v = dp[all][j] + dist[j][0];
            if v < ans {
                ans = v;
            }
        }
    }
    if ans.is_infinite() {
        Ok(StrykeValue::float(0.0))
    } else {
        Ok(StrykeValue::float(ans))
    }
}

/// TSP nearest-neighbour heuristic step: pick min unvisited distance.
fn builtin_tsp_nn_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let unvisited = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(unvisited.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// Christofides metric-TSP upper bound: `MST + MWPM` where `MWPM` is a
/// minimum-weight perfect matching on the odd-degree vertices of the MST
/// (full matching cost, not half).
fn builtin_tsp_christofides(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mst_cost = f1(args);
    let matching_cost = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(mst_cost + matching_cost))
}

/// Greedy graph colouring: pick smallest colour not used by neighbours.
fn builtin_graph_coloring_greedy(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let used = b75_to_ints(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let used_set: std::collections::HashSet<i64> = used.into_iter().collect();
    let mut c = 0_i64;
    while used_set.contains(&c) { c += 1; }
    Ok(StrykeValue::integer(c))
}

/// Welsh–Powell palette size bound: `Δ(G) + 1` (always a valid number of
/// colours for a greedy colouring that proceeds in non-increasing degree order).
fn builtin_welsh_powell(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let degrees = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if degrees.is_empty() {
        return Ok(StrykeValue::integer(0));
    }
    let max_d = degrees.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    Ok(StrykeValue::integer(max_d as i64 + 1))
}

// ───── isomorphism / matching ─────

/// VF2 feasibility: degree of the pattern vertex must match that of the mapped
/// host vertex (necessary, not sufficient).
fn builtin_vf2_consistent(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let deg_u = i1(args);
    let deg_v = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if deg_u == deg_v { 1 } else { 0 }))
}

/// Subgraph isomorphism feasibility: pattern degree must not exceed host degree.
fn builtin_subgraph_isomorphism(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let deg_pat = i1(args);
    let deg_host = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if deg_pat <= deg_host { 1 } else { 0 }))
}

/// Hungarian preprocessing: one full **row + column reduction** of a square
/// cost matrix (subtract row minima, then column minima). Args: flattened
/// `n×n` costs, then `n`. Returns the **total dual adjustment** (sum of all
/// subtracted row and column minima).
fn builtin_hungarian_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let flat = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    if n == 0 || flat.len() < n * n {
        return Ok(StrykeValue::float(0.0));
    }
    let mut a = flat[..n * n].to_vec();
    let mut reduction = 0.0_f64;
    for i in 0..n {
        let row: Vec<f64> = (0..n).map(|j| a[i * n + j]).collect();
        let mn = row.iter().cloned().fold(f64::INFINITY, f64::min);
        if mn.is_finite() {
            reduction += mn;
            for j in 0..n {
                a[i * n + j] -= mn;
            }
        }
    }
    for j in 0..n {
        let col: Vec<f64> = (0..n).map(|i| a[i * n + j]).collect();
        let mn = col.iter().cloned().fold(f64::INFINITY, f64::min);
        if mn.is_finite() {
            reduction += mn;
            for i in 0..n {
                a[i * n + j] -= mn;
            }
        }
    }
    Ok(StrykeValue::float(reduction))
}

/// One **phase** of Hopcroft–Karp: re-use the full bipartite cardinality matcher
/// (`hopcroft_karp`) on the same argument layout: `edges`, `n_left`, `n_right`.
fn builtin_hopcroft_karp_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_hopcroft_karp(args)
}

// ───── max clique / vertex cover ─────

/// Bron–Kerbosch recursion **terminal**: report a maximal clique when both `P`
/// and `X` are empty (only `R` nonempty in the search context). Args: |P|, |X|.
fn builtin_bron_kerbosch(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_size = i1(args);
    let x_size = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if p_size == 0 && x_size == 0 { 1 } else { 0 }))
}

/// Minimum vertex cover via König's theorem: n − max matching for bipartite.
fn builtin_min_vertex_cover(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let max_matching = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((n - max_matching).max(0)))
}

/// Independent set size: n − vertex cover.
fn builtin_max_independent_set(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let vc = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer((n - vc).max(0)))
}

/// Greedy dominating set: pick the vertex that **covers the most still-uncovered**
/// vertices. Args: nonnegative “coverage” scores per vertex; returns the index
/// of the maximum.
fn builtin_dominating_set_greedy(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cov = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut best_i = 0_usize;
    let mut best_v = f64::NEG_INFINITY;
    for (i, &c) in cov.iter().enumerate() {
        if c > best_v {
            best_v = c;
            best_i = i;
        }
    }
    Ok(StrykeValue::integer(if cov.is_empty() { -1 } else { best_i as i64 }))
}

/// Held–Karp Hamiltonian-path/TSP DP state-count: `n · 2^n` bit-DP subproblems.
fn builtin_hamiltonian_path(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).clamp(1, 20);
    Ok(StrykeValue::integer((1_i64 << n) * n))
}

// ───── flow / spanning ─────

/// Mehlhorn 1988 Steiner 2-approximation returns a tree whose cost is at most
/// the **metric closure MST** weight fed in (caller supplies that MST cost).
fn builtin_min_steiner_tree(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let metric_mst = f1(args);
    Ok(StrykeValue::float(metric_mst))
}

/// `k`-th order statistic among **candidate spanning-tree weights** (caller
/// precomputes the multiset; this picks the k-th smallest after sorting).
fn builtin_k_shortest_spanning(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut weights = b75_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    weights.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(StrykeValue::float(weights.get(k).copied().unwrap_or(f64::INFINITY)))
}

/// Random walk hitting probability: stationary π(v) = deg(v) / 2m.
fn builtin_random_walk_hitting(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let deg = f1(args);
    let two_m = args.get(1).map(|v| v.to_number()).unwrap_or(2.0).max(1e-300);
    Ok(StrykeValue::float(deg / two_m))
}

/// SimRank similarity: s(a, b) = (C / (|I(a)| · |I(b)|)) · Σ s(I_i(a), I_j(b)).
fn builtin_simrank(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let inner_sum = f1(args);
    let i_a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let i_b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let c = args.get(3).map(|v| v.to_number()).unwrap_or(0.8);
    Ok(StrykeValue::float(c * inner_sum / (i_a * i_b)))
}
