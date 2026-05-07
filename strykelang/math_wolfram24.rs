// Batch 24 — graph algorithms: SCC, articulation, bridges, flow, matching, centrality.

fn parse_adj_b24(v: &PerlValue) -> Vec<Vec<usize>> {
    let outer = arg_to_vec(v);
    outer.iter().map(|v| arg_to_vec(v).iter().map(|x| x.to_number() as usize).collect()).collect()
}

fn parse_edges_b24(v: &PerlValue) -> Vec<(usize, usize)> {
    let edges = arg_to_vec(v);
    edges.iter().filter_map(|e| {
        let p = arg_to_vec(e);
        if p.len() < 2 { None }
        else { Some((p[0].to_number() as usize, p[1].to_number() as usize)) }
    }).collect()
}

fn parse_weighted_edges_b24(v: &PerlValue) -> Vec<(usize, usize, f64)> {
    let edges = arg_to_vec(v);
    edges.iter().filter_map(|e| {
        let p = arg_to_vec(e);
        if p.len() < 2 { None }
        else if p.len() == 2 { Some((p[0].to_number() as usize, p[1].to_number() as usize, 1.0)) }
        else { Some((p[0].to_number() as usize, p[1].to_number() as usize, p[2].to_number())) }
    }).collect()
}

// Tarjan's SCC algorithm
#[allow(dead_code)]
fn builtin_tarjan_scc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut index_counter = 0_usize;
    let mut stack: Vec<usize> = vec![];
    let mut on_stack = vec![false; n];
    let mut indices = vec![usize::MAX; n];
    let mut low = vec![0_usize; n];
    let mut sccs: Vec<Vec<usize>> = vec![];

    #[allow(clippy::too_many_arguments)]
    fn strong(
        v: usize, adj: &[Vec<usize>], index_counter: &mut usize,
        stack: &mut Vec<usize>, on_stack: &mut [bool],
        indices: &mut [usize], low: &mut [usize], sccs: &mut Vec<Vec<usize>>,
    ) {
        indices[v] = *index_counter;
        low[v] = *index_counter;
        *index_counter += 1;
        stack.push(v);
        on_stack[v] = true;
        for &w in &adj[v] {
            if w >= adj.len() { continue; }
            if indices[w] == usize::MAX {
                strong(w, adj, index_counter, stack, on_stack, indices, low, sccs);
                low[v] = low[v].min(low[w]);
            } else if on_stack[w] {
                low[v] = low[v].min(indices[w]);
            }
        }
        if low[v] == indices[v] {
            let mut scc = vec![];
            while let Some(w) = stack.pop() {
                on_stack[w] = false;
                scc.push(w);
                if w == v { break; }
            }
            sccs.push(scc);
        }
    }

    for v in 0..n {
        if indices[v] == usize::MAX {
            strong(v, &adj, &mut index_counter, &mut stack, &mut on_stack, &mut indices, &mut low, &mut sccs);
        }
    }
    let out: Vec<PerlValue> = sccs.into_iter()
        .map(|s| PerlValue::array(s.into_iter().map(|x| PerlValue::integer(x as i64)).collect()))
        .collect();
    Ok(PerlValue::array(out))
}

// Kosaraju's SCC
fn builtin_kosaraju_scc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut radj = vec![vec![]; n];
    for u in 0..n {
        for &v in &adj[u] {
            if v < n { radj[v].push(u); }
        }
    }
    let mut visited = vec![false; n];
    let mut order = vec![];

    fn dfs1(v: usize, adj: &[Vec<usize>], visited: &mut [bool], order: &mut Vec<usize>) {
        visited[v] = true;
        for &w in &adj[v] {
            if w < adj.len() && !visited[w] { dfs1(w, adj, visited, order); }
        }
        order.push(v);
    }

    for v in 0..n {
        if !visited[v] { dfs1(v, &adj, &mut visited, &mut order); }
    }
    let mut comp = vec![usize::MAX; n];
    let mut c = 0;
    fn dfs2(v: usize, radj: &[Vec<usize>], comp: &mut [usize], c: usize) {
        comp[v] = c;
        for &w in &radj[v] {
            if w < radj.len() && comp[w] == usize::MAX { dfs2(w, radj, comp, c); }
        }
    }
    for &v in order.iter().rev() {
        if comp[v] == usize::MAX {
            dfs2(v, &radj, &mut comp, c);
            c += 1;
        }
    }
    let mut sccs: Vec<Vec<usize>> = vec![vec![]; c];
    for (v, &cc) in comp.iter().enumerate() {
        if cc != usize::MAX { sccs[cc].push(v); }
    }
    let out: Vec<PerlValue> = sccs.into_iter()
        .map(|s| PerlValue::array(s.into_iter().map(|x| PerlValue::integer(x as i64)).collect()))
        .collect();
    Ok(PerlValue::array(out))
}

