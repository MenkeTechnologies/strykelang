//! Linear algebra (matrices), graph algorithms,
//! calendar/date helpers, special math. Pure functions only.

use crate::value::StrykeValue;
use parking_lot::RwLock;
use std::sync::Arc;
use std::collections::{HashSet, VecDeque, BinaryHeap};
use std::cmp::Ordering;

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

fn arr_f64(v: Vec<f64>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(
        v.into_iter().map(StrykeValue::float).collect(),
    )))
}

fn arr_sv(v: Vec<StrykeValue>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(v)))
}

fn matrix_to_sv(m: &[Vec<f64>]) -> StrykeValue {
    let rows: Vec<StrykeValue> = m.iter().map(|r| arr_f64(r.clone())).collect();
    arr_sv(rows)
}

// ══════════════════════════════════════════════════════════════════════
// Matrix operations
// ══════════════════════════════════════════════════════════════════════

pub fn matrix_new(args: &[StrykeValue]) -> StrykeValue {
    let rows = arg_i64(args, 0).unwrap_or(1).max(0) as usize;
    let cols = arg_i64(args, 1).unwrap_or(rows as i64).max(0) as usize;
    let fill = arg_f64(args, 2).unwrap_or(0.0);
    matrix_to_sv(&vec![vec![fill; cols]; rows])
}

pub fn matrix_rows(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    StrykeValue::integer(m.len() as i64)
}

pub fn matrix_cols(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    StrykeValue::integer(m.first().map_or(0, |r| r.len()) as i64)
}

pub fn matrix_get(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let r = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let c = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    m.get(r)
        .and_then(|row| row.get(c))
        .map(|x| StrykeValue::float(*x))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn matrix_set(args: &[StrykeValue]) -> StrykeValue {
    let mut m = args.first().map(as_matrix).unwrap_or_default();
    let r = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let c = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    let v = arg_f64(args, 3).unwrap_or(0.0);
    if r < m.len() && c < m[r].len() {
        m[r][c] = v;
    }
    matrix_to_sv(&m)
}

pub fn matrix_from_cols(args: &[StrykeValue]) -> StrykeValue {
    let cols: Vec<Vec<f64>> = args.iter().map(as_vec_f64).collect();
    if cols.is_empty() {
        return matrix_to_sv(&[]);
    }
    let n_rows = cols.iter().map(|c| c.len()).max().unwrap_or(0);
    let mut m = vec![vec![0.0; cols.len()]; n_rows];
    for (j, col) in cols.iter().enumerate() {
        for (i, v) in col.iter().enumerate() {
            m[i][j] = *v;
        }
    }
    matrix_to_sv(&m)
}

pub fn matrix_minor(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let skip_r = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let skip_c = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    let result: Vec<Vec<f64>> = m
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != skip_r)
        .map(|(_, row)| {
            row.iter()
                .enumerate()
                .filter(|(j, _)| *j != skip_c)
                .map(|(_, v)| *v)
                .collect()
        })
        .collect();
    matrix_to_sv(&result)
}

fn determinant(m: &[Vec<f64>]) -> f64 {
    let n = m.len();
    if n == 0 || m[0].len() != n {
        return 0.0;
    }
    let mut a: Vec<Vec<f64>> = m.to_vec();
    let mut sign = 1.0;
    for i in 0..n {
        let mut pivot = i;
        for k in i + 1..n {
            if a[k][i].abs() > a[pivot][i].abs() {
                pivot = k;
            }
        }
        if a[pivot][i].abs() < 1e-12 {
            return 0.0;
        }
        if pivot != i {
            a.swap(i, pivot);
            sign = -sign;
        }
        for k in i + 1..n {
            let factor = a[k][i] / a[i][i];
            for j in i..n {
                a[k][j] -= factor * a[i][j];
            }
        }
    }
    let mut det = sign;
    for i in 0..n {
        det *= a[i][i];
    }
    det
}

pub fn matrix_determinant(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    StrykeValue::float(determinant(&m))
}

pub fn matrix_cofactor(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let n = m.len();
    if n == 0 || m[0].len() != n {
        return matrix_to_sv(&[]);
    }
    let mut out = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            let minor: Vec<Vec<f64>> = m
                .iter()
                .enumerate()
                .filter(|(r, _)| *r != i)
                .map(|(_, row)| {
                    row.iter()
                        .enumerate()
                        .filter(|(c, _)| *c != j)
                        .map(|(_, v)| *v)
                        .collect()
                })
                .collect();
            let sign = if (i + j) % 2 == 0 { 1.0 } else { -1.0 };
            out[i][j] = sign * determinant(&minor);
        }
    }
    matrix_to_sv(&out)
}

pub fn matrix_adjugate(args: &[StrykeValue]) -> StrykeValue {
    let c = as_matrix(&matrix_cofactor(args));
    let n = c.len();
    if n == 0 {
        return matrix_to_sv(&[]);
    }
    let mut t = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            t[j][i] = c[i][j];
        }
    }
    matrix_to_sv(&t)
}

pub fn matrix_norm_frobenius(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let sum: f64 = m.iter().flat_map(|r| r.iter()).map(|v| v * v).sum();
    StrykeValue::float(sum.sqrt())
}

pub fn matrix_norm_l1(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    if m.is_empty() {
        return StrykeValue::float(0.0);
    }
    let cols = m[0].len();
    let mut max_col = 0.0_f64;
    for j in 0..cols {
        let col_sum: f64 = m.iter().map(|r| r[j].abs()).sum();
        if col_sum > max_col {
            max_col = col_sum;
        }
    }
    StrykeValue::float(max_col)
}

pub fn matrix_norm_linf(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let max: f64 = m
        .iter()
        .map(|r| r.iter().map(|v| v.abs()).sum::<f64>())
        .fold(0.0, f64::max);
    StrykeValue::float(max)
}

pub fn matrix_kronecker(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_matrix).unwrap_or_default();
    let b = args.get(1).map(as_matrix).unwrap_or_default();
    if a.is_empty() || b.is_empty() {
        return matrix_to_sv(&[]);
    }
    let ar = a.len();
    let ac = a[0].len();
    let br = b.len();
    let bc = b[0].len();
    let mut out = vec![vec![0.0; ac * bc]; ar * br];
    for i in 0..ar {
        for j in 0..ac {
            for k in 0..br {
                for l in 0..bc {
                    out[i * br + k][j * bc + l] = a[i][j] * b[k][l];
                }
            }
        }
    }
    matrix_to_sv(&out)
}

pub fn matrix_vec_mul(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let v = args.get(1).map(as_vec_f64).unwrap_or_default();
    arr_f64(
        m.iter()
            .map(|row| row.iter().zip(v.iter()).map(|(a, b)| a * b).sum())
            .collect(),
    )
}

pub fn matrix_outer_product(args: &[StrykeValue]) -> StrykeValue {
    let u = args.first().map(as_vec_f64).unwrap_or_default();
    let v = args.get(1).map(as_vec_f64).unwrap_or_default();
    let out: Vec<Vec<f64>> = u
        .iter()
        .map(|a| v.iter().map(|b| a * b).collect())
        .collect();
    matrix_to_sv(&out)
}

pub fn matrix_concat_h(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_matrix).unwrap_or_default();
    let b = args.get(1).map(as_matrix).unwrap_or_default();
    let n = a.len().min(b.len());
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let mut row = a[i].clone();
        row.extend_from_slice(&b[i]);
        out.push(row);
    }
    matrix_to_sv(&out)
}

pub fn matrix_concat_v(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_matrix).unwrap_or_default();
    let b = args.get(1).map(as_matrix).unwrap_or_default();
    let mut out = a;
    out.extend(b);
    matrix_to_sv(&out)
}

pub fn matrix_reshape(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let flat: Vec<f64> = m.into_iter().flatten().collect();
    let new_r = arg_i64(args, 1).unwrap_or(1).max(1) as usize;
    let new_c = arg_i64(args, 2).unwrap_or(1).max(1) as usize;
    let mut out = vec![vec![0.0; new_c]; new_r];
    for (idx, v) in flat.iter().enumerate() {
        if idx >= new_r * new_c {
            break;
        }
        out[idx / new_c][idx % new_c] = *v;
    }
    matrix_to_sv(&out)
}

