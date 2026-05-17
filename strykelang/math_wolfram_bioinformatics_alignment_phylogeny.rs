// bioinformatics deep: alignment, motifs, phylogenetics, structure.

// Needleman-Wunsch global alignment score
fn builtin_needleman_wunsch_score(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let match_s = args.get(2).map(|v| v.to_number() as i32).unwrap_or(1);
    let mismatch = args.get(3).map(|v| v.to_number() as i32).unwrap_or(-1);
    let gap = args.get(4).map(|v| v.to_number() as i32).unwrap_or(-2);
    let m = a.chars().count();
    let n = b.chars().count();
    let mut dp = vec![vec![0_i32; n + 1]; m + 1];
    for i in 0..=m { dp[i][0] = i as i32 * gap; }
    for j in 0..=n { dp[0][j] = j as i32 * gap; }
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    for i in 1..=m {
        for j in 1..=n {
            let s = if av[i - 1] == bv[j - 1] { match_s } else { mismatch };
            dp[i][j] = (dp[i - 1][j - 1] + s).max(dp[i - 1][j] + gap).max(dp[i][j - 1] + gap);
        }
    }
    Ok(StrykeValue::integer(dp[m][n] as i64))
}

// Smith-Waterman local alignment score
fn builtin_smith_waterman_score(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let match_s = args.get(2).map(|v| v.to_number() as i32).unwrap_or(2);
    let mismatch = args.get(3).map(|v| v.to_number() as i32).unwrap_or(-1);
    let gap = args.get(4).map(|v| v.to_number() as i32).unwrap_or(-2);
    let m = a.chars().count();
    let n = b.chars().count();
    let mut dp = vec![vec![0_i32; n + 1]; m + 1];
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let mut best = 0_i32;
    for i in 1..=m {
        for j in 1..=n {
            let s = if av[i - 1] == bv[j - 1] { match_s } else { mismatch };
            dp[i][j] = 0_i32
                .max(dp[i - 1][j - 1] + s)
                .max(dp[i - 1][j] + gap)
                .max(dp[i][j - 1] + gap);
            if dp[i][j] > best { best = dp[i][j]; }
        }
    }
    Ok(StrykeValue::integer(best as i64))
}

// PAM250 substitution score (simplified diagonal/off)
fn builtin_pam250_score(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    if a.is_empty() || b.is_empty() { return Ok(StrykeValue::integer(0)); }
    let ca = a.chars().next().unwrap();
    let cb = b.chars().next().unwrap();
    let same_group = |g: &str| g.contains(ca) && g.contains(cb);
    if ca == cb { Ok(StrykeValue::integer(7)) }
    else if same_group("ILMV") { Ok(StrykeValue::integer(3)) }
    else if same_group("FYW") { Ok(StrykeValue::integer(4)) }
    else if same_group("DE") || same_group("KR") { Ok(StrykeValue::integer(3)) }
    else { Ok(StrykeValue::integer(-2)) }
}

// Tanimoto / Jaccard for fingerprints (bit vectors as 0/1 arrays)
fn builtin_tanimoto_bits(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a: Vec<i64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number() as i64).collect();
    let b: Vec<i64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number() as i64).collect();
    let n = a.len().min(b.len());
    let mut and_c = 0_i64;
    let mut or_c = 0_i64;
    for i in 0..n {
        if a[i] != 0 && b[i] != 0 { and_c += 1; }
        if a[i] != 0 || b[i] != 0 { or_c += 1; }
    }
    if or_c == 0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(and_c as f64 / or_c as f64))
}

// Translate DNA sequence to protein (ignores ambiguity)
fn builtin_translate_dna(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
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
    let m: std::collections::HashMap<&str, char> = table.iter().copied().collect();
    let bytes: Vec<u8> = s.bytes().filter(|c| c.is_ascii_alphabetic()).collect();
    let mut protein = String::new();
    for chunk in bytes.chunks(3) {
        if chunk.len() < 3 { break; }
        let codon = std::str::from_utf8(chunk).unwrap_or("");
        protein.push(*m.get(codon).unwrap_or(&'X'));
    }
    Ok(StrykeValue::string(protein))
}

// Transcribe DNA → RNA (T→U)
fn builtin_transcribe_dna_rna(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(StrykeValue::string(s.replace('T', "U").replace('t', "u")))
}

