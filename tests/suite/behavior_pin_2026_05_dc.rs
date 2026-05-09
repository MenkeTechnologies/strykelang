//! Behavior-pinning batch DC (2026-05-09): **math** (`hypot`, `atan2`, `log_base`, `safe_div`, `sign`, `round`, `floor`,
//! `ceil`, **`harmonic_number`**, **`stirling_approx`**, **`stirling2`**), **NT** (multi-arg **`gcd` / `lcm`**, **`modinv`**, **`crt`**,
//! **`prime_factors`**, **`next_prime`**), **stats** (`quartiles`, `iqr`, `mad`, `huber_m_estimator`, **`mse` / `rmse` / `mae`**),
//! **strings** (`substring_similarity`, `int_to_roman` / `roman_to_int`), **lists** (`zip`, `interleave`, **`unzip`** / **`unzip_pairs`** —
//! single **`[[pair],…]`** operand footgun **BUG-186**), **ML-lite** (`kmeans`, `silhouette_score`), **EM** (`freq_wavelength`,
//! `wavelength_freq`), **audio** (`hz_to_midi`, `db_to_amp`, `huber_loss`), **encodings** (`base32_encode` / `decode`, `base64_encode` /
//! `decode`, **`gzip`**, `is_base64`, `url_encode` / `url_decode`), **UUID** (`uuid_v4` layout via **`len`/`split`**), **scalars**
//! (`parse_float`, **`clamp`** three-arg order — cross-ref **BUG-151**), **`clamp_list(lo, hi, …)`** (valid bounds; **`lo > hi`** — **BUG-187**),
//! **`sorted_nums`**, `normalize_list`, `list_eq`, `pairwise`, **`kinetic_energy`**, **`list_count`**, **`l2_norm` / `vector_dot` / `vec_normalize`**,
//! **planar** **`angle_between`**, **`laplace_pdf` / `pweibull`**.

use crate::common::*;

