//! Behavior-pinning batch CR (2026-05-09): special-factorial helpers (`lgamma`, **`beta_fn` / `lbeta` /
//! `pbeta`**, **`betainc`**, **`qbeta`**, **`inverse_beta_regularized`**, **`inverse_gamma_regularized`**),
//! **Bessel-related** `digamma` / `trigamma` / **`polygamma`**, **list guards** **`take_while` / `drop_while` /
//! `reject`**, **charset-prefix** **`string_take_while` / `string_drop_while`** (**BUG-165** — not
//! predicate callbacks), **`nth`** on iterators vs inline **`ARRAYREF`** (**BUG-166**), merges,
//! **bitwise** ops, **radix** helpers, combinatorics (**`binomial`**, **`stirling2`**, **`euler_totient`**),
//! **crypto** / **approx** utilities.

use crate::common::*;

#[test]
fn hmac_sha256_ascii_key_message_cr() {
    assert_eq!(
        eval_string(r#"hmac_sha256("key", "data")"#),
        "5031fe3d989c6d1537a013fa6e739da23463fdaec3b70137d828e36ace221bd0"
    );
}

#[test]
fn lgamma_five_cr() {
    assert_eq!(eval_string(r#"sprintf("%.10g", lgamma(5))"#), "3.17805383");
}

#[test]
fn digamma_one_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", digamma(1))"#),
        "-0.5772156649"
    );
}

#[test]
fn trigamma_one_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", trigamma(1))"#),
        "1.643934567"
    );
}

#[test]
fn beta_fn_three_four_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", beta_fn(3, 4))"#),
        "0.01666666667"
    );
}

#[test]
fn lbeta_three_four_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", lbeta(3, 4))"#),
        "-4.094344562"
    );
}

#[test]
fn pbeta_half_two_five_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pbeta(0.5, 2, 5))"#),
        "0.8906253711"
    );
}

