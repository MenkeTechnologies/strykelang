//! Batch 20: combinatorics, audio synthesis, search algorithms,
//! physics 2D, noise generators, RNG variants, polygonal numbers.

use crate::value::StrykeValue;
use parking_lot::RwLock;
use std::sync::Arc;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::cmp::Ordering;

fn arg_f64(args: &[StrykeValue], idx: usize) -> Option<f64> {
    args.get(idx).map(|v| v.to_number())
}

fn arg_i64(args: &[StrykeValue], idx: usize) -> Option<i64> {
    args.get(idx).map(|v| v.to_int())
}

fn arg_u64(args: &[StrykeValue], idx: usize) -> Option<u64> {
    args.get(idx).map(|v| v.to_int() as u64)
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

fn arr_sv(v: Vec<StrykeValue>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(v)))
}

fn arr_f64(v: Vec<f64>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(
        v.into_iter().map(StrykeValue::float).collect(),
    )))
}

fn arr_i64(v: Vec<i64>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(
        v.into_iter().map(StrykeValue::integer).collect(),
    )))
}

// ══════════════════════════════════════════════════════════════════════
// Combinatorics
// ══════════════════════════════════════════════════════════════════════

pub fn derangement_count(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    if n == 0 {
        return StrykeValue::integer(1);
    }
    let mut d = vec![0_i64; n + 1];
    d[0] = 1;
    if n >= 1 {
        d[1] = 0;
    }
    for i in 2..=n {
        d[i] = (i as i64 - 1) * (d[i - 1] + d[i - 2]);
    }
    StrykeValue::integer(d[n])
}

pub fn partitions_count(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let mut p = vec![0_i64; n + 1];
    p[0] = 1;
    for i in 1..=n {
        let mut sum = 0_i64;
        let mut k = 1_i64;
        loop {
            let g1 = k * (3 * k - 1) / 2;
            let g2 = k * (3 * k + 1) / 2;
            if g1 as usize > i {
                break;
            }
            let sign = if k % 2 == 1 { 1 } else { -1 };
            sum += sign * p[i - g1 as usize];
            if (g2 as usize) <= i {
                sum += sign * p[i - g2 as usize];
            }
            k += 1;
        }
        p[i] = sum;
    }
    StrykeValue::integer(p[n])
}

pub fn compositions_count(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0);
    if n == 0 {
        return StrykeValue::integer(1);
    }
    StrykeValue::integer(1_i64 << (n - 1))
}

pub fn lattice_paths(args: &[StrykeValue]) -> StrykeValue {
    let m = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let n = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    fn binom(n: usize, k: usize) -> i64 {
        let k = k.min(n - k);
        let mut result = 1_i64;
        for i in 0..k {
            result = result * (n - i) as i64 / (i + 1) as i64;
        }
        result
    }
    StrykeValue::integer(binom(m + n, m))
}

pub fn multinomial_coefficient(args: &[StrykeValue]) -> StrykeValue {
    let parts: Vec<i64> = args.first().map(as_vec_sv).unwrap_or_default().iter().map(|x| x.to_int()).collect();
    if parts.is_empty() {
        return StrykeValue::integer(1);
    }
    let n: i64 = parts.iter().sum();
    fn fact(n: i64) -> f64 {
        libm::lgamma((n + 1) as f64)
    }
    let log_result = fact(n) - parts.iter().map(|&k| fact(k)).sum::<f64>();
    StrykeValue::float(log_result.exp().round())
}

pub fn super_factorial(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let mut sf = 1_i128;
    let mut f = 1_i128;
    for i in 1..=n {
        f *= i as i128;
        sf *= f;
    }
    StrykeValue::integer(sf as i64)
}

pub fn hyperfactorial(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let mut h = 1_i128;
    for i in 1..=n {
        h *= (i as i128).pow(i as u32);
    }
    StrykeValue::integer(h as i64)
}

pub fn primorial(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0);
    if n < 2 {
        return StrykeValue::integer(1);
    }
    let mut sieve = vec![true; (n + 1) as usize];
    sieve[0] = false;
    sieve[1] = false;
    for i in 2..=(n as usize) {
        if sieve[i] {
            let mut j = i * i;
            while j <= n as usize {
                sieve[j] = false;
                j += i;
            }
        }
    }
    let mut result = 1_i64;
    for i in 2..=(n as usize) {
        if sieve[i] {
            result = result.saturating_mul(i as i64);
        }
    }
    StrykeValue::integer(result)
}

pub fn fibonacci_matrix(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as u64;
    // [[1,1],[1,0]]^n
    fn mat_mul(a: [[u64; 2]; 2], b: [[u64; 2]; 2]) -> [[u64; 2]; 2] {
        [
            [
                a[0][0].wrapping_mul(b[0][0]).wrapping_add(a[0][1].wrapping_mul(b[1][0])),
                a[0][0].wrapping_mul(b[0][1]).wrapping_add(a[0][1].wrapping_mul(b[1][1])),
            ],
            [
                a[1][0].wrapping_mul(b[0][0]).wrapping_add(a[1][1].wrapping_mul(b[1][0])),
                a[1][0].wrapping_mul(b[0][1]).wrapping_add(a[1][1].wrapping_mul(b[1][1])),
            ],
        ]
    }
    let mut result = [[1, 0], [0, 1]];
    let mut base = [[1, 1], [1, 0]];
    let mut e = n;
    while e > 0 {
        if e & 1 == 1 {
            result = mat_mul(result, base);
        }
        base = mat_mul(base, base);
        e >>= 1;
    }
    StrykeValue::integer(result[0][1] as i64)
}

pub fn fibonacci_nth_fast(args: &[StrykeValue]) -> StrykeValue {
    fibonacci_matrix(args)
}

pub fn lucas_nth(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as u64;
    if n == 0 {
        return StrykeValue::integer(2);
    }
    if n == 1 {
        return StrykeValue::integer(1);
    }
    let mut a = 2_i64;
    let mut b = 1_i64;
    for _ in 2..=n {
        let c = a.wrapping_add(b);
        a = b;
        b = c;
    }
    StrykeValue::integer(b)
}

pub fn pell_nth(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as u64;
    if n == 0 {
        return StrykeValue::integer(0);
    }
    if n == 1 {
        return StrykeValue::integer(1);
    }
    let mut a = 0_i64;
    let mut b = 1_i64;
    for _ in 2..=n {
        let c = 2_i64.wrapping_mul(b).wrapping_add(a);
        a = b;
        b = c;
    }
    StrykeValue::integer(b)
}

