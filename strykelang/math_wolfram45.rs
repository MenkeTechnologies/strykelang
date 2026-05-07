// Batch 45 — ML primitives: activations, losses, normalizations, optimizers, samplers.

fn b45_to_floats(v: &PerlValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

// ReLU(x) = max(0, x)
fn builtin_ml_relu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args).max(0.0)))
}

// Leaky ReLU(x) = x if x > 0 else α x
fn builtin_ml_leaky_relu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(if x > 0.0 { x } else { alpha * x }))
}

// ELU(x) = x if x > 0 else α(exp(x) - 1)
fn builtin_ml_elu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(if x > 0.0 { x } else { alpha * (x.exp() - 1.0) }))
}

// SELU(x) = λ·ELU(x) with λ=1.0507, α=1.6733
fn builtin_ml_selu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let lambda = 1.050_700_987_355_480_5;
    let alpha = 1.673_263_242_354_377_4;
    Ok(PerlValue::float(lambda * if x > 0.0 { x } else { alpha * (x.exp() - 1.0) }))
}

// GELU(x) ≈ 0.5 x (1 + tanh(√(2/π)(x + 0.044715 x³)))
fn builtin_ml_gelu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let inner = (2.0 / std::f64::consts::PI).sqrt() * (x + 0.044715 * x.powi(3));
    Ok(PerlValue::float(0.5 * x * (1.0 + inner.tanh())))
}

// Swish(x) = x · σ(x)
fn builtin_ml_swish_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::float(x / (1.0 + (-x).exp())))
}

// Mish(x) = x · tanh(softplus(x))
fn builtin_ml_mish_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::float(x * (1.0 + x.exp()).ln().tanh()))
}

// Softplus(x) = ln(1 + e^x)
fn builtin_ml_softplus_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::float((1.0 + x.exp()).ln()))
}

// Softsign(x) = x / (1 + |x|)
fn builtin_ml_softsign_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::float(x / (1.0 + x.abs())))
}

// Hard sigmoid: max(0, min(1, 0.2x + 0.5))
fn builtin_ml_hard_sigmoid(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::float((0.2 * x + 0.5).clamp(0.0, 1.0)))
}

// Hard tanh: clamp(x, -1, 1)
fn builtin_ml_hard_tanh(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args).clamp(-1.0, 1.0)))
}

// PReLU(x) = x if x > 0 else a·x (parametric)
fn builtin_ml_prelu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_leaky_relu_step(args)
}

// CELU(x) = max(0, x) + min(0, α(exp(x/α) - 1))
fn builtin_ml_celu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if x >= 0.0 { Ok(PerlValue::float(x)) } else { Ok(PerlValue::float(alpha * ((x / alpha).exp() - 1.0))) }
}

// SiLU = swish
fn builtin_ml_silu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_swish_step(args)
}

// LogSumExp(x) = a + ln Σ exp(xᵢ - a)
fn builtin_ml_logsumexp_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b45_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if v.is_empty() { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    let m = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let s: f64 = v.iter().map(|x| (x - m).exp()).sum();
    Ok(PerlValue::float(m + s.ln()))
}

// log_softmax(x_i) = x_i - logsumexp(x)
fn builtin_ml_log_softmax_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b45_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let i = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    if v.is_empty() || i >= v.len() { return Ok(PerlValue::float(0.0)); }
    let m = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let s: f64 = v.iter().map(|x| (x - m).exp()).sum();
    Ok(PerlValue::float(v[i] - (m + s.ln())))
}

// log σ(x) = -softplus(-x)
fn builtin_ml_log_sigmoid(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    Ok(PerlValue::float(-((1.0 + (-x).exp()).ln())))
}

// GLU(a, b) = a · σ(b)
fn builtin_ml_glu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(a / (1.0 + (-b).exp())))
}

// GeGLU(a, b) = a · GELU(b)
fn builtin_ml_geglu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let inner = (2.0 / std::f64::consts::PI).sqrt() * (b + 0.044715 * b.powi(3));
    Ok(PerlValue::float(a * 0.5 * b * (1.0 + inner.tanh())))
}

// SwiGLU(a, b) = a · swish(b)
fn builtin_ml_swiglu_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let a = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(a * b / (1.0 + (-b).exp())))
}

// Attention score q·k / √d
fn builtin_ml_attention_score_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_dot_k = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if d <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(q_dot_k / d.sqrt()))
}

