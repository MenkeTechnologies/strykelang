// music theory: pitch / interval algebra, scale & mode catalogues,
// chord identification, tuning systems, tempo conversions, harmonic series.

fn b64_to_floats(v: &StrykeValue) -> Vec<f64> {
    arg_to_vec(v).iter().map(|x| x.to_number()).collect()
}

const B64_A4: f64 = 440.0;
const B64_MIDI_A4: i64 = 69;

/// Cents between two frequencies: 1200 · log₂(f₂/f₁).
fn builtin_cents_between_freqs(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let f1_v = f1(args).max(1e-9);
    let f2 = args.get(1).map(|v| v.to_number()).unwrap_or(f1_v).max(1e-9);
    Ok(StrykeValue::float(1200.0 * (f2 / f1_v).log2()))
}

/// Note name from MIDI number. Returns the integer of the (note class · 100 +
/// octave). Note classes: C=0, C#=1, D=2, ..., B=11. Octave per MIDI standard.
fn builtin_note_name_from_midi(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let midi = i1(args);
    let pc = midi.rem_euclid(12);
    let octave = midi.div_euclid(12) - 1;
    Ok(StrykeValue::integer(pc * 100 + octave))
}

/// Interval quality + size from semitones. Returns size·100 + quality_id.
/// quality_id: 0=perfect, 1=major, 2=minor, 3=augmented, 4=diminished.
fn builtin_interval_quality_size(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let semis = i1(args).rem_euclid(12);
    let (size, quality) = match semis {
        0 => (1, 0),  // perfect unison
        1 => (2, 2),  // minor 2nd
        2 => (2, 1),  // major 2nd
        3 => (3, 2),  // minor 3rd
        4 => (3, 1),  // major 3rd
        5 => (4, 0),  // perfect 4th
        6 => (5, 4),  // dim 5th / aug 4th
        7 => (5, 0),  // perfect 5th
        8 => (6, 2),  // minor 6th
        9 => (6, 1),  // major 6th
        10 => (7, 2), // minor 7th
        11 => (7, 1), // major 7th
        _ => (1, 0),
    };
    Ok(StrykeValue::integer((size as i64) * 100 + quality as i64))
}

/// Major scale pitches (C major: C D E F G A B = 0 2 4 5 7 9 11). Args: tonic
/// pitch class, returns the 7 pitch classes packed as p₀·1e6 + ... + p₆.
fn builtin_scale_pitches_major(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tonic = i1(args).rem_euclid(12);
    let intervals = [0_i64, 2, 4, 5, 7, 9, 11];
    let mut packed = 0_i64;
    for (i, &iv) in intervals.iter().enumerate() {
        let p = (tonic + iv).rem_euclid(12);
        packed += p * 100i64.pow(6 - i as u32);
    }
    Ok(StrykeValue::integer(packed))
}

/// Natural minor: 0 2 3 5 7 8 10.
fn builtin_scale_pitches_minor(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tonic = i1(args).rem_euclid(12);
    let intervals = [0_i64, 2, 3, 5, 7, 8, 10];
    let mut packed = 0_i64;
    for (i, &iv) in intervals.iter().enumerate() {
        let p = (tonic + iv).rem_euclid(12);
        packed += p * 100i64.pow(6 - i as u32);
    }
    Ok(StrykeValue::integer(packed))
}

/// Dorian mode: 0 2 3 5 7 9 10.
fn builtin_mode_pitches_dorian(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tonic = i1(args).rem_euclid(12);
    let intervals = [0_i64, 2, 3, 5, 7, 9, 10];
    let mut packed = 0_i64;
    for (i, &iv) in intervals.iter().enumerate() {
        packed += (tonic + iv).rem_euclid(12) * 100i64.pow(6 - i as u32);
    }
    Ok(StrykeValue::integer(packed))
}

/// Phrygian: 0 1 3 5 7 8 10.
fn builtin_mode_pitches_phrygian(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tonic = i1(args).rem_euclid(12);
    let intervals = [0_i64, 1, 3, 5, 7, 8, 10];
    let mut packed = 0_i64;
    for (i, &iv) in intervals.iter().enumerate() {
        packed += (tonic + iv).rem_euclid(12) * 100i64.pow(6 - i as u32);
    }
    Ok(StrykeValue::integer(packed))
}

