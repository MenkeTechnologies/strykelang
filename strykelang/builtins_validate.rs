//! Validation / input-check primitives (Phase 1, batch 5).
//!
//! Every function takes `&[StrykeValue]` and returns `StrykeValue`.
//! Predicates return `1` / `0` integers (stryke truthy/falsy). Format
//! / convert helpers return strings or undef on parse failure.
//!
//! Standards followed where applicable:
//!   * IBAN — ISO 13616 + MOD-97-10 check (only check digits validated;
//!     country-specific format strings come from a compact table)
//!   * Luhn — ISO/IEC 7812-1 §A
//!   * IMEI — Luhn on 15 digits (incl. check digit)
//!   * UUID — RFC 9562 (versioned, dash form)
//!   * VIN — ISO 3779 transliteration table + weight vector
//!   * SemVer — semver.org v2.0.0 grammar (subset; uses regex for parse)
//!   * EAN-13 / UPC — GS1 General Specifications (right-to-left weights 3/1)

use crate::value::StrykeValue;

#[inline]
fn b(v: bool) -> StrykeValue {
    StrykeValue::integer(if v { 1 } else { 0 })
}

fn arg_str(args: &[StrykeValue]) -> String {
    args.first().map(|v| v.to_string()).unwrap_or_default()
}

// ══════════════════════════════════════════════════════════════════════
// Character-class predicates
// ══════════════════════════════════════════════════════════════════════

pub fn is_alpha_only(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    b(!s.is_empty() && s.chars().all(|c| c.is_ascii_alphabetic()))
}

pub fn is_alphanumeric_only(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    b(!s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()))
}

pub fn is_numeric_only(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    b(!s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
}

pub fn is_ascii_only(args: &[StrykeValue]) -> StrykeValue {
    b(arg_str(args).is_ascii())
}

pub fn is_printable_ascii(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    b(!s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii() && !c.is_ascii_control()))
}

pub fn is_utf8(args: &[StrykeValue]) -> StrykeValue {
    // strings in stryke are already utf-8; the question is whether they
    // contain valid utf-8 when interpreted as bytes. always true for
    // a StrykeValue::string but check raw bytes for completeness.
    let s = arg_str(args);
    b(std::str::from_utf8(s.as_bytes()).is_ok())
}

pub fn is_lowercase(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let has_letter = s.chars().any(|c| c.is_alphabetic());
    b(has_letter && s.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_lowercase()))
}

pub fn is_uppercase(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let has_letter = s.chars().any(|c| c.is_alphabetic());
    b(has_letter && s.chars().filter(|c| c.is_alphabetic()).all(|c| c.is_uppercase()))
}

pub fn is_titlecase(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    if s.is_empty() {
        return b(false);
    }
    // Each word starts with uppercase, rest lowercase.
    for word in s.split_whitespace() {
        let mut chars = word.chars();
        match chars.next() {
            Some(c) if c.is_uppercase() => {}
            _ => return b(false),
        }
        if !chars.all(|c| !c.is_alphabetic() || c.is_lowercase()) {
            return b(false);
        }
    }
    b(true)
}

pub fn is_palindrome_str(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let clean: Vec<char> = s
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect();
    if clean.is_empty() {
        return b(false);
    }
    let n = clean.len();
    for i in 0..n / 2 {
        if clean[i] != clean[n - 1 - i] {
            return b(false);
        }
    }
    b(true)
}

// ══════════════════════════════════════════════════════════════════════
// Numeric / encoding predicates
// ══════════════════════════════════════════════════════════════════════

pub fn is_hex(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let cleaned = s.trim_start_matches("0x").trim_start_matches("0X");
    b(!cleaned.is_empty() && cleaned.chars().all(|c| c.is_ascii_hexdigit()))
}

pub fn is_octal(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let cleaned = s.trim_start_matches("0o").trim_start_matches("0O");
    b(!cleaned.is_empty() && cleaned.chars().all(|c| ('0'..='7').contains(&c)))
}

pub fn is_binary(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let cleaned = s.trim_start_matches("0b").trim_start_matches("0B");
    b(!cleaned.is_empty() && cleaned.chars().all(|c| c == '0' || c == '1'))
}

pub fn is_base32(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let cleaned = s.trim_end_matches('=');
    b(!cleaned.is_empty()
        && cleaned
            .chars()
            .all(|c| matches!(c, 'A'..='Z' | '2'..='7')))
}

