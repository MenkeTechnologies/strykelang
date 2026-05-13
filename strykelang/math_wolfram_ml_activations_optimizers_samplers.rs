// ML primitives: activations, losses, normalizations, optimizers, samplers.

fn b45_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// ReLU(x) = max(0, x)
fn builtin_ml_relu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(f1(args).max(0.0)))
}

/// Leaky ReLU(x) = x if x > 0 else α x
fn builtin_ml_leaky_relu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(StrykeValue::float(if x > 0.0 { x } else { alpha * x }))
}

/// ELU(x) = x if x > 0 else α(exp(x) - 1)
fn builtin_ml_elu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(if x > 0.0 { x } else { alpha * (x.exp() - 1.0) }))
}

/// SELU(x) = λ·ELU(x) with λ=1.0507, α=1.6733
fn builtin_ml_selu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let lambda = 1.050_700_987_355_480_5;
    let alpha = 1.673_263_242_354_377_4;
    Ok(StrykeValue::float(lambda * if x > 0.0 { x } else { alpha * (x.exp() - 1.0) }))
}

/// GELU(x) ≈ 0.5 x (1 + tanh(√(2/π)(x + 0.044715 x³)))
fn builtin_ml_gelu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let inner = (2.0 / std::f64::consts::PI).sqrt() * (x + 0.044715 * x.powi(3));
    Ok(StrykeValue::float(0.5 * x * (1.0 + inner.tanh())))
}

/// Swish(x) = x · σ(x)
fn builtin_ml_swish_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(x / (1.0 + (-x).exp())))
}

/// Mish(x) = x · tanh(softplus(x))
fn builtin_ml_mish_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(x * (1.0 + x.exp()).ln().tanh()))
}

/// Softplus(x) = ln(1 + e^x)
fn builtin_ml_softplus_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float((1.0 + x.exp()).ln()))
}

/// Softsign(x) = x / (1 + |x|)
fn builtin_ml_softsign_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(x / (1.0 + x.abs())))
}

/// Hard sigmoid: max(0, min(1, 0.2x + 0.5))
fn builtin_ml_hard_sigmoid(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float((0.2 * x + 0.5).clamp(0.0, 1.0)))
}

/// Hard tanh: clamp(x, -1, 1)
fn builtin_ml_hard_tanh(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(f1(args).clamp(-1.0, 1.0)))
}

/// PReLU(x) = x if x > 0 else a·x (parametric)
fn builtin_ml_prelu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_ml_leaky_relu_step(args)
}

/// CELU(x) = max(0, x) + min(0, α(exp(x/α) - 1))
fn builtin_ml_celu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if x >= 0.0 { Ok(StrykeValue::float(x)) } else { Ok(StrykeValue::float(alpha * ((x / alpha).exp() - 1.0))) }
}

/// SiLU = swish
fn builtin_ml_silu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_ml_swish_step(args)
}

/// LogSumExp(x) = a + ln Σ exp(xᵢ - a)
fn builtin_ml_logsumexp_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b45_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(f64::NEG_INFINITY)); }
    let m = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let s: f64 = v.iter().map(|x| (x - m).exp()).sum();
    Ok(StrykeValue::float(m + s.ln()))
}

/// log_softmax(x_i) = x_i - logsumexp(x)
fn builtin_ml_log_softmax_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b45_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let i = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    if v.is_empty() || i >= v.len() { return Ok(StrykeValue::float(0.0)); }
    let m = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let s: f64 = v.iter().map(|x| (x - m).exp()).sum();
    Ok(StrykeValue::float(v[i] - (m + s.ln())))
}

/// log σ(x) = -softplus(-x)
fn builtin_ml_log_sigmoid(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    Ok(StrykeValue::float(-((1.0 + (-x).exp()).ln())))
}

/// GLU(a, b) = a · σ(b)
fn builtin_ml_glu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(a / (1.0 + (-b).exp())))
}

