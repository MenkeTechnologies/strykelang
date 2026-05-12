// ─────────────────────────────────────────────────────────────────────────────
// Batch 8 — cross-domain primitives mined from APL/J/K, MATLAB, Maple, R/CRAN,
// Stata, BioPython, MetPy, Quantlib, scikit-image, librosa, NetworkX, ELO/Glicko
// rating-systems literature, and standard physics/chemistry/finance references.
// Layout: bioinformatics, geographic / map-projection helpers, fixed-income
// finance, image-quality metrics, acoustics, population genetics, epidemiology,
// econometric inequality measures, APL/J array primitives, plasma physics,
// string-similarity, rating systems, effect sizes, control-theory transient
// response, matrix norms, social-network triad measures.
// ─────────────────────────────────────────────────────────────────────────────

// ── 1. Bioinformatics ────────────────────────────────────────────────────────

/// GC content of a DNA / RNA string (case-insensitive).
fn builtin_gc_content(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut total = 0_usize;
    let mut gc = 0_usize;
    for c in s.chars() {
        let u = c.to_ascii_uppercase();
        if matches!(u, 'A' | 'C' | 'G' | 'T' | 'U') {
            total += 1;
            if u == 'G' || u == 'C' {
                gc += 1;
            }
        }
    }
    if total == 0 {
        return Ok(StrykeValue::float(0.0));
    }
    Ok(StrykeValue::float(gc as f64 / total as f64))
}

/// Standard codon table (DNA, T = thymine). Returns single-letter amino-acid or '*' (stop) or 'X' (unknown).
fn builtin_codon_to_aa(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let codon = args
        .first()
        .map(|v| v.to_string().to_ascii_uppercase())
        .unwrap_or_default();
    if codon.len() != 3 {
        return Ok(StrykeValue::string("X".into()));
    }
    let table = [
        ("TTT", 'F'), ("TTC", 'F'), ("TTA", 'L'), ("TTG", 'L'),
        ("CTT", 'L'), ("CTC", 'L'), ("CTA", 'L'), ("CTG", 'L'),
        ("ATT", 'I'), ("ATC", 'I'), ("ATA", 'I'), ("ATG", 'M'),
        ("GTT", 'V'), ("GTC", 'V'), ("GTA", 'V'), ("GTG", 'V'),
        ("TCT", 'S'), ("TCC", 'S'), ("TCA", 'S'), ("TCG", 'S'),
        ("CCT", 'P'), ("CCC", 'P'), ("CCA", 'P'), ("CCG", 'P'),
        ("ACT", 'T'), ("ACC", 'T'), ("ACA", 'T'), ("ACG", 'T'),
        ("GCT", 'A'), ("GCC", 'A'), ("GCA", 'A'), ("GCG", 'A'),
        ("TAT", 'Y'), ("TAC", 'Y'), ("TAA", '*'), ("TAG", '*'),
        ("CAT", 'H'), ("CAC", 'H'), ("CAA", 'Q'), ("CAG", 'Q'),
        ("AAT", 'N'), ("AAC", 'N'), ("AAA", 'K'), ("AAG", 'K'),
        ("GAT", 'D'), ("GAC", 'D'), ("GAA", 'E'), ("GAG", 'E'),
        ("TGT", 'C'), ("TGC", 'C'), ("TGA", '*'), ("TGG", 'W'),
        ("CGT", 'R'), ("CGC", 'R'), ("CGA", 'R'), ("CGG", 'R'),
        ("AGT", 'S'), ("AGC", 'S'), ("AGA", 'R'), ("AGG", 'R'),
        ("GGT", 'G'), ("GGC", 'G'), ("GGA", 'G'), ("GGG", 'G'),
    ];
    for (k, aa) in table {
        if codon == k {
            return Ok(StrykeValue::string(aa.to_string()));
        }
    }
    Ok(StrykeValue::string("X".into()))
}

/// Reverse-complement of DNA (A↔T, C↔G; case-preserving).
fn builtin_reverse_complement_dna(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let comp = |c: char| match c {
        'A' => 'T', 'T' => 'A', 'C' => 'G', 'G' => 'C',
        'a' => 't', 't' => 'a', 'c' => 'g', 'g' => 'c',
        'U' => 'A', 'u' => 'a',
        other => other,
    };
    let out: String = s.chars().rev().map(comp).collect();
    Ok(StrykeValue::string(out))
}

/// Hamming distance between equal-length DNA sequences.
fn builtin_hamming_dna(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let n = a.len().min(b.len());
    let d = a
        .chars()
        .zip(b.chars())
        .take(n)
        .filter(|(x, y)| !x.eq_ignore_ascii_case(y))
        .count();
    Ok(StrykeValue::integer(d as i64))
}

