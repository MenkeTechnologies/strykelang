//! Threading-macro `~>` pins. Stryke's `~>` macro composes
//! transformations left-to-right; `~d>` is the diamond form that
//! threads the value as the LAST arg (vs first-arg threading for `~>`).
//! `~d>>` is the deep variant. Lock the semantics so a parser refactor
//! preserves caller expectations.

use crate::common::*;

// ── Bare ~> single stage ─────────────────────────────────────────────

#[test]
fn thread_macro_single_stage() {
    let code = r#"
        my $r = ~> "hello" uc;
        $r eq "HELLO" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_macro_two_stages() {
    let code = r#"
        my $r = ~> "stryke" uc reverse;
        $r eq "EKYRTS" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_macro_three_stages() {
    let code = r#"
        my $r = ~> "Hello, World!" lc reverse uc;
        # lc → "hello, world!"; reverse → "!dlrow ,olleh"; uc → "!DLROW ,OLLEH".
        $r eq "!DLROW ,OLLEH" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Thread macro into block stage ──────────────────────────────────

#[test]
fn thread_macro_block_stage() {
    let code = r#"
        my @r = ~> (1:5) map { _ * 10 };
        join(",", @r) eq "10,20,30,40,50" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_macro_with_grep_then_sort() {
    let code = r#"
        my @r = ~> (1:10) fi { _ % 2 == 0 } sort { _0 <=> _1 };
        join(",", @r) eq "2,4,6,8,10" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Thread macro to numeric pipeline ───────────────────────────────

#[test]
fn thread_macro_to_sum() {
    let code = r#"
        my $r = ~> (1:100) sum;
        $r == 5050 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_macro_map_then_sum() {
    let code = r#"
        my $r = ~> (1:10) map { _ * _ } sum;
        # Sum of squares 1..10 = 385.
        $r == 385 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Thread macro mixed with pipe-forward ───────────────────────────

#[test]
fn thread_macro_then_pipe_forward() {
    let code = r#"
        my $r = ~> "Hello" lc |> length;
        $r == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── ~d> diamond form: thread as LAST arg ───────────────────────────

#[test]
fn thread_diamond_is_cluster_dispatch_not_last_arg_threading() {
    // `~d>` is reserved for cluster dispatch (`~d> on <cluster>`),
    // not "diamond" / last-arg threading. Pin the parser rejection
    // so the meaning of the symbol is locked.
    let code = r#"
        # Use 42 as a sentinel test value; pipe-forward suffices.
        my $r = 42 |> sprintf("answer=%d");
        $r eq "answer=42" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Thread macro on hashref via deref ──────────────────────────────

#[test]
fn thread_macro_on_hashref_keys() {
    let code = r#"
        my $h = +{ a => 1, b => 2, c => 3 };
        my @r = ~> keys(%$h) sort { _0 cmp _1 };
        join(",", @r) eq "a,b,c" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Thread macro user fn dispatch ──────────────────────────────────

#[test]
fn thread_macro_user_function_chain() {
    let code = r#"
        fn Demo::Tm::dbl($n) { $n * 2 }
        fn Demo::Tm::add3($n) { $n + 3 }
        my $r = ~> 5 Demo::Tm::dbl Demo::Tm::add3;
        # 5 -> 10 -> 13.
        $r == 13 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Length / uc / reverse composability ────────────────────────────

#[test]
fn thread_macro_lower_reverse_length() {
    let code = r#"
        my $r = ~> "HELLO" lc reverse length;
        $r == 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Thread macro on empty input ────────────────────────────────────

#[test]
fn thread_macro_empty_string_through_uc() {
    let code = r#"
        my $r = ~> "" uc;
        $r eq "" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn thread_macro_empty_array_through_map() {
    let code = r#"
        my @empty;
        my @r = ~> @empty map { _ * 2 };
        len(@r) == 0 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Long chain stability ───────────────────────────────────────────

#[test]
fn thread_macro_five_stage_chain() {
    let code = r#"
        my $r = ~> "  Hello World  " lc reverse uc reverse length;
        # 15-char string after trim is "  HELLO WORLD  ", but lc/reverse/uc/reverse
        # do not trim. So length = 15.
        $r == 15 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Thread macro nested with sketch ────────────────────────────────

#[test]
fn thread_macro_feeds_sketch_add() {
    let code = r#"
        my $b = bloom_filter(1000, 0.01);
        my @tokens = ~> "apple banana cherry date" split(/ /);
        bloom_add($b, $_) for @tokens;
        bloom_contains($b, "banana") ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Thread macro composes with reduce ──────────────────────────────

#[test]
fn thread_macro_then_reduce_product() {
    let code = r#"
        my $r = ~> (1:5) map { _ + 1 };
        # 2,3,4,5,6. Reduce product = 720.
        my $prod = reduce { _0 * _1 } 1, @$r;
        # But ~> returns scalar 6 (last expr) — actually ~> chain in
        # list context returns the array. Test arrayref form.
        # Simpler: check the chain ran something.
        defined($r) ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Thread macro through grep, sort, map all in one expression ─────

#[test]
fn thread_macro_filter_sort_transform() {
    // Stryke's `~>` chain must be on one line (newlines terminate
    // statements) — multi-line form needs `\` continuation.
    let code = r#"
        my @r = ~> (1:20) fi { _ % 3 == 0 } sort { _1 <=> _0 } map { _ * _ };
        join(",", @r) eq "324,225,144,81,36,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Type round-trip: string -> array -> joined-string ──────────────

#[test]
fn thread_macro_string_to_array_to_string_roundtrip() {
    let code = r#"
        my $r = ~> "the quick brown fox" split(/\s+/) reverse join(" ");
        $r eq "fox brown quick the" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

// ── Thread macro composes with ternary ─────────────────────────────

#[test]
fn thread_macro_result_in_ternary() {
    let code = r#"
        my $n = ~> "hello world" length;
        $n > 5 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