/// GeGLU(a, b) = a · GELU(b)
fn builtin_ml_geglu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let inner = (2.0 / std::f64::consts::PI).sqrt() * (b + 0.044715 * b.powi(3));
    Ok(StrykeValue::float(a * 0.5 * b * (1.0 + inner.tanh())))
}

/// SwiGLU(a, b) = a · swish(b)
fn builtin_ml_swiglu_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(a * b / (1.0 + (-b).exp())))
}

/// Attention score q·k / √d
fn builtin_ml_attention_score_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q_dot_k = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if d <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(q_dot_k / d.sqrt()))
}

/// Scaled dot-product softmax(QK^T/√d)V
fn builtin_ml_scaled_dot_product(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q_k = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if d <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(q_k * v / d.sqrt()))
}

/// Multi-head average across H heads
fn builtin_ml_multihead_avg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b45_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(v.iter().sum::<f64>() / v.len() as f64))
}

/// Softmax with temperature: exp(x/T) / Σ exp
fn builtin_ml_softmax_temperature(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b45_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let i = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    if v.is_empty() || t == 0.0 || i >= v.len() { return Ok(StrykeValue::float(0.0)); }
    let m = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let denom: f64 = v.iter().map(|x| ((x - m) / t).exp()).sum();
    Ok(StrykeValue::float(((v[i] - m) / t).exp() / denom))
}

/// Dropout mask probability (return 0 with prob p)
fn builtin_ml_dropout_mask_prob(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(if r < p { 0.0 } else { 1.0 / (1.0 - p) }))
}

/// LayerNorm: (x - μ) / √(σ² + ε)
fn builtin_ml_layer_norm_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mean = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let var = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let eps = args.get(3).map(|v| v.to_number()).unwrap_or(1e-5);
    Ok(StrykeValue::float((x - mean) / (var + eps).sqrt()))
}

/// BatchNorm (Ioffe & Szegedy 2015): normalize across the batch dimension only,
/// keeping per-channel statistics. y = γ(x − μ_B)/√(σ²_B + ε) + β. Args: x, μ_B,
/// σ²_B, γ, β, ε. NOTE: μ_B/σ²_B come from the batch — different from LayerNorm
/// which uses per-sample mean/var across feature dims.
fn builtin_ml_batch_norm_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mu_b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let var_b = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let gamma = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let eps = args.get(5).map(|v| v.to_number()).unwrap_or(1e-5);
    Ok(StrykeValue::float(gamma * (x - mu_b) / (var_b + eps).sqrt() + beta))
}

/// GroupNorm (Wu & He 2018): split channels into G groups, normalize within
/// each group's spatial × channel-block. Computes group-mean μ_g and var σ²_g
/// over (C/G · H · W) elements. Args: x, μ_g, σ²_g, γ, β, eps.
fn builtin_ml_group_norm_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mu_g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let var_g = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let gamma = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let eps = args.get(5).map(|v| v.to_number()).unwrap_or(1e-5);
    Ok(StrykeValue::float(gamma * (x - mu_g) / (var_g + eps).sqrt() + beta))
}

/// RMSNorm: x / √(mean(x²) + ε)
fn builtin_ml_rms_norm_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mean_sq = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let eps = args.get(2).map(|v| v.to_number()).unwrap_or(1e-6);
    Ok(StrykeValue::float(x / (mean_sq + eps).sqrt()))
}

/// InstanceNorm (Ulyanov et al. 2016): normalize per-sample × per-channel over
/// spatial (H×W) only. Differs from BatchNorm (no batch axis) and LayerNorm
/// (no channel axis). Args: x, μ_HW, σ²_HW, γ, β, eps.
fn builtin_ml_instance_norm_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let var = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let gamma = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let eps = args.get(5).map(|v| v.to_number()).unwrap_or(1e-5);
    Ok(StrykeValue::float(gamma * (x - mu) / (var + eps).sqrt() + beta))
}

/// WeightNorm: w / |w|
fn builtin_ml_weight_norm_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let w = f1(args);
    let norm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if norm == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(w / norm))
}

/// SpectralNorm via power iteration
fn builtin_ml_spectral_norm_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let w = f1(args);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if sigma == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(w / sigma))
}

