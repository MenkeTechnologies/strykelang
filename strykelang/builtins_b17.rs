//! Bioinformatics, 3D geometry, sequence alignment,
//! file format header parsers, resampling, Markov chains.

use crate::value::StrykeValue;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

fn arg_f64(args: &[StrykeValue], idx: usize) -> Option<f64> {
    args.get(idx).map(|v| v.to_number())
}

fn arg_i64(args: &[StrykeValue], idx: usize) -> Option<i64> {
    args.get(idx).map(|v| v.to_int())
}

fn arg_str(args: &[StrykeValue], idx: usize) -> Option<String> {
    args.get(idx).map(|v| v.as_str_or_empty())
}

fn as_vec_f64(v: &StrykeValue) -> Vec<f64> {
    if let Some(a) = v.as_array_ref() {
        return a.read().iter().map(|x| x.to_number()).collect();
    }
    if let Some(a) = v.as_array_vec() {
        return a.iter().map(|x| x.to_number()).collect();
    }
    Vec::new()
}

fn as_vec_sv(v: &StrykeValue) -> Vec<StrykeValue> {
    if let Some(a) = v.as_array_ref() {
        return a.read().clone();
    }
    if let Some(a) = v.as_array_vec() {
        return a.to_vec();
    }
    Vec::new()
}

fn as_matrix(v: &StrykeValue) -> Vec<Vec<f64>> {
    as_vec_sv(v).iter().map(as_vec_f64).collect()
}

fn arr_sv(v: Vec<StrykeValue>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(v)))
}

fn arr_f64(v: Vec<f64>) -> StrykeValue {
    StrykeValue::array_ref(Arc::new(RwLock::new(
        v.into_iter().map(StrykeValue::float).collect(),
    )))
}

fn matrix_to_sv(m: &[Vec<f64>]) -> StrykeValue {
    arr_sv(m.iter().map(|r| arr_f64(r.clone())).collect())
}

// ══════════════════════════════════════════════════════════════════════
// Bioinformatics
// ══════════════════════════════════════════════════════════════════════

pub fn dna_complement(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let out: String = s
        .chars()
        .map(|c| match c {
            'A' => 'T',
            'T' => 'A',
            'G' => 'C',
            'C' => 'G',
            'a' => 't',
            't' => 'a',
            'g' => 'c',
            'c' => 'g',
            'N' | 'n' => c,
            _ => c,
        })
        .collect();
    StrykeValue::string(out)
}

pub fn dna_reverse_complement(args: &[StrykeValue]) -> StrykeValue {
    let comp = dna_complement(args).as_str_or_empty();
    StrykeValue::string(comp.chars().rev().collect())
}

pub fn dna_transcribe(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::string(s.replace('T', "U").replace('t', "u"))
}

pub fn rna_to_dna(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    StrykeValue::string(s.replace('U', "T").replace('u', "t"))
}

const CODON_TABLE: &[(&str, char)] = &[
    ("TTT", 'F'),
    ("TTC", 'F'),
    ("TTA", 'L'),
    ("TTG", 'L'),
    ("CTT", 'L'),
    ("CTC", 'L'),
    ("CTA", 'L'),
    ("CTG", 'L'),
    ("ATT", 'I'),
    ("ATC", 'I'),
    ("ATA", 'I'),
    ("ATG", 'M'),
    ("GTT", 'V'),
    ("GTC", 'V'),
    ("GTA", 'V'),
    ("GTG", 'V'),
    ("TCT", 'S'),
    ("TCC", 'S'),
    ("TCA", 'S'),
    ("TCG", 'S'),
    ("CCT", 'P'),
    ("CCC", 'P'),
    ("CCA", 'P'),
    ("CCG", 'P'),
    ("ACT", 'T'),
    ("ACC", 'T'),
    ("ACA", 'T'),
    ("ACG", 'T'),
    ("GCT", 'A'),
    ("GCC", 'A'),
    ("GCA", 'A'),
    ("GCG", 'A'),
    ("TAT", 'Y'),
    ("TAC", 'Y'),
    ("TAA", '*'),
    ("TAG", '*'),
    ("CAT", 'H'),
    ("CAC", 'H'),
    ("CAA", 'Q'),
    ("CAG", 'Q'),
    ("AAT", 'N'),
    ("AAC", 'N'),
    ("AAA", 'K'),
    ("AAG", 'K'),
    ("GAT", 'D'),
    ("GAC", 'D'),
    ("GAA", 'E'),
    ("GAG", 'E'),
    ("TGT", 'C'),
    ("TGC", 'C'),
    ("TGA", '*'),
    ("TGG", 'W'),
    ("CGT", 'R'),
    ("CGC", 'R'),
    ("CGA", 'R'),
    ("CGG", 'R'),
    ("AGT", 'S'),
    ("AGC", 'S'),
    ("AGA", 'R'),
    ("AGG", 'R'),
    ("GGT", 'G'),
    ("GGC", 'G'),
    ("GGA", 'G'),
    ("GGG", 'G'),
];

fn codon_map() -> HashMap<&'static str, char> {
    CODON_TABLE.iter().cloned().collect()
}

pub fn codon_to_amino_acid(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    let map = codon_map();
    let aa = map.get(s.as_str()).copied().unwrap_or('?');
    StrykeValue::string(aa.to_string())
}

pub fn dna_translate(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    let map = codon_map();
    let mut out = String::new();
    let chars: Vec<char> = s
        .chars()
        .filter(|c| matches!(c, 'A' | 'C' | 'G' | 'T' | 'N'))
        .collect();
    let mut i = 0;
    while i + 3 <= chars.len() {
        let codon: String = chars[i..i + 3].iter().collect();
        let aa = map.get(codon.as_str()).copied().unwrap_or('?');
        if aa == '*' {
            break;
        }
        out.push(aa);
        i += 3;
    }
    StrykeValue::string(out)
}

pub fn dna_gc_content(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let total = s.chars().filter(|c| c.is_ascii_alphabetic()).count();
    if total == 0 {
        return StrykeValue::float(0.0);
    }
    let gc = s
        .chars()
        .filter(|c| matches!(c.to_ascii_uppercase(), 'G' | 'C'))
        .count();
    StrykeValue::float(gc as f64 / total as f64)
}

pub fn dna_at_content(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let total = s.chars().filter(|c| c.is_ascii_alphabetic()).count();
    if total == 0 {
        return StrykeValue::float(0.0);
    }
    let at = s
        .chars()
        .filter(|c| matches!(c.to_ascii_uppercase(), 'A' | 'T' | 'U'))
        .count();
    StrykeValue::float(at as f64 / total as f64)
}

pub fn dna_melting_temp(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let n = s.chars().filter(|c| c.is_ascii_alphabetic()).count();
    if n == 0 {
        return StrykeValue::float(0.0);
    }
    let gc = s
        .chars()
        .filter(|c| matches!(c.to_ascii_uppercase(), 'G' | 'C'))
        .count();
    let at = n - gc;
    if n < 14 {
        StrykeValue::float((2.0 * at as f64) + (4.0 * gc as f64))
    } else {
        StrykeValue::float(64.9 + 41.0 * (gc as f64 - 16.4) / n as f64)
    }
}

pub fn dna_kmer_count(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    let k = arg_i64(args, 1).unwrap_or(3).max(1) as usize;
    use indexmap::IndexMap;
    let mut counts: IndexMap<String, StrykeValue> = IndexMap::new();
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < k {
        return StrykeValue::hash_ref(Arc::new(RwLock::new(counts)));
    }
    for i in 0..=chars.len() - k {
        let kmer: String = chars[i..i + k].iter().collect();
        let entry = counts
            .entry(kmer)
            .or_insert_with(|| StrykeValue::integer(0));
        let n = entry.to_int();
        *entry = StrykeValue::integer(n + 1);
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(counts)))
}

pub fn dna_kmer_index(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    let k = arg_i64(args, 1).unwrap_or(3).max(1) as usize;
    use indexmap::IndexMap;
    let mut idx: IndexMap<String, StrykeValue> = IndexMap::new();
    let chars: Vec<char> = s.chars().collect();
    if chars.len() < k {
        return StrykeValue::hash_ref(Arc::new(RwLock::new(idx)));
    }
    let mut buckets: HashMap<String, Vec<StrykeValue>> = HashMap::new();
    for i in 0..=chars.len() - k {
        let kmer: String = chars[i..i + k].iter().collect();
        buckets
            .entry(kmer)
            .or_default()
            .push(StrykeValue::integer(i as i64));
    }
    for (k, v) in buckets {
        idx.insert(k, arr_sv(v));
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(idx)))
}

pub fn rna_hamming(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_str(args, 0).unwrap_or_default();
    let b = arg_str(args, 1).unwrap_or_default();
    let count = a.chars().zip(b.chars()).filter(|(x, y)| x != y).count();
    StrykeValue::integer(count as i64)
}

pub fn rna_reverse_complement(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let comp: String = s
        .chars()
        .map(|c| match c {
            'A' => 'U',
            'U' => 'A',
            'G' => 'C',
            'C' => 'G',
            'a' => 'u',
            'u' => 'a',
            'g' => 'c',
            'c' => 'g',
            _ => c,
        })
        .collect();
    StrykeValue::string(comp.chars().rev().collect())
}

