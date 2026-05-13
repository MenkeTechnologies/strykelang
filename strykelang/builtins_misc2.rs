//! Currency / ML / file-path / locale / channels.
//! Pure functions where possible. Channels honor capacity, oneshot auto-close,
//! and subscriber cursors; timeout args are accepted but no-op in the single-thread VM.

use crate::value::StrykeValue;
use parking_lot::RwLock;
use std::sync::Arc;

fn arg_str(args: &[StrykeValue]) -> String {
    args.first().map(|v| v.to_string()).unwrap_or_default()
}

fn arg_f64(args: &[StrykeValue], idx: usize) -> Option<f64> {
    args.get(idx).map(|v| v.to_number())
}

fn arg_i64(args: &[StrykeValue], idx: usize) -> Option<i64> {
    args.get(idx).map(|v| v.to_int())
}

fn arr(vs: Vec<StrykeValue>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(vs)))
}

fn list_elements(v: &StrykeValue) -> Vec<StrykeValue> {
    if let Some(a) = v.as_array_ref() {
        return a.read().clone();
    }
    if let Some(a) = v.as_array_vec() {
        return a;
    }
    Vec::new()
}

// ══════════════════════════════════════════════════════════════════════
// Currency
// ══════════════════════════════════════════════════════════════════════

fn currency_table() -> &'static [(&'static str, &'static str, u8)] {
    // (code, symbol, decimal places)
    &[
        ("USD", "$", 2),
        ("EUR", "€", 2),
        ("GBP", "£", 2),
        ("JPY", "¥", 0),
        ("CNY", "¥", 2),
        ("CHF", "Fr", 2),
        ("CAD", "C$", 2),
        ("AUD", "A$", 2),
        ("NZD", "NZ$", 2),
        ("SEK", "kr", 2),
        ("NOK", "kr", 2),
        ("DKK", "kr", 2),
        ("PLN", "zł", 2),
        ("CZK", "Kč", 2),
        ("HUF", "Ft", 2),
        ("RUB", "₽", 2),
        ("INR", "₹", 2),
        ("KRW", "₩", 0),
        ("BRL", "R$", 2),
        ("MXN", "$", 2),
        ("ZAR", "R", 2),
        ("HKD", "HK$", 2),
        ("SGD", "S$", 2),
        ("TRY", "₺", 2),
        ("ILS", "₪", 2),
        ("AED", "د.إ", 2),
        ("SAR", "﷼", 2),
        ("THB", "฿", 2),
        ("PHP", "₱", 2),
        ("VND", "₫", 0),
        ("BTC", "₿", 8),
        ("ETH", "Ξ", 18),
    ]
}

pub fn currency_format(args: &[StrykeValue]) -> StrykeValue {
    let amount = arg_f64(args, 0).unwrap_or(0.0);
    let code = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "USD".to_string());
    let entry = currency_table()
        .iter()
        .find(|(c, _, _)| *c == code.as_str());
    let (symbol, places) = entry.map(|(_, s, p)| (*s, *p)).unwrap_or(("$", 2));
    StrykeValue::string(format!("{}{:.*}", symbol, places as usize, amount))
}

pub fn currency_parse(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == ',')
        .collect();
    let cleaned = cleaned.replace(',', "");
    cleaned
        .parse::<f64>()
        .map(StrykeValue::float)
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn currency_round(args: &[StrykeValue]) -> StrykeValue {
    let amount = arg_f64(args, 0).unwrap_or(0.0);
    let code = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "USD".to_string());
    let entry = currency_table()
        .iter()
        .find(|(c, _, _)| *c == code.as_str());
    let places = entry.map(|(_, _, p)| *p).unwrap_or(2) as i32;
    let mult = 10f64.powi(places);
    StrykeValue::float((amount * mult).round() / mult)
}

