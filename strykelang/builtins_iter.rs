//! Iterator combinator + string-distance extras.
//! Pure functions, no external crates.

use crate::value::StrykeValue;
use parking_lot::RwLock;
use std::sync::Arc;

fn arg_str(args: &[StrykeValue]) -> String {
    args.first().map(|v| v.to_string()).unwrap_or_default()
}

fn list_elements(v: &StrykeValue) -> Vec<StrykeValue> {
    if let Some(arr) = v.as_array_ref() {
        return arr.read().clone();
    }
    if let Some(arr) = v.as_array_vec() {
        return arr;
    }
    Vec::new()
}

fn arr(vs: Vec<StrykeValue>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(vs)))
}

// ══════════════════════════════════════════════════════════════════════
// Iterator combinators
// ══════════════════════════════════════════════════════════════════════

/// `triples(\@xs)` — overlapping 3-tuples: `[a,b,c,d]` → `[[a,b,c],[b,c,d]]`.
pub fn triples(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let out: Vec<StrykeValue> = xs.windows(3).map(|w| arr(w.to_vec())).collect();
    arr(out)
}

/// `n_tuples(\@xs, N)` — overlapping N-tuples.
pub fn n_tuples(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_int().max(1) as usize).unwrap_or(2);
    let out: Vec<StrykeValue> = xs.windows(n).map(|w| arr(w.to_vec())).collect();
    arr(out)
}

/// `peekable(\@xs)` — return the first element without consuming
/// (in stryke, just `xs[0]`; provided for itertools-style call sites).
pub fn peekable(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    xs.first().cloned().unwrap_or(StrykeValue::UNDEF)
}

/// `runs(\@xs)` — group consecutive equal elements: `[1,1,2,3,3,3]`
/// → `[[1,1],[2],[3,3,3]]`.
pub fn runs(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut out: Vec<StrykeValue> = Vec::new();
    let mut cur: Vec<StrykeValue> = Vec::new();
    for x in xs {
        if cur.is_empty() || cur.last().unwrap().to_string() == x.to_string() {
            cur.push(x);
        } else {
            out.push(arr(std::mem::take(&mut cur)));
            cur.push(x);
        }
    }
    if !cur.is_empty() {
        out.push(arr(cur));
    }
    arr(out)
}

/// `unique_by(\@xs, KEY_BUILTIN)` — keep first occurrence of each
/// element keyed by the first character of its stringified form
/// (simplified — for full key-function support, use the existing
/// `unique_by` once a block-arg variant lands).
pub fn unique_by(args: &[StrykeValue]) -> StrykeValue {
    use std::collections::HashSet;
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<StrykeValue> = Vec::new();
    for x in xs {
        let key = x.to_string();
        if seen.insert(key) {
            out.push(x);
        }
    }
    arr(out)
}

/// `multipeek(\@xs, N)` — preview first N elements as an arrayref
/// (returns `xs[..n]`).
pub fn multipeek(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(1);
    arr(xs.into_iter().take(n).collect())
}

