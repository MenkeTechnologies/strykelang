//! Reference-value checks for the Reingold & Dershowitz calendrical builtins
//! (`strykelang/math_wolfram_calendrical_reingold_dershowitz.rs`).
//!
//! Expected values come from "Calendrical Calculations" sample data (R.D.
//! 710347 = 1945-11-12 Gregorian) and from independently known dates
//! (Easter tables, Hebrew new-year dates, the 13.0.0.0.0 Maya baktun ending
//! on 2012-12-21). Packed results follow the builtins' documented
//! `year*10000 + month*100 + day` layout.

use crate::common::*;

/// Gregorian date → R.D. via the builtin under test.
fn rd(y: i64, m: i64, d: i64) -> i64 {
    eval_int(&format!("fixed_from_gregorian({y}, {m}, {d})"))
}

#[test]
fn gregorian_epoch_and_sample_date() {
    assert_eq!(rd(1, 1, 1), 1);
    assert_eq!(rd(1945, 11, 12), 710347);
    assert_eq!(eval_int("gregorian_from_fixed(1)"), 10101);
    assert_eq!(eval_int("gregorian_from_fixed(710347)"), 19451112);
}

#[test]
fn gregorian_month_and_leap_boundaries() {
    // Feb 29 exists in 2000 and 2024, not in 1900 / 2100.
    assert_eq!(rd(2000, 3, 1) - rd(2000, 2, 28), 2);
    assert_eq!(rd(2024, 3, 1) - rd(2024, 2, 28), 2);
    assert_eq!(rd(1900, 3, 1) - rd(1900, 2, 28), 1);
    assert_eq!(rd(2100, 3, 1) - rd(2100, 2, 28), 1);
    assert_eq!(rd(2001, 1, 1) - rd(2000, 1, 1), 366);
    assert_eq!(rd(1901, 1, 1) - rd(1900, 1, 1), 365);
    // 400-year cycle is exactly 146097 days.
    assert_eq!(rd(2401, 1, 1) - rd(2001, 1, 1), 146097);
}

#[test]
fn gregorian_negative_year_uses_floor_division() {
    // R&D sample: R.D. -214193 = 24 July -586 (proleptic, astronomical years).
    assert_eq!(rd(-586, 7, 24), -214193);
    assert_eq!(rd(0, 12, 31), 0);
    assert_eq!(rd(0, 1, 1), -365); // year 0 is a leap year (divisible by 400)
    assert_eq!(
        eval_int("gregorian_from_fixed(-214193)"),
        -586 * 10000 + 7 * 100 + 24
    );
}

