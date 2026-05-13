// NLP: ranking, similarity, edit distance, language models, attention variants.

fn b46_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

/// BM25 score: IDF·(tf(k1+1))/(tf + k1(1 - b + b·dl/avgdl))
fn builtin_nlp_bm25_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let tf = f1(args);
    let idf = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let k1 = args.get(2).map(|v| v.to_number()).unwrap_or(1.5);
    let b = args.get(3).map(|v| v.to_number()).unwrap_or(0.75);
    let dl = args.get(4).map(|v| v.to_number()).unwrap_or(100.0);
    let avgdl = args.get(5).map(|v| v.to_number()).unwrap_or(100.0);
    if avgdl == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let denom = tf + k1 * (1.0 - b + b * dl / avgdl);
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(idf * tf * (k1 + 1.0) / denom))
}

/// TF-IDF
fn builtin_nlp_tf_idf_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let tf = f1(args);
    let idf = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(tf * idf))
}

/// Okapi BM25+ (Lv & Zhai 2011): BM25 with a lower-bound additive term δ that
/// fixes the long-document under-scoring of plain BM25:
///   score = IDF · ( (tf · (k₁+1)) / (tf + k₁(1−b+b·dl/avgdl)) + δ ).
/// Distinct numerical behavior: at high tf, BM25+ converges to IDF·(k₁+1+δ)
/// instead of BM25's IDF·(k₁+1). Args: tf, IDF, k₁, b, dl, avgdl, δ (default 1.0).
fn builtin_nlp_okapi_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let tf = f1(args);
    let idf = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let k1 = args.get(2).map(|v| v.to_number()).unwrap_or(1.5);
    let b = args.get(3).map(|v| v.to_number()).unwrap_or(0.75);
    let dl = args.get(4).map(|v| v.to_number()).unwrap_or(100.0);
    let avgdl = args.get(5).map(|v| v.to_number()).unwrap_or(100.0);
    let delta = args.get(6).map(|v| v.to_number()).unwrap_or(1.0);
    if avgdl == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let denom = tf + k1 * (1.0 - b + b * dl / avgdl);
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(idf * ((tf * (k1 + 1.0)) / denom + delta)))
}

/// Word frequency value: term count in document divided by total token count
/// (relative TF). Args: term count, doc length.
fn builtin_nlp_word_freq_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let count = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(count / total))
}

/// Document frequency step
fn builtin_nlp_doc_freq_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prev = f1(args);
    let increment = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(prev + increment))
}

/// IDF = log(N / df)
fn builtin_nlp_inverse_doc_freq(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let df = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if df <= 0.0 || n <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((n / df).ln()))
}

/// Cosine similarity (precomputed dot, norms)
fn builtin_nlp_cosine_similarity_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dot = f1(args);
    let na = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let nb = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if na == 0.0 || nb == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(dot / (na * nb)))
}

/// Jaccard similarity |A∩B|/|A∪B|
fn builtin_nlp_jaccard_similarity_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let intersection = f1(args);
    let union = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if union == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(intersection / union))
}

/// Overlap coefficient |A∩B|/min(|A|,|B|)
fn builtin_nlp_overlap_coefficient(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let intersection = f1(args);
    let min_size = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if min_size == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(intersection / min_size))
}

/// Dice coefficient 2|A∩B|/(|A|+|B|)
fn builtin_nlp_dice_coefficient_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let inter = f1(args);
    let a_plus_b = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if a_plus_b == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(2.0 * inter / a_plus_b))
}

/// Simpson coefficient |A∩B|/min
fn builtin_nlp_simpson_coefficient(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_overlap_coefficient(args)
}

/// Levenshtein distance scalar from precomputed table cell
fn builtin_nlp_levenshtein_dist(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let prev_diag = f1(args);
    let prev_left = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let prev_up = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let cost = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((prev_diag + cost).min((prev_left + 1.0).min(prev_up + 1.0))))
}

