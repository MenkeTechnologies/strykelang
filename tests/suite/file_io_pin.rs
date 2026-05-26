//! File-IO pins. Real stryke surface:
//!   * `spew(path, content)`  — write whole content (overwrite)
//!   * `slurp(path)`          — read whole file as string
//!   * `append_file(path, c)` — append to file
//!   * Perl-style `open(my $fh, "<", path); while (<$fh>) {...}`
//!   * file-test ops `-e`, `-s`
//!
//! No `lines_of` / `chunks_of` / `write_file` / `read_file` /
//! `slurp_file` builtins exist. Splitting on `/\n/` covers the
//! line-iterator case for the demos in this codebase.

use crate::common::*;

fn tmp_path(suffix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("/tmp/stryke_io_pin_{}_{}.tmp", nanos, suffix)
}

// ── spew / slurp round-trip ─────────────────────────────────────────

#[test]
fn spew_then_slurp_roundtrip_ascii() {
    let path = tmp_path("ascii");
    let code = format!(
        r#"
            my $p = "{path}";
            spew($p, "hello world\n");
            my $back = slurp($p);
            unlink $p;
            $back eq "hello world\n" ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn spew_then_slurp_roundtrip_unicode() {
    let path = tmp_path("unicode");
    let code = format!(
        r#"
            my $p = "{path}";
            spew($p, "café 🌟 Здравствуй\n");
            my $back = slurp($p);
            unlink $p;
            $back eq "café 🌟 Здравствуй\n" ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn spew_then_slurp_multiline() {
    let path = tmp_path("multi");
    let code = format!(
        r#"
            my $p = "{path}";
            spew($p, "line1\nline2\nline3\n");
            my $back = slurp($p);
            unlink $p;
            $back eq "line1\nline2\nline3\n" ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── split-after-slurp idiom for line iteration ──────────────────────

#[test]
fn slurp_then_split_yields_per_line_array() {
    let path = tmp_path("lines");
    let code = format!(
        r#"
            my $p = "{path}";
            spew($p, "alpha\nbeta\ngamma\ndelta\n");
            my @lines = split /\n/, slurp($p);
            # Trailing newline gives a phantom empty trailing element
            # only when the split limit allows it; stryke's default is
            # to drop trailing empties.
            unlink $p;
            (scalar(@lines) == 4
                && $lines[0] eq "alpha"
                && $lines[3] eq "delta") ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn slurp_empty_file_returns_empty_string() {
    let path = tmp_path("empty");
    let code = format!(
        r#"
            my $p = "{path}";
            spew($p, "");
            my $back = slurp($p);
            unlink $p;
            (defined($back) && $back eq "") ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── slurp on binary content is byte-faithful ────────────────────────

#[test]
fn slurp_preserves_arbitrary_bytes_through_spew_roundtrip() {
    // Write a binary payload that includes:
    //   * 0x00 — embedded NUL (would terminate a C string but not a Perl byte string)
    //   * 0xFF / 0xFE — high bytes invalid as a UTF-8 leading byte in isolation
    //   * 0x80 — valid UTF-8 continuation byte appearing without a leader
    //   * mixed printable + control + high bytes
    // After slurp + spew the on-disk bytes must be byte-identical.
    let src = tmp_path("bin_src");
    let dst = tmp_path("bin_dst");
    let payload: Vec<u8> = vec![
        0x00, 0x01, 0x7F, 0x80, 0xC3, 0x28, 0xFF, 0xFE, b'A', b'\n', 0xE2, 0x82, 0xAC,
    ];
    std::fs::write(&src, &payload).expect("seed binary src");
    let code = format!(
        r#"
            my $bytes = slurp("{src}");
            spew("{dst}", $bytes);
            length($bytes)
        "#,
        src = src,
        dst = dst,
    );
    let reported_len = eval_int(&code);
    let copied = std::fs::read(&dst).expect("read dst");
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&dst);
    assert_eq!(
        reported_len as usize,
        payload.len(),
        "length(slurp) should report raw byte count (Perl-default byte-string semantics)",
    );
    assert_eq!(
        copied, payload,
        "slurp -> spew must round-trip arbitrary bytes byte-for-byte",
    );
}

#[test]
fn slurp_supports_regex_and_substr_on_text_content() {
    // Regression: when slurp switched to returning bytes (Perl-default byte
    // strings), text ops on the result still have to work. This pins the
    // round-trip through regex match + substr against a normal text file.
    let path = tmp_path("regex");
    std::fs::write(&path, "alpha=1\nbeta=22\ngamma=333\n").expect("seed");
    let code = format!(
        r#"
            my $body = slurp("{path}");
            my $matched = ($body =~ /beta=(\d+)/) ? $1 : "";
            my $first6 = substr($body, 0, 6);
            unlink "{path}";
            ($matched eq "22" && $first6 eq "alpha=") ? 1 : 0
        "#,
        path = path,
    );
    assert_eq!(
        eval_int(&code),
        1,
        "regex capture and substr must work on slurp's bytes return value"
    );
}

#[test]
fn slurp_length_reports_bytes_not_chars_on_multibyte_utf8() {
    // Three-byte UTF-8 sequence (€ = 0xE2 0x82 0xAC). Perl default and Rust
    // bytes semantics: length() == 3. Char-based length would report 1.
    let path = tmp_path("euro");
    std::fs::write(&path, "\u{20AC}".as_bytes()).expect("seed euro file");
    let code = format!(r#"length(slurp("{path}"))"#, path = path);
    let n = eval_int(&code);
    let _ = std::fs::remove_file(&path);
    assert_eq!(n, 3, "expected byte count (3) not char count (1)");
}

// ── -e / -s file tests ──────────────────────────────────────────────

#[test]
fn file_test_minus_e_returns_true_for_existing() {
    let path = tmp_path("ex");
    let code = format!(
        r#"
            my $p = "{path}";
            spew($p, "x");
            my $r = -e $p ? 1 : 0;
            unlink $p;
            $r == 1 ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn file_test_minus_e_returns_false_for_missing() {
    let path = tmp_path("missing");
    let code = format!(
        r#"
            my $p = "{path}";
            -e $p ? 0 : 1
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn file_test_minus_s_returns_file_size() {
    let path = tmp_path("size");
    let code = format!(
        r#"
            my $p = "{path}";
            spew($p, "hello");
            my $sz = -s $p;
            unlink $p;
            $sz == 5 ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── overwrite vs append ─────────────────────────────────────────────

#[test]
fn spew_overwrites_existing_content() {
    let path = tmp_path("over");
    let code = format!(
        r#"
            my $p = "{path}";
            spew($p, "first");
            spew($p, "second");
            my $back = slurp($p);
            unlink $p;
            $back eq "second" ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn append_file_preserves_existing_content() {
    let path = tmp_path("app");
    let code = format!(
        r#"
            my $p = "{path}";
            spew($p, "first\n");
            append_file($p, "second\n");
            my $back = slurp($p);
            unlink $p;
            $back eq "first\nsecond\n" ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn append_to_missing_file_creates_it() {
    let path = tmp_path("app_new");
    let code = format!(
        r#"
            my $p = "{path}";
            append_file($p, "only-line\n");
            my $back = slurp($p);
            unlink $p;
            $back eq "only-line\n" ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Perl-style filehandle open / readline ───────────────────────────

#[test]
fn perl_style_open_read_close_works() {
    let path = tmp_path("perl_handle");
    let code = format!(
        r#"
            my $p = "{path}";
            spew($p, "one\ntwo\nthree\n");
            open(my $fh, "<", $p) or die "open\n";
            my @lines;
            while (my $line = <$fh>) {{
                chomp $line;
                push @lines, $line;
            }}
            close $fh;
            unlink $p;
            (scalar(@lines) == 3 && $lines[1] eq "two") ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn perl_style_open_write_works() {
    let path = tmp_path("perl_write");
    let code = format!(
        r#"
            my $p = "{path}";
            open(my $fh, ">", $p) or die "open_w\n";
            print $fh "hello via filehandle\n";
            close $fh;
            my $back = slurp($p);
            unlink $p;
            $back eq "hello via filehandle\n" ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Slurping a non-existent file ────────────────────────────────────

#[test]
fn slurp_missing_file_eval_safe() {
    let path = tmp_path("definitely_missing");
    let code = format!(
        r#"
            my $r = eval {{ slurp("{path}") }};
            !defined($r) || $@ ne "" ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Trailing newline preservation ──────────────────────────────────

#[test]
fn no_trailing_newline_preserved_through_roundtrip() {
    let path = tmp_path("trail");
    let code = format!(
        r#"
            my $p = "{path}";
            spew($p, "no trailing newline");
            my $back = slurp($p);
            unlink $p;
            $back eq "no trailing newline" ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Binary-ish content (newlines + nulls) survives ─────────────────

#[test]
fn binary_content_roundtrip() {
    let path = tmp_path("bin");
    let code = format!(
        r#"
            my $p = "{path}";
            my $payload = "abc\x00def\nghi\r\njkl";
            spew($p, $payload);
            my $back = slurp($p);
            unlink $p;
            $back eq $payload ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Thousand-line stress via slurp+split ───────────────────────────

#[test]
fn thousand_line_roundtrip_via_slurp_split() {
    let path = tmp_path("thousand");
    let code = format!(
        r#"
            my $p = "{path}";
            my @input = map {{ "line_$_" }} (1:1000);
            spew($p, join("\n", @input) . "\n");
            my @lines = split /\n/, slurp($p);
            unlink $p;
            (scalar(@lines) == 1000 && $lines[999] eq "line_1000") ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Successive append builds a log ─────────────────────────────────

#[test]
fn successive_append_builds_log() {
    let path = tmp_path("log");
    let code = format!(
        r#"
            my $p = "{path}";
            unlink $p if -e $p;
            for my $i (1:5) {{
                append_file($p, "entry-$i\n");
            }}
            my $back = slurp($p);
            unlink $p;
            $back eq "entry-1\nentry-2\nentry-3\nentry-4\nentry-5\n" ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}

// ── -e on a directory returns true ─────────────────────────────────

#[test]
fn file_test_minus_e_on_tmp_dir_is_true() {
    let code = r#"
        -e "/tmp" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Round-trip JSON via spew/slurp ─────────────────────────────────

#[test]
fn json_via_spew_slurp_roundtrip() {
    let path = tmp_path("json");
    let code = format!(
        r#"
            my $p = "{path}";
            my $orig = +{{ name => "alice", age => 30, tags => ["a", "b"] }};
            spew($p, to_json($orig));
            my $back = from_json(slurp($p));
            unlink $p;
            ($back->{{name}} eq "alice"
                && $back->{{age}} == 30
                && $back->{{tags}}->[1] eq "b") ? 1 : 0
        "#,
        path = path
    );
    assert_eq!(eval_int(&code), 1);
}