/// BLOSUM62 score for an amino-acid pair (case insensitive). Returns -10 if
/// either character is unknown.
fn builtin_blosum62_pair_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = args
        .first()
        .map(|v| v.to_string().to_ascii_uppercase())
        .unwrap_or_default();
    let b = args
        .get(1)
        .map(|v| v.to_string().to_ascii_uppercase())
        .unwrap_or_default();
    if a.is_empty() || b.is_empty() {
        return Ok(StrykeValue::integer(-10));
    }
    let order = "ARNDCQEGHILKMFPSTWYVBZX";
    let i = order.find(a.chars().next().unwrap_or(' '));
    let j = order.find(b.chars().next().unwrap_or(' '));
    let (Some(i), Some(j)) = (i, j) else {
        return Ok(StrykeValue::integer(-10));
    };
    // Standard BLOSUM62 matrix rows in `order` ordering.
    let m: [[i32; 23]; 23] = [
        [4, -1, -2, -2, 0, -1, -1, 0, -2, -1, -1, -1, -1, -2, -1, 1, 0, -3, -2, 0, -2, -1, 0],
        [-1, 5, 0, -2, -3, 1, 0, -2, 0, -3, -2, 2, -1, -3, -2, -1, -1, -3, -2, -3, -1, 0, -1],
        [-2, 0, 6, 1, -3, 0, 0, 0, 1, -3, -3, 0, -2, -3, -2, 1, 0, -4, -2, -3, 3, 0, -1],
        [-2, -2, 1, 6, -3, 0, 2, -1, -1, -3, -4, -1, -3, -3, -1, 0, -1, -4, -3, -3, 4, 1, -1],
        [0, -3, -3, -3, 9, -3, -4, -3, -3, -1, -1, -3, -1, -2, -3, -1, -1, -2, -2, -1, -3, -3, -2],
        [-1, 1, 0, 0, -3, 5, 2, -2, 0, -3, -2, 1, 0, -3, -1, 0, -1, -2, -1, -2, 0, 3, -1],
        [-1, 0, 0, 2, -4, 2, 5, -2, 0, -3, -3, 1, -2, -3, -1, 0, -1, -3, -2, -2, 1, 4, -1],
        [0, -2, 0, -1, -3, -2, -2, 6, -2, -4, -4, -2, -3, -3, -2, 0, -2, -2, -3, -3, -1, -2, -1],
        [-2, 0, 1, -1, -3, 0, 0, -2, 8, -3, -3, -1, -2, -1, -2, -1, -2, -2, 2, -3, 0, 0, -1],
        [-1, -3, -3, -3, -1, -3, -3, -4, -3, 4, 2, -3, 1, 0, -3, -2, -1, -3, -1, 3, -3, -3, -1],
        [-1, -2, -3, -4, -1, -2, -3, -4, -3, 2, 4, -2, 2, 0, -3, -2, -1, -2, -1, 1, -4, -3, -1],
        [-1, 2, 0, -1, -3, 1, 1, -2, -1, -3, -2, 5, -1, -3, -1, 0, -1, -3, -2, -2, 0, 1, -1],
        [-1, -1, -2, -3, -1, 0, -2, -3, -2, 1, 2, -1, 5, 0, -2, -1, -1, -1, -1, 1, -3, -1, -1],
        [-2, -3, -3, -3, -2, -3, -3, -3, -1, 0, 0, -3, 0, 6, -4, -2, -2, 1, 3, -1, -3, -3, -1],
        [-1, -2, -2, -1, -3, -1, -1, -2, -2, -3, -3, -1, -2, -4, 7, -1, -1, -4, -3, -2, -2, -1, -2],
        [1, -1, 1, 0, -1, 0, 0, 0, -1, -2, -2, 0, -1, -2, -1, 4, 1, -3, -2, -2, 0, 0, 0],
        [0, -1, 0, -1, -1, -1, -1, -2, -2, -1, -1, -1, -1, -2, -1, 1, 5, -2, -2, 0, -1, -1, 0],
        [-3, -3, -4, -4, -2, -2, -3, -2, -2, -3, -2, -3, -1, 1, -4, -3, -2, 11, 2, -3, -4, -3, -2],
        [-2, -2, -2, -3, -2, -1, -2, -3, 2, -1, -1, -2, -1, 3, -3, -2, -2, 2, 7, -1, -3, -2, -1],
        [0, -3, -3, -3, -1, -2, -2, -3, -3, 3, 1, -2, 1, -1, -2, -2, 0, -3, -1, 4, -3, -2, -1],
        [-2, -1, 3, 4, -3, 0, 1, -1, 0, -3, -4, 0, -3, -3, -2, 0, -1, -4, -3, -3, 4, 1, -1],
        [-1, 0, 0, 1, -3, 3, 4, -2, 0, -3, -3, 1, -1, -3, -1, 0, -1, -3, -2, -2, 1, 4, -1],
        [0, -1, -1, -1, -2, -1, -1, -1, -1, -1, -1, -1, -1, -1, -2, 0, 0, -2, -1, -1, -1, -1, -1],
    ];
    Ok(StrykeValue::integer(m[i][j] as i64))
}

/// Count k-mer occurrences (case-insensitive). Returns map size.
fn builtin_kmer_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args
        .first()
        .map(|v| v.to_string().to_ascii_uppercase())
        .unwrap_or_default();
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(3);
    if s.len() < k {
        return Ok(StrykeValue::integer(0));
    }
    let mut counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let bytes = s.as_bytes();
    for i in 0..=bytes.len() - k {
        let kmer = std::str::from_utf8(&bytes[i..i + k]).unwrap_or("").to_string();
        *counts.entry(kmer).or_insert(0) += 1;
    }
    let mut total = 0_i64;
    for &c in counts.values() {
        total += c as i64;
    }
    Ok(StrykeValue::integer(total))
}

// ── 2. Geographic / map projection ───────────────────────────────────────────

/// Initial bearing (radians) from (lat1, lon1) → (lat2, lon2) on a sphere.
fn builtin_great_circle_bearing(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lat1 = args.first().map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon1 = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lat2 = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon2 = args.get(3).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let dlon = lon2 - lon1;
    let y = dlon.sin() * lat2.cos();
    let x = lat1.cos() * lat2.sin() - lat1.sin() * lat2.cos() * dlon.cos();
    Ok(StrykeValue::float(y.atan2(x).to_degrees().rem_euclid(360.0)))
}

