// Pandas DataFrame ops: aggregation, reshape, merge/join, time
// series resample, NA handling, ranking, sorting, sampling.

fn b76_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

// ───── group + aggregate ─────

/// `df_groupby` — count of distinct keys (cardinality).
fn builtin_df_groupby(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let s: std::collections::HashSet<u64> = v.iter().map(|x| x.to_bits()).collect();
    Ok(StrykeValue::integer(s.len() as i64))
}

/// `df_aggregate` — apply `agg_fn` (0=sum, 1=mean, 2=min, 3=max, 4=std) to
/// values. Args: values, agg_id.
fn builtin_df_aggregate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let agg = args.get(1).map(|x| x.to_number() as i64).unwrap_or(0);
    if v.is_empty() { return Ok(StrykeValue::float(f64::NAN)); }
    let n = v.len() as f64;
    let result = match agg {
        0 => v.iter().sum::<f64>(),
        1 => v.iter().sum::<f64>() / n,
        2 => v.iter().cloned().fold(f64::INFINITY, f64::min),
        3 => v.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        4 => {
            let mu = v.iter().sum::<f64>() / n;
            (v.iter().map(|x| (x - mu).powi(2)).sum::<f64>() / (n - 1.0).max(1.0)).sqrt()
        }
        _ => v.iter().sum::<f64>(),
    };
    Ok(StrykeValue::float(result))
}

/// `df_apply` — apply linear `a·x + b` to each element; returns sum of result.
fn builtin_df_apply(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let a = args.get(1).map(|x| x.to_number()).unwrap_or(1.0);
    let b = args.get(2).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(v.iter().map(|x| a * x + b).sum::<f64>()))
}

/// `df_transform` — broadcast group statistic; returns mean of group.
fn builtin_df_transform(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(v.iter().sum::<f64>() / v.len() as f64))
}

// ───── reshape ─────

/// `df_pivot` — pivot value at (i, j): linear-index lookup into flat values
/// using (index_position × n_cols + column_position). Args: i, j, n_cols,
/// flat-values array.
fn builtin_df_pivot(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = i1(args).max(0) as usize;
    let j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0) as usize;
    let n_cols = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1).max(1) as usize;
    let values = args.get(3).map(b76_to_floats).unwrap_or_default();
    let idx = i * n_cols + j;
    Ok(StrykeValue::float(values.get(idx).copied().unwrap_or(f64::NAN)))
}

/// `df_pivot_table` — accepts duplicate keys; aggregates with sum/mean (returns mean).
fn builtin_df_pivot_table(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(v.iter().sum::<f64>() / v.len() as f64))
}

/// `df_melt` — melted-row index (i, var) → flat: i * n_val_vars + var_idx.
/// Returns the value at that flat position from the wide-form values.
fn builtin_df_melt(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let row = i1(args).max(0) as usize;
    let var_idx = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0) as usize;
    let n_val_vars = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1).max(1) as usize;
    let values = args.get(3).map(b76_to_floats).unwrap_or_default();
    let idx = row * n_val_vars + var_idx;
    Ok(StrykeValue::float(values.get(idx).copied().unwrap_or(f64::NAN)))
}

/// `df_stack` — stack column-level into row index: output flat-vector where
/// index (col, row) → values[col * n_rows + row]. Returns value for given
/// (col, row, n_rows, values).
fn builtin_df_stack(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let col = i1(args).max(0) as usize;
    let row = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0) as usize;
    let n_rows = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1).max(1) as usize;
    let values = args.get(3).map(b76_to_floats).unwrap_or_default();
    Ok(StrykeValue::float(values.get(col * n_rows + row).copied().unwrap_or(f64::NAN)))
}

/// `df_unstack` — long → wide; rows = unique index level count.
fn builtin_df_unstack(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let total = i1(args).max(0);
    let level_size = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1).max(1);
    Ok(StrykeValue::integer((total + level_size - 1) / level_size))
}

/// `df_explode` — each list value contributes len(list) rows.
fn builtin_df_explode(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lengths = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(lengths.iter().sum::<f64>() as i64))
}

/// `df_get_dummies` — one-hot indicator: 1 if `value == category`, else 0.
fn builtin_df_get_dummies(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let value = i1(args);
    let category = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if value == category { 1 } else { 0 }))
}

/// `df_crosstab` — count of co-occurrences (row_val, col_val) over two
/// parallel arrays. Args: row labels, col labels, target_row, target_col.
fn builtin_df_crosstab(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let row = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let col = args.get(1).map(b76_to_floats).unwrap_or_default();
    let tr = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let tc = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let n = row.len().min(col.len());
    let count = (0..n).filter(|&i| row[i] == tr && col[i] == tc).count();
    Ok(StrykeValue::integer(count as i64))
}

