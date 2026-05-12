//! HTML / JSON / XML / CSS primitives.
//! Uses `serde_json` and `scraper` (both already deps). XML/CSS ops
//! that don't have a clean crate-backed implementation use lightweight
//! regex/string parsing — pragmatic, not RFC-perfect.

use crate::value::StrykeValue;
use parking_lot::RwLock;
use std::sync::Arc;

fn arg_str(args: &[StrykeValue]) -> String {
    args.first().map(|v| v.to_string()).unwrap_or_default()
}

fn arr(vs: Vec<StrykeValue>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(vs)))
}

// ══════════════════════════════════════════════════════════════════════
// JSON jq-like operations (serde_json backed)
// ══════════════════════════════════════════════════════════════════════

fn parse_json(s: &str) -> Option<serde_json::Value> {
    serde_json::from_str(s).ok()
}

fn json_to_stryke(v: &serde_json::Value) -> StrykeValue {
    use indexmap::IndexMap;
    match v {
        serde_json::Value::Null => StrykeValue::UNDEF,
        serde_json::Value::Bool(b) => StrykeValue::integer(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                StrykeValue::integer(i)
            } else if let Some(f) = n.as_f64() {
                StrykeValue::float(f)
            } else {
                StrykeValue::string(n.to_string())
            }
        }
        serde_json::Value::String(s) => StrykeValue::string(s.clone()),
        serde_json::Value::Array(items) => {
            let elems: Vec<StrykeValue> = items.iter().map(json_to_stryke).collect();
            arr(elems)
        }
        serde_json::Value::Object(m) => {
            let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
            for (k, v) in m {
                h.insert(k.clone(), json_to_stryke(v));
            }
            StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
        }
    }
}

fn split_path(path: &str) -> Vec<&str> {
    path.split('.').filter(|s| !s.is_empty()).collect()
}

/// `jq_get(JSON_STR, "path.to.key")` — extract value at path. Path
/// uses dot notation; numeric segments index arrays.
pub fn jq_get(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let path = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let Some(mut v) = parse_json(&s) else {
        return StrykeValue::UNDEF;
    };
    for seg in split_path(&path) {
        if let Ok(idx) = seg.parse::<usize>() {
            v = match v {
                serde_json::Value::Array(arr) => {
                    arr.into_iter().nth(idx).unwrap_or(serde_json::Value::Null)
                }
                _ => serde_json::Value::Null,
            };
        } else {
            v = match v {
                serde_json::Value::Object(mut m) => m.remove(seg).unwrap_or(serde_json::Value::Null),
                _ => serde_json::Value::Null,
            };
        }
    }
    json_to_stryke(&v)
}

pub fn jq_set(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let path = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let new_val = args.get(2).map(|v| v.to_string()).unwrap_or_default();
    let Some(mut v) = parse_json(&s) else {
        return StrykeValue::UNDEF;
    };
    let new_v = parse_json(&new_val).unwrap_or(serde_json::Value::String(new_val.clone()));
    let segs = split_path(&path);
    fn set_path(v: &mut serde_json::Value, segs: &[&str], new_v: serde_json::Value) {
        if segs.is_empty() {
            *v = new_v;
            return;
        }
        let seg = segs[0];
        let rest = &segs[1..];
        if let Ok(idx) = seg.parse::<usize>() {
            if let serde_json::Value::Array(arr) = v {
                while arr.len() <= idx {
                    arr.push(serde_json::Value::Null);
                }
                set_path(&mut arr[idx], rest, new_v);
            }
        } else {
            if !v.is_object() {
                *v = serde_json::Value::Object(serde_json::Map::new());
            }
            if let serde_json::Value::Object(m) = v {
                let entry = m.entry(seg.to_string()).or_insert(serde_json::Value::Null);
                set_path(entry, rest, new_v);
            }
        }
    }
    set_path(&mut v, &segs, new_v);
    StrykeValue::string(serde_json::to_string(&v).unwrap_or_default())
}

pub fn jq_delete(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let path = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let Some(mut v) = parse_json(&s) else {
        return StrykeValue::UNDEF;
    };
    let segs = split_path(&path);
    if segs.is_empty() {
        return StrykeValue::string("null".to_string());
    }
    let last = segs[segs.len() - 1];
    let parents = &segs[..segs.len() - 1];
    fn descend<'a>(v: &'a mut serde_json::Value, segs: &[&str]) -> Option<&'a mut serde_json::Value> {
        let mut cur = v;
        for seg in segs {
            if let Ok(idx) = seg.parse::<usize>() {
                if let serde_json::Value::Array(arr) = cur {
                    cur = arr.get_mut(idx)?;
                } else {
                    return None;
                }
            } else if let serde_json::Value::Object(m) = cur {
                cur = m.get_mut(*seg)?;
            } else {
                return None;
            }
        }
        Some(cur)
    }
    if let Some(target) = descend(&mut v, parents) {
        if let Ok(idx) = last.parse::<usize>() {
            if let serde_json::Value::Array(arr) = target {
                if idx < arr.len() {
                    arr.remove(idx);
                }
            }
        } else if let serde_json::Value::Object(m) = target {
            m.remove(last);
        }
    }
    StrykeValue::string(serde_json::to_string(&v).unwrap_or_default())
}