pub fn tetranacci(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let mut t = [0_i64, 0, 0, 1];
    if n < 4 {
        return StrykeValue::integer(t[n]);
    }
    for _ in 4..=n {
        let next = t[0] + t[1] + t[2] + t[3];
        t.rotate_left(1);
        t[3] = next;
    }
    StrykeValue::integer(t[3])
}

pub fn narayana_cow(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0) as usize;
    let mut a = vec![1_i64; n + 1];
    for i in 3..=n {
        a[i] = a[i - 1] + a[i - 3];
    }
    if n < a.len() {
        StrykeValue::integer(a[n])
    } else {
        StrykeValue::integer(1)
    }
}

pub fn hexagonal_number(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0);
    StrykeValue::integer(n * (2 * n - 1))
}

pub fn heptagonal_number(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0);
    StrykeValue::integer(n * (5 * n - 3) / 2)
}

pub fn octagonal_number(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0);
    StrykeValue::integer(n * (3 * n - 2))
}

pub fn nonagonal_number(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0);
    StrykeValue::integer(n * (7 * n - 5) / 2)
}

pub fn decagonal_number(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0);
    StrykeValue::integer(n * (4 * n - 3))
}

pub fn centered_polygonal(args: &[StrykeValue]) -> StrykeValue {
    let k = arg_i64(args, 0).unwrap_or(3).max(3);
    let n = arg_i64(args, 1).unwrap_or(0).max(0);
    StrykeValue::integer(1 + k * n * (n - 1) / 2)
}

pub fn square_pyramidal(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0);
    StrykeValue::integer(n * (n + 1) * (2 * n + 1) / 6)
}

pub fn tetrahedral(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0);
    StrykeValue::integer(n * (n + 1) * (n + 2) / 6)
}

pub fn cube_number(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0);
    StrykeValue::integer(n * n * n)
}

pub fn icosahedral(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0);
    StrykeValue::integer(n * (5 * n * n - 5 * n + 2) / 2)
}

pub fn dodecahedral(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0);
    StrykeValue::integer(n * (3 * n - 1) * (3 * n - 2) / 2)
}

pub fn gnomonic_number(args: &[StrykeValue]) -> StrykeValue {
    let n = arg_i64(args, 0).unwrap_or(0).max(0);
    StrykeValue::integer(2 * n - 1)
}

// ══════════════════════════════════════════════════════════════════════
// Search algorithms
// ══════════════════════════════════════════════════════════════════════

fn adj_unweighted(v: &StrykeValue) -> Vec<Vec<usize>> {
    as_vec_sv(v)
        .iter()
        .map(|row| {
            as_vec_sv(row)
                .iter()
                .map(|e| {
                    if let Some(p) = e.as_array_ref() {
                        p.read().first().map(|x| x.to_int().max(0) as usize).unwrap_or(0)
                    } else if let Some(p) = e.as_array_vec() {
                        p.first().map(|x| x.to_int().max(0) as usize).unwrap_or(0)
                    } else {
                        e.to_int().max(0) as usize
                    }
                })
                .collect()
        })
        .collect()
}

