//! Subscripts through references and non-bareword keys inside double-quoted
//! strings: `"$$r[0]"` / `"$$h{k}"` / `"$$sref"`, `"@$r[0,1]"` /
//! `"@{$r}[1,2]"`, `"@{$h}{qw(a b)}"`, `"$h{'k'}"` / `"$h{"k"}"` /
//! `"$h{-k}"` / `"$h{ k }"`, and `"$a[$i+1]"`. Each expected string is what
//! `perl` prints for the same code; before this landed `"$$r[0]"` read the
//! named array `@r`, `"$h{'k'}"` looked up the key `'k'` with its quotes,
//! `"@$r[0,1]"` printed `@$r` followed by literal `[0,1]`, and `"$a[0+1]"`
//! used the index `"0+1"` (numified to 0).

use crate::common::*;

#[test]
fn double_dollar_scalar_ref_derefs_in_string() {
    let code = r#"
        my $x = 7;
        my $r = \$x;
        "val=$$r"
    "#;
    assert_eq!(eval_string(code), "val=7");
}

#[test]
fn double_dollar_arrayref_element_in_string() {
    let code = r#"
        my $r = [10, 20, 30];
        "$$r[0] $$r[-1] $$r[1]"
    "#;
    assert_eq!(eval_string(code), "10 30 20");
}

#[test]
fn double_dollar_hashref_element_in_string() {
    let code = r#"
        my $h = { a => 1, b => 2 };
        "$$h{a}-$$h{b} $h->{a} ${$h}{b}"
    "#;
    assert_eq!(eval_string(code), "1-2 1 2");
}

#[test]
fn double_dollar_element_chains_into_nested_subscripts() {
    let code = r#"
        my $d = { k => [ { x => 42 } ] };
        "$$d{k}[0]{x} $$d{k}->[0]->{x}"
    "#;
    assert_eq!(eval_string(code), "42 42");
}

#[test]
fn double_dollar_underscore_brace_is_topic_arrow() {
    // `"$$_{name}"` is `$_->{name}` (the `for` alias is a hashref).
    let code = r#"
        my $out = "";
        for ({ name => "a" }, { name => "b" }) { $out .= "$$_{name}" }
        $out
    "#;
    assert_eq!(eval_string(code), "ab");
}

#[test]
fn double_dollar_alone_is_still_the_process_id() {
    let code = r#"
        my $s = "pid=$$ end";
        $s =~ /^pid=\d+ end$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn array_slice_through_dollar_ref_in_string() {
    let code = r#"
        my $r = [10, 20, 30];
        "@$r[0,1]|@$r[1..2]|@$r"
    "#;
    assert_eq!(eval_string(code), "10 20|20 30|10 20 30");
}

#[test]
fn array_slice_through_braced_ref_in_string() {
    let code = r#"
        my $r = [10, 20, 30];
        "@{$r}[1,2]|@{$r}[0..$#$r]|@{$r}"
    "#;
    assert_eq!(eval_string(code), "20 30|10 20 30|10 20 30");
}

#[test]
fn array_slice_of_hash_value_arrayref_in_string() {
    let code = r#"
        my %h = (k => [1, 2, 3]);
        "els: @{$h{k}} first two: @{$h{k}}[0,1] last: $h{k}[-1] $#{$h{k}}"
    "#;
    assert_eq!(eval_string(code), "els: 1 2 3 first two: 1 2 last: 3 2");
}

#[test]
fn hash_slice_through_ref_in_string_under_compat() {
    // Hash-slice interpolation is a `--compat` behavior, as for named `@h{…}`.
    // Run in a child process so the compat flag never flips in this test binary.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_stryke"))
        .args([
            "--compat",
            "-e",
            r#"my $h = { a => 1, b => 2 }; print "@{$h}{qw(a b)}|@$h{'b'}|@$h{'a','b'}\n";"#,
        ])
        .output()
        .expect("run stryke");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1 2|2|1 2\n");
}

#[test]
fn single_quoted_hash_key_in_string() {
    let code = r#"
        my %h = (k => "v", "a b" => 2, "c-d" => 3);
        "$h{'k'} $h{'a b'} $h{'c-d'}"
    "#;
    assert_eq!(eval_string(code), "v 2 3");
}

#[test]
fn double_quoted_hash_key_in_string() {
    // `qq{}` so the inner `"` does not end the string (Perl rejects `"$h{"k"}"`).
    let code = r#"
        my %h = (k => "v", "a b" => 2);
        qq{$h{"k"} $h{"a b"}}
    "#;
    assert_eq!(eval_string(code), "v 2");
}

#[test]
fn bareword_keys_with_spaces_dash_and_digits_in_string() {
    // `{ k }` is the bareword `k`; `{-neg}` is the string `-neg`; `{5}` is `5`.
    let code = r#"
        my %h = (k => "v", "-neg" => "neg", 5 => "five");
        "$h{ k } $h{-neg} $h{5}"
    "#;
    assert_eq!(eval_string(code), "v neg five");
}

#[test]
fn expression_hash_key_in_string() {
    let code = r#"
        my %h = (ab => 1, AB => 2);
        my $x = "a";
        qq{$h{$x . "b"} $h{uc $x . "B"} $h{"${x}b"}}
    "#;
    assert_eq!(eval_string(code), "1 2 1");
}

#[test]
fn quoted_key_in_arrow_chain_and_named_capture_hash() {
    let code = r#"
        my $r = { k => { 'x y' => 9 } };
        "k=v" =~ /(?<key>\w)=(?<val>\w)/;
        "$r->{'k'}{'x y'} $+{'key'} $+{val}"
    "#;
    assert_eq!(eval_string(code), "9 k v");
}

#[test]
fn arithmetic_array_index_in_string() {
    let code = r#"
        my @a = (1, 2, 3);
        my $i = 1;
        "$a[0+1] $a[$i+1] $a[$i] $a[-1] $a[$#a] $a[ 1 ] $a[$i - 1]"
    "#;
    assert_eq!(eval_string(code), "2 3 2 3 3 2 1");
}

#[test]
fn arithmetic_index_in_arrow_chain_in_string() {
    let code = r#"
        my @aoa = ([1, 2], [3, 4]);
        my $r = \@aoa;
        "$aoa[1][0+1] $r->[0][1-1] $aoa[$#aoa][$#{$aoa[0]}]"
    "#;
    assert_eq!(eval_string(code), "4 1 4");
}

#[test]
fn array_slice_interpolation_leaves_email_at_signs_alone() {
    // `\@` escapes and `@` before a non-name stay literal.
    let code = r#"
        my $r = [1];
        "user\@example.com @ $$r[0]"
    "#;
    assert_eq!(eval_string(code), "user@example.com @ 1");
}
