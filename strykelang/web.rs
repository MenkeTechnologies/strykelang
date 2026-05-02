//! Web framework runtime — Rails-shaped builtins for stryke.
//!
//! The `s_web` CLI in the sibling `stryke_web/` crate scaffolds new apps
//! and resources by emitting `.stk` files at build time. THIS module is
//! the request-time runtime those generated apps load — every primitive
//! the user's controller, model, and view code calls is a `web_*` builtin
//! living here next to `serve` in `builtins.rs`.
//!
//! Naming: every builtin in this module is prefixed `web_` so app code
//! reads `web_render(...)`, `web_redirect(...)`, etc. — keeps the framework
//! surface unambiguous and greppable.
//!
//! PASS 1 (this commit): routing + dispatch + render/redirect/json/text.
//! PASS 2 (next):         ERB-style template engine + view resolution.
//! PASS 3 (next):         SQLite + Model ORM (find/all/where/save/destroy).
//! PASS 4 (next):         Migrations (create_table/add_column/Migrator).

use crate::error::PerlError;
use crate::interpreter::{FlowOrError, Interpreter};
use crate::value::PerlValue;
use indexmap::IndexMap;
use parking_lot::Mutex;
use regex::Regex;
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::OnceLock;

type Result<T> = std::result::Result<T, PerlError>;

// ── Global router + config state ────────────────────────────────────────
//
// `route` / `resources` / `root` mutate the global router at script load
// time (when `config/routes.stk` is required). `boot_application` then
// reads it at request-time. Mutex is plenty — registration is one-shot.

#[derive(Clone)]
struct Route {
    verb: String,
    pattern: String,
    re: Regex,
    captures: Vec<String>,
    action: String,
}

struct Router {
    routes: Vec<Route>,
}

static ROUTER: OnceLock<Mutex<Router>> = OnceLock::new();
static APP_CONFIG: OnceLock<Mutex<IndexMap<String, PerlValue>>> = OnceLock::new();

#[derive(Clone, Default)]
struct ControllerFilters {
    before: Vec<FilterEntry>,
    after: Vec<FilterEntry>,
}

#[derive(Clone)]
struct FilterEntry {
    method: String,
    only: Vec<String>,
    except: Vec<String>,
}

static FILTERS: OnceLock<Mutex<IndexMap<String, ControllerFilters>>> = OnceLock::new();

fn filters_slot() -> &'static Mutex<IndexMap<String, ControllerFilters>> {
    FILTERS.get_or_init(|| Mutex::new(IndexMap::new()))
}

fn router() -> &'static Mutex<Router> {
    ROUTER.get_or_init(|| Mutex::new(Router { routes: Vec::new() }))
}

fn app_config() -> &'static Mutex<IndexMap<String, PerlValue>> {
    APP_CONFIG.get_or_init(|| Mutex::new(IndexMap::new()))
}

// ── Per-request state (thread-local) ────────────────────────────────────
//
// During an action's execution, `web_render` / `web_redirect` / etc.
// populate this slot. After the action returns, the dispatcher reads it
// and emits an HTTP response. Thread-local because the `serve` worker pool
// is multi-threaded; each worker dispatches requests on its own thread.

#[derive(Default)]
struct RequestState {
    request: Option<PerlValue>,
    params: IndexMap<String, PerlValue>,
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
    rendered: bool,
    /// Set by the dispatcher before invoking an action.
    #[allow(dead_code)]
    resource: String,
    #[allow(dead_code)]
    action: String,
    /// Cookies parsed from the incoming `Cookie:` header.
    cookies_in: IndexMap<String, String>,
    /// Cookies the action set via `web_set_cookie` — written into a
    /// `Set-Cookie` header on the response.
    cookies_out: Vec<(String, String, CookieOpts)>,
    /// Session payload — read from a signed cookie at request start,
    /// re-serialized into the cookie at response time if mutated.
    session: IndexMap<String, PerlValue>,
    session_dirty: bool,
    /// Flash hashref — survives one redirect.
    flash_in: IndexMap<String, PerlValue>,
    flash_out: IndexMap<String, PerlValue>,
}

#[derive(Default, Clone, Debug)]
struct CookieOpts {
    max_age: Option<i64>,
    path: Option<String>,
    domain: Option<String>,
    http_only: bool,
    secure: bool,
    same_site: Option<String>,
}

/// `(status, headers, body, cookies)` — what an action returns to the
/// outer HTTP loop after the dispatcher peels it off the request state.
type DispatchResult = (
    u16,
    Vec<(String, String)>,
    String,
    Vec<(String, String, CookieOpts)>,
);

thread_local! {
    static CURRENT: RefCell<RequestState> = RefCell::new(RequestState {
        status: 200,
        ..Default::default()
    });
}

fn with_current<R>(f: impl FnOnce(&mut RequestState) -> R) -> R {
    CURRENT.with(|cell| f(&mut cell.borrow_mut()))
}

// ── Pattern → regex compiler (Rails-style :name and *splat) ────────────

fn compile_pattern(path: &str) -> (String, Vec<String>) {
    let mut captures = Vec::new();
    let mut out = String::with_capacity(path.len() + 8);
    out.push('^');
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b':' => {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                let name = std::str::from_utf8(&bytes[start..j])
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    out.push_str(&format!("(?P<__{}__>[^/]+)", name));
                    captures.push(name);
                    i = j;
                    continue;
                }
                out.push(':');
                i += 1;
            }
            b'*' => {
                let start = i + 1;
                let mut j = start;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                let name = std::str::from_utf8(&bytes[start..j])
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    out.push_str(&format!("(?P<__{}__>.+)", name));
                    captures.push(name);
                    i = j;
                    continue;
                }
                out.push('*');
                i += 1;
            }
            c if c.is_ascii_alphanumeric() || c == b'/' || c == b'-' || c == b'_' => {
                out.push(c as char);
                i += 1;
            }
            c => {
                // Escape the regex metachar.
                out.push('\\');
                out.push(c as char);
                i += 1;
            }
        }
    }
    out.push('$');
    (out, captures)
}

// ── Route registration builtins ────────────────────────────────────────

pub(crate) fn web_route(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "web_route: usage: web_route(\"VERB /path\", \"controller#action\")",
            line,
        ));
    }
    let spec = args[0].to_string();
    let action = args[1].to_string();
    let mut parts = spec.splitn(2, char::is_whitespace);
    let verb = parts.next().unwrap_or("").trim().to_ascii_uppercase();
    let path = parts.next().unwrap_or("").trim().to_string();
    if verb.is_empty() || path.is_empty() {
        return Err(PerlError::runtime(
            "web_route: spec must be \"VERB /path\"",
            line,
        ));
    }
    let (re_src, captures) = compile_pattern(&path);
    let re = Regex::new(&re_src)
        .map_err(|e| PerlError::runtime(format!("web_route: bad pattern {}: {}", path, e), line))?;
    router().lock().routes.push(Route {
        verb,
        pattern: path,
        re,
        captures,
        action,
    });
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_resources(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.is_empty() {
        return Err(PerlError::runtime(
            "web_resources: usage: web_resources(\"posts\")",
            line,
        ));
    }
    let name = args[0].to_string();
    for (verb, path, action) in [
        ("GET", format!("/{}", name), format!("{}#index", name)),
        ("GET", format!("/{}/new", name), format!("{}#new", name)),
        ("POST", format!("/{}", name), format!("{}#create", name)),
        ("GET", format!("/{}/:id", name), format!("{}#show", name)),
        (
            "GET",
            format!("/{}/:id/edit", name),
            format!("{}#edit", name),
        ),
        (
            "PATCH",
            format!("/{}/:id", name),
            format!("{}#update", name),
        ),
        ("PUT", format!("/{}/:id", name), format!("{}#update", name)),
        (
            "DELETE",
            format!("/{}/:id", name),
            format!("{}#destroy", name),
        ),
    ] {
        web_route(
            &[
                PerlValue::string(format!("{} {}", verb, path)),
                PerlValue::string(action),
            ],
            line,
        )?;
    }
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_root(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.is_empty() {
        return Err(PerlError::runtime(
            "web_root: usage: web_root(\"controller#action\")",
            line,
        ));
    }
    web_route(
        &[PerlValue::string("GET /".to_string()), args[0].clone()],
        line,
    )
}

pub(crate) fn web_application_config(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let cfg = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let map = if let Some(hr) = cfg.as_hash_ref() {
        hr.read().clone()
    } else if let Some(hm) = cfg.as_hash_map() {
        hm.clone()
    } else {
        return Err(PerlError::runtime(
            "web_application_config: arg must be a hashref",
            line,
        ));
    };
    let mut g = app_config().lock();
    for (k, v) in map {
        g.insert(k, v);
    }
    Ok(PerlValue::UNDEF)
}

// ── Per-request render / redirect / JSON / params / request helpers ────
//
// Called from inside controller actions. Each one mutates the thread-local
// `CURRENT` slot; the dispatcher reads `CURRENT` after the action returns
// and emits the HTTP response.

fn parse_render_opts(args: &[PerlValue]) -> IndexMap<String, PerlValue> {
    // `web_render(html => "...", status => 200)` — args is a flat list of
    // alternating key, value pairs. Build an IndexMap.
    let mut out = IndexMap::new();
    let mut i = 0;
    while i + 1 < args.len() {
        let k = args[i].to_string();
        let v = args[i + 1].clone();
        out.insert(k, v);
        i += 2;
    }
    out
}

pub(crate) fn web_render_dispatch(
    interp: &mut Interpreter,
    args: &[PerlValue],
    line: usize,
) -> Result<PerlValue> {
    interp.web_render(args, line)
}

pub(crate) fn web_redirect(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.is_empty() {
        return Err(PerlError::runtime(
            "web_redirect: usage: web_redirect(\"/path\")",
            line,
        ));
    }
    let url = args[0].to_string();
    let status = if args.len() >= 2 {
        args[1].to_int() as u16
    } else {
        302
    };
    with_current(|cur| {
        cur.status = status;
        cur.headers = vec![
            ("location".into(), url.clone()),
            ("content-type".into(), "text/plain; charset=utf-8".into()),
        ];
        cur.body = format!("Redirecting to {}", url);
        cur.rendered = true;
    });
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_json(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let val = args.first().cloned().unwrap_or(PerlValue::UNDEF);
    let body = crate::native_data::json_encode(&val).unwrap_or_else(|_| "null".to_string());
    let status = if args.len() >= 2 {
        args[1].to_int() as u16
    } else {
        200
    };
    with_current(|cur| {
        cur.status = status;
        cur.headers = vec![(
            "content-type".into(),
            "application/json; charset=utf-8".into(),
        )];
        cur.body = body;
        cur.rendered = true;
    });
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_text(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let body = args.first().map(|v| v.to_string()).unwrap_or_default();
    let status = if args.len() >= 2 {
        args[1].to_int() as u16
    } else {
        200
    };
    with_current(|cur| {
        cur.status = status;
        cur.headers = vec![("content-type".into(), "text/plain; charset=utf-8".into())];
        cur.body = body;
        cur.rendered = true;
    });
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_params(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let map = with_current(|cur| cur.params.clone());
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(map))))
}

pub(crate) fn web_request(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    Ok(with_current(|cur| {
        cur.request.clone().unwrap_or(PerlValue::UNDEF)
    }))
}

/// `web_before_action("authenticate", controller => "PostsController",
/// only => ["edit", "update"], except => ["index"])` — register a
/// before-filter that runs before each named action on the controller.
pub(crate) fn web_before_action(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    register_filter(args, line, true)
}

pub(crate) fn web_after_action(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    register_filter(args, line, false)
}

fn register_filter(args: &[PerlValue], line: usize, before: bool) -> Result<PerlValue> {
    let method = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| {
            PerlError::runtime(
                "web_before_action: usage: web_before_action(\"method\", controller => \"X\", only => [...], except => [...])",
                line,
            )
        })?;
    let opts = parse_opts(&args[1..]);
    let controller = opts
        .get("controller")
        .map(|v| v.to_string())
        .ok_or_else(|| {
            PerlError::runtime(
                "web_before_action: pass controller => \"PostsController\"",
                line,
            )
        })?;
    let only = opts.get("only").map(split_action_list).unwrap_or_default();
    let except = opts
        .get("except")
        .map(split_action_list)
        .unwrap_or_default();
    let entry = FilterEntry {
        method,
        only,
        except,
    };
    let mut g = filters_slot().lock();
    let f = g.entry(controller).or_default();
    if before {
        f.before.push(entry);
    } else {
        f.after.push(entry);
    }
    Ok(PerlValue::UNDEF)
}

fn parse_opts(args: &[PerlValue]) -> IndexMap<String, PerlValue> {
    let mut out = IndexMap::new();
    let mut i = 0;
    while i + 1 < args.len() {
        out.insert(args[i].to_string(), args[i + 1].clone());
        i += 2;
    }
    out
}