pub fn currency_split_thousands(args: &[StrykeValue]) -> StrykeValue {
    let amount = arg_f64(args, 0).unwrap_or(0.0);
    let int_part = amount.trunc() as i64;
    let frac = amount.fract().abs();
    let int_str = format!("{}", int_part.abs());
    let mut chars: Vec<char> = int_str.chars().rev().collect();
    let mut grouped: Vec<char> = Vec::new();
    for (i, c) in chars.drain(..).enumerate() {
        if i > 0 && i % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let int_grouped: String = grouped.into_iter().rev().collect();
    let sign = if amount < 0.0 { "-" } else { "" };
    if frac > 0.0 {
        StrykeValue::string(format!(
            "{}{}.{}",
            sign,
            int_grouped,
            &format!("{:.2}", frac)[2..]
        ))
    } else {
        StrykeValue::string(format!("{}{}", sign, int_grouped))
    }
}

pub fn currency_code_to_symbol(args: &[StrykeValue]) -> StrykeValue {
    let code = arg_str(args).to_ascii_uppercase();
    let sym = currency_table()
        .iter()
        .find(|(c, _, _)| *c == code.as_str())
        .map(|(_, s, _)| *s)
        .unwrap_or("");
    StrykeValue::string(sym.to_string())
}

pub fn currency_symbol_to_code(args: &[StrykeValue]) -> StrykeValue {
    let sym = arg_str(args);
    let code = currency_table()
        .iter()
        .find(|(_, s, _)| *s == sym.as_str())
        .map(|(c, _, _)| *c)
        .unwrap_or("");
    StrykeValue::string(code.to_string())
}

pub fn currency_iso_4217(args: &[StrykeValue]) -> StrykeValue {
    let code = arg_str(args).to_ascii_uppercase();
    let exists = currency_table().iter().any(|(c, _, _)| *c == code.as_str());
    StrykeValue::integer(if exists { 1 } else { 0 })
}

pub fn currency_decimal_places(args: &[StrykeValue]) -> StrykeValue {
    let code = arg_str(args).to_ascii_uppercase();
    let places = currency_table()
        .iter()
        .find(|(c, _, _)| *c == code.as_str())
        .map(|(_, _, p)| *p as i64)
        .unwrap_or(2);
    StrykeValue::integer(places)
}

// Banker's rounding (round-half-to-even) on a real-valued cent count.
fn bankers_round(x: f64) -> i64 {
    let floor = x.floor();
    let diff = x - floor;
    if diff < 0.5 {
        floor as i64
    } else if diff > 0.5 {
        (floor + 1.0) as i64
    } else if (floor as i64) % 2 == 0 {
        floor as i64
    } else {
        (floor + 1.0) as i64
    }
}

fn to_cents(amount: f64) -> i64 {
    bankers_round(amount * 100.0)
}

pub fn money_add(args: &[StrykeValue]) -> StrykeValue {
    let a = to_cents(arg_f64(args, 0).unwrap_or(0.0));
    let b = to_cents(arg_f64(args, 1).unwrap_or(0.0));
    StrykeValue::float(a.saturating_add(b) as f64 / 100.0)
}

pub fn money_sub(args: &[StrykeValue]) -> StrykeValue {
    let a = to_cents(arg_f64(args, 0).unwrap_or(0.0));
    let b = to_cents(arg_f64(args, 1).unwrap_or(0.0));
    StrykeValue::float(a.saturating_sub(b) as f64 / 100.0)
}

pub fn money_mul(args: &[StrykeValue]) -> StrykeValue {
    let a = to_cents(arg_f64(args, 0).unwrap_or(0.0));
    let factor = arg_f64(args, 1).unwrap_or(1.0);
    StrykeValue::float(bankers_round(a as f64 * factor) as f64 / 100.0)
}

pub fn money_div(args: &[StrykeValue]) -> StrykeValue {
    let a = to_cents(arg_f64(args, 0).unwrap_or(0.0));
    let divisor = arg_f64(args, 1).unwrap_or(1.0);
    if divisor == 0.0 {
        return StrykeValue::UNDEF;
    }
    StrykeValue::float(bankers_round(a as f64 / divisor) as f64 / 100.0)
}

pub fn money_compare(args: &[StrykeValue]) -> StrykeValue {
    let a = to_cents(arg_f64(args, 0).unwrap_or(0.0));
    let b = to_cents(arg_f64(args, 1).unwrap_or(0.0));
    StrykeValue::integer(if a < b {
        -1
    } else if a > b {
        1
    } else {
        0
    })
}

// ══════════════════════════════════════════════════════════════════════
// ML / embeddings helpers
// ══════════════════════════════════════════════════════════════════════

pub fn tokenize_simple(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    arr(s
        .split_whitespace()
        .map(|w| StrykeValue::string(w.to_string()))
        .collect())
}

pub fn tokenize_word(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let re = regex::Regex::new(r"\w+").unwrap();
    arr(re
        .find_iter(&s)
        .map(|m| StrykeValue::string(m.as_str().to_string()))
        .collect())
}

/// `tokenize_bpe(text, merges_array) → array` — real Byte-Pair-Encoding
/// tokenisation. Each word is split into individual characters, then the
/// merges (provided as `[["a","b"], ...]` in order) are applied greedily
/// until no more merges match. Returns the resulting token list.
pub fn tokenize_bpe(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let merges_raw = args.get(1).map(list_elements).unwrap_or_default();
    // Build (left, right) → merged pairs preserving order; rank = position.
    let mut ranks: indexmap::IndexMap<(String, String), usize> = indexmap::IndexMap::new();
    for (i, pair_v) in merges_raw.iter().enumerate() {
        let pair = list_elements(pair_v);
        if pair.len() < 2 {
            continue;
        }
        let a = pair[0].to_string();
        let b = pair[1].to_string();
        ranks.insert((a, b), i);
    }
    let mut out: Vec<StrykeValue> = Vec::new();
    for word in s.split_whitespace() {
        if word.is_empty() {
            continue;
        }
        let mut toks: Vec<String> = word.chars().map(|c| c.to_string()).collect();
        loop {
            let mut best: Option<(usize, usize)> = None; // (position, rank)
            for i in 0..toks.len().saturating_sub(1) {
                if let Some(&r) = ranks.get(&(toks[i].clone(), toks[i + 1].clone())) {
                    if best.is_none_or(|(_, br)| r < br) {
                        best = Some((i, r));
                    }
                }
            }
            match best {
                Some((pos, _)) => {
                    let merged = format!("{}{}", toks[pos], toks[pos + 1]);
                    toks[pos] = merged;
                    toks.remove(pos + 1);
                }
                None => break,
            }
        }
        for t in toks {
            out.push(StrykeValue::string(t));
        }
    }
    arr(out)
}

pub fn tokenize_subword(args: &[StrykeValue]) -> StrykeValue {
    // Naive subword: split on non-letter boundaries, then break long words into max_len-char chunks.
    let s = arg_str(args);
    let max_len = arg_i64(args, 1).unwrap_or(4).max(1) as usize;
    let mut out: Vec<StrykeValue> = Vec::new();
    for word in s.split(|c: char| !c.is_alphanumeric()) {
        if word.is_empty() {
            continue;
        }
        let chars: Vec<char> = word.chars().collect();
        if chars.len() <= max_len {
            out.push(StrykeValue::string(word.to_string()));
        } else {
            for c in chars.chunks(max_len) {
                out.push(StrykeValue::string(c.iter().collect::<String>()));
            }
        }
    }
    arr(out)
}

pub fn tokenize_sentencepiece(args: &[StrykeValue]) -> StrykeValue {
    // sentencepiece-like: prepend ▁ to word starts
    let s = arg_str(args);
    let mut out: Vec<StrykeValue> = Vec::new();
    for word in s.split_whitespace() {
        out.push(StrykeValue::string(format!("▁{word}")));
    }
    arr(out)
}

pub fn embed_text(args: &[StrykeValue]) -> StrykeValue {
    // Hashing trick — deterministic embedding for testing.
    let s = arg_str(args);
    let dim = arg_i64(args, 1).unwrap_or(128).max(1) as usize;
    let mut vec = vec![0f64; dim];
    use std::hash::{Hash, Hasher};
    for word in s.split_whitespace() {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        word.hash(&mut h);
        let i = (h.finish() as usize) % dim;
        vec[i] += 1.0;
    }
    arr(vec.into_iter().map(StrykeValue::float).collect())
}

fn as_vec(v: &StrykeValue) -> Vec<f64> {
    list_elements(v).iter().map(|x| x.to_number()).collect()
}

pub fn cosine_similarity(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (args.first(), args.get(1)) else {
        return StrykeValue::UNDEF;
    };
    let a = as_vec(a);
    let b = as_vec(b);
    let n = a.len().min(b.len());
    let dot: f64 = (0..n).map(|i| a[i] * b[i]).sum();
    let na: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        return StrykeValue::float(0.0);
    }
    StrykeValue::float(dot / (na * nb))
}

pub fn euclidean_distance(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (args.first(), args.get(1)) else {
        return StrykeValue::UNDEF;
    };
    let a = as_vec(a);
    let b = as_vec(b);
    let n = a.len().min(b.len());
    let sum: f64 = (0..n).map(|i| (a[i] - b[i]).powi(2)).sum();
    StrykeValue::float(sum.sqrt())
}

