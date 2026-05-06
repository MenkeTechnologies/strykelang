//! Behavior-pinning batch AN (2026-05-06): System, Env, Random Sampling.

use crate::common::*;

// ── System & Env ─────────────────────────────────────────────────────────────

#[test]
fn system_env_an() {
    assert_eq!(eval_int("cmd_exists('cargo')"), 1);
    assert_eq!(eval_int("cmd_exists('non-existent-cmd-xyz-abc')"), 0);

    // argc() depends on how integration tests are invoked, usually 0 or small
    let _ = eval_int("argc()");

    // script_name() returns binary name
    assert!(
        eval_string("script_name()").contains("stryke")
            || eval_string("script_name()").contains("integration")
    );

    // has_stdin_tty()
    let _ = eval_int("has_stdin_tty()");

    // env_keys()
    let code = r#"
        my @keys = env_keys();
        join("", grep { $_ eq "HOME" } @keys)
    "#;
    assert_eq!(eval_string(code), "HOME");
}

// ── Random Sampling ──────────────────────────────────────────────────────────

#[test]
fn random_sampling_an() {
    let code = r#"
        my @sample = random_sample(5, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
        len(@sample)
    "#;
    assert_eq!(eval_int(code), 5);

    // sample members should be in original list
    let code2 = r#"
        my @l = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
        my @sample = random_sample(5, @l);
        my $ok = 1;
        for my $s (@sample) {
            $ok = 0 unless grep { $_ == $s } @l;
        }
        $ok
    "#;
    assert_eq!(eval_int(code2), 1);
}

// ── File Stats ───────────────────────────────────────────────────────────────

#[test]
fn file_stats_an() {
    // file_mtime returns unix timestamp
    assert!(eval_int("file_mtime('README.md')") > 1700000000);
}

// ── Miscellaneous Predicates ────────────────────────────────────────────────

#[test]
fn misc_predicates_an() {
    assert_eq!(eval_int("is_uuid(uuid_v4())"), 1);
    assert_eq!(eval_int("is_uuid('not-a-uuid-either')"), 0);
}
