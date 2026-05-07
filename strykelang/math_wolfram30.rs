// Batch 30 — final mixed: astronomy, music, color, units, miscellaneous.

// Distance modulus: m - M = 5 log10(d/10pc)
fn builtin_distance_modulus_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let d_pc = f1(args);
    if d_pc <= 0.0 { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    Ok(PerlValue::float(5.0 * (d_pc / 10.0).log10()))
}
// Apparent magnitude from absolute and distance
fn builtin_apparent_magnitude_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let abs_mag = f1(args);
    let d_pc = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    if d_pc <= 0.0 { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    Ok(PerlValue::float(abs_mag + 5.0 * (d_pc / 10.0).log10()))
}
// Absolute magnitude from apparent and distance
fn builtin_absolute_magnitude(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let app_mag = f1(args);
    let d_pc = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    if d_pc <= 0.0 { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    Ok(PerlValue::float(app_mag - 5.0 * (d_pc / 10.0).log10()))
}
// Parsec to light years
fn builtin_pc_to_ly(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pc = f1(args);
    Ok(PerlValue::float(pc * 3.26156))
}
// Light years to parsecs
fn builtin_ly_to_pc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ly = f1(args);
    Ok(PerlValue::float(ly / 3.26156))
}
// Parsec to AU
fn builtin_pc_to_au(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pc = f1(args);
    Ok(PerlValue::float(pc * 206264.806))
}
// AU to meters
fn builtin_au_to_m(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let au = f1(args);
    Ok(PerlValue::float(au * 1.495978707e11))
}
// Solar mass to kg
fn builtin_solar_mass_to_kg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m_sun = f1(args);
    Ok(PerlValue::float(m_sun * 1.98892e30))
}
// Solar luminosity to watts
fn builtin_solar_luminosity_to_w(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l_sun = f1(args);
    Ok(PerlValue::float(l_sun * 3.828e26))
}
// Hubble distance D = cz/H0 (z, H0 in km/s/Mpc → returns Mpc)
fn builtin_hubble_distance_mpc(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = f1(args);
    let h0 = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    let c_kms = 299792.458;
    if h0 == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(c_kms * z / h0))
}
// Comoving distance approx (small z)
fn builtin_comoving_distance_approx(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let z = f1(args);
    let h0 = args.get(1).map(|v| v.to_number()).unwrap_or(70.0);
    let c_kms = 299792.458;
    if h0 == 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(c_kms / h0 * z * (1.0 - 0.5 * z)))
}
// Critical density of universe ρ_c = 3H₀² / (8πG)
fn builtin_critical_density(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h0_si = f1(args);
    let g = 6.674e-11;
    let pi = std::f64::consts::PI;
    Ok(PerlValue::float(3.0 * h0_si * h0_si / (8.0 * pi * g)))
}