fn adj_weighted(v: &StrykeValue) -> Vec<Vec<(usize, f64)>> {
    as_vec_sv(v)
        .iter()
        .map(|row| {
            as_vec_sv(row)
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

#[derive(PartialEq)]
struct AstarNode(f64, usize);

impl Eq for AstarNode {}

impl Ord for AstarNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.partial_cmp(&self.0).unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for AstarNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub fn ida_star_search(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_weighted).unwrap_or_default();
    let start = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let goal = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    let heuristic = args.get(3).map(as_vec_f64).unwrap_or_default();
    let h = |u: usize| heuristic.get(u).copied().unwrap_or(0.0);
    let mut threshold = h(start);
    for _ in 0..100 {
        let mut path = vec![start];
        let result = ida_star_dfs(&g, start, goal, 0.0, threshold, &h, &mut path);
        if result < 0.0 {
            return arr_sv(path.into_iter().map(|x| StrykeValue::integer(x as i64)).collect());
        }
        if result == f64::INFINITY {
            return arr_sv(vec![]);
        }
        threshold = result;
    }
    arr_sv(vec![])
}

fn ida_star_dfs(
    g: &[Vec<(usize, f64)>],
    u: usize,
    goal: usize,
    cost: f64,
    threshold: f64,
    h: &dyn Fn(usize) -> f64,
    path: &mut Vec<usize>,
) -> f64 {
    let f = cost + h(u);
    if f > threshold {
        return f;
    }
    if u == goal {
        return -1.0;
    }
    let mut min = f64::INFINITY;
    if u < g.len() {
        for &(v, w) in &g[u] {
            if !path.contains(&v) {
                path.push(v);
                let t = ida_star_dfs(g, v, goal, cost + w, threshold, h, path);
                if t < 0.0 {
                    return -1.0;
                }
                if t < min {
                    min = t;
                }
                path.pop();
            }
        }
    }
    min
}

pub fn bidirectional_bfs(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let s = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let t = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    let n = g.len();
    if s >= n || t >= n {
        return arr_sv(vec![]);
    }
    if s == t {
        return arr_i64(vec![s as i64]);
    }
    let mut visited_s: HashMap<usize, Option<usize>> = HashMap::new();
    let mut visited_t: HashMap<usize, Option<usize>> = HashMap::new();
    visited_s.insert(s, None);
    visited_t.insert(t, None);
    let mut q_s = VecDeque::from([s]);
    let mut q_t = VecDeque::from([t]);
    let mut meet: Option<usize> = None;
    while !q_s.is_empty() && !q_t.is_empty() && meet.is_none() {
        if let Some(u) = q_s.pop_front() {
            for &v in &g[u] {
                if let std::collections::hash_map::Entry::Vacant(e) = visited_s.entry(v) {
                    e.insert(Some(u));
                    if visited_t.contains_key(&v) {
                        meet = Some(v);
                        break;
                    }
                    q_s.push_back(v);
                }
            }
        }
        if meet.is_some() {
            break;
        }
        if let Some(u) = q_t.pop_front() {
            for &v in &g[u] {
                if let std::collections::hash_map::Entry::Vacant(e) = visited_t.entry(v) {
                    e.insert(Some(u));
                    if visited_s.contains_key(&v) {
                        meet = Some(v);
                        break;
                    }
                    q_t.push_back(v);
                }
            }
        }
    }
    match meet {
        Some(m) => {
            let mut left = Vec::new();
            let mut cur = Some(m);
            while let Some(u) = cur {
                left.push(u);
                cur = visited_s.get(&u).copied().flatten();
            }
            left.reverse();
            let mut right = Vec::new();
            let mut cur = visited_t.get(&m).copied().flatten();
            while let Some(u) = cur {
                right.push(u);
                cur = visited_t.get(&u).copied().flatten();
            }
            left.extend(right);
            arr_i64(left.into_iter().map(|x| x as i64).collect())
        }
        None => arr_sv(vec![]),
    }
}

pub fn a_star_grid(args: &[StrykeValue]) -> StrykeValue {
    let grid = args
        .first()
        .map(|v| {
            as_vec_sv(v)
                .iter()
                .map(|r| as_vec_sv(r).iter().map(|x| x.to_int()).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let start_v = args.get(1).map(as_vec_sv).unwrap_or_default();
    let goal_v = args.get(2).map(as_vec_sv).unwrap_or_default();
    if grid.is_empty() || start_v.len() < 2 || goal_v.len() < 2 {
        return arr_sv(vec![]);
    }
    let h = grid.len();
    let w = grid[0].len();
    let sx = start_v[0].to_int() as usize;
    let sy = start_v[1].to_int() as usize;
    let gx = goal_v[0].to_int() as usize;
    let gy = goal_v[1].to_int() as usize;
    if sx >= h || sy >= w || gx >= h || gy >= w {
        return arr_sv(vec![]);
    }
    let dirs = [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)];
    let mut g_score: HashMap<(usize, usize), f64> = HashMap::new();
    g_score.insert((sx, sy), 0.0);
    let mut came_from: HashMap<(usize, usize), (usize, usize)> = HashMap::new();
    let mut open: BinaryHeap<AstarNode> = BinaryHeap::new();
    open.push(AstarNode((gx as f64 - sx as f64).abs() + (gy as f64 - sy as f64).abs(), sx * w + sy));
    while let Some(AstarNode(_, idx)) = open.pop() {
        let x = idx / w;
        let y = idx % w;
        if (x, y) == (gx, gy) {
            let mut path = vec![(x, y)];
            let mut cur = (x, y);
            while let Some(&prev) = came_from.get(&cur) {
                path.push(prev);
                cur = prev;
            }
            path.reverse();
            return arr_sv(path.into_iter().map(|(a, b)| arr_i64(vec![a as i64, b as i64])).collect());
        }
        for (dx, dy) in &dirs {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx as usize >= h || ny as usize >= w {
                continue;
            }
            let nx = nx as usize;
            let ny = ny as usize;
            if grid[nx][ny] != 0 {
                continue;
            }
            let tentative = g_score[&(x, y)] + 1.0;
            if tentative < *g_score.get(&(nx, ny)).unwrap_or(&f64::INFINITY) {
                came_from.insert((nx, ny), (x, y));
                g_score.insert((nx, ny), tentative);
                let f = tentative + ((gx as f64 - nx as f64).abs() + (gy as f64 - ny as f64).abs());
                open.push(AstarNode(f, nx * w + ny));
            }
        }
    }
    arr_sv(vec![])
}

pub fn greedy_best_first(args: &[StrykeValue]) -> StrykeValue {
    let g = args.first().map(adj_unweighted).unwrap_or_default();
    let s = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let t = arg_i64(args, 2).unwrap_or(0).max(0) as usize;
    let heur = args.get(3).map(as_vec_f64).unwrap_or_default();
    let h = |u: usize| heur.get(u).copied().unwrap_or(0.0);
    let n = g.len();
    if s >= n || t >= n {
        return arr_sv(vec![]);
    }
    let mut visited = HashSet::new();
    let mut parent: HashMap<usize, usize> = HashMap::new();
    let mut open: BinaryHeap<AstarNode> = BinaryHeap::new();
    open.push(AstarNode(h(s), s));
    visited.insert(s);
    while let Some(AstarNode(_, u)) = open.pop() {
        if u == t {
            let mut path = vec![u];
            let mut cur = u;
            while let Some(&p) = parent.get(&cur) {
                path.push(p);
                cur = p;
            }
            path.reverse();
            return arr_i64(path.into_iter().map(|x| x as i64).collect());
        }
        for &v in &g[u] {
            if v < n && !visited.contains(&v) {
                visited.insert(v);
                parent.insert(v, u);
                open.push(AstarNode(h(v), v));
            }
        }
    }
    arr_sv(vec![])
}

pub fn floyd_cycle_detect(args: &[StrykeValue]) -> StrykeValue {
    let xs: Vec<i64> = args.first().map(as_vec_sv).unwrap_or_default().iter().map(|x| x.to_int()).collect();
    let n = xs.len();
    if n < 2 {
        return StrykeValue::integer(0);
    }
    let mut slow = 0;
    let mut fast = 0;
    loop {
        if (slow as i64) < 0 || slow >= n {
            return StrykeValue::integer(0);
        }
        slow = xs[slow] as usize;
        if (fast as i64) < 0 || fast >= n {
            return StrykeValue::integer(0);
        }
        fast = xs[fast] as usize;
        if fast >= n {
            return StrykeValue::integer(0);
        }
        fast = xs[fast] as usize;
        if slow == fast {
            return StrykeValue::integer(1);
        }
    }
}

pub fn ternary_search(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_sv).unwrap_or_default();
    let target = arg_i64(args, 1).unwrap_or(0);
    if xs.is_empty() {
        return StrykeValue::integer(-1);
    }
    let mut lo = 0_i64;
    let mut hi = (xs.len() - 1) as i64;
    while lo <= hi {
        let m1 = lo + (hi - lo) / 3;
        let m2 = hi - (hi - lo) / 3;
        let v1 = xs[m1 as usize].to_int();
        let v2 = xs[m2 as usize].to_int();
        if v1 == target {
            return StrykeValue::integer(m1);
        }
        if v2 == target {
            return StrykeValue::integer(m2);
        }
        if target < v1 {
            hi = m1 - 1;
        } else if target > v2 {
            lo = m2 + 1;
        } else {
            lo = m1 + 1;
            hi = m2 - 1;
        }
    }
    StrykeValue::integer(-1)
}

pub fn exponential_search(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_sv).unwrap_or_default();
    let target = arg_i64(args, 1).unwrap_or(0);
    let n = xs.len();
    if n == 0 {
        return StrykeValue::integer(-1);
    }
    if xs[0].to_int() == target {
        return StrykeValue::integer(0);
    }
    let mut i = 1_usize;
    while i < n && xs[i].to_int() <= target {
        i *= 2;
    }
    let lo = i / 2;
    let hi = n.min(i + 1);
    let mut l = lo as i64;
    let mut r = hi as i64 - 1;
    while l <= r {
        let mid = (l + r) / 2;
        let v = xs[mid as usize].to_int();
        if v == target {
            return StrykeValue::integer(mid);
        } else if v < target {
            l = mid + 1;
        } else {
            r = mid - 1;
        }
    }
    StrykeValue::integer(-1)
}

pub fn interpolation_search(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_sv).unwrap_or_default();
    let target = arg_i64(args, 1).unwrap_or(0);
    let n = xs.len();
    if n == 0 {
        return StrykeValue::integer(-1);
    }
    let mut lo = 0_i64;
    let mut hi = (n - 1) as i64;
    while lo <= hi && target >= xs[lo as usize].to_int() && target <= xs[hi as usize].to_int() {
        let denom = xs[hi as usize].to_int() - xs[lo as usize].to_int();
        let pos = if denom == 0 {
            lo
        } else {
            lo + ((target - xs[lo as usize].to_int()) * (hi - lo)) / denom
        };
        if pos < 0 || pos >= n as i64 {
            return StrykeValue::integer(-1);
        }
        let v = xs[pos as usize].to_int();
        if v == target {
            return StrykeValue::integer(pos);
        }
        if v < target {
            lo = pos + 1;
        } else {
            hi = pos - 1;
        }
    }
    StrykeValue::integer(-1)
}

// ══════════════════════════════════════════════════════════════════════
// Audio synthesis primitives
// ══════════════════════════════════════════════════════════════════════

pub fn wavetable_synth(args: &[StrykeValue]) -> StrykeValue {
    let table = args.first().map(as_vec_f64).unwrap_or_default();
    let freq = arg_f64(args, 1).unwrap_or(440.0);
    let sr = arg_f64(args, 2).unwrap_or(44100.0);
    let dur = arg_f64(args, 3).unwrap_or(1.0);
    if table.is_empty() {
        return arr_f64(vec![]);
    }
    let samples = (sr * dur) as usize;
    let mut out = Vec::with_capacity(samples);
    let n = table.len() as f64;
    for i in 0..samples {
        let phase = (i as f64 * freq * n / sr) % n;
        let idx = phase as usize;
        let frac = phase - idx as f64;
        let a = table[idx];
        let b = table[(idx + 1) % table.len()];
        out.push(a + (b - a) * frac);
    }
    arr_f64(out)
}

pub fn fm_synth_2op(args: &[StrykeValue]) -> StrykeValue {
    let carrier = arg_f64(args, 0).unwrap_or(440.0);
    let mod_freq = arg_f64(args, 1).unwrap_or(220.0);
    let mod_idx = arg_f64(args, 2).unwrap_or(2.0);
    let sr = arg_f64(args, 3).unwrap_or(44100.0);
    let dur = arg_f64(args, 4).unwrap_or(1.0);
    let n = (sr * dur) as usize;
    let two_pi = 2.0 * std::f64::consts::PI;
    arr_f64(
        (0..n)
            .map(|i| {
                let t = i as f64 / sr;
                let m = mod_idx * (two_pi * mod_freq * t).sin();
                (two_pi * carrier * t + m).sin()
            })
            .collect(),
    )
}

pub fn am_synth(args: &[StrykeValue]) -> StrykeValue {
    let carrier = arg_f64(args, 0).unwrap_or(440.0);
    let mod_freq = arg_f64(args, 1).unwrap_or(10.0);
    let depth = arg_f64(args, 2).unwrap_or(0.5);
    let sr = arg_f64(args, 3).unwrap_or(44100.0);
    let dur = arg_f64(args, 4).unwrap_or(1.0);
    let n = (sr * dur) as usize;
    let two_pi = 2.0 * std::f64::consts::PI;
    arr_f64(
        (0..n)
            .map(|i| {
                let t = i as f64 / sr;
                let m = 1.0 + depth * (two_pi * mod_freq * t).sin();
                (two_pi * carrier * t).sin() * m
            })
            .collect(),
    )
}

pub fn ring_modulate(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec_f64).unwrap_or_default();
    let b = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = a.len().min(b.len());
    arr_f64((0..n).map(|i| a[i] * b[i]).collect())
}

