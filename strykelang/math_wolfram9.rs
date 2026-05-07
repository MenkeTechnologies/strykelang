// ─────────────────────────────────────────────────────────────────────────────
// Batch 9 — long-tail Mathematica/MATLAB/R/Python utilities: list/string
// positional helpers, datetime navigation, calendar quirks (Easter, zodiac),
// WCAG accessibility colour metrics, music theory (chords/scales/intervals),
// astronomy (moon phase, equation of time), group / permutation primitives,
// linguistics readability, regression diagnostics, more combinatorics counts,
// and PRNG / hashing micro-utilities.
// ─────────────────────────────────────────────────────────────────────────────

// ── 1. List / array positional helpers ──────────────────────────────────────

/// `partition_at` — Partition at. Returns a list.
fn builtin_partition_at(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let arr = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let idx = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = arr.len();
    let split = idx.min(n);
    let left: Vec<PerlValue> = arr[..split].to_vec();
    let right: Vec<PerlValue> = arr[split..].to_vec();
    Ok(PerlValue::array(vec![
        PerlValue::array(left),
        PerlValue::array(right),
    ]))
}

/// `drop_at` — Drop at. Returns a list.
fn builtin_drop_at(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut arr = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let idx = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let n = arr.len() as i64;
    if idx < 0 || idx >= n {
        return Ok(PerlValue::array(arr));
    }
    arr.remove(idx as usize);
    Ok(PerlValue::array(arr))
}

/// `insert_at_idx` — Insert at idx. Returns a list.
fn builtin_insert_at_idx(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut arr = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let idx = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let val = args.get(2).cloned().unwrap_or(PerlValue::UNDEF);
    arr.insert(idx.min(arr.len()), val);
    Ok(PerlValue::array(arr))
}

/// `replace_at_index` — Replace at index. Returns a list.
fn builtin_replace_at_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut arr = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let idx = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let val = args.get(2).cloned().unwrap_or(PerlValue::UNDEF);
    if idx < arr.len() {
        arr[idx] = val;
    }
    Ok(PerlValue::array(arr))
}

/// `swap_indices` — Swap indices. Returns a list.
fn builtin_swap_indices(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut arr = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let i = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let j = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    if i < arr.len() && j < arr.len() {
        arr.swap(i, j);
    }
    Ok(PerlValue::array(arr))
}

/// `nth_largest` — Nth largest. Returns a float.
fn builtin_nth_largest(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    if xs.is_empty() || n > xs.len() {
        return Ok(PerlValue::UNDEF);
    }
    xs.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    Ok(PerlValue::float(xs[n - 1]))
}

/// `nth_smallest` — Nth smallest. Returns a float.
fn builtin_nth_smallest(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    if xs.is_empty() || n > xs.len() {
        return Ok(PerlValue::UNDEF);
    }
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(PerlValue::float(xs[n - 1]))
}

/// `position_of_all_matching` — Position of all matching. Returns an integer.
fn builtin_position_of_all_matching(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let arr = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let target = args.get(1).cloned().unwrap_or(PerlValue::UNDEF);
    let target_s = target.to_string();
    let positions: Vec<PerlValue> = arr
        .iter()
        .enumerate()
        .filter(|(_, v)| v.to_string() == target_s)
        .map(|(i, _)| PerlValue::integer(i as i64))
        .collect();
    Ok(PerlValue::array(positions))
}

// ── 2. String positional helpers ────────────────────────────────────────────

/// `string_take_first` — String take first. Returns a string.
fn builtin_string_take_first(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    Ok(PerlValue::string(s.chars().take(n).collect()))
}

/// `string_take_last` — String take last. Returns a string.
fn builtin_string_take_last(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let chars: Vec<char> = s.chars().collect();
    let m = chars.len();
    let start = m.saturating_sub(n);
    Ok(PerlValue::string(chars[start..].iter().collect()))
}

/// `string_drop_first` — String drop first. Returns a string.
fn builtin_string_drop_first(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    Ok(PerlValue::string(s.chars().skip(n).collect()))
}

/// `string_drop_last` — String drop last. Returns a string.
fn builtin_string_drop_last(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let chars: Vec<char> = s.chars().collect();
    let m = chars.len();
    let end = m.saturating_sub(n);
    Ok(PerlValue::string(chars[..end].iter().collect()))
}