// Find articulation points (cut vertices)

// Find bridges (cut edges)
fn builtin_bridges(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut visited = vec![false; n];
    let mut disc = vec![0_usize; n];
    let mut low = vec![0_usize; n];
    let mut bridges_v: Vec<(usize, usize)> = vec![];
    let mut time = 0_usize;

    #[allow(clippy::too_many_arguments)]
    fn dfs(
        u: usize, parent: usize, adj: &[Vec<usize>], visited: &mut [bool],
        disc: &mut [usize], low: &mut [usize], bridges: &mut Vec<(usize, usize)>, time: &mut usize,
    ) {
        visited[u] = true;
        disc[u] = *time;
        low[u] = *time;
        *time += 1;
        for &v in &adj[u] {
            if v >= adj.len() { continue; }
            if !visited[v] {
                dfs(v, u, adj, visited, disc, low, bridges, time);
                low[u] = low[u].min(low[v]);
                if low[v] > disc[u] { bridges.push((u, v)); }
            } else if v != parent {
                low[u] = low[u].min(disc[v]);
            }
        }
    }
    for i in 0..n {
        if !visited[i] {
            dfs(i, usize::MAX, &adj, &mut visited, &mut disc, &mut low, &mut bridges_v, &mut time);
        }
    }
    let out: Vec<PerlValue> = bridges_v.into_iter()
        .map(|(u, v)| PerlValue::array(vec![PerlValue::integer(u as i64), PerlValue::integer(v as i64)]))
        .collect();
    Ok(PerlValue::array(out))
}

// Edmonds-Karp BFS-based max flow
fn builtin_max_flow_ek(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let edges = parse_weighted_edges_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let s = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let t = args.get(2).map(|v| v.to_number() as usize).unwrap_or(1);
    let n_arg = args.get(3).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = n_arg.max(edges.iter().map(|(u, v, _)| u.max(v)).max().copied().unwrap_or(0) + 1);
    let mut cap = vec![vec![0.0; n]; n];
    for &(u, v, w) in &edges {
        if u < n && v < n { cap[u][v] += w; }
    }
    let mut flow = 0.0;
    loop {
        let mut parent = vec![usize::MAX; n];
        parent[s] = s;
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(s);
        let mut found = false;
        while let Some(u) = queue.pop_front() {
            if u == t { found = true; break; }
            for v in 0..n {
                if parent[v] == usize::MAX && cap[u][v] > 1e-12 {
                    parent[v] = u;
                    queue.push_back(v);
                }
            }
        }
        if !found { break; }
        let mut path_flow = f64::INFINITY;
        let mut v = t;
        while v != s {
            let u = parent[v];
            if cap[u][v] < path_flow { path_flow = cap[u][v]; }
            v = u;
        }
        let mut v = t;
        while v != s {
            let u = parent[v];
            cap[u][v] -= path_flow;
            cap[v][u] += path_flow;
            v = u;
        }
        flow += path_flow;
    }
    Ok(PerlValue::float(flow))
}

// Min-cut value (== max-flow value)
fn builtin_min_cut_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_max_flow_ek(args)
}