#[test]
fn gregorian_round_trip_across_four_centuries() {
    // Every 37th day across 1900..2300 covers all months, leap days and
    // century boundaries without running ~146k evals.
    let code = r#"
        my $bad = 0;
        my $n = fixed_from_gregorian(1900, 1, 1);
        my $end = fixed_from_gregorian(2300, 12, 31);
        while ($n <= $end) {
            my $g = gregorian_from_fixed($n);
            my $y = int($g / 10000);
            my $m = int(($g % 10000) / 100);
            my $d = $g % 100;
            $bad++ if fixed_from_gregorian($y, $m, $d) != $n;
            $n += 37;
        }
        $bad
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn julian_sample_date_both_directions() {
    // R&D: R.D. 710347 = 30 October 1945 (Julian).
    assert_eq!(eval_int("fixed_from_julian(1945, 10, 30)"), 710347);
    assert_eq!(eval_int("julian_from_fixed(710347)"), 19451030);
    // R.D. 1 (Gregorian 0001-01-01) is Julian 0001-01-03.
    assert_eq!(eval_int("julian_from_fixed(1)"), 10103);
    assert_eq!(eval_int("fixed_from_julian(1, 1, 1)"), -1);
}

#[test]
fn julian_gregorian_offset_is_13_days_in_20th_century() {
    // 1 Jan 1900 Gregorian .. 28 Feb 2100 Gregorian the calendars differ by 13 days.
    assert_eq!(eval_int("fixed_from_julian(2000, 1, 1)"), rd(2000, 1, 14));
    assert_eq!(eval_int("fixed_from_julian(1918, 1, 31)"), rd(1918, 2, 13));
}

#[test]
fn julian_round_trip_including_bce() {
    let code = r#"
        my $bad = 0;
        my $n = fixed_from_julian(1, 1, 1);
        while ($n <= fixed_from_julian(1600, 12, 31)) {
            my $j = julian_from_fixed($n);
            my $y = int($j / 10000);
            my $m = int(($j % 10000) / 100);
            my $d = $j % 100;
            $bad++ if fixed_from_julian($y, $m, $d) != $n;
            $n += 41;
        }
        $bad
    "#;
    assert_eq!(eval_int(code), 0);
    // Julian leap rule for BCE years: 1 BCE (-1) is leap, so Feb has 29 days.
    assert_eq!(
        eval_int("fixed_from_julian(-1, 3, 1) - fixed_from_julian(-1, 2, 28)"),
        2
    );
    assert_eq!(
        eval_int("fixed_from_julian(1, 1, 1) - fixed_from_julian(-1, 12, 31)"),
        1
    );
}

#[test]
fn iso_week_date_sample_and_year_boundaries() {
    assert_eq!(eval_int("iso_week_date(710347)"), 19454601);
    // 2021-01-03 (Sunday) still belongs to ISO week 2020-W53.
    assert_eq!(eval_int(&format!("iso_week_date({})", rd(2021, 1, 3))), 20205307);
    // 2024-12-30 (Monday) is ISO 2025-W01-1.
    assert_eq!(eval_int(&format!("iso_week_date({})", rd(2024, 12, 30))), 20250101);
    assert_eq!(eval_int(&format!("iso_day_number({})", rd(2024, 1, 1))), 1);
    assert_eq!(eval_int(&format!("iso_day_number({})", rd(2024, 1, 7))), 7);
}

#[test]
fn hebrew_year_lengths_match_observed_new_years() {
    // Rosh Hashanah (Gregorian): 5782 2021-09-07, 5783 2022-09-26,
    // 5784 2023-09-16, 5785 2024-10-03, 5786 2025-09-23.
    let rosh = [
        (5782, rd(2021, 9, 7)),
        (5783, rd(2022, 9, 26)),
        (5784, rd(2023, 9, 16)),
        (5785, rd(2024, 10, 3)),
        (5786, rd(2025, 9, 23)),
    ];
    for w in rosh.windows(2) {
        let (year, start) = w[0];
        let (_, next) = w[1];
        assert_eq!(
            eval_int(&format!("hebrew_year_length({year})")),
            next - start,
            "hebrew_year_length({year})"
        );
    }
}

#[test]
fn hebrew_year_length_is_always_a_legal_value() {
    let code = r#"
        my $bad = 0;
        for my $y (5600:5900) {
            my $len = hebrew_year_length($y);
            my $ok = hebrew_leap_year($y)
                ? ($len == 383 || $len == 384 || $len == 385)
                : ($len == 353 || $len == 354 || $len == 355);
            $bad++ unless $ok;
        }
        $bad
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn hebrew_leap_year_metonic_positions() {
    let leaps: Vec<i64> = (1..=19)
        .filter(|y| eval_int(&format!("hebrew_leap_year({y})")) == 1)
        .collect();
    assert_eq!(leaps, vec![3, 6, 8, 11, 14, 17, 19]);
}

#[test]
fn islamic_sample_date_and_leap_cycle() {
    assert_eq!(eval_int("fixed_from_islamic(1364, 12, 6)"), 710347);
    assert_eq!(eval_int("fixed_from_islamic(1, 1, 1)"), 227015);
    let leaps: Vec<i64> = (1..=30)
        .filter(|y| eval_int(&format!("islamic_leap_year({y})")) == 1)
        .collect();
    assert_eq!(leaps, vec![2, 5, 7, 10, 13, 16, 18, 21, 24, 26, 29]);
    // Leap year 2 has 355 days, common year 3 has 354.
    assert_eq!(eval_int("fixed_from_islamic(3, 1, 1) - fixed_from_islamic(2, 1, 1)"), 355);
    assert_eq!(eval_int("fixed_from_islamic(4, 1, 1) - fixed_from_islamic(3, 1, 1)"), 354);
}

#[test]
fn persian_arithmetic_sample_and_epoch() {
    assert_eq!(eval_int("fixed_from_persian(1324, 8, 21)"), 710347);
    assert_eq!(eval_int("fixed_from_persian(1, 1, 1)"), 226896);
    // Year length must agree with the leap predicate.
    for y in [1399, 1400, 1403, 1404] {
        let len = eval_int(&format!(
            "fixed_from_persian({}, 1, 1) - fixed_from_persian({y}, 1, 1)",
            y + 1
        ));
        let leap = eval_int(&format!("persian_arithmetic_leap({y})"));
        assert_eq!(len, 365 + leap, "persian year {y}");
    }
}

#[test]
fn coptic_and_ethiopic_sample_date() {
    // R&D: R.D. 710347 = Coptic 1662-03-03, Ethiopic 1938-03-03.
    assert_eq!(eval_int("coptic_from_fixed(710347)"), 16620303);
    assert_eq!(eval_int("ethiopic_from_fixed(710347)"), 19380303);
    // Epoch days are the 1st of month 1 of year 1.
    assert_eq!(eval_int("coptic_from_fixed(103605)"), 10101);
    assert_eq!(eval_int("ethiopic_from_fixed(2796)"), 10101);
    // Day before the epoch of Coptic year 2 is the 5th epagomenal day (month 13).
    assert_eq!(eval_int("coptic_from_fixed(103605 + 364)"), 11305);
}

#[test]
fn french_revolutionary_epoch_and_leap_rule() {
    assert_eq!(eval_int("fixed_from_french(1, 1, 1)"), rd(1792, 9, 22));
    assert_eq!(eval_int("french_revolutionary_leap(4)"), 1);
    assert_eq!(eval_int("french_revolutionary_leap(100)"), 0);
    assert_eq!(eval_int("french_revolutionary_leap(400)"), 1);
    assert_eq!(eval_int("french_revolutionary_leap(4000)"), 0);
}

#[test]
fn maya_calendars_at_baktun_13_ending() {
    // 21 Dec 2012 (GMT correlation 584283) = 13.0.0.0.0, 4 Ahau 3 Kankin.
    let n = rd(2012, 12, 21);
    assert_eq!(eval_int(&format!("maya_long_count_from_fixed({n})")), 1300000000);
    // Tzolkin packs number*100 + name; Ahau is name 20.
    assert_eq!(eval_int(&format!("mayan_tzolkin_from_fixed({n})")), 420);
    // Haab packs 0-indexed month*100 + day; Kankin is the 14th month.
    assert_eq!(eval_int(&format!("mayan_haab_from_fixed({n})")), 1303);
    // Long-count epoch itself: 0.0.0.0.0, 4 Ahau 8 Cumku (18th month).
    assert_eq!(eval_int("maya_long_count_from_fixed(-1137142)"), 0);
    assert_eq!(eval_int("mayan_tzolkin_from_fixed(-1137142)"), 420);
    assert_eq!(eval_int("mayan_haab_from_fixed(-1137142)"), 1708);
}

#[test]
fn tzolkin_has_period_260() {
    let code = r#"
        my $bad = 0;
        for my $n (700000:700300) {
            $bad++ if mayan_tzolkin_from_fixed($n) != mayan_tzolkin_from_fixed($n + 260);
            $bad++ if mayan_tzolkin_from_fixed($n) == mayan_tzolkin_from_fixed($n + 130);
        }
        $bad
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn easter_gregorian_known_dates() {
    for (year, md) in [(1961, 402), (2000, 423), (2008, 323), (2011, 424), (2024, 331), (2025, 420), (2038, 425)] {
        assert_eq!(eval_int(&format!("easter_gregorian_year({year})")), md, "{year}");
    }
}

#[test]
fn easter_orthodox_returns_gregorian_month_day() {
    for (year, md) in [(2008, 427), (2010, 404), (2021, 502), (2024, 505), (2025, 420)] {
        assert_eq!(eval_int(&format!("easter_orthodox_year({year})")), md, "{year}");
    }
}

#[test]
fn easter_julian_is_orthodox_date_on_old_calendar() {
    // 2024: Julian 22 April = Gregorian 5 May.
    assert_eq!(eval_int("easter_julian_year(2024)"), 422);
    let code = r#"
        my $bad = 0;
        for my $y (1950:2099) {
            my $j = easter_julian_year($y);
            my $o = easter_orthodox_year($y);
            my $n = fixed_from_julian($y, int($j / 100), $j % 100);
            my $g = gregorian_from_fixed($n) % 10000;
            $bad++ if $g != $o;
        }
        $bad
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn zeller_matches_rd_weekday() {
    // R.D. 1 is a Monday; Zeller returns 0 = Saturday .. 6 = Friday.
    let code = r#"
        my $bad = 0;
        for my $y (1999:2030) {
            for my $m (1:12) {
                my $n = fixed_from_gregorian($y, $m, 13);
                my $iso = iso_day_number($n);            # Mon=1..Sun=7
                my $z = day_of_week_zeller($y, $m, 13);  # Sat=0..Fri=6
                $bad++ if ($iso + 1) % 7 != $z;
            }
        }
        $bad
    "#;
    assert_eq!(eval_int(code), 0);
    assert_eq!(eval_int("day_of_week_zeller(2000, 1, 1)"), 0);
}

#[test]
fn weekday_name_short_wraps_negative_indices() {
    assert_eq!(eval_string("weekday_name_short(0)"), "Sun");
    assert_eq!(eval_string("weekday_name_short(6)"), "Sat");
    assert_eq!(eval_string("weekday_name_short(7)"), "Sun");
    assert_eq!(eval_string("weekday_name_short(-1)"), "Sat");
}

#[test]
fn chinese_zodiac_known_years() {
    // 0 = Rat .. 11 = Pig; 2020 Rat, 2024 Dragon, 1984 Rat, 2023 Rabbit.
    assert_eq!(eval_int("chinese_year_zodiac(2020)"), 0);
    assert_eq!(eval_int("chinese_year_zodiac(1984)"), 0);
    assert_eq!(eval_int("chinese_year_zodiac(2023)"), 3);
    assert_eq!(eval_int("chinese_year_zodiac(2024)"), 4);
    assert_eq!(eval_int("chinese_year_zodiac(2019)"), 11);
}