/// Naïve English pluralisation (s/es/ies rules). Adequate for code generators.
fn builtin_pluralize_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    if s.is_empty() {
        return Ok(PerlValue::string(s));
    }
    let lower = s.to_ascii_lowercase();
    let plural = if lower.ends_with("y")
        && lower.len() > 1
        && !"aeiou".contains(lower.chars().nth(lower.len() - 2).unwrap_or(' '))
    {
        format!("{}ies", &s[..s.len() - 1])
    } else if lower.ends_with("s") || lower.ends_with("x") || lower.ends_with("z")
        || lower.ends_with("sh") || lower.ends_with("ch")
    {
        format!("{}es", s)
    } else {
        format!("{}s", s)
    };
    Ok(PerlValue::string(plural))
}

/// Naïve English singularisation (inverse of pluralize_simple).
fn builtin_singularize_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    if s.is_empty() {
        return Ok(PerlValue::string(s));
    }
    let lower = s.to_ascii_lowercase();
    let singular = if lower.ends_with("ies") {
        format!("{}y", &s[..s.len() - 3])
    } else if lower.ends_with("es") && (lower.ends_with("ses") || lower.ends_with("xes")
        || lower.ends_with("zes") || lower.ends_with("shes") || lower.ends_with("ches"))
    {
        s[..s.len() - 2].to_string()
    } else if lower.ends_with("s") {
        s[..s.len() - 1].to_string()
    } else {
        s
    };
    Ok(PerlValue::string(singular))
}

/// Title-case each word (preserves separators).
fn builtin_capitalize_words(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut out = String::with_capacity(s.len());
    let mut new_word = true;
    for c in s.chars() {
        if c.is_whitespace() || c == '-' || c == '_' {
            out.push(c);
            new_word = true;
        } else if new_word {
            out.extend(c.to_uppercase());
            new_word = false;
        } else {
            out.extend(c.to_lowercase());
        }
    }
    Ok(PerlValue::string(out))
}

/// Render a 2-D matrix of values as a fixed-width ASCII table.
fn builtin_format_table_simple(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let rows = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| arg_to_vec(r).iter().map(|v| v.to_string()).collect())
        .collect();
    if cells.is_empty() {
        return Ok(PerlValue::string(String::new()));
    }
    let cols = cells.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = vec![0_usize; cols];
    for row in &cells {
        for (j, v) in row.iter().enumerate() {
            if v.len() > widths[j] {
                widths[j] = v.len();
            }
        }
    }
    let mut out = String::new();
    for row in &cells {
        for (j, v) in row.iter().enumerate() {
            out.push_str(&format!("{:>width$}", v, width = widths[j] + 1));
            if j + 1 < row.len() {
                out.push(' ');
            }
        }
        out.push('\n');
    }
    Ok(PerlValue::string(out))
}

// ── 3. Date / calendar navigation ───────────────────────────────────────────

fn ymd_to_days(y: i64, m: i64, d: i64) -> i64 {
    // Rata Die proleptic Gregorian.
    let (y, m) = if m <= 2 { (y - 1, m + 12) } else { (y, m) };
    365 * y + y / 4 - y / 100 + y / 400 + (153 * (m - 3) + 2) / 5 + d - 306
}

fn days_to_ymd(d: i64) -> (i64, i64, i64) {
    let z = d + 306;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as i64;
    let month = (mp as i64 + if mp < 10 { 3 } else { -9 }) as i64;
    let year = if month <= 2 { y + 1 } else { y };
    (year, month, day)
}

/// `days_between` — Days between. Returns an integer.
fn builtin_days_between(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y1 = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let m1 = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d1 = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let y2 = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let m2 = args.get(4).map(|v| v.to_number() as i64).unwrap_or(1);
    let d2 = args.get(5).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(PerlValue::integer(ymd_to_days(y2, m2, d2) - ymd_to_days(y1, m1, d1)))
}

/// `weeks_between` — Weeks between. Returns a float.
fn builtin_weeks_between(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = builtin_days_between(args)?.to_number();
    Ok(PerlValue::float(v / 7.0))
}

/// `months_between` — Months between. Returns an integer.
fn builtin_months_between(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y1 = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let m1 = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let _d1 = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let y2 = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let m2 = args.get(4).map(|v| v.to_number() as i64).unwrap_or(1);
    let _d2 = args.get(5).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(PerlValue::integer(12 * (y2 - y1) + (m2 - m1)))
}

/// `years_between` — Years between. Returns an integer.
fn builtin_years_between(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y1 = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let _m1 = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let y2 = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let _m2 = args.get(4).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(PerlValue::integer(y2 - y1))
}