// Hopcroft-Karp simplified bipartite matching
fn builtin_hopcroft_karp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let edges = parse_edges_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n_left = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let n_right = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n_left];
    for &(u, v) in &edges {
        if u < n_left { adj[u].push(v); }
    }
    let mut match_l = vec![usize::MAX; n_left];
    let mut match_r = vec![usize::MAX; n_right];

    fn try_kuhn(
        u: usize, adj: &[Vec<usize>], visited: &mut [bool],
        match_l: &mut [usize], match_r: &mut [usize],
    ) -> bool {
        for &v in &adj[u] {
            if v >= visited.len() { continue; }
            if visited[v] { continue; }
            visited[v] = true;
            if match_r[v] == usize::MAX || try_kuhn(match_r[v], adj, visited, match_l, match_r) {
                match_l[u] = v;
                match_r[v] = u;
                return true;
            }
        }
        false
    }

    let mut count = 0_usize;
    for u in 0..n_left {
        let mut visited = vec![false; n_right];
        if try_kuhn(u, &adj, &mut visited, &mut match_l, &mut match_r) {
            count += 1;
        }
    }
    Ok(PerlValue::integer(count as i64))
}

// Closeness centrality (unweighted, BFS)

// Betweenness centrality (Brandes algorithm)

// Eigenvector centrality (power iteration)

// Katz centrality
fn builtin_katz_centrality(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let iters = args.get(3).map(|v| v.to_number() as usize).unwrap_or(100);
    let n = adj.len();
    let mut x = vec![0.0; n];
    for _ in 0..iters {
        let mut new_x = vec![beta; n];
        for u in 0..n {
            for &v in &adj[u] {
                if v < n { new_x[v] += alpha * x[u]; }
            }
        }
        x = new_x;
    }
    Ok(PerlValue::array(x.into_iter().map(PerlValue::float).collect()))
}

// HITS hubs/authorities (returns [hubs, authorities])
fn builtin_hits_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let iters = args.get(1).map(|v| v.to_number() as usize).unwrap_or(50);
    let n = adj.len();
    let mut hubs = vec![1.0; n];
    let mut auth = vec![1.0; n];
    for _ in 0..iters {
        let mut new_auth = vec![0.0; n];
        for u in 0..n {
            for &v in &adj[u] {
                if v < n { new_auth[v] += hubs[u]; }
            }
        }
        let mut new_hubs = vec![0.0; n];
        for u in 0..n {
            for &v in &adj[u] {
                if v < n { new_hubs[u] += new_auth[v]; }
            }
        }
        let na: f64 = new_auth.iter().map(|v| v * v).sum::<f64>().sqrt();
        let nh: f64 = new_hubs.iter().map(|v| v * v).sum::<f64>().sqrt();
        if na > 0.0 { for v in &mut new_auth { *v /= na; } }
        if nh > 0.0 { for v in &mut new_hubs { *v /= nh; } }
        hubs = new_hubs;
        auth = new_auth;
    }
    Ok(PerlValue::array(vec![
        PerlValue::array(hubs.into_iter().map(PerlValue::float).collect()),
        PerlValue::array(auth.into_iter().map(PerlValue::float).collect()),
    ]))
}

// PageRank with damping
fn builtin_pagerank_damped(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let damping = args.get(1).map(|v| v.to_number()).unwrap_or(0.85);
    let iters = args.get(2).map(|v| v.to_number() as usize).unwrap_or(100);
    let n = adj.len();
    if n == 0 { return Ok(PerlValue::array(vec![])); }
    let mut pr = vec![1.0 / n as f64; n];
    let outdeg: Vec<f64> = adj.iter().map(|nbrs| nbrs.len() as f64).collect();
    for _ in 0..iters {
        let mut new_pr = vec![(1.0 - damping) / n as f64; n];
        for u in 0..n {
            if outdeg[u] > 0.0 {
                let share = damping * pr[u] / outdeg[u];
                for &v in &adj[u] {
                    if v < n { new_pr[v] += share; }
                }
            }
        }
        pr = new_pr;
    }
    Ok(PerlValue::array(pr.into_iter().map(PerlValue::float).collect()))
}

// Connected components count (undirected)
fn builtin_cc_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut visited = vec![false; n];
    let mut count = 0;
    for s in 0..n {
        if visited[s] { continue; }
        count += 1;
        let mut q = std::collections::VecDeque::new();
        q.push_back(s);
        visited[s] = true;
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] {
                if v < n && !visited[v] {
                    visited[v] = true;
                    q.push_back(v);
                }
            }
        }
    }
    Ok(PerlValue::integer(count as i64))
}