/// L2 normalize: x / √(Σx²)
fn builtin_ml_l2_normalize_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b45_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let i = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm == 0.0 || i >= v.len() { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(v[i] / norm))
}

/// Huber loss: ½ x² if |x| ≤ δ else δ(|x| - ½δ)
fn builtin_ml_huber_loss_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let delta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if x.abs() <= delta { Ok(StrykeValue::float(0.5 * x * x)) } else { Ok(StrykeValue::float(delta * (x.abs() - 0.5 * delta))) }
}

/// Smooth L1 loss (Girshick 2015): same shape as Huber but normalized by β so
/// the slope at large x is 1 (matching pure L1). For |x| < β: 0.5·x²/β; for
/// |x| ≥ β: |x| − 0.5·β. Differs from Huber by the 1/β scaling.
/// Args: x, β (default 1.0).
fn builtin_ml_smooth_l1_loss(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let beta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    if x.abs() < beta { Ok(StrykeValue::float(0.5 * x * x / beta)) }
    else { Ok(StrykeValue::float(x.abs() - 0.5 * beta)) }
}

/// Focal loss: -α(1-p)^γ log(p) for binary
fn builtin_ml_focal_loss_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.25);
    let gamma = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    if p <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-alpha * (1.0 - p).powf(gamma) * p.ln()))
}

/// Dice loss: 1 - 2|A∩B|/(|A|+|B|)
fn builtin_ml_dice_loss_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let intersection = f1(args);
    let sum = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if sum == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(1.0 - 2.0 * intersection / sum))
}

/// IoU loss: 1 - |A∩B|/|A∪B|
fn builtin_ml_iou_loss_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let inter = f1(args);
    let union = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if union == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(1.0 - inter / union))
}

/// Generalized IoU loss
fn builtin_ml_giou_loss_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let iou = f1(args);
    let enc_minus_union = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let enc = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if enc == 0.0 { return Ok(StrykeValue::float(1.0 - iou)); }
    Ok(StrykeValue::float(1.0 - iou + enc_minus_union / enc))
}

/// Distance IoU loss: 1 - IoU + ρ²/c²
fn builtin_ml_diou_loss_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let iou = f1(args);
    let rho2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if c2 == 0.0 { return Ok(StrykeValue::float(1.0 - iou)); }
    Ok(StrykeValue::float(1.0 - iou + rho2 / c2))
}

/// Complete IoU loss: DIoU + αv where v measures aspect ratio
fn builtin_ml_ciou_loss_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let diou = f1(args);
    let av = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(diou + av))
}

/// Contrastive loss: ½(y d² + (1-y)·max(0, m - d)²)
fn builtin_ml_contrastive_loss(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let term1 = y * d * d;
    let term2 = (1.0 - y) * (m - d).max(0.0).powi(2);
    Ok(StrykeValue::float(0.5 * (term1 + term2)))
}

/// Triplet loss: max(0, d(a, p) - d(a, n) + margin)
fn builtin_ml_triplet_loss_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let d_ap = f1(args);
    let d_an = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let margin = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float((d_ap - d_an + margin).max(0.0)))
}

/// ArcFace: cos(θ + m)
fn builtin_ml_arcface_loss_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cos_theta = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let theta = cos_theta.acos();
    Ok(StrykeValue::float((theta + m).cos()))
}

/// Center loss: ½|x - c|²
fn builtin_ml_center_loss_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(0.5 * (x - c).powi(2)))
}

/// KL divergence loss (mean reduction)
fn builtin_ml_kl_divergence_loss(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if p <= 0.0 || q <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(p * (p / q).ln()))
}

/// Cross entropy: -Σ y log p (single sample)
fn builtin_ml_cross_entropy_loss(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if p <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-y * p.ln()))
}

/// Binary cross entropy: -y log p - (1-y) log(1-p)
fn builtin_ml_binary_cross_entropy(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    if p <= 0.0 || p >= 1.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-(y * p.ln() + (1.0 - y) * (1.0 - p).ln())))
}

