// ─────────────────────────────────────────────────────────────────────────────
// figurate / OEIS sequences, set-theoretic operations,
// polynomial root-finding (Durand-Kerner, Lin-Bairstow), classic data-structure
// algorithms (heap ops, segment tree, Fenwick), string-algorithm staples
// (KMP failure function, Z-array, suffix array, Manacher, Rabin-Karp),
// regex helpers, classical scheduling, bit-twiddling tricks, more finance,
// and ISO 8601 / RFC 3339 datetime helpers.
// ─────────────────────────────────────────────────────────────────────────────

// ── 1. Figurate / OEIS sequences ────────────────────────────────────────────

/// Tetrahedral number T_n = n(n+1)(n+2)/6.
fn builtin_tetrahedral_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer(n * (n + 1) * (n + 2) / 6))
}

/// Square-pyramidal number n(n+1)(2n+1)/6.
fn builtin_square_pyramidal_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer(n * (n + 1) * (2 * n + 1) / 6))
}

/// Octahedral number n(2n²+1)/3.
fn builtin_octahedral_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer(n * (2 * n * n + 1) / 3))
}

/// Pentagonal-pyramidal n²(n+1)/2.
fn builtin_pentagonal_pyramidal_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer(n * n * (n + 1) / 2))
}

/// Cake number C_n = (n³ + 5n + 6)/6 (number of regions cutting a 3-D cake with n planes).
fn builtin_cake_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0);
    Ok(StrykeValue::integer((n * n * n + 5 * n + 6) / 6))
}

/// Cuban number c_n = (3n² + 6n + 1)·... Wait, define properly:
/// Cuban prime → not number. Use cuban polynomial: a_n = (n+1)³ - n³.
fn builtin_cuban_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(0);
    let a = (n + 1).pow(3) - n.pow(3);
    Ok(StrykeValue::integer(a))
}

/// Centered hexagonal number 3n(n−1) + 1.
fn builtin_centered_hexagonal_number(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(1);
    Ok(StrykeValue::integer(3 * n * (n - 1) + 1))
}

/// Test if N is a Carmichael number: composite, n > 1, and a^n ≡ a (mod n) for all a coprime to n.
fn builtin_carmichael_q(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    if n < 4 || is_prime_check(n) {
        return Ok(StrykeValue::integer(0));
    }
    if n & 1 == 0 {
        return Ok(StrykeValue::integer(0));
    }
    // Korselt's criterion: n square-free and (p − 1) | (n − 1) for every prime p | n.
    let mut nn = n;
    let mut p = 3_i64;
    while p * p <= nn {
        if nn % p == 0 {
            // Check square-free.
            if (nn / p) % p == 0 {
                return Ok(StrykeValue::integer(0));
            }
            if (n - 1) % (p - 1) != 0 {
                return Ok(StrykeValue::integer(0));
            }
            nn /= p;
        }
        p += 2;
    }
    if nn > 1 && (n - 1) % (nn - 1) != 0 {
        return Ok(StrykeValue::integer(0));
    }
    Ok(StrykeValue::integer(1))
}

/// Test if N is sphenic (product of three distinct primes).
fn builtin_sphenic_q(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    if n < 30 {
        return Ok(StrykeValue::integer(0));
    }
    let factors = prime_factorize(n);
    let mut uniq = factors.clone();
    uniq.sort();
    uniq.dedup();
    Ok(StrykeValue::integer(if factors.len() == 3 && uniq.len() == 3 { 1 } else { 0 }))
}

/// Smooth-up-to-B test (every prime factor ≤ 7). Convenient alias for batch-9 b_smooth_q with B=7.
fn builtin_seven_smooth_q(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    let factors = prime_factorize(n);
    Ok(StrykeValue::integer(if factors.iter().all(|&f| f <= 7) { 1 } else { 0 }))
}

// ── 2. Set theory ───────────────────────────────────────────────────────────