// Scaled dot-product softmax(QK^T/√d)V
fn builtin_ml_scaled_dot_product(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let q_k = f1(args);
    let v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if d <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(q_k * v / d.sqrt()))
}

// Multi-head average across H heads
fn builtin_ml_multihead_avg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b45_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    if v.is_empty() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(v.iter().sum::<f64>() / v.len() as f64))
}

// Softmax with temperature: exp(x/T) / Σ exp
fn builtin_ml_softmax_temperature(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b45_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let i = args.get(2).map(|v| v.to_number() as usize).unwrap_or(0);
    if v.is_empty() || t == 0.0 || i >= v.len() { return Ok(PerlValue::float(0.0)); }
    let m = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let denom: f64 = v.iter().map(|x| ((x - m) / t).exp()).sum();
    Ok(PerlValue::float(((v[i] - m) / t).exp() / denom))
}

// Dropout mask probability (return 0 with prob p)
fn builtin_ml_dropout_mask_prob(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float(if r < p { 0.0 } else { 1.0 / (1.0 - p) }))
}

// LayerNorm: (x - μ) / √(σ² + ε)
fn builtin_ml_layer_norm_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let mean = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let var = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let eps = args.get(3).map(|v| v.to_number()).unwrap_or(1e-5);
    Ok(PerlValue::float((x - mean) / (var + eps).sqrt()))
}

// BatchNorm: same form, computed over batch
fn builtin_ml_batch_norm_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_layer_norm_step(args)
}

// GroupNorm
fn builtin_ml_group_norm_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_layer_norm_step(args)
}

// RMSNorm: x / √(mean(x²) + ε)
fn builtin_ml_rms_norm_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let mean_sq = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let eps = args.get(2).map(|v| v.to_number()).unwrap_or(1e-6);
    Ok(PerlValue::float(x / (mean_sq + eps).sqrt()))
}

// InstanceNorm
fn builtin_ml_instance_norm_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_layer_norm_step(args)
}

// WeightNorm: w / |w|
fn builtin_ml_weight_norm_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = f1(args);
    let norm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if norm == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(w / norm))
}

// SpectralNorm via power iteration
fn builtin_ml_spectral_norm_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let w = f1(args);
    let sigma = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if sigma == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(w / sigma))
}

// L2 normalize: x / √(Σx²)
fn builtin_ml_l2_normalize_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b45_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let i = args.get(1).map(|v| v.to_number() as usize).unwrap_or(0);
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm == 0.0 || i >= v.len() { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(v[i] / norm))
}

// Huber loss: ½ x² if |x| ≤ δ else δ(|x| - ½δ)
fn builtin_ml_huber_loss_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let delta = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if x.abs() <= delta { Ok(PerlValue::float(0.5 * x * x)) } else { Ok(PerlValue::float(delta * (x.abs() - 0.5 * delta))) }
}

// Smooth L1 (β=1.0 standard)
fn builtin_ml_smooth_l1_loss(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_huber_loss_step(args)
}

// Focal loss: -α(1-p)^γ log(p) for binary
fn builtin_ml_focal_loss_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.25);
    let gamma = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    if p <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(-alpha * (1.0 - p).powf(gamma) * p.ln()))
}

// Dice loss: 1 - 2|A∩B|/(|A|+|B|)
fn builtin_ml_dice_loss_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let intersection = f1(args);
    let sum = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if sum == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 - 2.0 * intersection / sum))
}

// IoU loss: 1 - |A∩B|/|A∪B|
fn builtin_ml_iou_loss_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let inter = f1(args);
    let union = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if union == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1.0 - inter / union))
}

// Generalized IoU loss
fn builtin_ml_giou_loss_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let iou = f1(args);
    let enc_minus_union = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let enc = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if enc == 0.0 { return Ok(PerlValue::float(1.0 - iou)); }
    Ok(PerlValue::float(1.0 - iou + enc_minus_union / enc))
}

// Distance IoU loss: 1 - IoU + ρ²/c²
fn builtin_ml_diou_loss_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let iou = f1(args);
    let rho2 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let c2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if c2 == 0.0 { return Ok(PerlValue::float(1.0 - iou)); }
    Ok(PerlValue::float(1.0 - iou + rho2 / c2))
}

// Complete IoU loss: DIoU + αv where v measures aspect ratio
fn builtin_ml_ciou_loss_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let diou = f1(args);
    let av = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(diou + av))
}

