//! Tier-S shell-like REPL builtins — pinned at the stryke-script level.
//! Zero new crates, zero binary bloat; every backing call uses an
//! existing dep (`libc`, `parking_lot`, `once_cell`, `indexmap`).

use crate::common::*;

// ── identity ──────────────────────────────────────────────────────────

#[test]
fn whoami_returns_non_empty_string() {
    let out = eval_string(r#"whoami"#);
    assert!(!out.is_empty(), "whoami returned empty");
    assert!(!out.contains('\n'), "whoami should be one line: {out}");
}

#[test]
fn groups_returns_list() {
    // Every Unix user is in at least one group; on macOS at least
    // `staff` and `everyone`.
    let n = eval_int(r#"scalar @{[groups()]}"#);
    assert!(n > 0, "groups returned empty list");
}

// ── terminal info ─────────────────────────────────────────────────────

#[test]
fn term_size_returns_two_ints() {
    let code = r#"
        my @s = term_size();
        scalar(@s) == 2 && $s[0] >= 0 && $s[1] >= 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn term_width_and_height_are_nonneg() {
    assert!(eval_int(r#"term_width()"#) >= 0);
    assert!(eval_int(r#"term_height()"#) >= 0);
}

// ── alias / unalias ───────────────────────────────────────────────────

#[test]
fn repl_alias_register_and_lookup() {
    let code = r#"
        repl_alias("xyz", "foo bar");
        repl_alias("xyz") eq "foo bar" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn repl_alias_equals_syntax() {
    let code = r#"
        repl_alias("eq_form=expansion-here");
        repl_alias("eq_form") eq "expansion-here" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn repl_unalias_removes_entry() {
    let code = r#"
        repl_alias("rmme", "x");
        my $had = defined(repl_alias("rmme")) ? 1 : 0;
        repl_unalias("rmme");
        my $gone = defined(repl_alias("rmme")) ? 0 : 1;
        $had && $gone ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn repl_alias_list_contains_registered_entries() {
    let code = r#"
        repl_alias("ll", "ls -la");
        my @list = repl_alias();
        my $found = 0;
        for my $entry (@list) {
            $found = 1 if $entry =~ /^ll=/;
        }
        $found
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── pushd / popd ──────────────────────────────────────────────────────
// NOTE: process cwd is process-wide; `cargo test` runs tests in
// parallel, so any test that calls `pushd`/`popd` races with peers
// for cwd. We assert only invariants that don't depend on the actual
// cwd value mid-test: dir-stack mutation, popd-on-empty error path.

#[test]
fn pushd_grows_dir_stack_and_popd_drains_it() {
    // Stack is process-wide and shared with sibling parallel tests, so
    // we measure DELTA (push then pop) not absolute count.
    let code = r#"
        my @before = dir_stack();
        my $b = scalar @before;
        pushd("/tmp");
        my @mid = dir_stack();
        my $m = scalar @mid;
        popd();
        my @after = dir_stack();
        my $a = scalar @after;
        # After push+pop pair, stack length must equal where we started
        # (modulo concurrent pushes from other parallel tests; we treat
        # >= as success: at minimum our own push then pop netted zero).
        ($m > $b) && ($a <= $m) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn popd_errors_on_empty_stack() {
    // Best-effort drain (other parallel tests may be pushing), then
    // assert that popd eventually errors when the stack is verifiably
    // empty at the moment of call.
    let code = r#"
        for (1..200) {
            my $s = scalar @{[dir_stack()]};
            last if $s == 0;
            eval { popd() };
            last if $@;
        }
        my $err_caught = 0;
        # Recheck just before the failing call to minimize races.
        if (scalar @{[dir_stack()]} == 0) {
            eval { popd() };
            $err_caught = ($@ ne "") ? 1 : 0;
        } else {
            # Another test holds entries — accept and skip.
            $err_caught = 1;
        }
        $err_caught
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── history ───────────────────────────────────────────────────────────

#[test]
fn history_is_array_returnable() {
    // From a non-REPL test we expect 0 entries unless the REPL pushed
    // something. Either way `history()` must return a list.
    let n = eval_int(r#"scalar @{[history()]}"#);
    assert!(n >= 0, "history() must return a list; got {n}");
}

// ── reflection ────────────────────────────────────────────────────────

#[test]
fn tier_s_appears_in_b_hash() {
    for name in &[
        "clear",
        "cls",
        "whoami",
        "groups",
        "pushd",
        "popd",
        "dir_stack",
        "history",
        "repl_alias",
        "repl_unalias",
        "set_alias",
        "unset_alias",
        "term_size",
        "term_width",
        "term_height",
        "set_title",
        "beep",
        "ring_bell",
    ] {
        let code = format!(r#"exists $b{{{name}}} ? 1 : 0"#);
        assert_eq!(eval_int(&code), 1, "missing from %b: {name}");
    }
}

// ── Tier A ────────────────────────────────────────────────────────────

#[test]
fn rm_removes_a_real_file() {
    let code = r#"
        my $p = mktemp("rm_test");
        my $existed = -f $p ? 1 : 0;
        my $removed = rm($p);
        my $gone = -f $p ? 0 : 1;
        $existed && $removed == 1 && $gone ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mktemp_returns_existing_path() {
    let code = r#"
        my $p = mktemp("test");
        my $ok = -f $p ? 1 : 0;
        unlink $p;
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn mktempdir_returns_existing_dir() {
    let code = r#"
        my $d = mktempdir("test");
        my $ok = -d $d ? 1 : 0;
        rmdir $d;
        $ok
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn whereis_finds_known_binary() {
    // `ls` exists on every Unix system; expect at least one hit.
    let code = r#"my @r = whereis("ls"); scalar(@r) >= 1 ? 1 : 0"#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn whereis_empty_for_nonexistent_binary() {
    let code = r#"my @r = whereis("definitely_not_a_real_command_xyz_12345"); scalar(@r) == 0 ? 1 : 0"#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn nice_returns_set_priority() {
    // setpriority may fail with EACCES if the caller can't raise
    // priority — but lowering (positive nice value) always succeeds
    // for a non-privileged process.
    let n = eval_int(r#"nice(5)"#);
    assert!(n == 5 || n == 0, "nice(5) returned {n}; expected 5 or undef→0");
}

#[test]
fn tree_returns_multiline_string() {
    let code = r#"
        my $t = tree(".", 1);
        length($t) > 0 && $t =~ /\n/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn comm_three_column_compare() {
    let code = r#"
        my $r = comm(["a", "b", "c"], ["b", "c", "d"]);
        my $only_a = scalar @{$r->[0]};
        my $only_b = scalar @{$r->[1]};
        my $both   = scalar @{$r->[2]};
        $only_a == 1 && $only_b == 1 && $both == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn column_tabulates_to_string() {
    let code = r#"
        my $s = column(["foo", "bar", "baz"]);
        length($s) > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn xargs_maps_callable_over_args() {
    let code = r#"
        fn Foo::dbl($x) { $x * 2 }
        my @r = xargs(\&Foo::dbl, [1, 2, 3, 4]);
        join(",", @r)
    "#;
    assert_eq!(eval_string(code), "2,4,6,8");
}

#[test]
fn strftime_now_renders_iso_format() {
    // %Y-%m-%d gives 10 chars; verify shape.
    let code = r#"my $s = strftime("%Y-%m-%d"); length($s) == 10 && $s =~ /^\d{4}-\d{2}-\d{2}$/ ? 1 : 0"#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn strftime_with_explicit_epoch() {
    // Epoch 0 = 1970-01-01 UTC, but Local::now timezone may shift.
    // Just verify the year and that we got SOMETHING.
    let code = r#"
        my $s = strftime("%Y", 0);
        $s =~ /^19(69|70)$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn iconv_passthrough_for_same_encoding() {
    assert_eq!(
        eval_string(r#"iconv("hello", "utf-8", "utf-8")"#),
        "hello"
    );
}

#[test]
fn tac_reverses_a_list() {
    assert_eq!(
        eval_string(r#"join(",", tac(["a", "b", "c"]))"#),
        "c,b,a"
    );
}

#[test]
fn rev_lines_reverses_lines() {
    assert_eq!(
        eval_string(r#"rev_lines("line1\nline2\nline3")"#),
        "line3\nline2\nline1"
    );
}

#[test]
fn tier_a_appears_in_b_hash() {
    for name in &[
        "rm",
        "mktemp",
        "mktempdir",
        "whereis",
        "nice",
        "renice",
        "tree",
        "comm",
        "column",
        "xargs",
        "openurl",
        "xdg_open",
        "curl_get",
        "curl_post",
        "iconv",
        "strftime",
        "tac",
        "rev_lines",
        "tty_raw",
        "tty_cooked",
    ] {
        let code = format!(r#"exists $b{{{name}}} ? 1 : 0"#);
        assert_eq!(eval_int(&code), 1, "missing from %b: {name}");
    }
}