/// Damerau-Levenshtein step (with transposition)
fn builtin_nlp_damerau_levenshtein(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lev = f1(args);
    let trans = args.get(1).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::float(lev.min(trans)))
}

/// Jaro distance
fn builtin_nlp_jaro_distance(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    let s1 = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let s2 = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let t = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    if m == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((m / s1 + m / s2 + (m - t) / m) / 3.0))
}

/// Jaro-Winkler: jaro + l·p·(1 - jaro)
fn builtin_nlp_jaro_winkler(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let jaro = f1(args);
    let l = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let p = args.get(2).map(|v| v.to_number()).unwrap_or(0.1);
    Ok(StrykeValue::float(jaro + l * p * (1.0 - jaro)))
}

/// Hamming distance: count of differing positions in two equal-length sequences.
/// Args: array of code points for s1, array for s2.
fn builtin_nlp_hamming_distance(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let a = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let b = b46_to_floats(args.get(1).unwrap_or(&StrykeValue::array(vec![])));
    let n = a.len().min(b.len());
    let diff = (0..n).filter(|&i| (a[i] - b[i]).abs() > 1e-12).count();
    let extra = a.len().abs_diff(b.len());
    Ok(StrykeValue::integer((diff + extra) as i64))
}

/// LCS length
fn builtin_nlp_lcs_length(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dp_diag = f1(args);
    let same = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let max_lr = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if same > 0 { Ok(StrykeValue::float(dp_diag + 1.0)) } else { Ok(StrykeValue::float(max_lr)) }
}

/// LCS ratio = LCS / max(|s1|, |s2|)
fn builtin_nlp_lcs_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lcs = f1(args);
    let max_len = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if max_len == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(lcs / max_len))
}

/// METEOR (precision, recall, fragmentation form)
fn builtin_nlp_meteor_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let frag = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if p + 9.0 * r == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let f_mean = 10.0 * p * r / (p + 9.0 * r);
    Ok(StrykeValue::float(f_mean * (1.0 - 0.5 * frag.powi(3))))
}

/// BLEU n-gram with brevity penalty (sentence-level)
fn builtin_nlp_bleu_score_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_n = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let bp = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if p_n.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let log_sum: f64 = p_n.iter().filter(|&&x| x > 0.0).map(|x| x.ln()).sum();
    Ok(StrykeValue::float(bp * (log_sum / p_n.len() as f64).exp()))
}

/// ROUGE-N (precision form)
fn builtin_nlp_rouge_score_n(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let match_count = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(match_count / total))
}

/// chrF: char F-score
fn builtin_nlp_chrf_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let r = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(2.0);
    if beta * beta * p + r == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((1.0 + beta * beta) * p * r / (beta * beta * p + r)))
}

/// Translation edit rate
fn builtin_nlp_ter_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let edits = f1(args);
    let ref_len = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if ref_len == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(edits / ref_len))
}

/// Word error rate = (S+D+I)/N
fn builtin_nlp_wer_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_ter_score(args)
}

/// Character error rate
fn builtin_nlp_cer_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_ter_score(args)
}

/// Perplexity = exp(avg negative log likelihood)
fn builtin_nlp_perplexity_value(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let avg_nll = f1(args);
    Ok(StrykeValue::float(avg_nll.exp()))
}

/// Bits per character
fn builtin_nlp_bits_per_character(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let nll = f1(args);
    Ok(StrykeValue::float(nll / 2f64.ln()))
}

/// Character n-gram count: (L - n + 1)
fn builtin_nlp_char_ngram_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let len = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    Ok(StrykeValue::integer((len - n + 1).max(0)))
}

/// Word n-gram count
fn builtin_nlp_word_ngram_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_char_ngram_count(args)
}

