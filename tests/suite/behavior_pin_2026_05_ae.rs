//! Behavior-pinning batch AE (2026-05-05): System, Env, Random Sampling.

use crate::common::*;

// ── System & Env ─────────────────────────────────────────────────────────────

#[test]
fn system_env_ae() {
    assert_eq!(eval_int("cmd_exists('ls')"), 1);
    assert_eq!(eval_int("cmd_exists('non-existent-cmd-xyz')"), 0);
    
    // argc() depends on how integration tests are invoked, usually 0 or small
    let _ = eval_int("argc()");
    
    // script_name() returns binary name
    assert!(eval_string("script_name()").contains("stryke") || eval_string("script_name()").contains("integration"));
    
    // has_stdout_tty()
    let _ = eval_int("has_stdout_tty()");
    
    // env_keys()
    let code = r#"
        my @keys = env_keys();
        join("", grep { $_ eq "PATH" } @keys)
    "#;
    assert_eq!(eval_string(code), "PATH");
}

// ── Random Sampling ──────────────────────────────────────────────────────────

#[test]
fn random_sampling_ae() {
    let code = r#"
        my @sample = random_sample(3, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10);
        len(@sample)
    "#;
    assert_eq!(eval_int(code), 3);
    
    // sample members should be in original list
    let code2 = r#"
        my @l = (1, 2, 3, 4, 5);
        my @sample = random_sample(2, @l);
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
fn file_stats_ae() {
    // file_mtime returns unix timestamp
    assert!(eval_int("file_mtime('Cargo.toml')") > 1700000000);
}

// ── Miscellaneous Predicates ────────────────────────────────────────────────

#[test]
fn misc_predicates_ae() {
    assert_eq!(eval_int("is_uuid(uuid_v4())"), 1);
    assert_eq!(eval_int("is_uuid('not-a-uuid')"), 0);
}