pub fn jq_select(args: &[StrykeValue]) -> StrykeValue {
    jq_get(args)
}

pub fn jq_keys_at(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    if let Some(h) = v.as_hash_ref() {
        let g = h.read();
        return arr(g.keys().map(|k| StrykeValue::string(k.clone())).collect());
    }
    StrykeValue::UNDEF
}

pub fn jq_values_at(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    if let Some(h) = v.as_hash_ref() {
        let g = h.read();
        return arr(g.values().cloned().collect());
    }
    if let Some(a) = v.as_array_ref() {
        return StrykeValue::array_ref(a);
    }
    StrykeValue::UNDEF
}

pub fn jq_length_at(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    if let Some(h) = v.as_hash_ref() {
        return StrykeValue::integer(h.read().len() as i64);
    }
    if let Some(a) = v.as_array_ref() {
        return StrykeValue::integer(a.read().len() as i64);
    }
    if let Some(s) = v.as_str() {
        return StrykeValue::integer(s.chars().count() as i64);
    }
    StrykeValue::integer(0)
}

pub fn jq_type(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let Some(v) = parse_json(&s) else {
        return StrykeValue::string("invalid".to_string());
    };
    let t = match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    };
    StrykeValue::string(t.to_string())
}

pub fn jq_has(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    StrykeValue::integer(if v.is_undef() { 0 } else { 1 })
}

pub fn jq_paths(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let Some(v) = parse_json(&s) else {
        return StrykeValue::UNDEF;
    };
    fn walk(v: &serde_json::Value, prefix: String, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, vv) in m {
                    let p = if prefix.is_empty() { k.clone() } else { format!("{}.{}", prefix, k) };
                    out.push(p.clone());
                    walk(vv, p, out);
                }
            }
            serde_json::Value::Array(a) => {
                for (i, vv) in a.iter().enumerate() {
                    let p = if prefix.is_empty() { i.to_string() } else { format!("{}.{}", prefix, i) };
                    out.push(p.clone());
                    walk(vv, p, out);
                }
            }
            _ => {}
        }
    }
    let mut paths: Vec<String> = Vec::new();
    walk(&v, String::new(), &mut paths);
    arr(paths.into_iter().map(StrykeValue::string).collect())
}

pub fn jq_leaf_paths(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let Some(v) = parse_json(&s) else {
        return StrykeValue::UNDEF;
    };
    fn walk(v: &serde_json::Value, prefix: String, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, vv) in m {
                    let p = if prefix.is_empty() { k.clone() } else { format!("{}.{}", prefix, k) };
                    walk(vv, p, out);
                }
            }
            serde_json::Value::Array(a) => {
                for (i, vv) in a.iter().enumerate() {
                    let p = if prefix.is_empty() { i.to_string() } else { format!("{}.{}", prefix, i) };
                    walk(vv, p, out);
                }
            }
            _ => out.push(prefix),
        }
    }
    let mut paths: Vec<String> = Vec::new();
    walk(&v, String::new(), &mut paths);
    arr(paths.into_iter().map(StrykeValue::string).collect())
}

pub fn jq_walk(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let Some(v) = parse_json(&s) else {
        return StrykeValue::UNDEF;
    };
    json_to_stryke(&v)
}

pub fn jq_map_values(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let Some(v) = parse_json(&s) else {
        return StrykeValue::UNDEF;
    };
    json_to_stryke(&v)
}

pub fn jq_filter(args: &[StrykeValue]) -> StrykeValue {
    jq_get(args)
}

pub fn jq_to_entries(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let Some(v) = parse_json(&s) else {
        return StrykeValue::UNDEF;
    };
    if let serde_json::Value::Object(m) = v {
        use indexmap::IndexMap;
        let entries: Vec<StrykeValue> = m
            .into_iter()
            .map(|(k, v)| {
                let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
                h.insert("key".to_string(), StrykeValue::string(k));
                h.insert("value".to_string(), json_to_stryke(&v));
                StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
            })
            .collect();
        return arr(entries);
    }
    StrykeValue::UNDEF
}