pub fn matrix_submatrix(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let r0 = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let c0 = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    let nr = arg_i64(args, 3).unwrap_or(1).max(0) as usize;
    let nc = arg_i64(args, 4).unwrap_or(1).max(0) as usize;
    let out: Vec<Vec<f64>> = m
        .iter()
        .skip(r0)
        .take(nr)
        .map(|row| row.iter().skip(c0).take(nc).cloned().collect())
        .collect();
    matrix_to_sv(&out)
}

pub fn matrix_swap_rows(args: &[StrykeValue]) -> StrykeValue {
    let mut m = args.first().map(as_matrix).unwrap_or_default();
    let i = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let j = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    if i < m.len() && j < m.len() && i != j {
        m.swap(i, j);
    }
    matrix_to_sv(&m)
}

pub fn matrix_swap_cols(args: &[StrykeValue]) -> StrykeValue {
    let mut m = args.first().map(as_matrix).unwrap_or_default();
    let i = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let j = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    for row in &mut m {
        if i < row.len() && j < row.len() && i != j {
            row.swap(i, j);
        }
    }
    matrix_to_sv(&m)
}

pub fn matrix_to_string(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let rendered: Vec<String> = m
        .iter()
        .map(|row| {
            row.iter()
                .map(|v| format!("{v:.4}"))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .collect();
    StrykeValue::string(rendered.join("\n"))
}

pub fn matrix_lu_decompose(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let n = m.len();
    if n == 0 || m[0].len() != n {
        return arr_sv(vec![matrix_to_sv(&[]), matrix_to_sv(&[])]);
    }
    let mut l = vec![vec![0.0; n]; n];
    let mut u = vec![vec![0.0; n]; n];
    for i in 0..n {
        for k in i..n {
            let sum: f64 = (0..i).map(|j| l[i][j] * u[j][k]).sum();
            u[i][k] = m[i][k] - sum;
        }
        l[i][i] = 1.0;
        for k in i + 1..n {
            let sum: f64 = (0..i).map(|j| l[k][j] * u[j][i]).sum();
            if u[i][i].abs() < 1e-12 {
                return arr_sv(vec![matrix_to_sv(&[]), matrix_to_sv(&[])]);
            }
            l[k][i] = (m[k][i] - sum) / u[i][i];
        }
    }
    arr_sv(vec![matrix_to_sv(&l), matrix_to_sv(&u)])
}

pub fn matrix_qr_decompose(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let n = m.len();
    if n == 0 {
        return arr_sv(vec![matrix_to_sv(&[]), matrix_to_sv(&[])]);
    }
    let cols = m[0].len();
    let mut q = vec![vec![0.0; cols]; n];
    let mut r = vec![vec![0.0; cols]; cols];
    let mut a: Vec<Vec<f64>> = (0..cols)
        .map(|j| (0..n).map(|i| m[i][j]).collect())
        .collect();
    for j in 0..cols {
        for i in 0..j {
            let dot: f64 = (0..n).map(|k| a[j][k] * q[k][i]).sum();
            r[i][j] = dot;
            for k in 0..n {
                a[j][k] -= dot * q[k][i];
            }
        }
        let norm: f64 = a[j].iter().map(|v| v * v).sum::<f64>().sqrt();
        r[j][j] = norm;
        if norm > 1e-12 {
            for k in 0..n {
                q[k][j] = a[j][k] / norm;
            }
        }
    }
    arr_sv(vec![matrix_to_sv(&q), matrix_to_sv(&r)])
}

pub fn matrix_cholesky_decompose(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let n = m.len();
    if n == 0 || m[0].len() != n {
        return matrix_to_sv(&[]);
    }
    let mut l = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..=i {
            let sum: f64 = (0..j).map(|k| l[i][k] * l[j][k]).sum();
            if i == j {
                let d = m[i][i] - sum;
                if d < 0.0 {
                    return matrix_to_sv(&[]);
                }
                l[i][j] = d.sqrt();
            } else {
                if l[j][j].abs() < 1e-12 {
                    return matrix_to_sv(&[]);
                }
                l[i][j] = (m[i][j] - sum) / l[j][j];
            }
        }
    }
    matrix_to_sv(&l)
}

// ══════════════════════════════════════════════════════════════════════
// Graph algorithms
// ══════════════════════════════════════════════════════════════════════
// Adjacency representation: array of arrays of [neighbor, weight] pairs.
// adj[i] = [[j, w_ij], ...]

fn adj_from_arg(v: &StrykeValue) -> Vec<Vec<(usize, f64)>> {
    let rows = as_vec_sv(v);
    rows.iter()
        .map(|row| {
            let edges = as_vec_sv(row);
            edges
                .iter()
                .map(|e| {
                    let pair = as_vec_sv(e);
                    let n = pair.first().map(|x| x.to_int().max(0) as usize).unwrap_or(0);
                    let w = pair.get(1).map(|x| x.to_number()).unwrap_or(1.0);
                    (n, w)
                })
                .collect()
        })
        .collect()
}

fn adj_unweighted(v: &StrykeValue) -> Vec<Vec<usize>> {
    let rows = as_vec_sv(v);
    rows.iter()
        .map(|row| {
            let edges = as_vec_sv(row);
            edges
                .iter()
                .map(|e| {
                    if let Some(pair) = e.as_array_ref() {
                        pair.read().first().map(|x| x.to_int().max(0) as usize).unwrap_or(0)
                    } else if let Some(pair) = e.as_array_vec() {
                        pair.first().map(|x| x.to_int().max(0) as usize).unwrap_or(0)
                    } else {
                        e.to_int().max(0) as usize
                    }
                })
                .collect()
        })
        .collect()
}

pub fn graph_from_edges(args: &[StrykeValue]) -> StrykeValue {
    let edges = args.first().map(as_vec_sv).unwrap_or_default();
    let directed = arg_i64(args, 1).unwrap_or(0) != 0;
    let mut max_node = 0usize;
    let parsed: Vec<(usize, usize, f64)> = edges
        .iter()
        .map(|e| {
            let pair = as_vec_sv(e);
            let a = pair.first().map(|x| x.to_int().max(0) as usize).unwrap_or(0);
            let b = pair.get(1).map(|x| x.to_int().max(0) as usize).unwrap_or(0);
            let w = pair.get(2).map(|x| x.to_number()).unwrap_or(1.0);
            max_node = max_node.max(a).max(b);
            (a, b, w)
        })
        .collect();
    let n = max_node + 1;
    let mut adj: Vec<Vec<StrykeValue>> = vec![Vec::new(); n];
    for (a, b, w) in parsed {
        adj[a].push(arr_sv(vec![StrykeValue::integer(b as i64), StrykeValue::float(w)]));
        if !directed {
            adj[b].push(arr_sv(vec![StrykeValue::integer(a as i64), StrykeValue::float(w)]));
        }
    }
    arr_sv(adj.into_iter().map(arr_sv).collect())
}

pub fn graph_to_adj_matrix(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_from_arg).unwrap_or_default();
    let n = g.len();
    let mut m = vec![vec![0.0; n]; n];
    for (i, edges) in g.iter().enumerate() {
        for (j, w) in edges {
            if *j < n {
                m[i][*j] = *w;
            }
        }
    }
    matrix_to_sv(&m)
}

pub fn graph_to_adj_list(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let n = m.len();
    let mut adj: Vec<Vec<StrykeValue>> = vec![Vec::new(); n];
    for i in 0..n {
        for j in 0..m[i].len() {
            if m[i][j] != 0.0 {
                adj[i].push(arr_sv(vec![
                    StrykeValue::integer(j as i64),
                    StrykeValue::float(m[i][j]),
                ]));
            }
        }
    }
    arr_sv(adj.into_iter().map(arr_sv).collect())
}

pub fn graph_bfs(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let start = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let n = g.len();
    let mut visited = vec![false; n];
    let mut order = Vec::new();
    let mut q = VecDeque::new();
    if start < n {
        q.push_back(start);
        visited[start] = true;
    }
    while let Some(u) = q.pop_front() {
        order.push(u);
        for &v in &g[u] {
            if v < n && !visited[v] {
                visited[v] = true;
                q.push_back(v);
            }
        }
    }
    arr_sv(order.into_iter().map(|x| StrykeValue::integer(x as i64)).collect())
}

