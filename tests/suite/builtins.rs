use crate::common::*;
use forge::interpreter::Interpreter;

#[test]
fn array_ref() {
    assert_eq!(eval_int("my $r = [1,2,3]; $r->[1]"), 2);
}

#[test]
fn hash_ref() {
    assert_eq!(eval_int("my $r = {a => 1, b => 2}; $r->{b}"), 2);
}

#[test]
fn defined_undef() {
    assert_eq!(eval_int("defined(42)"), 1);
    assert_eq!(eval_int("defined(undef)"), 0);
}

#[test]
fn ref_type() {
    assert_eq!(eval_string(r#"ref([])"#), "ARRAY");
    assert_eq!(eval_string(r#"ref({})"#), "HASH");
    assert_eq!(eval_string(r#"ref(\42)"#), "SCALAR");
}

#[test]
fn bless_ref_type() {
    assert_eq!(eval_string(r#"ref(bless({}, "MyClass"))"#), "MyClass");
}

#[test]
fn eval_string_code() {
    assert_eq!(eval_int(r#"eval("2 + 2")"#), 4);
}

#[test]
fn wantarray_undef() {
    assert_eq!(eval_int("wantarray"), 0);
}

#[test]
fn take_first_n_from_list() {
    assert_eq!(eval_string(r#"join "-", take(qw(a b c d), 2)"#), "a-b");
    assert_eq!(eval_string(r#"join "", take(1, 2, 3, 10)"#), "123");
}

#[test]
fn take_scalar_context_last_of_head_like_list_util() {
    assert_eq!(eval_string(r#"scalar take(qw(a b c d e), 3)"#), "c");
    assert_eq!(eval_int(r#"defined(scalar take(1, 2, 0)) ? 1 : 0"#), 0);
}

#[test]
fn take_pipe_forward_inserts_list_before_n() {
    assert_eq!(eval_string(r#"qw(x y z) |> take 2 |> join ''"#), "xy");
}

#[test]
fn head_matches_take_positive_count() {
    assert_eq!(eval_string(r#"qw(a b c d) |> head 2 |> join '-'"#), "a-b");
    assert_eq!(eval_string(r#"scalar head(qw(a b c d e), 3)"#), "c");
}

#[test]
fn head_pipe_forward_same_as_take() {
    assert_eq!(eval_string(r#"qw(x y z) |> head 2 |> join ''"#), "xy");
}

#[test]
fn tail_last_n_from_list_negative_clamps_empty() {
    assert_eq!(eval_string(r#"qw(a b c d) |> tail 2 |> join '-'"#), "c-d");
    assert_eq!(eval_string(r#"scalar tail(qw(a b c d), 2)"#), "d");
    assert_eq!(eval_string(r#"(1, 2, 3) |> tail 10 |> join ''"#), "123");
    assert_eq!(eval_int(r#"defined(scalar tail(1, 2, -1)) ? 1 : 0"#), 0);
}

#[test]
fn tail_multiline_string_splits_lines() {
    assert_eq!(eval_string(r#""a\nb\nc" |> tail 2 |> join '/'"#), "b/c");
    assert_eq!(eval_string(r#"scalar tail("x\ny\nz", 1)"#), "z");
}

#[test]
fn drop_skips_first_n_pipe_and_lines() {
    assert_eq!(eval_string(r#"qw(a b c d) |> drop 2 |> join '-'"#), "c-d");
    assert_eq!(eval_string(r#"scalar drop(qw(a b c d e), 2)"#), "e");
    assert_eq!(eval_string(r#""a\nb\nc" |> drop 1 |> join '/'"#), "b/c");
    assert_eq!(eval_string(r#"qw(x y z) |> tail 2 |> join ''"#), "yz");
    assert_eq!(eval_string(r#"qw(x y z) |> drop 1 |> join ''"#), "yz");
}

#[test]
fn caller_builtin() {
    assert_eq!(eval_string(r#"caller() |> join ','"#), "main,-e,1");
}

/// `ssh LIST` runs the real `ssh` binary (argv only, no shell). No-op when `ssh` is missing.
#[cfg(unix)]
#[test]
fn ssh_builtin_matches_system_for_version_flag() {
    use std::process::Command;
    if !Command::new("ssh")
        .arg("-V")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        return;
    }
    assert_eq!(eval_int(r#"ssh("-V")"#), 0);
    assert_eq!(eval_int(r#"ssh("-V") == system("ssh -V") ? 1 : 0"#), 1);
}

#[test]
fn package_sets_package_glob() {
    assert_eq!(eval_string(r#"package Foo::Bar; $__PACKAGE__"#), "Foo::Bar");
}

#[test]
fn use_strict_noop() {
    assert_eq!(eval_int("use strict; 1"), 1);
}

#[test]
fn require_strict_noop() {
    assert_eq!(eval_int("require strict; 1"), 1);
}

#[test]
fn say_returns_true() {
    assert_eq!(eval_int("say 0"), 1);
}

#[test]
fn numeric_functions() {
    assert_eq!(eval_int("abs(-5)"), 5);
    assert_eq!(eval_int("int(3.7)"), 3);
    assert_eq!(eval_int("hex('ff')"), 255);
    assert_eq!(eval_int("oct('77')"), 63);
    assert_eq!(eval_string("chr(65)"), "A");
    assert_eq!(eval_int("ord('A')"), 65);
}

#[test]
fn die_in_eval() {
    let code = r#"eval { die "test error\n" }; $@ eq "test error\n" ? 1 : 0"#;
    let program = forge::parse(code).expect("parse failed");
    let mut interp = Interpreter::new();
    let result = interp.execute(&program);
    assert!(result.is_ok());
}

#[test]
fn ref_anon_sub_is_code() {
    assert_eq!(eval_string(r#"ref(fn { 1 })"#), "CODE");
}

#[test]
fn builtin_time_epoch_sane() {
    assert!(eval_int("time()") > 1_000_000_000);
}

#[test]
fn localtime_scalar_ends_with_newline() {
    let s = eval_string("scalar localtime(1234567890)");
    assert!(s.ends_with('\n'), "got {:?}", s);
}

#[test]
fn localtime_list_via_join_has_eight_commas() {
    let s = eval_string(r#"localtime(1234567890) |> join ','"#);
    assert_eq!(s.matches(',').count(), 8, "{s}");
}

#[test]
fn gmtime_list_via_join_has_eight_commas() {
    let s = eval_string(r#"gmtime(1234567890) |> join ','"#);
    assert_eq!(s.matches(',').count(), 8, "{s}");
}

#[cfg(unix)]
#[test]
fn getprotobyname_tcp_is_six() {
    assert_eq!(eval_int(r#"scalar getprotobyname("tcp")"#), 6);
}

#[cfg(unix)]
#[test]
fn getservbyname_http_tcp_is_eighty() {
    assert_eq!(eval_int(r#"scalar getservbyname("http", "tcp")"#), 80);
}

#[cfg(unix)]
#[test]
fn getppid_positive() {
    assert!(eval_int("getppid()") > 0);
}

#[cfg(unix)]
#[test]
fn getpriority_current_process() {
    let p = eval_int("getpriority(0, 0)");
    assert!((-20..=20).contains(&p), "nice value {p}");
}

#[test]
fn stringify_list_to_forge_literal() {
    assert_eq!(eval_string("str (1, 2, 3)"), "(1, 2, 3)");
}

#[test]
fn stringify_hash_ref_to_forge_literal() {
    assert_eq!(eval_string("str {a => 1}"), "+{a => 1}");
}

#[test]
fn stringify_array_ref_to_forge_literal() {
    assert_eq!(eval_string("str [1, 2]"), "[1, 2]");
}

#[test]
fn stringify_nested_structure() {
    assert_eq!(
        eval_string(r#"str {x => [1, 2], y => "hi"}"#),
        r#"+{x => [1, 2], y => "hi"}"#
    );
}

#[test]
fn stringify_escapes_special_chars() {
    assert_eq!(eval_string(r#"str 'line1\nline2'"#), r#""line1\\nline2""#);
}

#[test]
fn stringify_undef() {
    assert_eq!(eval_string("str undef"), "undef");
}

#[test]
fn stringify_in_pipeline() {
    assert_eq!(eval_string("(1..3) |> str"), "(1, 2, 3)");
}

#[test]
fn stringify_via_thread_macro() {
    assert_eq!(eval_string("thread (1,2,3) str"), "(1, 2, 3)");
}

#[test]
fn stringify_eval_roundtrip_hashref() {
    assert_eq!(
        eval_int(r#"my $s = str {a => 42}; my $h = eval $s; $h->{a}"#),
        42
    );
}

#[test]
fn stringify_eval_roundtrip_nested() {
    assert_eq!(
        eval_int(r#"my $s = str [1, {x => 99}]; my $a = eval $s; $a->[1]->{x}"#),
        99
    );
}

#[test]
fn stringify_coderef_simple() {
    let s = eval_string("str fn {$_ + 1}");
    assert!(s.contains("sub"));
    assert!(s.contains("$_ + 1"));
}

#[test]
fn stringify_coderef_with_params() {
    let s = eval_string("str fn ($x) { $x * 2 }");
    assert!(s.contains("sub"));
    assert!(s.contains("$x"));
    assert!(s.contains("$x * 2"));
}

#[test]
fn stringify_coderef_roundtrip() {
    assert_eq!(
        eval_int(r#"my $f = fn {$_ + 1}; my $s = str $f; my $g = eval $s; $g->(5)"#),
        6
    );
}

#[test]
fn stringify_coderef_with_params_roundtrip() {
    assert_eq!(
        eval_int(r#"my $f = fn ($a, $b) { $a * $b }; my $s = str $f; my $g = eval $s; $g->(6, 7)"#),
        42
    );
}

#[test]
fn stringify_coderef_with_control_flow_roundtrip() {
    assert_eq!(
        eval_string(
            r#"my $f = fn { if ($_ > 0) { "pos" } else { "neg" } }; my $s = str $f; my $g = eval $s; $g->(1)"#
        ),
        "pos"
    );
}
