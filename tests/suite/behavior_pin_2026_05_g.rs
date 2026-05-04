//! Behavior-pinning batch G (2026-05-04): glob/dir, file ops, tie, sort
//! stability, AOP intercept variables, deletion forms.

use crate::common::*;
use std::process::id as pid;

fn tmp_path(label: &str) -> String {
    std::env::temp_dir()
        .join(format!("stryke_pin_{}_{}", label, pid()))
        .to_string_lossy()
        .into_owned()
}

// ── glob() and dir ops ───────────────────────────────────────────────────────

#[test]
fn glob_function_form_lists_matches() {
    // /etc/hosts is present on every macOS+Linux dev host stryke targets.
    assert!(
        eval_int(r#"my @f = glob "/etc/host*"; scalar @f >= 1 ? 1 : 0"#) == 1,
        "expected at least one /etc/host* match"
    );
}

#[test]
fn angle_bracket_glob_form_is_parse_error_today() {
    // BUG-039: `<*.toml>` Perl shorthand is not parsed by stryke.
    use stryke::error::ErrorKind;
    let kind = parse_err_kind(r#"my @f = </etc/host*>; scalar @f"#);
    assert!(
        matches!(kind, ErrorKind::Syntax),
        "expected syntax error, got {:?}",
        kind
    );
}

#[test]
fn opendir_readdir_closedir_cycle() {
    // Use /tmp — we know we can read it on every dev host.
    let n = eval_int(
        r#"opendir my $dh, "/tmp" or die;
           my @e = grep { !/^\./ } readdir $dh;
           closedir $dh;
           scalar @e >= 0 ? 1 : 0"#,
    );
    assert_eq!(n, 1);
}

// ── File ops: mkdir/rmdir/-s/rename/stat/chmod/seek/tell ─────────────────────

#[test]
fn mkdir_creates_then_rmdir_removes() {
    let dir = tmp_path("dir_xx");
    let code = format!(
        r#"mkdir "{0}"; my $a = -d "{0}" ? 1 : 0;
           rmdir "{0}"; my $b = -d "{0}" ? 1 : 0;
           "$a$b""#,
        dir
    );
    assert_eq!(eval_string(&code), "10");
    let _ = std::fs::remove_dir(&dir);
}

#[test]
fn dash_s_returns_size_in_bytes() {
    let f = tmp_path("size_xx");
    let code = format!(
        r#"open my $fh, ">", "{0}" or die; print $fh "abc"; close $fh; my $n = -s "{0}"; unlink "{0}"; $n"#,
        f
    );
    assert_eq!(eval_int(&code), 3);
}

#[test]
fn rename_moves_file() {
    let a = tmp_path("ren_a");
    let b = tmp_path("ren_b");
    let code = format!(
        r#"open my $fh, ">", "{0}" or die; close $fh;
           rename "{0}", "{1}";
           my $a = -e "{0}" ? "Y" : "N";
           my $b = -e "{1}" ? "Y" : "N";
           unlink "{1}";
           "$a$b""#,
        a, b
    );
    assert_eq!(eval_string(&code), "NY");
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
}

#[test]
fn stat_returns_thirteen_fields() {
    assert_eq!(eval_int(r#"my @s = stat "/etc/hosts"; scalar @s"#), 13);
}

#[test]
fn chmod_sets_mode_and_stat_returns_it() {
    let f = tmp_path("chmod_xx");
    let code = format!(
        r#"open my $fh, ">", "{0}" or die; close $fh;
           chmod 0644, "{0}";
           my $mode = (stat "{0}")[2] & 0777;
           unlink "{0}";
           sprintf("%04o", $mode)"#,
        f
    );
    assert_eq!(eval_string(&code), "0644");
    let _ = std::fs::remove_file(&f);
}

#[test]
fn seek_then_tell_then_read_resumes_from_position() {
    let f = tmp_path("seek_xx");
    let code = format!(
        r#"open my $fh, ">", "{0}"; print $fh "hello world"; close $fh;
           open $fh, "<", "{0}";
           seek($fh, 6, 0);
           my $pos = tell($fh);
           my $rest = scalar <$fh>;
           close $fh;
           unlink "{0}";
           "$pos:$rest""#,
        f
    );
    assert_eq!(eval_string(&code), "6:world");
    let _ = std::fs::remove_file(&f);
}

#[test]
fn binmode_raw_roundtrip_preserves_bytes() {
    let f = tmp_path("bin_xx");
    let code = format!(
        r#"open my $fh, ">", "{0}"; binmode $fh, ":raw"; print $fh "abc"; close $fh;
           open $fh, "<", "{0}"; binmode $fh, ":raw"; my $r = do {{ local $/; <$fh> }}; close $fh;
           unlink "{0}";
           length($r) . ":" . $r"#,
        f
    );
    assert_eq!(eval_string(&code), "3:abc");
    let _ = std::fs::remove_file(&f);
}

// ── tie / FETCH / STORE not invoked today ────────────────────────────────────

#[test]
fn tie_scalar_fetch_store_not_invoked_today() {
    // BUG-040: `tie $x, "MyTie", "init"` succeeds (no error) but neither
    // FETCH nor STORE fire on subsequent reads/writes. Pin observed
    // behavior — pure-Perl-like assignment without tie hooks.
    let out = eval_string(
        r#"package MyTie;
           sub TIESCALAR { my $cls = shift; my $v = shift; bless \$v, $cls }
           sub FETCH { "fetched:" . ${$_[0]} }
           sub STORE { ${$_[0]} = $_[1] . "!" }
           package main;
           my $x;
           tie $x, "MyTie", "init";
           my $a = $x;
           $x = "new";
           my $b = $x;
           "a=$a/b=$b""#,
    );
    // With FETCH active, this would be "a=fetched:init/b=fetched:new!".
    assert_eq!(out, "a=/b=new");
}

// ── Sort stability + multi-key + Schwartzian ────────────────────────────────

#[test]
fn sort_is_stable_for_equal_keys() {
    // Items b and d share v=1 with a; stable sort keeps original a,b,d order
    // before the v=2 element c.
    assert_eq!(
        eval_string(
            r#"my @items = ({n=>"a",v=>1}, {n=>"b",v=>1}, {n=>"c",v=>2}, {n=>"d",v=>1});
               my @sorted = sort { $a->{v} <=> $b->{v} } @items;
               join(",", map { $_->{n} } @sorted)"#
        ),
        "a,b,d,c"
    );
}

#[test]
fn multi_key_sort_via_or_chain() {
    assert_eq!(
        eval_string(
            r#"my @items = ({a=>"x",b=>2}, {a=>"y",b=>1}, {a=>"x",b=>1}, {a=>"y",b=>2});
               my @s = sort { $a->{a} cmp $b->{a} || $a->{b} <=> $b->{b} } @items;
               join(";", map { "$_->{a}/$_->{b}" } @s)"#
        ),
        "x/1;x/2;y/1;y/2"
    );
}

#[test]
fn schwartzian_transform_orders_by_string_length() {
    assert_eq!(
        eval_string(
            r#"my @words = qw(apple banana cherry date);
               my @s = map { $_->[1] }
                       sort { $a->[0] <=> $b->[0] }
                       map { [length($_), $_] }
                       @words;
               "@s""#
        ),
        "date apple banana cherry"
    );
}

#[test]
fn sort_default_uppercase_before_lowercase() {
    assert_eq!(
        eval_string(r#"join(",", sort qw(banana Apple cherry apple Banana))"#),
        "Apple,Banana,apple,banana,cherry"
    );
}

// ── caller(N) shape with no arg ─────────────────────────────────────────────

#[test]
fn caller_without_arg_returns_three_field_list() {
    // Default caller() returns (package, file, line). Subname slot is the
    // BUG-005 case (covered separately); plain three-field caller is fine.
    let out = eval_string(
        r#"sub gx { my @c = caller; scalar(@c) . ":" . $c[0] . "/" . $c[2] }
           gx()"#,
    );
    assert!(out.starts_with("3:main/"), "got {:?}", out);
}

// ── Prototype `\@` not honored today ─────────────────────────────────────────

#[test]
fn backslash_at_prototype_does_not_auto_take_ref_today() {
    // BUG-041: `sub f (\@) { ... }` should auto-take `\@a` when called with
    // `f(@a)`. Stryke passes the raw array elements instead.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(
        r#"sub sortme (\@) { sort @{$_[0]} }
           my @a = (3,1,2);
           my @r = sortme @a;
           "@r""#,
    );
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected runtime error, got {:?}",
        kind
    );
}

// ── delete on slices ─────────────────────────────────────────────────────────

#[test]
fn delete_single_array_index_undefs_element() {
    assert_eq!(
        eval_string(
            r#"my @a = (10..15); delete $a[2]; join(",", map { defined $_ ? $_ : "U" } @a)"#
        ),
        "10,11,U,13,14,15"
    );
}

#[test]
fn delete_array_slice_is_rejected_today() {
    // BUG-042: `delete @a[1..3]` should undef multiple elements and return
    // the deleted values. Stryke rejects with "delete requires hash or
    // array element".
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"my @a = (10..15); delete @a[1..3]; "@a""#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected runtime error, got {:?}",
        kind
    );
}

#[test]
fn delete_hash_slice_is_rejected_today() {
    // BUG-043: `delete @h{qw(a b)}` similarly rejected.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(
        r#"my %h = (a=>1, b=>2, c=>3); delete @h{qw(a b)}; join(",", sort keys %h)"#,
    );
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type),
        "expected runtime error, got {:?}",
        kind
    );
}