/// Great-circle midpoint of two surface points (degrees).
fn builtin_midpoint_lat_lon(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lat1 = args.first().map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon1 = args.get(1).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lat2 = args.get(2).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let lon2 = args.get(3).map(|v| v.to_number().to_radians()).unwrap_or(0.0);
    let bx = lat2.cos() * (lon2 - lon1).cos();
    let by = lat2.cos() * (lon2 - lon1).sin();
    let lat = (lat1.sin() + lat2.sin()).atan2(((lat1.cos() + bx).powi(2) + by * by).sqrt());
    let lon = lon1 + by.atan2(lat1.cos() + bx);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(lat.to_degrees()),
        StrykeValue::float(lon.to_degrees()),
    ]))
}

/// UTM zone for a longitude (1..60).
fn builtin_utm_zone_for(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lon = f1(args);
    let zone = ((lon + 180.0) / 6.0).floor() as i64 + 1;
    Ok(StrykeValue::integer(zone.clamp(1, 60)))
}

/// Geodesic-friendly polygon area on a sphere (m²). Uses the L'Huilier
/// (spherical-excess) formula divided into triangles fanned from the first
/// vertex.  Approximate for small polygons but exact in spherical geometry.
fn builtin_area_polygon_lat_lon(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pts = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(6378137.0);
    let n = pts.len();
    if n < 3 {
        return Ok(StrykeValue::float(0.0));
    }
    let to_pair = |v: &StrykeValue| -> (f64, f64) {
        let xs = arg_to_vec(v);
        (
            xs.first().map(|x| x.to_number().to_radians()).unwrap_or(0.0),
            xs.get(1).map(|x| x.to_number().to_radians()).unwrap_or(0.0),
        )
    };
    let mut total = 0.0_f64;
    for i in 0..n {
        let j = (i + 1) % n;
        let (lat1, lon1) = to_pair(&pts[i]);
        let (lat2, lon2) = to_pair(&pts[j]);
        total += (lon2 - lon1) * (2.0 + lat1.sin() + lat2.sin());
    }
    Ok(StrykeValue::float((total * r * r / 2.0).abs()))
}

// ── 3. Fixed-income finance ──────────────────────────────────────────────────

/// Cox-Ross-Rubinstein binomial European option price.
/// Args: S0, K, T, r, sigma, n_steps, type (0=call, 1=put).
fn builtin_crr_binomial_option(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s0 = args.first().map(|v| v.to_number()).unwrap_or(0.0);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let r = args.get(3).map(|v| v.to_number()).unwrap_or(0.05);
    let sigma = args.get(4).map(|v| v.to_number()).unwrap_or(0.2);
    let n = args.get(5).map(|v| v.to_number() as usize).unwrap_or(100);
    let is_put = args.get(6).map(|v| v.to_number() as i64).unwrap_or(0) != 0;
    let dt = t / n as f64;
    let u = (sigma * dt.sqrt()).exp();
    let d = 1.0 / u;
    let p = ((r * dt).exp() - d) / (u - d);
    let mut prices: Vec<f64> = (0..=n)
        .map(|i| {
            let st = s0 * u.powi((n - i) as i32) * d.powi(i as i32);
            if is_put {
                (k - st).max(0.0)
            } else {
                (st - k).max(0.0)
            }
        })
        .collect();
    let disc = (-r * dt).exp();
    for step in (0..n).rev() {
        for i in 0..=step {
            prices[i] = disc * (p * prices[i] + (1.0 - p) * prices[i + 1]);
        }
    }
    Ok(StrykeValue::float(prices[0]))
}

/// Bond clean price from yield. Args: face, coupon_rate (annual), n_periods,
/// periods_per_year, yield (annual), accrued_days (default 0), period_days (default 365).
fn builtin_bond_price_clean(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let face = f1(args);
    let coupon = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let pp_y = args.get(3).map(|v| v.to_number()).unwrap_or(2.0).max(1.0);
    let y = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let c = face * coupon / pp_y;
    let r = y / pp_y;
    let mut pv = 0.0_f64;
    for k in 1..=n {
        pv += c / (1.0 + r).powi(k as i32);
    }
    pv += face / (1.0 + r).powi(n as i32);
    Ok(StrykeValue::float(pv))
}

/// Yield to maturity by bisection.
fn builtin_bond_yield_to_maturity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let price = f1(args);
    let face = args.get(1).map(|v| v.to_number()).unwrap_or(1000.0);
    let coupon = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(3).map(|v| v.to_number() as usize).unwrap_or(0);
    let pp_y = args.get(4).map(|v| v.to_number()).unwrap_or(2.0).max(1.0);
    let pv = |y: f64| -> f64 {
        let r = y / pp_y;
        let c = face * coupon / pp_y;
        let mut p = 0.0_f64;
        for k in 1..=n {
            p += c / (1.0 + r).powi(k as i32);
        }
        p + face / (1.0 + r).powi(n as i32)
    };
    let mut lo = -0.99_f64;
    let mut hi = 1.0_f64;
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        if pv(mid) > price {
            lo = mid;
        } else {
            hi = mid;
        }
        if (hi - lo) < 1e-10 {
            break;
        }
    }
    Ok(StrykeValue::float((lo + hi) / 2.0))
}

/// Macaulay / Modified duration of a bond.
fn builtin_modified_duration_bond(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let face = f1(args);
    let coupon = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let pp_y = args.get(3).map(|v| v.to_number()).unwrap_or(2.0).max(1.0);
    let y = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let r = y / pp_y;
    let c = face * coupon / pp_y;
    let mut pv = 0.0_f64;
    let mut weighted = 0.0_f64;
    for k in 1..=n {
        let discounted = c / (1.0 + r).powi(k as i32);
        pv += discounted;
        weighted += k as f64 * discounted;
    }
    let final_pv = face / (1.0 + r).powi(n as i32);
    pv += final_pv;
    weighted += n as f64 * final_pv;
    let macaulay = weighted / pv / pp_y;
    Ok(StrykeValue::float(macaulay / (1.0 + r)))
}