pub fn chorus_simple(args: &[StrykeValue]) -> StrykeValue {
    let signal = args.first().map(as_vec_f64).unwrap_or_default();
    let depth_ms = arg_f64(args, 1).unwrap_or(8.0);
    let rate_hz = arg_f64(args, 2).unwrap_or(0.5);
    let sr = arg_f64(args, 3).unwrap_or(44100.0);
    let mix = arg_f64(args, 4).unwrap_or(0.5);
    let n = signal.len();
    let two_pi = 2.0 * std::f64::consts::PI;
    let mut out = Vec::with_capacity(n);
    for (i, &x) in signal.iter().enumerate() {
        let delay_samples = depth_ms * 0.001 * sr * (1.0 + (two_pi * rate_hz * i as f64 / sr).sin()) / 2.0;
        let read = i as f64 - delay_samples;
        let delayed = if read >= 0.0 && (read as usize) < n {
            signal[read as usize]
        } else {
            0.0
        };
        out.push(x * (1.0 - mix) + delayed * mix);
    }
    arr_f64(out)
}

pub fn flanger_simple(args: &[StrykeValue]) -> StrykeValue {
    chorus_simple(args)
}

pub fn phaser_simple(args: &[StrykeValue]) -> StrykeValue {
    let signal = args.first().map(as_vec_f64).unwrap_or_default();
    let rate_hz = arg_f64(args, 1).unwrap_or(0.5);
    let depth = arg_f64(args, 2).unwrap_or(1.0);
    let sr = arg_f64(args, 3).unwrap_or(44100.0);
    let n = signal.len();
    let two_pi = 2.0 * std::f64::consts::PI;
    let mut prev = 0.0;
    arr_f64(
        signal
            .iter()
            .enumerate()
            .map(|(i, &x)| {
                let coef = depth * (two_pi * rate_hz * i as f64 / sr).sin();
                let out = -coef * x + prev + coef * (if i > 0 { signal[i - 1] } else { 0.0 });
                let _ = n;
                prev = out;
                out
            })
            .collect(),
    )
}