/// Skip-gram count: (L - n) C(n+k, k)
fn builtin_nlp_skip_gram_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let k = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let mut bin = 1_i64;
    for i in 0..k.min(50) { bin = bin.saturating_mul(n + i + 1) / (i + 1).max(1); }
    Ok(StrykeValue::integer((l - n).max(0).saturating_mul(bin)))
}

/// BPE merge step (simulated count of merges)
fn builtin_nlp_byte_pair_merge_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// WordPiece score: P(w|context) / P(token1)·P(token2)
fn builtin_nlp_wordpiece_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_w = f1(args);
    let p_split = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if p_split == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(p_w / p_split))
}

/// Unigram LM score
fn builtin_nlp_unigram_lm_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let probs = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(probs.iter().filter(|&&p| p > 0.0).map(|p| p.ln()).sum()))
}

/// Kneser-Ney step: P_KN(w|context) = max(c(context, w) - δ, 0) / Σ + λ(context) P_KN(w | shorter)
fn builtin_nlp_kneser_ney_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c = f1(args);
    let delta = args.get(1).map(|v| v.to_number()).unwrap_or(0.75);
    let total = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((c - delta).max(0.0) / total))
}

/// Witten-Bell smoothing step
fn builtin_nlp_witten_bell_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let unique = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if total + unique == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(c / (total + unique)))
}

/// Good-Turing count: c* = (c+1)·N_{c+1}/N_c
fn builtin_nlp_good_turing_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c = f1(args);
    let n_c = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let n_c_plus_1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if n_c == 0.0 { return Ok(StrykeValue::float(c)); }
    Ok(StrykeValue::float((c + 1.0) * n_c_plus_1 / n_c))
}

/// Laplace smoothing: (c+1)/(N+V)
fn builtin_nlp_laplace_smoothing(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if n + v == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((c + 1.0) / (n + v)))
}

/// Lidstone smoothing: (c+λ)/(N+λV)
fn builtin_nlp_lidstone_smoothing(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let v = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    let lambda = args.get(3).map(|v| v.to_number()).unwrap_or(0.5);
    if n + lambda * v == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((c + lambda) / (n + lambda * v)))
}

/// Jelinek-Mercer interpolation: λ P_higher + (1 - λ) P_lower
fn builtin_nlp_jelinek_mercer(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_high = f1(args);
    let p_low = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let lambda = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(lambda * p_high + (1.0 - lambda) * p_low))
}

/// Dirichlet smoothing: (c + μ P_collection) / (N + μ)
fn builtin_nlp_dirichlet_smoothing(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let c = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let mu = args.get(2).map(|v| v.to_number()).unwrap_or(2000.0);
    let p_coll = args.get(3).map(|v| v.to_number()).unwrap_or(1e-6);
    if n + mu == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((c + mu * p_coll) / (n + mu)))
}

/// Query likelihood: P(Q|D) = Π P(q_i|D)
fn builtin_nlp_query_likelihood_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_q = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(p_q.iter().filter(|&&p| p > 0.0).map(|p| p.ln()).sum()))
}

/// KL between language models
fn builtin_nlp_kl_lm_div(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p = f1(args);
    let q = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if p <= 0.0 || q <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(p * (p / q).ln()))
}

/// Pointwise mutual information
fn builtin_nlp_pmi_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_xy = f1(args);
    let p_x = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let p_y = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if p_x * p_y == 0.0 || p_xy <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((p_xy / (p_x * p_y)).log2()))
}

/// Normalized PMI
fn builtin_nlp_npmi_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pmi = f1(args);
    let p_xy = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if p_xy <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(pmi / (-p_xy.log2())))
}

/// χ² collocation test
fn builtin_nlp_chi2_collocation(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let observed = f1(args);
    let expected = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if expected == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((observed - expected).powi(2) / expected))
}

/// Log-likelihood collocation = 2·Σ obs · ln(obs/exp)
fn builtin_nlp_loglikelihood_collocation(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let obs = f1(args);
    let exp_v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if exp_v <= 0.0 || obs <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(2.0 * obs * (obs / exp_v).ln()))
}