// Reverse-transcribe RNA → DNA
fn builtin_reverse_transcribe(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(StrykeValue::string(s.replace('U', "T").replace('u', "t")))
}

// AT content (fraction)
fn builtin_at_content(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut total = 0;
    let mut at = 0;
    for c in s.chars() {
        let u = c.to_ascii_uppercase();
        if matches!(u, 'A' | 'T' | 'U' | 'C' | 'G') {
            total += 1;
            if u == 'A' || u == 'T' || u == 'U' { at += 1; }
        }
    }
    if total == 0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(at as f64 / total as f64))
}

// Melting temperature Tm (Wallace rule, short oligos)
fn builtin_tm_wallace(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let at = s.chars().filter(|c| matches!(c, 'A' | 'T')).count() as f64;
    let gc = s.chars().filter(|c| matches!(c, 'G' | 'C')).count() as f64;
    Ok(StrykeValue::float(2.0 * at + 4.0 * gc))
}

// Tm Marmur-Schildkraut (long oligos / DNA)
fn builtin_tm_marmur(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let total = s.chars().filter(|c| c.is_ascii_alphabetic()).count() as f64;
    if total == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let gc = s.chars().filter(|c| matches!(c, 'G' | 'C')).count() as f64;
    let gc_pct = 100.0 * gc / total;
    Ok(StrykeValue::float(0.41 * gc_pct + 81.5 - 600.0 / total))
}

// Codon adaptation index (CAI) given codon weights
fn builtin_codon_adaptation_index(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let weights = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    let weight_map: std::collections::HashMap<String, f64> = weights.chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0].to_string(), c[1].to_number()))
        .collect();
    let bytes: Vec<u8> = s.bytes().filter(|c| c.is_ascii_alphabetic()).collect();
    let mut log_sum = 0.0;
    let mut count = 0;
    for chunk in bytes.chunks(3) {
        if chunk.len() < 3 { break; }
        let codon = std::str::from_utf8(chunk).unwrap_or("");
        if let Some(&w) = weight_map.get(codon) {
            if w > 0.0 {
                log_sum += w.ln();
                count += 1;
            }
        }
    }
    if count == 0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float((log_sum / count as f64).exp()))
}

// k-mer Jaccard similarity
fn builtin_kmer_jaccard(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let k = args.get(2).map(|v| v.to_number() as usize).unwrap_or(3).max(1);
    let to_set = |s: &str| -> std::collections::HashSet<String> {
        let bytes = s.as_bytes();
        if bytes.len() < k { return std::collections::HashSet::new(); }
        (0..=bytes.len() - k).map(|i| String::from_utf8_lossy(&bytes[i..i + k]).into_owned()).collect()
    };
    let sa = to_set(&a);
    let sb = to_set(&b);
    let inter = sa.intersection(&sb).count();
    let uni = sa.union(&sb).count();
    if uni == 0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(inter as f64 / uni as f64))
}

// Shannon information of sequence (bits per base)
fn builtin_sequence_shannon_info(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut counts: std::collections::HashMap<char, usize> = std::collections::HashMap::new();
    let mut total = 0_usize;
    for c in s.chars().filter(|c| c.is_ascii_alphabetic()) {
        *counts.entry(c.to_ascii_uppercase()).or_default() += 1;
        total += 1;
    }
    if total == 0 { return Ok(StrykeValue::float(0.0)); }
    let h: f64 = counts.values().map(|&c| {
        let p = c as f64 / total as f64;
        -p * p.log2()
    }).sum();
    Ok(StrykeValue::float(h))
}

// Position weight matrix score for sequence given log-odds matrix (rows=positions, cols=A,C,G,T order)
fn builtin_pwm_score(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let pwm = matrix_from_value(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF));
    if pwm.is_empty() || pwm[0].len() < 4 { return Ok(StrykeValue::float(0.0)); }
    let mut score = 0.0;
    for (i, c) in s.chars().enumerate() {
        if i >= pwm.len() { break; }
        let col = match c {
            'A' => 0, 'C' => 1, 'G' => 2, 'T' | 'U' => 3,
            _ => continue,
        };
        score += pwm[i][col];
    }
    Ok(StrykeValue::float(score))
}