pub fn comb_filter(args: &[StrykeValue]) -> StrykeValue {
    let signal = args.first().map(as_vec_f64).unwrap_or_default();
    let delay = arg_i64(args, 1).unwrap_or(441).max(1) as usize;
    let gain = arg_f64(args, 2).unwrap_or(0.5);
    let n = signal.len();
    let mut out = vec![0.0; n];
    for i in 0..n {
        out[i] = signal[i] + if i >= delay { gain * out[i - delay] } else { 0.0 };
    }
    arr_f64(out)
}

pub fn all_pass_filter(args: &[StrykeValue]) -> StrykeValue {
    let signal = args.first().map(as_vec_f64).unwrap_or_default();
    let delay = arg_i64(args, 1).unwrap_or(441).max(1) as usize;
    let gain = arg_f64(args, 2).unwrap_or(0.5);
    let n = signal.len();
    let mut out = vec![0.0; n];
    for i in 0..n {
        let delayed = if i >= delay { out[i - delay] } else { 0.0 };
        out[i] = -gain * signal[i] + (if i >= delay { signal[i - delay] } else { 0.0 }) + gain * delayed;
    }
    arr_f64(out)
}

pub fn fir_filter(args: &[StrykeValue]) -> StrykeValue {
    let signal = args.first().map(as_vec_f64).unwrap_or_default();
    let taps = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n = signal.len();
    let k = taps.len();
    let mut out = vec![0.0; n];
    for i in 0..n {
        for j in 0..k {
            if i >= j {
                out[i] += taps[j] * signal[i - j];
            }
        }
    }
    arr_f64(out)
}

pub fn schroeder_reverb(args: &[StrykeValue]) -> StrykeValue {
    let signal = args.first().map(as_vec_f64).unwrap_or_default();
    // Pass through 4 comb filters in parallel, then 2 all-pass in series
    let delays = [1557, 1617, 1491, 1422];
    let gain = 0.7;
    let n = signal.len();
    let mut summed = vec![0.0; n];
    for &delay in &delays {
        let mut out = vec![0.0; n];
        for i in 0..n {
            out[i] = signal[i] + if i >= delay { gain * out[i - delay] } else { 0.0 };
        }
        for i in 0..n {
            summed[i] += out[i] * 0.25;
        }
    }
    let allpass = |x: &[f64], delay: usize, gain: f64| -> Vec<f64> {
        let n = x.len();
        let mut out = vec![0.0; n];
        for i in 0..n {
            let delayed = if i >= delay { out[i - delay] } else { 0.0 };
            out[i] = -gain * x[i] + (if i >= delay { x[i - delay] } else { 0.0 }) + gain * delayed;
        }
        out
    };
    let r = allpass(&allpass(&summed, 225, 0.5), 556, 0.5);
    arr_f64(r)
}

pub fn plate_reverb_simple(args: &[StrykeValue]) -> StrykeValue {
    schroeder_reverb(args)
}

pub fn freeverb_lite(args: &[StrykeValue]) -> StrykeValue {
    schroeder_reverb(args)
}

// ══════════════════════════════════════════════════════════════════════
// Physics 2D
// ══════════════════════════════════════════════════════════════════════

pub fn projectile_position(args: &[StrykeValue]) -> StrykeValue {
    let v0 = arg_f64(args, 0).unwrap_or(0.0);
    let angle = arg_f64(args, 1).unwrap_or(0.0).to_radians();
    let g = arg_f64(args, 2).unwrap_or(9.81);
    let t = arg_f64(args, 3).unwrap_or(0.0);
    let x = v0 * angle.cos() * t;
    let y = v0 * angle.sin() * t - 0.5 * g * t * t;
    arr_f64(vec![x, y])
}

pub fn projectile_velocity(args: &[StrykeValue]) -> StrykeValue {
    let v0 = arg_f64(args, 0).unwrap_or(0.0);
    let angle = arg_f64(args, 1).unwrap_or(0.0).to_radians();
    let g = arg_f64(args, 2).unwrap_or(9.81);
    let t = arg_f64(args, 3).unwrap_or(0.0);
    let vx = v0 * angle.cos();
    let vy = v0 * angle.sin() - g * t;
    arr_f64(vec![vx, vy])
}

pub fn spring_oscillator_pos(args: &[StrykeValue]) -> StrykeValue {
    let amplitude = arg_f64(args, 0).unwrap_or(1.0);
    let omega = arg_f64(args, 1).unwrap_or(1.0);
    let phase = arg_f64(args, 2).unwrap_or(0.0);
    let t = arg_f64(args, 3).unwrap_or(0.0);
    StrykeValue::float(amplitude * (omega * t + phase).cos())
}

pub fn damping_factor(args: &[StrykeValue]) -> StrykeValue {
    let c = arg_f64(args, 0).unwrap_or(0.0);
    let m = arg_f64(args, 1).unwrap_or(1.0).max(1e-12);
    let k = arg_f64(args, 2).unwrap_or(1.0).max(1e-12);
    StrykeValue::float(c / (2.0 * (m * k).sqrt()))
}

pub fn critical_damping(args: &[StrykeValue]) -> StrykeValue {
    let m = arg_f64(args, 0).unwrap_or(1.0);
    let k = arg_f64(args, 1).unwrap_or(1.0).max(0.0);
    StrykeValue::float(2.0 * (m * k).sqrt())
}

pub fn elastic_collision_1d(args: &[StrykeValue]) -> StrykeValue {
    let m1 = arg_f64(args, 0).unwrap_or(1.0);
    let v1 = arg_f64(args, 1).unwrap_or(0.0);
    let m2 = arg_f64(args, 2).unwrap_or(1.0);
    let v2 = arg_f64(args, 3).unwrap_or(0.0);
    let v1p = ((m1 - m2) * v1 + 2.0 * m2 * v2) / (m1 + m2);
    let v2p = ((m2 - m1) * v2 + 2.0 * m1 * v1) / (m1 + m2);
    arr_f64(vec![v1p, v2p])
}

