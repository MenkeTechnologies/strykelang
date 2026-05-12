// Batch 30 — final mixed: astronomy, music, color, units, miscellaneous.

// Distance modulus: m - M = 5 log10(d/10pc)
// Apparent magnitude from absolute and distance
// Absolute magnitude from apparent and distance
fn builtin_absolute_magnitude(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let app_mag = f1(args);
    let d_pc = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    if d_pc <= 0.0 { return Ok(StrykeValue::float(f64::NEG_INFINITY)); }
    Ok(StrykeValue::float(app_mag - 5.0 * (d_pc / 10.0).log10()))
}
// Parsec to light years
fn builtin_pc_to_ly(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pc = f1(args);
    Ok(StrykeValue::float(pc * 3.26156))
}
// Light years to parsecs
fn builtin_ly_to_pc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ly = f1(args);
    Ok(StrykeValue::float(ly / 3.26156))
}
// Parsec to AU
fn builtin_pc_to_au(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pc = f1(args);
    Ok(StrykeValue::float(pc * 206264.806))
}
// AU to meters
fn builtin_au_to_m(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let au = f1(args);
    Ok(StrykeValue::float(au * 1.495978707e11))
}
// Solar mass to kg
fn builtin_solar_mass_to_kg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m_sun = f1(args);
    Ok(StrykeValue::float(m_sun * 1.98892e30))
}
// Solar luminosity to watts
fn builtin_solar_luminosity_to_w(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l_sun = f1(args);
    Ok(StrykeValue::float(l_sun * 3.828e26))
}
// Hubble distance D = cz/H0 (z, H0 in km/s/Mpc → returns Mpc)
fn builtin_hubble_distance_mpc(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    let h0 = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    let c_kms = 299792.458;
    if h0 == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(c_kms * z / h0))
}
// Comoving distance approx (small z)
fn builtin_comoving_distance_approx(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let z = f1(args);
    let h0 = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    let c_kms = 299792.458;
    if h0 == 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(c_kms / h0 * z * (1.0 - 0.5 * z)))
}
// Critical density of universe ρ_c = 3H₀² / (8πG)
fn builtin_critical_density(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let h0_si = f1(args);
    let g = 6.674e-11;
    let pi = std::f64::consts::PI;
    Ok(StrykeValue::float(3.0 * h0_si * h0_si / (8.0 * pi * g)))
}

// Equal-temperament frequency ratio: r = 2^(1/12) for n=1
fn builtin_et_freq_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = f1(args);
    let edo = args.get(1).map(|v| v.to_number()).unwrap_or(12.0);
    Ok(StrykeValue::float(2_f64.powf(n / edo)))
}
// MIDI note to frequency
fn builtin_midi_to_hz(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let midi = f1(args);
    Ok(StrykeValue::float(440.0 * 2_f64.powf((midi - 69.0) / 12.0)))
}
// Frequency to MIDI note
fn builtin_hz_to_midi(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let hz = f1(args);
    if hz <= 0.0 { return Ok(StrykeValue::float(f64::NEG_INFINITY)); }
    Ok(StrykeValue::float(69.0 + 12.0 * (hz / 440.0).log2()))
}
// Cents between two frequencies
fn builtin_cents_between(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f1_hz = f1(args);
    let f2_hz = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if f1_hz <= 0.0 || f2_hz <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    Ok(StrykeValue::float(1200.0 * (f2_hz / f1_hz).log2()))
}
// Just intonation ratio for interval (semitones, returns float)
fn builtin_just_intonation_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let semitones = i1(args);
    let ratios = [
        (1.0_f64, 1.0_f64), (16.0, 15.0), (9.0, 8.0), (6.0, 5.0),
        (5.0, 4.0), (4.0, 3.0), (45.0, 32.0), (3.0, 2.0),
        (8.0, 5.0), (5.0, 3.0), (16.0, 9.0), (15.0, 8.0), (2.0, 1.0),
    ];
    let idx = (semitones.rem_euclid(12)) as usize;
    let (n, d) = ratios[idx];
    Ok(StrykeValue::float(n / d))
}
// Pythagorean ratio (3:2 stack)
fn builtin_pythagorean_ratio(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let n = i1(args);
    Ok(StrykeValue::float((3.0_f64 / 2.0).powi(n as i32)))
}
// Beat frequency
fn builtin_beat_frequency(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f1_hz = f1(args);
    let f2_hz = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(StrykeValue::float((f1_hz - f2_hz).abs()))
}
// BPM to seconds per beat
fn builtin_bpm_to_spb(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bpm = f1(args);
    if bpm <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(60.0 / bpm))
}
// Note name to MIDI (e.g. "C4" → 60)
fn builtin_note_name_to_midi(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    if s.is_empty() { return Ok(StrykeValue::integer(-1)); }
    let bytes = s.as_bytes();
    let semitones_per_letter = match bytes[0] as char {
        'C' => 0, 'D' => 2, 'E' => 4, 'F' => 5, 'G' => 7, 'A' => 9, 'B' => 11,
        _ => return Ok(StrykeValue::integer(-1)),
    };
    let mut idx = 1;
    let mut accidental: i32 = 0;
    if idx < bytes.len() {
        match bytes[idx] as char {
            '#' => { accidental = 1; idx += 1; },
            'b' => { accidental = -1; idx += 1; },
            _ => {},
        }
    }
    let octave: i32 = s[idx..].parse().unwrap_or(4);
    Ok(StrykeValue::integer((semitones_per_letter + accidental + 12 * (octave + 1)) as i64))
}

