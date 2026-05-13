// sklearn ML primitives: classifiers, regressors, clusterers,
// dimensionality reduction, preprocessing, model selection, metrics.

fn b79_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// `sk_logistic_predict` — sigmoid 1 / (1 + e^{−z}).
fn builtin_sk_logistic_predict(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    Ok(StrykeValue::float(1.0 / (1.0 + (-z).exp())))
}

/// `sk_logistic_fit` — gradient descent step: w ← w − η ∇L.
fn builtin_sk_logistic_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let w = f1(args);
    let grad = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lr = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(StrykeValue::float(w - lr * grad))
}

/// `sk_random_forest_fit` — bootstrap sample size for tree training.
fn builtin_sk_random_forest_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    let bootstrap_frac = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).clamp(0.0, 1.0);
    Ok(StrykeValue::integer((n as f64 * bootstrap_frac) as i64))
}

/// `sk_gbt_fit` — gradient-boosted tree leaf update: γ_j = −Σ g / Σ h.
fn builtin_sk_gbt_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g_sum = f1(args);
    let h_sum = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float(-g_sum / h_sum))
}

/// `sk_xgb_fit` — XGBoost split gain: ½ [G_L²/(H_L+λ) + G_R²/(H_R+λ) − G²/(H+λ)] − γ.
fn builtin_sk_xgb_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g_l = f1(args);
    let h_l = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let g_r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let h_r = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let gamma = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let g = g_l + g_r;
    let h = h_l + h_r;
    Ok(StrykeValue::float(0.5 * (g_l * g_l / (h_l + lambda) + g_r * g_r / (h_r + lambda)
        - g * g / (h + lambda)) - gamma))
}

/// `sk_lightgbm_fit` — LightGBM GOSS gain: weights small-gradient samples by
/// (1 − a) / b factor, where a = top-gradient fraction, b = small-gradient
/// sample fraction. Args: G_L, G_R, H_L, H_R, λ, a, b.
fn builtin_sk_lightgbm_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g_l = f1(args);
    let g_r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h_l = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let h_r = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(4).map(|v| v.to_number()).unwrap_or(1.0);
    let a = args.get(5).map(|v| v.to_number()).unwrap_or(0.2).clamp(0.0, 1.0);
    let b = args.get(6).map(|v| v.to_number()).unwrap_or(0.1).clamp(1e-15, 1.0);
    let goss_amp = (1.0 - a) / b;
    let g_l_adj = g_l * goss_amp;
    let g_r_adj = g_r * goss_amp;
    Ok(StrykeValue::float(0.5 * (g_l_adj * g_l_adj / (h_l + lambda)
        + g_r_adj * g_r_adj / (h_r + lambda))))
}

/// `sk_svm_fit` — hinge loss: max(0, 1 − y(w·x + b)).
fn builtin_sk_svm_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let z = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((1.0 - y * z).max(0.0)))
}

/// `sk_kmeans_fit` — Lloyd update: assign x to nearest centroid (returns dist).
fn builtin_sk_kmeans_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dists = b79_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(dists.iter().cloned().fold(f64::INFINITY, f64::min)))
}

/// `sk_dbscan_fit` — DBSCAN ε-neighbourhood count vs MinPts.
fn builtin_sk_dbscan_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_neighbours = i1(args);
    let min_pts = args.get(1).map(|v| v.to_number() as i64).unwrap_or(5);
    Ok(StrykeValue::integer(if n_neighbours >= min_pts { 1 } else { 0 }))
}

/// `sk_agglomerative_fit` — average linkage distance: Σ d(a, b) / (|A| · |B|).
fn builtin_sk_agglomerative_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d_sum = f1(args);
    let n_a = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let n_b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(d_sum / (n_a * n_b)))
}

/// `sk_pca_fit` — first principal component variance share λ_1 / Σλ.
fn builtin_sk_pca_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lambdas = b79_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let total: f64 = lambdas.iter().sum::<f64>().max(1e-300);
    Ok(StrykeValue::float(lambdas.first().copied().unwrap_or(0.0) / total))
}

/// `sk_tsne_fit` — pairwise Q-distribution weight 1 / (1 + ||y_i − y_j||²).
fn builtin_sk_tsne_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dist_sq = f1(args).max(0.0);
    Ok(StrykeValue::float(1.0 / (1.0 + dist_sq)))
}

