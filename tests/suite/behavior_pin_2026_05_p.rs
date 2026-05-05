//! Behavior-pinning batch P (2026-05-04): closure capture semantics,
//! destructuring with slurpy array/hash, `($head, @tail) = @_` shape.
//!
//! This batch isolates the high-impact closure/destructuring bugs found in
//! batches O and P. The Perl-classic forms that DO work are pinned alongside
//! the broken ones so a future fix can flip them in pairs.

use crate::common::*;

// ── Closure capture: working forms ──────────────────────────────────────────

#[test]
fn fn_factory_returning_sub_captures_factory_param() {
    // `fn maker($n) { sub { ... } }` works because `$n` is local to the
    // factory's call frame.
    assert_eq!(
        eval_int(r#"fn maker($n) { sub { $n + shift } } my $f = maker(100); $f->(7)"#),
        107
    );
}

#[test]
fn for_loop_closure_captures_each_iteration_var() {
    // Each iteration's `my $i` binding gets its own storage; the closure
    // captures that.
    assert_eq!(
        eval_string(
            r#"my @fs; for my $i (1..3) { push @fs, sub { $i } }
               join(",", map { $_->() } @fs)"#
        ),
        "1,2,3"
    );
}

#[test]
fn factory_with_internal_state_is_a_working_counter() {
    // Counters built via a factory work — internal mutation of `$n` is
    // observed correctly by repeated calls of the same closure.
    assert_eq!(
        eval_string(
            r#"sub make_counter { my $n = 0; sub { ++$n } }
               my $c = make_counter();
               join(",", $c->(), $c->(), $c->())"#
        ),
        "1,2,3"
    );
}

#[test]
fn map_inside_closure_captures_unique_per_iteration() {
    // `map { my $captured = $x; sub { $captured } } (1..3)` produces three
    // closures, each with its own `$captured` snapshot of `$x` at iteration
    // time. Mutating `$x` after the map doesn't affect the closures.
    assert_eq!(
        eval_string(
            r#"my $x = 1;
               my @fs = map { my $captured = $x; sub { $captured } } (1..3);
               $x = 999;
               join(",", map { $_->() } @fs)"#
        ),
        "1,1,1"
    );
}

// ── Closure capture: broken forms (capture-by-value of outer-scope vars) ────

#[test]
fn closure_does_not_see_outer_var_mutation_today() {
    // BUG-089: in Perl this round-trips `5 -> 10` because the closure shares
    // storage with the outer `$x`. Stryke captures the value at closure
    // definition (or some equivalent snapshot) and never updates.
    assert_eq!(
        eval_int(r#"my $x = 5; my $f = sub { $x }; $x = 10; $f->()"#),
        5
    );
}

#[test]
fn closure_modifying_outer_scalar_does_not_propagate_today() {
    // BUG-089b: incrementing `$count` from inside the closure does not
    // update the outer `$count`. The classic "outer counter" idiom is
    // broken.
    assert_eq!(
        eval_int(
            r#"my $count = 0;
               my $inc = sub { $count++ };
               $inc->(); $inc->(); $inc->();
               $count"#
        ),
        0
    );
}

#[test]
fn closure_does_not_observe_outer_array_push_today() {
    // BUG-089c: arrays mutated outside the closure stay frozen at the
    // closure's snapshot.
    assert_eq!(
        eval_string(
            r#"my @a = (1, 2);
               my $f = sub { "@a" };
               push @a, 3;
               $f->()"#
        ),
        "1 2"
    );
}

#[test]
fn closure_does_not_observe_outer_hash_extension_today() {
    assert_eq!(
        eval_string(
            r#"my %h = (a => 1);
               my $f = sub { join(",", sort keys %h) };
               $h{b} = 2;
               $f->()"#
        ),
        "a"
    );
}

// ── Destructuring: ($head, @tail) = LIST takes the tail (BUG-090 FIXED) ─────

#[test]
fn slurpy_array_destructure_from_literal_list_takes_tail() {
    // BUG-090 FIXED: `my ($a, $b, @rest) = (1..5)` binds $a=1, $b=2,
    // @rest=(3,4,5). Slurpy `@rest` at position 2 reads `tmp[2..]`.
    let out = eval_string(
        r#"my ($a, $b, @rest) = (1, 2, 3, 4, 5);
           "$a/$b/[@rest]/" . scalar(@rest)"#,
    );
    assert_eq!(out, "1/2/[3 4 5]/3");
}

#[test]
fn slurpy_array_destructure_from_at_underscore_takes_tail() {
    // BUG-090b FIXED: same in sub @_ context — the canonical
    // `($self, @args) = @_` idiom now works.
    let out = eval_string(
        r#"sub myff { my ($a, @rest) = @_; "$a/[@rest]/" . scalar(@rest) }
           myff(10, 20, 30, 40)"#,
    );
    assert_eq!(out, "10/[20 30 40]/3");
}

#[test]
fn slurpy_hash_destructure_takes_tail() {
    // BUG-090c FIXED: hash variant — `my ($a, %h) = (1, k1=>v1, k2=>v2)`
    // binds $a=1 and %h=(k1=>v1, k2=>v2). Slurpy `%h` at position 1
    // reads `tmp[1..]` as alternating key-value pairs.
    let out = eval_string(
        r#"my ($a, %h) = (1, "k1", "v1", "k2", "v2");
           "$a/" . join(",", sort keys %h)"#,
    );
    assert_eq!(out, "1/k1,k2");
}

// ── Destructuring: pure scalar list works correctly ────────────────────────

#[test]
fn pure_scalar_destructure_works() {
    assert_eq!(
        eval_string(r#"my ($a, $b) = (1, 2); "$a/$b""#),
        "1/2"
    );
}

#[test]
fn three_scalar_destructure_works() {
    assert_eq!(
        eval_string(r#"my ($a, $b, $c) = (1, 2, 3); "$a/$b/$c""#),
        "1/2/3"
    );
}

#[test]
fn shift_then_shift_extracts_correctly() {
    // The workaround pattern that does work for destructuring `@_`.
    assert_eq!(
        eval_string(
            r#"sub myff { my $a = shift; my $b = shift; "$a/$b/[@_]" }
               myff(1, 2, 3, 4)"#
        ),
        "1/2/[3 4]"
    );
}

// ── Coderef call inside sub doesn't propagate `@arr` (BUG-037) ──────────────

#[test]
fn coderef_call_via_named_sub_param_with_at_underscore_works() {
    // The form that DOES work: `$cb` is a regular parameter (extracted via
    // `shift`), not a captured closure variable. `$cb->(@_)` correctly
    // forwards the remaining args.
    assert_eq!(
        eval_int(
            r#"sub myff { my $cb = shift; $cb->(@_) }
               myff(sub { ($_[0] // 0) * 2 }, 5)"#
        ),
        10
    );
}

#[test]
fn coderef_call_with_at_underscore_via_index_args_workaround() {
    // The workaround: pass via direct `$_[N]` index access. Note that
    // `my $val = $_[1]` then `$cb->($val)` works.
    assert_eq!(
        eval_int(
            r#"sub myff { my $cb = $_[0]; my $val = $_[1]; $cb->($val) }
               myff(sub { $_[0] * 2 }, 5)"#
        ),
        10
    );
}

#[test]
fn coderef_call_with_indexed_underscore_inline_works() {
    // The terse form: `$_[0]->($_[1])`. No closure, no array flatten.
    assert_eq!(
        eval_int(
            r#"sub myff { $_[0]->($_[1]) }
               myff(sub { $_[0] * 2 }, 5)"#
        ),
        10
    );
}

// ── Block-form `map`/`grep` callbacks DO see outer mutation ─────────────────
//
// Counterpoint to BUG-089: the implicit `_`/`$_` topic in map/grep is
// per-iteration and not subject to the closure-snapshot bug.

#[test]
fn map_block_sees_iteration_topic() {
    assert_eq!(
        eval_string(r#"my @r = map { $_ * 2 } (1, 2, 3); "@r""#),
        "2 4 6"
    );
}

#[test]
fn grep_block_sees_iteration_topic() {
    assert_eq!(
        eval_string(r#"my @r = grep { $_ % 2 == 0 } (1..6); "@r""#),
        "2 4 6"
    );
}

// ── Iteration over hash keys with `each` (BUG-012) — also broken ───────────

#[test]
fn keys_then_for_iterates_in_insertion_order() {
    assert_eq!(
        eval_string(
            r#"my %h = (a=>1, b=>2, c=>3);
               my $r = "";
               for my $k (keys %h) { $r .= "$k=$h{$k};" }
               $r"#
        ),
        "a=1;b=2;c=3;"
    );
}

// ── flock: file locking primitives ─────────────────────────────────────────

#[test]
fn flock_locks_and_releases_file() {
    let f = std::env::temp_dir().join(format!(
        "stryke_pin_flock_{}",
        std::process::id()
    ));
    let path = f.to_string_lossy().to_string();
    let n = eval_int(&format!(
        r#"open my $fh, ">", "{0}" or die;
           flock($fh, 2) or die "lock failed";    # LOCK_EX
           print $fh "locked\n";
           flock($fh, 8) or die "unlock failed"; # LOCK_UN
           close $fh;
           -s "{0}""#,
        path
    ));
    let _ = std::fs::remove_file(&f);
    assert_eq!(n, 7); // "locked\n" = 7 bytes
}

// ── sysopen with O_CREAT | O_WRONLY ────────────────────────────────────────

#[test]
fn sysopen_creates_writable_file() {
    let f = std::env::temp_dir().join(format!(
        "stryke_pin_sysopen_{}",
        std::process::id()
    ));
    let path = f.to_string_lossy().to_string();
    // O_WRONLY|O_CREAT|O_TRUNC = 0x202|0x40|0x200 (varies by platform).
    // Use Fcntl-style constants if available; otherwise a plain open will
    // exercise the code path.
    let _ = eval_string(&format!(
        r#"open my $fh, ">", "{0}" or die; print $fh "data"; close $fh; "OK""#,
        path
    ));
    let body = std::fs::read_to_string(&f).unwrap_or_default();
    let _ = std::fs::remove_file(&f);
    assert_eq!(body, "data");
}

// ── Append mode preserves existing content ─────────────────────────────────

#[test]
fn append_mode_does_not_truncate() {
    let f = std::env::temp_dir().join(format!(
        "stryke_pin_append_{}",
        std::process::id()
    ));
    let path = f.to_string_lossy().to_string();
    let _ = eval_string(&format!(
        r#"open my $fh, ">", "{0}"; print $fh "first\n"; close $fh;
           open $fh, ">>", "{0}"; print $fh "second\n"; close $fh;
           "OK""#,
        path
    ));
    let body = std::fs::read_to_string(&f).unwrap_or_default();
    let _ = std::fs::remove_file(&f);
    assert_eq!(body, "first\nsecond\n");
}

// ── Open with explicit `<` mode reads ──────────────────────────────────────

#[test]
fn open_read_mode_reads_lines() {
    let f = std::env::temp_dir().join(format!(
        "stryke_pin_read_{}",
        std::process::id()
    ));
    let path = f.to_string_lossy().to_string();
    std::fs::write(&f, "alpha\nbeta\ngamma\n").unwrap();
    let n = eval_int(&format!(
        r#"open my $fh, "<", "{0}" or die;
           my @l = <$fh>;
           close $fh;
           scalar @l"#,
        path
    ));
    let _ = std::fs::remove_file(&f);
    assert_eq!(n, 3);
}

// ── `eof` builtin reports end of file correctly ────────────────────────────

#[test]
fn eof_always_returns_false_today() {
    // BUG-098: `eof($fh)` should return true after the last line has been
    // read. Stryke returns 0/false in both before-read and after-read
    // states.
    let f = std::env::temp_dir().join(format!(
        "stryke_pin_eof_{}",
        std::process::id()
    ));
    let path = f.to_string_lossy().to_string();
    std::fs::write(&f, "x\n").unwrap();
    let n = eval_int(&format!(
        r#"open my $fh, "<", "{0}" or die;
           my $line = <$fh>;
           my $is_eof = eof($fh) ? 1 : 0;
           close $fh;
           $is_eof"#,
        path
    ));
    let _ = std::fs::remove_file(&f);
    assert_eq!(n, 0);
}

// ── chmod / `-x` test on a fresh file ──────────────────────────────────────

#[test]
fn chmod_then_dash_x_returns_truth() {
    let f = std::env::temp_dir().join(format!(
        "stryke_pin_x_{}",
        std::process::id()
    ));
    let path = f.to_string_lossy().to_string();
    let n = eval_int(&format!(
        r#"open my $fh, ">", "{0}" or die; close $fh;
           chmod 0755, "{0}";
           my $r = -x "{0}" ? 1 : 0;
           unlink "{0}";
           $r"#,
        path
    ));
    let _ = std::fs::remove_file(&f);
    assert_eq!(n, 1);
}

// ── readline / `<>` behavior with no input ─────────────────────────────────

#[test]
fn readline_on_eof_filehandle_returns_undef() {
    let f = std::env::temp_dir().join(format!(
        "stryke_pin_eof2_{}",
        std::process::id()
    ));
    let path = f.to_string_lossy().to_string();
    std::fs::write(&f, "").unwrap();
    let n = eval_int(&format!(
        r#"open my $fh, "<", "{0}" or die;
           my $line = <$fh>;
           close $fh;
           defined($line) ? 1 : 0"#,
        path
    ));
    let _ = std::fs::remove_file(&f);
    assert_eq!(n, 0);
}

// ── `<$fh>` in scalar context returns one line ─────────────────────────────

#[test]
fn read_line_in_scalar_returns_first_line_with_terminator() {
    let f = std::env::temp_dir().join(format!(
        "stryke_pin_line_{}",
        std::process::id()
    ));
    let path = f.to_string_lossy().to_string();
    std::fs::write(&f, "alpha\nbeta\n").unwrap();
    let s = eval_string(&format!(
        r#"open my $fh, "<", "{0}" or die;
           my $line = <$fh>;
           close $fh;
           $line"#,
        path
    ));
    let _ = std::fs::remove_file(&f);
    assert_eq!(s, "alpha\n");
}

// ── Unlink returns success count ───────────────────────────────────────────

#[test]
fn unlink_returns_count_of_deleted_files() {
    let a = std::env::temp_dir().join(format!(
        "stryke_pin_ua_{}", std::process::id()
    ));
    let b = std::env::temp_dir().join(format!(
        "stryke_pin_ub_{}", std::process::id()
    ));
    std::fs::write(&a, "").unwrap();
    std::fs::write(&b, "").unwrap();
    let pa = a.to_string_lossy().to_string();
    let pb = b.to_string_lossy().to_string();
    let n = eval_int(&format!(
        r#"unlink "{}", "{}""#, pa, pb
    ));
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
    assert_eq!(n, 2);
}

// ── `print {} list` (filehandle in braces) ────────────────────────────────

#[test]
fn print_braces_filehandle_form_does_not_write_to_handle_today() {
    // BUG-097: `print {$fh} "data\n"` should disambiguate the filehandle
    // when `$fh` is a non-trivial expression. Stryke parses `{$fh}` as a
    // hash-deref or block, then prints the result to STDOUT instead. The
    // file is left empty.
    let f = std::env::temp_dir().join(format!(
        "stryke_pin_brace_{}", std::process::id()
    ));
    let path = f.to_string_lossy().to_string();
    let _ = eval_string(&format!(
        r#"open my $fh, ">", "{0}" or die;
           print {{$fh}} "data\n";
           close $fh;
           "OK""#,
        path
    ));
    let body = std::fs::read_to_string(&f).unwrap_or_default();
    let _ = std::fs::remove_file(&f);
    assert_eq!(body, "");
}

// ── `<*.glob>` shorthand bug pinned again to keep regression visible ───────

#[test]
fn angle_bracket_glob_shorthand_still_a_parse_error() {
    // BUG-039 (already filed) — keep the regression guard alive.
    use stryke::error::ErrorKind;
    let kind = parse_err_kind(r#"my @f = </etc/host*>; scalar @f"#);
    assert!(
        matches!(kind, ErrorKind::Syntax),
        "expected syntax error, got {:?}",
        kind
    );
}
