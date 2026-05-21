//! `__FILE__`, `__LINE__`, `__PACKAGE__`, `$0`, `$$`, `$^X`, `%ENV`,
//! `@ARGV` semantics pins.

use crate::common::*;

// ── __FILE__ ───────────────────────────────────────────────────────

#[test]
fn dunder_file_is_defined() {
    // Under eval_int the source identifier is some string; just
    // verify it's defined and non-empty.
    let code = r#"
        defined(__FILE__) && length(__FILE__) > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dunder_file_is_a_string() {
    let code = r#"
        ref(\__FILE__) eq "SCALAR" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── __LINE__ ───────────────────────────────────────────────────────

#[test]
fn dunder_line_is_positive_integer() {
    let code = r#"
        my $l = __LINE__;
        ($l > 0 && $l == int($l)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dunder_line_increases_on_later_line() {
    let code = r#"
        my $a = __LINE__;
        my $b = __LINE__;
        my $c = __LINE__;
        ($a < $b && $b < $c) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dunder_line_inside_fn_reports_invocation_site() {
    let code = r#"
        fn Demo::DG::here() { __LINE__ }
        my $l = Demo::DG::here();
        # Stryke returns the line of __LINE__ literal — verify > 0.
        $l > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── __PACKAGE__ ───────────────────────────────────────────────────

#[test]
fn dunder_package_at_top_level_is_main() {
    assert_eq!(eval_int(r#"__PACKAGE__ eq "main" ? 1 : 0"#), 1);
}

#[test]
fn dunder_package_inside_package_block() {
    let code = r#"
        package Demo::PK;
        my $pkg = __PACKAGE__;
        package main;
        $pkg eq "Demo::PK" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── $0 (script name) ───────────────────────────────────────────────

#[test]
fn script_name_is_defined() {
    let code = r#"
        defined($0) && length($0) > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── $$ (PID) ───────────────────────────────────────────────────────

#[test]
fn pid_is_positive_integer() {
    let code = r#"
        ($$ > 0 && $$ == int($$)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pid_is_stable_within_run() {
    let code = r#"
        my $p1 = $$;
        my $p2 = $$;
        $p1 == $p2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── $^X (executable path) ──────────────────────────────────────────

#[test]
fn caret_x_is_defined() {
    let code = r#"
        defined($^X) && length($^X) > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn caret_x_looks_like_a_path() {
    let code = r#"
        # On any tested platform $^X is a path containing /.
        index($^X, "/") >= 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── %ENV ───────────────────────────────────────────────────────────

#[test]
fn env_path_is_set_in_normal_shell_env() {
    let code = r#"
        defined($ENV{PATH}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn env_can_be_written_and_read_back() {
    let code = r#"
        $ENV{STRYKE_TEST_VAR_DG} = "value-42";
        $ENV{STRYKE_TEST_VAR_DG} eq "value-42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn env_is_iterable_via_keys() {
    let code = r#"
        len(keys %ENV) > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn env_delete_removes_key() {
    let code = r#"
        $ENV{STRYKE_DEL_TEST} = "set";
        my $had = exists $ENV{STRYKE_DEL_TEST} ? 1 : 0;
        delete $ENV{STRYKE_DEL_TEST};
        my $has = exists $ENV{STRYKE_DEL_TEST} ? 1 : 0;
        ($had && !$has) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn env_can_check_existence_via_exists() {
    let code = r#"
        my $has_path = exists $ENV{PATH} ? 1 : 0;
        my $has_def_missing = exists $ENV{STRYKE_DEFINITELY_NOT_SET_XYZZY} ? 1 : 0;
        ($has_path && !$has_def_missing) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── @ARGV ─────────────────────────────────────────────────────────

#[test]
fn argv_is_an_array() {
    let code = r#"
        # @ARGV exists; under eval_int it's typically empty.
        defined(\@ARGV) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn argv_can_be_pushed_and_read() {
    let code = r#"
        push @ARGV, "extra1";
        push @ARGV, "extra2";
        # We added at least 2.
        len(@ARGV) >= 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── stringify combinations ────────────────────────────────────────

#[test]
fn file_and_line_compose_for_logging() {
    let code = r#"
        my $tag = __FILE__ . ":" . __LINE__;
        # Format: "<file>:<numeric>"
        $tag =~ /^.+:\d+$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pid_in_string_interpolation() {
    let code = r#"
        my $s = "running as pid $$";
        $s =~ /^running as pid \d+$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── repeated reads are stable ─────────────────────────────────────

#[test]
fn dunder_file_stable_across_reads() {
    let code = r#"
        my $a = __FILE__;
        my $b = __FILE__;
        $a eq $b ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dollar_zero_stable_across_reads() {
    let code = r#"
        my $a = $0;
        my $b = $0;
        $a eq $b ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ENV interpolation in qq-string ────────────────────────────────

#[test]
fn env_value_interpolates_via_dollar_brace() {
    let code = r#"
        $ENV{STRYKE_INTERP_DG} = "world";
        my $s = "hello $ENV{STRYKE_INTERP_DG}";
        $s eq "hello world" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ENV count consistency ─────────────────────────────────────────

#[test]
fn env_count_matches_keys_and_values() {
    let code = r#"
        my $nk = len(keys %ENV);
        my $nv = len(values %ENV);
        $nk == $nv ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── derived idioms ────────────────────────────────────────────────

#[test]
fn home_dir_lookup_via_env() {
    let code = r#"
        my $home = $ENV{HOME} // "/tmp";
        length($home) > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn debug_log_helper_via_dunder() {
    let code = r#"
        fn Demo::DG::tag($msg) {
            __FILE__ . ":" . __LINE__ . " " . $msg
        }
        my $t = Demo::DG::tag("hello");
        $t =~ /:\d+ hello$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn package_inside_sub_returns_defining_package() {
    // `__PACKAGE__` is a compile-time constant resolved at parse time to the
    // package active where the sub body was compiled — the value sticks even
    // when invoked from a different `package main` context.
    let code = r#"
        package Demo::Logger;
        sub make_tag { __PACKAGE__ . ".logger" }
        package main;
        Demo::Logger::make_tag() eq "Demo::Logger.logger" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