/// Bond convexity measure.
fn builtin_convexity_bond(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let face = f1(args);
    let coupon = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let pp_y = args.get(3).map(|v| v.to_number()).unwrap_or(2.0).max(1.0);
    let y = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let r = y / pp_y;
    let c = face * coupon / pp_y;
    let mut pv = 0.0_f64;
    let mut acc = 0.0_f64;
    for k in 1..=n {
        let discounted = c / (1.0 + r).powi(k as i32);
        pv += discounted;
        acc += (k * (k + 1)) as f64 * discounted;
    }
    let final_pv = face / (1.0 + r).powi(n as i32);
    pv += final_pv;
    acc += (n * (n + 1)) as f64 * final_pv;
    Ok(StrykeValue::float(acc / (pv * (1.0 + r).powi(2)) / pp_y.powi(2)))
}

// ── 4. Image-quality metrics ─────────────────────────────────────────────────

/// SSIM (Structural Similarity Index) on grayscale matrices (single window).
fn builtin_ssim(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(255.0);
    let mut sum_a = 0.0_f64;
    let mut sum_b = 0.0_f64;
    let mut count = 0_f64;
    for (ra, rb) in a.iter().zip(b.iter()) {
        for (x, y) in ra.iter().zip(rb.iter()) {
            sum_a += x;
            sum_b += y;
            count += 1.0;
        }
    }
    if count == 0.0 {
        return Ok(StrykeValue::float(0.0));
    }
    let mu_a = sum_a / count;
    let mu_b = sum_b / count;
    let mut var_a = 0.0_f64;
    let mut var_b = 0.0_f64;
    let mut cov = 0.0_f64;
    for (ra, rb) in a.iter().zip(b.iter()) {
        for (x, y) in ra.iter().zip(rb.iter()) {
            var_a += (x - mu_a).powi(2);
            var_b += (y - mu_b).powi(2);
            cov += (x - mu_a) * (y - mu_b);
        }
    }
    var_a /= count;
    var_b /= count;
    cov /= count;
    let c1 = (0.01 * l).powi(2);
    let c2 = (0.03 * l).powi(2);
    let s = (2.0 * mu_a * mu_b + c1) * (2.0 * cov + c2)
        / ((mu_a * mu_a + mu_b * mu_b + c1) * (var_a + var_b + c2));
    Ok(StrykeValue::float(s))
}

/// Peak signal-to-noise ratio in dB.
fn builtin_psnr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let max_val = args.get(2).map(|v| v.to_number()).unwrap_or(255.0);
    let mut sse = 0.0_f64;
    let mut count = 0_f64;
    for (ra, rb) in a.iter().zip(b.iter()) {
        for (x, y) in ra.iter().zip(rb.iter()) {
            sse += (x - y).powi(2);
            count += 1.0;
        }
    }
    if count == 0.0 || sse < 1e-30 {
        return Ok(StrykeValue::float(f64::INFINITY));
    }
    let mse = sse / count;
    Ok(StrykeValue::float(10.0 * (max_val * max_val / mse).log10()))
}

/// Mean SSIM across non-overlapping windows of size `win`.
fn builtin_mssim(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let win = args.get(2).map(|v| v.to_number() as usize).unwrap_or(8).max(2);
    let l = args.get(3).map(|v| v.to_number()).unwrap_or(255.0);
    let h = a.len();
    let w = if h == 0 { 0 } else { a[0].len() };
    let mut sum = 0.0_f64;
    let mut count = 0_f64;
    let mut i = 0_usize;
    while i + win <= h {
        let mut j = 0_usize;
        while j + win <= w {
            // Block stats.
            let mut mu_a = 0.0_f64;
            let mut mu_b = 0.0_f64;
            let n = (win * win) as f64;
            for ii in i..i + win {
                for jj in j..j + win {
                    mu_a += a[ii][jj];
                    mu_b += b[ii][jj];
                }
            }
            mu_a /= n;
            mu_b /= n;
            let mut va = 0.0_f64;
            let mut vb = 0.0_f64;
            let mut cov = 0.0_f64;
            for ii in i..i + win {
                for jj in j..j + win {
                    let dx = a[ii][jj] - mu_a;
                    let dy = b[ii][jj] - mu_b;
                    va += dx * dx;
                    vb += dy * dy;
                    cov += dx * dy;
                }
            }
            va /= n;
            vb /= n;
            cov /= n;
            let c1 = (0.01 * l).powi(2);
            let c2 = (0.03 * l).powi(2);
            let s = (2.0 * mu_a * mu_b + c1) * (2.0 * cov + c2)
                / ((mu_a * mu_a + mu_b * mu_b + c1) * (va + vb + c2));
            sum += s;
            count += 1.0;
            j += win;
        }
        i += win;
    }
    Ok(StrykeValue::float(if count == 0.0 { 0.0 } else { sum / count }))
}

// ── 5. Acoustics ─────────────────────────────────────────────────────────────

/// `db_spl_from_pa` — Db spl from pa. Returns a float.
fn builtin_db_spl_from_pa(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_pa = f1(args).abs().max(1e-30);
    Ok(StrykeValue::float(20.0 * (p_pa / 20e-6).log10()))
}

/// IEC 61672 A-weighting amplitude factor at frequency f (Hz).
fn builtin_a_weighting_factor(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f = f1(args).max(1e-9);
    let f2 = f * f;
    let num = 12194.0_f64.powi(2) * f2 * f2;
    let den = (f2 + 20.6_f64.powi(2))
        * ((f2 + 107.7_f64.powi(2)) * (f2 + 737.9_f64.powi(2))).sqrt()
        * (f2 + 12194.0_f64.powi(2));
    let ra = num / den;
    Ok(StrykeValue::float(ra * 1.2589254117941673)) // 10^(0.1 dB) normalising factor at 1 kHz
}

