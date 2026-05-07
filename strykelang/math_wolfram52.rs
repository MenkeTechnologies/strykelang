// Batch 52 — calendrical algorithms (Reingold & Dershowitz, "Calendrical
// Calculations" 4th ed.). Reference epoch: RD = 0001-01-01 (Gregorian) → fixed 1.

/// Gregorian: day-count from RD epoch given proleptic Gregorian (y, m, d).
fn builtin_fixed_from_gregorian(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let y1 = y - 1;
    let leap_correction = if m <= 2 { 0 } else if leap_gregorian(y) { -1 } else { -2 };
    let n = 365 * y1 + y1 / 4 - y1 / 100 + y1 / 400
        + (367 * m - 362) / 12 + leap_correction + d;
    Ok(PerlValue::integer(n))
}

fn leap_gregorian(y: i64) -> bool {
    y.rem_euclid(4) == 0 && (y.rem_euclid(100) != 0 || y.rem_euclid(400) == 0)
}

/// Inverse of fixed_from_gregorian: (year, month, day) packed as y*10000+m*100+d.
fn builtin_gregorian_from_fixed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let d0 = n - 1;
    let n400 = d0.div_euclid(146097);
    let d1 = d0.rem_euclid(146097);
    let n100 = (d1 / 36524).min(3);
    let d2 = d1 - 36524 * n100;
    let n4 = d2 / 1461;
    let d3 = d2 - 1461 * n4;
    let n1 = (d3 / 365).min(3);
    let year = 400 * n400 + 100 * n100 + 4 * n4 + n1
        + if n100 == 4 || n1 == 4 { 0 } else { 1 };
    let prior_days = n - builtin_fixed_from_gregorian(&[
        PerlValue::integer(year), PerlValue::integer(1), PerlValue::integer(1)
    ]).unwrap().to_number() as i64;
    let correction = if n < builtin_fixed_from_gregorian(&[
        PerlValue::integer(year), PerlValue::integer(3), PerlValue::integer(1)
    ]).unwrap().to_number() as i64 { 0 }
    else if leap_gregorian(year) { 1 } else { 2 };
    let month = (12 * (prior_days + correction) + 373) / 367;
    let day = n - builtin_fixed_from_gregorian(&[
        PerlValue::integer(year), PerlValue::integer(month), PerlValue::integer(1)
    ]).unwrap().to_number() as i64 + 1;
    Ok(PerlValue::integer(year * 10000 + month * 100 + day))
}

/// Julian: leap if y mod 4 == 0 (no century rule). Epoch JD 0001-01-01 = RD -1.
fn builtin_fixed_from_julian(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let y1 = if y < 0 { y + 1 } else { y };
    let leap = y1.rem_euclid(4) == 0;
    let lc = if m <= 2 { 0 } else if leap { -1 } else { -2 };
    let n = -1 + 365 * (y1 - 1) + (y1 - 1) / 4 + (367 * m - 362) / 12 + lc + d;
    Ok(PerlValue::integer(n))
}

/// `julian_from_fixed`
fn builtin_julian_from_fixed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let approx = ((4 * (n + 1) + 1464) / 1461) - 1;
    let y = if approx <= 0 { approx - 1 } else { approx };
    let prior_days = n - builtin_fixed_from_julian(&[
        PerlValue::integer(y), PerlValue::integer(1), PerlValue::integer(1)
    ]).unwrap().to_number() as i64;
    let leap = y.rem_euclid(4) == 0;
    let correction = if n < builtin_fixed_from_julian(&[
        PerlValue::integer(y), PerlValue::integer(3), PerlValue::integer(1)
    ]).unwrap().to_number() as i64 { 0 }
    else if leap { 1 } else { 2 };
    let month = (12 * (prior_days + correction) + 373) / 367;
    let day = n - builtin_fixed_from_julian(&[
        PerlValue::integer(y), PerlValue::integer(month), PerlValue::integer(1)
    ]).unwrap().to_number() as i64 + 1;
    Ok(PerlValue::integer(y * 10000 + month * 100 + day))
}