// Equal-temperament frequency ratio: r = 2^(1/12) for n=1
fn builtin_et_freq_ratio(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = f1(args);
    let edo = args.get(1).map(|v| v.to_number()).unwrap_or(12.0);
    Ok(PerlValue::float(2_f64.powf(n / edo)))
}
// MIDI note to frequency
fn builtin_midi_to_hz(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let midi = f1(args);
    Ok(PerlValue::float(440.0 * 2_f64.powf((midi - 69.0) / 12.0)))
}
// Frequency to MIDI note
fn builtin_hz_to_midi(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let hz = f1(args);
    if hz <= 0.0 { return Ok(PerlValue::float(f64::NEG_INFINITY)); }
    Ok(PerlValue::float(69.0 + 12.0 * (hz / 440.0).log2()))
}
// Cents between two frequencies
fn builtin_cents_between(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f1_hz = f1(args);
    let f2_hz = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    if f1_hz <= 0.0 || f2_hz <= 0.0 { return Ok(PerlValue::float(0.0)); }
    Ok(PerlValue::float(1200.0 * (f2_hz / f1_hz).log2()))
}
// Just intonation ratio for interval (semitones, returns float)
fn builtin_just_intonation_ratio(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let semitones = i1(args);
    let ratios = [
        (1.0_f64, 1.0_f64), (16.0, 15.0), (9.0, 8.0), (6.0, 5.0),
        (5.0, 4.0), (4.0, 3.0), (45.0, 32.0), (3.0, 2.0),
        (8.0, 5.0), (5.0, 3.0), (16.0, 9.0), (15.0, 8.0), (2.0, 1.0),
    ];
    let idx = (semitones.rem_euclid(12)) as usize;
    let (n, d) = ratios[idx];
    Ok(PerlValue::float(n / d))
}
// Pythagorean ratio (3:2 stack)
fn builtin_pythagorean_ratio(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    Ok(PerlValue::float((3.0_f64 / 2.0).powi(n as i32)))
}
// Beat frequency
fn builtin_beat_frequency(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f1_hz = f1(args);
    let f2_hz = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float((f1_hz - f2_hz).abs()))
}
// BPM to seconds per beat
fn builtin_bpm_to_spb(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bpm = f1(args);
    if bpm <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(60.0 / bpm))
}
// Note name to MIDI (e.g. "C4" → 60)
fn builtin_note_name_to_midi(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    if s.is_empty() { return Ok(PerlValue::integer(-1)); }
    let bytes = s.as_bytes();
    let semitones_per_letter = match bytes[0] as char {
        'C' => 0, 'D' => 2, 'E' => 4, 'F' => 5, 'G' => 7, 'A' => 9, 'B' => 11,
        _ => return Ok(PerlValue::integer(-1)),
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
    Ok(PerlValue::integer((semitones_per_letter + accidental + 12 * (octave + 1)) as i64))
}

// RGB to HSL
fn builtin_rgb_to_hsl_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args) / 255.0;
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0) / 255.0;
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0) / 255.0;
    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let l = (max + min) / 2.0;
    let mut h = 0.0_f64;
    let mut s = 0.0_f64;
    if max != min {
        let d = max - min;
        s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        h = if max == r {
            (g - b) / d + (if g < b { 6.0 } else { 0.0 })
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        h /= 6.0;
    }
    Ok(PerlValue::array(vec![PerlValue::float(h * 360.0), PerlValue::float(s), PerlValue::float(l)]))
}
// HSL to RGB
fn builtin_hsl_to_rgb_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let h = f1(args) / 360.0;
    let s = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let l = args.get(2).map(|v| v.to_number()).unwrap_or(0.5);
    fn h_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
        if t < 0.0 { t += 1.0; }
        if t > 1.0 { t -= 1.0; }
        if t < 1.0 / 6.0 { return p + (q - p) * 6.0 * t; }
        if t < 0.5 { return q; }
        if t < 2.0 / 3.0 { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
        p
    }
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        (h_to_rgb(p, q, h + 1.0 / 3.0), h_to_rgb(p, q, h), h_to_rgb(p, q, h - 1.0 / 3.0))
    };
    Ok(PerlValue::array(vec![
        PerlValue::float(r * 255.0),
        PerlValue::float(g * 255.0),
        PerlValue::float(b * 255.0),
    ]))
}
// RGB to YIQ
fn builtin_rgb_to_yiq(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args) / 255.0;
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0) / 255.0;
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0) / 255.0;
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let i = 0.596 * r - 0.274 * g - 0.322 * b;
    let q = 0.211 * r - 0.523 * g + 0.312 * b;
    Ok(PerlValue::array(vec![PerlValue::float(y), PerlValue::float(i), PerlValue::float(q)]))
}
// RGB to YUV (BT.601)
fn builtin_rgb_to_yuv601(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let r = f1(args) / 255.0;
    let g = args.get(1).map(|v| v.to_number()).unwrap_or(0.0) / 255.0;
    let b = args.get(2).map(|v| v.to_number()).unwrap_or(0.0) / 255.0;
    let y = 0.299 * r + 0.587 * g + 0.114 * b;
    let u = -0.14713 * r - 0.28886 * g + 0.436 * b;
    let v = 0.615 * r - 0.51499 * g - 0.10001 * b;
    Ok(PerlValue::array(vec![PerlValue::float(y), PerlValue::float(u), PerlValue::float(v)]))
}
// CIE XYZ from sRGB (D65)
fn builtin_srgb_to_xyz(args: &[PerlValue]) -> PerlResult<PerlValue> {
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
    Ok(PerlValue::array(vec![PerlValue::float(x), PerlValue::float(y), PerlValue::float(z)]))
}
// XYZ to CIELAB (D65)
fn builtin_xyz_to_lab(args: &[PerlValue]) -> PerlResult<PerlValue> {
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
    Ok(PerlValue::array(vec![PerlValue::float(l), PerlValue::float(a), PerlValue::float(b)]))
}
// CIE76 ΔE
fn builtin_delta_e_76_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let l1 = f1(args);
    let a1 = args.get(1).map(|v| v.to_number()).unwrap_or(0.0);
    let b1 = args.get(2).map(|v| v.to_number()).unwrap_or(0.0);
    let l2 = args.get(3).map(|v| v.to_number()).unwrap_or(0.0);
    let a2 = args.get(4).map(|v| v.to_number()).unwrap_or(0.0);
    let b2 = args.get(5).map(|v| v.to_number()).unwrap_or(0.0);
    Ok(PerlValue::float(((l1 - l2).powi(2) + (a1 - a2).powi(2) + (b1 - b2).powi(2)).sqrt()))
}
// CIE94 ΔE (graphic arts default)
fn builtin_delta_e_94(args: &[PerlValue]) -> PerlResult<PerlValue> {
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
    Ok(PerlValue::float(term.sqrt()))
}