// RGB to HSL
// HSL to RGB
// RGB to YIQ
fn builtin_rgb_to_yiq(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args) / 255.0;
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0) / 255.0;
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0) / 255.0;
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let i = 0.596 * r - 0.274 * g - 0.322 * b;
    let q = 0.211 * r - 0.523 * g + 0.312 * b;
    Ok(StrykeValue::array(vec![StrykeValue::float(y), StrykeValue::float(i), StrykeValue::float(q)]))
}
// RGB to YUV (BT.601)
fn builtin_rgb_to_yuv601(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let r = f1(args) / 255.0;
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0) / 255.0;
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0) / 255.0;
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let u = -0.14713 * r - 0.28886 * g + 0.436 * b;
    let v = 0.615 * r - 0.51499 * g - 0.10001 * b;
    Ok(StrykeValue::array(vec![StrykeValue::float(y), StrykeValue::float(u), StrykeValue::float(v)]))
}
// CIE XYZ from sRGB (D65)
fn builtin_srgb_to_xyz(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lin = |c: f64| -> f64 {
        let c = c / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let r = lin(f1(args));
    let g = lin(args.get(1).map(|v| v.to_number()).unwrap_or(0.0));
    let b = lin(args.get(2).map(|v| v.to_number()).unwrap_or(0.0));
    let x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = 0.0193339 * r + 0.1191920 * g + 0.9503041 * b;
    Ok(StrykeValue::array(vec![StrykeValue::float(x), StrykeValue::float(y), StrykeValue::float(z)]))
}
// XYZ to CIELAB (D65)
fn builtin_xyz_to_lab(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let xn = 0.95047; let yn = 1.00000; let zn = 1.08883;
    let x = f1(args) / xn;
    let y = args.get(1).map(|v| v.to_number()).unwrap_or(0.0) / yn;
    let z = args.get(2).map(|v| v.to_number()).unwrap_or(0.0) / zn;
    fn ftype(t: f64) -> f64 {
        let delta: f64 = 6.0 / 29.0;
        if t > delta.powi(3) { t.cbrt() } else { t / (3.0 * delta * delta) + 4.0 / 29.0 }
    }
    let l = 116.0 * ftype(y) - 16.0;
    let a = 500.0 * (ftype(x) - ftype(y));
    let b = 200.0 * (ftype(y) - ftype(z));
    Ok(StrykeValue::array(vec![StrykeValue::float(l), StrykeValue::float(a), StrykeValue::float(b)]))
}
// CIE76 ΔE
// CIE94 ΔE (graphic arts default)
fn builtin_delta_e_94(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let l1 = f1(args);
    let a1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let l2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let a2 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let b2 = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    let dl = l1 - l2;
    let da = a1 - a2;
    let db = b1 - b2;
    let c1 = (a1 * a1 + b1 * b1).sqrt();
    let c2 = (a2 * a2 + b2 * b2).sqrt();
    let dc = c1 - c2;
    let dh_sq = (da * da + db * db - dc * dc).max(0.0);
    let kl = 1.0;
    let k1 = 0.045;
    let k2 = 0.015;
    let sl = 1.0;
    let sc = 1.0 + k1 * c1;
    let sh = 1.0 + k2 * c1;
    let term = (dl / (kl * sl)).powi(2) + (dc / sc).powi(2) + dh_sq / (sh * sh);
    Ok(StrykeValue::float(term.sqrt()))
}

// Celsius/Fahrenheit/Kelvin

// Distance unit conversions
fn builtin_feet_to_meters(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ft = f1(args);
    Ok(StrykeValue::float(ft * 0.3048))
}
fn builtin_meters_to_feet(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let m = f1(args);
    Ok(StrykeValue::float(m / 0.3048))
}

// Mass conversions
fn builtin_lb_to_kg(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let lb = f1(args);
    Ok(StrykeValue::float(lb * 0.45359237))
}
fn builtin_kg_to_lb(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kg = f1(args);
    Ok(StrykeValue::float(kg / 0.45359237))
}

// Speed
fn builtin_mph_to_kmh(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mph = f1(args);
    Ok(StrykeValue::float(mph * 1.609344))
}
fn builtin_kmh_to_mph(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kmh = f1(args);
    Ok(StrykeValue::float(kmh / 1.609344))
}
fn builtin_mps_to_kmh(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mps = f1(args);
    Ok(StrykeValue::float(mps * 3.6))
}
fn builtin_kmh_to_mps(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kmh = f1(args);
    Ok(StrykeValue::float(kmh / 3.6))
}
fn builtin_knots_to_kmh(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kn = f1(args);
    Ok(StrykeValue::float(kn * 1.852))
}

// Pressure
fn builtin_atm_to_pa(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let atm = f1(args);
    Ok(StrykeValue::float(atm * 101325.0))
}
fn builtin_pa_to_atm(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let pa = f1(args);
    Ok(StrykeValue::float(pa / 101325.0))
}
fn builtin_mmhg_to_pa(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mmhg = f1(args);
    Ok(StrykeValue::float(mmhg * 133.322))
}

// Energy
fn builtin_ev_to_joules(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let ev = f1(args);
    Ok(StrykeValue::float(ev * 1.602176634e-19))
}
fn builtin_joules_to_ev(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let j = f1(args);
    Ok(StrykeValue::float(j / 1.602176634e-19))
}
#[allow(dead_code)]
fn builtin_cal_to_joules(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let cal = f1(args);
    Ok(StrykeValue::float(cal * 4.184))
}
fn builtin_btu_to_joules(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let btu = f1(args);
    Ok(StrykeValue::float(btu * 1055.06))
}
fn builtin_kwh_to_joules(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let kwh = f1(args);
    Ok(StrykeValue::float(kwh * 3.6e6))
}

// Tempo to MIDI tick (pulses per quarter note)
fn builtin_bpm_to_midi_tick_us(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let bpm = f1(args);
    let ppqn = args.get(1).map(|v| v.to_number()).unwrap_or(480.0);
    if bpm <= 0.0 || ppqn <= 0.0 { return Ok(StrykeValue::float(f64::INFINITY)); }
    Ok(StrykeValue::float(60_000_000.0 / (bpm * ppqn)))
}

// ISO 226:2003 equal-loudness contour: SPL Lp at frequency f for loudness level Ln (phons).
// Uses the spec's αf, Lu, Tf tables at the 29 standard frequencies and the formula
//   A_f = 4.47e-3·(10^(0.025·Ln) − 1.15) + (0.4 · 10^((Tf + Lu)/10 − 9))^αf
//   Lp  = (10/αf)·log₁₀(A_f) − Lu + 94
// Arguments are interpolated (log-frequency, linear in tables) between adjacent bands.
fn builtin_iso226_phon_adjustment(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let f = f1(args);
    let phon = args.get(1).map(|v| v.to_number()).unwrap_or(60.0);
    if f <= 0.0 { return Ok(StrykeValue::float(0.0)); }
    const FREQ: [f64; 29] = [
        20.0, 25.0, 31.5, 40.0, 50.0, 63.0, 80.0, 100.0, 125.0, 160.0,
        200.0, 250.0, 315.0, 400.0, 500.0, 630.0, 800.0, 1000.0, 1250.0,
        1600.0, 2000.0, 2500.0, 3150.0, 4000.0, 5000.0, 6300.0, 8000.0,
        10000.0, 12500.0,
    ];
    const ALPHA_F: [f64; 29] = [
        0.532, 0.506, 0.480, 0.455, 0.432, 0.409, 0.387, 0.367, 0.349, 0.330,
        0.315, 0.301, 0.288, 0.276, 0.267, 0.259, 0.253, 0.250, 0.246, 0.244,
        0.243, 0.243, 0.243, 0.242, 0.242, 0.245, 0.254, 0.271, 0.301,
    ];
    const LU: [f64; 29] = [
        -31.6, -27.2, -23.0, -19.1, -15.9, -13.0, -10.3, -8.1, -6.2, -4.5,
        -3.1, -2.0, -1.1, -0.4, 0.0, 0.3, 0.5, 0.0, -2.7, -4.1,
        -1.0, 1.7, 2.5, 1.2, -2.1, -7.1, -11.2, -10.7, -3.1,
    ];
    const TF: [f64; 29] = [
        78.5, 68.7, 59.5, 51.1, 44.0, 37.5, 31.5, 26.5, 22.1, 17.9,
        14.4, 11.4, 8.6, 6.2, 4.4, 3.0, 2.2, 2.4, 3.5, 1.7,
        -1.3, -4.2, -6.0, -5.4, -1.5, 6.0, 12.6, 13.9, 12.3,
    ];
    let log_f = f.log10();
    let logs: Vec<f64> = FREQ.iter().map(|x| x.log10()).collect();
    let (mut i_lo, mut i_hi) = (0usize, FREQ.len() - 1);
    if log_f <= logs[0] { i_lo = 0; i_hi = 0; }
    else if log_f >= logs[FREQ.len() - 1] { i_lo = FREQ.len() - 1; i_hi = i_lo; }
    else {
        for i in 0..FREQ.len() - 1 {
            if log_f >= logs[i] && log_f <= logs[i + 1] { i_lo = i; i_hi = i + 1; break; }
        }
    }
    let t = if i_lo == i_hi { 0.0 } else { (log_f - logs[i_lo]) / (logs[i_hi] - logs[i_lo]) };
    let lerp = |a: f64, b: f64| a + t * (b - a);
    let alpha = lerp(ALPHA_F[i_lo], ALPHA_F[i_hi]);
    let lu = lerp(LU[i_lo], LU[i_hi]);
    let tf = lerp(TF[i_lo], TF[i_hi]);
    let af = 4.47e-3 * (10f64.powf(0.025 * phon) - 1.15)
           + (0.4 * 10f64.powf((tf + lu) / 10.0 - 9.0)).powf(alpha);
    let lp = (10.0 / alpha) * af.log10() - lu + 94.0;
    Ok(StrykeValue::float(lp))
}

// dB to linear amplitude
fn builtin_db_to_amp(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let db = f1(args);
    Ok(StrykeValue::float(10_f64.powf(db / 20.0)))
}
// Linear amp to dB
fn builtin_amp_to_db(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let amp = f1(args).max(1e-30);
    Ok(StrykeValue::float(20.0 * amp.log10()))
}

// Roman numeral encode (1..3999)
fn builtin_roman_encode(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut n = i1(args);
    if !(1..=3999).contains(&n) { return Ok(StrykeValue::string(String::new())); }
    let pairs = [
        (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"),
        (100, "C"), (90, "XC"), (50, "L"), (40, "XL"),
        (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
    ];
    let mut out = String::new();
    for &(v, s) in &pairs {
        while n >= v { out.push_str(s); n -= v; }
    }
    Ok(StrykeValue::string(out))
}
// Roman numeral decode
fn builtin_roman_decode(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let s = args.first().map(|v| v.to_string().to_ascii_uppercase()).unwrap_or_default();
    let val = |c: char| -> i64 {
        match c {
            'I' => 1, 'V' => 5, 'X' => 10, 'L' => 50,
            'C' => 100, 'D' => 500, 'M' => 1000, _ => 0,
        }
    };
    let chars: Vec<char> = s.chars().collect();
    let mut total = 0_i64;
    for i in 0..chars.len() {
        let v = val(chars[i]);
        if i + 1 < chars.len() && v < val(chars[i + 1]) {
            total -= v;
        } else {
            total += v;
        }
    }
    Ok(StrykeValue::integer(total))
}

// Number to English (simplified, 0..999)
fn builtin_number_to_english(args: &[StrykeValue]) -> PerlResult<StrykeValue> {
    let mut n = i1(args);
    if n == 0 { return Ok(StrykeValue::string("zero".into())); }
    let ones = ["", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
        "ten", "eleven", "twelve", "thirteen", "fourteen", "fifteen", "sixteen",
        "seventeen", "eighteen", "nineteen"];
    let tens = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
    let mut parts: Vec<String> = vec![];
    if n < 0 { parts.push("negative".into()); n = -n; }
    if n >= 1000 {
        let thou = n / 1000;
        n %= 1000;
        if thou < 20 { parts.push(ones[thou as usize].into()); }
        parts.push("thousand".into());
    }
    if n >= 100 {
        parts.push(ones[(n / 100) as usize].into());
        parts.push("hundred".into());
        n %= 100;
    }
    if n >= 20 {
        let tn = n / 10;
        let on = n % 10;
        if on > 0 { parts.push(format!("{}-{}", tens[tn as usize], ones[on as usize])); }
        else { parts.push(tens[tn as usize].into()); }
    } else if n > 0 {
        parts.push(ones[n as usize].into());
    }
    Ok(StrykeValue::string(parts.join(" ")))
}
