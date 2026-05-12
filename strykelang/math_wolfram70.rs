// Postgres-flavour SQL string, JSON, regex, aggregate, full-text,
// trigram-similarity primitives.

fn b70_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

fn b70_to_codepoints(v: &StrykeValue) -> Vec<i64> {
    arg_to_vec(v).iter().map(|x| x.to_number() as i64).collect()
}

/// btrim(s, chars) — trim chars from both sides. Args: code-points of s, then
/// char-set (as flattened code-points). Returns trimmed code-points.
fn builtin_btrim(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = b70_to_codepoints(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let set: std::collections::HashSet<i64> = args.get(1)
        .map(b70_to_codepoints)
        .unwrap_or_else(|| vec![b' ' as i64])
        .into_iter().collect();
    let mut start = 0;
    while start < s.len() && set.contains(&s[start]) { start += 1; }
    let mut end = s.len();
    while end > start && set.contains(&s[end - 1]) { end -= 1; }
    Ok(StrykeValue::array(s[start..end].iter().copied().map(StrykeValue::integer).collect()))
}

/// translate(s, from, to) — replace each char in `from` with the matching char
/// in `to`; chars without a match are deleted. Postgres semantics.
fn builtin_translate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = b70_to_codepoints(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let from = args.get(1).map(b70_to_codepoints).unwrap_or_default();
    let to = args.get(2).map(b70_to_codepoints).unwrap_or_default();
    let mut out = Vec::with_capacity(s.len());
    for c in s {
        if let Some(i) = from.iter().position(|&f| f == c) {
            if let Some(&t) = to.get(i) { out.push(t); }
        } else {
            out.push(c);
        }
    }
    Ok(StrykeValue::array(out.into_iter().map(StrykeValue::integer).collect()))
}

/// ascii(s) — code-point of first char, 0 for empty.
fn builtin_ascii(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = b70_to_codepoints(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(s.first().copied().unwrap_or(0)))
}

/// regexp_split — count of segments after split (we don't carry the regex
/// engine here; caller supplies pre-counted segment count + delimiter hits).
fn builtin_regexp_split(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let hits = i1(args);
    Ok(StrykeValue::integer(hits + 1))
}

/// regexp_matches — count of matches found.
fn builtin_regexp_matches(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let hits = i1(args);
    Ok(StrykeValue::integer(hits.max(0)))
}

/// regexp_replace — count of replacements made.
fn builtin_regexp_replace(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let hits = i1(args);
    let global = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if global != 0 { hits } else { hits.min(1) }))
}

/// json_build_object — produce a key-count given args. Postgres takes a
/// variadic (k, v, k, v, ...). We just check parity and return n_keys.
fn builtin_json_build_object(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = args.len() as i64;
    if n % 2 != 0 { return Ok(StrykeValue::integer(-1)); }
    Ok(StrykeValue::integer(n / 2))
}

/// jsonb_set(target, path, new_value, create_if_missing). Returns 1 on success.
fn builtin_jsonb_set(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let target_size = i1(args);
    let path_len = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if target_size > 0 && path_len > 0 { 1 } else { 0 }))
}

/// json_array_length.
fn builtin_json_array_length(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.len() as i64))
}

/// json_extract_path — depth of path traversal that succeeded.
fn builtin_json_extract_path(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let path = b70_to_codepoints(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(path.len() as i64))
}

/// json_strip_nulls — count of null fields removed.
fn builtin_json_strip_nulls(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let nulls = i1(args);
    Ok(StrykeValue::integer(nulls.max(0)))
}

/// jsonb_pretty — character count of pretty-printed output (newlines + 2-space
/// indent per level). Args: token_count, max_depth.
fn builtin_jsonb_pretty(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let tokens = i1(args);
    let depth = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(StrykeValue::integer(tokens + tokens * (1 + 2 * depth)))
}

/// jsonb_path_query — boolean: does path match.
fn builtin_jsonb_path_query(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let depth_match = i1(args);
    let path_len = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if depth_match == path_len && path_len > 0 { 1 } else { 0 }))
}

/// json_each — count of (key, value) pairs.
fn builtin_json_each(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer((v.len() / 2) as i64))
}

/// jsonb_array_length.
fn builtin_jsonb_array_length(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_json_array_length(args)
}

/// jsonb_object_keys — count of keys.
fn builtin_jsonb_object_keys(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer((v.len() / 2) as i64))
}

/// jsonb_typeof — returns numeric type id: 0 null, 1 bool, 2 number, 3 string,
/// 4 array, 5 object.
fn builtin_jsonb_typeof(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let id = i1(args).clamp(0, 5);
    Ok(StrykeValue::integer(id))
}

/// array_to_jsonb — count of elements emitted.
fn builtin_array_to_jsonb(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.len() as i64))
}