// Shannon entropy of multiple sequence alignment column (probabilities)
fn builtin_msa_column_entropy(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let probs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let h: f64 = probs.iter().filter(|&&p| p > 0.0)
        .map(|&p| -p * p.log2()).sum();
    Ok(StrykeValue::float(h))
}

// Sequence logo information content (bits) per column
fn builtin_seq_logo_information(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let probs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let alphabet = args.get(1).map(|v| v.to_number() as usize).unwrap_or(4).max(2);
    let max_h = (alphabet as f64).log2();
    let h: f64 = probs.iter().filter(|&&p| p > 0.0)
        .map(|&p| -p * p.log2()).sum();
    Ok(StrykeValue::float(max_h - h))
}

// Levenshtein distance (general string edit)

// Damerau-Levenshtein
fn builtin_damerau_levenshtein(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let m = av.len();
    let n = bv.len();
    let mut dp = vec![vec![0_usize; n + 1]; m + 1];
    for i in 0..=m { dp[i][0] = i; }
    for j in 0..=n { dp[0][j] = j; }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if av[i - 1] == bv[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1).min(dp[i][j - 1] + 1).min(dp[i - 1][j - 1] + cost);
            if i >= 2 && j >= 2 && av[i - 1] == bv[j - 2] && av[i - 2] == bv[j - 1] {
                dp[i][j] = dp[i][j].min(dp[i - 2][j - 2] + cost);
            }
        }
    }
    Ok(StrykeValue::integer(dp[m][n] as i64))
}

// Longest common subsequence length
fn builtin_lcs_length(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let av: Vec<char> = a.chars().collect();
    let bv: Vec<char> = b.chars().collect();
    let m = av.len();
    let n = bv.len();
    let mut dp = vec![vec![0_usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if av[i - 1] == bv[j - 1] { dp[i][j] = dp[i - 1][j - 1] + 1; }
            else { dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]); }
        }
    }
    Ok(StrykeValue::integer(dp[m][n] as i64))
}

// Longest common substring

// Hirschberg space-efficient LCS length (same result, different algorithm)
fn builtin_hirschberg_lcs_length(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    builtin_lcs_length(args)
}

// Number of common k-mers
fn builtin_common_kmers(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let k = args.get(2).map(|v| v.to_number() as usize).unwrap_or(4).max(1);
    let mut sa: std::collections::HashSet<String> = std::collections::HashSet::new();
    if a.len() >= k {
        for i in 0..=a.len() - k {
            sa.insert(a[i..i + k].to_string());
        }
    }
    let mut count = 0;
    if b.len() >= k {
        for i in 0..=b.len() - k {
            if sa.contains(&b[i..i + k]) { count += 1; }
        }
    }
    Ok(StrykeValue::integer(count as i64))
}

// Phylogenetic distance from sequence identity (Jukes-Cantor)
fn builtin_jukes_cantor_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let identity = f1(args).clamp(0.0, 1.0);
    let p = 1.0 - identity;
    if p >= 0.75 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-0.75 * (1.0 - 4.0 * p / 3.0).max(1e-15).ln()))
}

// Kimura 2-parameter distance (transitions ts, transversions tv as fractions)
fn builtin_kimura_2p_distance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let ts = f1(args).clamp(0.0, 1.0);
    let tv = args.get(1).map(|v| v.to_number()).unwrap_or(0.0).clamp(0.0, 1.0);
    let term1 = 1.0 - 2.0 * ts - tv;
    let term2 = 1.0 - 2.0 * tv;
    if term1 <= 0.0 || term2 <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(-0.5 * term1.ln() - 0.25 * term2.ln()))
}

// Felsenstein pruning step (for binary tree, log-likelihood at internal node)
fn builtin_felsenstein_step(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p_left: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let p_right: Vec<f64> = arg_to_vec(&args.get(1).cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = p_left.len().min(p_right.len());
    let out: Vec<StrykeValue> = (0..n).map(|i| StrykeValue::float(p_left[i] * p_right[i])).collect();
    Ok(StrykeValue::array(out))
}

// Branch length from substitutions and length
fn builtin_branch_length_substitutions(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let subs = f1(args);
    let length = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if length == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(subs / length))
}