#[test]
fn take_while_under_three_cr() {
    assert_eq!(
        eval_string(r#"stringify(take_while { $_ < 3 } 1, 2, 3, 4, 5)"#),
        "(1, 2)"
    );
}

#[test]
fn drop_while_under_three_cr() {
    assert_eq!(
        eval_string(r#"stringify(drop_while { $_ < 3 } 1, 2, 3, 4, 5)"#),
        "(3, 4, 5)"
    );
}

#[test]
fn reject_even_cr() {
    assert_eq!(
        eval_string(r#"stringify(reject { $_ % 2 == 0 } 1, 2, 3, 4)"#),
        "(1, 3)"
    );
}

#[test]
fn string_take_while_charset_prefix_not_predicate_cr() {
    assert_eq!(eval_string(r#"string_take_while("aabbc", "ab")"#), "aabb");
}

#[test]
fn string_drop_while_charset_prefix_not_predicate_cr() {
    assert_eq!(eval_string(r#"string_drop_while("aabbc", "ab")"#), "c");
}

#[test]
fn string_split_at_first_sep_cr() {
    assert_eq!(
        eval_string(r#"stringify(string_split_at_first("a=b=c", "="))"#),
        r#"("a", "b=c")"#
    );
}

#[test]
fn nth_zero_indexed_from_range_iterator_cr() {
    assert_eq!(eval_string(r#"nth(2, range(0, 5))"#), "2");
}

#[test]
fn nth_inline_arrayref_undef_bug_cr() {
    assert_eq!(eval_string(r#"stringify(nth(1, [10, 20, 30]))"#), "20");
}

#[test]
fn leading_zeros_eight_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", leading_zeros(8))"#), "60");
}

#[test]
fn trailing_zeros_eight_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", trailing_zeros(8))"#), "3");
}

#[test]
fn from_base_binary_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", from_base("1010", 2))"#),
        "10"
    );
}

#[test]
fn to_base_hex_lower_cr() {
    assert_eq!(eval_string(r#"to_base(255, 16)"#), "ff");
}

#[test]
fn base_convert_hex_to_dec_cr() {
    assert_eq!(eval_string(r#"base_convert("FF", 16, 10)"#), "255");
}

#[test]
fn merge_sorted_two_lists_cr() {
    assert_eq!(
        eval_string(r#"stringify(merge_sorted([1, 3, 5], [2, 4, 6]))"#),
        "(1, 2, 3, 4, 5, 6)"
    );
}

#[test]
fn is_even_is_odd_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", is_even(4))"#), "1");
    assert_eq!(eval_string(r#"sprintf("%.0f", is_odd(5))"#), "1");
}

#[test]
fn divmod_positive_cr() {
    assert_eq!(eval_string(r#"stringify(divmod(17, 5))"#), "(3, 2)");
}

#[test]
fn approx_eq_default_epsilon_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", approx_eq(1.0, 1.0 + 1e-10))"#),
        "1"
    );
}

#[test]
fn stirling_five_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", stirling(5))"#),
        "118.019168"
    );
}

#[test]
fn head_n_tail_n_cr() {
    assert_eq!(
        eval_string(r#"stringify(head_n(3, 1, 2, 3, 4, 5))"#),
        "(1, 2, 3)"
    );
    assert_eq!(
        eval_string(r#"stringify(tail_n(2, 1, 2, 3, 4, 5))"#),
        "(4, 5)"
    );
}

#[test]
fn first_variadic_cr() {
    assert_eq!(eval_string(r#"first(10, 20, 30)"#), "10");
}

#[test]
fn bit_and_or_xor_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", bit_and(12, 10))"#), "8");
    assert_eq!(eval_string(r#"sprintf("%.0f", bit_or(12, 10))"#), "14");
    assert_eq!(eval_string(r#"sprintf("%.0f", bit_xor(12, 10))"#), "6");
}

#[test]
fn sigmoid_zero_cr() {
    assert_eq!(eval_string(r#"sprintf("%.10g", sigmoid(0))"#), "0.5");
}

#[test]
fn relu_negative_zero_cr() {
    assert_eq!(eval_string(r#"sprintf("%.10g", relu(-3))"#), "0");
    assert_eq!(eval_string(r#"sprintf("%.10g", relu(3))"#), "3");
}

#[test]
fn tanh_one_cr() {
    assert_eq!(eval_string(r#"sprintf("%.10g", tanh(1))"#), "0.761594156");
}

#[test]
fn softmax_two_logits_cr() {
    let out = eval_string(r#"stringify(softmax([0, 1]))"#);
    assert!(out.starts_with('[') && out.ends_with(']'), "got {:?}", out);
    assert!(
        out.contains("0.2689414214") || out.contains("0.268941"),
        "got {:?}",
        out
    );
    assert!(
        out.contains("0.7310585786") || out.contains("0.731058"),
        "got {:?}",
        out
    );
}

#[test]
fn cross_entropy_loss_binary_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.6g", cross_entropy([1, 0], [0.8, 0.2]))"#),
        "0.223144"
    );
}

#[test]
fn mean_squared_error_lists_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mse([1, 2, 3], [1, 0, 3]))"#),
        "1.333333333"
    );
}

#[test]
fn gcd_trailing_operands_ignored_two_arg_only_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", gcd(12, 18, 35))"#), "6");
}

#[test]
fn lcm_trailing_operands_ignored_two_arg_only_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", lcm(4, 6, 10))"#), "12");
}

#[test]
fn factorial_seven_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", factorial(7))"#), "5040");
}

#[test]
fn double_factorial_nine_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", double_factorial(9))"#),
        "945"
    );
}

#[test]
fn rising_factorial_four_two_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", pochhammer(4, 2))"#), "20");
}

#[test]
fn falling_factorial_five_two_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", falling_factorial(5, 2))"#),
        "20"
    );
}

#[test]
fn multinomial_coeff_short_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", multinomial(4, 2, 2))"#), "6");
}

#[test]
fn eulerian_number_five_two_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", eulerian_number(5, 2))"#),
        "66"
    );
}

#[test]
fn bernoulli_number_six_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", bernoulli(6))"#),
        "0.02380952381"
    );
}

#[test]
fn catalan_five_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", catalan(5))"#), "42");
}

/// Regularized incomplete beta `betainc(x,a,b)`; see also **`pbeta`** pin above.
#[test]
fn betainc_half_two_five_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", betainc(0.5, 2, 5))"#),
        "0.890625"
    );
}

#[test]
fn qbeta_quarter_two_five_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", qbeta(0.25, 2, 5))"#),
        "0.1611629168"
    );
}

/// `inverse_beta_regularized(a, b, y)` — argument order is shape parameters then level.
#[test]
fn inverse_beta_regularized_two_five_quarter_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", inverse_beta_regularized(2, 5, 0.25))"#),
        "0.1611629168"
    );
}

#[test]
fn inverse_gamma_regularized_three_point_seven_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", inverse_gamma_regularized(3, 0.7))"#),
        "3.615567666"
    );
}

#[test]
fn polygamma_one_at_one_matches_trigamma_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", polygamma(1, 1))"#),
        "1.643934567"
    );
}

#[test]
fn binomial_ten_three_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", binomial(10, 3))"#), "120");
}

#[test]
fn stirling_second_kind_five_three_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", stirling2(5, 3))"#), "25");
}

#[test]
fn euler_totient_twelve_cr() {
    assert_eq!(eval_string(r#"sprintf("%.0f", euler_totient(12))"#), "4");
}

#[test]
fn pentagonal_number_five_cr() {
    assert_eq!(
        eval_string(r#"sprintf("%.0f", pentagonal_number(5))"#),
        "35"
    );
}
