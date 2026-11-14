//! Reference pins for the identifier / barcode / SemVer validators in
//! `builtins_validate.rs`, exercised through the language.
//!
//! Sources for expected values:
//! - Luhn: Wikipedia "Luhn algorithm" (`7992739871` -> check digit 3);
//!   IMEI `49015420323751` + check 8 from Wikipedia "IMEI".
//! - ISBN: Wikipedia "ISBN" (`0-306-40615-2` / `978-0-306-40615-7`,
//!   `0-8044-2957-X`).
//! - EAN-13 `4006381333931`, UPC-A `036000291452`: Wikipedia examples.
//! - VIN `1M8GDM9AXKP042788` (check digit X): Wikipedia "Vehicle
//!   identification number".
//! - SemVer: semver.org 2.0.0 item 11 precedence chain.
//! Every numeric check digit was re-derived with an independent Perl
//! implementation on the dev machine.

use crate::common::*;

fn yes(code: &str) -> bool {
    eval_int(&format!("({code}) ? 1 : 0")) == 1
}

// ── Luhn / IMEI ────────────────────────────────────────────────────

#[test]
fn luhn_digit_wikipedia_example() {
    assert_eq!(eval_int(r#"luhn_digit("7992739871")"#), 3);
}

#[test]
fn luhn_digit_ignores_separators() {
    assert_eq!(eval_int(r#"luhn_digit("7992-7398 71")"#), 3);
}

#[test]
fn luhn_digit_empty_is_undef() {
    assert!(yes(r#"!defined luhn_digit("")"#));
    assert!(yes(r#"!defined luhn_digit("abc")"#));
}

/// Appending `luhn_digit(x)` must always yield a Luhn-valid number.
#[test]
fn luhn_digit_completes_a_valid_number() {
    let code = r#"
        my $ok = 1;
        for my $p ("0", "1", "18", "4992739871", "123456789012345", "99999999") {
            $ok = 0 unless luhn_check($p . luhn_digit($p));
        }
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn imei_wikipedia_example() {
    assert!(yes(r#"is_imei("490154203237518")"#));
    assert!(yes(r#"is_imei("49-015420-323751-8")"#));
    assert!(!yes(r#"is_imei("490154203237517")"#));
    // Valid Luhn but wrong length.
    assert!(!yes(r#"is_imei("79927398713")"#));
}

// ── ISBN ───────────────────────────────────────────────────────────

#[test]
fn isbn_valid_and_invalid() {
    assert!(yes(r#"is_isbn("0-306-40615-2")"#));
    assert!(yes(r#"is_isbn("978-0-306-40615-7")"#));
    assert!(yes(r#"is_isbn("0-8044-2957-X")"#));
    assert!(!yes(r#"is_isbn("0-306-40615-3")"#));
    assert!(!yes(r#"is_isbn("978-0-306-40615-8")"#));
    // 'X' is only legal as the ISBN-10 check character.
    assert!(!yes(r#"is_isbn("X-306-40615-2")"#));
}

#[test]
fn isbn10_to_isbn13_reference() {
    assert_eq!(eval_string(r#"isbn10_to_isbn13("0-306-40615-2")"#), "9780306406157");
    assert_eq!(eval_string(r#"isbn10_to_isbn13("080442957X")"#), "9780804429573");
    assert!(yes(r#"!defined isbn10_to_isbn13("0306406153")"#));
}

#[test]
fn isbn13_to_isbn10_reference_and_979_prefix() {
    assert_eq!(eval_string(r#"isbn13_to_isbn10("978-0-306-40615-7")"#), "0306406152");
    assert_eq!(eval_string(r#"isbn13_to_isbn10("9780804429573")"#), "080442957X");
    // 979- ISBNs have no ISBN-10 form.
    assert!(yes(r#"!defined isbn13_to_isbn10("9791234567896")"#));
}

#[test]
fn isbn_conversion_roundtrip() {
    let code = r#"
        my $ok = 1;
        for my $i10 ("0306406152", "080442957X", "0131103628") {
            $ok = 0 unless isbn13_to_isbn10(isbn10_to_isbn13($i10)) eq $i10;
        }
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── EAN-13 / UPC-A ─────────────────────────────────────────────────

#[test]
fn ean13_reference() {
    assert!(yes(r#"is_ean13("4006381333931")"#));
    assert!(!yes(r#"is_ean13("4006381333932")"#));
    assert!(!yes(r#"is_ean13("400638133393")"#));
}

#[test]
fn upc_a_reference() {
    assert!(yes(r#"is_upc("036000291452")"#));
    assert!(!yes(r#"is_upc("036000291453")"#));
    // A UPC-A with a leading 0 is the same barcode as EAN-13.
    assert!(yes(r#"is_ean13("0036000291452")"#));
}

// ── VIN (ISO 3779 / 49 CFR 565) ────────────────────────────────────

#[test]
fn vin_check_digit_x() {
    assert!(yes(r#"is_vin("1M8GDM9AXKP042788")"#));
    assert!(yes(r#"is_vin("1m8gdm9axkp042788")"#));
    assert!(!yes(r#"is_vin("1M8GDM9A1KP042788")"#));
}

#[test]
fn vin_rejects_i_o_q_and_wrong_length() {
    assert!(yes(r#"is_vin("11111111111111111")"#));
    assert!(!yes(r#"is_vin("1111111111111111")"#));
    assert!(!yes(r#"is_vin("I1111111111111111")"#));
    assert!(!yes(r#"is_vin("1111111111111111O")"#));
    assert!(!yes(r#"is_vin("Q1111111111111111")"#));
}

#[test]
fn vin_decode_sections() {
    let code = r#"
        my $h = vin_decode("1M8GDM9AXKP042788");
        join("|", $h->{wmi}, $h->{vds}, $h->{vis}, $h->{plant})
    "#;
    assert_eq!(eval_string(code), "1M8|GDM9AX|KP042788|P");
    assert!(yes(r#"!defined vin_decode("SHORT")"#));
}

// ── BIC / SWIFT ────────────────────────────────────────────────────

#[test]
fn bic_shapes() {
    assert!(yes(r#"is_bic("DEUTDEFF")"#));
    assert!(yes(r#"is_bic("DEUTDEFF500")"#));
    assert!(yes(r#"is_swift("deutdeff")"#));
    assert!(!yes(r#"is_bic("DEUTDEF")"#));
    assert!(!yes(r#"is_bic("DEUTDEFF5")"#));
    assert!(!yes(r#"is_bic("DEU1DEFF")"#));
    assert!(!yes(r#"is_bic("DEUTD3FF")"#));
}

#[test]
fn iban_format_groups_by_four() {
    assert_eq!(
        eval_string(r#"iban_format("gb82west12345698765432")"#),
        "GB82 WEST 1234 5698 7654 32"
    );
    assert_eq!(eval_string(r#"iban_country("gb82 WEST")"#), "GB");
    assert!(yes(r#"!defined iban_country("12345")"#));
}

// ── US SSN / ZIP ───────────────────────────────────────────────────

#[test]
fn ssn_us_rules() {
    assert!(yes(r#"is_ssn_us("123-45-6789")"#));
    assert!(yes(r#"is_ssn_us("123456789")"#));
    assert!(!yes(r#"is_ssn_us("000-45-6789")"#));
    assert!(!yes(r#"is_ssn_us("666-45-6789")"#));
    assert!(!yes(r#"is_ssn_us("900-45-6789")"#));
    assert!(!yes(r#"is_ssn_us("123-00-6789")"#));
    assert!(!yes(r#"is_ssn_us("123-45-0000")"#));
    assert!(!yes(r#"is_ssn_us("123.45.6789")"#));
}

#[test]
fn zip_us_and_plus4() {
    assert!(yes(r#"is_zip_us("02134")"#));
    assert!(!yes(r#"is_zip_us("2134")"#));
    assert!(yes(r#"is_zip_plus4("02134-1234")"#));
    assert!(!yes(r#"is_zip_plus4("02134 1234")"#));
}

// ── SemVer 2.0.0 precedence (item 11) ──────────────────────────────

/// The full example chain from semver.org item 11; every adjacent and
/// every non-adjacent pair must compare `-1`, and the reverse `1`.
#[test]
fn semver_spec_precedence_chain() {
    let code = r#"
        my @v = ("1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta",
                 "1.0.0-beta", "1.0.0-beta.2", "1.0.0-beta.11",
                 "1.0.0-rc.1", "1.0.0", "2.0.0", "2.1.0", "2.1.1");
        my @bad;
        for my $i (0:$#v) {
            for my $j (0:$#v) {
                my $want = $i <=> $j;
                my $got = semver_compare($v[$i], $v[$j]);
                push @bad, "$v[$i] vs $v[$j]=$got" unless $got == $want;
            }
        }
        join(",", @bad)
    "#;
    assert_eq!(eval_string(code), "");
}

/// Numeric identifiers compare numerically, not as strings.
#[test]
fn semver_numeric_prerelease_identifiers() {
    assert_eq!(eval_int(r#"semver_compare("1.0.0-rc.9", "1.0.0-rc.10")"#), -1);
    assert_eq!(eval_int(r#"semver_compare("1.0.0-2", "1.0.0-10")"#), -1);
    // Numeric identifiers have lower precedence than alphanumeric ones.
    assert_eq!(eval_int(r#"semver_compare("1.0.0-999", "1.0.0-a")"#), -1);
}

#[test]
fn semver_build_metadata_ignored() {
    assert_eq!(eval_int(r#"semver_compare("1.0.0+build.1", "1.0.0+build.2")"#), 0);
    assert_eq!(eval_int(r#"semver_compare("1.0.0-rc.1+x", "1.0.0-rc.1")"#), 0);
}

#[test]
fn semver_compare_invalid_is_undef() {
    assert!(yes(r#"!defined semver_compare("1.0", "1.0.0")"#));
    assert!(yes(r#"!defined semver_compare("1.0.0", "x.y.z")"#));
}

#[test]
fn semver_satisfies_operators() {
    assert!(yes(r#"semver_satisfies("1.2.3", ">=1.2.3")"#));
    assert!(!yes(r#"semver_satisfies("1.2.3", ">1.2.3")"#));
    assert!(yes(r#"semver_satisfies("1.2.3", "<=1.2.3")"#));
    assert!(yes(r#"semver_satisfies("1.2.3", "1.2.3")"#));
    assert!(yes(r#"semver_satisfies("1.2.3", "=1.2.3")"#));
    assert!(yes(r#"semver_satisfies("1.2.3", "!=1.2.4")"#));
    // Pre-release sorts below its release.
    assert!(yes(r#"semver_satisfies("2.0.0-rc.1", "<2.0.0")"#));
    assert!(yes(r#"semver_satisfies("1.0.0-beta.11", ">1.0.0-beta.2")"#));
    assert!(yes(r#"!defined semver_satisfies("bogus", ">=1.0.0")"#));
}