pub fn manhattan_distance(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (args.first(), args.get(1)) else {
        return StrykeValue::UNDEF;
    };
    let a = as_vec(a);
    let b = as_vec(b);
    let n = a.len().min(b.len());
    let sum: f64 = (0..n).map(|i| (a[i] - b[i]).abs()).sum();
    StrykeValue::float(sum)
}

pub fn dot_product(args: &[StrykeValue]) -> StrykeValue {
    let (Some(a), Some(b)) = (args.first(), args.get(1)) else {
        return StrykeValue::UNDEF;
    };
    let a = as_vec(a);
    let b = as_vec(b);
    let n = a.len().min(b.len());
    let sum: f64 = (0..n).map(|i| a[i] * b[i]).sum();
    StrykeValue::float(sum)
}

pub fn normalize_vector(args: &[StrykeValue]) -> StrykeValue {
    let v = args.first().map(as_vec).unwrap_or_default();
    let norm: f64 = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm == 0.0 {
        return arr(v.iter().map(|_| StrykeValue::float(0.0)).collect());
    }
    arr(v.iter().map(|x| StrykeValue::float(x / norm)).collect())
}

pub fn vector_add(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec).unwrap_or_default();
    let b = args.get(1).map(as_vec).unwrap_or_default();
    let n = a.len().min(b.len());
    arr((0..n).map(|i| StrykeValue::float(a[i] + b[i])).collect())
}

pub fn vector_sub(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec).unwrap_or_default();
    let b = args.get(1).map(as_vec).unwrap_or_default();
    let n = a.len().min(b.len());
    arr((0..n).map(|i| StrykeValue::float(a[i] - b[i])).collect())
}

pub fn vector_scale(args: &[StrykeValue]) -> StrykeValue {
    let v = args.first().map(as_vec).unwrap_or_default();
    let s = arg_f64(args, 1).unwrap_or(1.0);
    arr(v.iter().map(|x| StrykeValue::float(x * s)).collect())
}

pub fn vector_mean(args: &[StrykeValue]) -> StrykeValue {
    let v = args.first().map(as_vec).unwrap_or_default();
    if v.is_empty() {
        return StrykeValue::UNDEF;
    }
    let sum: f64 = v.iter().sum();
    StrykeValue::float(sum / v.len() as f64)
}

pub fn top_k_indices(args: &[StrykeValue]) -> StrykeValue {
    let v = args.first().map(as_vec).unwrap_or_default();
    let k = arg_i64(args, 1).unwrap_or(1).max(0) as usize;
    let mut idx: Vec<(usize, f64)> = v.into_iter().enumerate().collect();
    idx.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    arr(idx
        .into_iter()
        .take(k)
        .map(|(i, _)| StrykeValue::integer(i as i64))
        .collect())
}

pub fn softmax(args: &[StrykeValue]) -> StrykeValue {
    let v = args.first().map(as_vec).unwrap_or_default();
    if v.is_empty() {
        return arr(vec![]);
    }
    let max = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = v.iter().map(|x| (x - max).exp()).collect();
    let sum: f64 = exps.iter().sum();
    arr(exps
        .into_iter()
        .map(|x| StrykeValue::float(x / sum))
        .collect())
}

pub fn sigmoid(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    StrykeValue::float(1.0 / (1.0 + (-x).exp()))
}

pub fn log_softmax(args: &[StrykeValue]) -> StrykeValue {
    let v = args.first().map(as_vec).unwrap_or_default();
    if v.is_empty() {
        return arr(vec![]);
    }
    let max = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let log_sum_exp: f64 = max + v.iter().map(|x| (x - max).exp()).sum::<f64>().ln();
    arr(v
        .iter()
        .map(|x| StrykeValue::float(x - log_sum_exp))
        .collect())
}

pub fn cross_entropy(args: &[StrykeValue]) -> StrykeValue {
    let p = args.first().map(as_vec).unwrap_or_default();
    let q = args.get(1).map(as_vec).unwrap_or_default();
    let n = p.len().min(q.len());
    let sum: f64 = (0..n).map(|i| -p[i] * q[i].max(1e-12).ln()).sum();
    StrykeValue::float(sum)
}

// ══════════════════════════════════════════════════════════════════════
// File / path extras
// ══════════════════════════════════════════════════════════════════════

pub fn path_canonical(args: &[StrykeValue]) -> StrykeValue {
    use std::path::Path;
    let s = arg_str(args);
    match std::fs::canonicalize(Path::new(&s)) {
        Ok(p) => StrykeValue::string(p.display().to_string()),
        Err(_) => StrykeValue::string(s),
    }
}

pub fn path_relative_to(args: &[StrykeValue]) -> StrykeValue {
    use std::path::Path;
    let p = arg_str(args);
    let base = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    match Path::new(&p).strip_prefix(&base) {
        Ok(rel) => StrykeValue::string(rel.display().to_string()),
        Err(_) => StrykeValue::UNDEF,
    }
}

pub fn path_components(args: &[StrykeValue]) -> StrykeValue {
    use std::path::Path;
    let p = arg_str(args);
    let parts: Vec<StrykeValue> = Path::new(&p)
        .components()
        .map(|c| StrykeValue::string(c.as_os_str().to_string_lossy().into_owned()))
        .collect();
    arr(parts)
}