/// t-score collocation = (mean - μ) / σ
fn builtin_nlp_t_score_collocation(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let obs = f1(args);
    let exp_v = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if obs <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((obs - exp_v) / obs.sqrt()))
}

/// Dunning log-likelihood (alias)
fn builtin_nlp_dunning_log_likelihood(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_loglikelihood_collocation(args)
}

/// LDA α update step
fn builtin_nlp_lda_alpha_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let alpha_old = f1(args);
    let log_term = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(alpha_old + log_term))
}

/// LDA β (topic-word) update: in collapsed Gibbs, after sampling a topic z for
/// word w, P(w|z) ∝ n_{w,z} + β / (n_z + W·β). Returns the unnormalized score
/// (n_wz + β) / (n_z + W·β). Args: n_wz, n_z, β, W (vocab size).
fn builtin_nlp_lda_beta_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_wz = f1(args);
    let n_z = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let beta = args.get(2).map(|v| v.to_number()).unwrap_or(0.01);
    let w_vocab = args.get(3).map(|v| v.to_number()).unwrap_or(10000.0);
    let denom = n_z + w_vocab * beta;
    if denom <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((n_wz + beta) / denom))
}

/// LDA topic distribution P(z|d) ∝ n_zd + α
fn builtin_nlp_lda_topic_dist(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n_zd = f1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.1);
    let total = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((n_zd + alpha) / total))
}

/// pLSA E-step: P(z|d, w) ∝ P(z|d) P(w|z). Returns posterior z|d,w.
fn builtin_nlp_plsa_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_z_d = f1(args);
    let p_w_z = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let denom = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if denom <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(p_z_d * p_w_z / denom))
}

/// Word2Vec skipgram loss = -log σ(v_c · v_w)
fn builtin_nlp_word2vec_skipgram_loss(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dot = f1(args);
    Ok(StrykeValue::float((1.0 + (-dot).exp()).ln()))
}

/// Word2Vec CBOW: predict center word v_c from average context vector
/// h = (1/2k) Σ_{|j|≤k, j≠0} v_{w+j}.  Loss = −log σ(v_c · h) using the
/// averaged context, NOT a single skip-gram pair. Args: dot of v_c with the
/// AVERAGED context h (caller pre-averages).
fn builtin_nlp_word2vec_cbow_loss(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let avg_dot = f1(args);
    Ok(StrykeValue::float((1.0 + (-avg_dot).exp()).ln()))
}

/// GloVe loss: f(x) (w_i · w_j + b_i + b_j - log x)²
fn builtin_nlp_glove_loss_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dot_b = f1(args);
    let log_x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let f_x = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(f_x * (dot_b - log_x).powi(2)))
}

/// FastText subword count: L - n + 1 + (L - n + 1 if both ends bounded)
fn builtin_nlp_fasttext_subword_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l = i1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(3);
    Ok(StrykeValue::integer((l + 2 - n + 1).max(0)))
}

/// Byte-level BPE step
fn builtin_nlp_byte_level_bpe_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_byte_pair_merge_step(args)
}

/// SentencePiece score (likelihood of segmentation)
fn builtin_nlp_sentencepiece_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_unigram_lm_score(args)
}

/// Unigram subword loss
fn builtin_nlp_unigram_subword_loss(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_unigram_lm_score(args)
}

/// Subword regularization: sample from top-k segmentations
fn builtin_nlp_subword_regularization(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    let alpha = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(alpha * n as f64))
}

/// Pointwise attention score: q · k / √d (no batch)
fn builtin_nlp_pointwise_attn_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let q_dot_k = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if d <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(q_dot_k / d.sqrt()))
}

/// Relative position bias (T5-style)
fn builtin_nlp_relative_position_bias(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dist = f1(args);
    let max_dist = args.get(1).map(|v| v.to_number()).unwrap_or(128.0);
    if max_dist == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(-dist.abs() / max_dist))
}