fn split_action_list(v: &PerlValue) -> Vec<String> {
    if let Some(arr) = v.as_array_ref() {
        return arr.read().iter().map(|x| x.to_string()).collect();
    }
    v.clone().to_list().iter().map(|x| x.to_string()).collect()
}

pub(crate) fn web_routes_table(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let r = router().lock();
    let mut out = String::new();
    out.push_str(&format!(
        "{:<8}  {:<30}  {}\n",
        "Verb", "Path", "Controller#Action"
    ));
    out.push_str(&format!(
        "{:<8}  {:<30}  {}\n",
        "----", "----", "-----------------"
    ));
    for r in &r.routes {
        out.push_str(&format!("{:<8}  {:<30}  {}\n", r.verb, r.pattern, r.action));
    }
    Ok(PerlValue::string(out))
}

// ── Session / cookie / flash / strong-params / password ───────────────

pub(crate) fn web_session(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let map = with_current(|cur| cur.session.clone());
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(map))))
}

pub(crate) fn web_session_set(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "web_session_set: usage: web_session_set(\"key\", $value)",
            line,
        ));
    }
    let k = args[0].to_string();
    let v = args[1].clone();
    with_current(|cur| {
        cur.session.insert(k, v);
        cur.session_dirty = true;
    });
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_session_get(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let k = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(with_current(|cur| {
        cur.session.get(&k).cloned().unwrap_or(PerlValue::UNDEF)
    }))
}

pub(crate) fn web_session_clear(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    with_current(|cur| {
        cur.session.clear();
        cur.session_dirty = true;
    });
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_set_cookie(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "web_set_cookie: usage: web_set_cookie(\"name\", \"value\", path => \"/\", max_age => 3600, http_only => 1, secure => 1, same_site => \"Lax\")",
            line,
        ));
    }
    let name = args[0].to_string();
    let value = args[1].to_string();
    let mut opts = CookieOpts::default();
    let mut i = 2;
    while i + 1 < args.len() {
        let key = args[i].to_string();
        let val = &args[i + 1];
        match key.as_str() {
            "path" => opts.path = Some(val.to_string()),
            "domain" => opts.domain = Some(val.to_string()),
            "max_age" => opts.max_age = Some(val.to_int()),
            "http_only" | "httponly" => opts.http_only = val.to_int() != 0,
            "secure" => opts.secure = val.to_int() != 0,
            "same_site" | "samesite" => opts.same_site = Some(val.to_string()),
            _ => {}
        }
        i += 2;
    }
    with_current(|cur| cur.cookies_out.push((name, value, opts)));
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_cookies(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let map = with_current(|cur| {
        let mut out = IndexMap::new();
        for (k, v) in &cur.cookies_in {
            out.insert(k.clone(), PerlValue::string(v.clone()));
        }
        out
    });
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(map))))
}

/// Replaces the stub `web_flash`. Read the incoming flash, set the
/// outgoing flash with `web_flash_set("notice", "Saved!")`. Outgoing
/// flash is cleared after one redirect (the cookie's `Max-Age=0` is
/// emitted otherwise).
pub(crate) fn web_flash_set(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "web_flash_set: usage: web_flash_set(\"notice\", \"Saved!\")",
            line,
        ));
    }
    let k = args[0].to_string();
    let v = args[1].clone();
    with_current(|cur| {
        cur.flash_out.insert(k, v);
    });
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_flash_get(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let k = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(with_current(|cur| {
        cur.flash_in.get(&k).cloned().unwrap_or(PerlValue::UNDEF)
    }))
}

/// `web_permit($params, "title", "body")` → new hashref containing only
/// those keys. Mirrors Rails strong params; rejects everything else.
pub(crate) fn web_permit(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.is_empty() {
        return Err(PerlError::runtime(
            "web_permit: usage: web_permit($params, \"key1\", \"key2\", ...)",
            line,
        ));
    }
    let src = args[0]
        .as_hash_map()
        .or_else(|| args[0].as_hash_ref().map(|h| h.read().clone()))
        .ok_or_else(|| PerlError::runtime("web_permit: first arg must be a hashref", line))?;
    let mut out = IndexMap::new();
    for keyv in &args[1..] {
        let k = keyv.to_string();
        if let Some(v) = src.get(&k) {
            out.insert(k, v.clone());
        }
    }
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(out))))
}

/// SHA-256(salt + password), salt prefixed with `web1$saltHex$`. Not
/// bcrypt — bcrypt would need a dependency. Strong-enough for stryke
/// web v0; users wiring real auth can swap to bcrypt later via
/// `crypt_util`.
pub(crate) fn web_password_hash(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let pw = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_password_hash: password required", line))?;
    let salt = random_bytes(16);
    let salt_hex = hex_encode(&salt);
    let combined = format!("{}{}", salt_hex, pw);
    let digest = sha256_hex(combined.as_bytes());
    Ok(PerlValue::string(format!("web1${}${}", salt_hex, digest)))
}

pub(crate) fn web_password_verify(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let pw = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_password_verify: password required", line))?;
    let stored = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_password_verify: stored hash required", line))?;
    let parts: Vec<&str> = stored.splitn(3, '$').collect();
    if parts.len() != 3 || parts[0] != "web1" {
        return Ok(PerlValue::integer(0));
    }
    let salt_hex = parts[1];
    let want = parts[2];
    let combined = format!("{}{}", salt_hex, pw);
    let got = sha256_hex(combined.as_bytes());
    Ok(PerlValue::integer(
        if constant_time_eq(got.as_bytes(), want.as_bytes()) {
            1
        } else {
            0
        },
    ))
}

static RNG_STATE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn random_bytes(n: usize) -> Vec<u8> {
    use std::sync::atomic::Ordering;
    use std::time::SystemTime;
    let mut state = RNG_STATE.load(Ordering::Relaxed);
    if state == 0 {
        let seed = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        state = seed ^ std::process::id() as u64 ^ 0x9E37_79B9_7F4A_7C15;
        if state == 0 {
            state = 0xDEADBEEFCAFEBABE;
        }
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.push((state & 0xFF) as u8);
    }
    RNG_STATE.store(state, Ordering::Relaxed);
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn sha256_hex(input: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(input);
    hex_encode(&h.finalize())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

// ── Cache / i18n / log / response helpers / signed payloads ───────────

/// `(value, optional_unix_expiry_secs)`.
type CacheEntry = (String, Option<i64>);
type CacheMap = IndexMap<String, CacheEntry>;
type LocaleMap = IndexMap<String, IndexMap<String, String>>;

static CACHE: OnceLock<Mutex<CacheMap>> = OnceLock::new();
static LOCALE: OnceLock<Mutex<LocaleMap>> = OnceLock::new();

fn cache_slot() -> &'static Mutex<CacheMap> {
    CACHE.get_or_init(|| Mutex::new(IndexMap::new()))
}

fn locale_slot() -> &'static Mutex<LocaleMap> {
    LOCALE.get_or_init(|| Mutex::new(IndexMap::new()))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn web_cache_get(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let key = args.first().map(|v| v.to_string()).unwrap_or_default();
    let now = unix_now();
    let map = cache_slot().lock();
    if let Some((val, expires)) = map.get(&key) {
        if expires.map(|e| e > now).unwrap_or(true) {
            return Ok(PerlValue::string(val.clone()));
        }
    }
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_cache_set(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "web_cache_set: usage: web_cache_set(\"key\", \"value\", ttl => 60)",
            line,
        ));
    }
    let key = args[0].to_string();
    let val = args[1].to_string();
    let mut ttl: Option<i64> = None;
    let mut i = 2;
    while i + 1 < args.len() {
        if args[i].to_string() == "ttl" {
            ttl = Some(args[i + 1].to_int());
        }
        i += 2;
    }
    let expires = ttl.map(|t| unix_now() + t);
    cache_slot().lock().insert(key, (val, expires));
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_cache_delete(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let key = args.first().map(|v| v.to_string()).unwrap_or_default();
    cache_slot().lock().shift_remove(&key);
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_cache_clear(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    cache_slot().lock().clear();
    Ok(PerlValue::UNDEF)
}

/// `web_t("welcome.title")` — translate from the loaded locale dict.
/// Falls back to the key itself when not found so views never crash.
pub(crate) fn web_t(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let key = args.first().map(|v| v.to_string()).unwrap_or_default();
    let lang = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "en".to_string());
    let dict = locale_slot().lock();
    if let Some(map) = dict.get(&lang) {
        if let Some(s) = map.get(&key) {
            return Ok(PerlValue::string(s.clone()));
        }
    }
    Ok(PerlValue::string(key))
}

/// `web_load_locale("en", +{ "welcome.title" => "Hello" })` registers
/// translations. Apps typically call this once at boot from a YAML/JSON
/// file (or the seed step).
pub(crate) fn web_load_locale(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let lang = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_load_locale: language required", line))?;
    let map = args
        .get(1)
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .ok_or_else(|| PerlError::runtime("web_load_locale: second arg must be a hashref", line))?;
    let mut flat = IndexMap::new();
    for (k, v) in map {
        flat.insert(k, v.to_string());
    }
    locale_slot().lock().insert(lang, flat);
    Ok(PerlValue::UNDEF)
}

/// `web_log("info", "request", $req)` — append to `log/$ENV.log`. Best-
/// effort, never throws.
pub(crate) fn web_log(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let level = args
        .first()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "info".to_string());
    let mut parts: Vec<String> = args.iter().skip(1).map(|v| v.to_string()).collect();
    if parts.is_empty() {
        parts.push(String::new());
    }
    let env = std::env::var("STRYKE_ENV").unwrap_or_else(|_| "development".to_string());
    let line_str = format!("[{}] [{}] {}\n", current_iso_time(), level, parts.join(" "));
    let _ = std::fs::create_dir_all("log");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("log/{}.log", env))
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(line_str.as_bytes())
        });
    Ok(PerlValue::UNDEF)
}

fn current_iso_time() -> String {
    let now = unix_now();
    let s = now % 60;
    let m = (now / 60) % 60;
    let h = (now / 3600) % 24;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// `web_set_header("X-Frame-Options", "DENY")`.
pub(crate) fn web_set_header(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "web_set_header: usage: web_set_header(\"name\", \"value\")",
            line,
        ));
    }
    let k = args[0].to_string().to_lowercase();
    let v = args[1].to_string();
    with_current(|cur| {
        cur.headers.retain(|(hk, _)| hk.to_lowercase() != k);
        cur.headers.push((k, v));
    });
    Ok(PerlValue::UNDEF)
}

/// `web_status(404)` — set status before render.
pub(crate) fn web_status(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    if let Some(v) = args.first() {
        let n = v.to_int() as u16;
        with_current(|cur| cur.status = n);
    }
    Ok(PerlValue::UNDEF)
}

/// RFC4122-ish UUID v4 (time-seeded — fine for IDs, not crypto-strength).
pub(crate) fn web_uuid(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let bytes = random_bytes(16);
    let mut hex = hex_encode(&bytes);
    hex.replace_range(12..13, "4");
    hex.replace_range(16..17, "8");
    Ok(PerlValue::string(format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )))
}

/// Returns the current unix-seconds timestamp.
pub(crate) fn web_now(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    Ok(PerlValue::integer(unix_now()))
}

/// `web_signed("payload")` returns `payload.HMAC` so the caller can
/// hand the round-tripped value to `web_unsigned` without trusting the
/// client. Used for one-click email links / password-reset tokens.
pub(crate) fn web_signed(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let payload = args.first().map(|v| v.to_string()).unwrap_or_default();
    let secret = secret_key();
    let mac = sha256_hex((secret + &payload).as_bytes());
    Ok(PerlValue::string(format!("{}.{}", payload, &mac[..32])))
}

pub(crate) fn web_unsigned(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let signed = args.first().map(|v| v.to_string()).unwrap_or_default();
    let Some((payload, mac)) = signed.rsplit_once('.') else {
        return Ok(PerlValue::UNDEF);
    };
    let secret = secret_key();
    let expected = sha256_hex((secret + payload).as_bytes());
    if constant_time_eq(mac.as_bytes(), &expected.as_bytes()[..32]) {
        Ok(PerlValue::string(payload.to_string()))
    } else {
        Ok(PerlValue::UNDEF)
    }
}

fn secret_key() -> String {
    let cfg = app_config().lock();
    if let Some(v) = cfg.get("secret_key_base") {
        let s = v.to_string();
        if !s.is_empty() {
            return s;
        }
    }
    "stryke_web_default_secret_change_me".to_string()
}

// ── JWT (HS256) ────────────────────────────────────────────────────────