pub fn path_filename(args: &[StrykeValue]) -> StrykeValue {
    use std::path::Path;
    let p = arg_str(args);
    Path::new(&p)
        .file_name()
        .map(|s| StrykeValue::string(s.to_string_lossy().into_owned()))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn path_stem(args: &[StrykeValue]) -> StrykeValue {
    use std::path::Path;
    let p = arg_str(args);
    Path::new(&p)
        .file_stem()
        .map(|s| StrykeValue::string(s.to_string_lossy().into_owned()))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn path_extension(args: &[StrykeValue]) -> StrykeValue {
    use std::path::Path;
    let p = arg_str(args);
    Path::new(&p)
        .extension()
        .map(|s| StrykeValue::string(s.to_string_lossy().into_owned()))
        .unwrap_or(StrykeValue::string(String::new()))
}

pub fn path_join_many(args: &[StrykeValue]) -> StrykeValue {
    use std::path::PathBuf;
    let mut buf = PathBuf::new();
    for a in args {
        buf.push(a.to_string());
    }
    StrykeValue::string(buf.display().to_string())
}

pub fn path_with_extension(args: &[StrykeValue]) -> StrykeValue {
    use std::path::Path;
    let p = arg_str(args);
    let ext = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let result = Path::new(&p).with_extension(&ext);
    StrykeValue::string(result.display().to_string())
}

pub fn path_with_filename(args: &[StrykeValue]) -> StrykeValue {
    use std::path::Path;
    let p = arg_str(args);
    let name = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let result = Path::new(&p).with_file_name(&name);
    StrykeValue::string(result.display().to_string())
}

pub fn path_is_subdirectory(args: &[StrykeValue]) -> StrykeValue {
    let child = arg_str(args);
    let parent = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    StrykeValue::integer(
        if child.starts_with(&parent) && child.len() > parent.len() {
            1
        } else {
            0
        },
    )
}

pub fn path_common_ancestor(args: &[StrykeValue]) -> StrykeValue {
    use std::path::Path;
    let paths: Vec<String> = args.iter().map(|v| v.to_string()).collect();
    if paths.is_empty() {
        return StrykeValue::UNDEF;
    }
    let comps: Vec<Vec<String>> = paths
        .iter()
        .map(|p| {
            Path::new(p)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .collect();
    let min_len = comps.iter().map(|c| c.len()).min().unwrap_or(0);
    let mut common: Vec<String> = Vec::new();
    'outer: for i in 0..min_len {
        let val = &comps[0][i];
        for c in &comps[1..] {
            if c[i] != *val {
                break 'outer;
            }
        }
        common.push(val.clone());
    }
    StrykeValue::string(common.join("/"))
}

pub fn path_glob_match_regex(args: &[StrykeValue]) -> StrykeValue {
    let glob = arg_str(args);
    let pattern: String = glob
        .chars()
        .map(|c| match c {
            '*' => ".*".to_string(),
            '?' => ".".to_string(),
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '\\' => format!("\\{}", c),
            c => c.to_string(),
        })
        .collect();
    StrykeValue::string(format!("^{}$", pattern))
}

pub fn file_mime(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_str(args);
    let ext = std::path::Path::new(&p)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" => "text/plain",
        "css" => "text/css",
        "js" => "application/javascript",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    };
    StrykeValue::string(mime.to_string())
}

pub fn file_kind(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_str(args);
    match std::fs::metadata(&p) {
        Ok(m) => {
            let kind = if m.is_file() {
                "file"
            } else if m.is_dir() {
                "directory"
            } else if m.is_symlink() {
                "symlink"
            } else {
                "other"
            };
            StrykeValue::string(kind.to_string())
        }
        Err(_) => StrykeValue::UNDEF,
    }
}

pub fn file_attr_get(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let p = arg_str(args);
    let m = match std::fs::metadata(&p) {
        Ok(m) => m,
        Err(_) => return StrykeValue::UNDEF,
    };
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("size".to_string(), StrykeValue::integer(m.len() as i64));
    h.insert(
        "is_file".to_string(),
        StrykeValue::integer(if m.is_file() { 1 } else { 0 }),
    );
    h.insert(
        "is_dir".to_string(),
        StrykeValue::integer(if m.is_dir() { 1 } else { 0 }),
    );
    h.insert(
        "is_readonly".to_string(),
        StrykeValue::integer(if m.permissions().readonly() { 1 } else { 0 }),
    );
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

/// `xattr_get(path, name) → string` — read an extended attribute via libc.
/// macOS `getxattr` / Linux `getxattr`. Returns UTF-8 lossy of the byte value,
/// or UNDEF if the attribute is missing.
#[cfg(unix)]
pub fn xattr_get(args: &[StrykeValue]) -> StrykeValue {
    use std::ffi::CString;
    let path = match CString::new(arg_str(args).as_bytes()) {
        Ok(c) => c,
        Err(_) => return StrykeValue::UNDEF,
    };
    let name_str = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let name = match CString::new(name_str.as_bytes()) {
        Ok(c) => c,
        Err(_) => return StrykeValue::UNDEF,
    };
    let mut buf = vec![0u8; 8192];
    #[cfg(target_os = "macos")]
    let n = unsafe {
        libc::getxattr(
            path.as_ptr(),
            name.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len(),
            0,
            0,
        )
    };
    #[cfg(target_os = "linux")]
    let n = unsafe {
        libc::getxattr(
            path.as_ptr(),
            name.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len(),
        )
    };
    if n < 0 {
        return StrykeValue::UNDEF;
    }
    buf.truncate(n as usize);
    StrykeValue::string(String::from_utf8_lossy(&buf).into_owned())
}
#[cfg(not(unix))]
pub fn xattr_get(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::UNDEF
}

/// `xattr_set(path, name, value) → 0|1` — set an extended attribute.
/// Returns 1 on success, 0 on failure (errno preserved in `$!`).
#[cfg(unix)]
pub fn xattr_set(args: &[StrykeValue]) -> StrykeValue {
    use std::ffi::CString;
    let path = match CString::new(arg_str(args).as_bytes()) {
        Ok(c) => c,
        Err(_) => return StrykeValue::integer(0),
    };
    let name_str = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let name = match CString::new(name_str.as_bytes()) {
        Ok(c) => c,
        Err(_) => return StrykeValue::integer(0),
    };
    let value = args.get(2).map(|v| v.to_string()).unwrap_or_default();
    let bytes = value.as_bytes();
    #[cfg(target_os = "macos")]
    let rc = unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            bytes.as_ptr() as *const libc::c_void,
            bytes.len(),
            0,
            0,
        )
    };
    #[cfg(target_os = "linux")]
    let rc = unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            bytes.as_ptr() as *const libc::c_void,
            bytes.len(),
            0,
        )
    };
    StrykeValue::integer(if rc == 0 { 1 } else { 0 })
}
#[cfg(not(unix))]
pub fn xattr_set(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::integer(0)
}

/// `xattr_list(path) → array` — list all extended-attribute names.
#[cfg(unix)]
pub fn xattr_list(args: &[StrykeValue]) -> StrykeValue {
    use std::ffi::CString;
    let path = match CString::new(arg_str(args).as_bytes()) {
        Ok(c) => c,
        Err(_) => return arr(vec![]),
    };
    let mut buf = vec![0u8; 16384];
    #[cfg(target_os = "macos")]
    let n = unsafe { libc::listxattr(path.as_ptr(), buf.as_mut_ptr() as *mut i8, buf.len(), 0) };
    #[cfg(target_os = "linux")]
    let n = unsafe { libc::listxattr(path.as_ptr(), buf.as_mut_ptr() as *mut i8, buf.len()) };
    if n <= 0 {
        return arr(vec![]);
    }
    let slice = &buf[..n as usize];
    let names: Vec<StrykeValue> = slice
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| StrykeValue::string(String::from_utf8_lossy(s).into_owned()))
        .collect();
    arr(names)
}
#[cfg(not(unix))]
pub fn xattr_list(_args: &[StrykeValue]) -> StrykeValue {
    arr(vec![])
}