/// N-way Cartesian product of arrays.
fn builtin_cartesian_product_n(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let lists = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let parsed: Vec<Vec<StrykeValue>> = lists.iter().map(arg_to_vec).collect();
    if parsed.is_empty() {
        return Ok(StrykeValue::array(vec![]));
    }
    let mut out: Vec<Vec<StrykeValue>> = vec![Vec::new()];
    for list in &parsed {
        let mut next: Vec<Vec<StrykeValue>> = Vec::new();
        for prefix in &out {
            for item in list {
                let mut new_row = prefix.clone();
                new_row.push(item.clone());
                next.push(new_row);
            }
        }
        out = next;
    }
    Ok(StrykeValue::array(
        out.into_iter().map(StrykeValue::array).collect(),
    ))
}

/// Multiset union (max counts). Output is sorted lexically so the result is
/// deterministic regardless of input ordering or `HashMap` rehashing.
fn builtin_multiset_union(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for v in &a {
        *counts.entry(v.to_string()).or_insert(0) += 1;
    }
    let mut counts_b: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for v in &b {
        *counts_b.entry(v.to_string()).or_insert(0) += 1;
    }
    let mut out = Vec::new();
    let mut keys: Vec<&String> = counts.keys().chain(counts_b.keys()).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        let mc = (*counts.get(k).unwrap_or(&0)).max(*counts_b.get(k).unwrap_or(&0));
        for _ in 0..mc {
            out.push(StrykeValue::string(k.clone()));
        }
    }
    Ok(StrykeValue::array(out))
}

/// Multiset intersection (min counts). Output is sorted lexically.
fn builtin_multiset_intersection(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for v in &a {
        *counts.entry(v.to_string()).or_insert(0) += 1;
    }
    let mut counts_b: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for v in &b {
        *counts_b.entry(v.to_string()).or_insert(0) += 1;
    }
    let mut out = Vec::new();
    for (k, v) in &counts {
        let mc = (*v).min(*counts_b.get(k).unwrap_or(&0));
        for _ in 0..mc {
            out.push(StrykeValue::string(k.clone()));
        }
    }
    Ok(StrykeValue::array(out))
}

/// Multiset difference (subtract counts, floor 0). Output is sorted lexically.
fn builtin_multiset_difference(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let b = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for v in &a {
        *counts.entry(v.to_string()).or_insert(0) += 1;
    }
    for v in &b {
        let entry = counts.entry(v.to_string()).or_insert(0);
        if *entry > 0 {
            *entry -= 1;
        }
    }
    let mut out = Vec::new();
    for (k, v) in &counts {
        for _ in 0..*v {
            out.push(StrykeValue::string(k.clone()));
        }
    }
    Ok(StrykeValue::array(out))
}

// ── 3. Polynomial root finding ──────────────────────────────────────────────

