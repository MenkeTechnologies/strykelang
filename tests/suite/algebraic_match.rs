//! Algebraic `match (EXPR) { PATTERN => EXPR, ... }` (stryke extension).

use stryke::ast::{ExprKind, MatchPattern, StmtKind};
use stryke::parse;
use stryke::run;

#[test]
fn parse_algebraic_match_shape() {
    let p = parse(
        r#"my $x = match ($y) {
        /^\d+$/ => "number",
        _ => "other",
    };"#,
    )
    .expect("parse");
    let stmt = &p.statements[0];
    let StmtKind::My(decls) = &stmt.kind else {
        panic!("expected my");
    };
    let d0 = decls.first().expect("one decl");
    let Some(init) = &d0.initializer else {
        panic!("initializer");
    };
    let ExprKind::AlgebraicMatch { subject, arms } = &init.kind else {
        panic!("expected AlgebraicMatch, got {:?}", init.kind);
    };
    assert!(matches!(subject.kind, ExprKind::ScalarVar(_)));
    assert_eq!(arms.len(), 2);
    assert!(matches!(arms[0].pattern, MatchPattern::Regex { .. }));
    assert!(matches!(arms[1].pattern, MatchPattern::Any));
}

#[test]
fn match_regex_literal_arm() {
    let v = run(r#"my $r = match ("42") {
        /^\d+$/ => "number",
        _ => "other",
    };
    $r;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "number");
}

#[test]
fn match_regex_arm_sets_topic_and_numbered_captures() {
    let v = run(r#"my $r = match ("ab-12") {
        /^(\w+)-(\d+)$/ => $1 . " " . $2,
        _ => "no",
    };
    $r;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "ab 12");
}

#[test]
fn match_wildcard_arm_sets_underscore_to_subject() {
    let v = run(r#"my $r = match ("hello") {
        _ => $_,
    };
    $r;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "hello");
}

#[test]
fn match_array_prefix_and_rest() {
    let v = run(r#"my $a = [1, 2, 9];
    my $r = match ($a) {
        [1, 2, *] => "ok",
        _ => "no",
    };
    $r;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "ok");
}

#[test]
fn match_hash_capture_and_interpolation() {
    let v = run(r#"my $h = { name => "Alice" };
    my $r = match ($h) {
        { name => $n } => "has name: " . $n,
        _ => "other",
    };
    $r;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "has name: Alice");
}

#[test]
fn match_non_exhaustive_errors() {
    let e = run(r#"match (1) {
        2 => "two",
    }"#)
    .expect_err("should fail");
    let msg = e.to_string();
    assert!(
        msg.contains("match") || msg.contains("no arm"),
        "unexpected: {}",
        msg
    );
}

#[test]
fn match_arm_guard_if_rejects_then_falls_through() {
    let v = run(r#"my $r = match (5) {
        _ if $_ > 10 => "big",
        _ => "small",
    };
    $r;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "small");
}

#[test]
fn match_arm_guard_if_accepts() {
    let v = run(r#"my $r = match (15) {
        _ if $_ > 10 => "big",
        _ => "small",
    };
    $r;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "big");
}

#[test]
fn if_let_array_head_tail_bind() {
    let v = run(r#"my @list = (7, 8, 9);
        my $out = "";
        if let [$h, @t] = \@list {
            $out = $h . ":" . join(",", @t);
        }
        $out;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "7:8,9");
}

#[test]
fn if_let_else_runs_when_pattern_fails() {
    let v = run(r#"my @list = ();
        my $r = "";
        if let [$h, @t] = \@list {
            $r = "many";
        } else {
            $r = "short";
        }
        $r;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "short");
}

#[test]
fn while_let_drains_stack() {
    let v = run(r#"my @stack = (10, 20, 30);
        my $s = 0;
        while let [$top, *] = \@stack {
            $s = $s + $top;
            shift @stack;
        }
        $s;
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 60);
}

#[test]
fn if_let_rhs_plain_at_list_binds_like_array_ref() {
    let v = run(r#"my @list = (7, 8, 9);
        my $out = "";
        if let [$h, @t] = @list {
            $out = $h . ":" . join(",", @t);
        }
        $out;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "7:8,9");
}

#[test]
fn while_let_some_matches_generator_next_pair() {
    let v = run(r#"my $g = gen { yield 10; yield 20; };
        my $s = 0;
        while let Some($x) = $g->next {
            $s = $s + $x;
        }
        $s;
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 30);
}

#[test]
fn match_bare_array_var_subject_prefix_rest() {
    let v = run(r#"my @a = (1, 2, 9);
        my $r = match (@a) {
            [1, 2, *] => "ok",
            _ => "no",
        };
        $r;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "ok");
}

#[test]
fn if_let_bare_hash_subject_key_capture() {
    let v = run(r#"my %H;
        $H{role} = "admin";
        my $out = "";
        if let { role => $r } = %H {
            $out = $r;
        }
        $out;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "admin");
}

#[test]
fn if_let_some_binds_first_cell_when_more_truthy() {
    let v = run(r#"my $pair = [7, 1];
        my $v = -1;
        if let Some($x) = $pair {
            $v = $x;
        }
        $v;
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 7);
}

#[test]
fn if_let_some_else_when_more_falsy() {
    let v = run(r#"my $pair = [99, 0];
        my $v = "";
        if let Some($x) = $pair {
            $v = "some:" . $x;
        } else {
            $v = "none";
        }
        $v;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "none");
}

#[test]
fn match_arm_some_pattern_on_pair() {
    let v = run(r#"my $r = match ([3, 1]) {
            Some($n) => $n * 2,
            _ => 0,
        };
        $r;
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 6);
}

#[test]
fn while_let_some_empty_generator_never_enters_body() {
    let v = run(r#"my $g = gen { };
        my $n = 0;
        while let Some($x) = $g->next {
            $n = $n + 1;
        }
        $n;
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 0);
}

#[test]
#[ignore = "Flow::Last from AST match arm cannot propagate to VM bytecode loop"]
fn parenless_zero_arg_method_then_block_not_consumed_as_arg() {
    // Regression: `->next` must not slurp `{` as a hash/method argument.
    let v = run(r#"my $g = gen { yield 1; };
        my $got = 0;
        if (1) {
            while let Some($x) = $g->next {
                $got = $x;
                last;
            }
        }
        $got;
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 1);
}

#[test]
fn if_let_some_no_match_for_scalar_subject() {
    let v = run(r#"my $v = "ok";
        if let Some($x) = 42 {
            $v = "bad";
        }
        $v;
    "#)
    .expect("run");
    assert_eq!(v.to_string(), "ok");
}

#[test]
fn if_let_some_no_match_for_single_element_array() {
    let v = run(r#"my $one = [99];
        my $v = 1;
        if let Some($x) = $one {
            $v = $x;
        }
        $v;
    "#)
    .expect("run");
    assert_eq!(v.to_int(), 1);
}