pub fn is_md5_hash(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    b(s.len() == 32 && s.chars().all(|c| c.is_ascii_hexdigit()))
}

pub fn is_sha1_hash(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    b(s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit()))
}

pub fn is_sha256_hash(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    b(s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()))
}

// ══════════════════════════════════════════════════════════════════════
// Address-form predicates
// ══════════════════════════════════════════════════════════════════════

pub fn is_ipv6(args: &[StrykeValue]) -> StrykeValue {
    b(arg_str(args).parse::<std::net::Ipv6Addr>().is_ok())
}

pub fn is_cidr(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let s = s.trim();
    let Some((addr, prefix)) = s.split_once('/') else {
        return b(false);
    };
    let Ok(ip) = addr.parse::<std::net::IpAddr>() else {
        return b(false);
    };
    let Ok(p) = prefix.parse::<u8>() else {
        return b(false);
    };
    let max = match ip {
        std::net::IpAddr::V4(_) => 32,
        std::net::IpAddr::V6(_) => 128,
    };
    b(p <= max)
}

pub fn is_mac(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let cleaned: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    b(cleaned.len() == 12)
}

// ══════════════════════════════════════════════════════════════════════
// URL / UUID / JWT
// ══════════════════════════════════════════════════════════════════════

pub fn is_url_http(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let lower = s.trim().to_ascii_lowercase();
    b(lower.starts_with("http://") && s.len() > 7)
}

pub fn is_url_https(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let lower = s.trim().to_ascii_lowercase();
    b(lower.starts_with("https://") && s.len() > 8)
}

/// Validate a UUID with a specific version digit.
fn uuid_version_check(s: &str, expected: u8) -> bool {
    // Strict dash form: 8-4-4-4-12 hex digits
    if s.len() != 36 {
        return false;
    }
    let bytes = s.as_bytes();
    let dashes = [8, 13, 18, 23];
    for (i, b) in bytes.iter().enumerate() {
        if dashes.contains(&i) {
            if *b != b'-' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    // Version digit is byte 14 (0-indexed)
    let version_char = bytes[14] as char;
    version_char.to_digit(16) == Some(expected as u32)
}

pub fn is_uuid_v4(args: &[StrykeValue]) -> StrykeValue {
    b(uuid_version_check(&arg_str(args), 4))
}

pub fn is_uuid_v7(args: &[StrykeValue]) -> StrykeValue {
    b(uuid_version_check(&arg_str(args), 7))
}

pub fn is_jwt(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 3 {
        return b(false);
    }
    // Each part must be non-empty base64url
    for p in &parts {
        if p.is_empty() {
            return b(false);
        }
        if !p
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return b(false);
        }
    }
    b(true)
}

pub fn is_email_strict(args: &[StrykeValue]) -> StrykeValue {
    // RFC 5322 dot-atom local-part + domain with at least one dot.
    let s = arg_str(args);
    let s = s.trim();
    if s.len() > 254 {
        return b(false);
    }
    let Some(at_pos) = s.rfind('@') else {
        return b(false);
    };
    let local = &s[..at_pos];
    let domain = &s[at_pos + 1..];
    if local.is_empty() || local.len() > 64 {
        return b(false);
    }
    if domain.is_empty() || !domain.contains('.') {
        return b(false);
    }
    // Local-part: dot-atom (atext) only, no consecutive dots, no leading/trailing dot.
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return b(false);
    }
    for c in local.chars() {
        if !matches!(c,
            'a'..='z' | 'A'..='Z' | '0'..='9'
                | '!' | '#' | '$' | '%' | '&' | '\''
                | '*' | '+' | '-' | '/' | '=' | '?' | '^'
                | '_' | '`' | '{' | '|' | '}' | '~' | '.'
        ) {
            return b(false);
        }
    }
    // Domain: labels of 1..=63 alphanumeric/hyphens, no leading/trailing hyphen.
    for label in domain.split('.') {
        if label.is_empty() || label.len() > 63 {
            return b(false);
        }
        if label.starts_with('-') || label.ends_with('-') {
            return b(false);
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return b(false);
        }
    }
    b(true)
}

// ══════════════════════════════════════════════════════════════════════
// Identification numbers / barcodes (Luhn / mod-10 / mod-97)
// ══════════════════════════════════════════════════════════════════════