pub fn file_chmod_string(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_str(args);
    match std::fs::metadata(&p) {
        Ok(m) => {
            let readonly = m.permissions().readonly();
            StrykeValue::string(if readonly {
                "r--r--r--".to_string()
            } else {
                "rw-r--r--".to_string()
            })
        }
        Err(_) => StrykeValue::UNDEF,
    }
}

pub fn file_chmod_octal(args: &[StrykeValue]) -> StrykeValue {
    let p = arg_str(args);
    match std::fs::metadata(&p) {
        Ok(m) => StrykeValue::integer(if m.permissions().readonly() {
            0o444
        } else {
            0o644
        }),
        Err(_) => StrykeValue::UNDEF,
    }
}

/// `file_locked(path) → 0|1` — try a non-blocking `flock` shared lock;
/// 1 if the file is currently locked by another process, 0 otherwise.
#[cfg(unix)]
pub fn file_locked(args: &[StrykeValue]) -> StrykeValue {
    use std::os::unix::io::AsRawFd;
    let path = arg_str(args);
    let file = match std::fs::OpenOptions::new().read(true).open(&path) {
        Ok(f) => f,
        Err(_) => return StrykeValue::integer(0),
    };
    let fd = file.as_raw_fd();
    // Non-blocking exclusive lock attempt.
    let rc = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        // We got the lock — the file wasn't held by anyone else. Release.
        unsafe { libc::flock(fd, libc::LOCK_UN) };
        StrykeValue::integer(0)
    } else {
        StrykeValue::integer(1)
    }
}
#[cfg(not(unix))]
pub fn file_locked(_args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::integer(0)
}

// ══════════════════════════════════════════════════════════════════════
// Locale / i18n / BCP47
// ══════════════════════════════════════════════════════════════════════

fn country_table() -> &'static [(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
)] {
    // (alpha2, alpha3, numeric, name, phone prefix)
    &[
        ("US", "USA", "840", "United States", "+1"),
        ("GB", "GBR", "826", "United Kingdom", "+44"),
        ("DE", "DEU", "276", "Germany", "+49"),
        ("FR", "FRA", "250", "France", "+33"),
        ("IT", "ITA", "380", "Italy", "+39"),
        ("ES", "ESP", "724", "Spain", "+34"),
        ("CA", "CAN", "124", "Canada", "+1"),
        ("AU", "AUS", "036", "Australia", "+61"),
        ("JP", "JPN", "392", "Japan", "+81"),
        ("CN", "CHN", "156", "China", "+86"),
        ("IN", "IND", "356", "India", "+91"),
        ("BR", "BRA", "076", "Brazil", "+55"),
        ("RU", "RUS", "643", "Russia", "+7"),
        ("MX", "MEX", "484", "Mexico", "+52"),
        ("KR", "KOR", "410", "South Korea", "+82"),
        ("NL", "NLD", "528", "Netherlands", "+31"),
        ("SE", "SWE", "752", "Sweden", "+46"),
        ("NO", "NOR", "578", "Norway", "+47"),
        ("DK", "DNK", "208", "Denmark", "+45"),
        ("FI", "FIN", "246", "Finland", "+358"),
        ("CH", "CHE", "756", "Switzerland", "+41"),
        ("AT", "AUT", "040", "Austria", "+43"),
        ("BE", "BEL", "056", "Belgium", "+32"),
        ("PL", "POL", "616", "Poland", "+48"),
        ("PT", "PRT", "620", "Portugal", "+351"),
        ("IE", "IRL", "372", "Ireland", "+353"),
        ("NZ", "NZL", "554", "New Zealand", "+64"),
        ("ZA", "ZAF", "710", "South Africa", "+27"),
        ("IL", "ISR", "376", "Israel", "+972"),
        ("AR", "ARG", "032", "Argentina", "+54"),
    ]
}

fn language_table() -> &'static [(&'static str, &'static str, &'static str, &'static str)] {
    // (iso 639-1, iso 639-2, iso 639-3, name)
    &[
        ("en", "eng", "eng", "English"),
        ("es", "spa", "spa", "Spanish"),
        ("fr", "fra", "fra", "French"),
        ("de", "deu", "deu", "German"),
        ("zh", "zho", "zho", "Chinese"),
        ("ja", "jpn", "jpn", "Japanese"),
        ("ko", "kor", "kor", "Korean"),
        ("it", "ita", "ita", "Italian"),
        ("pt", "por", "por", "Portuguese"),
        ("ru", "rus", "rus", "Russian"),
        ("ar", "ara", "ara", "Arabic"),
        ("hi", "hin", "hin", "Hindi"),
        ("nl", "nld", "nld", "Dutch"),
        ("sv", "swe", "swe", "Swedish"),
        ("no", "nor", "nor", "Norwegian"),
        ("da", "dan", "dan", "Danish"),
        ("fi", "fin", "fin", "Finnish"),
        ("pl", "pol", "pol", "Polish"),
        ("tr", "tur", "tur", "Turkish"),
        ("he", "heb", "heb", "Hebrew"),
        ("th", "tha", "tha", "Thai"),
        ("vi", "vie", "vie", "Vietnamese"),
        ("id", "ind", "ind", "Indonesian"),
        ("ms", "msa", "msa", "Malay"),
        ("el", "ell", "ell", "Greek"),
        ("cs", "ces", "ces", "Czech"),
        ("hu", "hun", "hun", "Hungarian"),
        ("ro", "ron", "ron", "Romanian"),
        ("uk", "ukr", "ukr", "Ukrainian"),
    ]
}

pub fn locale_parse(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    let s = arg_str(args).replace('_', "-");
    let parts: Vec<&str> = s.split('-').collect();
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    if let Some(lang) = parts.first() {
        h.insert(
            "language".to_string(),
            StrykeValue::string(lang.to_ascii_lowercase()),
        );
    }
    if let Some(region) = parts.get(1) {
        h.insert(
            "region".to_string(),
            StrykeValue::string(region.to_ascii_uppercase()),
        );
    }
    if let Some(variant) = parts.get(2) {
        h.insert(
            "variant".to_string(),
            StrykeValue::string(variant.to_string()),
        );
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

pub fn locale_format(args: &[StrykeValue]) -> StrykeValue {
    let lang = args.first().map(|v| v.to_string()).unwrap_or_default();
    let region = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    if region.is_empty() {
        StrykeValue::string(lang.to_ascii_lowercase())
    } else {
        StrykeValue::string(format!(
            "{}-{}",
            lang.to_ascii_lowercase(),
            region.to_ascii_uppercase()
        ))
    }
}

pub fn locale_language(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).replace('_', "-");
    StrykeValue::string(s.split('-').next().unwrap_or("").to_ascii_lowercase())
}

pub fn locale_region(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).replace('_', "-");
    s.split('-')
        .nth(1)
        .map(|r| StrykeValue::string(r.to_ascii_uppercase()))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn locale_script(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).replace('_', "-");
    for part in s.split('-') {
        if part.len() == 4 && part.chars().all(|c| c.is_ascii_alphabetic()) {
            return StrykeValue::string(part.to_string());
        }
    }
    StrykeValue::UNDEF
}