/// Label smoothing: y_smooth = y(1-ε) + ε/K
fn builtin_ml_label_smoothing(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let y = f1(args);
    let eps = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    if k == 0.0 { return Ok(StrykeValue::float(y)); }
    Ok(StrykeValue::float(y * (1.0 - eps) + eps / k))
}

/// Mixup λ from Beta distribution mean
fn builtin_ml_mixup_lambda(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let alpha = f1(args);
    if alpha <= 0.0 { return Ok(StrykeValue::float(0.5)); }
    Ok(StrykeValue::float(alpha / (alpha + alpha)))
}

/// CutMix box IoU
fn builtin_ml_cutmix_box_iou(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_ml_iou_loss_step(args)
}

/// Random erasing step (apply probability)
fn builtin_ml_random_erasing_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::integer(if r < p { 1 } else { 0 }))
}

/// Cosine LR schedule: η_t = η_min + ½(η_max - η_min)(1 + cos(πt/T))
fn builtin_ml_cosine_lr_schedule(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    let big_t = args.get(1).map(|v| v.to_number()).unwrap_or(1000.0);
    let eta_min = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let eta_max = args.get(3).map(|v| v.to_number()).unwrap_or(0.001);
    if big_t == 0.0 { return Ok(StrykeValue::float(eta_max)); }
    Ok(StrykeValue::float(eta_min + 0.5 * (eta_max - eta_min) * (1.0 + (std::f64::consts::PI * t / big_t).cos())))
}

/// Linear warmup LR step
fn builtin_ml_warmup_lr_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    let warmup = args.get(1).map(|v| v.to_number()).unwrap_or(1000.0);
    let lr_max = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    if warmup == 0.0 { return Ok(StrykeValue::float(lr_max)); }
    Ok(StrykeValue::float(lr_max * (t / warmup).min(1.0)))
}

/// Step LR schedule
fn builtin_ml_step_lr_schedule(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lr0 = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let n_steps = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(lr0 * gamma.powf(n_steps.floor())))
}

/// Exponential LR
fn builtin_ml_exponential_lr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lr0 = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.95);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(lr0 * gamma.powf(t)))
}

/// Polynomial LR: lr_t = (1 - t/T)^p · lr_0
fn builtin_ml_polynomial_lr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lr0 = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let big_t = args.get(2).map(|v| v.to_number()).unwrap_or(1000.0);
    let p = args.get(3).map(|v| v.to_number()).unwrap_or(2.0);
    if big_t == 0.0 { return Ok(StrykeValue::float(lr0)); }
    Ok(StrykeValue::float(lr0 * (1.0 - t / big_t).max(0.0).powf(p)))
}

/// One-cycle LR (triangle ramp)
fn builtin_ml_one_cycle_lr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    let half = args.get(1).map(|v| v.to_number()).unwrap_or(500.0);
    let lr_max = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    if half == 0.0 { return Ok(StrykeValue::float(lr_max)); }
    let phase = (t / half).min(2.0);
    if phase < 1.0 { Ok(StrykeValue::float(lr_max * phase)) } else { Ok(StrykeValue::float(lr_max * (2.0 - phase).max(0.0))) }
}

/// Inverse sqrt LR (used in transformer)
fn builtin_ml_inverse_sqrt_lr(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    if t <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(1.0 / t.sqrt()))
}

/// Smith's cyclic LR: triangular wave between lr_min and lr_max, periodic.
/// cycle = ⌊1 + t / (2·step_size)⌋; x = |t/step_size − 2·cycle + 1|;
/// lr = lr_min + (lr_max − lr_min)·max(0, 1 − x). Args: t, step_size, lr_min, lr_max.
fn builtin_ml_cyclic_lr_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t = f1(args);
    let step = args.get(1).map(|v| v.to_number()).unwrap_or(2000.0).max(1.0);
    let lr_min = args.get(2).map(|v| v.to_number()).unwrap_or(0.0001);
    let lr_max = args.get(3).map(|v| v.to_number()).unwrap_or(0.006);
    let cycle = (1.0 + t / (2.0 * step)).floor();
    let x = (t / step - 2.0 * cycle + 1.0).abs();
    Ok(StrykeValue::float(lr_min + (lr_max - lr_min) * (1.0 - x).max(0.0)))
}

