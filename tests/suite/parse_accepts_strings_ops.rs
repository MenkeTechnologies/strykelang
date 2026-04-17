//! String and comparison operators — parse acceptance (explicit tests; no batching).

fn p(src: &str) {
    perlrs::parse(src).unwrap_or_else(|e| panic!("parse failed for {src:?}: {e}"));
}

#[test]
fn str_eq() {
    p(r#""a" eq "b""#);
}

#[test]
fn str_ne() {
    p(r#""a" ne "b""#);
}

#[test]
fn str_lt() {
    p(r#""a" lt "b""#);
}

#[test]
fn str_gt() {
    p(r#""b" gt "a""#);
}

#[test]
fn str_le() {
    p(r#""a" le "b""#);
}

#[test]
fn str_ge() {
    p(r#""b" ge "a""#);
}

#[test]
fn str_cmp() {
    p(r#""a" cmp "b""#);
}

#[test]
fn num_eq_ne_lt() {
    p("1 == 2");
    p("1 != 2");
    p("1 < 2");
}

#[test]
fn num_gt_le_ge() {
    p("2 > 1");
    p("1 <= 2");
    p("2 >= 2");
}

#[test]
fn spaceship_pair() {
    p("1 <=> 2");
    p("2 <=> 2");
}

#[test]
fn concat_two_literals() {
    p(r#""x" . "y""#);
}

#[test]
fn concat_assign_expanded() {
    p(r#"my $s = "a"; $s = $s . "b""#);
}

#[test]
fn lc_uc_pair() {
    p("lc 'HELLO'");
    p("uc 'hello'");
}

#[test]
fn lcfirst_ucfirst_pair() {
    p("lcfirst 'Hello'");
    p("ucfirst 'hello'");
}

#[test]
fn index_rindex_pair() {
    p("index 'abc', 'b'");
    p("rindex 'aba', 'a'");
}

#[test]
fn sprintf_two_args() {
    p(r#"sprintf "%d-%s", 1, "x""#);
}