pub(crate) fn web_jwt_encode(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let payload = args
        .first()
        .ok_or_else(|| PerlError::runtime("web_jwt_encode: payload (hashref) required", line))?;
    let payload_map = payload
        .as_hash_map()
        .or_else(|| payload.as_hash_ref().map(|h| h.read().clone()))
        .ok_or_else(|| PerlError::runtime("web_jwt_encode: payload must be a hashref", line))?;
    let mut wrapped = IndexMap::new();
    for (k, v) in payload_map {
        wrapped.insert(k, v);
    }
    let header = r#"{"alg":"HS256","typ":"JWT"}"#;
    let header_b64 = base64url_encode(header.as_bytes());
    let payload_val = PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(wrapped)));
    let payload_json =
        crate::native_data::json_encode(&payload_val).unwrap_or_else(|_| "{}".into());
    let payload_b64 = base64url_encode(payload_json.as_bytes());
    let signing_input = format!("{}.{}", header_b64, payload_b64);
    let mac = hmac_sha256_b64(&signing_input, &secret_key());
    Ok(PerlValue::string(format!("{}.{}", signing_input, mac)))
}

pub(crate) fn web_jwt_decode(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let token = args.first().map(|v| v.to_string()).unwrap_or_default();
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Ok(PerlValue::UNDEF);
    }
    let signing_input = format!("{}.{}", parts[0], parts[1]);
    let want = hmac_sha256_b64(&signing_input, &secret_key());
    if !constant_time_eq(want.as_bytes(), parts[2].as_bytes()) {
        return Ok(PerlValue::UNDEF);
    }
    let payload_bytes = match base64url_decode(parts[1]) {
        Some(b) => b,
        None => return Ok(PerlValue::UNDEF),
    };
    let json = match std::str::from_utf8(&payload_bytes) {
        Ok(s) => s,
        Err(_) => return Ok(PerlValue::UNDEF),
    };
    crate::native_data::json_decode(json).or(Ok(PerlValue::UNDEF))
}

fn hmac_sha256_b64(input: &str, key: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).expect("hmac key");
    mac.update(input.as_bytes());
    base64url_encode(&mac.finalize().into_bytes())
}

// ── Rate limiting ──────────────────────────────────────────────────────
//
// Token-bucket style: each `(key, window_seconds)` keeps a Vec<unix_secs>
// of recent hits. `web_rate_limit("login:$ip", 5, 60)` returns 1 if the
// hit was allowed (incrementing the counter), 0 if the limit is hit.

static RATE_BUCKETS: OnceLock<Mutex<IndexMap<String, Vec<i64>>>> = OnceLock::new();

fn rate_buckets() -> &'static Mutex<IndexMap<String, Vec<i64>>> {
    RATE_BUCKETS.get_or_init(|| Mutex::new(IndexMap::new()))
}

pub(crate) fn web_rate_limit(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.len() < 3 {
        return Err(PerlError::runtime(
            "web_rate_limit: usage: web_rate_limit(\"key\", limit_n, window_seconds)",
            line,
        ));
    }
    let key = args[0].to_string();
    let limit = args[1].to_int().max(1);
    let window = args[2].to_int().max(1);
    let now = unix_now();
    let mut g = rate_buckets().lock();
    let bucket = g.entry(key).or_default();
    bucket.retain(|t| now - t < window);
    if (bucket.len() as i64) >= limit {
        return Ok(PerlValue::integer(0));
    }
    bucket.push(now);
    Ok(PerlValue::integer(1))
}

// ── TOTP (RFC 6238 — SHA1, 30s, 6 digits) ──────────────────────────────

pub(crate) fn web_otp_secret(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    // 20 random bytes encoded as base32 (no padding) — what authenticator
    // apps expect.
    let bytes = random_bytes(20);
    let s = base32_encode(&bytes);
    Ok(PerlValue::string(s))
}

pub(crate) fn web_otp_generate(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let secret_b32 = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_otp_generate: secret required", line))?;
    let key = match base32_decode(&secret_b32) {
        Some(k) => k,
        None => {
            return Err(PerlError::runtime(
                "web_otp_generate: bad base32 secret",
                line,
            ))
        }
    };
    let counter = (unix_now() / 30) as u64;
    let code = totp_code(&key, counter);
    Ok(PerlValue::string(format!("{:06}", code)))
}

pub(crate) fn web_otp_verify(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let secret_b32 = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_otp_verify: secret required", line))?;
    let code = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_otp_verify: code required", line))?;
    let key = match base32_decode(&secret_b32) {
        Some(k) => k,
        None => return Ok(PerlValue::integer(0)),
    };
    let counter = (unix_now() / 30) as u64;
    // Allow ±1 step (30s skew either side).
    for delta in &[-1i64, 0, 1] {
        let c = (counter as i64 + delta) as u64;
        let want = format!("{:06}", totp_code(&key, c));
        if constant_time_eq(want.as_bytes(), code.as_bytes()) {
            return Ok(PerlValue::integer(1));
        }
    }
    Ok(PerlValue::integer(0))
}

fn totp_code(key: &[u8], counter: u64) -> u32 {
    use hmac::{Hmac, Mac};
    use sha1::Sha1;
    type HmacSha1 = Hmac<Sha1>;
    let mut mac = HmacSha1::new_from_slice(key).expect("hmac key");
    mac.update(&counter.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let offset = (h[h.len() - 1] & 0x0f) as usize;
    let bin = ((h[offset] as u32 & 0x7f) << 24)
        | ((h[offset + 1] as u32) << 16)
        | ((h[offset + 2] as u32) << 8)
        | (h[offset + 3] as u32);
    bin % 1_000_000
}

fn base32_encode(bytes: &[u8]) -> String {
    base32::encode(base32::Alphabet::Rfc4648 { padding: false }, bytes)
}

fn base32_decode(s: &str) -> Option<Vec<u8>> {
    base32::decode(base32::Alphabet::Rfc4648 { padding: false }, s)
}

// ── Faker ──────────────────────────────────────────────────────────────

pub(crate) fn web_faker_name(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let firsts = [
        "Alice", "Bob", "Carol", "Dan", "Eve", "Frank", "Grace", "Heidi", "Ivan", "Judy", "Kim",
        "Leo", "Mallory", "Niaj", "Olivia", "Peggy", "Quinn", "Rupert", "Sybil", "Trent", "Ursula",
        "Victor", "Walter", "Xena", "Yvonne", "Zane",
    ];
    let lasts = [
        "Adams",
        "Brown",
        "Chen",
        "Davis",
        "Evans",
        "Foster",
        "Garcia",
        "Harris",
        "Iqbal",
        "Jones",
        "Khan",
        "Lopez",
        "Miller",
        "Nguyen",
        "O'Brien",
        "Patel",
        "Quinn",
        "Rivera",
        "Smith",
        "Tran",
        "Underwood",
        "Vasquez",
        "Williams",
        "Xu",
        "Young",
        "Zhang",
    ];
    let s = format!("{} {}", pick(&firsts), pick(&lasts));
    Ok(PerlValue::string(s))
}

pub(crate) fn web_faker_email(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let users = ["alice", "bob", "carol", "dan", "eve", "frank", "grace"];
    let domains = [
        "example.com",
        "test.io",
        "demo.net",
        "stryke.dev",
        "mail.app",
    ];
    let n = (random_bytes(2)[0] as i64) % 1000;
    Ok(PerlValue::string(format!(
        "{}{}@{}",
        pick(&users),
        n,
        pick(&domains)
    )))
}

pub(crate) fn web_faker_sentence(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let words = [
        "stryke",
        "neon",
        "cyberpunk",
        "matrix",
        "ghost",
        "shell",
        "shadow",
        "void",
        "echo",
        "spire",
        "flux",
        "axion",
        "quasar",
        "pulse",
        "stack",
        "lattice",
        "vector",
        "kernel",
        "phantom",
        "aurora",
        "nova",
        "quantum",
    ];
    let n = 5 + (random_bytes(1)[0] as usize % 8);
    let mut out = String::new();
    for i in 0..n {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(pick(&words));
    }
    if let Some(c) = out.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    out.push('.');
    Ok(PerlValue::string(out))
}

pub(crate) fn web_faker_paragraph(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let n = 3 + (random_bytes(1)[0] as usize % 4);
    let mut out = String::new();
    for i in 0..n {
        if i > 0 {
            out.push(' ');
        }
        let s = web_faker_sentence(&[], 0)?.to_string();
        out.push_str(&s);
    }
    Ok(PerlValue::string(out))
}

pub(crate) fn web_faker_int(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let min = args.first().map(|v| v.to_int()).unwrap_or(0);
    let max = args.get(1).map(|v| v.to_int()).unwrap_or(100);
    let span = (max - min).max(1) as u64;
    let bytes = random_bytes(8);
    let mut n: u64 = 0;
    for b in bytes {
        n = (n << 8) | (b as u64);
    }
    Ok(PerlValue::integer(min + (n % span) as i64))
}

fn pick<'a>(arr: &'a [&'a str]) -> &'a str {
    let i = (random_bytes(1)[0] as usize) % arr.len();
    arr[i]
}

// ── Markdown ───────────────────────────────────────────────────────────
//
// Tiny commonmark-subset: headings, bold/italic, inline code, links,
// fenced code, paragraphs, lists. Enough for blog posts and docs without
// pulling in a full crate.

pub(crate) fn web_markdown(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let src = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(render_markdown(&src)))
}

fn render_markdown(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 64);
    let mut in_code = false;
    let mut code_lang;
    let mut in_list = false;
    let mut paragraph: Vec<String> = Vec::new();

    let flush_para = |para: &mut Vec<String>, out: &mut String| {
        if !para.is_empty() {
            out.push_str("<p>");
            out.push_str(&inline_md(&para.join(" ")));
            out.push_str("</p>\n");
            para.clear();
        }
    };
    let close_list = |open: &mut bool, out: &mut String| {
        if *open {
            out.push_str("</ul>\n");
            *open = false;
        }
    };

    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("```") {
            if !in_code {
                flush_para(&mut paragraph, &mut out);
                close_list(&mut in_list, &mut out);
                in_code = true;
                code_lang = rest.trim().to_string();
                out.push_str(&format!(
                    "<pre><code class=\"language-{}\">",
                    html_escape(&code_lang)
                ));
                let _ = &code_lang;
            } else {
                in_code = false;
                out.push_str("</code></pre>\n");
            }
            continue;
        }
        if in_code {
            out.push_str(&html_escape(line));
            out.push('\n');
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            flush_para(&mut paragraph, &mut out);
            close_list(&mut in_list, &mut out);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("###### ") {
            flush_para(&mut paragraph, &mut out);
            close_list(&mut in_list, &mut out);
            out.push_str(&format!("<h6>{}</h6>\n", inline_md(rest)));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("##### ") {
            flush_para(&mut paragraph, &mut out);
            close_list(&mut in_list, &mut out);
            out.push_str(&format!("<h5>{}</h5>\n", inline_md(rest)));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("#### ") {
            flush_para(&mut paragraph, &mut out);
            close_list(&mut in_list, &mut out);
            out.push_str(&format!("<h4>{}</h4>\n", inline_md(rest)));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("### ") {
            flush_para(&mut paragraph, &mut out);
            close_list(&mut in_list, &mut out);
            out.push_str(&format!("<h3>{}</h3>\n", inline_md(rest)));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            flush_para(&mut paragraph, &mut out);
            close_list(&mut in_list, &mut out);
            out.push_str(&format!("<h2>{}</h2>\n", inline_md(rest)));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            flush_para(&mut paragraph, &mut out);
            close_list(&mut in_list, &mut out);
            out.push_str(&format!("<h1>{}</h1>\n", inline_md(rest)));
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            flush_para(&mut paragraph, &mut out);
            if !in_list {
                out.push_str("<ul>\n");
                in_list = true;
            }
            out.push_str(&format!("  <li>{}</li>\n", inline_md(rest)));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("> ") {
            flush_para(&mut paragraph, &mut out);
            close_list(&mut in_list, &mut out);
            out.push_str(&format!("<blockquote>{}</blockquote>\n", inline_md(rest)));
            continue;
        }
        if trimmed == "---" || trimmed == "***" {
            flush_para(&mut paragraph, &mut out);
            close_list(&mut in_list, &mut out);
            out.push_str("<hr>\n");
            continue;
        }
        paragraph.push(line.to_string());
    }
    flush_para(&mut paragraph, &mut out);
    close_list(&mut in_list, &mut out);
    if in_code {
        out.push_str("</code></pre>\n");
    }
    out
}

fn inline_md(s: &str) -> String {
    let escaped = html_escape(s);
    let mut out = String::with_capacity(escaped.len());
    let bytes = escaped.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Bold: **...**
        if i + 1 < bytes.len() && bytes[i] == b'*' && bytes[i + 1] == b'*' {
            if let Some(end) = find_close_marker(bytes, i + 2, b"**") {
                let inner = std::str::from_utf8(&bytes[i + 2..end]).unwrap_or("");
                out.push_str(&format!("<strong>{}</strong>", inner));
                i = end + 2;
                continue;
            }
        }
        // Italic: *...*
        if bytes[i] == b'*' {
            if let Some(end) = find_close_marker(bytes, i + 1, b"*") {
                let inner = std::str::from_utf8(&bytes[i + 1..end]).unwrap_or("");
                out.push_str(&format!("<em>{}</em>", inner));
                i = end + 1;
                continue;
            }
        }
        // Inline code: `...`
        if bytes[i] == b'`' {
            if let Some(end) = find_close_marker(bytes, i + 1, b"`") {
                let inner = std::str::from_utf8(&bytes[i + 1..end]).unwrap_or("");
                out.push_str(&format!("<code>{}</code>", inner));
                i = end + 1;
                continue;
            }
        }
        // Links: [text](href)
        if bytes[i] == b'[' {
            if let Some(close_text) = find_close_marker(bytes, i + 1, b"]") {
                if close_text + 1 < bytes.len() && bytes[close_text + 1] == b'(' {
                    if let Some(close_url) = find_close_marker(bytes, close_text + 2, b")") {
                        let text = std::str::from_utf8(&bytes[i + 1..close_text]).unwrap_or("");
                        let url =
                            std::str::from_utf8(&bytes[close_text + 2..close_url]).unwrap_or("");
                        out.push_str(&format!("<a href=\"{}\">{}</a>", url, text));
                        i = close_url + 1;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn find_close_marker(bytes: &[u8], from: usize, marker: &[u8]) -> Option<usize> {
    let mut i = from;
    while i + marker.len() <= bytes.len() {
        if &bytes[i..i + marker.len()] == marker {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ── HTTP cache (ETag / 304 Not Modified) ───────────────────────────────

pub(crate) fn web_etag(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let payload = args.first().map(|v| v.to_string()).unwrap_or_default();
    let etag = format!("\"{}\"", &sha256_hex(payload.as_bytes())[..16]);
    let inm = with_current(|cur| {
        cur.request
            .as_ref()
            .and_then(|r| r.as_hash_ref())
            .and_then(|h| h.read().get("headers").cloned())
            .and_then(|hv| hv.as_hash_ref().map(|h| h.read().clone()))
            .and_then(|m| m.get("if-none-match").map(|v| v.to_string()))
    });
    if let Some(client_tag) = inm {
        if client_tag == etag {
            with_current(|cur| {
                cur.status = 304;
                cur.body = String::new();
                cur.headers = vec![("etag".into(), etag.clone())];
                cur.rendered = true;
            });
            return Ok(PerlValue::integer(1));
        }
    }
    with_current(|cur| {
        cur.headers.push(("etag".into(), etag.clone()));
    });
    Ok(PerlValue::integer(0))
}

// ── CSV export ─────────────────────────────────────────────────────────

pub(crate) fn web_csv(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    // Accept either `web_csv(\@rows)` (array_ref) or `web_csv(@rows)`
    // (flat list of hashrefs) — both shapes occur naturally in stryke
    // depending on whether the caller has a ref or a list in hand.
    let rows: Vec<PerlValue> = if args.len() == 1 {
        if let Some(arr) = args[0].as_array_ref() {
            arr.read().clone()
        } else {
            args.to_vec()
        }
    } else {
        args.to_vec()
    };
    let mut out = String::new();
    let mut header_written = false;
    for row in &rows {
        let h = match row.as_hash_ref() {
            Some(h) => h,
            None => continue,
        };
        let map = h.read().clone();
        if !header_written {
            let cols: Vec<String> = map.keys().map(|k| csv_field(k)).collect();
            out.push_str(&cols.join(","));
            out.push('\n');
            header_written = true;
        }
        let cells: Vec<String> = map.values().map(|v| csv_field(&v.to_string())).collect();
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    Ok(PerlValue::string(out))
}

fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

// ── Content blocks (yield_content / content_for) ───────────────────────

thread_local! {
    static CONTENT_BLOCKS: RefCell<IndexMap<String, String>> = RefCell::new(IndexMap::new());
}

pub(crate) fn web_content_for(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "web_content_for: usage: web_content_for(\"name\", \"<html>\")",
            line,
        ));
    }
    let name = args[0].to_string();
    let body = args[1].to_string();
    CONTENT_BLOCKS.with(|c| {
        c.borrow_mut().insert(name, body);
    });
    Ok(PerlValue::UNDEF)
}

pub(crate) fn web_yield_content(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let name = args.first().map(|v| v.to_string()).unwrap_or_default();
    let s = CONTENT_BLOCKS.with(|c| c.borrow().get(&name).cloned());
    Ok(PerlValue::string(s.unwrap_or_default()))
}

// ── Render partial ─────────────────────────────────────────────────────
//
// `web_render_partial("posts/form", +{ post => $p })` reads
// `app/views/posts/_form.html.erb` and returns the rendered string for
// embedding inside another template. The leading underscore matches the
// Rails partial convention.

impl Interpreter {
    pub(crate) fn web_render_partial(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> Result<PerlValue> {
        let name = args.first().map(|v| v.to_string()).ok_or_else(|| {
            PerlError::runtime(
                "web_render_partial: usage: web_render_partial(\"path\", locals_hashref)",
                line,
            )
        })?;
        let locals = args
            .get(1)
            .and_then(|v| {
                v.as_hash_map()
                    .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
            })
            .unwrap_or_default();
        let (dir, file) = match name.rsplit_once('/') {
            Some((d, f)) => (d.to_string(), f.to_string()),
            None => (String::new(), name.clone()),
        };
        let underscore = if file.starts_with('_') {
            file.clone()
        } else {
            format!("_{}", file)
        };
        let path = if dir.is_empty() {
            format!("app/views/{}.html.erb", underscore)
        } else {
            format!("app/views/{}/{}.html.erb", dir, underscore)
        };
        let src = std::fs::read_to_string(&path).map_err(|e| {
            PerlError::runtime(
                format!("web_render_partial: can't read {}: {}", path, e),
                line,
            )
        })?;
        self.render_erb(&src, &locals, line).map(PerlValue::string)
    }
}

// ── Security headers / CSP ────────────────────────────────────────────

pub(crate) fn web_security_headers(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    with_current(|cur| {
        cur.headers.push(("x-frame-options".into(), "DENY".into()));
        cur.headers
            .push(("x-content-type-options".into(), "nosniff".into()));
        cur.headers.push((
            "referrer-policy".into(),
            "strict-origin-when-cross-origin".into(),
        ));
        cur.headers.push((
            "strict-transport-security".into(),
            "max-age=31536000; includeSubDomains".into(),
        ));
        cur.headers.push((
            "permissions-policy".into(),
            "geolocation=(), microphone=(), camera=()".into(),
        ));
    });
    Ok(PerlValue::UNDEF)
}

// ── OpenAPI route dump ─────────────────────────────────────────────────

pub(crate) fn web_openapi(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let r = router().lock();
    let mut paths: IndexMap<String, IndexMap<String, PerlValue>> = IndexMap::new();
    for route in &r.routes {
        let oas_path = openapi_path(&route.pattern);
        let entry = paths.entry(oas_path).or_default();
        let mut op = IndexMap::new();
        op.insert(
            "operationId".to_string(),
            PerlValue::string(route.action.replace('#', "_")),
        );
        op.insert(
            "summary".to_string(),
            PerlValue::string(route.action.clone()),
        );
        op.insert(
            "responses".to_string(),
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new({
                let mut m: IndexMap<String, PerlValue> = IndexMap::new();
                let mut ok = IndexMap::new();
                ok.insert(
                    "description".to_string(),
                    PerlValue::string("OK".to_string()),
                );
                m.insert(
                    "200".to_string(),
                    PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(ok))),
                );
                m
            }))),
        );
        entry.insert(
            route.verb.to_lowercase(),
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(op))),
        );
    }
    let mut paths_out: IndexMap<String, PerlValue> = IndexMap::new();
    for (k, v) in paths {
        paths_out.insert(
            k,
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(v))),
        );
    }
    let mut info = IndexMap::new();
    info.insert(
        "title".to_string(),
        PerlValue::string("stryke_web app".to_string()),
    );
    info.insert("version".to_string(), PerlValue::string("1.0".into()));
    let mut root = IndexMap::new();
    root.insert("openapi".to_string(), PerlValue::string("3.0.3".into()));
    root.insert(
        "info".to_string(),
        PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(info))),
    );
    root.insert(
        "paths".to_string(),
        PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(paths_out))),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(
        root,
    ))))
}