const AA_MW: &[(char, f64)] = &[
    ('A', 89.09),
    ('R', 174.20),
    ('N', 132.12),
    ('D', 133.10),
    ('C', 121.16),
    ('Q', 146.15),
    ('E', 147.13),
    ('G', 75.07),
    ('H', 155.16),
    ('I', 131.17),
    ('L', 131.17),
    ('K', 146.19),
    ('M', 149.21),
    ('F', 165.19),
    ('P', 115.13),
    ('S', 105.09),
    ('T', 119.12),
    ('W', 204.23),
    ('Y', 181.19),
    ('V', 117.15),
];

pub fn protein_molecular_weight(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    let map: HashMap<char, f64> = AA_MW.iter().cloned().collect();
    let total: f64 = s.chars().filter_map(|c| map.get(&c).copied()).sum();
    let water = 18.02
        * (s.chars()
            .filter(|c| map.contains_key(c))
            .count()
            .saturating_sub(1)) as f64;
    StrykeValue::float(total - water)
}

const AA_PK: &[(char, [f64; 3])] = &[
    ('D', [2.05, 3.65, 0.0]),
    ('E', [2.10, 4.07, 0.0]),
    ('C', [1.96, 8.18, 0.0]),
    ('Y', [2.18, 10.07, 0.0]),
    ('H', [1.77, 0.0, 6.00]),
    ('K', [2.20, 0.0, 10.54]),
    ('R', [1.82, 0.0, 12.48]),
];