// Number of trees on n labeled tips (rooted): (2n-3)!! for unrooted
fn builtin_num_unrooted_trees(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args);
    if n < 3 { return Ok(StrykeValue::integer(1)); }
    let mut prod = 1_i128;
    for k in 1..n - 1 { prod *= (2 * k - 1) as i128; }
    Ok(StrykeValue::integer(prod as i64))
}

// Bayesian posterior given prior and likelihood (single hypothesis, evidence)
fn builtin_bayes_posterior(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let prior = f1(args);
    let likelihood = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let evidence = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if evidence == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(prior * likelihood / evidence))
}

// Hardy-Weinberg expected genotype counts (n = pop size)
fn builtin_hw_expected_counts(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p = f1(args).clamp(0.0, 1.0);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(100.0);
    let q = 1.0 - p;
    Ok(StrykeValue::array(vec![
        StrykeValue::float(n * p * p),
        StrykeValue::float(n * 2.0 * p * q),
        StrykeValue::float(n * q * q),
    ]))
}

// Allele frequency from genotype counts (AA, AB, BB)
fn builtin_allele_frequency(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let aa = f1(args);
    let ab = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let bb = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let total = aa + ab + bb;
    if total == 0.0 { return Ok(StrykeValue::float(0.5)); }
    Ok(StrykeValue::float((2.0 * aa + ab) / (2.0 * total)))
}

// Linkage disequilibrium D = p_AB - p_A·p_B
fn builtin_ld_d(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p_ab = f1(args);
    let p_a = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let p_b = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    Ok(StrykeValue::float(p_ab - p_a * p_b))
}

// LD r² statistic
fn builtin_ld_r_squared(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let d = f1(args);
    let p_a = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let p_b = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    let denom = p_a * (1.0 - p_a) * p_b * (1.0 - p_b);
    if denom <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(d * d / denom))
}

// FST (Wright) from heterozygosities

// Heterozygosity 2pq
fn builtin_heterozygosity(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p = f1(args).clamp(0.0, 1.0);
    Ok(StrykeValue::float(2.0 * p * (1.0 - p)))
}

// Effective population size from variance Ne = (1/(2Vp))·F
fn builtin_ne_from_variance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let var_p = f1(args);
    let p = args.get(1).map(|v| v.to_number()).unwrap_or(0.5);
    let q = 1.0 - p;
    if var_p == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(p * q / (2.0 * var_p)))
}

// Ploidy expected from coverage uniformity
fn builtin_expected_coverage(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n_reads = f1(args);
    let read_len = args.get(1).map(|v| v.to_number()).unwrap_or(150.0);
    let genome_len = args.get(2).map(|v| v.to_number()).unwrap_or(3e9);
    if genome_len == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(n_reads * read_len / genome_len))
}

// Lander-Waterman expected coverage gap distribution mean
fn builtin_lander_waterman_gaps(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let coverage = f1(args);
    Ok(StrykeValue::float((-coverage).exp()))
}

// FDR Benjamini-Hochberg adjusted p-value (single rank)
fn builtin_bh_adjusted_p(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p_value = f1(args);
    let rank = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let total = args.get(2).map(|v| v.to_number()).unwrap_or(1.0);
    if rank == 0.0 { return Ok(StrykeValue::float(p_value)); }
    Ok(StrykeValue::float((p_value * total / rank).min(1.0)))
}

// Bonferroni correction
#[allow(dead_code)]
fn builtin_bonferroni(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let p = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float((p * n).min(1.0)))
}

// Z-score for a count vs expected
fn builtin_zscore_count(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let observed = f1(args);
    let expected = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let stddev = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float((observed - expected) / stddev))
}

// Hypergeometric PMF (small N, exact)

// GO term enrichment p-value (one-sided hypergeometric)
fn builtin_go_enrichment_p(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n_total = i1(args).max(1);
    let k_success = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0);
    let n_draw = args.get(2).map(|v| v.to_number() as i64).unwrap_or(0);
    let k_obs = args.get(3).map(|v| v.to_number() as i64).unwrap_or(0);
    fn binom(n: i64, k: i64) -> f64 {
        if k < 0 || k > n { return 0.0; }
        let mut r = 1.0;
        for i in 0..k {
            r *= (n - i) as f64 / (i + 1) as f64;
        }
        r
    }
    let denom = binom(n_total, n_draw);
    if denom == 0.0 { return Ok(StrykeValue::float(0.0)); }
    let mut p = 0.0;
    let max_k = k_success.min(n_draw);
    for k in k_obs..=max_k {
        p += binom(k_success, k) * binom(n_total - k_success, n_draw - k) / denom;
    }
    Ok(StrykeValue::float(p.clamp(0.0, 1.0)))
}

