// machine learning: trees, ensembles, activations, losses, metrics.

// Gini impurity
fn builtin_gini_impurity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let probs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let total: f64 = probs.iter().sum();
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let g: f64 = 1.0 - probs.iter().map(|&p| (p / total).powi(2)).sum::<f64>();
    Ok(StrykeValue::float(g))
}

// Entropy (Shannon, in bits)
fn builtin_entropy_bits(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let probs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let total: f64 = probs.iter().sum();
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let h: f64 = probs.iter().filter(|&&p| p > 0.0)
        .map(|&p| { let q = p / total; -q * q.log2() }).sum();
    Ok(StrykeValue::float(h))
}

// Information gain split (parent entropy minus weighted child entropies)
fn builtin_information_gain(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let parent: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let left: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let right: Vec<f64> = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    fn ent(p: &[f64]) -> f64 {
        let total: f64 = p.iter().sum();
        if total == 0.0 { return 0.0; }
        p.iter().filter(|&&x| x > 0.0).map(|&x| { let q = x / total; -q * q.log2() }).sum()
    }
    let total: f64 = parent.iter().sum();
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let l_t: f64 = left.iter().sum();
    let r_t: f64 = right.iter().sum();
    Ok(StrykeValue::float(ent(&parent) - (l_t / total) * ent(&left) - (r_t / total) * ent(&right)))
}

// Gain ratio
fn builtin_gain_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ig_v = builtin_information_gain(args)?;
    let ig = ig_v.to_number();
    let l_t: f64 = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).sum();
    let r_t: f64 = arg_to_vec(&args.get(2).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).sum();
    let total = l_t + r_t;
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let pl = l_t / total;
    let pr = r_t / total;
    let split_info = if pl > 0.0 && pr > 0.0 {
        -pl * pl.log2() - pr * pr.log2()
    } else { 0.0 };
    if split_info == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(ig / split_info))
}

// Naive Bayes Gaussian likelihood (single feature)
fn builtin_nb_gaussian_likelihood(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    let pi = std::f64::consts::PI;
    Ok(StrykeValue::float((1.0 / (2.0 * pi * sigma2).sqrt()) * (-((x - mu).powi(2)) / (2.0 * sigma2)).exp()))
}

// Bernoulli NB likelihood
fn builtin_nb_bernoulli_likelihood(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.5).clamp(1e-12, 1.0 - 1e-12);
    Ok(StrykeValue::float(p.powf(x) * (1.0 - p).powf(1.0 - x)))
}

// Multinomial NB log-likelihood (smoothed)
fn builtin_nb_multinomial_log_likelihood(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let counts: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let probs: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let mut sum = 0.0;
    for i in 0..counts.len().min(probs.len()) {
        sum += counts[i] * (probs[i] + alpha).ln();
    }
    Ok(StrykeValue::float(sum))
}

// AdaBoost weight update
fn builtin_adaboost_alpha(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let err = f1(args).clamp(1e-12, 1.0 - 1e-12);
    Ok(StrykeValue::float(0.5 * ((1.0 - err) / err).ln()))
}

// Soft-margin SVM hinge loss
fn builtin_hinge_loss(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let pred = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((1.0 - y * pred).max(0.0)))
}

// Squared hinge loss
fn builtin_squared_hinge(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let pred = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let h = (1.0 - y * pred).max(0.0);
    Ok(StrykeValue::float(h * h))
}

// Logistic loss (binary cross-entropy from logit)
fn builtin_logistic_loss(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let logit = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let p = 1.0 / (1.0 + (-logit).exp());
    let p = p.clamp(1e-12, 1.0 - 1e-12);
    Ok(StrykeValue::float(-(y * p.ln() + (1.0 - y) * (1.0 - p).ln())))
}

// Cross-entropy (multi-class)

// KL divergence
#[allow(dead_code)]
fn builtin_kl_div(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let q: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let mut s = 0.0;
    for i in 0..p.len().min(q.len()) {
        if p[i] > 0.0 && q[i] > 0.0 { s += p[i] * (p[i] / q[i]).ln(); }
    }
    Ok(StrykeValue::float(s))
}