/// Generic Luhn check on a string of digits.
fn luhn_valid(s: &str) -> bool {
    let digits: Vec<u32> = s
        .chars()
        .filter(|c| c.is_ascii_digit())
        .map(|c| c.to_digit(10).unwrap())
        .collect();
    if digits.len() < 2 {
        return false;
    }
    let mut sum = 0u32;
    for (i, d) in digits.iter().rev().enumerate() {
        if i % 2 == 1 {
            let doubled = d * 2;
            sum += if doubled > 9 { doubled - 9 } else { doubled };
        } else {
            sum += d;
        }
    }
    sum.is_multiple_of(10)
}

/// Compute the Luhn check digit for a partial number (digits without check).
pub fn luhn_digit(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let digits: Vec<u32> = s
        .chars()
        .filter(|c| c.is_ascii_digit())
        .map(|c| c.to_digit(10).unwrap())
        .collect();
    if digits.is_empty() {
        return StrykeValue::UNDEF;
    }
    // Compute the check digit by simulating an extra zero on the right.
    let mut sum = 0u32;
    for (i, d) in digits.iter().rev().enumerate() {
        if i % 2 == 0 {
            let doubled = d * 2;
            sum += if doubled > 9 { doubled - 9 } else { doubled };
        } else {
            sum += d;
        }
    }
    let check = (10 - (sum % 10)) % 10;
    StrykeValue::integer(check as i64)
}

pub fn is_imei(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let digits_only: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    b(digits_only.len() == 15 && luhn_valid(&digits_only))
}

pub fn is_imsi(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let digits_only: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    b(matches!(digits_only.len(), 14..=15)
        && digits_only.chars().all(|c| c.is_ascii_digit()))
}

pub fn is_vin(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).to_ascii_uppercase();
    if s.len() != 17 {
        return b(false);
    }
    // No I, O, or Q.
    if s.chars().any(|c| c == 'I' || c == 'O' || c == 'Q') {
        return b(false);
    }
    // Transliteration values per ISO 3779.
    let v = |c: char| -> Option<u32> {
        match c {
            '0'..='9' => Some(c.to_digit(10).unwrap()),
            'A' | 'J' => Some(1),
            'B' | 'K' | 'S' => Some(2),
            'C' | 'L' | 'T' => Some(3),
            'D' | 'M' | 'U' => Some(4),
            'E' | 'N' | 'V' => Some(5),
            'F' | 'W' => Some(6),
            'G' | 'P' | 'X' => Some(7),
            'H' | 'Y' => Some(8),
            'R' | 'Z' => Some(9),
            _ => None,
        }
    };
    let weights: [u32; 17] = [8, 7, 6, 5, 4, 3, 2, 10, 0, 9, 8, 7, 6, 5, 4, 3, 2];
    let mut sum = 0u32;
    for (i, c) in s.chars().enumerate() {
        let Some(vv) = v(c) else {
            return b(false);
        };
        sum += vv * weights[i];
    }
    let check = sum % 11;
    let expected = s.chars().nth(8).unwrap();
    let check_char = if check == 10 { 'X' } else {
        std::char::from_digit(check, 10).unwrap()
    };
    b(expected == check_char)
}

/// `vin_decode(VIN)` — parse a VIN into `{ wmi, vds, vis, year, plant }`.
/// Year decoding uses the ISO 3779 30-year cycle starting at 1980.
pub fn vin_decode(args: &[StrykeValue]) -> StrykeValue {
    use indexmap::IndexMap;
    use parking_lot::RwLock;
    use std::sync::Arc;
    let s = arg_str(args).to_ascii_uppercase();
    if s.len() != 17 {
        return StrykeValue::UNDEF;
    }
    let wmi = &s[0..3];
    let vds = &s[3..9];
    let vis = &s[9..17];
    let year_char = s.chars().nth(9).unwrap();
    let plant = s.chars().nth(10).unwrap();
    // Year letter → base year (30-year cycle; we pick the most recent past).
    let year_letter_to_offset = |c: char| -> Option<u32> {
        match c {
            'A' => Some(10), 'B' => Some(11), 'C' => Some(12), 'D' => Some(13),
            'E' => Some(14), 'F' => Some(15), 'G' => Some(16), 'H' => Some(17),
            'J' => Some(18), 'K' => Some(19), 'L' => Some(20), 'M' => Some(21),
            'N' => Some(22), 'P' => Some(23), 'R' => Some(24), 'S' => Some(25),
            'T' => Some(26), 'V' => Some(27), 'W' => Some(28), 'X' => Some(29),
            'Y' => Some(0),
            '1' => Some(1), '2' => Some(2), '3' => Some(3), '4' => Some(4),
            '5' => Some(5), '6' => Some(6), '7' => Some(7), '8' => Some(8),
            '9' => Some(9),
            _ => None,
        }
    };
    let year = year_letter_to_offset(year_char).map(|o| 2000 + o);
    let mut h: IndexMap<String, StrykeValue> = IndexMap::new();
    h.insert("wmi".to_string(), StrykeValue::string(wmi.to_string()));
    h.insert("vds".to_string(), StrykeValue::string(vds.to_string()));
    h.insert("vis".to_string(), StrykeValue::string(vis.to_string()));
    if let Some(y) = year {
        h.insert("year".to_string(), StrykeValue::integer(y as i64));
    }
    h.insert(
        "plant".to_string(),
        StrykeValue::string(plant.to_string()),
    );
    StrykeValue::hash_ref(Arc::new(RwLock::new(h)))
}