/// Durand-Kerner iteration on a polynomial. Returns approximate complex roots
/// as `[Re, Im]` pairs. Coefficients are low-to-high.
fn builtin_polynomial_roots_dk(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let coeffs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    // Empty polynomial → no roots; without this guard `coeffs.len()
    // - 1` usize-underflows and panics.
    if coeffs.is_empty() {
        return Ok(StrykeValue::array(vec![]));
    }
    let n = coeffs.len() - 1;
    if n == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    let lead = coeffs[n];
    if lead.abs() < 1e-15 {
        return Ok(StrykeValue::array(vec![]));
    }
    let mon: Vec<f64> = coeffs.iter().map(|c| c / lead).collect();
    // Initial guesses: 0.4 + 0.9i raised to k-th power.
    let mut roots: Vec<(f64, f64)> = (0..n)
        .map(|k| {
            let theta = 2.0 * std::f64::consts::PI * k as f64 / n as f64;
            (theta.cos() * 0.4, theta.sin() * 0.4 + 0.9)
        })
        .collect();
    let eval = |r: (f64, f64), mon: &[f64]| -> (f64, f64) {
        let mut re = 0.0_f64;
        let mut im = 0.0_f64;
        for &c in mon.iter().rev() {
            // (re + i im) * (r.0 + i r.1) + c
            let nr = re * r.0 - im * r.1 + c;
            let ni = re * r.1 + im * r.0;
            re = nr;
            im = ni;
        }
        (re, im)
    };
    for _ in 0..200 {
        let mut max_d: f64 = 0.0;
        for i in 0..n {
            let mut denom_re = 1.0_f64;
            let mut denom_im = 0.0_f64;
            for j in 0..n {
                if i == j {
                    continue;
                }
                let dr = roots[i].0 - roots[j].0;
                let di = roots[i].1 - roots[j].1;
                let nr = denom_re * dr - denom_im * di;
                let ni = denom_re * di + denom_im * dr;
                denom_re = nr;
                denom_im = ni;
            }
            let f = eval(roots[i], &mon);
            let mag2 = denom_re * denom_re + denom_im * denom_im;
            if mag2 < 1e-30 {
                continue;
            }
            let q_re = (f.0 * denom_re + f.1 * denom_im) / mag2;
            let q_im = (f.1 * denom_re - f.0 * denom_im) / mag2;
            roots[i].0 -= q_re;
            roots[i].1 -= q_im;
            let d = (q_re * q_re + q_im * q_im).sqrt();
            if d > max_d {
                max_d = d;
            }
        }
        if max_d < 1e-12 {
            break;
        }
    }
    Ok(StrykeValue::array(
        roots
            .into_iter()
            .map(|(r, i)| {
                StrykeValue::array(vec![StrykeValue::float(r), StrykeValue::float(i)])
            })
            .collect(),
    ))
}

/// Lin-Bairstow factorisation: extracts one quadratic factor x² + ux + v from
/// a polynomial. Returns `[u, v, deflated_coeffs]`.
fn builtin_lin_bairstow_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut a: Vec<f64> = poly_from_value(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    if a.len() < 3 {
        return Ok(StrykeValue::array(vec![]));
    }
    let mut u = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let mut v = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let n = a.len() - 1;
    let mut b = vec![0.0_f64; n + 1];
    let mut c = vec![0.0_f64; n + 1];
    for _ in 0..200 {
        b[n] = a[n];
        b[n - 1] = a[n - 1] - u * b[n];
        for k in (0..n - 1).rev() {
            b[k] = a[k] - u * b[k + 1] - v * b[k + 2];
        }
        c[n] = b[n];
        c[n - 1] = b[n - 1] - u * c[n];
        for k in (0..n - 1).rev() {
            c[k] = b[k] - u * c[k + 1] - v * c[k + 2];
        }
        let det = c[2] * c[2] - c[3] * c[1];
        if det.abs() < 1e-30 {
            break;
        }
        let du = (b[1] * c[2] - b[0] * c[3]) / det;
        let dv = (b[0] * c[2] - b[1] * c[1]) / det;
        u += du;
        v += dv;
        if du.abs() < 1e-12 && dv.abs() < 1e-12 {
            break;
        }
    }
    // Deflated polynomial coefficients are b[2..=n].
    let deflated: Vec<f64> = b[2..=n].to_vec();
    a.truncate(0);
    Ok(StrykeValue::array(vec![
        StrykeValue::float(u),
        StrykeValue::float(v),
        poly_to_value(&deflated),
    ]))
}

// ── 4. Tree / heap / Fenwick / segment-tree utilities ───────────────────────

/// Sift-down on a 0-indexed binary max-heap (in-place semantics).
fn builtin_heap_sift_down(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut arr: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut i = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let n = arr.len();
    loop {
        let l = 2 * i + 1;
        let r = 2 * i + 2;
        let mut largest = i;
        if l < n && arr[l] > arr[largest] {
            largest = l;
        }
        if r < n && arr[r] > arr[largest] {
            largest = r;
        }
        if largest == i {
            break;
        }
        arr.swap(i, largest);
        i = largest;
    }
    Ok(StrykeValue::array(arr.into_iter().map(StrykeValue::float).collect()))
}

