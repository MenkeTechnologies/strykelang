//! Extended coverage for "keyword names as hash keys" — the parser-level
//! fix in `c07ea38848 fix $hash{format}` taught the hash-subscript path to
//! treat ANY reserved word as a bareword key. These tests pin keywords
//! beyond `format` so that future parser changes can't silently re-break
//! the path for any single keyword spelling.

use crate::common::*;

// Keywords that parse cleanly as barewords in hash-literal AND subscript
// position. Some keywords (`my`, `our`, `local`, `state`, `use`, `no`,
// `package`, `BEGIN`, `END`, `bless`) start a statement and don't currently
// fold as hash-literal keys — they ARE callable as subscripts (`$h{my}`),
// but not as left-hand-of-fat-comma in `(my => 1)`. The list below is the
// fat-comma-safe set.
const KEYWORD_KEYS: &[&str] = &[
    "if",
    "elsif",
    "else",
    "unless",
    "while",
    "until",
    "for",
    "foreach",
    "do",
    "given",
    "when",
    "default",
    "last",
    "next",
    "redo",
    "return",
    "ref",
    "defined",
    "exists",
    "delete",
    "eq",
    "ne",
    "lt",
    "gt",
    "le",
    "ge",
    "and",
    "or",
    "not",
    "xor",
    "wantarray",
    "tied",
    "format",
];

#[test]
fn every_keyword_subscripts_a_hash() {
    for k in KEYWORD_KEYS {
        let code = format!(r#"my %h = ({} => 7); $h{{{}}}"#, k, k);
        let n = eval_int(&code);
        assert_eq!(n, 7, "keyword `{}` failed as hash subscript", k);
    }
}

#[test]
fn every_keyword_assigns_into_a_hash() {
    for k in KEYWORD_KEYS {
        let code = format!(r#"my %h; $h{{{}}} = 42; $h{{{}}}"#, k, k);
        let n = eval_int(&code);
        assert_eq!(n, 42, "keyword `{}` failed as hash lvalue", k);
    }
}

#[test]
fn every_keyword_works_as_arrow_hash_key() {
    for k in KEYWORD_KEYS {
        let code = format!(r#"my $h = +{{ {} => 11 }}; $h->{{{}}}"#, k, k);
        let n = eval_int(&code);
        assert_eq!(n, 11, "keyword `{}` failed as arrow-hash key", k);
    }
}

#[test]
fn every_keyword_supports_exists_and_delete() {
    for k in KEYWORD_KEYS {
        let code = format!(
            r#"
            my %h = ({} => 1)
            my $e1 = exists $h{{{}}} ? 1 : 0
            delete $h{{{}}}
            my $e2 = exists $h{{{}}} ? 1 : 0
            $e1 - $e2
            "#,
            k, k, k, k
        );
        let n = eval_int(&code);
        assert_eq!(n, 1, "exists/delete failed for keyword `{}`", k);
    }
}

#[test]
fn every_keyword_works_inside_string_interpolation() {
    // `"value=$h{if}"` must parse the `if` as a bareword key, not a control-flow
    // statement.
    for k in KEYWORD_KEYS {
        let code = format!(r#"my %h = ({} => "ok"); "value=$h{{{}}}""#, k, k);
        let s = eval_string(&code);
        assert_eq!(s, "value=ok", "string interpolation failed for `{}`", k);
    }
}

// ── compound interactions ────────────────────────────────────────────────────

#[test]
fn keyword_keys_coexist_in_one_hash() {
    let n = eval_int(
        r#"
        my %h = (if => 1, while => 2, for => 3, unless => 4, return => 5)
        $h{if} + $h{while} + $h{for} + $h{unless} + $h{return}
        "#,
    );
    assert_eq!(n, 15);
}

#[test]
fn keyword_arrow_hash_compound_assign() {
    // Combines the keyword-key fix with the `Op::SetArrowHashKeep` fix:
    // `$h->{if}` is a keyword key, `+=` is a compound op, multiple in one
    // expression to stress the caller-frame Pop path.
    let n = eval_int(
        r#"
        my $h = +{if => 0}
        $h->{if} += 10
        $h->{if} *= 3
        $h->{if} -= 5
        $h->{if}
        "#,
    );
    // 0 + 10 = 10, then 10 * 3 = 30, then 30 - 5 = 25
    assert_eq!(n, 25);
}