pub fn graph_dfs(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let start = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let n = g.len();
    let mut visited = vec![false; n];
    let mut order = Vec::new();
    let mut stack = vec![start];
    if start >= n {
        return arr_sv(vec![]);
    }
    while let Some(u) = stack.pop() {
        if u >= n || visited[u] {
            continue;
        }
        visited[u] = true;
        order.push(u);
        for &v in g[u].iter().rev() {
            if v < n && !visited[v] {
                stack.push(v);
            }
        }
    }
    arr_sv(order.into_iter().map(|x| StrykeValue::integer(x as i64)).collect())
}

#[derive(PartialEq)]
struct DjkNode(f64, usize);

impl Eq for DjkNode {}

impl Ord for DjkNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for DjkNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn graph_dijkstra(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_from_arg).unwrap_or_default();
    let start = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let n = g.len();
    let mut dist = vec![f64::INFINITY; n];
    if start < n {
        dist[start] = 0.0;
    }
    let mut heap = BinaryHeap::new();
    heap.push(DjkNode(0.0, start));
    while let Some(DjkNode(d, u)) = heap.pop() {
        if u >= n || d > dist[u] {
            continue;
        }
        for &(v, w) in &g[u] {
            if v < n {
                let nd = d + w;
                if nd < dist[v] {
                    dist[v] = nd;
                    heap.push(DjkNode(nd, v));
                }
            }
        }
    }
    arr_f64(dist)
}

pub fn graph_bellman_ford(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_from_arg).unwrap_or_default();
    let start = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let n = g.len();
    let mut dist = vec![f64::INFINITY; n];
    if start < n {
        dist[start] = 0.0;
    }
    for _ in 0..n.saturating_sub(1) {
        let mut changed = false;
        for u in 0..n {
            for &(v, w) in &g[u] {
                if v < n && dist[u] + w < dist[v] {
                    dist[v] = dist[u] + w;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    arr_f64(dist)
}

pub fn graph_floyd_warshall(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let n = m.len();
    let mut d = vec![vec![f64::INFINITY; n]; n];
    for i in 0..n {
        for j in 0..n {
            if i == j {
                d[i][j] = 0.0;
            } else if m[i].get(j).is_some_and(|&v| v != 0.0) {
                d[i][j] = m[i][j];
            }
        }
    }
    for k in 0..n {
        for i in 0..n {
            for j in 0..n {
                if d[i][k] + d[k][j] < d[i][j] {
                    d[i][j] = d[i][k] + d[k][j];
                }
            }
        }
    }
    matrix_to_sv(&d)
}

pub fn graph_kruskal_mst(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_from_arg).unwrap_or_default();
    let n = g.len();
    let mut edges: Vec<(f64, usize, usize)> = Vec::new();
    for (u, es) in g.iter().enumerate() {
        for &(v, w) in es {
            if u < v {
                edges.push((w, u, v));
            }
        }
    }
    edges.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        if p[x] != x {
            let r = find(p, p[x]);
            p[x] = r;
        }
        p[x]
    }
    let mut mst = Vec::new();
    for (w, u, v) in edges {
        let pu = find(&mut parent, u);
        let pv = find(&mut parent, v);
        if pu != pv {
            parent[pu] = pv;
            mst.push(arr_sv(vec![
                StrykeValue::integer(u as i64),
                StrykeValue::integer(v as i64),
                StrykeValue::float(w),
            ]));
        }
    }
    arr_sv(mst)
}

pub fn graph_prim_mst(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_from_arg).unwrap_or_default();
    let n = g.len();
    if n == 0 {
        return arr_sv(vec![]);
    }
    let mut in_mst = vec![false; n];
    let mut mst = Vec::new();
    let mut heap: BinaryHeap<(std::cmp::Reverse<i64>, usize, usize)> = BinaryHeap::new();
    in_mst[0] = true;
    for &(v, w) in &g[0] {
        heap.push((std::cmp::Reverse((w * 1e6) as i64), 0, v));
    }
    while let Some((std::cmp::Reverse(wi), u, v)) = heap.pop() {
        if v >= n || in_mst[v] {
            continue;
        }
        in_mst[v] = true;
        let w = wi as f64 / 1e6;
        mst.push(arr_sv(vec![
            StrykeValue::integer(u as i64),
            StrykeValue::integer(v as i64),
            StrykeValue::float(w),
        ]));
        for &(nv, nw) in &g[v] {
            if !in_mst[nv] {
                heap.push((std::cmp::Reverse((nw * 1e6) as i64), v, nv));
            }
        }
    }
    arr_sv(mst)
}

pub fn graph_topological_sort(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let n = g.len();
    let mut in_deg = vec![0usize; n];
    for edges in &g {
        for &v in edges {
            if v < n {
                in_deg[v] += 1;
            }
        }
    }
    let mut q: VecDeque<usize> = in_deg.iter().enumerate().filter(|(_, d)| **d == 0).map(|(i, _)| i).collect();
    let mut order = Vec::new();
    while let Some(u) = q.pop_front() {
        order.push(u);
        for &v in &g[u] {
            if v < n {
                in_deg[v] -= 1;
                if in_deg[v] == 0 {
                    q.push_back(v);
                }
            }
        }
    }
    if order.len() != n {
        return arr_sv(vec![]);
    }
    arr_sv(order.into_iter().map(|x| StrykeValue::integer(x as i64)).collect())
}

pub fn graph_connected_components(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let n = g.len();
    let mut comp = vec![usize::MAX; n];
    let mut k = 0;
    for start in 0..n {
        if comp[start] != usize::MAX {
            continue;
        }
        let mut q = VecDeque::new();
        q.push_back(start);
        comp[start] = k;
        while let Some(u) = q.pop_front() {
            for &v in &g[u] {
                if v < n && comp[v] == usize::MAX {
                    comp[v] = k;
                    q.push_back(v);
                }
            }
        }
        k += 1;
    }
    arr_sv(comp.into_iter().map(|c| StrykeValue::integer(c as i64)).collect())
}

pub fn graph_strongly_connected_components(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let n = g.len();
    let mut order = Vec::new();
    let mut visited = vec![false; n];
    fn dfs1(u: usize, g: &[Vec<usize>], visited: &mut [bool], order: &mut Vec<usize>) {
        visited[u] = true;
        for &v in &g[u] {
            if v < g.len() && !visited[v] {
                dfs1(v, g, visited, order);
            }
        }
        order.push(u);
    }
    for i in 0..n {
        if !visited[i] {
            dfs1(i, &g, &mut visited, &mut order);
        }
    }
    let mut rev = vec![Vec::new(); n];
    for u in 0..n {
        for &v in &g[u] {
            if v < n {
                rev[v].push(u);
            }
        }
    }
    let mut comp = vec![usize::MAX; n];
    let mut k = 0;
    fn dfs2(u: usize, rev: &[Vec<usize>], comp: &mut [usize], k: usize) {
        comp[u] = k;
        for &v in &rev[u] {
            if comp[v] == usize::MAX {
                dfs2(v, rev, comp, k);
            }
        }
    }
    for &u in order.iter().rev() {
        if comp[u] == usize::MAX {
            dfs2(u, &rev, &mut comp, k);
            k += 1;
        }
    }
    arr_sv(comp.into_iter().map(|c| StrykeValue::integer(c as i64)).collect())
}

pub fn graph_cycle_detect(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let n = g.len();
    let mut state = vec![0u8; n]; // 0=white, 1=gray, 2=black
    fn dfs(u: usize, g: &[Vec<usize>], state: &mut [u8]) -> bool {
        state[u] = 1;
        for &v in &g[u] {
            if v < g.len() {
                if state[v] == 1 {
                    return true;
                }
                if state[v] == 0 && dfs(v, g, state) {
                    return true;
                }
            }
        }
        state[u] = 2;
        false
    }
    for i in 0..n {
        if state[i] == 0 && dfs(i, &g, &mut state) {
            return StrykeValue::integer(1);
        }
    }
    StrykeValue::integer(0)
}