/// ALiBi position bias = -|i - j|·m
fn builtin_nlp_alibi_position_bias(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dist = f1(args);
    let m = args.get(1).map(|v| v.to_number()).unwrap_or(0.0625);
    Ok(StrykeValue::float(-dist.abs() * m))
}

/// Rotary position angle θ_i = pos / 10000^(2i/d)
fn builtin_nlp_rope_rotary_angle(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pos = f1(args);
    let i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(64.0);
    if d == 0.0 { return Ok(StrykeValue::float(pos)); }
    Ok(StrykeValue::float(pos / 10000f64.powf(2.0 * i / d)))
}

/// RoPE apply: rotate (x, y) by θ
fn builtin_nlp_rope_apply_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let x = f1(args);
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let theta = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(x * theta.cos() - y * theta.sin()))
}

/// Positional encoding sin: sin(pos/10000^(2i/d))
fn builtin_nlp_position_encoding_sin(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pos = f1(args);
    let i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(64.0);
    if d == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((pos / 10000f64.powf(2.0 * i / d)).sin()))
}

/// Positional encoding cos
fn builtin_nlp_position_encoding_cos(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pos = f1(args);
    let i = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(64.0);
    if d == 0.0 { return Ok(StrykeValue::float(1.0)); }
    Ok(StrykeValue::float((pos / 10000f64.powf(2.0 * i / d)).cos()))
}

/// PE frequency band: 1 / 10000^(2i/d)
fn builtin_nlp_pe_freq_band(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let i = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(64.0);
    if d == 0.0 { return Ok(StrykeValue::float(1.0)); }
    Ok(StrykeValue::float(1.0 / 10000f64.powf(2.0 * i / d)))
}

/// Max sequence length check
fn builtin_nlp_max_seq_len_check(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let len = f1(args);
    let max = args.get(1).map(|v| v.to_number()).unwrap_or(2048.0);
    Ok(StrykeValue::integer(if len <= max { 1 } else { 0 }))
}

/// Token drop rate
fn builtin_nlp_token_drop_rate(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dropped = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(dropped / total))
}

/// Byte frequency: count of target byte in stream / total bytes.
fn builtin_nlp_byte_frequency(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let stream = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let target = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if stream.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let count = stream.iter().filter(|&&b| (b - target).abs() < 0.5).count();
    Ok(StrykeValue::float(count as f64 / stream.len() as f64))
}

/// Character frequency: same form but interpreted as code points.
fn builtin_nlp_char_frequency(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_byte_frequency(args)
}

/// Punctuation ratio
fn builtin_nlp_punct_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let punct = f1(args);
    let total = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(punct / total))
}

/// Uppercase ratio
fn builtin_nlp_uppercase_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_punct_ratio(args)
}

/// Digit ratio
fn builtin_nlp_digit_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_punct_ratio(args)
}

/// Emoji ratio
fn builtin_nlp_emoji_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_punct_ratio(args)
}

/// URL count: occurrences of "://" trigram in text given as code-point array.
fn builtin_nlp_url_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut n = 0_i64;
    for w in s.windows(3) {
        if w[0] as i64 == ':' as i64 && w[1] as i64 == '/' as i64 && w[2] as i64 == '/' as i64 { n += 1; }
    }
    Ok(StrykeValue::integer(n))
}

/// Email count: occurrences of '@' between alphanumerics (simple heuristic: '@'
/// not at boundary). Code-point array input.
fn builtin_nlp_email_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut n = 0_i64;
    for i in 1..s.len().saturating_sub(1) {
        if s[i] as i64 == '@' as i64 && s[i - 1] as i64 > 32 && s[i + 1] as i64 > 32 { n += 1; }
    }
    Ok(StrykeValue::integer(n))
}