#[test]
fn splice_workaround_for_array_slice_delete_works() {
    assert_eq!(
        eval_string(r#"my @a = (10..15); splice(@a, 2, 1); "@a""#),
        "10 11 13 14 15"
    );
}

// ── AOP intercept variables ──────────────────────────────────────────────────

#[test]
fn aop_intercept_name_visible_in_after() {
    // `$INTERCEPT_NAME` is set in a special scope, not `main::`. The
    // unqualified form reads it; `$main::INTERCEPT_NAME` is undef.
    assert_eq!(
        eval_string(
            r#"our $captured = "";
               fn payload { 1 }
               after "payload" { $main::captured = $INTERCEPT_NAME }
               payload();
               $captured"#
        ),
        "payload"
    );
}

#[test]
fn aop_after_dollar_question_is_zero_not_return_value_today() {
    // BUG-044: documentation says `$?` in after-advice carries the original
    // sub's return value. Today it stays 0.
    assert_eq!(
        eval_int(
            r#"our $captured = -1;
               fn payload { 42 }
               after "payload" { $main::captured = $? }
               payload();
               $captured"#
        ),
        0
    );
}

// ── Hash reverse swaps keys and values ───────────────────────────────────────

#[test]
fn hash_reverse_swaps_keys_and_values() {
    assert_eq!(
        eval_string(
            r#"my %h = (a=>1, b=>2); my %r = reverse %h; "$r{1}/$r{2}""#
        ),
        "a/b"
    );
}

// ── Trim idiom via s///r ────────────────────────────────────────────────────

#[test]
fn trim_via_substitute_r_flag() {
    assert_eq!(
        eval_string(r#""  trimme  " =~ s/^\s+|\s+$//gr"#),
        "trimme"
    );
}

// ── async/await over array of futures ───────────────────────────────────────

#[test]
fn async_array_then_await_all() {
    assert_eq!(
        eval_string(
            r#"my @futs = map { async { _ * 10 } } 1..5;
               my @r = map { await $_ } @futs;
               "@r""#
        ),
        "10 20 30 40 50"
    );
}

#[test]
fn async_returns_asynctask_ref_kind() {
    assert_eq!(
        eval_string(r#"my $f = async { 42 }; ref($f)"#),
        "ASYNCTASK"
    );
}

// ── Print returns 1; explicit `return;` yields undef ────────────────────────

#[test]
fn print_in_scalar_context_returns_one() {
    // We can't observe print's stdout here, but we can capture its return.
    let out = eval_int(r#"my $r = print ""; $r"#);
    assert_eq!(out, 1);
}

#[test]
fn explicit_bare_return_yields_undef() {
    assert_eq!(
        eval_int(r#"sub myr { return; } defined(myr()) ? 1 : 0"#),
        0
    );
}

// ── Non-greedy and alternation captures ─────────────────────────────────────

#[test]
fn non_greedy_a_plus_question_takes_one() {
    assert_eq!(
        eval_string(r#""aaaa" =~ /(a+?)/; $1"#),
        "a"
    );
}

#[test]
fn greedy_a_plus_takes_all() {
    assert_eq!(
        eval_string(r#""aaaa" =~ /(a+)/; $1"#),
        "aaaa"
    );
}

#[test]
fn alternation_capture_returns_match() {
    assert_eq!(
        eval_string(r#""cat" =~ /^(cat|dog|bird)$/; $1"#),
        "cat"
    );
    assert_eq!(
        eval_int(r#""elephant" =~ /^(cat|dog|bird)$/ ? 1 : 0"#),
        0
    );
}

// ── Lookbehind only ─────────────────────────────────────────────────────────

#[test]
fn lookbehind_two_chars_then_match() {
    assert_eq!(
        eval_int(r#""abc" =~ /(?<=ab)c/ ? 1 : 0"#),
        1
    );
}

// ── Array concatenation flattens ────────────────────────────────────────────

#[test]
fn array_concat_via_list_flattens() {
    assert_eq!(
        eval_string(r#"my @a = (1,2); my @b = (3,4); my @c = (@a, @b); "@c""#),
        "1 2 3 4"
    );
}

// ── @ENV interpolation ──────────────────────────────────────────────────────

#[test]
fn env_var_interpolates_in_double_quoted_string() {
    let h = std::env::var("HOME").unwrap_or_else(|_| String::new());
    let out = eval_string(r#""h=$ENV{HOME}""#);
    assert_eq!(out, format!("h={}", h));
}