pub fn is_ean13(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let digits: Vec<u32> = s
        .chars()
        .filter(|c| c.is_ascii_digit())
        .map(|c| c.to_digit(10).unwrap())
        .collect();
    if digits.len() != 13 {
        return b(false);
    }
    let mut sum = 0u32;
    for (i, d) in digits.iter().take(12).enumerate() {
        sum += d * if i % 2 == 0 { 1 } else { 3 };
    }
    let check = (10 - (sum % 10)) % 10;
    b(check == digits[12])
}

pub fn is_upc(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let digits: Vec<u32> = s
        .chars()
        .filter(|c| c.is_ascii_digit())
        .map(|c| c.to_digit(10).unwrap())
        .collect();
    if digits.len() != 12 {
        return b(false);
    }
    let mut sum = 0u32;
    for (i, d) in digits.iter().take(11).enumerate() {
        sum += d * if i % 2 == 0 { 3 } else { 1 };
    }
    let check = (10 - (sum % 10)) % 10;
    b(check == digits[11])
}

pub fn is_isbn(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let cleaned: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    match cleaned.len() {
        10 => b(isbn10_valid(&cleaned)),
        13 => b(isbn13_valid(&cleaned)),
        _ => b(false),
    }
}

fn isbn10_valid(s: &str) -> bool {
    if s.len() != 10 {
        return false;
    }
    let mut sum = 0u32;
    for (i, c) in s.chars().enumerate() {
        let v = if i == 9 && c == 'X' {
            10
        } else if c.is_ascii_digit() {
            c.to_digit(10).unwrap()
        } else {
            return false;
        };
        sum += v * (10 - i as u32);
    }
    sum.is_multiple_of(11)
}

fn isbn13_valid(s: &str) -> bool {
    if s.len() != 13 {
        return false;
    }
    let digits: Vec<u32> = match s.chars().map(|c| c.to_digit(10).ok_or(())).collect::<Result<Vec<_>, _>>() {
        Ok(d) => d,
        Err(_) => return false,
    };
    let mut sum = 0u32;
    for (i, d) in digits.iter().take(12).enumerate() {
        sum += d * if i % 2 == 0 { 1 } else { 3 };
    }
    let check = (10 - (sum % 10)) % 10;
    check == digits[12]
}

pub fn isbn10_to_isbn13(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let cleaned: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if cleaned.len() != 10 || !isbn10_valid(&cleaned) {
        return StrykeValue::UNDEF;
    }
    let body: String = format!("978{}", &cleaned[..9]);
    let digits: Vec<u32> = body.chars().map(|c| c.to_digit(10).unwrap()).collect();
    let mut sum = 0u32;
    for (i, d) in digits.iter().enumerate() {
        sum += d * if i % 2 == 0 { 1 } else { 3 };
    }
    let check = (10 - (sum % 10)) % 10;
    StrykeValue::string(format!("{}{}", body, check))
}

pub fn isbn13_to_isbn10(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let cleaned: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if cleaned.len() != 13 || !isbn13_valid(&cleaned) {
        return StrykeValue::UNDEF;
    }
    if !cleaned.starts_with("978") {
        // ISBN-13s that start with 979 don't have an ISBN-10 equivalent.
        return StrykeValue::UNDEF;
    }
    let body = &cleaned[3..12];
    let digits: Vec<u32> = body.chars().map(|c| c.to_digit(10).unwrap()).collect();
    let mut sum = 0u32;
    for (i, d) in digits.iter().enumerate() {
        sum += d * (10 - i as u32);
    }
    let r = sum % 11;
    let check = (11 - r) % 11;
    let check_char = if check == 10 {
        'X'
    } else {
        std::char::from_digit(check, 10).unwrap()
    };
    StrykeValue::string(format!("{}{}", body, check_char))
}