/// Phone count: digit-runs of length 7+ (E.164-shaped local/intl). Code-point input.
fn builtin_nlp_phone_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut n = 0_i64;
    let mut run = 0_usize;
    for &c in &s {
        let cp = c as i64;
        if (b'0' as i64..=b'9' as i64).contains(&cp) { run += 1; }
        else { if run >= 7 { n += 1; } run = 0; }
    }
    if run >= 7 { n += 1; }
    Ok(StrykeValue::integer(n))
}

/// Hashtag count: '#' followed by alnum, preceded by start or whitespace.
fn builtin_nlp_hashtag_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut n = 0_i64;
    for i in 0..s.len() {
        if s[i] as i64 != '#' as i64 { continue; }
        let prev_ok = i == 0 || (s[i - 1] as i64) <= 32;
        let next_ok = i + 1 < s.len() && (s[i + 1] as i64) > 47;
        if prev_ok && next_ok { n += 1; }
    }
    Ok(StrykeValue::integer(n))
}

/// Mention count: '@' preceded by start or whitespace, followed by alnum.
fn builtin_nlp_mention_count(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    let mut n = 0_i64;
    for i in 0..s.len() {
        if s[i] as i64 != '@' as i64 { continue; }
        let prev_ok = i == 0 || (s[i - 1] as i64) <= 32;
        let next_ok = i + 1 < s.len() && (s[i + 1] as i64) > 47;
        if prev_ok && next_ok { n += 1; }
    }
    Ok(StrykeValue::integer(n))
}

/// Token overlap of two sequences
fn builtin_nlp_token_overlap_two(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_jaccard_similarity_two(args)
}

/// Word Mover's distance step
fn builtin_nlp_word_mover_dist(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().sum()))
}

/// SIF weight a / (a + p(w))
fn builtin_nlp_sif_weight_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let p_w = f1(args);
    let a = args.get(1).map(|v| v.to_number()).unwrap_or(1e-3);
    if a + p_w == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(a / (a + p_w)))
}

/// Doc embedding average
fn builtin_nlp_doc_embedding_avg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if v.is_empty() { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(v.iter().sum::<f64>() / v.len() as f64))
}

/// Attention pool step (Σ α_i x_i)
fn builtin_nlp_attention_pool_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let alpha = f1(args);
    let x = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(alpha * x))
}

/// Max pool step
fn builtin_nlp_max_pool_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// Avg pool step
fn builtin_nlp_avg_pool_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_doc_embedding_avg(args)
}

/// Sum pool step
fn builtin_nlp_sum_pool_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let v = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    Ok(StrykeValue::float(v.iter().sum()))
}

/// Self-attention compute step
fn builtin_nlp_self_attn_compute_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_pointwise_attn_score(args)
}

/// Cross attention compute step
fn builtin_nlp_cross_attn_compute_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    builtin_nlp_pointwise_attn_score(args)
}

/// Window attention step (within radius w)
fn builtin_nlp_window_attn_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dist = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(64.0);
    let score = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(if dist.abs() <= w { score } else { f64::NEG_INFINITY }))
}

/// Strided attention step
fn builtin_nlp_strided_attn_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dist = i1(args);
    let stride = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let score = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if stride == 0 { return Ok(StrykeValue::float(score)); }
    Ok(StrykeValue::float(if dist % stride == 0 { score } else { f64::NEG_INFINITY }))
}

/// Block attention step (block-diagonal mask)
fn builtin_nlp_block_attn_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let block_i = i1(args);
    let block_j = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let score = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(if block_i == block_j { score } else { f64::NEG_INFINITY }))
}

/// Sliding-window attention (Mistral 7B / Longformer): a token at position i
/// attends to positions [i − w, i] (causal), but the EFFECTIVE receptive field
/// across L stacked layers is L · w due to layer-by-layer information
/// propagation. Returns the multi-layer reach for given (w, L). Args: window w,
/// layers L. Reach saturates at sequence length.
fn builtin_nlp_sliding_window_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let w = f1(args);
    let layers = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1.0);
    let max_seq = args.get(2).map(|v| v.to_number()).unwrap_or(f64::INFINITY);
    Ok(StrykeValue::float((w * layers).min(max_seq)))
}