pub fn inelastic_collision_1d(args: &[StrykeValue]) -> StrykeValue {
    let m1 = arg_f64(args, 0).unwrap_or(1.0);
    let v1 = arg_f64(args, 1).unwrap_or(0.0);
    let m2 = arg_f64(args, 2).unwrap_or(1.0);
    let v2 = arg_f64(args, 3).unwrap_or(0.0);
    StrykeValue::float((m1 * v1 + m2 * v2) / (m1 + m2))
}

pub fn collision_response_2d(args: &[StrykeValue]) -> StrykeValue {
    let v1 = as_vec_f64(args.first().unwrap_or(&StrykeValue::UNDEF));
    let v2 = as_vec_f64(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let n = as_vec_f64(args.get(2).unwrap_or(&StrykeValue::UNDEF));
    let m1 = arg_f64(args, 3).unwrap_or(1.0);
    let m2 = arg_f64(args, 4).unwrap_or(1.0);
    if v1.len() < 2 || v2.len() < 2 || n.len() < 2 {
        return arr_sv(vec![]);
    }
    let nlen = (n[0].powi(2) + n[1].powi(2)).sqrt().max(1e-12);
    let nx = n[0] / nlen;
    let ny = n[1] / nlen;
    let vrel_dot_n = (v1[0] - v2[0]) * nx + (v1[1] - v2[1]) * ny;
    let j = -2.0 * vrel_dot_n / (1.0 / m1 + 1.0 / m2);
    let new_v1 = vec![v1[0] + j * nx / m1, v1[1] + j * ny / m1];
    let new_v2 = vec![v2[0] - j * nx / m2, v2[1] - j * ny / m2];
    arr_sv(vec![arr_f64(new_v1), arr_f64(new_v2)])
}

pub fn torque_arm(args: &[StrykeValue]) -> StrykeValue {
    let f = arg_f64(args, 0).unwrap_or(0.0);
    let r = arg_f64(args, 1).unwrap_or(0.0);
    let angle = arg_f64(args, 2).unwrap_or(90.0).to_radians();
    StrykeValue::float(f * r * angle.sin())
}

pub fn moment_of_inertia_disc(args: &[StrykeValue]) -> StrykeValue {
    let m = arg_f64(args, 0).unwrap_or(1.0);
    let r = arg_f64(args, 1).unwrap_or(1.0);
    StrykeValue::float(0.5 * m * r * r)
}

pub fn moment_of_inertia_rod(args: &[StrykeValue]) -> StrykeValue {
    let m = arg_f64(args, 0).unwrap_or(1.0);
    let l = arg_f64(args, 1).unwrap_or(1.0);
    StrykeValue::float(m * l * l / 12.0)
}

pub fn moment_of_inertia_sphere(args: &[StrykeValue]) -> StrykeValue {
    let m = arg_f64(args, 0).unwrap_or(1.0);
    let r = arg_f64(args, 1).unwrap_or(1.0);
    StrykeValue::float(0.4 * m * r * r)
}

pub fn moment_of_inertia_cylinder(args: &[StrykeValue]) -> StrykeValue {
    let m = arg_f64(args, 0).unwrap_or(1.0);
    let r = arg_f64(args, 1).unwrap_or(1.0);
    StrykeValue::float(0.5 * m * r * r)
}

pub fn center_of_mass_2d(args: &[StrykeValue]) -> StrykeValue {
    let masses = args.first().map(as_vec_f64).unwrap_or_default();
    let positions: Vec<Vec<f64>> = args
        .get(1)
        .map(|v| as_vec_sv(v).iter().map(as_vec_f64).collect())
        .unwrap_or_default();
    let total_m: f64 = masses.iter().sum();
    if total_m < 1e-12 {
        return arr_f64(vec![0.0, 0.0]);
    }
    let mut cx = 0.0_f64;
    let mut cy = 0.0_f64;
    for (i, p) in positions.iter().enumerate() {
        let m = masses.get(i).copied().unwrap_or(0.0);
        cx += m * p.first().copied().unwrap_or(0.0);
        cy += m * p.get(1).copied().unwrap_or(0.0);
    }
    arr_f64(vec![cx / total_m, cy / total_m])
}

pub fn center_of_mass_3d(args: &[StrykeValue]) -> StrykeValue {
    let masses = args.first().map(as_vec_f64).unwrap_or_default();
    let positions: Vec<Vec<f64>> = args
        .get(1)
        .map(|v| as_vec_sv(v).iter().map(as_vec_f64).collect())
        .unwrap_or_default();
    let total_m: f64 = masses.iter().sum();
    if total_m < 1e-12 {
        return arr_f64(vec![0.0, 0.0, 0.0]);
    }
    let mut c = [0.0_f64; 3];
    for (i, p) in positions.iter().enumerate() {
        let m = masses.get(i).copied().unwrap_or(0.0);
        for j in 0..3 {
            c[j] += m * p.get(j).copied().unwrap_or(0.0);
        }
    }
    arr_f64(vec![c[0] / total_m, c[1] / total_m, c[2] / total_m])
}

pub fn buoyancy_force(args: &[StrykeValue]) -> StrykeValue {
    let rho = arg_f64(args, 0).unwrap_or(1000.0);
    let v = arg_f64(args, 1).unwrap_or(0.0);
    let g = arg_f64(args, 2).unwrap_or(9.81);
    StrykeValue::float(rho * v * g)
}

pub fn lift_force(args: &[StrykeValue]) -> StrykeValue {
    let cl = arg_f64(args, 0).unwrap_or(0.0);
    let rho = arg_f64(args, 1).unwrap_or(1.225);
    let v = arg_f64(args, 2).unwrap_or(0.0);
    let a = arg_f64(args, 3).unwrap_or(0.0);
    StrykeValue::float(0.5 * cl * rho * v * v * a)
}

pub fn poisson_brackets(args: &[StrykeValue]) -> StrykeValue {
    // {f, g} for 1D: df/dq * dg/dp - df/dp * dg/dq
    let df_dq = arg_f64(args, 0).unwrap_or(0.0);
    let df_dp = arg_f64(args, 1).unwrap_or(0.0);
    let dg_dq = arg_f64(args, 2).unwrap_or(0.0);
    let dg_dp = arg_f64(args, 3).unwrap_or(0.0);
    StrykeValue::float(df_dq * dg_dp - df_dp * dg_dq)
}

// ══════════════════════════════════════════════════════════════════════
// Noise generators
// ══════════════════════════════════════════════════════════════════════

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + t * (b - a)
}