/// SGD step: θ ← θ - η g
fn builtin_ml_sgd_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(theta - eta * g))
}

/// Momentum step: v ← μv + g; θ ← θ - η v
fn builtin_ml_momentum_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.9);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(mu * v + g))
}

/// Nesterov momentum step
fn builtin_ml_nesterov_momentum(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.9);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(mu * (mu * v + g) + g))
}

/// AdaGrad step
fn builtin_ml_adagrad_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let s = args.get(3).map(|v| v.to_number()).unwrap_or(1e-8);
    Ok(StrykeValue::float(theta - eta * g / (s + 1e-8).sqrt()))
}

/// RMSProp (Hinton 2012): exponentially-decayed running average of squared
/// gradients (NOT cumulative like AdaGrad):  v_t = ρ·v_{t-1} + (1-ρ)·g²;
/// θ ← θ − η · g / (√v_t + ε). Args: θ, η, g, prev_v, ρ, ε.
fn builtin_ml_rmsprop_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(1e-3);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let prev_v = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let rho = args.get(4).map(|v| v.to_number()).unwrap_or(0.9);
    let eps = args.get(5).map(|v| v.to_number()).unwrap_or(1e-8);
    let v = rho * prev_v + (1.0 - rho) * g * g;
    Ok(StrykeValue::float(theta - eta * g / (v.sqrt() + eps)))
}

/// Adam step
fn builtin_ml_adam_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m_hat = f1(args);
    let v_hat = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let eta = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    let eps = args.get(3).map(|v| v.to_number()).unwrap_or(1e-8);
    Ok(StrykeValue::float(eta * m_hat / (v_hat.sqrt() + eps)))
}

/// AdamW step
fn builtin_ml_adamw_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m_hat = f1(args);
    let v_hat = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let eta = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    let weight_decay = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let theta = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let eps = args.get(5).map(|v| v.to_number()).unwrap_or(1e-8);
    Ok(StrykeValue::float(eta * (m_hat / (v_hat.sqrt() + eps) + weight_decay * theta)))
}

/// Adamax step (∞-norm)
fn builtin_ml_adamax_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m_hat = f1(args);
    let u = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let eta = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    if u == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(eta * m_hat / u))
}

/// Nadam (Dozat 2016): Nesterov-momentum-aware Adam. Lookahead m̂' instead of m̂:
///   m̂' = β₁ · m̂_t + (1 − β₁) · g / (1 − β₁^t).
///   θ ← θ − η · m̂' / (√v̂ + ε).
/// Args: m_hat, v_hat, g, β₁, t (step), η, ε.
fn builtin_ml_nadam_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m_hat = f1(args);
    let v_hat = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let b1 = args.get(3).map(|v| v.to_number()).unwrap_or(0.9);
    let t = args.get(4).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let eta = args.get(5).map(|v| v.to_number()).unwrap_or(1e-3);
    let eps = args.get(6).map(|v| v.to_number()).unwrap_or(1e-8);
    let bias_correct = 1.0 - b1.powf(t);
    let m_lookahead = b1 * m_hat + (1.0 - b1) * g / bias_correct;
    Ok(StrykeValue::float(eta * m_lookahead / (v_hat.sqrt() + eps)))
}

