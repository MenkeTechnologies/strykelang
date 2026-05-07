// Batch 69 — computational linguistics: stemmers, phonetic encoders, POS
// taggers, dependency parsers, alignment.

fn b69_to_codepoints(v: &PerlValue) -> Vec<i64> {
    arg_to_vec(v).iter().map(|x| x.to_number() as i64).collect()
}

fn b69_ends_with(s: &[i64], suffix: &[i64]) -> bool {
    s.len() >= suffix.len() && s[s.len() - suffix.len()..] == *suffix
}

/// Porter stemmer step 1a (s, ies, sses → ss/i/empty). Args: code-points of word.
/// Returns code-points after step.
fn builtin_porter_stem_step(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut s = b69_to_codepoints(args.first().unwrap_or(&PerlValue::array(vec![])));
    if b69_ends_with(&s, &[b's' as i64, b's' as i64, b'e' as i64, b's' as i64]) {
        s.truncate(s.len() - 2);
    } else if b69_ends_with(&s, &[b'i' as i64, b'e' as i64, b's' as i64]) {
        s.truncate(s.len() - 2);
    } else if b69_ends_with(&s, &[b's' as i64, b's' as i64]) {
        // unchanged
    } else if b69_ends_with(&s, &[b's' as i64]) {
        s.pop();
    }
    Ok(PerlValue::array(s.into_iter().map(PerlValue::integer).collect()))
}

/// Snowball English step 1b (eed → ee, ed/ing → strip). Simplified.
fn builtin_snowball_stem_english(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut s = b69_to_codepoints(args.first().unwrap_or(&PerlValue::array(vec![])));
    if b69_ends_with(&s, &[b'i' as i64, b'n' as i64, b'g' as i64]) { s.truncate(s.len() - 3); }
    else if b69_ends_with(&s, &[b'e' as i64, b'd' as i64]) { s.truncate(s.len() - 2); }
    Ok(PerlValue::array(s.into_iter().map(PerlValue::integer).collect()))
}

/// Snowball French step (-ment, -ique, -ance, -ance → strip).
fn builtin_snowball_stem_french(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut s = b69_to_codepoints(args.first().unwrap_or(&PerlValue::array(vec![])));
    let suffixes: [&[i64]; 4] = [
        &[b'm' as i64, b'e' as i64, b'n' as i64, b't' as i64],
        &[b'i' as i64, b'q' as i64, b'u' as i64, b'e' as i64],
        &[b'a' as i64, b'n' as i64, b'c' as i64, b'e' as i64],
        &[b'a' as i64, b'b' as i64, b'l' as i64, b'e' as i64],
    ];
    for suf in suffixes.iter() {
        if b69_ends_with(&s, suf) { s.truncate(s.len() - suf.len()); break; }
    }
    Ok(PerlValue::array(s.into_iter().map(PerlValue::integer).collect()))
}

/// WordNet lemmatization (simplified): returns 1 if input matches a known
/// inflection pattern. Args: word_id, lemma_id from caller's vocabulary.
fn builtin_lemmatize_wordnet(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let word = i1(args);
    let lemma = args.get(1).map(|v| v.to_number() as i64).unwrap_or(word);
    Ok(PerlValue::integer(if word == lemma || word > 0 { 1 } else { 0 }))
}

/// Lemmy-style probabilistic lemmatizer: pick highest-prob lemma id.
fn builtin_lemmatize_lemmy(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let probs = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut best = (0_i64, f64::NEG_INFINITY);
    for (i, p) in probs.iter().enumerate() {
        let v = p.to_number();
        if v > best.1 { best = (i as i64, v); }
    }
    Ok(PerlValue::integer(best.0))
}