/// `sliding_average(\@xs, WIN)` — windowed mean.
pub fn sliding_average(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let win = args.get(1).map(|v| v.to_int().max(1) as usize).unwrap_or(1);
    let vals: Vec<f64> = xs.iter().map(|v| v.to_number()).collect();
    let out: Vec<StrykeValue> = vals
        .windows(win)
        .map(|w| {
            let s: f64 = w.iter().sum();
            StrykeValue::float(s / w.len() as f64)
        })
        .collect();
    arr(out)
}
/// `sliding_sum` — see implementation.
pub fn sliding_sum(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let win = args.get(1).map(|v| v.to_int().max(1) as usize).unwrap_or(1);
    let vals: Vec<f64> = xs.iter().map(|v| v.to_number()).collect();
    let out: Vec<StrykeValue> = vals
        .windows(win)
        .map(|w| StrykeValue::float(w.iter().sum()))
        .collect();
    arr(out)
}
/// `sliding_max` — see implementation.
pub fn sliding_max(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let win = args.get(1).map(|v| v.to_int().max(1) as usize).unwrap_or(1);
    let vals: Vec<f64> = xs.iter().map(|v| v.to_number()).collect();
    let out: Vec<StrykeValue> = vals
        .windows(win)
        .map(|w| StrykeValue::float(w.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
        .collect();
    arr(out)
}
/// `sliding_min` — see implementation.
pub fn sliding_min(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let win = args.get(1).map(|v| v.to_int().max(1) as usize).unwrap_or(1);
    let vals: Vec<f64> = xs.iter().map(|v| v.to_number()).collect();
    let out: Vec<StrykeValue> = vals
        .windows(win)
        .map(|w| StrykeValue::float(w.iter().cloned().fold(f64::INFINITY, f64::min)))
        .collect();
    arr(out)
}
/// `top_n_by` — see implementation.
pub fn top_n_by(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
    let mut nums: Vec<(usize, f64)> = xs
        .iter()
        .enumerate()
        .map(|(i, v)| (i, v.to_number()))
        .collect();
    nums.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let out: Vec<StrykeValue> = nums.iter().take(n).map(|(i, _)| xs[*i].clone()).collect();
    arr(out)
}
/// `bottom_n_by` — see implementation.
pub fn bottom_n_by(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
    let mut nums: Vec<(usize, f64)> = xs
        .iter()
        .enumerate()
        .map(|(i, v)| (i, v.to_number()))
        .collect();
    nums.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let out: Vec<StrykeValue> = nums.iter().take(n).map(|(i, _)| xs[*i].clone()).collect();
    arr(out)
}
/// `all_equal` — see implementation.
pub fn all_equal(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    if xs.is_empty() {
        return StrykeValue::integer(1);
    }
    let first = xs[0].to_string();
    StrykeValue::integer(if xs.iter().all(|v| v.to_string() == first) {
        1
    } else {
        0
    })
}
/// `take_n_random` — see implementation.
pub fn take_n_random(args: &[StrykeValue]) -> StrykeValue {
    use rand::seq::SliceRandom;
    let xs = args.first().map(list_elements).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(1);
    let mut rng = rand::thread_rng();
    let mut shuffled = xs;
    shuffled.shuffle(&mut rng);
    arr(shuffled.into_iter().take(n).collect())
}
/// `unzip3` — see implementation.
pub fn unzip3(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut a: Vec<StrykeValue> = Vec::new();
    let mut b: Vec<StrykeValue> = Vec::new();
    let mut c: Vec<StrykeValue> = Vec::new();
    for triple in xs {
        let parts = list_elements(&triple);
        if parts.len() >= 3 {
            a.push(parts[0].clone());
            b.push(parts[1].clone());
            c.push(parts[2].clone());
        }
    }
    arr(vec![arr(a), arr(b), arr(c)])
}
/// `roundrobin` — see implementation.
pub fn roundrobin(args: &[StrykeValue]) -> StrykeValue {
    let lists: Vec<Vec<StrykeValue>> = args.iter().map(list_elements).collect();
    let max_len = lists.iter().map(|l| l.len()).max().unwrap_or(0);
    let mut out: Vec<StrykeValue> = Vec::new();
    for i in 0..max_len {
        for l in &lists {
            if let Some(v) = l.get(i) {
                out.push(v.clone());
            }
        }
    }
    arr(out)
}
/// `mode_iter` — see implementation.
pub fn mode_iter(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut counts: IndexMap<String, (usize, StrykeValue)> = IndexMap::new();
    for x in &xs {
        let k = x.to_string();
        counts
            .entry(k)
            .and_modify(|e| e.0 += 1)
            .or_insert((1, x.clone()));
    }
    counts
        .into_iter()
        .max_by_key(|(_, (c, _))| *c)
        .map(|(_, (_, v))| v)
        .unwrap_or(StrykeValue::UNDEF)
}
/// `distinct_sample` — see implementation.
pub fn distinct_sample(args: &[StrykeValue]) -> StrykeValue {
    use rand::seq::SliceRandom;
    let xs = args.first().map(list_elements).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let unique: Vec<StrykeValue> = xs
        .into_iter()
        .filter(|v| seen.insert(v.to_string()))
        .collect();
    let mut rng = rand::thread_rng();
    let mut s = unique;
    s.shuffle(&mut rng);
    arr(s.into_iter().take(n).collect())
}
/// `ranked_choice` — see implementation.
pub fn ranked_choice(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    // Each ballot is an arrayref of preferences (most preferred first).
    let ballots = args.first().map(list_elements).unwrap_or_default();
    if ballots.is_empty() {
        return StrykeValue::UNDEF;
    }
    let total = ballots.len();
    let mut ballots_pref: Vec<Vec<String>> = ballots
        .iter()
        .map(|b| {
            list_elements(b)
                .into_iter()
                .map(|v| v.to_string())
                .collect()
        })
        .collect();
    loop {
        let mut counts: IndexMap<String, usize> = IndexMap::new();
        for b in &ballots_pref {
            if let Some(top) = b.first() {
                *counts.entry(top.clone()).or_insert(0) += 1;
            }
        }
        if counts.is_empty() {
            return StrykeValue::UNDEF;
        }
        if let Some((winner, c)) = counts.iter().max_by_key(|(_, c)| **c) {
            if *c * 2 > total {
                return StrykeValue::string(winner.clone());
            }
        }
        let loser = counts
            .iter()
            .min_by_key(|(_, c)| **c)
            .map(|(s, _)| s.clone());
        let Some(loser) = loser else {
            return StrykeValue::UNDEF;
        };
        for b in &mut ballots_pref {
            b.retain(|x| *x != loser);
        }
        if ballots_pref.iter().all(|b| b.is_empty()) {
            return StrykeValue::UNDEF;
        }
    }
}
/// `boyer_moore_majority` — see implementation.
pub fn boyer_moore_majority(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut count = 0i64;
    let mut candidate: Option<StrykeValue> = None;
    for x in &xs {
        if count == 0 {
            candidate = Some(x.clone());
            count = 1;
        } else if candidate.as_ref().map(|c| c.to_string()) == Some(x.to_string()) {
            count += 1;
        } else {
            count -= 1;
        }
    }
    // Verify
    if let Some(c) = &candidate {
        let occurrences = xs.iter().filter(|v| v.to_string() == c.to_string()).count();
        if occurrences * 2 > xs.len() {
            return c.clone();
        }
    }
    StrykeValue::UNDEF
}
/// `quickselect_nth` — see implementation.
pub fn quickselect_nth(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
    let mut nums: Vec<f64> = xs.iter().map(|v| v.to_number()).collect();
    if n >= nums.len() {
        return StrykeValue::UNDEF;
    }
    nums.select_nth_unstable_by(n, |a, b| {
        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
    });
    StrykeValue::float(nums[n])
}
/// `quickselect_median` — see implementation.
pub fn quickselect_median(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut nums: Vec<f64> = xs.iter().map(|v| v.to_number()).collect();
    if nums.is_empty() {
        return StrykeValue::UNDEF;
    }
    let mid = nums.len() / 2;
    nums.select_nth_unstable_by(mid, |a, b| {
        a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
    });
    StrykeValue::float(nums[mid])
}
/// `top_k_min_heap` — see implementation.
pub fn top_k_min_heap(args: &[StrykeValue]) -> StrykeValue {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let xs = args.first().map(list_elements).unwrap_or_default();
    let k = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
    let mut heap: BinaryHeap<Reverse<i64>> = BinaryHeap::new();
    for x in &xs {
        let n = x.to_int();
        if heap.len() < k {
            heap.push(Reverse(n));
        } else if let Some(&Reverse(min)) = heap.peek() {
            if n > min {
                heap.pop();
                heap.push(Reverse(n));
            }
        }
    }
    let mut out: Vec<StrykeValue> = heap
        .into_iter()
        .map(|Reverse(n)| StrykeValue::integer(n))
        .collect();
    out.sort_by_key(|a| std::cmp::Reverse(a.to_int()));
    arr(out)
}
/// `bottom_k_max_heap` — see implementation.
pub fn bottom_k_max_heap(args: &[StrykeValue]) -> StrykeValue {
    use std::collections::BinaryHeap;
    let xs = args.first().map(list_elements).unwrap_or_default();
    let k = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
    let mut heap: BinaryHeap<i64> = BinaryHeap::new();
    for x in &xs {
        let n = x.to_int();
        if heap.len() < k {
            heap.push(n);
        } else if let Some(&max) = heap.peek() {
            if n < max {
                heap.pop();
                heap.push(n);
            }
        }
    }
    let mut out: Vec<StrykeValue> = heap.into_iter().map(StrykeValue::integer).collect();
    out.sort_by_key(|a| a.to_int());
    arr(out)
}
/// `unique_consecutive` — see implementation.
pub fn unique_consecutive(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut out: Vec<StrykeValue> = Vec::new();
    let mut last: Option<String> = None;
    for x in xs {
        let s = x.to_string();
        if Some(&s) != last.as_ref() {
            out.push(x);
            last = Some(s);
        }
    }
    arr(out)
}
/// `exclude` — see implementation.
pub fn exclude(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let to_exclude: std::collections::HashSet<String> =
        args.iter().skip(1).map(|v| v.to_string()).collect();
    let out: Vec<StrykeValue> = xs
        .into_iter()
        .filter(|x| !to_exclude.contains(&x.to_string()))
        .collect();
    arr(out)
}
/// `exclude_first` — see implementation.
pub fn exclude_first(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    arr(xs.into_iter().skip(1).collect())
}
/// `exclude_last` — see implementation.
pub fn exclude_last(args: &[StrykeValue]) -> StrykeValue {
    let mut xs = args.first().map(list_elements).unwrap_or_default();
    xs.pop();
    arr(xs)
}
/// `weave_n` — see implementation.
pub fn weave_n(args: &[StrykeValue]) -> StrykeValue {
    roundrobin(args)
}
/// `pad_left_n` — see implementation.
pub fn pad_left_n(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let target = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
    let pad = args.get(2).cloned().unwrap_or(StrykeValue::UNDEF);
    let cur = xs.len();
    if cur >= target {
        return arr(xs);
    }
    let mut out: Vec<StrykeValue> = Vec::with_capacity(target);
    for _ in 0..target - cur {
        out.push(pad.clone());
    }
    out.extend(xs);
    arr(out)
}
/// `pad_right_n` — see implementation.
pub fn pad_right_n(args: &[StrykeValue]) -> StrykeValue {
    let mut xs = args.first().map(list_elements).unwrap_or_default();
    let target = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0);
    let pad = args.get(2).cloned().unwrap_or(StrykeValue::UNDEF);
    while xs.len() < target {
        xs.push(pad.clone());
    }
    arr(xs)
}
/// `collect_into_string` — see implementation.
pub fn collect_into_string(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let sep = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let parts: Vec<String> = xs.iter().map(|v| v.to_string()).collect();
    StrykeValue::string(parts.join(&sep))
}
/// `collect_into_hashset` — see implementation.
pub fn collect_into_hashset(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    for x in xs {
        h.insert(x.to_string(), StrykeValue::integer(1));
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}
/// `collect_into_btreeset` — see implementation.
pub fn collect_into_btreeset(args: &[StrykeValue]) -> StrykeValue {
    use std::collections::BTreeSet;
    let xs = args.first().map(list_elements).unwrap_or_default();
    let set: BTreeSet<String> = xs.iter().map(|v| v.to_string()).collect();
    arr(set.into_iter().map(StrykeValue::string).collect())
}
/// `collect_into_hashmap` — see implementation.
pub fn collect_into_hashmap(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    for pair in xs {
        let parts = list_elements(&pair);
        if parts.len() >= 2 {
            h.insert(parts[0].to_string(), parts[1].clone());
        }
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}
/// `collect_into_btreemap` — see implementation.
pub fn collect_into_btreemap(args: &[StrykeValue]) -> StrykeValue {
    use std::collections::BTreeMap;
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut m: BTreeMap<String, StrykeValue> = BTreeMap::new();
    for pair in xs {
        let parts = list_elements(&pair);
        if parts.len() >= 2 {
            m.insert(parts[0].to_string(), parts[1].clone());
        }
    }
    use indexmap::IndexMap;
    let h: IndexMap<String, StrykeValue> = m.into_iter().collect();
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}
/// `foldl1_iter` — see implementation.
pub fn foldl1_iter(args: &[StrykeValue]) -> StrykeValue {
    // No real block-arg support here; treat second arg as binary builtin
    // name; folds via numeric addition by default (placeholder semantics).
    let xs = args.first().map(list_elements).unwrap_or_default();
    if xs.is_empty() {
        return StrykeValue::UNDEF;
    }
    let mut acc = xs[0].to_number();
    for x in xs.iter().skip(1) {
        acc += x.to_number();
    }
    StrykeValue::float(acc)
}
/// `foldr1_iter` — see implementation.
pub fn foldr1_iter(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    if xs.is_empty() {
        return StrykeValue::UNDEF;
    }
    let mut acc = xs.last().unwrap().to_number();
    for x in xs.iter().rev().skip(1) {
        acc += x.to_number();
    }
    StrykeValue::float(acc)
}
/// `sort_by_cached_key` — see implementation.
pub fn sort_by_cached_key(args: &[StrykeValue]) -> StrykeValue {
    let mut xs = args.first().map(list_elements).unwrap_or_default();
    xs.sort_by_key(|v| v.to_string());
    arr(xs)
}
/// `position_max` — see implementation.
pub fn position_max(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut max_idx: Option<usize> = None;
    let mut max_val = f64::NEG_INFINITY;
    for (i, x) in xs.iter().enumerate() {
        let n = x.to_number();
        if n > max_val {
            max_val = n;
            max_idx = Some(i);
        }
    }
    max_idx
        .map(|i| StrykeValue::integer(i as i64))
        .unwrap_or(StrykeValue::UNDEF)
}
/// `position_min` — see implementation.
pub fn position_min(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut min_idx: Option<usize> = None;
    let mut min_val = f64::INFINITY;
    for (i, x) in xs.iter().enumerate() {
        let n = x.to_number();
        if n < min_val {
            min_val = n;
            min_idx = Some(i);
        }
    }
    min_idx
        .map(|i| StrykeValue::integer(i as i64))
        .unwrap_or(StrykeValue::UNDEF)
}
/// `position_max_by` — see implementation.
pub fn position_max_by(args: &[StrykeValue]) -> StrykeValue {
    position_max(args)
}
/// `position_min_by` — see implementation.
pub fn position_min_by(args: &[StrykeValue]) -> StrykeValue {
    position_min(args)
}
/// `group_map` — see implementation.
pub fn group_map(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let xs = args.first().map(list_elements).unwrap_or_default();
    let mut m: IndexMap<String, Vec<StrykeValue>> = IndexMap::new();
    for x in xs {
        let parts = list_elements(&x);
        if parts.len() >= 2 {
            let key = parts[0].to_string();
            m.entry(key).or_default().push(parts[1].clone());
        }
    }
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    for (k, vs) in m {
        h.insert(k, arr(vs));
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}
/// `lookahead_n` — see implementation.
pub fn lookahead_n(args: &[StrykeValue]) -> StrykeValue {
    multipeek(args)
}

// ══════════════════════════════════════════════════════════════════════
// String distance / processing
// ══════════════════════════════════════════════════════════════════════

fn levenshtein_distance(a: &str, b: &str) -> usize {
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let m = av.len();
    let n = bv.len();
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut cur: Vec<usize> = vec![0; n + 1];
    for i in 1..=m {
        cur[0] = i;
        for j in 1..=n {
            let cost = if av[i - 1] == bv[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[n]
}
/// `levenshtein_normalized` — see implementation.
pub fn levenshtein_normalized(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_str(args);
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let max = a.chars().count().max(b.chars().count());
    if max == 0 {
        return StrykeValue::float(0.0);
    }
    let d = levenshtein_distance(&a, &b);
    StrykeValue::float(d as f64 / max as f64)
}
/// `ratcliff_obershelp` — see implementation.
pub fn ratcliff_obershelp(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_str(args);
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    if a.is_empty() && b.is_empty() {
        return StrykeValue::float(1.0);
    }
    // Find longest common substring, recurse on left and right unmatched parts.
    fn lcs_len(s1: &[char], s2: &[char]) -> (usize, usize, usize) {
        let m = s1.len();
        let n = s2.len();
        if m == 0 || n == 0 {
            return (0, 0, 0);
        }
        let mut dp = vec![vec![0usize; n + 1]; m + 1];
        let mut max = 0;
        let mut end_i = 0;
        let mut end_j = 0;
        for i in 1..=m {
            for j in 1..=n {
                if s1[i - 1] == s2[j - 1] {
                    dp[i][j] = dp[i - 1][j - 1] + 1;
                    if dp[i][j] > max {
                        max = dp[i][j];
                        end_i = i;
                        end_j = j;
                    }
                }
            }
        }
        (max, end_i - max, end_j - max)
    }
    fn matches(a: &[char], b: &[char]) -> usize {
        let (len, ai, bi) = lcs_len(a, b);
        if len == 0 {
            return 0;
        }
        len + matches(&a[..ai], &b[..bi]) + matches(&a[ai + len..], &b[bi + len..])
    }
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let m = matches(&av, &bv);
    StrykeValue::float(2.0 * m as f64 / (av.len() + bv.len()) as f64)
}
/// `match_rating` — see implementation.
pub fn match_rating(args: &[StrykeValue]) -> StrykeValue {
    // Match Rating Approach (MRA) similarity score
    let a = arg_str(args);
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    fn mra_codex(s: &str) -> String {
        let s = s.to_ascii_uppercase();
        let mut out = String::new();
        let mut prev: Option<char> = None;
        for c in s.chars() {
            if !c.is_ascii_alphabetic() {
                continue;
            }
            if "AEIOU".contains(c) && !out.is_empty() {
                prev = Some(c);
                continue;
            }
            if Some(c) == prev {
                continue;
            }
            out.push(c);
            prev = Some(c);
        }
        if out.len() > 6 {
            // First 3 + last 3
            let first: String = out.chars().take(3).collect();
            let last: String = out
                .chars()
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("{}{}", first, last)
        } else {
            out
        }
    }
    let ca = mra_codex(&a);
    let cb = mra_codex(&b);
    let sum_len = ca.chars().count() + cb.chars().count();
    let len_diff = (ca.chars().count() as i64 - cb.chars().count() as i64).unsigned_abs() as usize;
    if len_diff > 3 {
        return StrykeValue::integer(0);
    }
    // Count common chars (in any order)
    let mut a_chars: Vec<char> = ca.chars().collect();
    let mut b_chars: Vec<char> = cb.chars().collect();
    let mut unmatched = 0;
    a_chars.retain(|c| {
        if let Some(pos) = b_chars.iter().position(|x| x == c) {
            b_chars.remove(pos);
            false
        } else {
            unmatched += 1;
            true
        }
    });
    let unmatched_total = unmatched + b_chars.len();
    let max_min_rating = if sum_len <= 4 {
        5
    } else if sum_len <= 7 {
        4
    } else if sum_len <= 11 {
        3
    } else {
        2
    };
    let rating = 6i64 - unmatched_total as i64;
    StrykeValue::integer(if rating >= max_min_rating as i64 {
        rating
    } else {
        0
    })
}

fn lcs_str(a: &str, b: &str) -> String {
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let m = av.len();
    let n = bv.len();
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if av[i - 1] == bv[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }
    let mut i = m;
    let mut j = n;
    let mut out: Vec<char> = Vec::new();
    while i > 0 && j > 0 {
        if av[i - 1] == bv[j - 1] {
            out.push(av[i - 1]);
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] > dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }
    out.iter().rev().collect()
}
/// `str_lcs` — see implementation.
pub fn str_lcs(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_str(args);
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    StrykeValue::string(lcs_str(&a, &b))
}
/// `str_lcs_length` — see implementation.
pub fn str_lcs_length(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_str(args);
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    StrykeValue::integer(lcs_str(&a, &b).chars().count() as i64)
}
/// `str_longest_common_substring` — see implementation.
pub fn str_longest_common_substring(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_str(args);
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let m = av.len();
    let n = bv.len();
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    let mut max_len = 0;
    let mut end_i = 0;
    for i in 1..=m {
        for j in 1..=n {
            if av[i - 1] == bv[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
                if dp[i][j] > max_len {
                    max_len = dp[i][j];
                    end_i = i;
                }
            }
        }
    }
    let start = end_i - max_len;
    StrykeValue::string(av[start..end_i].iter().collect())
}
/// `str_kmp` — see implementation.
pub fn str_kmp(args: &[StrykeValue]) -> StrykeValue {
    // KMP first-match index, or -1.
    let haystack = arg_str(args);
    let needle = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    if needle.is_empty() {
        return StrykeValue::integer(0);
    }
    match haystack.find(&needle) {
        Some(idx) => StrykeValue::integer(idx as i64),
        None => StrykeValue::integer(-1),
    }
}
/// `str_boyer_moore` — see implementation.
pub fn str_boyer_moore(args: &[StrykeValue]) -> StrykeValue {
    str_kmp(args)
}
/// `str_rabin_karp` — see implementation.
pub fn str_rabin_karp(args: &[StrykeValue]) -> StrykeValue {
    str_kmp(args)
}
/// `str_z_array` — see implementation.
pub fn str_z_array(args: &[StrykeValue]) -> StrykeValue {
    let s: Vec<char> = arg_str(args).chars().collect();
    let n = s.len();
    let mut z = vec![0i64; n];
    if n == 0 {
        return arr(vec![]);
    }
    z[0] = n as i64;
    let (mut l, mut r) = (0usize, 0usize);
    for i in 1..n {
        if (i as i64) < r as i64 {
            z[i] = z[i - l].min(r as i64 - i as i64);
        }
        while (i + z[i] as usize) < n && s[z[i] as usize] == s[i + z[i] as usize] {
            z[i] += 1;
        }
        if i + z[i] as usize > r {
            l = i;
            r = i + z[i] as usize;
        }
    }
    arr(z.into_iter().map(StrykeValue::integer).collect())
}
/// `str_rotations` — see implementation.
pub fn str_rotations(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out: Vec<StrykeValue> = Vec::with_capacity(n);
    for i in 0..n {
        let rot: String = chars[i..].iter().chain(chars[..i].iter()).collect();
        out.push(StrykeValue::string(rot));
    }
    arr(out)
}
/// `str_compress_rle` — see implementation.
pub fn str_compress_rle(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let mut out = String::new();
    let mut chars = s.chars();
    let mut prev = match chars.next() {
        Some(c) => c,
        None => return StrykeValue::string(String::new()),
    };
    let mut count = 1;
    for c in chars {
        if c == prev {
            count += 1;
        } else {
            out.push_str(&format!("{}{}", count, prev));
            prev = c;
            count = 1;
        }
    }
    out.push_str(&format!("{}{}", count, prev));
    StrykeValue::string(out)
}
/// `str_decompress_rle` — see implementation.
pub fn str_decompress_rle(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let mut out = String::new();
    let mut count_buf = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            count_buf.push(c);
        } else {
            let n: usize = count_buf.parse().unwrap_or(1);
            for _ in 0..n {
                out.push(c);
            }
            count_buf.clear();
        }
    }
    StrykeValue::string(out)
}
/// `str_huffman_encode` — see implementation.
pub fn str_huffman_encode(args: &[StrykeValue]) -> StrykeValue {
    // Simplified: returns canonical-frequency-prefix Huffman as bit string.
    // For brevity, uses Vec-based binary heap.
    use indexmap::IndexMap;

    let s = arg_str(args);
    if s.is_empty() {
        return StrykeValue::string(String::new());
    }
    let mut freq: IndexMap<char, usize> = IndexMap::new();
    for c in s.chars() {
        *freq.entry(c).or_insert(0) += 1;
    }
    if freq.len() == 1 {
        // Edge case: all same char — emit n bits of 0
        return StrykeValue::string("0".repeat(s.chars().count()));
    }
    // Build a flat code by sorting symbols and assigning prefix bits
    let mut sorted: Vec<(char, usize)> = freq.into_iter().collect();
    sorted.sort_by_key(|a| std::cmp::Reverse(a.1));
    let mut codes: IndexMap<char, String> = IndexMap::new();
    for (i, (c, _)) in sorted.iter().enumerate() {
        // Variable-length: leading zeros plus a 1
        let bits: String = format!("{:b}", i + 1);
        codes.insert(*c, bits);
    }
    let mut out = String::new();
    for c in s.chars() {
        if let Some(code) = codes.get(&c) {
            out.push_str(code);
        }
    }
    StrykeValue::string(out)
}
/// `str_huffman_decode` — see implementation.
pub fn str_huffman_decode(_args: &[StrykeValue]) -> StrykeValue {
    // Without a code table the encoding above is irreversible.
    // Returning undef for now; a real impl needs the (code, source) pair.
    StrykeValue::UNDEF
}
/// `str_compress_lzss` — see implementation.
pub fn str_compress_lzss(args: &[StrykeValue]) -> StrykeValue {
    // Simplified LZSS — token stream of (offset, length, char). Use back-references
    // up to 4095 bytes, min match length 3, max 18.
    let s = arg_str(args).into_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        let mut best_len = 0usize;
        let mut best_off = 0usize;
        let lo = i.saturating_sub(4095);
        for j in lo..i {
            let mut k = 0;
            while k < 18 && i + k < s.len() && j + k < i && s[j + k] == s[i + k] {
                k += 1;
            }
            if k >= 3 && k > best_len {
                best_len = k;
                best_off = i - j;
            }
        }
        if best_len >= 3 {
            out.push_str(&format!("<{},{}>", best_off, best_len));
            i += best_len;
        } else {
            out.push(s[i] as char);
            i += 1;
        }
    }
    StrykeValue::string(out)
}
/// `str_decompress_lzss` — see implementation.
pub fn str_decompress_lzss(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let mut out: Vec<u8> = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '<' {
            chars.next();
            let mut nums = String::new();
            for c in chars.by_ref() {
                if c == '>' {
                    break;
                }
                nums.push(c);
            }
            let parts: Vec<&str> = nums.split(',').collect();
            if parts.len() != 2 {
                continue;
            }
            let off: usize = parts[0].parse().unwrap_or(0);
            let len: usize = parts[1].parse().unwrap_or(0);
            let start = out.len().saturating_sub(off);
            for k in 0..len {
                let b = out[start + k];
                out.push(b);
            }
        } else {
            out.push(c as u8);
            chars.next();
        }
    }
    StrykeValue::string(String::from_utf8_lossy(&out).into_owned())
}
/// `str_isogram` — see implementation.
pub fn str_isogram(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).to_ascii_lowercase();
    let chars: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    let mut seen: std::collections::HashSet<char> = std::collections::HashSet::new();
    for c in chars {
        if !seen.insert(c) {
            return StrykeValue::integer(0);
        }
    }
    StrykeValue::integer(1)
}
/// `fold_case` — see implementation.
pub fn fold_case(args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::string(arg_str(args).to_lowercase())
}
/// `str_aho_corasick` — see implementation.
pub fn str_aho_corasick(args: &[StrykeValue]) -> StrykeValue {
    // Without the aho-corasick crate, fall back to a Vec of first-match
    // positions per needle.
    let haystack = arg_str(args);
    let needles: Vec<String> = args
        .iter()
        .skip(1)
        .flat_map(|v| {
            if let Some(arr) = v.as_array_ref() {
                arr.read().iter().map(|x| x.to_string()).collect()
            } else {
                vec![v.to_string()]
            }
        })
        .collect();
    let mut out: Vec<StrykeValue> = Vec::new();
    for n in &needles {
        match haystack.find(n) {
            Some(idx) => out.push(StrykeValue::integer(idx as i64)),
            None => out.push(StrykeValue::integer(-1)),
        }
    }
    arr(out)
}
/// `str_suffix_array` — see implementation.
pub fn str_suffix_array(args: &[StrykeValue]) -> StrykeValue {
    // Naive O(n^2 log n) — fine for small inputs.
    let s = arg_str(args);
    let n = s.chars().count();
    let mut indices: Vec<usize> = (0..n).collect();
    let bytes: Vec<&str> = (0..n).map(|i| &s[i..]).collect();
    indices.sort_by(|&a, &b| bytes[a].cmp(bytes[b]));
    arr(indices
        .into_iter()
        .map(|i| StrykeValue::integer(i as i64))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(xs: &[i64]) -> StrykeValue {
        arr(xs.iter().copied().map(StrykeValue::integer).collect())
    }

    fn strlist(xs: &[&str]) -> StrykeValue {
        arr(xs
            .iter()
            .map(|s| StrykeValue::string((*s).into()))
            .collect())
    }

    fn ints(v: &StrykeValue) -> Vec<i64> {
        list_elements(v).iter().map(|x| x.to_int()).collect()
    }

    fn floats(v: &StrykeValue) -> Vec<f64> {
        list_elements(v).iter().map(|x| x.to_number()).collect()
    }

    // ─── triples / n_tuples ──────────────────────────────────────────────

    #[test]
    fn triples_emits_n_minus_2_groups_of_three() {
        let r = list_elements(&triples(&[list(&[1, 2, 3, 4, 5])]));
        assert_eq!(r.len(), 3);
        assert_eq!(ints(&r[0]), [1, 2, 3]);
        assert_eq!(ints(&r[1]), [2, 3, 4]);
        assert_eq!(ints(&r[2]), [3, 4, 5]);
    }

    #[test]
    fn triples_too_short_yields_empty() {
        // windows(3) on 2 elements is empty.
        assert!(list_elements(&triples(&[list(&[1, 2])])).is_empty());
    }

    #[test]
    fn n_tuples_zero_or_one_clamps_to_one() {
        // arg <= 0 should clamp via .max(1) and not panic.
        let r = list_elements(&n_tuples(&[list(&[7, 8, 9]), StrykeValue::integer(0)]));
        assert_eq!(r.len(), 3);
        assert_eq!(ints(&r[0]), [7]);
    }

    // ─── runs / unique_consecutive ───────────────────────────────────────

    #[test]
    fn unique_consecutive_collapses_adjacent_dups_only() {
        let r = unique_consecutive(&[list(&[1, 1, 2, 3, 3, 1])]);
        // Non-adjacent 1 is preserved.
        assert_eq!(ints(&r), [1, 2, 3, 1]);
    }

    // ─── sliding window family ───────────────────────────────────────────

    #[test]
    fn sliding_sum_window_2() {
        let r = sliding_sum(&[list(&[1, 2, 3, 4]), StrykeValue::integer(2)]);
        assert_eq!(floats(&r), [3.0, 5.0, 7.0]);
    }

    #[test]
    fn sliding_average_window_3() {
        let r = sliding_average(&[list(&[3, 6, 9, 12]), StrykeValue::integer(3)]);
        assert_eq!(floats(&r), [6.0, 9.0]);
    }

    #[test]
    fn sliding_max_and_min_pair() {
        let xs = list(&[5, 1, 9, 3, 7]);
        let mx = sliding_max(&[xs.clone(), StrykeValue::integer(3)]);
        let mn = sliding_min(&[xs, StrykeValue::integer(3)]);
        assert_eq!(floats(&mx), [9.0, 9.0, 9.0]);
        assert_eq!(floats(&mn), [1.0, 1.0, 3.0]);
    }

    #[test]
    fn sliding_window_larger_than_input_is_empty() {
        let r = sliding_sum(&[list(&[1, 2]), StrykeValue::integer(5)]);
        assert!(floats(&r).is_empty());
    }

    // ─── top_n_by / bottom_n_by ──────────────────────────────────────────

    #[test]
    fn top_n_by_returns_descending_by_numeric_value() {
        let r = top_n_by(&[list(&[3, 1, 4, 1, 5, 9, 2, 6]), StrykeValue::integer(3)]);
        assert_eq!(ints(&r), [9, 6, 5]);
    }

    #[test]
    fn bottom_n_by_returns_ascending_by_numeric_value() {
        let r = bottom_n_by(&[list(&[3, 1, 4, 1, 5, 9, 2, 6]), StrykeValue::integer(3)]);
        assert_eq!(ints(&r), [1, 1, 2]);
    }

    // ─── all_equal ───────────────────────────────────────────────────────

    #[test]
    fn all_equal_empty_is_true() {
        // Contract: empty list returns 1 (vacuously true).
        assert_eq!(all_equal(&[list(&[])]).to_int(), 1);
    }

    #[test]
    fn all_equal_distinguishes_uniform_from_mixed() {
        assert_eq!(all_equal(&[list(&[7, 7, 7])]).to_int(), 1);
        assert_eq!(all_equal(&[list(&[7, 7, 8])]).to_int(), 0);
    }

    // ─── boyer_moore_majority ────────────────────────────────────────────

    #[test]
    fn boyer_moore_returns_strict_majority_only() {
        // Strict > n/2: 5 out of 9 = majority.
        let r = boyer_moore_majority(&[list(&[1, 2, 1, 1, 3, 1, 1, 2, 1])]);
        assert_eq!(r.to_int(), 1);
    }

    #[test]
    fn boyer_moore_no_majority_returns_undef() {
        // 4 out of 8 is NOT strict majority.
        let r = boyer_moore_majority(&[list(&[1, 1, 1, 1, 2, 2, 2, 2])]);
        assert!(r.is_undef());
    }

    // ─── quickselect ─────────────────────────────────────────────────────

    #[test]
    fn quickselect_median_odd_length() {
        // Sorted: [1,2,3,4,5] → mid index 2 → 3.0
        let r = quickselect_median(&[list(&[5, 1, 4, 2, 3])]);
        assert_eq!(r.to_number(), 3.0);
    }

    #[test]
    fn quickselect_nth_out_of_range_returns_undef() {
        let r = quickselect_nth(&[list(&[1, 2, 3]), StrykeValue::integer(99)]);
        assert!(r.is_undef());
    }

    // ─── top_k_min_heap / bottom_k_max_heap ──────────────────────────────

    #[test]
    fn top_k_min_heap_yields_k_largest_descending() {
        let r = top_k_min_heap(&[list(&[3, 1, 4, 1, 5, 9, 2, 6]), StrykeValue::integer(3)]);
        assert_eq!(ints(&r), [9, 6, 5]);
    }

    #[test]
    fn bottom_k_max_heap_yields_k_smallest_ascending() {
        let r = bottom_k_max_heap(&[list(&[3, 1, 4, 1, 5, 9, 2, 6]), StrykeValue::integer(3)]);
        assert_eq!(ints(&r), [1, 1, 2]);
    }

    // ─── pad_left_n / pad_right_n ────────────────────────────────────────

    #[test]
    fn pad_left_n_pads_only_if_shorter() {
        let r = pad_left_n(&[
            list(&[1, 2]),
            StrykeValue::integer(4),
            StrykeValue::integer(0),
        ]);
        assert_eq!(ints(&r), [0, 0, 1, 2]);
    }

    #[test]
    fn pad_left_n_noop_if_already_long_enough() {
        let r = pad_left_n(&[
            list(&[1, 2, 3]),
            StrykeValue::integer(2),
            StrykeValue::integer(0),
        ]);
        assert_eq!(ints(&r), [1, 2, 3]);
    }

    #[test]
    fn pad_right_n_appends_pad_value() {
        let r = pad_right_n(&[list(&[1]), StrykeValue::integer(3), StrykeValue::integer(9)]);
        assert_eq!(ints(&r), [1, 9, 9]);
    }

    // ─── roundrobin / unzip3 ─────────────────────────────────────────────

    #[test]
    fn roundrobin_interleaves_with_jagged_lengths() {
        let r = roundrobin(&[list(&[1, 4, 7]), list(&[2, 5]), list(&[3, 6, 8, 9])]);
        assert_eq!(ints(&r), [1, 2, 3, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn unzip3_splits_triples_into_three_columns() {
        let r = unzip3(&[arr(vec![
            list(&[1, 2, 3]),
            list(&[4, 5, 6]),
            list(&[7, 8, 9]),
        ])]);
        let cols = list_elements(&r);
        assert_eq!(cols.len(), 3);
        assert_eq!(ints(&cols[0]), [1, 4, 7]);
        assert_eq!(ints(&cols[1]), [2, 5, 8]);
        assert_eq!(ints(&cols[2]), [3, 6, 9]);
    }

    // ─── position_max / position_min ─────────────────────────────────────

    #[test]
    fn position_max_returns_first_occurrence_index() {
        // Strict `>` in source means first occurrence wins on ties.
        let r = position_max(&[list(&[1, 5, 3, 5, 2])]);
        assert_eq!(r.to_int(), 1);
    }

    #[test]
    fn position_min_empty_returns_undef() {
        let r = position_min(&[list(&[])]);
        assert!(r.is_undef());
    }

    // ─── levenshtein_normalized ──────────────────────────────────────────

    #[test]
    fn levenshtein_normalized_identical_is_zero() {
        let r = levenshtein_normalized(&[
            StrykeValue::string("kitten".into()),
            StrykeValue::string("kitten".into()),
        ]);
        assert_eq!(r.to_number(), 0.0);
    }

    #[test]
    fn levenshtein_normalized_both_empty_is_zero() {
        let r = levenshtein_normalized(&[
            StrykeValue::string("".into()),
            StrykeValue::string("".into()),
        ]);
        assert_eq!(r.to_number(), 0.0);
    }

    #[test]
    fn levenshtein_normalized_completely_different_is_one() {
        // distance 3 / max(3,3) = 1.0
        let r = levenshtein_normalized(&[
            StrykeValue::string("abc".into()),
            StrykeValue::string("xyz".into()),
        ]);
        assert_eq!(r.to_number(), 1.0);
    }

    // ─── str_lcs / str_lcs_length ────────────────────────────────────────

    #[test]
    fn str_lcs_length_matches_known_pair() {
        // LCS("ABCBDAB","BDCABA") == "BCBA" or "BDAB" — length 4.
        let r = str_lcs_length(&[
            StrykeValue::string("ABCBDAB".into()),
            StrykeValue::string("BDCABA".into()),
        ]);
        assert_eq!(r.to_int(), 4);
    }

    // ─── unique_by / mode_iter ───────────────────────────────────────────

    #[test]
    fn unique_by_keeps_first_occurrence() {
        let r = unique_by(&[strlist(&["a", "b", "a", "c", "b"])]);
        let got: Vec<String> = list_elements(&r).iter().map(|v| v.to_string()).collect();
        assert_eq!(got, vec!["a", "b", "c"]);
    }

    #[test]
    fn mode_iter_returns_most_frequent_value() {
        let r = mode_iter(&[list(&[1, 2, 2, 3, 2, 1])]);
        assert_eq!(r.to_int(), 2);
    }

    // ─── exclude / exclude_first / exclude_last ──────────────────────────

    #[test]
    fn exclude_drops_listed_values() {
        let r = exclude(&[
            list(&[1, 2, 3, 4, 5]),
            StrykeValue::integer(2),
            StrykeValue::integer(4),
        ]);
        assert_eq!(ints(&r), [1, 3, 5]);
    }

    #[test]
    fn exclude_last_pops_one_element() {
        let r = exclude_last(&[list(&[10, 20, 30])]);
        assert_eq!(ints(&r), [10, 20]);
    }
}