fn openapi_path(p: &str) -> String {
    // `/posts/:id` → `/posts/{id}` per OpenAPI convention.
    let mut out = String::with_capacity(p.len());
    let bytes = p.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                j += 1;
            }
            out.push('{');
            out.push_str(std::str::from_utf8(&bytes[start..j]).unwrap_or(""));
            out.push('}');
            i = j;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

// ── Token mint / consume (password resets, email verify links) ────────

pub(crate) fn web_token_for(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let user_id = args
        .first()
        .map(|v| v.to_int())
        .ok_or_else(|| PerlError::runtime("web_token_for: user_id required", line))?;
    let purpose = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "default".to_string());
    let ttl = args.get(2).map(|v| v.to_int()).unwrap_or(3600);
    let exp = unix_now() + ttl;
    let payload = format!("{}|{}|{}", purpose, user_id, exp);
    let mac = sha256_hex((secret_key() + &payload).as_bytes());
    Ok(PerlValue::string(format!(
        "{}.{}",
        base64url_encode(payload.as_bytes()),
        &mac[..32]
    )))
}

pub(crate) fn web_token_consume(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let token = args.first().map(|v| v.to_string()).unwrap_or_default();
    let purpose = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "default".to_string());
    let Some((b64, mac)) = token.rsplit_once('.') else {
        return Ok(PerlValue::UNDEF);
    };
    let bytes = match base64url_decode(b64) {
        Some(b) => b,
        None => return Ok(PerlValue::UNDEF),
    };
    let payload = match std::str::from_utf8(&bytes) {
        Ok(s) => s,
        Err(_) => return Ok(PerlValue::UNDEF),
    };
    let want = sha256_hex((secret_key() + payload).as_bytes());
    if !constant_time_eq(mac.as_bytes(), &want.as_bytes()[..32]) {
        return Ok(PerlValue::UNDEF);
    }
    let parts: Vec<&str> = payload.splitn(3, '|').collect();
    if parts.len() != 3 {
        return Ok(PerlValue::UNDEF);
    }
    if parts[0] != purpose {
        return Ok(PerlValue::UNDEF);
    }
    let exp = parts[2].parse::<i64>().unwrap_or(0);
    if unix_now() > exp {
        return Ok(PerlValue::UNDEF);
    }
    let user_id = parts[1].parse::<i64>().unwrap_or(0);
    Ok(PerlValue::integer(user_id))
}

// ── Permissions / can ─────────────────────────────────────────────────

pub(crate) fn web_can(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let action = args.first().map(|v| v.to_string()).unwrap_or_default();
    let user = args.get(1).cloned().unwrap_or(PerlValue::UNDEF);
    let user_map = user
        .as_hash_map()
        .or_else(|| user.as_hash_ref().map(|h| h.read().clone()));
    if user_map.is_none() {
        return Ok(PerlValue::integer(0));
    }
    let user_map = user_map.unwrap();
    // Convention: admins can do anything; otherwise check `permissions`
    // text field for `action` substring.
    let role = user_map
        .get("role")
        .map(|v| v.to_string())
        .unwrap_or_default();
    if role == "admin" || role == "owner" {
        return Ok(PerlValue::integer(1));
    }
    let perms = user_map
        .get("permissions")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let allowed = perms
        .split(',')
        .any(|p| p.trim() == action || p.trim() == "*");
    Ok(PerlValue::integer(if allowed { 1 } else { 0 }))
}

// ── JSON:API helpers ───────────────────────────────────────────────────

/// `web_jsonapi_resource("posts", $row)` → wraps a single hashref into
/// a JSON:API `{data: {type, id, attributes}}` envelope.
pub(crate) fn web_jsonapi_resource(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let kind = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_jsonapi_resource: type required", line))?;
    let row = args
        .get(1)
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .unwrap_or_default();
    let id = row.get("id").map(|v| v.to_string()).unwrap_or_default();
    let mut attrs = row.clone();
    attrs.shift_remove("id");

    let mut data = IndexMap::new();
    data.insert("type".into(), PerlValue::string(kind));
    data.insert("id".into(), PerlValue::string(id));
    data.insert(
        "attributes".into(),
        PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(attrs))),
    );
    let mut envelope = IndexMap::new();
    envelope.insert(
        "data".into(),
        PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(data))),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(
        envelope,
    ))))
}