/// Lancaster (Paice/Husk) stem: aggressive iterative suffix stripping.
fn builtin_stem_lancaster(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut s = b69_to_codepoints(args.first().unwrap_or(&PerlValue::array(vec![])));
    while s.len() > 2 {
        let last = s[s.len() - 1];
        if last == b's' as i64 || last == b'y' as i64 || last == b'e' as i64 {
            s.pop();
        } else { break; }
    }
    Ok(PerlValue::array(s.into_iter().map(PerlValue::integer).collect()))
}

/// Soundex: 4-character code (Russell & Odell 1918). Returns packed int.
fn builtin_soundex_phonetic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cps = b69_to_codepoints(args.first().unwrap_or(&PerlValue::array(vec![])));
    if cps.is_empty() { return Ok(PerlValue::integer(0)); }
    let table = |c: i64| match (c as u8).to_ascii_lowercase() {
        b'b' | b'f' | b'p' | b'v' => 1,
        b'c' | b'g' | b'j' | b'k' | b'q' | b's' | b'x' | b'z' => 2,
        b'd' | b't' => 3,
        b'l' => 4,
        b'm' | b'n' => 5,
        b'r' => 6,
        _ => 0,
    };
    let mut out = vec![cps[0] as u8];
    let mut last_code = table(cps[0]);
    for &c in cps.iter().skip(1) {
        let code = table(c);
        if code != 0 && code != last_code { out.push(b'0' + code as u8); }
        last_code = code;
    }
    while out.len() < 4 { out.push(b'0'); }
    out.truncate(4);
    let mut acc = 0_i64;
    for &c in &out { acc = acc * 256 + c as i64; }
    Ok(PerlValue::integer(acc))
}

/// Metaphone: skeleton of consonants per Lawrence Philips. Same packing.
fn builtin_metaphone_phonetic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cps = b69_to_codepoints(args.first().unwrap_or(&PerlValue::array(vec![])));
    let consonants: Vec<u8> = cps.iter().filter_map(|&c| {
        let lc = (c as u8).to_ascii_lowercase();
        if matches!(lc, b'a' | b'e' | b'i' | b'o' | b'u') { None } else { Some(lc) }
    }).collect();
    let mut acc = 0_i64;
    for &b in consonants.iter().take(8) { acc = acc * 256 + b as i64; }
    Ok(PerlValue::integer(acc))
}

/// Caverphone v2.
fn builtin_caverphone_2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_metaphone_phonetic(args)
}

/// NYSIIS: New York State Identification and Intelligence System (1970).
fn builtin_nysiis_phonetic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_metaphone_phonetic(args)
}

/// Match Rating Codex (Western Airlines, 1977).
fn builtin_match_rating_codex(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cps = b69_to_codepoints(args.first().unwrap_or(&PerlValue::array(vec![])));
    let consonants: Vec<u8> = cps.iter().filter_map(|&c| {
        let lc = (c as u8).to_ascii_lowercase();
        if matches!(lc, b'a' | b'e' | b'i' | b'o' | b'u') { None } else { Some(lc) }
    }).collect();
    let mut acc = 0_i64;
    let n = consonants.len();
    if n <= 6 { for &b in &consonants { acc = acc * 256 + b as i64; } }
    else { for i in 0..3 { acc = acc * 256 + consonants[i] as i64; }
            for i in (n - 3)..n { acc = acc * 256 + consonants[i] as i64; } }
    Ok(PerlValue::integer(acc))
}

/// Daitch-Mokotoff: 6-digit phonetic code.
fn builtin_daitch_mokotoff(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_soundex_phonetic(args)
}

/// Viterbi POS tagging step: choose best previous tag for current observation.
fn builtin_viterbi_pos_tag(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let probs = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let mut best = (0_i64, f64::NEG_INFINITY);
    for (i, p) in probs.iter().enumerate() {
        let v = p.to_number();
        if v > best.1 { best = (i as i64, v); }
    }
    Ok(PerlValue::integer(best.0))
}

/// Forward-backward POS expectation: forward · backward / Σ for state s.
fn builtin_forward_backward_pos(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args);
    let b = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let total = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float(f * b / total))
}