// Connected components (returns label per node)
fn builtin_cc_labels(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut label = vec![usize::MAX; n];
    let mut count = 0_usize;
    for s in 0..n {
        if label[s] != usize::MAX { continue; }
        let mut q = std::collections::VecDeque::new();
        q.push_back(s);
        label[s] = count;
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] {
                if v < n && label[v] == usize::MAX {
                    label[v] = count;
                    q.push_back(v);
                }
            }
        }
        count += 1;
    }
    Ok(PerlValue::array(label.into_iter().map(|l| PerlValue::integer(l as i64)).collect()))
}

// Topological sort (Kahn's)
fn builtin_topological_sort_kahn(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut indeg = vec![0_usize; n];
    for u in 0..n {
        for &v in &adj[u] {
            if v < n { indeg[v] += 1; }
        }
    }
    let mut q = std::collections::VecDeque::new();
    for i in 0..n {
        if indeg[i] == 0 { q.push_back(i); }
    }
    let mut order = vec![];
    while let Some(u) = q.pop_front() {
        order.push(u);
        for &v in &adj[u] {
            if v < n {
                indeg[v] -= 1;
                if indeg[v] == 0 { q.push_back(v); }
            }
        }
    }
    Ok(PerlValue::array(order.into_iter().map(|x| PerlValue::integer(x as i64)).collect()))
}

// Has cycle (directed)
fn builtin_has_cycle_directed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut color = vec![0_u8; n];
    fn dfs(u: usize, adj: &[Vec<usize>], color: &mut [u8]) -> bool {
        color[u] = 1;
        for &v in &adj[u] {
            if v >= adj.len() { continue; }
            if color[v] == 1 { return true; }
            if color[v] == 0 && dfs(v, adj, color) { return true; }
        }
        color[u] = 2;
        false
    }
    for i in 0..n {
        if color[i] == 0 && dfs(i, &adj, &mut color) { return Ok(PerlValue::integer(1)); }
    }
    Ok(PerlValue::integer(0))
}

// Has cycle (undirected) via DFS
fn builtin_has_cycle_undirected(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut visited = vec![false; n];
    fn dfs(u: usize, parent: usize, adj: &[Vec<usize>], visited: &mut [bool]) -> bool {
        visited[u] = true;
        for &v in &adj[u] {
            if v >= adj.len() { continue; }
            if !visited[v] {
                if dfs(v, u, adj, visited) { return true; }
            } else if v != parent {
                return true;
            }
        }
        false
    }
    for i in 0..n {
        if !visited[i] && dfs(i, usize::MAX, &adj, &mut visited) { return Ok(PerlValue::integer(1)); }
    }
    Ok(PerlValue::integer(0))
}

// BFS distances from source

// Diameter (BFS from each)
fn builtin_diameter_bfs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut max_d = 0_i64;
    for s in 0..n {
        let mut dist = vec![-1_i64; n];
        dist[s] = 0;
        let mut q = std::collections::VecDeque::new();
        q.push_back(s);
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] {
                if v < n && dist[v] == -1 {
                    dist[v] = dist[u] + 1;
                    q.push_back(v);
                    if dist[v] > max_d { max_d = dist[v]; }
                }
            }
        }
    }
    Ok(PerlValue::integer(max_d))
}

// Eccentricity per node

// Radius = min eccentricity
fn builtin_radius_bfs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    if n == 0 { return Ok(PerlValue::integer(0)); }
    let mut min_ecc = i64::MAX;
    for s in 0..n {
        let mut dist = vec![-1_i64; n];
        dist[s] = 0;
        let mut q = std::collections::VecDeque::new();
        q.push_back(s);
        let mut m = 0_i64;
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] {
                if v < n && dist[v] == -1 {
                    dist[v] = dist[u] + 1;
                    q.push_back(v);
                    if dist[v] > m { m = dist[v]; }
                }
            }
        }
        if m < min_ecc { min_ecc = m; }
    }
    Ok(PerlValue::integer(min_ecc))
}