/// Build a Fenwick (BIT) tree from an array. Returns the prefix-sum tree.
fn builtin_fenwick_build(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let arr: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let n = arr.len();
    let mut bit = vec![0.0_f64; n + 1];
    for (i, &x) in arr.iter().enumerate() {
        let mut idx = i + 1;
        while idx <= n {
            bit[idx] += x;
            idx += idx & idx.wrapping_neg();
        }
    }
    Ok(StrykeValue::array(bit.into_iter().map(StrykeValue::float).collect()))
}

/// Prefix sum on a Fenwick tree up to (and including) `i`.
fn builtin_fenwick_query(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let bit: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut i = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0) + 1;
    let mut s = 0.0_f64;
    while i > 0 && i < bit.len() {
        s += bit[i];
        i -= i & i.wrapping_neg();
    }
    Ok(StrykeValue::float(s))
}

/// Segment-tree sum query on a 1-D array `[arr, l, r]` (inclusive).
fn builtin_segment_tree_sum(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let arr: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    // Empty array → sum is 0; avoid the `arr.len() - 1` underflow
    // panic below.
    if arr.is_empty() {
        return Ok(StrykeValue::float(0.0));
    }
    let l = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let r = args
        .get(2)
        .map(|v| v.to_number() as usize)
        .unwrap_or_else(|| arr.len().saturating_sub(1));
    // Clamp `l` to the in-range region to avoid slicing past the
    // array end (which panics on `arr[l..=...]` when `l >= arr.len()`).
    let l = l.min(arr.len() - 1);
    let s: f64 = arr[l..=r.min(arr.len() - 1)].iter().sum();
    Ok(StrykeValue::float(s))
}

// ── 5. String algorithms ────────────────────────────────────────────────────

/// KMP failure function.
fn builtin_kmp_failure(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut f = vec![0_i64; n];
    let mut k = 0_i64;
    for i in 1..n {
        while k > 0 && chars[k as usize] != chars[i] {
            k = f[(k - 1) as usize];
        }
        if chars[k as usize] == chars[i] {
            k += 1;
        }
        f[i] = k;
    }
    Ok(StrykeValue::array(f.into_iter().map(StrykeValue::integer).collect()))
}

/// Z-array (Z-algorithm).
fn builtin_z_array(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut z = vec![0_i64; n];
    if n == 0 {
        return Ok(StrykeValue::array(vec![]));
    }
    z[0] = n as i64;
    let mut l = 0_i64;
    let mut r = 0_i64;
    for i in 1..n as i64 {
        if i < r {
            z[i as usize] = (r - i).min(z[(i - l) as usize]);
        }
        while (i + z[i as usize]) < n as i64
            && chars[z[i as usize] as usize] == chars[(i + z[i as usize]) as usize]
        {
            z[i as usize] += 1;
        }
        if i + z[i as usize] > r {
            l = i;
            r = i + z[i as usize];
        }
    }
    Ok(StrykeValue::array(z.into_iter().map(StrykeValue::integer).collect()))
}

/// Naïve suffix array (O(n² log n) — for educational use; longest expected n ~ a few thousand).
fn builtin_suffix_array_naive(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let n = s.chars().count();
    let mut idx: Vec<usize> = (0..n).collect();
    let bytes = s.as_bytes();
    idx.sort_by(|&a, &b| bytes[a..].cmp(&bytes[b..]));
    Ok(StrykeValue::array(
        idx.into_iter().map(|i| StrykeValue::integer(i as i64)).collect(),
    ))
}