/// ts_match @@ — boolean: does tsvector match tsquery (1 if any term overlaps).
fn builtin_ts_match(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let vec_terms: std::collections::HashSet<i64> =
        b70_to_codepoints(args.first().unwrap_or(&StrykeValue::array(vec![]))).into_iter().collect();
    let query_terms = args.get(1).map(b70_to_codepoints).unwrap_or_default();
    Ok(StrykeValue::integer(if query_terms.iter().any(|t| vec_terms.contains(t)) { 1 } else { 0 }))
}

/// ts_rank — Postgres tsvector ranking: Σ weight_i / (1 + log(doc_len)).
fn builtin_ts_rank(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let weights = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let doc_len = args.get(1).map(|v| v.to_number().max(1.0)).unwrap_or(1.0);
    let s: f64 = weights.iter().sum();
    Ok(StrykeValue::float(s / (1.0 + doc_len.ln())))
}

/// ts_headline — number of highlighted snippets.
fn builtin_ts_headline(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let max_words = i1(args).max(1);
    let matches = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(matches.min(max_words)))
}

/// substring_similarity — pg_trgm similarity between substring and target.
fn builtin_substring_similarity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a: std::collections::HashSet<i64> =
        b70_to_codepoints(args.first().unwrap_or(&StrykeValue::array(vec![]))).into_iter().collect();
    let b: std::collections::HashSet<i64> =
        args.get(1).map(b70_to_codepoints).unwrap_or_default().into_iter().collect();
    if a.is_empty() && b.is_empty() { return Ok(StrykeValue::float(1.0)); }
    let inter = a.intersection(&b).count() as f64;
    let union = a.union(&b).count() as f64;
    Ok(StrykeValue::float(inter / union.max(1.0)))
}

/// levenshtein_dist — classic edit distance.
fn builtin_levenshtein_dist(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b70_to_codepoints(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = args.get(1).map(b70_to_codepoints).unwrap_or_default();
    let (la, lb) = (a.len(), b.len());
    let mut prev: Vec<usize> = (0..=lb).collect();
    let mut cur = vec![0_usize; lb + 1];
    for i in 1..=la {
        cur[0] = i;
        for j in 1..=lb {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    Ok(StrykeValue::integer(prev[lb] as i64))
}

/// word_similarity — pg_trgm word_similarity (left side substring of right).
fn builtin_word_similarity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_substring_similarity(args)
}

/// strict_word_similarity — same but with word boundaries (boolean for hit).
fn builtin_strict_word_similarity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sim = f1(args);
    Ok(StrykeValue::float(if sim >= 0.5 { sim } else { 0.0 }))
}

/// hstore_to_array — flatten {k=>v, ...} into [k1, v1, k2, v2, ...].
fn builtin_hstore_to_array(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::array(v))
}

/// array_to_hstore — fold flat array back into key-count.
fn builtin_array_to_hstore(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer((v.len() / 2) as i64))
}

/// string_agg — total characters in concatenation with separator length s.
fn builtin_string_agg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let sep = args.get(1).map(|x| x.to_number() as i64).unwrap_or(0);
    let total: f64 = v.iter().sum();
    let between = (v.len() as i64 - 1).max(0) * sep;
    Ok(StrykeValue::integer(total as i64 + between))
}

/// array_agg — element count.
fn builtin_array_agg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.len() as i64))
}

/// corr_agg — Pearson correlation aggregate.
fn builtin_corr_agg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let ys = args.get(1).map(b70_to_floats).unwrap_or_default();
    let n = xs.len().min(ys.len()) as f64;
    if n < 2.0 { return Ok(StrykeValue::float(0.0)); }
    let mx = xs.iter().take(n as usize).sum::<f64>() / n;
    let my = ys.iter().take(n as usize).sum::<f64>() / n;
    let mut num = 0.0;
    let mut dx2 = 0.0;
    let mut dy2 = 0.0;
    for i in 0..n as usize {
        let dx = xs[i] - mx;
        let dy = ys[i] - my;
        num += dx * dy;
        dx2 += dx * dx;
        dy2 += dy * dy;
    }
    let denom = (dx2 * dy2).sqrt();
    Ok(StrykeValue::float(if denom > 0.0 { num / denom } else { 0.0 }))
}

/// covar_pop — population covariance.
fn builtin_covar_pop(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let ys = args.get(1).map(b70_to_floats).unwrap_or_default();
    let n = xs.len().min(ys.len());
    if n == 0 { return Ok(StrykeValue::float(0.0)); }
    let mx = xs.iter().take(n).sum::<f64>() / n as f64;
    let my = ys.iter().take(n).sum::<f64>() / n as f64;
    let s: f64 = (0..n).map(|i| (xs[i] - mx) * (ys[i] - my)).sum();
    Ok(StrykeValue::float(s / n as f64))
}