/// `first_of_month` — First of month. Returns an integer.
fn builtin_first_of_month(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(PerlValue::array(vec![
        PerlValue::integer(y),
        PerlValue::integer(m),
        PerlValue::integer(1),
    ]))
}

/// `last_of_month` — Last of month. Returns an integer.
fn builtin_last_of_month(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let next_m = if m == 12 { 1 } else { m + 1 };
    let next_y = if m == 12 { y + 1 } else { y };
    let last_day = ymd_to_days(next_y, next_m, 1) - 1;
    let (y_, m_, d_) = days_to_ymd(last_day);
    Ok(PerlValue::array(vec![
        PerlValue::integer(y_),
        PerlValue::integer(m_),
        PerlValue::integer(d_),
    ]))
}

/// Day of week (0 = Mon, 6 = Sun) for a given Y/M/D.
fn builtin_day_of_week_iso(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let days = ymd_to_days(y, m, d);
    Ok(PerlValue::integer(days.rem_euclid(7)))
}

/// Easter Sunday for a Gregorian year (Anonymous Gregorian / Meeus algorithm).
fn builtin_easter_sunday(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let a = y % 19;
    let b = y / 100;
    let c = y % 100;
    let d_v = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d_v - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = ((h + l - 7 * m + 114) % 31) + 1;
    Ok(PerlValue::array(vec![
        PerlValue::integer(y),
        PerlValue::integer(month),
        PerlValue::integer(day),
    ]))
}

/// Chinese zodiac for a year. Returns one of the 12 animal names.
fn builtin_chinese_zodiac(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let names = [
        "Rat", "Ox", "Tiger", "Rabbit", "Dragon", "Snake", "Horse", "Goat", "Monkey",
        "Rooster", "Dog", "Pig",
    ];
    let idx = (y - 4).rem_euclid(12) as usize;
    Ok(PerlValue::string(names[idx].to_string()))
}

/// ISO 8601 week number of a date.
fn builtin_iso_week_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let days = ymd_to_days(y, m, d);
    let dow = days.rem_euclid(7); // 0 = Mon
    // Thursday of this ISO week:
    let thursday = days + (3 - dow);
    // Thursday of week 1 of the same ISO year:
    let (yy, _, _) = days_to_ymd(thursday);
    let jan4 = ymd_to_days(yy, 1, 4);
    let jan4_dow = jan4.rem_euclid(7);
    let thursday_of_week1 = jan4 + (3 - jan4_dow);
    Ok(PerlValue::integer(
        (thursday - thursday_of_week1) / 7 + 1,
    ))
}

// ── 4. Accessibility colour ─────────────────────────────────────────────────

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG relative luminance (0..1) given sRGB 0..255 components.
fn builtin_relative_luminance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = args.first().map(|v| v.to_number() / 255.0).unwrap_or(0.0);
    let g = args.get(1).map(|v| v.to_number() / 255.0).unwrap_or(0.0);
    let b = args.get(2).map(|v| v.to_number() / 255.0).unwrap_or(0.0);
    let l = 0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b);
    Ok(PerlValue::float(l))
}

/// WCAG contrast ratio between two sRGB colors (each as `[R, G, B]`).
fn builtin_contrast_ratio_wcag(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let to_rgb = |v: &PerlValue| {
        let xs = arg_to_vec(v);
        (
            xs.first().map(|x| x.to_number()).unwrap_or(0.0),
            xs.get(1).map(|x| x.to_number()).unwrap_or(0.0),
            xs.get(2).map(|x| x.to_number()).unwrap_or(0.0),
        )
    };
    let (r1, g1, b1) = to_rgb(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let (r2, g2, b2) = to_rgb(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let l1 = builtin_relative_luminance(&[
        PerlValue::float(r1),
        PerlValue::float(g1),
        PerlValue::float(b1),
    ])?
    .to_number();
    let l2 = builtin_relative_luminance(&[
        PerlValue::float(r2),
        PerlValue::float(g2),
        PerlValue::float(b2),
    ])?
    .to_number();
    let (a, b) = (l1.max(l2), l1.min(l2));
    Ok(PerlValue::float((a + 0.05) / (b + 0.05)))
}

/// CIE76 colour difference Δe = √Δl² + Δa² + Δb².
fn builtin_delta_e_76(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l1 = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let l2 = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let mut s = 0.0_f64;
    for i in 0..3 {
        let a = l1.get(i).map(|v| v.to_number()).unwrap_or(0.0);
        let b = l2.get(i).map(|v| v.to_number()).unwrap_or(0.0);
        s += (a - b).powi(2);
    }
    Ok(PerlValue::float(s.sqrt()))
}

/// Linear blend between two sRGB colors at parameter t ∈ [0, 1].
fn builtin_color_blend_t(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c1 = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let c2 = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF));
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    let blended: Vec<PerlValue> = (0..3)
        .map(|i| {
            let a = c1.get(i).map(|v| v.to_number()).unwrap_or(0.0);
            let b = c2.get(i).map(|v| v.to_number()).unwrap_or(0.0);
            PerlValue::float(a * (1.0 - t) + b * t)
        })
        .collect();
    Ok(PerlValue::array(blended))
}