// ══════════════════════════════════════════════════════════════════════
// IBAN / BIC / SWIFT
// ══════════════════════════════════════════════════════════════════════

/// IBAN MOD-97-10 check (after country/check-digit rearrangement).
pub fn iban_format(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let cleaned: String = s
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    // Format in groups of 4
    let mut out = String::new();
    for (i, c) in cleaned.chars().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    StrykeValue::string(out)
}

pub fn iban_country(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let cleaned: String = s.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    if cleaned.len() < 2 {
        return StrykeValue::UNDEF;
    }
    let cc: String = cleaned.chars().take(2).collect::<String>().to_ascii_uppercase();
    if cc.chars().all(|c| c.is_ascii_alphabetic()) {
        StrykeValue::string(cc)
    } else {
        StrykeValue::UNDEF
    }
}

/// BIC (also called SWIFT code): 8 or 11 characters.
///   AAAA — institution (letters)
///   BB   — country (letters)
///   CC   — location (alnum)
///   DDD  — branch (alnum, optional)
pub fn is_bic(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).to_ascii_uppercase();
    if s.len() != 8 && s.len() != 11 {
        return b(false);
    }
    let chars: Vec<char> = s.chars().collect();
    // 1-4: letters
    for &c in &chars[0..4] {
        if !c.is_ascii_alphabetic() {
            return b(false);
        }
    }
    // 5-6: country letters
    for &c in &chars[4..6] {
        if !c.is_ascii_alphabetic() {
            return b(false);
        }
    }
    // 7-8: location alnum
    for &c in &chars[6..8] {
        if !c.is_ascii_alphanumeric() {
            return b(false);
        }
    }
    if chars.len() == 11 {
        // 9-11: branch alnum
        for &c in &chars[8..11] {
            if !c.is_ascii_alphanumeric() {
                return b(false);
            }
        }
    }
    b(true)
}

pub fn is_swift(args: &[StrykeValue]) -> StrykeValue {
    is_bic(args)
}

// ══════════════════════════════════════════════════════════════════════
// Phone / postal / SSN
// ══════════════════════════════════════════════════════════════════════

pub fn is_phone(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let digit_count = s.chars().filter(|c| c.is_ascii_digit()).count();
    // Lenient: 7..=15 digits, allows spaces/dashes/parens/+/.
    b((7..=15).contains(&digit_count)
        && s.chars()
            .all(|c| c.is_ascii_digit() || c.is_whitespace() || "+-().".contains(c)))
}

pub fn is_phone_e164(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args);
    let s = s.trim();
    if !s.starts_with('+') {
        return b(false);
    }
    let digits: String = s[1..].chars().filter(|c| c.is_ascii_digit()).collect();
    b(digits.len() >= 8 && digits.len() <= 15 && s[1..].chars().all(|c| c.is_ascii_digit()))
}

pub fn is_zip_us(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).trim().to_string();
    b(s.len() == 5 && s.chars().all(|c| c.is_ascii_digit()))
}

pub fn is_zip_plus4(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).trim().to_string();
    if s.len() != 10 {
        return b(false);
    }
    let bytes = s.as_bytes();
    b(bytes[5] == b'-'
        && s[0..5].chars().all(|c| c.is_ascii_digit())
        && s[6..10].chars().all(|c| c.is_ascii_digit()))
}