pub fn jq_from_entries(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let Some(arr_ref) = args.first().and_then(|v| v.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let g = arr_ref.read();
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    for entry in g.iter() {
        if let Some(eh) = entry.as_hash_ref() {
            let eg = eh.read();
            let key = eg.get("key").map(|v| v.to_string()).unwrap_or_default();
            let val = eg.get("value").cloned().unwrap_or(StrykeValue::UNDEF);
            h.insert(key, val);
        }
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

pub fn jq_with_entries(args: &[StrykeValue]) -> StrykeValue {
    jq_to_entries(args)
}

pub fn jq_recurse(args: &[StrykeValue]) -> StrykeValue {
    jq_paths(args)
}

pub fn jq_min_by(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    if let Some(arr_ref) = v.as_array_ref() {
        let g = arr_ref.read();
        return g.iter()
            .min_by(|a, b| a.to_number().partial_cmp(&b.to_number()).unwrap_or(std::cmp::Ordering::Equal))
            .cloned()
            .unwrap_or(StrykeValue::UNDEF);
    }
    StrykeValue::UNDEF
}

pub fn jq_max_by(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    if let Some(arr_ref) = v.as_array_ref() {
        let g = arr_ref.read();
        return g.iter()
            .max_by(|a, b| a.to_number().partial_cmp(&b.to_number()).unwrap_or(std::cmp::Ordering::Equal))
            .cloned()
            .unwrap_or(StrykeValue::UNDEF);
    }
    StrykeValue::UNDEF
}

pub fn jq_sort_by(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    if let Some(arr_ref) = v.as_array_ref() {
        let mut g: Vec<StrykeValue> = arr_ref.read().clone();
        g.sort_by(|a, b| a.to_number().partial_cmp(&b.to_number()).unwrap_or(std::cmp::Ordering::Equal));
        return arr(g);
    }
    StrykeValue::UNDEF
}

pub fn jq_group_by(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let v = jq_get(args);
    if let Some(arr_ref) = v.as_array_ref() {
        let mut m: IndexMap<String, Vec<StrykeValue>> = IndexMap::new();
        for x in arr_ref.read().iter() {
            m.entry(x.to_string()).or_default().push(x.clone());
        }
        let groups: Vec<StrykeValue> = m.into_values().map(arr).collect();
        return arr(groups);
    }
    StrykeValue::UNDEF
}

pub fn jq_unique_by(args: &[StrykeValue]) -> StrykeValue {
    use std::collections::HashSet;
    let v = jq_get(args);
    if let Some(arr_ref) = v.as_array_ref() {
        let mut seen: HashSet<String> = HashSet::new();
        let out: Vec<StrykeValue> = arr_ref
            .read()
            .iter()
            .filter(|v| seen.insert(v.to_string()))
            .cloned()
            .collect();
        return arr(out);
    }
    StrykeValue::UNDEF
}

pub fn jq_any(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    if let Some(arr_ref) = v.as_array_ref() {
        return StrykeValue::integer(if arr_ref.read().iter().any(|v| v.is_true()) { 1 } else { 0 });
    }
    StrykeValue::integer(0)
}

pub fn jq_all(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    if let Some(arr_ref) = v.as_array_ref() {
        let g = arr_ref.read();
        if g.is_empty() {
            return StrykeValue::integer(1);
        }
        return StrykeValue::integer(if g.iter().all(|v| v.is_true()) { 1 } else { 0 });
    }
    StrykeValue::integer(0)
}

pub fn jq_flatten(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    if let Some(arr_ref) = v.as_array_ref() {
        let mut out: Vec<StrykeValue> = Vec::new();
        fn walk(v: &StrykeValue, out: &mut Vec<StrykeValue>) {
            if let Some(a) = v.as_array_ref() {
                for x in a.read().iter() {
                    walk(x, out);
                }
            } else {
                out.push(v.clone());
            }
        }
        for x in arr_ref.read().iter() {
            walk(x, &mut out);
        }
        return arr(out);
    }
    v
}

pub fn jq_index(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    let needle = args.get(2).map(|v| v.to_string()).unwrap_or_default();
    if let Some(arr_ref) = v.as_array_ref() {
        let g = arr_ref.read();
        for (i, x) in g.iter().enumerate() {
            if x.to_string() == needle {
                return StrykeValue::integer(i as i64);
            }
        }
        return StrykeValue::integer(-1);
    }
    StrykeValue::integer(-1)
}

pub fn jq_indices(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    let needle = args.get(2).map(|v| v.to_string()).unwrap_or_default();
    if let Some(arr_ref) = v.as_array_ref() {
        let g = arr_ref.read();
        let mut out: Vec<StrykeValue> = Vec::new();
        for (i, x) in g.iter().enumerate() {
            if x.to_string() == needle {
                out.push(StrykeValue::integer(i as i64));
            }
        }
        return arr(out);
    }
    StrykeValue::UNDEF
}

pub fn jq_first(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    if let Some(arr_ref) = v.as_array_ref() {
        return arr_ref.read().first().cloned().unwrap_or(StrykeValue::UNDEF);
    }
    v
}

pub fn jq_last(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    if let Some(arr_ref) = v.as_array_ref() {
        return arr_ref.read().last().cloned().unwrap_or(StrykeValue::UNDEF);
    }
    v
}

pub fn jq_split_at(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    let n = args.get(2).map(|v| v.to_int() as usize).unwrap_or(0);
    if let Some(arr_ref) = v.as_array_ref() {
        let g = arr_ref.read();
        let (l, r): (Vec<_>, Vec<_>) = g.iter().enumerate().partition(|(i, _)| *i < n);
        let left: Vec<StrykeValue> = l.into_iter().map(|(_, v)| v.clone()).collect();
        let right: Vec<StrykeValue> = r.into_iter().map(|(_, v)| v.clone()).collect();
        return arr(vec![arr(left), arr(right)]);
    }
    StrykeValue::UNDEF
}

pub fn jq_chunks(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    let n = args.get(2).map(|v| v.to_int().max(1) as usize).unwrap_or(1);
    if let Some(arr_ref) = v.as_array_ref() {
        let g = arr_ref.read();
        let out: Vec<StrykeValue> = g.chunks(n).map(|c| arr(c.to_vec())).collect();
        return arr(out);
    }
    StrykeValue::UNDEF
}

pub fn jq_zip(args: &[StrykeValue]) -> StrykeValue {
    let a = jq_get(args);
    let b = jq_get(&[
        args.get(2).cloned().unwrap_or(StrykeValue::UNDEF),
        args.get(3).cloned().unwrap_or(StrykeValue::UNDEF),
    ]);
    let (Some(a_arr), Some(b_arr)) = (a.as_array_ref(), b.as_array_ref()) else {
        return StrykeValue::UNDEF;
    };
    let ag = a_arr.read();
    let bg = b_arr.read();
    let n = ag.len().min(bg.len());
    let out: Vec<StrykeValue> = (0..n)
        .map(|i| arr(vec![ag[i].clone(), bg[i].clone()]))
        .collect();
    arr(out)
}

pub fn jq_combinations(args: &[StrykeValue]) -> StrykeValue {
    let v = jq_get(args);
    let k = args.get(2).map(|v| v.to_int() as usize).unwrap_or(2);
    let Some(arr_ref) = v.as_array_ref() else {
        return StrykeValue::UNDEF;
    };
    let g = arr_ref.read();
    let n = g.len();
    if k > n {
        return arr(vec![]);
    }
    let mut indices: Vec<usize> = (0..k).collect();
    let mut out: Vec<StrykeValue> = Vec::new();
    loop {
        out.push(arr(indices.iter().map(|i| g[*i].clone()).collect()));
        let mut i = k;
        while i > 0 {
            i -= 1;
            if indices[i] < n - k + i {
                indices[i] += 1;
                for j in i + 1..k {
                    indices[j] = indices[j - 1] + 1;
                }
                break;
            }
            if i == 0 {
                return arr(out);
            }
        }
    }
}

pub fn json_diff(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_str(args);
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let (Some(va), Some(vb)) = (parse_json(&a), parse_json(&b)) else {
        return StrykeValue::UNDEF;
    };
    if va == vb {
        StrykeValue::string("[]".to_string())
    } else {
        // Simple text diff representation
        StrykeValue::string(format!(
            "[{{\"old\":{}}},{{\"new\":{}}}]",
            serde_json::to_string(&va).unwrap_or_default(),
            serde_json::to_string(&vb).unwrap_or_default()
        ))
    }
}

pub fn json_patch(args: &[StrykeValue]) -> StrykeValue {
    // RFC 6902 — applied naively: only "replace" / "add" / "remove" ops.
    let s = arg_str(args);
    let patches_s = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let Some(mut v) = parse_json(&s) else {
        return StrykeValue::UNDEF;
    };
    let Some(patches) = parse_json(&patches_s) else {
        return StrykeValue::UNDEF;
    };
    let serde_json::Value::Array(ops) = patches else {
        return StrykeValue::UNDEF;
    };
    for op in ops {
        let serde_json::Value::Object(m) = op else { continue };
        let op_kind = m.get("op").and_then(|v| v.as_str()).unwrap_or("");
        let path = m.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let value = m.get("value").cloned();
        let segs: Vec<&str> = path.split('/').skip(1).collect();
        match op_kind {
            "replace" | "add" => {
                if let Some(nv) = value {
                    if segs.is_empty() {
                        v = nv;
                    } else {
                        let path_str = segs.join(".");
                        let new_s = jq_set(&[
                            StrykeValue::string(serde_json::to_string(&v).unwrap_or_default()),
                            StrykeValue::string(path_str),
                            StrykeValue::string(serde_json::to_string(&nv).unwrap_or_default()),
                        ]).to_string();
                        if let Some(parsed) = parse_json(&new_s) {
                            v = parsed;
                        }
                    }
                }
            }
            "remove" => {
                let path_str = segs.join(".");
                let new_s = jq_delete(&[
                    StrykeValue::string(serde_json::to_string(&v).unwrap_or_default()),
                    StrykeValue::string(path_str),
                ]).to_string();
                if let Some(parsed) = parse_json(&new_s) {
                    v = parsed;
                }
            }
            _ => {}
        }
    }
    StrykeValue::string(serde_json::to_string(&v).unwrap_or_default())
}

pub fn json_merge_patch(args: &[StrykeValue]) -> StrykeValue {
    // RFC 7396 — recursive merge.
    let s = arg_str(args);
    let patch_s = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let (Some(mut v), Some(p)) = (parse_json(&s), parse_json(&patch_s)) else {
        return StrykeValue::UNDEF;
    };
    fn merge(v: &mut serde_json::Value, p: serde_json::Value) {
        match (v, p) {
            (serde_json::Value::Object(target), serde_json::Value::Object(patch)) => {
                for (k, pv) in patch {
                    if pv.is_null() {
                        target.remove(&k);
                    } else {
                        let entry = target.entry(k).or_insert(serde_json::Value::Null);
                        merge(entry, pv);
                    }
                }
            }
            (v, p) => *v = p,
        }
    }
    merge(&mut v, p);
    StrykeValue::string(serde_json::to_string(&v).unwrap_or_default())
}

pub fn json_pointer_resolve(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let ptr = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let Some(v) = parse_json(&s) else {
        return StrykeValue::UNDEF;
    };
    let resolved = v.pointer(&ptr).cloned().unwrap_or(serde_json::Value::Null);
    json_to_stryke(&resolved)
}

pub fn json_pointer_set(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let ptr = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let new_val = args.get(2).map(|v| v.to_string()).unwrap_or_default();
    let Some(mut v) = parse_json(&s) else {
        return StrykeValue::UNDEF;
    };
    let new_v = parse_json(&new_val).unwrap_or(serde_json::Value::String(new_val.clone()));
    if let Some(target) = v.pointer_mut(&ptr) {
        *target = new_v;
    }
    StrykeValue::string(serde_json::to_string(&v).unwrap_or_default())
}

// ══════════════════════════════════════════════════════════════════════
// HTML / DOM (scraper backed)
// ══════════════════════════════════════════════════════════════════════

pub fn html_parse(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let _doc = scraper::Html::parse_document(&s);
    // Return the raw HTML as a marker (full DOM doesn't fit StrykeValue)
    StrykeValue::string(s)
}

pub fn html_to_text(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let doc = scraper::Html::parse_document(&s);
    let body_sel = scraper::Selector::parse("body").unwrap();
    let text: String = doc
        .select(&body_sel)
        .flat_map(|n| n.text())
        .collect::<Vec<_>>()
        .join(" ");
    StrykeValue::string(text.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn html_pretty(args: &[StrykeValue]) -> StrykeValue {
    // No real pretty-printer without an additional crate; return as-is.
    StrykeValue::string(arg_str(args))
}

pub fn html_minify(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r">\s+<").unwrap();
    let collapsed = re.replace_all(&s, "><").to_string();
    StrykeValue::string(collapsed.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn html_sanitize(args: &[StrykeValue]) -> StrykeValue {
    // Strip scripts, styles, on* attrs, and javascript: URLs.
    let s = arg_str(args);
    let s = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>")
        .unwrap()
        .replace_all(&s, "")
        .to_string();
    let s = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>")
        .unwrap()
        .replace_all(&s, "")
        .to_string();
    let s = regex::Regex::new(r#"(?i)\son\w+\s*=\s*"[^"]*""#)
        .unwrap()
        .replace_all(&s, "")
        .to_string();
    let s = regex::Regex::new(r#"(?i)javascript:[^"'\s>]*"#)
        .unwrap()
        .replace_all(&s, "")
        .to_string();
    StrykeValue::string(s)
}

pub fn html_strip_tags(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r"<[^>]+>").unwrap();
    StrykeValue::string(re.replace_all(&s, "").to_string())
}

pub fn html_strip_scripts(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    StrykeValue::string(re.replace_all(&s, "").to_string())
}

pub fn html_strip_styles(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    StrykeValue::string(re.replace_all(&s, "").to_string())
}

fn extract_attrs(html: &str, selector: &str, attr: &str) -> Vec<String> {
    let doc = scraper::Html::parse_document(html);
    let Ok(sel) = scraper::Selector::parse(selector) else {
        return Vec::new();
    };
    doc.select(&sel)
        .filter_map(|n| n.value().attr(attr).map(|s| s.to_string()))
        .collect()
}

pub fn html_extract_links(args: &[StrykeValue]) -> StrykeValue {
    let urls = extract_attrs(&arg_str(args), "a[href]", "href");
    arr(urls.into_iter().map(StrykeValue::string).collect())
}

pub fn html_extract_images(args: &[StrykeValue]) -> StrykeValue {
    let urls = extract_attrs(&arg_str(args), "img[src]", "src");
    arr(urls.into_iter().map(StrykeValue::string).collect())
}

pub fn html_extract_text(args: &[StrykeValue]) -> StrykeValue {
    html_to_text(args)
}

pub fn html_extract_meta(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let doc = scraper::Html::parse_document(&arg_str(args));
    let sel = scraper::Selector::parse("meta").unwrap();
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    for n in doc.select(&sel) {
        let v = n.value();
        let name = v.attr("name").or_else(|| v.attr("property"));
        let content = v.attr("content");
        if let (Some(name), Some(content)) = (name, content) {
            h.insert(name.to_string(), StrykeValue::string(content.to_string()));
        }
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

pub fn html_extract_title(args: &[StrykeValue]) -> StrykeValue {
    let doc = scraper::Html::parse_document(&arg_str(args));
    let sel = scraper::Selector::parse("title").unwrap();
    let t: String = doc
        .select(&sel)
        .next()
        .map(|n| n.text().collect::<Vec<_>>().join(""))
        .unwrap_or_default();
    StrykeValue::string(t.trim().to_string())
}

pub fn html_extract_headings(args: &[StrykeValue]) -> StrykeValue {
    let doc = scraper::Html::parse_document(&arg_str(args));
    let sel = scraper::Selector::parse("h1, h2, h3, h4, h5, h6").unwrap();
    let out: Vec<StrykeValue> = doc
        .select(&sel)
        .map(|n| StrykeValue::string(n.text().collect::<Vec<_>>().join("").trim().to_string()))
        .collect();
    arr(out)
}

pub fn html_extract_tables(args: &[StrykeValue]) -> StrykeValue {
    let doc = scraper::Html::parse_document(&arg_str(args));
    let table_sel = scraper::Selector::parse("table").unwrap();
    let row_sel = scraper::Selector::parse("tr").unwrap();
    let cell_sel = scraper::Selector::parse("td, th").unwrap();
    let mut tables: Vec<StrykeValue> = Vec::new();
    for t in doc.select(&table_sel) {
        let mut rows: Vec<StrykeValue> = Vec::new();
        for r in t.select(&row_sel) {
            let cells: Vec<StrykeValue> = r
                .select(&cell_sel)
                .map(|c| StrykeValue::string(c.text().collect::<Vec<_>>().join("").trim().to_string()))
                .collect();
            rows.push(arr(cells));
        }
        tables.push(arr(rows));
    }
    arr(tables)
}

pub fn html_inner_text(args: &[StrykeValue]) -> StrykeValue {
    html_to_text(args)
}

pub fn html_canonical_url(args: &[StrykeValue]) -> StrykeValue {
    let doc = scraper::Html::parse_document(&arg_str(args));
    let sel = scraper::Selector::parse("link[rel='canonical']").unwrap();
    let url = doc
        .select(&sel)
        .next()
        .and_then(|n| n.value().attr("href"))
        .unwrap_or_default()
        .to_string();
    StrykeValue::string(url)
}

pub fn html_meta_charset(args: &[StrykeValue]) -> StrykeValue {
    let doc = scraper::Html::parse_document(&arg_str(args));
    let sel = scraper::Selector::parse("meta[charset]").unwrap();
    let cs = doc
        .select(&sel)
        .next()
        .and_then(|n| n.value().attr("charset"))
        .unwrap_or("utf-8")
        .to_string();
    StrykeValue::string(cs)
}

pub fn html_meta_keywords(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let doc = scraper::Html::parse_document(&s);
    let sel = scraper::Selector::parse("meta[name='keywords']").unwrap();
    let kw = doc
        .select(&sel)
        .next()
        .and_then(|n| n.value().attr("content"))
        .unwrap_or("")
        .to_string();
    StrykeValue::string(kw)
}

pub fn html_meta_description(args: &[StrykeValue]) -> StrykeValue {
    let doc = scraper::Html::parse_document(&arg_str(args));
    let sel = scraper::Selector::parse("meta[name='description']").unwrap();
    let d = doc
        .select(&sel)
        .next()
        .and_then(|n| n.value().attr("content"))
        .unwrap_or("")
        .to_string();
    StrykeValue::string(d)
}

pub fn html_meta_og(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let doc = scraper::Html::parse_document(&arg_str(args));
    let sel = scraper::Selector::parse("meta[property]").unwrap();
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    for n in doc.select(&sel) {
        let v = n.value();
        if let (Some(p), Some(c)) = (v.attr("property"), v.attr("content")) {
            if p.starts_with("og:") {
                h.insert(p.to_string(), StrykeValue::string(c.to_string()));
            }
        }
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

pub fn html_meta_twitter(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let doc = scraper::Html::parse_document(&arg_str(args));
    let sel = scraper::Selector::parse("meta[name]").unwrap();
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    for n in doc.select(&sel) {
        let v = n.value();
        if let (Some(name), Some(c)) = (v.attr("name"), v.attr("content")) {
            if name.starts_with("twitter:") {
                h.insert(name.to_string(), StrykeValue::string(c.to_string()));
            }
        }
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

pub fn html_to_markdown(args: &[StrykeValue]) -> StrykeValue {
    // Best-effort: strip tags, preserve basic structure.
    let s = arg_str(args);
    let s = s.replace("<h1>", "# ").replace("</h1>", "\n");
    let s = s.replace("<h2>", "## ").replace("</h2>", "\n");
    let s = s.replace("<h3>", "### ").replace("</h3>", "\n");
    let s = s.replace("<strong>", "**").replace("</strong>", "**");
    let s = s.replace("<em>", "_").replace("</em>", "_");
    let s = s.replace("<br>", "\n").replace("<br/>", "\n");
    let s = s.replace("<p>", "").replace("</p>", "\n\n");
    let re = regex::Regex::new(r"<[^>]+>").unwrap();
    StrykeValue::string(re.replace_all(&s, "").to_string())
}

pub fn markdown_to_html(args: &[StrykeValue]) -> StrykeValue {
    // Best-effort transform — for full markdown, ship pulldown-cmark.
    let s = arg_str(args);
    let mut out = String::new();
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("### ") {
            out.push_str(&format!("<h3>{rest}</h3>\n"));
        } else if let Some(rest) = line.strip_prefix("## ") {
            out.push_str(&format!("<h2>{rest}</h2>\n"));
        } else if let Some(rest) = line.strip_prefix("# ") {
            out.push_str(&format!("<h1>{rest}</h1>\n"));
        } else if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str(&format!("<p>{line}</p>\n"));
        }
    }
    StrykeValue::string(out)
}

pub fn markdown_render(args: &[StrykeValue]) -> StrykeValue {
    markdown_to_html(args)
}

// ══════════════════════════════════════════════════════════════════════
// XML (regex-based; pragmatic, not RFC-perfect)
// ══════════════════════════════════════════════════════════════════════

pub fn xml_parse(args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::string(arg_str(args))
}

pub fn xml_pretty(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let mut out = String::new();
    let mut depth = 0i32;
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // find tag end
            let start = i;
            while i < bytes.len() && bytes[i] != b'>' {
                i += 1;
            }
            i += 1;
            let tag = &s[start..i];
            if tag.starts_with("</") {
                depth -= 1;
            }
            for _ in 0..depth.max(0) {
                out.push_str("  ");
            }
            out.push_str(tag);
            out.push('\n');
            if !tag.starts_with("</") && !tag.ends_with("/>") && !tag.starts_with("<?") {
                depth += 1;
            }
        } else {
            // text
            let start = i;
            while i < bytes.len() && bytes[i] != b'<' {
                i += 1;
            }
            let text = s[start..i].trim();
            if !text.is_empty() {
                for _ in 0..depth.max(0) {
                    out.push_str("  ");
                }
                out.push_str(text);
                out.push('\n');
            }
        }
    }
    StrykeValue::string(out)
}

pub fn xml_minify(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r">\s+<").unwrap();
    StrykeValue::string(re.replace_all(&s, "><").to_string())
}

pub fn xml_namespace(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r#"xmlns(?::\w+)?\s*=\s*"([^"]+)""#).unwrap();
    if let Some(c) = re.captures(&s) {
        return StrykeValue::string(c[1].to_string());
    }
    StrykeValue::UNDEF
}

pub fn xml_text(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r"<[^>]+>").unwrap();
    let stripped = re.replace_all(&s, " ").to_string();
    StrykeValue::string(stripped.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn xml_attrs(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let s = arg_str(args);
    let re = regex::Regex::new(r#"(\w+)\s*=\s*"([^"]*)""#).unwrap();
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    for cap in re.captures_iter(&s) {
        h.insert(cap[1].to_string(), StrykeValue::string(cap[2].to_string()));
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

pub fn xml_children_by_tag(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let tag = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let re = regex::Regex::new(&format!(r"<{}\b[^>]*>(.*?)</{}>", regex::escape(&tag), regex::escape(&tag))).unwrap();
    let out: Vec<StrykeValue> = re
        .captures_iter(&s)
        .map(|c| StrykeValue::string(c[0].to_string()))
        .collect();
    arr(out)
}

pub fn xml_root(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r"<(\w+)").unwrap();
    if let Some(c) = re.captures(s.trim_start_matches(|c: char| c == '<' && {
        let i = s.find('<').unwrap();
        s[i..].starts_with("<?")
    })) {
        return StrykeValue::string(c[1].to_string());
    }
    StrykeValue::UNDEF
}

pub fn xpath_select_one(args: &[StrykeValue]) -> StrykeValue {
    // Naive xpath: only supports //tagname.
    let s = arg_str(args);
    let xp = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    if let Some(tag) = xp.strip_prefix("//") {
        let re = regex::Regex::new(&format!(r"<{}\b[^>]*>(.*?)</{}>", regex::escape(tag), regex::escape(tag))).unwrap();
        if let Some(c) = re.captures(&s) {
            return StrykeValue::string(c[0].to_string());
        }
    }
    StrykeValue::UNDEF
}

pub fn xpath_attribute(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let attr = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let re = regex::Regex::new(&format!(r#"\b{}\s*=\s*"([^"]*)""#, regex::escape(&attr))).unwrap();
    if let Some(c) = re.captures(&s) {
        return StrykeValue::string(c[1].to_string());
    }
    StrykeValue::UNDEF
}

pub fn xpath_text(args: &[StrykeValue]) -> StrykeValue {
    xml_text(args)
}

pub fn xml_to_json(args: &[StrykeValue]) -> StrykeValue {
    // Very rough — wrap text content with tag names as JSON keys.
    let attrs = xml_attrs(args);
    if let Some(h) = attrs.as_hash_ref() {
        let g = h.read();
        let s: Vec<String> = g
            .iter()
            .map(|(k, v)| format!("\"{}\":\"{}\"", k, v.to_string().replace('"', "\\\"")))
            .collect();
        return StrykeValue::string(format!("{{{}}}", s.join(",")));
    }
    StrykeValue::string("{}".to_string())
}

pub fn json_to_xml(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let Some(v) = parse_json(&s) else {
        return StrykeValue::UNDEF;
    };
    fn render(v: &serde_json::Value, tag: &str) -> String {
        match v {
            serde_json::Value::Object(m) => {
                let inner: String = m.iter().map(|(k, v)| render(v, k)).collect();
                format!("<{}>{}</{}>", tag, inner, tag)
            }
            serde_json::Value::Array(a) => a.iter().map(|v| render(v, tag)).collect(),
            _ => format!("<{}>{}</{}>", tag, v.to_string().trim_matches('"'), tag),
        }
    }
    StrykeValue::string(render(&v, "root"))
}

pub fn xml_canonicalize(args: &[StrykeValue]) -> StrykeValue {
    xml_minify(args)
}

// ══════════════════════════════════════════════════════════════════════
// CSS basics (regex-based)
// ══════════════════════════════════════════════════════════════════════

pub fn css_parse(args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::string(arg_str(args))
}

pub fn css_minify(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r"/\*.*?\*/").unwrap();
    let s = re.replace_all(&s, "").to_string();
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let s = s.replace(" {", "{").replace("{ ", "{").replace(" }", "}").replace("; ", ";").replace(": ", ":");
    StrykeValue::string(s)
}

pub fn css_pretty(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let s = s.replace("{", " {\n  ").replace("}", "\n}\n").replace(";", ";\n  ");
    StrykeValue::string(s)
}

pub fn css_selector_parse(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let parts: Vec<StrykeValue> = s
        .split(',')
        .map(|p| StrykeValue::string(p.trim().to_string()))
        .collect();
    arr(parts)
}

pub fn css_rule_extract(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r"([^{}]+)\{([^}]*)\}").unwrap();
    let mut rules: Vec<StrykeValue> = Vec::new();
    for cap in re.captures_iter(&s) {
        rules.push(arr(vec![
            StrykeValue::string(cap[1].trim().to_string()),
            StrykeValue::string(cap[2].trim().to_string()),
        ]));
    }
    arr(rules)
}

pub fn css_specificity(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let mut id = 0i64;
    let mut class = 0i64;
    let mut tag = 0i64;
    for part in s.split(|c: char| c.is_whitespace() || c == '>' || c == '+' || c == '~') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        // Count #, ., :, and tags
        id += p.matches('#').count() as i64;
        class += (p.matches('.').count() + p.matches(':').count() - p.matches("::").count()) as i64;
        if p.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false) {
            tag += 1;
        }
        tag += p.matches("::").count() as i64;
    }
    arr(vec![
        StrykeValue::integer(id),
        StrykeValue::integer(class),
        StrykeValue::integer(tag),
    ])
}

pub fn css_var_resolve(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let var_re = regex::Regex::new(r"--([\w-]+)\s*:\s*([^;}]+)").unwrap();
    use indexmap::IndexMap;
    let mut vars: IndexMap<String, String> = IndexMap::new();
    for cap in var_re.captures_iter(&s) {
        vars.insert(cap[1].to_string(), cap[2].trim().to_string());
    }
    let use_re = regex::Regex::new(r"var\(\s*--([\w-]+)(?:\s*,\s*([^)]*))?\)").unwrap();
    let out = use_re.replace_all(&s, |cap: &regex::Captures| {
        vars.get(&cap[1])
            .cloned()
            .or_else(|| cap.get(2).map(|m| m.as_str().to_string()))
            .unwrap_or_default()
    });
    StrykeValue::string(out.to_string())
}

pub fn css_property_set(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let prop = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let value = args.get(2).map(|v| v.to_string()).unwrap_or_default();
    let re = regex::Regex::new(&format!(r"{}\s*:\s*[^;}}]+", regex::escape(&prop))).unwrap();
    let new = format!("{}: {}", prop, value);
    if re.is_match(&s) {
        StrykeValue::string(re.replace(&s, new.as_str()).to_string())
    } else {
        StrykeValue::string(format!("{}; {}", s, new))
    }
}

pub fn css_property_get(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let prop = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let re = regex::Regex::new(&format!(r"{}\s*:\s*([^;}}]+)", regex::escape(&prop))).unwrap();
    if let Some(c) = re.captures(&s) {
        return StrykeValue::string(c[1].trim().to_string());
    }
    StrykeValue::UNDEF
}

pub fn css_url_extract(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r#"url\(\s*['"]?([^'")]+)['"]?\s*\)"#).unwrap();
    let urls: Vec<StrykeValue> = re
        .captures_iter(&s)
        .map(|c| StrykeValue::string(c[1].to_string()))
        .collect();
    arr(urls)
}

pub fn css_import_extract(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r#"@import\s+(?:url\()?['"]([^'"]+)['"]"#).unwrap();
    let urls: Vec<StrykeValue> = re
        .captures_iter(&s)
        .map(|c| StrykeValue::string(c[1].to_string()))
        .collect();
    arr(urls)
}

pub fn css_font_extract(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r"font-family\s*:\s*([^;}]+)").unwrap();
    let fonts: Vec<StrykeValue> = re
        .captures_iter(&s)
        .map(|c| StrykeValue::string(c[1].trim().to_string()))
        .collect();
    arr(fonts)
}

pub fn selector_to_xpath(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    // Very basic conversion: `a.b` → `//a[@class='b']`, `#id` → `//*[@id='id']`
    let s = regex::Regex::new(r"^(\w+)\.([\w-]+)").unwrap().replace(&s, "//$1[@class='$2']");
    let s = regex::Regex::new(r"^#([\w-]+)").unwrap().replace(&s, "//*[@id='$1']");
    let s = regex::Regex::new(r"^(\w+)$").unwrap().replace(&s, "//$1");
    StrykeValue::string(s.to_string())
}

pub fn xpath_to_selector(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let s = regex::Regex::new(r#"//(\w+)\[@class='([\w-]+)'\]"#).unwrap().replace(&s, "$1.$2");
    let s = regex::Regex::new(r#"//\*\[@id='([\w-]+)'\]"#).unwrap().replace(&s, "#$1");
    let s = regex::Regex::new(r"^//(\w+)$").unwrap().replace(&s, "$1");
    StrykeValue::string(s.to_string())
}