/// ISO week date: returns iso_year*10000 + week*100 + day.
fn builtin_iso_week_date(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let approx_year = gregorian_from_fixed_year(n - 3);
    let year = if n >= fixed_iso_year_start(approx_year + 1) { approx_year + 1 } else { approx_year };
    let week = (n - fixed_iso_year_start(year)) / 7 + 1;
    let day = (n - 1).rem_euclid(7) + 1;
    Ok(PerlValue::integer(year * 10000 + week * 100 + day))
}

fn gregorian_from_fixed_year(n: i64) -> i64 {
    let v = builtin_gregorian_from_fixed(&[PerlValue::integer(n)]).unwrap().to_number() as i64;
    v / 10000
}

fn fixed_iso_year_start(year: i64) -> i64 {
    let jan4 = builtin_fixed_from_gregorian(&[
        PerlValue::integer(year), PerlValue::integer(1), PerlValue::integer(4)
    ]).unwrap().to_number() as i64;
    jan4 - (jan4 - 1).rem_euclid(7)
}

/// Hebrew calendar: leap year if year mod 19 ∈ {0, 3, 6, 8, 11, 14, 17}.
fn builtin_hebrew_leap_year(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let r = y.rem_euclid(19);
    let leap = matches!(r, 0 | 3 | 6 | 8 | 11 | 14 | 17);
    Ok(PerlValue::integer(if leap { 1 } else { 0 }))
}

/// Hebrew year length: 353, 354, 355 (common); 383, 384, 385 (leap).
fn builtin_hebrew_year_length(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let elapsed = hebrew_elapsed_days(y + 1) - hebrew_elapsed_days(y);
    Ok(PerlValue::integer(elapsed))
}

fn hebrew_elapsed_days(year: i64) -> i64 {
    let months_elapsed = 235 * (year - 1).div_euclid(19) + 12 * (year - 1).rem_euclid(19)
        + 7 * (year - 1).rem_euclid(19) / 19;
    let parts_elapsed = 204 + 793 * months_elapsed.rem_euclid(1080);
    let hours_elapsed = 5 + 12 * months_elapsed + 793 * months_elapsed.div_euclid(1080)
        + parts_elapsed.div_euclid(1080);
    let day = 1 + 29 * months_elapsed + hours_elapsed.div_euclid(24);
    let parts = 1080 * hours_elapsed.rem_euclid(24) + parts_elapsed.rem_euclid(1080);
    let alt_day = if parts >= 19440
        || (day.rem_euclid(7) == 2 && parts >= 9924 && hebrew_leap_year_b(year) == 0)
        || (day.rem_euclid(7) == 1 && parts >= 16789 && hebrew_leap_year_b(year - 1) == 1)
    { day + 1 } else { day };
    if matches!(alt_day.rem_euclid(7), 0 | 3 | 5) { alt_day + 1 } else { alt_day }
}

fn hebrew_leap_year_b(y: i64) -> i64 {
    let r = y.rem_euclid(19);
    if matches!(r, 0 | 3 | 6 | 8 | 11 | 14 | 17) { 1 } else { 0 }
}

/// `fixed_from_hebrew`
fn builtin_fixed_from_hebrew(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let mut sum_months = 0_i64;
    for k in 1..m { sum_months += hebrew_month_length(y, k); }
    let hebrew_epoch_rd: i64 = -1373427;
    Ok(PerlValue::integer(hebrew_epoch_rd + hebrew_elapsed_days(y) + sum_months + d - 1))
}

fn hebrew_month_length(year: i64, month: i64) -> i64 {
    let leap = hebrew_leap_year_b(year) == 1;
    let year_len = hebrew_elapsed_days(year + 1) - hebrew_elapsed_days(year);
    match month {
        1 | 3 | 5 | 7 | 11 => 30,
        4 | 6 | 10 => 29,
        2 => if year_len == 355 || year_len == 385 { 30 } else { 29 },
        8 => 30,
        9 => if year_len == 353 || year_len == 383 { 29 } else { 30 },
        12 => if leap { 30 } else { 29 },
        13 => if leap { 29 } else { 0 },
        _ => 0,
    }
}