// Celsius/Fahrenheit/Kelvin
fn builtin_c_to_f_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c = f1(args);
    Ok(PerlValue::float(c * 9.0 / 5.0 + 32.0))
}
fn builtin_f_to_c_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args);
    Ok(PerlValue::float((f - 32.0) * 5.0 / 9.0))
}
fn builtin_c_to_k_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let c = f1(args);
    Ok(PerlValue::float(c + 273.15))
}
fn builtin_k_to_c_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = f1(args);
    Ok(PerlValue::float(k - 273.15))
}
fn builtin_f_to_k_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args);
    Ok(PerlValue::float((f - 32.0) * 5.0 / 9.0 + 273.15))
}
fn builtin_k_to_f_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let k = f1(args);
    Ok(PerlValue::float((k - 273.15) * 9.0 / 5.0 + 32.0))
}

// Distance unit conversions
fn builtin_inches_to_cm_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let i = f1(args);
    Ok(PerlValue::float(i * 2.54))
}
fn builtin_cm_to_inches_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cm = f1(args);
    Ok(PerlValue::float(cm / 2.54))
}
fn builtin_miles_to_km_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mi = f1(args);
    Ok(PerlValue::float(mi * 1.609344))
}
fn builtin_km_to_miles_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let km = f1(args);
    Ok(PerlValue::float(km / 1.609344))
}
fn builtin_feet_to_meters(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ft = f1(args);
    Ok(PerlValue::float(ft * 0.3048))
}
fn builtin_meters_to_feet(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let m = f1(args);
    Ok(PerlValue::float(m / 0.3048))
}

// Mass conversions
fn builtin_lb_to_kg(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let lb = f1(args);
    Ok(PerlValue::float(lb * 0.45359237))
}
fn builtin_kg_to_lb(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let kg = f1(args);
    Ok(PerlValue::float(kg / 0.45359237))
}
fn builtin_oz_to_g_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let oz = f1(args);
    Ok(PerlValue::float(oz * 28.3495))
}
fn builtin_g_to_oz_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let g = f1(args);
    Ok(PerlValue::float(g / 28.3495))
}

