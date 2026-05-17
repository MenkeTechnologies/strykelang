// graph algorithms, more strings, dates, calendar variants, tax/loan,
// fluid mechanics, optics, more PRNG.

// ── Graph algorithms ────────────────────────────────────────────────────────

fn builtin_bfs_distances(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let s = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = adj.len();
    let mut dist = vec![-1_i64; n];
    if s >= n { return Ok(StrykeValue::array(dist.into_iter().map(StrykeValue::integer).collect())); }
    dist[s] = 0;
    let mut q: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    q.push_back(s);
    while let Some(u) = q.pop_front() {
        for &v in &adj[u] {
            if v < n && dist[v] == -1 { dist[v] = dist[u] + 1; q.push_back(v); }
        }
    }
    Ok(StrykeValue::array(dist.into_iter().map(StrykeValue::integer).collect()))
}
fn builtin_dfs_preorder(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let s = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = adj.len();
    let mut visited = vec![false; n];
    let mut out: Vec<i64> = Vec::new();
    let mut stack = vec![s];
    while let Some(u) = stack.pop() {
        if u >= n || visited[u] { continue; }
        visited[u] = true;
        out.push(u as i64);
        for &v in adj[u].iter().rev() {
            if v < n && !visited[v] { stack.push(v); }
        }
    }
    Ok(StrykeValue::array(out.into_iter().map(StrykeValue::integer).collect()))
}
fn builtin_connected_components(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let mut comp = vec![-1_i64; n];
    let mut k = 0_i64;
    for s in 0..n {
        if comp[s] != -1 { continue; }
        comp[s] = k;
        let mut stack = vec![s];
        while let Some(u) = stack.pop() {
            for &v in &adj[u] {
                if v < n && comp[v] == -1 { comp[v] = k; stack.push(v); }
            }
        }
        k += 1;
    }
    Ok(StrykeValue::array(comp.into_iter().map(StrykeValue::integer).collect()))
}
fn builtin_graph_is_tree(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    if n == 0 { return Ok(StrykeValue::integer(1)); }
    let edges: usize = adj.iter().map(|nbrs| nbrs.len()).sum::<usize>() / 2;
    if edges != n - 1 { return Ok(StrykeValue::integer(0)); }
    let comp = builtin_connected_components(args)?;
    let arr = arg_to_vec(&comp);
    let unique: std::collections::HashSet<i64> = arr.iter().map(|v| v.to_number() as i64).collect();
    Ok(StrykeValue::integer(if unique.len() == 1 { 1 } else { 0 }))
}
fn builtin_graph_density(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len() as f64;
    if n < 2.0 { return Ok(StrykeValue::float(0.0)); }
    let edges: usize = adj.iter().map(|nbrs| nbrs.len()).sum::<usize>() / 2;
    Ok(StrykeValue::float(2.0 * edges as f64 / (n * (n - 1.0))))
}
fn builtin_graph_average_degree(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len() as f64;
    if n == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let total: usize = adj.iter().map(|nbrs| nbrs.len()).sum();
    Ok(StrykeValue::float(total as f64 / n))
}
fn builtin_graph_max_degree(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    Ok(StrykeValue::integer(adj.iter().map(|n| n.len()).max().unwrap_or(0) as i64))
}
fn builtin_graph_min_degree(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    Ok(StrykeValue::integer(adj.iter().map(|n| n.len()).min().unwrap_or(0) as i64))
}
fn builtin_graph_complement(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let sets: Vec<std::collections::HashSet<usize>> = adj.iter().map(|nbrs| nbrs.iter().copied().collect()).collect();
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in 0..n {
            if i != j && !sets[i].contains(&j) { out[i].push(j); }
        }
    }
    Ok(StrykeValue::array(out.into_iter().map(|nbrs| {
        StrykeValue::array(nbrs.into_iter().map(|v| StrykeValue::integer(v as i64)).collect())
    }).collect()))
}
fn builtin_in_degree_directed(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let mut in_deg = vec![0_i64; n];
    for nbrs in &adj {
        for &v in nbrs { if v < n { in_deg[v] += 1; } }
    }
    Ok(StrykeValue::array(in_deg.into_iter().map(StrykeValue::integer).collect()))
}
fn builtin_out_degree_directed(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    Ok(StrykeValue::array(adj.iter().map(|n| StrykeValue::integer(n.len() as i64)).collect()))
}
fn builtin_graph_eccentricity_all(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let mut out = Vec::with_capacity(n);
    for v in 0..n {
        let mut dist = vec![-1_i64; n];
        let mut q = std::collections::VecDeque::new();
        dist[v] = 0; q.push_back(v);
        let mut max_d = 0_i64;
        while let Some(u) = q.pop_front() {
            for &w in &adj[u] {
                if w < n && dist[w] == -1 { dist[w] = dist[u] + 1; max_d = max_d.max(dist[w]); q.push_back(w); }
            }
        }
        out.push(StrykeValue::integer(max_d));
    }
    Ok(StrykeValue::array(out))
}
fn builtin_is_connected(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    if n == 0 { return Ok(StrykeValue::integer(1)); }
    let comp = builtin_connected_components(args)?;
    let arr = arg_to_vec(&comp);
    let unique: std::collections::HashSet<i64> = arr.iter().map(|v| v.to_number() as i64).collect();
    Ok(StrykeValue::integer(if unique.len() == 1 { 1 } else { 0 }))
}
fn builtin_articulation_points(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let mut visited = vec![false; n];
    let mut disc = vec![0_i64; n];
    let mut low = vec![0_i64; n];
    let mut parent = vec![-1_i64; n];
    let mut ap = vec![false; n];
    let mut timer = 0_i64;
    #[allow(clippy::too_many_arguments)]
    fn dfs(u: usize, adj: &[Vec<usize>], visited: &mut [bool], disc: &mut [i64], low: &mut [i64], parent: &mut [i64], ap: &mut [bool], timer: &mut i64) {
        let n = adj.len();
        visited[u] = true; *timer += 1; disc[u] = *timer; low[u] = *timer;
        let mut children = 0_usize;
        for &v in &adj[u] {
            if v >= n { continue; }
            if !visited[v] {
                children += 1;
                parent[v] = u as i64;
                dfs(v, adj, visited, disc, low, parent, ap, timer);
                low[u] = low[u].min(low[v]);
                if parent[u] == -1 && children > 1 { ap[u] = true; }
                if parent[u] != -1 && low[v] >= disc[u] { ap[u] = true; }
            } else if v as i64 != parent[u] {
                low[u] = low[u].min(disc[v]);
            }
        }
    }
    for u in 0..n {
        if !visited[u] {
            dfs(u, &adj, &mut visited, &mut disc, &mut low, &mut parent, &mut ap, &mut timer);
        }
    }
    Ok(StrykeValue::array(
        ap.iter().enumerate().filter_map(|(i, &v)| if v { Some(StrykeValue::integer(i as i64)) } else { None }).collect()
    ))
}
fn builtin_bridges_edges(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let mut visited = vec![false; n];
    let mut disc = vec![0_i64; n]; let mut low = vec![0_i64; n];
    let mut parent = vec![-1_i64; n];
    let mut bridges: Vec<(i64, i64)> = Vec::new();
    let mut timer = 0_i64;
    #[allow(clippy::too_many_arguments)]
    fn dfs(u: usize, adj: &[Vec<usize>], visited: &mut [bool], disc: &mut [i64], low: &mut [i64], parent: &mut [i64], bridges: &mut Vec<(i64, i64)>, timer: &mut i64) {
        let n = adj.len();
        visited[u] = true; *timer += 1; disc[u] = *timer; low[u] = *timer;
        for &v in &adj[u] {
            if v >= n { continue; }
            if !visited[v] {
                parent[v] = u as i64;
                dfs(v, adj, visited, disc, low, parent, bridges, timer);
                low[u] = low[u].min(low[v]);
                if low[v] > disc[u] { bridges.push((u as i64, v as i64)); }
            } else if v as i64 != parent[u] {
                low[u] = low[u].min(disc[v]);
            }
        }
    }
    for u in 0..n {
        if !visited[u] { dfs(u, &adj, &mut visited, &mut disc, &mut low, &mut parent, &mut bridges, &mut timer); }
    }
    Ok(StrykeValue::array(bridges.into_iter().map(|(a, b)| {
        StrykeValue::array(vec![StrykeValue::integer(a), StrykeValue::integer(b)])
    }).collect()))
}
fn builtin_eulerian_path_q(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let odd_count = adj.iter().filter(|n| n.len() % 2 == 1).count();
    Ok(StrykeValue::integer(if odd_count == 0 || odd_count == 2 { 1 } else { 0 }))
}
fn builtin_hamiltonian_brute(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    if n == 0 { return Ok(StrykeValue::integer(0)); }
    let sets: Vec<std::collections::HashSet<usize>> = adj.iter().map(|nbrs| nbrs.iter().copied().collect()).collect();
    let mut path = vec![0_usize];
    let mut visited = vec![false; n];
    visited[0] = true;
    fn rec(path: &mut Vec<usize>, visited: &mut [bool], sets: &[std::collections::HashSet<usize>], n: usize) -> bool {
        if path.len() == n { return true; }
        let last = *path.last().unwrap();
        for &v in &sets[last] {
            if !visited[v] {
                visited[v] = true; path.push(v);
                if rec(path, visited, sets, n) { return true; }
                path.pop(); visited[v] = false;
            }
        }
        false
    }
    Ok(StrykeValue::integer(if rec(&mut path, &mut visited, &sets, n) { 1 } else { 0 }))
}