pub fn graph_has_path(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let s = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let t = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    let n = g.len();
    if s >= n || t >= n {
        return StrykeValue::integer(0);
    }
    let mut visited = vec![false; n];
    let mut q = VecDeque::new();
    q.push_back(s);
    visited[s] = true;
    while let Some(u) = q.pop_front() {
        if u == t {
            return StrykeValue::integer(1);
        }
        for &v in &g[u] {
            if v < n && !visited[v] {
                visited[v] = true;
                q.push_back(v);
            }
        }
    }
    StrykeValue::integer(0)
}

pub fn graph_shortest_path(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let s = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let t = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    let n = g.len();
    if s >= n || t >= n {
        return arr_sv(vec![]);
    }
    let mut parent = vec![usize::MAX; n];
    let mut visited = vec![false; n];
    let mut q = VecDeque::new();
    q.push_back(s);
    visited[s] = true;
    while let Some(u) = q.pop_front() {
        if u == t {
            break;
        }
        for &v in &g[u] {
            if v < n && !visited[v] {
                visited[v] = true;
                parent[v] = u;
                q.push_back(v);
            }
        }
    }
    if !visited[t] {
        return arr_sv(vec![]);
    }
    let mut path = Vec::new();
    let mut cur = t;
    while cur != usize::MAX {
        path.push(cur);
        if cur == s {
            break;
        }
        cur = parent[cur];
    }
    path.reverse();
    arr_sv(path.into_iter().map(|x| StrykeValue::integer(x as i64)).collect())
}

pub fn graph_eccentricity(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_from_arg).unwrap_or_default();
    let v = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let n = g.len();
    if v >= n {
        return StrykeValue::float(0.0);
    }
    let mut dist = vec![f64::INFINITY; n];
    dist[v] = 0.0;
    let mut heap = BinaryHeap::new();
    heap.push(DjkNode(0.0, v));
    while let Some(DjkNode(d, u)) = heap.pop() {
        if u >= n || d > dist[u] {
            continue;
        }
        for &(nv, w) in &g[u] {
            if nv < n {
                let nd = d + w;
                if nd < dist[nv] {
                    dist[nv] = nd;
                    heap.push(DjkNode(nd, nv));
                }
            }
        }
    }
    let ecc = dist.iter().cloned().filter(|d| d.is_finite()).fold(0.0_f64, f64::max);
    StrykeValue::float(ecc)
}

pub fn graph_clustering_coefficient(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let v = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let n = g.len();
    if v >= n {
        return StrykeValue::float(0.0);
    }
    let neighbors: HashSet<usize> = g[v].iter().cloned().collect();
    let k = neighbors.len();
    if k < 2 {
        return StrykeValue::float(0.0);
    }
    let mut edges = 0usize;
    for &u in &neighbors {
        if u < n {
            for &w in &g[u] {
                if neighbors.contains(&w) {
                    edges += 1;
                }
            }
        }
    }
    StrykeValue::float(edges as f64 / (k * (k - 1)) as f64)
}

pub fn graph_degree(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let v = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    StrykeValue::integer(g.get(v).map_or(0, |n| n.len()) as i64)
}

pub fn graph_in_degree(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let v = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let mut count = 0i64;
    for edges in &g {
        if edges.contains(&v) {
            count += 1;
        }
    }
    StrykeValue::integer(count)
}

pub fn graph_out_degree(args: &[StrykeValue]) -> StrykeValue {
    graph_degree(args)
}

pub fn graph_pagerank(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let damp = arg_f64(args, 1).unwrap_or(0.85);
    let iters = arg_i64(args, 2).unwrap_or(50).max(1) as usize;
    let n = g.len();
    if n == 0 {
        return arr_f64(vec![]);
    }
    let mut pr = vec![1.0 / n as f64; n];
    for _ in 0..iters {
        let mut next = vec![(1.0 - damp) / n as f64; n];
        for u in 0..n {
            let out = g[u].len();
            if out == 0 {
                for v in 0..n {
                    next[v] += damp * pr[u] / n as f64;
                }
            } else {
                for &v in &g[u] {
                    if v < n {
                        next[v] += damp * pr[u] / out as f64;
                    }
                }
            }
        }
        pr = next;
    }
    arr_f64(pr)
}

pub fn graph_betweenness(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let n = g.len();
    let mut bc = vec![0.0; n];
    for s in 0..n {
        let mut stack = Vec::new();
        let mut pred: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut sigma = vec![0.0; n];
        sigma[s] = 1.0;
        let mut d = vec![-1i64; n];
        d[s] = 0;
        let mut q = VecDeque::new();
        q.push_back(s);
        while let Some(v) = q.pop_front() {
            stack.push(v);
            for &w in &g[v] {
                if w < n {
                    if d[w] < 0 {
                        d[w] = d[v] + 1;
                        q.push_back(w);
                    }
                    if d[w] == d[v] + 1 {
                        sigma[w] += sigma[v];
                        pred[w].push(v);
                    }
                }
            }
        }
        let mut delta = vec![0.0; n];
        while let Some(w) = stack.pop() {
            for &v in &pred[w] {
                if sigma[w] > 0.0 {
                    delta[v] += (sigma[v] / sigma[w]) * (1.0 + delta[w]);
                }
            }
            if w != s {
                bc[w] += delta[w];
            }
        }
    }
    arr_f64(bc)
}

pub fn graph_closeness(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let v = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let n = g.len();
    if v >= n {
        return StrykeValue::float(0.0);
    }
    let mut d = vec![-1i64; n];
    d[v] = 0;
    let mut q = VecDeque::new();
    q.push_back(v);
    while let Some(u) = q.pop_front() {
        for &w in &g[u] {
            if w < n && d[w] < 0 {
                d[w] = d[u] + 1;
                q.push_back(w);
            }
        }
    }
    let sum: i64 = d.iter().filter(|x| **x > 0).sum();
    if sum == 0 {
        return StrykeValue::float(0.0);
    }
    StrykeValue::float((n as f64 - 1.0) / sum as f64)
}

pub fn graph_eigenvector_centrality(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let n = g.len();
    if n == 0 {
        return arr_f64(vec![]);
    }
    let mut x = vec![1.0 / (n as f64).sqrt(); n];
    for _ in 0..100 {
        let mut next = vec![0.0; n];
        for u in 0..n {
            for &v in &g[u] {
                if v < n {
                    next[v] += x[u];
                }
            }
        }
        let norm = next.iter().map(|y| y * y).sum::<f64>().sqrt();
        if norm < 1e-12 {
            break;
        }
        for v in &mut next {
            *v /= norm;
        }
        x = next;
    }
    arr_f64(x)
}

pub fn graph_kosaraju(args: &[StrykeValue]) -> StrykeValue {
    graph_strongly_connected_components(args)
}