/// Islamic (arithmetic, tabular): 30-year cycle with 11 leap years at
/// positions {2, 5, 7, 10, 13, 16, 18, 21, 24, 26, 29}. Months alternate 30/29.
fn builtin_islamic_leap_year(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let r = (14 + 11 * y).rem_euclid(30);
    Ok(PerlValue::integer(if r < 11 { 1 } else { 0 }))
}

/// `fixed_from_islamic`
fn builtin_fixed_from_islamic(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let islamic_epoch_rd: i64 = 227015;
    let n = islamic_epoch_rd - 1
        + (y - 1) * 354
        + (3 + 11 * y).div_euclid(30)
        + 29 * (m - 1) + (m / 2) + d;
    Ok(PerlValue::integer(n))
}

/// Persian (arithmetic / tabular). RD epoch for AP 1 farvardin 1 = 226896.
fn builtin_persian_arithmetic_leap(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let y1 = if y > 0 { y - 474 } else { y - 473 };
    let cycle_year = y1.rem_euclid(2820) + 474;
    let leap = ((cycle_year + 38) * 682).rem_euclid(2816) < 682;
    Ok(PerlValue::integer(if leap { 1 } else { 0 }))
}

/// `fixed_from_persian`
fn builtin_fixed_from_persian(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let persian_epoch_rd: i64 = 226896;
    let y1 = if y > 0 { y - 474 } else { y - 473 };
    let cycle_y = y1.rem_euclid(2820) + 474;
    let month_days = if m <= 7 { 31 * (m - 1) } else { 30 * (m - 1) + 6 };
    let n = persian_epoch_rd - 1
        + 1029983 * y1.div_euclid(2820)
        + 365 * (cycle_y - 1)
        + (682 * cycle_y - 110).div_euclid(2816)
        + month_days + d;
    Ok(PerlValue::integer(n))
}

/// Coptic: epoch RD 103605 = 0001-01-01 Coptic. Leap if year mod 4 == 3.
fn builtin_coptic_from_fixed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let coptic_epoch_rd: i64 = 103605;
    let year = (4 * (n - coptic_epoch_rd) + 1463) / 1461;
    let coptic_year_start = coptic_epoch_rd - 1
        + 365 * (year - 1) + (year / 4);
    let prior = n - coptic_year_start;
    let month = prior / 30 + 1;
    let day = prior - 30 * (month - 1) + 1;
    Ok(PerlValue::integer(year * 10000 + month * 100 + day))
}

/// Ethiopic: RD epoch 2796. Same leap rule as Coptic, year shift 276.
fn builtin_ethiopic_from_fixed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let eth_epoch_rd: i64 = 2796;
    let year = (4 * (n - eth_epoch_rd) + 1463) / 1461;
    let eth_year_start = eth_epoch_rd - 1
        + 365 * (year - 1) + (year / 4);
    let prior = n - eth_year_start;
    let month = prior / 30 + 1;
    let day = prior - 30 * (month - 1) + 1;
    Ok(PerlValue::integer(year * 10000 + month * 100 + day))
}

/// French Revolutionary (arithmetic Romme variant): leap if year mod 4 == 0 (with
/// adjustments for 100, 400, 4000 — equivalent to a modified Gregorian).
fn builtin_french_revolutionary_leap(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let leap = y.rem_euclid(4) == 0
        && (y.rem_euclid(100) != 0 || y.rem_euclid(400) == 0)
        && y.rem_euclid(4000) != 0;
    Ok(PerlValue::integer(if leap { 1 } else { 0 }))
}

/// `fixed_from_french`
fn builtin_fixed_from_french(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let d = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let french_epoch_rd: i64 = 654415;
    let n = french_epoch_rd - 1
        + 365 * (y - 1) + (y - 1) / 4 - (y - 1) / 100 + (y - 1) / 400
        + 30 * (m - 1) + d;
    Ok(PerlValue::integer(n))
}

/// Chinese: 12-animal zodiac sign for year y (relative to 0 BCE = year 1
/// in the proleptic Chinese counting): zodiac index = (y - 4) mod 12, with
/// 0 = Rat, 1 = Ox, 2 = Tiger, ..., 11 = Pig.
fn builtin_chinese_year_zodiac(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    Ok(PerlValue::integer((y - 4).rem_euclid(12)))
}