fn perlin_grad2(hash: u32, x: f64, y: f64) -> f64 {
    let h = hash & 3;
    let u = if h < 2 { x } else { y };
    let v = if h < 2 { y } else { x };
    (if h & 1 == 0 { u } else { -u }) + (if h & 2 == 0 { v } else { -v })
}

fn hash_xy(x: i32, y: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(73856093) ^ (y as u32).wrapping_mul(19349663);
    h = h.wrapping_mul(2654435761);
    h ^= h >> 16;
    h
}

pub fn perlin_2d(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let y = arg_f64(args, 1).unwrap_or(0.0);
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let xf = x - xi as f64;
    let yf = y - yi as f64;
    let u = fade(xf);
    let v = fade(yf);
    let aa = perlin_grad2(hash_xy(xi, yi), xf, yf);
    let ab = perlin_grad2(hash_xy(xi, yi + 1), xf, yf - 1.0);
    let ba = perlin_grad2(hash_xy(xi + 1, yi), xf - 1.0, yf);
    let bb = perlin_grad2(hash_xy(xi + 1, yi + 1), xf - 1.0, yf - 1.0);
    StrykeValue::float(lerp(lerp(aa, ba, u), lerp(ab, bb, u), v))
}

pub fn perlin_3d(args: &[StrykeValue]) -> StrykeValue {
    // Use 2D perlin slice
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let y = arg_f64(args, 1).unwrap_or(0.0);
    let z = arg_f64(args, 2).unwrap_or(0.0);
    let a = perlin_2d(&[StrykeValue::float(x), StrykeValue::float(y)]).to_number();
    let b = perlin_2d(&[StrykeValue::float(y), StrykeValue::float(z)]).to_number();
    let c = perlin_2d(&[StrykeValue::float(x), StrykeValue::float(z)]).to_number();
    StrykeValue::float((a + b + c) / 3.0)
}

pub fn simplex_2d(args: &[StrykeValue]) -> StrykeValue {
    // Simplified 2D simplex noise
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let y = arg_f64(args, 1).unwrap_or(0.0);
    let f2 = 0.5 * (3.0_f64.sqrt() - 1.0);
    let g2 = (3.0 - 3.0_f64.sqrt()) / 6.0;
    let s = (x + y) * f2;
    let i = (x + s).floor();
    let j = (y + s).floor();
    let t = (i + j) * g2;
    let x0 = x - (i - t);
    let y0 = y - (j - t);
    let (i1, j1) = if x0 > y0 { (1, 0) } else { (0, 1) };
    let x1 = x0 - i1 as f64 + g2;
    let y1 = y0 - j1 as f64 + g2;
    let x2 = x0 - 1.0 + 2.0 * g2;
    let y2 = y0 - 1.0 + 2.0 * g2;
    let ii = i as i32;
    let jj = j as i32;
    let t0 = (0.5_f64 - x0 * x0 - y0 * y0).max(0.0);
    let t1 = (0.5_f64 - x1 * x1 - y1 * y1).max(0.0);
    let t2 = (0.5_f64 - x2 * x2 - y2 * y2).max(0.0);
    let n0 = t0.powi(4) * perlin_grad2(hash_xy(ii, jj), x0, y0);
    let n1 = t1.powi(4) * perlin_grad2(hash_xy(ii + i1, jj + j1), x1, y1);
    let n2 = t2.powi(4) * perlin_grad2(hash_xy(ii + 1, jj + 1), x2, y2);
    StrykeValue::float(70.0 * (n0 + n1 + n2))
}

pub fn worley_2d(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let y = arg_f64(args, 1).unwrap_or(0.0);
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let mut min_dist = f64::INFINITY;
    for di in -1..=1 {
        for dj in -1..=1 {
            let h = hash_xy(xi + di, yi + dj);
            let px = xi as f64 + di as f64 + (h & 0xFFFF) as f64 / 65536.0;
            let py = yi as f64 + dj as f64 + ((h >> 16) & 0xFFFF) as f64 / 65536.0;
            let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
            if d < min_dist {
                min_dist = d;
            }
        }
    }
    StrykeValue::float(min_dist)
}

pub fn value_noise_2d(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let y = arg_f64(args, 1).unwrap_or(0.0);
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let xf = x - xi as f64;
    let yf = y - yi as f64;
    let val = |a: i32, b: i32| (hash_xy(a, b) as f64 / u32::MAX as f64) * 2.0 - 1.0;
    let v00 = val(xi, yi);
    let v01 = val(xi, yi + 1);
    let v10 = val(xi + 1, yi);
    let v11 = val(xi + 1, yi + 1);
    let u = fade(xf);
    let v = fade(yf);
    StrykeValue::float(lerp(lerp(v00, v10, u), lerp(v01, v11, u), v))
}

pub fn fbm_noise_2d(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let y = arg_f64(args, 1).unwrap_or(0.0);
    let octaves = arg_i64(args, 2).unwrap_or(4).max(1) as usize;
    let lacunarity = arg_f64(args, 3).unwrap_or(2.0);
    let gain = arg_f64(args, 4).unwrap_or(0.5);
    let mut total = 0.0_f64;
    let mut freq = 1.0_f64;
    let mut amp = 1.0_f64;
    let mut max_amp = 0.0_f64;
    for _ in 0..octaves {
        total += perlin_2d(&[StrykeValue::float(x * freq), StrykeValue::float(y * freq)]).to_number() * amp;
        max_amp += amp;
        freq *= lacunarity;
        amp *= gain;
    }
    StrykeValue::float(total / max_amp.max(1e-12))
}

pub fn ridge_noise_2d(args: &[StrykeValue]) -> StrykeValue {
    let n = perlin_2d(args).to_number();
    StrykeValue::float(1.0 - n.abs())
}

pub fn turbulence_noise_2d(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    let y = arg_f64(args, 1).unwrap_or(0.0);
    let octaves = arg_i64(args, 2).unwrap_or(4).max(1) as usize;
    let mut total = 0.0_f64;
    let mut freq = 1.0_f64;
    let mut amp = 1.0_f64;
    for _ in 0..octaves {
        total += perlin_2d(&[StrykeValue::float(x * freq), StrykeValue::float(y * freq)])
            .to_number()
            .abs()
            * amp;
        freq *= 2.0;
        amp *= 0.5;
    }
    StrykeValue::float(total)
}

