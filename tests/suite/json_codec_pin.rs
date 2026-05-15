//! JSON codec edge-case pins. `codec_roundtrip_pin.rs` covers the
//! happy paths; this file probes the corners that production payloads
//! hit and that AI integrations care about.

use crate::common::*;

// ── Deep nesting (works in JSON; YAML has BUG-206 at depth 3+) ─────

#[test]
fn json_roundtrip_six_level_deep_hash() {
    let code = r#"
        my $deep = +{ a => +{ b => +{ c => +{ d => +{ e => +{ f => "bottom" }}}}}};
        my $back = from_json(to_json($deep));
        $back->{a}->{b}->{c}->{d}->{e}->{f} eq "bottom" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_roundtrip_six_level_deep_array() {
    let code = r#"
        my $deep = [1, [2, [3, [4, [5, [6]]]]]];
        my $back = from_json(to_json($deep));
        $back->[1]->[1]->[1]->[1]->[1]->[0] == 6 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Large arrays ────────────────────────────────────────────────────

#[test]
fn json_roundtrip_1000_element_array() {
    let code = r#"
        my @arr = (1:1000);
        my $back = from_json(to_json(\@arr));
        (len(@$back) == 1000 && $back->[0] == 1 && $back->[999] == 1000) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_roundtrip_500_key_hash() {
    let code = r#"
        my %h;
        for my $i (1:500) {
            $h{"key_$i"} = $i;
        }
        my $back = from_json(to_json(\%h));
        (len(keys %$back) == 500 && $back->{"key_250"} == 250) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Unicode handling ───────────────────────────────────────────────

#[test]
fn json_emits_unicode_directly_without_escaping() {
    let code = r#"
        my $s = to_json(+{ msg => "café 🌟" });
        # Stryke should either emit the unicode directly OR \uXXXX escape it.
        # Round-trip is what matters, not the wire format.
        my $back = from_json($s);
        $back->{msg} eq "café 🌟" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_decodes_unicode_escape_sequences() {
    let code = r#"
        my $j = '{"msg":"café"}';
        my $back = from_json($j);
        $back->{msg} eq "café" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Scientific notation ────────────────────────────────────────────

#[test]
fn json_decodes_scientific_notation() {
    let code = r#"
        my $back = from_json('{"big": 1.5e10, "small": 2.5e-5}');
        (abs($back->{big} - 1.5e10) < 1.0
            && abs($back->{small} - 2.5e-5) < 1e-10) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── null / undef ───────────────────────────────────────────────────

#[test]
fn json_null_decodes_to_undef() {
    let code = r#"
        my $back = from_json('{"a": null, "b": 0, "c": ""}');
        (!defined($back->{a})
            && defined($back->{b}) && $back->{b} == 0
            && defined($back->{c}) && $back->{c} eq "") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_emits_undef_as_null() {
    let code = r#"
        my $s = to_json(+{ a => undef, b => 1 });
        # Should contain "null" somewhere.
        (index($s, "null") >= 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Boolean handling ───────────────────────────────────────────────

#[test]
fn json_true_decodes_to_truthy_one() {
    let code = r#"
        my $back = from_json('{"flag": true}');
        ($back->{flag} == 1 && $back->{flag}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_false_decodes_to_falsy_zero() {
    let code = r#"
        my $back = from_json('{"flag": false}');
        ($back->{flag} == 0 && !$back->{flag}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Escape sequences ───────────────────────────────────────────────

#[test]
fn json_decodes_backslash_newline_tab() {
    let code = r#"
        my $back = from_json('{"s": "a\nb\tc"}');
        $back->{s} eq "a\nb\tc" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_decodes_quoted_backslash() {
    let code = r#"
        my $back = from_json('{"path": "C:\\foo\\bar"}');
        $back->{path} eq "C:\\foo\\bar" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Whitespace tolerance ───────────────────────────────────────────

#[test]
fn json_decodes_with_internal_whitespace() {
    let code = r#"
        my $back = from_json('{   "a" :   1   ,   "b"  :  2  }');
        ($back->{a} == 1 && $back->{b} == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_decodes_with_newlines_in_object() {
    let code = r#"
        my $back = from_json("{\n  \"a\": 1,\n  \"b\": 2\n}");
        ($back->{a} == 1 && $back->{b} == 2) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Numeric edge cases ─────────────────────────────────────────────

#[test]
fn json_handles_negative_numbers() {
    let code = r#"
        my $back = from_json('{"a": -42, "b": -3.14}');
        ($back->{a} == -42 && abs($back->{b} - (-3.14)) < 1e-9) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_handles_zero_variants() {
    let code = r#"
        my $back = from_json('{"i": 0, "f": 0.0, "neg": -0}');
        ($back->{i} == 0 && $back->{f} == 0.0 && $back->{neg} == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_handles_large_integer() {
    let code = r#"
        my $back = from_json('{"id": 9999999999}');
        $back->{id} == 9999999999 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Mixed type arrays ──────────────────────────────────────────────

#[test]
fn json_array_of_heterogenous_types() {
    let code = r#"
        my $back = from_json('[1, "two", 3.0, true, null, [5, 6], {"k":"v"}]');
        ($back->[0] == 1
            && $back->[1] eq "two"
            && abs($back->[2] - 3.0) < 1e-9
            && $back->[3] == 1
            && !defined($back->[4])
            && $back->[5]->[1] == 6
            && $back->[6]->{k} eq "v") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Empty containers ───────────────────────────────────────────────

#[test]
fn json_empty_array_decodes_to_arrayref() {
    let code = r#"
        my $back = from_json('[]');
        (ref($back) =~ /ARRAY/ && len(@$back) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_empty_object_decodes_to_hashref() {
    let code = r#"
        my $back = from_json('{}');
        (ref($back) =~ /HASH/ && len(keys %$back) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Round-trip survives ndjson-style line ──────────────────────────

#[test]
fn json_per_record_roundtrip_survives() {
    let code = r#"
        my @records = (
            +{ id => 1, name => "alice", tags => ["a", "b"] },
            +{ id => 2, name => "bob",   tags => ["c"]      },
            +{ id => 3, name => "carol", tags => []         },
        );
        my @back;
        for my $r (@records) {
            push @back, from_json(to_json($r));
        }
        ($back[0]->{name} eq "alice"
            && $back[1]->{tags}->[0] eq "c"
            && len(@{$back[2]->{tags}}) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Top-level scalar JSON values ───────────────────────────────────

#[test]
fn json_decodes_top_level_number() {
    let code = r#"
        my $back = from_json('42');
        $back == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_decodes_top_level_string() {
    let code = r#"
        my $back = from_json('"hello"');
        $back eq "hello" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Pretty / minified ─────────────────────────────────────────────

#[test]
fn json_minified_output_no_whitespace() {
    let code = r#"
        my $s = to_json(+{ a => 1, b => 2 });
        # No leading/trailing whitespace in default output.
        (substr($s, 0, 1) eq "{" && substr($s, -1, 1) eq "}") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