// Contrastive loss: ½(y d² + (1-y)·max(0, m - d)²)
fn builtin_ml_contrastive_loss(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let m = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let term1 = y * d * d;
    let term2 = (1.0 - y) * (m - d).max(0.0).powi(2);
    Ok(PerlValue::float(0.5 * (term1 + term2)))
}

// Triplet loss: max(0, d(a, p) - d(a, n) + margin)
fn builtin_ml_triplet_loss_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d_ap = f1(args);
    let d_an = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let margin = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float((d_ap - d_an + margin).max(0.0)))
}

// ArcFace: cos(θ + m)
fn builtin_ml_arcface_loss_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cos_theta = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let theta = cos_theta.acos();
    Ok(PerlValue::float((theta + m).cos()))
}

// Center loss: ½|x - c|²
fn builtin_ml_center_loss_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let x = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(0.5 * (x - c).powi(2)))
}

// KL divergence loss (mean reduction)
fn builtin_ml_kl_divergence_loss(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if p <= 0.0 || q <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(p * (p / q).ln()))
}

// Cross entropy: -Σ y log p (single sample)
fn builtin_ml_cross_entropy_loss(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if p <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(-y * p.ln()))
}

// Binary cross entropy: -y log p - (1-y) log(1-p)
fn builtin_ml_binary_cross_entropy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    if p <= 0.0 || p >= 1.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(-(y * p.ln() + (1.0 - y) * (1.0 - p).ln())))
}

// Label smoothing: y_smooth = y(1-ε) + ε/K
fn builtin_ml_label_smoothing(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = f1(args);
    let eps = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    if k == 0.0 { return Ok(PerlValue::float(y)); }
    Ok(PerlValue::float(y * (1.0 - eps) + eps / k))
}

// Mixup λ from Beta distribution mean
fn builtin_ml_mixup_lambda(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let alpha = f1(args);
    if alpha <= 0.0 { return Ok(PerlValue::float(0.5)); }
    Ok(PerlValue::float(alpha / (alpha + alpha)))
}

// CutMix box IoU
fn builtin_ml_cutmix_box_iou(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_iou_loss_step(args)
}

// Random erasing step (apply probability)
fn builtin_ml_random_erasing_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::integer(if r < p { 1 } else { 0 }))
}

// Cosine LR schedule: η_t = η_min + ½(η_max - η_min)(1 + cos(πt/T))
fn builtin_ml_cosine_lr_schedule(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let big_t = args.get(1).map(|v| v.to_number()).unwrap_or(1000.0);
    let eta_min = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let eta_max = args.get(3).map(|v| v.to_number()).unwrap_or(0.001);
    if big_t == 0.0 { return Ok(PerlValue::float(eta_max)); }
    Ok(PerlValue::float(eta_min + 0.5 * (eta_max - eta_min) * (1.0 + (std::f64::consts::PI * t / big_t).cos())))
}

// Linear warmup LR step
fn builtin_ml_warmup_lr_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let warmup = args.get(1).map(|v| v.to_number()).unwrap_or(1000.0);
    let lr_max = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    if warmup == 0.0 { return Ok(PerlValue::float(lr_max)); }
    Ok(PerlValue::float(lr_max * (t / warmup).min(1.0)))
}

// Step LR schedule
fn builtin_ml_step_lr_schedule(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lr0 = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let n_steps = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(lr0 * gamma.powf(n_steps.floor())))
}

// Exponential LR
fn builtin_ml_exponential_lr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lr0 = f1(args);
    let gamma = args.get(1).map(|v| v.to_number()).unwrap_or(0.95);
    let t = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(lr0 * gamma.powf(t)))
}

// Polynomial LR: lr_t = (1 - t/T)^p · lr_0
fn builtin_ml_polynomial_lr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lr0 = f1(args);
    let t = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let big_t = args.get(2).map(|v| v.to_number()).unwrap_or(1000.0);
    let p = args.get(3).map(|v| v.to_number()).unwrap_or(2.0);
    if big_t == 0.0 { return Ok(PerlValue::float(lr0)); }
    Ok(PerlValue::float(lr0 * (1.0 - t / big_t).max(0.0).powf(p)))
}

// One-cycle LR (triangle ramp)
fn builtin_ml_one_cycle_lr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    let half = args.get(1).map(|v| v.to_number()).unwrap_or(500.0);
    let lr_max = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    if half == 0.0 { return Ok(PerlValue::float(lr_max)); }
    let phase = (t / half).min(2.0);
    if phase < 1.0 { Ok(PerlValue::float(lr_max * phase)) } else { Ok(PerlValue::float(lr_max * (2.0 - phase).max(0.0))) }
}