// ── Strings ─────────────────────────────────────────────────────────────────

fn builtin_string_to_charcodes(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(StrykeValue::array(s.chars().map(|c| StrykeValue::integer(c as i64)).collect()))
}
fn builtin_charcodes_to_string(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let codes: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number() as i64).collect();
    let s: String = codes.into_iter().filter_map(|c| char::from_u32(c as u32)).collect();
    Ok(StrykeValue::string(s))
}
fn builtin_string_xor(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let bv = b.as_bytes();
    if bv.is_empty() { return Ok(StrykeValue::string(a)); }
    let out: Vec<u8> = a.bytes().enumerate().map(|(i, c)| c ^ bv[i % bv.len()]).collect();
    Ok(StrykeValue::string(String::from_utf8_lossy(&out).into_owned()))
}
fn builtin_string_camel_to_snake(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut out = String::with_capacity(s.len() + 4);
    for (i, c) in s.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 { out.push('_'); }
        out.extend(c.to_lowercase());
    }
    Ok(StrykeValue::string(out))
}
fn builtin_string_snake_to_camel(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let parts: Vec<&str> = s.split('_').collect();
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() { continue; }
        if i == 0 {
            out.push_str(&p.to_lowercase());
        } else {
            let mut c = p.chars();
            if let Some(first) = c.next() { out.extend(first.to_uppercase()); out.push_str(&c.as_str().to_lowercase()); }
        }
    }
    Ok(StrykeValue::string(out))
}
fn builtin_string_kebab_to_snake(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::string(args.first().map(|v| v.to_string()).unwrap_or_default().replace('-', "_")))
}
fn builtin_string_snake_to_kebab(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::string(args.first().map(|v| v.to_string()).unwrap_or_default().replace('_', "-")))
}
fn builtin_palindromic_q(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let chars: Vec<char> = s.chars().filter(|c| c.is_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
    let n = chars.len();
    let palindrome = (0..n / 2).all(|i| chars[i] == chars[n - 1 - i]);
    Ok(StrykeValue::integer(if palindrome { 1 } else { 0 }))
}
fn builtin_substring_count(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let needle = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    if needle.is_empty() { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(s.matches(&needle as &str).count() as i64))
}
fn builtin_string_truncate_ellipsis(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    if s.chars().count() <= n { return Ok(StrykeValue::string(s)); }
    let truncated: String = s.chars().take(n.saturating_sub(1)).collect();
    Ok(StrykeValue::string(format!("{}…", truncated)))
}
fn builtin_string_expand_tabs(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let tabsize = args.get(1).map(|v| v.to_number() as usize).unwrap_or(8);
    let mut out = String::new();
    let mut col = 0_usize;
    for c in s.chars() {
        match c {
            '\t' => { let pad = tabsize - (col % tabsize); for _ in 0..pad { out.push(' '); col += 1; } }
            '\n' => { out.push('\n'); col = 0; }
            _ => { out.push(c); col += 1; }
        }
    }
    Ok(StrykeValue::string(out))
}
fn builtin_string_normalize_spaces(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let normalized: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    Ok(StrykeValue::string(normalized))
}

// ── Dates / calendars ───────────────────────────────────────────────────────

fn builtin_days_in_year(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let y = i1(args);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    Ok(StrykeValue::integer(if leap { 366 } else { 365 }))
}
fn builtin_quarter_of_year(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let m = i1(args);
    Ok(StrykeValue::integer(((m - 1) / 3) + 1))
}
fn builtin_zeller_day_of_week(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let y = args.first().map(|v| v.to_number() as i64).unwrap_or(2000);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let (yy, mm) = if m < 3 { (y - 1, m + 12) } else { (y, m) };
    let k = yy % 100; let j = yy / 100;
    let h = (d + 13 * (mm + 1) / 5 + k + k / 4 + j / 4 + 5 * j) % 7;
    // Zeller: 0 = Saturday, 1 = Sunday, ... convert to 0 = Monday.
    let dow = ((h + 5) % 7 + 7) % 7;
    Ok(StrykeValue::integer(dow))
}
fn builtin_age_from_birthdate(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let by = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let bm = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let bd = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let cy = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let cm = args.get(4).map(|v| v.to_number() as i64).unwrap_or(1);
    let cd = args.get(5).map(|v| v.to_number() as i64).unwrap_or(1);
    let mut age = cy - by;
    if (cm, cd) < (bm, bd) { age -= 1; }
    Ok(StrykeValue::integer(age))
}
fn builtin_business_days_between(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let y1 = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let m1 = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d1 = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let y2 = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let m2 = args.get(4).map(|v| v.to_number() as i64).unwrap_or(1);
    let d2 = args.get(5).map(|v| v.to_number() as i64).unwrap_or(1);
    fn ymd_to_days_local(y: i64, m: i64, d: i64) -> i64 {
        let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
        365 * y + y / 4 - y / 100 + y / 400 + (153 * (m - 3) + 2) / 5 + d - 306
    }
    let s = ymd_to_days_local(y1, m1, d1);
    let e = ymd_to_days_local(y2, m2, d2);
    let mut count = 0_i64;
    for d in s..e {
        let dow = d.rem_euclid(7);
        if dow < 5 { count += 1; }
    }
    Ok(StrykeValue::integer(count))
}
fn builtin_unix_epoch_to_iso(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let secs = i1(args);
    let days = secs.div_euclid(86400);
    let seconds_today = secs.rem_euclid(86400);
    let h = seconds_today / 3600;
    let m = (seconds_today % 3600) / 60;
    let s = seconds_today % 60;
    let epoch_to_ymd = |d: i64| -> (i64, i64, i64) {
        let z = d + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = (z - era * 146097) as u64;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe as i64 + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as i64;
        let month = if mp < 10 { mp + 3 } else { mp.wrapping_sub(9) } as i64;
        let year = if month <= 2 { y + 1 } else { y };
        (year, month, day)
    };
    let (y, mo, d) = epoch_to_ymd(days);
    Ok(StrykeValue::string(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, s)))
}