// Speed
fn builtin_mph_to_kmh(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mph = f1(args);
    Ok(PerlValue::float(mph * 1.609344))
}
fn builtin_kmh_to_mph(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let kmh = f1(args);
    Ok(PerlValue::float(kmh / 1.609344))
}
fn builtin_mps_to_kmh(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mps = f1(args);
    Ok(PerlValue::float(mps * 3.6))
}
fn builtin_kmh_to_mps(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let kmh = f1(args);
    Ok(PerlValue::float(kmh / 3.6))
}
fn builtin_knots_to_kmh(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let kn = f1(args);
    Ok(PerlValue::float(kn * 1.852))
}

// Pressure
fn builtin_psi_to_pa_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let psi = f1(args);
    Ok(PerlValue::float(psi * 6894.757))
}
fn builtin_pa_to_psi_b30(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pa = f1(args);
    Ok(PerlValue::float(pa / 6894.757))
}
fn builtin_atm_to_pa(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let atm = f1(args);
    Ok(PerlValue::float(atm * 101325.0))
}
fn builtin_pa_to_atm(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let pa = f1(args);
    Ok(PerlValue::float(pa / 101325.0))
}
fn builtin_mmhg_to_pa(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mmhg = f1(args);
    Ok(PerlValue::float(mmhg * 133.322))
}

// Energy
fn builtin_ev_to_joules(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let ev = f1(args);
    Ok(PerlValue::float(ev * 1.602176634e-19))
}
fn builtin_joules_to_ev(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let j = f1(args);
    Ok(PerlValue::float(j / 1.602176634e-19))
}
#[allow(dead_code)]
fn builtin_cal_to_joules(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let cal = f1(args);
    Ok(PerlValue::float(cal * 4.184))
}
fn builtin_btu_to_joules(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let btu = f1(args);
    Ok(PerlValue::float(btu * 1055.06))
}
fn builtin_kwh_to_joules(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let kwh = f1(args);
    Ok(PerlValue::float(kwh * 3.6e6))
}

// Tempo to MIDI tick (pulses per quarter note)
fn builtin_bpm_to_midi_tick_us(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let bpm = f1(args);
    let ppqn = args.get(1).map(|v| v.to_number()).unwrap_or(480.0);
    if bpm <= 0.0 || ppqn <= 0.0 { return Ok(PerlValue::float(f64::INFINITY)); }
    Ok(PerlValue::float(60_000_000.0 / (bpm * ppqn)))
}

// Loudness equal-loudness contour ISO 226 — return relative SPL adjustment (placeholder)
fn builtin_iso226_phon_adjustment(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let f = f1(args);
    let phon = args.get(1).map(|v| v.to_number()).unwrap_or(60.0);
    let _ = phon;
    if f <= 0.0 { return Ok(PerlValue::float(0.0)); }
    let log_f = f.log10();
    Ok(PerlValue::float(20.0 * (log_f - 3.0).powi(2) - 10.0))
}

// dB to linear amplitude
fn builtin_db_to_amp(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let db = f1(args);
    Ok(PerlValue::float(10_f64.powf(db / 20.0)))
}
// Linear amp to dB
fn builtin_amp_to_db(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let amp = f1(args).max(1e-30);
    Ok(PerlValue::float(20.0 * amp.log10()))
}

// Roman numeral encode (1..3999)
fn builtin_roman_encode(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut n = i1(args);
    if !(1..=3999).contains(&n) { return Ok(PerlValue::string(String::new())); }
    let pairs = [
        (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"),
        (100, "C"), (90, "XC"), (50, "L"), (40, "XL"),
        (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
    ];
    let mut out = String::new();
    for &(v, s) in &pairs {
        while n >= v { out.push_str(s); n -= v; }
    }
    Ok(PerlValue::string(out))
}
// Roman numeral decode
fn builtin_roman_decode(args: &[PerlValue]) -> PerlResult<PerlValue> {
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
    Ok(PerlValue::integer(total))
}

// Number to English (simplified, 0..999)
fn builtin_number_to_english(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let mut n = i1(args);
    if n == 0 { return Ok(PerlValue::string("zero".into())); }
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
    Ok(PerlValue::string(parts.join(" ")))
}