/// covar_samp — sample covariance (n-1 divisor).
fn builtin_covar_samp(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let ys = args.get(1).map(b70_to_floats).unwrap_or_default();
    let n = xs.len().min(ys.len());
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let mx = xs.iter().take(n).sum::<f64>() / n as f64;
    let my = ys.iter().take(n).sum::<f64>() / n as f64;
    let s: f64 = (0..n).map(|i| (xs[i] - mx) * (ys[i] - my)).sum();
    Ok(StrykeValue::float(s / (n - 1) as f64))
}

/// regr_slope — linear-regression slope = cov(x,y) / var(x).
fn builtin_regr_slope(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let ys = args.get(1).map(b70_to_floats).unwrap_or_default();
    let n = xs.len().min(ys.len());
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let mx = xs.iter().take(n).sum::<f64>() / n as f64;
    let my = ys.iter().take(n).sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 0..n {
        let dx = xs[i] - mx;
        num += dx * (ys[i] - my);
        den += dx * dx;
    }
    Ok(StrykeValue::float(if den > 0.0 { num / den } else { 0.0 }))
}

/// regr_intercept — y - slope · x.
fn builtin_regr_intercept(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let ys = args.get(1).map(b70_to_floats).unwrap_or_default();
    let n = xs.len().min(ys.len());
    if n == 0 { return Ok(StrykeValue::float(0.0)); }
    let mx = xs.iter().take(n).sum::<f64>() / n as f64;
    let my = ys.iter().take(n).sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut den = 0.0;
    for i in 0..n {
        let dx = xs[i] - mx;
        num += dx * (ys[i] - my);
        den += dx * dx;
    }
    let slope = if den > 0.0 { num / den } else { 0.0 };
    Ok(StrykeValue::float(my - slope * mx))
}

/// regr_r2 — coefficient of determination.
fn builtin_regr_r2(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let ys = args.get(1).map(b70_to_floats).unwrap_or_default();
    let n = xs.len().min(ys.len());
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let mx = xs.iter().take(n).sum::<f64>() / n as f64;
    let my = ys.iter().take(n).sum::<f64>() / n as f64;
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    let mut syy = 0.0;
    for i in 0..n {
        let dx = xs[i] - mx;
        let dy = ys[i] - my;
        sxy += dx * dy;
        sxx += dx * dx;
        syy += dy * dy;
    }
    let denom = sxx * syy;
    if denom <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    let r = sxy / denom.sqrt();
    Ok(StrykeValue::float(r * r))
}

/// percentile_cont — continuous percentile via linear interpolation.
fn builtin_percentile_cont(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut v = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let p = args.get(1).map(|x| x.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let h = p * (v.len() as f64 - 1.0);
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    let frac = h - lo as f64;
    Ok(StrykeValue::float(v[lo] + frac * (v[hi] - v[lo])))
}

/// percentile_disc — first value where rank ≥ p.
fn builtin_percentile_disc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut v = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let p = args.get(1).map(|x| x.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((p * v.len() as f64).ceil() as usize).saturating_sub(1).min(v.len() - 1);
    Ok(StrykeValue::float(v[idx]))
}

/// mode_agg — most frequent value (first wins on tie).
fn builtin_mode_agg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let mut counts = std::collections::HashMap::<u64, (f64, usize)>::new();
    let mut best = (v[0], 0_usize);
    for &x in &v {
        let key = x.to_bits();
        let entry = counts.entry(key).or_insert((x, 0));
        entry.1 += 1;
        if entry.1 > best.1 { best = (x, entry.1); }
    }
    Ok(StrykeValue::float(best.0))
}

/// array_to_string — total length of "x1 sep x2 sep x3" given lengths array.
fn builtin_array_to_string(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_string_agg(args)
}

/// array_position — 1-based index of v in array (0 if not found, Postgres style).
fn builtin_array_position(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let needle = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    for (i, &x) in v.iter().enumerate() {
        if x == needle { return Ok(StrykeValue::integer((i + 1) as i64)); }
    }
    Ok(StrykeValue::integer(0))
}

/// array_positions — count of occurrences (caller can derive list).
fn builtin_array_positions(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let needle = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(v.iter().filter(|&&x| x == needle).count() as i64))
}

/// array_remove — count after removing all occurrences of v.
fn builtin_array_remove(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let needle = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(v.iter().filter(|&&x| x != needle).count() as i64))
}

/// array_replace — count of replacements made.
fn builtin_array_replace(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let needle = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(v.iter().filter(|&&x| x == needle).count() as i64))
}

/// xmlforest — count of well-formed (name, value) pairs emitted.
fn builtin_xmlforest(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer((v.len() / 2) as i64))
}

/// xmlagg — Σ child-node counts.
fn builtin_xmlagg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b70_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.iter().sum::<f64>() as i64))
}
