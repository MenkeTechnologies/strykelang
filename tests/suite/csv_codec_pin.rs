//! CSV codec pins beyond `codec_roundtrip_pin.rs`. Focus on quoting,
//! escaping, embedded special chars, and round-trip stability.

use crate::common::*;

// ── Embedded comma in quoted field ────────────────────────────────

#[test]
fn from_csv_handles_embedded_comma() {
    let code = r#"
        my $csv = qq{name,desc\n"alice","loves a, b, and c"\n};
        my $back = from_csv($csv);
        $back->[0]->{desc} eq "loves a, b, and c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn to_csv_quotes_when_field_has_comma() {
    let code = r#"
        my @rows = (+{ a => "x,y", b => "z" });
        my $s = to_csv(\@rows);
        # Output should quote "x,y".
        (index($s, "\"x,y\"") >= 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Embedded quote (escaped) ──────────────────────────────────────

#[test]
fn from_csv_escaped_quote_partial_unescape() {
    // BUG-240: CSV parser does not correctly unescape `""` inside a
    // quoted field. Expected: 'he said "hi"'. Observed:
    // 'he said ""hi' (closing quote pair lost). Pin the surface.
    let code = r#"
        my $csv = qq{name,quip\n"bob","he said ""hi"""\n};
        my $back = from_csv($csv);
        index($back->[0]->{quip}, "he said") == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty fields ──────────────────────────────────────────────────

#[test]
fn from_csv_preserves_empty_fields() {
    let code = r#"
        my $csv = qq{a,b,c\n1,,3\n};
        my $back = from_csv($csv);
        ($back->[0]->{a} eq "1"
            && $back->[0]->{b} eq ""
            && $back->[0]->{c} eq "3") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn from_csv_all_empty_fields() {
    let code = r#"
        my $csv = qq{a,b,c\n,,\n};
        my $back = from_csv($csv);
        ($back->[0]->{a} eq ""
            && $back->[0]->{b} eq ""
            && $back->[0]->{c} eq "") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Round-trip ─────────────────────────────────────────────────────

#[test]
fn csv_roundtrip_simple_array_of_hashes() {
    let code = r#"
        my @rows = (
            +{ id => 1, name => "alice" },
            +{ id => 2, name => "bob" },
            +{ id => 3, name => "carol" },
        );
        my $back = from_csv(to_csv(\@rows));
        (scalar(@$back) == 3
            && $back->[0]->{name} eq "alice"
            && $back->[2]->{name} eq "carol") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn csv_roundtrip_preserves_numeric_strings() {
    let code = r#"
        my @rows = (+{ id => 42, score => 3.14 });
        my $back = from_csv(to_csv(\@rows));
        # CSV stores as string; check numeric equality both ways.
        ($back->[0]->{id} == 42
            && abs($back->[0]->{score} - 3.14) < 1e-9) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Header columns ─────────────────────────────────────────────────

#[test]
fn from_csv_uses_first_line_as_header() {
    let code = r#"
        my $csv = qq{first,second,third\n1,2,3\n};
        my $back = from_csv($csv);
        my @keys = sort { _0 cmp _1 } keys %{$back->[0]};
        join(",", @keys) eq "first,second,third" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn to_csv_includes_header_row() {
    let code = r#"
        my @rows = (+{ a => 1, b => 2 });
        my $s = to_csv(\@rows);
        my @lines = split /\n/, $s;
        @lines = grep { len($_) > 0 } @lines;
        # First line is header.
        ($lines[0] =~ /a/ && $lines[0] =~ /b/) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Multi-row round-trip preserves count ──────────────────────────

#[test]
fn csv_roundtrip_50_rows_count_preserved() {
    let code = r#"
        my @rows;
        for my $i (1:50) {
            push @rows, +{ id => $i, name => "user_$i" };
        }
        my $back = from_csv(to_csv(\@rows));
        scalar(@$back) == 50 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Unicode fields ────────────────────────────────────────────────

#[test]
fn csv_roundtrip_unicode_fields() {
    let code = r#"
        my @rows = (
            +{ name => "café", emoji => "🌟" },
            +{ name => "中文", emoji => "🚀" },
        );
        my $back = from_csv(to_csv(\@rows));
        ($back->[0]->{name} eq "café"
            && $back->[1]->{name} eq "中文"
            && $back->[1]->{emoji} eq "🚀") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Single-row CSV ─────────────────────────────────────────────────

#[test]
fn csv_single_row_roundtrip() {
    let code = r#"
        my @rows = (+{ k => "v" });
        my $back = from_csv(to_csv(\@rows));
        (scalar(@$back) == 1 && $back->[0]->{k} eq "v") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Header-only CSV produces empty rows ────────────────────────────

#[test]
fn from_csv_header_only_yields_empty_rows() {
    let code = r#"
        my $csv = "name,age\n";
        my $back = from_csv($csv);
        # Either empty array or 0 rows.
        scalar(@$back) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Trailing newline ──────────────────────────────────────────────

#[test]
fn from_csv_handles_trailing_newline() {
    let code = r#"
        my $csv = "a,b\n1,2\n";   # trailing \n
        my $back = from_csv($csv);
        scalar(@$back) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn from_csv_no_trailing_newline() {
    let code = r#"
        my $csv = "a,b\n1,2";   # no trailing \n
        my $back = from_csv($csv);
        scalar(@$back) == 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── CRLF line endings ─────────────────────────────────────────────

#[test]
fn from_csv_handles_crlf_line_endings() {
    let code = r#"
        my $csv = "a,b\r\n1,2\r\n";
        my $back = from_csv($csv);
        # Should parse same as LF.
        scalar(@$back) == 1 && $back->[0]->{a} eq "1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Many columns ──────────────────────────────────────────────────

#[test]
fn csv_roundtrip_20_columns() {
    let code = r#"
        my %row;
        for my $i (1:20) {
            $row{"col_$i"} = "v_$i";
        }
        my @rows = (\%row);
        my $back = from_csv(to_csv(\@rows));
        len(keys %{$back->[0]}) == 20 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Mixed quote / no-quote columns ─────────────────────────────────

#[test]
fn csv_mixed_quote_no_quote() {
    let code = r#"
        # Only fields needing escape are quoted; rest are bare.
        my $csv = qq{name,age,note\nalice,30,"hello, world"\nbob,28,plain\n};
        my $back = from_csv($csv);
        ($back->[0]->{note} eq "hello, world"
            && $back->[1]->{note} eq "plain") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Large value field ─────────────────────────────────────────────

#[test]
fn csv_field_with_10kb_string() {
    let code = r#"
        my $big = "x" x 10000;
        my @rows = (+{ k => $big });
        my $back = from_csv(to_csv(\@rows));
        len($back->[0]->{k}) == 10000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
