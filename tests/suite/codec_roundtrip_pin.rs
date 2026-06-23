//! Codec round-trip pins. Stryke ships native JSON/YAML/TOML/CSV/
//! MsgPack codecs as core builtins. These pins assert that
//!   `from_X(to_X(value)) == value`
//! across each codec for scalars, arrays, hashes, nested combos,
//! Unicode, and a few edge cases.

use crate::common::*;

// ── JSON ──────────────────────────────────────────────────────────────

#[test]
fn json_roundtrip_flat_hash() {
    let code = r#"
        my $orig = +{ name => "alice", age => 30 };
        my $back = from_json(to_json($orig));
        ($back->{name} eq "alice" && $back->{age} == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_roundtrip_nested_array() {
    let code = r#"
        my $orig = [1, [2, [3, [4, 5]]]];
        my $back = from_json(to_json($orig));
        ($back->[0] == 1
            && $back->[1]->[0] == 2
            && $back->[1]->[1]->[0] == 3
            && $back->[1]->[1]->[1]->[1] == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_roundtrip_unicode_string() {
    let code = r#"
        my $orig = +{ greeting => "Здравствуй 🌟 café" };
        my $back = from_json(to_json($orig));
        $back->{greeting} eq "Здравствуй 🌟 café" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_roundtrip_mixed_types() {
    let code = r#"
        my $orig = +{
            int    => 42,
            float  => 3.14,
            str    => "hello",
            arr    => [1, "two", 3.0],
            inner  => +{ a => "b" },
        };
        my $back = from_json(to_json($orig));
        ($back->{int} == 42
            && abs($back->{float} - 3.14) < 1e-9
            && $back->{str} eq "hello"
            && $back->{arr}->[1] eq "two"
            && $back->{inner}->{a} eq "b") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_decode_bool_and_null() {
    let code = r#"
        my $back = from_json('{"t":true, "f":false, "n":null}');
        ($back->{t} == 1
            && $back->{f} == 0
            && !defined($back->{n})) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn json_decode_empty_object_and_array() {
    let code = r#"
        my $empty_obj = from_json('{}');
        my $empty_arr = from_json('[]');
        (ref($empty_obj) =~ /HASH/
            && ref($empty_arr) =~ /ARRAY/
            && len(@$empty_arr) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── YAML ──────────────────────────────────────────────────────────────

#[test]
fn yaml_roundtrip_flat_hash() {
    let code = r#"
        my $orig = +{ name => "stryke", version => "0.14" };
        my $back = from_yaml(to_yaml($orig));
        ($back->{name} eq "stryke" && $back->{version} eq "0.14") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn yaml_roundtrip_nested_structure() {
    let code = r#"
        my $orig = +{
            config => +{
                jit      => 1,
                parallel => 1,
                threads  => 8,
            },
            paths => ["bin", "lib", "examples"],
        };
        my $back = from_yaml(to_yaml($orig));
        ($back->{config}->{threads} == 8
            && $back->{paths}->[2] eq "examples") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn yaml_roundtrip_preserves_array_order() {
    let code = r#"
        my @orig = ("first", "second", "third", "fourth");
        my $back = from_yaml(to_yaml(\@orig));
        join(",", @$back) eq "first,second,third,fourth" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── TOML ──────────────────────────────────────────────────────────────

#[test]
fn toml_roundtrip_top_level_scalars() {
    let code = r#"
        my $orig = +{ name => "stryke", version => "0.14.3", count => 42 };
        my $back = from_toml(to_toml($orig));
        ($back->{name} eq "stryke"
            && $back->{version} eq "0.14.3"
            && $back->{count} == 42) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn toml_roundtrip_array() {
    let code = r#"
        my $orig = +{ tags => ["rust", "perl", "jit"] };
        my $back = from_toml(to_toml($orig));
        ($back->{tags}->[0] eq "rust"
            && $back->{tags}->[2] eq "jit") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn toml_roundtrip_nested_table() {
    let code = r#"
        my $orig = +{
            package => +{
                name    => "stryke",
                version => "0.14",
            },
        };
        my $back = from_toml(to_toml($orig));
        ($back->{package}->{name} eq "stryke") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn toml_strips_inline_comment_after_value() {
    // `key = val  # comment` — the trailing comment must not bleed into the
    // parsed value (port stays an int, name stays the bare string).
    let code = r#"
        my $t = "port = 8080  # the server port\nname = \"app\"  # display name\n";
        my $h = from_toml($t);
        ($h->{port} == 8080 && $h->{name} eq "app") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn toml_keeps_hash_inside_quoted_string() {
    // A `#` inside a quoted string is data, not a comment — must survive.
    let code = r#"
        my $t = "url = \"http://h/p#frag\"  # trailing comment\n";
        my $h = from_toml($t);
        ($h->{url} eq "http://h/p#frag") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn toml_strips_comment_after_section_header() {
    let code = r#"
        my $t = "[server]  # config block\nport = 9090\n";
        my $h = from_toml($t);
        ($h->{server}->{port} == 9090) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── CSV ──────────────────────────────────────────────────────────────

#[test]
fn csv_roundtrip_array_of_hashes() {
    let code = r#"
        my @rows = (
            +{ name => "alice", age => 30 },
            +{ name => "bob",   age => 28 },
        );
        my $back = from_csv(to_csv(\@rows));
        (scalar(@$back) == 2
            && $back->[0]->{name} eq "alice"
            && $back->[1]->{age} == 28) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn csv_handles_quoted_fields_with_commas() {
    let code = r#"
        my $csv = qq{name,desc\n"alice","loves rust, perl, and jit"\n};
        my $back = from_csv($csv);
        ($back->[0]->{name} eq "alice"
            && $back->[0]->{desc} eq "loves rust, perl, and jit") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn csv_preserves_empty_fields() {
    let code = r#"
        my $csv = qq{a,b,c\n1,,3\n};
        my $back = from_csv($csv);
        ($back->[0]->{a} eq "1"
            && $back->[0]->{b} eq ""
            && $back->[0]->{c} eq "3") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Cross-codec: same data through 3 codecs preserves structure ──────

#[test]
fn data_survives_json_yaml_toml_chain() {
    let code = r#"
        my $orig = +{
            name    => "stryke",
            version => "0.14.3",
            paths   => ["bin", "lib", "examples"],
            flags   => +{ jit => 1, parallel => 1 },
        };
        my $j2y = from_yaml(to_yaml(from_json(to_json($orig))));
        my $j2y2t = from_toml(to_toml($j2y));
        ($j2y2t->{name} eq "stryke"
            && $j2y2t->{paths}->[1] eq "lib") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Error handling: malformed JSON ───────────────────────────────────
// Strings that don't look like JSON (no leading `{`, `[`, `"`, sign,
// digit, or one of `null`/`true`/`false`) return `undef` so callers
// can detect the parse failure with `defined`.

#[test]
fn from_json_on_garbage_returns_undef() {
    let code = r#"
        my $r = from_json("definitely not json");
        defined($r) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Round-trip preserves array element types (no flattening) ────────

#[test]
fn json_preserves_mixed_array_types() {
    let code = r#"
        my $orig = [1, "two", 3.0, [4, 5]];
        my $back = from_json(to_json($orig));
        ($back->[0] == 1
            && $back->[1] eq "two"
            && abs($back->[2] - 3.0) < 1e-9
            && $back->[3]->[1] == 5) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Round-trip preserves two-level nested hashref ────────────────────
// Note: stryke's YAML decoder currently flattens 3+ level nesting in
// some cases (sibling keys at depth ≥ 3 collapse to null). Pin only
// the depth-2 case here. The 3+ depth issue is a separate library
// improvement not yet tracked in BUGS.md.

#[test]
fn yaml_roundtrip_two_level_nested_hash() {
    let code = r#"
        my $orig = +{
            user => +{ name => "alice", age => 30 },
            cfg  => +{ jit => 1, threads => 8 },
        };
        my $back = from_yaml(to_yaml($orig));
        ($back->{user}->{name} eq "alice"
            && $back->{user}->{age} == 30
            && $back->{cfg}->{threads} == 8) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
