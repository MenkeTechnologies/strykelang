//! Behavior-pinning batch BJ (2026-05-08): String transformations, Encoding, and Roman numerals.

use crate::common::*;

// ── String Transformations ──────────────────────────────────────────────────

#[test]
fn string_transformations_bj() {
    assert_eq!(eval_string(r#"acronym("stryke language")"#), "SL");
    assert_eq!(eval_string(r#"atbash("abcXYZ")"#), "zyxCBA");
    assert_eq!(eval_string(r#"pig_latin("stryke")"#), "estrykay");
    assert_eq!(eval_string(r#"leetspeak("hello world")"#), "h3ll0 w0rld");
    
    // zalgo is non-deterministic or at least visually messy, 
    // but we can check if it returns a longer string.
    let z = eval_string(r#"zalgo("abc")"#);
    assert!(z.len() > 3);
}

// ── Encoding & Phonetics ────────────────────────────────────────────────────

#[test]
fn encoding_phonetics_bj() {
    assert_eq!(eval_string(r#"morse_encode("SOS")"#), "... --- ...");
    assert_eq!(eval_string(r#"morse_decode("... --- ...")"#), "SOS");
    
    assert_eq!(eval_string(r#"nato_phonetic("stk")"#), "Sierra Tango Kilo");
    
    // braille_encode("abc") -> ⠁⠃⠉
    assert_eq!(eval_string(r#"braille_encode("abc")"#), "⠁⠃⠉");
}

// ── Roman Numerals ──────────────────────────────────────────────────────────

#[test]
fn roman_numerals_bj() {
    assert_eq!(eval_string("int_to_roman(2026)"), "MMXXVI");
    assert_eq!(eval_int(r#"roman_to_int("MMXXVI")"#), 2026);
    assert_eq!(eval_string(r#"roman_add("X", "V")"#), "XV"); // X + V = XV
}

// ── Base & Gray Code ────────────────────────────────────────────────────────

#[test]
fn base_gray_code_bj() {
    // base_convert(num, from_base, to_base)
    assert_eq!(eval_string("base_convert(255, 10, 16)"), "ff");
    assert_eq!(eval_string("base_convert('FF', 16, 10)"), "255");
    
    // binary_to_gray / gray_to_binary
    assert_eq!(eval_int("binary_to_gray(10)"), 15); // 1010 -> 1111 (binary 15)
    assert_eq!(eval_int("gray_to_binary(15)"), 10);
    
    let res = eval("gray_code_sequence(3)").as_array_vec().unwrap();
    // 000, 001, 011, 010, 110, 111, 101, 100 -> 0, 1, 3, 2, 6, 7, 5, 4
    assert_eq!(res.iter().map(|v| v.to_int()).collect::<Vec<_>>(), vec![0, 1, 3, 2, 6, 7, 5, 4]);
}