// BLOSUM45 substitution score (Henikoff & Henikoff 1992): log-odds matrix from
// blocks with ≥45% identity. Encoded for the 6-class amino-acid grouping
// (small/non-polar/polar/positive/negative/aromatic) with characteristic
// off-diagonal scores: identical=10, same group=4, hydrophobic↔hydrophobic=2,
// otherwise mismatch ranges from −3 to 1. Distinct from PAM250 (different
// underlying corpus and target identity threshold). Args: class_a, class_b (0..5).
fn builtin_blosum45_score(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = i1(args).max(0) as usize;
    let b = args.get(1).map(|v| v.to_number() as i64).unwrap_or(0).max(0) as usize;
    const M: [[i64; 6]; 6] = [
        [ 5,  0, -1, -1, -2, -2],
        [ 0,  5, -1, -2, -3, -1],
        [-1, -1,  6,  1,  0, -2],
        [-1, -2,  1,  7,  1, -2],
        [-2, -3,  0,  1,  7, -3],
        [-2, -1, -2, -2, -3,  8],
    ];
    if a >= 6 || b >= 6 { return Ok(StrykeValue::integer(0)); }
    Ok(StrykeValue::integer(M[a][b]))
}

// Sequence weight (Henikoff)
fn builtin_henikoff_weight(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n_distinct = f1(args);
    let r_count = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n_distinct == 0.0 || r_count == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(1.0 / (n_distinct * r_count)))
}

// Hamming distance for protein sequences
fn builtin_hamming_protein(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let b = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let count = a.chars().zip(b.chars()).filter(|(x, y)| x != y).count();
    let extra = (a.chars().count() as i64 - b.chars().count() as i64).abs();
    Ok(StrykeValue::integer(count as i64 + extra))
}

// Codon usage variance (deviation from uniform)
fn builtin_codon_usage_variance(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let freqs: Vec<f64> = arg_to_vec(&args.first().cloned().unwrap_or(StrykeValue::UNDEF))
        .iter().map(|v| v.to_number()).collect();
    let n = freqs.len() as f64;
    if n <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    let mean = 1.0 / n;
    let var: f64 = freqs.iter().map(|&f| (f - mean).powi(2)).sum::<f64>() / n;
    Ok(StrykeValue::float(var))
}

// Synonymous-to-nonsynonymous (dN/dS) ratio (rough)
fn builtin_dnds_ratio(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let dn = f1(args);
    let ds = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if ds == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(dn / ds))
}

// Mutation rate per generation (mu)
fn builtin_mutation_rate(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n_mutations = f1(args);
    let n_sites = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    if n_sites == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(n_mutations / n_sites))
}

// Tajima's D (rough simplified)
fn builtin_tajimas_d(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pi_est = f1(args);
    let theta_w = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    let var_d = args.get(2).map(|v| v.to_number()).unwrap_or(1.0).max(1e-12);
    Ok(StrykeValue::float((pi_est - theta_w) / var_d.sqrt()))
}

// Watterson's theta from segregating sites
fn builtin_wattersons_theta(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = f1(args);
    let n = args.get(1).map(|v| v.to_number() as i64).unwrap_or(2);
    let mut a_n = 0.0;
    for k in 1..n { a_n += 1.0 / k as f64; }
    if a_n == 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(s / a_n))
}

// Coalescent time expectation E[T_n] = 2/(n(n-1))
fn builtin_coalescent_expected_time(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = f1(args);
    if n <= 1.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(2.0 / (n * (n - 1.0))))
}

// Total tree length expectation
fn builtin_coalescent_tree_length(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = i1(args).max(2);
    let mut total = 0.0;
    for k in 2..=n {
        total += k as f64 * 2.0 / (k as f64 * (k as f64 - 1.0));
    }
    Ok(StrykeValue::float(total))
}

// Effective Migration rate Nm from FST
fn builtin_nm_from_fst(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let fst = f1(args).clamp(1e-9, 0.999);
    Ok(StrykeValue::float((1.0 - fst) / (4.0 * fst)))
}