pub fn graph_tarjan(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let n = g.len();
    let mut idx = vec![-1i64; n];
    let mut low = vec![0i64; n];
    let mut on_stack = vec![false; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut counter = 0i64;
    let mut comp = vec![usize::MAX; n];
    let mut k = 0;
    #[allow(clippy::too_many_arguments)]
    fn strongconnect(
        u: usize,
        g: &[Vec<usize>],
        idx: &mut [i64],
        low: &mut [i64],
        on_stack: &mut [bool],
        stack: &mut Vec<usize>,
        counter: &mut i64,
        comp: &mut [usize],
        k: &mut usize,
    ) {
        idx[u] = *counter;
        low[u] = *counter;
        *counter += 1;
        stack.push(u);
        on_stack[u] = true;
        for &v in &g[u] {
            if v < g.len() {
                if idx[v] == -1 {
                    strongconnect(v, g, idx, low, on_stack, stack, counter, comp, k);
                    low[u] = low[u].min(low[v]);
                } else if on_stack[v] {
                    low[u] = low[u].min(idx[v]);
                }
            }
        }
        if low[u] == idx[u] {
            loop {
                let w = stack.pop().unwrap();
                on_stack[w] = false;
                comp[w] = *k;
                if w == u {
                    break;
                }
            }
            *k += 1;
        }
    }
    for u in 0..n {
        if idx[u] == -1 {
            strongconnect(u, &g, &mut idx, &mut low, &mut on_stack, &mut stack, &mut counter, &mut comp, &mut k);
        }
    }
    arr_sv(comp.into_iter().map(|c| StrykeValue::integer(c as i64)).collect())
}

pub fn graph_articulation_points(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let n = g.len();
    let mut visited = vec![false; n];
    let mut disc = vec![0i64; n];
    let mut low = vec![0i64; n];
    let mut parent = vec![-1i64; n];
    let mut ap = vec![false; n];
    let mut timer = 0i64;
    #[allow(clippy::too_many_arguments)]
    fn dfs(
        u: usize,
        g: &[Vec<usize>],
        visited: &mut [bool],
        disc: &mut [i64],
        low: &mut [i64],
        parent: &mut [i64],
        ap: &mut [bool],
        timer: &mut i64,
    ) {
        visited[u] = true;
        *timer += 1;
        disc[u] = *timer;
        low[u] = *timer;
        let mut children = 0;
        for &v in &g[u] {
            if v >= g.len() {
                continue;
            }
            if !visited[v] {
                children += 1;
                parent[v] = u as i64;
                dfs(v, g, visited, disc, low, parent, ap, timer);
                low[u] = low[u].min(low[v]);
                if parent[u] == -1 && children > 1 {
                    ap[u] = true;
                }
                if parent[u] != -1 && low[v] >= disc[u] {
                    ap[u] = true;
                }
            } else if v as i64 != parent[u] {
                low[u] = low[u].min(disc[v]);
            }
        }
    }
    for u in 0..n {
        if !visited[u] {
            dfs(u, &g, &mut visited, &mut disc, &mut low, &mut parent, &mut ap, &mut timer);
        }
    }
    let pts: Vec<StrykeValue> = ap
        .iter()
        .enumerate()
        .filter(|(_, &b)| b)
        .map(|(i, _)| StrykeValue::integer(i as i64))
        .collect();
    arr_sv(pts)
}

pub fn graph_bridges(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let n = g.len();
    let mut visited = vec![false; n];
    let mut disc = vec![0i64; n];
    let mut low = vec![0i64; n];
    let mut bridges: Vec<(usize, usize)> = Vec::new();
    let mut timer = 0i64;
    #[allow(clippy::too_many_arguments)]
    fn dfs(
        u: usize,
        parent: i64,
        g: &[Vec<usize>],
        visited: &mut [bool],
        disc: &mut [i64],
        low: &mut [i64],
        bridges: &mut Vec<(usize, usize)>,
        timer: &mut i64,
    ) {
        visited[u] = true;
        *timer += 1;
        disc[u] = *timer;
        low[u] = *timer;
        for &v in &g[u] {
            if v >= g.len() {
                continue;
            }
            if !visited[v] {
                dfs(v, u as i64, g, visited, disc, low, bridges, timer);
                low[u] = low[u].min(low[v]);
                if low[v] > disc[u] {
                    bridges.push((u, v));
                }
            } else if v as i64 != parent {
                low[u] = low[u].min(disc[v]);
            }
        }
    }
    for u in 0..n {
        if !visited[u] {
            dfs(u, -1, &g, &mut visited, &mut disc, &mut low, &mut bridges, &mut timer);
        }
    }
    let result: Vec<StrykeValue> = bridges
        .into_iter()
        .map(|(u, v)| arr_sv(vec![StrykeValue::integer(u as i64), StrykeValue::integer(v as i64)]))
        .collect();
    arr_sv(result)
}

pub fn graph_is_connected(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let n = g.len();
    if n == 0 {
        return StrykeValue::integer(1);
    }
    let mut visited = vec![false; n];
    let mut q = VecDeque::new();
    q.push_back(0);
    visited[0] = true;
    let mut count = 1;
    while let Some(u) = q.pop_front() {
        for &v in &g[u] {
            if v < n && !visited[v] {
                visited[v] = true;
                count += 1;
                q.push_back(v);
            }
        }
    }
    StrykeValue::integer(if count == n { 1 } else { 0 })
}

pub fn graph_is_bipartite(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let n = g.len();
    let mut color = vec![-1i8; n];
    for start in 0..n {
        if color[start] != -1 {
            continue;
        }
        color[start] = 0;
        let mut q = VecDeque::new();
        q.push_back(start);
        while let Some(u) = q.pop_front() {
            for &v in &g[u] {
                if v >= n {
                    continue;
                }
                if color[v] == -1 {
                    color[v] = 1 - color[u];
                    q.push_back(v);
                } else if color[v] == color[u] {
                    return StrykeValue::integer(0);
                }
            }
        }
    }
    StrykeValue::integer(1)
}

pub fn graph_color_greedy(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let n = g.len();
    let mut color = vec![usize::MAX; n];
    for u in 0..n {
        let mut used = HashSet::new();
        for &v in &g[u] {
            if v < n && color[v] != usize::MAX {
                used.insert(color[v]);
            }
        }
        let mut c = 0;
        while used.contains(&c) {
            c += 1;
        }
        color[u] = c;
    }
    arr_sv(color.into_iter().map(|c| StrykeValue::integer(c as i64)).collect())
}

// ══════════════════════════════════════════════════════════════════════
// Calendar / date helpers (chrono)
// ══════════════════════════════════════════════════════════════════════

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc, Weekday};

fn parse_date_unix(args: &[StrykeValue], idx: usize) -> Option<DateTime<Utc>> {
    let v = args.get(idx)?;
    let n = v.to_int();
    Utc.timestamp_opt(n, 0).single()
}

fn parse_date_arg(args: &[StrykeValue], idx: usize) -> Option<DateTime<Utc>> {
    if let Some(s) = args.get(idx).map(|v| v.as_str_or_empty()) {
        if let Ok(d) = DateTime::parse_from_rfc3339(&s) {
            return Some(d.with_timezone(&Utc));
        }
        if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
            return Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0)?).into();
        }
    }
    parse_date_unix(args, idx)
}

pub fn date_year(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| StrykeValue::integer(d.year() as i64))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_month(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| StrykeValue::integer(d.month() as i64))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_day(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| StrykeValue::integer(d.day() as i64))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_hour(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| StrykeValue::integer(d.hour() as i64))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_minute(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| StrykeValue::integer(d.minute() as i64))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_second(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| StrykeValue::integer(d.second() as i64))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_dayofweek(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| StrykeValue::integer(d.weekday().num_days_from_sunday() as i64))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_dayofyear(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| StrykeValue::integer(d.ordinal() as i64))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_weekofyear(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| StrykeValue::integer(d.iso_week().week() as i64))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_quarter(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| StrykeValue::integer((d.month() as i64 - 1) / 3 + 1))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_is_leap(args: &[StrykeValue]) -> StrykeValue {
    let y = arg_i64(args, 0).unwrap_or(0);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    StrykeValue::integer(if leap { 1 } else { 0 })
}

pub fn date_days_in_month(args: &[StrykeValue]) -> StrykeValue {
    let y = arg_i64(args, 0).unwrap_or(2000) as i32;
    let m = arg_i64(args, 1).unwrap_or(1) as u32;
    let next_m = if m == 12 { 1 } else { m + 1 };
    let next_y = if m == 12 { y + 1 } else { y };
    let d0 = NaiveDate::from_ymd_opt(y, m, 1);
    let d1 = NaiveDate::from_ymd_opt(next_y, next_m, 1);
    match (d0, d1) {
        (Some(a), Some(b)) => StrykeValue::integer((b - a).num_days()),
        _ => StrykeValue::UNDEF,
    }
}

pub fn date_business_days_between(args: &[StrykeValue]) -> StrykeValue {
    let a = parse_date_arg(args, 0);
    let b = parse_date_arg(args, 1);
    match (a, b) {
        (Some(d1), Some(d2)) => {
            let (s, e) = if d1 < d2 { (d1, d2) } else { (d2, d1) };
            let mut count = 0i64;
            let mut cur = s.date_naive();
            let end = e.date_naive();
            while cur < end {
                let wd = cur.weekday();
                if wd != Weekday::Sat && wd != Weekday::Sun {
                    count += 1;
                }
                cur += Duration::days(1);
            }
            StrykeValue::integer(count)
        }
        _ => StrykeValue::UNDEF,
    }
}