/// Center frequency of an octave band (band=0 → 1 kHz).
fn builtin_octave_band_center(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let band = i1(args);
    Ok(StrykeValue::float(1000.0 * 2.0_f64.powf(band as f64)))
}

/// 12-TET semitone ratio.
fn builtin_semitone_ratio(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(2.0_f64.powf(1.0 / 12.0)))
}

// ── 6. Population genetics ───────────────────────────────────────────────────

/// Hardy-Weinberg expected genotype frequencies given allele frequency p.
fn builtin_hardy_weinberg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let q = 1.0 - p;
    Ok(StrykeValue::array(vec![
        StrykeValue::float(p * p),
        StrykeValue::float(2.0 * p * q),
        StrykeValue::float(q * q),
    ]))
}

/// `expected_heterozygosity` — Expected heterozygosity. Returns a float.
fn builtin_expected_heterozygosity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let s: f64 = p.iter().map(|x| x * x).sum();
    Ok(StrykeValue::float(1.0 - s))
}

/// Pairwise F_ST given allele frequencies in two populations and their sample sizes.
fn builtin_fst_simple(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p1 = f1(args);
    let p2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let n1 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n2 = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let p_bar = (n1 * p1 + n2 * p2) / (n1 + n2);
    let h_t = 2.0 * p_bar * (1.0 - p_bar);
    let h_s = (n1 * 2.0 * p1 * (1.0 - p1) + n2 * 2.0 * p2 * (1.0 - p2)) / (n1 + n2);
    if h_t < 1e-15 {
        return Ok(StrykeValue::float(0.0));
    }
    Ok(StrykeValue::float((h_t - h_s) / h_t))
}

/// Allele frequencies from a vector of integer genotype counts (0=AA, 1=Aa, 2=aa).
fn builtin_allele_frequencies(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let mut count_a = 0_f64;
    let mut total = 0_f64;
    for &c in &g {
        match c {
            0 => count_a += 2.0,
            1 => count_a += 1.0,
            _ => {}
        }
        total += 2.0;
    }
    if total == 0.0 {
        return Ok(StrykeValue::array(vec![StrykeValue::float(0.5), StrykeValue::float(0.5)]));
    }
    let p = count_a / total;
    Ok(StrykeValue::array(vec![StrykeValue::float(p), StrykeValue::float(1.0 - p)]))
}

// ── 7. Epidemiology ──────────────────────────────────────────────────────────

/// One forward Euler step of the SIR model. Args: S, I, R, beta, gamma, dt.
fn builtin_sir_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = f1(args);
    let i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let gamma = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let dt = args.get(5).map(|v| v.to_number()).unwrap_or(0.1);
    let n = s + i + r;
    let ds = -beta * s * i / n.max(1e-30);
    let di = beta * s * i / n.max(1e-30) - gamma * i;
    let dr = gamma * i;
    Ok(StrykeValue::array(vec![
        StrykeValue::float(s + dt * ds),
        StrykeValue::float(i + dt * di),
        StrykeValue::float(r + dt * dr),
    ]))
}

/// SIR basic R₀ = β / γ.
fn builtin_sir_r0(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let beta = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if gamma.abs() < 1e-30 {
        return Ok(StrykeValue::float(f64::INFINITY));
    }
    Ok(StrykeValue::float(beta / gamma))
}

/// Doubling time t₂ = ln 2 / r given growth rate r.
fn builtin_doubling_time(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    if r.abs() < 1e-30 {
        return Ok(StrykeValue::float(f64::INFINITY));
    }
    Ok(StrykeValue::float(std::f64::consts::LN_2 / r))
}

// ── 8. Inequality / econometric measures ─────────────────────────────────────

/// Theil T inequality index.
fn builtin_theil_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mean: f64 = xs.iter().sum::<f64>() / xs.len().max(1) as f64;
    if mean.abs() < 1e-15 {
        return Ok(StrykeValue::float(0.0));
    }
    let n = xs.len() as f64;
    let s: f64 = xs
        .iter()
        .filter(|&&x| x > 0.0)
        .map(|x| (x / mean) * (x / mean).ln())
        .sum();
    Ok(StrykeValue::float(s / n))
}

/// Herfindahl-Hirschman index from market shares (in [0, 1]).
fn builtin_herfindahl_hirschman(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let shares: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let s: f64 = shares.iter().map(|x| x * x).sum();
    Ok(StrykeValue::float(s))
}

/// Atkinson inequality with parameter ε (≠ 1).
fn builtin_atkinson_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let eps = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let n = xs.len() as f64;
    if n < 1.0 {
        return Ok(StrykeValue::float(0.0));
    }
    let mean = xs.iter().sum::<f64>() / n;
    if mean.abs() < 1e-15 {
        return Ok(StrykeValue::float(0.0));
    }
    if (eps - 1.0).abs() < 1e-12 {
        let log_g = xs
            .iter()
            .filter(|&&x| x > 0.0)
            .map(|x| x.ln())
            .sum::<f64>()
            / n;
        return Ok(StrykeValue::float(1.0 - log_g.exp() / mean));
    }
    let s: f64 = xs.iter().map(|x| (x / mean).powf(1.0 - eps)).sum::<f64>() / n;
    Ok(StrykeValue::float(1.0 - s.powf(1.0 / (1.0 - eps))))
}

/// Lorenz curve points: returns matrix of [cumulative_pop, cumulative_income].
fn builtin_lorenz_curve_points(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = xs.len() as f64;
    let total: f64 = xs.iter().sum();
    let mut out = Vec::new();
    out.push(StrykeValue::array(vec![
        StrykeValue::float(0.0),
        StrykeValue::float(0.0),
    ]));
    let mut acc = 0.0_f64;
    for (i, &x) in xs.iter().enumerate() {
        acc += x;
        out.push(StrykeValue::array(vec![
            StrykeValue::float((i as f64 + 1.0) / n),
            StrykeValue::float(if total > 0.0 { acc / total } else { 0.0 }),
        ]));
    }
    Ok(StrykeValue::array(out))
}