/// RAdam (Liu et al. 2020): rectifies Adam's variance. ρ_∞ = 2/(1−β₂) − 1;
///   ρ_t = ρ_∞ − 2t·β₂^t / (1−β₂^t).
///   if ρ_t > 4: r_t = √((ρ_t − 4)(ρ_t − 2)ρ_∞ / ((ρ_∞ − 4)(ρ_∞ − 2)ρ_t));
///               step = η · r_t · m̂ / (√v̂ + ε).
///   else:       step = η · m̂  (fallback to SGD-with-momentum).
/// Args: m_hat, v_hat, β₂, t, η, ε.
fn builtin_ml_radam_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m_hat = f1(args);
    let v_hat = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let b2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.999);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let eta = args.get(4).map(|v| v.to_number()).unwrap_or(1e-3);
    let eps = args.get(5).map(|v| v.to_number()).unwrap_or(1e-8);
    let rho_inf = 2.0 / (1.0 - b2) - 1.0;
    let rho_t = rho_inf - 2.0 * t * b2.powf(t) / (1.0 - b2.powf(t));
    if rho_t > 4.0 {
        let r = (((rho_t - 4.0) * (rho_t - 2.0) * rho_inf)
            / ((rho_inf - 4.0) * (rho_inf - 2.0) * rho_t)).sqrt();
        Ok(StrykeValue::float(eta * r * m_hat / (v_hat.sqrt() + eps)))
    } else {
        Ok(StrykeValue::float(eta * m_hat))
    }
}

/// Lookahead step: θ_slow + α(θ_fast - θ_slow)
fn builtin_ml_lookahead_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let slow = f1(args);
    let fast = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(slow + alpha * (fast - slow)))
}

/// LAMB step
fn builtin_ml_lamb_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    let phi_w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let phi_g = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let eta = args.get(3).map(|v| v.to_number()).unwrap_or(0.001);
    if phi_g == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(eta * phi_w / phi_g * r))
}

/// LARS (You et al. 2017): per-layer learning-rate scaling proportional to the
/// ratio ‖w_l‖ / (‖∇w_l‖ + λ‖w_l‖). NO Adam-style m, v moments (that's LAMB).
///   η_l = η · trust · ‖w‖ / (‖g‖ + λ‖w‖);   w ← w − η_l · (g + λ·w).
/// Args: η, w_norm, g_norm, weight_decay λ, trust coefficient.
fn builtin_ml_lars_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let eta = f1(args);
    let w_norm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let g_norm = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let wd = args.get(3).map(|v| v.to_number()).unwrap_or(1e-4);
    let trust = args.get(4).map(|v| v.to_number()).unwrap_or(1e-3);
    let denom = g_norm + wd * w_norm;
    if denom.abs() < 1e-15 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(eta * trust * w_norm / denom))
}

/// Yogi step (adaptive method)
fn builtin_ml_yogi_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = f1(args);
    let g_sq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.999);
    Ok(StrykeValue::float(v - (1.0 - beta2) * (v - g_sq).signum() * g_sq))
}

/// AMSGrad step (max of past v_hat)
fn builtin_ml_amsgrad_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v_hat = f1(args);
    let v_hat_max = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(v_hat.max(v_hat_max)))
}

/// AdaBelief (Zhuang et al. 2020): replaces Adam's v_t = E[g²] with the variance
/// of g around the moving mean: s_t = β₂·s_{t-1} + (1−β₂)·(g − m_t)² + ε.
/// Update: θ ← θ − η · m̂ / (√ŝ + ε). Args: m_hat, g, m_running, prev_s, β₂, η, ε.
fn builtin_ml_adabelief_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m_hat = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m_running = args.get(2).map(|v| v.to_number()).unwrap_or(m_hat);
    let prev_s = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let b2 = args.get(4).map(|v| v.to_number()).unwrap_or(0.999);
    let eta = args.get(5).map(|v| v.to_number()).unwrap_or(1e-3);
    let eps = args.get(6).map(|v| v.to_number()).unwrap_or(1e-8);
    let s = b2 * prev_s + (1.0 - b2) * (g - m_running).powi(2) + eps;
    Ok(StrykeValue::float(eta * m_hat / (s.sqrt() + eps)))
}