pub fn hash_2d_int(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_i64(args, 0).unwrap_or(0) as i32;
    let y = arg_i64(args, 1).unwrap_or(0) as i32;
    StrykeValue::integer(hash_xy(x, y) as i64)
}

// ══════════════════════════════════════════════════════════════════════
// RNG variants
// ══════════════════════════════════════════════════════════════════════

pub fn mulberry32_next(args: &[StrykeValue]) -> StrykeValue {
    let seed = arg_u64(args, 0).unwrap_or(0) as u32;
    let mut z = seed.wrapping_add(0x6D2B79F5);
    z = (z ^ (z >> 15)).wrapping_mul(z | 1);
    z = z.wrapping_add(z ^ z.wrapping_shr(7).wrapping_mul(z | 61));
    StrykeValue::integer((z ^ (z >> 14)) as i64)
}

pub fn xorshift32_next(args: &[StrykeValue]) -> StrykeValue {
    let mut s = arg_u64(args, 0).unwrap_or(1) as u32;
    if s == 0 {
        s = 1;
    }
    s ^= s << 13;
    s ^= s >> 17;
    s ^= s << 5;
    StrykeValue::integer(s as i64)
}

pub fn pcg32_next(args: &[StrykeValue]) -> StrykeValue {
    let state = arg_u64(args, 0).unwrap_or(0);
    let inc = arg_u64(args, 1).unwrap_or(1) | 1;
    let new_state = state.wrapping_mul(6364136223846793005).wrapping_add(inc);
    let xorshifted = ((new_state >> 18) ^ new_state) >> 27;
    let rot = (new_state >> 59) as u32;
    let result = (xorshifted as u32).rotate_right(rot);
    StrykeValue::integer(result as i64)
}

pub fn splitmix64_next(args: &[StrykeValue]) -> StrykeValue {
    let mut s = arg_u64(args, 0).unwrap_or(0);
    s = s.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = s;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^= z >> 31;
    StrykeValue::integer(z as i64)
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

    #[test]
    fn derangement_count_5() {
        assert_eq!(derangement_count(&[sv_i(5)]).to_int(), 44);
    }

    #[test]
    fn partitions_count_10() {
        assert_eq!(partitions_count(&[sv_i(10)]).to_int(), 42);
    }

    #[test]
    fn lattice_paths_3x3() {
        assert_eq!(lattice_paths(&[sv_i(3), sv_i(3)]).to_int(), 20);
    }

    #[test]
    fn primorial_10() {
        // 2 * 3 * 5 * 7 = 210
        assert_eq!(primorial(&[sv_i(10)]).to_int(), 210);
    }

    #[test]
    fn fibonacci_matrix_10() {
        assert_eq!(fibonacci_matrix(&[sv_i(10)]).to_int(), 55);
    }

    #[test]
    fn lucas_10() {
        // L(10) = 123
        assert_eq!(lucas_nth(&[sv_i(10)]).to_int(), 123);
    }

    #[test]
    fn hexagonal_n5() {
        // H(5) = 5*9 = 45
        assert_eq!(hexagonal_number(&[sv_i(5)]).to_int(), 45);
    }

    #[test]
    fn tetrahedral_n4() {
        // T(4) = 4*5*6/6 = 20
        assert_eq!(tetrahedral(&[sv_i(4)]).to_int(), 20);
    }

    #[test]
    fn elastic_collision_equal_masses() {
        // Equal masses swap velocities
        let r = elastic_collision_1d(&[sv(1.0), sv(2.0), sv(1.0), sv(0.0)]);
        let xs = as_vec_f64(&r);
        assert_eq!(xs, vec![0.0, 2.0]);
    }

    #[test]
    fn projectile_position_45deg() {
        let r = projectile_position(&[sv(10.0), sv(45.0), sv(9.81), sv(0.5)]);
        let xs = as_vec_f64(&r);
        assert!((xs[0] - 10.0 * 45f64.to_radians().cos() * 0.5).abs() < 1e-9);
    }

    #[test]
    fn moment_of_inertia_disc_unit() {
        // 0.5 * 1 * 1 = 0.5
        assert!((moment_of_inertia_disc(&[sv(1.0), sv(1.0)]).to_number() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn ternary_search_finds() {
        let xs = arr_i64(vec![1, 3, 5, 7, 9, 11, 13]);
        let r = ternary_search(&[xs, sv_i(9)]).to_int();
        assert_eq!(r, 4);
    }

    #[test]
    fn bidir_bfs_finds_path() {
        // 0 - 1 - 2 - 3
        let g = arr_sv(vec![
            arr_sv(vec![sv_i(1)]),
            arr_sv(vec![sv_i(0), sv_i(2)]),
            arr_sv(vec![sv_i(1), sv_i(3)]),
            arr_sv(vec![sv_i(2)]),
        ]);
        let r = bidirectional_bfs(&[g, sv_i(0), sv_i(3)]);
        let xs: Vec<i64> = as_vec_sv(&r).iter().map(|x| x.to_int()).collect();
        assert!(xs.contains(&0) && xs.contains(&3));
    }

    #[test]
    fn perlin_consistency() {
        let a = perlin_2d(&[sv(0.5), sv(0.5)]).to_number();
        let b = perlin_2d(&[sv(0.5), sv(0.5)]).to_number();
        assert_eq!(a, b);
    }

    #[test]
    fn fbm_finite() {
        let r = fbm_noise_2d(&[sv(0.5), sv(0.5), sv_i(4), sv(2.0), sv(0.5)]).to_number();
        assert!(r.is_finite());
    }

    #[test]
    fn am_synth_sample_count() {
        let r = am_synth(&[sv(440.0), sv(10.0), sv(0.5), sv(8000.0), sv(0.5)]);
        let n = as_vec_f64(&r).len();
        assert_eq!(n, 4000);
    }

    #[test]
    fn fir_filter_passthrough() {
        let r = fir_filter(&[arr_f64(vec![1.0, 2.0, 3.0]), arr_f64(vec![1.0])]);
        assert_eq!(as_vec_f64(&r), vec![1.0, 2.0, 3.0]);
    }
}