pub fn date_add_days(args: &[StrykeValue]) -> StrykeValue {
    let d = parse_date_arg(args, 0);
    let n = arg_i64(args, 1).unwrap_or(0);
    d.and_then(|x| x.checked_add_signed(Duration::days(n)))
        .map(|x| StrykeValue::integer(x.timestamp()))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_add_months(args: &[StrykeValue]) -> StrykeValue {
    let d = parse_date_arg(args, 0);
    let n = arg_i64(args, 1).unwrap_or(0);
    d.and_then(|x| {
        let total = x.year() as i64 * 12 + x.month() as i64 - 1 + n;
        let ny = (total.div_euclid(12)) as i32;
        let nm = (total.rem_euclid(12)) as u32 + 1;
        NaiveDate::from_ymd_opt(ny, nm, x.day().min(28))
            .and_then(|d| d.and_hms_opt(x.hour(), x.minute(), x.second()))
            .map(|nd| Utc.from_utc_datetime(&nd))
    })
    .map(|x| StrykeValue::integer(x.timestamp()))
    .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_add_years(args: &[StrykeValue]) -> StrykeValue {
    let d = parse_date_arg(args, 0);
    let n = arg_i64(args, 1).unwrap_or(0) as i32;
    d.and_then(|x| {
        NaiveDate::from_ymd_opt(x.year() + n, x.month(), x.day().min(28))
            .and_then(|d| d.and_hms_opt(x.hour(), x.minute(), x.second()))
            .map(|nd| Utc.from_utc_datetime(&nd))
    })
    .map(|x| StrykeValue::integer(x.timestamp()))
    .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_diff_days(args: &[StrykeValue]) -> StrykeValue {
    match (parse_date_arg(args, 0), parse_date_arg(args, 1)) {
        (Some(a), Some(b)) => StrykeValue::integer((b - a).num_days()),
        _ => StrykeValue::UNDEF,
    }
}

pub fn date_diff_hours(args: &[StrykeValue]) -> StrykeValue {
    match (parse_date_arg(args, 0), parse_date_arg(args, 1)) {
        (Some(a), Some(b)) => StrykeValue::integer((b - a).num_hours()),
        _ => StrykeValue::UNDEF,
    }
}

pub fn date_diff_minutes(args: &[StrykeValue]) -> StrykeValue {
    match (parse_date_arg(args, 0), parse_date_arg(args, 1)) {
        (Some(a), Some(b)) => StrykeValue::integer((b - a).num_minutes()),
        _ => StrykeValue::UNDEF,
    }
}

pub fn date_diff_seconds(args: &[StrykeValue]) -> StrykeValue {
    match (parse_date_arg(args, 0), parse_date_arg(args, 1)) {
        (Some(a), Some(b)) => StrykeValue::integer((b - a).num_seconds()),
        _ => StrykeValue::UNDEF,
    }
}

pub fn date_easter(args: &[StrykeValue]) -> StrykeValue {
    let year = arg_i64(args, 0).unwrap_or(2025) as i32;
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    StrykeValue::string(format!("{year:04}-{month:02}-{day:02}"))
}

pub fn date_is_weekend(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| {
            let w = d.weekday();
            StrykeValue::integer(if w == Weekday::Sat || w == Weekday::Sun { 1 } else { 0 })
        })
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_first_of_month(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .and_then(|d| {
            NaiveDate::from_ymd_opt(d.year(), d.month(), 1)
                .and_then(|n| n.and_hms_opt(0, 0, 0))
                .map(|n| Utc.from_utc_datetime(&n))
        })
        .map(|x| StrykeValue::integer(x.timestamp()))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_last_of_month(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .and_then(|d| {
            let m = d.month();
            let next_m = if m == 12 { 1 } else { m + 1 };
            let next_y = if m == 12 { d.year() + 1 } else { d.year() };
            let next = NaiveDate::from_ymd_opt(next_y, next_m, 1)?;
            (next - Duration::days(1))
                .and_hms_opt(0, 0, 0)
                .map(|n| Utc.from_utc_datetime(&n))
        })
        .map(|x| StrykeValue::integer(x.timestamp()))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_iso_week(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| {
            let w = d.iso_week();
            StrykeValue::string(format!("{}-W{:02}", w.year(), w.week()))
        })
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_iso_format(args: &[StrykeValue]) -> StrykeValue {
    parse_date_arg(args, 0)
        .map(|d| StrykeValue::string(d.to_rfc3339()))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_unix_to_str(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0);
    let fmt = arg_str(args, 1).unwrap_or_else(|| "%Y-%m-%d %H:%M:%S".to_string());
    Utc.timestamp_opt(n, 0)
        .single()
        .map(|d| StrykeValue::string(d.format(&fmt).to_string()))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn date_str_to_unix(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let fmt = arg_str(args, 1).unwrap_or_else(|| "%Y-%m-%d %H:%M:%S".to_string());
    if let Ok(d) = DateTime::parse_from_rfc3339(&s) {
        return StrykeValue::integer(d.timestamp());
    }
    if let Ok(d) = chrono::NaiveDateTime::parse_from_str(&s, &fmt) {
        return StrykeValue::integer(d.and_utc().timestamp());
    }
    if let Ok(d) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
        if let Some(dt) = d.and_hms_opt(0, 0, 0) {
            return StrykeValue::integer(dt.and_utc().timestamp());
        }
    }
    StrykeValue::UNDEF
}

pub fn sun_rise_unix(args: &[StrykeValue]) -> StrykeValue {
    let lat = arg_f64(args, 0).unwrap_or(0.0);
    let _lon = arg_f64(args, 1).unwrap_or(0.0);
    let ts = arg_i64(args, 2).unwrap_or(0);
    let d = Utc.timestamp_opt(ts, 0).single();
    if let Some(d) = d {
        let doy = d.ordinal() as f64;
        let lat_r = lat.to_radians();
        let decl = (23.44_f64).to_radians() * ((360.0 / 365.0) * (doy - 81.0)).to_radians().sin();
        let cos_ha = (-lat_r.tan() * decl.tan()).clamp(-1.0, 1.0);
        let ha = cos_ha.acos().to_degrees() / 15.0;
        let solar_noon_utc = 12.0 - args.get(1).map(|v| v.to_number()).unwrap_or(0.0) / 15.0;
        let rise_h = solar_noon_utc - ha;
        let hour = rise_h.floor() as i64;
        let min = ((rise_h - hour as f64) * 60.0) as i64;
        let nd = NaiveDate::from_ymd_opt(d.year(), d.month(), d.day())
            .and_then(|nd| nd.and_hms_opt(hour.rem_euclid(24) as u32, min.rem_euclid(60) as u32, 0));
        if let Some(nd) = nd {
            return StrykeValue::integer(Utc.from_utc_datetime(&nd).timestamp());
        }
    }
    StrykeValue::UNDEF
}

pub fn sun_set_unix(args: &[StrykeValue]) -> StrykeValue {
    let lat = arg_f64(args, 0).unwrap_or(0.0);
    let _lon = arg_f64(args, 1).unwrap_or(0.0);
    let ts = arg_i64(args, 2).unwrap_or(0);
    let d = Utc.timestamp_opt(ts, 0).single();
    if let Some(d) = d {
        let doy = d.ordinal() as f64;
        let lat_r = lat.to_radians();
        let decl = (23.44_f64).to_radians() * ((360.0 / 365.0) * (doy - 81.0)).to_radians().sin();
        let cos_ha = (-lat_r.tan() * decl.tan()).clamp(-1.0, 1.0);
        let ha = cos_ha.acos().to_degrees() / 15.0;
        let solar_noon_utc = 12.0 - args.get(1).map(|v| v.to_number()).unwrap_or(0.0) / 15.0;
        let set_h = solar_noon_utc + ha;
        let hour = set_h.floor() as i64;
        let min = ((set_h - hour as f64) * 60.0) as i64;
        let nd = NaiveDate::from_ymd_opt(d.year(), d.month(), d.day())
            .and_then(|nd| nd.and_hms_opt(hour.rem_euclid(24) as u32, min.rem_euclid(60) as u32, 0));
        if let Some(nd) = nd {
            return StrykeValue::integer(Utc.from_utc_datetime(&nd).timestamp());
        }
    }
    StrykeValue::UNDEF
}

pub fn zodiac_sign(args: &[StrykeValue]) -> StrykeValue {
    let m = arg_i64(args, 0).unwrap_or(1);
    let d = arg_i64(args, 1).unwrap_or(1);
    let sign = match (m, d) {
        (3, 21..=31) | (4, 1..=19) => "Aries",
        (4, 20..=30) | (5, 1..=20) => "Taurus",
        (5, 21..=31) | (6, 1..=20) => "Gemini",
        (6, 21..=30) | (7, 1..=22) => "Cancer",
        (7, 23..=31) | (8, 1..=22) => "Leo",
        (8, 23..=31) | (9, 1..=22) => "Virgo",
        (9, 23..=30) | (10, 1..=22) => "Libra",
        (10, 23..=31) | (11, 1..=21) => "Scorpio",
        (11, 22..=30) | (12, 1..=21) => "Sagittarius",
        (12, 22..=31) | (1, 1..=19) => "Capricorn",
        (1, 20..=31) | (2, 1..=18) => "Aquarius",
        (2, 19..=29) | (3, 1..=20) => "Pisces",
        _ => "",
    };
    StrykeValue::string(sign.to_string())
}

// ══════════════════════════════════════════════════════════════════════
// Special math
// ══════════════════════════════════════════════════════════════════════

fn lgamma(x: f64) -> f64 {
    libm::lgamma(x)
}

fn gamma(x: f64) -> f64 {
    libm::tgamma(x)
}

pub fn beta_function(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_f64(args, 0).unwrap_or(1.0);
    let b = arg_f64(args, 1).unwrap_or(1.0);
    StrykeValue::float(gamma(a) * gamma(b) / gamma(a + b))
}

pub fn beta_incomplete(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.5).clamp(0.0, 1.0);
    let a = arg_f64(args, 1).unwrap_or(1.0);
    let b = arg_f64(args, 2).unwrap_or(1.0);
    if x == 0.0 {
        return StrykeValue::float(0.0);
    }
    if x == 1.0 {
        return StrykeValue::float(1.0);
    }
    // Continued-fraction expansion (Lentz).
    let bt = (lgamma(a + b) - lgamma(a) - lgamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    let cf = |x: f64, a: f64, b: f64| {
        let max_it = 200;
        let eps = 3e-12;
        let qab = a + b;
        let qap = a + 1.0;
        let qam = a - 1.0;
        let mut c = 1.0;
        let mut d = 1.0 - qab * x / qap;
        if d.abs() < 1e-30 {
            d = 1e-30;
        }
        d = 1.0 / d;
        let mut h = d;
        for m in 1..=max_it {
            let m_f = m as f64;
            let m2 = 2.0 * m_f;
            let aa = m_f * (b - m_f) * x / ((qam + m2) * (a + m2));
            d = 1.0 + aa * d;
            if d.abs() < 1e-30 {
                d = 1e-30;
            }
            c = 1.0 + aa / c;
            if c.abs() < 1e-30 {
                c = 1e-30;
            }
            d = 1.0 / d;
            h *= d * c;
            let aa2 = -(a + m_f) * (qab + m_f) * x / ((a + m2) * (qap + m2));
            d = 1.0 + aa2 * d;
            if d.abs() < 1e-30 {
                d = 1e-30;
            }
            c = 1.0 + aa2 / c;
            if c.abs() < 1e-30 {
                c = 1e-30;
            }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < eps {
                break;
            }
        }
        h
    };
    if x < (a + 1.0) / (a + b + 2.0) {
        StrykeValue::float(bt * cf(x, a, b) / a)
    } else {
        StrykeValue::float(1.0 - bt * cf(1.0 - x, b, a) / b)
    }
}

pub fn gamma_regularized_p(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_f64(args, 0).unwrap_or(1.0);
    let x = arg_f64(args, 1).unwrap_or(0.0).max(0.0);
    if x == 0.0 {
        return StrykeValue::float(0.0);
    }
    // Series for x < a+1, continued fraction otherwise.
    let gln = lgamma(a);
    if x < a + 1.0 {
        let mut ap = a;
        let mut sum = 1.0 / a;
        let mut del = sum;
        for _ in 0..200 {
            ap += 1.0;
            del *= x / ap;
            sum += del;
            if del.abs() < sum.abs() * 1e-12 {
                break;
            }
        }
        StrykeValue::float(sum * (-x + a * x.ln() - gln).exp())
    } else {
        let mut b = x + 1.0 - a;
        let mut c = 1.0 / 1e-30;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..=200 {
            let an = -(i as f64) * (i as f64 - a);
            b += 2.0;
            d = an * d + b;
            if d.abs() < 1e-30 {
                d = 1e-30;
            }
            c = b + an / c;
            if c.abs() < 1e-30 {
                c = 1e-30;
            }
            d = 1.0 / d;
            let del = d * c;
            h *= del;
            if (del - 1.0).abs() < 1e-12 {
                break;
            }
        }
        StrykeValue::float(1.0 - (-x + a * x.ln() - gln).exp() * h)
    }
}

pub fn gamma_regularized_q(args: &[StrykeValue]) -> StrykeValue {
    let p = gamma_regularized_p(args).to_number();
    StrykeValue::float(1.0 - p)
}

pub fn ei(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(1.0);
    if x == 0.0 {
        return StrykeValue::float(f64::NEG_INFINITY);
    }
    if x < 0.0 {
        // -E1(-x)
        let mut sum = 0.0_f64;
        let mut term = 1.0_f64;
        for k in 1..200 {
            term *= -x / k as f64;
            sum += term / k as f64;
            if term.abs() < 1e-15 {
                break;
            }
        }
        let euler = 0.5772156649015329_f64;
        return StrykeValue::float(-(euler + (-x).ln() + sum));
    }
    let mut sum = 0.0_f64;
    let mut term = 1.0_f64;
    for k in 1..200 {
        term *= x / k as f64;
        sum += term / k as f64;
        if term.abs() < 1e-15 {
            break;
        }
    }
    let euler = 0.5772156649015329_f64;
    StrykeValue::float(euler + x.abs().ln() + sum)
}

pub fn expint(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(1).max(0) as u32;
    let x = arg_f64(args, 1).unwrap_or(1.0).max(0.0);
    if n == 0 {
        return StrykeValue::float((-x).exp() / x);
    }
    // Series expansion for E_n(x), small x.
    if x < 1.0 {
        let euler = 0.5772156649015329_f64;
        let mut psi = -euler;
        for i in 1..n {
            psi += 1.0 / i as f64;
        }
        let mut sum = if n == 1 {
            -x.ln() - euler
        } else {
            1.0 / (n - 1) as f64
        };
        let mut term = if n == 1 { 1.0 } else { -1.0 / (n - 1) as f64 };
        let _ = psi;
        let mut k = 1usize;
        while k < 100 {
            term *= -x / k as f64;
            if (k as u32) + 1 != n {
                sum -= term / ((k as i64 - n as i64 + 1) as f64);
            } else {
                let mut psi2 = -euler;
                for i in 1..=k {
                    psi2 += 1.0 / i as f64;
                }
                sum += term * (psi2 - x.ln());
            }
            k += 1;
        }
        return StrykeValue::float(sum);
    }
    // Continued fraction for large x.
    let mut b = x + n as f64;
    let mut c = 1.0 / 1e-30;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..200 {
        let a = -(i as f64) * (n as f64 - 1.0 + i as f64);
        b += 2.0;
        d = a * d + b;
        if d.abs() < 1e-30 {
            d = 1e-30;
        }
        c = b + a / c;
        if c.abs() < 1e-30 {
            c = 1e-30;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;
        if (del - 1.0).abs() < 1e-12 {
            break;
        }
    }
    StrykeValue::float((-x).exp() * h)
}

pub fn si(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    // Series expansion: Si(x) = sum_{k=0}^inf (-1)^k * x^(2k+1) / ((2k+1)*(2k+1)!)
    let mut sum = 0.0;
    let mut term = x;
    sum += term;
    for k in 1..200 {
        let kk = (2 * k) as f64;
        term *= -x * x / (kk * (kk + 1.0));
        sum += term / (kk + 1.0);
        if term.abs() < 1e-15 {
            break;
        }
    }
    StrykeValue::float(sum)
}

pub fn li(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(2.0);
    if x <= 0.0 || x == 1.0 {
        return StrykeValue::UNDEF;
    }
    // Li(x) = Ei(ln x)
    let new_args = vec![StrykeValue::float(x.ln())];
    ei(&new_args)
}

pub fn zeta_riemann(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_f64(args, 0).unwrap_or(2.0);
    if s == 1.0 {
        return StrykeValue::float(f64::INFINITY);
    }
    let mut sum = 0.0;
    for n in 1..10000 {
        sum += 1.0 / (n as f64).powf(s);
    }
    StrykeValue::float(sum)
}

pub fn hypergeom_2f1(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_f64(args, 0).unwrap_or(1.0);
    let b = arg_f64(args, 1).unwrap_or(1.0);
    let c = arg_f64(args, 2).unwrap_or(1.0);
    let z = arg_f64(args, 3).unwrap_or(0.5);
    if z.abs() >= 1.0 {
        return StrykeValue::UNDEF;
    }
    let mut sum = 1.0;
    let mut term = 1.0;
    for k in 0..200 {
        term *= (a + k as f64) * (b + k as f64) / ((c + k as f64) * (k as f64 + 1.0)) * z;
        sum += term;
        if term.abs() < 1e-15 {
            break;
        }
    }
    StrykeValue::float(sum)
}

pub fn hypergeom_1f1(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_f64(args, 0).unwrap_or(1.0);
    let c = arg_f64(args, 1).unwrap_or(1.0);
    let z = arg_f64(args, 2).unwrap_or(0.5);
    let mut sum = 1.0;
    let mut term = 1.0;
    for k in 0..500 {
        term *= (a + k as f64) / ((c + k as f64) * (k as f64 + 1.0)) * z;
        sum += term;
        if term.abs() < 1e-15 {
            break;
        }
    }
    StrykeValue::float(sum)
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
    fn sv_a(xs: Vec<StrykeValue>) -> StrykeValue {
        arr_sv(xs)
    }

    #[test]
    fn matrix_determinant_2x2() {
        let m = sv_a(vec![sv_a(vec![sv(1.0), sv(2.0)]), sv_a(vec![sv(3.0), sv(4.0)])]);
        let r = matrix_determinant(&[m]);
        assert!((r.to_number() - (-2.0)).abs() < 1e-9);
    }

    #[test]
    fn matrix_determinant_3x3() {
        let m = sv_a(vec![
            sv_a(vec![sv(2.0), sv(1.0), sv(3.0)]),
            sv_a(vec![sv(1.0), sv(0.0), sv(2.0)]),
            sv_a(vec![sv(0.0), sv(1.0), sv(1.0)]),
        ]);
        let r = matrix_determinant(&[m]);
        // Manual: 2(0-2) - 1(1-0) + 3(1-0) = -4 -1 + 3 = -2
        assert!((r.to_number() - (-2.0)).abs() < 1e-9);
    }

    #[test]
    fn matrix_kronecker_2x2() {
        let a = sv_a(vec![sv_a(vec![sv(1.0), sv(2.0)]), sv_a(vec![sv(3.0), sv(4.0)])]);
        let b = sv_a(vec![sv_a(vec![sv(0.0), sv(5.0)]), sv_a(vec![sv(6.0), sv(7.0)])]);
        let r = matrix_kronecker(&[a, b]);
        let m = as_matrix(&r);
        assert_eq!(m.len(), 4);
        assert!((m[0][1] - 5.0).abs() < 1e-9);
        assert!((m[0][3] - 10.0).abs() < 1e-9);
        assert!((m[2][3] - 20.0).abs() < 1e-9);
        assert!((m[3][3] - 28.0).abs() < 1e-9);
    }

    #[test]
    fn matrix_cholesky_pd() {
        let m = sv_a(vec![
            sv_a(vec![sv(4.0), sv(12.0), sv(-16.0)]),
            sv_a(vec![sv(12.0), sv(37.0), sv(-43.0)]),
            sv_a(vec![sv(-16.0), sv(-43.0), sv(98.0)]),
        ]);
        let r = matrix_cholesky_decompose(&[m]);
        let l = as_matrix(&r);
        assert_eq!(l.len(), 3);
        assert!((l[0][0] - 2.0).abs() < 1e-6);
        assert!((l[1][0] - 6.0).abs() < 1e-6);
    }

    #[test]
    fn graph_bfs_simple() {
        // 0 -> 1, 2; 1 -> 3; 2 -> 3
        let g = sv_a(vec![
            sv_a(vec![sv_i(1), sv_i(2)]),
            sv_a(vec![sv_i(3)]),
            sv_a(vec![sv_i(3)]),
            sv_a(vec![]),
        ]);
        let r = graph_bfs(&[g, sv_i(0)]);
        let v: Vec<i64> = as_vec_sv(&r).iter().map(|x| x.to_int()).collect();
        assert_eq!(v, vec![0, 1, 2, 3]);
    }

    #[test]
    fn graph_dijkstra_weighted() {
        let g = sv_a(vec![
            sv_a(vec![sv_a(vec![sv_i(1), sv(4.0)]), sv_a(vec![sv_i(2), sv(1.0)])]),
            sv_a(vec![sv_a(vec![sv_i(3), sv(1.0)])]),
            sv_a(vec![sv_a(vec![sv_i(1), sv(2.0)]), sv_a(vec![sv_i(3), sv(5.0)])]),
            sv_a(vec![]),
        ]);
        let r = graph_dijkstra(&[g, sv_i(0)]);
        let v = as_vec_f64(&r);
        // 0->2(1) ->1(1+2=3) ->3(3+1=4) so dists = [0, 3, 1, 4]
        assert!((v[0] - 0.0).abs() < 1e-9);
        assert!((v[1] - 3.0).abs() < 1e-9);
        assert!((v[2] - 1.0).abs() < 1e-9);
        assert!((v[3] - 4.0).abs() < 1e-9);
    }

    #[test]
    fn graph_topological_acyclic() {
        let g = sv_a(vec![
            sv_a(vec![sv_i(1), sv_i(2)]),
            sv_a(vec![sv_i(3)]),
            sv_a(vec![sv_i(3)]),
            sv_a(vec![]),
        ]);
        let r = graph_topological_sort(&[g]);
        let v: Vec<i64> = as_vec_sv(&r).iter().map(|x| x.to_int()).collect();
        assert!(v.iter().position(|&x| x == 0) < v.iter().position(|&x| x == 3));
    }

    #[test]
    fn date_is_leap_basics() {
        assert_eq!(date_is_leap(&[sv_i(2000)]).to_int(), 1);
        assert_eq!(date_is_leap(&[sv_i(2100)]).to_int(), 0);
        assert_eq!(date_is_leap(&[sv_i(2024)]).to_int(), 1);
        assert_eq!(date_is_leap(&[sv_i(2023)]).to_int(), 0);
    }

    #[test]
    fn date_easter_2024() {
        let r = date_easter(&[sv_i(2024)]);
        assert_eq!(r.as_str_or_empty(), "2024-03-31");
    }

    #[test]
    fn beta_function_symmetry() {
        let a = beta_function(&[sv(2.0), sv(3.0)]).to_number();
        let b = beta_function(&[sv(3.0), sv(2.0)]).to_number();
        assert!((a - b).abs() < 1e-9);
        // B(2,3) = 1!*2! / 4! = 2/24 = 1/12
        assert!((a - 1.0 / 12.0).abs() < 1e-9);
    }

    #[test]
    fn zeta_2_is_pi_squared_over_6() {
        let z = zeta_riemann(&[sv(2.0)]).to_number();
        assert!((z - std::f64::consts::PI.powi(2) / 6.0).abs() < 1e-3);
    }

    #[test]
    fn zodiac_signs() {
        assert_eq!(zodiac_sign(&[sv_i(3), sv_i(25)]).as_str_or_empty(), "Aries");
        assert_eq!(zodiac_sign(&[sv_i(7), sv_i(15)]).as_str_or_empty(), "Cancer");
    }

    #[test]
    fn date_diff_days_2() {
        let a = date_str_to_unix(&[sv_s("2024-01-01"), sv_s("%Y-%m-%d")]);
        let b = date_str_to_unix(&[sv_s("2024-01-08"), sv_s("%Y-%m-%d")]);
        let r = date_diff_days(&[a, b]);
        assert_eq!(r.to_int(), 7);
    }
}