// JS divergence
#[allow(dead_code)]
fn builtin_js_div(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let q: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = p.len().min(q.len());
    let m: Vec<f64> = (0..n).map(|i| 0.5 * (p[i] + q[i])).collect();
    let mut kl_pm = 0.0;
    let mut kl_qm = 0.0;
    for i in 0..n {
        if p[i] > 0.0 && m[i] > 0.0 { kl_pm += p[i] * (p[i] / m[i]).ln(); }
        if q[i] > 0.0 && m[i] > 0.0 { kl_qm += q[i] * (q[i] / m[i]).ln(); }
    }
    Ok(StrykeValue::float(0.5 * kl_pm + 0.5 * kl_qm))
}

// Wasserstein distance 1D (sorted)

// Sigmoid
// Sigmoid derivative
fn builtin_sigmoid_grad(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let s = 1.0 / (1.0 + (-x).exp());
    Ok(StrykeValue::float(s * (1.0 - s)))
}
// tanh derivative
fn builtin_tanh_grad(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(1.0 - x.tanh().powi(2)))
}
// ReLU
// ReLU derivative
fn builtin_relu_grad(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(if x > 0.0 { 1.0 } else { 0.0 }))
}
// Leaky ReLU
// ELU
// SELU
// Swish (SiLU)
#[allow(dead_code)]
fn builtin_swish(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(x / (1.0 + (-x).exp())))
}
// GELU
// Mish
// Softsign
fn builtin_softsign(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(x / (1.0 + x.abs())))
}
// Hardswish
#[allow(dead_code)]
fn builtin_hardswish(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let s = ((x + 3.0) / 6.0).clamp(0.0, 1.0);
    Ok(StrykeValue::float(x * s))
}
// PReLU
fn builtin_prelu(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.25);
    Ok(StrykeValue::float(if x > 0.0 { x } else { alpha * x }))
}
// Threshold
fn builtin_threshold_act(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let thresh = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let value = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(if x > thresh { x } else { value }))
}

// Confusion matrix counts (returns [TP, FP, TN, FN])
fn builtin_confusion_counts(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y_true: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number() as i64).collect();
    let y_pred: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number() as i64).collect();
    let mut tp = 0_i64; let mut fp = 0_i64; let mut tn = 0_i64; let mut fn_ = 0_i64;
    for i in 0..y_true.len().min(y_pred.len()) {
        match (y_true[i], y_pred[i]) {
            (1, 1) => tp += 1,
            (0, 1) => fp += 1,
            (0, 0) => tn += 1,
            (1, 0) => fn_ += 1,
            _ => {},
        }
    }
    Ok(StrykeValue::array(vec![
        StrykeValue::integer(tp), StrykeValue::integer(fp),
        StrykeValue::integer(tn), StrykeValue::integer(fn_),
    ]))
}
// MCC (Matthews correlation coefficient)
fn builtin_mcc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cv = builtin_confusion_counts(args)?;
    let v = arg_to_vec(&cv);
    let tp = v[0].to_number();
    let fp = v[1].to_number();
    let tn = v[2].to_number();
    let fn_ = v[3].to_number();
    let num = tp * tn - fp * fn_;
    let den = ((tp + fp) * (tp + fn_) * (tn + fp) * (tn + fn_)).sqrt();
    if den == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(num / den))
}
// F-beta
fn builtin_f_beta(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prec = f1(args);
    let rec = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let denom = beta * beta * prec + rec;
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((1.0 + beta * beta) * prec * rec / denom))
}
// Specificity
fn builtin_specificity(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cv = builtin_confusion_counts(args)?;
    let v = arg_to_vec(&cv);
    let tn = v[2].to_number();
    let fp = v[1].to_number();
    if tn + fp == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(tn / (tn + fp)))
}
// NPV (negative predictive value)
// Balanced accuracy
fn builtin_balanced_accuracy(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cv = builtin_confusion_counts(args)?;
    let v = arg_to_vec(&cv);
    let tp = v[0].to_number();
    let fp = v[1].to_number();
    let tn = v[2].to_number();
    let fn_ = v[3].to_number();
    let sens = if tp + fn_ > 0.0 { tp / (tp + fn_) } else { 0.0 };
    let spec = if tn + fp > 0.0 { tn / (tn + fp) } else { 0.0 };
    Ok(StrykeValue::float(0.5 * (sens + spec)))
}

// Cohen's kappa
fn builtin_cohen_kappa(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cv = builtin_confusion_counts(args)?;
    let v = arg_to_vec(&cv);
    let tp = v[0].to_number();
    let fp = v[1].to_number();
    let tn = v[2].to_number();
    let fn_ = v[3].to_number();
    let total = tp + fp + tn + fn_;
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let p_o = (tp + tn) / total;
    let p_yes = ((tp + fn_) / total) * ((tp + fp) / total);
    let p_no = ((tn + fp) / total) * ((tn + fn_) / total);
    let p_e = p_yes + p_no;
    if 1.0 - p_e == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((p_o - p_e) / (1.0 - p_e)))
}