/// Manacher's algorithm: longest palindromic-substring radii (odd lengths only).
fn builtin_manacher_radii(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut p = vec![0_i64; n];
    let mut center = 0_i64;
    let mut right = 0_i64;
    for i in 0..n as i64 {
        let mirror = 2 * center - i;
        if i < right {
            p[i as usize] = (right - i).min(p[mirror.max(0) as usize]);
        }
        while (i + p[i as usize] + 1) < n as i64
            && (i - p[i as usize] - 1) >= 0
            && chars[(i + p[i as usize] + 1) as usize]
                == chars[(i - p[i as usize] - 1) as usize]
        {
            p[i as usize] += 1;
        }
        if i + p[i as usize] > right {
            center = i;
            right = i + p[i as usize];
        }
    }
    Ok(StrykeValue::array(p.into_iter().map(StrykeValue::integer).collect()))
}

/// Polynomial Rabin-Karp rolling hash of a string with prime modulus.
fn builtin_rabin_karp_hash(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let base = args.get(1).map(|v| v.to_number() as u64).unwrap_or(257);
    let modulus = args.get(2).map(|v| v.to_number() as u64).unwrap_or(1_000_000_007);
    let mut h = 0_u64;
    for b in s.bytes() {
        h = (h.wrapping_mul(base).wrapping_add(b as u64)) % modulus;
    }
    Ok(StrykeValue::integer(h as i64))
}

/// Longest-common-prefix array from sorted suffix array (Kasai-like O(n)).
fn builtin_lcp_array(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut sa: Vec<usize> = (0..n).collect();
    sa.sort_by(|&a, &b| bytes[a..].cmp(&bytes[b..]));
    let mut rank = vec![0_usize; n];
    for (i, &p) in sa.iter().enumerate() {
        rank[p] = i;
    }
    let mut lcp = vec![0_i64; n];
    let mut h = 0_usize;
    for i in 0..n {
        if rank[i] > 0 {
            let j = sa[rank[i] - 1];
            while i + h < n && j + h < n && bytes[i + h] == bytes[j + h] {
                h += 1;
            }
            lcp[rank[i]] = h as i64;
            h = h.saturating_sub(1);
        }
    }
    Ok(StrykeValue::array(lcp.into_iter().map(StrykeValue::integer).collect()))
}

// ── 6. Regex helpers ────────────────────────────────────────────────────────

/// Escape regex metacharacters in a literal string.
fn builtin_regex_escape_simple(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if matches!(
            c,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' | '#'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    Ok(StrykeValue::string(out))
}

/// Boyer-Moore-Horspool string search; returns first index or -1.
fn builtin_horspool_search(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let text = args.first().map(|v| v.to_string()).unwrap_or_default();
    let pattern = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let tb = text.as_bytes();
    let pb = pattern.as_bytes();
    let m = pb.len();
    let n = tb.len();
    if m == 0 {
        return Ok(StrykeValue::integer(0));
    }
    if m > n {
        return Ok(StrykeValue::integer(-1));
    }
    let mut shift = vec![m; 256];
    for (i, &b) in pb.iter().take(m - 1).enumerate() {
        shift[b as usize] = m - 1 - i;
    }
    let mut i = 0_usize;
    while i + m <= n {
        let mut j = m - 1;
        while tb[i + j] == pb[j] {
            if j == 0 {
                return Ok(StrykeValue::integer(i as i64));
            }
            j -= 1;
        }
        i += shift[tb[i + m - 1] as usize];
    }
    Ok(StrykeValue::integer(-1))
}

// ── 7. Scheduling ───────────────────────────────────────────────────────────

/// Largest-Processing-Time list scheduling onto m identical machines.
/// Returns assigned-machine index per job (in original order) and the makespan.
fn builtin_lpt_schedule(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let jobs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let m = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1).max(1);
    let mut load = vec![0.0_f64; m];
    let n = jobs.len();
    let mut indexed: Vec<(usize, f64)> = jobs.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut machine = vec![0_i64; n];
    for (orig, t) in indexed {
        let (idx, _) = load
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();
        machine[orig] = idx as i64;
        load[idx] += t;
    }
    let makespan = load.iter().cloned().fold(0.0_f64, f64::max);
    Ok(StrykeValue::array(vec![
        StrykeValue::array(machine.into_iter().map(StrykeValue::integer).collect()),
        StrykeValue::float(makespan),
    ]))
}