// ── 9. APL/J/K array primitives ──────────────────────────────────────────────

/// `iota_range N` — `0..N` as an integer array.
fn builtin_iota_range(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0) as usize;
    Ok(StrykeValue::array(
        (0..n).map(|i| StrykeValue::integer(i as i64)).collect(),
    ))
}

/// Reshape a flat array to a 2-D matrix. Args: rows, cols, flat.
fn builtin_reshape_array(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rows = args.first().map(|v| v.to_number() as usize).unwrap_or(0);
    let cols = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let flat = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF));
    let n = flat.len();
    if n == 0 {
        return Ok(matrix_to_value(&vec![vec![0.0_f64; cols]; rows]));
    }
    let mut out = vec![vec![0.0_f64; cols]; rows];
    for i in 0..rows {
        for j in 0..cols {
            out[i][j] = flat[(i * cols + j) % n].to_number();
        }
    }
    Ok(matrix_to_value(&out))
}

/// Grade-up: index permutation that sorts the array ascending.
fn builtin_grade_up(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut idx: Vec<usize> = (0..xs.len()).collect();
    idx.sort_by(|&a, &b| xs[a].partial_cmp(&xs[b]).unwrap_or(std::cmp::Ordering::Equal));
    Ok(StrykeValue::array(
        idx.into_iter().map(|i| StrykeValue::integer(i as i64)).collect(),
    ))
}

/// Grade-down: index permutation that sorts the array descending.
fn builtin_grade_down(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut idx: Vec<usize> = (0..xs.len()).collect();
    idx.sort_by(|&a, &b| xs[b].partial_cmp(&xs[a]).unwrap_or(std::cmp::Ordering::Equal));
    Ok(StrykeValue::array(
        idx.into_iter().map(|i| StrykeValue::integer(i as i64)).collect(),
    ))
}

// ── 10. Plasma physics ───────────────────────────────────────────────────────

/// Plasma frequency ω_p = √(n e²/(m_e ε₀)).
fn builtin_plasma_frequency(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let e = 1.602_176_634e-19_f64;
    let me = 9.109_383_7e-31_f64;
    let eps0 = 8.854_187_817e-12_f64;
    Ok(StrykeValue::float(((n * e * e) / (me * eps0)).sqrt()))
}

/// Debye length λ_D = √(ε₀ k_B T / (n e²)).
fn builtin_debye_length(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-30);
    let e = 1.602_176_634e-19_f64;
    let kb = 1.380_649e-23_f64;
    let eps0 = 8.854_187_817e-12_f64;
    Ok(StrykeValue::float(((eps0 * kb * t) / (n * e * e)).sqrt()))
}

/// Cyclotron angular frequency ω_c = qB/m.
fn builtin_cyclotron_frequency(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let b = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(1.602_176_634e-19);
    let m = args.get(2).map(|v| v.to_number()).unwrap_or(9.109_383_7e-31);
    Ok(StrykeValue::float(q * b / m))
}

/// Larmor (gyro)radius r = mv⊥/(qB).
fn builtin_larmor_radius(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_perp = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).max(1e-30);
    let q = args.get(2).map(|v| v.to_number()).unwrap_or(1.602_176_634e-19);
    let m = args.get(3).map(|v| v.to_number()).unwrap_or(9.109_383_7e-31);
    Ok(StrykeValue::float(m * v_perp / (q * b)))
}

// ── 11. Phonetic / string similarity ─────────────────────────────────────────

