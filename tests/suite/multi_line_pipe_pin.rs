//! Pin multi-line `|>` pipeline continuation per
//! `docs/STYLE_GUIDE.md` §13: when a fresh line starts with `|>`,
//! the parser auto-extends the previous statement; pipelines never
//! need `\` line-continuation. Probed against the running
//! interpreter on 2026-05-23.

use crate::common::*;

#[test]
fn two_stage_continuation_with_leading_pipe() {
    let code = r#"
        my $r = (1..10)
            |> grep { _ > 5 }
            |> sum;
        $r
    "#;
    // 6+7+8+9+10 = 40
    assert_eq!(eval_int(code), 40);
}

#[test]
fn three_stage_continuation_with_array_result() {
    let code = r#"
        my @r = (1..5)
            |> map { _ * 10 }
            |> grep { _ > 20 };
        join(",", @r) eq "30,40,50" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pipeline_continues_through_blank_indented_stages() {
    let code = r#"
        my $r = "  hello  "
            |> trim
            |> uc
            |> len;
        $r
    "#;
    assert_eq!(eval_int(code), 5);
}

#[test]
fn pipeline_continues_with_block_arg_stage() {
    let code = r#"
        my $r = (1..6)
            |> map { _ * _ }
            |> sum;
        # 1 + 4 + 9 + 16 + 25 + 36 = 91
        $r
    "#;
    assert_eq!(eval_int(code), 91);
}

#[test]
fn pipeline_after_assignment_break() {
    // RHS may also start on a fresh line.
    let code = r#"
        my $r =
            (1..10)
            |> sum;
        $r
    "#;
    assert_eq!(eval_int(code), 55);
}

#[test]
fn pipeline_terminates_at_next_statement() {
    // Two consecutive pipelines must each be a separate statement.
    let code = r#"
        my $a = (1..5)
            |> sum;
        my $b = (1..3)
            |> sum;
        $a + $b
    "#;
    // (1+2+3+4+5) + (1+2+3) = 15 + 6 = 21
    assert_eq!(eval_int(code), 21);
}

#[test]
fn pipeline_with_grep_then_sort_continuation() {
    let code = r#"
        my @r = (5, 1, 4, 1, 5, 9, 2, 6, 5)
            |> grep { _ > 2 }
            |> sort
            |> uniq;
        join(",", @r) eq "4,5,6,9" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pipeline_continuation_inside_fn_body() {
    // The arrayref must be deref'd into list context first — `map`
    // applied to a scalar arrayref treats it as a singleton list of
    // ARRAY refs, which numifies to 0 under arithmetic.
    let code = r#"
        fn Demo::Mlp::compute($xs) {
            @$xs
                |> map { _ * _ }
                |> sum
        }
        Demo::Mlp::compute([1, 2, 3, 4])
    "#;
    // 1 + 4 + 9 + 16 = 30
    assert_eq!(eval_int(code), 30);
}

#[test]
fn pipeline_continuation_short_one_per_line() {
    // The most aggressive vertical-stack form — every stage on
    // its own line.
    let code = r#"
        my $r = "stryke"
            |> uc
            |> rev;
        $r eq "EKYRTS" ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}

#[test]
fn pipeline_continuation_in_complex_expr_context() {
    // Pipeline as a sub-expression — must still terminate cleanly
    // before the trailing `+ 1`.
    let code = r#"
        my $r = ((1..5)
            |> sum) + 100;
        $r == 115 ? 1 : 0
    "#;
    assert_eq!(eval_int(code), 1);
}