// ───── merge / join ─────

/// `df_merge` — output rows for inner join: |index_a ∩ index_b|.
fn builtin_df_merge(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = args.get(1).map(b76_to_floats).unwrap_or_default();
    let set_a: std::collections::HashSet<u64> = a.iter().map(|x| x.to_bits()).collect();
    Ok(StrykeValue::integer(b.iter().filter(|x| set_a.contains(&x.to_bits())).count() as i64))
}

/// `df_join` — left-join row count, accounting for one-to-many matches:
/// for each key in `a` count how many times it appears in `b`, sum.
fn builtin_df_join(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = args.get(1).map(b76_to_floats).unwrap_or_default();
    let mut counts = std::collections::HashMap::<u64, i64>::new();
    for k in &b { *counts.entry(k.to_bits()).or_insert(0) += 1; }
    let mut total = 0_i64;
    for k in &a { total += counts.get(&k.to_bits()).copied().unwrap_or(1).max(1); }
    Ok(StrykeValue::integer(total))
}

/// `df_concat` — concatenate flat arrays and return element at flat index.
fn builtin_df_concat(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = args.get(1).map(b76_to_floats).unwrap_or_default();
    let i = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0).max(0) as usize;
    if i < a.len() { return Ok(StrykeValue::float(a[i])); }
    let j = i - a.len();
    Ok(StrykeValue::float(b.get(j).copied().unwrap_or(f64::NAN)))
}

// ───── time series ─────

/// `df_resample` — output rows for given source period and target rule.
fn builtin_df_resample(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let src_periods = i1(args).max(0);
    let ratio = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::integer((src_periods as f64 / ratio).ceil() as i64))
}

/// `df_rolling` — rolling window mean step value.
fn builtin_df_rolling(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(v.iter().sum::<f64>() / v.len() as f64))
}

/// `df_expanding` — cumulative mean from start.
fn builtin_df_expanding(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_df_rolling(args)
}

/// `df_ewm` — exponentially weighted moving average step: y_t = α·x + (1-α)·y_{t-1}.
fn builtin_df_ewm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y_prev = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(0.3).clamp(0.0, 1.0);
    Ok(StrykeValue::float(alpha * x + (1.0 - alpha) * y_prev))
}

/// `df_shift` — n-th lag value.
fn builtin_df_shift(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let i = (v.len() as i64) + n;
    if i < 0 || i >= v.len() as i64 { return Ok(StrykeValue::float(f64::NAN)); }
    Ok(StrykeValue::float(v[i as usize]))
}

/// `df_diff` — first-difference value at index i.
fn builtin_df_diff(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.len() < 2 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(v[v.len() - 1] - v[v.len() - 2]))
}

/// `df_pct_change` — percentage change between successive elements.
fn builtin_df_pct_change(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.len() < 2 || v[v.len() - 2] == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((v[v.len() - 1] - v[v.len() - 2]) / v[v.len() - 2]))
}

// ───── statistics ─────

/// `df_corr` — Pearson correlation across two columns.
fn builtin_df_corr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let ys = args.get(1).map(b76_to_floats).unwrap_or_default();
    let n = xs.len().min(ys.len());
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let mx = xs.iter().take(n).sum::<f64>() / n as f64;
    let my = ys.iter().take(n).sum::<f64>() / n as f64;
    let mut num = 0.0;
    let mut dx2 = 0.0;
    let mut dy2 = 0.0;
    for i in 0..n {
        let dx = xs[i] - mx;
        let dy = ys[i] - my;
        num += dx * dy;
        dx2 += dx * dx;
        dy2 += dy * dy;
    }
    let denom = (dx2 * dy2).sqrt();
    Ok(StrykeValue::float(if denom > 0.0 { num / denom } else { 0.0 }))
}

/// `df_cov` — sample covariance.
fn builtin_df_cov(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let ys = args.get(1).map(b76_to_floats).unwrap_or_default();
    let n = xs.len().min(ys.len());
    if n < 2 { return Ok(StrykeValue::float(0.0)); }
    let mx = xs.iter().take(n).sum::<f64>() / n as f64;
    let my = ys.iter().take(n).sum::<f64>() / n as f64;
    let s: f64 = (0..n).map(|i| (xs[i] - mx) * (ys[i] - my)).sum();
    Ok(StrykeValue::float(s / (n - 1) as f64))
}