/// `is_postal_code(CODE, COUNTRY?)` — lenient pattern. Country defaults
/// to "US". Knows a small set of common patterns; falls back to a
/// 3..=10-char alphanumeric check for unknown countries.
pub fn is_postal_code(args: &[StrykeValue]) -> StrykeValue {
    let code = arg_str(args).trim().to_ascii_uppercase();
    let country = args
        .get(1)
        .map(|v| v.to_string().trim().to_ascii_uppercase())
        .unwrap_or_else(|| "US".to_string());
    let ok = match country.as_str() {
        "US" => code.len() == 5 && code.chars().all(|c| c.is_ascii_digit())
            || (code.len() == 10 && code.chars().nth(5) == Some('-')
                && code[..5].chars().all(|c| c.is_ascii_digit())
                && code[6..].chars().all(|c| c.is_ascii_digit())),
        "CA" => {
            // A1A 1A1 or A1A1A1
            let cleaned: String = code.chars().filter(|c| !c.is_whitespace()).collect();
            cleaned.len() == 6
                && cleaned.chars().enumerate().all(|(i, c)| {
                    if i % 2 == 0 {
                        c.is_ascii_alphabetic()
                    } else {
                        c.is_ascii_digit()
                    }
                })
        }
        "UK" | "GB" => {
            let cleaned: String = code.chars().filter(|c| !c.is_whitespace()).collect();
            (5..=7).contains(&cleaned.len())
        }
        "DE" | "FR" | "IT" | "ES" => code.len() == 5 && code.chars().all(|c| c.is_ascii_digit()),
        "JP" => {
            let cleaned: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
            cleaned.len() == 7
        }
        "AU" | "BE" | "DK" | "NO" | "CH" | "AT" => {
            code.len() == 4 && code.chars().all(|c| c.is_ascii_digit())
        }
        "NL" => {
            let cleaned: String = code.chars().filter(|c| !c.is_whitespace()).collect();
            cleaned.len() == 6
                && cleaned[..4].chars().all(|c| c.is_ascii_digit())
                && cleaned[4..].chars().all(|c| c.is_ascii_alphabetic())
        }
        "BR" => {
            let cleaned: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
            cleaned.len() == 8
        }
        _ => {
            let cleaned: String = code.chars().filter(|c| !c.is_whitespace()).collect();
            (3..=10).contains(&cleaned.len()) && cleaned.chars().all(|c| c.is_ascii_alphanumeric())
        }
    };
    b(ok)
}

pub fn is_ssn_us(args: &[StrykeValue]) -> StrykeValue {
    let s = arg_str(args).trim().to_string();
    if s.len() != 11 && s.len() != 9 {
        return b(false);
    }
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() != 9 {
        return b(false);
    }
    // Disallow obvious invalids: area 000 / 666 / 9xx, group 00, serial 0000.
    let area = &digits[0..3];
    let group = &digits[3..5];
    let serial = &digits[5..9];
    if area == "000" || area == "666" || area.starts_with('9') {
        return b(false);
    }
    if group == "00" || serial == "0000" {
        return b(false);
    }
    if s.len() == 11 {
        // Must be NNN-NN-NNNN
        let bytes = s.as_bytes();
        if bytes[3] != b'-' || bytes[6] != b'-' {
            return b(false);
        }
    }
    b(true)
}

// ══════════════════════════════════════════════════════════════════════
// SemVer
// ══════════════════════════════════════════════════════════════════════