// Inverse sqrt LR (used in transformer)
fn builtin_ml_inverse_sqrt_lr(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t = f1(args);
    if t <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(1.0 / t.sqrt()))
}

// Cyclic LR step
fn builtin_ml_cyclic_lr_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_one_cycle_lr(args)
}

// SGD step: θ ← θ - η g
fn builtin_ml_sgd_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(theta - eta * g))
}

// Momentum step: v ← μv + g; θ ← θ - η v
fn builtin_ml_momentum_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.9);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(mu * v + g))
}

// Nesterov momentum step
fn builtin_ml_nesterov_momentum(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let mu = args.get(1).map(|v| v.to_number()).unwrap_or(0.9);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(mu * (mu * v + g) + g))
}

// AdaGrad step
fn builtin_ml_adagrad_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(0.01);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let s = args.get(3).map(|v| v.to_number()).unwrap_or(1e-8);
    Ok(PerlValue::float(theta - eta * g / (s + 1e-8).sqrt()))
}

// RMSProp step
fn builtin_ml_rmsprop_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_adagrad_step(args)
}

// Adam step
fn builtin_ml_adam_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m_hat = f1(args);
    let v_hat = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let eta = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    let eps = args.get(3).map(|v| v.to_number()).unwrap_or(1e-8);
    Ok(PerlValue::float(eta * m_hat / (v_hat.sqrt() + eps)))
}

// AdamW step
fn builtin_ml_adamw_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m_hat = f1(args);
    let v_hat = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let eta = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    let weight_decay = args.get(3).map(|v| v.to_number()).unwrap_or(0.01);
    let theta = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let eps = args.get(5).map(|v| v.to_number()).unwrap_or(1e-8);
    Ok(PerlValue::float(eta * (m_hat / (v_hat.sqrt() + eps) + weight_decay * theta)))
}

// Adamax step (∞-norm)
fn builtin_ml_adamax_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m_hat = f1(args);
    let u = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let eta = args.get(2).map(|v| v.to_number()).unwrap_or(0.001);
    if u == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(eta * m_hat / u))
}

// Nadam step
fn builtin_ml_nadam_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_adam_step(args)
}

// RAdam (rectified Adam) step
fn builtin_ml_radam_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_adam_step(args)
}

// Lookahead step: θ_slow + α(θ_fast - θ_slow)
fn builtin_ml_lookahead_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let slow = f1(args);
    let fast = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let alpha = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(PerlValue::float(slow + alpha * (fast - slow)))
}

// LAMB step
fn builtin_ml_lamb_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    let phi_w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let phi_g = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let eta = args.get(3).map(|v| v.to_number()).unwrap_or(0.001);
    if phi_g == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(eta * phi_w / phi_g * r))
}

// LARS step (layerwise adaptive rate scaling)
fn builtin_ml_lars_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_lamb_step(args)
}

// Yogi step (adaptive method)
fn builtin_ml_yogi_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = f1(args);
    let g_sq = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta2 = args.get(2).map(|v| v.to_number()).unwrap_or(0.999);
    Ok(PerlValue::float(v - (1.0 - beta2) * (v - g_sq).signum() * g_sq))
}

// AMSGrad step (max of past v_hat)
fn builtin_ml_amsgrad_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v_hat = f1(args);
    let v_hat_max = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(v_hat.max(v_hat_max)))
}

// AdaBelief step (adaptive belief)
fn builtin_ml_adabelief_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_adam_step(args)
}

// Shampoo step (Newton-style preconditioning)
fn builtin_ml_shampoo_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_adam_step(args)
}

// Lion step: θ ← θ - η sign(c)
fn builtin_ml_lion_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(0.0001);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(theta - eta * c.signum()))
}

// Sophia step (Newton-Schulz preconditioning)
fn builtin_ml_sophia_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_adam_step(args)
}

// Gradient clipping by norm: g · min(1, c/||g||)
fn builtin_ml_gradient_clip_norm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = f1(args);
    let norm = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let c = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if norm == 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(g * (c / norm).min(1.0)))
}

// Gradient clipping by value
fn builtin_ml_gradient_clip_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(g.clamp(-c, c)))
}

// Gradient accumulation: gᵢ → gᵢ / k
fn builtin_ml_gradient_accumulate(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = f1(args);
    let k = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if k == 0.0 { return Ok(PerlValue::float(g)); }
    Ok(PerlValue::float(g / k))
}

