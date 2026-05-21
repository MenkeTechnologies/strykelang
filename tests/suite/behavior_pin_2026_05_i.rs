//! Behavior-pinning batch I (2026-05-04): random/srand, hashes (md5/sha),
//! base64, $SIG handlers, PerlIO layers, prototype, coderef forms,
//! Scalar::Util-style builtins, locale uc/lc, %- multi-match.

use crate::common::*;

// ── Random / srand reproducibility ──────────────────────────────────────────

#[test]
fn srand_with_same_seed_produces_same_value() {
    assert_eq!(
        eval_int(
            r#"srand(42); my $a = int(rand(1000));
               srand(42); my $b = int(rand(1000));
               $a == $b ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn rand_returns_in_range_zero_to_n() {
    assert_eq!(
        eval_int(r#"srand(42); my $r = int(rand(10)); ($r >= 0 && $r < 10) ? 1 : 0"#),
        1
    );
}

// ── Crypto hashes (built-in, not via Digest::SHA module) ────────────────────

#[test]
fn md5_hello_known_hex() {
    assert_eq!(
        eval_string(r#"md5("hello")"#),
        "5d41402abc4b2a76b9719d911017c592"
    );
}

#[test]
fn sha256_hello_known_hex() {
    assert_eq!(
        eval_string(r#"sha256("hello")"#),
        "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    );
}

#[test]
fn sha1_hello_known_hex() {
    assert_eq!(
        eval_string(r#"sha1("hello")"#),
        "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
    );
}

#[test]
fn digest_sha_module_load_fails_today() {
    // Stryke can't parse upstream Digest::SHA.pm. The hash builtins are
    // provided directly without `use`.
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"use Digest::SHA qw(sha256_hex); sha256_hex("hello")"#);
    assert!(
        matches!(
            kind,
            ErrorKind::Syntax | ErrorKind::Runtime | ErrorKind::Type | ErrorKind::FileNotFound
        ),
        "expected error, got {:?}",
        kind
    );
}

// ── Base64 round-trip ────────────────────────────────────────────────────────

#[test]
fn base64_encode_then_decode_roundtrip() {
    assert_eq!(
        eval_string(r#"base64_decode(base64_encode("hello world"))"#),
        "hello world"
    );
}

#[test]
fn base64_encode_known_vector() {
    assert_eq!(
        eval_string(r#"base64_encode("hello world")"#),
        "aGVsbG8gd29ybGQ="
    );
}

// ── $SIG hash ────────────────────────────────────────────────────────────────

#[test]
fn sig_int_default_is_undef() {
    assert_eq!(eval_int(r#"defined($SIG{INT}) ? 1 : 0"#), 0);
}

#[test]
fn sig_handler_assignment_returns_code_ref() {
    assert_eq!(
        eval_string(r#"$SIG{USR1} = sub { 1 }; ref $SIG{USR1}"#),
        "CODE"
    );
}

// The handler sites use `our` for the captured state since closures over
// `my` variables don't propagate mutations back today (BUG-089). Once that
// bug is fixed, these can be rewritten to use plain `my`.

#[test]
fn sig_die_handler_runs_inside_eval() {
    // BUG-050 FIXED: `$SIG{__DIE__}` is invoked when `die` runs inside an
    // `eval { }` block; the original error still propagates afterwards.
    assert_eq!(
        eval_int(
            r#"our $caught = 0;
               $SIG{__DIE__} = sub { $main::caught = 1 };
               eval { die "x" };
               $caught"#
        ),
        1
    );
}

#[test]
fn sig_die_handler_can_swap_error_by_redieing() {
    // The handler can `die` itself to substitute a different error; the
    // outer `eval` then sees that swapped message in `$@`.
    let out = eval_string(
        r#"$SIG{__DIE__} = sub { die "swap:" . $_[0] };
           eval { die "orig\n" };
           $@"#,
    );
    assert!(out.starts_with("swap:orig"), "got {:?}", out);
}

#[test]
fn sig_die_handler_recursion_guard_prevents_loop() {
    // The handler's own `die` doesn't re-enter the handler infinitely.
    let out = eval_string(
        r#"our $count = 0;
           $SIG{__DIE__} = sub { $main::count++; die "x" };
           eval { die "orig" };
           $count"#,
    );
    // Handler ran exactly once for the original die.
    assert_eq!(out, "1");
}

#[test]
fn sig_warn_handler_runs_on_warn() {
    // BUG-025 FIXED: `$SIG{__WARN__}` is invoked when `warn` runs.
    assert_eq!(
        eval_int(
            r#"our $caught = 0;
               $SIG{__WARN__} = sub { $main::caught = 1 };
               warn "test\n";
               $caught"#
        ),
        1
    );
}

#[test]
fn sig_warn_handler_receives_message_with_newline() {
    // The handler gets the formatted message (including the newline-or-
    // line-info suffix) as `$_[0]`.
    let out = eval_string(
        r#"our $captured = "";
           $SIG{__WARN__} = sub { $main::captured = $_[0] };
           warn "hi\n";
           $captured"#,
    );
    assert_eq!(out, "hi\n");
}

#[test]
fn sig_warn_handler_recursion_guard_prevents_loop() {
    // A `__WARN__` handler that calls `warn` again does not re-enter the
    // handler — the inner warn falls back to stderr.
    assert_eq!(
        eval_int(
            r#"our $depth = 0;
               $SIG{__WARN__} = sub { $main::depth++; warn "nested\n" };
               warn "outer\n";
               $depth"#
        ),
        1
    );
}

// ── PerlIO layers in open mode ──────────────────────────────────────────────

#[test]
fn open_with_utf8_layer_is_rejected_today() {
    // BUG-051: `>:utf8` and `<:raw` open modes raise "Unknown open mode".
    use stryke::error::ErrorKind;
    let kind = eval_err_kind(r#"my $f = "/tmp/stryke_pin_utf8_open"; open my $fh, ">:utf8", $f"#);
    assert!(
        matches!(kind, ErrorKind::Runtime | ErrorKind::Type | ErrorKind::IO),
        "expected error, got {:?}",
        kind
    );
}

// ── prototype() on builtin returns empty (Perl returns the proto string) ────

#[test]
fn prototype_of_push_is_empty_today() {
    // BUG-052: Perl returns "+@" for push; stryke returns empty.
    assert_eq!(eval_string(r#"prototype("push")"#), "");
}

#[test]
fn prototype_of_scalar_is_empty_today() {
    // Perl returns "$" for scalar.
    assert_eq!(eval_string(r#"prototype("scalar")"#), "");
}

#[test]
fn prototype_of_user_sub_returns_proto_string() {
    assert_eq!(eval_string(r#"sub myf ($) { $_[0] } prototype \&myf"#), "$");
    assert_eq!(
        eval_string(r#"sub myf (\@) { $_[0] } prototype \&myf"#),
        "\\@"
    );
}

// ── exists &subname is parse error today ─────────────────────────────────────

#[test]
fn exists_ampersand_subname_is_parse_error_today() {
    // BUG-053: `exists &main::myf` → "Unexpected token BitAnd". Perl accepts
    // `exists &name` for sub-existence checks.
    use stryke::error::ErrorKind;
    let kind = parse_err_kind(r#"sub myf { 1 } exists &main::myf"#);
    assert!(
        matches!(kind, ErrorKind::Syntax),
        "expected syntax error, got {:?}",
        kind
    );
}

// ── defined &name works as a workaround ─────────────────────────────────────

#[test]
fn defined_ampersand_subname_works() {
    assert_eq!(eval_int(r#"sub myff { 1 } defined &myff ? 1 : 0"#), 1);
}

// ── Coderef invocation forms (\&sub, &$ref, &$ref(), &$ref(args)) ───────────

#[test]
fn coderef_arrow_call() {
    assert_eq!(eval_int(r#"sub myff { 42 } my $r = \&myff; $r->()"#), 42);
}

#[test]
fn coderef_ampersand_call_with_parens() {
    assert_eq!(eval_int(r#"sub myff { 42 } my $r = \&myff; &$r()"#), 42);
}

#[test]
fn coderef_ampersand_call_no_parens_passes_no_args() {
    // `&$r` (no parens) calls without passing @_; here the sub returns 42
    // unconditionally so the value is the same.
    assert_eq!(eval_int(r#"sub myff { 42 } my $r = \&myff; &$r"#), 42);
}

#[test]
fn coderef_ampersand_call_passes_explicit_args() {
    assert_eq!(
        eval_int(r#"sub myff { scalar @_ } my $r = \&myff; &$r(1,2,3)"#),
        3
    );
}

// ── Scalar::Util-style builtins available without `use` ─────────────────────

#[test]
fn reftype_unblessed_arrayref_is_array() {
    assert_eq!(eval_string(r#"reftype([1,2,3])"#), "ARRAY");
}

#[test]
fn reftype_blessed_arrayref_still_array() {
    assert_eq!(
        eval_string(r#"my $r = bless [1,2], "X"; reftype($r)"#),
        "ARRAY"
    );
}

#[test]
fn reftype_hashref_is_hash() {
    assert_eq!(eval_string(r#"reftype({a=>1})"#), "HASH");
}

#[test]
fn blessed_returns_class_for_blessed_ref() {
    assert_eq!(eval_string(r#"my $b = bless {}, "C"; blessed($b)"#), "C");
}

#[test]
fn blessed_returns_undef_for_unblessed_ref() {
    assert_eq!(eval_int(r#"defined(blessed([1,2])) ? 1 : 0"#), 0);
}

#[test]
fn blessed_returns_undef_for_plain_string() {
    assert_eq!(eval_int(r#"defined(blessed("hello")) ? 1 : 0"#), 0);
}

#[test]
fn looks_like_number_recognizes_numeric_strings() {
    // `Scalar::Util::looks_like_number` parity: real numbers (incl. exponent
    // notation and signed inf) are truthy; non-numeric strings are 0.
    assert_eq!(eval_int(r#"looks_like_number("3.14")"#), 1);
    assert_eq!(eval_int(r#"looks_like_number("-1e5")"#), 1);
    assert_eq!(eval_int(r#"looks_like_number("inf")"#), 1);
    assert_eq!(eval_int(r#"looks_like_number("nope")"#), 0);
    assert_eq!(eval_int(r#"looks_like_number("")"#), 0);
}

// ── Sub stringification: `CODE(__ANON__)` placeholder ───────────────────────

#[test]
fn anon_sub_stringifies_with_placeholder() {
    let s = eval_string(r#"my $r = sub { 1 }; "$r""#);
    assert!(s.starts_with("CODE("), "got {:?}", s);
}

#[test]
fn ref_of_anon_sub_is_code() {
    assert_eq!(eval_string(r#"ref(sub { 1 })"#), "CODE");
}

// ── Case-modifier escapes in interpolation ──────────────────────────────────

#[test]
fn upper_case_escape_uppercases_until_e() {
    assert_eq!(eval_string(r#"my $w = "abc"; "\U$w\E end""#), "ABC end");
}

#[test]
fn lower_case_escape_lowercases_until_e() {
    assert_eq!(eval_string(r#"my $w = "ABC"; "\L$w\E end""#), "abc end");
}

#[test]
fn ucfirst_escape_uppercases_first_char_only() {
    assert_eq!(eval_string(r#"my $w = "abc"; "\u$w end""#), "Abc end");
}

#[test]
fn lcfirst_escape_lowercases_first_char_only() {
    assert_eq!(eval_string(r#"my $w = "ABC"; "\l$w end""#), "aBC end");
}

// ── \U/\L do NOT work in s/// replacement today ──────────────────────────────

#[test]
fn upper_case_escape_in_substitution_is_literal_today() {
    // BUG-055: `\U$1` in `s/.../...` replacement should uppercase $1.
    // Stryke leaves `\U` literal.
    assert_eq!(
        eval_string(r#"my $s = "abc def"; $s =~ s/\b(\w)/\U$1/g; $s"#),
        "\\Uabc \\Udef"
    );
}

#[test]
fn s_e_flag_with_uc_call_works() {
    // /e with uc() is the working alternative.
    assert_eq!(
        eval_string(r#"my $s = "abc def"; $s =~ s/\b(\w)/uc($1)/ge; $s"#),
        "Abc Def"
    );
}

// ── %- multi-capture named hash returns only the last match today ───────────

#[test]
fn percent_minus_multi_capture_returns_only_last_today() {
    // BUG-056: `%-` should accumulate all named captures across `/g`. Stryke
    // exposes only the last one.
    assert_eq!(
        eval_string(
            r#""abc 123 def 456" =~ /(?<wd>\w+)/g;
               join(",", @{$-{wd}})"#
        ),
        "456"
    );
}

#[test]
fn percent_plus_named_capture_works() {
    // `%+` (single-match named captures) does work correctly.
    assert_eq!(
        eval_string(
            r#""abc 123" =~ /(?<word>\w+)\s+(?<num>\d+)/;
               "$+{word}/$+{num}""#
        ),
        "abc/123"
    );
}

// ── Large-number overflow saturates instead of wrapping ─────────────────────

#[test]
fn printf_d_with_large_float_saturates_to_i64_max_today() {
    // PARITY-018: Perl's `printf "%d", 1e20` wraps and yields -1. Stryke
    // saturates because Rust's `as i64` saturates on overflow.
    assert_eq!(eval_string(r#"sprintf("%d", 1e20)"#), "9223372036854775807");
}

// ── %a hex-float format not implemented today ───────────────────────────────

#[test]
fn sprintf_a_hex_float_emits_c99_form() {
    // BUG-057 FIXED: %a emits hex-float matching C99/POSIX: sign,
    // normalized hex mantissa, then `p[+-]N` decimal exponent.
    assert_eq!(eval_string(r#"sprintf("%a", 1.5)"#), "0x1.8p+0");
    assert_eq!(eval_string(r#"sprintf("%a", 0.5)"#), "0x1p-1");
    assert_eq!(eval_string(r#"sprintf("%a", -1.5)"#), "-0x1.8p+0");
    assert_eq!(eval_string(r#"sprintf("%a", 0)"#), "0x0p+0");
}

// ── localtime vector formatting ─────────────────────────────────────────────

#[test]
fn localtime_vector_formats_to_iso_via_printf() {
    // Pin a specific epoch so the output is deterministic per-host.
    // 1700000000 = 2023-11-14 (UTC). Local time depends on TZ; check just
    // year and month-day.
    let s = eval_string(
        r#"my @t = localtime(1700000000);
           sprintf("%04d-%02d", $t[5]+1900, $t[4]+1)"#,
    );
    assert_eq!(s, "2023-11");
}

// ── uc / lc on UTF-8 strings (treated as bytes by default) ──────────────────

#[test]
fn uc_uppercases_utf8_aware_bytes() {
    // `résumé` becomes `RÉSUMÉ` even without `use utf8` because stryke's
    // implementation handles UTF-8 directly.
    assert_eq!(eval_string(r#"uc("résumé")"#), "RÉSUMÉ");
}

#[test]
fn lc_lowercases_utf8_aware_bytes() {
    assert_eq!(eval_string(r#"lc("RÉSUMÉ")"#), "résumé");
}

// ── push/pop/shift/unshift on arrayref via @$r ──────────────────────────────

#[test]
fn push_arrayref_via_at_dollar_r() {
    assert_eq!(
        eval_string(r#"my $r = [1,2,3]; push @$r, 4, 5; "@$r""#),
        "1 2 3 4 5"
    );
}

#[test]
fn pop_arrayref_returns_last_and_shrinks() {
    assert_eq!(
        eval_string(r#"my $r = [1,2,3]; my $p = pop @$r; "p=$p left=@$r""#),
        "p=3 left=1 2"
    );
}

#[test]
fn unshift_arrayref_prepends() {
    assert_eq!(
        eval_string(r#"my $r = [1,2,3]; unshift @$r, 0; "@$r""#),
        "0 1 2 3"
    );
}

// ── Splice with replacement list ─────────────────────────────────────────────

#[test]
fn splice_replaces_range_with_more_elements() {
    assert_eq!(
        eval_string(r#"my @a = (1..5); splice(@a, 1, 2, "x", "y", "z"); "@a""#),
        "1 x y z 4 5"
    );
}

// ── Array slices with negative ranges ───────────────────────────────────────

#[test]
fn array_slice_negative_range_yields_tail() {
    assert_eq!(eval_string(r#"my @a = (10..20); "@a[-3..-1]""#), "18 19 20");
}

// ── Stringy range with letters ──────────────────────────────────────────────

#[test]
fn alpha_range_three_dot_form_works_like_two_dot() {
    // In list context `..` and `...` are equivalent. The flip-flop forms
    // differ only inside conditionals.
    assert_eq!(eval_string(r#"my @a = ("a"..."f"); "@a""#), "a b c d e f");
}

// ── Sub returns CODE(__ANON__) under string concat ──────────────────────────

#[test]
fn sub_value_concatenated_into_string() {
    let s = eval_string(r#"my $r = sub { 1 }; "ref:" . $r"#);
    assert!(s.starts_with("ref:CODE("), "got {:?}", s);
}

// ── while + /g + named captures works ───────────────────────────────────────

#[test]
fn while_g_named_captures_iterates_all_matches() {
    assert_eq!(
        eval_string(
            r#"my $s = "key=value other=stuff";
               my $log = "";
               while ($s =~ /(\w+)=(\w+)/g) { $log .= "$1->$2;" }
               $log"#
        ),
        "key->value;other->stuff;"
    );
}