/// Shampoo (Gupta-Koren-Singer 2018): full-matrix preconditioner.  For a
/// parameter matrix W ∈ ℝ^{m×n} with grad G, accumulate
///   L_t = L_{t-1} + G·Gᵀ,    R_t = R_{t-1} + Gᵀ·G,
/// then update W ← W − η · L_t^{−1/4} · G · R_t^{−1/4}.  Returns the scalar
/// preconditioner factor (l⁻¹ᐟ⁴ · r⁻¹ᐟ⁴) given two diagonal traces.
/// Args: g (scalar gradient), prev_l, prev_r, η.
fn builtin_ml_shampoo_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g = f1(args);
    let prev_l = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let prev_r = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let eta = args.get(3).map(|v| v.to_number()).unwrap_or(1e-3);
    let l = prev_l + g * g;
    let r = prev_r + g * g;
    let pre = (l.max(1e-12).powf(-0.25)) * (r.max(1e-12).powf(-0.25));
    Ok(StrykeValue::float(eta * pre * g))
}

/// Lion step: θ ← θ - η sign(c)
fn builtin_ml_lion_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0001);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(theta - eta * c.signum()))
}

/// Sophia-G (Liu et al. 2023): clipped Hessian-aware update.
///   m_t = β₁·m_{t-1} + (1−β₁)·g.
///   h_t = β₂·h_{t-1} + (1−β₂)·diag(H)   (Hessian diagonal estimate).
///   θ ← θ − η · clip(m_t / max(h_t, ε), −ρ, ρ).
/// Args: prev_m, g, prev_h, h_diag (estimate), β₁, β₂, η, ρ, ε.
fn builtin_ml_sophia_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prev_m = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let prev_h = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let h_diag = args.get(3).map(|v| v.to_number()).unwrap_or(1.0);
    let b1 = args.get(4).map(|v| v.to_number()).unwrap_or(0.965);
    let b2 = args.get(5).map(|v| v.to_number()).unwrap_or(0.99);
    let eta = args.get(6).map(|v| v.to_number()).unwrap_or(6e-4);
    let rho = args.get(7).map(|v| v.to_number()).unwrap_or(0.04);
    let eps = args.get(8).map(|v| v.to_number()).unwrap_or(1e-12);
    let m = b1 * prev_m + (1.0 - b1) * g;
    let h = b2 * prev_h + (1.0 - b2) * h_diag;
    let raw = m / h.abs().max(eps);
    Ok(StrykeValue::float(eta * raw.clamp(-rho, rho)))
}

/// Gradient clipping by norm: g · min(1, c/||g||)
fn builtin_ml_gradient_clip_norm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g = f1(args);
    let norm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if norm == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(g * (c / norm).min(1.0)))
}

/// Gradient clipping by value
fn builtin_ml_gradient_clip_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(g.clamp(-c, c)))
}

/// Gradient accumulation: gᵢ → gᵢ / k
fn builtin_ml_gradient_accumulate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if k == 0.0 { return Ok(StrykeValue::float(g)); }
    Ok(StrykeValue::float(g / k))
}

/// Gradient centralization: g - mean(g)
fn builtin_ml_gradient_centralize(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let g = f1(args);
    let mean = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(g - mean))
}

/// Weight decay: θ ← (1 - η·λ) θ
fn builtin_ml_weight_decay_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let theta = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(0.001);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(StrykeValue::float(theta * (1.0 - eta * lambda)))
}

/// He init: N(0, √(2/n))
fn builtin_ml_he_init_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_in = f1(args);
    if n_in <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((2.0 / n_in).sqrt()))
}

/// Xavier init: N(0, √(1/n))
fn builtin_ml_xavier_init_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_in = f1(args);
    if n_in <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((1.0 / n_in).sqrt()))
}

/// Glorot init: U(-√(6/(n_in+n_out)), √(6/(n_in+n_out)))
fn builtin_ml_glorot_init_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_in = f1(args);
    let n_out = args.get(1).map(|v| v.to_number()).unwrap_or(n_in);
    if n_in + n_out <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((6.0 / (n_in + n_out)).sqrt()))
}

/// Orthogonal init scaling: gain · √(2 / (1 + α²)) for tanh-style; default gain=1.
/// Returns the σ-scaling factor for an orthogonal weight matrix QR-decomposed from
/// a Gaussian draw. Args: gain, leaky_alpha.
fn builtin_ml_orthogonal_init(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let gain = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(gain * (2.0 / (1.0 + alpha * alpha)).sqrt()))
}