/// `web_jsonapi_collection("posts", $rows)` → wraps an array_ref of
/// hashrefs into `{data: [{type, id, attributes}, ...], meta: {count}}`.
pub(crate) fn web_jsonapi_collection(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let kind = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("web_jsonapi_collection: type required", line))?;
    let rows: Vec<PerlValue> = match args.get(1) {
        Some(v) => v
            .as_array_ref()
            .map(|a| a.read().clone())
            .unwrap_or_else(|| v.clone().to_list()),
        None => Vec::new(),
    };

    let mut wrapped = Vec::with_capacity(rows.len());
    for row in &rows {
        let map = row
            .as_hash_map()
            .or_else(|| row.as_hash_ref().map(|h| h.read().clone()))
            .unwrap_or_default();
        let id = map.get("id").map(|v| v.to_string()).unwrap_or_default();
        let mut attrs = map.clone();
        attrs.shift_remove("id");
        let mut entry = IndexMap::new();
        entry.insert("type".into(), PerlValue::string(kind.clone()));
        entry.insert("id".into(), PerlValue::string(id));
        entry.insert(
            "attributes".into(),
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(attrs))),
        );
        wrapped.push(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(
            entry,
        ))));
    }

    let mut meta = IndexMap::new();
    meta.insert("count".into(), PerlValue::integer(wrapped.len() as i64));

    let mut envelope = IndexMap::new();
    envelope.insert(
        "data".into(),
        PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(wrapped))),
    );
    envelope.insert(
        "meta".into(),
        PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(meta))),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(
        envelope,
    ))))
}

/// `web_jsonapi_error(404, "not_found", "Post 42 missing")` →
/// `{errors: [{status, code, title}]}` per JSON:API.
pub(crate) fn web_jsonapi_error(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let status = args.first().map(|v| v.to_int()).unwrap_or(500);
    let code = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "internal_error".into());
    let title = args
        .get(2)
        .map(|v| v.to_string())
        .unwrap_or_else(|| code.clone());
    let mut err = IndexMap::new();
    err.insert("status".into(), PerlValue::string(status.to_string()));
    err.insert("code".into(), PerlValue::string(code));
    err.insert("title".into(), PerlValue::string(title));
    let mut envelope = IndexMap::new();
    envelope.insert(
        "errors".into(),
        PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(vec![
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(err))),
        ]))),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(
        envelope,
    ))))
}

/// `web_bearer_token()` → returns the `Authorization: Bearer X` token
/// from the request, or undef if missing/malformed. Pair with
/// `web_jwt_decode` for token-auth APIs.
pub(crate) fn web_bearer_token(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let auth = with_current(|cur| {
        cur.request
            .as_ref()
            .and_then(|r| r.as_hash_ref())
            .and_then(|h| h.read().get("headers").cloned())
            .and_then(|hv| hv.as_hash_ref().map(|h| h.read().clone()))
            .and_then(|m| m.get("authorization").map(|v| v.to_string()))
    });
    let Some(s) = auth else {
        return Ok(PerlValue::UNDEF);
    };
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("Bearer ") {
        return Ok(PerlValue::string(rest.trim().to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix("bearer ") {
        return Ok(PerlValue::string(rest.trim().to_string()));
    }
    Ok(PerlValue::UNDEF)
}

// ── View helpers ───────────────────────────────────────────────────────
//
// Pure string-builders used inside `<%= ... %>` blocks. They mirror the
// Rails ActionView helper API closely enough that ported templates can
// drop straight in once the user wires their auth / asset choices.

/// HTML-escape: `&` `<` `>` `"` `'`. Use everywhere user content lands
/// inside HTML — `<%= web_h($post->{title}) %>`.
pub(crate) fn web_h(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(html_escape(&s)))
}

/// `web_link_to(label, href, +{class => "btn"})` → `<a href="...">label</a>`
/// with optional html-attribute hashref. The label is HTML-escaped; the
/// href is attribute-escaped.
pub(crate) fn web_link_to(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let label = args.first().map(|v| v.to_string()).unwrap_or_default();
    let href = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let attrs = collect_html_attrs_from_pairs(&args[args.len().min(2)..]);
    let mut html = String::from("<a");
    push_attr(&mut html, "href", &href);
    push_attrs(&mut html, &attrs);
    html.push('>');
    html.push_str(&html_escape(&label));
    html.push_str("</a>");
    Ok(PerlValue::string(html))
}

/// `web_button_to(label, href, method => "delete")` → renders a form
/// containing a single submit button. Used for non-GET destructive
/// actions where a plain `<a>` would pollute the URL or be cached.
pub(crate) fn web_button_to(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let label = args.first().map(|v| v.to_string()).unwrap_or_default();
    let action = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let opts = collect_kv_pairs(&args[args.len().min(2)..]);
    let method_raw = opts
        .get("method")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "post".to_string());
    let (form_method, hidden_override) =
        if method_raw.eq_ignore_ascii_case("get") || method_raw.eq_ignore_ascii_case("post") {
            (method_raw.to_lowercase(), None)
        } else {
            ("post".to_string(), Some(method_raw.to_lowercase()))
        };
    let mut html = String::from("<form");
    push_attr(&mut html, "action", &action);
    push_attr(&mut html, "method", &form_method);
    push_attr(&mut html, "style", "display:inline");
    html.push('>');
    if let Some(m) = hidden_override {
        html.push_str(&format!(
            "<input type=\"hidden\" name=\"_method\" value=\"{}\">",
            attr_escape(&m)
        ));
    }
    if let Some(confirm) = opts.get("confirm") {
        html.push_str(&format!(
            "<button type=\"submit\" onclick=\"return confirm('{}')\">{}</button>",
            attr_escape(&confirm.to_string()),
            html_escape(&label)
        ));
    } else {
        html.push_str(&format!(
            "<button type=\"submit\">{}</button>",
            html_escape(&label)
        ));
    }
    html.push_str("</form>");
    Ok(PerlValue::string(html))
}

/// `web_form_with(url => "/posts", method => "post")` → opening `<form>`
/// tag. Pair with `web_form_close` (or write `</form>` directly) at the
/// end. Optional `_method` override for PATCH/PUT/DELETE rendered as a
/// hidden input — same convention Rails uses.
pub(crate) fn web_form_with(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let opts = collect_kv_pairs(args);
    let url = opts.get("url").map(|v| v.to_string()).unwrap_or_default();
    let method_raw = opts
        .get("method")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "post".to_string());
    let (form_method, hidden_override) =
        if method_raw.eq_ignore_ascii_case("get") || method_raw.eq_ignore_ascii_case("post") {
            (method_raw.to_lowercase(), None)
        } else {
            ("post".to_string(), Some(method_raw.to_lowercase()))
        };
    let mut html = String::from("<form");
    push_attr(&mut html, "action", &url);
    push_attr(&mut html, "method", &form_method);
    if let Some(class) = opts.get("class") {
        push_attr(&mut html, "class", &class.to_string());
    }
    html.push('>');
    if let Some(m) = hidden_override {
        html.push_str(&format!(
            "<input type=\"hidden\" name=\"_method\" value=\"{}\">",
            attr_escape(&m)
        ));
    }
    Ok(PerlValue::string(html))
}

/// Closing companion for `web_form_with` — emits `</form>`. Lets the
/// caller stay symmetric instead of hardcoding the close tag.
pub(crate) fn web_form_close(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    Ok(PerlValue::string("</form>".to_string()))
}

/// `web_text_field("title", $post->{title})` → `<input type="text" ...>`.
pub(crate) fn web_text_field(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let name = args.first().map(|v| v.to_string()).unwrap_or_default();
    let value = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(format!(
        "<input type=\"text\" name=\"{}\" value=\"{}\">",
        attr_escape(&name),
        attr_escape(&value)
    )))
}

/// `web_text_area("body", $post->{body})` → `<textarea ...>...</textarea>`.
pub(crate) fn web_text_area(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let name = args.first().map(|v| v.to_string()).unwrap_or_default();
    let value = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(format!(
        "<textarea name=\"{}\">{}</textarea>",
        attr_escape(&name),
        html_escape(&value)
    )))
}

/// `web_check_box("published", $post->{published})` → checkbox plus a
/// hidden `0` companion so an unchecked box still posts a value.
pub(crate) fn web_check_box(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let name = args.first().map(|v| v.to_string()).unwrap_or_default();
    let truthy = args
        .get(1)
        .map(|v| v.to_int() != 0 && !v.to_string().is_empty() && v.to_string() != "0")
        .unwrap_or(false);
    let checked = if truthy { " checked" } else { "" };
    Ok(PerlValue::string(format!(
        "<input type=\"hidden\" name=\"{name}\" value=\"0\"><input type=\"checkbox\" name=\"{name}\" value=\"1\"{checked}>",
        name = attr_escape(&name),
        checked = checked
    )))
}

/// CSRF placeholder — emits a `<meta>` tag the form helpers can pair
/// with `_csrf_token` hidden inputs. Real CSRF wiring lands when the
/// session middleware ships.
pub(crate) fn web_csrf_meta_tag(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    Ok(PerlValue::string(
        "<meta name=\"csrf-token\" content=\"\">".to_string(),
    ))
}

/// `web_stylesheet_link_tag("application")` → `<link rel="stylesheet" ...>`.
/// Multiple names render multiple link tags. `media => "all"` is the
/// default. Uses the Rails convention of looking under `/assets/`.
pub(crate) fn web_stylesheet_link_tag(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let mut out = String::new();
    let mut media = "all".to_string();
    let names: Vec<String> = args
        .iter()
        .take_while(|v| {
            // Trailing `media => "all"` etc. would show up as a string
            // arg followed by another. We treat all leading scalar args
            // as names; once we see a kv-shape (next arg is even-indexed
            // value) we stop. For simplicity, walk until first `media`
            // / `defer` keyword.
            let s = v.to_string();
            !matches!(s.as_str(), "media" | "defer" | "integrity" | "nonce")
        })
        .map(|v| v.to_string())
        .collect();
    let opts = collect_kv_pairs(&args[names.len()..]);
    if let Some(m) = opts.get("media") {
        media = m.to_string();
    }
    for name in &names {
        let href = if name.starts_with('/') || name.starts_with("http") {
            name.clone()
        } else {
            format!("/assets/{}.css", name)
        };
        out.push_str(&format!(
            "<link rel=\"stylesheet\" href=\"{}\" media=\"{}\">\n",
            attr_escape(&href),
            attr_escape(&media)
        ));
    }
    Ok(PerlValue::string(out))
}

/// `web_javascript_link_tag("application", defer => 1)` → `<script>` tags.
pub(crate) fn web_javascript_link_tag(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let mut out = String::new();
    let names: Vec<String> = args
        .iter()
        .take_while(|v| {
            let s = v.to_string();
            !matches!(s.as_str(), "media" | "defer" | "async" | "type" | "nonce")
        })
        .map(|v| v.to_string())
        .collect();
    let opts = collect_kv_pairs(&args[names.len()..]);
    let defer = opts.get("defer").map(|v| v.to_int() != 0).unwrap_or(false);
    let asyn = opts.get("async").map(|v| v.to_int() != 0).unwrap_or(false);
    for name in &names {
        let src = if name.starts_with('/') || name.starts_with("http") {
            name.clone()
        } else {
            format!("/assets/{}.js", name)
        };
        out.push_str(&format!(
            "<script src=\"{}\"{}{}></script>\n",
            attr_escape(&src),
            if defer { " defer" } else { "" },
            if asyn { " async" } else { "" }
        ));
    }
    Ok(PerlValue::string(out))
}

/// `web_image_tag("logo.png", alt => "logo")` → `<img src="..." alt="...">`.
pub(crate) fn web_image_tag(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let src = args.first().map(|v| v.to_string()).unwrap_or_default();
    let opts = collect_kv_pairs(&args[args.len().min(1)..]);
    let resolved = if src.starts_with('/') || src.starts_with("http") {
        src.clone()
    } else {
        format!("/assets/{}", src)
    };
    let mut html = String::from("<img");
    push_attr(&mut html, "src", &resolved);
    if let Some(alt) = opts.get("alt") {
        push_attr(&mut html, "alt", &alt.to_string());
    }
    if let Some(class) = opts.get("class") {
        push_attr(&mut html, "class", &class.to_string());
    }
    html.push('>');
    Ok(PerlValue::string(html))
}

/// `web_truncate("long string", length => 30, omission => "…")` → cap a
/// string for display. Defaults: 30 chars, ellipsis suffix.
pub(crate) fn web_truncate(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    let opts = collect_kv_pairs(&args[args.len().min(1)..]);
    let length = opts
        .get("length")
        .map(|v| v.to_int() as usize)
        .unwrap_or(30);
    let omission = opts
        .get("omission")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "…".to_string());
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= length {
        return Ok(PerlValue::string(s));
    }
    let take = length.saturating_sub(omission.chars().count());
    let mut out: String = chars.iter().take(take).collect();
    out.push_str(&omission);
    Ok(PerlValue::string(out))
}

/// `web_pluralize(3, "post")` → `"3 posts"`. Trivial English pluralizer
/// — handles a few common irregulars; defers to `name + "s"` otherwise.
pub(crate) fn web_pluralize(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let n = args.first().map(|v| v.to_int()).unwrap_or(0);
    let word = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let plural = if n == 1 {
        word.clone()
    } else if let Some(p) = irregular_plural(&word) {
        p.to_string()
    } else if word.ends_with('y')
        && !word.ends_with("ay")
        && !word.ends_with("ey")
        && !word.ends_with("oy")
        && !word.ends_with("uy")
    {
        format!("{}ies", &word[..word.len() - 1])
    } else if word.ends_with('s')
        || word.ends_with('x')
        || word.ends_with('z')
        || word.ends_with("sh")
        || word.ends_with("ch")
    {
        format!("{}es", word)
    } else {
        format!("{}s", word)
    };
    Ok(PerlValue::string(format!("{} {}", n, plural)))
}