#[test]
fn hypot_atan2_log_base_safe_div_dc() {
    assert_eq!(eval_string(r#"sprintf("%.10g", hypot(3, 4))"#), "5");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", atan2(1, 1))"#),
        "0.7853981634"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", log_base(8, 2))"#), "3");
    assert_eq!(eval_string(r#"sprintf("%.10g", safe_div(10, 0))"#), "0");
}

#[test]
fn sign_round_floor_ceil_dc() {
    assert_eq!(eval_string(r#"sprintf("%.10g", sign(-3))"#), "-1");
    assert_eq!(eval_string(r#"sprintf("%.10g", sign(0))"#), "0");
    assert_eq!(eval_string(r#"sprintf("%.10g", round(3.6))"#), "4");
    assert_eq!(eval_string(r#"sprintf("%.10g", floor(-1.2))"#), "-2");
    assert_eq!(eval_string(r#"sprintf("%.10g", ceil(-1.2))"#), "-1");
}

#[test]
fn gcd_lcm_multi_arg_dc() {
    assert_eq!(eval_int(r#"gcd(12, 18, 24)"#), 6);
    assert_eq!(eval_int(r#"lcm(4, 6, 8)"#), 12);
}

#[test]
fn modinv_crt_dc() {
    assert_eq!(eval_int(r#"modinv(3, 11)"#), 4);
    assert_eq!(
        eval_string(r#"sprintf("%.10g", chinese_remainder([2, 3], [3, 5]))"#),
        "8"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", crt([2, 3], [3, 5]))"#),
        "8"
    );
}

#[test]
fn prime_factors_next_prime_dc() {
    assert_eq!(
        eval_string(r#"stringify(prime_factors(12))"#),
        "(2, 2, 3)"
    );
    assert_eq!(eval_int(r#"next_prime(13)"#), 17);
}

#[test]
fn harmonic_stirling_numbers_dc() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", harmonic_number(5))"#),
        "2.283333333"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", stirling_approx(10))"#),
        "3598695.619"
    );
    assert_eq!(eval_string(r#"sprintf("%.0f", stirling2(5, 2))"#), "15");
}

#[test]
fn quartiles_iqr_mad_dc() {
    assert_eq!(
        eval_string(r#"stringify(quartiles([1, 2, 3, 4, 5, 6, 7, 8]))"#),
        "(3, 5, 7)"
    );
    assert_eq!(eval_string(r#"sprintf("%.10g", iqr([1, 2, 3, 4, 5, 6, 7, 100]))"#), "4");
    assert_eq!(eval_string(r#"sprintf("%.10g", mad([1, 2, 3, 4, 5]))"#), "1");
}

#[test]
fn huber_substring_similarity_dc() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", huber_m_estimator([1, 2, 3, 100]))"#),
        "3.329379787"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", substring_similarity("hello", "hallo"))"#),
        "1"
    );
}

#[test]
fn kmeans_two_clusters_silhouette_dc() {
    assert_eq!(
        eval_string(r#"stringify(kmeans([[1, 2], [2, 1], [8, 9], [9, 8]], 2))"#),
        "(0, 1, 0, 1)"
    );
    assert_eq!(
        eval_string(
            r#"sprintf("%.10g", silhouette_score([[0, 0], [1, 0], [10, 0]], [0, 0, 1]))"#
        ),
        "0.8944444444"
    );
}

#[test]
fn zip_interleave_unzip_flat_dc() {
    assert_eq!(
        eval_string(r#"stringify(zip([1, 2], [10, 20]))"#),
        "([1, 10], [2, 20])"
    );
    assert_eq!(
        eval_string(r#"stringify(interleave([1, 2], [3, 4]))"#),
        "(1, 3, 2, 4)"
    );
    assert_eq!(
        eval_string(r#"stringify(unzip(1, 10, 2, 20))"#),
        "([1, 2], [10, 20])"
    );
    assert_eq!(
        eval_string(r#"stringify(unzip_pairs([[1, 10], [2, 20]]))"#),
        "([1, 2], [10, 20])"
    );
}

/// **`unzip`** flattens **`args`** into **[row0col0, row0col1, row1col0, …]**; a **single** nested **`[[a,b],[c,d]]`** becomes **two** cells and mis-pairs (**BUG-186**).
#[test]
fn unzip_nested_aof_pairs_mispairs_bug_dc() {
    assert_eq!(
        eval_string(r#"stringify(unzip([[1, 10], [2, 20]]))"#),
        "([[1, 10]], [[2, 20]])"
    );
}

#[test]
fn mse_rmse_mae_dc() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mse([1, 2, 3], [1.1, 2.1, 2.9]))"#),
        "0.01"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", rmse([1, 2, 3], [1.1, 2.1, 2.9]))"#),
        "0.1"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", mae([1, 2, 3], [1.1, 2.1, 2.9]))"#),
        "0.1"
    );
}

#[test]
fn freq_wavelength_wavelength_freq_same_numeric_dc() {
    let a = eval_string(r#"sprintf("%.10g", freq_wavelength(440))"#);
    let b = eval_string(r#"sprintf("%.10g", wavelength_freq(440))"#);
    assert_eq!(a, b);
    assert_eq!(a, "681346.4955");
}

#[test]
fn hz_to_midi_db_to_amp_huber_loss_dc() {
    assert_eq!(eval_string(r#"sprintf("%.10g", hz_to_midi(440))"#), "69");
    assert_eq!(eval_string(r#"sprintf("%.10g", db_to_amp(20))"#), "10");
    assert_eq!(eval_string(r#"sprintf("%.10g", huber_loss(0.5, 1))"#), "0.125");
}

#[test]
fn base32_roundtrip_dc() {
    assert_eq!(eval_string(r#"base32_encode("abc")"#), "MFRGG===");
    assert_eq!(eval_string(r#"base32_decode("MFRGG===")"#), "abc");
}

#[test]
fn base64_gzip_is_base64_dc() {
    assert_eq!(eval_string(r#"base64_encode("abc")"#), "YWJj");
    assert_eq!(eval_string(r#"base64_decode("YWJj")"#), "abc");
    assert_eq!(eval_int(r#"length(gzip("hello"))"#), 25);
    assert_eq!(eval_int(r#"is_base64("YWJj")"#), 1);
}

#[test]
fn url_encode_decode_dc() {
    assert_eq!(eval_string(r#"url_encode("a b")"#), "a%20b");
    assert_eq!(eval_string(r#"url_decode("hello%20world")"#), "hello world");
}

#[test]
fn roman_1999_roundtrip_dc() {
    assert_eq!(eval_string(r#"int_to_roman(1999)"#), "MCMXCIX");
    assert_eq!(eval_int(r#"roman_to_int("MCMXCIX")"#), 1999);
}

#[test]
fn uuid_v4_len_and_version_token_dc() {
    assert_eq!(eval_int(r#"length(uuid_v4())"#), 36);
    assert_eq!(eval_int(r#"len(split("-", uuid_v4()))"#), 5);
}

#[test]
fn parse_float_simple_dc() {
    assert_eq!(eval_string(r#"sprintf("%.10g", parse_float("3.14"))"#), "3.14");
}

/// Three-scalar **`clamp`** call-shape (**BUG-151**): **`clamp(5, 0, 10)`** is read as **`clamp(min=5, max=0, …)`**, not “clamp **5** into **[0,10]**”.
#[test]
fn clamp_scalar_three_arg_heuristic_dc() {
    assert_eq!(eval_string(r#"sprintf("%.10g", clamp(5, 0, 10))"#), "0");
}

#[test]
fn clamp_list_lo_hi_then_values_dc() {
    assert_eq!(
        eval_string(r#"stringify(clamp_list(0, 10, 1, 50, 5))"#),
        "(1, 10, 5)"
    );
}

#[test]
fn sorted_nums_normalize_list_dc() {
    assert_eq!(
        eval_string(r#"stringify(sorted_nums([10, 2, 1]))"#),
        "(1, 2, 10)"
    );
    assert_eq!(
        eval_string(r#"stringify(normalize_list([1, 2, 3]))"#),
        "(0, 0.5, 1)"
    );
}

#[test]
fn list_eq_pairwise_kinetic_list_count_dc() {
    assert_eq!(eval_int(r#"list_eq([1, 2], [1, 2])"#), 1);
    assert_eq!(eval_int(r#"list_eq([1, 2], [1, 3])"#), 0);
    assert_eq!(
        eval_string(r#"stringify(pairwise([1, 2, 3, 4]))"#),
        "([1, 2], [2, 3], [3, 4])"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", kinetic_energy(2, 3))"#),
        "9"
    );
    assert_eq!(eval_int(r#"list_count([1, 2], [3])"#), 3);
}

#[test]
fn lerp_smoothstep_dc() {
    assert_eq!(eval_string(r#"sprintf("%.10g", lerp(0, 10, 0.25))"#), "2.5");
    assert_eq!(
        eval_string(r#"sprintf("%.10g", smoothstep(0, 1, 0.5))"#),
        "0.5"
    );
}

#[test]
fn cbrt_scalar_dc() {
    assert_eq!(eval_string(r#"sprintf("%.10g", cbrt(27))"#), "3");
}

#[test]
fn l2_norm_vector_dot_unit_vec_dc() {
    assert_eq!(eval_string(r#"sprintf("%.10g", l2_norm([3, 4]))"#), "5");
    assert_eq!(eval_string(r#"sprintf("%.10g", vector_dot([1, 2], [3, 4]))"#), "11");
    assert_eq!(
        eval_string(r#"stringify(vec_normalize([3, 4]))"#),
        "[0.6, 0.8]"
    );
}

#[test]
fn angle_between_planar_degrees_dc() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", angle_between(0, 0, 1, 1))"#),
        "45"
    );
}

#[test]
fn laplace_pweibull_dc() {
    assert_eq!(
        eval_string(r#"sprintf("%.10g", laplace_pdf(0, 0, 1))"#),
        "0.5"
    );
    assert_eq!(
        eval_string(r#"sprintf("%.10g", pweibull(1, 1, 2))"#),
        "0.3934693403"
    );
}