pub fn locale_variant(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).replace('_', "-");
    s.split('-')
        .nth(2)
        .map(|v| StrykeValue::string(v.to_string()))
        .unwrap_or(StrykeValue::UNDEF)
}

pub fn locale_canonical(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).replace('_', "-");
    let parts: Vec<String> = s
        .split('-')
        .enumerate()
        .map(|(i, p)| match i {
            0 => p.to_ascii_lowercase(),
            1 => p.to_ascii_uppercase(),
            _ => p.to_string(),
        })
        .collect();
    StrykeValue::string(parts.join("-"))
}

pub fn bcp47_validate(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).replace('_', "-");
    let parts: Vec<&str> = s.split('-').collect();
    let valid = parts
        .first()
        .map(|p| (2..=3).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphabetic()))
        .unwrap_or(false);
    StrykeValue::integer(if valid { 1 } else { 0 })
}

pub fn language_tag_match(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_str(args).to_ascii_lowercase().replace('_', "-");
    let b = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .replace('_', "-");
    let a_lang = a.split('-').next().unwrap_or("");
    let b_lang = b.split('-').next().unwrap_or("");
    StrykeValue::integer(if a_lang == b_lang { 1 } else { 0 })
}

pub fn language_tag_subtags(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).replace('_', "-");
    arr(s
        .split('-')
        .map(|p| StrykeValue::string(p.to_string()))
        .collect())
}

pub fn locale_likely_subtags(args: &[StrykeValue]) -> StrykeValue {
    // Naive: just add Latn script + US region for English.
    let s = arg_str(args).to_ascii_lowercase();
    let canonical = match s.as_str() {
        "en" => "en-Latn-US",
        "es" => "es-Latn-ES",
        "fr" => "fr-Latn-FR",
        "de" => "de-Latn-DE",
        "zh" => "zh-Hans-CN",
        "ja" => "ja-Jpan-JP",
        "ko" => "ko-Kore-KR",
        _ => return locale_canonical(args),
    };
    StrykeValue::string(canonical.to_string())
}

pub fn locale_collation(args: &[StrykeValue]) -> StrykeValue {
    StrykeValue::string(arg_str(args))
}

pub fn locale_calendar(args: &[StrykeValue]) -> StrykeValue {
    let lang = locale_language(args).to_string();
    let cal = match lang.as_str() {
        "ar" | "fa" => "islamic",
        "th" => "buddhist",
        "he" => "hebrew",
        _ => "gregorian",
    };
    StrykeValue::string(cal.to_string())
}

pub fn locale_currency(args: &[StrykeValue]) -> StrykeValue {
    let region = locale_region(args).to_string();
    let code = country_table()
        .iter()
        .find(|(a, _, _, _, _)| *a == region.as_str())
        .map(|_| match region.as_str() {
            "US" => "USD",
            "GB" => "GBP",
            "DE" | "FR" | "IT" | "ES" | "NL" | "AT" | "BE" | "IE" | "PT" | "FI" => "EUR",
            "JP" => "JPY",
            "CN" => "CNY",
            "CA" => "CAD",
            "AU" => "AUD",
            "NZ" => "NZD",
            "CH" => "CHF",
            "SE" => "SEK",
            "NO" => "NOK",
            "DK" => "DKK",
            "PL" => "PLN",
            "RU" => "RUB",
            "IN" => "INR",
            "KR" => "KRW",
            "BR" => "BRL",
            "MX" => "MXN",
            "ZA" => "ZAR",
            _ => "USD",
        })
        .unwrap_or("USD");
    StrykeValue::string(code.to_string())
}

pub fn locale_number_format(args: &[StrykeValue]) -> StrykeValue {
    let lang = locale_language(args).to_string();
    let fmt = match lang.as_str() {
        "de" | "fr" | "it" | "es" | "pt" | "nl" => "1.234,56",
        "ar" | "fa" => "1٬234٫56",
        _ => "1,234.56",
    };
    StrykeValue::string(fmt.to_string())
}

pub fn locale_date_format(args: &[StrykeValue]) -> StrykeValue {
    let region = locale_region(args).to_string();
    let fmt = match region.as_str() {
        "US" => "MM/DD/YYYY",
        "GB" | "FR" | "ES" | "DE" | "IT" => "DD/MM/YYYY",
        "JP" | "CN" | "KR" => "YYYY-MM-DD",
        _ => "YYYY-MM-DD",
    };
    StrykeValue::string(fmt.to_string())
}

pub fn locale_time_format(args: &[StrykeValue]) -> StrykeValue {
    let region = locale_region(args).to_string();
    let fmt = match region.as_str() {
        "US" => "h:mm AM/PM",
        _ => "HH:mm",
    };
    StrykeValue::string(fmt.to_string())
}

pub fn locale_decimal_separator(args: &[StrykeValue]) -> StrykeValue {
    let lang = locale_language(args).to_string();
    let sep = match lang.as_str() {
        "de" | "fr" | "it" | "es" | "pt" | "nl" | "sv" | "no" | "da" | "fi" | "pl" | "ru" => ",",
        _ => ".",
    };
    StrykeValue::string(sep.to_string())
}

pub fn locale_group_separator(args: &[StrykeValue]) -> StrykeValue {
    let lang = locale_language(args).to_string();
    let sep = match lang.as_str() {
        "de" | "it" | "es" | "pt" | "nl" => ".",
        "fr" | "sv" | "no" | "da" | "fi" | "pl" | "ru" => " ",
        _ => ",",
    };
    StrykeValue::string(sep.to_string())
}

pub fn locale_first_day_of_week(args: &[StrykeValue]) -> StrykeValue {
    let region = locale_region(args).to_string();
    let day = match region.as_str() {
        "US" | "CA" | "JP" | "BR" | "MX" => 0, // Sunday
        _ => 1,                                // Monday (ISO)
    };
    StrykeValue::integer(day)
}