fn irregular_plural(word: &str) -> Option<&'static str> {
    match word.to_ascii_lowercase().as_str() {
        "person" => Some("people"),
        "child" => Some("children"),
        "mouse" => Some("mice"),
        "goose" => Some("geese"),
        "man" => Some("men"),
        "woman" => Some("women"),
        "foot" => Some("feet"),
        "tooth" => Some("teeth"),
        _ => None,
    }
}

/// `web_time_ago_in_words($timestamp)` — best-effort relative time
/// string ("3 minutes ago", "yesterday"). Accepts ISO-8601-ish input
/// from the timestamps `web_model_create` writes.
pub(crate) fn web_time_ago_in_words(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    Ok(PerlValue::string(format_time_ago(&s)))
}

fn format_time_ago(ts: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let then_secs = parse_simple_iso(ts).unwrap_or(now_secs);
    let diff = now_secs - then_secs;
    if diff < 60 {
        return "less than a minute ago".to_string();
    }
    if diff < 3600 {
        let m = diff / 60;
        return format!("{} minute{} ago", m, if m == 1 { "" } else { "s" });
    }
    if diff < 86400 {
        let h = diff / 3600;
        return format!("{} hour{} ago", h, if h == 1 { "" } else { "s" });
    }
    if diff < 86400 * 30 {
        let d = diff / 86400;
        return format!("{} day{} ago", d, if d == 1 { "" } else { "s" });
    }
    if diff < 86400 * 365 {
        let mo = diff / (86400 * 30);
        return format!("{} month{} ago", mo, if mo == 1 { "" } else { "s" });
    }
    let y = diff / (86400 * 365);
    format!("{} year{} ago", y, if y == 1 { "" } else { "s" })
}

fn parse_simple_iso(s: &str) -> Option<i64> {
    // Accept `YYYY-MM-DD HH:MM:SS` (the shape `web_orm::current_timestamp`
    // writes). Returns Unix seconds.
    let parts: Vec<&str> = s.splitn(2, ' ').collect();
    if parts.len() != 2 {
        return None;
    }
    let ymd: Vec<&str> = parts[0].split('-').collect();
    let hms: Vec<&str> = parts[1].split(':').collect();
    if ymd.len() != 3 || hms.len() != 3 {
        return None;
    }
    let y: i64 = ymd[0].parse().ok()?;
    let m: i64 = ymd[1].parse().ok()?;
    let d: i64 = ymd[2].parse().ok()?;
    let hh: i64 = hms[0].parse().ok()?;
    let mm: i64 = hms[1].parse().ok()?;
    let ss: i64 = hms[2].parse().ok()?;
    let y_adj = if m <= 2 { y - 1 } else { y };
    let m_adj = if m <= 2 { m + 12 } else { m };
    let era = if y_adj >= 0 { y_adj } else { y_adj - 399 } / 400;
    let yoe = y_adj - era * 400;
    let doy = (153 * (m_adj - 3) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hh * 3600 + mm * 60 + ss)
}

/// `web_flash()` returns the flash hashref. PASS 5 uses thread-local
/// storage; multi-process flash needs cookie middleware.
pub(crate) fn web_flash(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let map = with_current(|cur| {
        cur.params.clone() // placeholder — flash slot is added in PASS 6 if user wants
    });
    let _ = map; // suppress unused if features change
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(
        IndexMap::new(),
    ))))
}

// ── HTML/attribute escaping + helpers ──────────────────────────────────

fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn attr_escape(s: &str) -> String {
    // Same escape set as html_escape — fine for all browser-quoted attrs.
    html_escape(s)
}

fn push_attr(html: &mut String, k: &str, v: &str) {
    html.push(' ');
    html.push_str(k);
    html.push_str("=\"");
    html.push_str(&attr_escape(v));
    html.push('"');
}

fn push_attrs(html: &mut String, attrs: &IndexMap<String, String>) {
    for (k, v) in attrs {
        push_attr(html, k, v);
    }
}

/// Walk a tail of `[k1, v1, k2, v2, ...]` PerlValue args and turn it
/// into an attribute map. Stops at first odd entry (no value pair).
fn collect_html_attrs_from_pairs(args: &[PerlValue]) -> IndexMap<String, String> {
    let mut out = IndexMap::new();
    let mut i = 0;
    while i + 1 < args.len() {
        out.insert(args[i].to_string(), args[i + 1].to_string());
        i += 2;
    }
    out
}

/// Same as `collect_html_attrs_from_pairs` but keeps PerlValues so
/// callers can switch on type (booleans for `defer`, etc.).
fn collect_kv_pairs(args: &[PerlValue]) -> IndexMap<String, PerlValue> {
    let mut out = IndexMap::new();
    let mut i = 0;
    while i + 1 < args.len() {
        out.insert(args[i].to_string(), args[i + 1].clone());
        i += 2;
    }
    out
}

// ── Boot + dispatch ────────────────────────────────────────────────────

impl Interpreter {
    /// Builtin entry: `web_render(...)`. Dispatches to one of: `text`,
    /// `html`, `body`, `json`, `template`. The template branch reads the
    /// matching `.erb` file and runs it through the ERB engine below,
    /// optionally wrapping in `app/views/layouts/application.html.erb`.
    pub(crate) fn web_render(&mut self, args: &[PerlValue], line: usize) -> Result<PerlValue> {
        let opts = parse_render_opts(args);
        let status = opts.get("status").map(|v| v.to_int() as u16).unwrap_or(200);
        let (body, ct) = if let Some(v) = opts.get("json") {
            let s = crate::native_data::json_encode(v).unwrap_or_else(|_| "null".to_string());
            (s, "application/json; charset=utf-8".to_string())
        } else if let Some(v) = opts.get("text") {
            (v.to_string(), "text/plain; charset=utf-8".to_string())
        } else if let Some(v) = opts.get("html") {
            (v.to_string(), "text/html; charset=utf-8".to_string())
        } else if let Some(v) = opts.get("body") {
            (v.to_string(), "text/html; charset=utf-8".to_string())
        } else if let Some(tv) = opts.get("template") {
            let template = tv.to_string();
            let locals = opts
                .get("locals")
                .and_then(|v| {
                    v.as_hash_map()
                        .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
                })
                .unwrap_or_default();
            let layout = opts.get("layout").map(|v| v.to_string());
            let body = self.render_template(&template, &locals, layout.as_deref(), line)?;
            (body, "text/html; charset=utf-8".to_string())
        } else {
            ("".to_string(), "text/html; charset=utf-8".to_string())
        };
        let mut headers = vec![("content-type".to_string(), ct)];
        if let Some(hv) = opts.get("headers") {
            let map = hv
                .as_hash_map()
                .or_else(|| hv.as_hash_ref().map(|h| h.read().clone()));
            if let Some(m) = map {
                for (k, v) in m {
                    let lk = k.to_lowercase();
                    headers.retain(|(hk, _)| hk.to_lowercase() != lk);
                    headers.push((lk, v.to_string()));
                }
            }
        }
        with_current(|cur| {
            cur.status = status;
            cur.headers = headers;
            cur.body = body;
            cur.rendered = true;
        });
        Ok(PerlValue::UNDEF)
    }

    /// Render `.erb` from `app/views/<name>.html.erb`. If `layout` is
    /// `Some("application")` (default when None) and a layout file with
    /// that stem exists at `app/views/layouts/<layout>.html.erb`, the
    /// template's body is interpolated where `<%= yield %>` appears.
    pub(crate) fn render_template(
        &mut self,
        name: &str,
        locals: &IndexMap<String, PerlValue>,
        layout: Option<&str>,
        line: usize,
    ) -> Result<String> {
        let view_path = format!("app/views/{}.html.erb", name);
        let body_src = std::fs::read_to_string(&view_path).map_err(|e| {
            PerlError::runtime(format!("web_render: can't read {}: {}", view_path, e), line)
        })?;
        let body = self.render_erb(&body_src, locals, line)?;

        let layout_name = layout.unwrap_or("application");
        let layout_path = format!("app/views/layouts/{}.html.erb", layout_name);
        if let Ok(layout_src) = std::fs::read_to_string(&layout_path) {
            // Inject `$content_for_layout` / replace `<%= yield %>` with
            // the inner body. We pre-substitute literal `<%= yield %>`
            // (and `<%= yield_content %>`) before running the engine so
            // the body's HTML escapes-or-not policy doesn't matter.
            let layout_with_yield = layout_src
                .replace("<%= yield %>", &body)
                .replace("<%=yield%>", &body)
                .replace("<%= yield_content %>", &body);
            return self.render_erb(&layout_with_yield, locals, line);
        }
        Ok(body)
    }

    /// Compile + evaluate ERB. Recognised tags:
    ///
    /// ```text
    /// <%# comment %>     dropped
    /// <% stmt %>         executed for side effects (no output)
    /// <%= expr %>        evaluated and stringified into output
    /// <%- ... -%>        same as above with whitespace trimming
    /// ```
    ///
    /// Locals are installed as `my $name = $value` in a child scope so
    /// the views can reference them as plain scalars.
    pub(crate) fn render_erb(
        &mut self,
        src: &str,
        locals: &IndexMap<String, PerlValue>,
        line: usize,
    ) -> Result<String> {
        // Push a child scope, declare locals, then walk the template.
        self.scope_push_hook();
        for (k, v) in locals {
            self.scope.declare_scalar(k, v.clone());
        }
        let result = self.render_erb_inner(src, line);
        self.scope_pop_hook();
        result
    }

    fn render_erb_inner(&mut self, src: &str, line: usize) -> Result<String> {
        // Two-pass compile: walk segments → build one stryke program that
        // appends to `$__erb_buf`, then run that program once. This is
        // how ERB / EJS / Jinja avoid the multi-tag control-flow problem
        // (a `for { ... }` opens in one tag and closes in another).
        let mut program = String::with_capacity(src.len() * 2 + 64);
        program.push_str("my $__erb_buf = \"\";\n");
        let mut i = 0;
        let bytes = src.as_bytes();
        while i < bytes.len() {
            if let Some(open) = find_substr(bytes, i, b"<%") {
                if open > i {
                    append_text_segment(&mut program, &src[i..open]);
                }
                let mut tag_start = open + 2;
                let mut kind = ErbKind::Stmt;
                if tag_start < bytes.len() && bytes[tag_start] == b'#' {
                    kind = ErbKind::Comment;
                    tag_start += 1;
                } else if tag_start < bytes.len() && bytes[tag_start] == b'=' {
                    kind = ErbKind::Expr;
                    tag_start += 1;
                } else if tag_start < bytes.len() && bytes[tag_start] == b'-' {
                    tag_start += 1;
                    if tag_start < bytes.len() && bytes[tag_start] == b'=' {
                        kind = ErbKind::Expr;
                        tag_start += 1;
                    }
                    // Trim trailing whitespace+newline from the program's
                    // last text-append literal — `<%-` swallows it. We
                    // approximate by trimming the program string itself
                    // when the prior segment ended with a literal-append.
                    trim_trailing_ws_in_last_append(&mut program);
                }
                let close = find_substr(bytes, tag_start, b"%>")
                    .ok_or_else(|| PerlError::runtime("erb: unclosed `<%` tag", line))?;
                let mut content_end = close;
                let mut after = close + 2;
                if content_end > tag_start && bytes[content_end - 1] == b'-' {
                    content_end -= 1;
                    if after < bytes.len() && bytes[after] == b'\n' {
                        after += 1;
                    }
                }
                let content = &src[tag_start..content_end];
                match kind {
                    ErbKind::Comment => {}
                    ErbKind::Stmt => {
                        program.push_str(content);
                        program.push('\n');
                    }
                    ErbKind::Expr => {
                        program.push_str("$__erb_buf .= (");
                        program.push_str(content);
                        program.push_str(") // \"\";\n");
                    }
                }
                i = after;
            } else {
                if i < bytes.len() {
                    append_text_segment(&mut program, &src[i..]);
                }
                break;
            }
        }
        program.push_str("$__erb_buf\n");

        let val = crate::parse_and_run_string(&program, self)
            .map_err(|e| PerlError::runtime(format!("erb: {}", e), line))?;
        Ok(val.to_string())
    }

