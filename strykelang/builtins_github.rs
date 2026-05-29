//! GitHub REST API primitives — pragmatic wrappers around `api.github.com`.
//!
//! Designed for parallel-map workflows:
//! ```stryke
//! my @repos = gh_repos("MenkeTechnologies")
//! my @stars = pmap { gh_repo($_->{full_name})->{stargazers_count} } @repos
//! ```
//!
//! All builtins authenticate via the `GITHUB_TOKEN` environment variable
//! when present (5000 req/hour); otherwise fall back to unauthenticated
//! access (60 req/hour). List endpoints auto-paginate up to a safety cap
//! (`GH_MAX_PAGES`, default 10 = up to 1000 items at per_page=100).
//!
//! Builtins:
//!   gh_get(PATH, [opts])              — generic GET, parsed JSON
//!   gh_user(USER)                     — /users/USER
//!   gh_org(ORG)                       — /orgs/ORG
//!   gh_repo(OWNER, REPO)              — /repos/OWNER/REPO
//!   gh_repos(USER)                    — /users/USER/repos          (paginated)
//!   gh_org_repos(ORG)                 — /orgs/ORG/repos            (paginated)
//!   gh_starred(USER)                  — /users/USER/starred        (paginated)
//!   gh_followers(USER)                — /users/USER/followers      (paginated)
//!   gh_following(USER)                — /users/USER/following      (paginated)
//!   gh_gists(USER)                    — /users/USER/gists          (paginated)
//!   gh_gist(ID)                       — /gists/ID
//!   gh_issues(OWNER, REPO)            — /repos/OWNER/REPO/issues   (paginated)
//!   gh_prs(OWNER, REPO)               — /repos/OWNER/REPO/pulls    (paginated)
//!   gh_commits(OWNER, REPO)           — /repos/OWNER/REPO/commits  (paginated)
//!   gh_branches(OWNER, REPO)          — /repos/OWNER/REPO/branches (paginated)
//!   gh_tags(OWNER, REPO)              — /repos/OWNER/REPO/tags     (paginated)
//!   gh_releases(OWNER, REPO)          — /repos/OWNER/REPO/releases (paginated)
//!   gh_contributors(OWNER, REPO)      — /repos/OWNER/REPO/contributors (paginated)
//!   gh_forks(OWNER, REPO)             — /repos/OWNER/REPO/forks    (paginated)
//!   gh_stargazers(OWNER, REPO)        — /repos/OWNER/REPO/stargazers (paginated)
//!   gh_topics(OWNER, REPO)            — array of topic names
//!   gh_languages(OWNER, REPO)         — { language => bytes } hashref
//!   gh_readme(OWNER, REPO)            — decoded README content (string)
//!   gh_workflows(OWNER, REPO)         — workflows array
//!   gh_runs(OWNER, REPO)              — workflow runs array
//!   gh_search_repos(QUERY)            — /search/repositories       (paginated)
//!   gh_search_users(QUERY)            — /search/users              (paginated)
//!   gh_search_code(QUERY)             — /search/code               (paginated)
//!   gh_search_issues(QUERY)           — /search/issues             (paginated)
//!   gh_rate_limit()                   — /rate_limit
//!   gh_meta()                         — /meta
//!   gh_zen()                          — /zen (plain-text string)
//!   gh_emojis()                       — /emojis (hashref)
//!
//! Errors: network / 4xx / 5xx → runtime error. 404 returns `undef` so callers
//! can `pmap { gh_repo(...) }` over a list including dead names without
//! aborting the whole pipeline.

use crate::error::{StrykeError, StrykeResult};
use crate::value::StrykeValue;
use indexmap::IndexMap;
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;

// ── helpers ────────────────────────────────────────────────────────────

const API_ROOT: &str = "https://api.github.com";
const USER_AGENT: &str = "strykelang-gh-builtins";
const DEFAULT_MAX_PAGES: usize = 10;

fn arg_str(args: &[StrykeValue], i: usize) -> String {
    args.get(i).map(|v| v.to_string()).unwrap_or_default()
}