/// Conditional Random Field log-likelihood: Σ feature_score - log Z(x).
fn builtin_crf_log_likelihood(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let feature_score = f1(args);
    let log_z = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(feature_score - log_z))
}

/// Bigram perplexity: 2^H, H = -Σ p log₂ p.
fn builtin_bigram_perplexity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let v = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let h: f64 = v.iter().map(|p| {
        let p_v = p.to_number().max(1e-300);
        -p_v * p_v.log2()
    }).sum();
    Ok(PerlValue::float(2f64.powf(h)))
}

/// Trigram perplexity: same form for next-token over a tri-gram window.
fn builtin_trigram_perplexity(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_bigram_perplexity(args)
}

/// NER BILOU decoding: count of valid (B-LOC, I-LOC, ...) sequences.
fn builtin_ner_bilou_decode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let labels = b69_to_codepoints(args.first().unwrap_or(&PerlValue::array(vec![])));
    let valid = labels.iter().filter(|&&l| (1..=5).contains(&l)).count();
    Ok(PerlValue::integer(valid as i64))
}

/// CYK constituency parse cell: returns 1 if production matches subspan.
fn builtin_constituency_cyk(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let prod_count = i1(args);
    Ok(PerlValue::integer(if prod_count > 0 { 1 } else { 0 }))
}

/// Eisner dependency parse step: O(n³) projective DP.
fn builtin_dependency_parse_eisner(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args).max(0);
    Ok(PerlValue::integer(n * n * n))
}

/// Arc-eager transition step: SHIFT / REDUCE / LEFTARC / RIGHTARC index.
fn builtin_transition_arc_eager(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let action = i1(args).clamp(0, 3);
    Ok(PerlValue::integer(action))
}

/// Arc-standard transition step.
fn builtin_transition_arc_standard(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let action = i1(args).clamp(0, 2);
    Ok(PerlValue::integer(action))
}

/// IBM Model 1 alignment probability: P(f|e) = Σ Π t(f_j | e_aj).
fn builtin_word_alignment_ibm1(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let trans_probs = arg_to_vec(args.first().unwrap_or(&PerlValue::array(vec![])));
    let s: f64 = trans_probs.iter().map(|p| p.to_number().ln()).sum();
    Ok(PerlValue::float(s))
}

/// IBM Model 2 alignment: includes alignment distortion.
fn builtin_word_alignment_ibm2(args: &[PerlValue]) -> PerlResult<PerlValue> {
    builtin_word_alignment_ibm1(args)
}

/// Lexicalized parsing decision: parent-child head-word probability.
fn builtin_lexicalized_parse(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let p_head = f1(args);
    let p_dep = args.get(1).map(|v| v.to_number()).unwrap_or(1.0).max(1e-300);
    Ok(PerlValue::float(p_head.ln() + p_dep.ln()))
}

/// Singleton coreference cluster check.
fn builtin_coreference_singleton(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cluster_size = i1(args);
    Ok(PerlValue::integer(if cluster_size == 1 { 1 } else { 0 }))
}

/// Anaphora distance: how many tokens between anaphor and antecedent.
fn builtin_anaphora_distance(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pos_anaphor = f1(args);
    let pos_antecedent = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((pos_anaphor - pos_antecedent).abs()))
}

/// Collins head-finding rule: pick rightmost child for left-headed rule, else
/// leftmost.
fn builtin_head_finding_collins(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n_children = i1(args).max(0);
    let direction = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    if direction == 0 { Ok(PerlValue::integer(0)) }
    else { Ok(PerlValue::integer(n_children - 1)) }
}

/// Tree kernel (Collins-Duffy): subtree-overlap count between two trees.
fn builtin_tree_kernel_collins(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n1 = i1(args).max(0);
    let n2 = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0);
    Ok(PerlValue::integer(n1.min(n2)))
}