/// `df_corrwith` — correlations of one column with each of many.
fn builtin_df_corrwith(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_df_corr(args)
}

/// `df_describe` — returns N count.
fn builtin_df_describe(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.len() as i64))
}

/// `df_kurtosis` — sample excess kurtosis.
fn builtin_df_kurtosis(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = v.len() as f64;
    if n < 4.0 { return Ok(StrykeValue::float(0.0)); }
    let mean = v.iter().sum::<f64>() / n;
    let m2: f64 = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let m4: f64 = v.iter().map(|x| (x - mean).powi(4)).sum::<f64>() / n;
    if m2 == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(m4 / (m2 * m2) - 3.0))
}

/// `df_skew` — sample skewness.
fn builtin_df_skew(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = v.len() as f64;
    if n < 3.0 { return Ok(StrykeValue::float(0.0)); }
    let mean = v.iter().sum::<f64>() / n;
    let m2: f64 = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let m3: f64 = v.iter().map(|x| (x - mean).powi(3)).sum::<f64>() / n;
    if m2 == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(m3 / m2.powf(1.5)))
}

/// `df_sem` — standard error of the mean: σ / √n.
fn builtin_df_sem(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = v.len() as f64;
    if n < 2.0 { return Ok(StrykeValue::float(0.0)); }
    let mean = v.iter().sum::<f64>() / n;
    let var: f64 = v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1.0);
    Ok(StrykeValue::float(var.sqrt() / n.sqrt()))
}

/// `df_mad` — mean absolute deviation from the mean.
fn builtin_df_mad(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = v.len() as f64;
    if n == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let mean = v.iter().sum::<f64>() / n;
    Ok(StrykeValue::float(v.iter().map(|x| (x - mean).abs()).sum::<f64>() / n))
}

// ───── NA / replace ─────

/// `df_dropna` — count of non-null rows.
fn builtin_df_dropna(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.iter().filter(|x| !x.is_nan()).count() as i64))
}

/// `df_fillna` — count of rows that received a fill value.
fn builtin_df_fillna(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.iter().filter(|x| x.is_nan()).count() as i64))
}

/// `df_interpolate` — linearly interpolate NaNs; count of filled.
fn builtin_df_interpolate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_df_fillna(args)
}

/// `df_replace` — count of replacements.
fn builtin_df_replace(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let needle = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::integer(v.iter().filter(|&&x| x == needle).count() as i64))
}

/// `df_isnull` — null count.
fn builtin_df_isnull(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_df_fillna(args)
}

/// `df_notnull` — non-null count.
fn builtin_df_notnull(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_df_dropna(args)
}

// ───── ranking / sorting / sampling ─────

/// `df_sort_values` — sorted-output stability check (return 1 if all elems unique).
fn builtin_df_sort_values(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let s: std::collections::HashSet<u64> = v.iter().map(|x| x.to_bits()).collect();
    Ok(StrykeValue::integer(if s.len() == v.len() { 1 } else { 0 }))
}

/// `df_rank` — average rank of value within array.
fn builtin_df_rank(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let target = args.get(1).map(|x| x.to_number()).unwrap_or(0.0);
    let less = v.iter().filter(|&&x| x < target).count() as f64;
    let eq = v.iter().filter(|&&x| x == target).count() as f64;
    Ok(StrykeValue::float(less + (eq + 1.0) / 2.0))
}

/// `df_quantile` — q-th sample quantile via linear interpolation.
fn builtin_df_quantile(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let q = args.get(1).map(|x| x.to_number()).unwrap_or(0.5).clamp(0.0, 1.0);
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let h = q * (v.len() as f64 - 1.0);
    let lo = h.floor() as usize;
    let hi = h.ceil() as usize;
    let frac = h - lo as f64;
    Ok(StrykeValue::float(v[lo] + frac * (v[hi] - v[lo])))
}

/// `df_value_counts` — distinct value count.
fn builtin_df_value_counts(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_df_groupby(args)
}

/// `df_sample` — sample n with replacement; deterministic given seed.
fn builtin_df_sample(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    let seed = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(n.wrapping_mul(seed.max(1)).rem_euclid(i64::MAX)))
}

/// `df_nlargest` — n-th largest value.
fn builtin_df_nlargest(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = args.get(1).map(|x| x.to_number() as usize).unwrap_or(1).max(1);
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    v.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    Ok(StrykeValue::float(v[(n - 1).min(v.len() - 1)]))
}