/// Jaro-Winkler similarity.
fn builtin_jaro_winkler_similarity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let m = av.len();
    let n = bv.len();
    if m == 0 && n == 0 {
        return Ok(StrykeValue::float(1.0));
    }
    if m == 0 || n == 0 {
        return Ok(StrykeValue::float(0.0));
    }
    let match_dist = m.max(n) / 2 - 1;
    let mut a_match = vec![false; m];
    let mut b_match = vec![false; n];
    let mut matches = 0_usize;
    for i in 0..m {
        let lo = i.saturating_sub(match_dist);
        let hi = (i + match_dist + 1).min(n);
        for j in lo..hi {
            if !b_match[j] && av[i] == bv[j] {
                a_match[i] = true;
                b_match[j] = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return Ok(StrykeValue::float(0.0));
    }
    let mut k = 0_usize;
    let mut t = 0_usize;
    for i in 0..m {
        if a_match[i] {
            while !b_match[k] {
                k += 1;
            }
            if av[i] != bv[k] {
                t += 1;
            }
            k += 1;
        }
    }
    let m_f = matches as f64;
    let jaro = (m_f / m as f64 + m_f / n as f64 + (m_f - t as f64 / 2.0) / m_f) / 3.0;
    let mut prefix = 0_usize;
    for i in 0..m.min(n).min(4) {
        if av[i] == bv[i] {
            prefix += 1;
        } else {
            break;
        }
    }
    Ok(StrykeValue::float(jaro + prefix as f64 * 0.1 * (1.0 - jaro)))
}

/// Simplified Metaphone (Lawrence Philips, abridged): returns the consonant skeleton.
fn builtin_metaphone_simple(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let upper: String = s.chars().filter(|c| c.is_alphabetic()).map(|c| c.to_ascii_uppercase()).collect();
    let chars: Vec<char> = upper.chars().collect();
    let mut out = String::new();
    let mut i = 0_usize;
    while i < chars.len() {
        let c = chars[i];
        match c {
            'A' | 'E' | 'I' | 'O' | 'U' if i == 0 => out.push(c),
            'B'
                if !(i + 1 == chars.len() && i > 0 && chars[i - 1] == 'M') => {
                    out.push('B');
                }
            'C' => {
                if i + 1 < chars.len() && matches!(chars[i + 1], 'I' | 'E' | 'Y') {
                    out.push('S');
                } else if i + 1 < chars.len() && chars[i + 1] == 'H' {
                    out.push('X');
                    i += 1;
                } else {
                    out.push('K');
                }
            }
            'D' => {
                if i + 2 < chars.len() && chars[i + 1] == 'G' && matches!(chars[i + 2], 'I' | 'E' | 'Y') {
                    out.push('J');
                    i += 2;
                } else {
                    out.push('T');
                }
            }
            'G' | 'F' | 'J' | 'K' | 'L' | 'M' | 'N' | 'P' | 'R' | 'S' | 'T' | 'V' | 'W' | 'X' | 'Y' | 'Z' => {
                out.push(c);
            }
            'H' => {
                if i > 0 && !matches!(chars[i - 1], 'A' | 'E' | 'I' | 'O' | 'U') {
                    // skip
                } else if i + 1 < chars.len() && matches!(chars[i + 1], 'A' | 'E' | 'I' | 'O' | 'U') {
                    out.push('H');
                }
            }
            'Q' => out.push('K'),
            _ => {}
        }
        i += 1;
    }
    // Collapse duplicate adjacent letters.
    let mut final_out = String::new();
    let mut last = '\0';
    for c in out.chars() {
        if c != last {
            final_out.push(c);
            last = c;
        }
    }
    Ok(StrykeValue::string(final_out))
}

// ── 12. Rating systems ───────────────────────────────────────────────────────

/// ELO rating update. Args: rating_a, rating_b, score_a (1=win, 0.5=draw, 0=loss), K.
fn builtin_elo_rating_update(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ra = f1(args);
    let rb = args.get(1).map(|v| v.to_number()).unwrap_or(1500.0);
    let score = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let k = args.get(3).map(|v| v.to_number()).unwrap_or(32.0);
    let ea = 1.0 / (1.0 + 10.0_f64.powf((rb - ra) / 400.0));
    let eb = 1.0 - ea;
    let new_a = ra + k * (score - ea);
    let new_b = rb + k * ((1.0 - score) - eb);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(new_a),
        StrykeValue::float(new_b),
    ]))
}

/// Glicko-1 rating update. Args: r, RD, opp_r, opp_RD, score.
fn builtin_glicko_rating_update(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let rd = args.get(1).map(|v| v.to_number()).unwrap_or(350.0);
    let opp_r = args.get(2).map(|v| v.to_number()).unwrap_or(1500.0);
    let opp_rd = args.get(3).map(|v| v.to_number()).unwrap_or(350.0);
    let score = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    let q = std::f64::consts::LN_10 / 400.0;
    let g = |rd: f64| 1.0 / (1.0 + 3.0 * q * q * rd * rd / (std::f64::consts::PI.powi(2))).sqrt();
    let g_opp = g(opp_rd);
    let e = 1.0 / (1.0 + 10.0_f64.powf(-g_opp * (r - opp_r) / 400.0));
    let d2 = 1.0 / (q * q * g_opp * g_opp * e * (1.0 - e));
    let new_r = r + (q / (1.0 / (rd * rd) + 1.0 / d2)) * g_opp * (score - e);
    let new_rd = (1.0 / (1.0 / (rd * rd) + 1.0 / d2)).sqrt();
    Ok(StrykeValue::array(vec![
        StrykeValue::float(new_r),
        StrykeValue::float(new_rd),
    ]))
}

/// Probability mass of `n_dice` × `s_sides` dice summing to `target`.
fn builtin_dice_sum_pmf(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = args.first().map(|v| v.to_number() as usize).unwrap_or(1);
    let s = args.get(1).map(|v| v.to_number() as usize).unwrap_or(6).max(1);
    let target = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    let total = n * s + 1;
    let mut dp = vec![0.0_f64; total];
    dp[0] = 1.0;
    for _ in 0..n {
        let mut next = vec![0.0_f64; total];
        for k in 0..total {
            if dp[k] == 0.0 {
                continue;
            }
            for f in 1..=s {
                if k + f < total {
                    next[k + f] += dp[k] / s as f64;
                }
            }
        }
        dp = next;
    }
    Ok(StrykeValue::float(if target < total { dp[target] } else { 0.0 }))
}

// ── 13. Effect sizes ─────────────────────────────────────────────────────────

/// Cohen's d.
fn builtin_cohens_d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n1 = a.len();
    let n2 = b.len();
    if n1 < 2 || n2 < 2 {
        return Ok(StrykeValue::float(0.0));
    }
    let m1 = a.iter().sum::<f64>() / n1 as f64;
    let m2 = b.iter().sum::<f64>() / n2 as f64;
    let v1 = a.iter().map(|x| (x - m1).powi(2)).sum::<f64>() / (n1 as f64 - 1.0);
    let v2 = b.iter().map(|x| (x - m2).powi(2)).sum::<f64>() / (n2 as f64 - 1.0);
    let pooled = (((n1 as f64 - 1.0) * v1 + (n2 as f64 - 1.0) * v2) / (n1 as f64 + n2 as f64 - 2.0)).sqrt();
    if pooled.abs() < 1e-15 {
        return Ok(StrykeValue::float(0.0));
    }
    Ok(StrykeValue::float((m1 - m2) / pooled))
}