/// Johnson's two-machine flow-shop scheduling. Returns the optimal sequence as
/// 0-based job indices. Args: list of `[a_i, b_i]` pairs.
fn builtin_johnsons_rule(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let raw = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF));
    let jobs: Vec<(f64, f64)> = raw
        .iter()
        .map(|p| {
            let v = arg_to_vec(p);
            (
                v.first().map(|x| x.to_number()).unwrap_or(0.0),
                v.get(1).map(|x| x.to_number()).unwrap_or(0.0),
            )
        })
        .collect();
    let n = jobs.len();
    let mut head: Vec<usize> = Vec::new();
    let mut tail: Vec<usize> = Vec::new();
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&a, &b| {
        let ka = jobs[a].0.min(jobs[a].1);
        let kb = jobs[b].0.min(jobs[b].1);
        ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
    });
    for i in indices {
        if jobs[i].0 <= jobs[i].1 {
            head.push(i);
        } else {
            tail.insert(0, i);
        }
    }
    head.extend(tail);
    Ok(StrykeValue::array(
        head.into_iter().map(|i| StrykeValue::integer(i as i64)).collect(),
    ))
}

// ── 8. Bit twiddling ────────────────────────────────────────────────────────

/// Reverse the bits of a 32-bit integer.
fn builtin_bit_reverse_32(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as u32;
    Ok(StrykeValue::integer(n.reverse_bits() as i64))
}

/// Convert a binary number to its Gray-code value.
fn builtin_bin_to_gray(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as u64;
    Ok(StrykeValue::integer((n ^ (n >> 1)) as i64))
}

/// Convert a Gray-code value back to its binary representation.
fn builtin_gray_to_bin(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut g = i1(args) as u64;
    let mut n = g;
    g >>= 1;
    while g != 0 {
        n ^= g;
        g >>= 1;
    }
    Ok(StrykeValue::integer(n as i64))
}

/// Swap two arbitrary bit positions in a 64-bit integer.
fn builtin_swap_bits_pos(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut n = i1(args);
    let i = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let j = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let bit_i = (n >> i) & 1;
    let bit_j = (n >> j) & 1;
    if bit_i != bit_j {
        n ^= (1 << i) | (1 << j);
    }
    Ok(StrykeValue::integer(n))
}

/// Hamming weight of an integer (popcount).
fn builtin_hamming_weight(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args) as u64;
    Ok(StrykeValue::integer(n.count_ones() as i64))
}

/// Hamming distance between two integers.
fn builtin_hamming_distance_int(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = i1(args) as u64;
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0) as u64;
    Ok(StrykeValue::integer((a ^ b).count_ones() as i64))
}

// ── 9. Finance ──────────────────────────────────────────────────────────────

/// Internal Rate of Return via Newton iteration.
fn builtin_internal_rate_of_return(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cf: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut r = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    for _ in 0..200 {
        let mut npv = 0.0_f64;
        let mut d_npv = 0.0_f64;
        for (t, &c) in cf.iter().enumerate() {
            let factor = (1.0 + r).powi(t as i32);
            npv += c / factor;
            if t > 0 {
                d_npv -= t as f64 * c / (factor * (1.0 + r));
            }
        }
        if d_npv.abs() < 1e-15 {
            break;
        }
        let dr = npv / d_npv;
        r -= dr;
        if dr.abs() < 1e-12 {
            break;
        }
    }
    Ok(StrykeValue::float(r))
}