fn json_to_perl(v: serde_json::Value) -> StrykeValue {
    match v {
        serde_json::Value::Null => StrykeValue::UNDEF,
        serde_json::Value::Bool(b) => StrykeValue::integer(i64::from(b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                StrykeValue::integer(i)
            } else if let Some(u) = n.as_u64() {
                StrykeValue::integer(u as i64)
            } else {
                StrykeValue::float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => StrykeValue::string(s),
        serde_json::Value::Array(a) => StrykeValue::array_ref(Arc::new(RwLock::new(
            a.into_iter().map(json_to_perl).collect(),
        ))),
        serde_json::Value::Object(o) => {
            let mut map = IndexMap::new();
            for (k, v) in o {
                map.insert(k, json_to_perl(v));
            }
            StrykeValue::hash_ref(Arc::new(RwLock::new(map)))
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build()
}

fn prepare_request(req: ureq::Request) -> ureq::Request {
    let req = req
        .set("Accept", "application/vnd.github+json")
        .set("User-Agent", USER_AGENT)
        .set("X-GitHub-Api-Version", "2022-11-28");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            return req.set("Authorization", &format!("Bearer {}", token));
        }
    }
    req
}

fn build_url(path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else if let Some(rest) = path.strip_prefix('/') {
        format!("{}/{}", API_ROOT, rest)
    } else {
        format!("{}/{}", API_ROOT, path)
    }
}

fn max_pages() -> usize {
    std::env::var("GH_MAX_PAGES")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_MAX_PAGES)
}

fn http_get_json(url: &str) -> StrykeResult<Option<serde_json::Value>> {
    let req = prepare_request(agent().get(url));
    match req.call() {
        Ok(resp) => {
            let body = resp
                .into_string()
                .map_err(|e| StrykeError::runtime(format!("gh: read body: {}", e), 0))?;
            if body.is_empty() {
                return Ok(Some(serde_json::Value::Null));
            }
            let v: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| StrykeError::runtime(format!("gh: parse json: {}", e), 0))?;
            Ok(Some(v))
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            let snippet = body.chars().take(200).collect::<String>();
            Err(StrykeError::runtime(
                format!("gh: HTTP {}: {}", code, snippet),
                0,
            ))
        }
        Err(e) => Err(StrykeError::runtime(format!("gh: {}", e), 0)),
    }
}

fn http_get_text(url: &str) -> StrykeResult<Option<String>> {
    let req = prepare_request(agent().get(url));
    match req.call() {
        Ok(resp) => resp
            .into_string()
            .map(Some)
            .map_err(|e| StrykeError::runtime(format!("gh: read body: {}", e), 0)),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            let snippet = body.chars().take(200).collect::<String>();
            Err(StrykeError::runtime(
                format!("gh: HTTP {}: {}", code, snippet),
                0,
            ))
        }
        Err(e) => Err(StrykeError::runtime(format!("gh: {}", e), 0)),
    }
}

/// Fetch a single endpoint and return the parsed JSON as a StrykeValue.
/// 404 → undef.
fn single(path: &str) -> StrykeResult<StrykeValue> {
    let url = build_url(path);
    match http_get_json(&url)? {
        Some(v) => Ok(json_to_perl(v)),
        None => Ok(StrykeValue::UNDEF),
    }
}