// ── 5. Music theory ─────────────────────────────────────────────────────────

const NOTE_TO_SEMITONE: &[(&str, i32)] = &[
    ("C", 0), ("C#", 1), ("Db", 1), ("D", 2), ("D#", 3), ("Eb", 3),
    ("E", 4), ("F", 5), ("F#", 6), ("Gb", 6), ("G", 7), ("G#", 8),
    ("Ab", 8), ("A", 9), ("A#", 10), ("Bb", 10), ("B", 11),
];

fn note_name_to_semitone(name: &str) -> Option<i32> {
    let upper: String = name.chars().enumerate()
        .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
        .collect();
    NOTE_TO_SEMITONE.iter().find(|(n, _)| n == &upper).map(|(_, s)| *s)
}

/// Frequencies of a chord. Args: root note name (e.g. "C4"), chord type (`major`,
/// `minor`, `maj7`, `min7`, `dom7`, `dim`, `aug`).
fn builtin_chord_to_freqs(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let root = args.first().map(|v| v.to_string()).unwrap_or_else(|| "C4".to_string());
    let chord = args
        .get(1)
        .map(|v| v.to_string().to_ascii_lowercase())
        .unwrap_or_else(|| "major".to_string());
    let intervals: &[i32] = match chord.as_str() {
        "minor" | "min" => &[0, 3, 7],
        "maj7" | "major7" => &[0, 4, 7, 11],
        "min7" | "minor7" => &[0, 3, 7, 10],
        "dom7" | "7" => &[0, 4, 7, 10],
        "dim" | "diminished" => &[0, 3, 6],
        "aug" | "augmented" => &[0, 4, 8],
        "sus2" => &[0, 2, 7],
        "sus4" => &[0, 5, 7],
        _ => &[0, 4, 7],
    };
    // Parse note + octave (last digit).
    let last_digit = root.chars().last().filter(|c| c.is_ascii_digit());
    let octave: i32 = last_digit.and_then(|c| c.to_digit(10)).unwrap_or(4) as i32;
    let note_part = if last_digit.is_some() {
        &root[..root.len() - 1]
    } else {
        &root[..]
    };
    let semitone = note_name_to_semitone(note_part).unwrap_or(0);
    let midi_root = 12 * (octave + 1) + semitone;
    let freqs: Vec<PerlValue> = intervals
        .iter()
        .map(|i| PerlValue::float(440.0 * 2.0_f64.powf((midi_root + i - 69) as f64 / 12.0)))
        .collect();
    Ok(PerlValue::array(freqs))
}

/// Semitone intervals of a named scale. Returns flat array.
fn builtin_scale_to_intervals(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let name = args
        .first()
        .map(|v| v.to_string().to_ascii_lowercase())
        .unwrap_or_else(|| "major".to_string());
    let intervals: &[i32] = match name.as_str() {
        "major" | "ionian" => &[0, 2, 4, 5, 7, 9, 11, 12],
        "natural_minor" | "aeolian" => &[0, 2, 3, 5, 7, 8, 10, 12],
        "harmonic_minor" => &[0, 2, 3, 5, 7, 8, 11, 12],
        "melodic_minor" => &[0, 2, 3, 5, 7, 9, 11, 12],
        "dorian" => &[0, 2, 3, 5, 7, 9, 10, 12],
        "phrygian" => &[0, 1, 3, 5, 7, 8, 10, 12],
        "lydian" => &[0, 2, 4, 6, 7, 9, 11, 12],
        "mixolydian" => &[0, 2, 4, 5, 7, 9, 10, 12],
        "locrian" => &[0, 1, 3, 5, 6, 8, 10, 12],
        "pentatonic_major" => &[0, 2, 4, 7, 9, 12],
        "pentatonic_minor" => &[0, 3, 5, 7, 10, 12],
        "blues" => &[0, 3, 5, 6, 7, 10, 12],
        "chromatic" => &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        "whole_tone" => &[0, 2, 4, 6, 8, 10, 12],
        _ => &[0, 2, 4, 5, 7, 9, 11, 12],
    };
    Ok(PerlValue::array(
        intervals.iter().map(|i| PerlValue::integer(*i as i64)).collect(),
    ))
}