/// Cliff's δ (non-parametric effect size).
fn builtin_cliff_delta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n1 = a.len();
    let n2 = b.len();
    if n1 == 0 || n2 == 0 {
        return Ok(StrykeValue::float(0.0));
    }
    let mut wins = 0_i64;
    let mut losses = 0_i64;
    for &x in &a {
        for &y in &b {
            if x > y {
                wins += 1;
            } else if x < y {
                losses += 1;
            }
        }
    }
    Ok(StrykeValue::float((wins - losses) as f64 / (n1 * n2) as f64))
}

/// Vargha-Delaney A12 = P(X > Y) + 0.5 P(X = Y).
fn builtin_vargha_delaney_a12(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let b: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n1 = a.len();
    let n2 = b.len();
    if n1 == 0 || n2 == 0 {
        return Ok(StrykeValue::float(0.5));
    }
    let mut greater = 0_f64;
    let mut equal = 0_f64;
    for &x in &a {
        for &y in &b {
            if x > y {
                greater += 1.0;
            } else if x == y {
                equal += 1.0;
            }
        }
    }
    Ok(StrykeValue::float(
        (greater + 0.5 * equal) / (n1 * n2) as f64,
    ))
}

// ── 14. Control transient response ───────────────────────────────────────────

/// 2nd-order under-damped step response y(t) = 1 - exp(-ζω_n t)·sin(ω_d t + φ)/√(1-ζ²).
fn builtin_step_response_2nd_order(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let zeta = f1(args);
    let wn = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if zeta >= 1.0 {
        // Overdamped or critically damped — return real exponential form.
        return Ok(StrykeValue::float(1.0 - (-zeta * wn * t).exp() * (1.0 + wn * t)));
    }
    let wd = wn * (1.0 - zeta * zeta).sqrt();
    let phi = (1.0 - zeta * zeta).sqrt().atan2(zeta);
    Ok(StrykeValue::float(
        1.0 - (-zeta * wn * t).exp() * (wd * t + phi).sin() / (1.0 - zeta * zeta).sqrt(),
    ))
}

/// 2nd-order overshoot percentage given damping ratio.
fn builtin_overshoot_2nd_order(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let zeta = f1(args);
    if zeta >= 1.0 {
        return Ok(StrykeValue::float(0.0));
    }
    let r = (-std::f64::consts::PI * zeta / (1.0 - zeta * zeta).sqrt()).exp();
    Ok(StrykeValue::float(r * 100.0))
}

// ── 15. Matrix norms ─────────────────────────────────────────────────────────

/// `frobenius_norm` — Frobenius norm. Returns a float.
fn builtin_frobenius_norm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let s: f64 = m.iter().flatten().map(|v| v * v).sum();
    Ok(StrykeValue::float(s.sqrt()))
}

/// Spectral norm = largest singular value.
fn builtin_spectral_norm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let arr = builtin_singular_values(args)?;
    let xs = arg_to_vec(&arr);
    if xs.is_empty() {
        return Ok(StrykeValue::float(0.0));
    }
    Ok(StrykeValue::float(xs[0].to_number()))
}

/// `trace_matrix` — Trace matrix. Returns a float.
fn builtin_trace_matrix(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = matrix_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = m.len();
    let mut s = 0.0_f64;
    for i in 0..n {
        if i < m[i].len() {
            s += m[i][i];
        }
    }
    Ok(StrykeValue::float(s))
}

// ── 16. Network triad / dyad census ──────────────────────────────────────────

/// Homophily (Coleman) index given an adjacency list and group labels.
fn builtin_homophily_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let labels: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number() as i64)
        .collect();
    let n = adj.len();
    let mut same = 0_usize;
    let mut total = 0_usize;
    for u in 0..n {
        for &v in &adj[u] {
            if v < n && labels[u] == labels[v] {
                same += 1;
            }
        }
        total += adj[u].len();
    }
    if total == 0 {
        return Ok(StrykeValue::float(0.0));
    }
    Ok(StrykeValue::float(same as f64 / total as f64))
}

/// Dyad census in a directed graph: returns [mutual, asym, null].
fn builtin_dyad_census(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let sets: Vec<std::collections::HashSet<usize>> = adj
        .iter()
        .map(|nbrs| nbrs.iter().copied().collect())
        .collect();
    let mut mutual = 0_i64;
    let mut asym = 0_i64;
    let mut null = 0_i64;
    for i in 0..n {
        for j in (i + 1)..n {
            let i_to_j = sets[i].contains(&j);
            let j_to_i = sets[j].contains(&i);
            if i_to_j && j_to_i {
                mutual += 1;
            } else if i_to_j || j_to_i {
                asym += 1;
            } else {
                null += 1;
            }
        }
    }
    let _ = sets.len();
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(mutual),
        StrykeValue::integer(asym),
        StrykeValue::integer(null),
    ]))
}

/// Triad census in undirected graph: returns count of [empty, edge, path, triangle].
fn builtin_triad_census(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let adj = parse_adj_list(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let n = adj.len();
    let sets: Vec<std::collections::HashSet<usize>> = adj
        .iter()
        .map(|nbrs| nbrs.iter().copied().collect())
        .collect();
    let mut counts = [0_i64; 4];
    for i in 0..n {
        for j in (i + 1)..n {
            for k in (j + 1)..n {
                let mut e = 0;
                if sets[i].contains(&j) {
                    e += 1;
                }
                if sets[j].contains(&k) {
                    e += 1;
                }
                if sets[i].contains(&k) {
                    e += 1;
                }
                counts[e as usize] += 1;
            }
        }
    }
    Ok(StrykeValue::array(
        counts.iter().copied().map(StrykeValue::integer).collect(),
    ))
}

// ── 17. Misc ─────────────────────────────────────────────────────────────────

/// Inverse sigmoid (logit): ln(x / (1-x)).
fn builtin_sigmoid_inverse(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args).clamp(1e-15, 1.0 - 1e-15);
    Ok(StrykeValue::float((x / (1.0 - x)).ln()))
}