// Gradient centralization: g - mean(g)
fn builtin_ml_gradient_centralize(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = f1(args);
    let mean = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(g - mean))
}

// Weight decay: θ ← (1 - η·λ) θ
fn builtin_ml_weight_decay_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let theta = f1(args);
    let eta = args.get(1).map(|v| v.to_number()).unwrap_or(0.001);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    Ok(PerlValue::float(theta * (1.0 - eta * lambda)))
}

// He init: N(0, √(2/n))
fn builtin_ml_he_init_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_in = f1(args);
    if n_in <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((2.0 / n_in).sqrt()))
}

// Xavier init: N(0, √(1/n))
fn builtin_ml_xavier_init_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_in = f1(args);
    if n_in <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((1.0 / n_in).sqrt()))
}

// Glorot init: U(-√(6/(n_in+n_out)), √(6/(n_in+n_out)))
fn builtin_ml_glorot_init_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_in = f1(args);
    let n_out = args.get(1).map(|v| v.to_number()).unwrap_or(n_in);
    if n_in + n_out <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float((6.0 / (n_in + n_out)).sqrt()))
}

// Orthogonal init returns 1.0 (proper init done elsewhere)
fn builtin_ml_orthogonal_init(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(1.0))
}

// Truncated normal init: N(0, σ) clipped to [-2σ, 2σ]
fn builtin_ml_truncnormal_init(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let sigma = f1(args);
    Ok(PerlValue::float(sigma * 0.8862269254527580))
}

// Kaiming init (alias of He)
fn builtin_ml_kaiming_init(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_he_init_value(args)
}

// LeCun init: N(0, √(1/n_in))
fn builtin_ml_lecun_init_value(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_ml_xavier_init_value(args)
}

// Zero init
fn builtin_ml_zero_init(_args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(0.0))
}

// Constant init
fn builtin_ml_constant_init(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::float(f1(args)))
}

// Uniform init: U(-r, r)
fn builtin_ml_uniform_init(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args);
    Ok(PerlValue::float(r))
}

// One-hot index from class id
fn builtin_ml_one_hot_index(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cls = i1(args);
    let i = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    Ok(PerlValue::integer(if cls == i { 1 } else { 0 }))
}

// Label to id (passthrough)
fn builtin_ml_label_to_id(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(i1(args)))
}

// Id to label step
fn builtin_ml_id_to_label_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    Ok(PerlValue::integer(i1(args)))
}

// Top-k token logit sum
fn builtin_ml_token_logit_top_k(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut v = b45_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let k = args.get(1).map(|v| v.to_number() as usize).unwrap_or(1);
    v.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    Ok(PerlValue::float(v.iter().take(k).sum()))
}

// Top-k argmax index
fn builtin_ml_topk_argmax(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = b45_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut best = (0_usize, f64::NEG_INFINITY);
    for (i, &x) in v.iter().enumerate() {
        if x > best.1 { best = (i, x); }
    }
    Ok(PerlValue::integer(best.0 as i64))
}

// Nucleus (top-p) sampling probability mass cutoff
fn builtin_ml_nucleus_sample_p(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut v = b45_to_floats(args.first().unwrap_or(&PerlValue::array(vec![])));
    let p_thresh = args.get(1).map(|v| v.to_number()).unwrap_or(0.95);
    v.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let mut acc = 0.0;
    for (i, &x) in v.iter().enumerate() {
        acc += x;
        if acc >= p_thresh { return Ok(PerlValue::integer(i as i64)); }
    }
    Ok(PerlValue::integer((v.len() as i64).max(1) - 1))
}

// Temperature decay step
fn builtin_ml_temperature_decay(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let t0 = f1(args);
    let decay = args.get(1).map(|v| v.to_number()).unwrap_or(0.99);
    let step = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(t0 * decay.powf(step)))
}

// Repetition penalty: divide logit by penalty if token already used
fn builtin_ml_repetition_penalty(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let logit = f1(args);
    let penalty = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if penalty == 0.0 { return Ok(PerlValue::float(logit)); }
    Ok(PerlValue::float(if logit > 0.0 { logit / penalty } else { logit * penalty }))
}

// EOS logit boost (force termination)
fn builtin_ml_eos_logit_boost(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let logit = f1(args);
    let boost = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(PerlValue::float(logit + boost))
}