/// Modified IRR. Args: cashflows, finance_rate, reinvest_rate.
fn builtin_modified_irr(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cf: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let finance_rate = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let reinvest_rate = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    let n = cf.len();
    if n < 2 {
        return Ok(StrykeValue::float(0.0));
    }
    let mut neg_pv = 0.0_f64;
    let mut pos_fv = 0.0_f64;
    for (t, &c) in cf.iter().enumerate() {
        if c < 0.0 {
            neg_pv += c / (1.0 + finance_rate).powi(t as i32);
        } else if c > 0.0 {
            pos_fv += c * (1.0 + reinvest_rate).powi((n - 1 - t) as i32);
        }
    }
    if neg_pv.abs() < 1e-15 {
        return Ok(StrykeValue::float(0.0));
    }
    Ok(StrykeValue::float(
        (-pos_fv / neg_pv).powf(1.0 / (n as f64 - 1.0)) - 1.0,
    ))
}

/// Payback period (fractional) given initial outflow and uniform-period cashflows.
fn builtin_payback_period_simple(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let cf: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter()
        .map(|v| v.to_number())
        .collect();
    let mut acc = 0.0_f64;
    for (i, &c) in cf.iter().enumerate() {
        let prev = acc;
        acc += c;
        if acc >= 0.0 && prev < 0.0 {
            let frac = -prev / c.max(1e-30);
            return Ok(StrykeValue::float(i as f64 - 1.0 + frac));
        }
    }
    Ok(StrykeValue::float(f64::INFINITY))
}

// ── 10. ISO 8601 / RFC 3339 datetime helpers ────────────────────────────────

fn pad2(n: i64) -> String {
    format!("{:02}", n)
}

/// Format Y/M/D h:m:s as RFC 3339 ("YYYY-MM-DDThh:mm:ssZ").
fn builtin_rfc3339_format(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let y = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let mo = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let h = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    let mi = args.get(4).map(|v| v.to_number() as i64).unwrap_or(0);
    let s = args.get(5).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::string(format!(
        "{:04}-{}-{}T{}:{}:{}Z",
        y, pad2(mo), pad2(d), pad2(h), pad2(mi), pad2(s)
    )))
}

/// Parse "YYYY-MM-DDThh:mm:ssZ" into [Y, M, D, h, m, s]. Naïve, UTC only.
fn builtin_rfc3339_parse(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let parts_date_time: Vec<&str> = s.split('T').collect();
    if parts_date_time.len() != 2 {
        return Err(StrykeError::runtime("rfc3339_parse: missing 'T'", 0));
    }
    let date_parts: Vec<&str> = parts_date_time[0].split('-').collect();
    let time_str = parts_date_time[1].trim_end_matches('Z');
    let time_parts: Vec<&str> = time_str.split(':').collect();
    if date_parts.len() != 3 || time_parts.len() != 3 {
        return Err(StrykeError::runtime("rfc3339_parse: malformed", 0));
    }
    let y: i64 = date_parts[0].parse().unwrap_or(0);
    let mo: i64 = date_parts[1].parse().unwrap_or(1);
    let d: i64 = date_parts[2].parse().unwrap_or(1);
    let h: i64 = time_parts[0].parse().unwrap_or(0);
    let mi: i64 = time_parts[1].parse().unwrap_or(0);
    let sec: f64 = time_parts[2].parse().unwrap_or(0.0);
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(y),
        StrykeValue::integer(mo),
        StrykeValue::integer(d),
        StrykeValue::integer(h),
        StrykeValue::integer(mi),
        StrykeValue::float(sec),
    ]))
}

/// Format Y/M/D as ISO 8601 ordinal date "YYYY-DDD".
fn builtin_iso_ordinal_date(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let y = args.first().map(|v| v.to_number() as i64).unwrap_or(0);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let day_of_year = ymd_to_days(y, m, d) - ymd_to_days(y, 1, 1) + 1;
    Ok(StrykeValue::string(format!("{:04}-{:03}", y, day_of_year)))
}