/// Chinese lunation count following winter solstice (rough astronomical
/// approximation: 12.368 lunations per tropical year × years since 1900-12-21
/// approximate winter solstice).
fn builtin_chinese_lunation_winter(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let solstice_1900: f64 = 693595.0;
    let synodic_month: f64 = 29.530_588_853;
    Ok(PerlValue::integer(((n as f64 - solstice_1900) / synodic_month).floor() as i64))
}

/// Hindu solar year (Old Surya Siddhanta tabular): rd → year given the Saka
/// epoch (RD 84371 for Saka 1 caitra 1) and a 365.25876 day mean year.
fn builtin_hindu_solar_year(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let saka_epoch_rd: f64 = 84371.0;
    let mean_year: f64 = 365.258_756_481_481;
    Ok(PerlValue::integer(((n as f64 - saka_epoch_rd) / mean_year).floor() as i64 + 1))
}

/// Hindu lunisolar tabular month index 1..12 within current solar year.
fn builtin_hindu_lunisolar_month(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let synodic_month: f64 = 29.530_588_853;
    let saka_epoch_rd: f64 = 84371.0;
    let lunations = ((n as f64 - saka_epoch_rd) / synodic_month).floor() as i64;
    Ok(PerlValue::integer(lunations.rem_euclid(12) + 1))
}

/// Maya Long Count baktun.katun.tun.uinal.kin from RD. Epoch (Goodman-
/// Martínez-Thompson): Long Count 0.0.0.0.0 = Aug 11, -3113 Greg = RD -1137142.
fn builtin_maya_long_count_from_fixed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let mlc_epoch: i64 = -1137142;
    let count = n - mlc_epoch;
    let baktun = count.div_euclid(144000);
    let r1 = count.rem_euclid(144000);
    let katun = r1.div_euclid(7200);
    let r2 = r1.rem_euclid(7200);
    let tun = r2.div_euclid(360);
    let r3 = r2.rem_euclid(360);
    let uinal = r3.div_euclid(20);
    let kin = r3.rem_euclid(20);
    Ok(PerlValue::integer(
        baktun * 100000000 + katun * 1000000 + tun * 10000 + uinal * 100 + kin
    ))
}

/// Maya Haab calendar (year of 365 days = 18 × 20-day months + 5-day Wayeb).
/// Returns month_index*100 + day_in_month, 0-indexed.
fn builtin_mayan_haab_from_fixed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let mlc_epoch: i64 = -1137142;
    let count = (n - mlc_epoch + 348).rem_euclid(365);
    let month = count.div_euclid(20);
    let day = count.rem_euclid(20);
    Ok(PerlValue::integer(month * 100 + day))
}

/// Tzolkin: 13 numbers × 20 day-names = 260-day cycle.
fn builtin_mayan_tzolkin_from_fixed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let mlc_epoch: i64 = -1137142;
    let count = n - mlc_epoch;
    let number = (count + 4).rem_euclid(13) + 1;
    let name = (count + 19).rem_euclid(20) + 1;
    Ok(PerlValue::integer(number * 100 + name))
}

/// Bahá'í Badí' calendar: year, 19-month structure of 19 days each + intercalary.
/// Year-from-RD using astronomical Persian epoch shift (RD 673222 = 1844-03-21).
fn builtin_badi_year_from_fixed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let badi_epoch_rd: i64 = 673222;
    Ok(PerlValue::integer((n - badi_epoch_rd).div_euclid(365) + 1))
}

/// `bahai_from_fixed`
fn builtin_bahai_from_fixed(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let badi_epoch_rd: i64 = 673222;
    let year = (n - badi_epoch_rd).div_euclid(365) + 1;
    let day_of_year = (n - badi_epoch_rd).rem_euclid(365);
    let month = if day_of_year >= 342 { 19 } else { day_of_year / 19 + 1 };
    let day = if month == 19 { day_of_year - 342 + 1 } else { day_of_year.rem_euclid(19) + 1 };
    Ok(PerlValue::integer(year * 10000 + month * 100 + day))
}