/// Truncated normal init: N(0, σ) clipped to [-2σ, 2σ]
fn builtin_ml_truncnormal_init(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let sigma = f1(args);
    Ok(StrykeValue::float(sigma * 0.886_226_925_452_758))
}

/// Kaiming init (alias of He)
fn builtin_ml_kaiming_init(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_ml_he_init_value(args)
}

/// LeCun init (LeCun et al. 1998): σ = √(1/n_in). DIFFERS from Xavier (Glorot)
/// which uses √(2/(n_in+n_out)). Args: n_in.
fn builtin_ml_lecun_init_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_in = f1(args).max(1.0);
    Ok(StrykeValue::float((1.0_f64 / n_in).sqrt()))
}

/// Zero init: returns 0 for any (i, j) tensor index. Defining property.
fn builtin_ml_zero_init(_args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(0.0))
}

/// Constant init: every cell = c (returns c). Defining property.
fn builtin_ml_constant_init(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    Ok(StrykeValue::float(f1(args)))
}

/// Uniform init: U(-r, r)
fn builtin_ml_uniform_init(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args);
    Ok(StrykeValue::float(r))
}

/// One-hot index from class id
fn builtin_ml_one_hot_index(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cls = i1(args);
    let i = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(StrykeValue::integer(if cls == i { 1 } else { 0 }))
}

/// Label-to-id via lookup in vocabulary array. Args: label_index, vocab array.
/// Returns offset of matching entry, -1 if missing.
fn builtin_ml_label_to_id(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let label = i1(args);
    let vocab = b45_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    for (i, v) in vocab.iter().enumerate() {
        if v.round() as i64 == label { return Ok(StrykeValue::integer(i as i64)); }
    }
    Ok(StrykeValue::integer(-1))
}

/// Id-to-label: bounds check + return id (the id IS the label after vocab lookup).
fn builtin_ml_id_to_label_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let id = i1(args);
    let vocab_size = args.get(1).map(|v| v.to_number() as i64).unwrap_or(i64::MAX);
    if id < 0 || id >= vocab_size { return Ok(StrykeValue::integer(-1)); }
    Ok(StrykeValue::integer(id))
}

/// Top-k token logit sum
fn builtin_ml_token_logit_top_k(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut v = b45_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    v.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    Ok(StrykeValue::float(v.iter().take(k).sum()))
}

/// Top-k argmax index
fn builtin_ml_topk_argmax(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b45_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut best = (0_usize, f64::NEG_INFINITY);
    for (i, &x) in v.iter().enumerate() {
        if x > best.1 { best = (i, x); }
    }
    Ok(StrykeValue::integer(best.0 as i64))
}

/// Nucleus (top-p) sampling probability mass cutoff
fn builtin_ml_nucleus_sample_p(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut v = b45_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let p_thresh = args.get(1).map(|v| v.to_number()).unwrap_or(0.95);
    v.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let mut acc = 0.0;
    for (i, &x) in v.iter().enumerate() {
        acc += x;
        if acc >= p_thresh { return Ok(StrykeValue::integer(i as i64)); }
    }
    Ok(StrykeValue::integer((v.len() as i64).max(1) - 1))
}

/// Temperature decay step
fn builtin_ml_temperature_decay(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let t0 = f1(args);
    let decay = args.get(1).map(|v| v.to_number()).unwrap_or(0.99);
    let step = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(t0 * decay.powf(step)))
}

/// Repetition penalty: divide logit by penalty if token already used
fn builtin_ml_repetition_penalty(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let logit = f1(args);
    let penalty = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if penalty == 0.0 { return Ok(StrykeValue::float(logit)); }
    Ok(StrykeValue::float(if logit > 0.0 { logit / penalty } else { logit * penalty }))
}

/// EOS logit boost (force termination)
fn builtin_ml_eos_logit_boost(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let logit = f1(args);
    let boost = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(logit + boost))
}
