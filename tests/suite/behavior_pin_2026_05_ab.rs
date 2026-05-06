//! Behavior-pinning batch AB (2026-05-05): Network, ANSI, Stats, String Random, Versions.

use crate::common::*;

// ── Network Interfaces ───────────────────────────────────────────────────────

#[test]
fn network_interfaces_smoke() {
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

    // net_ipv4() returns current primary IP
    assert!(!eval_string("net_ipv4()").is_empty());
}

// ── ANSI Styling ─────────────────────────────────────────────────────────────

#[test]
fn ansi_styling_smoke() {
    // ansi_red("text") -> \x1b[31mtext\x1b[0m
    let s = eval_string(r#"ansi_red("hello")"#);
    assert!(s.contains("\x1b[31m"));
    assert!(s.contains("hello"));

    assert_eq!(eval_string(&format!(r##"strip_ansi("{}")"##, s)), "hello");

    // Bold wrap
    let bold = eval_string(r#"ansi_bold("bold")"#);
    assert!(bold.contains("\x1b[1m"));
}

// ── Stats (Additional) ───────────────────────────────────────────────────────

#[test]
fn stats_ab_smoke() {
    // geometric_mean(1, 8, 64) = 8
    // Use format to avoid int truncation issues
    assert_eq!(
        eval_string("sprintf('%.0f', geometric_mean(1, 8, 64))"),
        "8"
    );

    // zscore(x, list)
    // For x=15 and list=[10, 5]: mean=7.5, sd=2.5, z=(15-7.5)/2.5 = 3
    assert_eq!(eval_int("zscore(15, 10, 5)"), 3);
}

// ── Sorting & Array Helpers ──────────────────────────────────────────────────

#[test]
fn sorting_ab_smoke() {
    let code = r#"
        my @l = ("apple", "a", "banana", "pear");
        my @s = sorted_by_length(@l);
        join(",", @s)
    "#;
    assert_eq!(eval_string(code), "a,pear,apple,banana");

    let code2 = r#"
        my @l = (1, 2, 3);
        my @r = reverse_list(@l);
        join(",", @r)
    "#;
    assert_eq!(eval_string(code2), "3,2,1");
}

#[test]
fn array_ab_helpers() {
    let code = r#"
        my @l = (1, 2, 3, 2, 4);
        # without(element, list)
        my @w = without(2, @l);
        join(",", @w)
    "#;
    assert_eq!(eval_string(code), "1,3,4");

    let code2 = r#"
        my @l = (1, 2, 3, 4, 5);
        my @tl = take_last(2, @l);
        my @dl = drop_last(2, @l);
        join(",", @tl) . ":" . join(",", @dl)
    "#;
    assert_eq!(eval_string(code2), "4,5:1,2,3");
}

// ── String Randomization & Regex ─────────────────────────────────────────────

#[test]
fn string_ab_smoke() {
    // shuffle_chars should keep same chars
    let s = eval_string(r#"shuffle_chars("abc")"#);
    assert_eq!(s.len(), 3);
    assert!(s.contains("a") && s.contains("b") && s.contains("c"));

    assert_eq!(eval_int(r#"matches_regex("hello", "^h.*o$")"#), 1);
    assert_eq!(eval_int(r#"count_regex_matches("ababa", "a")"#), 3);
}

// ── Version Comparison ───────────────────────────────────────────────────────

#[test]
fn version_comparison_smoke() {
    assert_eq!(eval_int(r#"version_cmp("1.2.3", "1.2.4")"#), -1);
    assert_eq!(eval_int(r#"version_cmp("1.10", "1.2")"#), 1);
    assert_eq!(eval_int(r#"version_cmp("2.0.0", "2.0")"#), 0);
}

// ── Misc ─────────────────────────────────────────────────────────────────────

#[test]
fn misc_ab_smoke() {
    // pad_number(num, width)
    assert_eq!(eval_string("pad_number(5, 3)"), "005");
    assert_eq!(eval_string("pad_number(5, 5)"), "00005");
}