// ── Loans / amortization ────────────────────────────────────────────────────

fn builtin_loan_payment_pmt(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let principal = f1(args); let rate_per_period = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let n_periods = args.get(2).map(|x| x.to_number() as usize).unwrap_or(0);
    if rate_per_period.abs() < 1e-15 { return Ok(StrykeValue::float(principal / n_periods.max(1) as f64)); }
    let factor = (1.0 + rate_per_period).powi(n_periods as i32);
    Ok(StrykeValue::float(principal * rate_per_period * factor / (factor - 1.0)))
}
fn builtin_loan_balance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let principal = f1(args); let rate = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let payment = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    let n_paid = args.get(3).map(|x| x.to_number() as usize).unwrap_or(0);
    let factor = (1.0 + rate).powi(n_paid as i32);
    Ok(StrykeValue::float(principal * factor - payment * (factor - 1.0) / rate.max(1e-30)))
}
fn builtin_amortization_total_interest(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let principal = f1(args); let rate = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|x| x.to_number() as usize).unwrap_or(0);
    let pmt = if rate.abs() < 1e-15 { principal / n as f64 } else {
        let factor = (1.0 + rate).powi(n as i32);
        principal * rate * factor / (factor - 1.0)
    };
    Ok(StrykeValue::float(pmt * n as f64 - principal))
}
fn builtin_apr_to_apy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let apr = f1(args); let n = args.get(1).map(|x| x.to_number()).unwrap_or(12.0);
    Ok(StrykeValue::float((1.0 + apr / n).powf(n) - 1.0))
}
fn builtin_apy_to_apr(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let apy = f1(args); let n = args.get(1).map(|x| x.to_number()).unwrap_or(12.0);
    Ok(StrykeValue::float(n * ((1.0 + apy).powf(1.0 / n) - 1.0)))
}
fn builtin_compound_interest_periods(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pv = f1(args); let r = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|x| x.to_number() as i32).unwrap_or(1);
    let t = args.get(3).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(pv * (1.0 + r / n as f64).powf(n as f64 * t)))
}
fn builtin_simple_interest_compute(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p = f1(args); let r = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(p * r * t))
}
fn builtin_perpetuity_value(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cash = f1(args); let r = args.get(1).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(StrykeValue::float(cash / r))
}
fn builtin_growing_perpetuity(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cash = f1(args); let r = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let g = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    let denom = (r - g).max(1e-30);
    Ok(StrykeValue::float(cash / denom))
}
fn builtin_annuity_present_value(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cash = f1(args); let r = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|x| x.to_number() as i32).unwrap_or(0);
    if r.abs() < 1e-15 { return Ok(StrykeValue::float(cash * n as f64)); }
    Ok(StrykeValue::float(cash * (1.0 - (1.0 + r).powi(-n)) / r))
}
fn builtin_annuity_future_value(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cash = f1(args); let r = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|x| x.to_number() as i32).unwrap_or(0);
    if r.abs() < 1e-15 { return Ok(StrykeValue::float(cash * n as f64)); }
    Ok(StrykeValue::float(cash * ((1.0 + r).powi(n) - 1.0) / r))
}
fn builtin_capm_expected_return(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let rf = f1(args); let beta = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let rm = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(rf + beta * (rm - rf)))
}
fn builtin_treynor_ratio(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let rp = f1(args); let rf = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(StrykeValue::float((rp - rf) / beta))
}
fn builtin_jensens_alpha(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let rp = f1(args); let rf = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    let rm = args.get(3).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(rp - (rf + beta * (rm - rf))))
}
fn builtin_information_ratio(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let rp = f1(args); let rb = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let tracking = args.get(2).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(StrykeValue::float((rp - rb) / tracking))
}