    pub(crate) fn web_boot_application(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> Result<PerlValue> {
        let port = args.first().map(|v| v.to_int()).unwrap_or(3000);
        if !(1..=65535).contains(&port) {
            return Err(PerlError::runtime(
                format!("web_boot_application: bad port {}", port),
                line,
            ));
        }
        let listener = std::net::TcpListener::bind(format!("0.0.0.0:{}", port))
            .map_err(|e| PerlError::runtime(format!("web_boot_application: bind: {}", e), line))?;
        eprintln!("stryke web: serving on http://0.0.0.0:{}", port);

        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    if let Err(e) = self.dispatch_one(s, line) {
                        eprintln!("stryke web: request error: {}", e);
                    }
                }
                Err(e) => eprintln!("stryke web: accept: {}", e),
            }
        }
        Ok(PerlValue::UNDEF)
    }

    /// Read one HTTP request off `stream`, route it, dispatch the matching
    /// controller action, and write the response back. Blocking — the
    /// caller's accept loop is what fans out across connections; this
    /// keeps the dispatch logic single-threaded so we can reuse the
    /// interpreter directly without the worker-pool dance `serve` does.
    fn dispatch_one(&mut self, mut stream: std::net::TcpStream, line: usize) -> Result<()> {
        use std::io::{BufRead, BufReader, Write};

        let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
        let mut reader = BufReader::new(&stream);

        // Request line.
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).unwrap_or(0) == 0 {
            return Ok(());
        }
        let parts: Vec<&str> = request_line.trim().splitn(3, ' ').collect();
        if parts.len() < 2 {
            let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
            return Ok(());
        }
        let method = parts[0].to_ascii_uppercase();
        let raw_path = parts[1];
        let (path, query) = raw_path.split_once('?').unwrap_or((raw_path, ""));

        // Headers.
        let mut headers_map: IndexMap<String, PerlValue> = IndexMap::new();
        let mut content_length: usize = 0;
        loop {
            let mut hline = String::new();
            if reader.read_line(&mut hline).unwrap_or(0) == 0 {
                break;
            }
            let trimmed = hline.trim_end();
            if trimmed.is_empty() {
                break;
            }
            if let Some((k, v)) = trimmed.split_once(':') {
                let key = k.trim().to_lowercase();
                let val = v.trim().to_string();
                if key == "content-length" {
                    content_length = val.parse().unwrap_or(0);
                }
                headers_map.insert(key, PerlValue::string(val));
            }
        }

        // Body.
        let body = if content_length > 0 {
            let mut buf = vec![0u8; content_length.min(10 * 1024 * 1024)];
            let n = std::io::Read::read(&mut reader, &mut buf).unwrap_or(0);
            buf.truncate(n);
            String::from_utf8_lossy(&buf).into_owned()
        } else {
            String::new()
        };

        // Build params: query + body (form-urlencoded) + path captures.
        let mut params: IndexMap<String, PerlValue> = IndexMap::new();
        parse_query(query, &mut params);
        if matches!(method.as_str(), "POST" | "PATCH" | "PUT") {
            // form-urlencoded only for now; JSON/multipart in a later pass.
            if let Some(ct) = headers_map.get("content-type") {
                let ct_str = ct.to_string().to_lowercase();
                if ct_str.starts_with("application/x-www-form-urlencoded") {
                    parse_query(&body, &mut params);
                } else if ct_str.starts_with("application/json") {
                    if let Ok(parsed) = crate::native_data::json_decode(&body) {
                        if let Some(map) = parsed
                            .as_hash_map()
                            .or_else(|| parsed.as_hash_ref().map(|h| h.read().clone()))
                        {
                            for (k, v) in map {
                                params.insert(k, v);
                            }
                        }
                    }
                }
            }
        }

        // Method override — `_method=patch|put|delete` from a form lets
        // browsers post non-POST verbs through a hidden input.
        let effective_method = if method == "POST" {
            params
                .get("_method")
                .map(|v| v.to_string().to_ascii_uppercase())
                .filter(|m| matches!(m.as_str(), "PATCH" | "PUT" | "DELETE"))
                .unwrap_or(method.clone())
        } else {
            method.clone()
        };

        // Parse cookies from request and seed thread-local session/flash.
        let cookies_in = parse_cookie_header(&headers_map);
        let session_data = decode_session_cookie(&cookies_in);
        let flash_in = decode_flash_cookie(&cookies_in);

        // Request log line — best-effort, dropped if FS unavailable.
        let req_started = std::time::Instant::now();
        let log_line_prefix = format!("{} {}", effective_method, path);

        // CORS preflight — global, only when `cors_origin` is in app config.
        let cors = app_config()
            .lock()
            .get("cors_origin")
            .map(|v| v.to_string());
        if effective_method == "OPTIONS" && cors.is_some() {
            let headers: Vec<(String, String)> = vec![
                (
                    "access-control-allow-origin".to_string(),
                    cors.clone().unwrap(),
                ),
                (
                    "access-control-allow-methods".to_string(),
                    "GET,POST,PATCH,PUT,DELETE,OPTIONS".to_string(),
                ),
                (
                    "access-control-allow-headers".to_string(),
                    "content-type,authorization".to_string(),
                ),
                ("access-control-max-age".to_string(), "86400".to_string()),
            ];
            return write_response(&mut stream, 204, &headers, "", &[]);
        }

        // Built-in /health endpoint — no user code, framework guarantee.
        if effective_method == "GET" && path == "/health" {
            let body = "{\"status\":\"ok\",\"framework\":\"stryke_web\",\"version\":1}".to_string();
            return write_response(
                &mut stream,
                200,
                &[("content-type".into(), "application/json".into())],
                &body,
                &[],
            );
        }

        // Built-in /openapi.json — serializes the live route table as
        // an OpenAPI 3.0 doc with no app code.
        if effective_method == "GET" && path == "/openapi.json" {
            let doc = web_openapi(&[], 0).unwrap_or(PerlValue::UNDEF);
            let body = crate::native_data::json_encode(&doc).unwrap_or_else(|_| "{}".into());
            return write_response(
                &mut stream,
                200,
                &[("content-type".into(), "application/json".into())],
                &body,
                &[],
            );
        }

        // Built-in /docs — Swagger UI HTML pulling /openapi.json. Pure
        // CDN static page, no app code, works even on `--api` mode.
        if effective_method == "GET" && path == "/docs" {
            return write_response(
                &mut stream,
                200,
                &[("content-type".into(), "text/html; charset=utf-8".into())],
                SWAGGER_UI_HTML,
                &[],
            );
        }
        if effective_method == "GET" && path == "/docs/redoc" {
            return write_response(
                &mut stream,
                200,
                &[("content-type".into(), "text/html; charset=utf-8".into())],
                REDOC_HTML,
                &[],
            );
        }

        // Static file fallback — serve from `public/` for safe paths so
        // CSS/JS/images work without a separate web server.
        if effective_method == "GET" {
            if let Some((status, ct, bytes)) = try_serve_public(path) {
                return write_response_bytes(
                    &mut stream,
                    status,
                    &[("content-type".into(), ct)],
                    &bytes,
                    &[],
                );
            }
        }

        // Match route.
        let route_match = router().lock().routes.iter().find_map(|r| {
            if r.verb != effective_method {
                return None;
            }
            let caps = r.re.captures(path)?;
            let mut path_params: IndexMap<String, PerlValue> = IndexMap::new();
            for name in &r.captures {
                if let Some(m) = caps.name(&format!("__{}__", name)) {
                    path_params.insert(name.clone(), PerlValue::string(m.as_str().to_string()));
                }
            }
            Some((r.action.clone(), path_params))
        });

        let (status, resp_headers, resp_body, set_cookies) = match route_match {
            None => (
                404,
                vec![("content-type".into(), "text/plain; charset=utf-8".into())],
                format!("404 Not Found: {} {}", effective_method, path),
                Vec::new(),
            ),
            Some((action, path_params)) => {
                for (k, v) in path_params {
                    params.insert(k, v);
                }
                let (s, h, b, c) = self.invoke_action(
                    &action,
                    &headers_map,
                    &params,
                    &effective_method,
                    path,
                    query,
                    &body,
                    cookies_in.clone(),
                    session_data.clone(),
                    flash_in.clone(),
                    line,
                );
                (s, h, b, c)
            }
        };

        let mut resp_headers = resp_headers;
        if let Some(origin) = &cors {
            resp_headers.push(("access-control-allow-origin".into(), origin.clone()));
        }

        let result = write_response(&mut stream, status, &resp_headers, &resp_body, &set_cookies);

        // Best-effort request log.
        let env = std::env::var("STRYKE_ENV").unwrap_or_else(|_| "development".to_string());
        let elapsed_ms = req_started.elapsed().as_millis();
        let log_line = format!(
            "[{}] {} {} {}ms\n",
            current_iso_time(),
            log_line_prefix,
            status,
            elapsed_ms
        );
        let _ = std::fs::create_dir_all("log");
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("log/{}.log", env))
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(log_line.as_bytes())
            });
        result
    }

    /// Run before/after filters registered for this controller against
    /// the given action. Stops at the first filter whose `only` /
    /// `except` matches; runs the static method named in the entry.
    fn run_filters(
        &mut self,
        class_name: &str,
        action: &str,
        before: bool,
        line: usize,
    ) -> std::result::Result<(), PerlError> {
        let entries: Vec<FilterEntry> = {
            let g = filters_slot().lock();
            match g.get(class_name) {
                Some(f) => {
                    if before {
                        f.before.clone()
                    } else {
                        f.after.clone()
                    }
                }
                None => Vec::new(),
            }
        };
        if entries.is_empty() {
            return Ok(());
        }
        let class_def = match self.class_defs.get(class_name).cloned() {
            Some(c) => c,
            None => return Ok(()),
        };
        for e in entries {
            if !e.only.is_empty() && !e.only.iter().any(|a| a == action) {
                continue;
            }
            if e.except.iter().any(|a| a == action) {
                continue;
            }
            let m = match class_def
                .methods
                .iter()
                .find(|m| m.name == e.method)
                .cloned()
            {
                Some(m) => m,
                None => continue,
            };
            let body = match m.body {
                Some(b) => b,
                None => continue,
            };
            match self.call_static_class_method(&body, &m.params, vec![], line) {
                Ok(_) | Err(FlowOrError::Flow(_)) => {}
                Err(FlowOrError::Error(err)) => return Err(err),
            }
            // Filter rendered → short-circuit.
            if before && with_current(|cur| cur.rendered) {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Resolve `posts#show` → call `PostsController::show()` (static class
    /// method).
    #[allow(clippy::too_many_arguments)]
    fn invoke_action(
        &mut self,
        action: &str,
        headers_map: &IndexMap<String, PerlValue>,
        params: &IndexMap<String, PerlValue>,
        method: &str,
        path: &str,
        query: &str,
        body: &str,
        cookies_in: IndexMap<String, String>,
        session: IndexMap<String, PerlValue>,
        flash_in: IndexMap<String, PerlValue>,
        line: usize,
    ) -> DispatchResult {
        let (resource, act) = match action.split_once('#') {
            Some((r, a)) => (r, a),
            None => {
                return (
                    500,
                    vec![("content-type".into(), "text/plain; charset=utf-8".into())],
                    format!("invalid route action: {}", action),
                    Vec::new(),
                );
            }
        };

        let req_hash = build_request_hash(headers_map, method, path, query, body);
        with_current(|cur| {
            *cur = RequestState {
                request: Some(req_hash),
                params: params.clone(),
                status: 200,
                headers: Vec::new(),
                body: String::new(),
                rendered: false,
                resource: resource.to_string(),
                action: act.to_string(),
                cookies_in,
                cookies_out: Vec::new(),
                session,
                session_dirty: false,
                flash_in,
                flash_out: IndexMap::new(),
            };
        });
        let class_name = format!("{}Controller", to_pascal_case(resource));

        let class_def = match self.class_defs.get(&class_name).cloned() {
            Some(c) => c,
            None => {
                return (
                    500,
                    vec![("content-type".into(), "text/plain; charset=utf-8".into())],
                    format!("controller not found: {} (route: {})", class_name, action),
                    Vec::new(),
                );
            }
        };
        let method_def = class_def.methods.iter().find(|m| m.name == act).cloned();
        let (body_block, params_sig) =
            match method_def.and_then(|m| m.body.clone().map(|b| (b, m.params.clone()))) {
                Some(p) => p,
                None => {
                    return (
                        500,
                        vec![("content-type".into(), "text/plain; charset=utf-8".into())],
                        format!("action not found: {}#{}", class_name, act),
                        Vec::new(),
                    );
                }
            };

        // Run before_action filters. Skip the main action if any
        // filter rendered/redirected (sets `cur.rendered = true`).
        if let Err(e) = self.run_filters(&class_name, act, true, line) {
            return (
                500,
                vec![("content-type".into(), "text/plain; charset=utf-8".into())],
                format!("before_action error: {}", e),
                Vec::new(),
            );
        }
        let already_rendered = with_current(|cur| cur.rendered);

        if !already_rendered {
            let call_result = self.call_static_class_method(&body_block, &params_sig, vec![], line);
            match call_result {
                Ok(_) | Err(FlowOrError::Flow(_)) => {}
                Err(FlowOrError::Error(e)) => {
                    return (
                        500,
                        vec![("content-type".into(), "text/plain; charset=utf-8".into())],
                        format!("action error: {}", e),
                        Vec::new(),
                    );
                }
            }
        }

        // Run after_action filters (advisory — runs even if action
        // didn't render so the user can fix it up).
        let _ = self.run_filters(&class_name, act, false, line);

        let need_default_render = with_current(|cur| !cur.rendered);
        if need_default_render {
            let template_name = format!("{}/{}", resource, act);
            if let Ok(body) = self.render_template(&template_name, &IndexMap::new(), None, line) {
                with_current(|cur| {
                    cur.status = 200;
                    cur.headers = vec![("content-type".into(), "text/html; charset=utf-8".into())];
                    cur.body = body;
                    cur.rendered = true;
                });
            }
        }

        // Pull response back out + collect cookies/session/flash.
        let (status, headers, body, rendered, mut cookies_out, session, dirty, flash_out) =
            with_current(|cur| {
                (
                    cur.status,
                    cur.headers.clone(),
                    std::mem::take(&mut cur.body),
                    cur.rendered,
                    std::mem::take(&mut cur.cookies_out),
                    cur.session.clone(),
                    cur.session_dirty,
                    std::mem::take(&mut cur.flash_out),
                )
            });
        if dirty {
            cookies_out.push((
                "_stryke_session".into(),
                encode_session_payload(&session),
                CookieOpts {
                    path: Some("/".into()),
                    http_only: true,
                    same_site: Some("Lax".into()),
                    ..CookieOpts::default()
                },
            ));
        }
        if !flash_out.is_empty() {
            cookies_out.push((
                "_stryke_flash".into(),
                encode_session_payload(&flash_out),
                CookieOpts {
                    path: Some("/".into()),
                    http_only: true,
                    same_site: Some("Lax".into()),
                    ..CookieOpts::default()
                },
            ));
        } else {
            // Clear flash cookie after one redirect.
            cookies_out.push((
                "_stryke_flash".into(),
                String::new(),
                CookieOpts {
                    path: Some("/".into()),
                    max_age: Some(0),
                    ..CookieOpts::default()
                },
            ));
        }
        let _ = headers; // mutated only conditionally above
        if !rendered {
            return (
                204,
                vec![("content-type".into(), "text/plain; charset=utf-8".into())],
                String::new(),
                cookies_out,
            );
        }
        let (status, headers, body, _) = (status, headers, body, ());
        (status, headers, body, cookies_out)
    }
}

// ── Helpers ────────────────────────────────────────────────────────────

fn build_request_hash(
    headers: &IndexMap<String, PerlValue>,
    method: &str,
    path: &str,
    query: &str,
    body: &str,
) -> PerlValue {
    let mut req: IndexMap<String, PerlValue> = IndexMap::new();
    req.insert("method".into(), PerlValue::string(method.to_string()));
    req.insert("path".into(), PerlValue::string(path.to_string()));
    req.insert("query".into(), PerlValue::string(query.to_string()));
    req.insert("body".into(), PerlValue::string(body.to_string()));
    req.insert(
        "headers".into(),
        PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(headers.clone()))),
    );
    PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(req)))
}