/// `sk_umap_fit` — UMAP edge weight = exp(−(d − ρ) / σ).
fn builtin_sk_umap_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d = f1(args);
    let rho = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float((-(d - rho).max(0.0) / sigma).exp()))
}

/// `sk_isolation_forest_fit` — average path length normalisation.
fn builtin_sk_isolation_forest_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args).max(2.0);
    let h = (n - 1.0).ln() + 0.5772156649;
    Ok(StrykeValue::float(2.0 * h - 2.0 * (n - 1.0) / n))
}

/// `sk_lof_fit` — Local Outlier Factor: lrd_avg / lrd_self.
fn builtin_sk_lof_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lrd_avg = f1(args);
    let lrd_self = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float(lrd_avg / lrd_self))
}

/// `sk_kfold_split` — fold size for K-fold cross-validation.
fn builtin_sk_kfold_split(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args).max(0);
    let k = args.get(1).map(|v| v.to_number() as i64).unwrap_or(5).max(1);
    Ok(StrykeValue::integer(n / k))
}

/// `sk_stratified_kfold` — class proportion in each fold.
fn builtin_sk_stratified_kfold(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_class = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(n_class / total))
}

/// `sk_cross_val_score` — mean of fold scores.
fn builtin_sk_cross_val_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let scores = b79_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if scores.is_empty() { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(scores.iter().sum::<f64>() / scores.len() as f64))
}

/// `sk_grid_search` — number of model fits = product of grid sizes × n_folds.
fn builtin_sk_grid_search(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sizes = b79_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let n_folds = args.get(1).map(|v| v.to_number()).unwrap_or(5.0);
    Ok(StrykeValue::float(sizes.iter().product::<f64>() * n_folds))
}

/// `sk_random_search` — n_iter random samples × n_folds.
fn builtin_sk_random_search(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_iter = f1(args);
    let n_folds = args.get(1).map(|v| v.to_number()).unwrap_or(5.0);
    Ok(StrykeValue::float(n_iter * n_folds))
}

/// `sk_bayes_search` — acquisition function (expected improvement).
fn builtin_sk_bayes_search(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mu = f1(args);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let f_best = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let z = (mu - f_best) / sigma;
    Ok(StrykeValue::float(sigma * (z * 0.5 * (1.0 + libm::erf(z / 2_f64.sqrt()))
        + (-(z * z) / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt())))
}

/// `sk_pipeline_fit` — chained transformer count.
fn builtin_sk_pipeline_fit(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::integer(args.len() as i64))
}

/// `sk_standard_scaler` — z-score: (x − μ) / σ.
fn builtin_sk_standard_scaler(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float((x - mu) / sigma))
}

/// `sk_min_max_scaler` — (x − min) / (max − min).
fn builtin_sk_min_max_scaler(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let min = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let max = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let span = (max - min).max(1e-15);
    Ok(StrykeValue::float((x - min) / span))
}

/// `sk_robust_scaler` — (x − median) / IQR.
fn builtin_sk_robust_scaler(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let median = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let iqr = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float((x - median) / iqr))
}

/// `sk_quantile_transform` — empirical CDF rank / (n − 1).
fn builtin_sk_quantile_transform(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let rank = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(2.0);
    Ok(StrykeValue::float(rank / (n - 1.0)))
}

/// `sk_power_transform` — Box-Cox: (x^λ − 1) / λ for λ ≠ 0.
fn builtin_sk_power_transform(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args).max(1e-15);
    let lambda = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if lambda.abs() < 1e-15 { return Ok(StrykeValue::float(x.ln())); }
    Ok(StrykeValue::float((x.powf(lambda) - 1.0) / lambda))
}

/// `sk_one_hot` — boolean indicator value.
fn builtin_sk_one_hot(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cat = i1(args);
    let target = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if cat == target { 1 } else { 0 }))
}

/// `sk_ordinal_encode` — index of category in canonical order.
fn builtin_sk_ordinal_encode(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::integer(i1(args)))
}

/// `sk_label_encode` — assign new label = current count.
fn builtin_sk_label_encode(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::integer(i1(args)))
}