/// Number of semitones in a named interval.
fn builtin_interval_semitones(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let name = args
        .first()
        .map(|v| v.to_string().to_ascii_lowercase())
        .unwrap_or_default();
    let n = match name.as_str() {
        "unison" | "P1" | "p1" => 0,
        "minor_second" | "m2" => 1,
        "major_second" | "M2" | "m2p" => 2,
        "minor_third" | "m3" => 3,
        "major_third" | "M3" => 4,
        "perfect_fourth" | "P4" => 5,
        "tritone" | "TT" => 6,
        "perfect_fifth" | "P5" => 7,
        "minor_sixth" | "m6" => 8,
        "major_sixth" | "M6" => 9,
        "minor_seventh" | "m7" => 10,
        "major_seventh" | "M7" => 11,
        "octave" | "P8" => 12,
        _ => -1,
    };
    Ok(PerlValue::integer(n))
}

/// `transpose_freq_semitones` — Transpose freq semitones. Returns a float.
fn builtin_transpose_freq_semitones(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(f * 2.0_f64.powf(n / 12.0)))
}

/// `bpm_to_period` — Bpm to period. Returns a float.
fn builtin_bpm_to_period(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bpm = f1(args).max(1e-9);
    Ok(PerlValue::float(60.0 / bpm))
}

/// `midi_to_pitch_class` — Midi to pitch class. Returns an integer.
fn builtin_midi_to_pitch_class(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = i1(args);
    Ok(PerlValue::integer(m.rem_euclid(12)))
}

/// Sharps/flats count of a major-key signature (positive = sharps, negative = flats).
fn builtin_key_signature_for(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let key = args.first().map(|v| v.to_string()).unwrap_or_else(|| "C".to_string());
    let counts: &[(&str, i64)] = &[
        ("C", 0), ("G", 1), ("D", 2), ("A", 3), ("E", 4), ("B", 5),
        ("F#", 6), ("C#", 7), ("F", -1), ("Bb", -2), ("Eb", -3),
        ("Ab", -4), ("Db", -5), ("Gb", -6), ("Cb", -7),
    ];
    for (k, n) in counts {
        if &key == k {
            return Ok(PerlValue::integer(*n));
        }
    }
    Ok(PerlValue::integer(0))
}

/// Note name reached by N steps around the circle of fifths starting from `start`.
fn builtin_circle_of_fifths_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let start = args.first().map(|v| v.to_string()).unwrap_or_else(|| "C".to_string());
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let order = ["C", "G", "D", "A", "E", "B", "F#", "C#", "Ab", "Eb", "Bb", "F"];
    let idx = order.iter().position(|x| *x == start).unwrap_or(0);
    let pos = (idx as i64 + n).rem_euclid(12) as usize;
    Ok(PerlValue::string(order[pos].to_string()))
}

// ── 6. Astronomy ────────────────────────────────────────────────────────────

/// Approximate moon phase fraction (0 = new, 0.5 = full).
fn builtin_moon_phase(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let jd = f1(args);
    let synodic = 29.530_589_f64;
    let known_new = 2_451_550.1; // 2000 Jan 6 18:14 UT
    let phase = ((jd - known_new) / synodic).rem_euclid(1.0);
    Ok(PerlValue::float(phase))
}

/// Equation of time (minutes) — Spencer 1971 series.
fn builtin_equation_of_time(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let day_of_year = f1(args);
    let b = 2.0 * std::f64::consts::PI * (day_of_year - 1.0) / 365.0;
    let eot = 229.18
        * (0.000_075
            + 0.001_868 * b.cos()
            - 0.032_077 * b.sin()
            - 0.014_615 * (2.0 * b).cos()
            - 0.040_849 * (2.0 * b).sin());
    Ok(PerlValue::float(eot))
}