fn parse_query(s: &str, out: &mut IndexMap<String, PerlValue>) {
    if s.is_empty() {
        return;
    }
    for pair in s.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let k = url_decode(k);
        let v = url_decode(v);
        out.insert(k, PerlValue::string(v));
    }
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_digit(bytes[i + 1]);
                let lo = hex_digit(bytes[i + 2]);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push(((h << 4) | l) as char);
                    i += 3;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + (b - b'a')),
        b'A'..=b'F' => Some(10 + (b - b'A')),
        _ => None,
    }
}

fn to_pascal_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut up = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            up = true;
            continue;
        }
        if up {
            for cc in c.to_uppercase() {
                out.push(cc);
            }
            up = false;
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Copy, Clone, Debug)]
enum ErbKind {
    Stmt,
    Expr,
    Comment,
}

// ── Built-in /docs HTML (Swagger UI + Redoc) ───────────────────────────

const SWAGGER_UI_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <title>API Docs · Swagger UI</title>
    <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui.css">
    <style>body { margin: 0; }</style>
</head>
<body>
    <div id="swagger-ui"></div>
    <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
    <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-standalone-preset.js"></script>
    <script>
        window.onload = () => {
            window.ui = SwaggerUIBundle({
                url: '/openapi.json',
                dom_id: '#swagger-ui',
                deepLinking: true,
                presets: [
                    SwaggerUIBundle.presets.apis,
                    SwaggerUIStandalonePreset
                ],
                layout: 'BaseLayout',
                docExpansion: 'list',
                tryItOutEnabled: true,
            });
        };
    </script>
</body>
</html>
"#;

const REDOC_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <title>API Docs · Redoc</title>
    <style>body { margin: 0; }</style>
</head>
<body>
    <redoc spec-url="/openapi.json"></redoc>
    <script src="https://cdn.jsdelivr.net/npm/redoc@next/bundles/redoc.standalone.js"></script>
</body>
</html>
"#;

// ── Static-file fallback / response writer / cookies / sessions ────────

fn try_serve_public(path: &str) -> Option<(u16, String, Vec<u8>)> {
    if !path.starts_with('/') || path.contains("..") || path.contains('\0') {
        return None;
    }
    let cleaned = &path[1..];
    if cleaned.is_empty() {
        return None;
    }
    let file_path = std::path::PathBuf::from("public").join(cleaned);
    if !file_path.exists() || !file_path.is_file() {
        return None;
    }
    let bytes = std::fs::read(&file_path).ok()?;
    let ct = mime_type_for(&file_path);
    Some((200, ct, bytes))
}

fn mime_type_for(p: &std::path::Path) -> String {
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
    .to_string()
}

fn write_response(
    stream: &mut std::net::TcpStream,
    status: u16,
    headers: &[(String, String)],
    body: &str,
    cookies: &[(String, String, CookieOpts)],
) -> Result<()> {
    write_response_bytes(stream, status, headers, body.as_bytes(), cookies)
}

fn write_response_bytes(
    stream: &mut std::net::TcpStream,
    status: u16,
    headers: &[(String, String)],
    body: &[u8],
    cookies: &[(String, String, CookieOpts)],
) -> Result<()> {
    use std::io::Write;
    let status_text = http_status_text(status);
    let mut resp = format!("HTTP/1.1 {} {}\r\n", status, status_text);
    let mut has_ct = false;
    for (k, v) in headers {
        resp.push_str(&format!("{}: {}\r\n", k, v));
        if k.eq_ignore_ascii_case("content-type") {
            has_ct = true;
        }
    }
    if !has_ct {
        resp.push_str("content-type: text/plain; charset=utf-8\r\n");
    }
    for (k, v, opts) in cookies {
        resp.push_str(&format!(
            "set-cookie: {}\r\n",
            encode_set_cookie(k, v, opts)
        ));
    }
    resp.push_str(&format!("content-length: {}\r\n", body.len()));
    resp.push_str("connection: close\r\n\r\n");
    let _ = stream.write_all(resp.as_bytes());
    let _ = stream.write_all(body);
    let _ = stream.flush();
    Ok(())
}

fn encode_set_cookie(name: &str, value: &str, opts: &CookieOpts) -> String {
    let mut s = format!("{}={}", name, value);
    if let Some(p) = &opts.path {
        s.push_str(&format!("; Path={}", p));
    }
    if let Some(d) = &opts.domain {
        s.push_str(&format!("; Domain={}", d));
    }
    if let Some(m) = opts.max_age {
        s.push_str(&format!("; Max-Age={}", m));
    }
    if opts.http_only {
        s.push_str("; HttpOnly");
    }
    if opts.secure {
        s.push_str("; Secure");
    }
    if let Some(ss) = &opts.same_site {
        s.push_str(&format!("; SameSite={}", ss));
    }
    s
}

fn parse_cookie_header(headers: &IndexMap<String, PerlValue>) -> IndexMap<String, String> {
    let mut out = IndexMap::new();
    if let Some(v) = headers.get("cookie") {
        for pair in v.to_string().split(';') {
            let pair = pair.trim();
            if let Some((k, v)) = pair.split_once('=') {
                out.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
    }
    out
}

/// Session cookie value is base64url-encoded JSON. No HMAC for v0 — flag
/// the cookie HttpOnly + SameSite=Lax. PASS 8 wires HMAC-SHA256 signing
/// using the `secret_key_base` from `web_application_config`.
fn decode_session_cookie(cookies: &IndexMap<String, String>) -> IndexMap<String, PerlValue> {
    cookies
        .get("_stryke_session")
        .and_then(|raw| decode_session_payload(raw))
        .unwrap_or_default()
}

fn decode_flash_cookie(cookies: &IndexMap<String, String>) -> IndexMap<String, PerlValue> {
    cookies
        .get("_stryke_flash")
        .and_then(|raw| decode_session_payload(raw))
        .unwrap_or_default()
}

fn decode_session_payload(raw: &str) -> Option<IndexMap<String, PerlValue>> {
    let bytes = base64url_decode(raw)?;
    let json = std::str::from_utf8(&bytes).ok()?;
    let v = crate::native_data::json_decode(json).ok()?;
    v.as_hash_map()
        .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
}

fn encode_session_payload(map: &IndexMap<String, PerlValue>) -> String {
    let val = PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(map.clone())));
    let json = crate::native_data::json_encode(&val).unwrap_or_else(|_| "{}".into());
    base64url_encode(json.as_bytes())
}

fn base64url_encode(input: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((input.len() * 4).div_ceil(3));
    let mut i = 0;
    while i + 3 <= input.len() {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
        out.push(CHARS[(n >> 18 & 63) as usize] as char);
        out.push(CHARS[(n >> 12 & 63) as usize] as char);
        out.push(CHARS[(n >> 6 & 63) as usize] as char);
        out.push(CHARS[(n & 63) as usize] as char);
        i += 3;
    }
    let rem = input.len() - i;
    if rem == 1 {
        let n = (input[i] as u32) << 16;
        out.push(CHARS[(n >> 18 & 63) as usize] as char);
        out.push(CHARS[(n >> 12 & 63) as usize] as char);
    } else if rem == 2 {
        let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
        out.push(CHARS[(n >> 18 & 63) as usize] as char);
        out.push(CHARS[(n >> 12 & 63) as usize] as char);
        out.push(CHARS[(n >> 6 & 63) as usize] as char);
    }
    out
}

fn base64url_decode(input: &str) -> Option<Vec<u8>> {
    fn dec(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|b| !matches!(b, b'=' | b'\n' | b'\r' | b' '))
        .collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut i = 0;
    while i + 4 <= bytes.len() {
        let a = dec(bytes[i])?;
        let b = dec(bytes[i + 1])?;
        let c = dec(bytes[i + 2])?;
        let d = dec(bytes[i + 3])?;
        out.push((a << 2) | (b >> 4));
        out.push((b << 4) | (c >> 2));
        out.push((c << 6) | d);
        i += 4;
    }
    let rem = bytes.len() - i;
    if rem == 2 {
        let a = dec(bytes[i])?;
        let b = dec(bytes[i + 1])?;
        out.push((a << 2) | (b >> 4));
    } else if rem == 3 {
        let a = dec(bytes[i])?;
        let b = dec(bytes[i + 1])?;
        let c = dec(bytes[i + 2])?;
        out.push((a << 2) | (b >> 4));
        out.push((b << 4) | (c >> 2));
    }
    Some(out)
}

/// Emit a stryke statement that appends the literal `text` to `$__erb_buf`.
/// Use a single-quoted string so `$` / `@` / `#{...}` in template HTML
/// never trigger stryke interpolation. Single-quoted strings only escape
/// `\` and `'` — perfect for HTML which is otherwise opaque.
fn append_text_segment(program: &mut String, text: &str) {
    program.push_str("$__erb_buf .= '");
    for c in text.chars() {
        match c {
            '\\' => program.push_str("\\\\"),
            '\'' => program.push_str("\\'"),
            other => program.push(other),
        }
    }
    program.push_str("';\n");
}

/// `<%-` whitespace trim: strip trailing space/newline from the text the
/// previous append-segment emitted. Operates on the raw single-quoted
/// payload between the opening `'` and the closing `';`.
fn trim_trailing_ws_in_last_append(program: &mut String) {
    if let Some(close) = program.rfind("';\n") {
        let head = &program[..close];
        if let Some(open) = head.rfind("$__erb_buf .= '") {
            let body_start = open + "$__erb_buf .= '".len();
            let mut s = String::from(&program[body_start..close]);
            while s.ends_with(' ') || s.ends_with('\t') || s.ends_with('\n') {
                s.pop();
            }
            program.replace_range(body_start..close, &s);
        }
    }
}

fn find_substr(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from > haystack.len() {
        return None;
    }
    let last = haystack.len().saturating_sub(needle.len());
    let mut i = from;
    while i <= last {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn http_status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "OK",
    }
}
