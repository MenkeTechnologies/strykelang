//! Behavior-pinning batch AK (2026-05-06): Network, ANSI, Stats, String Random, Versions.

use crate::common::*;

// ── Network Interfaces ───────────────────────────────────────────────────────

#[test]
fn network_interfaces_ak() {
    let code = r#"
        my @ifs = net_interfaces();
        if (len(@ifs) > 0) {
            my $it = $ifs[0];
            # Fields: name, ipv4, ipv6, mac, is_up
            len($it->{name}) > 0 ? 1 : 0
        } else {
            1 # Skip if no interfaces found in environment
        }
    "#;
    assert_eq!(eval_int(code), 1);

    // net_ipv4() returns current primary IP or empty string
    eval_string("net_ipv4()");
}

// ── ANSI Styling ─────────────────────────────────────────────────────────────

#[test]
fn ansi_styling_ak() {
    // ansi_green("text") -> \x1b[32mtext\x1b[0m
    let s = eval_string(r#"ansi_green("hello")"#);
    assert!(s.contains("\x1b[32m"));
    assert!(s.contains("hello"));

    assert_eq!(eval_string(&format!(r##"strip_ansi("{}")"##, s)), "hello");

    // Underline wrap
    let underline = eval_string(r#"ansi_underline("underline")"#);
    assert!(underline.contains("\x1b[4m"));
}

// ── Stats (Additional) ───────────────────────────────────────────────────────

#[test]
fn stats_ak() {
    // geometric_mean of a single number is the number itself
    assert_eq!(
        eval_string("sprintf('%.0f', geometric_mean(10))"),
        "10"
    );

    // zscore with same values should be 0
    assert_eq!(eval_int("zscore(10, 10, 10)"), 0);
}

// ── Sorting & Array Helpers ──────────────────────────────────────────────────

#[test]
fn sorting_ak() {
    let code = r#"
        my @l = ("apple", "a", "banana", "pear", "A");
        my @s = sorted_by_length(@l);
        join(",", @s)
    "#;
    assert_eq!(eval_string(code), "a,A,pear,apple,banana");

    let code2 = r#"
        my @l = (1, 2, 3, 4, 5);
        my @r = reverse_list(@l);
        join(",", @r)
    "#;
    assert_eq!(eval_string(code2), "5,4,3,2,1");
}

#[test]
fn array_ak_helpers() {
    let code = r#"
        my @l = (1, 2, 3, 2, 4, 2);
        # without(element, list)
        my @w = without(2, @l);
        join(",", @w)
    "#;
    assert_eq!(eval_string(code), "1,3,4");

    let code2 = r#"
        my @l = (1, 2, 3, 4, 5, 6);
        my @tl = take_last(3, @l);
        my @dl = drop_last(4, @l);
        join(",", @tl) . ":" . join(",", @dl)
    "#;
    assert_eq!(eval_string(code2), "4,5,6:1,2");
}

// ── String Randomization & Regex ─────────────────────────────────────────────

#[test]
fn string_ak() {
    // shuffle_chars should keep same chars
    let s = eval_string(r#"shuffle_chars("abcde")"#);
    assert_eq!(s.len(), 5);
    assert!(s.contains("a") && s.contains("b") && s.contains("c") && s.contains("d") && s.contains("e"));

    assert_eq!(eval_int(r#"matches_regex("hello world", "^h.*d$")"#), 1);
    assert_eq!(eval_int(r#"count_regex_matches("abababa", "a")"#), 4);
}

// ── Version Comparison ───────────────────────────────────────────────────────

#[test]
fn version_comparison_ak() {
    assert_eq!(eval_int(r#"version_cmp("1.2.3", "1.2.4")"#), -1);
    assert_eq!(eval_int(r#"version_cmp("1.10.1", "1.2.3")"#), 1);
    assert_eq!(eval_int(r#"version_cmp("2.0.0", "2.0.0")"#), 0);
    assert_eq!(eval_int(r#"version_cmp("1.0", "1.0.0")"#), 0);
}

// ── Misc ─────────────────────────────────────────────────────────────────────

#[test]
fn misc_ak() {
    // pad_number(num, width)
    assert_eq!(eval_string("pad_number(15, 4)"), "0015");
    assert_eq!(eval_string("pad_number(-5, 3)"), "-05");
}
