//! Game theory, ML inference primitives, operations research,
//! chemistry, language modeling, information-theoretic divergences.

use crate::value::StrykeValue;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

fn arg_f64(args: &[StrykeValue], idx: usize) -> Option<f64> {
    args.get(idx).map(|v| v.to_number())
}

fn arg_i64(args: &[StrykeValue], idx: usize) -> Option<i64> {
    args.get(idx).map(|v| v.to_int())
}

fn arg_str(args: &[StrykeValue], idx: usize) -> Option<String> {
    args.get(idx).map(|v| v.as_str_or_empty())
}

fn as_vec_f64(v: &StrykeValue) -> Vec<f64> {
    if let Some(a) = v.as_array_ref() {
        return a.read().iter().map(|x| x.to_number()).collect();
    }
    if let Some(a) = v.as_array_vec() {
        return a.iter().map(|x| x.to_number()).collect();
    }
    Vec::new()
}

fn as_vec_sv(v: &StrykeValue) -> Vec<StrykeValue> {
    if let Some(a) = v.as_array_ref() {
        return a.read().clone();
    }
    if let Some(a) = v.as_array_vec() {
        return a.to_vec();
    }
    Vec::new()
}

fn as_matrix(v: &StrykeValue) -> Vec<Vec<f64>> {
    as_vec_sv(v).iter().map(as_vec_f64).collect()
}

fn arr_sv(v: Vec<StrykeValue>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(v)))
}

fn arr_f64(v: Vec<f64>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(
        v.into_iter().map(StrykeValue::float).collect(),
    )))
}

fn matrix_to_sv(m: &[Vec<f64>]) -> StrykeValue {
    arr_sv(m.iter().map(|r| arr_f64(r.clone())).collect())
}