/// Local attention (Longformer / BigBird): combines a sliding window of radius
/// w with a fixed set of g GLOBAL tokens that attend to/from everyone.
/// Per-token attention cost is O(w + g), TOTAL O(N(w + g)) rather than
/// pure sliding window's O(N·w). Args: window w, global g. Returns per-token
/// keys-attended-to.
fn builtin_nlp_local_attn_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let w = f1(args);
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(2.0 * w + 1.0 + g))
}

/// Dilated attention step
fn builtin_nlp_dilated_attn_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let dist = i1(args);
    let dilation = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let score = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    if dilation == 0 { return Ok(StrykeValue::float(score)); }
    Ok(StrykeValue::float(if dist % dilation == 0 { score } else { f64::NEG_INFINITY }))
}

/// Global attention: every token attends to every other (cost N²·d).
/// Args: scores array, returns max-stabilized softmax denominator over full window.
fn builtin_nlp_global_attn_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = b46_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if s.is_empty() { return Ok(StrykeValue::float(0.0)); }
    let m = s.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let denom: f64 = s.iter().map(|x| (x - m).exp()).sum();
    Ok(StrykeValue::float(m + denom.ln()))
}

/// Sparse attention score
fn builtin_nlp_sparse_attn_score(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mask = i1(args);
    let score = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float(if mask != 0 { score } else { f64::NEG_INFINITY }))
}

/// Linformer: project K, V to k×d (k << N). FLOPs ≈ 2·N·d·k. Args: N, d, k.
fn builtin_nlp_linformer_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(64.0);
    let k = args.get(2).map(|v| v.to_number()).unwrap_or(256.0);
    Ok(StrykeValue::float(2.0 * n * d * k))
}

/// Performer FAVOR+: random feature kernel. softmax(QK')V ≈ (φ(Q) · (φ(K)' V)).
/// Cost = O(N · d · m) for m random features. Args: N, d, m.
fn builtin_nlp_performer_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(64.0);
    let m = args.get(2).map(|v| v.to_number()).unwrap_or(64.0);
    Ok(StrykeValue::float(n * d * m))
}

/// Reformer LSH attention: cost N · log(N) · d. Args: N, d.
fn builtin_nlp_reformer_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let d = args.get(1).map(|v| v.to_number()).unwrap_or(64.0);
    if n <= 1.0 { return Ok(StrykeValue::float(d)); }
    Ok(StrykeValue::float(n * n.log2() * d))
}

/// Longformer: sliding window w + global g. Cost ≈ N(w + g)·d.
fn builtin_nlp_longformer_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(512.0);
    let g = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let d = args.get(3).map(|v| v.to_number()).unwrap_or(64.0);
    Ok(StrykeValue::float(n * (w + g) * d))
}

/// BigBird: window w + r random + g global. Cost ≈ N(w + r + g)·d.
fn builtin_nlp_bigbird_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let w = args.get(1).map(|v| v.to_number()).unwrap_or(64.0);
    let r = args.get(2).map(|v| v.to_number()).unwrap_or(64.0);
    let g = args.get(3).map(|v| v.to_number()).unwrap_or(64.0);
    let d = args.get(4).map(|v| v.to_number()).unwrap_or(64.0);
    Ok(StrykeValue::float(n * (w + r + g) * d))
}

/// Routing attention: top-k routing of clusters. Cost ≈ N·c·d for c clusters.
fn builtin_nlp_routing_attn_step(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let c = args.get(1).map(|v| v.to_number()).unwrap_or(n.sqrt());
    let d = args.get(2).map(|v| v.to_number()).unwrap_or(64.0);
    Ok(StrykeValue::float(n * c * d))
}
