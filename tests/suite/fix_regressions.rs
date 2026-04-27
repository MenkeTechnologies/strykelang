//! Integration regressions locking in recent parity fixes: UTF-8/Latin-1 text reads, regex class
//! edge cases and `$`/`/m`, `quotemeta`, `tell`/`sysseek`, `splice` on array refs, hash/array slices
//! and slice `+=` (last slot only), `@INC` order for `require`, aggregate ops on refs, `split` limit,
//! `reverse` scalar vs list, short-circuit `&&`/`||`, `eval { }`, `set_subname`/`subname`,
//! `unicode_to_native`, `do { }` list context, `chomp`, `CORE::tell`, string `eval`, `sprintf`,
//! `hex`/`oct`/`int`, `map`/`grep`/`sort` contexts, `//`/`//=`, bitwise ops, `for`/`last`/`next`/`until`,
//! `unless`/`elsif`, subs and `wantarray`, list builtins, `values`, `split`, append `open`,
//! `\\A`/`\z`, negative array indices, slice assign, postfix `if`, `tr`/`substr`, `||=`/`&&=`, `__LINE__`,
//! `$@` after `eval { die }`, filetests (`-e`/`-f`/`-d`; `-s` is nonzero-size, not byte count), `cos`,
//! `mkdir`/`unlink`, `map`/`grep` blocks, `index`/`rindex`, `exp`/`log`/`atan2`/`sin`, `bless`, `return`,
//! `head`/`tail`/`none`/`notall`, `scalar` comma list, `and`/`or`, `pack`/`unpack`, `qw`,
//! `rename`, `sum0`, `pairkeys`/`pairvalues`/`zip`, `$$`/`$^O`, `delete`/`exists` (avoid bareword keys like
//! `p`/`q`/`m` that tokenize as ops). `use constant`, `qr//`, `/g` list capture, `s///g`, `mesh`,
//! `shuffle`, `pop`/`unshift`/`$#array`, `split` on escaped metachars, `sprintf` float, pi via
//! `atan2(0,-1)`, `cos` at pi, empty `product`, `defined`/`undef`, `minstr`/`maxstr`, sorted `values`,
//! `readdir` list context draining + `rewinddir`/`telldir`, `chop`, `sprintf` `%b`/`%o`, `fc`, `uniqint`,
//! `ref`, string/`join`/`+` coercion edge cases, `abs`, `substr` replacement, `pos`, `oct`/`hex` prefixes,
//! `sprintf` `%e` / rounded `%f`, bit-shift assigns, `sort` blocks, `splice` insert lists, `**` associativity,
//! `unshift` multi-arg, `split` limits, compound assigns (`*=`, `/=`, `%=`, `||=`, `//=`, `&&=`),
//! `uc` edge cases, list assignment unpack, hash slices, `pack` space-padding, `qx`/`readpipe`, empty aggregates,
//! `${^PREMATCH}`/`$&`/`$1`, `not`, postfix `if`/`unless` guards, `$"`, extra `sprintf` widths, `substr` from end,
//! float array index / `%`, `%g`, `|=`/`&=`, unary `+`, `.=`, boolean falsity of `0`/`""`/`"0"`, `ord`/`chr`,
//! `blessed`/`reftype`, `for` default topic after loop, `sort` by `length`, `seekdir(0)`, `-z`, `push` expr,
//! postfix `unless`, regex `(?i)`, `\d`, captures, lookahead, `/s`/`/m`, `\A`, `\Q…\E`, `split //`, array `[0..N]`,
//! `int` on float/string, `**` edge cases, `sprintf` `%X`/`%c`, bitwise `~`, `pack` `v`/`n` endianness,
//! regex `(?!…)` / `+?` / `\1` / `\s`/`\w`/`\b` / `(?<=…)` / `(?:…)`, `$+`, `reduce`, `grep` boolean negation,
//! `unpack` `x` skip, `pack` `N`/`V`, fractional `**` (via `sprintf`), `sqrt`, `s///g` scalar count, array copy,
//! `->` deref, `-=` / `^=`, `sprintf` `%+f`, `ref` of `sub`, `(?<!…)` lookbehind, `${^POSTMATCH}`, `\h`/`\v`,
//! `m{…}` delimiters, `sort`/`cmp`, `prototype`, possessive `*+`, `map` on `qw`, list `x` in `push`, `sprintf` `#`,
//! empty `substr` replacement, `split /\s+/`, `\R` line break, `\K` keep, named captures `$+{…}`, `sleep`/`srand`,
//! leading-zero `oct`, `\p{Lu}` / `\p{N}`, slice assign `@a[0,1]`, `index`/`rindex` with pos,
//! `$|`, alternate `m!…!`, `study`, `0+` on zero-padded strings, list slices, `(1) x N`, `push` length,
//! `chop`/`chomp` edge cases, bare `sprintf "%%"`, `splice` return + `qw` insert, `"05"` `eq` vs `==`,
//! braced `\x{…}` escapes, short RHS list assign, `reverse sort`, `pack "a5"` NUL padding,
//! `pack "Z"`, `splice` zero-length insert, `sprintf` + `undef`, `x` repeat to empty, `for` over `..`,
//! split with regex + limit, `//` with string `"0"`, `keys %$ref`, list flattening,
//! postdecrement, `""` + postincrement, slices with leading `undef`, `\e`, `qq|…|`, `ne` on `""` vs `"0"`.
//! `m#…#` / `s#…#…#`, `quotemeta ""`, magic `++` on `"9"`, `abs` on floats, two-arg `substr`,
//! `*=` on array elems, lex `gt`, `do { }` scalar value, `unless`/`else`, `for`/`map` on ranges,
//! `last`/`next` labels, `unshift` + `qw`, `$$aref[]`, `index` with empty substring.
//! `rindex` + `""`, unary `join`, `sprintf` `%b`/`%o`, `(a)\\1`, trim `s///`, hex+oct literals,
//! `!~`, `++` on hash slots, `//=` RHS value, `$a[-1]` assign, `join` + range list, `x` negative count,
//! `fc` on ASCII, `atan2(1,-1)`.
//! `push` + range, `s///g` digit runs, `join` through `()`, default `sort`, `sprintf` `%.0e`, `local`,
//! `s///e`, `reverse` into `@a`, `our`, `@a = map`, `uc` on a number, `int(log(exp))`, `grep { !$_ }`.
//! `use constant` floats, `\\A…\\z`, CSV-ish `split`, `s/\\./_/g`, `$#array` holes, `cos`/`sin` via `atan2`,
//! negative-base `**`, bare `return`, `exists` on slot 0 of an unused array.
//! `join` + `..`, hash slice `= ()`, `sprintf` `%.4f` / `%i`, `splice` negative index, `while` postfix,
//! `\\Q…\\E`, `//=` chain, `pack C`/`unpack H2`, `!!` coercion sum.
//! hash `//=`, `index` + start, `&`/`|`, postfix `for`, `shift`/`pop`, `substr` `$[-1]`, `map`+`chr`,
//! `int("1e2")`, default `sort` on single digits, `||` chain, `lc` on a number.
//! `s/^/` / `s/$/`, empty list in comma list, sparse `scalar @a`, `--`, grouped `**`, `.` chain,
//! `reverse` range, `ord ""`, slice assign past end, `sprintf "%o"` zero, `hex ""`.
//! `oct`/`int` on `""`, `undef` `eq`, `splice` insert-at-0 / scalar multi-remove, `cmp` vs `undef`,
//! `map` squares, Perl-style `a < b < c`, `chomp` `\\r\\n`, `qq[]`, overlapping `rindex`.
//! `push` + `()`, `..` slice assign (interior indices), `sprintf` `%.0f` rounding, `undef` numeric `+`, `.=` + `x`,
//! `and`/`or` chains, `s///` vs `s///g`, `sprintf` `%b` on0, `join` with leading `""`.
//! `sprintf` `%-` / `%+` / `%03d`, hash slice `= (…)`, assign-in-expr, list `[-1]`, `m//g` scalar,
//! `split` `/\\n/` + limit, excess list assign, `pack x`, `0+` on `\\t` digits, `q{}` eq `""`, slice subscript scalar, `m//g` + `0+`.
//! List/array slice joins + copy-assign, `delete` mid + `scalar @a`, `splice` replace, `sort values`, `cmp`,
//! postincrement + concat, `chop`/`substr`, `qw` `sort`, `unshift` list, `sprintf` `%.1f` / `%u`, `$#` empty,
//! `reverse` list + assign-back, `chr(ord)`, overlapping `rindex`, `/g` list into `join`, `grep` numeric, `||=` on `""`.
//! `<=>`, `index`/`rindex`, bankers `sprintf` `%.0f`, `push` flatten, `delete`+`exists`, `grep defined`, `//` vs `0`,
//! `splice` ends, `split //` + limit, `unpack H2`, `ref bless`, `$#a` / `$a[$#a]`, `atan2`, `/=` / `%=`, slice `=` / range slice,
//! `!!`, `x` on scalar, `ord` newline, `chr` LF, `map`+`uc`, coderef `&` / `->`.
//! `join` + leading `undef`, `log`/`exp` round-trip, `splice` delete-only, `&=` / `|=` bits, `quotemeta` dots,
//! `unpack` `v` / `C*`, `sort keys`, grouped `**`, unary `+`/`-`, scalar `=` list, `push` tail, `grep` `%` on `..`,
//! `eval` string, `abs` stringy, `lt` digit ordering, repeated slice index, `sqrt` scalar, `map` `+0`, `split` `-1`,
//! anon hash `->{}`.
//! `..` join + `map` on range, `sprintf` max width / pad, `scalar grep`/`map` on `@a`, `do {}` value, `sort` in-place,
//! `reverse sort`, `s///g` + count, `map ord`, `pack`/`unpack` `a2`, `&&=` / `+=` coercion, `exists` past end, `map lc`,
//! `splice` tail, `*=` / `-=` / `int`, `.=` join flatten, `//` + `//=`, truthiness `""`/`"0"`/`"0E0"`, `bless` `[]`.
//! `scalar` `@{[…]}`, `sort` `keys`/`values`, `qw`/`join`, `$#`, `ref` `[]`/`{}`, `->[]`, `/./g`, `split` `\\s+`,
//! `sprintf` `%b`/`%o`, `splice` zero-remove insert, `all`, `map chr`, `shift`/`pop`, `atan2`/`sqrt`,
//! postfix `for` `*=`, `unshift`, string `x`, list slice, `sprintf` `%.0f`, `3 & ~1`, `map` reassignment, `@a[i,k]`,
//! `substr`, `index`, `pack`/`unpack` `H*`, `reverse` `..`.
//! `map` squares + `reverse` `qw`, slice reorder + `..` slice, `<=>` / `cmp`, `tr`/`s` cleanup, `s` backref,
//! `keys` growth, `scalar values`, `push` flatten tail, subscript expr, `int(cos+sin)`, `/g` + `join`, `grep` `eq`,
//! sparse `scalar`/`$#`, first-slot array, scalar `/` float.
//! `map` float + range `grep`, `product`/`min`, `index`/`rindex`/`substr`, `fc`, `sprintf` `%.2f`,
//! slice assign, `delete`+`keys`, `@` in scalar, `defined`, `$a[-1]` read/write, `map` `++`, `%`, `|=`/`&=`,
//! `/^…/`, `grep` `%`, `pack C`, `undef` assign, `//=`, `exists`/`delete`+`map//`, `/g` doubles, `s` capture rotate,
//! `"01"` `==`/`eq`, `splice` head, `atan2(0,-1)>3`.
//! `map` over `..`, `grep` scalar + list, `sprintf` `%d` stringy octal, `eq`, slice subscript scalar, hash slice `=`,
//! `split` `|`/`,`, `..` slice, float `+`/`*`, `int` pair, `scalar @a`/empty, `//` display, `/[aeiou]/g`, `lc`/`uc`,
//! `sprintf` `%x`/`%X`/`%o`, `@a[i,j,k]`, `sort` `$b cmp $a`, `s/./x/g`, `int(log(near-e))`, hash `//=`, `4==4.0`,
//! `join` `-`+`/./g`, `scalar reverse @a`, `-2**2`, `*= -1`.
//! List literal slice, `map` scale + `..`, `grep` scalar on `@a`, `max`/`min`, `substr` negative + len,
//! `index`, `sprintf` `%04d`/`%+d`, `*=`, `^`, hash `+=`, `tr`, `s/\\d/_`, `/g` doubles, `grep`→`@a[…]`, `exp`/`atan2`,
//! sum first+last index, `reverse` `qw`, `map` case flip + `split`, `ne` on stringy ints, `sort` by `length`.
//! `map` default `$_`, `reverse`+`map`, `unpack` `C*`, `sort` longest-first, `&&`/`||` stringy, `//`, `<=>` chain,
//! `map` `**`, numeric range + list slice, `substr`, `split` `\s+`, `grep`+`length`, `.=`+`x`, `%o`, `abs`+`int`,
//! `sqrt` near-2, `int` pair, `!!` on `"0"`/`"0E0"`, `grep`+`int`, `&`|`^`, `chr`+`ord` on `qw`, `grep` count.
//! `--`/`++` `map` on array, `+` float `map`, multi-neg slice, `chr(64+$_)`, `sort` length `cmp`, `join` list,
//! cubes/`%x`, lex `gt`/`>`, `length` `map`, `grep` `ge`, hash `map`, `pack`/`unpack` `a2`, nybble sum, `ucfirst`/`lc`/`fc`,
//! float `==` pitfall, `int`/`3`, `qw` slice, `/aba/g`, `index`, `%` cycle, `push`/`splice`, `eval` string, `oct` `0b`,
//! `hex` list, `/1./g`, `substr`+`map`, `grep` `^` class, `!!eval`, `sprintf` `%.0f` half.
//! Run via `cargo test fix_regressions` or the full integration harness.

use crate::common::*;
use std::path::PathBuf;

// ── slurp / readline: decode_utf8_or_latin1 (no U+FFFD for high octets) ──

#[test]
fn slurp_valid_utf8_round_trips() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_slurp_utf8_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("u.txt");
    std::fs::write(&path, "café\n").unwrap();
    let p = path.to_str().expect("utf-8 path");
    assert_eq!(
        eval_string(&format!(r#"my $t = slurp "{p}"; chomp $t; $t"#)),
        "café"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn slurp_invalid_utf8_octets_map_to_u_plus_00xx() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_slurp_latin1_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("b.bin");
    std::fs::write(&path, [0xffu8, b'a', b'b']).unwrap();
    let p = path.to_str().expect("utf-8 path");
    assert_eq!(
        eval_int(&format!(
            r#"my $s = slurp "{p}"; $s eq (chr(255) . "ab") ? 1 : 0"#
        )),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn slurp_mixed_utf8_and_latin1_lines_per_line_decode() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_slurp_mixed_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mix.txt");
    let mut v = b"ascii\n".to_vec();
    v.push(0xfe);
    v.push(b'\n');
    std::fs::write(&path, v).unwrap();
    let p = path.to_str().expect("utf-8 path");
    assert_eq!(
        eval_string(&format!(
            r#"my $s = slurp "{p}"; my @L = split /\n/, $s; $L[1]"#
        )),
        "\u{00fe}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn readline_decodes_high_octet_line_as_latin1() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_readline_l1_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rl.txt");
    let mut v = b"ok\n".to_vec();
    v.push(0xfd);
    v.push(b'\n');
    std::fs::write(&path, v).unwrap();
    let p = path.to_str().expect("utf-8 path");
    assert_eq!(
        eval_int(&format!(
            r#"
            open R, "<", "{p}";
            my $a = <R>;
            my $b = <R>;
            close R;
            chomp $a;
            chomp $b;
            ($a eq "ok" && $b eq chr(253)) ? 1 : 0
        "#
        )),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── regex: leading `]` in char class + `$` rewrite must not corrupt class ──

#[test]
fn regex_char_class_leading_close_bracket_matches_bracket() {
    assert_eq!(eval_int(r#"my $s = "x]"; ($s =~ /[]]/) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "x"; ($s =~ /[]]/) ? 1 : 0"#), 0);
    assert_eq!(eval_int(r#"my $s = "]"; ($s =~ /^[]]$/) ? 1 : 0"#), 1);
}

#[test]
fn regex_negated_class_leading_close_bracket_excludes_bracket() {
    assert_eq!(eval_int(r#"my $s = "a"; ($s =~ /^[^]]$/) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "]"; ($s =~ /^[^]]$/) ? 1 : 0"#), 0);
    assert_eq!(eval_string(r#"my $s = "ba]"; $s =~ /[^]]/; $&"#), "b");
}

#[test]
fn regex_dollar_end_matches_before_optional_trailing_newline() {
    assert_eq!(eval_int(r#"my $s = "foo\n"; ($s =~ /foo$/) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "foo"; ($s =~ /foo$/) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "foox\n"; ($s =~ /foo$/) ? 1 : 0"#), 0);
}

#[test]
fn regex_char_class_with_leading_bracket_literals_includes_dollar() {
    // Use `m{…}` so `*/` inside the class does not terminate a `/…/` pattern.
    assert_eq!(eval_int(r#"my $s = "$"; ($s =~ m{[]\[^$.*/]}) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "]"; ($s =~ m{[]\[^$.*/]}) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "z"; ($s =~ m{[]\[^$.*/]}) ? 1 : 0"#), 0);
}

#[test]
fn regex_qe_escape_span_quotemeta_includes_slashes() {
    assert_eq!(
        eval_int(r#"my $s = "/a/b"; ($s =~ m{\Q/a/b\E}) ? 1 : 0"#),
        1
    );
    assert_eq!(eval_int(r#"my $s = "ab"; ($s =~ m{\Q/a/b\E}) ? 1 : 0"#), 0);
}

// ── quotemeta: slashes and regex metacharacters ──

#[test]
fn quotemeta_escapes_regex_metacharacters() {
    assert_eq!(
        eval_string(r#"quotemeta('*$+?{}[]()|')"#),
        r"\*\$\+\?\{\}\[\]\(\)\|"
    );
    assert_eq!(eval_string(r#"quotemeta('a b')"#), r"a\ b");
    assert_eq!(eval_string(r#"quotemeta('\\')"#), r"\\");
}

// ── tell ──

#[test]
fn tell_read_handle_matches_bytes_after_sysread() {
    // `readline` uses a `BufReader` that may prefetch; `tell` shares the underlying `File` cursor
    // with `sysread`, so byte offsets are predictable after `sysread`.
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_tell_read_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.dat");
    std::fs::write(&path, b"abcdefghij").unwrap();
    let p = path.to_str().expect("utf-8 path");
    assert_eq!(
        eval_int(&format!(
            r#"
            open T, "<", "{p}";
            my $buf;
            my $n = sysread T, $buf, 5;
            my $pos = tell T;
            close T;
            ($n == 5 && $pos == 5) ? 1 : 0;
        "#
        )),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── splice @$aref (Op::SpliceArrayDeref) ──

#[test]
fn splice_aref_insert_with_zero_length() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $v = [1, 2, 3];
            splice @$v, 1, 0, 9;
            join "-", @$v"#
        ),
        "1-9-2-3"
    );
}

#[test]
fn splice_aref_replace_all_elems() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $v = [1, 2];
            splice @$v, 0, 2, 7, 8, 9;
            join "", @$v"#
        ),
        "789"
    );
}

#[test]
fn splice_aref_void_removes_without_returning_list() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $v = [10, 20, 30];
            splice @$v, 1, 1;
            join "-", @$v"#
        ),
        "10-30"
    );
}

// ── Sub::Util (native stub: return coderef for CPAN bootstrap) ──

#[test]
fn sub_util_set_subname_returns_coderef_still_invokable() {
    assert_eq!(
        eval_int(
            r#"
            my $f = fn { 41 };
            my $g = set_subname("main::x", $f);
            $g->() + 1;
        "#
        ),
        42
    );
}

#[test]
fn sub_util_subname_alias_returns_second_arg() {
    assert_eq!(
        eval_int(
            r#"
            my $f = fn { 3 };
            my $h = subname("pkg::y", $f);
            $h->() + 4;
        "#
        ),
        7
    );
}

/// Native stub for core XS (`JSON::PP` BEGIN) — must exist before `utf8_heavy` loads.
#[test]
fn utf8_unicode_to_native_stub_returns_codepoint_as_integer() {
    assert_eq!(eval_int(r#"unicode_to_native(0x20AC)"#), 0x20AC);
    assert_eq!(eval_int(r#"unicode_to_native()"#), 0);
}

#[test]
fn scalar_splice_aref_returns_last_removed_element() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $a = [3, 4, 5];
            scalar splice @$a, 0, 2"#
        ),
        4
    );
}

#[test]
fn splice_aref_no_length_removes_tail_and_returns_removed_list_stringified() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $v = [1, 2, 3, 4, 5];
            my @r = splice @$v, 3;
            join "-", @r"#
        ),
        "4-5"
    );
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $v = [1, 2, 3, 4, 5];
            splice @$v, 3;
            join "-", @$v"#
        ),
        "1-2-3"
    );
}

// ── @INC: first directory wins (vendor / shadowed .pm layout) ──

#[test]
fn require_scans_inc_in_array_order_first_dir_shadows_later() {
    let base: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_inc_order_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let d1 = base.join("first");
    let d2 = base.join("second");
    std::fs::create_dir_all(&d1).unwrap();
    std::fs::create_dir_all(&d2).unwrap();
    std::fs::write(
        d1.join("Shadow.pm"),
        "package Shadow;\nfn marker { 101 }\n1;\n",
    )
    .unwrap();
    std::fs::write(
        d2.join("Shadow.pm"),
        "package Shadow;\nfn marker { 202 }\n1;\n",
    )
    .unwrap();
    let p1 = d1.to_str().expect("utf-8");
    let p2 = d2.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(
            r#"
            BEGIN {{
                unshift @INC, "{p2}";
                unshift @INC, "{p1}";
            }}
            require Shadow;
            Shadow::marker();
        "#
        )),
        101
    );
    assert_eq!(
        eval_int(&format!(
            r#"
            BEGIN {{
                unshift @INC, "{p1}";
                unshift @INC, "{p2}";
            }}
            require Shadow;
            Shadow::marker();
        "#
        )),
        202
    );
    std::fs::remove_dir_all(&base).ok();
}

// ── Hash / array slices through refs (VM deref ops) ──

/// Avoid `q =>` / `qq` tokenization — keys must not start quote-like ops after fat comma.
#[test]
fn hash_slice_through_hashref_list_context_has_two_elements() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %h = (x => 3, y => 4);
            my $r = \%h;
            scalar @$r{'x', 'y'}"#
        ),
        2
    );
}

#[test]
fn hash_slice_through_hashref_assigns_pairs() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my %h = (a => 1, b => 2);
            my $r = \%h;
            @$r{'a', 'b'} = (9, 8);
            $h{a} . "-" . $h{b}"#
        ),
        "9-8"
    );
}

#[test]
fn array_slice_through_arrayref_reads_indices() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $v = [10, 20, 30, 40];
            join "-", @$v[1, 3]"#
        ),
        "20-40"
    );
}

// ── Regex: multiline, substitution with leading-] class ──

#[test]
fn regex_dollar_with_m_flag_matches_internal_line_end() {
    assert_eq!(
        eval_int(r#"my $s = "foo\nbar\n"; ($s =~ /^bar/m) ? 1 : 0"#),
        1
    );
    assert_eq!(
        eval_int(r#"my $s = "foo\nbar\n"; ($s =~ /foo$/m) ? 1 : 0"#),
        1
    );
}

#[test]
fn regex_substitution_char_class_leading_close_bracket() {
    assert_eq!(
        eval_string(r#"my $t = "a]b]c"; $t =~ s/[]]/_/g; $t"#),
        "a_b_c"
    );
}

#[test]
fn regex_match_uses_literal_dollar_inside_char_class() {
    assert_eq!(eval_int(r#"my $s = 'x$y'; ($s =~ /[$]/) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = 'xz'; ($s =~ /[$]/) ? 1 : 0"#), 0);
}

// ── Slurp / readline: CRLF and trailing CR stripping ──

#[test]
fn slurp_crlf_file_contains_both_bytes_without_replacement_char() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_slurp_crlf_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("crlf.txt");
    std::fs::write(&path, b"a\r\nb\r\n").unwrap();
    let p = path.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(
            r#"my $s = slurp "{p}";
            (index($s, "a") >= 0 && index($s, "b") >= 0 && index($s, "\r") >= 0 && index($s, chr(0xFFFD)) < 0) ? 1 : 0"#
        )),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── do { } list context (assign into aggregate) ──

#[test]
fn do_block_list_context_assigns_to_my_array() {
    assert_eq!(
        eval_string(
            r#"my @x = do { (1, 2, 3) };
            join "", @x"#
        ),
        "123"
    );
}

#[test]
fn do_block_list_context_assigns_to_my_hash() {
    assert_eq!(
        eval_int(
            r#"my %h = do { ("u", 5, "v", 6) };
 $h{u} + $h{v}"#
        ),
        11
    );
}

// ── chomp: ORS / line endings ──

#[test]
fn chomp_removes_single_trailing_newline() {
    assert_eq!(eval_string(r#"my $s = "ab\n"; chomp $s; $s"#), "ab");
}

// ── sysseek + tell (integration, matches crate VM tests) ──

#[test]
fn sysseek_seek_set_then_tell_reports_offset() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_sysseek_tell_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("seek.dat");
    std::fs::write(&path, b"WXYZ").unwrap();
    let p = path.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(
            r#"
            open S, "<", "{p}";
            sysseek S, 2, 0;
            my $pos = tell S;
            close S;
            $pos;
        "#
        )),
        2
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── CORE::tell qualified ──

#[test]
fn core_tell_stdout_is_negative_one() {
    assert_eq!(eval_int(r#"CORE::tell STDOUT"#), -1);
}

// ── Splice @$aref: more edge shapes ──

#[test]
fn splice_aref_omit_length_removes_rest_default() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $v = [1, 2, 3, 4];
            my @t = splice @$v, 1;
            join ":", @t, "", join "-", @$v"#
        ),
        "2:3:4::1"
    );
}

#[test]
fn splice_aref_zero_length_noop_keeps_order() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $v = [8, 9];
            my @r = splice @$v, 1, 0;
            (scalar @r) . "|" . join "-", @$v"#
        ),
        "0|8-9"
    );
}

// ── quotemeta: control and wide chars ──

#[test]
fn quotemeta_escapes_newline_and_tab() {
    assert_eq!(eval_string(r#"quotemeta("\n\t")"#), "\\\n\\\t");
}

#[test]
fn quotemeta_preserves_ascii_alnum_underscore() {
    assert_eq!(eval_string(r#"quotemeta("A_z09")"#), "A_z09");
}

// ── Hash / array slice compound assign (last slot only, Perl 5) ──

#[test]
fn hash_slice_deref_compound_plus_eq_updates_last_key_only() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my %h = (a => 2, b => 3);
            my $r = \%h;
            @$r{qw(a b)} += 10;
            $h{a} . ":" . $h{b}"#
        ),
        "2:13"
    );
}

#[test]
fn array_slice_deref_compound_plus_eq_updates_last_index_only() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $r = [1, 2, 3];
            @$r[0, 2] += 5;
            join " ", @$r"#
        ),
        "1 2 8"
    );
}

// ── Aggregate ops through array ref ──

#[test]
fn push_unshift_pop_shift_on_array_ref_mutates_one_array() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $r = [10];
            push @$r, 20, 30;
            unshift @$r, 5;
            my $t = pop @$r;
            my $h = shift @$r;
            join "-", $h, @$r, $t"#
        ),
        "5-10-20-30"
    );
}