fn make_hash(pairs: Vec<(&str, StrykeValue)>) -> StrykeValue {
    use indexmap::IndexMap;
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    for (k, v) in pairs {
        h.insert(k.to_string(), v);
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

// ══════════════════════════════════════════════════════════════════════
// Game theory
// ══════════════════════════════════════════════════════════════════════
/// `minimax_value` — see implementation.

pub fn minimax_value(args: &[StrykeValue]) -> StrykeValue {
    let leaves = args.first().map(as_vec_f64).unwrap_or_default();
    let depth = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    if leaves.is_empty() {
        return StrykeValue::float(0.0);
    }
    fn helper(values: &[f64], depth: usize, maximizing: bool) -> f64 {
        if depth == 0 || values.len() == 1 {
            return values[0];
        }
        let half = values.len() / 2;
        let left = helper(&values[..half], depth - 1, !maximizing);
        let right = helper(&values[half..], depth - 1, !maximizing);
        if maximizing {
            left.max(right)
        } else {
            left.min(right)
        }
    }
    StrykeValue::float(helper(&leaves, depth, true))
}
/// `alphabeta_value` — see implementation.

pub fn alphabeta_value(args: &[StrykeValue]) -> StrykeValue {
    let leaves = args.first().map(as_vec_f64).unwrap_or_default();
    let depth = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    if leaves.is_empty() {
        return StrykeValue::float(0.0);
    }
    fn helper(
        values: &[f64],
        depth: usize,
        mut alpha: f64,
        mut beta: f64,
        maximizing: bool,
    ) -> f64 {
        if depth == 0 || values.len() == 1 {
            return values[0];
        }
        let half = values.len() / 2;
        if maximizing {
            let mut value = f64::NEG_INFINITY;
            let left = helper(&values[..half], depth - 1, alpha, beta, false);
            value = value.max(left);
            alpha = alpha.max(value);
            if alpha >= beta {
                return value;
            }
            let right = helper(&values[half..], depth - 1, alpha, beta, false);
            value.max(right)
        } else {
            let mut value = f64::INFINITY;
            let left = helper(&values[..half], depth - 1, alpha, beta, true);
            value = value.min(left);
            beta = beta.min(value);
            if alpha >= beta {
                return value;
            }
            let right = helper(&values[half..], depth - 1, alpha, beta, true);
            value.min(right)
        }
    }
    StrykeValue::float(helper(
        &leaves,
        depth,
        f64::NEG_INFINITY,
        f64::INFINITY,
        true,
    ))
}
/// `expectiminimax_value` — see implementation.

pub fn expectiminimax_value(args: &[StrykeValue]) -> StrykeValue {
    let leaves = args.first().map(as_vec_f64).unwrap_or_default();
    let depth = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let chance_prob = arg_f64(args, 2).unwrap_or(0.5);
    if leaves.is_empty() {
        return StrykeValue::float(0.0);
    }
    fn helper(values: &[f64], depth: usize, mode: u8, p: f64) -> f64 {
        if depth == 0 || values.len() == 1 {
            return values[0];
        }
        let half = values.len() / 2;
        let left = helper(&values[..half], depth - 1, (mode + 1) % 3, p);
        let right = helper(&values[half..], depth - 1, (mode + 1) % 3, p);
        match mode {
            0 => left.max(right),
            1 => left.min(right),
            _ => p * left + (1.0 - p) * right,
        }
    }
    StrykeValue::float(helper(&leaves, depth, 0, chance_prob))
}
/// `mixed_strategy_2x2` — see implementation.

pub fn mixed_strategy_2x2(args: &[StrykeValue]) -> StrykeValue {
    // 2x2 zero-sum game with row player payoff matrix [[a,b],[c,d]].
    // For non-zero-sum games the col strategy formula is wrong; this only
    // works when col's payoff = -row's payoff.
    let m = args.first().map(as_matrix).unwrap_or_default();
    if m.len() < 2 || m[0].len() < 2 || m[1].len() < 2 {
        return StrykeValue::UNDEF;
    }
    let a = m[0][0];
    let b = m[0][1];
    let c = m[1][0];
    let d = m[1][1];
    let denom = a - b - c + d;
    if denom.abs() < 1e-12 {
        return StrykeValue::UNDEF;
    }
    let p_row = (d - c) / denom;
    let p_col = (d - b) / denom;
    make_hash(vec![
        ("row_strategy", StrykeValue::float(p_row.clamp(0.0, 1.0))),
        ("col_strategy", StrykeValue::float(p_col.clamp(0.0, 1.0))),
    ])
}
/// `zero_sum_value` — see implementation.

pub fn zero_sum_value(args: &[StrykeValue]) -> StrykeValue {
    // Zero-sum game value for row player's payoff matrix.
    // Pure-strategy: maximin (row) == minmax (col) → that value.
    // 2x2 with no saddle point: mixed-strategy value v = (a·d − b·c) / (a + d − b − c).
    // Larger matrices with no saddle: UNDEF (caller must use LP).
    let m = args.first().map(as_matrix).unwrap_or_default();
    if m.is_empty() || m[0].is_empty() {
        return StrykeValue::UNDEF;
    }
    let cols = m.iter().map(|r| r.len()).min().unwrap_or(0);
    if cols == 0 {
        return StrykeValue::UNDEF;
    }
    let row_mins: Vec<f64> = m
        .iter()
        .map(|r| r[..cols].iter().cloned().fold(f64::INFINITY, f64::min))
        .collect();
    let col_maxs: Vec<f64> = (0..cols)
        .map(|j| m.iter().map(|r| r[j]).fold(f64::NEG_INFINITY, f64::max))
        .collect();
    let maximin = row_mins.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let minmax = col_maxs.iter().cloned().fold(f64::INFINITY, f64::min);
    if (maximin - minmax).abs() < 1e-12 {
        return StrykeValue::float(maximin);
    }
    if m.len() == 2 && cols == 2 {
        let a = m[0][0];
        let b = m[0][1];
        let c = m[1][0];
        let d = m[1][1];
        let denom = a + d - b - c;
        if denom.abs() < 1e-12 {
            return StrykeValue::UNDEF;
        }
        return StrykeValue::float((a * d - b * c) / denom);
    }
    StrykeValue::UNDEF
}

// ══════════════════════════════════════════════════════════════════════
// Operations research
// ══════════════════════════════════════════════════════════════════════
/// `knapsack_unbounded` — see implementation.

pub fn knapsack_unbounded(args: &[StrykeValue]) -> StrykeValue {
    let values: Vec<i64> = args
        .first()
        .map(as_vec_sv)
        .unwrap_or_default()
        .iter()
        .map(|x| x.to_int())
        .collect();
    let weights: Vec<i64> = args
        .get(1)
        .map(as_vec_sv)
        .unwrap_or_default()
        .iter()
        .map(|x| x.to_int())
        .collect();
    let cap = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    let n = values.len().min(weights.len());
    let mut dp = vec![0i64; cap + 1];
    for w in 1..=cap {
        for i in 0..n {
            if weights[i] >= 0 && weights[i] as usize <= w {
                dp[w] = dp[w].max(dp[w - weights[i] as usize] + values[i]);
            }
        }
    }
    StrykeValue::integer(dp[cap])
}
/// `knapsack_fractional` — see implementation.

pub fn knapsack_fractional(args: &[StrykeValue]) -> StrykeValue {
    let values: Vec<f64> = args.first().map(as_vec_f64).unwrap_or_default();
    let weights: Vec<f64> = args.get(1).map(as_vec_f64).unwrap_or_default();
    let cap = arg_f64(args, 2).unwrap_or(0.0).max(0.0);
    let n = values.len().min(weights.len());
    let mut items: Vec<(f64, f64, f64)> = (0..n)
        .filter(|&i| weights[i] > 0.0)
        .map(|i| (values[i] / weights[i], values[i], weights[i]))
        .collect();
    items.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut total = 0.0_f64;
    let mut remaining = cap;
    for (_, v, w) in items {
        if remaining >= w {
            total += v;
            remaining -= w;
        } else if remaining > 0.0 {
            total += v * remaining / w;
            break;
        }
    }
    StrykeValue::float(total)
}
/// `tsp_2opt` — see implementation.

pub fn tsp_2opt(args: &[StrykeValue]) -> StrykeValue {
    let dist = args.first().map(as_matrix).unwrap_or_default();
    let n = dist.len();
    if n < 2 {
        return arr_f64((0..n as i64).map(|i| i as f64).collect());
    }
    let mut tour: Vec<usize> = (0..n).collect();
    let tour_dist = |t: &[usize]| -> f64 {
        let mut d = 0.0;
        for i in 0..t.len() {
            let a = t[i];
            let b = t[(i + 1) % t.len()];
            d += dist[a].get(b).copied().unwrap_or(0.0);
        }
        d
    };
    let mut improved = true;
    while improved {
        improved = false;
        for i in 1..n - 1 {
            for j in i + 1..n {
                let mut new_tour = tour.clone();
                new_tour[i..=j].reverse();
                if tour_dist(&new_tour) < tour_dist(&tour) {
                    tour = new_tour;
                    improved = true;
                }
            }
        }
    }
    arr_sv(
        tour.into_iter()
            .map(|x| StrykeValue::integer(x as i64))
            .collect(),
    )
}
/// `lp_simplex_max` — see implementation.

pub fn lp_simplex_max(args: &[StrykeValue]) -> StrykeValue {
    // Revised simplex method for LP max c·x  subject to  Ax ≤ b, x ≥ 0.
    // Builds the standard-form tableau with slack columns, then pivots
    // until reduced costs are non-negative.
    let c = args.first().map(as_vec_f64).unwrap_or_default();
    let a_mat = args.get(1).map(as_matrix).unwrap_or_default();
    let b = args.get(2).map(as_vec_f64).unwrap_or_default();
    let m = a_mat.len();
    let n = c.len();
    if m == 0 || n == 0 {
        return StrykeValue::float(0.0);
    }
    // Build tableau with slack variables
    let mut tab = vec![vec![0.0_f64; n + m + 1]; m + 1];
    for i in 0..m {
        for j in 0..n {
            tab[i][j] = a_mat[i].get(j).copied().unwrap_or(0.0);
        }
        tab[i][n + i] = 1.0;
        tab[i][n + m] = b.get(i).copied().unwrap_or(0.0);
    }
    for j in 0..n {
        tab[m][j] = -c[j];
    }
    for _ in 0..200 {
        let pivot_col = (0..n + m)
            .min_by(|&a, &b| {
                tab[m][a]
                    .partial_cmp(&tab[m][b])
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(0);
        if tab[m][pivot_col] >= -1e-12 {
            break;
        }
        let mut pivot_row = usize::MAX;
        let mut min_ratio = f64::INFINITY;
        for i in 0..m {
            if tab[i][pivot_col] > 1e-12 {
                let ratio = tab[i][n + m] / tab[i][pivot_col];
                if ratio < min_ratio {
                    min_ratio = ratio;
                    pivot_row = i;
                }
            }
        }
        if pivot_row == usize::MAX {
            return StrykeValue::float(f64::INFINITY);
        }
        let pivot = tab[pivot_row][pivot_col];
        for j in 0..n + m + 1 {
            tab[pivot_row][j] /= pivot;
        }
        for i in 0..=m {
            if i != pivot_row {
                let factor = tab[i][pivot_col];
                for j in 0..n + m + 1 {
                    tab[i][j] -= factor * tab[pivot_row][j];
                }
            }
        }
    }
    StrykeValue::float(tab[m][n + m])
}
/// `lp_simplex_min` — see implementation.

pub fn lp_simplex_min(args: &[StrykeValue]) -> StrykeValue {
    // Convert to max by negating c
    let c: Vec<f64> = args
        .first()
        .map(as_vec_f64)
        .unwrap_or_default()
        .into_iter()
        .map(|x| -x)
        .collect();
    let neg_c = arr_f64(c);
    let mut new_args = vec![neg_c];
    if let Some(a) = args.get(1).cloned() {
        new_args.push(a);
    }
    if let Some(b) = args.get(2).cloned() {
        new_args.push(b);
    }
    StrykeValue::float(-lp_simplex_max(&new_args).to_number())
}
/// `job_schedule_spt` — see implementation.

pub fn job_schedule_spt(args: &[StrykeValue]) -> StrykeValue {
    // Shortest processing time first
    let durations: Vec<(usize, f64)> = args
        .first()
        .map(as_vec_f64)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .collect();
    let mut sorted = durations;
    sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    arr_sv(
        sorted
            .iter()
            .map(|(i, _)| StrykeValue::integer(*i as i64))
            .collect(),
    )
}
/// `job_schedule_ljf` — see implementation.

pub fn job_schedule_ljf(args: &[StrykeValue]) -> StrykeValue {
    // Longest job first
    let durations: Vec<(usize, f64)> = args
        .first()
        .map(as_vec_f64)
        .unwrap_or_default()
        .into_iter()
        .enumerate()
        .collect();
    let mut sorted = durations;
    sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    arr_sv(
        sorted
            .iter()
            .map(|(i, _)| StrykeValue::integer(*i as i64))
            .collect(),
    )
}

fn bfs_augment(capacity: &mut [Vec<f64>], s: usize, t: usize) -> Option<(Vec<usize>, f64)> {
    use std::collections::VecDeque;
    let n = capacity.len();
    let mut parent = vec![usize::MAX; n];
    parent[s] = s;
    let mut q = VecDeque::new();
    q.push_back(s);
    while let Some(u) = q.pop_front() {
        if u == t {
            let mut path = vec![t];
            let mut cur = t;
            let mut min_cap = f64::INFINITY;
            while cur != s {
                let p = parent[cur];
                min_cap = min_cap.min(capacity[p][cur]);
                cur = p;
                path.push(cur);
            }
            path.reverse();
            return Some((path, min_cap));
        }
        for v in 0..n {
            if parent[v] == usize::MAX && capacity[u][v] > 1e-12 {
                parent[v] = u;
                q.push_back(v);
            }
        }
    }
    None
}
/// `edmonds_karp_max_flow` — see implementation.

pub fn edmonds_karp_max_flow(args: &[StrykeValue]) -> StrykeValue {
    let mut cap = args.first().map(as_matrix).unwrap_or_default();
    let s = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let t = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    let n = cap.len();
    if s >= n || t >= n {
        return StrykeValue::float(0.0);
    }
    let mut flow = 0.0;
    while let Some((path, min_cap)) = bfs_augment(&mut cap, s, t) {
        for w in path.windows(2) {
            cap[w[0]][w[1]] -= min_cap;
            cap[w[1]][w[0]] += min_cap;
        }
        flow += min_cap;
    }
    StrykeValue::float(flow)
}
/// `matching_bipartite_greedy` — see implementation.

pub fn matching_bipartite_greedy(args: &[StrykeValue]) -> StrykeValue {
    let edges = args.first().map(as_vec_sv).unwrap_or_default();
    let mut matched_left: HashMap<i64, i64> = HashMap::new();
    let mut matched_right: HashMap<i64, i64> = HashMap::new();
    for e in edges {
        let pair = as_vec_sv(&e);
        if pair.len() < 2 {
            continue;
        }
        let u = pair[0].to_int();
        let v = pair[1].to_int();
        if !matched_left.contains_key(&u) && !matched_right.contains_key(&v) {
            matched_left.insert(u, v);
            matched_right.insert(v, u);
        }
    }
    let result: Vec<StrykeValue> = matched_left
        .iter()
        .map(|(&u, &v)| arr_sv(vec![StrykeValue::integer(u), StrykeValue::integer(v)]))
        .collect();
    arr_sv(result)
}
/// `matching_bipartite_hungarian` — see implementation.

pub fn matching_bipartite_hungarian(args: &[StrykeValue]) -> StrykeValue {
    // Hungarian algorithm for min-cost bipartite matching on a square cost matrix
    let cost = args.first().map(as_matrix).unwrap_or_default();
    let n = cost.len();
    if n == 0 || cost[0].len() != n {
        return arr_sv(vec![]);
    }
    let mut u = vec![0.0_f64; n + 1];
    let mut v = vec![0.0_f64; n + 1];
    let mut p = vec![0usize; n + 1];
    let mut way = vec![0usize; n + 1];
    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0;
        let mut minv = vec![f64::INFINITY; n + 1];
        let mut used = vec![false; n + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = f64::INFINITY;
            let mut j1 = 0;
            for j in 1..=n {
                if !used[j] {
                    let cur = cost[i0 - 1][j - 1] - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=n {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut result: Vec<StrykeValue> = Vec::new();
    for j in 1..=n {
        if p[j] > 0 {
            result.push(arr_sv(vec![
                StrykeValue::integer((p[j] - 1) as i64),
                StrykeValue::integer((j - 1) as i64),
            ]));
        }
    }
    arr_sv(result)
}

// ══════════════════════════════════════════════════════════════════════
// ML inference primitives
// ══════════════════════════════════════════════════════════════════════
/// `ml_sigmoid_layer` — see implementation.

pub fn ml_sigmoid_layer(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    arr_f64(xs.iter().map(|x| 1.0 / (1.0 + (-x).exp())).collect())
}
/// `ml_tanh_layer` — see implementation.

pub fn ml_tanh_layer(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    arr_f64(xs.iter().map(|x| x.tanh()).collect())
}
/// `ml_relu_layer` — see implementation.

pub fn ml_relu_layer(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    arr_f64(xs.iter().map(|x| x.max(0.0)).collect())
}
/// `ml_leaky_relu_layer` — see implementation.

pub fn ml_leaky_relu_layer(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    let slope = arg_f64(args, 1).unwrap_or(0.01);
    arr_f64(
        xs.iter()
            .map(|x| if *x > 0.0 { *x } else { slope * x })
            .collect(),
    )
}
/// `ml_elu_layer` — see implementation.

pub fn ml_elu_layer(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    let alpha = arg_f64(args, 1).unwrap_or(1.0);
    arr_f64(
        xs.iter()
            .map(|x| {
                if *x > 0.0 {
                    *x
                } else {
                    alpha * (x.exp() - 1.0)
                }
            })
            .collect(),
    )
}
/// `ml_softmax_layer` — see implementation.

pub fn ml_softmax_layer(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    if xs.is_empty() {
        return arr_f64(vec![]);
    }
    let max = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = xs.iter().map(|x| (x - max).exp()).collect();
    let sum: f64 = exps.iter().sum();
    arr_f64(exps.iter().map(|x| x / sum).collect())
}
/// `ml_softplus_layer` — see implementation.

pub fn ml_softplus_layer(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    arr_f64(xs.iter().map(|x| (1.0 + x.exp()).ln()).collect())
}
/// `ml_swish_layer` — see implementation.

pub fn ml_swish_layer(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    arr_f64(xs.iter().map(|x| x / (1.0 + (-x).exp())).collect())
}
/// `ml_gelu_layer` — see implementation.

pub fn ml_gelu_layer(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    arr_f64(
        xs.iter()
            .map(|x| {
                0.5 * x
                    * (1.0
                        + ((2.0 / std::f64::consts::PI).sqrt() * (x + 0.044715 * x.powi(3))).tanh())
            })
            .collect(),
    )
}
/// `ml_mish_layer` — see implementation.

pub fn ml_mish_layer(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    arr_f64(xs.iter().map(|x| x * (1.0 + x.exp()).ln().tanh()).collect())
}
/// `ml_dropout_mask` — see implementation.

pub fn ml_dropout_mask(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let p = arg_f64(args, 1).unwrap_or(0.5).clamp(0.0, 1.0);
    let seed = arg_i64(args, 2).unwrap_or(0) as u64;
    let mut state = seed.wrapping_add(0x9E3779B97F4A7C15);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let r = (state >> 32) as f64 / u32::MAX as f64;
        out.push(if r > p { 1.0 / (1.0 - p) } else { 0.0 });
    }
    arr_f64(out)
}
/// `ml_batch_norm` — see implementation.

pub fn ml_batch_norm(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    if xs.is_empty() {
        return arr_f64(vec![]);
    }
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64;
    let eps = arg_f64(args, 1).unwrap_or(1e-5);
    let std = (var + eps).sqrt();
    arr_f64(xs.iter().map(|x| (x - mean) / std).collect())
}
/// `ml_attention_score` — see implementation.

pub fn ml_attention_score(args: &[StrykeValue]) -> StrykeValue {
    let q = args.first().map(as_vec_f64).unwrap_or_default();
    let k = args.get(1).map(as_vec_f64).unwrap_or_default();
    if q.len() != k.len() {
        return StrykeValue::float(0.0);
    }
    let dot: f64 = q.iter().zip(k.iter()).map(|(a, b)| a * b).sum();
    StrykeValue::float(dot / (q.len() as f64).sqrt())
}
/// `ml_dot_product_attention` — see implementation.

pub fn ml_dot_product_attention(args: &[StrykeValue]) -> StrykeValue {
    let q = args.first().map(as_vec_f64).unwrap_or_default();
    let keys = args.get(1).map(as_matrix).unwrap_or_default();
    let values = args.get(2).map(as_matrix).unwrap_or_default();
    if keys.is_empty() || values.is_empty() {
        return arr_f64(vec![]);
    }
    let dim = q.len() as f64;
    let scores: Vec<f64> = keys
        .iter()
        .map(|k| q.iter().zip(k.iter()).map(|(a, b)| a * b).sum::<f64>() / dim.sqrt())
        .collect();
    let weights = as_vec_f64(&ml_softmax_layer(&[arr_f64(scores)]));
    let n = values[0].len();
    let mut out = vec![0.0; n];
    for (i, w) in weights.iter().enumerate() {
        if let Some(v_row) = values.get(i) {
            for (j, v) in v_row.iter().enumerate() {
                if j < n {
                    out[j] += w * v;
                }
            }
        }
    }
    arr_f64(out)
}
/// `ml_self_attention` — see implementation.

pub fn ml_self_attention(args: &[StrykeValue]) -> StrykeValue {
    let x = args.first().map(as_matrix).unwrap_or_default();
    if x.is_empty() {
        return matrix_to_sv(&[]);
    }
    let n = x.len();
    let dim = x[0].len() as f64;
    let mut out = Vec::with_capacity(n);
    for q in &x {
        let scores: Vec<f64> = x
            .iter()
            .map(|k| q.iter().zip(k.iter()).map(|(a, b)| a * b).sum::<f64>() / dim.sqrt())
            .collect();
        let weights = as_vec_f64(&ml_softmax_layer(&[arr_f64(scores)]));
        let mut row = vec![0.0; x[0].len()];
        for (i, w) in weights.iter().enumerate() {
            for (j, v) in x[i].iter().enumerate() {
                row[j] += w * v;
            }
        }
        out.push(row);
    }
    matrix_to_sv(&out)
}
/// `ml_position_encoding` — see implementation.

pub fn ml_position_encoding(args: &[StrykeValue]) -> StrykeValue {
    let seq_len = arg_i64(args, 0).unwrap_or(10).max(1) as usize;
    let dim = arg_i64(args, 1).unwrap_or(8).max(2) as usize;
    let mut out = vec![vec![0.0; dim]; seq_len];
    for pos in 0..seq_len {
        for i in 0..dim / 2 {
            let div = 10000.0_f64.powf(2.0 * i as f64 / dim as f64);
            out[pos][2 * i] = (pos as f64 / div).sin();
            out[pos][2 * i + 1] = (pos as f64 / div).cos();
        }
    }
    matrix_to_sv(&out)
}
/// `ml_mse_loss` — see implementation.

pub fn ml_mse_loss(args: &[StrykeValue]) -> StrykeValue {
    let pred = args.first().map(as_vec_f64).unwrap_or_default();
    let target = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = pred.len().min(target.len());
    if n == 0 {
        return StrykeValue::float(0.0);
    }
    let sum: f64 = (0..n).map(|i| (pred[i] - target[i]).powi(2)).sum();
    StrykeValue::float(sum / n as f64)
}
/// `ml_mae_loss` — see implementation.

pub fn ml_mae_loss(args: &[StrykeValue]) -> StrykeValue {
    let pred = args.first().map(as_vec_f64).unwrap_or_default();
    let target = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = pred.len().min(target.len());
    if n == 0 {
        return StrykeValue::float(0.0);
    }
    let sum: f64 = (0..n).map(|i| (pred[i] - target[i]).abs()).sum();
    StrykeValue::float(sum / n as f64)
}
/// `ml_huber_loss` — see implementation.

pub fn ml_huber_loss(args: &[StrykeValue]) -> StrykeValue {
    let pred = args.first().map(as_vec_f64).unwrap_or_default();
    let target = args.get(1).map(as_vec_f64).unwrap_or_default();
    let delta = arg_f64(args, 2).unwrap_or(1.0);
    let n = pred.len().min(target.len());
    if n == 0 {
        return StrykeValue::float(0.0);
    }
    let sum: f64 = (0..n)
        .map(|i| {
            let diff = (pred[i] - target[i]).abs();
            if diff <= delta {
                0.5 * diff.powi(2)
            } else {
                delta * (diff - 0.5 * delta)
            }
        })
        .sum();
    StrykeValue::float(sum / n as f64)
}
/// `ml_hinge_loss` — see implementation.

pub fn ml_hinge_loss(args: &[StrykeValue]) -> StrykeValue {
    let pred = args.first().map(as_vec_f64).unwrap_or_default();
    let target = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = pred.len().min(target.len());
    if n == 0 {
        return StrykeValue::float(0.0);
    }
    let sum: f64 = (0..n).map(|i| (1.0 - target[i] * pred[i]).max(0.0)).sum();
    StrykeValue::float(sum / n as f64)
}
/// `ml_kl_div_loss` — see implementation.

pub fn ml_kl_div_loss(args: &[StrykeValue]) -> StrykeValue {
    let p = args.first().map(as_vec_f64).unwrap_or_default();
    let q = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = p.len().min(q.len());
    let sum: f64 = (0..n)
        .filter(|&i| p[i] > 0.0 && q[i] > 0.0)
        .map(|i| p[i] * (p[i] / q[i]).ln())
        .sum();
    StrykeValue::float(sum)
}
/// `ml_one_hot_encode` — see implementation.

pub fn ml_one_hot_encode(args: &[StrykeValue]) -> StrykeValue {
    let idx = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let n_classes = arg_i64(args, 1).unwrap_or(10).max(1) as usize;
    let mut out = vec![0.0; n_classes];
    if idx < n_classes {
        out[idx] = 1.0;
    }
    arr_f64(out)
}
/// `ml_label_smooth` — see implementation.

pub fn ml_label_smooth(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    let smoothing = arg_f64(args, 1).unwrap_or(0.1);
    let n = xs.len();
    if n == 0 {
        return arr_f64(vec![]);
    }
    arr_f64(
        xs.iter()
            .map(|x| x * (1.0 - smoothing) + smoothing / n as f64)
            .collect(),
    )
}

// ══════════════════════════════════════════════════════════════════════
// Chemistry
// ══════════════════════════════════════════════════════════════════════

const ATOMIC_MASSES: &[(&str, f64)] = &[
    ("H", 1.008),
    ("He", 4.003),
    ("Li", 6.941),
    ("Be", 9.012),
    ("B", 10.811),
    ("C", 12.011),
    ("N", 14.007),
    ("O", 15.999),
    ("F", 18.998),
    ("Ne", 20.180),
    ("Na", 22.990),
    ("Mg", 24.305),
    ("Al", 26.982),
    ("Si", 28.086),
    ("P", 30.974),
    ("S", 32.065),
    ("Cl", 35.453),
    ("Ar", 39.948),
    ("K", 39.098),
    ("Ca", 40.078),
    ("Fe", 55.845),
    ("Cu", 63.546),
    ("Zn", 65.380),
    ("Ag", 107.868),
    ("I", 126.904),
    ("Au", 196.967),
    ("Hg", 200.590),
    ("Pb", 207.200),
    ("U", 238.029),
];

fn parse_formula(formula: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let chars: Vec<char> = formula.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_uppercase() {
            i += 1;
            continue;
        }
        let mut elem = chars[i].to_string();
        i += 1;
        while i < chars.len() && chars[i].is_ascii_lowercase() {
            elem.push(chars[i]);
            i += 1;
        }
        let mut num_str = String::new();
        while i < chars.len() && chars[i].is_ascii_digit() {
            num_str.push(chars[i]);
            i += 1;
        }
        let count = num_str.parse().unwrap_or(1);
        out.push((elem, count));
    }
    out
}
/// `chem_formula_parse` — see implementation.

pub fn chem_formula_parse(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let parsed = parse_formula(&s);
    let entries: Vec<StrykeValue> = parsed
        .into_iter()
        .map(|(e, n)| arr_sv(vec![StrykeValue::string(e), StrykeValue::integer(n as i64)]))
        .collect();
    arr_sv(entries)
}
/// `chem_molar_mass` — see implementation.

pub fn chem_molar_mass(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let masses: HashMap<&str, f64> = ATOMIC_MASSES.iter().cloned().collect();
    let parsed = parse_formula(&s);
    let total: f64 = parsed
        .iter()
        .filter_map(|(e, n)| masses.get(e.as_str()).map(|m| m * *n as f64))
        .sum();
    StrykeValue::float(total)
}
/// `chem_balance_check` — see implementation.

pub fn chem_balance_check(args: &[StrykeValue]) -> StrykeValue {
    let lhs = arg_str(args, 0).unwrap_or_default();
    let rhs = arg_str(args, 1).unwrap_or_default();
    let mut left_count: HashMap<String, u32> = HashMap::new();
    for (e, n) in parse_formula(&lhs) {
        *left_count.entry(e).or_insert(0) += n;
    }
    let mut right_count: HashMap<String, u32> = HashMap::new();
    for (e, n) in parse_formula(&rhs) {
        *right_count.entry(e).or_insert(0) += n;
    }
    StrykeValue::integer(if left_count == right_count { 1 } else { 0 })
}
/// `chem_pka_lookup` — see implementation.

pub fn chem_pka_lookup(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let lookup: HashMap<&str, f64> = [
        ("HCl", -7.0),
        ("H2SO4", -3.0),
        ("HNO3", -1.4),
        ("HF", 3.17),
        ("CH3COOH", 4.76),
        ("H2CO3", 6.35),
        ("NH4+", 9.25),
        ("HCN", 9.21),
        ("HCO3-", 10.33),
        ("H2O", 15.7),
        ("NH3", 38.0),
    ]
    .into_iter()
    .collect();
    lookup
        .get(s.as_str())
        .map(|v| StrykeValue::float(*v))
        .unwrap_or(StrykeValue::UNDEF)
}
/// `chem_isoelectric_estimate` — see implementation.

pub fn chem_isoelectric_estimate(args: &[StrykeValue]) -> StrykeValue {
    // For simple amino-acid input: pI = (pK1 + pK2)/2
    let pk1 = arg_f64(args, 0).unwrap_or(2.0);
    let pk2 = arg_f64(args, 1).unwrap_or(10.0);
    StrykeValue::float((pk1 + pk2) / 2.0)
}
/// `chem_avogadro` — see implementation.

pub fn chem_avogadro(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(6.022_140_76e23)
}
/// `chem_ideal_gas_volume` — see implementation.

pub fn chem_ideal_gas_volume(args: &[StrykeValue]) -> StrykeValue {
    // PV = nRT => V = nRT/P, in liters with R = 0.08206 L·atm/(mol·K)
    let n = arg_f64(args, 0).unwrap_or(1.0);
    let t_k = arg_f64(args, 1).unwrap_or(273.15);
    let p_atm = arg_f64(args, 2).unwrap_or(1.0);
    StrykeValue::float(n * 0.08206 * t_k / p_atm)
}
/// `chem_partial_pressure` — see implementation.

pub fn chem_partial_pressure(args: &[StrykeValue]) -> StrykeValue {
    let total = arg_f64(args, 0).unwrap_or(1.0);
    let mole_frac = arg_f64(args, 1).unwrap_or(0.5);
    StrykeValue::float(total * mole_frac)
}
/// `chem_henderson_hasselbalch` — see implementation.

pub fn chem_henderson_hasselbalch(args: &[StrykeValue]) -> StrykeValue {
    let pka = arg_f64(args, 0).unwrap_or(0.0);
    let conj_base = arg_f64(args, 1).unwrap_or(1.0);
    let acid = arg_f64(args, 2).unwrap_or(1.0);
    if acid <= 0.0 {
        return StrykeValue::UNDEF;
    }
    StrykeValue::float(pka + (conj_base / acid).log10())
}
/// `chem_buffer_capacity` — see implementation.

pub fn chem_buffer_capacity(args: &[StrykeValue]) -> StrykeValue {
    // β = 2.303 * (Kw/[H+] + [H+] + Ca*Ka*[H+]/(Ka+[H+])^2)
    let h = arg_f64(args, 0).unwrap_or(1e-7);
    let ca = arg_f64(args, 1).unwrap_or(0.1);
    let ka = arg_f64(args, 2).unwrap_or(1.8e-5);
    let kw = 1e-14;
    let beta = 2.303 * (kw / h + h + ca * ka * h / (ka + h).powi(2));
    StrykeValue::float(beta)
}
/// `chem_dilution` — see implementation.

pub fn chem_dilution(args: &[StrykeValue]) -> StrykeValue {
    // C1V1 = C2V2 — solve for V2 given C1, V1, C2
    let c1 = arg_f64(args, 0).unwrap_or(1.0);
    let v1 = arg_f64(args, 1).unwrap_or(1.0);
    let c2 = arg_f64(args, 2).unwrap_or(0.1);
    if c2 == 0.0 {
        return StrykeValue::UNDEF;
    }
    StrykeValue::float(c1 * v1 / c2)
}
/// `chem_concentration_to_molarity` — see implementation.

pub fn chem_concentration_to_molarity(args: &[StrykeValue]) -> StrykeValue {
    let mass_g = arg_f64(args, 0).unwrap_or(0.0);
    let molar_mass = arg_f64(args, 1).unwrap_or(1.0).max(1e-12);
    let volume_l = arg_f64(args, 2).unwrap_or(1.0).max(1e-12);
    StrykeValue::float(mass_g / molar_mass / volume_l)
}
/// `chem_ph_from_h` — see implementation.

pub fn chem_ph_from_h(args: &[StrykeValue]) -> StrykeValue {
    let h = arg_f64(args, 0).unwrap_or(1e-7).max(1e-30);
    StrykeValue::float(-h.log10())
}
/// `chem_h_from_ph` — see implementation.

pub fn chem_h_from_ph(args: &[StrykeValue]) -> StrykeValue {
    let ph = arg_f64(args, 0).unwrap_or(7.0);
    StrykeValue::float(10f64.powf(-ph))
}
/// `chem_kelvin_to_celsius` — see implementation.

pub fn chem_kelvin_to_celsius(args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(arg_f64(args, 0).unwrap_or(0.0) - 273.15)
}
/// `chem_celsius_to_kelvin` — see implementation.

pub fn chem_celsius_to_kelvin(args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(arg_f64(args, 0).unwrap_or(0.0) + 273.15)
}
/// `chem_fahrenheit_to_celsius` — see implementation.

pub fn chem_fahrenheit_to_celsius(args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float((arg_f64(args, 0).unwrap_or(32.0) - 32.0) * 5.0 / 9.0)
}
/// `chem_celsius_to_fahrenheit` — see implementation.

pub fn chem_celsius_to_fahrenheit(args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(arg_f64(args, 0).unwrap_or(0.0) * 9.0 / 5.0 + 32.0)
}
/// `chem_kelvin_to_fahrenheit` — see implementation.

pub fn chem_kelvin_to_fahrenheit(args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float((arg_f64(args, 0).unwrap_or(0.0) - 273.15) * 9.0 / 5.0 + 32.0)
}
/// `chem_fahrenheit_to_kelvin` — see implementation.

pub fn chem_fahrenheit_to_kelvin(args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float((arg_f64(args, 0).unwrap_or(32.0) - 32.0) * 5.0 / 9.0 + 273.15)
}
/// `chem_rankine_to_kelvin` — see implementation.

pub fn chem_rankine_to_kelvin(args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(arg_f64(args, 0).unwrap_or(0.0) * 5.0 / 9.0)
}
/// `chem_kelvin_to_rankine` — see implementation.

pub fn chem_kelvin_to_rankine(args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::float(arg_f64(args, 0).unwrap_or(0.0) * 9.0 / 5.0)
}
/// `chem_molality` — see implementation.

pub fn chem_molality(args: &[StrykeValue]) -> StrykeValue {
    let moles = arg_f64(args, 0).unwrap_or(0.0);
    let kg_solvent = arg_f64(args, 1).unwrap_or(1.0).max(1e-12);
    StrykeValue::float(moles / kg_solvent)
}
/// `chem_molarity_to_normality` — see implementation.

pub fn chem_molarity_to_normality(args: &[StrykeValue]) -> StrykeValue {
    let molarity = arg_f64(args, 0).unwrap_or(0.0);
    let equiv_per_mole = arg_f64(args, 1).unwrap_or(1.0);
    StrykeValue::float(molarity * equiv_per_mole)
}
/// `chem_freezing_point_depression` — see implementation.

pub fn chem_freezing_point_depression(args: &[StrykeValue]) -> StrykeValue {
    // ΔTf = Kf * m * i
    let kf = arg_f64(args, 0).unwrap_or(1.86);
    let molality = arg_f64(args, 1).unwrap_or(1.0);
    let i = arg_f64(args, 2).unwrap_or(1.0);
    StrykeValue::float(kf * molality * i)
}
/// `chem_boiling_point_elevation` — see implementation.

pub fn chem_boiling_point_elevation(args: &[StrykeValue]) -> StrykeValue {
    let kb = arg_f64(args, 0).unwrap_or(0.512);
    let molality = arg_f64(args, 1).unwrap_or(1.0);
    let i = arg_f64(args, 2).unwrap_or(1.0);
    StrykeValue::float(kb * molality * i)
}
/// `chem_arrhenius_k` — see implementation.

pub fn chem_arrhenius_k(args: &[StrykeValue]) -> StrykeValue {
    // k = A * exp(-Ea/(RT)), R = 8.314 J/(mol·K)
    let a = arg_f64(args, 0).unwrap_or(1.0);
    let ea = arg_f64(args, 1).unwrap_or(0.0);
    let t_k = arg_f64(args, 2).unwrap_or(298.15);
    StrykeValue::float(a * (-ea / (8.314 * t_k)).exp())
}

// ══════════════════════════════════════════════════════════════════════
// Language modeling / information theory
// ══════════════════════════════════════════════════════════════════════
/// `ngram_train` — see implementation.

pub fn ngram_train(args: &[StrykeValue]) -> StrykeValue {
    let tokens: Vec<String> = args
        .first()
        .map(as_vec_sv)
        .unwrap_or_default()
        .iter()
        .map(|x| x.as_str_or_empty())
        .collect();
    let n = arg_i64(args, 1).unwrap_or(2).max(1) as usize;
    use indexmap::IndexMap;
    let mut counts: HashMap<String, HashMap<String, u64>> = HashMap::new();
    let mut totals: HashMap<String, u64> = HashMap::new();
    if tokens.len() < n {
        let h: IndexMap<String, StrykeValue> = IndexMap::new();
        return StrykeValue::hash_ref(Arc::new(RwLock::new(h)));
    }
    for i in 0..=tokens.len() - n {
        let ctx = tokens[i..i + n - 1].join(" ");
        let next = tokens[i + n - 1].clone();
        *counts
            .entry(ctx.clone())
            .or_default()
            .entry(next)
            .or_insert(0) += 1;
        *totals.entry(ctx).or_insert(0) += 1;
    }
    let mut model: IndexMap<String, StrykeValue> = IndexMap::new();
    for (ctx, next_counts) in counts {
        let total = totals[&ctx] as f64;
        let mut next_probs: IndexMap<String, StrykeValue> = IndexMap::new();
        for (next, count) in next_counts {
            next_probs.insert(next, StrykeValue::float(count as f64 / total));
        }
        model.insert(
            ctx,
            StrykeValue::hash_ref(Arc::new(RwLock::new(next_probs))),
        );
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(model)))
}
/// `ngram_prob` — see implementation.

pub fn ngram_prob(args: &[StrykeValue]) -> StrykeValue {
    let model = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let ctx = arg_str(args, 1).unwrap_or_default();
    let next = arg_str(args, 2).unwrap_or_default();
    if let Some(m) = model.as_hash_ref() {
        let m = m.read();
        if let Some(next_probs) = m.get(&ctx) {
            if let Some(np) = next_probs.as_hash_ref() {
                let np = np.read();
                if let Some(p) = np.get(&next) {
                    return p.clone();
                }
            }
        }
    }
    StrykeValue::float(0.0)
}
/// `ngram_perplexity` — see implementation.

pub fn ngram_perplexity(args: &[StrykeValue]) -> StrykeValue {
    let model = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let tokens: Vec<String> = args
        .get(1)
        .map(as_vec_sv)
        .unwrap_or_default()
        .iter()
        .map(|x| x.as_str_or_empty())
        .collect();
    let n = arg_i64(args, 2).unwrap_or(2).max(1) as usize;
    if tokens.len() < n {
        return StrykeValue::float(f64::INFINITY);
    }
    let mut log_sum = 0.0_f64;
    let mut count = 0;
    for i in 0..=tokens.len() - n {
        let ctx = tokens[i..i + n - 1].join(" ");
        let next = &tokens[i + n - 1];
        let p = ngram_prob(&[
            model.clone(),
            StrykeValue::string(ctx),
            StrykeValue::string(next.clone()),
        ])
        .to_number();
        if p > 0.0 {
            log_sum += p.ln();
        } else {
            log_sum += -30.0_f64;
        }
        count += 1;
    }
    if count == 0 {
        return StrykeValue::float(f64::INFINITY);
    }
    StrykeValue::float((-log_sum / count as f64).exp())
}
/// `ngram_top_k_next` — see implementation.

pub fn ngram_top_k_next(args: &[StrykeValue]) -> StrykeValue {
    let model = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let ctx = arg_str(args, 1).unwrap_or_default();
    let k = arg_i64(args, 2).unwrap_or(5).max(1) as usize;
    if let Some(m) = model.as_hash_ref() {
        let m = m.read();
        if let Some(next_probs) = m.get(&ctx) {
            if let Some(np) = next_probs.as_hash_ref() {
                let np = np.read();
                let mut entries: Vec<(String, f64)> =
                    np.iter().map(|(k, v)| (k.clone(), v.to_number())).collect();
                entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                let top: Vec<StrykeValue> = entries
                    .into_iter()
                    .take(k)
                    .map(|(s, p)| arr_sv(vec![StrykeValue::string(s), StrykeValue::float(p)]))
                    .collect();
                return arr_sv(top);
            }
        }
    }
    arr_sv(vec![])
}
/// `kl_divergence_distributions` — see implementation.

pub fn kl_divergence_distributions(args: &[StrykeValue]) -> StrykeValue {
    let p = args.first().map(as_vec_f64).unwrap_or_default();
    let q = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = p.len().min(q.len());
    let sum: f64 = (0..n)
        .filter(|&i| p[i] > 0.0 && q[i] > 0.0)
        .map(|i| p[i] * (p[i] / q[i]).ln())
        .sum();
    StrykeValue::float(sum)
}
/// `js_divergence_distributions` — see implementation.

pub fn js_divergence_distributions(args: &[StrykeValue]) -> StrykeValue {
    let p = args.first().map(as_vec_f64).unwrap_or_default();
    let q = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = p.len().min(q.len());
    let m: Vec<f64> = (0..n).map(|i| (p[i] + q[i]) / 2.0).collect();
    let kl_pm: f64 = (0..n)
        .filter(|&i| p[i] > 0.0 && m[i] > 0.0)
        .map(|i| p[i] * (p[i] / m[i]).ln())
        .sum();
    let kl_qm: f64 = (0..n)
        .filter(|&i| q[i] > 0.0 && m[i] > 0.0)
        .map(|i| q[i] * (q[i] / m[i]).ln())
        .sum();
    StrykeValue::float(0.5 * (kl_pm + kl_qm))
}
/// `conditional_entropy` — see implementation.

pub fn conditional_entropy(args: &[StrykeValue]) -> StrykeValue {
    let joint = args.first().map(as_matrix).unwrap_or_default();
    if joint.is_empty() {
        return StrykeValue::float(0.0);
    }
    let mut total = 0.0_f64;
    let py: Vec<f64> = (0..joint[0].len())
        .map(|j| joint.iter().map(|r| r.get(j).copied().unwrap_or(0.0)).sum())
        .collect();
    for (i, row) in joint.iter().enumerate() {
        for (j, &pxy) in row.iter().enumerate() {
            if pxy > 0.0 && py.get(j).copied().unwrap_or(0.0) > 0.0 {
                let _ = i;
                total -= pxy * (pxy / py[j]).ln();
            }
        }
    }
    StrykeValue::float(total)
}
/// `joint_entropy` — see implementation.

pub fn joint_entropy(args: &[StrykeValue]) -> StrykeValue {
    let joint = args.first().map(as_matrix).unwrap_or_default();
    let total: f64 = joint
        .iter()
        .flat_map(|r| r.iter())
        .filter(|&&p| p > 0.0)
        .map(|&p| -p * p.ln())
        .sum();
    StrykeValue::float(total)
}
/// `relative_entropy` — see implementation.

pub fn relative_entropy(args: &[StrykeValue]) -> StrykeValue {
    kl_divergence_distributions(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sv(x: f64) -> StrykeValue {
        StrykeValue::float(x)
    }
    fn sv_i(x: i64) -> StrykeValue {
        StrykeValue::integer(x)
    }
    fn sv_s(x: &str) -> StrykeValue {
        StrykeValue::string(x.to_string())
    }

    #[test]
    fn minimax_simple() {
        let leaves = arr_f64(vec![3.0, 5.0, 2.0, 9.0]);
        let r = minimax_value(&[leaves, sv_i(2)]).to_number();
        // Max of mins: min(3,5)=3, min(2,9)=2; max(3,2)=3
        assert_eq!(r, 3.0);
    }

    #[test]
    fn mixed_strategy_matching_pennies() {
        // [[1,-1],[-1,1]] — mixed strategy is 0.5
        let m = matrix_to_sv(&[vec![1.0, -1.0], vec![-1.0, 1.0]]);
        let r = mixed_strategy_2x2(&[m]);
        if let Some(h) = r.as_hash_ref() {
            let h = h.read();
            assert!((h.get("row_strategy").unwrap().to_number() - 0.5).abs() < 1e-9);
        }
    }

    #[test]
    fn knapsack_unbounded_basic() {
        let v = arr_sv(vec![sv_i(60), sv_i(100), sv_i(120)]);
        let w = arr_sv(vec![sv_i(10), sv_i(20), sv_i(30)]);
        let r = knapsack_unbounded(&[v, w, sv_i(50)]).to_int();
        // 10*5 = 60*5 = 300
        assert_eq!(r, 300);
    }

    #[test]
    fn knapsack_fractional_basic() {
        let v = arr_f64(vec![60.0, 100.0, 120.0]);
        let w = arr_f64(vec![10.0, 20.0, 30.0]);
        let r = knapsack_fractional(&[v, w, sv(50.0)]).to_number();
        // Density: 6, 5, 4 — take 10@6=60, 20@5=100, 20/30*120=80 → 240
        assert!((r - 240.0).abs() < 1e-9);
    }

    #[test]
    fn lp_max_simple() {
        // Max x+y s.t. x+y<=10, x>=0, y>=0
        let c = arr_f64(vec![1.0, 1.0]);
        let a = matrix_to_sv(&[vec![1.0, 1.0]]);
        let b = arr_f64(vec![10.0]);
        let r = lp_simplex_max(&[c, a, b]).to_number();
        assert!((r - 10.0).abs() < 1e-6);
    }

    #[test]
    fn ml_sigmoid_zero() {
        let r = ml_sigmoid_layer(&[arr_f64(vec![0.0, 1.0])]);
        let xs = as_vec_f64(&r);
        assert!((xs[0] - 0.5).abs() < 1e-9);
        assert!((xs[1] - 0.7310585786).abs() < 1e-6);
    }

    #[test]
    fn ml_softmax_sum_one() {
        let r = ml_softmax_layer(&[arr_f64(vec![1.0, 2.0, 3.0])]);
        let xs = as_vec_f64(&r);
        let sum: f64 = xs.iter().sum();
        assert!((sum - 1.0).abs() < 1e-9);
    }

    #[test]
    fn ml_relu_clamps() {
        let r = ml_relu_layer(&[arr_f64(vec![-1.0, 0.0, 5.0])]);
        let xs = as_vec_f64(&r);
        assert_eq!(xs, vec![0.0, 0.0, 5.0]);
    }

    #[test]
    fn ml_mse_loss_perfect() {
        let pred = arr_f64(vec![1.0, 2.0, 3.0]);
        let r = ml_mse_loss(&[pred.clone(), pred]).to_number();
        assert_eq!(r, 0.0);
    }

    #[test]
    fn chem_molar_mass_water() {
        let r = chem_molar_mass(&[sv_s("H2O")]).to_number();
        assert!((r - 18.015).abs() < 0.01);
    }

    #[test]
    fn chem_molar_mass_glucose() {
        // C6H12O6 = 180.16
        let r = chem_molar_mass(&[sv_s("C6H12O6")]).to_number();
        assert!((r - 180.156).abs() < 0.1);
    }

    #[test]
    fn chem_balance_water() {
        // Hardcoded — "H2O" vs "H2O" balanced
        let r = chem_balance_check(&[sv_s("H2O"), sv_s("H2O")]).to_int();
        assert_eq!(r, 1);
    }

    #[test]
    fn chem_ph_h_roundtrip() {
        let h = chem_h_from_ph(&[sv(7.0)]).to_number();
        let ph = chem_ph_from_h(&[sv(h)]).to_number();
        assert!((ph - 7.0).abs() < 1e-9);
    }

    #[test]
    fn temp_conversions() {
        assert!((chem_celsius_to_fahrenheit(&[sv(100.0)]).to_number() - 212.0).abs() < 1e-9);
        assert!((chem_fahrenheit_to_celsius(&[sv(32.0)]).to_number() - 0.0).abs() < 1e-9);
        assert!((chem_celsius_to_kelvin(&[sv(0.0)]).to_number() - 273.15).abs() < 1e-9);
    }

    #[test]
    fn ml_kl_div_zero_for_same() {
        let p = arr_f64(vec![0.3, 0.7]);
        let r = ml_kl_div_loss(&[p.clone(), p]).to_number();
        assert!(r.abs() < 1e-9);
    }

    #[test]
    fn ngram_perplexity_finite() {
        let tokens = arr_sv(vec![
            sv_s("the"),
            sv_s("quick"),
            sv_s("brown"),
            sv_s("fox"),
            sv_s("the"),
            sv_s("quick"),
        ]);
        let model = ngram_train(&[tokens.clone(), sv_i(2)]);
        let r = ngram_perplexity(&[model, tokens, sv_i(2)]).to_number();
        assert!(r.is_finite() && r > 0.0);
    }

    #[test]
    fn hungarian_3x3() {
        // Assignment problem with known min cost
        let cost = matrix_to_sv(&[
            vec![4.0, 1.0, 3.0],
            vec![2.0, 0.0, 5.0],
            vec![3.0, 2.0, 2.0],
        ]);
        let r = matching_bipartite_hungarian(&[cost]);
        let pairs = as_vec_sv(&r);
        assert_eq!(pairs.len(), 3);
    }
}