// Brier score
fn builtin_brier_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y_true: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let y_prob: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = y_true.len().min(y_prob.len());
    if n == 0 { return Ok(StrykeValue::float(0.0)); }
    let s: f64 = (0..n).map(|i| (y_prob[i] - y_true[i]).powi(2)).sum();
    Ok(StrykeValue::float(s / n as f64))
}

// LogLoss
fn builtin_log_loss(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y_true: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let y_prob: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = y_true.len().min(y_prob.len());
    if n == 0 { return Ok(StrykeValue::float(0.0)); }
    let s: f64 = (0..n).map(|i| {
        let p = y_prob[i].clamp(1e-12, 1.0 - 1e-12);
        -(y_true[i] * p.ln() + (1.0 - y_true[i]) * (1.0 - p).ln())
    }).sum();
    Ok(StrykeValue::float(s / n as f64))
}

// Tversky index (asymmetric similarity)
fn builtin_tversky(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let tp = f1(args);
    let fp = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let fn_ = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    let beta = args.get(4).map(|v| v.to_number()).unwrap_or(0.5);
    let denom = tp + alpha * fp + beta * fn_;
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(tp / denom))
}

// Mahalanobis distance squared (1D simplified)
fn builtin_mahalanobis_1d(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let sigma2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float((x - mu).powi(2) / sigma2))
}

// Soft-max normalization

// Log-softmax
#[allow(dead_code)]
fn builtin_log_softmax(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    if xs.is_empty() { return Ok(StrykeValue::array(vec![])); }
    let m = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let log_sum: f64 = m + xs.iter().map(|&x| (x - m).exp()).sum::<f64>().ln();
    let out: Vec<StrykeValue> = xs.into_iter().map(|x| StrykeValue::float(x - log_sum)).collect();
    Ok(StrykeValue::array(out))
}

// One-hot encode
fn builtin_one_hot(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let idx = i1(args) as usize;
    let n = args.get(1).map(|v| v.to_number() as usize).unwrap_or(2);
    let out: Vec<StrykeValue> = (0..n).map(|i| StrykeValue::float(if i == idx { 1.0 } else { 0.0 })).collect();
    Ok(StrykeValue::array(out))
}

// Argmax

// Top-k indices
fn builtin_topk_indices(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    let mut indexed: Vec<(usize, f64)> = xs.iter().enumerate().map(|(i, &v)| (i, v)).collect();
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let out: Vec<StrykeValue> = indexed.into_iter().take(k).map(|(i, _)| StrykeValue::integer(i as i64)).collect();
    Ok(StrykeValue::array(out))
}

// Min-max scale
fn builtin_minmax_scale(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    if xs.is_empty() { return Ok(StrykeValue::array(vec![])); }
    let mn = xs.iter().cloned().fold(f64::INFINITY, f64::min);
    let mx = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = mx - mn;
    if range == 0.0 { return Ok(StrykeValue::array(xs.iter().map(|_| StrykeValue::float(0.5)).collect())); }
    let out: Vec<StrykeValue> = xs.into_iter().map(|x| StrykeValue::float((x - mn) / range)).collect();
    Ok(StrykeValue::array(out))
}

// Z-score normalize
fn builtin_zscore_norm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    if xs.is_empty() { return Ok(StrykeValue::array(vec![])); }
    let n = xs.len() as f64;
    let mean: f64 = xs.iter().sum::<f64>() / n;
    let var: f64 = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let sd = var.sqrt().max(1e-12);
    let out: Vec<StrykeValue> = xs.into_iter().map(|x| StrykeValue::float((x - mean) / sd)).collect();
    Ok(StrykeValue::array(out))
}

// Robust scale (subtract median, divide by IQR)
fn builtin_robust_scale(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut xs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    if xs.is_empty() { return Ok(StrykeValue::array(vec![])); }
    let mut sorted = xs.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    let median = sorted[n / 2];
    let q1 = sorted[n / 4];
    let q3 = sorted[3 * n / 4];
    let iqr = (q3 - q1).max(1e-12);
    for x in &mut xs { *x = (*x - median) / iqr; }
    Ok(StrykeValue::array(xs.into_iter().map(StrykeValue::float).collect()))
}