/// Solar declination (radians) on a given day of year.
fn builtin_solar_declination(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let theta = 2.0 * std::f64::consts::PI * (n - 1.0) / 365.0;
    let dec = 0.006_918 - 0.399_912 * theta.cos()
        + 0.070_257 * theta.sin()
        - 0.006_758 * (2.0 * theta).cos()
        + 0.000_907 * (2.0 * theta).sin()
        - 0.002_697 * (3.0 * theta).cos()
        + 0.001_480 * (3.0 * theta).sin();
    Ok(PerlValue::float(dec))
}

/// Sidereal day length in seconds.
fn builtin_sidereal_day_period(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(86164.0905))
}

/// Mean obliquity of the ecliptic (radians) at JD.
fn builtin_ecliptic_obliquity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let jd = f1(args);
    let t = (jd - 2_451_545.0) / 36_525.0;
    let eps_deg = 23.4392911
        - (46.8150 * t + 0.000_59 * t * t - 0.001_813 * t.powi(3)) / 3600.0;
    Ok(PerlValue::float(eps_deg.to_radians()))
}

// ── 7. Group / permutation primitives ───────────────────────────────────────

/// Order of a permutation = LCM of its cycle lengths.
fn builtin_permutation_order(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p: Vec<usize> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as usize)
        .collect();
    let n = p.len();
    let mut visited = vec![false; n];
    let mut order: i64 = 1;
    for i in 0..n {
        if !visited[i] {
            let mut len = 0_i64;
            let mut j = i;
            while !visited[j] {
                visited[j] = true;
                j = p[j];
                len += 1;
            }
            // LCM
            let g = gcd_i64(order, len);
            if g != 0 {
                order = order / g * len;
            }
        }
    }
    Ok(PerlValue::integer(order))
}

/// Sign of a permutation: +1 (even) or −1 (odd).
fn builtin_permutation_parity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p: Vec<usize> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as usize)
        .collect();
    let n = p.len();
    let mut visited = vec![false; n];
    let mut parity = 1_i64;
    for i in 0..n {
        if !visited[i] {
            let mut len = 0_i64;
            let mut j = i;
            while !visited[j] {
                visited[j] = true;
                j = p[j];
                len += 1;
            }
            if len % 2 == 0 {
                parity = -parity;
            }
        }
    }
    Ok(PerlValue::integer(parity))
}

/// `identity_permutation` — Identity permutation. Returns an integer.
fn builtin_identity_permutation(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0) as usize;
    Ok(PerlValue::array(
        (0..n).map(|i| PerlValue::integer(i as i64)).collect(),
    ))
}

/// Compose two permutations: (p ∘ q)(i) = p[q[i]].
fn builtin_permutation_compose(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p: Vec<usize> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as usize)
        .collect();
    let q: Vec<usize> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as usize)
        .collect();
    let n = p.len().min(q.len());
    let composed: Vec<PerlValue> = (0..n)
        .map(|i| PerlValue::integer(p.get(q[i]).copied().unwrap_or(0) as i64))
        .collect();
    Ok(PerlValue::array(composed))
}

// ── 8. Linguistics readability ──────────────────────────────────────────────

fn count_syllables_word(w: &str) -> usize {
    let lower = w.to_ascii_lowercase();
    let mut prev_vowel = false;
    let mut count = 0_usize;
    for c in lower.chars() {
        let is_vowel = "aeiouy".contains(c);
        if is_vowel && !prev_vowel {
            count += 1;
        }
        prev_vowel = is_vowel;
    }
    if lower.ends_with('e') && count > 1 {
        count -= 1;
    }
    count.max(1)
}

fn analyse_text(text: &str) -> (usize, usize, usize) {
    // (sentences, words, syllables)
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphabetic() && c != '\'')
        .filter(|s| !s.is_empty())
        .collect();
    let n_words = words.len();
    let n_syllables: usize = words.iter().map(|w| count_syllables_word(w)).sum();
    let n_sentences = text
        .chars()
        .filter(|c| matches!(c, '.' | '!' | '?'))
        .count()
        .max(1);
    (n_sentences, n_words, n_syllables)
}

/// Flesch reading ease score.
fn builtin_flesch_reading_ease(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let text = args.first().map(|v| v.to_string()).unwrap_or_default();
    let (s, w, syl) = analyse_text(&text);
    if w == 0 {
        return Ok(PerlValue::float(100.0));
    }
    Ok(PerlValue::float(
        206.835 - 1.015 * w as f64 / s as f64 - 84.6 * syl as f64 / w as f64,
    ))
}