#[allow(non_snake_case)]
pub fn protein_pI(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    let pk: HashMap<char, [f64; 3]> = AA_PK.iter().cloned().collect();
    let mut lo = 0.0_f64;
    let mut hi = 14.0_f64;
    for _ in 0..50 {
        let mid = (lo + hi) / 2.0;
        let mut charge = 0.0_f64;
        for c in s.chars() {
            if let Some(p) = pk.get(&c) {
                if p[1] > 0.0 {
                    charge -= 1.0 / (1.0 + 10f64.powf(p[1] - mid));
                }
                if p[2] > 0.0 {
                    charge += 1.0 / (1.0 + 10f64.powf(mid - p[2]));
                }
            }
        }
        charge += 1.0 / (1.0 + 10f64.powf(9.69 - mid));
        charge -= 1.0 / (1.0 + 10f64.powf(mid - 2.34));
        if charge > 0.0 {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    StrykeValue::float((lo + hi) / 2.0)
}

const AA_KD_HYDROPATHY: &[(char, f64)] = &[
    ('A', 1.8),
    ('R', -4.5),
    ('N', -3.5),
    ('D', -3.5),
    ('C', 2.5),
    ('Q', -3.5),
    ('E', -3.5),
    ('G', -0.4),
    ('H', -3.2),
    ('I', 4.5),
    ('L', 3.8),
    ('K', -3.9),
    ('M', 1.9),
    ('F', 2.8),
    ('P', -1.6),
    ('S', -0.8),
    ('T', -0.7),
    ('W', -0.9),
    ('Y', -1.3),
    ('V', 4.2),
];

pub fn protein_hydrophobicity(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    let map: HashMap<char, f64> = AA_KD_HYDROPATHY.iter().cloned().collect();
    let scores: Vec<f64> = s.chars().filter_map(|c| map.get(&c).copied()).collect();
    if scores.is_empty() {
        return StrykeValue::float(0.0);
    }
    StrykeValue::float(scores.iter().sum::<f64>() / scores.len() as f64)
}

pub fn protein_charge_at_ph(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    let ph = arg_f64(args, 1).unwrap_or(7.0);
    let pk: HashMap<char, [f64; 3]> = AA_PK.iter().cloned().collect();
    let mut charge = 0.0_f64;
    for c in s.chars() {
        if let Some(p) = pk.get(&c) {
            if p[1] > 0.0 {
                charge -= 1.0 / (1.0 + 10f64.powf(p[1] - ph));
            }
            if p[2] > 0.0 {
                charge += 1.0 / (1.0 + 10f64.powf(ph - p[2]));
            }
        }
    }
    charge += 1.0 / (1.0 + 10f64.powf(9.69 - ph));
    charge -= 1.0 / (1.0 + 10f64.powf(ph - 2.34));
    StrykeValue::float(charge)
}

pub fn codon_optimize(args: &[StrykeValue]) -> StrykeValue {
    let protein = arg_str(args, 0).unwrap_or_default().to_uppercase();
    let preferred: HashMap<char, &str> = [
        ('F', "TTC"),
        ('L', "CTG"),
        ('I', "ATC"),
        ('M', "ATG"),
        ('V', "GTG"),
        ('S', "AGC"),
        ('P', "CCG"),
        ('T', "ACC"),
        ('A', "GCC"),
        ('Y', "TAC"),
        ('H', "CAC"),
        ('Q', "CAG"),
        ('N', "AAC"),
        ('K', "AAG"),
        ('D', "GAC"),
        ('E', "GAG"),
        ('C', "TGC"),
        ('W', "TGG"),
        ('R', "CGT"),
        ('G', "GGC"),
        ('*', "TAA"),
    ]
    .into_iter()
    .collect();
    let dna: String = protein
        .chars()
        .filter_map(|aa| preferred.get(&aa).copied())
        .collect();
    StrykeValue::string(dna)
}

pub fn codon_usage_table(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default().to_uppercase();
    use indexmap::IndexMap;
    let mut table: IndexMap<String, StrykeValue> = IndexMap::new();
    let chars: Vec<char> = s
        .chars()
        .filter(|c| matches!(c, 'A' | 'C' | 'G' | 'T'))
        .collect();
    let mut i = 0;
    while i + 3 <= chars.len() {
        let codon: String = chars[i..i + 3].iter().collect();
        let entry = table
            .entry(codon)
            .or_insert_with(|| StrykeValue::integer(0));
        let n = entry.to_int();
        *entry = StrykeValue::integer(n + 1);
        i += 3;
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(table)))
}

// ══════════════════════════════════════════════════════════════════════
// Sequence alignment
// ══════════════════════════════════════════════════════════════════════

pub fn levenshtein_edit_path(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_str(args, 0).unwrap_or_default();
    let b = arg_str(args, 1).unwrap_or_default();
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let m = ac.len();
    let n = bc.len();
    let mut dp = vec![vec![0; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i;
    }
    for j in 0..=n {
        dp[0][j] = j;
    }
    for i in 1..=m {
        for j in 1..=n {
            let cost = if ac[i - 1] == bc[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    let mut path: Vec<StrykeValue> = Vec::new();
    let mut i = m;
    let mut j = n;
    while i > 0 || j > 0 {
        if i > 0
            && j > 0
            && dp[i][j] == dp[i - 1][j - 1] + if ac[i - 1] == bc[j - 1] { 0 } else { 1 }
        {
            let op = if ac[i - 1] == bc[j - 1] {
                "match"
            } else {
                "sub"
            };
            path.push(StrykeValue::string(format!(
                "{op}:{}->{}",
                ac[i - 1],
                bc[j - 1]
            )));
            i -= 1;
            j -= 1;
        } else if i > 0 && dp[i][j] == dp[i - 1][j] + 1 {
            path.push(StrykeValue::string(format!("del:{}", ac[i - 1])));
            i -= 1;
        } else {
            path.push(StrykeValue::string(format!("ins:{}", bc[j - 1])));
            j -= 1;
        }
    }
    path.reverse();
    arr_sv(path)
}

pub fn nw_score(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_str(args, 0).unwrap_or_default();
    let b = arg_str(args, 1).unwrap_or_default();
    let match_score = arg_f64(args, 2).unwrap_or(1.0);
    let mismatch = arg_f64(args, 3).unwrap_or(-1.0);
    let gap = arg_f64(args, 4).unwrap_or(-2.0);
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let m = ac.len();
    let n = bc.len();
    let mut dp = vec![vec![0.0_f64; n + 1]; m + 1];
    for i in 0..=m {
        dp[i][0] = i as f64 * gap;
    }
    for j in 0..=n {
        dp[0][j] = j as f64 * gap;
    }
    for i in 1..=m {
        for j in 1..=n {
            let diag = dp[i - 1][j - 1]
                + if ac[i - 1] == bc[j - 1] {
                    match_score
                } else {
                    mismatch
                };
            let up = dp[i - 1][j] + gap;
            let left = dp[i][j - 1] + gap;
            dp[i][j] = diag.max(up).max(left);
        }
    }
    StrykeValue::float(dp[m][n])
}

pub fn sw_score(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_str(args, 0).unwrap_or_default();
    let b = arg_str(args, 1).unwrap_or_default();
    let match_score = arg_f64(args, 2).unwrap_or(2.0);
    let mismatch = arg_f64(args, 3).unwrap_or(-1.0);
    let gap = arg_f64(args, 4).unwrap_or(-2.0);
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let m = ac.len();
    let n = bc.len();
    let mut dp = vec![vec![0.0_f64; n + 1]; m + 1];
    let mut best = 0.0_f64;
    for i in 1..=m {
        for j in 1..=n {
            let diag = dp[i - 1][j - 1]
                + if ac[i - 1] == bc[j - 1] {
                    match_score
                } else {
                    mismatch
                };
            let up = dp[i - 1][j] + gap;
            let left = dp[i][j - 1] + gap;
            dp[i][j] = diag.max(up).max(left).max(0.0);
            best = best.max(dp[i][j]);
        }
    }
    StrykeValue::float(best)
}

pub fn sequence_identity_pct(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_str(args, 0).unwrap_or_default();
    let b = arg_str(args, 1).unwrap_or_default();
    let len = a.chars().count().min(b.chars().count());
    if len == 0 {
        return StrykeValue::float(0.0);
    }
    let matches = a.chars().zip(b.chars()).filter(|(x, y)| x == y).count();
    StrykeValue::float(100.0 * matches as f64 / len as f64)
}

pub fn sequence_similarity_pct(args: &[StrykeValue]) -> StrykeValue {
    // For proteins: use BLOSUM-like grouping for similar amino acids
    let a = arg_str(args, 0).unwrap_or_default().to_uppercase();
    let b = arg_str(args, 1).unwrap_or_default().to_uppercase();
    let len = a.chars().count().min(b.chars().count());
    if len == 0 {
        return StrykeValue::float(0.0);
    }
    let group = |c: char| -> usize {
        match c {
            'A' | 'V' | 'L' | 'I' | 'M' => 0,       // aliphatic
            'F' | 'W' | 'Y' => 1,                   // aromatic
            'S' | 'T' | 'C' | 'P' | 'N' | 'Q' => 2, // polar
            'K' | 'R' | 'H' => 3,                   // basic
            'D' | 'E' => 4,                         // acidic
            'G' => 5,
            _ => 99,
        }
    };
    let similar = a
        .chars()
        .zip(b.chars())
        .filter(|(x, y)| x == y || (group(*x) == group(*y) && group(*x) != 99))
        .count();
    StrykeValue::float(100.0 * similar as f64 / len as f64)
}

// ══════════════════════════════════════════════════════════════════════
// 3D geometry: vectors, matrices, quaternions
// ══════════════════════════════════════════════════════════════════════

fn unpack_vec3(v: &StrykeValue) -> [f64; 3] {
    let xs = as_vec_f64(v);
    [
        xs.first().copied().unwrap_or(0.0),
        xs.get(1).copied().unwrap_or(0.0),
        xs.get(2).copied().unwrap_or(0.0),
    ]
}

fn pack_vec3(v: [f64; 3]) -> StrykeValue {
    arr_f64(v.to_vec())
}

fn unpack_vec4(v: &StrykeValue) -> [f64; 4] {
    let xs = as_vec_f64(v);
    [
        xs.first().copied().unwrap_or(0.0),
        xs.get(1).copied().unwrap_or(0.0),
        xs.get(2).copied().unwrap_or(0.0),
        xs.get(3).copied().unwrap_or(0.0),
    ]
}

fn pack_vec4(v: [f64; 4]) -> StrykeValue {
    arr_f64(v.to_vec())
}

pub fn vec3_add(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    pack_vec3([a[0] + b[0], a[1] + b[1], a[2] + b[2]])
}

pub fn vec3_sub(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    pack_vec3([a[0] - b[0], a[1] - b[1], a[2] - b[2]])
}

pub fn vec3_scale(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let s = arg_f64(args, 1).unwrap_or(1.0);
    pack_vec3([a[0] * s, a[1] * s, a[2] * s])
}

pub fn vec3_dot(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    StrykeValue::float(a[0] * b[0] + a[1] * b[1] + a[2] * b[2])
}

pub fn vec3_cross(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    pack_vec3([
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ])
}

pub fn vec3_length(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    StrykeValue::float((a[0].powi(2) + a[1].powi(2) + a[2].powi(2)).sqrt())
}

pub fn vec3_normalize(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let l = (a[0].powi(2) + a[1].powi(2) + a[2].powi(2)).sqrt();
    if l < 1e-12 {
        return pack_vec3([0.0; 3]);
    }
    pack_vec3([a[0] / l, a[1] / l, a[2] / l])
}

pub fn vec3_distance(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    StrykeValue::float(
        ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt(),
    )
}

pub fn vec3_lerp(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let t = arg_f64(args, 2).unwrap_or(0.5).clamp(0.0, 1.0);
    pack_vec3([
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ])
}

pub fn vec3_reflect(args: &[StrykeValue]) -> StrykeValue {
    let v = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let n = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let dot2 = 2.0 * (v[0] * n[0] + v[1] * n[1] + v[2] * n[2]);
    pack_vec3([v[0] - dot2 * n[0], v[1] - dot2 * n[1], v[2] - dot2 * n[2]])
}

pub fn vec3_refract(args: &[StrykeValue]) -> StrykeValue {
    let v = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let n = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let eta = arg_f64(args, 2).unwrap_or(1.0);
    let dot = v[0] * n[0] + v[1] * n[1] + v[2] * n[2];
    let k = 1.0 - eta * eta * (1.0 - dot * dot);
    if k < 0.0 {
        return pack_vec3([0.0; 3]);
    }
    let f = eta * dot + k.sqrt();
    pack_vec3([
        eta * v[0] - f * n[0],
        eta * v[1] - f * n[1],
        eta * v[2] - f * n[2],
    ])
}

pub fn vec3_project(args: &[StrykeValue]) -> StrykeValue {
    let v = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let u = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let denom = u[0] * u[0] + u[1] * u[1] + u[2] * u[2];
    if denom < 1e-12 {
        return pack_vec3([0.0; 3]);
    }
    let s = (v[0] * u[0] + v[1] * u[1] + v[2] * u[2]) / denom;
    pack_vec3([s * u[0], s * u[1], s * u[2]])
}

pub fn vec4_add(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec4(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_vec4(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    pack_vec4([a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]])
}

pub fn vec4_sub(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec4(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_vec4(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    pack_vec4([a[0] - b[0], a[1] - b[1], a[2] - b[2], a[3] - b[3]])
}

pub fn vec4_scale(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec4(args.first().unwrap_or(&StrykeValue::UNDEF));
    let s = arg_f64(args, 1).unwrap_or(1.0);
    pack_vec4([a[0] * s, a[1] * s, a[2] * s, a[3] * s])
}

pub fn vec4_dot(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec4(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_vec4(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    StrykeValue::float(a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3])
}

pub fn vec4_length(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec4(args.first().unwrap_or(&StrykeValue::UNDEF));
    StrykeValue::float((a[0].powi(2) + a[1].powi(2) + a[2].powi(2) + a[3].powi(2)).sqrt())
}

// 4x4 matrix as flat 16-element array or 4-row nested array.
fn mat4_to_flat(m: &StrykeValue) -> [f64; 16] {
    let nested = as_matrix(m);
    if nested.len() == 4 && nested[0].len() == 4 {
        let mut out = [0.0; 16];
        for i in 0..4 {
            for j in 0..4 {
                out[i * 4 + j] = nested[i][j];
            }
        }
        return out;
    }
    let flat = as_vec_f64(m);
    let mut out = [0.0; 16];
    for (i, v) in flat.iter().take(16).enumerate() {
        out[i] = *v;
    }
    out
}

fn flat_to_mat4(m: [f64; 16]) -> StrykeValue {
    let rows: Vec<Vec<f64>> = (0..4).map(|i| m[i * 4..i * 4 + 4].to_vec()).collect();
    matrix_to_sv(&rows)
}

pub fn mat4_identity(_args: &[StrykeValue]) -> StrykeValue {
    let mut m = [0.0; 16];
    for i in 0..4 {
        m[i * 4 + i] = 1.0;
    }
    flat_to_mat4(m)
}

pub fn mat4_translate(args: &[StrykeValue]) -> StrykeValue {
    let v = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let mut m = [0.0; 16];
    for i in 0..4 {
        m[i * 4 + i] = 1.0;
    }
    m[3] = v[0];
    m[7] = v[1];
    m[11] = v[2];
    flat_to_mat4(m)
}

pub fn mat4_scale(args: &[StrykeValue]) -> StrykeValue {
    let v = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let mut m = [0.0; 16];
    m[0] = v[0];
    m[5] = v[1];
    m[10] = v[2];
    m[15] = 1.0;
    flat_to_mat4(m)
}

pub fn mat4_rotate_x(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_f64(args, 0).unwrap_or(0.0);
    let (s, c) = (a.sin(), a.cos());
    let mut m = [0.0; 16];
    m[0] = 1.0;
    m[5] = c;
    m[6] = -s;
    m[9] = s;
    m[10] = c;
    m[15] = 1.0;
    flat_to_mat4(m)
}

pub fn mat4_rotate_y(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_f64(args, 0).unwrap_or(0.0);
    let (s, c) = (a.sin(), a.cos());
    let mut m = [0.0; 16];
    m[0] = c;
    m[2] = s;
    m[5] = 1.0;
    m[8] = -s;
    m[10] = c;
    m[15] = 1.0;
    flat_to_mat4(m)
}

pub fn mat4_rotate_z(args: &[StrykeValue]) -> StrykeValue {
    let a = arg_f64(args, 0).unwrap_or(0.0);
    let (s, c) = (a.sin(), a.cos());
    let mut m = [0.0; 16];
    m[0] = c;
    m[1] = -s;
    m[4] = s;
    m[5] = c;
    m[10] = 1.0;
    m[15] = 1.0;
    flat_to_mat4(m)
}

pub fn mat4_rotate_axis(args: &[StrykeValue]) -> StrykeValue {
    let axis = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let angle = arg_f64(args, 1).unwrap_or(0.0);
    let len = (axis[0].powi(2) + axis[1].powi(2) + axis[2].powi(2)).sqrt();
    if len < 1e-12 {
        return mat4_identity(args);
    }
    let x = axis[0] / len;
    let y = axis[1] / len;
    let z = axis[2] / len;
    let (s, c) = (angle.sin(), angle.cos());
    let t = 1.0 - c;
    let m = [
        t * x * x + c,
        t * x * y - s * z,
        t * x * z + s * y,
        0.0,
        t * x * y + s * z,
        t * y * y + c,
        t * y * z - s * x,
        0.0,
        t * x * z - s * y,
        t * y * z + s * x,
        t * z * z + c,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ];
    flat_to_mat4(m)
}

pub fn mat4_perspective(args: &[StrykeValue]) -> StrykeValue {
    let fov_y = arg_f64(args, 0).unwrap_or(std::f64::consts::FRAC_PI_2);
    let aspect = arg_f64(args, 1).unwrap_or(1.0);
    let near = arg_f64(args, 2).unwrap_or(0.1);
    let far = arg_f64(args, 3).unwrap_or(100.0);
    let f = 1.0 / (fov_y / 2.0).tan();
    let mut m = [0.0; 16];
    m[0] = f / aspect;
    m[5] = f;
    m[10] = (far + near) / (near - far);
    m[11] = (2.0 * far * near) / (near - far);
    m[14] = -1.0;
    flat_to_mat4(m)
}

pub fn mat4_orthographic(args: &[StrykeValue]) -> StrykeValue {
    let l = arg_f64(args, 0).unwrap_or(-1.0);
    let r = arg_f64(args, 1).unwrap_or(1.0);
    let b = arg_f64(args, 2).unwrap_or(-1.0);
    let t = arg_f64(args, 3).unwrap_or(1.0);
    let n = arg_f64(args, 4).unwrap_or(-1.0);
    let f = arg_f64(args, 5).unwrap_or(1.0);
    let mut m = [0.0; 16];
    m[0] = 2.0 / (r - l);
    m[5] = 2.0 / (t - b);
    m[10] = -2.0 / (f - n);
    m[3] = -(r + l) / (r - l);
    m[7] = -(t + b) / (t - b);
    m[11] = -(f + n) / (f - n);
    m[15] = 1.0;
    flat_to_mat4(m)
}

pub fn mat4_look_at(args: &[StrykeValue]) -> StrykeValue {
    let eye = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let center = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let up = unpack_vec3(args.get(2).unwrap_or(&StrykeValue::UNDEF));
    let f = {
        let fx = center[0] - eye[0];
        let fy = center[1] - eye[1];
        let fz = center[2] - eye[2];
        let l = (fx * fx + fy * fy + fz * fz).sqrt().max(1e-12);
        [fx / l, fy / l, fz / l]
    };
    let s = {
        let sx = f[1] * up[2] - f[2] * up[1];
        let sy = f[2] * up[0] - f[0] * up[2];
        let sz = f[0] * up[1] - f[1] * up[0];
        let l = (sx * sx + sy * sy + sz * sz).sqrt().max(1e-12);
        [sx / l, sy / l, sz / l]
    };
    let u = [
        s[1] * f[2] - s[2] * f[1],
        s[2] * f[0] - s[0] * f[2],
        s[0] * f[1] - s[1] * f[0],
    ];
    let m = [
        s[0],
        s[1],
        s[2],
        -(s[0] * eye[0] + s[1] * eye[1] + s[2] * eye[2]),
        u[0],
        u[1],
        u[2],
        -(u[0] * eye[0] + u[1] * eye[1] + u[2] * eye[2]),
        -f[0],
        -f[1],
        -f[2],
        f[0] * eye[0] + f[1] * eye[1] + f[2] * eye[2],
        0.0,
        0.0,
        0.0,
        1.0,
    ];
    flat_to_mat4(m)
}

pub fn mat4_multiply(args: &[StrykeValue]) -> StrykeValue {
    let a = mat4_to_flat(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = mat4_to_flat(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let mut out = [0.0; 16];
    for i in 0..4 {
        for j in 0..4 {
            let mut s = 0.0;
            for k in 0..4 {
                s += a[i * 4 + k] * b[k * 4 + j];
            }
            out[i * 4 + j] = s;
        }
    }
    flat_to_mat4(out)
}

pub fn mat4_transpose(args: &[StrykeValue]) -> StrykeValue {
    let a = mat4_to_flat(args.first().unwrap_or(&StrykeValue::UNDEF));
    let mut out = [0.0; 16];
    for i in 0..4 {
        for j in 0..4 {
            out[i * 4 + j] = a[j * 4 + i];
        }
    }
    flat_to_mat4(out)
}

fn mat4_det_inner(m: &[f64; 16]) -> f64 {
    let a = m;
    a[0] * (a[5] * (a[10] * a[15] - a[11] * a[14]) - a[6] * (a[9] * a[15] - a[11] * a[13])
        + a[7] * (a[9] * a[14] - a[10] * a[13]))
        - a[1]
            * (a[4] * (a[10] * a[15] - a[11] * a[14]) - a[6] * (a[8] * a[15] - a[11] * a[12])
                + a[7] * (a[8] * a[14] - a[10] * a[12]))
        + a[2]
            * (a[4] * (a[9] * a[15] - a[11] * a[13]) - a[5] * (a[8] * a[15] - a[11] * a[12])
                + a[7] * (a[8] * a[13] - a[9] * a[12]))
        - a[3]
            * (a[4] * (a[9] * a[14] - a[10] * a[13]) - a[5] * (a[8] * a[14] - a[10] * a[12])
                + a[6] * (a[8] * a[13] - a[9] * a[12]))
}

pub fn mat4_determinant(args: &[StrykeValue]) -> StrykeValue {
    let m = mat4_to_flat(args.first().unwrap_or(&StrykeValue::UNDEF));
    StrykeValue::float(mat4_det_inner(&m))
}

pub fn mat4_inverse(args: &[StrykeValue]) -> StrykeValue {
    let m = mat4_to_flat(args.first().unwrap_or(&StrykeValue::UNDEF));
    let det = mat4_det_inner(&m);
    if det.abs() < 1e-12 {
        return mat4_identity(args);
    }
    let mut inv = [0.0; 16];
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
        + m[9] * m[7] * m[14]
        + m[13] * m[6] * m[11]
        - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
        - m[8] * m[7] * m[14]
        - m[12] * m[6] * m[11]
        + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
        + m[8] * m[7] * m[13]
        + m[12] * m[5] * m[11]
        - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
        - m[8] * m[6] * m[13]
        - m[12] * m[5] * m[10]
        + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
        - m[9] * m[3] * m[14]
        - m[13] * m[2] * m[11]
        + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
        + m[8] * m[3] * m[14]
        + m[12] * m[2] * m[11]
        - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
        - m[8] * m[3] * m[13]
        - m[12] * m[1] * m[11]
        + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
        + m[8] * m[2] * m[13]
        + m[12] * m[1] * m[10]
        - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
        + m[5] * m[3] * m[14]
        + m[13] * m[2] * m[7]
        - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
        - m[4] * m[3] * m[14]
        - m[12] * m[2] * m[7]
        + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
        + m[4] * m[3] * m[13]
        + m[12] * m[1] * m[7]
        - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
        - m[4] * m[2] * m[13]
        - m[12] * m[1] * m[6]
        + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
        - m[5] * m[3] * m[10]
        - m[9] * m[2] * m[7]
        + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
        + m[4] * m[3] * m[10]
        + m[8] * m[2] * m[7]
        - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
        - m[4] * m[3] * m[9]
        - m[8] * m[1] * m[7]
        + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
        + m[4] * m[2] * m[9]
        + m[8] * m[1] * m[6]
        - m[8] * m[2] * m[5];
    for v in &mut inv {
        *v /= det;
    }
    flat_to_mat4(inv)
}

// Quaternions: stored as [w, x, y, z]
fn unpack_quat(v: &StrykeValue) -> [f64; 4] {
    unpack_vec4(v)
}

fn pack_quat(q: [f64; 4]) -> StrykeValue {
    pack_vec4(q)
}

pub fn quat_identity(_args: &[StrykeValue]) -> StrykeValue {
    pack_quat([1.0, 0.0, 0.0, 0.0])
}

pub fn quat_from_euler(args: &[StrykeValue]) -> StrykeValue {
    let roll = arg_f64(args, 0).unwrap_or(0.0);
    let pitch = arg_f64(args, 1).unwrap_or(0.0);
    let yaw = arg_f64(args, 2).unwrap_or(0.0);
    let (cr, sr) = ((roll * 0.5).cos(), (roll * 0.5).sin());
    let (cp, sp) = ((pitch * 0.5).cos(), (pitch * 0.5).sin());
    let (cy, sy) = ((yaw * 0.5).cos(), (yaw * 0.5).sin());
    pack_quat([
        cr * cp * cy + sr * sp * sy,
        sr * cp * cy - cr * sp * sy,
        cr * sp * cy + sr * cp * sy,
        cr * cp * sy - sr * sp * cy,
    ])
}

pub fn quat_to_euler(args: &[StrykeValue]) -> StrykeValue {
    let q = unpack_quat(args.first().unwrap_or(&StrykeValue::UNDEF));
    let (w, x, y, z) = (q[0], q[1], q[2], q[3]);
    let roll = (2.0 * (w * x + y * z)).atan2(1.0 - 2.0 * (x * x + y * y));
    let sinp = 2.0 * (w * y - z * x);
    let pitch = if sinp.abs() >= 1.0 {
        std::f64::consts::FRAC_PI_2.copysign(sinp)
    } else {
        sinp.asin()
    };
    let yaw = (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z));
    arr_f64(vec![roll, pitch, yaw])
}

pub fn quat_multiply(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_quat(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_quat(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    pack_quat([
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ])
}

pub fn quat_normalize(args: &[StrykeValue]) -> StrykeValue {
    let q = unpack_quat(args.first().unwrap_or(&StrykeValue::UNDEF));
    let len = (q[0].powi(2) + q[1].powi(2) + q[2].powi(2) + q[3].powi(2)).sqrt();
    if len < 1e-12 {
        return pack_quat([1.0, 0.0, 0.0, 0.0]);
    }
    pack_quat([q[0] / len, q[1] / len, q[2] / len, q[3] / len])
}

pub fn quat_conjugate(args: &[StrykeValue]) -> StrykeValue {
    let q = unpack_quat(args.first().unwrap_or(&StrykeValue::UNDEF));
    pack_quat([q[0], -q[1], -q[2], -q[3]])
}

pub fn quat_inverse(args: &[StrykeValue]) -> StrykeValue {
    let q = unpack_quat(args.first().unwrap_or(&StrykeValue::UNDEF));
    let n = q[0].powi(2) + q[1].powi(2) + q[2].powi(2) + q[3].powi(2);
    if n < 1e-12 {
        return pack_quat([1.0, 0.0, 0.0, 0.0]);
    }
    pack_quat([q[0] / n, -q[1] / n, -q[2] / n, -q[3] / n])
}

pub fn quat_dot(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_quat(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_quat(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    StrykeValue::float(a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3])
}

pub fn quat_to_mat4(args: &[StrykeValue]) -> StrykeValue {
    let q = unpack_quat(args.first().unwrap_or(&StrykeValue::UNDEF));
    let (w, x, y, z) = (q[0], q[1], q[2], q[3]);
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let xz = x * z;
    let yz = y * z;
    let wx = w * x;
    let wy = w * y;
    let wz = w * z;
    let m = [
        1.0 - 2.0 * (yy + zz),
        2.0 * (xy - wz),
        2.0 * (xz + wy),
        0.0,
        2.0 * (xy + wz),
        1.0 - 2.0 * (xx + zz),
        2.0 * (yz - wx),
        0.0,
        2.0 * (xz - wy),
        2.0 * (yz + wx),
        1.0 - 2.0 * (xx + yy),
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
    ];
    flat_to_mat4(m)
}

// ══════════════════════════════════════════════════════════════════════
// AABB / ray / sphere / plane
// ══════════════════════════════════════════════════════════════════════

pub fn aabb_new(args: &[StrykeValue]) -> StrykeValue {
    let min = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let max = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    arr_sv(vec![pack_vec3(min), pack_vec3(max)])
}

pub fn aabb_contains_point(args: &[StrykeValue]) -> StrykeValue {
    let aabb = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF));
    if aabb.len() < 2 {
        return StrykeValue::integer(0);
    }
    let min = unpack_vec3(&aabb[0]);
    let max = unpack_vec3(&aabb[1]);
    let p = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let inside = p[0] >= min[0]
        && p[0] <= max[0]
        && p[1] >= min[1]
        && p[1] <= max[1]
        && p[2] >= min[2]
        && p[2] <= max[2];
    StrykeValue::integer(if inside { 1 } else { 0 })
}

pub fn aabb_intersects(args: &[StrykeValue]) -> StrykeValue {
    let a = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = as_vec_sv(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    if a.len() < 2 || b.len() < 2 {
        return StrykeValue::integer(0);
    }
    let amin = unpack_vec3(&a[0]);
    let amax = unpack_vec3(&a[1]);
    let bmin = unpack_vec3(&b[0]);
    let bmax = unpack_vec3(&b[1]);
    let hit = amin[0] <= bmax[0]
        && amax[0] >= bmin[0]
        && amin[1] <= bmax[1]
        && amax[1] >= bmin[1]
        && amin[2] <= bmax[2]
        && amax[2] >= bmin[2];
    StrykeValue::integer(if hit { 1 } else { 0 })
}

pub fn aabb_union(args: &[StrykeValue]) -> StrykeValue {
    let a = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = as_vec_sv(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    if a.len() < 2 || b.len() < 2 {
        return arr_sv(vec![]);
    }
    let amin = unpack_vec3(&a[0]);
    let amax = unpack_vec3(&a[1]);
    let bmin = unpack_vec3(&b[0]);
    let bmax = unpack_vec3(&b[1]);
    arr_sv(vec![
        pack_vec3([
            amin[0].min(bmin[0]),
            amin[1].min(bmin[1]),
            amin[2].min(bmin[2]),
        ]),
        pack_vec3([
            amax[0].max(bmax[0]),
            amax[1].max(bmax[1]),
            amax[2].max(bmax[2]),
        ]),
    ])
}

pub fn aabb_volume(args: &[StrykeValue]) -> StrykeValue {
    let aabb = as_vec_sv(args.first().unwrap_or(&StrykeValue::UNDEF));
    if aabb.len() < 2 {
        return StrykeValue::float(0.0);
    }
    let min = unpack_vec3(&aabb[0]);
    let max = unpack_vec3(&aabb[1]);
    StrykeValue::float((max[0] - min[0]) * (max[1] - min[1]) * (max[2] - min[2]))
}

pub fn ray_aabb_intersect(args: &[StrykeValue]) -> StrykeValue {
    let origin = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let dir = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let aabb = as_vec_sv(args.get(2).unwrap_or(&StrykeValue::UNDEF));
    if aabb.len() < 2 {
        return StrykeValue::float(-1.0);
    }
    let min = unpack_vec3(&aabb[0]);
    let max = unpack_vec3(&aabb[1]);
    let mut tmin = f64::NEG_INFINITY;
    let mut tmax = f64::INFINITY;
    for i in 0..3 {
        if dir[i].abs() < 1e-12 {
            if origin[i] < min[i] || origin[i] > max[i] {
                return StrykeValue::float(-1.0);
            }
        } else {
            let t1 = (min[i] - origin[i]) / dir[i];
            let t2 = (max[i] - origin[i]) / dir[i];
            let (lo, hi) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
            tmin = tmin.max(lo);
            tmax = tmax.min(hi);
        }
    }
    if tmin > tmax || tmax < 0.0 {
        return StrykeValue::float(-1.0);
    }
    StrykeValue::float(if tmin >= 0.0 { tmin } else { tmax })
}

pub fn ray_plane_intersect(args: &[StrykeValue]) -> StrykeValue {
    let origin = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let dir = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let point = unpack_vec3(args.get(2).unwrap_or(&StrykeValue::UNDEF));
    let normal = unpack_vec3(args.get(3).unwrap_or(&StrykeValue::UNDEF));
    let denom = dir[0] * normal[0] + dir[1] * normal[1] + dir[2] * normal[2];
    if denom.abs() < 1e-12 {
        return StrykeValue::float(-1.0);
    }
    let diff = [
        point[0] - origin[0],
        point[1] - origin[1],
        point[2] - origin[2],
    ];
    let t = (diff[0] * normal[0] + diff[1] * normal[1] + diff[2] * normal[2]) / denom;
    StrykeValue::float(if t >= 0.0 { t } else { -1.0 })
}

pub fn sphere_aabb_intersect(args: &[StrykeValue]) -> StrykeValue {
    let center = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let radius = arg_f64(args, 1).unwrap_or(0.0);
    let aabb = as_vec_sv(args.get(2).unwrap_or(&StrykeValue::UNDEF));
    if aabb.len() < 2 {
        return StrykeValue::integer(0);
    }
    let min = unpack_vec3(&aabb[0]);
    let max = unpack_vec3(&aabb[1]);
    let mut d2 = 0.0;
    for i in 0..3 {
        let v = center[i];
        if v < min[i] {
            d2 += (min[i] - v).powi(2);
        } else if v > max[i] {
            d2 += (v - max[i]).powi(2);
        }
    }
    StrykeValue::integer(if d2 <= radius * radius { 1 } else { 0 })
}

pub fn sphere_sphere_intersect(args: &[StrykeValue]) -> StrykeValue {
    let c1 = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let r1 = arg_f64(args, 1).unwrap_or(0.0);
    let c2 = unpack_vec3(args.get(2).unwrap_or(&StrykeValue::UNDEF));
    let r2 = arg_f64(args, 3).unwrap_or(0.0);
    let dx = c1[0] - c2[0];
    let dy = c1[1] - c2[1];
    let dz = c1[2] - c2[2];
    let d2 = dx * dx + dy * dy + dz * dz;
    let r = r1 + r2;
    StrykeValue::integer(if d2 <= r * r { 1 } else { 0 })
}

pub fn plane_distance_to_point(args: &[StrykeValue]) -> StrykeValue {
    let point = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let normal = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let plane_point = unpack_vec3(args.get(2).unwrap_or(&StrykeValue::UNDEF));
    let n_len = (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
    if n_len < 1e-12 {
        return StrykeValue::float(0.0);
    }
    let diff = [
        point[0] - plane_point[0],
        point[1] - plane_point[1],
        point[2] - plane_point[2],
    ];
    let dot = diff[0] * normal[0] + diff[1] * normal[1] + diff[2] * normal[2];
    StrykeValue::float(dot / n_len)
}

pub fn plane_normalize(args: &[StrykeValue]) -> StrykeValue {
    let normal = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let len = (normal[0].powi(2) + normal[1].powi(2) + normal[2].powi(2)).sqrt();
    if len < 1e-12 {
        return pack_vec3([0.0; 3]);
    }
    pack_vec3([normal[0] / len, normal[1] / len, normal[2] / len])
}

pub fn triangle_normal(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let c = unpack_vec3(args.get(2).unwrap_or(&StrykeValue::UNDEF));
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let len = (n[0].powi(2) + n[1].powi(2) + n[2].powi(2)).sqrt();
    if len < 1e-12 {
        return pack_vec3([0.0; 3]);
    }
    pack_vec3([n[0] / len, n[1] / len, n[2] / len])
}

pub fn triangle_area_3d(args: &[StrykeValue]) -> StrykeValue {
    let a = unpack_vec3(args.first().unwrap_or(&StrykeValue::UNDEF));
    let b = unpack_vec3(args.get(1).unwrap_or(&StrykeValue::UNDEF));
    let c = unpack_vec3(args.get(2).unwrap_or(&StrykeValue::UNDEF));
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    StrykeValue::float(0.5 * (cross[0].powi(2) + cross[1].powi(2) + cross[2].powi(2)).sqrt())
}

// ══════════════════════════════════════════════════════════════════════
// File format header parsers (byte-array input as space-separated hex)
// ══════════════════════════════════════════════════════════════════════

fn parse_hex_bytes(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let cleaned: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let bytes = cleaned.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&bytes[i..i + 2]).unwrap_or(""), 16) {
            out.push(v);
        }
        i += 2;
    }
    out
}

fn make_hash(pairs: Vec<(&str, StrykeValue)>) -> StrykeValue {
    use indexmap::IndexMap;
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    for (k, v) in pairs {
        h.insert(k.to_string(), v);
    }
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

pub fn bmp_header_read(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = parse_hex_bytes(&s);
    if bytes.len() < 54 || bytes[0] != b'B' || bytes[1] != b'M' {
        return StrykeValue::UNDEF;
    }
    let width = i32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
    let height = i32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
    let bpp = u16::from_le_bytes([bytes[28], bytes[29]]);
    make_hash(vec![
        ("format", StrykeValue::string("BMP".into())),
        ("width", StrykeValue::integer(width as i64)),
        ("height", StrykeValue::integer(height as i64)),
        ("bits_per_pixel", StrykeValue::integer(bpp as i64)),
    ])
}

pub fn png_header_read(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = parse_hex_bytes(&s);
    if bytes.len() < 24 || bytes[..8] != [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A] {
        return StrykeValue::UNDEF;
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    let bit_depth = bytes.get(24).copied().unwrap_or(0);
    let color_type = bytes.get(25).copied().unwrap_or(0);
    make_hash(vec![
        ("format", StrykeValue::string("PNG".into())),
        ("width", StrykeValue::integer(width as i64)),
        ("height", StrykeValue::integer(height as i64)),
        ("bit_depth", StrykeValue::integer(bit_depth as i64)),
        ("color_type", StrykeValue::integer(color_type as i64)),
    ])
}

pub fn jpeg_markers(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = parse_hex_bytes(&s);
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return arr_sv(vec![]);
    }
    let mut out: Vec<StrykeValue> = Vec::new();
    let mut i = 2;
    while i + 1 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        out.push(StrykeValue::string(format!("FF{:02X}", marker)));
        if marker == 0xD9 {
            break;
        }
        i += 2;
        if (0xD0..=0xD9).contains(&marker) {
            continue;
        }
        if i + 1 < bytes.len() {
            let seg_len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
            i += seg_len;
        }
    }
    arr_sv(out)
}

pub fn wav_header_read(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = parse_hex_bytes(&s);
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return StrykeValue::UNDEF;
    }
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]);
    let sample_rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    let bits_per_sample = u16::from_le_bytes([bytes[34], bytes[35]]);
    make_hash(vec![
        ("format", StrykeValue::string("WAV".into())),
        ("channels", StrykeValue::integer(channels as i64)),
        ("sample_rate", StrykeValue::integer(sample_rate as i64)),
        (
            "bits_per_sample",
            StrykeValue::integer(bits_per_sample as i64),
        ),
    ])
}

pub fn gif_header_read(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = parse_hex_bytes(&s);
    if bytes.len() < 13 || &bytes[0..3] != b"GIF" {
        return StrykeValue::UNDEF;
    }
    let version = String::from_utf8_lossy(&bytes[3..6]).to_string();
    let width = u16::from_le_bytes([bytes[6], bytes[7]]);
    let height = u16::from_le_bytes([bytes[8], bytes[9]]);
    make_hash(vec![
        ("format", StrykeValue::string("GIF".into())),
        ("version", StrykeValue::string(version)),
        ("width", StrykeValue::integer(width as i64)),
        ("height", StrykeValue::integer(height as i64)),
    ])
}

pub fn zip_central_directory(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = parse_hex_bytes(&s);
    // Find EOCD signature: 0x06054b50 little-endian
    let sig = [0x50u8, 0x4B, 0x05, 0x06];
    let n = bytes.len();
    if n < 22 {
        return StrykeValue::UNDEF;
    }
    for i in (0..=n - 22).rev() {
        if bytes[i..i + 4] == sig {
            let entries = u16::from_le_bytes([bytes[i + 10], bytes[i + 11]]);
            let cd_size =
                u32::from_le_bytes([bytes[i + 12], bytes[i + 13], bytes[i + 14], bytes[i + 15]]);
            let cd_offset =
                u32::from_le_bytes([bytes[i + 16], bytes[i + 17], bytes[i + 18], bytes[i + 19]]);
            return make_hash(vec![
                ("entries", StrykeValue::integer(entries as i64)),
                ("cd_size", StrykeValue::integer(cd_size as i64)),
                ("cd_offset", StrykeValue::integer(cd_offset as i64)),
            ]);
        }
    }
    StrykeValue::UNDEF
}

pub fn zip_local_file_header(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = parse_hex_bytes(&s);
    if bytes.len() < 30 || bytes[..4] != [0x50, 0x4B, 0x03, 0x04] {
        return StrykeValue::UNDEF;
    }
    let compression = u16::from_le_bytes([bytes[8], bytes[9]]);
    let crc32 = u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]);
    let compressed_size = u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
    let uncompressed_size = u32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]);
    make_hash(vec![
        ("compression", StrykeValue::integer(compression as i64)),
        ("crc32", StrykeValue::integer(crc32 as i64)),
        (
            "compressed_size",
            StrykeValue::integer(compressed_size as i64),
        ),
        (
            "uncompressed_size",
            StrykeValue::integer(uncompressed_size as i64),
        ),
    ])
}

pub fn tar_header_read(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = parse_hex_bytes(&s);
    if bytes.len() < 512 {
        return StrykeValue::UNDEF;
    }
    let name = String::from_utf8_lossy(&bytes[..100])
        .trim_end_matches('\0')
        .to_string();
    let size_str = String::from_utf8_lossy(&bytes[124..136])
        .trim_end_matches('\0')
        .to_string();
    let size = i64::from_str_radix(size_str.trim(), 8).unwrap_or(0);
    make_hash(vec![
        ("name", StrykeValue::string(name)),
        ("size", StrykeValue::integer(size)),
    ])
}

pub fn ico_header_read(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = parse_hex_bytes(&s);
    if bytes.len() < 6 {
        return StrykeValue::UNDEF;
    }
    let typ = u16::from_le_bytes([bytes[2], bytes[3]]);
    let count = u16::from_le_bytes([bytes[4], bytes[5]]);
    make_hash(vec![
        ("type", StrykeValue::integer(typ as i64)),
        ("image_count", StrykeValue::integer(count as i64)),
    ])
}

pub fn elf_header_read(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = parse_hex_bytes(&s);
    if bytes.len() < 24 || bytes[..4] != [0x7F, 0x45, 0x4C, 0x46] {
        return StrykeValue::UNDEF;
    }
    let class = bytes[4]; // 1=32-bit, 2=64-bit
    let endianness = bytes[5]; // 1=little, 2=big
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    make_hash(vec![
        ("class", StrykeValue::integer(class as i64)),
        ("endianness", StrykeValue::integer(endianness as i64)),
        ("machine", StrykeValue::integer(machine as i64)),
    ])
}

pub fn mach_o_header_read(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args, 0).unwrap_or_default();
    let bytes = parse_hex_bytes(&s);
    if bytes.len() < 28 {
        return StrykeValue::UNDEF;
    }
    let magic = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let valid = matches!(magic, 0xFEEDFACE | 0xCEFAEDFE | 0xFEEDFACF | 0xCFFAEDFE);
    if !valid {
        return StrykeValue::UNDEF;
    }
    let cputype = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let filetype = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    make_hash(vec![
        ("magic", StrykeValue::integer(magic as i64)),
        ("cputype", StrykeValue::integer(cputype as i64)),
        ("filetype", StrykeValue::integer(filetype as i64)),
    ])
}

// ══════════════════════════════════════════════════════════════════════
// Resampling and Markov chains
// ══════════════════════════════════════════════════════════════════════

pub fn bootstrap_resample(args: &[StrykeValue]) -> StrykeValue {
    let xs = args.first().map(as_vec_f64).unwrap_or_default();
    let n = arg_i64(args, 1).unwrap_or(xs.len() as i64).max(0) as usize;
    let seed = arg_i64(args, 2).unwrap_or(0) as u64;
    if xs.is_empty() {
        return arr_f64(vec![]);
    }
    let mut state = seed.wrapping_add(0x9E3779B97F4A7C15);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let idx = (state >> 32) as usize % xs.len();
        out.push(xs[idx]);
    }
    arr_f64(out)
}

pub fn shuffle_resample(args: &[StrykeValue]) -> StrykeValue {
    let mut xs = args.first().map(as_vec_f64).unwrap_or_default();
    let seed = arg_i64(args, 1).unwrap_or(0) as u64;
    let mut state = seed.wrapping_add(0x9E3779B97F4A7C15);
    for i in (1..xs.len()).rev() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (state >> 32) as usize % (i + 1);
        xs.swap(i, j);
    }
    arr_f64(xs)
}

pub fn permutation_test(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(as_vec_f64).unwrap_or_default();
    let b = args.get(1).map(as_vec_f64).unwrap_or_default();
    let n_perms = arg_i64(args, 2).unwrap_or(1000).max(1) as u64;
    let seed = arg_i64(args, 3).unwrap_or(0) as u64;
    if a.is_empty() || b.is_empty() {
        return StrykeValue::float(f64::NAN);
    }
    let mean = |xs: &[f64]| xs.iter().sum::<f64>() / xs.len() as f64;
    let observed = (mean(&a) - mean(&b)).abs();
    let mut combined: Vec<f64> = a.iter().chain(b.iter()).cloned().collect();
    let na = a.len();
    let mut state = seed.wrapping_add(0x9E3779B97F4A7C15);
    let mut count = 0u64;
    for _ in 0..n_perms {
        for i in (1..combined.len()).rev() {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = (state >> 32) as usize % (i + 1);
            combined.swap(i, j);
        }
        let m1 = mean(&combined[..na]);
        let m2 = mean(&combined[na..]);
        if (m1 - m2).abs() >= observed {
            count += 1;
        }
    }
    StrykeValue::float(count as f64 / n_perms as f64)
}

pub fn markov_transition_matrix(args: &[StrykeValue]) -> StrykeValue {
    let seq: Vec<i64> = args
        .first()
        .map(as_vec_sv)
        .unwrap_or_default()
        .iter()
        .map(|x| x.to_int())
        .collect();
    if seq.len() < 2 {
        return matrix_to_sv(&[]);
    }
    let max = *seq.iter().max().unwrap_or(&0) as usize;
    let n = max + 1;
    let mut counts = vec![vec![0.0_f64; n]; n];
    for win in seq.windows(2) {
        counts[win[0] as usize][win[1] as usize] += 1.0;
    }
    for row in &mut counts {
        let sum: f64 = row.iter().sum();
        if sum > 0.0 {
            for v in row {
                *v /= sum;
            }
        }
    }
    matrix_to_sv(&counts)
}

pub fn markov_stationary(args: &[StrykeValue]) -> StrykeValue {
    let m = args.first().map(as_matrix).unwrap_or_default();
    let n = m.len();
    if n == 0 {
        return arr_f64(vec![]);
    }
    let mut pi = vec![1.0 / n as f64; n];
    for _ in 0..200 {
        let mut next = vec![0.0; n];
        for j in 0..n {
            for i in 0..n {
                next[j] += pi[i] * m[i].get(j).copied().unwrap_or(0.0);
            }
        }
        let diff: f64 = pi.iter().zip(next.iter()).map(|(a, b)| (a - b).abs()).sum();
        pi = next;
        if diff < 1e-12 {
            break;
        }
    }
    arr_f64(pi)
}

pub fn viterbi_decode(args: &[StrykeValue]) -> StrykeValue {
    let obs: Vec<usize> = args
        .first()
        .map(as_vec_sv)
        .unwrap_or_default()
        .iter()
        .map(|x| x.to_int().max(0) as usize)
        .collect();
    let init = args.get(1).map(as_vec_f64).unwrap_or_default();
    let trans = args.get(2).map(as_matrix).unwrap_or_default();
    let emit = args.get(3).map(as_matrix).unwrap_or_default();
    let n_states = init.len();
    if n_states == 0 || obs.is_empty() {
        return arr_sv(vec![]);
    }
    let t = obs.len();
    let mut viterbi = vec![vec![0.0_f64; n_states]; t];
    let mut backpointer = vec![vec![0usize; n_states]; t];
    for s in 0..n_states {
        viterbi[0][s] = init[s].ln()
            + emit
                .get(s)
                .and_then(|r| r.get(obs[0]))
                .copied()
                .unwrap_or(0.0)
                .ln();
    }
    for i in 1..t {
        for s in 0..n_states {
            let mut best = f64::NEG_INFINITY;
            let mut bp = 0;
            for prev in 0..n_states {
                let score = viterbi[i - 1][prev]
                    + trans
                        .get(prev)
                        .and_then(|r| r.get(s))
                        .copied()
                        .unwrap_or(0.0)
                        .ln();
                if score > best {
                    best = score;
                    bp = prev;
                }
            }
            viterbi[i][s] = best
                + emit
                    .get(s)
                    .and_then(|r| r.get(obs[i]))
                    .copied()
                    .unwrap_or(0.0)
                    .ln();
            backpointer[i][s] = bp;
        }
    }
    let mut path = vec![0usize; t];
    let mut best_final = 0;
    let mut best_score = f64::NEG_INFINITY;
    for s in 0..n_states {
        if viterbi[t - 1][s] > best_score {
            best_score = viterbi[t - 1][s];
            best_final = s;
        }
    }
    path[t - 1] = best_final;
    for i in (0..t - 1).rev() {
        path[i] = backpointer[i + 1][path[i + 1]];
    }
    arr_sv(
        path.into_iter()
            .map(|x| StrykeValue::integer(x as i64))
            .collect(),
    )
}

pub fn forward_algorithm(args: &[StrykeValue]) -> StrykeValue {
    let obs: Vec<usize> = args
        .first()
        .map(as_vec_sv)
        .unwrap_or_default()
        .iter()
        .map(|x| x.to_int().max(0) as usize)
        .collect();
    let init = args.get(1).map(as_vec_f64).unwrap_or_default();
    let trans = args.get(2).map(as_matrix).unwrap_or_default();
    let emit = args.get(3).map(as_matrix).unwrap_or_default();
    let n_states = init.len();
    if n_states == 0 || obs.is_empty() {
        return StrykeValue::float(0.0);
    }
    let mut alpha = vec![0.0_f64; n_states];
    for s in 0..n_states {
        alpha[s] = init[s]
            * emit
                .get(s)
                .and_then(|r| r.get(obs[0]))
                .copied()
                .unwrap_or(0.0);
    }
    for i in 1..obs.len() {
        let mut next = vec![0.0_f64; n_states];
        for j in 0..n_states {
            for prev in 0..n_states {
                next[j] += alpha[prev]
                    * trans
                        .get(prev)
                        .and_then(|r| r.get(j))
                        .copied()
                        .unwrap_or(0.0);
            }
            next[j] *= emit
                .get(j)
                .and_then(|r| r.get(obs[i]))
                .copied()
                .unwrap_or(0.0);
        }
        alpha = next;
    }
    StrykeValue::float(alpha.iter().sum())
}

pub fn backward_algorithm(args: &[StrykeValue]) -> StrykeValue {
    let obs: Vec<usize> = args
        .first()
        .map(as_vec_sv)
        .unwrap_or_default()
        .iter()
        .map(|x| x.to_int().max(0) as usize)
        .collect();
    let init = args.get(1).map(as_vec_f64).unwrap_or_default();
    let trans = args.get(2).map(as_matrix).unwrap_or_default();
    let emit = args.get(3).map(as_matrix).unwrap_or_default();
    let n_states = init.len();
    if n_states == 0 || obs.is_empty() {
        return StrykeValue::float(0.0);
    }
    let t = obs.len();
    let mut beta = vec![1.0_f64; n_states];
    for i in (0..t - 1).rev() {
        let mut next = vec![0.0_f64; n_states];
        for s in 0..n_states {
            for ns in 0..n_states {
                next[s] += trans.get(s).and_then(|r| r.get(ns)).copied().unwrap_or(0.0)
                    * emit
                        .get(ns)
                        .and_then(|r| r.get(obs[i + 1]))
                        .copied()
                        .unwrap_or(0.0)
                    * beta[ns];
            }
        }
        beta = next;
    }
    let total: f64 = (0..n_states)
        .map(|s| {
            init[s]
                * emit
                    .get(s)
                    .and_then(|r| r.get(obs[0]))
                    .copied()
                    .unwrap_or(0.0)
                * beta[s]
        })
        .sum();
    StrykeValue::float(total)
}

pub fn hyperloglog_pp_new(args: &[StrykeValue]) -> StrykeValue {
    let precision = arg_i64(args, 0).unwrap_or(14).clamp(4, 16) as usize;
    let m = 1usize << precision;
    let registers: Vec<StrykeValue> = (0..m).map(|_| StrykeValue::integer(0)).collect();
    make_hash(vec![
        ("precision", StrykeValue::integer(precision as i64)),
        ("registers", arr_sv(registers)),
    ])
}

pub fn hyperloglog_pp_add(args: &[StrykeValue]) -> StrykeValue {
    let hll = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    let item = arg_str(args, 1).unwrap_or_default();
    if let Some(h) = hll.as_hash_ref() {
        let h = h.read();
        let precision = h.get("precision").map(|v| v.to_int()).unwrap_or(14) as usize;
        let regs_v = h.get("registers").cloned().unwrap_or(StrykeValue::UNDEF);
        let mut regs: Vec<i64> = as_vec_sv(&regs_v).iter().map(|x| x.to_int()).collect();
        let m = 1usize << precision;
        if regs.len() != m {
            regs.resize(m, 0);
        }
        let hash = item.bytes().fold(0xCBF29CE484222325u64, |h, b| {
            h.wrapping_mul(0x100000001B3).wrapping_add(b as u64)
        });
        let idx = (hash >> (64 - precision)) as usize;
        let w = (hash << precision) | (1u64 << (precision - 1));
        let leading = w.leading_zeros() as i64 + 1;
        if leading > regs[idx] {
            regs[idx] = leading;
        }
        let regs_sv: Vec<StrykeValue> = regs.into_iter().map(StrykeValue::integer).collect();
        return make_hash(vec![
            ("precision", StrykeValue::integer(precision as i64)),
            ("registers", arr_sv(regs_sv)),
        ]);
    }
    hll
}

pub fn hyperloglog_pp_estimate(args: &[StrykeValue]) -> StrykeValue {
    let hll = args.first().cloned().unwrap_or(StrykeValue::UNDEF);
    if let Some(h) = hll.as_hash_ref() {
        let h = h.read();
        let precision = h.get("precision").map(|v| v.to_int()).unwrap_or(14) as usize;
        let regs_v = h.get("registers").cloned().unwrap_or(StrykeValue::UNDEF);
        let regs: Vec<i64> = as_vec_sv(&regs_v).iter().map(|x| x.to_int()).collect();
        let m = 1usize << precision;
        let alpha = match m {
            16 => 0.673,
            32 => 0.697,
            64 => 0.709,
            _ => 0.7213 / (1.0 + 1.079 / m as f64),
        };
        let sum: f64 = regs.iter().map(|&r| 2.0_f64.powi(-(r as i32))).sum();
        let raw = alpha * (m as f64).powi(2) / sum;
        let zeros = regs.iter().filter(|&&r| r == 0).count();
        let est = if raw <= 2.5 * m as f64 && zeros > 0 {
            m as f64 * (m as f64 / zeros as f64).ln()
        } else {
            raw
        };
        return StrykeValue::float(est);
    }
    StrykeValue::float(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sv_s(x: &str) -> StrykeValue {
        StrykeValue::string(x.to_string())
    }
    fn sv(x: f64) -> StrykeValue {
        StrykeValue::float(x)
    }
    fn sv_i(x: i64) -> StrykeValue {
        StrykeValue::integer(x)
    }

    #[test]
    fn dna_complement_basic() {
        assert_eq!(dna_complement(&[sv_s("ATGC")]).as_str_or_empty(), "TACG");
    }

    #[test]
    fn dna_reverse_complement_basic() {
        assert_eq!(
            dna_reverse_complement(&[sv_s("AAATGGC")]).as_str_or_empty(),
            "GCCATTT"
        );
    }

    #[test]
    fn dna_translate_atg_start() {
        let r = dna_translate(&[sv_s("ATGGCAGAATAA")]);
        assert_eq!(r.as_str_or_empty(), "MAE");
    }

    #[test]
    fn gc_content_50() {
        assert!((dna_gc_content(&[sv_s("ATGC")]).to_number() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn protein_mw_glycine() {
        // G alone: 75.07
        let mw = protein_molecular_weight(&[sv_s("G")]).to_number();
        assert!((mw - 75.07).abs() < 0.01);
    }

    #[test]
    fn nw_score_identical() {
        let s = sv_s("ACGT");
        let r = nw_score(&[s.clone(), s, sv(1.0), sv(-1.0), sv(-2.0)]).to_number();
        assert_eq!(r, 4.0);
    }

    #[test]
    fn sw_score_local_match() {
        let r = sw_score(&[
            sv_s("XXACGTYY"),
            sv_s("ZZACGTWW"),
            sv(2.0),
            sv(-1.0),
            sv(-2.0),
        ])
        .to_number();
        assert!(r >= 8.0);
    }

    #[test]
    fn vec3_cross_xyz() {
        let r = vec3_cross(&[arr_f64(vec![1.0, 0.0, 0.0]), arr_f64(vec![0.0, 1.0, 0.0])]);
        let xs = as_vec_f64(&r);
        assert_eq!(xs, vec![0.0, 0.0, 1.0]);
    }

    #[test]
    fn vec3_normalize_basic() {
        let r = vec3_normalize(&[arr_f64(vec![3.0, 0.0, 4.0])]);
        let xs = as_vec_f64(&r);
        assert!((xs[0] - 0.6).abs() < 1e-9);
        assert!((xs[2] - 0.8).abs() < 1e-9);
    }

    #[test]
    fn mat4_identity_multiplied_is_self() {
        let id = mat4_identity(&[]);
        let r_mat = mat4_rotate_z(&[sv(0.5)]);
        let prod = mat4_multiply(&[id, r_mat.clone()]);
        let a = mat4_to_flat(&prod);
        let b = mat4_to_flat(&r_mat);
        for (x, y) in a.iter().zip(b.iter()) {
            assert!((x - y).abs() < 1e-9);
        }
    }

    #[test]
    fn quat_identity_check() {
        let q = quat_identity(&[]);
        let xs = as_vec_f64(&q);
        assert_eq!(xs, vec![1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn aabb_contains_point_inside() {
        let aabb = arr_sv(vec![
            arr_f64(vec![0.0, 0.0, 0.0]),
            arr_f64(vec![10.0, 10.0, 10.0]),
        ]);
        assert_eq!(
            aabb_contains_point(&[aabb, arr_f64(vec![5.0, 5.0, 5.0])]).to_int(),
            1
        );
    }

    #[test]
    fn ray_aabb_hit() {
        let aabb = arr_sv(vec![
            arr_f64(vec![0.0, 0.0, 0.0]),
            arr_f64(vec![1.0, 1.0, 1.0]),
        ]);
        let origin = arr_f64(vec![-1.0, 0.5, 0.5]);
        let dir = arr_f64(vec![1.0, 0.0, 0.0]);
        let t = ray_aabb_intersect(&[origin, dir, aabb]).to_number();
        assert!((t - 1.0).abs() < 1e-9);
    }

    #[test]
    fn png_header_parse() {
        // Synthesize 24-byte PNG header for 100x50 8-bit RGB
        let bytes_hex = "89504E470D0A1A0A0000000D49484452000000640000003208020000000000000000";
        let r = png_header_read(&[sv_s(bytes_hex)]);
        if let Some(h) = r.as_hash_ref() {
            let h = h.read();
            assert_eq!(h.get("width").unwrap().to_int(), 100);
            assert_eq!(h.get("height").unwrap().to_int(), 50);
        }
    }

    #[test]
    fn markov_stationary_2state() {
        // Matrix [[0.7, 0.3], [0.4, 0.6]] — stationary ≈ [4/7, 3/7]
        let m = matrix_to_sv(&[vec![0.7, 0.3], vec![0.4, 0.6]]);
        let pi = markov_stationary(&[m]);
        let xs = as_vec_f64(&pi);
        assert!((xs[0] - 4.0 / 7.0).abs() < 1e-3);
        assert!((xs[1] - 3.0 / 7.0).abs() < 1e-3);
    }

    #[test]
    fn bootstrap_basic() {
        let r = bootstrap_resample(&[arr_f64(vec![1.0, 2.0, 3.0, 4.0, 5.0]), sv_i(100), sv_i(42)]);
        let xs = as_vec_f64(&r);
        assert_eq!(xs.len(), 100);
        for v in xs {
            assert!(v >= 1.0 && v <= 5.0);
        }
    }
}