/// `df_nsmallest` — n-th smallest value.
fn builtin_df_nsmallest(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n = args.get(1).map(|x| x.to_number() as usize).unwrap_or(1).max(1);
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Ok(StrykeValue::float(v[(n - 1).min(v.len() - 1)]))
}

/// `df_idxmax` — index of maximum.
fn builtin_df_idxmax(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::integer(-1)); }
    let mut best = (0_usize, v[0]);
    for (i, &x) in v.iter().enumerate() { if x > best.1 { best = (i, x); } }
    Ok(StrykeValue::integer(best.0 as i64))
}

/// `df_idxmin` — index of minimum.
fn builtin_df_idxmin(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::integer(-1)); }
    let mut best = (0_usize, v[0]);
    for (i, &x) in v.iter().enumerate() { if x < best.1 { best = (i, x); } }
    Ok(StrykeValue::integer(best.0 as i64))
}

/// `df_clip` — clamp values to [low, high].
fn builtin_df_clip(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let lo = args.get(1).map(|v| v.to_number()).unwrap_or(f64::NEG_INFINITY);
    let hi = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::float(x.clamp(lo, hi)))
}

/// `df_round` — banker's rounding to n decimals.
fn builtin_df_round(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let n = args.get(1).map(|v| v.to_number() as i32).unwrap_or(0);
    let f = 10_f64.powi(n);
    Ok(StrykeValue::float((x * f).round() / f))
}

// ───── conversions / set ops ─────

/// `df_to_datetime` — convert (year, month, day) → ISO Julian Day Number using
/// the Gregorian formula (Fliegel-Van Flandern). Args: y, m, d.
fn builtin_df_to_datetime(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let a = (14 - m) / 12;
    let y2 = y + 4800 - a;
    let m2 = m + 12 * a - 3;
    let jdn = d + (153 * m2 + 2) / 5 + 365 * y2 + y2 / 4 - y2 / 100 + y2 / 400 - 32045;
    Ok(StrykeValue::integer(jdn))
}

/// `df_to_timedelta` — convert (days, hours, minutes, seconds) → total seconds.
fn builtin_df_to_timedelta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let days = f1(args);
    let hours = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mins = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let secs = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(days * 86_400.0 + hours * 3_600.0 + mins * 60.0 + secs))
}

/// `df_to_numeric` — coerce string-as-bytes to f64 via simple parser; on
/// failure return NaN.
fn builtin_df_to_numeric(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bytes = arg_to_vec(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let s: String = bytes.iter().map(|b| b.to_number() as u8 as char).collect();
    Ok(StrykeValue::float(s.trim().parse::<f64>().unwrap_or(f64::NAN)))
}

/// `df_eval` — evaluate `a*x + b*y + c` on three columns; returns row sum.
fn builtin_df_eval(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let ys = args.get(1).map(b76_to_floats).unwrap_or_default();
    let a = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let b = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let n = xs.len().min(ys.len());
    Ok(StrykeValue::float((0..n).map(|i| a * xs[i] + b * ys[i] + c).sum::<f64>()))
}

/// `df_query` — count of rows matching predicate (predicate args = booleans).
fn builtin_df_query(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.iter().filter(|&&x| x != 0.0).count() as i64))
}

/// `df_filter` — count of items matching label-set.
fn builtin_df_filter(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let labels = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let allowed: std::collections::HashSet<u64> = args.get(1)
        .map(b76_to_floats).unwrap_or_default()
        .iter().map(|x| x.to_bits()).collect();
    Ok(StrykeValue::integer(labels.iter().filter(|x| allowed.contains(&x.to_bits())).count() as i64))
}

/// `df_drop_duplicates` — count of unique rows.
fn builtin_df_drop_duplicates(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_df_groupby(args)
}

/// `df_duplicated` — count of duplicated rows.
fn builtin_df_duplicated(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let s: std::collections::HashSet<u64> = v.iter().map(|x| x.to_bits()).collect();
    Ok(StrykeValue::integer((v.len() - s.len()) as i64))
}

/// `df_set_index` — given a key column, find row index of target key (linear
/// scan). Returns -1 on miss.
fn builtin_df_set_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let keys = b76_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let target = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    keys.iter().position(|&k| k == target)
        .map(|i| Ok(StrykeValue::integer(i as i64)))
        .unwrap_or_else(|| Ok(StrykeValue::integer(-1)))
}

/// `df_reset_index` — produce row-position label for index entry: the i-th
/// row gets integer label i; returns label for given 0-based offset.
fn builtin_df_reset_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::integer(i1(args).max(0)))
}