pub fn locale_measurement_system(args: &[StrykeValue]) -> StrykeValue {
    let region = locale_region(args).to_string();
    let sys = match region.as_str() {
        "US" | "LR" | "MM" => "imperial",
        _ => "metric",
    };
    StrykeValue::string(sys.to_string())
}

pub fn country_code_alpha2(args: &[StrykeValue]) -> StrykeValue {
    let needle = arg_str(args).to_ascii_uppercase();
    let result = country_table()
        .iter()
        .find(|(a2, a3, num, name, _)| {
            *a2 == needle.as_str()
                || *a3 == needle.as_str()
                || *num == needle.as_str()
                || name.eq_ignore_ascii_case(&needle)
        })
        .map(|(a2, _, _, _, _)| *a2)
        .unwrap_or("");
    StrykeValue::string(result.to_string())
}

pub fn country_code_alpha3(args: &[StrykeValue]) -> StrykeValue {
    let needle = arg_str(args).to_ascii_uppercase();
    let result = country_table()
        .iter()
        .find(|(a2, a3, num, name, _)| {
            *a2 == needle.as_str()
                || *a3 == needle.as_str()
                || *num == needle.as_str()
                || name.eq_ignore_ascii_case(&needle)
        })
        .map(|(_, a3, _, _, _)| *a3)
        .unwrap_or("");
    StrykeValue::string(result.to_string())
}

pub fn country_code_numeric(args: &[StrykeValue]) -> StrykeValue {
    let needle = arg_str(args).to_ascii_uppercase();
    let result = country_table()
        .iter()
        .find(|(a2, a3, _, name, _)| {
            *a2 == needle.as_str() || *a3 == needle.as_str() || name.eq_ignore_ascii_case(&needle)
        })
        .map(|(_, _, num, _, _)| *num)
        .unwrap_or("");
    StrykeValue::string(result.to_string())
}

pub fn country_name(args: &[StrykeValue]) -> StrykeValue {
    let needle = arg_str(args).to_ascii_uppercase();
    let result = country_table()
        .iter()
        .find(|(a2, a3, num, _, _)| {
            *a2 == needle.as_str() || *a3 == needle.as_str() || *num == needle.as_str()
        })
        .map(|(_, _, _, name, _)| *name)
        .unwrap_or("");
    StrykeValue::string(result.to_string())
}

pub fn country_phone_prefix(args: &[StrykeValue]) -> StrykeValue {
    let needle = arg_str(args).to_ascii_uppercase();
    let result = country_table()
        .iter()
        .find(|(a2, a3, _, name, _)| {
            *a2 == needle.as_str() || *a3 == needle.as_str() || name.eq_ignore_ascii_case(&needle)
        })
        .map(|(_, _, _, _, p)| *p)
        .unwrap_or("");
    StrykeValue::string(result.to_string())
}

pub fn country_languages(args: &[StrykeValue]) -> StrykeValue {
    let code = arg_str(args).to_ascii_uppercase();
    let langs: &[&str] = match code.as_str() {
        "US" | "GB" | "AU" | "NZ" | "IE" => &["en"],
        "CA" => &["en", "fr"],
        "DE" | "AT" => &["de"],
        "FR" => &["fr"],
        "ES" => &["es"],
        "IT" => &["it"],
        "JP" => &["ja"],
        "CN" => &["zh"],
        "KR" => &["ko"],
        "BR" | "PT" => &["pt"],
        "RU" => &["ru"],
        "IN" => &["hi", "en"],
        "CH" => &["de", "fr", "it"],
        "BE" => &["nl", "fr"],
        "MX" => &["es"],
        _ => &[],
    };
    arr(langs
        .iter()
        .map(|l| StrykeValue::string(l.to_string()))
        .collect())
}

pub fn language_iso_639_1(args: &[StrykeValue]) -> StrykeValue {
    let needle = arg_str(args).to_ascii_lowercase();
    let result = language_table()
        .iter()
        .find(|(a1, a2, a3, name)| {
            *a1 == needle.as_str()
                || *a2 == needle.as_str()
                || *a3 == needle.as_str()
                || name.eq_ignore_ascii_case(&needle)
        })
        .map(|(a1, _, _, _)| *a1)
        .unwrap_or("");
    StrykeValue::string(result.to_string())
}

pub fn language_iso_639_2(args: &[StrykeValue]) -> StrykeValue {
    let needle = arg_str(args).to_ascii_lowercase();
    let result = language_table()
        .iter()
        .find(|(a1, a2, a3, name)| {
            *a1 == needle.as_str()
                || *a2 == needle.as_str()
                || *a3 == needle.as_str()
                || name.eq_ignore_ascii_case(&needle)
        })
        .map(|(_, a2, _, _)| *a2)
        .unwrap_or("");
    StrykeValue::string(result.to_string())
}

pub fn language_iso_639_3(args: &[StrykeValue]) -> StrykeValue {
    let needle = arg_str(args).to_ascii_lowercase();
    let result = language_table()
        .iter()
        .find(|(a1, a2, a3, name)| {
            *a1 == needle.as_str()
                || *a2 == needle.as_str()
                || *a3 == needle.as_str()
                || name.eq_ignore_ascii_case(&needle)
        })
        .map(|(_, _, a3, _)| *a3)
        .unwrap_or("");
    StrykeValue::string(result.to_string())
}

pub fn language_name(args: &[StrykeValue]) -> StrykeValue {
    let needle = arg_str(args).to_ascii_lowercase();
    let result = language_table()
        .iter()
        .find(|(a1, a2, a3, _)| {
            *a1 == needle.as_str() || *a2 == needle.as_str() || *a3 == needle.as_str()
        })
        .map(|(_, _, _, name)| *name)
        .unwrap_or("");
    StrykeValue::string(result.to_string())
}

// ══════════════════════════════════════════════════════════════════════
// Channels / messaging
//
// Single-thread VM channels: kind ∈ {unbounded, bounded, sync, oneshot,
// broadcast, subscriber}. Capacity is enforced for bounded; oneshot
// auto-closes after one send; broadcast holds the full buffer and each
// subscriber consumes via an independent cursor. Timeout args are accepted
// but ignored (no blocking in a single-thread VM).
// ══════════════════════════════════════════════════════════════════════