#[test]
fn delete_and_exists_on_hash_through_ref() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $r = { u => 1, v => 2 };
            delete $r->{u};
            (exists $r->{v} && !exists $r->{u} && $r->{v} == 2) ? 1 : 0"#
        ),
        1
    );
}

// ── Text builtins ──

#[test]
fn rev_scalar_revs_string_bytes_not_list() {
    assert_eq!(eval_string(r#"scalar rev "Perl""#), "lreP");
}

#[test]
fn reverse_list_reverses_element_order() {
    assert_eq!(eval_string(r#"join "", rev (1, 2, 3)"#), "321");
}

#[test]
fn split_with_limit_keeps_remainder_in_last_field() {
    assert_eq!(
        eval_string(r#"join "|", split /:/, "a:b:c:d", 3"#),
        "a|b|c:d"
    );
}

#[test]
fn index_rindex_find_substrings() {
    assert_eq!(
        eval_int(r#"index("foobar", "bar") == 3 && rindex("foobar", "o") == 2 ? 1 : 0"#),
        1
    );
}

#[test]
fn lc_uc_ascii_round_trip() {
    assert_eq!(eval_string(r#"lc("AbC") . uc("xYz")"#), "abcXYZ");
}

// ── Short-circuit ops ──

#[test]
fn logical_and_skips_rhs_when_false() {
    assert_eq!(
        eval_int(
            r#"my $ran = 0;
            (0 && ($ran = 1));
            $ran"#
        ),
        0
    );
}

#[test]
fn logical_or_skips_rhs_when_true() {
    assert_eq!(
        eval_int(
            r#"my $ran = 0;
            (1 || ($ran = 1));
            $ran"#
        ),
        0
    );
}

// ── Context: scalar vs list ──

#[test]
fn scalar_array_deref_is_length() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $r = [7, 8, 9];
            scalar @$r"#
        ),
        3
    );
}

#[test]
fn scalar_keys_on_named_hash_counts_entries() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %h = (x => 1, y => 2, z => 3);
            scalar keys %h"#
        ),
        3
    );
}

// ── Range in list context ──

#[test]
fn dot_dot_in_list_context_expands_numeric_range() {
    assert_eq!(eval_string(r#"join "", (3 .. 6)"#), "3456");
}

// ── `eval` block: value and isolation ──

#[test]
fn eval_block_returns_last_expression() {
    assert_eq!(eval_int(r#"eval { 10 + 20 }"#), 30);
}

#[test]
fn eval_block_sees_outer_lexical() {
    assert_eq!(
        eval_int(
            r#"my $x = 5;
            eval { $x + 1 }"#
        ),
        6
    );
}

// ── Substitution: word boundary + capture ──

#[test]
fn regex_substitution_backreference_replacement() {
    assert_eq!(eval_string(r#"my $s = "axxb"; $s =~ s/x+/_/g; $s"#), "a_b");
}

#[test]
fn regex_match_captures_in_numbered_groups() {
    assert_eq!(
        eval_string(r#"my $s = "alpha-beta"; $s =~ /^(\w+)-(\w+)$/; $2 . "/" . $1"#),
        "beta/alpha"
    );
}

// ── Empty / edge aggregates ──

#[test]
fn splice_empty_aref_insert_still_inserts() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $r = [];
            splice @$r, 0, 0, 9, 8;
            join "-", @$r"#
        ),
        "9-8"
    );
}

#[test]
fn join_empty_list_yields_empty_string() {
    assert_eq!(eval_string(r#"join "-", ()"#), "");
}

// ── String eval, sprintf, numeric conversions ──

#[test]
fn string_eval_runs_expression_text() {
    assert_eq!(eval_int(r#"eval "21 * 2""#), 42);
}

#[test]
fn sprintf_zero_pads_integers() {
    assert_eq!(eval_string(r#"sprintf("%05d", 7)"#), "00007");
}

#[test]
fn sprintf_multiple_placeholders() {
    assert_eq!(eval_string(r#"sprintf("%s=%d", "n", 99)"#), "n=99");
}

#[test]
fn hex_and_oct_literals_from_strings() {
    assert_eq!(eval_int(r#"hex("ff") + oct("10")"#), 255 + 8);
}

#[test]
fn int_truncates_toward_zero() {
    assert_eq!(eval_int(r#"int(3.9) + int(-3.9)"#), 0);
}

#[test]
fn length_counts_code_units_ascii() {
    assert_eq!(eval_int(r#"length("abcd")"#), 4);
}

// ── List ops: map / grep / sort contexts ──

#[test]
fn sort_strings_default_lexicographic() {
    assert_eq!(
        eval_string(r#"join ",", sort ("cc", "a", "bb")"#),
        "a,bb,cc"
    );
}

#[test]
fn sort_numeric_explicit_spaceship() {
    assert_eq!(
        eval_string(r#"join "-", sort { $a <=> $b } (30, 5, 10)"#),
        "5-10-30"
    );
}

#[test]
fn map_scalar_context_yields_element_count() {
    assert_eq!(eval_int(r#"scalar map { $_ + 1 } (10, 20, 30)"#), 3);
}

#[test]
fn grep_scalar_context_yields_match_count() {
    assert_eq!(eval_int(r#"scalar grep { $_ > 2 } (1, 2, 3, 4, 5)"#), 3);
}

#[test]
fn map_grep_chain_filters_then_transforms() {
    assert_eq!(
        eval_string(r#"join "", map { uc $_ } grep { length($_) == 1 } ("ab", "x", "yz")"#),
        "X"
    );
}

// ── Defined-or and ternary ──

#[test]
fn defined_or_returns_rhs_for_undef() {
    assert_eq!(
        eval_int(
            r#"my $u;
            (defined($u) ? 0 : 1) && (($u // 40) == 40) ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn defined_or_assign_sets_once() {
    assert_eq!(
        eval_int(
            r#"my $v;
            $v //= 8;
            my $n = $v;
            $v //= 99;
            ($n == 8 && $v == 8) ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn ternary_right_associative_chain() {
    assert_eq!(eval_int(r#"1 ? (0 ? 2 : 3) : 4"#), 3);
}

// ── Increments on aggregate elements ──

#[test]
fn postincrement_array_element() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @t = (100);
            my $x = $t[0]++;
            $x * 10 + $t[0]"#
        ),
        1101
    );
}

#[test]
fn preincrement_hash_element() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %g = (k => 4);
            ++$g{k};
            $g{k}"#
        ),
        5
    );
}

// ── Bit shifts and bitwise (integer) ──

#[test]
fn exponentiation_integer_pow() {
    assert_eq!(eval_int(r#"2 ** 6"#), 64);
}

#[test]
fn shift_left_binary_after_term() {
    assert_eq!(eval_int(r#"1 << 4"#), 16);
    assert_eq!(eval_int(r#"3 << 2"#), 12);
}

#[test]
fn destroy_runs_when_lexical_blessed_ref_dropped() {
    assert_eq!(
        eval_int(
            r#"our $d = 0;
            fn Dtor::DESTROY { $main::d = $main::d + 1; }
            { my $o = bless {}, "Dtor"; }
            $d"#
        ),
        1
    );
}

#[test]
fn bitwise_or_and_xor_combine_bits() {
    assert_eq!(eval_int(r#"(5 | 2) + (5 ^ 2)"#), 7 + 7);
}

// ── String repeat ──

#[test]
fn string_repeat_x_duplicates_pattern() {
    assert_eq!(eval_string(r#"'.' . ('ab' x 3)"#), ".ababab");
}

// ── String comparison ops ──

#[test]
fn string_compare_eq_ne_lt() {
    assert_eq!(
        eval_int(r#"(("a" eq "a") && ("a" ne "b") && ("apple" lt "banana")) ? 1 : 0"#),
        1
    );
}

#[test]
fn string_cmp_three_way() {
    assert_eq!(eval_int(r#"("b" cmp "a") + ("a" cmp "b")"#), 0);
}

// ── `for` loop with topic and accumulator ──

#[test]
fn for_my_iterates_lexical_list() {
    assert_eq!(
        eval_int(
            r#"my $s = 0;
            for my $n (1, 2, 3, 4) { $s += $n; }
            $s"#
        ),
        10
    );
}

// ── `last` exits nearest loop ──

#[test]
fn last_exits_loop_early() {
    assert_eq!(
        eval_int(
            r#"my $i = 0;
            while ($i < 10) {
                $i++;
                if ($i == 4) { last; }
            }
            $i"#
        ),
        4
    );
}

// ── Loop / conditional forms ──

#[test]
fn next_skips_remaining_iteration() {
    assert_eq!(
        eval_int(
            r#"my $s = 0;
            for my $k (1, 2, 3, 4, 5) {
                next if $k == 2;
                $s += $k;
            }
            $s"#
        ),
        13
    );
}

#[test]
fn until_runs_until_condition_true() {
    assert_eq!(
        eval_int(
            r#"my $n = 0;
            until ($n >= 4) {
                $n++;
            }
            $n"#
        ),
        4
    );
}

#[test]
fn unless_runs_block_when_condition_false() {
    assert_eq!(eval_int(r#"unless (0) { 77 }"#), 77);
}

#[test]
fn elsif_chain_picks_first_true_branch() {
    assert_eq!(
        eval_int(r#"if (0) { 1 } elsif (0) { 2 } elsif (1) { 3 } else { 4 }"#),
        3
    );
}

// ── Subroutines ──

#[test]
fn sub_params_via_at_underscore() {
    assert_eq!(
        eval_int(
            r#"fn add2 {
                my ($a, $b) = @_;
                $a + $b;
            }
            add2(30, 12)"#
        ),
        42
    );
}

#[test]
fn wantarray_distinct_scalar_vs_list_return_single_value() {
    assert_eq!(
        eval_int(
            r#"fn ctx {
                wantarray ? 42 : 0;
            }
            my @v = ctx();
            my $s = ctx();
            $v[0] + $s"#
        ),
        42
    );
}

#[test]
fn wantarray_false_in_scalar_context() {
    assert_eq!(
        eval_string(
            r#"fn ctx {
                wantarray ? ("aa", "bb") : "scalar";
            }
            ctx()"#
        ),
        "scalar"
    );
}

// ── Native list / scalar builtins (CPAN bootstrap paths) ──

#[test]
fn bare_builtin_sum_adds_numbers() {
    assert_eq!(eval_int(r#"sum(10, 20, 30)"#), 60);
}

#[test]
fn bare_builtin_uniq_preserves_first_occurrence_order() {
    assert_eq!(eval_string(r#"join "-", uniq(1, 1, 2, 2, 3)"#), "1-2-3");
}

#[test]
fn scalar_util_reftype_arrayref() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            reftype([])"#
        ),
        "ARRAY"
    );
}

#[test]
fn scalar_util_reftype_hashref() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            reftype({})"#
        ),
        "HASH"
    );
}

// ── Hash / list builtins ──

#[test]
fn values_in_scalar_context_counts_hash_entries() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %t = (a => 1, b => 2, c => 3);
            scalar values %t"#
        ),
        3
    );
}

#[test]
fn grep_regex_argument_uses_topic() {
    assert_eq!(eval_string(r#"join "", grep /2/, (12, 22, 32)"#), "122232");
}

#[test]
fn split_with_no_pattern_splits_whitespace_on_topic() {
    assert_eq!(
        eval_string(
            r#"$_ = "x y z";
            join "-", split"#
        ),
        "x-y-z"
    );
}

// ── File open append ──

#[test]
fn open_append_mode_preserves_then_extends_file() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_append_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log.txt");
    std::fs::write(&path, b"first\n").unwrap();
    let p = path.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(
            r#"
            open AP, ">>", "{p}";
            print AP "second\n";
            close AP;
            my $body = slurp "{p}";
            ($body eq "first\nsecond\n") ? 1 : 0;
        "#
        )),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── Regex: `\\A` / `\\z` string anchors ──

#[test]
fn regex_az_anchor_requires_start_of_string() {
    assert_eq!(eval_int(r#"my $s = "xab"; ($s =~ /\Aab/) ? 1 : 0"#), 0);
    assert_eq!(eval_int(r#"my $s = "ab"; ($s =~ /\Aab/) ? 1 : 0"#), 1);
}

#[test]
fn regex_z_anchor_requires_end_of_string() {
    assert_eq!(eval_int(r#"my $s = "ab\n"; ($s =~ /b\z/) ? 1 : 0"#), 0);
    assert_eq!(eval_int(r#"my $s = "ab"; ($s =~ /b\z/) ? 1 : 0"#), 1);
}

// ── Arrays: negative indices, slice assign ──

#[test]
fn array_negative_index_counts_from_end() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @d = (10, 20, 30);
            $d[-1] + $d[-2]"#
        ),
        50
    );
}

#[test]
fn array_slice_assign_sets_multiple_indices() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @b = (0, 1, 2);
            @b[0, 2] = (9, 8);
            join "-", @b"#
        ),
        "9-1-8"
    );
}

// ── Statement modifiers ──

#[test]
fn postfix_if_runs_statement_when_condition_true() {
    assert_eq!(
        eval_int(
            r#"my $n = 0;
            $n = 15 if 1;
            $n"#
        ),
        15
    );
}

#[test]
fn postfix_if_skips_when_condition_false() {
    assert_eq!(
        eval_int(
            r#"my $n = 3;
            $n = 99 if 0;
            $n"#
        ),
        3
    );
}

// ── `substr` replacement form, `tr///` ──

#[test]
fn substr_four_arg_splices_replacement_into_string() {
    assert_eq!(
        eval_string(
            r#"my $s = "abcde";
            substr($s, 1, 2, "XX");
            $s"#
        ),
        "aXXde"
    );
}

#[test]
fn transliterate_tr_maps_character_set() {
    assert_eq!(
        eval_string(
            r#"my $s = "abc";
            $s =~ tr/abc/ABC/;
            $s"#
        ),
        "ABC"
    );
}

// ── Hash duplicate keys: last initializer wins ──

#[test]
fn hash_literal_duplicate_key_keeps_last_value() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %g = (x => 1, x => 2);
            $g{x}"#
        ),
        2
    );
}

// ── Logical assignment ops ──

#[test]
fn logical_or_assign_sets_when_falsy() {
    assert_eq!(
        eval_int(
            r#"my $u = 0;
            $u ||= 40;
            $u"#
        ),
        40
    );
}

#[test]
fn logical_and_assign_short_circuits_on_falsy() {
    assert_eq!(
        eval_int(
            r#"my $v = 5;
            $v &&= 0;
            $v"#
        ),
        0
    );
}

// ── Magic / introspection ──

#[test]
fn line_and_package_special_tokens() {
    assert_eq!(eval_string(r#"join ":", __LINE__, __PACKAGE__"#), "1:main");
}

// ── `eval { die }` sets `$@` ──

#[test]
fn eval_block_die_populates_at_exception() {
    assert_eq!(
        eval_int(
            r#"eval { die "boom\n" };
            $@ eq "boom\n" ? 1 : 0"#
        ),
        1
    );
}

// ── More list-builtin native paths ──

#[test]
fn bare_builtin_max_min_product_combine() {
    assert_eq!(
        eval_int(r#"max(3, 9, 4) - min(3, 9, 4) + product(2, 3)"#),
        12
    );
    assert_eq!(eval_int(r#"product(1, 2, 3, 4)"#), 24);
}

#[test]
fn bare_builtin_any_all_with_coderef() {
    assert_eq!(eval_int(r#"any(fn { $_ > 2 }, 1, 2, 3)"#), 1);
    assert_eq!(eval_int(r#"all(fn { $_ > 0 }, 1, 2, 3)"#), 1);
}

// ── Math: `cos` at zero ──

#[test]
fn cos_zero_is_one() {
    assert_eq!(eval_int(r#"int(cos(0))"#), 1);
}

// ── File test `-e` ──

#[test]
fn filetest_e_true_for_existing_path() {
    let dir: PathBuf = std::env::temp_dir().join(format!("stryke_fix_fte_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("exists.dat");
    std::fs::write(&path, b"x").unwrap();
    let p = path.to_str().expect("utf-8");
    assert_eq!(eval_int(&format!(r#"(-e "{p}") ? 1 : 0"#)), 1);
    assert_eq!(
        eval_int(r#"(-e "/nonexistent_stryke_fte_987654321") ? 1 : 0"#),
        0
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── File tests: `-f`, `-d`, `-s`; `mkdir`, `unlink` ──

#[test]
fn filetest_f_and_s_for_regular_file() {
    let dir: PathBuf = std::env::temp_dir().join(format!("stryke_fix_fs_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("n.dat");
    std::fs::write(&path, b"abcd").unwrap();
    let p = path.to_str().expect("utf-8");
    // `-s` is truthy when size > 0 (not the byte count like Perl 5).
    assert_eq!(
        eval_int(&format!(
            r#"((-f "{p}") && (-s "{p}") && length(slurp "{p}") == 4) ? 1 : 0"#
        )),
        1
    );
    assert_eq!(eval_int(&format!(r#"(-d "{p}") ? 1 : 0"#)), 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn mkdir_creates_directory_and_negative_d_test() {
    let base: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_mkdir_d_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let pb = base.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(r#"mkdir("{pb}", 0755); (-d "{pb}") ? 1 : 0"#)),
        1
    );
    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn unlink_removes_file_and_negative_e_test() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_unlink_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("gone.txt");
    std::fs::write(&path, b"z").unwrap();
    let p = path.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(r#"(unlink("{p}") == 1 && !(-e "{p}")) ? 1 : 0"#)),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

// ── `map` / `grep` blocks, `keys` order ──

#[test]
fn map_block_multiplies_and_join_commas() {
    assert_eq!(
        eval_string(r#"join ",", map { $_ * 2 } (1, 2, 3)"#),
        "2,4,6"
    );
}

#[test]
fn grep_block_filters_even_numbers() {
    assert_eq!(
        eval_string(r#"join "-", grep { $_ % 2 == 0 } (1, 2, 3, 4, 5)"#),
        "2-4"
    );
}

#[test]
fn keys_sorted_lexically_joined() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my %h = (z => 1, a => 2, u => 3);
            join "", sort keys %h"#
        ),
        "auz"
    );
}

// ── `index` / `rindex` with offset ──

#[test]
fn index_with_start_position_skips_earlier_match() {
    assert_eq!(eval_int(r#"index("abcabc", "b", 2)"#), 4);
}

#[test]
fn rindex_finds_rightmost_occurrence() {
    assert_eq!(eval_int(r#"rindex("abcabc", "b")"#), 4);
}

// ── Transcendentals: `exp` / `log` / `atan2` / `sin` ──

#[test]
fn log_exp_round_trip_on_e() {
    assert_eq!(eval_int(r#"int(log(exp(1)))"#), 1);
}

#[test]
fn atan2_first_quadrant_pi_over_four() {
    assert_eq!(eval_int(r#"int(atan2(1, 1) * 1000)"#), 785);
}

#[test]
fn sin_zero_is_zero() {
    assert_eq!(eval_int(r#"int(sin(0) * 1000)"#), 0);
}

// ── `bless`, early `return`, `head` / `tail` ──

#[test]
fn bless_sets_ref_type_name() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            ref(bless({}, "Zeta"))"#
        ),
        "Zeta"
    );
}

#[test]
fn return_short_circuits_sub_rest() {
    assert_eq!(
        eval_int(
            r#"fn early {
                return 7 if 1;
                99;
            }
            early()"#
        ),
        7
    );
}

#[test]
fn bare_builtin_head_and_tail_take_slice_ends() {
    assert_eq!(eval_string(r#"join "-", head(10, 20, 30, 40, 2)"#), "10-20");
    assert_eq!(eval_string(r#"join "-", tail(10, 20, 30, 40, 2)"#), "30-40");
    assert_eq!(eval_string(r#"scalar head(qw(a b c d), 2)"#), "b");
    assert_eq!(eval_string(r#"scalar tail(qw(a b c d), 2)"#), "d");
}

#[test]
fn bare_builtin_none_and_notall_predicates() {
    assert_eq!(eval_int(r#"none(fn { $_ < 0 }, 1, 2, 3)"#), 1);
    assert_eq!(eval_int(r#"notall(fn { $_ > 0 }, 1, -1, 2)"#), 1);
}

#[test]
fn lc_and_abs_concatenated() {
    assert_eq!(eval_string(r#"lc("LMN") . abs(-9)"#), "lmn9");
}

// ── Scalar context, low-precedence `and` / `or` ──

#[test]
fn scalar_comma_list_yields_last_value() {
    assert_eq!(eval_int(r#"scalar(10, 20, 30)"#), 30);
}

#[test]
fn low_precedence_and_returns_first_falsy() {
    assert_eq!(eval_int(r#"(0 and 2)"#), 0);
}

#[test]
fn low_precedence_or_returns_first_truthy() {
    assert_eq!(eval_int(r#"(0 or 3)"#), 3);
}

// ── `sprintf` / `pack` / `unpack` ──

#[test]
fn sprintf_percent_x_formats_hex() {
    assert_eq!(eval_string(r#"sprintf("%x", 255)"#), "ff");
}

#[test]
fn pack_unpack_round_trip_unsigned_bytes() {
    assert_eq!(eval_string(r#"pack("C3", 65, 66, 67)"#), "ABC");
    assert_eq!(eval_int(r#"0+unpack("C", "Z")"#), 90);
}

// ── `qw`, `reverse`, named-array `splice` ──

#[test]
fn qw_list_joins_with_custom_separator() {
    assert_eq!(
        eval_string(r#"join "-", qw(one two three)"#),
        "one-two-three"
    );
}

#[test]
fn reverse_list_of_words_concatenates_cba() {
    assert_eq!(eval_string(r#"join "", rev qw(a b c)"#), "cba");
}

#[test]
fn splice_named_array_removes_middle_element() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @n = (1, 2, 3);
            splice @n, 1, 1;
            join "-", @n"#
        ),
        "1-3"
    );
}

// ── `exists` on arrays; `delete` on hash keys ──

#[test]
fn exists_array_element_reports_autovivification_rules() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @e = (9);
            (exists $e[0] && !exists $e[99]) ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn delete_hash_key_removes_slot() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %k = (u => 1, v => 2);
            delete $k{u};
            (!exists $k{u} && $k{v} == 2) ? 1 : 0"#
        ),
        1
    );
}

// ── Process / OS specials (no hard-coded OS string) ──

#[test]
fn dollar_dollar_process_id_positive() {
    assert_eq!(eval_int(r#"($$ > 0) ? 1 : 0"#), 1);
}

#[test]
fn caret_o_osname_nonempty() {
    assert_eq!(eval_int(r#"(length($^O) >= 2) ? 1 : 0"#), 1);
}

// ── `rename`, more list builtins ──

#[test]
fn rename_moves_file_contents_preserved() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_rename_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let old_p = dir.join("old.txt");
    let new_p = dir.join("new.txt");
    std::fs::write(&old_p, b"payload").unwrap();
    let po = old_p.to_str().expect("utf-8");
    let pn = new_p.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(
            r#"rename "{po}", "{pn}";
            ((!(-e "{po}")) && (-e "{pn}")) ? 1 : 0"#
        )),
        1
    );
    assert_eq!(eval_string(&format!(r#"slurp "{pn}""#)), "payload");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn bare_builtin_sum0_empty_list_is_zero() {
    assert_eq!(eval_int(r#"sum0()"#), 0);
}

#[test]
fn bare_builtin_pairkeys_pairvalues_split_pairs() {
    assert_eq!(eval_string(r#"join "-", pairkeys(1, 10, 2, 20)"#), "1-2");
    assert_eq!(
        eval_string(r#"join "-", pairvalues(1, 10, 2, 20)"#),
        "10-20"
    );
}

#[test]
fn bare_builtin_zip_pairs_first_row_snapshot() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @z = zip(1, 2, 10, 20);
            $z[0]->[0] + $z[0]->[1]"#
        ),
        3
    );
}

// ── `use constant`, `qr//`, `/g` list matches, `s///g` ──

#[test]
fn use_constant_defines_bareword() {
    assert_eq!(
        eval_int(
            r#"use constant CIX => 99;
            CIX"#
        ),
        99
    );
}

#[test]
fn regex_global_match_in_list_context() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @hits = ("aba" =~ /a/g);
            scalar @hits"#
        ),
        2
    );
}

#[test]
fn qr_compiled_pattern_reused_with_match() {
    assert_eq!(
        eval_int(
            r#"my $rx = qr/z./;
            ("aze" =~ $rx) ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn substitution_global_flag_replaces_every_occurrence() {
    assert_eq!(
        eval_string(
            r#"my $t = "aaa";
            $t =~ s/a/b/g;
            $t"#
        ),
        "bbb"
    );
}

// ── `mesh`, `shuffle` (length only) ──

#[test]
fn bare_builtin_mesh_interleaves_parallel_lists() {
    assert_eq!(eval_string(r#"join "", mesh(1, 2, 10, 20)"#), "121020");
}

#[test]
fn shuffle_returns_permutation_of_same_length() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @s = shuffle(7, 8, 9, 10);
            scalar @s"#
        ),
        4
    );
}

// ── Array stack ops: `pop` / `unshift` ──

#[test]
fn pop_empty_array_yields_undef() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @empty;
            defined(pop @empty) ? 1 : 0"#
        ),
        0
    );
}

#[test]
fn unshift_returns_new_array_length() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @st;
            unshift @st, 40, 50"#
        ),
        2
    );
}

#[test]
fn array_dollar_hash_last_index() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @ix = (100, 200, 300);
            $#ix"#
        ),
        2
    );
}

// ── `split` on non-trivial pattern; `sprintf` float ──

#[test]
fn split_on_escaped_metachar_pattern() {
    assert_eq!(
        eval_string(
            r#"my $row = "x|y|z";
            join "-", split /\|/, $row"#
        ),
        "x-y-z"
    );
}

#[test]
fn sprintf_one_decimal_sqrt_two() {
    assert_eq!(eval_string(r#"sprintf("%.1f", sqrt(2))"#), "1.4");
}

// ── Transcendentals at π; empty `product` ──

#[test]
fn atan2_negative_x_axis_is_pi() {
    assert_eq!(eval_int(r#"int(atan2(0, -1) * 1000)"#), 3141);
}

#[test]
fn cosine_at_pi_is_negative_one() {
    assert_eq!(
        eval_int(
            r#"my $pi = atan2(0, -1);
            int(cos($pi) * 1000)"#
        ),
        -1000
    );
}

/// `product()` of an empty list is `1` (the multiplicative identity).
/// Pinned at the unit-test layer in `strykelang/list_builtins.rs`
/// (`product_empty_is_one`); this is the user-visible pin from the
/// bytecode VM path. Compare with `sum0()` → `0` (additive identity) and
/// `sum()` → `undef`.
#[test]
fn bare_builtin_product_empty_list_is_one() {
    assert_eq!(eval_int(r#"product()"#), 1);
}

// ── `defined`, `undef`; string min/max ──

#[test]
fn assign_undef_makes_defined_false() {
    assert_eq!(
        eval_int(
            r#"my $w = 1;
            $w = undef;
            defined($w) ? 1 : 0"#
        ),
        0
    );
}

#[test]
fn bare_builtin_minstr_maxstr_lexical() {
    assert_eq!(
        eval_string(r#"join ",", minstr("dog", "cat"), maxstr("dog", "cat")"#),
        "cat,dog"
    );
}

#[test]
fn values_sorted_join_numeric_strings() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my %n = (x => 30, y => 10, z => 20);
            join "", sort { $a <=> $b } values %n"#
        ),
        "102030"
    );
}

// ── More list builtins: zip_shortest, mesh_shortest, uniqstr/uniqnum, pairs ──

#[test]
fn bare_builtin_zip_shortest_first_row_snapshot() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @z = zip_shortest(1, 2, 3, 10, 20);
            $z[0]->[0] + $z[0]->[1]"#
        ),
        3
    );
}

#[test]
fn bare_builtin_mesh_shortest_interleaves_until_shorter_exhausted() {
    assert_eq!(
        eval_string(r#"join "", mesh_shortest(1, 2, 3, 10, 20)"#),
        "1231020"
    );
}

#[test]
fn bare_builtin_uniqstr_and_uniqnum_dedupe() {
    assert_eq!(eval_string(r#"join "-", uniqstr("a", "a", "b")"#), "a-b");
    assert_eq!(eval_string(r#"join "-", uniqnum(1.0, 1, 2)"#), "1-2");
}

#[test]
fn bare_builtin_pairs_returns_one_pair_object_per_kv() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @pr = pairs(1, 10, 2, 20);
            scalar @pr"#
        ),
        2
    );
}

// ── `opendir` / `readdir` / `closedir`, `glob` (temp dir, deterministic) ──

#[test]
fn readdir_lists_created_files_in_directory() {
    let dir: PathBuf = std::env::temp_dir().join(format!("stryke_fix_rd_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("alpha.txt"), b"a").unwrap();
    std::fs::write(dir.join("beta.txt"), b"b").unwrap();
    let p = dir.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(
            r#"opendir DH, "{p}";
            my @f = readdir DH;
            closedir DH;
            (scalar grep {{ $_ eq "alpha.txt" }} @f) && (scalar grep {{ $_ eq "beta.txt" }} @f) ? 1 : 0"#
        )),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn readdir_list_context_second_read_on_same_handle_is_empty() {
    let dir: PathBuf = std::env::temp_dir().join(format!("stryke_fix_rd2_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("alpha.txt"), b"a").unwrap();
    std::fs::write(dir.join("beta.txt"), b"b").unwrap();
    let p = dir.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(
            r#"opendir DH, "{p}";
            my @f = readdir DH;
            my @g = readdir DH;
            closedir DH;
            (scalar @f > 0) && (scalar @g == 0) ? 1 : 0"#
        )),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn readdir_rewinddir_refills_stream_same_length_as_first_list_read() {
    let dir: PathBuf = std::env::temp_dir().join(format!("stryke_fix_rdrw_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("alpha.txt"), b"a").unwrap();
    std::fs::write(dir.join("beta.txt"), b"b").unwrap();
    let p = dir.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(
            r#"opendir DH, "{p}";
            my @a = readdir DH;
            my @b = readdir DH;
            rewinddir DH;
            my @c = readdir DH;
            closedir DH;
            (scalar @b == 0)
                && (scalar @c == scalar @a)
                && (scalar grep {{ $_ eq "alpha.txt" }} @c)
                && (scalar grep {{ $_ eq "beta.txt" }} @c)
                ? 1 : 0"#
        )),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn telldir_after_list_readdir_matches_number_of_entries_read() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_telld_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("only.txt"), b"x").unwrap();
    let p = dir.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(
            r#"opendir DH, "{p}";
            my @f = readdir DH;
            my $t = telldir DH;
            closedir DH;
            ($t == scalar @f) ? 1 : 0"#
        )),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn glob_expands_star_pattern_in_directory() {
    let dir: PathBuf = std::env::temp_dir().join(format!("stryke_fix_glob_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("one.g"), b"1").unwrap();
    std::fs::write(dir.join("two.g"), b"2").unwrap();
    let p = dir.to_str().expect("utf-8");
    let out = eval_string(&format!(
        r#"my $pat = "{p}" . "/*.g";
        join "|", sort glob $pat"#
    ));
    let mut parts: Vec<&str> = out.split('|').collect();
    parts.sort();
    assert_eq!(parts.join("|"), format!("{p}/one.g|{p}/two.g"));
    std::fs::remove_dir_all(&dir).ok();
}

// ── Case mapping (`uc`/`lc`/`ucfirst`/`lcfirst`/`fc`) ──

#[test]
fn ucfirst_lcfirst_toggle_initial_case_ascii() {
    assert_eq!(
        eval_string(r#"my $u = ucfirst "hello"; my $l = lcfirst "HELLO"; $u . " " . $l"#),
        "Hello hELLO"
    );
}

#[test]
fn lc_uc_round_trip_non_ascii_letter() {
    assert_eq!(eval_string(r#"lc "Ä""#), "ä");
    assert_eq!(eval_string(r#"uc "ä""#), "Ä");
}

#[test]
fn fc_foldcase_german_eszett() {
    assert_eq!(eval_string(r#"fc "Straße""#), "strasse");
}

// ── `split` limits, `chop`, `sprintf` extra formats, `ref`, numeric coercion ──

#[test]
fn split_negative_limit_preserves_trailing_empty_field() {
    assert_eq!(eval_string(r#"join "-", split /:/, "a:b:", -1"#), "a-b-");
}

#[test]
fn chop_removes_one_character_from_end() {
    assert_eq!(eval_string(r#"my $s = "ab\n"; chop $s; $s"#), "ab");
    assert_eq!(eval_string(r#"my $t = "ab"; chop $t; $t"#), "a");
}

#[test]
fn sub_return_list_in_scalar_context_yields_last_element() {
    assert_eq!(
        eval_int(
            r#"fn trip { return (10, 20, 30) }
            my $x = trip();
            $x"#
        ),
        30
    );
}

#[test]
fn join_inserts_empty_between_defined_elements_when_slot_is_undef() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, undef, 2);
            join "-", @a"#
        ),
        "1--2"
    );
}

#[test]
fn addition_uses_numeric_prefix_of_string_operand() {
    assert_eq!(eval_int(r#"5 + "3z""#), 8);
}

#[test]
fn exponentiation_two_to_ten_is_1024() {
    assert_eq!(eval_int(r#"2 ** 10"#), 1024);
}

#[test]
fn ref_builtin_reports_array_and_hash_container_types() {
    assert_eq!(eval_string(r#"ref []"#), "ARRAY");
    assert_eq!(eval_string(r#"ref {}"#), "HASH");
}

#[test]
fn sprintf_percent_b_and_percent_o_formats() {
    assert_eq!(eval_string(r#"sprintf "%b", 5"#), "101");
    assert_eq!(eval_string(r#"sprintf "%o", 8"#), "10");
}

#[test]
fn bare_builtin_uniqint_deduplicates_integer_values() {
    assert_eq!(eval_string(r#"join "-", uniqint(1, 1, 2, 2, 3)"#), "1-2-3");
}

#[test]
fn string_concatenation_interpolates_number_as_string() {
    assert_eq!(eval_string(r#""n" . 42 . "x""#), "n42x");
}

#[test]
fn filetest_f_is_false_for_directory_path() {
    let dir: PathBuf = std::env::temp_dir().join(format!("stryke_fix_ftd_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(
            r#"(-e "{p}") && (! -f "{p}") && (-d "{p}") ? 1 : 0"#
        )),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn postincrement_in_array_subscript_uses_then_advances_index() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @x = (1, 2, 3);
            my $i = 0;
            $x[$i++] = 9;
            join "-", @x"#
        ),
        "9-2-3"
    );
}

// ── More builtins: `abs`, `substr`, `pos`, numeric bases, `sprintf`, shifts, `sort`/`splice`, assigns ──

#[test]
fn abs_returns_non_negative_for_negative_operand() {
    assert_eq!(eval_int(r#"abs(-7)"#), 7);
    assert_eq!(eval_int(r#"abs(7)"#), 7);
}

#[test]
fn substr_four_arg_replaces_leading_slice_in_place() {
    assert_eq!(
        eval_string(r#"my $s = "abc"; substr($s, 0, 1, "z"); $s"#),
        "zbc"
    );
}

#[test]
fn pos_sets_and_reads_match_offset_into_string() {
    assert_eq!(eval_int(r#"my $s = "abc"; pos($s) = 1; pos($s)"#), 1);
}

#[test]
fn oct_accepts_binary_literal_prefix() {
    assert_eq!(eval_int(r#"oct("0b101")"#), 5);
}

#[test]
fn hex_accepts_0x_prefix() {
    assert_eq!(eval_int(r#"hex("0x10")"#), 16);
}

#[test]
fn unary_minus_applies_to_numeric_string() {
    assert_eq!(eval_int(r#"-("3") + 1"#), -2);
}

#[test]
fn length_empty_string_and_index_into_empty() {
    assert_eq!(eval_int(r#"length ''"#), 0);
    assert_eq!(eval_int(r#"index '', 'x'"#), -1);
}

#[test]
fn int_truncates_float_toward_zero_after_division() {
    assert_eq!(eval_string(r#""". int(10 / 4)"#), "2");
}

#[test]
fn modulo_assign_updates_lvalue() {
    assert_eq!(eval_int(r#"my $x = 7; $x %= 3; $x"#), 1);
}

#[test]
fn regex_match_scalar_context_truthy_and_falsy() {
    assert_eq!(eval_int(r#"my $s = "aba"; ($s =~ /a/) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "aba"; ($s =~ /c/) ? 1 : 0"#), 0);
}

#[test]
fn cmp_and_spaceship_three_way_compare() {
    assert_eq!(eval_int(r#""b" cmp "a""#), 1);
    assert_eq!(eval_int(r#"(2 <=> 5)"#), -1);
    assert_eq!(eval_int(r#"(5 <=> 5)"#), 0);
}

#[test]
fn delete_array_element_leaves_undef_slot() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            delete $a[1];
            join "-", map { defined($_) ? $_ : "u" } @a"#
        ),
        "1-u-3"
    );
}

#[test]
fn sprintf_scientific_and_bankers_round_snapshot() {
    assert_eq!(eval_string(r#"sprintf "%e", 1.23"#), "1.230000e0");
    assert_eq!(eval_string(r#"sprintf "%.2f", 1.005"#), "1.00");
}

#[test]
fn bit_shift_assign_updates_scalar() {
    assert_eq!(eval_int(r#"my $x = 8; $x >>= 1; $x"#), 4);
    assert_eq!(eval_int(r#"my $x = 3; $x <<= 2; $x"#), 12);
}

#[test]
fn sort_block_numeric_descending() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (9, 8, 7);
            @a = sort { $b <=> $a } @a;
            join "", @a"#
        ),
        "987"
    );
}

#[test]
fn splice_removes_middle_element_from_named_array() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            splice @a, 1, 1;
            join "", @a"#
        ),
        "13"
    );
}

#[test]
fn splice_replaces_one_element_with_two() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            splice @a, 1, 1, 9, 8;
            join "", @a"#
        ),
        "1983"
    );
}

#[test]
fn exponentiation_is_right_associative() {
    assert_eq!(eval_int(r#"2 ** 3 ** 2"#), 512);
}

#[test]
fn unshift_prepends_multiple_values() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1);
            unshift @a, 9, 8;
            join "", @a"#
        ),
        "981"
    );
}

#[test]
fn split_limit_leaves_remainder_in_final_field() {
    assert_eq!(eval_string(r#"join "-", split /\|/, "a|b|c", 2"#), "a-b|c");
}

#[test]
fn multiply_and_divide_assign_on_scalar() {
    assert_eq!(eval_int(r#"my $x = 1; $x *= 5; $x"#), 5);
    assert_eq!(eval_string(r#"my $x = 10; $x /= 4; "". $x"#), "2.5");
}

#[test]
fn logical_or_defined_or_and_assign_mutators() {
    assert_eq!(eval_int(r#"my $x = 0; $x ||= 7; $x"#), 7);
    assert_eq!(eval_int(r#"my $x = 5; $x &&= 0; $x"#), 0);
    assert_eq!(eval_int(r#"my $x = 5; $x //= 3; $x"#), 5);
    assert_eq!(eval_int(r#"my $x; $x //= 8; $x"#), 8);
}

#[test]
fn uc_empty_string_and_eszett_snapshot() {
    assert_eq!(eval_string(r#"uc """#), "");
    assert_eq!(eval_string(r#"uc "ß""#), "SS");
    assert_eq!(eval_int(r#"length uc "ß""#), 2);
}

#[test]
fn list_assign_unpacks_lexical_pair_from_rhs() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my ($a, $b) = (2, 3);
            $a + $b"#
        ),
        5
    );
}

#[test]
fn hash_slice_list_context_reads_quoted_keys() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my %h = ("x", 1, "y", 2);
            join "-", @h{"x", "y"}"#
        ),
        "1-2"
    );
}

#[test]
fn array_slice_list_context_non_contiguous_indices() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            join "", @a[0, 2]"#
        ),
        "13"
    );
}

#[test]
fn pack_space_padded_unpack_hex_matches_octets() {
    assert_eq!(eval_string(r#"unpack("H*", pack("a3", "ab"))"#), "616200");
}

#[test]
fn sprintf_escapes_percent_literal() {
    assert_eq!(eval_string(r#"sprintf "%s%%", 50"#), "50%");
}

#[test]
fn clearing_named_array_or_hash_empties_aggregate() {
    assert_eq!(eval_int(r#"my @a = (1, 2, 3); @a = (); scalar @a"#), 0);
    assert_eq!(eval_int(r#"my %h = ("u", 1); %h = (); scalar keys %h"#), 0);
}

#[test]
fn readpipe_qx_strips_trailing_newline_with_chomp() {
    assert_eq!(
        eval_string(r#"my $o = `printf '%s' ab`; chomp $o; $o"#,),
        "ab"
    );
}

#[test]
fn bless_scalar_reference_sets_package_name() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $x = 1;
            my $r = bless \$x, "PkgZ";
            ref $r"#
        ),
        "PkgZ"
    );
}

#[test]
fn array_max_index_dollar_hash_after_sparse_tail_assign() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a = (1, 2);
            $a[5] = 1;
            $#a"#
        ),
        5
    );
}

#[test]
fn scalar_array_counts_past_max_index_including_holes() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a = (1, 2);
            $a[5] = 1;
            scalar @a"#
        ),
        6
    );
}

// ── Regex capture fields, logic, `$"`, `sprintf`, `substr`, assigns, truthiness, `sort`, dir, filetests ──

#[test]
fn regex_prematch_match_and_capture_concatenate() {
    assert_eq!(
        eval_string(
            r#"my $s = "abc";
            $s =~ /(b)/;
            ${^PREMATCH} . "-" . $& . "-" . $1"#
        ),
        "a-b-b"
    );
}

#[test]
fn logical_not_coerces_to_boolean() {
    assert_eq!(eval_int(r#"(not 0) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"(not 1) ? 1 : 0"#), 0);
}

#[test]
fn postfix_if_skips_statement_when_condition_false() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $x;
            $x = 5 if 0;
            defined($x) ? 1 : 0"#
        ),
        0
    );
}

#[test]
fn postfix_if_skips_postincrement_when_condition_false() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $x = 0;
            $x++ if 0;
            $x"#
        ),
        0
    );
}

#[test]
fn join_uses_subfield_separator_for_interpolation() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2);
            join $", @a"#
        ),
        "1 2"
    );
}

#[test]
fn sprintf_string_width_right_and_left_align() {
    assert_eq!(eval_string(r#"sprintf "[%10s]", "ab""#), "[        ab]");
    assert_eq!(eval_string(r#"sprintf "%-10s|", "ab""#), "ab        |");
}

#[test]
fn substr_with_negative_offset_counts_from_end() {
    assert_eq!(eval_string(r#"my $s = "abc"; substr($s, -2)"#), "bc");
}

#[test]
fn array_element_lookup_truncates_float_subscript_toward_zero() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a = (10, 20, 30);
            $a[1.9]"#
        ),
        20
    );
}

#[test]
fn modulo_negative_dividend_snapshot() {
    assert_eq!(eval_int(r#"(-5) % 3"#), -2);
}

#[test]
fn sprintf_g_style_compact_float_snapshot() {
    assert_eq!(eval_string(r#"sprintf "%g", 1234.56"#), "1234.560000");
}

#[test]
fn bitwise_or_assign_and_and_assign_combine() {
    assert_eq!(eval_int(r#"my $x = 5; $x |= 2; $x"#), 7);
    assert_eq!(eval_int(r#"my $x = 5; $x &= 3; $x"#), 1);
}

#[test]
fn unary_plus_coerces_numeric_string() {
    assert_eq!(eval_int(r#"+("3")"#), 3);
}

#[test]
fn dot_equal_appends_to_scalar() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $x = 1;
            $x .= 2;
            $x"#
        ),
        "12"
    );
}

#[test]
fn falsey_strings_and_zero_in_boolean_ternary() {
    assert_eq!(eval_int(r#"my $z = 0; $z ? 1 : 0"#), 0);
    assert_eq!(eval_int(r#"my $z = ""; $z ? 1 : 0"#), 0);
    assert_eq!(eval_int(r#"my $z = "0"; $z ? 1 : 0"#), 0);
}

#[test]
fn ord_chr_round_trip_for_ascii_printable() {
    assert_eq!(eval_string(r#"my $c = "M"; chr(ord($c))"#), "M");
}

#[test]
fn scalar_util_blessed_reports_package_for_blessed_ref() {
    assert_eq!(eval_string(r#"blessed(bless {}, "Zpkg")"#), "Zpkg");
}

#[test]
fn scalar_util_reftype_array_under_blessed_arrayref() {
    assert_eq!(eval_string(r#"reftype(bless [], "Zpkg")"#), "ARRAY");
}

#[test]
fn for_list_topic_undefined_after_bare_for_loop() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $ok;
            for $_ (4, 5, 6) { }
            $ok = defined($_) ? 0 : 1;
            $ok"#
        ),
        1
    );
}

#[test]
fn sort_block_orders_by_string_length() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = sort { length($a) <=> length($b) } qw(xx x xxx);
            join "", @a"#
        ),
        "xxxxxx"
    );
}

#[test]
fn push_evaluates_arithmetic_before_append() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a;
            push @a, 1 + 2 * 3;
            $a[0]"#
        ),
        7
    );
}

#[test]
fn seekdir_zero_then_scalar_readdir_matches_first_list_entry() {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("stryke_fix_skdir_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("first.txt"), b"a").unwrap();
    std::fs::write(dir.join("second.txt"), b"b").unwrap();
    let p = dir.to_str().expect("utf-8");
    assert_eq!(
        eval_int(&format!(
            r#"opendir DH, "{p}";
            my @all = readdir DH;
            seekdir DH, 0;
            my $one = readdir DH;
            closedir DH;
            ($one eq $all[0]) ? 1 : 0"#
        )),
        1
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn filetest_z_true_for_empty_regular_file() {
    let dir: PathBuf = std::env::temp_dir().join(format!("stryke_fix_z_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("empty.dat");
    std::fs::write(&path, b"").unwrap();
    let p = path.to_str().expect("utf-8");
    assert_eq!(eval_int(&format!(r#"(-f "{p}") && (-z "{p}") ? 1 : 0"#)), 1);
    std::fs::remove_dir_all(&dir).ok();
}

// ── Postfix `unless`, regex flags / classes / lookahead / `\Q`, `split //`, `pack` endian, bitwise, `**` edges ──

#[test]
fn postfix_unless_runs_statement_when_condition_is_false() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $x;
            $x = 7 unless 0;
            $x"#
        ),
        7
    );
}

#[test]
fn regex_inline_case_insensitive_flag_matches_uppercase_letter() {
    assert_eq!(eval_string(r#"my $s = "abc"; $s =~ /(?i)B/; $&"#), "b");
}

#[test]
fn regex_digit_class_global_match_joins_all_digits() {
    assert_eq!(eval_string(r#"join "", ("a1b2c3" =~ /\d/g)"#), "123");
}

#[test]
fn regex_capture_plus_quantifier_takes_longest_digit_run() {
    assert_eq!(
        eval_string(r#"my $s = "foo123bar"; $s =~ /(\d+)/; $1"#),
        "123"
    );
}

#[test]
fn regex_positive_lookahead_requires_following_char() {
    assert_eq!(eval_int(r#"my $s = "axc"; ($s =~ /a(?=x)/) ? 1 : 0"#), 1);
}

#[test]
fn regex_single_line_flag_lets_dot_match_newline() {
    assert_eq!(eval_int(r#"my $s = "a\nb"; ($s =~ /a.b/s) ? 1 : 0"#), 1);
}

#[test]
fn regex_multiline_flag_lets_caret_match_after_newline() {
    assert_eq!(eval_int(r#"my $s = "a\nb"; ($s =~ /^b/m) ? 1 : 0"#), 1);
}

#[test]
fn regex_string_begin_anchor_does_not_use_multiline_line_starts() {
    assert_eq!(eval_int(r#"my $s = "a\nb"; ($s =~ /\Ab/m) ? 1 : 0"#), 0);
}

#[test]
fn regex_quotemeta_span_treats_metachar_as_literal() {
    assert_eq!(eval_int(r#"my $s = "x*y"; ($s =~ /\Q*\E/) ? 1 : 0"#), 1);
}

#[test]
fn split_slash_slash_includes_empty_fields_around_chars() {
    assert_eq!(eval_string(r#"join "-", split //, "ab""#), "-a-b-");
}

#[test]
fn array_index_slice_with_dot_dot_range_in_list_context() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (10, 20, 30);
            join "-", @a[0 .. 2]"#
        ),
        "10-20-30"
    );
}

#[test]
fn int_truncates_positive_float_and_stops_at_non_numeric_tail() {
    assert_eq!(eval_int(r#"int(2.7)"#), 2);
    assert_eq!(eval_int(r#"int("3.9xyz")"#), 3);
}

#[test]
fn exponentiation_zero_raised_to_zero_is_one() {
    assert_eq!(eval_int(r#"2 ** 0"#), 1);
    assert_eq!(eval_int(r#"0 ** 0"#), 1);
}

#[test]
fn unary_minus_binds_after_exponentiation() {
    assert_eq!(eval_int(r#"-2 ** 2"#), -4);
}

#[test]
fn numeric_concatenation_chain_coerces_to_string() {
    assert_eq!(eval_string(r#"1 . 2 . 3"#), "123");
}

#[test]
fn compound_plus_eq_and_star_star_eq_update_lexical() {
    assert_eq!(eval_int(r#"my $a = 1; $a += 2; $a"#), 3);
    assert_eq!(eval_int(r#"my $a = 2; $a **= 3; $a"#), 8);
}

#[test]
fn zero_e_zero_string_true_in_boolean_coerces_numeric_to_zero() {
    assert_eq!(eval_int(r#""0e0" ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"0 + "0e0""#), 0);
}

#[test]
fn sprintf_uppercase_hex_and_char_conversion() {
    assert_eq!(eval_string(r#"sprintf "%X", 255"#), "FF");
    assert_eq!(eval_string(r#"sprintf "%c", 65"#), "A");
}

#[test]
fn sprintf_one_fractional_digit_rounding_snapshot() {
    assert_eq!(eval_string(r#"sprintf "%.1f", 2.25"#), "2.2");
}

#[test]
fn bitwise_unary_tilde_is_complement() {
    assert_eq!(eval_int(r#"~1"#), -2);
}

#[test]
fn pack_little_endian_v_and_big_endian_n_differ_for_same_integer() {
    assert_eq!(eval_string(r#"unpack("H*", pack("v", 0x3412))"#), "1234");
    assert_eq!(eval_string(r#"unpack("H*", pack("n", 0x3412))"#), "3412");
}

// ── More regex, `reduce`/`grep`, `pack`/`unpack`, `**` fractional, `sqrt`, `s///g` count, aggregates ──

#[test]
fn regex_negative_lookahead_rejects_when_followed_by_literal() {
    assert_eq!(eval_int(r#"my $s = "axc"; ($s =~ /a(?!y)/) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "ayc"; ($s =~ /a(?!y)/) ? 1 : 0"#), 0);
}

#[test]
fn regex_non_greedy_plus_stops_at_first_satisfying_suffix() {
    assert_eq!(eval_int(r#"my $s = "aab"; ($s =~ /a+?b/) ? 1 : 0"#), 1);
}

#[test]
fn regex_backreference_requires_repeated_substring() {
    assert_eq!(eval_int(r#"my $s = "abab"; ($s =~ /(ab)\1/) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "abac"; ($s =~ /(ab)\1/) ? 1 : 0"#), 0);
}

#[test]
fn regex_whitespace_word_and_boundary_classes() {
    assert_eq!(eval_int(r#"my $s = "a b"; ($s =~ /\s/) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "a1"; ($s =~ /\w/) ? 1 : 0"#), 1);
    assert_eq!(
        eval_int(r#"my $s = "hello world"; ($s =~ /\bworld\b/) ? 1 : 0"#),
        1
    );
}

#[test]
fn dollar_plus_is_string_value_of_last_paren_capture() {
    assert_eq!(
        eval_string(
            r#"my $s = "abc";
            $s =~ /(b)/;
            $+"#
        ),
        "b"
    );
}

#[test]
fn regex_fixed_length_lookbehind_matches_after_prefix() {
    assert_eq!(eval_int(r#"my $s = "cba"; ($s =~ /(?<=b)a/) ? 1 : 0"#), 1);
}

#[test]
fn regex_non_capturing_group_omits_slot_between_numbered_groups() {
    assert_eq!(
        eval_string(
            r#"my $s = "aba";
            $s =~ /(a)(?:b)(a)/;
            $1 . "-" . $2"#
        ),
        "a-a"
    );
}

#[test]
fn regex_literal_tab_and_newline_escapes() {
    assert_eq!(eval_int(r#"my $s = "a\tb"; ($s =~ /\t/) ? 1 : 0"#), 1);
    assert_eq!(eval_int(r#"my $s = "a\nb"; ($s =~ /\n/) ? 1 : 0"#), 1);
}

#[test]
fn minus_eq_and_xor_eq_compound_assignments() {
    assert_eq!(eval_int(r#"my $x = 5; $x -= 2; $x"#), 3);
    assert_eq!(eval_int(r#"my $x = 5; $x ^= 3; $x"#), 6);
}

#[test]
fn sprintf_plus_f_positive_float_snapshot() {
    assert_eq!(eval_string(r#"sprintf "%+f", 3.14"#), "3.140000");
}

#[test]
fn ref_anon_subroutine_is_code() {
    assert_eq!(eval_string(r#"ref fn { 1 }"#), "CODE");
}

#[test]
fn bare_builtin_reduce_concatenates_list_left_to_right() {
    assert_eq!(eval_string(r#"qw(x y z) |> reduce { $a . $b }"#), "xyz");
}

#[test]
fn bare_builtin_fold_alias_concatenates_like_reduce() {
    assert_eq!(eval_string(r#"qw(x y z) |> fold { $a . $b }"#), "xyz");
}

#[test]
fn grep_block_logical_not_selects_even_numbers() {
    assert_eq!(
        eval_string(r#"join "", grep { !($_ % 2) } 1, 2, 3, 4"#),
        "24"
    );
}

#[test]
fn unpack_x_skips_bytes_between_fixed_width_hex_fields() {
    assert_eq!(eval_string(r#"unpack("H2x2H2", "abcd")"#), "6164");
}

#[test]
fn pack_n_big_endian_and_v_little_endian_32_bit() {
    assert_eq!(
        eval_string(r#"unpack("H*", pack("N", 0x01020304))"#),
        "01020304"
    );
    assert_eq!(
        eval_string(r#"unpack("H*", pack("V", 0x01020304))"#),
        "04030201"
    );
}

#[test]
fn exponentiation_negative_integer_exponent_is_reciprocal() {
    assert_eq!(eval_string(r#"sprintf "%.5f", 2 ** -2"#), "0.25000");
}

#[test]
fn exponentiation_half_is_square_root() {
    assert_eq!(eval_string(r#"sprintf "%.5g", 4 ** 0.5"#), "2.00000");
}

#[test]
fn sqrt_builtin_integer_perfect_square() {
    assert_eq!(eval_int(r#"sqrt(9)"#), 3);
}

#[test]
fn substitution_global_in_scalar_context_counts_replacements() {
    assert_eq!(
        eval_int(
            r#"my $s = "aba";
            $s =~ s/b/_/g"#
        ),
        1
    );
}

#[test]
fn array_list_assignment_copies_elements_for_independent_scalars() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            my @b = @a;
            $b[0] = 9;
            join "", @a"#
        ),
        "123"
    );
}

#[test]
fn anon_hash_ref_arrow_access_reads_value() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $r = { "x", 1 };
            $r->{"x"}"#
        ),
        1
    );
}

#[test]
fn anon_array_ref_arrow_access_reads_index() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $r = [10, 20];
            $r->[1]"#
        ),
        20
    );
}

// ── Lookbehind, `POSTMATCH`, `Regexp` ref, `prototype`, `substr` delete, `sprintf` `#`, `split`, `m{}` ──

#[test]
fn regex_negative_lookbehind_global_count_differs_by_prefix() {
    assert_eq!(eval_int(r#"my $s = "cba"; 0 + ($s =~ /(?<!b)a/g)"#), 0);
    assert_eq!(eval_int(r#"my $s = "aca"; 0 + ($s =~ /(?<!b)a/g)"#), 2);
}

#[test]
fn regex_postmatch_is_suffix_after_match() {
    assert_eq!(
        eval_string(
            r#"my $s = "abc";
            $s =~ /b/;
            ${^POSTMATCH}"#
        ),
        "c"
    );
}

#[test]
fn ref_qr_returns_regexp() {
    assert_eq!(eval_string(r#"ref qr/ab/"#), "Regexp");
}

#[test]
fn named_sub_prototypes_empty_and_scalar_snapshot() {
    assert_eq!(
        eval_int(
            r#"fn empty_proto { 1 }
            (prototype(\&empty_proto) eq "") ? 1 : 0"#
        ),
        1
    );
    assert_eq!(
        eval_string(
            r#"fn one_scalar ($) { 1 }
            prototype(\&one_scalar)"#
        ),
        "$"
    );
}

#[test]
fn substr_four_arg_empty_replacement_removes_span() {
    assert_eq!(
        eval_string(
            r#"my $s = "hello";
            substr($s, 1, 2, "");
            $s"#
        ),
        "hlo"
    );
}

#[test]
fn sprintf_alternate_form_octal_and_hex_snapshot() {
    assert_eq!(eval_string(r#"sprintf "%#.3o", 8"#), "10");
    assert_eq!(eval_string(r#"sprintf "%#x", 255"#), "ff");
}

#[test]
fn split_slash_s_plus_splits_on_whitespace_runs() {
    assert_eq!(eval_string(r#"join "-", split /\s+/, "  a b ""#), "-a-b-");
}

#[test]
fn push_flattens_list_repeat_operand() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1);
            push @a, (0) x 3;
            join "", @a"#
        ),
        "1000"
    );
}

#[test]
fn list_assign_from_empty_list_leaves_scalar_undef() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my ($u) = ();
            defined($u) ? 1 : 0"#
        ),
        0
    );
}

#[test]
fn coderef_invoked_with_arrow_operator() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $f = fn { 9 };
            $f->()"#
        ),
        9
    );
}

#[test]
fn regex_possessive_star_matches_minimal_then_literal() {
    assert_eq!(eval_int(r#"my $s = "aab"; ($s =~ /a*+b/) ? 1 : 0"#), 1);
}

#[test]
fn map_block_multiplies_numeric_strings_from_qw() {
    assert_eq!(eval_string(r#"join "", map { $_ * 2 } qw(1 2)"#), "24");
}

#[test]
fn regex_horizontal_whitespace_class_matches_tab() {
    assert_eq!(eval_int(r#"my $s = "a\tb"; ($s =~ /\h/) ? 1 : 0"#), 1);
}

#[test]
fn regex_vertical_whitespace_class_matches_vertical_tab() {
    assert_eq!(eval_int(r#"my $s = "a\x0bb"; ($s =~ /\v/) ? 1 : 0"#), 1);
}

#[test]
fn regex_brace_delimiter_matches_literal() {
    assert_eq!(eval_int(r#"my $s = "x"; ($s =~ m{x}) ? 1 : 0"#), 1);
}

#[test]
fn regex_brace_delimiter_allows_slash_in_pattern() {
    assert_eq!(eval_int(r#"my $s = "a/b"; ($s =~ m{/}) ? 1 : 0"#), 1);
}

#[test]
fn sort_block_lexical_cmp_orders_pair() {
    assert_eq!(eval_string(r#"join "", sort { $a cmp $b } qw(z y)"#), "yz");
}

// ── `\R` / `\K`, named capture, `sleep`/`srand`, `oct`, `join`, `\p{}`, slices, `index`/`rindex`, `$|`, `m!` ──

#[test]
fn regex_linebreak_class_matches_unicode_newline_sequence() {
    assert_eq!(eval_int(r#"my $s = "a\nb"; ($s =~ /\R/) ? 1 : 0"#), 1);
}

#[test]
fn regex_keep_escape_drops_prefix_from_consumed_match() {
    assert_eq!(eval_int(r#"my $s = "aba"; ($s =~ /a\Kb/) ? 1 : 0"#), 1);
}

#[test]
fn regex_named_capture_accessible_via_plus_brace_hash() {
    assert_eq!(
        eval_string(
            r#"my $s = "ab";
            $s =~ /(?<vx>a)b/;
            $+{vx}"#
        ),
        "a"
    );
}

#[test]
fn sleep_zero_returns_immediately() {
    assert_eq!(eval_int(r#"sleep 0"#), 0);
}

#[test]
fn srand_makes_rand_repeatable_for_same_seed() {
    assert_eq!(
        eval_int(
            r#"srand 42;
            my $a = rand();
            srand 42;
            my $b = rand();
            ($a == $b) ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn oct_interprets_leading_zero_as_octal_digits() {
    assert_eq!(eval_int(r#"oct("0377")"#), 255);
}

#[test]
fn unicode_property_uppercase_letter_class() {
    assert_eq!(eval_int(r#"my $s = "A"; ($s =~ /\p{Lu}/) ? 1 : 0"#), 1);
}

#[test]
fn unicode_property_numeric_class() {
    assert_eq!(eval_int(r#"my $s = "7"; ($s =~ /\p{N}/) ? 1 : 0"#), 1);
}

#[test]
fn array_slice_assign_replaces_contiguous_prefix_elements() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            @a[0, 1] = (9, 8);
            join "", @a"#
        ),
        "983"
    );
}

#[test]
fn index_with_position_targets_later_occurrence() {
    assert_eq!(eval_int(r#"index("hello", "l", 3)"#), 3);
}

#[test]
fn rindex_with_max_position_finds_last_at_or_before() {
    assert_eq!(eval_int(r#"rindex("hello", "l", 3)"#), 3);
}

#[test]
fn output_autoflush_scalar_one_when_enabled() {
    assert_eq!(eval_int(r#"$| = 1; $| ? 1 : 0"#), 1);
}

#[test]
fn regex_exclamation_delimiter_avoids_slash_escapes() {
    assert_eq!(eval_int(r#"my $s = "a/b"; ($s =~ m!/!) ? 1 : 0"#), 1);
}

// ── `study`, numeric `0+`, slices, `(1)xN`, `push`/`chop`/`chomp`, `sprintf %%`, `splice`+`qw`, `eq`/`==`, `\x{}`, list assign, `reverse sort`, `pack` ──

#[test]
fn numeric_plus_coerces_zero_padded_digit_string_as_decimal() {
    // Unlike `oct("0377")` (octal source), `0+` on this string follows decimal-ish coercion (377).
    assert_eq!(eval_int(r#"0 + "0377""#), 377);
}

#[test]
fn study_on_literal_returns_truthy() {
    assert_eq!(eval_int(r#"study "xyzzy""#), 1);
}

#[test]
fn list_literal_slice_noncontiguous_indices() {
    assert_eq!(eval_string(r#"join "", (9, 8, 7)[0, 2]"#), "97");
}

#[test]
fn list_repeat_of_scalar_in_parens_triples_in_list_context() {
    assert_eq!(eval_string(r#"join "", (1) x 3"#), "111");
}

#[test]
fn push_returns_new_length_when_starting_nonempty() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a = (10, 20);
            push @a, 30"#
        ),
        3
    );
}

#[test]
fn chop_empty_string_leaves_operand_empty() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "";
            chop $s;
            $s"#
        ),
        ""
    );
}

#[test]
fn string_eq_distinguishes_leading_zero_from_plain_digit() {
    assert_eq!(eval_int(r#""05" eq "5" ? 1 : 0"#), 0);
}

#[test]
fn numeric_eq_coerces_leading_zero_string_like_decimal() {
    assert_eq!(eval_int(r#""05" == "5" ? 1 : 0"#), 1);
}

#[test]
fn splice_returns_removed_elements_and_inserts_qw_list() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = qw(a b c);
            my @r = splice @a, 1, 1, qw(x y);
            join("", @r) . ":" . join("", @a)"#
        ),
        "b:axyc"
    );
}

#[test]
fn sprintf_percent_escape_without_value_is_literal_percent() {
    assert_eq!(eval_string(r#"sprintf "%%""#), "%");
}

#[test]
fn chomp_without_trailing_newline_returns_zero() {
    assert_eq!(eval_int(r#"my $s = "nope"; chomp $s"#), 0);
}

#[test]
fn double_quoted_braced_hex_escape_yields_codepoint() {
    assert_eq!(eval_string(r#""\x{41}""#), "A");
}

#[test]
fn double_quoted_braced_u_escape_yields_codepoint() {
    assert_eq!(eval_string(r#""\u{0301}""#), "\u{0301}");
    assert_eq!(eval_string(r#""\u{0041}""#), "A");
    assert_eq!(eval_string(r#""\u{1F600}""#), "\u{1F600}");
}

#[test]
fn double_quoted_u_escape_without_brace_is_case_modifier() {
    // \u without braces is ucfirst, not a Unicode escape
    assert_eq!(eval_string(r#""\uhello""#), "Hello");
}

#[test]
fn octal_escape_unbraced() {
    assert_eq!(eval_string(r#""\101""#), "A"); // octal 101 = 65 = 'A'
    assert_eq!(eval_string(r#""\012""#), "\n"); // octal 012 = 10 = newline
    assert_eq!(eval_string(r#""\0""#), "\0"); // bare \0 = NUL
    assert_eq!(eval_string(r#""\177""#), "\x7F"); // octal 177 = 127 = DEL
}

#[test]
fn octal_escape_braced() {
    assert_eq!(eval_string(r#""\o{101}""#), "A");
    assert_eq!(eval_string(r#""\o{12}""#), "\n");
    assert_eq!(eval_string(r#""\o{360}""#), "\u{F0}"); // octal 360 = 240 = U+00F0
}

#[test]
fn control_char_escape() {
    assert_eq!(eval_string(r#""\cA""#), "\x01");
    assert_eq!(eval_string(r#""\cZ""#), "\x1A");
    assert_eq!(eval_string(r#""\ca""#), "\x01"); // case-insensitive
    assert_eq!(eval_string(r#""\c[""#), "\x1B"); // ESC
}

#[test]
fn unicode_name_escape_u_plus() {
    assert_eq!(eval_string(r#""\N{U+0041}""#), "A");
    assert_eq!(eval_string(r#""\N{U+00E9}""#), "é");
}

#[test]
fn unicode_name_escape_by_name() {
    assert_eq!(eval_string(r#""\N{LATIN SMALL LETTER E WITH ACUTE}""#), "é");
    assert_eq!(eval_string(r#""\N{SNOWMAN}""#), "☃");
}

#[test]
fn list_assign_with_short_rhs_leaves_trailing_lexical_undef() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my ($x, $y) = (1);
            defined($y) ? 1 : 0"#
        ),
        0
    );
}

#[test]
fn scalar_preincrement_returns_incremented_value() {
    assert_eq!(eval_int(r#"my $n = 5; ++$n"#), 6);
}

#[test]
fn reverse_sort_applies_after_lexical_sort() {
    assert_eq!(eval_string(r#"join "", rev sort qw(c a b)"#), "cba");
}

#[test]
fn pack_null_pads_fixed_width_a_template() {
    assert_eq!(
        eval_string(r#"unpack("H*", pack("a5", "hi"))"#),
        "6869000000"
    );
}

// ── `pack Z`, `splice` insert-0, `sprintf`/undef, `x` to empty, `for ..`, split+limit, `//`+`"0"`,
//    `ne`, `keys %$ref`, list flatten, `--$n`, concat + postinc, slice after `undef`, `\e`, `qq|…|` ──

#[test]
fn pack_z_truncates_or_nul_pads_to_width() {
    assert_eq!(eval_string(r#"unpack("H*", pack("Z4", "ab"))"#), "61620000");
}

#[test]
fn splice_zero_length_insert_inserts_before_index_without_removal() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            splice @a, 1, 0, 9;
            join "", @a"#
        ),
        "1923"
    );
}

#[test]
fn sprintf_percent_s_formats_undef_as_empty() {
    assert_eq!(eval_string(r#"sprintf "%s", undef"#), "");
}

#[test]
fn string_repeat_with_zero_copies_is_empty() {
    assert_eq!(eval_string(r#""ab" x 0"#), "");
}

#[test]
fn repeat_undef_as_empty_string_yields_empty() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $u;
            $u x 4"#
        ),
        ""
    );
}

#[test]
fn for_statement_topic_accumulates_range_sum() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $t = 0;
            $t += $_ for 1..3;
            $t"#
        ),
        6
    );
}

#[test]
fn split_regex_with_limit_leaves_remainder_in_last_field() {
    assert_eq!(eval_string(r#"join "-", split /x/, "axbxc", 2"#), "a-bxc");
}

#[test]
fn defined_or_keeps_string_zero_without_coercing_to_fallback() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $z = "0";
            $z // "seven""#
        ),
        "0"
    );
}

#[test]
fn string_ne_distinguishes_empty_from_single_digit_zero() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $e = "";
            ($e ne "0") ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn keys_on_hash_through_scalar_ref_sorted_join() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $r = { b => 2, a => 1 };
            join "", sort keys %$r"#
        ),
        "ab"
    );
}

#[test]
fn comma_operator_flattens_adjacent_parenthesized_lists() {
    assert_eq!(eval_string(r#"join "", (1, 2), (3, 4)"#), "1234");
}

#[test]
fn scalar_postdecrement_returns_prior_then_leaves_one_less() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $n = 5;
            $n--;
            $n"#
        ),
        4
    );
}

#[test]
fn string_concatenation_sees_postincrement_value_after_concat() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $i = 0;
            "" . $i++ . $i"#
        ),
        "01"
    );
}

#[test]
fn list_slice_skips_leading_undef_element() {
    assert_eq!(eval_int(r#"(undef, 9, 8)[1]"#), 9);
}

#[test]
fn escape_e_is_ascii_escape_character() {
    assert_eq!(eval_int(r#"ord("\e")"#), 27);
}

#[test]
fn qq_pipe_delimiter_embeds_slash_without_ending_pattern() {
    assert_eq!(eval_string(r#"qq|a/b|"#), "a/b");
}

// ── `m#`/`s#`, `quotemeta`, magic `++`, `abs`, `substr`, `*=`, `gt`, `do`/`unless`/`else`, ranges, labels, `$$[]`, `index` ──

#[test]
fn regex_hash_delimiter_matches_without_slash_escapes() {
    assert_eq!(eval_int(r#"my $s = "a/b"; ($s =~ m#/#) ? 1 : 0"#), 1);
}

#[test]
fn substitution_hash_delimiters_replace_middle_character() {
    assert_eq!(
        eval_string(
            r#"my $t = "aba";
            $t =~ s#b#B#;
            $t"#
        ),
        "aBa"
    );
}

#[test]
fn quotemeta_empty_string_stays_empty() {
    assert_eq!(eval_string(r#"quotemeta("")"#), "");
}

#[test]
fn preincrement_pure_digit_string_carries_like_iv() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $d = "9";
            ++$d;
            $d"#
        ),
        "10"
    );
}

#[test]
fn abs_returns_positive_magnitude_for_negative_float() {
    assert_eq!(eval_string(r#"sprintf "%.1f", abs(-2.5)"#), "2.5");
}

#[test]
fn substr_two_arg_form_returns_suffix_from_offset() {
    assert_eq!(eval_string(r#"substr("abcde", 2)"#), "cde");
}

#[test]
fn array_element_star_eq_multiplies_slot_in_place() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @v = (4);
            $v[0] *= 3;
            $v[0]"#
        ),
        12
    );
}

#[test]
fn string_gt_lexicographic_orders_digit_before_two_char_string() {
    assert_eq!(eval_int(r#"("2" gt "10") ? 1 : 0"#), 1);
}

#[test]
fn do_block_in_scalar_context_returns_last_expression() {
    assert_eq!(
        eval_int(
            r#"my $x = do { 10; 20 };
            $x"#
        ),
        20
    );
}

#[test]
fn unless_else_runs_first_block_when_condition_is_false() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $o = "";
            unless (0) { $o = "ok" } else { $o = "no" }
            $o"#
        ),
        "ok"
    );
}

#[test]
fn foreach_topic_concatenates_dot_dot_range() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "";
            $s .= $_ for 1..3;
            $s"#
        ),
        "123"
    );
}

#[test]
fn map_block_doubles_each_element_of_range() {
    assert_eq!(eval_string(r#"join "", map { $_ * 2 } 1..3"#), "246");
}

#[test]
fn last_label_exits_outer_loop_from_inner() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $c = 0;
            OUTL: for (1..2) {
                for (1..2) {
                    $c++;
                    last OUTL;
                }
            }
            $c"#
        ),
        1
    );
}

#[test]
fn next_label_advances_outer_for_from_inner() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "";
            OUTN: for (1..2) {
                for (1..2) {
                    $s .= $_ . $_;
                    next OUTN;
                }
            }
            $s"#
        ),
        "1111"
    );
}

#[test]
fn unshift_prepends_literal_before_qw_elements() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @w = qw(a b);
            unshift @w, "z";
            join "", @w"#
        ),
        "zab"
    );
}

#[test]
fn array_ref_elem_via_dollar_dollar_bracket() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $r = [10, 20, 30];
            $$r[1]"#
        ),
        20
    );
}

#[test]
fn index_with_empty_substring_finds_start() {
    assert_eq!(eval_int(r#"index("abc", "")"#), 0);
}

// ── `rindex`/empty, `join` unary, `sprintf` `%b`/`%o`, regex `\\1`, trim, literals, `!~`, hash `++`, `//=`, `$[-1]`, range `join`, `x -N`, `fc`, `atan2` ──

#[test]
fn rindex_with_empty_substring_reports_end_position() {
    assert_eq!(eval_int(r#"rindex("abc", "")"#), 3);
}

#[test]
fn join_with_single_list_argument_stringifies_scalar() {
    assert_eq!(eval_string(r#"join ":", 42"#), "42");
}

#[test]
fn sprintf_zero_pads_binary_field_to_width() {
    assert_eq!(eval_string(r#"sprintf "%03b", 5"#), "101");
}

#[test]
fn regex_backreference_matches_doubled_single_character() {
    assert_eq!(eval_int(r#"my $s = "aa"; ($s =~ /(a)\1/) ? 1 : 0"#), 1);
}

#[test]
fn substitution_alternation_strips_leading_and_trailing_space_runs() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "  hi  ";
            $s =~ s/^\s+|\s+$//g;
            $s"#
        ),
        "hi"
    );
}

#[test]
fn hex_and_oct_numeric_literals_sum_in_expression() {
    assert_eq!(eval_int(r#"0x10 + 075"#), 77);
}

#[test]
fn regex_not_binding_operator_rejects_pattern() {
    assert_eq!(eval_int(r#"my $s = "abc"; ($s !~ /z/) ? 1 : 0"#), 1);
}

#[test]
fn hash_value_postincrement_updates_slot() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %h = (k => 4);
            $h{k}++;
            $h{k}"#
        ),
        5
    );
}

#[test]
fn defined_or_assign_returns_assigned_value_in_expression() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $z;
            ($z //= 17)"#
        ),
        17
    );
}

#[test]
fn array_negative_one_subscript_assigns_tail_element() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @t = (1, 2);
            $t[-1] = 9;
            join "", @t"#
        ),
        "19"
    );
}

#[test]
fn sprintf_percent_o_formats_unsigned_octal() {
    assert_eq!(eval_string(r#"sprintf "%o", 8"#), "10");
}

#[test]
fn join_concatenates_range_list_without_map() {
    assert_eq!(eval_string(r#"join "", 1..4"#), "1234");
}

#[test]
fn string_repeat_with_negative_count_yields_empty() {
    assert_eq!(eval_string(r#""ab" x -3"#), "");
}

#[test]
fn fc_foldcases_ascii_upper_letters() {
    assert_eq!(eval_string(r#"fc "XY""#), "xy");
}

#[test]
fn atan2_second_quadrant_close_to_three_pi_over_four() {
    assert_eq!(eval_int(r#"int(atan2(1, -1) * 1000)"#), 2356);
}

// ── `push`+range, `s///g` counts, `join`/(), default `sort`, `%.0e`, `local`, `s///e`, `reverse` into `@a`, `our`, `map` assign, `uc`, `log`/`exp`, `grep !` ──

#[test]
fn push_appends_range_list_as_separate_elements() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @p;
            push @p, 1..3;
            join "", @p"#
        ),
        "123"
    );
}

#[test]
fn substitution_global_counts_digit_span_replacements() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $s = "a1b2";
            $s =~ s/\d+/*/g"#
        ),
        2
    );
}

#[test]
fn join_with_empty_parenthesized_list_skips_extra_separator_fields() {
    assert_eq!(eval_string(r#"join "-", "a", (), "b""#), "a-b");
}

#[test]
fn sort_default_uses_lexical_string_order_for_digit_prefixes() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @n = (10, 2, 1);
            @n = sort @n;
            join "", @n"#
        ),
        "1102"
    );
}

#[test]
fn sprintf_compact_scientific_rounds_to_single_digit_mantissa() {
    assert_eq!(eval_string(r#"sprintf "%.0e", 1234"#), "1e3");
}

#[test]
fn local_restores_prior_package_scalar_after_block() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            $FixregLoc01 = 7;
            { local $FixregLoc01 = 100; }
            $FixregLoc01"#
        ),
        7
    );
}

#[test]
fn substitution_eval_flag_re_evaluates_replacement_expression() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "2";
            $s =~ s/(\d)/$1 + 7/e;
            $s"#
        ),
        "9"
    );
}

#[test]
fn substitution_stacked_eval_ee_second_pass_evals_interpolated_string() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $x = "a2";
            $x =~ s/(\d)/q{$1+1}/ee;
            $x"#
        ),
        "a3"
    );
}

#[test]
fn substitution_triple_eval_eeg_parity_194() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $x = "a2";
            $x =~ s/(\d)/$1+1/eeg;
            $x"#
        ),
        "a3"
    );
}

#[test]
fn reverse_list_assigns_back_into_named_array() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @r = (1, 2, 3);
            @r = rev @r;
            join "", @r"#
        ),
        "321"
    );
}

#[test]
fn our_declares_package_alias_readable_in_expression() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            our $FixregOur01 = 55;
            $FixregOur01"#
        ),
        55
    );
}

#[test]
fn map_block_replacement_rebuilds_array_from_prior_contents() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @m = (2, 3, 4);
            @m = map { $_ * 3 } @m;
            join "", @m"#
        ),
        "6912"
    );
}

#[test]
fn uc_on_bare_number_yields_decimal_string() {
    assert_eq!(eval_string(r#"sprintf "%s", uc(8)"#), "8");
}

#[test]
fn int_of_log_exp_roundtrips_small_integer() {
    assert_eq!(eval_int(r#"int(log(exp(3)))"#), 3);
}

#[test]
fn grep_logical_not_selects_only_falsy_topics() {
    assert_eq!(eval_string(r#"join ",", grep { !$_ } (0, 1, 2)"#), "0");
}

// ── `use constant` float, `\\A\\z`, CSV split, dot→underscore `s///g`, `$#` with holes, trig via `atan2`, `**`, `return`, `exists` ──

#[test]
fn use_constant_float_participates_in_arithmetic() {
    assert_eq!(
        eval_string(
            r#"use constant FIXREG_PI_FRAC => 3.14;
            sprintf "%.2f", FIXREG_PI_FRAC * 2"#
        ),
        "6.28"
    );
}

#[test]
fn regex_az_pair_matches_only_full_string() {
    assert_eq!(eval_int(r#"my $s = "abc"; ($s =~ /\Aabc\z/) ? 1 : 0"#), 1);
}

#[test]
fn split_comma_pattern_consumes_surrounding_whitespace() {
    assert_eq!(
        eval_string(r#"join "-", split /\s*,\s*/, "a, b ,c""#),
        "a-b-c"
    );
}

#[test]
fn substitution_global_replaces_literal_dots_with_underscores() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $p = "A.B.C";
            $p =~ s/\./_/g;
            $p"#
        ),
        "A_B_C"
    );
}

#[test]
fn dollar_hash_reports_last_index_with_leading_gap() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @g = (1);
            $g[3] = 9;
            $#g"#
        ),
        3
    );
}

#[test]
fn cos_pi_is_negative_one_via_scaled_atan2() {
    assert_eq!(eval_int(r#"int(cos(atan2(0, -1)) * 1000)"#), -1000);
}

#[test]
fn sin_negative_pi_over_two_is_negative_one_via_scaled_atan2() {
    assert_eq!(eval_int(r#"int(sin(atan2(-1, 0)) * 1000)"#), -1000);
}

#[test]
fn exponentiation_negative_integer_base_to_odd_power_is_negative() {
    assert_eq!(eval_int(r#"(-2) ** 3"#), -8);
}

#[test]
fn subroutine_bare_return_yields_undef_in_scalar_context() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            fn fixreg_empty_ret { return; }
            defined(fixreg_empty_ret()) ? 1 : 0"#
        ),
        0
    );
}

#[test]
fn exists_index_zero_is_false_on_unused_array() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @z;
            exists $z[0] ? 1 : 0"#
        ),
        0
    );
}

// ── `join`+`..`, hash slice clear, `sprintf`, `splice` `<0`, postfix `while`, `\\Q`, `//=` chain, `pack`, `!!` ──

#[test]
fn join_flattens_integer_inclusive_range() {
    assert_eq!(eval_string(r#"join "", 0..3"#), "0123");
}

#[test]
fn hash_slice_assign_empty_list_clears_multiple_keys() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %h = (a => 1, b => 2, c => 3);
            @h{qw(a b)} = ();
            (!defined($h{a}) && !defined($h{b}) && $h{c} == 3) ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn sprintf_four_fraction_digits_on_one_third() {
    assert_eq!(eval_string(r#"sprintf "%.4f", 1 / 3"#), "0.3333");
}

#[test]
fn sprintf_percent_i_truncates_float_toward_zero() {
    assert_eq!(eval_string(r#"sprintf "%i", -3.7"#), "-3");
}

#[test]
fn splice_negative_offset_removes_element_before_tail() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @s = (1, 2, 3, 4, 5);
            splice @s, -2, 1;
            join "", @s"#
        ),
        "1235"
    );
}

#[test]
fn postfix_while_repeats_statement_until_condition_fails() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $i = 0;
            my $out = "";
            $out .= $i while ++$i < 4;
            $out"#
        ),
        "123"
    );
}

#[test]
fn regex_quoted_span_treats_stars_as_literals() {
    assert_eq!(eval_int(r#"("a.*b" =~ /a\Q.*\Eb/) ? 1 : 0"#), 1);
}

#[test]
fn defined_or_assign_rhs_visible_in_chained_lexical_assignment() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my ($u, $v);
            $u = $v //= 11;
            join ":", $u, $v"#
        ),
        "11:11"
    );
}

#[test]
fn pack_c_zero_round_trips_through_hex_unpack() {
    assert_eq!(eval_string(r#"unpack("H2", pack("C", 0))"#), "00");
}

#[test]
fn double_logical_not_maps_truth_to_one_and_falsity_to_zero() {
    assert_eq!(eval_int(r#"!!0 + !!7"#), 1);
}

// ── hash `//=`, `index`+pos, bitwise `&|`, postfix `for`, `shift`/`pop`, `substr(-1)`, `map`+`chr`, `int 1e2`, `sort`, `||` chain, `lc` ──

#[test]
fn defined_or_assign_on_missing_hash_slot_inserts_value() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %h = (fixreg_hk1 => 1);
            $h{fixreg_hk2} //= 42;
            $h{fixreg_hk2}"#
        ),
        42
    );
}

#[test]
fn index_with_start_finds_second_occurrence_in_banana() {
    assert_eq!(eval_int(r#"index("banana", "na", 3)"#), 4);
}

#[test]
fn bitwise_and_binds_tighter_than_or() {
    assert_eq!(eval_int(r#"(4 & 6) | 1"#), 5);
}

#[test]
fn postfix_for_statement_increments_once_per_range_element() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $c = 0;
            $c++ for 1..3;
            $c"#
        ),
        3
    );
}

#[test]
fn shift_removes_leading_array_element_and_shortens() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @q = (7, 8, 9);
            shift @q;
            scalar @q"#
        ),
        2
    );
}

#[test]
fn pop_drops_trailing_element_leaving_prior_joined() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @q = (7, 8, 9);
            pop @q;
            join "", @q"#
        ),
        "78"
    );
}

#[test]
fn substr_negative_one_returns_final_character() {
    assert_eq!(eval_string(r#"substr("fix", -1)"#), "x");
}

#[test]
fn map_chr_builds_abc_from_successive_ord_offsets() {
    assert_eq!(eval_string(r#"join "", map { chr(64 + $_) } 1..3"#), "ABC");
}

#[test]
fn int_truncates_scientific_notation_integer_string() {
    assert_eq!(eval_int(r#"int("1e2")"#), 100);
}

#[test]
fn sort_default_lexical_orders_single_digit_strings() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @d = (9, 8, 7);
            @d = sort @d;
            join "", @d"#
        ),
        "789"
    );
}

#[test]
fn logical_or_chain_returns_first_truthy_after_falsy_run() {
    assert_eq!(eval_int(r#"0 || "" || 13"#), 13);
}

#[test]
fn lc_on_numeric_coerces_to_decimal_string() {
    assert_eq!(eval_string(r#"sprintf "%s", lc(6)"#), "6");
}

// ── `s/^/`/`s/$/`, comma+`()`, sparse length, `--`, `(a**b)**c`, `.` chain, `reverse`.., `ord ""`, slice assign, `sprintf %o`0, `hex ""` ──

#[test]
fn substitution_inserts_prefix_before_start_of_string() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "abc";
            $s =~ s/^/X/;
            $s"#
        ),
        "Xabc"
    );
}

#[test]
fn substitution_inserts_suffix_after_end_of_string() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "abc";
            $s =~ s/$/Z/;
            $s"#
        ),
        "abcZ"
    );
}

#[test]
fn comma_list_flattens_empty_parenthesized_segment() {
    assert_eq!(eval_string(r#"join "", ("z", ())"#), "z");
}

#[test]
fn scalar_array_counts_through_highest_assigned_index() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @w = (1);
            $w[10] = 0;
            scalar @w"#
        ),
        11
    );
}

#[test]
fn double_unary_minus_negates_negative_integer() {
    assert_eq!(eval_int(r#"-(-9)"#), 9);
}

#[test]
fn grouped_exponentiation_applies_outer_power_after_inner() {
    assert_eq!(eval_int(r#"(2 ** 3) ** 2"#), 64);
}

#[test]
fn dot_concatenation_interleaves_scalar_string_and_number() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $x = 4;
            $x = $x . "5" . 6;
            $x"#
        ),
        "456"
    );
}

#[test]
fn reverse_range_list_orders_descending_when_two_elements() {
    assert_eq!(eval_string(r#"join "", rev 1..2"#), "21");
}

#[test]
fn ord_empty_string_yields_zero() {
    assert_eq!(eval_int(r#"ord("")"#), 0);
}

#[test]
fn slice_assign_after_pair_extends_array_with_two_new_slots() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @v = (1, 2);
            @v[2, 3] = (9, 8);
            join "", @v"#
        ),
        "1298"
    );
}

#[test]
fn sprintf_octal_of_zero_is_single_digit() {
    assert_eq!(eval_string(r#"sprintf "%o", 0"#), "0");
}

#[test]
fn hex_empty_string_is_zero() {
    assert_eq!(eval_int(r#"hex("")"#), 0);
}

// ── `oct`/`int` `""`, `undef` `eq`, `splice`, `cmp`, `map` `**2`, `<` chain, `chomp` CRLF, `qq[]`, `rindex` overlap ──

#[test]
fn oct_empty_string_coerces_to_zero() {
    assert_eq!(eval_int(r#"oct("")"#), 0);
}

#[test]
fn int_empty_string_truncates_to_zero() {
    assert_eq!(eval_int(r#"int("")"#), 0);
}

#[test]
fn undef_lexical_is_string_equal_to_empty() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $u;
            ($u eq "") ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn undef_eq_undef_in_scalar_comparison() {
    assert_eq!(eval_int(r#"(undef eq undef) ? 1 : 0"#), 1);
}

#[test]
fn splice_zero_length_at_start_prepends_on_named_array() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @t = (1, 2, 3);
            splice @t, 0, 0, 9;
            join "", @t"#
        ),
        "9123"
    );
}

#[test]
fn scalar_splice_removing_two_returns_last_removed_value() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @t = (1, 2, 3, 4);
            scalar splice @t, 1, 2"#
        ),
        3
    );
}

#[test]
fn cmp_places_undef_before_ascii_string() {
    assert_eq!(eval_int(r#"(undef cmp "m")"#), -1);
}

#[test]
fn map_squares_each_integer_in_range() {
    assert_eq!(eval_string(r#"join "", map { $_ * $_ } 1..3"#), "149");
}

#[test]
fn chained_less_than_raku_style() {
    // Raku-style chained comparison: `1 < 2 < 3` → `(1 < 2) && (2 < 3)` → true
    assert_eq!(eval_int(r#"(1 < 2 < 3) ? 1 : 0"#), 1);
}

#[test]
fn chomp_crlf_pair_leaves_two_code_units() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $s = "a\r\n";
            chomp $s;
            length $s"#
        ),
        2
    );
}

#[test]
fn qq_brackets_allow_unescaped_square_brackets() {
    assert_eq!(eval_string(r#"qq[a[b]]"#), "a[b]");
}

#[test]
fn rindex_finds_rightmost_overlapping_needle() {
    assert_eq!(eval_int(r#"rindex("aaaa", "aa")"#), 2);
}

// ── `push`/empty, `..` slice (interior), `sprintf` round, `undef`+, `.=`/`x`, `and`/`or`, `s///`, `%b`0, `join`/"" ──

#[test]
fn push_with_empty_list_argument_is_noop() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @p = (9);
            push @p, ();
            scalar @p"#
        ),
        1
    );
}

#[test]
fn array_slice_range_assign_replaces_interior_pair() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @t = (1, 2, 3);
            @t[1 .. 2] = (7, 6);
            join "", @t"#
        ),
        "176"
    );
}

#[test]
fn sprintf_zero_fraction_rounds_toward_nearest_integer_up() {
    assert_eq!(eval_string(r#"sprintf "%.0f", 2.7"#), "3");
}

#[test]
fn sprintf_zero_fraction_rounds_half_up_for_three_point_five() {
    assert_eq!(eval_string(r#"sprintf "%.0f", 3.5"#), "4");
}

#[test]
fn undef_plus_zero_coerces_to_numeric_zero() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $u;
            $u + 0"#
        ),
        0
    );
}

#[test]
fn dot_equal_appends_repeated_pattern_from_string_repeat() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "x";
            $s .= "y" x 2;
            $s"#
        ),
        "xyy"
    );
}

#[test]
fn low_precedence_and_chain_returns_final_truthy_operand() {
    assert_eq!(eval_int(r#"1 and 2 and 3"#), 3);
}

#[test]
fn low_precedence_or_chain_returns_first_truthy_operand() {
    assert_eq!(eval_int(r#"0 or 2 or 3"#), 2);
}

#[test]
fn substitution_without_g_removes_first_match_only() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "abc";
            $s =~ s/b//;
            $s"#
        ),
        "ac"
    );
}

#[test]
fn substitution_with_g_removes_every_occurrence_of_char() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "aba";
            $s =~ s/a//g;
            $s"#
        ),
        "b"
    );
}

#[test]
fn sprintf_binary_of_zero_is_single_digit() {
    assert_eq!(eval_string(r#"sprintf "%b", 0"#), "0");
}

#[test]
fn join_skips_leading_empty_string_field() {
    assert_eq!(eval_string(r#"join "", ("", "q")"#), "q");
}

// ── `sprintf` flags, hash slice `=`, assign-in-expr, list `[-1]`, `m//g`, `split \\n`, list assign tail, `pack x`, `q{}`, slice scalar ──

#[test]
fn sprintf_minus_left_aligns_string_in_fixed_field() {
    assert_eq!(eval_string(r#"sprintf "%-5s", "ab""#), "ab   ");
}

#[test]
fn hash_slice_list_assign_rewrites_single_key() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %h = (fixreg_hs_x => 1);
            @h{qw(fixreg_hs_x)} = (44);
            $h{fixreg_hs_x}"#
        ),
        44
    );
}

#[test]
fn parenthesized_assignment_used_as_numeric_operand() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $v;
            ($v = 3) * 4"#
        ),
        12
    );
}

#[test]
fn list_literal_negative_index_selects_trailing_element() {
    assert_eq!(eval_int(r#"(1, 2, 3)[-1]"#), 3);
}

#[test]
fn regex_match_global_in_scalar_numeric_context_is_one_when_matched() {
    assert_eq!(eval_int(r#"0 + ("tone" =~ /e/g)"#), 1);
    assert_eq!(eval_int(r#"0 + ("tone" =~ /x/g)"#), 0);
}

#[test]
fn sprintf_plus_includes_sign_on_negative_float() {
    assert_eq!(eval_string(r#"sprintf "%+f", -2.5"#), "-2.500000");
}

#[test]
fn split_newline_with_limit_keeps_remainder_in_last_field() {
    assert_eq!(
        eval_string(r#"join "|", split /\n/, "p\nq\nr", 2"#),
        "p|q\nr"
    );
}

#[test]
fn list_assign_to_two_lexicals_drops_extra_rhs_values() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my ($p, $q) = (1, 2, 3);
            join "", $p, $q"#
        ),
        "12"
    );
}

#[test]
fn pack_x_inserts_nul_bytes_before_following_literal() {
    assert_eq!(
        eval_string(r#"unpack("H*", pack("x2") . "ab")"#),
        "00006162"
    );
}

#[test]
fn numeric_coercion_strips_leading_whitespace_from_digits() {
    assert_eq!(eval_int(r#"0 + "\t5""#), 5);
}

#[test]
fn empty_single_quoted_literal_eq_empty_string() {
    assert_eq!(eval_int(r#"(q{} eq "") ? 1 : 0"#), 1);
}

#[test]
fn array_subscript_slice_in_scalar_context_yields_last_indexed_element() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @z = (1, 2, 3);
            $z[1, 2, 0]"#
        ),
        1
    );
}

#[test]
fn sprintf_zero_pad_preserves_minus_sign_on_negative() {
    assert_eq!(eval_string(r#"sprintf "%03d", -1"#), "-01");
}

// ── List/array slices, `delete`, `splice`, `sort values`, `cmp`, `chop`/`substr`, `reverse`, `rindex`, `/g`+`join`, `grep`, `||=` ──

#[test]
fn list_literal_non_contiguous_slice_joins_selected_elements() {
    assert_eq!(eval_string(r#"join "", (1, 2, 3)[0, 2]"#), "13");
}

#[test]
fn array_copy_from_slice_indices_preserves_gaps_in_list() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @i = (1, 2, 3);
            my @j = @i[0, 2];
            join "", @j"#
        ),
        "13"
    );
}

#[test]
fn delete_middle_element_does_not_shrink_scalar_array_length() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @g = (1, 2, 3);
            delete $g[1];
            scalar @g"#
        ),
        3
    );
}

#[test]
fn splice_replaces_two_elements_with_singleton_list() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @f = (1, 2, 3, 4);
            my $n = scalar splice @f, 1, 2, (9);
            join "", $n, ":", @f"#
        ),
        "3:194"
    );
}

#[test]
fn sort_values_joins_hash_values_lexicographically() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my %h = (fr_sv_a => 3, fr_sv_b => 4);
            join "", sort values %h"#
        ),
        "34"
    );
}

#[test]
fn string_cmp_returns_minus_one_before_and_zero_when_equal() {
    assert_eq!(eval_int(r#""abc" cmp "abd""#), -1);
    assert_eq!(eval_int(r#""abc" cmp "abc""#), 0);
}

#[test]
fn postincrement_value_concatenated_before_increment_visible() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $m = 2;
            $m++ . $m"#
        ),
        "23"
    );
}

#[test]
fn chop_removes_last_byte_when_no_trailing_newline() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "abcd";
            chop $s;
            $s"#
        ),
        "abc"
    );
}

#[test]
fn substr_length_two_reads_middle_pair() {
    assert_eq!(eval_string(r#"substr "abcdef", 2, 2"#), "cd");
}

#[test]
fn sort_qw_joins_lexicographic_order() {
    assert_eq!(eval_string(r#"join "", sort qw(z a m)"#), "amz");
}

#[test]
fn unshift_inserts_list_before_existing_head() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @h = (1);
            unshift @h, (2, 3);
            join "", @h"#
        ),
        "231"
    );
}

#[test]
fn scalar_array_subscript_slice_is_last_slice_element() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @c = (10, 20, 30);
            $c[1, 2]"#
        ),
        30
    );
}

#[test]
fn sprintf_one_fraction_digit_truncates_toward_zero_on_quarter() {
    assert_eq!(eval_string(r#"sprintf "%.1f", 2.25"#), "2.2");
}

#[test]
fn map_plus_one_topic_joined_with_hyphen() {
    assert_eq!(eval_string(r#"join "-", map { $_ + 1 } (1, 2)"#), "2-3");
}

#[test]
fn xor_eq_from_one_bit_pattern() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $t = 1;
            $t ^= 3;
            $t"#
        ),
        2
    );
}

#[test]
fn sprintf_unsigned_formats_negative_as_two_complement() {
    assert_eq!(eval_string(r#"sprintf "%u", -1"#), "18446744073709551615");
}

#[test]
fn last_index_special_minus_one_on_empty_array() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @b = ();
            $#b"#
        ),
        -1
    );
}

#[test]
fn reverse_list_literal_joins_inverted_order() {
    assert_eq!(eval_string(r#"join "", rev (1, 2, 3)"#), "321");
}

#[test]
fn reverse_assign_back_mutates_named_array() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @m = (1, 2, 3);
            @m = rev @m;
            join "", @m"#
        ),
        "321"
    );
}

#[test]
fn chr_ord_round_trips_ascii_uppercase_a() {
    assert_eq!(eval_string(r#"chr ord "A""#), "A");
}

#[test]
fn rindex_finds_latest_offset_for_overlapping_substring() {
    assert_eq!(eval_int(r#"rindex "aaa", "aa""#), 1);
}

#[test]
fn join_split_regex_global_matches_repeated_char() {
    assert_eq!(eval_string(r#"join "-", ("aaa" =~ /a/g)"#), "a-a-a");
}

#[test]
fn join_dot_regex_global_yields_all_characters() {
    assert_eq!(eval_string(r#"join "", ("abc" =~ /./g)"#), "abc");
}

#[test]
fn scalar_keys_on_cleared_hash_counts_zero() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %h = ();
            scalar keys %h"#
        ),
        0
    );
}

#[test]
fn logical_or_assign_replaces_empty_string() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $b = "";
            $b ||= 8;
            $b"#
        ),
        8
    );
}

#[test]
fn grep_numeric_gt_zero_joins_truthy_topics() {
    assert_eq!(
        eval_string(r#"join "", grep { $_ > 0 } (-1, 2, 0, 3)"#),
        "23"
    );
}

// ── `<=>`, `index`/`rindex`, `sprintf` half, `push`, `delete`, `grep defined`, `//`, `splice`, `split //`, `unpack`, `bless`, `$#`, `atan2`, assigns, slices, `!!`, `x`, `ord`/`chr`, `map uc`, coderefs ──

#[test]
fn spaceship_orders_greater_equal_and_less() {
    assert_eq!(eval_int(r#"(5 <=> 3)"#), 1);
    assert_eq!(eval_int(r#"(3 <=> 3)"#), 0);
}

#[test]
fn index_and_rindex_find_na_in_banana_without_extra_offset() {
    assert_eq!(eval_int(r#"index("banana", "na")"#), 2);
    assert_eq!(eval_int(r#"rindex("banana", "na")"#), 4);
}

#[test]
fn sprintf_rounds_half_to_nearest_even_integer() {
    assert_eq!(eval_string(r#"sprintf "%.0f", 0.5"#), "0");
    assert_eq!(eval_string(r#"sprintf "%.0f", 1.5"#), "2");
}

#[test]
fn push_flattens_rhs_array_onto_tail() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2);
            push @a, @a;
            join "", @a"#
        ),
        "1212"
    );
}

#[test]
fn delete_hash_key_drops_exists() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %h = (fr_del_k => 1);
            delete $h{fr_del_k};
            exists $h{fr_del_k} ? 1 : 0"#
        ),
        0
    );
}

#[test]
fn grep_defined_skips_undef_list_elements() {
    assert_eq!(
        eval_string(r#"join "", grep { defined $_ } (1, undef, 2)"#),
        "12"
    );
}

#[test]
fn defined_or_falls_back_only_for_undef_not_zero() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $u;
            my $z = 0;
            join "", ($u // "m"), ($z // "n")"#
        ),
        "m0"
    );
}

#[test]
fn splice_with_negative_offset_removes_last_element() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @y = (1, 2, 3);
            my $t = splice @y, -1, 1;
            join ":", $t, (join "", @y)"#
        ),
        "3:12"
    );
}

#[test]
fn splice_at_zero_removes_head_element() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @x = (1, 2, 3);
            my $h = splice @x, 0, 1;
            join ":", $h, (join "", @x)"#
        ),
        "1:23"
    );
}

#[test]
fn split_slash_slash_with_limit_two_yields_single_field() {
    assert_eq!(eval_string(r#"join "", split //, "ab", 2"#), "ab");
}

#[test]
fn unpack_hex_from_raw_one_byte_string() {
    assert_eq!(eval_string(r#"unpack("H2", "A")"#), "41");
}

#[test]
fn ref_blessed_anon_hash_reports_package() {
    assert_eq!(eval_string(r#"ref bless {}, "FrBlessPkg""#), "FrBlessPkg");
}

#[test]
fn array_last_element_via_max_index_subscript() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            $a[$#a]"#
        ),
        3
    );
}

#[test]
fn max_index_of_singleton_list_is_zero() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a = (9);
            $#a"#
        ),
        0
    );
}

#[test]
fn atan2_one_one_sprintf_matches_fixed_decimal() {
    assert_eq!(
        eval_string(r#"sprintf "%.10f", atan2 1, 1"#),
        "0.7853981634"
    );
}

#[test]
fn divide_eq_leaves_float_on_int_rhs() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $w = 7;
            $w /= 2;
            "$w""#
        ),
        "3.5"
    );
}

#[test]
fn modulo_eq_reduces_modulus() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $v = 10;
            $v %= 3;
            $v"#
        ),
        1
    );
}

#[test]
fn slice_assign_short_rhs_leaves_trailing_elements() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (4, 5, 6);
            @a[0, 1] = (9);
            join "", @a"#
        ),
        "96"
    );
}

#[test]
fn range_slice_assign_replaces_interior_run() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @b = (1, 2, 3, 4);
            @b[1 .. 2] = (7, 8);
            join "", @b"#
        ),
        "1784"
    );
}

#[test]
fn double_bang_numeric_coercion_matches_truthiness() {
    assert_eq!(eval_int(r#"0 + (!!0)"#), 0);
    assert_eq!(eval_int(r#"0 + (!!"")"#), 0);
    assert_eq!(eval_int(r#"0 + (!!undef)"#), 0);
    assert_eq!(eval_int(r#"0 + (!!"x")"#), 1);
}

#[test]
fn repeat_scalar_duplicates_stringified_number() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $n = 3;
            $n x 2"#
        ),
        "33"
    );
}

#[test]
fn ord_line_feed_is_ten() {
    assert_eq!(eval_int(r#"ord "\n""#), 10);
}

#[test]
fn chr_ten_equals_line_feed_escape() {
    assert_eq!(eval_int(r#"(chr 10 eq "\n") ? 1 : 0"#), 1);
}

#[test]
fn map_uc_block_over_qw_list() {
    assert_eq!(
        eval_string(r#"join "", map { uc $_ } qw(fr_a fr_b)"#),
        "FR_AFR_B"
    );
}

#[test]
fn coderef_call_ampersand_and_arrow() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $f = fn { return 3; };
            my $g = fn { 5 };
            join "", &$f(), $g->()"#
        ),
        "35"
    );
}

// ── `join`+`undef`, `log`/`exp`, `splice` noop-insert, bit assigns, `unpack`, keys, `**` parens, unary ops, list assign, `eval`, `abs`, `lt`, slice, `sqrt`, `map`, `split -1`, anon hash ──

#[test]
fn join_stringifies_undef_as_empty_between_defined_elems() {
    assert_eq!(eval_string(r#"join "", (undef, 1, 2)"#), "12");
}

#[test]
fn int_log_exp_round_trips_small_integer() {
    assert_eq!(eval_int(r#"int(log(exp(2)))"#), 2);
}

#[test]
fn int_exp_log_round_trips_small_integer() {
    assert_eq!(eval_int(r#"int(exp(log(2)))"#), 2);
}

#[test]
fn splice_remove_one_without_insert_returns_removed_in_scalar_context() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            my $r = scalar splice @a, 1, 1, ();
            join ":", $r, (join "", @a)"#
        ),
        "2:13"
    );
}

#[test]
fn and_eq_masks_bits_in_place() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $u = 1;
            $u &= 3;
            $u"#
        ),
        1
    );
}

#[test]
fn or_eq_combines_bits_in_place() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $v = 1;
            $v |= 2;
            $v"#
        ),
        3
    );
}

#[test]
fn quotemeta_escapes_dot_outside_char_class() {
    assert_eq!(eval_string(r#"quotemeta "a.b""#), r"a\.b");
}

#[test]
fn unpack_v_reads_unsigned_short_big_endian() {
    assert_eq!(eval_int(r#"unpack("v", "AB")"#), 16961);
}

#[test]
fn unpack_c_star_lists_byte_values() {
    assert_eq!(eval_string(r#"join "", unpack("C*", "AB")"#), "6566");
}

#[test]
fn sort_keys_joins_lexicographically() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my %h = (fr3_zz => 1, fr3_aa => 2);
            join "", sort keys %h"#
        ),
        "fr3_aafr3_zz"
    );
}

#[test]
fn exponentiation_parentheses_force_inner_power_first() {
    assert_eq!(eval_int(r#"((2 ** 3) ** 2)"#), 64);
}

#[test]
fn unary_minus_negates_negative_to_positive() {
    assert_eq!(eval_int(r#"-(-5)"#), 5);
}

#[test]
fn unary_plus_is_numeric_noop() {
    assert_eq!(eval_int(r#"+5"#), 5);
}

#[test]
fn scalar_assign_from_paren_list_keeps_last_element() {
    // In stryke mode, `$z = (1,2,3)` is a syntax error — use `$z = (list)[-1]` explicitly.
    // Test that the compat mode (Perl 5) preserves the "last element" behavior.
    stryke::set_compat_mode(true);
    let result = eval_int(
        r#"no strict 'vars';
            my $z;
            $z = (1, 2, 3);
            $z"#,
    );
    stryke::set_compat_mode(false);
    assert_eq!(result, 3);
}

#[test]
fn grep_modulo_two_filters_evens_from_numeric_range() {
    assert_eq!(
        eval_string(r#"join "", grep { $_ % 2 == 0 } (1 .. 6)"#),
        "246"
    );
}

#[test]
fn repeat_paren_string_duplicates_four_times() {
    assert_eq!(
        eval_string(r#"join "", ("fr3_x") x 4"#),
        "fr3_xfr3_xfr3_xfr3_x"
    );
}

#[test]
fn push_appends_two_scalar_arguments() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @m = (1, 2);
            push @m, 3, 4;
            join "", @m"#
        ),
        "1234"
    );
}

#[test]
fn string_eval_adds_two_constants() {
    assert_eq!(eval_int(r#"eval "2+2""#), 4);
}

#[test]
fn abs_integer_magnitude() {
    assert_eq!(eval_int(r#"abs -9"#), 9);
}

#[test]
fn abs_coerces_numeric_string() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = abs "-3.5";
            "$s""#
        ),
        "3.5"
    );
}

#[test]
fn string_lt_compares_first_codepoint_not_numeric_value() {
    assert_eq!(eval_int(r#"0 + ("3" lt "12")"#), 0);
    assert_eq!(eval_int(r#"0 + ("12" lt "3")"#), 1);
}

#[test]
fn scalar_subscript_slice_with_repeated_index_yields_last_list_slot() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a = (5, 6, 7);
            $a[1, 2, 1]"#
        ),
        6
    );
}

#[test]
fn sqrt_reads_radicand_from_scalar() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $x = 9;
            sqrt $x"#
        ),
        3
    );
}

#[test]
fn map_plus_zero_numericizes_zero_padded_strings() {
    assert_eq!(eval_string(r#"join "", map { $_ + 0 } ("03", "04")"#), "34");
}

#[test]
fn split_limit_negative_one_retains_trailing_empty_fields() {
    assert_eq!(
        eval_string(r#"join "|", split /:/, "a:b::c", -1"#),
        "a|b||c"
    );
}

#[test]
fn anon_hash_arrow_reads_slot() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $r = { fr3_slot => 42 };
            $r->{fr3_slot}"#
        ),
        42
    );
}

// ── Range join/`map`, `sprintf` width, `scalar grep`/`map`, `do`, `sort`, `s///g`, `pack a2`, assigns, `exists`, `splice`, truthiness, `bless` ──

#[test]
fn join_empty_string_interpolates_numeric_range_without_delimiter() {
    assert_eq!(eval_string(r#"join "", 10 .. 12"#), "101112");
}

#[test]
fn map_increment_over_one_to_three_range() {
    assert_eq!(eval_string(r#"join "", map { $_ + 1 } 1 .. 3"#), "234");
}

#[test]
fn sprintf_percent_dot_truncates_string_to_two_chars() {
    assert_eq!(eval_string(r#"sprintf "%.2s", "abcd""#), "ab");
}

#[test]
fn sprintf_space_pads_short_string_to_two_columns() {
    assert_eq!(eval_string(r#"sprintf "%2s", "x""#), " x");
}

#[test]
fn sprintf_space_pads_integer_to_two_columns() {
    assert_eq!(eval_string(r#"sprintf "%2d", 7"#), " 7");
}

#[test]
fn scalar_grep_counts_matches_on_named_array() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            scalar grep { $_ > 1 } @a"#
        ),
        2
    );
}

#[test]
fn scalar_map_counts_elements_on_named_array() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            scalar map { $_ * 2 } @a"#
        ),
        3
    );
}

#[test]
fn do_block_scalar_assign_is_last_statement() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $x;
            $x = do { 1; 2; 3 };
            $x"#
        ),
        3
    );
}

#[test]
fn sort_rebinds_named_array_to_sorted_order() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @b = (9, 8);
            @b = sort @b;
            join "", @b"#
        ),
        "89"
    );
}

#[test]
fn reverse_sort_inverts_lexical_ordering() {
    assert_eq!(eval_string(r#"join "", rev sort qw(p o m)"#), "pom");
}

#[test]
fn substitution_global_replaces_each_match() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "aba";
            $s =~ s/a/x/g;
            $s"#
        ),
        "xbx"
    );
}

#[test]
fn substitution_global_count_in_scalar_context() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $t = "aba";
            scalar ($t =~ s/a/x/g)"#
        ),
        2
    );
}

#[test]
fn map_ord_joins_code_units_from_char_list() {
    assert_eq!(eval_string(r#"join "", map ord, ("A", "B")"#), "6566");
}

#[test]
fn pack_unpack_a2_round_trips_ascii_pair() {
    assert_eq!(
        eval_string(
            r#"my $p = pack("a2", "XY");
            unpack("a2", $p)"#
        ),
        "XY"
    );
}

#[test]
fn and_assign_preserves_rhs_when_lhs_true() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $f = 1;
            $f &&= 2;
            $f"#
        ),
        2
    );
}

#[test]
fn plus_assign_coerces_quoted_numeral() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $k = "5";
            $k += 2;
            $k"#
        ),
        7
    );
}

#[test]
fn plus_assign_strips_whitespace_from_numeric_string() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $m = " 8 ";
            $m += 0;
            $m"#
        ),
        8
    );
}

#[test]
fn exists_false_on_array_index_beyond_current_tail() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @h = (1, 2, 3);
            exists $h[5] ? 1 : 0"#
        ),
        0
    );
}

#[test]
fn exists_true_after_assign_past_end_of_array() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @h = (1, 2, 3);
            $h[5] = 0;
            exists $h[5] ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn map_lc_lowercases_qw_topics() {
    assert_eq!(eval_string(r#"join "", map { lc $_ } qw(X Y)"#), "xy");
}

#[test]
fn splice_removes_tail_pair_and_returns_it() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3, 4);
            my @r = splice @a, 2, 2;
            join "", @r, ":", join "", @a"#
        ),
        "34:12"
    );
}

#[test]
fn multiply_eq_scales_scalar() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $v = 6;
            $v *= 2;
            $v"#
        ),
        12
    );
}

#[test]
fn subtract_eq_reduces_scalar() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $w = 7;
            $w -= 3;
            $w"#
        ),
        4
    );
}

#[test]
fn int_truncates_positive_and_negative_toward_zero() {
    assert_eq!(eval_int(r#"int(2.9) + int(-2.9)"#), 0);
}

#[test]
fn dot_equal_appends_to_lexical_string() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "hi";
            $s .= " there";
            $s"#
        ),
        "hi there"
    );
}

#[test]
fn comma_list_flattens_adjacent_arrays() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @x = (1, 2);
            my @y = (3, 4);
            join "", (@x, @y)"#
        ),
        "1234"
    );
}

#[test]
fn defined_or_falls_back_for_undef_in_addition() {
    assert_eq!(eval_int(r#"0 + (undef // 1)"#), 1);
}

#[test]
fn defined_or_assign_sets_only_when_lexical_undef() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $p = 7;
            my $q;
            $p //= 3;
            $q //= 8;
            $p + $q"#
        ),
        15
    );
}

#[test]
fn map_identity_preserves_comma_list_order() {
    assert_eq!(eval_string(r#"join "", map { $_ } 1, 2, 3"#), "123");
}

#[test]
fn ref_bless_anon_array_reports_package() {
    assert_eq!(eval_string(r#"ref bless [], "Fr4ArrPkg""#), "Fr4ArrPkg");
}

#[test]
fn string_truthiness_distinguishes_zero_exponent_from_empty() {
    assert_eq!(
        eval_int(r#"(("" ? 1 : 0) + ("0" ? 1 : 0) + ("0E0" ? 1 : 0))"#),
        1
    );
}

// ── `scalar @{[]}`, hash `keys`/`values`, `ref`, `aref`, regex/split, `sprintf`, `splice`, `all`, `shift`/`pop`, `atan2`, `for`, `pack`/`unpack` `H*` ──

#[test]
fn scalar_deref_anonymous_array_brackets_counts_elements() {
    assert_eq!(eval_int(r#"scalar @{[7, 8, 9]}"#), 3);
}

#[test]
fn sort_joins_keys_and_values_of_named_hash() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my %h = (fr6_u2 => 20, fr6_u1 => 10);
            join "", (join "", sort keys %h), ":", (join "", sort values %h)"#
        ),
        "fr6_u1fr6_u2:1020"
    );
}

#[test]
fn join_hyphen_interpolates_qw_topics() {
    assert_eq!(
        eval_string(r#"join "-", qw(fr6_p fr6_q fr6_r)"#),
        "fr6_p-fr6_q-fr6_r"
    );
}

#[test]
fn max_index_three_element_array_is_two() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @t = (1, 2, 3);
            $#t"#
        ),
        2
    );
}

#[test]
fn ref_bracket_constructor_yields_array() {
    assert_eq!(eval_string(r#"ref [1, 2]"#), "ARRAY");
}

#[test]
fn ref_brace_constructor_yields_hash() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            ref { fr6_hk => 1 }"#
        ),
        "HASH"
    );
}

#[test]
fn array_ref_arrow_subscript_reads_element() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $r = [10, 20, 30];
            $r->[1]"#
        ),
        20
    );
}

#[test]
fn join_dot_regex_global_splits_into_characters() {
    assert_eq!(eval_string(r#"join "", ("fr6_ab" =~ /./g)"#), "fr6_ab");
}

#[test]
fn split_whitespace_pattern_skips_leading_blanks() {
    assert_eq!(
        eval_string(r#"join "", split /\s+/, "  fr6_a  fr6_b  ""#),
        "fr6_afr6_b"
    );
}

#[test]
fn sprintf_binary_and_octal_formats() {
    assert_eq!(eval_string(r#"sprintf "%b", 12"#), "1100");
    assert_eq!(eval_string(r#"sprintf "%o", 9"#), "11");
}

#[test]
fn splice_inserts_before_index_without_removal() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @c = (1, 2, 3, 4, 5);
            splice @c, 1, 0, 9;
            join "", @c"#
        ),
        "192345"
    );
}

#[test]
fn defined_or_assign_leaves_defined_positive_unchanged() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $z = 1;
            $z //= 0;
            $z"#
        ),
        1
    );
}

#[test]
fn bare_builtin_all_true_under_upper_bound() {
    assert_eq!(eval_int(r#"0 + all(fn { $_ < 10 }, 1, 2, 3)"#), 1);
}

#[test]
fn map_chr_from_codepoint_list_joins_string() {
    assert_eq!(eval_string(r#"join "", map { chr $_ } (65, 66)"#), "AB");
}

#[test]
fn shift_returns_head_and_shortens_array() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @d = (1, 2, 3);
            my $h = shift @d;
            join ":", $h, (join "", @d)"#
        ),
        "1:23"
    );
}

#[test]
fn pop_returns_tail_and_shortens_array() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @e = (1, 2, 3);
            my $t = pop @e;
            join ":", $t, (join "", @e)"#
        ),
        "3:12"
    );
}

#[test]
fn atan2_with_sqrt_matches_fixed_decimal() {
    assert_eq!(
        eval_string(r#"sprintf "%.12f", atan2(1, sqrt(3))"#),
        "0.523598775598"
    );
}

#[test]
fn statement_modifier_for_multiplies_accumulator() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $g = 1;
            $g *= $_ for 1 .. 2;
            $g"#
        ),
        2
    );
}

#[test]
fn unshift_prepends_single_scalar_element() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @f = (1, 2, 3);
            unshift @f, 0;
            join "", @f"#
        ),
        "0123"
    );
}

#[test]
fn join_empty_string_concatenates_qw_pair() {
    assert_eq!(eval_string(r#"join "", qw(fr6_j fr6_k)"#), "fr6_jfr6_k");
}

#[test]
fn repeat_operator_triples_digit_string() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $i = "2";
            $i x 3"#
        ),
        "222"
    );
}

#[test]
fn list_literal_slice_joins_tail_pair() {
    assert_eq!(eval_string(r#"join "", (1, 2, 3)[1, 2]"#), "23");
}

#[test]
fn sprintf_rounds_quarter_and_three_quarters() {
    assert_eq!(eval_string(r#"sprintf "%.0f", 2.4"#), "2");
    assert_eq!(eval_string(r#"sprintf "%.0f", 2.6"#), "3");
}

#[test]
fn bitwise_and_with_tilde_one_masks_low_bit() {
    assert_eq!(eval_int(r#"3 & ~1"#), 2);
}

#[test]
fn map_rebind_doubles_each_array_element() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            @a = map { $_ * 2 } @a;
            join "", @a"#
        ),
        "246"
    );
}

#[test]
fn array_slice_noncontiguous_indices_join() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @b = (1, 2, 3, 4);
            join "", @b[0, 3]"#
        ),
        "14"
    );
}

#[test]
fn substr_prefix_and_negative_offset_suffix() {
    assert_eq!(eval_string(r#"substr "fr6_hello", 0, 2"#), "fr");
    assert_eq!(eval_string(r#"substr "fr6_hello", -2"#), "lo");
}

#[test]
fn index_finds_multi_character_substring() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $s = "fr6_abc";
            index $s, "bc""#
        ),
        5
    );
}

#[test]
fn pack_hex_template_decodes_nibbles_to_bytes() {
    assert_eq!(eval_string(r#"pack("H*", "4142")"#), "AB");
}

#[test]
fn unpack_hex_template_encodes_bytes_to_nibbles() {
    assert_eq!(eval_string(r#"unpack("H*", "AB")"#), "4142");
}

#[test]
fn reverse_range_list_inverts_numeric_order() {
    assert_eq!(eval_string(r#"join "", rev 1 .. 3"#), "321");
}

// ── `map`/`reverse`/slice/`cmp`/`tr`/`s`, hash `values`, `push`, subscript, trig, `grep`, sparse arrays, division ──

#[test]
fn map_squares_two_topics_into_concatenated_digits() {
    assert_eq!(eval_string(r#"join "", map { $_ * $_ } 4, 5"#), "1625");
}

#[test]
fn map_identity_reverses_qw_order() {
    assert_eq!(
        eval_string(r#"join "", map { $_ } rev qw(fr8_x fr8_y fr8_z)"#),
        "fr8_zfr8_yfr8_x"
    );
}

#[test]
fn array_slice_reorders_non_contiguous_indices() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            join "", @a[2, 0, 1]"#
        ),
        "312"
    );
}

#[test]
fn spaceship_orders_one_before_two() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $x = 1;
            $x <=> 2"#
        ),
        -1
    );
}

#[test]
fn cmp_orders_double_a_before_ab() {
    assert_eq!(eval_int(r#""aa" cmp "ab""#), -1);
}

#[test]
fn string_nan_literal_equals_itself() {
    assert_eq!(eval_int(r#"0 + ("nan" eq "nan")"#), 1);
}

#[test]
fn array_range_slice_selects_interior_run() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @b = (1, 2, 3, 4);
            join "", @b[1 .. 3]"#
        ),
        "234"
    );
}

#[test]
fn transliterate_replaces_class_with_single_replacement_char() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "abc";
            $s =~ tr/a/b/;
            $s"#
        ),
        "bbc"
    );
}

#[test]
fn substitution_global_strips_digit_runs() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $t = "a1b2";
            $t =~ s/\d+//g;
            $t"#
        ),
        "ab"
    );
}

#[test]
fn substitution_global_replaces_repeated_literal_char() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "axay";
            $s =~ s/x/y/g;
            $s"#
        ),
        "ayay"
    );
}

#[test]
fn join_captures_all_single_char_matches_from_slash_g() {
    assert_eq!(eval_string(r#"join "", ("aba" =~ /a/g)"#), "aa");
}

#[test]
fn grep_scalar_counts_string_equality_matches() {
    assert_eq!(
        eval_int(r#"0 + grep { $_ eq "fr8_mid" } qw(fr8_lo fr8_mid fr8_hi)"#),
        1
    );
}

#[test]
fn hash_keys_sorted_after_new_key_insertion() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my %h = (fr8_k2 => 3);
            $h{fr8_k1} = 2;
            join "", sort keys %h"#
        ),
        "fr8_k1fr8_k2"
    );
}

#[test]
fn scalar_values_counts_anon_hash_entries() {
    assert_eq!(eval_int(r#"scalar values %{ { fr8_only => 9 } }"#), 1);
}

#[test]
fn push_flattens_parenthesized_list_and_trailing_scalar() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @c = (1, 2);
            push @c, (3, 4), 5;
            join "", @c"#
        ),
        "12345"
    );
}

#[test]
fn array_subscript_uses_arithmetic_on_index_variable() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @d = (1, 2, 3);
            my $x = 2;
            $d[$x - 1]"#
        ),
        2
    );
}

#[test]
fn int_cos_zero_plus_sin_zero_is_one() {
    assert_eq!(eval_int(r#"int(cos(0) + sin(0))"#), 1);
}

#[test]
fn substitution_backref_prefixes_each_digit() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $h = "123";
            $h =~ s/(\d)/x$1/g;
            $h"#
        ),
        "x1x2x3"
    );
}

#[test]
fn scalar_length_includes_trailing_index_after_sparse_assign() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @f = (1);
            $f[3] = 9;
            scalar @f"#
        ),
        4
    );
}

#[test]
fn max_index_after_assign_to_fifth_slot_from_pair() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @g = (1, 2);
            $g[5] = 0;
            $#g"#
        ),
        5
    );
}

#[test]
fn single_element_assign_defines_array_length_one() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @x;
            $x[0] = 1;
            scalar @x"#
        ),
        1
    );
}

#[test]
fn scalar_division_yields_float_for_odd_numerator() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $a = 5;
            $a = $a / 2;
            "$a""#
        ),
        "2.5"
    );
}

// ── `map`/`grep`/list builtins, string ops, hash/array surgery, bitwise, regex, `pack`, `splice`, `atan2` ──

#[test]
fn map_adds_fraction_to_integer_topics() {
    assert_eq!(eval_string(r#"join "", map { $_ + 0.5 } 1, 2"#), "1.52.5");
}

#[test]
fn map_doubles_numeric_range_with_hyphen_join() {
    assert_eq!(eval_string(r#"join "-", map { $_ * 2 } 1 .. 2"#), "2-4");
}

#[test]
fn grep_filters_named_array_by_numeric_comparison() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3, 4);
            join "", grep { $_ < 3 } @a"#
        ),
        "12"
    );
}

#[test]
fn bare_builtin_product_three_integers() {
    assert_eq!(eval_int(r#"product(2, 3, 4)"#), 24);
}

#[test]
fn bare_builtin_min_of_three_integers() {
    assert_eq!(eval_int(r#"min(8, 3, 5)"#), 3);
}

#[test]
fn rindex_finds_first_codepoint_at_zero() {
    assert_eq!(eval_int(r#"rindex("fr9_abc", "f")"#), 0);
}

#[test]
fn index_with_offset_finds_ab_after_prefix() {
    assert_eq!(eval_int(r#"index("fr9_abab", "ab", 1)"#), 4);
    assert_eq!(eval_int(r#"index("abab", "ab", 1)"#), 2);
}

#[test]
fn substr_one_arg_drops_leading_codepoint() {
    assert_eq!(eval_string(r#"substr "fr9_hello", 1"#), "r9_hello");
}

#[test]
fn fc_foldcases_ascii_mixed_pair() {
    assert_eq!(eval_string(r#"fc "Fr9_Ab""#), "fr9_ab");
}

#[test]
fn sprintf_two_fraction_digits_on_tenth_and_bankers_quarter() {
    assert_eq!(eval_string(r#"sprintf "%.2f", 1.2"#), "1.20");
    assert_eq!(eval_string(r#"sprintf "%.2f", 1.255"#), "1.25");
}

#[test]
fn array_element_assign_through_slice_subscript() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @b = (5, 6, 7);
            $b[1] = 8;
            join "", @b"#
        ),
        "587"
    );
}

#[test]
fn delete_hash_key_leaves_other_sorted_key() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my %h = (fr9_del_a => 1);
            $h{fr9_del_b} = 2;
            delete $h{fr9_del_a};
            join "", sort keys %h"#
        ),
        "fr9_del_b"
    );
}

#[test]
fn array_name_in_scalar_yields_element_count() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @c = (1, 2, 3);
            my $n = @c;
            $n"#
        ),
        3
    );
}

#[test]
fn defined_distinguishes_undef_from_defined_zero() {
    assert_eq!(eval_int(r#"0 + (defined undef)"#), 0);
    assert_eq!(eval_int(r#"0 + (defined 0)"#), 1);
}

#[test]
fn negative_one_subscript_reads_last_element() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            $a[-1]"#
        ),
        3
    );
}

#[test]
fn negative_one_subscript_assigns_last_element() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @b = (1, 2, 3);
            $b[-1] = 9;
            join "", @b"#
        ),
        "129"
    );
}

#[test]
fn map_preincrement_copies_array_topics() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @t = (1, 2);
            join "", map { ++$_ } @t"#
        ),
        "23"
    );
}

#[test]
fn modulo_positive_divisor_reduces_nonnegative() {
    assert_eq!(eval_int(r#"10 % 3"#), 1);
}

#[test]
fn or_eq_combines_bit_flags() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $v = 1;
            $v |= 4;
            $v"#
        ),
        5
    );
}

#[test]
fn and_mask_keeps_intersection_bits() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $w = 7;
            $w &= 5;
            $w"#
        ),
        5
    );
}

#[test]
fn regex_anchor_matches_leading_literal() {
    assert_eq!(eval_int(r#"("fr9_xyz" =~ /^fr9/) ? 1 : 0"#), 1);
}

#[test]
fn grep_modulo_selects_evens_from_named_array() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @a = (1, 2, 3, 4);
            join "", grep { $_ % 2 == 0 } @a"#
        ),
        "24"
    );
}

#[test]
fn map_pack_c_builds_byte_string_from_codepoints() {
    assert_eq!(
        eval_string(r#"join "", map { pack("C", $_) } (65, 66)"#),
        "AB"
    );
}

#[test]
fn undef_lexical_assigns_numeric() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $x = undef;
            $x = 3;
            $x"#
        ),
        3
    );
}

#[test]
fn defined_or_assign_sets_undef_lexical_once() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $y;
            $y //= 7;
            $y"#
        ),
        7
    );
}

#[test]
fn exists_true_for_initialized_array_index() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @d = (1, 2, 3);
            exists $d[1] ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn delete_array_slot_joins_defined_or_placeholder() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @e = (1, 2, 3);
            delete $e[1];
            join "", map { $_ // "x" } @e"#
        ),
        "1x3"
    );
}

#[test]
fn join_global_matches_on_repeated_single_char() {
    assert_eq!(eval_string(r#"join "", ("fr9_xx" =~ /x/g)"#), "xx");
}

#[test]
fn substitution_capture_rotates_three_letters() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $z = "abc";
            $z =~ /^(.)(.)(.)$/;
            $z = "$3$2$1";
            $z"#
        ),
        "cba"
    );
}

#[test]
fn zero_padded_string_numeric_compare_and_string_eq() {
    assert_eq!(eval_int(r#"0 + ("01" == 1)"#), 1);
    assert_eq!(eval_int(r#"0 + ("01" eq "01")"#), 1);
}

#[test]
fn scalar_splice_removing_head_returns_removed_element() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @f = (1, 2, 3);
            scalar splice @f, 0, 1"#
        ),
        1
    );
}

#[test]
fn splice_removing_head_leaves_tail_joined() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @g = (1, 2, 3);
            splice @g, 0, 1;
            join "", @g"#
        ),
        "23"
    );
}

#[test]
fn pi_constant_exceeds_three_from_atan2() {
    assert_eq!(eval_int(r#"(atan2(0, -1) > 3) ? 1 : 0"#), 1);
}

// ── `map`/`..`, `grep`, coercions, slices, splits, floats, `//`, regex class, `sort`, `s///`, `log`, hash `//=`, `reverse`, `**` ──

#[test]
fn map_identity_joins_low_numeric_range() {
    assert_eq!(eval_string(r#"join "", map { $_ } 0 .. 2"#), "012");
}

#[test]
fn grep_scalar_counts_strictly_positive_topics() {
    assert_eq!(eval_int(r#"0 + grep { $_ > 0 } (-1, 0, 1)"#), 1);
}

#[test]
fn sprintf_percent_d_coerces_zero_padded_decimal_string() {
    assert_eq!(eval_string(r#"sprintf "%d", "007""#), "7");
}

#[test]
fn string_eq_same_ascii_digit_is_true() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $x = "3";
            $x eq "3" ? 1 : 0"#
        ),
        1
    );
}

#[test]
fn scalar_array_subscript_slice_is_last_indexed_value() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @b = (1, 2, 3);
            $b[0, 1]"#
        ),
        2
    );
}

#[test]
fn hash_slice_list_assign_overwrites_single_named_key() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %h = (fr11_slot => 1);
            @h{qw(fr11_slot)} = (9);
            $h{fr11_slot}"#
        ),
        9
    );
}

#[test]
fn split_pipe_pattern_joins_with_hyphen() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "fr11_a|fr11_b|fr11_c";
            join "-", split /\|/, $s"#
        ),
        "fr11_a-fr11_b-fr11_c"
    );
}

#[test]
fn split_comma_pattern_joins_fields_without_separator() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $t = "fr11_x,fr11_y,fr11_z";
            join "", split /,/, $t"#
        ),
        "fr11_xfr11_yfr11_z"
    );
}

#[test]
fn array_range_slice_joins_interior_pair() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @c = (1, 2, 3, 4);
            join "", @c[1 .. 2]"#
        ),
        "23"
    );
}

#[test]
fn scalar_plus_fractional_half_from_integer() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $u = 1;
            $u = $u + 0.5;
            "$u""#
        ),
        "1.5"
    );
}

#[test]
fn int_truncation_of_opposite_signed_halves_sums_zero() {
    assert_eq!(eval_int(r#"int(1.9) + int(-1.9)"#), 0);
}

#[test]
fn float_multiplication_quarter_times_four() {
    assert_eq!(eval_int(r#"int(1.25 * 4)"#), 5);
}

#[test]
fn scalar_array_counts_elements_and_empty_is_zero() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @d = (1, 2, 3);
            my @e;
            scalar @d + scalar @e"#
        ),
        3
    );
}

#[test]
fn defined_or_shows_nil_only_for_undef_lexical() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $v;
            my $w = 0;
            ($v // "fr11_nil") . ":" . $w"#
        ),
        "fr11_nil:0"
    );
}

#[test]
fn vowel_class_global_joins_matches_on_ascii_word() {
    assert_eq!(eval_string(r#"join "", ("fr11_abc" =~ /[aeiou]/g)"#), "a");
}

#[test]
fn lc_and_uc_round_trip_ascii_triple_letters() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $p = "FR11_ABC";
            my $q = "fr11_xyz";
            lc($p) . ":" . uc($q)"#
        ),
        "fr11_abc:FR11_XYZ"
    );
}

#[test]
fn sprintf_hex_and_oct_formats_for_ten_and_eight() {
    assert_eq!(eval_string(r#"sprintf "%x", 10"#), "a");
    assert_eq!(eval_string(r#"sprintf "%X", 10"#), "A");
    assert_eq!(eval_string(r#"sprintf "%o", 8"#), "10");
}

#[test]
fn array_three_index_slice_takes_first_third_fifth() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @f = (1, 2, 3, 4, 5);
            join "", @f[0, 2, 4]"#
        ),
        "135"
    );
}

#[test]
fn sort_block_reverse_lexical_on_three_letters() {
    assert_eq!(
        eval_string(r#"join "", sort { $b cmp $a } qw(fr11_m fr11_a fr11_z)"#),
        "fr11_zfr11_mfr11_a"
    );
}

#[test]
fn substitution_global_replaces_each_char_with_lowercase_x() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $t = "fr11_ab";
            $t =~ s/./x/g;
            $t"#
        ),
        "xxxxxxx"
    );
}

#[test]
fn int_log_of_near_e_constant_truncates_to_one() {
    assert_eq!(eval_int(r#"int(log(2.71828182845904523536))"#), 1);
}

#[test]
fn hash_defined_or_assigns_missing_slot_only() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %h = (fr11_a => 1);
            $h{fr11_b} //= 2;
            $h{fr11_b}"#
        ),
        2
    );
}

#[test]
fn hash_defined_or_leaves_existing_slot_unchanged() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %i = (fr11_c => 1);
            $i{fr11_c} //= 9;
            $i{fr11_c}"#
        ),
        1
    );
}

#[test]
fn numeric_equality_int_and_float_same_magnitude() {
    assert_eq!(eval_int(r#"0 + (4 == 4.0)"#), 1);
}

#[test]
fn join_hyphen_interpolates_dot_regex_global_fields() {
    assert_eq!(
        eval_string(r#"join "-", ("fr11_abc" =~ /./g)"#),
        "f-r-1-1-_-a-b-c"
    );
}

#[test]
fn scalar_reverse_list_stringifies_concatenated_digits() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @c = (1, 2, 3);
            scalar rev @c"#
        ),
        "321"
    );
}

#[test]
fn exponentiation_binds_before_unary_minus_on_literal() {
    assert_eq!(eval_int(r#"-2 ** 2"#), -4);
}

#[test]
fn multiply_eq_flips_sign_with_negative_one() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my $k = 1;
            $k *= -1;
            $k"#
        ),
        -1
    );
}

#[test]
fn rindex_finds_last_occurrence_of_single_letter() {
    assert_eq!(eval_int(r#"rindex("fr11_aba", "a")"#), 7);
}

// ── List/map/grep, list builtins, strings, `sprintf`, assigns, `tr`/`s`, regex, math, `reverse`, sort by length ──

#[test]
fn list_literal_non_contiguous_indices_join() {
    assert_eq!(eval_string(r#"join "", (1, 2, 3, 4, 5)[1, 3]"#), "24");
}

#[test]
fn map_triples_each_element_of_short_numeric_range() {
    assert_eq!(eval_string(r#"join "", map { $_ * 3 } 1 .. 2"#), "36");
}

#[test]
fn scalar_grep_counts_equality_hits_on_named_array() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @a = (1, 2, 3);
            scalar grep { $_ == 2 } @a"#
        ),
        1
    );
}

#[test]
fn bare_builtin_max_one_through_four_inclusive() {
    assert_eq!(eval_int(r#"max(1, 2, 3, 4)"#), 4);
}

#[test]
fn bare_builtin_min_among_nine_four_and_seven() {
    assert_eq!(eval_int(r#"min(9, 4, 7)"#), 4);
}

#[test]
fn substr_from_negative_offset_with_length() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $s = "hello";
            substr $s, -3, 2"#
        ),
        "ll"
    );
}

#[test]
fn index_finds_embedded_pair_after_prefix() {
    assert_eq!(eval_int(r#"index("fr12_abab", "ab")"#), 5);
}

#[test]
fn sprintf_zero_pads_to_four_places() {
    assert_eq!(eval_string(r#"sprintf "%04d", 7"#), "0007");
}

#[test]
fn sprintf_plus_includes_sign_on_negative_integer() {
    assert_eq!(eval_string(r#"sprintf "%+d", -3"#), "-3");
}

#[test]
fn array_element_compound_multiplies_in_place() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @b = (1, 2, 3);
            $b[1] *= 2;
            join "", @b"#
        ),
        "143"
    );
}

#[test]
fn bitwise_xor_two_small_integers() {
    assert_eq!(eval_int(r#"5 ^ 2"#), 7);
}

#[test]
fn hash_slot_plus_eq_accumulates() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my %h = (fr12_acc => 1);
            $h{fr12_acc} += 4;
            $h{fr12_acc}"#
        ),
        5
    );
}

#[test]
fn transliterate_swaps_one_letter_class() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $t = "abc";
            $t =~ tr/b/c/;
            $t"#
        ),
        "acc"
    );
}

#[test]
fn substitution_global_rewrites_digits_to_underscores() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $u = "a1c2";
            $u =~ s/\d/_/g;
            $u"#
        ),
        "a_c_"
    );
}

#[test]
fn join_global_matches_on_double_leading_letter() {
    assert_eq!(eval_string(r#"join "", ("aab" =~ /a/g)"#), "aa");
}

#[test]
fn array_subscript_uses_grep_values_as_index_list() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @c = (1, 2, 3, 4);
            join "", @c[grep { $_ % 2 == 0 } @c]"#
        ),
        "3"
    );
}

#[test]
fn int_exp_one_truncates_to_two() {
    assert_eq!(eval_int(r#"int(exp(1))"#), 2);
}

#[test]
fn int_double_atan2_half_pi_truncates_to_three() {
    assert_eq!(eval_int(r#"int(atan2(1, 0) * 2)"#), 3);
}

#[test]
fn sum_first_and_last_array_element_via_negative_index() {
    assert_eq!(
        eval_int(
            r#"no strict 'vars';
            my @d = (10, 20, 30);
            $d[0] + $d[-1]"#
        ),
        40
    );
}

#[test]
fn reverse_bare_qw_joins_opposite_order() {
    assert_eq!(
        eval_string(r#"join "", rev qw(fr12_p fr12_q fr12_r)"#),
        "fr12_rfr12_qfr12_p"
    );
}

#[test]
fn map_flips_case_per_ascii_letter_class_on_split() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my $z = "AbCd";
            join "", map { $_ =~ /[A-Z]/ ? lc $_ : uc $_ } split //, $z"#
        ),
        "aBcD"
    );
}

#[test]
fn string_ne_distinguishes_integer_from_zero_padded() {
    assert_eq!(eval_int(r#"0 + ("3" ne "03")"#), 1);
}

#[test]
fn sort_numeric_by_string_length() {
    assert_eq!(
        eval_string(r#"join "", sort { length($a) <=> length($b) } qw(fr12_qq fr12_q fr12_rrr)"#),
        "fr12_qfr12_qqfr12_rrr"
    );
}

#[test]
fn map_block_uc_without_explicit_dollarunderscore() {
    assert_eq!(
        eval_string(r#"join "", map { uc } qw(fr20_a fr20_b)"#),
        "FR20_AFR20_B"
    );
}

#[test]
fn reverse_map_adds_ten_then_reverses_numeric_list_order() {
    assert_eq!(
        eval_string(r#"join "", rev map { $_ + 10 } (1, 2, 3)"#),
        "131211"
    );
}

#[test]
fn unpack_c_star_lists_byte_ordinals_of_ascii_hi() {
    assert_eq!(eval_string(r#"join "", unpack "C*", "Hi""#), "72105");
}

#[test]
fn sort_strings_by_descending_length() {
    assert_eq!(
        eval_string(r#"join "", sort { length($b) <=> length($a) } qw(fr20_xx fr20_x fr20_xxx)"#),
        "fr20_xxxfr20_xxfr20_x"
    );
}

#[test]
fn stringy_and_returns_rhs_when_lhs_is_truthy() {
    assert_eq!(eval_int(r#"0 + (("a" && "b") eq "b")"#), 1);
}

#[test]
fn stringy_or_returns_rhs_when_lhs_is_falsy() {
    assert_eq!(eval_int(r#"0 + (("0" || "1") eq "1")"#), 1);
}

#[test]
fn defined_or_replaces_undef_scalar() {
    assert_eq!(eval_int(r#"0 + ((undef // 7) == 7)"#), 1);
}

#[test]
fn spaceship_three_way_comparison_chain() {
    assert_eq!(
        eval_int(r#"0 + ((3 <=> 5) == -1 && (5 <=> 5) == 0 && (7 <=> 3) == 1)"#),
        1
    );
}

#[test]
fn map_square_operator_on_integer_list() {
    assert_eq!(eval_string(r#"join "", map { $_**2 } (1, 2, 3)"#), "149");
}

#[test]
fn numeric_range_slice_middle_segment_joins_with_hyphens() {
    assert_eq!(eval_string(r#"join "-", (10..20)[3..6]"#), "13-14-15-16");
}

#[test]
fn substr_from_offset_nine_to_end_of_tagged_foobar() {
    assert_eq!(eval_string(r#"substr "fr20_foobar", 9"#), "ar");
}

#[test]
fn split_whitespace_regex_drops_leading_empty_fields() {
    assert_eq!(
        eval_string(r#"join "", split /\s+/, "  fr20_x  fr20_y  ""#),
        "fr20_xfr20_y"
    );
}

#[test]
fn grep_truthy_length_filters_out_empty_strings() {
    assert_eq!(
        eval_string(r#"join "", grep { length $_ } "", "fr20_z", """#),
        "fr20_z"
    );
}

#[test]
fn concat_assign_repeated_bang_via_string_repeat() {
    assert_eq!(
        eval_string(r#"do { my $x = "fr20_hi"; $x .= "!" x 2; $x }"#),
        "fr20_hi!!"
    );
}

#[test]
fn map_squares_over_inclusive_range_two_to_five() {
    assert_eq!(
        eval_string(r#"join "-", map { $_ * $_ } (2..5)"#),
        "4-9-16-25"
    );
}

#[test]
fn sprintf_percent_o_formats_decimal_twenty_four() {
    assert_eq!(eval_string(r#"sprintf "%o", 24"#), "30");
}

#[test]
fn list_literal_slice_with_range_subscript() {
    assert_eq!(
        eval_string(r#"join "-", (10, 11, 12, 13, 14)[2..4]"#),
        "12-13-14"
    );
}

#[test]
fn abs_of_truncated_negative_float() {
    assert_eq!(eval_int(r#"abs int(-2.7)"#), 2);
}

#[test]
fn sort_fixed_tagged_tokens_lexically() {
    assert_eq!(
        eval_string(r#"join "", sort { $a cmp $b } qw(fr20_c fr20_a fr20_b)"#),
        "fr20_afr20_bfr20_c"
    );
}

#[test]
fn sqrt_times_sqrt_exceeds_one_point_nine_nine_nine() {
    assert_eq!(eval_int(r#"0 + (sqrt(2) * sqrt(2) > 1.999)"#), 1);
}

#[test]
fn int_map_pair_joins_truncated_floats_with_hyphen() {
    assert_eq!(
        eval_string(r#"join "-", map { int($_) } (2.7, -2.7)"#),
        "2--2"
    );
}

#[test]
fn double_bang_string_zero_is_falsy() {
    assert_eq!(eval_int(r#"0 + (!!"0")"#), 0);
}

#[test]
fn double_bang_string_zero_e_zero_is_truthy() {
    assert_eq!(eval_int(r#"0 + (!!"0E0")"#), 1);
}

#[test]
fn grep_integer_equality_excludes_floats() {
    assert_eq!(
        eval_string(r#"join "-", grep { int($_) == $_ } (1, 2.5, 3, 4.2)"#),
        "1-3"
    );
}

#[test]
fn map_bitwise_and_with_seven_masks_low_three_bits() {
    assert_eq!(eval_string(r#"join "", map { $_ & 7 } (9, 10, 11)"#), "123");
}

#[test]
fn map_bitwise_or_with_two_sets_low_bit() {
    assert_eq!(eval_string(r#"join "", map { $_ | 2 } (1, 2, 3)"#), "323");
}

#[test]
fn map_bitwise_xor_with_three() {
    assert_eq!(
        eval_string(r#"join "", map { $_ ^ 3 } (1, 2, 3, 4)"#),
        "2107"
    );
}

#[test]
fn map_chr_ord_round_trips_each_character_from_qw() {
    assert_eq!(
        eval_string(r#"join "", map { chr(ord($_)) } qw(X Y)"#),
        "XY"
    );
}

#[test]
fn grep_scalar_count_finds_single_eq_match_in_list() {
    assert_eq!(
        eval_int(r#"0 + grep { $_ eq "fr20_hit" } qw(fr20_miss fr20_hit fr20_nope)"#),
        1
    );
}

#[test]
fn map_predecrement_mutates_copied_array_in_do_block() {
    assert_eq!(
        eval_string(r#"join "", map { --$_ } do { my @x = (3, 4, 5); @x }"#),
        "234"
    );
}

#[test]
fn map_adds_one_half_to_each_integer() {
    assert_eq!(
        eval_string(r#"join "-", map { $_ + 0.5 } (1, 2, 3)"#),
        "1.5-2.5-3.5"
    );
}

#[test]
fn list_literal_slice_negative_first_and_last_indices() {
    assert_eq!(eval_string(r#"join "", (0, 1, 2, 3)[-4, -1]"#), "03");
}

#[test]
fn negative_subscript_second_from_end_equals_numeric() {
    assert_eq!(eval_int(r#"0 + ((10, 20, 30)[-2] == 20)"#), 1);
}

#[test]
fn map_chr_from_64_plus_offset() {
    assert_eq!(
        eval_string(r#"join "", map { chr(64 + $_) } (1, 2, 3)"#),
        "ABC"
    );
}

#[test]
fn sort_strings_by_length_then_lexically_with_cmp() {
    assert_eq!(
        eval_string(r#"join "-", sort { length($a) cmp length($b) } qw(fr22_a fr22_aaa fr22_bb)"#),
        "fr22_a-fr22_bb-fr22_aaa"
    );
}

#[test]
fn join_empty_string_flattens_integer_list_like_concat() {
    assert_eq!(eval_int(r#"0 + (join("", (1, 2, 3)) eq "123")"#), 1);
}

#[test]
fn map_cubes_each_element() {
    assert_eq!(
        eval_string(r#"join "", map { $_ * $_ * $_ } (1, 2, 3)"#),
        "1827"
    );
}

#[test]
fn map_sprintf_lower_hex_no_width() {
    assert_eq!(
        eval_string(r#"join "", map { sprintf "%x", $_ } (10, 11, 255)"#),
        "abff"
    );
}

#[test]
fn lexicographic_string_gt_differs_from_numeric_gt_on_embedded_numbers() {
    assert_eq!(eval_int(r#"0 + ("fr22_9" > "fr22_10")"#), 0);
    assert_eq!(eval_int(r#"0 + ("fr22_9" gt "fr22_10")"#), 1);
}

#[test]
fn map_length_of_each_fixed_token() {
    assert_eq!(
        eval_string(r#"join "-", map { length($_) } qw(fr22_x fr22_yy)"#),
        "6-7"
    );
}

#[test]
fn grep_string_greater_or_equal_midpoint() {
    assert_eq!(
        eval_string(r#"join "", grep { $_ ge "fr22_m" } qw(fr22_a fr22_m fr22_z)"#),
        "fr22_mfr22_z"
    );
}

#[test]
fn map_hash_value_lookup_in_key_order_argument() {
    assert_eq!(
        eval_string(
            r#"do { my %h = (fr22_a => 1, fr22_b => 2); join "-", map { $h{$_} } qw(fr22_b fr22_a) }"#
        ),
        "2-1"
    );
}

#[test]
fn pack_a2_twice_joins_fixed_width_ascii_fields() {
    assert_eq!(
        eval_string(r#"join "", map { pack "a2", $_ } qw(XX YY)"#),
        "XXYY"
    );
}

#[test]
fn unpack_a2a2_reads_two_pairs_from_buffer() {
    assert_eq!(eval_string(r#"unpack "a2a2", "ABCD""#), "ABCD");
}

#[test]
fn map_sum_of_low_and_high_nybbles() {
    assert_eq!(
        eval_string(r#"join "-", map { ($_ & 15) + ($_ >> 4) } (0x12, 0x34)"#),
        "3-7"
    );
}

#[test]
fn grep_scalar_count_strictly_positive() {
    assert_eq!(eval_int(r#"0 + grep { $_ > 0 } (-1, 0, 1, 2)"#), 2);
}

#[test]
fn map_ucfirst_after_lc_on_mixed_case_token() {
    assert_eq!(
        eval_string(r#"join "", map { ucfirst lc $_ } qw(fr22_hELLO)"#),
        "Fr22_hello"
    );
}

#[test]
fn map_fc_casefolds_ascii_tokens() {
    assert_eq!(
        eval_string(r#"join "-", map { fc $_ } qw(fr22_B b)"#),
        "fr22_b-b"
    );
}

#[test]
fn float_multiplication_three_six_not_exactly_equal() {
    assert_eq!(eval_int(r#"0 + ((1.2 * 3) == 3.6)"#), 0);
}

#[test]
fn map_int_division_across_zero_to_eight() {
    assert_eq!(
        eval_string(r#"join "", map { int($_ / 3) } (0..8)"#),
        "000111222"
    );
}

#[test]
fn slice_of_qw_list_with_two_indices() {
    assert_eq!(
        eval_string(r#"join "-", (qw(fr22_p fr22_q fr22_r))[1, 2]"#),
        "fr22_q-fr22_r"
    );
}

#[test]
fn regex_global_finds_two_non_overlapping_aba() {
    assert_eq!(eval_string(r#"join "", ("fr22_ababa" =~ /aba/g)"#), "aba");
}

#[test]
fn index_finds_first_z_in_tagged_triple_z() {
    assert_eq!(eval_int(r#"index("fr22_zzz", "z")"#), 5);
}

#[test]
fn map_mod_three_over_one_to_nine() {
    assert_eq!(
        eval_string(r#"join "", map { $_ % 3 } (1..9)"#),
        "120120120"
    );
}

#[test]
fn push_appends_list_and_join_stringifies() {
    assert_eq!(
        eval_string(r#"do { my @a = (1); push @a, 2, 3; join "", @a }"#),
        "123"
    );
}

#[test]
fn splice_replaces_middle_span_with_singleton() {
    assert_eq!(
        eval_string(r#"do { my @a = (1, 2, 3, 4); splice @a, 1, 2, (9); join "", @a }"#),
        "194"
    );
}

#[test]
fn map_eval_arithmetic_string_doubles_each() {
    assert_eq!(
        eval_string(r#"join "", map { eval "$_ * 2" } (3, 4)"#),
        "68"
    );
}

#[test]
fn string_double_zero_numeric_compare_eq_zero() {
    assert_eq!(eval_int(r#"0 + ("fr23_00" == 0)"#), 1);
}

#[test]
fn oct_map_accepts_leading_zero_and_binary_literal() {
    assert_eq!(
        eval_string(r#"join "-", map { oct($_) } qw(010 0b11)"#),
        "8-3"
    );
}

#[test]
fn hex_map_on_0x_and_bare_ff() {
    assert_eq!(
        eval_string(r#"join "", map { hex($_) } qw(0x10 ff)"#),
        "16255"
    );
}

#[test]
fn regex_global_one_dot_matches_pairs_in_binary_string() {
    assert_eq!(eval_string(r#"join "", ("fr23_10101" =~ /1./g)"#), "1010");
}

#[test]
fn map_substr_fixed_width_prefix_two_words() {
    assert_eq!(
        eval_string(r#"join "-", map { substr $_, 0, 3 } qw(fr23_abcdef fr23_xyz12)"#),
        "fr2-fr2"
    );
}

#[test]
fn grep_class_anchor_initial_vowel_after_prefix() {
    assert_eq!(
        eval_string(r#"join "", grep { $_ =~ /^fr23_[aeiou]/ } qw(fr23_a fr23_b fr23_e)"#),
        "fr23_afr23_e"
    );
}

#[test]
fn double_bang_eval_block_constant_is_truthy() {
    assert_eq!(eval_int(r#"0 + !!(eval { 1 })"#), 1);
}

#[test]
fn sprintf_round_half_to_even_two_point_five() {
    assert_eq!(eval_string(r#"sprintf "%.0f", 2.5"#), "2");
}

// ── Chained comparisons (Raku-style) ──

#[test]
fn chained_comparison_less_than_true() {
    // `1 < 2 < 3` → `(1 < 2) && (2 < 3)` → true
    assert_eq!(eval_int(r#"(1 < 2 < 3) ? 1 : 0"#), 1);
}

#[test]
fn chained_comparison_less_than_false_right() {
    // `1 < 2 < 1` → `(1 < 2) && (2 < 1)` → false
    assert_eq!(eval_int(r#"(1 < 2 < 1) ? 1 : 0"#), 0);
}

#[test]
fn chained_comparison_less_than_false_left() {
    // `3 < 2 < 5` → `(3 < 2) && (2 < 5)` → false (short-circuits)
    assert_eq!(eval_int(r#"(3 < 2 < 5) ? 1 : 0"#), 0);
}

#[test]
fn chained_comparison_three_way() {
    // `1 < 2 < 3 < 4` → `(1<2) && (2<3) && (3<4)` → true
    assert_eq!(eval_int(r#"(1 < 2 < 3 < 4) ? 1 : 0"#), 1);
}

#[test]
fn chained_comparison_mixed_lt_le() {
    // `1 < 2 <= 2` → `(1 < 2) && (2 <= 2)` → true
    assert_eq!(eval_int(r#"(1 < 2 <= 2) ? 1 : 0"#), 1);
}

#[test]
fn chained_comparison_greater_than() {
    // `5 > 3 > 1` → `(5 > 3) && (3 > 1)` → true
    assert_eq!(eval_int(r#"(5 > 3 > 1) ? 1 : 0"#), 1);
}

#[test]
fn chained_comparison_greater_than_false() {
    // `5 > 3 > 4` → `(5 > 3) && (3 > 4)` → false
    assert_eq!(eval_int(r#"(5 > 3 > 4) ? 1 : 0"#), 0);
}

#[test]
fn chained_comparison_with_variable() {
    assert_eq!(eval_int(r#"my $x = 5; (1 < $x < 10) ? 1 : 0"#), 1);
}

#[test]
fn chained_comparison_variable_out_of_range() {
    assert_eq!(eval_int(r#"my $x = 15; (1 < $x < 10) ? 1 : 0"#), 0);
}

#[test]
fn chained_comparison_string_lt() {
    // `"a" lt "b" lt "c"` → true
    assert_eq!(eval_int(r#"("a" lt "b" lt "c") ? 1 : 0"#), 1);
}

#[test]
fn chained_comparison_string_gt() {
    // `"c" gt "b" gt "a"` → true
    assert_eq!(eval_int(r#"("c" gt "b" gt "a") ? 1 : 0"#), 1);
}

// ── Default parameter values ──
// Note: In stryke, calling foo() with zero args implicitly passes $_ as the first arg.
// This is a stryke extension for lambda-style programming. Default parameters kick in
// only when argc < param count (after the implicit $_ is added).

#[test]
fn fn_scalar_param_default_used() {
    // With 2 params, foo() passes $_ to first, default used for second
    assert_eq!(eval_int(r#"fn foo($a, $x = 42) { $x } foo(1)"#), 42);
}

#[test]
fn fn_scalar_param_default_overridden() {
    assert_eq!(eval_int(r#"fn foo($a, $x = 42) { $x } foo(1, 99)"#), 99);
}

#[test]
fn fn_scalar_param_default_expression() {
    assert_eq!(eval_int(r#"fn foo($a, $x = 2 + 3) { $x } foo(1)"#), 5);
}

#[test]
fn fn_scalar_param_default_with_type() {
    assert_eq!(eval_int(r#"fn foo($a, $x: Int = 42) { $x } foo(1)"#), 42);
}

#[test]
fn fn_multiple_params_with_defaults() {
    // First param gets the one explicit arg, rest get defaults
    assert_eq!(
        eval_int(r#"fn foo($a, $b = 2, $c = 3) { $a + $b + $c } foo(1)"#),
        6
    );
}

#[test]
fn fn_mixed_params_some_defaults() {
    assert_eq!(eval_int(r#"fn foo($a, $b = 10) { $a + $b } foo(5)"#), 15);
}

#[test]
fn fn_array_param_default() {
    // Scalar first, then array default
    assert_eq!(
        eval_string(r#"fn foo($x, @a = (1, 2, 3)) { join "-", @a } foo(0)"#),
        "1-2-3"
    );
}

#[test]
fn fn_array_param_default_overridden() {
    assert_eq!(
        eval_string(r#"fn foo($x, @a = (1, 2, 3)) { join "-", @a } foo(0, 7, 8, 9)"#),
        "7-8-9"
    );
}

#[test]
fn fn_hash_param_default() {
    assert_eq!(
        eval_int(r#"fn foo($x, %h = (a => 1, b => 2)) { $h{a} + $h{b} } foo(0)"#),
        3
    );
}

#[test]
fn fn_hash_param_default_overridden() {
    assert_eq!(
        eval_int(r#"fn foo($x, %h = (a => 1, b => 2)) { $h{a} + $h{b} } foo(0, a => 10, b => 20)"#),
        30
    );
}

#[test]
fn fn_default_uses_outer_scope_variable() {
    assert_eq!(
        eval_int(r#"my $base = 100; fn foo($a, $x = $base) { $x } foo(1)"#),
        100
    );
}

#[test]
fn fn_default_calls_function() {
    assert_eq!(
        eval_int(r#"fn get_default { 99 } fn foo($a, $x = get_default()) { $x } foo(1)"#),
        99
    );
}

#[test]
fn fn_default_second_uses_first() {
    // Later param can reference earlier param
    assert_eq!(eval_int(r#"fn foo($a, $b = $a * 2) { $b } foo(10)"#), 20);
}

#[test]
fn fn_default_explicit_undef_not_use_default() {
    // Passing explicit undef should NOT use default (matches Perl behavior)
    assert_eq!(
        eval_string(r#"fn foo($a, $x = "default") { defined($x) ? $x : "undef" } foo(1, undef)"#),
        "undef"
    );
}

#[test]
fn fn_default_zero_does_not_trigger_default() {
    assert_eq!(eval_int(r#"fn foo($a, $x = 42) { $x } foo(1, 0)"#), 0);
}

#[test]
fn fn_default_empty_string_does_not_trigger_default() {
    assert_eq!(
        eval_string(r#"fn foo($a, $x = "default") { $x eq "" ? "empty" : $x } foo(1, "")"#),
        "empty"
    );
}

#[test]
fn fn_array_default_expression() {
    assert_eq!(
        eval_string(r#"fn foo($x, @a = (1..5)) { join "-", @a } foo(0)"#),
        "1-2-3-4-5"
    );
}

#[test]
fn fn_default_with_ternary() {
    assert_eq!(
        eval_int(r#"my $flag = 1; fn foo($a, $x = $flag ? 100 : 0) { $x } foo(1)"#),
        100
    );
}

#[test]
fn fn_single_param_default_topic_passes_through() {
    // When calling foo() with 0 explicit args, $_ becomes the implicit first arg
    // So for a single-param function, the default is NOT used
    assert_eq!(eval_int(r#"$_ = 999; fn foo($x = 42) { $x } foo()"#), 999);
}

// ── Autovivify: assign past end of array ──

#[test]
fn array_sparse_assign_leaves_intermediate_slots_undef() {
    assert_eq!(
        eval_string(
            r#"no strict 'vars';
            my @g = (1, 2);
            $g[3] = 9;
            join "-", map { defined($_) ? $_ : "x" } @g"#
        ),
        "1-2-x-9"
    );
}

// ── Thread macro with p/say/print/warn/die stages ──

#[test]
fn thread_macro_inc_stage() {
    assert_eq!(eval_int(r#"my $x = t 10 inc; $x"#), 11);
}

#[test]
fn thread_macro_inc_p_stage() {
    assert_eq!(eval_int(r#"t 10 inc p; 1"#), 1);
}

#[test]
fn thread_macro_inc_print_stage() {
    assert_eq!(eval_int(r#"t 10 inc print; 1"#), 1);
}

#[test]
fn thread_macro_dec_p_stage() {
    assert_eq!(eval_int(r#"my $x = t 10 dec; $x"#), 9);
}

#[test]
fn thread_macro_chained_inc_dec_p() {
    assert_eq!(eval_int(r#"my $x = t 5 inc inc dec; $x"#), 6);
}
