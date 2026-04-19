//! Runtime `pack` / `unpack` through the interpreter (complements `pack::` unit tests).

use crate::common::*;

#[test]
fn pack_unpack_n_roundtrip_scalar() {
    assert_eq!(
        eval_int(r#"scalar unpack 'N', pack 'N', 305419896"#),
        305419896
    );
}

#[test]
fn pack_unpack_v_roundtrip_le() {
    assert_eq!(
        eval_int(r#"scalar unpack 'V', pack 'V', 0x04030201"#),
        0x04030201
    );
}

#[test]
fn pack_c_star_length_matches_arg_count() {
    assert_eq!(eval_int(r#"length pack 'C*', 65, 66, 67"#), 3);
}

#[test]
fn unpack_c_star_first_byte_roundtrip() {
    assert_eq!(
        eval_int(r#"my @u = unpack 'C*', pack 'C*', 10, 20, 30; $u[0]"#),
        10
    );
}

#[test]
fn pack_a_and_a3_fixed_width() {
    assert_eq!(eval_int(r#"length pack 'a3', 'x'"#), 3);
    assert_eq!(eval_int(r#"length pack 'A3', 'x'"#), 3);
}

#[test]
fn pack_z_null_terminated() {
    assert_eq!(eval_int(r#"length pack 'Z', 'hi'"#), 3);
}

#[test]
fn pack_x_inserts_nul_padding() {
    assert_eq!(eval_int(r#"ord substr pack('x2 C', 9), 2, 1"#), 9);
}

#[test]
fn unpack_n_star_multiple_words() {
    let code = r#"
        my $b = pack 'N*', 100, 200;
        my @u = unpack 'N*', $b;
        scalar @u == 2 && $u[0] == 100 && $u[1] == 200 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pack_n2_two_words_big_endian() {
    let code = r#"
        my $b = pack 'N2', 1, 2;
        length($b) == 8 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pack_q_preserves_negative() {
    assert_eq!(eval_int(r#"scalar unpack 'q', pack 'q', -999"#), -999);
}

#[test]
fn pack_unpack_errors_are_runtime() {
    use stryke::error::ErrorKind;
    let program = stryke::parse(r#"unpack 'N', "12""#).expect("parse");
    let mut interp = stryke::interpreter::Interpreter::new();
    let e = interp.execute(&program).expect_err("too short for N");
    assert_eq!(e.kind, ErrorKind::Runtime);
}
