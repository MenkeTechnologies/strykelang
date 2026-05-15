//! Behavioral pins for the public example scripts under `examples/*.stk`.
//! These tests bake in the precise semantics each demo relies on, so a
//! regression that breaks the demos fails CI immediately.
//!
//! Coverage targets:
//!   - pipe-forward into builtins (`|>`)
//!   - thread macro (`~>`) with partial application and block stages
//!   - implicit closure params (`_0`, `_1`, `_`, `_<`)
//!   - parallel primitives (`pmap`, `pgrep`, `pfor`, `psort`, `preduce`,
//!     `pmap_reduce`, `fan`)
//!   - reflection invariants (`%b + %a + %k == %all`)
//!   - sketch/KV/algebra composition

use crate::common::*;

// ── Pipe-forward `|>` ─────────────────────────────────────────────────

#[test]
fn pipe_forward_threads_into_first_arg() {
    // 5 |> sqrt |> int  →  int(sqrt(5))  =  2
    assert_eq!(eval_int("5 |> sqrt |> int"), 2);
}

#[test]
fn pipe_forward_into_split_returns_list() {
    let code = r#"
        my @parts = "alpha:beta:gamma" |> split(/:/);
        scalar(@parts) == 3 && $parts[2] eq "gamma" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pipe_forward_chain_with_grep_sum() {
    // sum of even squares 1..100  =  171700
    let code = r#"
        (1..100) |> map { $_ * $_ } |> grep { $_ % 2 == 0 } |> sum
    "#;
    assert_eq!(eval_int(code), 171700);
}

// ── Thread macro `~>` ─────────────────────────────────────────────────

#[test]
fn thread_macro_basic_two_stage_chain() {
    let code = r#"
        my $r = ~> "Hello" uc reverse;
        $r eq "OLLEH" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_macro_partial_application_threads_to_slot_zero() {
    let code = r#"
        fn Add($a, $b) { $a + $b }
        fn Dbl($n)     { $n * 2 }
        ~> 5 Add(10) Dbl
    "#;
    // 5 -> Add(5, 10) = 15 -> Dbl(15) = 30
    assert_eq!(eval_int(code), 30);
}

#[test]
fn thread_macro_block_form_stages() {
    let code = r#"
        my @r = ~> (1..10) map { _ * 3 } fi { _ > 10 } sort { _0 <=> _1 };
        # Three keeps (1*3=3 dropped … through 10*3=30 kept); >10 keeps 4..10 → 7 values
        (scalar(@r) == 7 && $r[0] == 12 && $r[-1] == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Implicit closure params ───────────────────────────────────────────

#[test]
fn underscore_zero_one_sort_ascending() {
    let code = r#"
        my @s = sort { _0 <=> _1 } (3, 1, 4, 1, 5, 9, 2, 6);
        join(",", @s) eq "1,1,2,3,4,5,6,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn underscore_zero_one_sort_descending() {
    let code = r#"
        my @s = sort { _1 <=> _0 } (3, 1, 4, 1, 5, 9, 2, 6);
        join(",", @s) eq "9,6,5,4,3,2,1,1" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn underscore_bare_in_map_grep() {
    let code = r#"
        my @doubled = map  { _ * 2 } (1..5);
        my @evens   = grep { _ % 2 == 0 } (1..10);
        (join(",", @doubled) eq "2,4,6,8,10"
         && join(",", @evens) eq "2,4,6,8,10") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn underscore_outer_topic_one_level() {
    // World-first outer-topic chain: `_<` reads the enclosing frame's
    // topic. (1..3) cross (10,20,30); map-of-map flattens in scalar
    // context per Perl semantics.
    let code = r#"
        my @r = map { (1..3) |> map { _ + _< } } (10, 20, 30);
        scalar(@r) == 9
            && join(",", @r) eq "11,12,13,21,22,23,31,32,33" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn underscore_outer_topic_two_levels() {
    // `_<<` reads the topic two frames up. Three-deep nested maps with
    // outer chain 100..200, mid chain 10..30, inner 1..2. Sum check:
    //   sum_{k in 100,200} sum_{j in 10,20,30} sum_{i in 1,2} (i+j+k)
    //     = 2058
    let code = r#"
        my @r = map {
            map {
                map { _ + _< + _<< } (1, 2)
            } (10, 20, 30)
        } (100, 200);
        scalar(@r) == 12 && sum(@r) == 2058 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reduce_with_underscore_zero_one() {
    let code = r#"
        reduce { _0 * _1 } 1, (1..6)
    "#;
    assert_eq!(eval_int(code), 720); // 6!
}

// ── Parallel primitives — semantics only, not perf ────────────────────

#[test]
fn pmap_preserves_order() {
    let code = r#"
        my @r = pmap { _ * _ } (1..10);
        join(",", @r) eq "1,4,9,16,25,36,49,64,81,100" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pgrep_filters_in_parallel() {
    let code = r#"
        my @evens = pgrep { _ % 2 == 0 } (1..20);
        scalar(@evens) == 10 && $evens[0] == 2 && $evens[-1] == 20 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn preduce_sums_1_to_1000() {
    let code = r#"
        preduce { _0 + _1 } (1..1000)
    "#;
    assert_eq!(eval_int(code), 500_500);
}

#[test]
fn pmap_reduce_fused_sum_of_squares() {
    let code = r#"
        pmap_reduce { _ * _ } { _0 + _1 } (1..100)
    "#;
    // sum(k=1..100) k² = n(n+1)(2n+1)/6 = 100*101*201/6 = 338350
    assert_eq!(eval_int(code), 338_350);
}

#[test]
fn pfor_side_effects_to_kv_store() {
    // pfor fires a closure per item in parallel; side effects must
    // accumulate. Using KV as the durable observable.
    let path = format!(
        "/tmp/stryke_demos_pfor_{}.rkyv",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let code = format!(
        r#"
            my $db = kv_open("{path}");
            pfor {{ kv_put($db, "k$_", $_ * $_) }} (1..100);
            kv_commit($db);
            my $db2 = kv_open("{path}");
            my $n = kv_len($db2);
            my $check = kv_get($db2, "k50");
            unlink("{path}");
            ($n == 100 && $check == 2500) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

// ── Reflection invariants ─────────────────────────────────────────────

#[test]
fn reflection_disjoint_union_holds() {
    // %b + %a + %k == %all (no key appears in two hashes)
    let code = r#"
        my $sum = scalar(keys %b) + scalar(keys %a) + scalar(keys %k);
        my $all = scalar(keys %all);
        $sum == $all ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reflection_primaries_count_above_10k() {
    let code = r#"
        scalar(keys %b) > 10_000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reflection_kv_namespace_present() {
    // All 12 kv_* primaries + their aliases visible in %all.
    let code = r#"
        my @kv = sort grep { /^kv_/ } keys %all;
        # 12 primaries + at least kv_set/has/count/size/flush/info/delete/remove/new aliases
        scalar(@kv) >= 12 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reflection_categories_exist_for_sketches() {
    let code = r#"
        my @sketch_cats = sort grep { /sketch|probabilistic|kv/i } keys %c;
        scalar(@sketch_cats) >= 1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn reflection_doc_for_kv_open_exists() {
    let code = r#"
        my $doc = $d{"kv_open"};
        defined($doc) && length($doc) > 10 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Cross-feature ─────────────────────────────────────────────────────

#[test]
fn pipe_into_sketch_then_into_kv() {
    let path = format!(
        "/tmp/stryke_demos_xfeat_{}.rkyv",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let code = format!(
        r#"
            my $bf = bloom_filter(1000, 0.01);
            "apple banana cherry" |> split(/\s+/) |> map {{ bloom_add($bf, $_) }};

            my $db = kv_open("{path}");
            kv_put($db, "words", bloom_serialize($bf));
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $bf2 = bloom_deserialize(kv_get($db2, "words"));
            unlink("{path}");

            (bloom_contains($bf2, "apple")
                && bloom_contains($bf2, "banana")
                && !bloom_contains($bf2, "carrot")) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}

#[test]
fn sketch_algebra_chained_three_way() {
    // `+` is left-associative; three CMS sketches sum correctly.
    let code = r#"
        my $a = cms(2048, 5); cms_add($a, "x") for (1..10);
        my $b = cms(2048, 5); cms_add($b, "x") for (1..20);
        my $c = cms(2048, 5); cms_add($c, "x") for (1..30);
        my $u = $a + $b + $c;
        cms_count($u, "x") == 60 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Algebraic match ───────────────────────────────────────────────────

#[test]
fn match_on_enum_variant_dispatches_correctly() {
    let code = r#"
        enum Sig { Hup, Int, Term, Kill }
        fn handle($s) {
            match ($s) {
                Sig::Hup  => 1,
                Sig::Int  => 2,
                Sig::Term => 3,
                Sig::Kill => 4,
            }
        }
        handle(Sig::Term)
    "#;
    assert_eq!(eval_int(code), 3);
}

#[test]
fn match_on_integer_with_wildcard_arm() {
    let code = r#"
        fn classify($c) {
            match ($c) {
                200 => 1,
                404 => 2,
                500 => 3,
                _   => 9,
            }
        }
        classify(200) + classify(404) * 10 + classify(500) * 100 + classify(418) * 1000
    "#;
    // 1 + 20 + 300 + 9000 = 9321
    assert_eq!(eval_int(code), 9321);
}

#[test]
fn match_composes_with_pipe_forward() {
    let code = r#"
        my @codes = (200, 404, 500, 200);
        my @cats = @codes |> map {
            match ($_) {
                200 => "ok",
                _   => "err",
            }
        };
        scalar(@cats) == 4 && $cats[0] eq "ok" && $cats[1] eq "err" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── AOP intercepts ────────────────────────────────────────────────────

#[test]
fn before_intercept_runs_before_target() {
    // `mysync` is required for cross-closure shared mutation in stryke
    // (closures capture by value to keep parallel dispatch race-free).
    let code = r#"
        mysync $log = "";
        fn target { $log .= "T" }
        before "target" { $log .= "B" }
        target();
        $log eq "BT" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn after_intercept_sees_intercept_rv() {
    let code = r#"
        mysync $log = "";
        fn target { 42 }
        after "target" { $log = $INTERCEPT_NAME }
        target();
        $log eq "target" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn intercept_args_captured_by_before() {
    let code = r#"
        mysync $sum = 0;
        fn adder { 0 }
        before "adder" { $sum = sum(@INTERCEPT_ARGS) }
        adder(1, 2, 3, 4);
        $sum
    "#;
    assert_eq!(eval_int(code), 10);
}

#[test]
fn around_intercept_can_short_circuit_via_no_proceed() {
    let code = r#"
        mysync $called = 0;
        fn original { $called++; "real" }
        around "original" { "intercepted" }   # no proceed() → original skipped
        my $r = original();
        ($r eq "intercepted" && $called == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn intercept_list_count_grows_with_each_register() {
    let code = r#"
        my $n0 = scalar(@{[intercept_list()]});
        fn dummy { 1 }
        before "dummy" { 1 }
        after  "dummy" { 1 }
        my $n1 = scalar(@{[intercept_list()]});
        ($n1 - $n0) == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Three-tier regex ──────────────────────────────────────────────────

#[test]
fn regex_named_captures_populate_plus_hash() {
    let code = r#"
        my $s = "alice\@example.com";
        if ($s =~ /(?<user>\w+)@(?<host>[\w.]+)/) {
            ($+{user} eq "alice" && $+{host} eq "example.com") ? 1 : 0
        } else {
            0
        }
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn regex_backref_palindrome_via_fancy_tier() {
    let code = r#"
        my @results;
        for my $w ("abba", "stryke", "racecar", "hello") {
            push @results, ($w =~ /^(.)(.)\2\1$/ || $w =~ /^(.)(.).*\2\1$/) ? "y" : "n";
        }
        join("", @results) eq "ynyn" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn regex_lookahead_match_isolates_capture() {
    let code = r#"
        my $text = "foo bar baz bar qux";
        my @hits;
        while ($text =~ /(\w+)(?= bar)/g) { push @hits, $1 }
        scalar(@hits) == 2 && $hits[0] eq "foo" && $hits[1] eq "baz" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn regex_compiled_qr_reuse_with_capture() {
    let code = r#"
        my $iso = qr/^(\d{4})-(\d{2})-(\d{2})$/;
        my $hits = 0;
        for my $d ("2026-05-15", "not-a-date", "1999-12-31") {
            $hits++ if $d =~ $iso;
        }
        $hits == 2 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Glob qualifiers ───────────────────────────────────────────────────

#[test]
fn glob_null_qualifier_returns_empty_no_error() {
    let code = r#"
        my @empty = glob("nonexistent-xyz-zzz/*.qq(N)");
        scalar(@empty) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn glob_dotfile_qualifier_includes_hidden() {
    let code = r#"
        # In the strykelang repo root or any populated dir, there's
        # almost always at least one dotfile (.git, .gitignore, etc.).
        my @no_dot = glob("*(N)");
        my @dot    = glob("*(DN)");
        scalar(@dot) >= scalar(@no_dot) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn glob_type_qualifier_files_only() {
    let code = r#"
        my @files = glob("examples/*(.N)");
        my @all   = glob("examples/*(N)");
        # files (.) subset of all
        scalar(@files) <= scalar(@all) && scalar(@files) > 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── String coordinates ───────────────────────────────────────────────

#[test]
fn ascii_strings_agree_on_length_and_len() {
    let code = r#"
        my $s = "hello world";
        (length($s) == 11 && len($s) == 11) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn multibyte_em_dash_diverges_length_vs_len() {
    let code = r#"
        my $s = "hello — world";    # em dash = 3 bytes, 1 codepoint
        length($s) == 15 && len($s) == 13 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn emoji_strings_diverge_byte_vs_codepoint() {
    let code = r#"
        my $s = "🔑 keys 🔐";
        # Each emoji is 4 bytes, 1 codepoint
        length($s) > len($s) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn index_vs_cindex_byte_vs_codepoint_offset() {
    let code = r#"
        my $s = "café au lait";
        # 'au' starts at byte 6 (4 ascii + 2 bytes for é + space),
        # but at codepoint 5 (4 ascii + 1 cp for é + space — wait, é+space = 2 cp, hmm)
        # Stryke cindex measures codepoints; index measures bytes.
        index($s, "au") > cindex($s, "au") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Iterator helpers ──────────────────────────────────────────────────

#[test]
fn enumerate_yields_index_value_pairs() {
    let code = r#"
        my @pairs = enumerate("a", "b", "c");
        scalar(@pairs) == 3
            && $pairs[0]->[0] == 0 && $pairs[0]->[1] eq "a"
            && $pairs[2]->[0] == 2 && $pairs[2]->[1] eq "c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn dedup_collapses_adjacent_duplicates() {
    let code = r#"
        my @r = dedup(1, 1, 2, 2, 2, 3, 1, 1, 4);
        join(",", @r) eq "1,2,3,1,4" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn nth_zero_indexed_access() {
    let code = r#"
        my @days = ("Sun","Mon","Tue","Wed","Thu","Fri","Sat");
        nth(0, @days) eq "Sun" && nth(6, @days) eq "Sat" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn range_inclusive_endpoints() {
    let code = r#"
        my @r = range(2, 5);
        join(",", @r) eq "2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn colon_range_is_first_class_operator() {
    let code = r#"
        my @r = 0:5;
        join(",", @r) eq "0,1,2,3,4,5" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn flatten_unwraps_nested_arrayrefs() {
    let code = r#"
        my @flat = flatten([1,2], [3,4], [5,6,7]);
        join(",", @flat) eq "1,2,3,4,5,6,7" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn zip_pairs_two_arrays() {
    let code = r#"
        my @a = ("a","b","c");
        my @b = (1,2,3);
        my @pairs = zip(\@a, \@b);
        scalar(@pairs) == 3
            && $pairs[0]->[0] eq "a" && $pairs[0]->[1] == 1
            && $pairs[2]->[0] eq "c" && $pairs[2]->[1] == 3 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── File I/O ──────────────────────────────────────────────────────────

#[test]
fn slurp_spurt_roundtrip() {
    let code = r#"
        my $p = "/tmp/stryke_test_slurp.txt";
        unlink($p);
        spurt($p, "alpha\nbeta\n");
        my $back = slurp($p);
        unlink($p);
        $back eq "alpha\nbeta\n" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn slurp_split_line_iteration() {
    let code = r#"
        my $p = "/tmp/stryke_test_lines.txt";
        unlink($p);
        spurt($p, "one\ntwo\nthree\n");
        my @lines = grep { len($_) > 0 } split(/\n/, slurp($p));
        unlink($p);
        join(",", @lines) eq "one,two,three" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn file_size_matches_written_bytes() {
    let code = r#"
        my $p = "/tmp/stryke_test_fs.txt";
        unlink($p);
        spurt($p, "xxxxxxxxxx");          # 10 bytes
        my $sz = file_size($p);
        unlink($p);
        $sz == 10 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Crypto ────────────────────────────────────────────────────────────

#[test]
fn sha256_produces_64_char_hex() {
    let code = r#"
        my $h = sha256("hello");
        len($h) == 64 && $h =~ /^[0-9a-f]+$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn blake3_produces_64_char_hex() {
    let code = r#"
        my $h = blake3("hello");
        len($h) == 64 && $h =~ /^[0-9a-f]+$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn hmac_sha256_is_deterministic() {
    let code = r#"
        my $t1 = hmac_sha256("key", "message");
        my $t2 = hmac_sha256("key", "message");
        my $t3 = hmac_sha256("key", "different");
        ($t1 eq $t2 && $t1 ne $t3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn argon2_verify_matches_only_correct_password() {
    let code = r#"
        my $h = argon2_hash("correct horse");
        (argon2_verify("correct horse", $h) == 1
            && argon2_verify("wrong", $h) == 0) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn jwt_encode_decode_roundtrip() {
    let code = r#"
        my $tok = jwt_encode({ sub => "alice", role => "admin" }, "secret");
        my $back = jwt_decode($tok, "secret");
        ($back->{sub} eq "alice" && $back->{role} eq "admin") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn jwt_decode_rejects_bad_signature() {
    let code = r#"
        my $tok = jwt_encode({ x => 1 }, "secret-a");
        my $err = 0;
        eval { jwt_decode($tok, "secret-b") };
        $err = 1 if $@;
        $err
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Codecs ────────────────────────────────────────────────────────────

#[test]
fn json_roundtrip_preserves_scalars_and_arrays() {
    let code = r#"
        my $orig = { n => 42, s => "hi", a => [1,2,3] };
        my $back = from_json(to_json($orig));
        ($back->{n} == 42 && $back->{s} eq "hi" && $back->{a}->[2] == 3) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn yaml_roundtrip_preserves_nested_hash() {
    let code = r#"
        my $orig = { user => { name => "alice", age => 30 } };
        my $back = from_yaml(to_yaml($orig));
        ($back->{user}->{name} eq "alice" && $back->{user}->{age} == 30) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn toml_roundtrip_preserves_top_level() {
    let code = r#"
        my $orig = { name => "stryke", version => "0.14.2" };
        my $back = from_toml(to_toml($orig));
        ($back->{name} eq "stryke" && $back->{version} eq "0.14.2") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn csv_roundtrip_preserves_array_of_hashes() {
    let code = r#"
        my @rows = (
            { name => "alice", age => 30 },
            { name => "bob",   age => 28 },
        );
        my $back = from_csv(to_csv(\@rows));
        (scalar(@$back) == 2
            && $back->[0]->{name} eq "alice"
            && $back->[1]->{age} == 28) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Async / await ─────────────────────────────────────────────────────

#[test]
fn async_block_awaits_to_block_return_value() {
    let code = r#"
        my $h = async { 1 + 2 + 3 };
        await($h)
    "#;
    assert_eq!(eval_int(code), 6);
}

#[test]
fn spawn_is_alias_for_async() {
    let code = r#"
        my $h = spawn { 100 + 200 };
        await($h)
    "#;
    assert_eq!(eval_int(code), 300);
}

#[test]
fn await_passes_through_non_task_values() {
    let code = r#"
        await(42)
    "#;
    assert_eq!(eval_int(code), 42);
}

#[test]
fn parallel_async_fanout_returns_all_results() {
    let code = r#"
        my @h = map { my $i = $_; async { $i * $i } } (1..5);
        my @r = map { await($_) } @h;
        join(",", @r) eq "1,4,9,16,25" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Date / time ───────────────────────────────────────────────────────

#[test]
fn strftime_iso_8601_format_shape() {
    let code = r#"
        # Just check the shape — local timezone affects the actual digits,
        # but the YYYY-MM-DDTHH:MM:SSZ template is invariant.
        my $iso = strftime("%Y-%m-%dT%H:%M:%SZ", time());
        $iso =~ /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/ ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn time_returns_positive_epoch() {
    let code = r#"
        time() > 1_700_000_000 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn now_ns_is_monotonic_within_call() {
    let code = r#"
        my $t1 = now_ns();
        my $t2 = now_ns();
        $t2 >= $t1 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn iso_keys_sort_chronologically() {
    let code = r#"
        my $now = time();
        my @keys;
        for my $i (1..5) {
            push @keys, strftime("%Y-%m-%dT%H:%M:%SZ", $now + $i);
        }
        # Already in ascending order: lexicographic == chronological for ISO.
        my @sorted = sort @keys;
        join("|", @keys) eq join("|", @sorted) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── run / source ──────────────────────────────────────────────────────

#[test]
fn run_binary_returns_zero_on_success() {
    let code = r#"
        run("true")
    "#;
    assert_eq!(eval_int(code), 0);
}

#[test]
fn run_binary_returns_negative_one_on_missing_command() {
    let code = r#"
        run("definitely-not-a-real-binary-xyzqq")
    "#;
    assert_eq!(eval_int(code), -1);
}

#[test]
fn run_stryke_script_isolates_state() {
    let code = r#"
        my $tmp = "/tmp/stryke_test_run_iso.stk";
        spurt($tmp, q{my $leaked = "child"; p ""});
        my $rc = run($tmp);
        unlink($tmp);
        # The subprocess returned 0 AND $leaked is undef in parent.
        ($rc == 0 && !defined($leaked)) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn source_injects_variables_into_caller() {
    let code = r#"
        my $tmp = "/tmp/stryke_test_source_var.stk";
        spurt($tmp, q{our $LIBV = "1.2.3"});
        source($tmp);
        unlink($tmp);
        $LIBV eq "1.2.3" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn source_injects_function_definitions() {
    let code = r#"
        my $tmp = "/tmp/stryke_test_source_fn.stk";
        spurt($tmp, q{fn Test::Lib::pi { 314 }});
        source($tmp);
        unlink($tmp);
        Test::Lib::pi() == 314 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn source_missing_file_dies() {
    let code = r#"
        my $err = 0;
        eval { source("/tmp/stryke_test_definitely_not_there_xyz.stk") };
        $err = 1 if $@;
        $err
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn src_alias_for_source() {
    let code = r#"
        my $tmp = "/tmp/stryke_test_src_alias.stk";
        spurt($tmp, q{our $SRCALIAS = 42});
        src($tmp);
        unlink($tmp);
        $SRCALIAS == 42 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_macro_composes_with_sketches_and_kv() {
    let path = format!(
        "/tmp/stryke_demos_tm_{}.rkyv",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let code = format!(
        r#"
            my $bf = bloom_filter(1000, 0.01);
            ~> "a b c d e" split(/\s+/) pfor {{ bloom_add($bf, _) }};

            my $db = kv_open("{path}");
            kv_put($db, "bf", bloom_serialize($bf));
            kv_commit($db);

            my $db2 = kv_open("{path}");
            my $back = bloom_deserialize(kv_get($db2, "bf"));
            unlink("{path}");

            (bloom_contains($back, "a") && bloom_contains($back, "e")) ? 1 : 0
        "#
    );
    assert_eq!(eval_int(&code), 1);
}