/// `sk_tfidf` — TF-IDF: tf · log(N / df).
fn builtin_sk_tfidf(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let tf = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let df = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    Ok(StrykeValue::float(tf * (n / df).ln()))
}

/// `sk_count_vectorize` — bag-of-words count.
fn builtin_sk_count_vectorize(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b79_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::integer(v.iter().filter(|&&x| x > 0.0).count() as i64))
}

/// `sk_silhouette` — silhouette score: (b − a) / max(a, b).
fn builtin_sk_silhouette(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let denom = a.max(b).max(1e-15);
    Ok(StrykeValue::float((b - a) / denom))
}

/// `sk_calinski_harabasz` — CH index = (B/(k-1)) / (W/(n-k)).
fn builtin_sk_calinski_harabasz(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let b = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(2.0).max(2.0);
    let n = args.get(3).map(|v| v.to_number()).unwrap_or(2.0).max(k + 1.0);
    Ok(StrykeValue::float((b / (k - 1.0)) / (w / (n - k))))
}

/// `sk_davies_bouldin` — DB index: average max similarity ratio.
fn builtin_sk_davies_bouldin(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s_i_plus_s_j = f1(args);
    let d_ij = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float(s_i_plus_s_j / d_ij))
}

/// `sk_adjusted_rand` — ARI = (RI − E[RI]) / (max(RI) − E[RI]).
fn builtin_sk_adjusted_rand(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ri = f1(args);
    let e_ri = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let max_ri = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(e_ri + 1e-15);
    Ok(StrykeValue::float((ri - e_ri) / (max_ri - e_ri)))
}

/// `sk_mutual_info` — MI: Σ p(x,y) log(p(x,y) / (p(x) p(y))).
fn builtin_sk_mutual_info(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_xy = f1(args).max(1e-300);
    let p_x = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    let p_y = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(StrykeValue::float(p_xy * (p_xy / (p_x * p_y)).ln()))
}

/// `sk_lda_topic` — LDA topic-word probability update step.
fn builtin_sk_lda_topic(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_kw = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    let n_k = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let v = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float((n_kw + beta) / (n_k + v * beta)))
}

/// `sk_nmf_topic` — non-negative matrix factorization update for W.
fn builtin_sk_nmf_topic(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let w = f1(args).max(1e-15);
    let num = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let den = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-15);
    Ok(StrykeValue::float(w * num / den))
}

/// `sk_word2vec_train` — skip-gram negative-sampling loss term.
fn builtin_sk_word2vec_train(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dot = f1(args);
    Ok(StrykeValue::float(-(1.0 / (1.0 + (-dot).exp())).ln()))
}

/// `sk_doc2vec_train` — Distributed Memory (PV-DM) paragraph-vector update:
/// concatenates paragraph_vec with context word vectors, predicts target;
/// loss = −log σ(target_dot) + Σ log σ(−neg_dot). Args: target_dot, neg_dots.
fn builtin_sk_doc2vec_train(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let target_dot = f1(args);
    let neg_dots = b79_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let pos_loss = -(1.0 / (1.0 + (-target_dot).exp())).ln();
    let neg_loss: f64 = neg_dots.iter()
        .map(|&d| (1.0 / (1.0 + d.exp())).ln())
        .sum();
    Ok(StrykeValue::float(pos_loss - neg_loss))
}

/// `sk_naive_bayes_predict` — log p(y) + Σ log p(x_i | y).
fn builtin_sk_naive_bayes_predict(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let log_pri = f1(args);
    let log_like_sum = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(log_pri + log_like_sum))
}

/// `sk_knn_predict` — majority vote over k nearest labels.
fn builtin_sk_knn_predict(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let labels = b79_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut counts = std::collections::HashMap::<u64, i64>::new();
    let mut best = (0_u64, 0_i64);
    for l in &labels {
        let c = counts.entry(l.to_bits()).or_insert(0);
        *c += 1;
        if *c > best.1 { best = (l.to_bits(), *c); }
    }
    Ok(StrykeValue::float(f64::from_bits(best.0)))
}

/// `sk_decision_tree_split` — Gini impurity: 1 − Σ p_i².
fn builtin_sk_decision_tree_split(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = b79_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(1.0 - p.iter().map(|x| x * x).sum::<f64>()))
}