// Number of edges
fn builtin_num_edges(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let total: usize = adj.iter().map(|n| n.len()).sum();
    Ok(PerlValue::integer((total / 2) as i64))
}

// Density = 2E / (N(N-1))

// k-core decomposition (returns coreness)
fn builtin_k_coreness(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut deg: Vec<usize> = adj.iter().map(|n| n.len()).collect();
    let mut alive = vec![true; n];
    let mut core = vec![0_i64; n];
    let mut k = 0_usize;
    loop {
        let mut changed = true;
        while changed {
            changed = false;
            for v in 0..n {
                if alive[v] && deg[v] <= k {
                    alive[v] = false;
                    core[v] = k as i64;
                    for &u in &adj[v] {
                        if u < n && alive[u] && deg[u] > 0 {
                            deg[u] -= 1;
                        }
                    }
                    changed = true;
                }
            }
        }
        if !alive.iter().any(|&a| a) { break; }
        k += 1;
    }
    Ok(PerlValue::array(core.into_iter().map(PerlValue::integer).collect()))
}

// Graph coloring greedy (welsh-powell)
fn builtin_greedy_coloring(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| adj[b].len().cmp(&adj[a].len()));
    let mut color = vec![-1_i64; n];
    for v in order {
        let used: std::collections::HashSet<i64> = adj[v].iter().filter_map(|&u| {
            if u < n && color[u] >= 0 { Some(color[u]) } else { None }
        }).collect();
        let mut c = 0_i64;
        while used.contains(&c) { c += 1; }
        color[v] = c;
    }
    Ok(PerlValue::array(color.into_iter().map(PerlValue::integer).collect()))
}

// Chromatic number greedy estimate (max color + 1)
fn builtin_chromatic_number_greedy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = builtin_greedy_coloring(args)?;
    let colors = arg_to_vec(&r);
    let max_c = colors.iter().map(|v| v.to_number() as i64).max().unwrap_or(-1);
    Ok(PerlValue::integer(max_c + 1))
}

// Sum of degrees (= 2 * edges)
fn builtin_sum_degrees(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let total: usize = adj.iter().map(|n| n.len()).sum();
    Ok(PerlValue::integer(total as i64))
}

// Average degree
fn builtin_avg_degree(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    if n == 0 { return Ok(PerlValue::float(0.0)); }
    let total: usize = adj.iter().map(|n| n.len()).sum();
    Ok(PerlValue::float(total as f64 / n as f64))
}

// Max degree
fn builtin_max_degree(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let m = adj.iter().map(|n| n.len()).max().unwrap_or(0);
    Ok(PerlValue::integer(m as i64))
}

// Graph is tree?
fn builtin_is_tree(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let total: usize = adj.iter().map(|n| n.len()).sum();
    if total / 2 != n.saturating_sub(1) { return Ok(PerlValue::integer(0)); }
    let mut visited = vec![false; n];
    let mut q = std::collections::VecDeque::new();
    if n > 0 { q.push_back(0); visited[0] = true; }
    while let Some(u) = q.pop_front() {
        for &v in &adj[u] {
            if v < n && !visited[v] {
                visited[v] = true;
                q.push_back(v);
            }
        }
    }
    Ok(PerlValue::integer(if visited.iter().all(|&v| v) { 1 } else { 0 }))
}

// Girth (shortest cycle, BFS-based)
fn builtin_girth(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let adj = parse_adj_b24(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let n = adj.len();
    let mut min_girth = i64::MAX;
    for s in 0..n {
        let mut dist = vec![-1_i64; n];
        let mut parent = vec![usize::MAX; n];
        dist[s] = 0;
        let mut q = std::collections::VecDeque::new();
        q.push_back(s);
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] {
                if v >= n { continue; }
                if dist[v] == -1 {
                    dist[v] = dist[u] + 1;
                    parent[v] = u;
                    q.push_back(v);
                } else if parent[u] != v {
                    let cycle_len = dist[u] + dist[v] + 1;
                    if cycle_len < min_girth { min_girth = cycle_len; }
                }
            }
        }
    }
    if min_girth == i64::MAX { Ok(PerlValue::integer(-1)) }
    else { Ok(PerlValue::integer(min_girth)) }
}