/// Fetch a paginated list endpoint, concatenating page results into a
/// single flat list (Perl list context — `my @r = gh_repos(...)` works).
/// Stops on first empty/short page or hitting `max_pages`. 404 → empty list.
fn paginated(path: &str) -> StrykeResult<StrykeValue> {
    let per_page = 100usize;
    let cap = max_pages();
    let mut all: Vec<StrykeValue> = Vec::new();
    let join = if path.contains('?') { '&' } else { '?' };
    for page in 1..=cap {
        let url = build_url(&format!(
            "{}{}per_page={}&page={}",
            path, join, per_page, page
        ));
        let Some(v) = http_get_json(&url)? else {
            break;
        };
        match v {
            serde_json::Value::Array(items) => {
                let n = items.len();
                all.extend(items.into_iter().map(json_to_perl));
                if n < per_page {
                    break;
                }
            }
            // Search endpoints wrap results in { items: [...], total_count }
            serde_json::Value::Object(ref o) if o.contains_key("items") => {
                let Some(serde_json::Value::Array(items)) = o.get("items").cloned() else {
                    break;
                };
                let n = items.len();
                all.extend(items.into_iter().map(json_to_perl));
                if n < per_page {
                    break;
                }
            }
            _ => break,
        }
    }
    Ok(StrykeValue::array(all))
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// ── generic ────────────────────────────────────────────────────────────

/// `gh_get(PATH)` — GET an arbitrary GitHub REST endpoint. `PATH` can be
/// a relative path (`/users/MenkeTechnologies`) or a full URL. Returns
/// parsed JSON as a stryke value; 404 → `undef`.
pub fn gh_get(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    single(&arg_str(args, 0))
}

// ── user / org ─────────────────────────────────────────────────────────
/// `gh_user` — see implementation.

pub fn gh_user(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    single(&format!("/users/{}", arg_str(args, 0)))
}
/// `gh_org` — see implementation.

pub fn gh_org(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    single(&format!("/orgs/{}", arg_str(args, 0)))
}
/// `gh_followers` — see implementation.

pub fn gh_followers(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    paginated(&format!("/users/{}/followers", arg_str(args, 0)))
}
/// `gh_following` — see implementation.

pub fn gh_following(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    paginated(&format!("/users/{}/following", arg_str(args, 0)))
}

// ── repos ──────────────────────────────────────────────────────────────
/// `gh_repo` — see implementation.

pub fn gh_repo(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let s = arg_str(args, 0);
    // Accept either `gh_repo("owner/repo")` or `gh_repo("owner", "repo")`
    let path = if let Some((owner, repo)) = s.split_once('/') {
        format!("/repos/{}/{}", owner, repo)
    } else {
        format!("/repos/{}/{}", s, arg_str(args, 1))
    };
    single(&path)
}
/// `gh_repos` — see implementation.

pub fn gh_repos(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    paginated(&format!("/users/{}/repos", arg_str(args, 0)))
}
/// `gh_org_repos` — see implementation.

pub fn gh_org_repos(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    paginated(&format!("/orgs/{}/repos", arg_str(args, 0)))
}
/// `gh_starred` — see implementation.

pub fn gh_starred(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    paginated(&format!("/users/{}/starred", arg_str(args, 0)))
}

// ── gists ──────────────────────────────────────────────────────────────
/// `gh_gists` — see implementation.

pub fn gh_gists(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    paginated(&format!("/users/{}/gists", arg_str(args, 0)))
}
/// `gh_gist` — see implementation.

pub fn gh_gist(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    single(&format!("/gists/{}", arg_str(args, 0)))
}

// ── repo-scoped collections ────────────────────────────────────────────

fn split_owner_repo(args: &[StrykeValue]) -> (String, String) {
    let a = arg_str(args, 0);
    if let Some((o, r)) = a.split_once('/') {
        (o.to_string(), r.to_string())
    } else {
        (a, arg_str(args, 1))
    }
}
/// `gh_issues` — see implementation.

pub fn gh_issues(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    paginated(&format!("/repos/{}/{}/issues", o, r))
}
/// `gh_prs` — see implementation.

pub fn gh_prs(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    paginated(&format!("/repos/{}/{}/pulls", o, r))
}
/// `gh_commits` — see implementation.

pub fn gh_commits(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    paginated(&format!("/repos/{}/{}/commits", o, r))
}
/// `gh_branches` — see implementation.

pub fn gh_branches(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    paginated(&format!("/repos/{}/{}/branches", o, r))
}
/// `gh_tags` — see implementation.

pub fn gh_tags(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    paginated(&format!("/repos/{}/{}/tags", o, r))
}
/// `gh_releases` — see implementation.

pub fn gh_releases(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    paginated(&format!("/repos/{}/{}/releases", o, r))
}
/// `gh_contributors` — see implementation.

pub fn gh_contributors(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    paginated(&format!("/repos/{}/{}/contributors", o, r))
}
/// `gh_forks` — see implementation.

pub fn gh_forks(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    paginated(&format!("/repos/{}/{}/forks", o, r))
}
/// `gh_stargazers` — see implementation.

pub fn gh_stargazers(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    paginated(&format!("/repos/{}/{}/stargazers", o, r))
}
/// `gh_workflows` — see implementation.

pub fn gh_workflows(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    match single(&format!("/repos/{}/{}/actions/workflows", o, r))? {
        v if v.is_undef() => Ok(StrykeValue::array_ref(Arc::new(RwLock::new(vec![])))),
        v => {
            let ws = v
                .as_hash_ref()
                .and_then(|h| h.read().get("workflows").cloned())
                .unwrap_or(StrykeValue::UNDEF);
            Ok(ws)
        }
    }
}
/// `gh_runs` — see implementation.

pub fn gh_runs(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    match single(&format!("/repos/{}/{}/actions/runs", o, r))? {
        v if v.is_undef() => Ok(StrykeValue::array_ref(Arc::new(RwLock::new(vec![])))),
        v => {
            let runs = v
                .as_hash_ref()
                .and_then(|h| h.read().get("workflow_runs").cloned())
                .unwrap_or(StrykeValue::UNDEF);
            Ok(runs)
        }
    }
}

/// `gh_topics(OWNER, REPO)` — returns an arrayref of topic name strings.
pub fn gh_topics(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    let v = single(&format!("/repos/{}/{}/topics", o, r))?;
    if v.is_undef() {
        return Ok(StrykeValue::array_ref(Arc::new(RwLock::new(vec![]))));
    }
    let names = v
        .as_hash_ref()
        .and_then(|h| h.read().get("names").cloned())
        .unwrap_or(StrykeValue::UNDEF);
    Ok(names)
}

/// `gh_languages(OWNER, REPO)` — `{ language => bytes }` hashref.
pub fn gh_languages(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    single(&format!("/repos/{}/{}/languages", o, r))
}

/// `gh_readme(OWNER, REPO)` — base64-decoded README content as a UTF-8 string.
pub fn gh_readme(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let (o, r) = split_owner_repo(args);
    let v = single(&format!("/repos/{}/{}/readme", o, r))?;
    if v.is_undef() {
        return Ok(StrykeValue::UNDEF);
    }
    let h = match v.as_hash_ref() {
        Some(h) => h,
        None => return Ok(StrykeValue::UNDEF),
    };
    let guard = h.read();
    let encoding = guard
        .get("encoding")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let content = guard
        .get("content")
        .map(|v| v.to_string())
        .unwrap_or_default();
    drop(guard);
    if encoding == "base64" {
        let cleaned: String = content.chars().filter(|c| !c.is_whitespace()).collect();
        use base64::Engine;
        match base64::engine::general_purpose::STANDARD.decode(cleaned.as_bytes()) {
            Ok(bytes) => Ok(StrykeValue::string(
                String::from_utf8_lossy(&bytes).into_owned(),
            )),
            Err(_) => Ok(StrykeValue::string(content)),
        }
    } else {
        Ok(StrykeValue::string(content))
    }
}

// ── search ─────────────────────────────────────────────────────────────
/// `gh_search_repos` — see implementation.

pub fn gh_search_repos(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let q = url_encode(&arg_str(args, 0));
    paginated(&format!("/search/repositories?q={}", q))
}
/// `gh_search_users` — see implementation.

pub fn gh_search_users(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let q = url_encode(&arg_str(args, 0));
    paginated(&format!("/search/users?q={}", q))
}
/// `gh_search_code` — see implementation.

pub fn gh_search_code(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let q = url_encode(&arg_str(args, 0));
    paginated(&format!("/search/code?q={}", q))
}
/// `gh_search_issues` — see implementation.

pub fn gh_search_issues(args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let q = url_encode(&arg_str(args, 0));
    paginated(&format!("/search/issues?q={}", q))
}

// ── meta ───────────────────────────────────────────────────────────────
/// `gh_rate_limit` — see implementation.

pub fn gh_rate_limit(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    single("/rate_limit")
}
/// `gh_meta` — see implementation.

pub fn gh_meta(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    single("/meta")
}
/// `gh_emojis` — see implementation.

pub fn gh_emojis(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    single("/emojis")
}

/// `gh_zen()` — GitHub's "zen" endpoint. Returns a plain-text string,
/// not JSON.
pub fn gh_zen(_args: &[StrykeValue]) -> StrykeResult<StrykeValue> {
    let url = build_url("/zen");
    match http_get_text(&url)? {
        Some(s) => Ok(StrykeValue::string(s)),
        None => Ok(StrykeValue::UNDEF),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── build_url ───────────────────────────────────────────────────────

    #[test]
    fn build_url_prepends_api_root_for_leading_slash() {
        assert_eq!(build_url("/users/foo"), format!("{}/users/foo", API_ROOT));
    }

    #[test]
    fn build_url_prepends_api_root_for_bare_path() {
        assert_eq!(build_url("users/foo"), format!("{}/users/foo", API_ROOT));
    }

    #[test]
    fn build_url_passes_through_absolute_https() {
        let abs = "https://example.com/x";
        assert_eq!(build_url(abs), abs);
    }

    #[test]
    fn build_url_passes_through_absolute_http() {
        let abs = "http://example.com/x";
        assert_eq!(build_url(abs), abs);
    }

    // ─── url_encode ──────────────────────────────────────────────────────

    #[test]
    fn url_encode_preserves_unreserved_chars() {
        // RFC 3986 unreserved set: ALPHA / DIGIT / "-" / "_" / "." / "~"
        assert_eq!(url_encode("AZaz09-_.~"), "AZaz09-_.~");
    }

    #[test]
    fn url_encode_percent_encodes_space() {
        assert_eq!(url_encode("a b"), "a%20b");
    }

    #[test]
    fn url_encode_percent_encodes_slash_and_question() {
        assert_eq!(url_encode("a/b?c"), "a%2Fb%3Fc");
    }

    #[test]
    fn url_encode_multibyte_utf8_per_byte() {
        // é is U+00E9 → UTF-8 bytes C3 A9
        assert_eq!(url_encode("é"), "%C3%A9");
    }

    #[test]
    fn url_encode_empty_is_empty() {
        assert_eq!(url_encode(""), "");
    }

    // ─── split_owner_repo ────────────────────────────────────────────────

    #[test]
    fn split_owner_repo_single_arg_with_slash() {
        let (o, r) = split_owner_repo(&[StrykeValue::string("MenkeTechnologies/zpwr".into())]);
        assert_eq!(o, "MenkeTechnologies");
        assert_eq!(r, "zpwr");
    }

    #[test]
    fn split_owner_repo_two_args_no_slash() {
        let (o, r) = split_owner_repo(&[
            StrykeValue::string("Owner".into()),
            StrykeValue::string("Repo".into()),
        ]);
        assert_eq!(o, "Owner");
        assert_eq!(r, "Repo");
    }

    #[test]
    fn split_owner_repo_missing_repo_returns_empty_string() {
        // arg_str returns "" when arg missing → repo half is empty.
        let (o, r) = split_owner_repo(&[StrykeValue::string("only_owner".into())]);
        assert_eq!(o, "only_owner");
        assert_eq!(r, "");
    }

    // ─── json_to_perl ────────────────────────────────────────────────────

    #[test]
    fn json_to_perl_null_becomes_undef() {
        assert!(json_to_perl(serde_json::Value::Null).is_undef());
    }

    #[test]
    fn json_to_perl_bool_maps_to_one_or_zero() {
        assert_eq!(json_to_perl(serde_json::json!(true)).to_int(), 1);
        assert_eq!(json_to_perl(serde_json::json!(false)).to_int(), 0);
    }

    #[test]
    fn json_to_perl_object_round_trip_keys_preserved() {
        let v = json_to_perl(serde_json::json!({"k": 1, "name": "x"}));
        let h = v.as_hash_ref().expect("hash_ref");
        let g = h.read();
        assert_eq!(g.get("k").unwrap().to_int(), 1);
        assert_eq!(g.get("name").unwrap().to_string(), "x");
    }

    #[test]
    fn json_to_perl_array_length_preserved() {
        let v = json_to_perl(serde_json::json!([1, 2, 3, 4]));
        let arr = v.as_array_ref().unwrap();
        assert_eq!(arr.read().len(), 4);
    }

    // ─── arg_str ─────────────────────────────────────────────────────────

    #[test]
    fn arg_str_missing_index_returns_empty_string() {
        assert_eq!(arg_str(&[], 0), "");
        assert_eq!(arg_str(&[StrykeValue::string("x".into())], 5), "");
    }
}