/// Lydian: 0 2 4 6 7 9 11.
fn builtin_mode_pitches_lydian(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tonic = i1(args).rem_euclid(12);
    let intervals = [0_i64, 2, 4, 6, 7, 9, 11];
    let mut packed = 0_i64;
    for (i, &iv) in intervals.iter().enumerate() {
        packed += (tonic + iv).rem_euclid(12) * 100i64.pow(6 - i as u32);
    }
    Ok(StrykeValue::integer(packed))
}

/// Chord root + inversion from pitch-class set. Args: pitch classes ascending.
/// Returns root_pc·10 + inversion.
fn builtin_chord_root_inversion(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pcs = b64_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if pcs.is_empty() { return Ok(StrykeValue::integer(0)); }
    let bass = pcs[0] as i64;
    let mut intervals: Vec<i64> = pcs.iter()
        .map(|p| (*p as i64 - bass).rem_euclid(12))
        .collect();
    intervals.sort();
    let inversion = match intervals.as_slice() {
        [0, 4, 7] | [0, 3, 7] => 0,
        [0, 3, 8] | [0, 4, 9] => 1,
        [0, 5, 8] | [0, 5, 9] => 2,
        _ => 0,
    };
    Ok(StrykeValue::integer(bass * 10 + inversion))
}

/// Chord quality classify (major / minor / dim / aug / 7th). Returns id 0..7.
fn builtin_chord_quality_classify(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let pcs = b64_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    if pcs.is_empty() { return Ok(StrykeValue::integer(-1)); }
    let bass = pcs[0] as i64;
    let mut intervals: Vec<i64> = pcs.iter()
        .map(|p| (*p as i64 - bass).rem_euclid(12))
        .collect();
    intervals.sort(); intervals.dedup();
    let id = match intervals.as_slice() {
        [0, 4, 7] => 0,         // major
        [0, 3, 7] => 1,         // minor
        [0, 3, 6] => 2,         // diminished
        [0, 4, 8] => 3,         // augmented
        [0, 4, 7, 10] => 4,     // dominant 7
        [0, 4, 7, 11] => 5,     // major 7
        [0, 3, 7, 10] => 6,     // minor 7
        [0, 3, 6, 9] => 7,      // diminished 7
        _ => -1,
    };
    Ok(StrykeValue::integer(id))
}

/// Chord voicing close: returns sum of interval gaps in semitones between
/// consecutive ascending pitches. Tighter (lower sum) → closer voicing.
fn builtin_chord_voicing_close(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let mut pcs = b64_to_floats(args.first().unwrap_or(&StrykeValue::array(vec![])));
    pcs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if pcs.len() < 2 { return Ok(StrykeValue::float(0.0)); }
    let s: f64 = pcs.windows(2).map(|w| w[1] - w[0]).sum();
    Ok(StrykeValue::float(s))
}

/// Number of sharps in a major key signature (C=0, G=1, D=2, ..., F#=6, C#=7).
fn builtin_key_signature_sharps(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tonic = i1(args).rem_euclid(12);
    let order = [0_i64, 7, 2, 9, 4, 11, 6, 1, 8, 3, 10, 5];
    Ok(StrykeValue::integer(order.iter().position(|&t| t == tonic).unwrap_or(0) as i64))
}

/// Number of flats: C=0, F=1, B♭=2, ... (anti-circle).
fn builtin_key_signature_flats(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tonic = i1(args).rem_euclid(12);
    let order = [0_i64, 5, 10, 3, 8, 1, 6, 11, 4, 9, 2, 7];
    Ok(StrykeValue::integer(order.iter().position(|&t| t == tonic).unwrap_or(0) as i64))
}

/// Tempo BPM → milliseconds per beat: 60000 / BPM.
fn builtin_tempo_to_ms(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let bpm = f1(args).max(1e-9);
    Ok(StrykeValue::float(60000.0 / bpm))
}

/// Beat to seconds at given BPM.
fn builtin_beat_to_seconds(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let beats = f1(args);
    let bpm = args.get(1).map(|v| v.to_number()).unwrap_or(120.0).max(1e-9);
    Ok(StrykeValue::float(beats * 60.0 / bpm))
}

/// Time-signature subdivision count: numerator · power-of-2 from denominator.
/// 4/4 with quarter subdivisions = 4 ticks; 6/8 with eighth subdivisions = 6.
fn builtin_time_sig_subdivision(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let num = i1(args);
    let denom = args.get(1).map(|v| v.to_number() as i64).unwrap_or(4);
    let sub_div_factor = args.get(2).map(|v| v.to_number() as i64).unwrap_or(denom);
    Ok(StrykeValue::integer(num * sub_div_factor / denom))
}

