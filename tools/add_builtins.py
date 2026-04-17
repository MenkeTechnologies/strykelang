#!/usr/bin/env python3
"""Add batch of trivial builtins to src/builtins.rs.

Inserts dispatch entries before the "inc" line and implementations before
the url_scheme function. Each fn gets a /// doc comment auto-generated
from its name. Idempotent — skips names already dispatched.

    python3 tools/add_builtins.py
"""
import re

BUILTINS_PATH = "src/builtins.rs"

# (dispatch_name, aliases, impl_body, doc_override)
# impl_body uses {interp} and {args} placeholders; if it contains
# "first_arg_or_topic" the fn signature gets interp param.
NEW_FNS = [
    # ── More string ──
    ("chomp_str", [], 'let s = first_arg_or_topic(interp, args).to_string(); Ok(PerlValue::string(s.trim_end_matches("\\n").to_string()))', None),
    ("chop_str", [], 'let mut s = first_arg_or_topic(interp, args).to_string(); s.pop(); Ok(PerlValue::string(s))', None),
    ("ljust", [], 'let s = args.first().map(|v| v.to_string()).unwrap_or_default(); let w = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0); Ok(PerlValue::string(format!("{:<width$}", s, width = w)))', None),
    ("rjust", [], 'let s = args.first().map(|v| v.to_string()).unwrap_or_default(); let w = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0); Ok(PerlValue::string(format!("{:>width$}", s, width = w)))', None),
    ("zfill", [], 'let s = args.first().map(|v| v.to_string()).unwrap_or_default(); let w = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0); let pad = w.saturating_sub(s.len()); Ok(PerlValue::string(format!("{}{}", "0".repeat(pad), s)))', None),
    ("remove_whitespace", [], 'let s = first_arg_or_topic(interp, args).to_string(); Ok(PerlValue::string(s.chars().filter(|c| !c.is_whitespace()).collect()))', None),
    ("normalize_whitespace", [], 'let s = first_arg_or_topic(interp, args).to_string(); Ok(PerlValue::string(s.split_whitespace().collect::<Vec<_>>().join(" ")))', None),
    ("char_at", [], 'let s = args.first().map(|v| v.to_string()).unwrap_or_default(); let i = args.get(1).map(|v| v.to_int() as usize).unwrap_or(0); Ok(s.chars().nth(i).map(|c| PerlValue::string(c.to_string())).unwrap_or(PerlValue::UNDEF))', None),
    ("byte_length", [], 'Ok(PerlValue::integer(first_arg_or_topic(interp, args).to_string().len() as i64))', None),
    ("char_length", [], 'Ok(PerlValue::integer(first_arg_or_topic(interp, args).to_string().chars().count() as i64))', None),
    ("is_prefix", [], 'let h = args.first().map(|v| v.to_string()).unwrap_or_default(); let p = args.get(1).map(|v| v.to_string()).unwrap_or_default(); Ok(bool_iv(h.starts_with(&p as &str)))', None),
    ("is_suffix", [], 'let h = args.first().map(|v| v.to_string()).unwrap_or_default(); let p = args.get(1).map(|v| v.to_string()).unwrap_or_default(); Ok(bool_iv(h.ends_with(&p as &str)))', None),
    ("substring", [], 'let s = args.first().map(|v| v.to_string()).unwrap_or_default(); let start = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0); let len = args.get(2).map(|v| v.to_int().max(0) as usize).unwrap_or(usize::MAX); Ok(PerlValue::string(s.chars().skip(start).take(len).collect()))', None),
    ("insert_str", [], 'let mut s = args.first().map(|v| v.to_string()).unwrap_or_default(); let pos = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0).min(s.len()); let ins = args.get(2).map(|v| v.to_string()).unwrap_or_default(); s.insert_str(pos, &ins); Ok(PerlValue::string(s))', None),
    ("remove_str", [], 'let s = args.first().map(|v| v.to_string()).unwrap_or_default(); let target = args.get(1).map(|v| v.to_string()).unwrap_or_default(); Ok(PerlValue::string(s.replace(&target as &str, "")))', None),
    ("between", [], 'let n = args.first().map(|v| v.to_number()).unwrap_or(0.0); let lo = args.get(1).map(|v| v.to_number()).unwrap_or(0.0); let hi = args.get(2).map(|v| v.to_number()).unwrap_or(0.0); Ok(bool_iv(n >= lo && n <= hi))', None),
    # ── More numeric ──
    ("succ", [], 'let v = first_arg_or_topic(interp, args); Ok(if let Some(n) = v.as_integer() { PerlValue::integer(n.wrapping_add(1)) } else { PerlValue::float(v.to_number() + 1.0) })', "Successor (n + 1)."),
    ("pred", [], 'let v = first_arg_or_topic(interp, args); Ok(if let Some(n) = v.as_integer() { PerlValue::integer(n.wrapping_sub(1)) } else { PerlValue::float(v.to_number() - 1.0) })', "Predecessor (n - 1)."),
    ("reciprocal", [], 'let n = first_arg_or_topic(interp, args).to_number(); Ok(if n == 0.0 { PerlValue::UNDEF } else { PerlValue::float(1.0 / n) })', None),
    ("square_root", [], 'Ok(PerlValue::float(first_arg_or_topic(interp, args).to_number().sqrt()))', "Alias for sqrt."),
    ("cube_root", [], 'Ok(PerlValue::float(first_arg_or_topic(interp, args).to_number().cbrt()))', "Alias for cbrt."),
    ("is_even", [], 'Ok(bool_iv(first_arg_or_topic(interp, args).to_int() % 2 == 0))', "Alias for even."),
    ("is_odd", [], 'Ok(bool_iv(first_arg_or_topic(interp, args).to_int() % 2 != 0))', "Alias for odd."),
    ("is_zero", [], 'Ok(bool_iv(first_arg_or_topic(interp, args).to_number() == 0.0))', "Alias for zero."),
    ("is_positive", [], 'Ok(bool_iv(first_arg_or_topic(interp, args).to_number() > 0.0))', "Alias for positive."),
    ("is_negative", [], 'Ok(bool_iv(first_arg_or_topic(interp, args).to_number() < 0.0))', "Alias for negative."),
    ("is_nonzero", [], 'Ok(bool_iv(first_arg_or_topic(interp, args).to_number() != 0.0))', "Alias for nonzero."),
    ("approx_eq", [], 'let a = args.first().map(|v| v.to_number()).unwrap_or(0.0); let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0); let eps = args.get(2).map(|v| v.to_number()).unwrap_or(1e-9); Ok(bool_iv((a - b).abs() < eps))', "Test approximate equality within epsilon."),
    ("lerp", [], 'let a = args.first().map(|v| v.to_number()).unwrap_or(0.0); let b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0); let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.5); Ok(PerlValue::float(a + (b - a) * t))', "Linear interpolation between a and b."),
    ("remap", [], 'let v = args.first().map(|v| v.to_number()).unwrap_or(0.0); let a1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0); let b1 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0); let a2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0); let b2 = args.get(4).map(|v| v.to_number()).unwrap_or(1.0); let r = b1 - a1; Ok(PerlValue::float(if r == 0.0 { a2 } else { a2 + (v - a1) / r * (b2 - a2) }))', "Remap value from one range to another."),
    ("smoothstep", [], 'let e0 = args.first().map(|v| v.to_number()).unwrap_or(0.0); let e1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0); let x = args.get(2).map(|v| v.to_number()).unwrap_or(0.5); let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0); Ok(PerlValue::float(t * t * (3.0 - 2.0 * t)))', "Smooth Hermite interpolation."),
    ("sigmoid", [], 'let x = first_arg_or_topic(interp, args).to_number(); Ok(PerlValue::float(1.0 / (1.0 + (-x).exp())))', None),
    ("relu", [], 'let x = first_arg_or_topic(interp, args).to_number(); Ok(PerlValue::float(x.max(0.0)))', None),
    ("step", [], 'let x = first_arg_or_topic(interp, args).to_number(); Ok(PerlValue::float(if x >= 0.0 { 1.0 } else { 0.0 }))', "Heaviside step function."),
    # ── More list ──
    ("head_n", [], 'let n = args.first().map(|v| v.to_int().max(0) as usize).unwrap_or(1); let xs = flatten_args(&args[1..]); Ok(PerlValue::array(xs.into_iter().take(n).collect()))', "Take first N elements."),
    ("tail_n", [], 'let n = args.first().map(|v| v.to_int().max(0) as usize).unwrap_or(1); let xs = flatten_args(&args[1..]); let s = xs.len().saturating_sub(n); Ok(PerlValue::array(xs[s..].to_vec()))', "Take last N elements."),
    ("init_list", [], 'let xs = flatten_args(args); if xs.is_empty() { return Ok(PerlValue::array(vec![])); } Ok(PerlValue::array(xs[..xs.len()-1].to_vec()))', "All but the last element."),
    ("last_elem", [], 'Ok(flatten_args(args).last().cloned().unwrap_or(PerlValue::UNDEF))', "Last element of list."),
    ("first_elem", [], 'Ok(flatten_args(args).first().cloned().unwrap_or(PerlValue::UNDEF))', "First element of list."),
    ("second_elem", [], 'Ok(flatten_args(args).get(1).cloned().unwrap_or(PerlValue::UNDEF))', "Second element of list."),
    ("third_elem", [], 'Ok(flatten_args(args).get(2).cloned().unwrap_or(PerlValue::UNDEF))', "Third element of list."),
    ("elem_at", [], 'let i = args.first().map(|v| v.to_int() as usize).unwrap_or(0); let xs = flatten_args(&args[1..]); Ok(xs.get(i).cloned().unwrap_or(PerlValue::UNDEF))', "Element at index."),
    ("insert_at", [], 'let i = args.first().map(|v| v.to_int().max(0) as usize).unwrap_or(0); let v = args.get(1).cloned().unwrap_or(PerlValue::UNDEF); let mut xs = flatten_args(&args[2..]); let pos = i.min(xs.len()); xs.insert(pos, v); Ok(PerlValue::array(xs))', None),
    ("remove_at", [], 'let i = args.first().map(|v| v.to_int().max(0) as usize).unwrap_or(0); let mut xs = flatten_args(&args[1..]); if i < xs.len() { xs.remove(i); } Ok(PerlValue::array(xs))', None),
    ("replace_at", [], 'let i = args.first().map(|v| v.to_int().max(0) as usize).unwrap_or(0); let v = args.get(1).cloned().unwrap_or(PerlValue::UNDEF); let mut xs = flatten_args(&args[2..]); if i < xs.len() { xs[i] = v; } Ok(PerlValue::array(xs))', None),
    ("repeat_elem", [], 'let v = args.first().cloned().unwrap_or(PerlValue::UNDEF); let n = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(0); Ok(PerlValue::array(vec![v; n]))', "Create list of N copies of value."),
    ("sum_list", [], 'Ok(PerlValue::float(collect_numbers(args).iter().sum()))', "Sum of numeric list."),
    ("product_list", [], 'Ok(PerlValue::float(collect_numbers(args).iter().product()))', "Product of numeric list."),
    ("mean_list", [], 'let xs = collect_numbers(args); if xs.is_empty() { return Ok(PerlValue::UNDEF); } Ok(PerlValue::float(xs.iter().sum::<f64>() / xs.len() as f64))', "Mean of numeric list."),
    ("min_list", [], 'let xs = collect_numbers(args); Ok(xs.into_iter().fold(f64::INFINITY, f64::min).into_if_finite())', "Minimum of numeric list."),
    ("max_list", [], 'let xs = collect_numbers(args); Ok(xs.into_iter().fold(f64::NEG_INFINITY, f64::max).into_if_finite())', "Maximum of numeric list."),
    ("span", [], 'let xs = collect_numbers(args); if xs.is_empty() { return Ok(PerlValue::UNDEF); } let mn = xs.iter().copied().fold(f64::INFINITY, f64::min); let mx = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max); Ok(PerlValue::float(mx - mn))', "Difference between max and min."),
    ("prepend", [], 'let v = args.first().cloned().unwrap_or(PerlValue::UNDEF); let mut xs = flatten_args(&args[1..]); xs.insert(0, v); Ok(PerlValue::array(xs))', "Prepend element to list."),
    ("append_elem", [], 'let v = args.first().cloned().unwrap_or(PerlValue::UNDEF); let mut xs = flatten_args(&args[1..]); xs.push(v); Ok(PerlValue::array(xs))', "Append element to list."),
    ("contains_elem", [], 'let t = args.first().map(|v| v.to_string()).unwrap_or_default(); let xs = flatten_args(&args[1..]); Ok(bool_iv(xs.iter().any(|v| v.to_string() == t)))', "Test if list contains element."),
    ("index_of_elem", [], 'let t = args.first().map(|v| v.to_string()).unwrap_or_default(); let xs = flatten_args(&args[1..]); Ok(xs.iter().position(|v| v.to_string() == t).map(|i| PerlValue::integer(i as i64)).unwrap_or(PerlValue::integer(-1)))', None),
    ("count_elem", [], 'let t = args.first().map(|v| v.to_string()).unwrap_or_default(); let xs = flatten_args(&args[1..]); Ok(PerlValue::integer(xs.iter().filter(|v| v.to_string() == t).count() as i64))', "Count occurrences of element."),
    ("remove_elem", [], 'let t = args.first().map(|v| v.to_string()).unwrap_or_default(); let xs = flatten_args(&args[1..]); Ok(PerlValue::array(xs.into_iter().filter(|v| v.to_string() != t).collect()))', "Remove all occurrences of element."),
    ("remove_first_elem", [], 'let t = args.first().map(|v| v.to_string()).unwrap_or_default(); let xs = flatten_args(&args[1..]); let mut removed = false; Ok(PerlValue::array(xs.into_iter().filter(|v| { if !removed && v.to_string() == t { removed = true; false } else { true } }).collect()))', None),
    # ── More hash ──
    ("hash_map_values", [], 'Ok(args.first().cloned().unwrap_or(PerlValue::UNDEF))', "Apply function to each hash value (placeholder)."),
    ("hash_filter_keys", [], 'Ok(args.first().cloned().unwrap_or(PerlValue::UNDEF))', "Filter hash by key predicate (placeholder)."),
    ("hash_merge_deep", [], 'builtin_merge_hash(args)', "Deep merge of hashes."),
    ("hash_to_list", [], 'let Some(hr) = args.first().and_then(|v| v.as_hash_ref()) else { return Ok(PerlValue::array(vec![])); }; let mut out = Vec::new(); for (k, v) in hr.read().iter() { out.push(PerlValue::string(k.clone())); out.push(v.clone()); } Ok(PerlValue::array(out))', "Flatten hash to alternating key-value list."),
    ("hash_from_list", [], 'let xs = flatten_args(args); let mut m = indexmap::IndexMap::new(); let mut it = xs.into_iter(); while let Some(k) = it.next() { let v = it.next().unwrap_or(PerlValue::UNDEF); m.insert(k.to_string(), v); } Ok(PerlValue::hash_ref(Arc::new(RwLock::new(m))))', "Build hash from alternating key-value list."),
    ("hash_zip", [], 'builtin_zipmap(args)', "Zip two arrays into a hash."),
    # ── More predicates ──
    ("is_between", [], 'let n = args.first().map(|v| v.to_number()).unwrap_or(0.0); let lo = args.get(1).map(|v| v.to_number()).unwrap_or(0.0); let hi = args.get(2).map(|v| v.to_number()).unwrap_or(0.0); Ok(bool_iv(n >= lo && n <= hi))', None),
    ("is_in_range", [], 'let n = args.first().map(|v| v.to_number()).unwrap_or(0.0); let lo = args.get(1).map(|v| v.to_number()).unwrap_or(0.0); let hi = args.get(2).map(|v| v.to_number()).unwrap_or(0.0); Ok(bool_iv(n >= lo && n < hi))', "Half-open range check [lo, hi)."),
    ("is_multiple_of", [], 'let a = args.first().map(|v| v.to_int()).unwrap_or(0); let b = args.get(1).map(|v| v.to_int()).unwrap_or(1); Ok(bool_iv(b != 0 && a % b == 0))', None),
    ("is_divisible_by", [], 'let a = args.first().map(|v| v.to_int()).unwrap_or(0); let b = args.get(1).map(|v| v.to_int()).unwrap_or(1); Ok(bool_iv(b != 0 && a % b == 0))', None),
    ("is_power_of", [], 'let n = args.first().map(|v| v.to_int()).unwrap_or(0); let base = args.get(1).map(|v| v.to_int()).unwrap_or(2); if base <= 1 || n <= 0 { return Ok(bool_iv(false)); } let mut x = n; while x > 1 { if x % base != 0 { return Ok(bool_iv(false)); } x /= base; } Ok(bool_iv(true))', None),
    ("is_perfect_square", [], 'let n = first_arg_or_topic(interp, args).to_int(); if n < 0 { return Ok(bool_iv(false)); } let r = (n as f64).sqrt() as i64; Ok(bool_iv(r * r == n))', None),
    ("is_triangular", [], 'let n = first_arg_or_topic(interp, args).to_int(); if n < 0 { return Ok(bool_iv(false)); } let d = 8 * n + 1; let s = (d as f64).sqrt() as i64; Ok(bool_iv(s * s == d && (s - 1) % 2 == 0))', "Test if number is triangular."),
    ("is_fibonacci", [], 'let n = first_arg_or_topic(interp, args).to_int(); if n < 0 { return Ok(bool_iv(false)); } let check = |x: i64| { let s = (x as f64).sqrt() as i64; s * s == x }; Ok(bool_iv(check(5 * n * n + 4) || check(5 * n * n - 4)))', None),
    # ── Sequence generators ──
    ("fibonacci_seq", [], 'let n = args.first().map(|v| v.to_int().max(0) as usize).unwrap_or(10); let (mut a, mut b) = (0i64, 1i64); let mut out = Vec::with_capacity(n); for _ in 0..n { out.push(PerlValue::integer(a)); let t = b; b = a.saturating_add(b); a = t; } Ok(PerlValue::array(out))', "Generate first N Fibonacci numbers."),
    ("triangular_seq", [], 'let n = args.first().map(|v| v.to_int().max(0) as usize).unwrap_or(10); Ok(PerlValue::array((1..=n).map(|i| PerlValue::integer((i * (i + 1) / 2) as i64)).collect()))', "Generate first N triangular numbers."),
    ("squares_seq", [], 'let n = args.first().map(|v| v.to_int().max(0) as usize).unwrap_or(10); Ok(PerlValue::array((1..=n).map(|i| PerlValue::integer((i * i) as i64)).collect()))', "Generate first N square numbers."),
    ("cubes_seq", [], 'let n = args.first().map(|v| v.to_int().max(0) as usize).unwrap_or(10); Ok(PerlValue::array((1..=n).map(|i| PerlValue::integer((i * i * i) as i64)).collect()))', "Generate first N cube numbers."),
    ("powers_of_seq", [], 'let base = args.first().map(|v| v.to_int()).unwrap_or(2); let n = args.get(1).map(|v| v.to_int().max(0) as usize).unwrap_or(10); Ok(PerlValue::array((0..n).map(|i| PerlValue::integer(base.saturating_pow(i as u32))).collect()))', "Generate N powers of base."),
    ("primes_seq", [], 'builtin_sieve_primes(args)', "Generate primes up to N."),
    ("digits_of", [], 'let n = first_arg_or_topic(interp, args).to_int().abs(); if n == 0 { return Ok(PerlValue::array(vec![PerlValue::integer(0)])); } let mut ds = Vec::new(); let mut x = n; while x > 0 { ds.push(PerlValue::integer(x % 10)); x /= 10; } ds.reverse(); Ok(PerlValue::array(ds))', "Split integer into digits."),
    ("from_digits", [], 'let xs = flatten_args(args); let mut n: i64 = 0; for v in xs { n = n * 10 + v.to_int(); } Ok(PerlValue::integer(n))', "Assemble digits into integer."),
    ("range_inclusive", [], 'let lo = args.first().map(|v| v.to_int()).unwrap_or(0); let hi = args.get(1).map(|v| v.to_int()).unwrap_or(0); Ok(PerlValue::array((lo..=hi).map(PerlValue::integer).collect()))', None),
    ("range_exclusive", [], 'let lo = args.first().map(|v| v.to_int()).unwrap_or(0); let hi = args.get(1).map(|v| v.to_int()).unwrap_or(0); Ok(PerlValue::array((lo..hi).map(PerlValue::integer).collect()))', None),
    # ── Encoding extras ──
    ("hex_to_bytes", [], 'let s = first_arg_or_topic(interp, args).to_string(); let bytes: Vec<PerlValue> = (0..s.len()/2).filter_map(|i| u8::from_str_radix(&s[i*2..i*2+2], 16).ok().map(|b| PerlValue::integer(b as i64))).collect(); Ok(PerlValue::array(bytes))', None),
    ("bytes_to_hex_str", [], 'let xs = flatten_args(args); Ok(PerlValue::string(xs.iter().map(|v| format!("{:02x}", v.to_int() as u8)).collect::<String>()))', None),
    ("xor_strings", [], 'let a = args.first().map(|v| v.to_string()).unwrap_or_default(); let b = args.get(1).map(|v| v.to_string()).unwrap_or_default(); Ok(PerlValue::string(a.bytes().zip(b.bytes().cycle()).map(|(x, y)| (x ^ y) as char).collect()))', "XOR two strings byte-by-byte."),
    # ── More geometry ──
    ("haversine", [], 'let lat1 = args.first().map(|v| v.to_number().to_radians()).unwrap_or(0.0); let lon1 = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0); let lat2 = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(0.0); let lon2 = args.get(3).map(|v| v.to_number().to_radians()).unwrap_or(0.0); let dlat = lat2 - lat1; let dlon = lon2 - lon1; let a = (dlat/2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon/2.0).sin().powi(2); Ok(PerlValue::float(6371.0 * 2.0 * a.sqrt().asin()))', "Great-circle distance in km between two lat/lon points."),
    ("manhattan_distance", [], 'let x1 = args.first().map(|v| v.to_number()).unwrap_or(0.0); let y1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0); let x2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0); let y2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0); Ok(PerlValue::float((x2-x1).abs() + (y2-y1).abs()))', None),
    ("chebyshev_distance", [], 'let x1 = args.first().map(|v| v.to_number()).unwrap_or(0.0); let y1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0); let x2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0); let y2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0); Ok(PerlValue::float((x2-x1).abs().max((y2-y1).abs())))', None),
    ("angle_between_deg", [], 'let dx = args.first().map(|v| v.to_number()).unwrap_or(0.0); let dy = args.get(1).map(|v| v.to_number()).unwrap_or(0.0); Ok(PerlValue::float(dy.atan2(dx).to_degrees()))', None),
    ("rotate_point", [], 'let x = args.first().map(|v| v.to_number()).unwrap_or(0.0); let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0); let angle = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(0.0); let nx = x * angle.cos() - y * angle.sin(); let ny = x * angle.sin() + y * angle.cos(); Ok(PerlValue::array_ref(Arc::new(RwLock::new(vec![PerlValue::float(nx), PerlValue::float(ny)]))))', None),
    # ── Constants ──
    ("speed_of_light", [], 'Ok(PerlValue::float(299792458.0))', "Speed of light in m/s."),
    ("avogadro", [], 'Ok(PerlValue::float(6.02214076e23))', "Avogadro constant."),
    ("boltzmann", [], 'Ok(PerlValue::float(1.380649e-23))', "Boltzmann constant."),
    ("planck", [], 'Ok(PerlValue::float(6.62607015e-34))', "Planck constant."),
    ("gravity", [], 'Ok(PerlValue::float(9.80665))', "Standard gravity in m/s^2."),
    ("golden_ratio", [], 'Ok(PerlValue::float(1.618033988749895))', None),
    ("sqrt2", [], 'Ok(PerlValue::float(std::f64::consts::SQRT_2))', "Square root of 2."),
    ("ln2", [], 'Ok(PerlValue::float(std::f64::consts::LN_2))', "Natural log of 2."),
    ("ln10", [], 'Ok(PerlValue::float(std::f64::consts::LN_10))', "Natural log of 10."),
    # ── Misc useful ──
    ("noop_val", [], 'Ok(args.first().cloned().unwrap_or(PerlValue::UNDEF))', "Return argument unchanged (identity)."),
    ("die_if", [], 'if args.first().map(|v| v.is_true()).unwrap_or(false) { let msg = args.get(1).map(|v| v.to_string()).unwrap_or("condition was true".into()); return Err(PerlError::runtime(msg, 0).into()); } Ok(PerlValue::UNDEF)', None),
    ("die_unless", [], 'if !args.first().map(|v| v.is_true()).unwrap_or(false) { let msg = args.get(1).map(|v| v.to_string()).unwrap_or("condition was false".into()); return Err(PerlError::runtime(msg, 0).into()); } Ok(PerlValue::UNDEF)', None),
    ("assert_type", [], 'let v = args.first().cloned().unwrap_or(PerlValue::UNDEF); let expected = args.get(1).map(|v| v.to_string()).unwrap_or_default(); let actual = if v.is_undef() { "undef" } else if v.is_integer_like() { "integer" } else if v.is_float_like() { "float" } else if v.as_array_ref().is_some() { "arrayref" } else if v.as_hash_ref().is_some() { "hashref" } else { "string" }; if actual != expected { return Err(PerlError::runtime(format!("expected {}, got {}", expected, actual), 0).into()); } Ok(v)', None),
    ("tap_debug", [], 'let v = first_arg_or_topic(interp, args); eprintln!("[tap] {}", v.to_string()); Ok(v)', "Print value to stderr and pass through."),
    ("measure", [], 'let start = std::time::Instant::now(); let v = first_arg_or_topic(interp, args); let elapsed = start.elapsed().as_secs_f64(); eprintln!("[measure] {:.6}s", elapsed); Ok(v)', "Measure and print elapsed time."),
    ("clamp_int", [], 'let v = args.first().map(|v| v.to_int()).unwrap_or(0); let lo = args.get(1).map(|v| v.to_int()).unwrap_or(i64::MIN); let hi = args.get(2).map(|v| v.to_int()).unwrap_or(i64::MAX); Ok(PerlValue::integer(v.clamp(lo, hi)))', None),
    ("wrap_index", [], 'let i = args.first().map(|v| v.to_int()).unwrap_or(0); let len = args.get(1).map(|v| v.to_int()).unwrap_or(1).max(1); Ok(PerlValue::integer(((i % len) + len) % len))', "Wrap index into valid range [0, len)."),
    ("to_bool", [], 'Ok(bool_iv(first_arg_or_topic(interp, args).is_true()))', "Convert to boolean 0/1."),
    ("to_int", [], 'Ok(PerlValue::integer(first_arg_or_topic(interp, args).to_int()))', "Convert to integer."),
    ("to_float", [], 'Ok(PerlValue::float(first_arg_or_topic(interp, args).to_number()))', "Convert to float."),
    ("to_string", [], 'Ok(PerlValue::string(first_arg_or_topic(interp, args).to_string()))', "Convert to string."),
    ("to_array", [], 'Ok(PerlValue::array(flatten_args(args)))', "Convert/flatten to array."),
]

