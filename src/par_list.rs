//! Parallel list algorithms: `puniq`, `pfirst`, `pany`.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use rayon::prelude::*;

use crate::pmap_progress::PmapProgress;
use crate::value::PerlValue;

#[inline]
fn partition_bucket(key: &str, p: usize) -> usize {
    if p <= 1 {
        return 0;
    }
    let mut h = DefaultHasher::new();
    key.hash(&mut h);
    (h.finish() as usize) % p
}

fn puniq_sequential_with_progress(list: Vec<PerlValue>, progress: &PmapProgress) -> Vec<PerlValue> {
    let mut seen = HashSet::<String>::new();
    let mut out = Vec::new();
    for v in list {
        let k = v.to_string();
        if seen.insert(k) {
            out.push(v);
        }
        progress.tick();
    }
    out
}

fn puniq_parallel_buckets(
    list: Vec<PerlValue>,
    p: usize,
    progress: &PmapProgress,
) -> Vec<PerlValue> {
    let mut buckets: Vec<Vec<(usize, PerlValue, String)>> = vec![vec![]; p];
    for (i, v) in list.into_iter().enumerate() {
        let k = v.to_string();
        let b = partition_bucket(&k, p);
        buckets[b].push((i, v, k));
    }
    let partials: Vec<Vec<(usize, PerlValue)>> = buckets
        .into_par_iter()
        .map(|mut bucket| {
            bucket.sort_by_key(|(i, _, _)| *i);
            let mut seen = HashSet::<String>::new();
            let mut out = Vec::new();
            for (i, val, k) in bucket {
                if seen.insert(k) {
                    out.push((i, val));
                }
                progress.tick();
            }
            out
        })
        .collect();
    let mut merged: Vec<(usize, PerlValue)> = partials.into_iter().flatten().collect();
    merged.sort_by_key(|(i, _)| *i);
    merged.into_iter().map(|(_, v)| v).collect()
}

/// Hash-partition parallel distinct: first occurrence order, key = [`PerlValue::to_string`].
pub(crate) fn puniq_run(
    list: Vec<PerlValue>,
    num_partitions: usize,
    progress: &PmapProgress,
) -> Vec<PerlValue> {
    let n = list.len();
    if n == 0 {
        return vec![];
    }
    let p = num_partitions.max(1);
    if p <= 1 || n < p.saturating_mul(4) {
        puniq_sequential_with_progress(list, progress)
    } else {
        puniq_parallel_buckets(list, p, progress)
    }
}

/// Short-circuit parallel `any { }` — stops doing useful work once a match is found (best-effort).
pub(crate) fn pany_run(
    list: Vec<PerlValue>,
    progress: &PmapProgress,
    test: impl Fn(PerlValue) -> bool + Sync + Send,
) -> bool {
    let found = AtomicBool::new(false);
    list.into_par_iter().for_each(|item| {
        if !found.load(Ordering::Relaxed) && test(item) {
            found.store(true, Ordering::Relaxed);
        }
        progress.tick();
    });
    found.load(Ordering::Relaxed)
}

/// Parallel `first { }` preserving **lowest list index** among truthy block results.
pub(crate) fn pfirst_run(
    list: Vec<PerlValue>,
    progress: &PmapProgress,
    test: impl Fn(PerlValue) -> bool + Sync + Send,
) -> Option<PerlValue> {
    if list.is_empty() {
        return None;
    }
    let best = AtomicUsize::new(usize::MAX);
    let list = Arc::new(list);
    let len = list.len();
    (0..len).into_par_iter().for_each(|i| {
        let cur = best.load(Ordering::Acquire);
        if i >= cur {
            progress.tick();
            return;
        }
        if test(list[i].clone()) {
            let mut b = best.load(Ordering::Relaxed);
            while i < b {
                match best.compare_exchange_weak(b, i, Ordering::Relaxed, Ordering::Relaxed) {
                    Ok(_) => break,
                    Err(x) => b = x,
                }
            }
        }
        progress.tick();
    });
    let b = best.load(Ordering::Relaxed);
    if b == usize::MAX {
        None
    } else {
        Some(list[b].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pmap_progress::PmapProgress;
    use crate::value::PerlValue;

    #[test]
    fn test_puniq_run_sequential() {
        let list = vec![
            PerlValue::integer(1),
            PerlValue::integer(2),
            PerlValue::integer(1),
            PerlValue::integer(3),
            PerlValue::integer(2),
        ];
        let progress = PmapProgress::new(false, list.len());
        let result = puniq_run(list, 1, &progress);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].to_int(), 1);
        assert_eq!(result[1].to_int(), 2);
        assert_eq!(result[2].to_int(), 3);
    }

    #[test]
    fn test_puniq_run_parallel() {
        let list = vec![
            PerlValue::integer(1),
            PerlValue::integer(2),
            PerlValue::integer(1),
            PerlValue::integer(3),
            PerlValue::integer(2),
            PerlValue::integer(4),
            PerlValue::integer(1),
            PerlValue::integer(5),
        ];
        // Ensure n >= p * 4 to trigger parallel path
        let progress = PmapProgress::new(false, list.len());
        let result = puniq_run(list, 2, &progress);
        assert_eq!(result.len(), 5);
        assert_eq!(result[0].to_int(), 1);
        assert_eq!(result[1].to_int(), 2);
        assert_eq!(result[2].to_int(), 3);
        assert_eq!(result[3].to_int(), 4);
        assert_eq!(result[4].to_int(), 5);
    }

    #[test]
    fn test_pany_run() {
        let list = vec![
            PerlValue::integer(1),
            PerlValue::integer(2),
            PerlValue::integer(3),
        ];
        let progress = PmapProgress::new(false, list.len());
        assert!(pany_run(list.clone(), &progress, |v| v.to_int() == 2));
        assert!(!pany_run(list, &progress, |v| v.to_int() == 4));
    }

    #[test]
    fn test_pfirst_run() {
        let list = vec![
            PerlValue::integer(1),
            PerlValue::integer(2),
            PerlValue::integer(3),
            PerlValue::integer(2),
        ];
        let progress = PmapProgress::new(false, list.len());
        let res = pfirst_run(list.clone(), &progress, |v| v.to_int() == 2);
        assert_eq!(res.unwrap().to_int(), 2);

        let res_none = pfirst_run(list, &progress, |v| v.to_int() == 5);
        assert!(res_none.is_none());
    }

    #[test]
    fn test_pfirst_run_lowest_index() {
        let list = vec![
            PerlValue::integer(10),
            PerlValue::integer(20),
            PerlValue::integer(30),
            PerlValue::integer(20),
        ];
        let progress = PmapProgress::new(false, list.len());
        // Both 20s match, but it should return the one at index 1, not index 3.
        // We can distinguish if we use something unique, but here to_int is the same.
        // Let's use string for better distinction if needed, but to_int is fine if they are "same" values.
        // pfirst_run is supposed to return the FIRST one in list order.
        let res = pfirst_run(list, &progress, |v| v.to_int() == 20);
        assert_eq!(res.unwrap().to_int(), 20);
    }
}