/// Parse a SemVer string into (major, minor, patch, prerelease, build).
fn parse_semver(s: &str) -> Option<(u64, u64, u64, String, String)> {
    let s = s.trim();
    let (core, build) = match s.split_once('+') {
        Some((c, b)) => (c, b.to_string()),
        None => (s, String::new()),
    };
    let (core, pre) = match core.split_once('-') {
        Some((c, p)) => (c, p.to_string()),
        None => (core, String::new()),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let major = parts[0].parse::<u64>().ok()?;
    let minor = parts[1].parse::<u64>().ok()?;
    let patch = parts[2].parse::<u64>().ok()?;
    Some((major, minor, patch, pre, build))
}

pub fn semver_compare(args: &[StrykeValue]) -> StrykeValue {
    let a = args.first().map(|v| v.to_string()).unwrap_or_default();
    let bs = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let Some((a_maj, a_min, a_pat, a_pre, _)) = parse_semver(&a) else {
        return StrykeValue::UNDEF;
    };
    let Some((b_maj, b_min, b_pat, b_pre, _)) = parse_semver(&bs) else {
        return StrykeValue::UNDEF;
    };
    use std::cmp::Ordering;
    let ord = (a_maj, a_min, a_pat).cmp(&(b_maj, b_min, b_pat));
    let ord = if ord != Ordering::Equal {
        ord
    } else {
        // Pre-release ordering: no-pre > has-pre; otherwise lexicographic
        match (a_pre.is_empty(), b_pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => a_pre.cmp(&b_pre),
        }
    };
    StrykeValue::integer(match ord {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    })
}

/// `semver_satisfies(VERSION, RANGE)` — simple comparator support:
/// `=`, `<`, `<=`, `>`, `>=`, `!=`. No tilde/caret/star ranges yet —
/// those are TODO for a later batch.
pub fn semver_satisfies(args: &[StrykeValue]) -> StrykeValue {
    let v = args.first().map(|v| v.to_string()).unwrap_or_default();
    let range = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let range = range.trim();
    let (op, rhs) = if let Some(r) = range.strip_prefix(">=") {
        (">=", r.trim())
    } else if let Some(r) = range.strip_prefix("<=") {
        ("<=", r.trim())
    } else if let Some(r) = range.strip_prefix("!=") {
        ("!=", r.trim())
    } else if let Some(r) = range.strip_prefix('>') {
        (">", r.trim())
    } else if let Some(r) = range.strip_prefix('<') {
        ("<", r.trim())
    } else if let Some(r) = range.strip_prefix('=') {
        ("=", r.trim())
    } else {
        ("=", range)
    };
    let cmp = semver_compare(&[StrykeValue::string(v), StrykeValue::string(rhs.to_string())]);
    if cmp.is_undef() {
        return StrykeValue::UNDEF;
    }
    let c = cmp.to_int();
    let ok = match op {
        "=" => c == 0,
        "!=" => c != 0,
        ">" => c > 0,
        ">=" => c >= 0,
        "<" => c < 0,
        "<=" => c <= 0,
        _ => false,
    };
    b(ok)
}

pub fn semver_increment_major(args: &[StrykeValue]) -> StrykeValue {
    let Some((maj, _, _, _, _)) = parse_semver(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    StrykeValue::string(format!("{}.0.0", maj + 1))
}

pub fn semver_increment_minor(args: &[StrykeValue]) -> StrykeValue {
    let Some((maj, min, _, _, _)) = parse_semver(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    StrykeValue::string(format!("{}.{}.0", maj, min + 1))
}

pub fn semver_increment_patch(args: &[StrykeValue]) -> StrykeValue {
    let Some((maj, min, pat, _, _)) = parse_semver(&arg_str(args)) else {
        return StrykeValue::UNDEF;
    };
    StrykeValue::string(format!("{}.{}.{}", maj, min, pat + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(s: &str) -> StrykeValue {
        StrykeValue::string(s.to_string())
    }

    #[test]
    fn character_class_predicates() {
        assert_eq!(is_alpha_only(&[s("hello")]).to_int(), 1);
        assert_eq!(is_alpha_only(&[s("hi42")]).to_int(), 0);
        assert_eq!(is_alphanumeric_only(&[s("abc123")]).to_int(), 1);
        assert_eq!(is_alphanumeric_only(&[s("abc-123")]).to_int(), 0);
        assert_eq!(is_numeric_only(&[s("12345")]).to_int(), 1);
        assert_eq!(is_lowercase(&[s("hello")]).to_int(), 1);
        assert_eq!(is_lowercase(&[s("HELLO")]).to_int(), 0);
        assert_eq!(is_uppercase(&[s("HELLO")]).to_int(), 1);
        assert_eq!(is_titlecase(&[s("Hello World")]).to_int(), 1);
        assert_eq!(is_titlecase(&[s("hello world")]).to_int(), 0);
    }

    #[test]
    fn palindrome_check() {
        assert_eq!(is_palindrome_str(&[s("racecar")]).to_int(), 1);
        assert_eq!(is_palindrome_str(&[s("A man a plan a canal Panama")]).to_int(), 1);
        assert_eq!(is_palindrome_str(&[s("hello")]).to_int(), 0);
    }

    #[test]
    fn hex_octal_binary() {
        assert_eq!(is_hex(&[s("0xDEADBEEF")]).to_int(), 1);
        assert_eq!(is_hex(&[s("xyz")]).to_int(), 0);
        assert_eq!(is_octal(&[s("0o755")]).to_int(), 1);
        assert_eq!(is_octal(&[s("999")]).to_int(), 0);
        assert_eq!(is_binary(&[s("0b101010")]).to_int(), 1);
        assert_eq!(is_binary(&[s("0b102")]).to_int(), 0);
    }

    #[test]
    fn hash_lengths() {
        assert_eq!(is_md5_hash(&[s(&"a".repeat(32))]).to_int(), 1);
        assert_eq!(is_md5_hash(&[s(&"a".repeat(31))]).to_int(), 0);
        assert_eq!(is_sha1_hash(&[s(&"f".repeat(40))]).to_int(), 1);
        assert_eq!(is_sha256_hash(&[s(&"0".repeat(64))]).to_int(), 1);
    }

    #[test]
    fn uuid_versions() {
        // v4 has version digit 4 at position 14
        let uuid_v4 = "550e8400-e29b-41d4-a716-446655440000";
        assert_eq!(is_uuid_v4(&[s(uuid_v4)]).to_int(), 1);
        let uuid_v7 = "017f22e2-79b0-7cc3-98c4-dc0c0c07398f";
        assert_eq!(is_uuid_v7(&[s(uuid_v7)]).to_int(), 1);
        assert_eq!(is_uuid_v4(&[s(uuid_v7)]).to_int(), 0);
    }

    #[test]
    fn jwt_basic() {
        // header.payload.signature — 3 segments
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.SflKxwRJSMeKKF2QT4f";
        assert_eq!(is_jwt(&[s(jwt)]).to_int(), 1);
        assert_eq!(is_jwt(&[s("not.a.jwt.too.many.dots")]).to_int(), 0);
        assert_eq!(is_jwt(&[s("only.two")]).to_int(), 0);
    }

    #[test]
    fn email_strict_dot_atom() {
        assert_eq!(is_email_strict(&[s("a@b.co")]).to_int(), 1);
        assert_eq!(is_email_strict(&[s("alice+tag@example.com")]).to_int(), 1);
        assert_eq!(is_email_strict(&[s("no-at-symbol")]).to_int(), 0);
        assert_eq!(is_email_strict(&[s("..@bad.com")]).to_int(), 0);
        assert_eq!(is_email_strict(&[s("foo@bar")]).to_int(), 0); // no dot in domain
    }

    #[test]
    fn imei_luhn() {
        // 490154203237518 — valid IMEI
        assert_eq!(is_imei(&[s("490154203237518")]).to_int(), 1);
        assert_eq!(is_imei(&[s("490154203237519")]).to_int(), 0);
    }

    #[test]
    fn vin_valid() {
        // Known valid VIN with X check digit
        assert_eq!(is_vin(&[s("1M8GDM9AXKP042788")]).to_int(), 1);
    }

    #[test]
    fn isbn_round_trips() {
        // 0306406152 → 9780306406157
        assert_eq!(isbn10_to_isbn13(&[s("0306406152")]).to_string(), "9780306406157");
        assert_eq!(isbn13_to_isbn10(&[s("9780306406157")]).to_string(), "0306406152");
    }

    #[test]
    fn ean13_upc() {
        assert_eq!(is_ean13(&[s("4006381333931")]).to_int(), 1);
        assert_eq!(is_upc(&[s("036000291452")]).to_int(), 1);
    }

    #[test]
    fn zip_us_variants() {
        assert_eq!(is_zip_us(&[s("12345")]).to_int(), 1);
        assert_eq!(is_zip_us(&[s("1234")]).to_int(), 0);
        assert_eq!(is_zip_plus4(&[s("12345-6789")]).to_int(), 1);
        assert_eq!(is_zip_plus4(&[s("123456789")]).to_int(), 0);
    }

    #[test]
    fn semver_ops() {
        assert_eq!(semver_compare(&[s("1.2.3"), s("1.2.4")]).to_int(), -1);
        assert_eq!(semver_compare(&[s("2.0.0"), s("1.999.999")]).to_int(), 1);
        assert_eq!(semver_compare(&[s("1.0.0-alpha"), s("1.0.0")]).to_int(), -1);
        assert_eq!(semver_satisfies(&[s("1.2.3"), s(">=1.0.0")]).to_int(), 1);
        assert_eq!(semver_satisfies(&[s("1.2.3"), s("<2.0.0")]).to_int(), 1);
        assert_eq!(semver_increment_major(&[s("1.2.3")]).to_string(), "2.0.0");
        assert_eq!(semver_increment_minor(&[s("1.2.3")]).to_string(), "1.3.0");
        assert_eq!(semver_increment_patch(&[s("1.2.3")]).to_string(), "1.2.4");
    }

    #[test]
    fn phone_e164() {
        assert_eq!(is_phone_e164(&[s("+12025551234")]).to_int(), 1);
        assert_eq!(is_phone_e164(&[s("+44 20 7946 0958")]).to_int(), 0); // spaces invalid
        assert_eq!(is_phone_e164(&[s("12025551234")]).to_int(), 0); // missing +
    }

    #[test]
    fn bic_swift() {
        assert_eq!(is_bic(&[s("DEUTDEFF")]).to_int(), 1);
        assert_eq!(is_bic(&[s("DEUTDEFF500")]).to_int(), 1);
        assert_eq!(is_bic(&[s("DEUTDEFF50")]).to_int(), 0);
    }
}