def needs_interp(body):
    return "first_arg_or_topic" in body or "interp" in body

def generate_doc(name, body, doc_override):
    if doc_override:
        return doc_override
    human = name.replace("_", " ")
    if body.startswith("Ok(bool_iv("):
        return f"Test: {human}. Returns 1 (true) or 0 (false)."
    if "unit_scale" in body or "_to_" in name:
        parts = human.split(" to ")
        if len(parts) == 2:
            return f"Convert {parts[0]} to {parts[1]}."
    return f"{human[0].upper()}{human[1:]}."

def main():
    with open(BUILTINS_PATH, "r") as f:
        content = f.read()

    # Find existing dispatch names to skip
    existing = set(re.findall(r'"([a-z_][a-z0-9_]*)"[^=]*=>\s*Some\(', content))

    # Filter to only new names
    to_add = [(n, a, b, d) for n, a, b, d in NEW_FNS if n not in existing]
    print(f"{len(to_add)} new builtins to add ({len(NEW_FNS) - len(to_add)} already exist)")

    if not to_add:
        return

    # Build dispatch block
    dispatch_lines = []
    for name, aliases, body, doc in to_add:
        fn_name = f"builtin_{name}"
        all_names = [name] + aliases
        pattern = " | ".join(f'"{n}"' for n in all_names)
        if needs_interp(body):
            dispatch_lines.append(f'        {pattern} => Some({fn_name}(interp, args)),')
        else:
            dispatch_lines.append(f'        {pattern} => Some({fn_name}(args)),')
    dispatch_block = "\n".join(dispatch_lines) + "\n"

    # Build impl block
    impl_lines = []
    for name, aliases, body, doc_override in to_add:
        fn_name = f"builtin_{name}"
        doc = generate_doc(name, body, doc_override)
        impl_lines.append(f"/// `{name}` — {doc}")
        if needs_interp(body):
            impl_lines.append(f"fn {fn_name}(interp: &Interpreter, args: &[PerlValue]) -> PerlResult<PerlValue> {{ {body} }}")
        else:
            impl_lines.append(f"fn {fn_name}(args: &[PerlValue]) -> PerlResult<PerlValue> {{ {body} }}")
    impl_block = "\n".join(impl_lines) + "\n"

    # Insert dispatch before "inc" line
    inc_pattern = '        "inc" => Some(builtin_inc(args)),'
    if inc_pattern not in content:
        print("ERROR: can't find 'inc' dispatch line")
        return
    content = content.replace(inc_pattern, dispatch_block + inc_pattern)

    # Insert impls before url_scheme function
    url_pattern = "fn builtin_url_scheme("
    idx = content.rfind(url_pattern)
    if idx == -1:
        print("ERROR: can't find url_scheme function")
        return
    content = content[:idx] + impl_block + "\n" + content[idx:]

    with open(BUILTINS_PATH, "w") as f:
        f.write(content)
    print(f"added {len(to_add)} builtins")

if __name__ == "__main__":
    main()