// ── Fluid mechanics ─────────────────────────────────────────────────────────

fn builtin_friction_factor_laminar(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let re = f1(args).max(1e-30);
    Ok(StrykeValue::float(64.0 / re))
}
fn builtin_swamee_jain_factor(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let re = f1(args); let eps_d = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let inner = (eps_d / 3.7) + (5.74 / re.powf(0.9));
    Ok(StrykeValue::float(0.25 / (inner.log10()).powi(2)))
}
fn builtin_pipe_pressure_drop(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let f = f1(args); let l = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let d = args.get(2).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    let rho = args.get(3).map(|x| x.to_number()).unwrap_or(1000.0);
    let v = args.get(4).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(f * (l / d) * 0.5 * rho * v * v))
}
fn builtin_orifice_velocity(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let dp = f1(args); let rho = args.get(1).map(|x| x.to_number()).unwrap_or(1000.0).max(1e-30);
    Ok(StrykeValue::float((2.0 * dp / rho).sqrt()))
}
fn builtin_chezy_velocity(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let c = f1(args); let r = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let s = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(c * (r * s).sqrt()))
}
fn builtin_manning_velocity(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = f1(args).max(1e-30); let r = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let s = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(r.powf(2.0 / 3.0) * s.sqrt() / n))
}
fn builtin_froude_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let v = f1(args); let g = args.get(1).map(|x| x.to_number()).unwrap_or(9.81);
    let l = args.get(2).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(StrykeValue::float(v / (g * l).sqrt()))
}
fn builtin_weber_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    // We = ρ·v²·L / σ. Returns Infinity (or 0 for zero numerator) when σ is
    // omitted or non-positive, instead of clamping σ to 1e-30 and emitting
    // a giant spurious finite number (BUG-134).
    let rho = f1(args);
    let v = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let l = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    let sigma = args.get(3).map(|x| x.to_number()).unwrap_or(0.0);
    let num = rho * v * v * l;
    if sigma <= 0.0 {
        if num == 0.0 {
            return Ok(StrykeValue::float(f64::NAN));
        }
        return Ok(StrykeValue::float(num.signum() * f64::INFINITY));
    }
    Ok(StrykeValue::float(num / sigma))
}
fn builtin_grashof_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let g = f1(args); let beta = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let dt = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    let l = args.get(3).map(|x| x.to_number()).unwrap_or(0.0);
    let nu = args.get(4).map(|x| x.to_number()).unwrap_or(0.0).max(1e-30);
    Ok(StrykeValue::float(g * beta * dt * l.powi(3) / (nu * nu)))
}
fn builtin_nusselt_dittus_boelter(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let re = f1(args); let pr = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|x| x.to_number()).unwrap_or(0.4);
    Ok(StrykeValue::float(0.023 * re.powf(0.8) * pr.powf(n)))
}