/// Flesch-Kincaid grade level.
fn builtin_flesch_kincaid_grade(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let text = args.first().map(|v| v.to_string()).unwrap_or_default();
    let (s, w, syl) = analyse_text(&text);
    if w == 0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float(
        0.39 * w as f64 / s as f64 + 11.8 * syl as f64 / w as f64 - 15.59,
    ))
}

/// Gunning fog index.
fn builtin_gunning_fog(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let text = args.first().map(|v| v.to_string()).unwrap_or_default();
    let (s, w, _) = analyse_text(&text);
    if w == 0 {
        return Ok(PerlValue::float(0.0));
    }
    let complex_words = text
        .split(|c: char| !c.is_alphabetic() && c != '\'')
        .filter(|w| !w.is_empty() && count_syllables_word(w) >= 3)
        .count();
    Ok(PerlValue::float(
        0.4 * (w as f64 / s as f64 + 100.0 * complex_words as f64 / w as f64),
    ))
}

/// Automated readability index.
fn builtin_automated_readability_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let text = args.first().map(|v| v.to_string()).unwrap_or_default();
    let chars = text.chars().filter(|c| c.is_alphanumeric()).count();
    let (s, w, _) = analyse_text(&text);
    if w == 0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float(
        4.71 * chars as f64 / w as f64 + 0.5 * w as f64 / s as f64 - 21.43,
    ))
}

/// LIX readability score.
fn builtin_lix(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let text = args.first().map(|v| v.to_string()).unwrap_or_default();
    let (s, w, _) = analyse_text(&text);
    if w == 0 {
        return Ok(PerlValue::float(0.0));
    }
    let long = text
        .split(|c: char| !c.is_alphabetic() && c != '\'')
        .filter(|w| w.chars().count() > 6)
        .count();
    Ok(PerlValue::float(
        w as f64 / s as f64 + 100.0 * long as f64 / w as f64,
    ))
}

// ── 9. Regression diagnostics ───────────────────────────────────────────────

/// Adjusted R² = 1 − (1 − R²)(n − 1)/(n − k − 1).
fn builtin_adjusted_r_squared(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r2 = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = n - k - 1.0;
    if denom < 1.0 {
        return Ok(PerlValue::float(r2));
    }
    Ok(PerlValue::float(1.0 - (1.0 - r2) * (n - 1.0) / denom))
}

/// AIC = 2k − 2 ln L.
fn builtin_aic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = f1(args);
    let log_lik = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(2.0 * k - 2.0 * log_lik))
}

/// BIC = k ln n − 2 ln L.
fn builtin_bic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let log_lik = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(k * n.ln() - 2.0 * log_lik))
}

/// Compute residuals y − ŷ.
fn builtin_residuals_compute(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let yhat: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let res: Vec<PerlValue> = y
        .iter()
        .zip(yhat.iter())
        .map(|(a, b)| PerlValue::float(a - b))
        .collect();
    Ok(PerlValue::array(res))
}

// ── 10. More combinatorial counts ───────────────────────────────────────────

/// Compositions of n: 2^(n−1) for n ≥ 1, 1 for n = 0.
fn builtin_composition_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    if n == 0 {
        return Ok(PerlValue::integer(1));
    }
    Ok(PerlValue::integer(1_i64 << (n - 1)))
}

/// Weak-composition count: C(n + k − 1, k − 1).
fn builtin_weak_composition_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    if k == 0 {
        return Ok(PerlValue::integer(if n == 0 { 1 } else { 0 }));
    }
    Ok(PerlValue::integer(
        binomial_f(n + k - 1, k - 1).round() as i64,
    ))
}

/// Number of distinct necklaces of n beads in k colours (Burnside / cycle index).
fn builtin_necklace_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = args.first().map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let mut sum = 0_i64;
    let mut d = 1_i64;
    while d * d <= n {
        if n % d == 0 {
            // Euler's totient of n/d times k^d.
            let phi = euler_totient_simple((n / d) as i64);
            sum += phi as i64 * k.pow(d as u32);
            if d != n / d {
                let phi2 = euler_totient_simple(d);
                sum += phi2 as i64 * k.pow((n / d) as u32);
            }
        }
        d += 1;
    }
    Ok(PerlValue::integer(sum / n))
}

fn euler_totient_simple(n: i64) -> i64 {
    let mut result = n;
    let mut nn = n;
    let mut p = 2_i64;
    while p * p <= nn {
        if nn % p == 0 {
            while nn % p == 0 {
                nn /= p;
            }
            result -= result / p;
        }
        p += 1;
    }
    if nn > 1 {
        result -= result / nn;
    }
    result
}

