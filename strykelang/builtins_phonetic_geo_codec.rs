//! Phonetic algorithms, geo projections, base58/base91,
//! astronomy, CRC variants, color blending, compression primitives.

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

fn arg_u64(args: &[StrykeValue], idx: usize) -> Option<u64> {
    args.get(idx).map(|v| v.to_int() as u64)
}

fn arg_str(args: &[StrykeValue], idx: usize) -> Option<String> {
    args.get(idx).map(|v| v.as_str_or_empty())
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

fn make_hash(pairs: Vec<(&str, StrykeValue)>) -> StrykeValue {
    use indexmap::IndexMap;
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    for (k, v) in pairs {
        h.insert(k.to_string(), v);
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

// ══════════════════════════════════════════════════════════════════════
// Phonetic algorithms
// ══════════════════════════════════════════════════════════════════════

pub fn soundex_v1(args: &[StrykeValue]) -> StrykeValue {
    // Classic American Soundex (4-char: letter + 3 digits)
    let s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    let mut chars = s.chars().filter(|c| c.is_ascii_alphabetic());
    let first = match chars.next() {
        Some(c) => c,
        None => return StrykeValue::string(String::new()),
    };
    let code = |c: char| -> char {
        match c {
            'B' | 'F' | 'P' | 'V' => '1',
            'C' | 'G' | 'J' | 'K' | 'Q' | 'S' | 'X' | 'Z' => '2',
            'D' | 'T' => '3',
            'L' => '4',
            'M' | 'N' => '5',
            'R' => '6',
            _ => '0',
        }
    };
    let mut out = String::with_capacity(4);
    out.push(first);
    let mut prev = code(first);
    for c in chars {
        let cc = code(c);
        if cc != '0' && cc != prev {
            out.push(cc);
            if out.len() == 4 {
                break;
            }
        }
        if cc != '0' {
            prev = cc;
        } else if c != 'H' && c != 'W' {
            prev = '0';
        }
    }
    while out.len() < 4 {
        out.push('0');
    }
    StrykeValue::string(out)
}

pub fn soundex_v2(args: &[StrykeValue]) -> StrykeValue {
    // Apache "Refined Soundex" — finer-grained groups (1:BP 2:FV 3:CKS 4:GJ
    // 5:QXZ 6:DT 7:L 8:MN 9:R), no length cap, vowels and H/W skipped.
    let s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    let chars: Vec<char> = s.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    if chars.is_empty() {
        return StrykeValue::string(String::new());
    }
    let code = |c: char| -> char {
        match c {
            'B' | 'P' => '1',
            'F' | 'V' => '2',
            'C' | 'K' | 'S' => '3',
            'G' | 'J' => '4',
            'Q' | 'X' | 'Z' => '5',
            'D' | 'T' => '6',
            'L' => '7',
            'M' | 'N' => '8',
            'R' => '9',
            _ => '0',
        }
    };
    let mut out = String::new();
    out.push(chars[0]);
    let mut prev = code(chars[0]);
    for &c in &chars[1..] {
        let cc = code(c);
        if cc != '0' && cc != prev {
            out.push(cc);
        }
        prev = cc;
    }
    StrykeValue::string(out)
}

pub fn nysiis(args: &[StrykeValue]) -> StrykeValue {
    let mut s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    if s.is_empty() {
        return StrykeValue::string(String::new());
    }
    let replace_prefix = |s: &mut String, pre: &str, repl: &str| {
        if s.starts_with(pre) {
            *s = format!("{}{}", repl, &s[pre.len()..]);
        }
    };
    let replace_suffix = |s: &mut String, suf: &str, repl: &str| {
        if s.ends_with(suf) {
            let pos = s.len() - suf.len();
            *s = format!("{}{}", &s[..pos], repl);
        }
    };
    replace_prefix(&mut s, "MAC", "MCC");
    replace_prefix(&mut s, "KN", "NN");
    replace_prefix(&mut s, "K", "C");
    replace_prefix(&mut s, "PH", "FF");
    replace_prefix(&mut s, "PF", "FF");
    replace_prefix(&mut s, "SCH", "SSS");
    replace_suffix(&mut s, "EE", "Y");
    replace_suffix(&mut s, "IE", "Y");
    replace_suffix(&mut s, "DT", "D");
    replace_suffix(&mut s, "RT", "D");
    replace_suffix(&mut s, "RD", "D");
    replace_suffix(&mut s, "NT", "D");
    replace_suffix(&mut s, "ND", "D");
    let mut out = String::new();
    if let Some(c) = s.chars().next() {
        out.push(c);
    }
    let chars: Vec<char> = s.chars().collect();
    let mut i = 1;
    while i < chars.len() {
        let c = chars[i];
        let mapped = match c {
            'E' if chars.get(i + 1) == Some(&'V') => {
                i += 1;
                Some('F')
            }
            'A' | 'E' | 'I' | 'O' | 'U' => Some('A'),
            'Q' => Some('G'),
            'Z' => Some('S'),
            'M' => Some('N'),
            'K' if chars.get(i + 1) == Some(&'N') => Some('N'),
            'K' => Some('C'),
            'S' if chars.get(i..i + 2) == Some(&['S', 'C', 'H'][..2])
                && chars.get(i + 2) == Some(&'H') =>
            {
                i += 2;
                Some('S')
            }
            'P' if chars.get(i + 1) == Some(&'H') => {
                i += 1;
                Some('F')
            }
            'H' => {
                let prev = chars.get(i - 1).copied();
                let next = chars.get(i + 1).copied();
                let prev_vowel = matches!(prev, Some('A' | 'E' | 'I' | 'O' | 'U'));
                let next_vowel = matches!(next, Some('A' | 'E' | 'I' | 'O' | 'U'));
                if !prev_vowel || !next_vowel {
                    prev
                } else {
                    Some('H')
                }
            }
            'W' if matches!(chars.get(i - 1), Some('A' | 'E' | 'I' | 'O' | 'U')) => {
                chars.get(i - 1).copied()
            }
            _ => Some(c),
        };
        if let Some(m) = mapped {
            if !out.ends_with(m) {
                out.push(m);
            }
        }
        i += 1;
    }
    if out.ends_with('S') {
        out.pop();
    }
    if out.ends_with("AY") {
        out.pop();
        out.pop();
        out.push('Y');
    }
    if out.ends_with('A') {
        out.pop();
    }
    StrykeValue::string(out)
}

pub fn caverphone(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default().to_lowercase();
    if s.is_empty() {
        return StrykeValue::string(String::new());
    }
    let mut s: String = s.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    let replacements: &[(&str, &str)] = &[
        ("cough", "cou2f"),
        ("rough", "rou2f"),
        ("tough", "tou2f"),
        ("enough", "enou2f"),
        ("trough", "trou2f"),
        ("gn", "2n"),
        ("mb", "m2"),
    ];
    for (a, b) in replacements {
        s = s.replace(a, b);
    }
    let single: &[(char, char)] = &[
        ('c', 'k'),
        ('q', 'k'),
        ('x', 'k'),
        ('v', 'f'),
        ('d', 't'),
        ('l', 'L'),
        ('w', 'W'),
        ('p', 'p'),
    ];
    let _ = single;
    let mut chars: Vec<char> = s.chars().collect();
    for c in chars.iter_mut() {
        *c = match *c {
            'c' | 'q' | 'x' => 'k',
            'v' => 'f',
            'd' => 't',
            'z' => 's',
            _ => *c,
        };
    }
    let s: String = chars.into_iter().collect();
    let s = s.replace("tch", "2ch");
    let s = s.replace("ph", "fh");
    // Collapse vowels to '1' (interior), keep first vowel as 'A'
    let mut out = String::with_capacity(s.len());
    let mut prev = '\0';
    for c in s.chars() {
        let mapped = match c {
            'a' | 'e' | 'i' | 'o' | 'u' => {
                if out.is_empty() {
                    'A'
                } else {
                    '1'
                }
            }
            _ => c,
        };
        if mapped != prev || mapped.is_alphabetic() {
            out.push(mapped);
            prev = mapped;
        }
    }
    let mut padded = out;
    padded.push_str("111111");
    padded.truncate(6);
    StrykeValue::string(padded)
}

pub fn caverphone2(args: &[StrykeValue]) -> StrykeValue {
    let s = caverphone(args).as_str_or_empty();
    let mut padded = s;
    padded.push_str("1111111111");
    padded.truncate(10);
    StrykeValue::string(padded)
}

pub fn phonex(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    if s.is_empty() {
        return StrykeValue::string(String::new());
    }
    let mut s = s;
    if s.starts_with("KN") {
        s = format!("N{}", &s[2..]);
    } else if s.starts_with("PH") {
        s = format!("F{}", &s[2..]);
    } else if s.starts_with("WR") {
        s = format!("R{}", &s[2..]);
    }
    if s.starts_with('H') && s.len() > 1 {
        s = s[1..].to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let code = |c: char| -> char {
        match c {
            'B' | 'P' | 'F' | 'V' => '1',
            'C' | 'S' | 'K' | 'G' | 'J' | 'Q' | 'X' | 'Z' => '2',
            'D' | 'T' => '3',
            'L' => '4',
            'M' | 'N' => '5',
            'R' => '6',
            _ => '0',
        }
    };
    let mut out = String::new();
    out.push(chars[0]);
    let mut prev = code(chars[0]);
    for &c in &chars[1..] {
        let cc = code(c);
        if cc != '0' && cc != prev {
            out.push(cc);
        }
        prev = cc;
    }
    while out.len() < 4 {
        out.push('0');
    }
    out.truncate(4);
    StrykeValue::string(out)
}

pub fn match_rating_compare(args: &[StrykeValue]) -> StrykeValue {
    let s1 = arg_str(args, 0).unwrap_or_default();
    let s2 = arg_str(args, 1).unwrap_or_default();
    let c1 = match_rating_codex_impl(&s1);
    let c2 = match_rating_codex_impl(&s2);
    let len_diff = (c1.len() as i64 - c2.len() as i64).abs();
    if len_diff > 3 {
        return StrykeValue::integer(0);
    }
    let a: Vec<char> = c1.chars().collect();
    let b: Vec<char> = c2.chars().collect();
    let mut a_rem: Vec<char> = Vec::new();
    let mut b_rem: Vec<char> = b.clone();
    for ch in &a {
        if let Some(pos) = b_rem.iter().position(|x| x == ch) {
            b_rem.remove(pos);
        } else {
            a_rem.push(*ch);
        }
    }
    let unmatched = a_rem.len() + b_rem.len();
    let sum = a.len() + b.len();
    let min_rating = match sum {
        0..=4 => 5,
        5..=7 => 4,
        8..=11 => 3,
        12..=15 => 2,
        _ => 1,
    };
    let rating = 7 - unmatched as i64;
    StrykeValue::integer(if rating >= min_rating { 1 } else { 0 })
}

pub(crate) fn match_rating_codex_impl(s: &str) -> String {
    let s: String = s
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect();
    let mut chars: Vec<char> = Vec::new();
    let s_chars: Vec<char> = s.chars().collect();
    for (i, &c) in s_chars.iter().enumerate() {
        if i > 0 && matches!(c, 'A' | 'E' | 'I' | 'O' | 'U') {
            continue;
        }
        if !chars.is_empty() && *chars.last().unwrap() == c {
            continue;
        }
        chars.push(c);
    }
    if chars.len() > 6 {
        let half = (chars.len() - 6) / 2;
        let _ = half;
        let n = chars.len();
        chars = chars
            .iter()
            .enumerate()
            .filter(|(i, _)| *i < 3 || *i >= n - 3)
            .map(|(_, c)| *c)
            .collect();
    }
    chars.into_iter().collect()
}

pub fn fuzzy_substring_match(args: &[StrykeValue]) -> StrykeValue {
    let needle = arg_str(args, 0).unwrap_or_default();
    let haystack = arg_str(args, 1).unwrap_or_default();
    let max_dist = arg_i64(args, 2).unwrap_or(2).max(0) as usize;
    let n_chars: Vec<char> = needle.chars().collect();
    let h_chars: Vec<char> = haystack.chars().collect();
    if n_chars.is_empty() {
        return StrykeValue::integer(0);
    }
    let nlen = n_chars.len();
    let hlen = h_chars.len();
    if hlen < nlen.saturating_sub(max_dist) {
        return StrykeValue::integer(-1);
    }
    for start in 0..=hlen.saturating_sub(nlen.saturating_sub(max_dist)) {
        for length in nlen.saturating_sub(max_dist)..=(nlen + max_dist).min(hlen - start) {
            let mut prev = (0..=length).collect::<Vec<_>>();
            let mut curr = vec![0; length + 1];
            for (i, &nc) in n_chars.iter().enumerate() {
                curr[0] = i + 1;
                for j in 0..length {
                    let hc = h_chars[start + j];
                    let cost = if nc == hc { 0 } else { 1 };
                    curr[j + 1] = (curr[j] + 1).min(prev[j + 1] + 1).min(prev[j] + cost);
                }
                std::mem::swap(&mut prev, &mut curr);
            }
            if prev[length] <= max_dist {
                return StrykeValue::integer(start as i64);
            }
        }
    }
    StrykeValue::integer(-1)
}

// ══════════════════════════════════════════════════════════════════════
// Geo projections
// ══════════════════════════════════════════════════════════════════════

pub fn mercator_project_x(args: &[StrykeValue]) -> StrykeValue {
    let lon = arg_f64(args, 0).unwrap_or(0.0);
    StrykeValue::float(lon.to_radians() * 6378137.0)
}

pub fn mercator_project_y(args: &[StrykeValue]) -> StrykeValue {
    let lat = arg_f64(args, 0)
        .unwrap_or(0.0)
        .clamp(-85.05112878, 85.05112878);
    let phi = lat.to_radians();
    StrykeValue::float(6378137.0 * (std::f64::consts::FRAC_PI_4 + phi / 2.0).tan().ln())
}

pub fn mercator_unproject_lat(args: &[StrykeValue]) -> StrykeValue {
    let y = arg_f64(args, 0).unwrap_or(0.0);
    let phi = 2.0 * (y / 6378137.0).exp().atan() - std::f64::consts::FRAC_PI_2;
    StrykeValue::float(phi.to_degrees())
}

pub fn mercator_unproject_lon(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_f64(args, 0).unwrap_or(0.0);
    StrykeValue::float((x / 6378137.0).to_degrees())
}

pub fn lambert_project(args: &[StrykeValue]) -> StrykeValue {
    // Lambert Conformal Conic with one standard parallel `lat0`, on a sphere.
    // n = sin(lat0); F = cos(lat0)·tan^n(π/4 + lat0/2)
    // ρ(φ) = R·F / tan^n(π/4 + φ/2);  ρ₀ = ρ(lat0)
    // x = ρ·sin(n·(λ − λ₀));  y = ρ₀ − ρ·cos(n·(λ − λ₀))
    let lat = arg_f64(args, 0).unwrap_or(0.0).to_radians();
    let lon = arg_f64(args, 1).unwrap_or(0.0).to_radians();
    let lat0 = arg_f64(args, 2).unwrap_or(0.0).to_radians();
    let lon0 = arg_f64(args, 3).unwrap_or(0.0).to_radians();
    let r = 6371000.0_f64;
    let n = lat0.sin();
    if n.abs() < 1e-9 {
        // Degenerate cone at equator → fall back to equirectangular at lat0.
        let x = r * (lon - lon0);
        let y = r * (lat - lat0);
        return arr_sv(vec![StrykeValue::float(x), StrykeValue::float(y)]);
    }
    let quarter = std::f64::consts::FRAC_PI_4;
    let f = lat0.cos() * (quarter + lat0 / 2.0).tan().powf(n);
    let rho = r * f / (quarter + lat / 2.0).tan().powf(n);
    let rho0 = r * f / (quarter + lat0 / 2.0).tan().powf(n);
    let theta = n * (lon - lon0);
    let x = rho * theta.sin();
    let y = rho0 - rho * theta.cos();
    arr_sv(vec![StrykeValue::float(x), StrykeValue::float(y)])
}

/// Initial bearing for the Vincenty inverse problem on the WGS84 ellipsoid.
/// Iterates λ (longitude difference on the auxiliary sphere) until
/// convergence, then computes the initial azimuth. Returns degrees in [0, 360).
/// Falls back to spherical bearing for the antipodal case where Vincenty
/// fails to converge. Use `great_circle_bearing` for the cheap spherical form.
pub fn vincenty_bearing(args: &[StrykeValue]) -> StrykeValue {
    let lat1 = arg_f64(args, 0).unwrap_or(0.0).to_radians();
    let lon1 = arg_f64(args, 1).unwrap_or(0.0).to_radians();
    let lat2 = arg_f64(args, 2).unwrap_or(0.0).to_radians();
    let lon2 = arg_f64(args, 3).unwrap_or(0.0).to_radians();
    let f = 1.0 / 298.257223563_f64;
    let l = lon2 - lon1;
    let u1 = ((1.0 - f) * lat1.tan()).atan();
    let u2 = ((1.0 - f) * lat2.tan()).atan();
    let sin_u1 = u1.sin();
    let cos_u1 = u1.cos();
    let sin_u2 = u2.sin();
    let cos_u2 = u2.cos();
    let mut lambda = l;
    let mut sin_l: f64 = 0.0;
    let mut cos_l: f64 = 0.0;
    let mut converged = false;
    for _ in 0..100 {
        sin_l = lambda.sin();
        cos_l = lambda.cos();
        let sin_sigma =
            ((cos_u2 * sin_l).powi(2) + (cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_l).powi(2)).sqrt();
        if sin_sigma == 0.0 {
            return StrykeValue::float(0.0);
        }
        let cos_sigma = sin_u1 * sin_u2 + cos_u1 * cos_u2 * cos_l;
        let sigma = sin_sigma.atan2(cos_sigma);
        let sin_alpha = cos_u1 * cos_u2 * sin_l / sin_sigma;
        let cos_sq_alpha = 1.0 - sin_alpha * sin_alpha;
        let cos_2sigma_m = if cos_sq_alpha == 0.0 {
            0.0
        } else {
            cos_sigma - 2.0 * sin_u1 * sin_u2 / cos_sq_alpha
        };
        let c = f / 16.0 * cos_sq_alpha * (4.0 + f * (4.0 - 3.0 * cos_sq_alpha));
        let lambda_prev = lambda;
        lambda = l
            + (1.0 - c)
                * f
                * sin_alpha
                * (sigma
                    + c * sin_sigma
                        * (cos_2sigma_m + c * cos_sigma * (-1.0 + 2.0 * cos_2sigma_m.powi(2))));
        if (lambda - lambda_prev).abs() < 1e-12 {
            converged = true;
            break;
        }
    }
    if !converged {
        let dlon = lon2 - lon1;
        let y = dlon.sin() * lat2.cos();
        let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
        return StrykeValue::float(y.atan2(x).to_degrees().rem_euclid(360.0));
    }
    let y = cos_u2 * sin_l;
    let x = cos_u1 * sin_u2 - sin_u1 * cos_u2 * cos_l;
    StrykeValue::float(y.atan2(x).to_degrees().rem_euclid(360.0))
}

pub fn destination_lat_lon(args: &[StrykeValue]) -> StrykeValue {
    let lat = arg_f64(args, 0).unwrap_or(0.0).to_radians();
    let lon = arg_f64(args, 1).unwrap_or(0.0).to_radians();
    let bearing = arg_f64(args, 2).unwrap_or(0.0).to_radians();
    let dist = arg_f64(args, 3).unwrap_or(0.0);
    let r = 6371000.0;
    let ad = dist / r;
    let lat2 = (lat.sin() * ad.cos() + lat.cos() * ad.sin() * bearing.cos()).asin();
    let lon2 =
        lon + (bearing.sin() * ad.sin() * lat.cos()).atan2(ad.cos() - lat.sin() * lat2.sin());
    arr_sv(vec![
        StrykeValue::float(lat2.to_degrees()),
        StrykeValue::float(lon2.to_degrees()),
    ])
}

pub fn utm_zone(args: &[StrykeValue]) -> StrykeValue {
    let lon = arg_f64(args, 0).unwrap_or(0.0);
    let zone = ((lon + 180.0) / 6.0).floor() as i64 + 1;
    StrykeValue::integer(zone.clamp(1, 60))
}

pub fn lat_lon_to_utm(args: &[StrykeValue]) -> StrykeValue {
    let lat = arg_f64(args, 0).unwrap_or(0.0);
    let lon = arg_f64(args, 1).unwrap_or(0.0);
    let zone = ((lon + 180.0) / 6.0).floor() as i64 + 1;
    let lon0 = (zone as f64 - 1.0) * 6.0 - 180.0 + 3.0;
    let a = 6378137.0_f64;
    let e2 = 0.00669437999014_f64;
    let k0 = 0.9996_f64;
    let phi = lat.to_radians();
    let lambda = lon.to_radians();
    let lambda0 = lon0.to_radians();
    let n = a / (1.0 - e2 * phi.sin().powi(2)).sqrt();
    let t = phi.tan().powi(2);
    let c = e2 / (1.0 - e2) * phi.cos().powi(2);
    let a_arg = phi.cos() * (lambda - lambda0);
    let m = a
        * ((1.0 - e2 / 4.0 - 3.0 * e2 * e2 / 64.0) * phi
            - (3.0 * e2 / 8.0 + 3.0 * e2 * e2 / 32.0) * (2.0 * phi).sin()
            + (15.0 * e2 * e2 / 256.0) * (4.0 * phi).sin());
    let x = k0 * n * (a_arg + (1.0 - t + c) * a_arg.powi(3) / 6.0) + 500000.0;
    let y = k0
        * (m + n
            * phi.tan()
            * (a_arg.powi(2) / 2.0 + (5.0 - t + 9.0 * c + 4.0 * c.powi(2)) * a_arg.powi(4) / 24.0));
    let y = if lat < 0.0 { y + 10_000_000.0 } else { y };
    arr_sv(vec![
        StrykeValue::integer(zone),
        StrykeValue::float(x),
        StrykeValue::float(y),
        StrykeValue::string(if lat >= 0.0 { "N".into() } else { "S".into() }),
    ])
}

pub fn utm_to_lat_lon(args: &[StrykeValue]) -> StrykeValue {
    let zone = arg_i64(args, 0).unwrap_or(1).clamp(1, 60);
    let easting = arg_f64(args, 1).unwrap_or(0.0);
    let northing = arg_f64(args, 2).unwrap_or(0.0);
    let hemi = arg_str(args, 3).unwrap_or_else(|| "N".into());
    let a = 6378137.0_f64;
    let e2 = 0.00669437999014_f64;
    let k0 = 0.9996_f64;
    let lon0 = (zone as f64 - 1.0) * 6.0 - 180.0 + 3.0;
    let x = easting - 500000.0;
    let y = if hemi == "S" {
        northing - 10_000_000.0
    } else {
        northing
    };
    let m = y / k0;
    let mu = m / (a * (1.0 - e2 / 4.0 - 3.0 * e2 * e2 / 64.0));
    let e1 = (1.0 - (1.0 - e2).sqrt()) / (1.0 + (1.0 - e2).sqrt());
    let phi1 = mu
        + (3.0 * e1 / 2.0 - 27.0 * e1.powi(3) / 32.0) * (2.0 * mu).sin()
        + (21.0 * e1.powi(2) / 16.0) * (4.0 * mu).sin()
        + (151.0 * e1.powi(3) / 96.0) * (6.0 * mu).sin();
    let n1 = a / (1.0 - e2 * phi1.sin().powi(2)).sqrt();
    let t1 = phi1.tan().powi(2);
    let c1 = e2 / (1.0 - e2) * phi1.cos().powi(2);
    let r1 = a * (1.0 - e2) / (1.0 - e2 * phi1.sin().powi(2)).powf(1.5);
    let d = x / (n1 * k0);
    let lat = phi1
        - (n1 * phi1.tan() / r1)
            * (d.powi(2) / 2.0
                - (5.0 + 3.0 * t1 + 10.0 * c1 - 4.0 * c1.powi(2)) * d.powi(4) / 24.0);
    let lon = lon0.to_radians() + (d - (1.0 + 2.0 * t1 + c1) * d.powi(3) / 6.0) / phi1.cos();
    arr_sv(vec![
        StrykeValue::float(lat.to_degrees()),
        StrykeValue::float(lon.to_degrees()),
    ])
}

pub fn geomag_declination(args: &[StrykeValue]) -> StrykeValue {
    // Magnetic declination via a single-dipole model anchored at the geomagnetic
    // north pole. Not WMM/IGRF-accurate; useful only for rough azimuth correction.
    let lat = arg_f64(args, 0).unwrap_or(0.0);
    let lon = arg_f64(args, 1).unwrap_or(0.0);
    let phi = lat.to_radians();
    let lambda = lon.to_radians();
    // Use simple dipole approximation. Magnetic North ~ 86.5N, 162.9W
    let m_lat = 86.5_f64.to_radians();
    let m_lon = (-162.9_f64).to_radians();
    let dlon = lambda - m_lon;
    let y = dlon.sin() * m_lat.cos();
    let x = phi.cos() * m_lat.sin() - phi.sin() * m_lat.cos() * dlon.cos();
    StrykeValue::float(y.atan2(x).to_degrees())
}

// ══════════════════════════════════════════════════════════════════════
// Base58 / Base91
// ══════════════════════════════════════════════════════════════════════

const BASE58_ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

fn base58_encode_bytes(input: &[u8]) -> String {
    let mut num: Vec<u8> = input.to_vec();
    let zeros = num.iter().take_while(|&&b| b == 0).count();
    let mut result = Vec::new();
    while !num.is_empty() {
        let mut rem = 0u32;
        let mut new_num = Vec::with_capacity(num.len());
        let mut started = false;
        for b in &num {
            let cur = rem * 256 + *b as u32;
            let q = cur / 58;
            rem = cur % 58;
            if started || q != 0 {
                new_num.push(q as u8);
                started = true;
            }
        }
        result.push(BASE58_ALPHABET[rem as usize]);
        num = new_num;
    }
    for _ in 0..zeros {
        result.push(BASE58_ALPHABET[0]);
    }
    result.reverse();
    String::from_utf8(result).unwrap_or_default()
}

fn base58_decode_bytes(input: &str) -> Vec<u8> {
    let zeros = input.chars().take_while(|&c| c == '1').count();
    let mut num: Vec<u8> = Vec::new();
    for c in input.chars() {
        let pos = match BASE58_ALPHABET.iter().position(|&b| b as char == c) {
            Some(p) => p as u32,
            None => return Vec::new(),
        };
        let mut carry = pos;
        for b in num.iter_mut() {
            carry += *b as u32 * 58;
            *b = (carry & 0xFF) as u8;
            carry >>= 8;
        }
        while carry > 0 {
            num.push((carry & 0xFF) as u8);
            carry >>= 8;
        }
    }
    let mut result = vec![0u8; zeros];
    result.extend(num.iter().rev());
    result
}

fn sha256d(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let a = Sha256::digest(data);
    let b = Sha256::digest(a);
    let mut out = [0u8; 32];
    out.copy_from_slice(&b);
    out
}

pub fn base58check_encode(args: &[StrykeValue]) -> StrykeValue {
    let payload = arg_str(args, 0).unwrap_or_default();
    let bytes = payload.into_bytes();
    let checksum = sha256d(&bytes);
    let mut all = bytes;
    all.extend_from_slice(&checksum[..4]);
    StrykeValue::string(base58_encode_bytes(&all))
}

pub fn base58check_decode(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = base58_decode_bytes(&s);
    if bytes.len() < 4 {
        return StrykeValue::UNDEF;
    }
    let (payload, checksum) = bytes.split_at(bytes.len() - 4);
    let h = sha256d(payload);
    if h[..4] != *checksum {
        return StrykeValue::UNDEF;
    }
    StrykeValue::string(String::from_utf8_lossy(payload).into_owned())
}

const BASE91_ALPHABET: &[u8] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!#$%&()*+,./:;<=>?@[]^_`{|}~\"";

pub fn base91_encode(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let input = s.as_bytes();
    let mut out = String::new();
    let mut b: u32 = 0;
    let mut n: u32 = 0;
    for &x in input {
        b |= (x as u32) << n;
        n += 8;
        if n > 13 {
            let mut v = b & 8191;
            if v > 88 {
                b >>= 13;
                n -= 13;
            } else {
                v = b & 16383;
                b >>= 14;
                n -= 14;
            }
            out.push(BASE91_ALPHABET[(v % 91) as usize] as char);
            out.push(BASE91_ALPHABET[(v / 91) as usize] as char);
        }
    }
    if n > 0 {
        out.push(BASE91_ALPHABET[(b % 91) as usize] as char);
        if n > 7 || b > 90 {
            out.push(BASE91_ALPHABET[(b / 91) as usize] as char);
        }
    }
    StrykeValue::string(out)
}

pub fn base91_decode(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let mut map = HashMap::new();
    for (i, &b) in BASE91_ALPHABET.iter().enumerate() {
        map.insert(b as char, i as u32);
    }
    let mut out = Vec::new();
    let mut b: u32 = 0;
    let mut n: u32 = 0;
    let mut v: i64 = -1;
    for ch in s.chars() {
        let c = match map.get(&ch) {
            Some(&c) => c,
            None => continue,
        };
        if v < 0 {
            v = c as i64;
        } else {
            v += (c as i64) * 91;
            b |= (v as u32) << n;
            n += if (v & 8191) > 88 { 13 } else { 14 };
            while n > 7 {
                out.push((b & 255) as u8);
                b >>= 8;
                n -= 8;
            }
            v = -1;
        }
    }
    if v >= 0 {
        b |= (v as u32) << n;
        out.push((b & 255) as u8);
    }
    StrykeValue::string(String::from_utf8_lossy(&out).into_owned())
}

#[allow(non_snake_case)]
pub fn basE91_encode(args: &[StrykeValue]) -> StrykeValue {
    base91_encode(args)
}

#[allow(non_snake_case)]
pub fn basE91_decode(args: &[StrykeValue]) -> StrykeValue {
    base91_decode(args)
}

const Z85_ALPHABET: &[u8] =
    b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.-:+=^!/*?&<>()[]{}@%$#";

pub fn z85_encode(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let input = s.as_bytes();
    if !input.len().is_multiple_of(4) {
        return StrykeValue::UNDEF;
    }
    let mut out = String::with_capacity(input.len() / 4 * 5);
    for chunk in input.chunks(4) {
        let mut value: u32 = 0;
        for &b in chunk {
            value = value * 256 + b as u32;
        }
        let mut buf = [0u8; 5];
        for i in (0..5).rev() {
            buf[i] = Z85_ALPHABET[(value % 85) as usize];
            value /= 85;
        }
        out.extend(buf.iter().map(|&b| b as char));
    }
    StrykeValue::string(out)
}

pub fn z85_decode(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    if !s.len().is_multiple_of(5) {
        return StrykeValue::UNDEF;
    }
    let mut map = HashMap::new();
    for (i, &b) in Z85_ALPHABET.iter().enumerate() {
        map.insert(b as char, i as u32);
    }
    let mut out = Vec::with_capacity(s.len() / 5 * 4);
    for chunk in s.as_bytes().chunks(5) {
        let mut value: u32 = 0;
        for &b in chunk {
            value = value * 85 + map.get(&(b as char)).copied().unwrap_or(0);
        }
        for i in (0..4).rev() {
            out.push(((value >> (i * 8)) & 0xFF) as u8);
        }
    }
    StrykeValue::string(String::from_utf8_lossy(&out).into_owned())
}

// ══════════════════════════════════════════════════════════════════════
// Astronomy
// ══════════════════════════════════════════════════════════════════════

pub fn modified_julian_date(args: &[StrykeValue]) -> StrykeValue {
    let unix = arg_i64(args, 0).unwrap_or(0);
    // MJD epoch is 1858-11-17, Unix epoch is 1970-01-01; difference is 40587 days.
    StrykeValue::float(40587.0 + unix as f64 / 86400.0)
}

pub fn julian_to_unix(args: &[StrykeValue]) -> StrykeValue {
    let jd = arg_f64(args, 0).unwrap_or(2440587.5); // = unix 0
    let unix = (jd - 2440587.5) * 86400.0;
    StrykeValue::integer(unix as i64)
}

pub fn unix_to_julian(args: &[StrykeValue]) -> StrykeValue {
    let unix = arg_i64(args, 0).unwrap_or(0);
    StrykeValue::float(2440587.5 + unix as f64 / 86400.0)
}

pub fn sidereal_time_greenwich(args: &[StrykeValue]) -> StrykeValue {
    let unix = arg_i64(args, 0).unwrap_or(0);
    let jd = 2440587.5 + unix as f64 / 86400.0;
    let t = (jd - 2451545.0) / 36525.0;
    let mut gst = 280.46061837 + 360.98564736629 * (jd - 2451545.0) + 0.000387933 * t.powi(2)
        - t.powi(3) / 38710000.0;
    gst = gst.rem_euclid(360.0);
    StrykeValue::float(gst / 15.0)
}

pub fn sidereal_time_local(args: &[StrykeValue]) -> StrykeValue {
    let gst_hours = sidereal_time_greenwich(args).to_number();
    let lon = arg_f64(args, 1).unwrap_or(0.0);
    let lst = (gst_hours + lon / 15.0).rem_euclid(24.0);
    StrykeValue::float(lst)
}

pub fn solar_noon_unix(args: &[StrykeValue]) -> StrykeValue {
    let lon = arg_f64(args, 0).unwrap_or(0.0);
    let unix = arg_i64(args, 1).unwrap_or(0);
    let day_start = unix - unix.rem_euclid(86400);
    // Day-of-year from the unix-day index (approximate, sufficient for EoT amplitude).
    let day_of_year = ((day_start as f64 / 86400.0).rem_euclid(365.2422)).floor();
    let b = 2.0 * std::f64::consts::PI * (day_of_year - 81.0) / 365.0;
    // Equation of time in minutes (Spencer/Carruthers form, ±16 min amplitude).
    let eot_min = 9.873 * (2.0 * b).sin() - 7.655 * b.sin();
    let noon = day_start + 43200 - (lon * 240.0) as i64 - (eot_min * 60.0) as i64;
    StrykeValue::integer(noon)
}

pub fn moon_age_days(args: &[StrykeValue]) -> StrykeValue {
    let unix = arg_i64(args, 0).unwrap_or(0);
    let synodic = 29.530588853_f64;
    let known_new_moon_unix = 947182440_f64; // 2000-01-06 18:14 UTC
    let days = (unix as f64 - known_new_moon_unix) / 86400.0;
    let age = days.rem_euclid(synodic);
    StrykeValue::float(age)
}

/// Approximate Earth-Moon distance in km via the anomalistic cycle
/// (perigee-to-perigee period 27.554 days). Modeled as a sinusoid between
/// ~364,000 km (perigee) and ~406,000 km (apogee). Anchored at the
/// 2000-01-19 20:00 UTC perigee. Accurate to a few thousand km; for
/// observation-grade precision use a real ephemeris.
pub fn moon_distance_km(args: &[StrykeValue]) -> StrykeValue {
    let unix = arg_i64(args, 0).unwrap_or(0);
    let anomalistic_days = 27.554549878_f64;
    // Verified perigee: 2000-01-19 20:00 UTC → unix 948_312_000.
    let known_perigee_unix = 948_312_000_f64;
    let days = (unix as f64 - known_perigee_unix) / 86400.0;
    let phase = (days / anomalistic_days) * 2.0 * std::f64::consts::PI;
    // Mean 384,400 km; amplitude ~21,000 km matches observed perigee/apogee bounds.
    StrykeValue::float(384_400.0 - 21_000.0 * phase.cos())
}

pub fn season_of_year(args: &[StrykeValue]) -> StrykeValue {
    let month = arg_i64(args, 0).unwrap_or(1);
    let day = arg_i64(args, 1).unwrap_or(1);
    let hemisphere = arg_str(args, 2).unwrap_or_else(|| "N".into());
    let season = match month {
        12 if day >= 21 => "Winter",
        1 | 2 => "Winter",
        3 if day < 20 => "Winter",
        3 if day >= 20 => "Spring",
        4 | 5 => "Spring",
        6 if day < 21 => "Spring",
        6 if day >= 21 => "Summer",
        7 | 8 => "Summer",
        9 if day < 22 => "Summer",
        9 if day >= 22 => "Autumn",
        10 | 11 => "Autumn",
        12 => "Autumn",
        _ => "Winter",
    };
    let result = if hemisphere == "S" {
        match season {
            "Winter" => "Summer",
            "Summer" => "Winter",
            "Spring" => "Autumn",
            "Autumn" => "Spring",
            x => x,
        }
    } else {
        season
    };
    StrykeValue::string(result.to_string())
}

pub fn new_moon_julian(args: &[StrykeValue]) -> StrykeValue {
    let cycle = arg_i64(args, 0).unwrap_or(0);
    let synodic = 29.530588853;
    let jd = 2451550.09765 + synodic * cycle as f64;
    StrykeValue::float(jd)
}

pub fn full_moon_julian(args: &[StrykeValue]) -> StrykeValue {
    let cycle = arg_i64(args, 0).unwrap_or(0);
    let synodic = 29.530588853;
    let jd = 2451550.09765 + synodic * cycle as f64 + synodic / 2.0;
    StrykeValue::float(jd)
}

// ══════════════════════════════════════════════════════════════════════
// CRC variants
// ══════════════════════════════════════════════════════════════════════

fn crc_bitwise(
    data: &[u8],
    poly: u64,
    init: u64,
    refin: bool,
    refout: bool,
    xorout: u64,
    width: u32,
) -> u64 {
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    };
    let top = 1u64 << (width - 1);
    let mut crc = init & mask;
    for &b in data {
        let byte = if refin { b.reverse_bits() } else { b };
        crc ^= (byte as u64) << (width - 8);
        for _ in 0..8 {
            if crc & top != 0 {
                crc = (crc << 1) ^ poly;
            } else {
                crc <<= 1;
            }
            crc &= mask;
        }
    }
    if refout {
        let mut rev = 0u64;
        let mut val = crc;
        for _ in 0..width {
            rev = (rev << 1) | (val & 1);
            val >>= 1;
        }
        rev ^ xorout
    } else {
        crc ^ xorout
    }
}

pub fn crc24(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::integer(crc_bitwise(s.as_bytes(), 0x864CFB, 0xB704CE, false, false, 0, 24) as i64)
}

pub fn crc64_ecma(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::integer(
        crc_bitwise(s.as_bytes(), 0x42F0E1EBA9EA3693, 0, false, false, 0, 64) as i64,
    )
}

pub fn crc64_xz(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::integer(crc_bitwise(
        s.as_bytes(),
        0x42F0E1EBA9EA3693,
        u64::MAX,
        true,
        true,
        u64::MAX,
        64,
    ) as i64)
}

pub fn crc6_itu(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::integer(crc_bitwise(s.as_bytes(), 0x03, 0, true, true, 0, 6) as i64)
}

pub fn crc10_atm(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::integer(crc_bitwise(s.as_bytes(), 0x233, 0, false, false, 0, 10) as i64)
}

pub fn crc12_dect(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::integer(crc_bitwise(s.as_bytes(), 0x80F, 0, false, false, 0, 12) as i64)
}

pub fn crc32_bzip2(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::integer(crc_bitwise(
        s.as_bytes(),
        0x04C11DB7,
        u32::MAX as u64,
        false,
        false,
        u32::MAX as u64,
        32,
    ) as i64)
}

pub fn crc32_jamcrc(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::integer(
        crc_bitwise(s.as_bytes(), 0x04C11DB7, u32::MAX as u64, true, true, 0, 32) as i64,
    )
}

pub fn crc32_mpeg2(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::integer(crc_bitwise(
        s.as_bytes(),
        0x04C11DB7,
        u32::MAX as u64,
        false,
        false,
        0,
        32,
    ) as i64)
}

pub fn crc32_xfer(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::integer(crc_bitwise(s.as_bytes(), 0x000000AF, 0, false, false, 0, 32) as i64)
}

pub fn adler32_combine(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_u64(args, 0).unwrap_or(0);
    let b = arg_u64(args, 1).unwrap_or(0);
    let blen = arg_i64(args, 2).unwrap_or(0).max(0) as u64;
    const BASE: u64 = 65521;
    if blen == 0 {
        return StrykeValue::integer(a as i64);
    }
    let blen = blen % BASE;
    let mut sum1 = a & 0xFFFF;
    let mut sum2 = (a >> 16) & 0xFFFF;
    sum2 = (sum2 + blen * sum1) % BASE;
    sum1 = (sum1 + (b & 0xFFFF) - 1) % BASE;
    sum2 = (sum2 + ((b >> 16) & 0xFFFF) - blen) % BASE;
    StrykeValue::integer(((sum2 << 16) | sum1) as i64)
}

pub fn fletcher16(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let mut sum1: u16 = 0;
    let mut sum2: u16 = 0;
    for b in s.bytes() {
        sum1 = (sum1 as u32 + b as u32).rem_euclid(255) as u16;
        sum2 = (sum2 as u32 + sum1 as u32).rem_euclid(255) as u16;
    }
    StrykeValue::integer(((sum2 as u32) << 8 | sum1 as u32) as i64)
}

pub fn fletcher32(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = s.as_bytes();
    let mut sum1: u32 = 0;
    let mut sum2: u32 = 0;
    let mut i = 0;
    while i + 1 < bytes.len() {
        let word = (bytes[i] as u32) | ((bytes[i + 1] as u32) << 8);
        sum1 = (sum1 + word).rem_euclid(65535);
        sum2 = (sum2 + sum1).rem_euclid(65535);
        i += 2;
    }
    if i < bytes.len() {
        sum1 = (sum1 + bytes[i] as u32).rem_euclid(65535);
        sum2 = (sum2 + sum1).rem_euclid(65535);
    }
    StrykeValue::integer(((sum2 << 16) | sum1) as i64)
}

pub fn fletcher64(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = s.as_bytes();
    let mut sum1: u64 = 0;
    let mut sum2: u64 = 0;
    let mut i = 0;
    while i + 3 < bytes.len() {
        let word = (bytes[i] as u64)
            | ((bytes[i + 1] as u64) << 8)
            | ((bytes[i + 2] as u64) << 16)
            | ((bytes[i + 3] as u64) << 24);
        sum1 = (sum1 + word).rem_euclid(4294967295);
        sum2 = (sum2 + sum1).rem_euclid(4294967295);
        i += 4;
    }
    StrykeValue::integer(((sum2 << 32) | sum1) as i64)
}

// ══════════════════════════════════════════════════════════════════════
// Color blending and gamma
// ══════════════════════════════════════════════════════════════════════

fn unpack_rgb(v: &StrykeValue) -> (f64, f64, f64) {
    let xs = as_vec_sv(v);
    let r = xs.first().map(|x| x.to_number()).unwrap_or(0.0);
    let g = xs.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let b = xs.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    (r, g, b)
}

fn pack_rgb(r: f64, g: f64, b: f64) -> StrykeValue {
    arr_sv(vec![
        StrykeValue::float(r.clamp(0.0, 255.0)),
        StrykeValue::float(g.clamp(0.0, 255.0)),
        StrykeValue::float(b.clamp(0.0, 255.0)),
    ])
}

pub fn rgb_blend_normal(args: &[StrykeValue]) -> StrykeValue {
    let (r1, g1, b1) = unpack_rgb(args.first().unwrap_or(&StrykeValue::UNDEF));
    let (r2, g2, b2) = unpack_rgb(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let alpha = arg_f64(args, 2).unwrap_or(1.0).clamp(0.0, 1.0);
    pack_rgb(
        r1 * (1.0 - alpha) + r2 * alpha,
        g1 * (1.0 - alpha) + g2 * alpha,
        b1 * (1.0 - alpha) + b2 * alpha,
    )
}

pub fn rgb_blend_multiply(args: &[StrykeValue]) -> StrykeValue {
    let (r1, g1, b1) = unpack_rgb(args.first().unwrap_or(&StrykeValue::UNDEF));
    let (r2, g2, b2) = unpack_rgb(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    pack_rgb(r1 * r2 / 255.0, g1 * g2 / 255.0, b1 * b2 / 255.0)
}

pub fn rgb_blend_screen(args: &[StrykeValue]) -> StrykeValue {
    let (r1, g1, b1) = unpack_rgb(args.first().unwrap_or(&StrykeValue::UNDEF));
    let (r2, g2, b2) = unpack_rgb(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    pack_rgb(
        255.0 - (255.0 - r1) * (255.0 - r2) / 255.0,
        255.0 - (255.0 - g1) * (255.0 - g2) / 255.0,
        255.0 - (255.0 - b1) * (255.0 - b2) / 255.0,
    )
}

fn overlay_channel(a: f64, b: f64) -> f64 {
    if a < 128.0 {
        2.0 * a * b / 255.0
    } else {
        255.0 - 2.0 * (255.0 - a) * (255.0 - b) / 255.0
    }
}

pub fn rgb_blend_overlay(args: &[StrykeValue]) -> StrykeValue {
    let (r1, g1, b1) = unpack_rgb(args.first().unwrap_or(&StrykeValue::UNDEF));
    let (r2, g2, b2) = unpack_rgb(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    pack_rgb(
        overlay_channel(r1, r2),
        overlay_channel(g1, g2),
        overlay_channel(b1, b2),
    )
}

pub fn rgb_blend_darken(args: &[StrykeValue]) -> StrykeValue {
    let (r1, g1, b1) = unpack_rgb(args.first().unwrap_or(&StrykeValue::UNDEF));
    let (r2, g2, b2) = unpack_rgb(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    pack_rgb(r1.min(r2), g1.min(g2), b1.min(b2))
}

pub fn rgb_blend_lighten(args: &[StrykeValue]) -> StrykeValue {
    let (r1, g1, b1) = unpack_rgb(args.first().unwrap_or(&StrykeValue::UNDEF));
    let (r2, g2, b2) = unpack_rgb(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    pack_rgb(r1.max(r2), g1.max(g2), b1.max(b2))
}

pub fn rgb_blend_color_dodge(args: &[StrykeValue]) -> StrykeValue {
    let (r1, g1, b1) = unpack_rgb(args.first().unwrap_or(&StrykeValue::UNDEF));
    let (r2, g2, b2) = unpack_rgb(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let dodge = |a: f64, b: f64| {
        if b >= 255.0 {
            255.0
        } else {
            (a * 255.0 / (255.0 - b)).min(255.0)
        }
    };
    pack_rgb(dodge(r1, r2), dodge(g1, g2), dodge(b1, b2))
}

pub fn rgb_blend_color_burn(args: &[StrykeValue]) -> StrykeValue {
    let (r1, g1, b1) = unpack_rgb(args.first().unwrap_or(&StrykeValue::UNDEF));
    let (r2, g2, b2) = unpack_rgb(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let burn = |a: f64, b: f64| {
        if b == 0.0 {
            0.0
        } else {
            (255.0 - (255.0 - a) * 255.0 / b).max(0.0)
        }
    };
    pack_rgb(burn(r1, r2), burn(g1, g2), burn(b1, b2))
}

pub fn gamma_correct(args: &[StrykeValue]) -> StrykeValue {
    let v = arg_f64(args, 0).unwrap_or(0.0).clamp(0.0, 1.0);
    let gamma = arg_f64(args, 1).unwrap_or(2.2);
    StrykeValue::float(v.powf(1.0 / gamma))
}

pub fn gamma_uncorrect(args: &[StrykeValue]) -> StrykeValue {
    let v = arg_f64(args, 0).unwrap_or(0.0).clamp(0.0, 1.0);
    let gamma = arg_f64(args, 1).unwrap_or(2.2);
    StrykeValue::float(v.powf(gamma))
}

// ══════════════════════════════════════════════════════════════════════
// Compression primitives
// ══════════════════════════════════════════════════════════════════════

pub fn rle_compress(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let mut out = String::new();
    let mut chars = s.chars();
    if let Some(mut prev) = chars.next() {
        let mut count = 1usize;
        for c in chars {
            if c == prev && count < 9 {
                count += 1;
            } else {
                out.push((b'0' + count as u8) as char);
                out.push(prev);
                prev = c;
                count = 1;
            }
        }
        out.push((b'0' + count as u8) as char);
        out.push(prev);
    }
    StrykeValue::string(out)
}

pub fn rle_decompress(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i + 1 < chars.len() {
        let count = chars[i].to_digit(10).unwrap_or(1) as usize;
        let c = chars[i + 1];
        for _ in 0..count {
            out.push(c);
        }
        i += 2;
    }
    StrykeValue::string(out)
}

pub fn delta_encode(args: &[StrykeValue]) -> StrykeValue {
    let xs: Vec<i64> = args
        .first()
        .map(as_vec_sv)
        .unwrap_or_default()
        .iter()
        .map(|x| x.to_int())
        .collect();
    if xs.is_empty() {
        return arr_sv(vec![]);
    }
    let mut out = vec![StrykeValue::integer(xs[0])];
    for i in 1..xs.len() {
        out.push(StrykeValue::integer(xs[i] - xs[i - 1]));
    }
    arr_sv(out)
}

pub fn delta_decode(args: &[StrykeValue]) -> StrykeValue {
    let xs: Vec<i64> = args
        .first()
        .map(as_vec_sv)
        .unwrap_or_default()
        .iter()
        .map(|x| x.to_int())
        .collect();
    if xs.is_empty() {
        return arr_sv(vec![]);
    }
    let mut out = vec![StrykeValue::integer(xs[0])];
    let mut acc = xs[0];
    for x in &xs[1..] {
        acc += *x;
        out.push(StrykeValue::integer(acc));
    }
    arr_sv(out)
}

pub fn zigzag_encode(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_i64(args, 0).unwrap_or(0);
    StrykeValue::integer((x << 1) ^ (x >> 63))
}

pub fn zigzag_decode(args: &[StrykeValue]) -> StrykeValue {
    let x = arg_u64(args, 0).unwrap_or(0);
    let result = (x >> 1) as i64 ^ -((x & 1) as i64);
    StrykeValue::integer(result)
}

pub fn varint_encode(args: &[StrykeValue]) -> StrykeValue {
    let mut x = arg_u64(args, 0).unwrap_or(0);
    let mut out: Vec<StrykeValue> = Vec::new();
    while x >= 0x80 {
        out.push(StrykeValue::integer(((x & 0x7F) | 0x80) as i64));
        x >>= 7;
    }
    out.push(StrykeValue::integer(x as i64));
    arr_sv(out)
}

pub fn varint_decode(args: &[StrykeValue]) -> StrykeValue {
    let bytes: Vec<u8> = args
        .first()
        .map(as_vec_sv)
        .unwrap_or_default()
        .iter()
        .map(|x| x.to_int() as u8)
        .collect();
    let mut result: u64 = 0;
    let mut shift = 0;
    for b in bytes {
        result |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    StrykeValue::integer(result as i64)
}

pub fn bwt_transform(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = s.as_bytes();
    let n = bytes.len();
    if n == 0 {
        return make_hash(vec![
            ("data", StrykeValue::string(String::new())),
            ("index", StrykeValue::integer(0)),
        ]);
    }
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&a, &b| {
        let mut i = 0;
        loop {
            let ca = bytes[(a + i) % n];
            let cb = bytes[(b + i) % n];
            if ca != cb {
                return ca.cmp(&cb);
            }
            i += 1;
            if i >= n {
                return std::cmp::Ordering::Equal;
            }
        }
    });
    let last: Vec<u8> = indices.iter().map(|&i| bytes[(i + n - 1) % n]).collect();
    let idx = indices.iter().position(|&i| i == 0).unwrap_or(0);
    make_hash(vec![
        (
            "data",
            StrykeValue::string(String::from_utf8_lossy(&last).into_owned()),
        ),
        ("index", StrykeValue::integer(idx as i64)),
    ])
}

pub fn bwt_invert(args: &[StrykeValue]) -> StrykeValue {
    let data = arg_str(args, 0).unwrap_or_default();
    let idx = arg_i64(args, 1).unwrap_or(0).max(0) as usize;
    let bytes = data.as_bytes();
    let n = bytes.len();
    if n == 0 {
        return StrykeValue::string(String::new());
    }
    let mut sorted: Vec<u8> = bytes.to_vec();
    sorted.sort();
    let mut next = vec![0usize; n];
    let mut used = vec![false; n];
    for (i, &c) in bytes.iter().enumerate() {
        for j in 0..n {
            if !used[j] && sorted[j] == c {
                next[j] = i;
                used[j] = true;
                break;
            }
        }
    }
    let mut out = Vec::with_capacity(n);
    let mut p = idx.min(n - 1);
    for _ in 0..n {
        out.push(sorted[p]);
        p = next[p];
    }
    StrykeValue::string(String::from_utf8_lossy(&out).into_owned())
}

pub fn huffman_encode(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    if s.is_empty() {
        return make_hash(vec![
            ("bits", StrykeValue::string(String::new())),
            ("table", arr_sv(vec![])),
        ]);
    }
    let mut freq: HashMap<char, u64> = HashMap::new();
    for c in s.chars() {
        *freq.entry(c).or_insert(0) += 1;
    }
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    #[derive(Eq, PartialEq)]
    struct Node {
        freq: u64,
        ch: Option<char>,
        left: Option<Box<Node>>,
        right: Option<Box<Node>>,
    }
    impl Ord for Node {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            other.freq.cmp(&self.freq)
        }
    }
    impl PartialOrd for Node {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    let mut heap: BinaryHeap<Reverse<Node>> = BinaryHeap::new();
    for (&c, &f) in &freq {
        heap.push(Reverse(Node {
            freq: f,
            ch: Some(c),
            left: None,
            right: None,
        }));
    }
    while heap.len() > 1 {
        let a = heap.pop().unwrap().0;
        let b = heap.pop().unwrap().0;
        heap.push(Reverse(Node {
            freq: a.freq + b.freq,
            ch: None,
            left: Some(Box::new(a)),
            right: Some(Box::new(b)),
        }));
    }
    let root = heap.pop().unwrap().0;
    let mut codes: HashMap<char, String> = HashMap::new();
    fn walk(node: &Node, prefix: String, codes: &mut HashMap<char, String>) {
        if let Some(c) = node.ch {
            codes.insert(
                c,
                if prefix.is_empty() {
                    "0".into()
                } else {
                    prefix
                },
            );
            return;
        }
        if let Some(ref l) = node.left {
            walk(l, prefix.clone() + "0", codes);
        }
        if let Some(ref r) = node.right {
            walk(r, prefix + "1", codes);
        }
    }
    walk(&root, String::new(), &mut codes);
    let bits: String = s
        .chars()
        .map(|c| codes.get(&c).cloned().unwrap_or_default())
        .collect();
    let table: Vec<StrykeValue> = codes
        .iter()
        .map(|(&c, code)| {
            arr_sv(vec![
                StrykeValue::string(c.to_string()),
                StrykeValue::string(code.clone()),
            ])
        })
        .collect();
    make_hash(vec![
        ("bits", StrykeValue::string(bits)),
        ("table", arr_sv(table)),
    ])
}

pub fn huffman_decode(args: &[StrykeValue]) -> StrykeValue {
    let bits = arg_str(args, 0).unwrap_or_default();
    let table_v = args.get(1).map(as_vec_sv).unwrap_or_default();
    let mut decoder: HashMap<String, char> = HashMap::new();
    for entry in table_v {
        let pair = as_vec_sv(&entry);
        if pair.len() < 2 {
            continue;
        }
        let c = pair[0].as_str_or_empty().chars().next();
        let code = pair[1].as_str_or_empty();
        if let Some(c) = c {
            decoder.insert(code, c);
        }
    }
    let mut out = String::new();
    let mut buf = String::new();
    for b in bits.chars() {
        buf.push(b);
        if let Some(&c) = decoder.get(&buf) {
            out.push(c);
            buf.clear();
        }
    }
    StrykeValue::string(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sv_s(x: &str) -> StrykeValue {
        StrykeValue::string(x.to_string())
    }
    fn sv_i(x: i64) -> StrykeValue {
        StrykeValue::integer(x)
    }
    fn sv(x: f64) -> StrykeValue {
        StrykeValue::float(x)
    }

    #[test]
    fn soundex_robert() {
        assert_eq!(soundex_v1(&[sv_s("Robert")]).as_str_or_empty(), "R163");
    }

    #[test]
    fn soundex_pad() {
        assert_eq!(soundex_v1(&[sv_s("Lee")]).as_str_or_empty(), "L000");
    }

    #[test]
    fn vincenty_bearing_known() {
        // NYC (40.7128, -74.0060) to London (51.5074, -0.1278) bearing ~51 degrees
        let r = vincenty_bearing(&[sv(40.7128), sv(-74.006), sv(51.5074), sv(-0.1278)]).to_number();
        assert!((r - 51.0).abs() < 3.0, "got {r}");
    }

    #[test]
    fn utm_roundtrip() {
        // Roundtrip via UTM: 40.7128N, -74.006E
        let u = lat_lon_to_utm(&[sv(40.7128), sv(-74.006)]);
        let xs = as_vec_sv(&u);
        let back = utm_to_lat_lon(&xs);
        let xs = as_vec_sv(&back);
        let lat = xs[0].to_number();
        let lon = xs[1].to_number();
        assert!((lat - 40.7128).abs() < 0.001, "lat={lat}");
        assert!((lon - (-74.006)).abs() < 0.001, "lon={lon}");
    }

    #[test]
    fn base58_roundtrip() {
        let s = sv_s("hello world");
        let enc = base58check_encode(&[s.clone()]);
        let dec = base58check_decode(&[enc]);
        assert_eq!(dec.as_str_or_empty(), "hello world");
    }

    #[test]
    fn base91_roundtrip() {
        let s = sv_s("The quick brown fox");
        let enc = base91_encode(&[s.clone()]);
        let dec = base91_decode(&[enc]);
        assert_eq!(dec.as_str_or_empty(), "The quick brown fox");
    }

    #[test]
    fn z85_roundtrip() {
        let s = sv_s("12345678"); // 8 bytes, divisible by 4
        let enc = z85_encode(&[s.clone()]);
        let dec = z85_decode(&[enc]);
        assert_eq!(dec.as_str_or_empty(), "12345678");
    }

    #[test]
    fn julian_roundtrip() {
        let jd = unix_to_julian(&[sv_i(1672531200)]); // 2023-01-01 UTC
        let unix = julian_to_unix(&[jd]);
        assert_eq!(unix.to_int(), 1672531200);
    }

    #[test]
    fn crc24_known() {
        // CRC24 OpenPGP of "" is 0xB704CE
        let r = crc24(&[sv_s("")]).to_int();
        assert_eq!(r, 0xB704CE);
    }

    #[test]
    fn fletcher16_known() {
        // Fletcher-16 of "abcde" = 0xC8F0
        let r = fletcher16(&[sv_s("abcde")]).to_int();
        assert_eq!(r as u32, 0xC8F0);
    }

    #[test]
    fn rgb_blend_multiply_zero() {
        let r = rgb_blend_multiply(&[
            arr_sv(vec![sv(255.0), sv(0.0), sv(128.0)]),
            arr_sv(vec![sv(0.0), sv(255.0), sv(128.0)]),
        ]);
        let xs = as_vec_sv(&r);
        assert_eq!(xs[0].to_number(), 0.0);
        assert_eq!(xs[1].to_number(), 0.0);
        assert!((xs[2].to_number() - 64.25).abs() < 1.0);
    }

    #[test]
    fn rle_roundtrip() {
        let s = sv_s("aaabbc");
        let enc = rle_compress(&[s]);
        let dec = rle_decompress(&[enc]);
        assert_eq!(dec.as_str_or_empty(), "aaabbc");
    }

    #[test]
    fn zigzag_known() {
        assert_eq!(zigzag_encode(&[sv_i(0)]).to_int(), 0);
        assert_eq!(zigzag_encode(&[sv_i(-1)]).to_int(), 1);
        assert_eq!(zigzag_encode(&[sv_i(1)]).to_int(), 2);
        assert_eq!(
            zigzag_decode(&[sv_i(zigzag_encode(&[sv_i(-100)]).to_int())]).to_int(),
            -100
        );
    }

    #[test]
    fn bwt_roundtrip() {
        let s = sv_s("banana");
        let t = bwt_transform(&[s]);
        if let Some(h) = t.as_hash_ref() {
            let h = h.read();
            let data = h.get("data").cloned().unwrap();
            let idx = h.get("index").cloned().unwrap();
            let back = bwt_invert(&[data, idx]);
            assert_eq!(back.as_str_or_empty(), "banana");
        }
    }

    #[test]
    fn huffman_roundtrip() {
        let s = "this is a test of huffman encoding";
        let enc = huffman_encode(&[sv_s(s)]);
        if let Some(h) = enc.as_hash_ref() {
            let h = h.read();
            let bits = h.get("bits").cloned().unwrap();
            let table = h.get("table").cloned().unwrap();
            let dec = huffman_decode(&[bits, table]);
            assert_eq!(dec.as_str_or_empty(), s);
        }
    }

    #[test]
    fn modified_julian_date_unix_zero() {
        // Unix epoch 1970-01-01 = MJD 40587
        let r = modified_julian_date(&[sv_i(0)]).to_number();
        assert!((r - 40587.0).abs() < 1e-9);
    }
}
