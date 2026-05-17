//! Dispatch-only tests for the `gh_*` GitHub REST builtins. These pin the
//! WIRING (parser + extension table + builtins-hash reflection) without
//! making any network calls — CI-safe.
//!
//! Network-touching behavior (auth, pagination, JSON shape) is exercised
//! manually via `examples/gh_*.stk`; we deliberately don't run those in CI.

use crate::common::*;

const GH_PRIMARIES: &[&str] = &[
    "gh_get",
    "gh_user",
    "gh_org",
    "gh_followers",
    "gh_following",
    "gh_repo",
    "gh_repos",
    "gh_org_repos",
    "gh_starred",
    "gh_gists",
    "gh_gist",
    "gh_issues",
    "gh_prs",
    "gh_commits",
    "gh_branches",
    "gh_tags",
    "gh_releases",
    "gh_contributors",
    "gh_forks",
    "gh_stargazers",
    "gh_topics",
    "gh_languages",
    "gh_readme",
    "gh_workflows",
    "gh_runs",
    "gh_search_repos",
    "gh_search_users",
    "gh_search_code",
    "gh_search_issues",
    "gh_rate_limit",
    "gh_meta",
    "gh_emojis",
    "gh_zen",
];

const GH_ALIASES: &[&str] = &["gh_pulls", "gh_langs"];

#[test]
fn every_gh_primary_is_in_builtins_hash() {
    for name in GH_PRIMARIES {
        let code = format!(r#"exists $b{{{}}} ? 1 : 0"#, name);
        let n = eval_int(&code);
        assert_eq!(n, 1, "expected %b to contain primary `{}`", name);
    }
}

#[test]
fn every_gh_primary_is_in_all_hash() {
    for name in GH_PRIMARIES {
        let code = format!(r#"exists $all{{{}}} ? 1 : 0"#, name);
        let n = eval_int(&code);
        assert_eq!(n, 1, "expected %all to contain `{}`", name);
    }
}

#[test]
fn every_gh_alias_resolves_to_a_primary() {
    for alias in GH_ALIASES {
        let code = format!(r#"exists $a{{{}}} ? 1 : 0"#, alias);
        let n = eval_int(&code);
        assert_eq!(n, 1, "expected %a to contain alias `{}`", alias);
    }
}

#[test]
fn gh_pulls_alias_points_at_gh_prs() {
    let s = eval_string(r#"$a{gh_pulls}"#);
    assert_eq!(s, "gh_prs");
}

#[test]
fn gh_langs_alias_points_at_gh_languages() {
    let s = eval_string(r#"$a{gh_langs}"#);
    assert_eq!(s, "gh_languages");
}

#[test]
fn every_gh_primary_carries_a_category_tag() {
    // CATEGORY_MAP entries are populated by build.rs from the `// ── github
    // / gh REST API ──` section comment in parser.rs. Confirms the names
    // landed in the right reflection section, not just the dispatch table.
    for name in GH_PRIMARIES {
        let code = format!(r#"$b{{{}}}"#, name);
        let s = eval_string(&code);
        assert!(
            !s.is_empty(),
            "expected non-empty category for `{}`, got `{}`",
            name,
            s
        );
    }
}

#[test]
fn gh_alias_count_matches_dispatch_arms() {
    // `gh_pulls` and `gh_langs` are the only two alias spellings in the gh
    // dispatch table. Pin the count so future arms don't accidentally
    // shadow or duplicate them.
    let n = eval_int(r#"len(grep { /^gh_/ } keys %a)"#);
    assert_eq!(n, 2);
}
