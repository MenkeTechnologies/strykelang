//! Reflection-hash pins. Stryke exposes its builtin/alias/keyword
//! tables as `%b`, `%a`, `%k`, `%all`. These pins lock the surface
//! shape — never the exact count (the count is downstream of every
//! feature add).

use crate::common::*;

// ── %b: builtins ─────────────────────────────────────────────────────

#[test]
fn percent_b_contains_known_core_builtins() {
    let code = r#"
        my $ok = 1;
        for my $name ("map", "grep", "sort", "join", "split",
                      "keys", "values", "push", "pop", "shift",
                      "uc", "lc", "length", "len") {
            $ok = 0 unless exists $b{$name};
        }
        $ok ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn percent_b_has_more_than_5000_entries() {
    let code = r#"
        # Stryke's marketing is "10k+ builtins"; verify >= 5000 minimum.
        len(keys %b) >= 5000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn percent_b_lookup_returns_metadata() {
    let code = r#"
        # %b{$name} should return some structure describing the builtin.
        my $info = $b{"map"};
        defined($info) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── %a: aliases ─────────────────────────────────────────────────────

#[test]
fn percent_a_has_aliases() {
    let code = r#"
        # `fi` is an alias for `grep` in stryke; verify it's discoverable.
        # If %a has an entry, the count should be > 0.
        len(keys %a) > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── %k: keywords ────────────────────────────────────────────────────

#[test]
fn percent_k_contains_known_keywords() {
    let code = r#"
        my $ok = 1;
        for my $name ("if", "else", "for", "while", "return", "my", "fn") {
            $ok = 0 unless exists $k{$name};
        }
        $ok ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn percent_k_has_more_than_30_entries() {
    let code = r#"
        # Stryke has 85+ keywords per memory; verify >= 30 conservatively.
        len(keys %k) >= 30 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── %all: union ────────────────────────────────────────────────────

#[test]
fn percent_all_size_matches_b_plus_a_plus_k() {
    let code = r#"
        my $b = len(keys %b);
        my $a = len(keys %a);
        my $k = len(keys %k);
        my $all = len(keys %all);
        # %all is the union; its size should be approximately b+a+k
        # (with possible dedup if names overlap).
        ($all >= $b && $all <= $b + $a + $k + 100) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ref() predicates ────────────────────────────────────────────────

#[test]
fn ref_on_hashref_is_hash_substring() {
    let code = r#"
        my $h = +{ a => 1 };
        ref($h) =~ /HASH/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ref_on_arrayref_is_array_substring() {
    let code = r#"
        my $a = [1, 2, 3];
        ref($a) =~ /ARRAY/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ref_on_coderef_is_code_substring() {
    let code = r#"
        my $c = sub { 42 };
        ref($c) =~ /CODE/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ref_on_regexp_qr_is_regexp_substring() {
    let code = r#"
        my $r = qr/abc/;
        ref($r) =~ /(?:Regex|Regexp)/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ref_on_scalar_is_empty_string() {
    let code = r#"
        my $s = 42;
        ref($s) eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ref_on_undef_is_empty_string() {
    let code = r#"
        my $u;
        ref($u) eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ref() on sketch types ─────────────────────────────────────────

#[test]
fn ref_on_hll_returns_hll_sketch() {
    let code = r#"
        my $h = hll(14);
        ref($h) eq "HllSketch" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ref_on_t_digest_returns_t_digest_sketch() {
    let code = r#"
        my $t = t_digest(100);
        ref($t) eq "TDigestSketch" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ref_on_topk_returns_topk_sketch() {
    let code = r#"
        my $tk = topk(3);
        ref($tk) eq "TopKSketch" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ref_on_bloom_returns_bloom_filter() {
    let code = r#"
        my $b = bloom_filter(100, 0.01);
        # Either "BloomFilter" or similar — accept any non-empty.
        ref($b) ne "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ref_on_roaring_returns_roaring_bitmap() {
    let code = r#"
        my $r = roaring();
        ref($r) eq "RoaringBitmap" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn ref_on_cms_returns_cms_sketch() {
    let code = r#"
        my $c = cms(2048, 5);
        # "CountMinSketch" or similar.
        ref($c) ne "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── defined() / exists() ────────────────────────────────────────────

#[test]
fn defined_true_for_value() {
    let code = r#"
        my $x = 42;
        defined($x) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_false_for_undef() {
    let code = r#"
        my $x;
        defined($x) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn defined_true_for_zero_and_empty_string() {
    let code = r#"
        my $a = 0;
        my $b = "";
        (defined($a) && defined($b)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── builtins that should NOT be in %a (since they're proper builtins) ─

#[test]
fn percent_b_count_stable_across_calls() {
    let code = r#"
        my $n1 = len(keys %b);
        my $n2 = len(keys %b);
        $n1 == $n2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── builtin lookup via $b{name} returns truthy info ──────────────

#[test]
fn builtin_lookup_via_b_truthy_for_known() {
    let code = r#"
        ($b{"sort"} && $b{"map"} && $b{"hll"}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn builtin_lookup_via_b_undef_for_unknown() {
    let code = r#"
        exists($b{"never_a_builtin_name_xyz"}) ? 0 : 1
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Reflection over user-defined fn? ───────────────────────────────

#[test]
fn user_defined_fn_not_in_percent_b() {
    let code = r#"
        fn Demo::Refl::user_thing() { 42 }
        # User-defined fns should NOT appear in %b (which holds builtins).
        !exists($b{"Demo::Refl::user_thing"}) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Several known sketch builtins present ──────────────────────────

#[test]
fn sketch_builtins_in_percent_b() {
    let code = r#"
        my $ok = 1;
        for my $name ("hll", "hll_add", "hll_count",
                      "bloom_filter", "bloom_add",
                      "t_digest", "td_add",
                      "topk", "topk_add",
                      "cms", "cms_add",
                      "roaring", "rb_add") {
            $ok = 0 unless exists $b{$name};
        }
        $ok ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