/// Equal-tempered frequency: f = A4 · 2^((n - 69) / 12) for MIDI n.
fn builtin_equal_tempered_freq(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let n = f1(args);
    Ok(StrykeValue::float(B64_A4 * 2f64.powf((n - B64_MIDI_A4 as f64) / 12.0)))
}

/// Just intonation frequency for tonic + interval (pure ratios). Args: tonic
/// freq, interval semitones (0..11). 5-limit JI ratios per ratio table.
fn builtin_just_intonation_freq(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tonic = f1(args);
    // Use `args.get(1..)` to avoid panicking when `args` is empty —
    // `&args[1..]` requires `args.len() >= 1`.
    let semis = i1(args.get(1..).unwrap_or(&[])).rem_euclid(12);
    let ratios = [
        1.0_f64, 16.0/15.0, 9.0/8.0, 6.0/5.0, 5.0/4.0, 4.0/3.0,
        45.0/32.0, 3.0/2.0, 8.0/5.0, 5.0/3.0, 9.0/5.0, 15.0/8.0,
    ];
    Ok(StrykeValue::float(tonic * ratios[semis as usize]))
}

/// Pythagorean tuning: ratios via stacked perfect 5ths (3/2)^n / 2^k.
fn builtin_pythagorean_freq(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tonic = f1(args);
    let semis = i1(args.get(1..).unwrap_or(&[])).rem_euclid(12);
    let ratios = [
        1.0_f64, 256.0/243.0, 9.0/8.0, 32.0/27.0, 81.0/64.0, 4.0/3.0,
        729.0/512.0, 3.0/2.0, 128.0/81.0, 27.0/16.0, 16.0/9.0, 243.0/128.0,
    ];
    Ok(StrykeValue::float(tonic * ratios[semis as usize]))
}

/// Quarter-comma meantone tuning ratios. The fifth is tempered to 5^(1/4) (≈
/// 696.578 cents). Cleaner thirds at the cost of impure fifths.
fn builtin_mean_tone_freq(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tonic = f1(args);
    let semis = i1(args.get(1..).unwrap_or(&[])).rem_euclid(12);
    let fifth = 5f64.powf(1.0 / 4.0);
    let cents_per_step = [0.0_f64, 76.0, 193.0, 310.0, 386.0, 503.0, 580.0,
                          697.0, 773.0, 890.0, 1007.0, 1083.0];
    let cents = cents_per_step[semis as usize] / 1200.0;
    Ok(StrykeValue::float(tonic * 2f64.powf(cents) * fifth.powf(0.0)))
}

/// Werckmeister III "well-tempered": closest ratios from standard table.
fn builtin_werckmeister_iii(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tonic = f1(args);
    let semis = i1(args.get(1..).unwrap_or(&[])).rem_euclid(12);
    let cents = [0.0_f64, 90.225, 192.180, 294.135, 390.225, 498.045,
                 588.270, 696.090, 792.180, 888.270, 996.090, 1092.180];
    Ok(StrykeValue::float(tonic * 2f64.powf(cents[semis as usize] / 1200.0)))
}

/// Kirnberger III well-tempered tuning.
fn builtin_kirnberger_iii(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let tonic = f1(args);
    let semis = i1(args.get(1..).unwrap_or(&[])).rem_euclid(12);
    let cents = [0.0_f64, 90.225, 193.157, 294.135, 386.314, 498.045,
                 590.224, 696.578, 792.180, 889.735, 996.089, 1088.269];
    Ok(StrykeValue::float(tonic * 2f64.powf(cents[semis as usize] / 1200.0)))
}

/// Dynamics dB level: pp = -54, p = -42, mp = -30, mf = -18, f = -6, ff = +6,
/// fff = +18, given dynamic ID 0..7.
fn builtin_dynamics_db_level(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let id = i1(args).clamp(0, 7) as usize;
    let levels = [-54.0_f64, -42.0, -30.0, -18.0, -12.0, -6.0, 6.0, 18.0];
    Ok(StrykeValue::float(levels[id]))
}

/// Harmonic partial frequency: f_n = n · f₀.
fn builtin_harmonics_partial(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let f0 = f1(args);
    let n = args.get(1).map(|v| v.to_number()).unwrap_or(1.0);
    Ok(StrykeValue::float(n * f0))
}