fn mk_channel(kind: &str, cap: i64) -> StrykeValue {
    use indexmap::IndexMap;
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("kind".to_string(), StrykeValue::string(kind.to_string()));
    h.insert("capacity".to_string(), StrykeValue::integer(cap));
    h.insert("buffer".to_string(), arr(vec![]));
    h.insert("closed".to_string(), StrykeValue::integer(0));
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

pub fn channel_unbounded(_args: &[StrykeValue]) -> StrykeValue {
    mk_channel("unbounded", -1)
}

pub fn channel_bounded(args: &[StrykeValue]) -> StrykeValue {
    let cap = arg_i64(args, 0).unwrap_or(1024);
    mk_channel("bounded", cap)
}

pub fn channel_sync(_args: &[StrykeValue]) -> StrykeValue {
    mk_channel("sync", 0)
}

/// `channel_send_timeout(ch, value, [ms])` — send a value to the channel.
/// Returns 1 on success, 0 if the channel is closed or full (bounded /
/// sync channels with `capacity` ≥ 0 reject the send when at-capacity).
/// The timeout is non-functional in the single-thread VM; behavior is
/// equivalent to `channel_try_send` for capacity-honoring semantics.
pub fn channel_send_timeout(args: &[StrykeValue]) -> StrykeValue {
    let Some(ch) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::integer(0);
    };
    let val = args.get(1).cloned().unwrap_or(StrykeValue::UNDEF);
    let g = ch.read();
    if g.get("closed").is_some_and(|v| v.is_true()) {
        return StrykeValue::integer(0);
    }
    let cap = g.get("capacity").map(|v| v.to_int()).unwrap_or(-1);
    let kind = g.get("kind").map(|v| v.to_string()).unwrap_or_default();
    let buf_v = g.get("buffer").cloned().unwrap_or(StrykeValue::UNDEF);
    drop(g);
    let Some(buf) = buf_v.as_array_ref() else {
        return StrykeValue::integer(0);
    };
    let mut bw = buf.write();
    // Capacity enforcement: -1 = unbounded; 0 = rendezvous (no buffering,
    // always reject in a single-thread VM); n > 0 = bounded.
    if cap == 0 {
        return StrykeValue::integer(0);
    }
    if cap > 0 && (bw.len() as i64) >= cap {
        return StrykeValue::integer(0);
    }
    bw.push(val);
    // Oneshot auto-closes after the single permitted send.
    if kind == "oneshot" {
        drop(bw);
        ch.write()
            .insert("closed".to_string(), StrykeValue::integer(1));
    }
    StrykeValue::integer(1)
}

/// `channel_recv_timeout(ch, [ms])` — pull the next available value.
/// Returns UNDEF when the channel is empty.
///
/// Subscriber-channel handling: when `ch.kind == "subscriber"`, the read
/// uses an independent cursor into the parent broadcast channel's buffer,
/// so each subscriber sees every published message exactly once.
pub fn channel_recv_timeout(args: &[StrykeValue]) -> StrykeValue {
    let Some(ch) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::UNDEF;
    };
    let kind = ch
        .read()
        .get("kind")
        .map(|v| v.to_string())
        .unwrap_or_default();
    if kind == "subscriber" {
        let parent_v = ch
            .read()
            .get("parent")
            .cloned()
            .unwrap_or(StrykeValue::UNDEF);
        let cursor = ch.read().get("cursor").map(|v| v.to_int()).unwrap_or(0);
        let parent = match parent_v.as_hash_ref() {
            Some(p) => p,
            None => return StrykeValue::UNDEF,
        };
        let buf_v = parent
            .read()
            .get("buffer")
            .cloned()
            .unwrap_or(StrykeValue::UNDEF);
        let buf = match buf_v.as_array_ref() {
            Some(b) => b,
            None => return StrykeValue::UNDEF,
        };
        let bg = buf.read();
        if (cursor as usize) >= bg.len() {
            return StrykeValue::UNDEF;
        }
        let val = bg[cursor as usize].clone();
        drop(bg);
        ch.write()
            .insert("cursor".to_string(), StrykeValue::integer(cursor + 1));
        return val;
    }
    let buf_v = ch
        .read()
        .get("buffer")
        .cloned()
        .unwrap_or(StrykeValue::UNDEF);
    if let Some(buf) = buf_v.as_array_ref() {
        let mut bw = buf.write();
        if !bw.is_empty() {
            return bw.remove(0);
        }
    }
    StrykeValue::UNDEF
}

pub fn channel_drain(args: &[StrykeValue]) -> StrykeValue {
    let Some(ch) = args.first().and_then(|v| v.as_hash_ref()) else {
        return arr(vec![]);
    };
    let g = ch.read();
    let buf_v = g.get("buffer").cloned().unwrap_or(StrykeValue::UNDEF);
    drop(g);
    if let Some(buf) = buf_v.as_array_ref() {
        let mut bw = buf.write();
        let drained: Vec<StrykeValue> = std::mem::take(&mut *bw);
        return arr(drained);
    }
    arr(vec![])
}

pub fn channel_close(args: &[StrykeValue]) -> StrykeValue {
    let Some(ch) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::integer(0);
    };
    ch.write()
        .insert("closed".to_string(), StrykeValue::integer(1));
    StrykeValue::integer(1)
}

pub fn channel_is_closed(args: &[StrykeValue]) -> StrykeValue {
    let Some(ch) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::integer(0);
    };
    let g = ch.read();
    StrykeValue::integer(if g.get("closed").is_some_and(|v| v.is_true()) {
        1
    } else {
        0
    })
}

pub fn broadcast_channel_new(args: &[StrykeValue]) -> StrykeValue {
    let cap = arg_i64(args, 0).unwrap_or(1024);
    mk_channel("broadcast", cap)
}

/// `broadcast_channel_subscribe(broadcast_ch) → subscriber_ch` — create
/// a new subscriber with an independent read cursor. Each subscriber sees
/// every message published after it subscribed; messages are stored in
/// the broadcast channel's shared buffer and read by cursor offset.
pub fn broadcast_channel_subscribe(args: &[StrykeValue]) -> StrykeValue {
    let Some(parent) = args.first().and_then(|v| v.as_hash_ref()) else {
        return StrykeValue::UNDEF;
    };
    let parent_buf_len = parent
        .read()
        .get("buffer")
        .and_then(|v| v.as_array_ref().map(|a| a.read().len() as i64))
        .unwrap_or(0);
    use indexmap::IndexMap;
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert(
        "kind".to_string(),
        StrykeValue::string("subscriber".to_string()),
    );
    h.insert(
        "parent".to_string(),
        args.first().cloned().unwrap_or(StrykeValue::UNDEF),
    );
    // Read cursor starts at the current published count, so subscribers
    // only receive messages from now on.
    h.insert("cursor".to_string(), StrykeValue::integer(parent_buf_len));
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

/// `oneshot_new()` — single-value rendezvous channel. After the first
/// successful send, the channel is auto-closed and further sends fail.
pub fn oneshot_new(_args: &[StrykeValue]) -> StrykeValue {
    mk_channel("oneshot", 1)
}