/// Easter dates per Gauss / Meeus algorithm (Anonymous Gregorian). Returns
/// month*100 + day for the given Gregorian year.
fn builtin_easter_gregorian_year(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let a = y.rem_euclid(19);
    let b = y.div_euclid(100);
    let c = y.rem_euclid(100);
    let d = b.div_euclid(4);
    let e = b.rem_euclid(4);
    let f = (b + 8).div_euclid(25);
    let g = (b - f + 1).div_euclid(3);
    let h = (19 * a + b - d - g + 15).rem_euclid(30);
    let i = c.div_euclid(4);
    let k = c.rem_euclid(4);
    let l = (32 + 2 * e + 2 * i - h - k).rem_euclid(7);
    let m = (a + 11 * h + 22 * l).div_euclid(451);
    let month = (h + l - 7 * m + 114).div_euclid(31);
    let day = (h + l - 7 * m + 114).rem_euclid(31) + 1;
    Ok(PerlValue::integer(month * 100 + day))
}

/// Orthodox (Julian-rule) Easter, via Meeus formula. Returns month*100 + day
/// in the GREGORIAN calendar.
fn builtin_easter_orthodox_year(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let a = y.rem_euclid(4);
    let b = y.rem_euclid(7);
    let c = y.rem_euclid(19);
    let d = (19 * c + 15).rem_euclid(30);
    let e = (2 * a + 4 * b - d + 34).rem_euclid(7);
    let day_of_march = d + e + 114;
    let month_julian = day_of_march.div_euclid(31);
    let day_julian = day_of_march.rem_euclid(31) + 1;
    let julian_n = builtin_fixed_from_julian(&[
        PerlValue::integer(y), PerlValue::integer(month_julian), PerlValue::integer(day_julian)
    ]).unwrap().to_number() as i64;
    let greg = builtin_gregorian_from_fixed(&[PerlValue::integer(julian_n)])
        .unwrap().to_number() as i64;
    Ok(PerlValue::integer((greg / 100) % 10000))
}

/// Easter on the Julian (old-calendar) date: month*100 + day in JULIAN.
fn builtin_easter_julian_year(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let a = y.rem_euclid(4);
    let b = y.rem_euclid(7);
    let c = y.rem_euclid(19);
    let d = (19 * c + 15).rem_euclid(30);
    let e = (2 * a + 4 * b - d + 34).rem_euclid(7);
    let day_of_march = d + e + 114;
    let month = day_of_march.div_euclid(31);
    let day = day_of_march.rem_euclid(31) + 1;
    Ok(PerlValue::integer(month * 100 + day))
}

/// Day of week using Zeller's congruence (Gregorian). Returns 0 = Saturday,
/// 1 = Sunday, ..., 6 = Friday.
fn builtin_day_of_week_zeller(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    let m = args.get(1).map(|v| v.to_number() as i64).unwrap_or(1);
    let q = args.get(2).map(|v| v.to_number() as i64).unwrap_or(1);
    let (year, month) = if m < 3 { (y - 1, m + 12) } else { (y, m) };
    let k = year.rem_euclid(100);
    let j = year.div_euclid(100);
    let h = (q + (13 * (month + 1)).div_euclid(5) + k + k.div_euclid(4)
        + j.div_euclid(4) - 2 * j).rem_euclid(7);
    Ok(PerlValue::integer(h))
}

/// ISO 8601 day number in the week (Mon=1, ..., Sun=7).
fn builtin_iso_day_number(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    Ok(PerlValue::integer((n - 1).rem_euclid(7) + 1))
}

/// English short weekday name index: 0..6 → Sun..Sat.
fn builtin_weekday_name_short(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let n = i1(args);
    let names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let idx = n.rem_euclid(names.len() as i64) as usize;
    Ok(PerlValue::string(names[idx].to_string()))
}

/// Leap year by Gregorian rule.
fn builtin_leap_year_gregorian(args: &[PerlValue]) -> PerlResult<PerlValue> {
    let y = i1(args);
    Ok(PerlValue::integer(if leap_gregorian(y) { 1 } else { 0 }))
}