/// Number of distinct bracelets (necklaces under reflection too).
fn builtin_bracelet_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let necklaces = builtin_necklace_count(args)?.to_number() as i64;
    let n = args.first().map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    let reflections = if n & 1 == 0 {
        (k.pow((n / 2) as u32) + k.pow((n / 2 + 1) as u32)) / 2
    } else {
        k.pow((n / 2 + 1) as u32)
    };
    Ok(PerlValue::integer((necklaces + reflections) / 2))
}

/// Number of distinct permutations of a multiset of counts (multinomial coef).
fn builtin_multiset_permutations_count(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let counts: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    use statrs::function::gamma::ln_gamma;
    let n: i64 = counts.iter().sum();
    let mut log = ln_gamma(n as f64 + 1.0);
    for &c in &counts {
        log -= ln_gamma(c as f64 + 1.0);
    }
    Ok(PerlValue::integer(log.exp().round() as i64))
}

// ── 11. PRNG / hashing ──────────────────────────────────────────────────────

/// Pearson hash byte: rolls a permutation table to compress a string to one byte.
fn builtin_pearson_hash_byte(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    // Standard 256-byte permutation (Pearson 1990).
    let table: [u8; 256] = [
        98, 6, 85, 150, 36, 23, 112, 164, 135, 207, 169, 5, 26, 64, 165, 219, 61, 20, 68, 89, 130,
        63, 52, 102, 24, 229, 132, 245, 80, 216, 195, 115, 90, 168, 156, 203, 177, 120, 2, 190,
        188, 7, 100, 185, 174, 243, 162, 10, 237, 18, 253, 225, 8, 208, 172, 244, 255, 126, 101,
        79, 145, 235, 228, 121, 123, 251, 67, 250, 161, 0, 107, 97, 241, 111, 181, 82, 249, 33,
        69, 55, 59, 153, 29, 9, 213, 167, 84, 93, 30, 46, 94, 75, 151, 114, 73, 222, 197, 96, 210,
        45, 16, 227, 248, 202, 51, 152, 252, 125, 81, 206, 215, 186, 39, 158, 178, 187, 131, 136,
        1, 49, 50, 17, 141, 91, 47, 129, 60, 99, 154, 35, 86, 171, 105, 34, 38, 200, 147, 58, 77,
        118, 173, 246, 76, 254, 133, 232, 196, 144, 198, 124, 53, 4, 108, 74, 223, 234, 134, 230,
        157, 139, 189, 205, 199, 128, 176, 19, 211, 236, 127, 192, 231, 70, 233, 88, 146, 44, 183,
        201, 22, 83, 13, 214, 116, 109, 159, 32, 95, 226, 140, 220, 57, 12, 221, 31, 209, 182, 143,
        92, 149, 184, 148, 62, 113, 65, 37, 27, 106, 166, 3, 14, 204, 72, 21, 41, 56, 66, 28, 193,
        40, 217, 25, 54, 179, 117, 238, 87, 240, 155, 180, 170, 242, 212, 191, 163, 78, 218, 137,
        194, 175, 110, 43, 119, 224, 71, 122, 142, 42, 160, 104, 48, 247, 103, 15, 11, 138, 239,
    ];
    let mut h: u8 = 0;
    for b in s.bytes() {
        h = table[(h ^ b) as usize];
    }
    Ok(PerlValue::integer(h as i64))
}

/// 32-bit xorshift PRNG step. Args: state. Returns next state.
fn builtin_xorshift32_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut x = (i1(args) as u32).max(1);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    Ok(PerlValue::integer(x as i64))
}

/// Numerical-Recipes LCG step: state = state·1664525 + 1013904223 mod 2³².
fn builtin_lcg_next_u32(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = i1(args) as u64;
    let next = (s.wrapping_mul(1_664_525) + 1_013_904_223) & 0xFFFF_FFFF;
    Ok(PerlValue::integer(next as i64))
}

/// Fisher-Yates shuffle of an array (returns shuffled copy).
fn builtin_fisher_yates_shuffle(args: &[PerlValue]) -> PerlResult<PerlValue> {
    use rand::Rng;
    let mut arr = arg_to_vec(&args.first().cloned().unwrap_or(PerlValue::UNDEF));
    let mut rng = rand::thread_rng();
    let n = arr.len();
    for i in (1..n).rev() {
        let j = rng.gen_range(0..=i);
        arr.swap(i, j);
    }
    Ok(PerlValue::array(arr))
}
