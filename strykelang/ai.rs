//! AI primitives — Phase 0 ("walking skeleton") of the design in
//! `docs/AI_PRIMITIVES.md`.
//!
//! What this module ships TODAY:
//!
//!   * `ai($prompt, opts...)`         — single-shot, no-tools call to the
//!                                       configured provider
//!   * `prompt($prompt, opts...)`     — alias for `ai`
//!   * `stream_prompt($prompt, opts)` — streaming variant; returns one
//!                                       concatenated string for v0
//!                                       (real `Stream<Str>` is Phase 5)
//!   * `chat($messages, opts...)`     — explicit message-list version
//!   * `embed($text)` / `embed(@texts)` — text embedding via Voyage AI
//!   * `tokens_of($text)`             — char/4 heuristic
//!   * `ai_cost()`                    — running USD spent in this process
//!   * `ai_cache_clear()` / `ai_cache_size()` / `ai_cache_stats()`
//!   * `ai_mock_install("pattern", "response")` / `ai_mock_clear()`
//!   * `ai_config_get("key")`         — read the loaded `[ai]` table
//!
//! What's intentionally NOT in this module yet (per AI_PRIMITIVES.md
//! phases):
//!
//!   * `tool fn` declaration syntax — needs parser work (Phase 1)
//!   * Full agent loop with auto-registered tools (Phase 1)
//!   * `mcp_server { ... }` declarative block (Phase 2)
//!   * `ai_filter` / `ai_map` / `ai_classify` / etc. (Phase 3)
//!   * llama.cpp local backend (Phase 4)
//!   * Cluster fanout integration (Phase 5)
//!
//! Configuration: looks for `[ai]` and `[ai.<provider>]` tables in
//! `./stryke.toml` at the program's cwd at the first `ai`/`prompt`/etc.
//! call. Falls back to env vars (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`)
//! and built-in defaults (`provider="anthropic"`,
//! `model="claude-opus-4-5"`).

use crate::error::PerlError;
use crate::interpreter::{FlowOrError, Interpreter, WantarrayCtx};
use crate::value::PerlValue;
use indexmap::IndexMap;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

type Result<T> = std::result::Result<T, PerlError>;

// ── Config ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct AiConfig {
    provider: String,
    model: String,
    api_key_env: String,
    cache: bool,
    max_cost_run_usd: f64,
    embed_provider: String,
    embed_model: String,
    embed_api_key_env: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: "claude-opus-4-5".to_string(),
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            cache: true,
            max_cost_run_usd: 5.0,
            embed_provider: "voyage".to_string(),
            embed_model: "voyage-3".to_string(),
            embed_api_key_env: "VOYAGE_API_KEY".to_string(),
        }
    }
}

static CONFIG: OnceLock<Mutex<AiConfig>> = OnceLock::new();

fn config() -> &'static Mutex<AiConfig> {
    CONFIG.get_or_init(|| {
        let cfg = load_config_from_toml().unwrap_or_default();
        Mutex::new(cfg)
    })
}

fn load_config_from_toml() -> Option<AiConfig> {
    let raw = std::fs::read_to_string("stryke.toml").ok()?;
    let parsed: toml::Value = toml::from_str(&raw).ok()?;
    let ai = parsed.get("ai")?.as_table()?.clone();

    let mut cfg = AiConfig::default();
    if let Some(v) = ai.get("provider").and_then(|v| v.as_str()) {
        cfg.provider = v.to_string();
    }
    if let Some(v) = ai.get("model").and_then(|v| v.as_str()) {
        cfg.model = v.to_string();
    }
    if let Some(v) = ai.get("api_key_env").and_then(|v| v.as_str()) {
        cfg.api_key_env = v.to_string();
    }
    if let Some(v) = ai.get("cache").and_then(|v| v.as_bool()) {
        cfg.cache = v;
    }
    if let Some(v) = ai.get("max_cost_run").and_then(|v| v.as_float()) {
        cfg.max_cost_run_usd = v;
    }
    if let Some(embed) = ai.get("embed").and_then(|v| v.as_table()) {
        if let Some(v) = embed.get("provider").and_then(|v| v.as_str()) {
            cfg.embed_provider = v.to_string();
        }
        if let Some(v) = embed.get("model").and_then(|v| v.as_str()) {
            cfg.embed_model = v.to_string();
        }
        if let Some(v) = embed.get("api_key_env").and_then(|v| v.as_str()) {
            cfg.embed_api_key_env = v.to_string();
        }
    }
    Some(cfg)
}

// ── Cost tracking ──────────────────────────────────────────────────────

static COST_USD_MICROS: AtomicU64 = AtomicU64::new(0);
static INPUT_TOKENS: AtomicU64 = AtomicU64::new(0);
static OUTPUT_TOKENS: AtomicU64 = AtomicU64::new(0);
static EMBED_TOKENS: AtomicU64 = AtomicU64::new(0);
static CACHE_CREATION_TOKENS: AtomicU64 = AtomicU64::new(0);
static CACHE_READ_TOKENS: AtomicU64 = AtomicU64::new(0);

fn add_cost(usd: f64) {
    let micros = (usd * 1_000_000.0) as u64;
    COST_USD_MICROS.fetch_add(micros, Ordering::Relaxed);
}

fn current_cost_usd() -> f64 {
    COST_USD_MICROS.load(Ordering::Relaxed) as f64 / 1_000_000.0
}

/// Best-effort published-rate price table for tokens. Hardcoded so it
/// doesn't depend on a network call. Updated when major models drop.
fn price_per_1k_tokens(model: &str) -> (f64, f64) {
    // (input_per_1k, output_per_1k) in USD.
    match model {
        m if m.starts_with("claude-opus") => (15.0 / 1000.0, 75.0 / 1000.0),
        m if m.starts_with("claude-sonnet") => (3.0 / 1000.0, 15.0 / 1000.0),
        m if m.starts_with("claude-haiku") => (0.80 / 1000.0, 4.0 / 1000.0),
        m if m.starts_with("gpt-4o-mini") => (0.15 / 1000.0, 0.60 / 1000.0),
        m if m.starts_with("gpt-4o") => (2.50 / 1000.0, 10.0 / 1000.0),
        m if m.starts_with("gpt-5") => (5.0 / 1000.0, 20.0 / 1000.0),
        m if m.starts_with("o1") => (15.0 / 1000.0, 60.0 / 1000.0),
        m if m.starts_with("gemini-2.5-pro") => (1.25 / 1000.0, 10.0 / 1000.0),
        m if m.starts_with("gemini") => (0.30 / 1000.0, 2.50 / 1000.0),
        _ => (3.0 / 1000.0, 15.0 / 1000.0), // sensible default
    }
}

// ── Cache ──────────────────────────────────────────────────────────────

static CACHE: OnceLock<Mutex<IndexMap<String, String>>> = OnceLock::new();
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

fn cache() -> &'static Mutex<IndexMap<String, String>> {
    CACHE.get_or_init(|| Mutex::new(IndexMap::new()))
}

fn cache_key(provider: &str, model: &str, system: &str, prompt: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(provider.as_bytes());
    h.update(b"\x00");
    h.update(model.as_bytes());
    h.update(b"\x00");
    h.update(system.as_bytes());
    h.update(b"\x00");
    h.update(prompt.as_bytes());
    let bytes = h.finalize();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ── Mock mode ──────────────────────────────────────────────────────────

static MOCKS: OnceLock<Mutex<Vec<(regex::Regex, String)>>> = OnceLock::new();

static LAST_THINKING_BUF: OnceLock<Mutex<String>> = OnceLock::new();
static LAST_CITATIONS_BUF: OnceLock<Mutex<Vec<serde_json::Value>>> = OnceLock::new();

fn last_thinking() -> &'static Mutex<String> {
    LAST_THINKING_BUF.get_or_init(|| Mutex::new(String::new()))
}

fn last_citations() -> &'static Mutex<Vec<serde_json::Value>> {
    LAST_CITATIONS_BUF.get_or_init(|| Mutex::new(Vec::new()))
}

fn mocks() -> &'static Mutex<Vec<(regex::Regex, String)>> {
    MOCKS.get_or_init(|| Mutex::new(Vec::new()))
}

fn match_mock(prompt: &str) -> Option<String> {
    let g = mocks().lock();
    for (re, response) in g.iter() {
        if re.is_match(prompt) {
            return Some(response.clone());
        }
    }
    None
}

fn mock_only_mode() -> bool {
    matches!(
        std::env::var("STRYKE_AI_MODE").as_deref(),
        Ok("mock-only") | Ok("mock_only")
    )
}

// ── Public builtins ────────────────────────────────────────────────────

/// `ai $prompt, [system => "...", model => "...", max_tokens => N, ...]`
/// — single-shot LLM call. Phase 0 has no tool/agent loop yet — that
/// arrives with `tool fn` parser support in Phase 1.
pub(crate) fn ai_prompt(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let prompt = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai/prompt: prompt required", line))?;
    let opts = parse_opts(&args[1..]);
    let provider = opt_str(&opts, "provider", &config().lock().provider);
    let model = opt_str(&opts, "model", &config().lock().model);
    let system = opt_str(&opts, "system", "");
    let max_tokens = opt_int(&opts, "max_tokens", 1024);
    let temperature = opt_float(&opts, "temperature", -1.0);
    let cache_enabled = opt_bool(&opts, "cache", config().lock().cache);
    let timeout = opt_int(&opts, "timeout", 60);
    let cache_control = opt_bool(&opts, "cache_control", false);
    let thinking = opt_bool(&opts, "thinking", false);
    let thinking_budget = opt_int(&opts, "thinking_budget", 5000);

    // 1. Mock mode wins everything.
    if let Some(resp) = match_mock(&prompt) {
        record_history(
            &provider,
            &model,
            &prompt,
            resp.chars().count(),
            0,
            0,
            0.0,
            false,
        );
        return Ok(PerlValue::string(resp));
    }
    if mock_only_mode() {
        return Err(PerlError::runtime(
            format!(
                "ai: STRYKE_AI_MODE=mock-only and no mock matched prompt {:?}",
                truncate(&prompt, 60)
            ),
            line,
        ));
    }

    // 2. Cache check.
    let key = cache_key(&provider, &model, &system, &prompt);
    if cache_enabled {
        if let Some(hit) = cache().lock().get(&key).cloned() {
            CACHE_HITS.fetch_add(1, Ordering::Relaxed);
            record_history(
                &provider,
                &model,
                &prompt,
                hit.chars().count(),
                0,
                0,
                0.0,
                true,
            );
            return Ok(PerlValue::string(hit));
        }
        CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
    }

    // 3. Cost ceiling.
    let ceiling = config().lock().max_cost_run_usd;
    if ceiling > 0.0 && current_cost_usd() >= ceiling {
        return Err(PerlError::runtime(
            format!(
                "ai: max_cost_run_usd={:.2} exceeded (current ${:.4})",
                ceiling,
                current_cost_usd()
            ),
            line,
        ));
    }

    // 4. Dispatch by provider.
    let base_url_override = opt_str(&opts, "base_url", "");
    let result = match provider.as_str() {
        "anthropic" => call_anthropic(
            &prompt,
            &system,
            &model,
            max_tokens,
            temperature,
            timeout,
            cache_control,
            thinking,
            thinking_budget,
            line,
        )?,
        "openai" => call_openai_with_base(
            &prompt,
            &system,
            &model,
            max_tokens,
            temperature,
            timeout,
            "https://api.openai.com/v1/chat/completions",
            "OPENAI_API_KEY",
            line,
        )?,
        // OpenAI-compatible local servers: LM Studio (default :1234),
        // vLLM, llama-server, llamafile, anything-llm. Same wire shape;
        // user just sets `base_url => "http://localhost:1234/v1/chat/completions"`
        // (or via `[ai] base_url = "..."`).
        "openai_compat" | "compat" | "local" => {
            let base = if !base_url_override.is_empty() {
                base_url_override.clone()
            } else {
                std::env::var("STRYKE_AI_BASE_URL")
                    .unwrap_or_else(|_| "http://localhost:1234/v1/chat/completions".into())
            };
            call_openai_with_base(
                &prompt,
                &system,
                &model,
                max_tokens,
                temperature,
                timeout,
                &base,
                "STRYKE_AI_LOCAL_KEY",
                line,
            )?
        }
        // Ollama's native (non-OpenAI) generate API.
        "ollama" => {
            let base = if !base_url_override.is_empty() {
                base_url_override.clone()
            } else {
                std::env::var("OLLAMA_HOST")
                    .map(|h| {
                        if h.starts_with("http") {
                            h
                        } else {
                            format!("http://{}", h)
                        }
                    })
                    .unwrap_or_else(|_| "http://localhost:11434".into())
            };
            call_ollama(
                &prompt,
                &system,
                &model,
                max_tokens,
                temperature,
                timeout,
                &base,
                line,
            )?
        }
        "gemini" | "google" => call_gemini(
            &prompt,
            &system,
            &model,
            max_tokens,
            temperature,
            timeout,
            line,
        )?,
        other => {
            return Err(PerlError::runtime(
                format!(
                    "ai: provider `{}` not implemented (try anthropic/openai/ollama/local/gemini)",
                    other
                ),
                line,
            ))
        }
    };

    if cache_enabled {
        cache().lock().insert(key, result.clone());
    }
    record_history(
        &provider,
        &model,
        &prompt,
        result.chars().count(),
        INPUT_TOKENS.load(Ordering::Relaxed),
        OUTPUT_TOKENS.load(Ordering::Relaxed),
        current_cost_usd(),
        false,
    );
    Ok(PerlValue::string(result))
}

pub(crate) fn ai_stream_prompt(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    // Iter-context streaming: returns a PerlValue::iterator that yields
    // one text-delta chunk per `next()`. Scalar context still works
    // because stryke iterators stringify by collecting + joining.
    //
    // Mock-mode and missing API key fall back to the buffered string
    // path so tests stay deterministic offline.
    let prompt = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("stream_prompt: prompt required", line))?;
    let opts = parse_opts(&args[1..]);

    if let Some(resp) = match_mock(&prompt) {
        return Ok(make_string_chunked_iter(resp));
    }
    if mock_only_mode() {
        return Err(PerlError::runtime(
            "stream_prompt: STRYKE_AI_MODE=mock-only and no mock matched",
            line,
        ));
    }

    let provider = opt_str(&opts, "provider", &config().lock().provider);
    if provider != "anthropic" {
        return ai_prompt(args, line);
    }
    let model = opt_str(&opts, "model", &config().lock().model);
    let system = opt_str(&opts, "system", "");
    let max_tokens = opt_int(&opts, "max_tokens", 1024);
    let temperature = opt_float(&opts, "temperature", -1.0);
    let timeout = opt_int(&opts, "timeout", 120);

    let key_env = config().lock().api_key_env.clone();
    let api_key = std::env::var(&key_env).map_err(|_| {
        PerlError::runtime(format!("stream_prompt: ${} env var not set", key_env), line)
    })?;
    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": [{ "role": "user", "content": prompt }],
        "stream": true,
    });
    if !system.is_empty() {
        body["system"] = serde_json::Value::String(system);
    }
    if temperature >= 0.0 {
        body["temperature"] = serde_json::Value::from(temperature);
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post("https://api.anthropic.com/v1/messages")
        .set("x-api-key", &api_key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .set("accept", "text/event-stream")
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("stream_prompt: anthropic: {}", e), line))?;
    let reader = std::io::BufReader::new(resp.into_reader());
    let iter = AnthropicStreamIter {
        reader: parking_lot::Mutex::new(Some(reader)),
        done: parking_lot::Mutex::new(false),
        model,
    };
    Ok(PerlValue::iterator(Arc::new(iter)))
}

struct AnthropicStreamIter {
    reader: parking_lot::Mutex<Option<std::io::BufReader<Box<dyn std::io::Read + Send + Sync>>>>,
    done: parking_lot::Mutex<bool>,
    model: String,
}

impl crate::value::PerlIterator for AnthropicStreamIter {
    fn next_item(&self) -> Option<PerlValue> {
        use std::io::BufRead;
        if *self.done.lock() {
            return None;
        }
        let mut guard = self.reader.lock();
        let reader = guard.as_mut()?;
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;
        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line).unwrap_or(0);
            if n == 0 {
                *self.done.lock() = true;
                let (in_per_1k, out_per_1k) = price_per_1k_tokens(&self.model);
                INPUT_TOKENS.fetch_add(input_tokens, std::sync::atomic::Ordering::Relaxed);
                OUTPUT_TOKENS.fetch_add(output_tokens, std::sync::atomic::Ordering::Relaxed);
                add_cost(
                    input_tokens as f64 / 1000.0 * in_per_1k
                        + output_tokens as f64 / 1000.0 * out_per_1k,
                );
                return None;
            }
            let Some(payload) = line
                .trim_end()
                .strip_prefix("data: ")
                .map(|s| s.to_string())
            else {
                continue;
            };
            if payload == "[DONE]" {
                *self.done.lock() = true;
                return None;
            }
            let v: serde_json::Value = match serde_json::from_str(&payload) {
                Ok(v) => v,
                Err(_) => continue,
            };
            match v["type"].as_str() {
                Some("content_block_delta") => {
                    if let Some(t) = v["delta"]["text"].as_str() {
                        return Some(PerlValue::string(t.to_string()));
                    }
                }
                Some("message_start") => {
                    if let Some(n) = v["message"]["usage"]["input_tokens"].as_u64() {
                        input_tokens = n;
                    }
                }
                Some("message_delta") => {
                    if let Some(n) = v["usage"]["output_tokens"].as_u64() {
                        output_tokens = n;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Mock-mode iterator: chunks a known string into character-sized
/// pieces so tests can drive `for my $chunk in stream_prompt("...")`
/// loops deterministically without the network.
fn make_string_chunked_iter(s: String) -> PerlValue {
    struct Iter {
        chars: parking_lot::Mutex<std::collections::VecDeque<char>>,
    }
    impl crate::value::PerlIterator for Iter {
        fn next_item(&self) -> Option<PerlValue> {
            self.chars
                .lock()
                .pop_front()
                .map(|c| PerlValue::string(c.to_string()))
        }
    }
    let chars: std::collections::VecDeque<char> = s.chars().collect();
    PerlValue::iterator(Arc::new(Iter {
        chars: parking_lot::Mutex::new(chars),
    }))
}

pub(crate) fn ai_chat(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let messages = args
        .first()
        .ok_or_else(|| PerlError::runtime("chat: messages array required", line))?;
    let arr = messages
        .as_array_ref()
        .map(|a| a.read().clone())
        .unwrap_or_else(|| messages.clone().to_list());
    if arr.is_empty() {
        return Err(PerlError::runtime("chat: messages array is empty", line));
    }
    // Find the last user-role message to use as the prompt; treat
    // earlier messages as system context. Phase 1 will hand the full
    // message list to the provider as-is.
    let mut last_user = String::new();
    let mut sys = String::new();
    for m in &arr {
        let h = m
            .as_hash_map()
            .or_else(|| m.as_hash_ref().map(|h| h.read().clone()))
            .unwrap_or_default();
        let role = h.get("role").map(|v| v.to_string()).unwrap_or_default();
        let content = h.get("content").map(|v| v.to_string()).unwrap_or_default();
        if role == "user" {
            last_user = content;
        } else if role == "system" {
            if !sys.is_empty() {
                sys.push('\n');
            }
            sys.push_str(&content);
        } else {
            if !sys.is_empty() {
                sys.push('\n');
            }
            sys.push_str(&format!("({}): {}", role, content));
        }
    }
    let mut new_args: Vec<PerlValue> = vec![PerlValue::string(last_user)];
    if !sys.is_empty() {
        new_args.push(PerlValue::string("system".to_string()));
        new_args.push(PerlValue::string(sys));
    }
    for v in &args[1..] {
        new_args.push(v.clone());
    }
    ai_prompt(&new_args, line)
}

pub(crate) fn ai_embed(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let inputs: Vec<String> = if let Some(first) = args.first() {
        if let Some(arr) = first.as_array_ref() {
            arr.read().iter().map(|v| v.to_string()).collect()
        } else {
            vec![first.to_string()]
        }
    } else {
        return Err(PerlError::runtime("embed: text required", line));
    };

    if let Some(resp) = match_mock(&format!("embed:{}", inputs.join("|"))) {
        // Mock returns a comma-separated float string per text.
        let vec: Vec<PerlValue> = resp
            .split(',')
            .filter_map(|s| s.trim().parse::<f64>().ok())
            .map(PerlValue::float)
            .collect();
        return Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
            vec,
        ))));
    }
    if mock_only_mode() {
        return Err(PerlError::runtime(
            "embed: STRYKE_AI_MODE=mock-only and no embed mock installed",
            line,
        ));
    }

    let cfg = config().lock().clone();
    // Routing table override wins over the cfg default.
    let provider = routing()
        .lock()
        .get("embed")
        .cloned()
        .unwrap_or(cfg.embed_provider.clone());
    let api_key_env = match provider.as_str() {
        "openai" => "OPENAI_API_KEY".to_string(),
        _ => cfg.embed_api_key_env.clone(),
    };
    match provider.as_str() {
        "voyage" => call_voyage_embed(&inputs, &cfg.embed_model, &api_key_env, line),
        "openai" => call_openai_embed(&inputs, &cfg.embed_model, &api_key_env, line),
        "ollama" => call_ollama_embed(&inputs, &cfg.embed_model, line),
        other => Err(PerlError::runtime(
            format!("embed: provider `{}` not implemented", other),
            line,
        )),
    }
}

pub(crate) fn ai_tokens_of(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let s = args.first().map(|v| v.to_string()).unwrap_or_default();
    // Heuristic: ~4 chars per token, never < 1 for non-empty strings.
    let approx = if s.is_empty() {
        0
    } else {
        s.chars().count().div_ceil(4).max(1) as i64
    };
    Ok(PerlValue::integer(approx))
}

pub(crate) fn ai_cost(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let mut h = IndexMap::new();
    h.insert("usd".to_string(), PerlValue::float(current_cost_usd()));
    h.insert(
        "input_tokens".to_string(),
        PerlValue::integer(INPUT_TOKENS.load(Ordering::Relaxed) as i64),
    );
    h.insert(
        "output_tokens".to_string(),
        PerlValue::integer(OUTPUT_TOKENS.load(Ordering::Relaxed) as i64),
    );
    h.insert(
        "embed_tokens".to_string(),
        PerlValue::integer(EMBED_TOKENS.load(Ordering::Relaxed) as i64),
    );
    h.insert(
        "cache_creation_tokens".to_string(),
        PerlValue::integer(CACHE_CREATION_TOKENS.load(Ordering::Relaxed) as i64),
    );
    h.insert(
        "cache_read_tokens".to_string(),
        PerlValue::integer(CACHE_READ_TOKENS.load(Ordering::Relaxed) as i64),
    );
    h.insert(
        "cache_hits".to_string(),
        PerlValue::integer(CACHE_HITS.load(Ordering::Relaxed) as i64),
    );
    h.insert(
        "cache_misses".to_string(),
        PerlValue::integer(CACHE_MISSES.load(Ordering::Relaxed) as i64),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))))
}

/// `ai_last_thinking()` → the extended-thinking block text from the
/// most recent Anthropic call, or empty string if none.
pub(crate) fn ai_last_thinking(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    Ok(PerlValue::string(last_thinking().lock().clone()))
}

/// `ai_dashboard()` → ANSI-colored multi-line summary of cost/tokens/cache.
/// Useful for dropping at the end of a script or during an interactive
/// session: `print ai_dashboard()`. Pure output formatter — no side
/// effects on counters. Color codes are stripped automatically when
/// stdout is not a tty (so logs stay clean).
pub(crate) fn ai_dashboard(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let usd = current_cost_usd();
    let inp = INPUT_TOKENS.load(Ordering::Relaxed);
    let out = OUTPUT_TOKENS.load(Ordering::Relaxed);
    let emb = EMBED_TOKENS.load(Ordering::Relaxed);
    let cc = CACHE_CREATION_TOKENS.load(Ordering::Relaxed);
    let cr = CACHE_READ_TOKENS.load(Ordering::Relaxed);
    let hits = CACHE_HITS.load(Ordering::Relaxed);
    let misses = CACHE_MISSES.load(Ordering::Relaxed);
    let total = hits + misses;
    let hit_pct = if total > 0 {
        (hits as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    let cache_savings_tokens = cr;
    let cap = config().lock().max_cost_run_usd;
    let cap_str = if cap > 0.0 && cap.is_finite() {
        format!("${:.4} / ${:.4}", usd, cap)
    } else {
        format!("${:.4}", usd)
    };
    let use_color = is_tty_stdout();
    let (c_cyan, c_green, c_yellow, c_dim, c_reset) = if use_color {
        (
            "\x1b[1;36m",
            "\x1b[1;32m",
            "\x1b[1;33m",
            "\x1b[2m",
            "\x1b[0m",
        )
    } else {
        ("", "", "", "", "")
    };
    let mut s = String::new();
    s.push_str(&format!(
        "{c_cyan}┌─ stryke ai dashboard ─{c_reset}\n",
        c_cyan = c_cyan,
        c_reset = c_reset
    ));
    s.push_str(&format!(
        "│ {c_dim}cost{c_reset}     {c_green}{}{c_reset}\n",
        cap_str,
        c_dim = c_dim,
        c_green = c_green,
        c_reset = c_reset
    ));
    s.push_str(&format!(
        "│ {c_dim}prompt{c_reset}   in {} / out {}\n",
        inp,
        out,
        c_dim = c_dim,
        c_reset = c_reset
    ));
    if emb > 0 {
        s.push_str(&format!(
            "│ {c_dim}embed{c_reset}    {} tokens\n",
            emb,
            c_dim = c_dim,
            c_reset = c_reset
        ));
    }
    if cc > 0 || cr > 0 {
        s.push_str(&format!(
            "│ {c_dim}prompt cache{c_reset}  write {} / read {} {c_yellow}(saved ~{} tokens){c_reset}\n",
            cc,
            cr,
            cache_savings_tokens,
            c_dim = c_dim,
            c_yellow = c_yellow,
            c_reset = c_reset
        ));
    }
    if total > 0 {
        s.push_str(&format!(
            "│ {c_dim}result cache{c_reset}  {}/{} ({:.1}% hit)\n",
            hits,
            total,
            hit_pct,
            c_dim = c_dim,
            c_reset = c_reset
        ));
    }
    s.push_str(&format!(
        "{c_cyan}└──────{c_reset}\n",
        c_cyan = c_cyan,
        c_reset = c_reset
    ));
    Ok(PerlValue::string(s))
}

/// `ai_pricing($model)` → hashref `+{ input => $usd_per_1k, output => $usd_per_1k }`.
/// Returns the per-1K-token pricing the runtime uses for cost tracking. Call
/// `ai_pricing("claude-opus-4-7")` etc. to see what an upcoming request will
/// cost without sending it. Models we don't recognize fall back to the
/// "sensible default" tier (Sonnet-class).
pub(crate) fn ai_pricing(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let model = args
        .first()
        .map(|v| v.to_string())
        .unwrap_or_else(|| config().lock().model.clone());
    let (in_per_1k, out_per_1k) = price_per_1k_tokens(&model);
    let mut h = IndexMap::new();
    h.insert("model".to_string(), PerlValue::string(model));
    h.insert("input".to_string(), PerlValue::float(in_per_1k));
    h.insert("output".to_string(), PerlValue::float(out_per_1k));
    h.insert(
        "input_per_1m".to_string(),
        PerlValue::float(in_per_1k * 1000.0),
    );
    h.insert(
        "output_per_1m".to_string(),
        PerlValue::float(out_per_1k * 1000.0),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))))
}

/// `ai_describe("path/or/url.png", style => "concise")` — convenience wrapper
/// around `ai_vision` that asks for a description. `style => "concise"` (one
/// sentence), `"detailed"` (paragraph), or any custom prompt suffix appended
/// to "Describe this image".
pub(crate) fn ai_describe(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let img = args
        .first()
        .cloned()
        .ok_or_else(|| PerlError::runtime("ai_describe: image path/url required", line))?;
    let opts = parse_opts(&args[1..]);
    let style = opt_str(&opts, "style", "concise");
    let suffix = match style.as_str() {
        "concise" => " in one sentence.",
        "detailed" => " in detail, including objects, colors, and atmosphere.",
        "alt" => " as an HTML alt-text attribute, ≤125 chars.",
        custom if !custom.is_empty() => &format!(" {}", custom),
        _ => ".",
    };
    let prompt = format!("Describe this image{}", suffix);
    let mut vision_args: Vec<PerlValue> = vec![
        PerlValue::string(prompt),
        PerlValue::string("image".to_string()),
        img,
    ];
    for (k, v) in opts.iter().filter(|(k, _)| k.as_str() != "style") {
        vision_args.push(PerlValue::string(k.clone()));
        vision_args.push(v.clone());
    }
    ai_vision(&vision_args, line)
}

/// True if stdout is a tty — drives ANSI color emission in `ai_dashboard`.
fn is_tty_stdout() -> bool {
    #[cfg(unix)]
    unsafe {
        libc::isatty(libc::STDOUT_FILENO) == 1
    }
    #[cfg(not(unix))]
    {
        false
    }
}

pub(crate) fn ai_cache_clear(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    cache().lock().clear();
    CACHE_HITS.store(0, Ordering::Relaxed);
    CACHE_MISSES.store(0, Ordering::Relaxed);
    Ok(PerlValue::UNDEF)
}

pub(crate) fn ai_cache_size(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    Ok(PerlValue::integer(cache().lock().len() as i64))
}

/// `ai_mock_install("regex pattern", "response string")` — install a
/// mock that intercepts any subsequent `ai`/`prompt` call whose prompt
/// matches the regex. Stack-ordered: first match wins. Use
/// `ai_mock_clear()` between tests.
pub(crate) fn ai_mock_install(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let pattern = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_mock_install: pattern required", line))?;
    let response = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_mock_install: response required", line))?;
    let re = regex::Regex::new(&pattern).map_err(|e| {
        PerlError::runtime(
            format!("ai_mock_install: bad regex `{}`: {}", pattern, e),
            line,
        )
    })?;
    mocks().lock().push((re, response));
    Ok(PerlValue::UNDEF)
}

pub(crate) fn ai_mock_clear(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    mocks().lock().clear();
    Ok(PerlValue::UNDEF)
}

pub(crate) fn ai_config_get(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let key = args.first().map(|v| v.to_string()).unwrap_or_default();
    let cfg = config().lock().clone();
    let v = match key.as_str() {
        "provider" => PerlValue::string(cfg.provider),
        "model" => PerlValue::string(cfg.model),
        "api_key_env" => PerlValue::string(cfg.api_key_env),
        "cache" => PerlValue::integer(if cfg.cache { 1 } else { 0 }),
        "max_cost_run" => PerlValue::float(cfg.max_cost_run_usd),
        "embed_provider" => PerlValue::string(cfg.embed_provider),
        "embed_model" => PerlValue::string(cfg.embed_model),
        "embed_api_key_env" => PerlValue::string(cfg.embed_api_key_env),
        "" => {
            // Return the whole config as a hashref.
            let mut h = IndexMap::new();
            h.insert("provider".into(), PerlValue::string(cfg.provider));
            h.insert("model".into(), PerlValue::string(cfg.model));
            h.insert("api_key_env".into(), PerlValue::string(cfg.api_key_env));
            h.insert(
                "cache".into(),
                PerlValue::integer(if cfg.cache { 1 } else { 0 }),
            );
            h.insert(
                "max_cost_run".into(),
                PerlValue::float(cfg.max_cost_run_usd),
            );
            h.insert(
                "embed_provider".into(),
                PerlValue::string(cfg.embed_provider),
            );
            h.insert("embed_model".into(), PerlValue::string(cfg.embed_model));
            return Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))));
        }
        other => {
            return Err(PerlError::runtime(
                format!("ai_config_get: unknown key `{}`", other),
                0,
            ))
        }
    };
    Ok(v)
}

/// `ai_config_set("model", "claude-haiku-...")` — at-runtime override.
pub(crate) fn ai_config_set(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "ai_config_set: usage: ai_config_set(\"key\", value)",
            line,
        ));
    }
    let key = args[0].to_string();
    let val = &args[1];
    let mut cfg = config().lock();
    match key.as_str() {
        "provider" => cfg.provider = val.to_string(),
        "model" => cfg.model = val.to_string(),
        "api_key_env" => cfg.api_key_env = val.to_string(),
        "cache" => cfg.cache = val.to_int() != 0,
        "max_cost_run" => cfg.max_cost_run_usd = val.to_number(),
        "embed_provider" => cfg.embed_provider = val.to_string(),
        "embed_model" => cfg.embed_model = val.to_string(),
        "embed_api_key_env" => cfg.embed_api_key_env = val.to_string(),
        other => {
            return Err(PerlError::runtime(
                format!("ai_config_set: unknown key `{}`", other),
                line,
            ))
        }
    }
    Ok(PerlValue::UNDEF)
}

// ── Provider: Anthropic ───────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn call_anthropic(
    prompt: &str,
    system: &str,
    model: &str,
    max_tokens: i64,
    temperature: f64,
    timeout: i64,
    cache_control: bool,
    thinking: bool,
    thinking_budget: i64,
    line: usize,
) -> Result<String> {
    let key_env = config().lock().api_key_env.clone();
    let api_key = std::env::var(&key_env)
        .map_err(|_| PerlError::runtime(format!("ai: ${} env var not set", key_env), line))?;
    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": [{ "role": "user", "content": prompt }],
    });
    if !system.is_empty() {
        if cache_control {
            // Anthropic prompt caching — system block becomes a list
            // with cache_control set so subsequent calls reuse the
            // same prefix at ~10% of normal input cost.
            body["system"] = serde_json::json!([{
                "type": "text",
                "text": system,
                "cache_control": { "type": "ephemeral" }
            }]);
        } else {
            body["system"] = serde_json::Value::String(system.to_string());
        }
    }
    if temperature >= 0.0 {
        body["temperature"] = serde_json::Value::from(temperature);
    }
    if thinking {
        body["thinking"] = serde_json::json!({
            "type": "enabled",
            "budget_tokens": thinking_budget,
        });
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let json = call_anthropic_with_retry(&agent, &api_key, body, line)?;

    // Track tokens / cost — including the cache stats Anthropic returns
    // when prompt caching is in use. Cache writes cost ~25% extra,
    // cache reads cost ~10% of normal input. We charge accordingly.
    let usage = &json["usage"];
    let input = usage["input_tokens"].as_u64().unwrap_or(0);
    let output = usage["output_tokens"].as_u64().unwrap_or(0);
    let cache_creation = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
    let cache_read = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
    INPUT_TOKENS.fetch_add(input, Ordering::Relaxed);
    OUTPUT_TOKENS.fetch_add(output, Ordering::Relaxed);
    CACHE_CREATION_TOKENS.fetch_add(cache_creation, Ordering::Relaxed);
    CACHE_READ_TOKENS.fetch_add(cache_read, Ordering::Relaxed);
    let (in_per_1k, out_per_1k) = price_per_1k_tokens(model);
    let normal_cost = input as f64 / 1000.0 * in_per_1k + output as f64 / 1000.0 * out_per_1k;
    let cache_cost = cache_creation as f64 / 1000.0 * in_per_1k * 1.25
        + cache_read as f64 / 1000.0 * in_per_1k * 0.10;
    add_cost(normal_cost + cache_cost);

    // Extract assistant text — and any extended-thinking blocks if the
    // model included them. Both stay in `content[]`. Citations attached
    // to text blocks (Anthropic Citations feature) accumulate into
    // `LAST_CITATIONS_BUF` and are surfaced via `ai_citations()`.
    let mut out = String::new();
    let mut thinking_text = String::new();
    let mut citations: Vec<serde_json::Value> = Vec::new();
    if let Some(arr) = json["content"].as_array() {
        for chunk in arr {
            match chunk["type"].as_str() {
                Some("text") => {
                    if let Some(t) = chunk["text"].as_str() {
                        out.push_str(t);
                    }
                    if let Some(cs) = chunk["citations"].as_array() {
                        citations.extend(cs.iter().cloned());
                    }
                }
                Some("thinking") => {
                    if let Some(t) = chunk["thinking"].as_str() {
                        thinking_text.push_str(t);
                    }
                }
                _ => {}
            }
        }
    }
    if !thinking_text.is_empty() {
        *last_thinking().lock() = thinking_text;
    } else {
        last_thinking().lock().clear();
    }
    *last_citations().lock() = citations;
    if out.is_empty() {
        return Err(PerlError::runtime(
            format!(
                "ai: anthropic returned no content (raw: {})",
                truncate(&json.to_string(), 200)
            ),
            line,
        ));
    }
    Ok(out)
}

// ── Provider: OpenAI ──────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn call_openai_with_base(
    prompt: &str,
    system: &str,
    model: &str,
    max_tokens: i64,
    temperature: f64,
    timeout: i64,
    url: &str,
    key_env: &str,
    line: usize,
) -> Result<String> {
    // Local OpenAI-compatible servers (LM Studio, llama-server, vLLM,
    // ollama in compat mode) often don't require auth. Treat missing
    // env as empty and let the server reject if it cares.
    let api_key = std::env::var(key_env).unwrap_or_default();
    let mut messages: Vec<serde_json::Value> = Vec::new();
    if !system.is_empty() {
        messages.push(serde_json::json!({"role": "system", "content": system}));
    }
    messages.push(serde_json::json!({"role": "user", "content": prompt}));

    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": messages,
    });
    if temperature >= 0.0 {
        body["temperature"] = serde_json::Value::from(temperature);
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let mut req = agent.post(url).set("content-type", "application/json");
    if !api_key.is_empty() {
        req = req.set("authorization", &format!("Bearer {}", api_key));
    }
    let resp = req
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("ai: openai request: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai: openai decode: {}", e), line))?;

    let usage = &json["usage"];
    let input = usage["prompt_tokens"].as_u64().unwrap_or(0);
    let output = usage["completion_tokens"].as_u64().unwrap_or(0);
    INPUT_TOKENS.fetch_add(input, Ordering::Relaxed);
    OUTPUT_TOKENS.fetch_add(output, Ordering::Relaxed);
    let (in_per_1k, out_per_1k) = price_per_1k_tokens(model);
    add_cost(input as f64 / 1000.0 * in_per_1k + output as f64 / 1000.0 * out_per_1k);

    let text = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if text.is_empty() {
        return Err(PerlError::runtime(
            format!(
                "ai: openai returned no content (raw: {})",
                truncate(&json.to_string(), 200)
            ),
            line,
        ));
    }
    Ok(text)
}

// ── Embedding providers ───────────────────────────────────────────────

fn call_voyage_embed(
    inputs: &[String],
    model: &str,
    api_key_env: &str,
    line: usize,
) -> Result<PerlValue> {
    let api_key = std::env::var(api_key_env)
        .map_err(|_| PerlError::runtime(format!("embed: ${} not set", api_key_env), line))?;
    let body = serde_json::json!({
        "input": inputs,
        "model": model,
    });
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let resp = agent
        .post("https://api.voyageai.com/v1/embeddings")
        .set("authorization", &format!("Bearer {}", api_key))
        .set("content-type", "application/json")
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("embed: voyage request: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("embed: voyage decode: {}", e), line))?;

    if let Some(t) = json["usage"]["total_tokens"].as_u64() {
        EMBED_TOKENS.fetch_add(t, Ordering::Relaxed);
        // Voyage-3 is $0.06 / 1M tokens.
        add_cost(t as f64 / 1_000_000.0 * 0.06);
    }

    embeddings_response_to_perl(&json["data"])
}

fn call_openai_embed(
    inputs: &[String],
    model: &str,
    api_key_env: &str,
    line: usize,
) -> Result<PerlValue> {
    let api_key = std::env::var(api_key_env)
        .map_err(|_| PerlError::runtime(format!("embed: ${} not set", api_key_env), line))?;
    let body = serde_json::json!({
        "input": inputs,
        "model": model,
    });
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let resp = agent
        .post("https://api.openai.com/v1/embeddings")
        .set("authorization", &format!("Bearer {}", api_key))
        .set("content-type", "application/json")
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("embed: openai request: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("embed: openai decode: {}", e), line))?;
    if let Some(t) = json["usage"]["total_tokens"].as_u64() {
        EMBED_TOKENS.fetch_add(t, Ordering::Relaxed);
        // text-embedding-3-small is $0.02 / 1M.
        add_cost(t as f64 / 1_000_000.0 * 0.02);
    }
    embeddings_response_to_perl(&json["data"])
}

/// Local embeddings via Ollama's `/api/embed` endpoint. Cost is zero because
/// the model runs on the user's hardware. The default model is
/// `nomic-embed-text` — small, fast, English-tuned. Other models work as long
/// as they're pulled into Ollama (`ollama pull mxbai-embed-large`, etc.).
fn call_ollama_embed(inputs: &[String], model: &str, line: usize) -> Result<PerlValue> {
    let base = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".into());
    let url = format!("{}/api/embed", base.trim_end_matches('/'));
    let model = if model.is_empty() || model == "voyage-3" || model.starts_with("text-embedding") {
        "nomic-embed-text"
    } else {
        model
    };
    let body = serde_json::json!({
        "model": model,
        "input": inputs,
    });
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();
    let resp = agent
        .post(&url)
        .set("content-type", "application/json")
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("embed: ollama request: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("embed: ollama decode: {}", e), line))?;
    // Ollama returns `{embeddings: [[...], [...]]}` — reshape into the
    // OpenAI-style `data: [{embedding: [...]}, ...]` we already parse.
    let embeddings = json["embeddings"].as_array().cloned().unwrap_or_default();
    let mut shaped: Vec<serde_json::Value> = Vec::with_capacity(embeddings.len());
    for emb in embeddings {
        shaped.push(serde_json::json!({ "embedding": emb }));
    }
    embeddings_response_to_perl(&serde_json::Value::Array(shaped))
}

fn embeddings_response_to_perl(data: &serde_json::Value) -> Result<PerlValue> {
    let arr = data
        .as_array()
        .ok_or_else(|| PerlError::runtime("embed: provider returned non-array data", 0))?;
    let mut all: Vec<PerlValue> = Vec::with_capacity(arr.len());
    for item in arr {
        let vec_arr = item["embedding"]
            .as_array()
            .ok_or_else(|| PerlError::runtime("embed: missing embedding array", 0))?;
        let floats: Vec<PerlValue> = vec_arr
            .iter()
            .filter_map(|x| x.as_f64().map(PerlValue::float))
            .collect();
        all.push(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
            floats,
        ))));
    }
    if all.len() == 1 {
        // Single input → single embedding hashref.
        return Ok(all.into_iter().next().unwrap());
    }
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        all,
    ))))
}

// ── Helpers ────────────────────────────────────────────────────────────

fn parse_opts(args: &[PerlValue]) -> IndexMap<String, PerlValue> {
    let mut out = IndexMap::new();
    let mut i = 0;
    while i + 1 < args.len() {
        out.insert(args[i].to_string(), args[i + 1].clone());
        i += 2;
    }
    out
}

fn opt_str(opts: &IndexMap<String, PerlValue>, k: &str, default: &str) -> String {
    opts.get(k)
        .map(|v| v.to_string())
        .unwrap_or_else(|| default.to_string())
}

fn opt_int(opts: &IndexMap<String, PerlValue>, k: &str, default: i64) -> i64 {
    opts.get(k).map(|v| v.to_int()).unwrap_or(default)
}

fn opt_float(opts: &IndexMap<String, PerlValue>, k: &str, default: f64) -> f64 {
    opts.get(k).map(|v| v.to_number()).unwrap_or(default)
}

fn opt_bool(opts: &IndexMap<String, PerlValue>, k: &str, default: bool) -> bool {
    match opts.get(k) {
        Some(v) => v.to_int() != 0,
        None => default,
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push('…');
        out
    }
}

// ── Agent loop (Phase 1) ──────────────────────────────────────────────
//
// Without parser changes for `tool fn`, the user passes tools as a
// list of hashrefs:
//
//     ai "task description",
//         tools => [
//             +{ name => "search", description => "Web search",
//                parameters => +{ q => "string" },
//                run => sub { my $q = $_[0]->{q}; ... return $result } },
//             ...
//         ],
//         max_turns => 10
//
// The runtime drives Anthropic's tool_use protocol: model returns a
// tool_use block, we invoke the matching `run` coderef, hand the
// result back, repeat until the model returns a final text answer or
// `max_turns` is hit.

impl Interpreter {
    pub(crate) fn ai_agent(&mut self, args: &[PerlValue], line: usize) -> Result<PerlValue> {
        let prompt = args
            .first()
            .map(|v| v.to_string())
            .ok_or_else(|| PerlError::runtime("ai_agent: prompt required", line))?;
        let opts = parse_opts(&args[1..]);

        // Three sources contribute tools to the agent loop:
        //   1. Explicit `tools => [...]` arg.
        //   2. Globally registered tools via `ai_register_tool(...)`.
        //   3. Attached MCP servers via `mcp_attach_to_ai(...)` when
        //      `auto_mcp => 1` is set (default on for v0).
        let tools_list: Vec<PerlValue> = match opts.get("tools") {
            Some(v) => v
                .as_array_ref()
                .map(|a| a.read().clone())
                .unwrap_or_else(|| v.clone().to_list()),
            None => Vec::new(),
        };
        let registered = registered_tools().lock().clone();
        let auto_mcp = opt_int(&opts, "auto_mcp", 1) != 0;
        let attached = if auto_mcp {
            crate::mcp::collect_attached_tools(line)
        } else {
            Vec::new()
        };

        if tools_list.is_empty() && registered.is_empty() && attached.is_empty() {
            return ai_prompt(args, line);
        }

        let provider = opt_str(&opts, "provider", &config().lock().provider);
        let model = opt_str(&opts, "model", &config().lock().model);
        let system = opt_str(&opts, "system", "");
        let max_tokens = opt_int(&opts, "max_tokens", 1024);
        let max_turns = opt_int(&opts, "max_turns", 10);
        let temperature = opt_float(&opts, "temperature", -1.0);
        let timeout = opt_int(&opts, "timeout", 60);

        // Mock mode short-circuits the whole loop — tests want one
        // string response, not a multi-turn dance.
        if let Some(resp) = match_mock(&prompt) {
            return Ok(PerlValue::string(resp));
        }
        if mock_only_mode() {
            return Err(PerlError::runtime(
                format!(
                    "ai_agent: STRYKE_AI_MODE=mock-only and no mock matched prompt {:?}",
                    truncate(&prompt, 60)
                ),
                line,
            ));
        }

        // Build the tools list as a JSON array shaped for the provider.
        let mut compiled: Vec<CompiledTool> =
            Vec::with_capacity(tools_list.len() + registered.len() + attached.len());
        for t in &tools_list {
            compiled.push(compile_tool(t, line)?);
        }
        for r in &registered {
            compiled.push(CompiledTool {
                name: r.name.clone(),
                description: r.description.clone(),
                input_schema: parameters_to_json_schema(&r.parameters),
                run_sub: Some(r.run_sub.clone()),
                mcp_handle_id: None,
                native_id: None,
            });
        }
        for at in &attached {
            // MCP tool spec uses `inputSchema` (camelCase) per protocol.
            let schema = at
                .spec
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));
            let name = at
                .spec
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let description = at
                .spec
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            // Prefix with server name to disambiguate when multiple MCP
            // servers expose the same tool name.
            let display_name = if compiled.iter().any(|c| c.name == name) {
                format!("{}_{}", at.server_name, name)
            } else {
                name.clone()
            };
            compiled.push(CompiledTool {
                name: display_name,
                description,
                input_schema: schema,
                run_sub: None,
                mcp_handle_id: Some((at.handle_id, name)),
                native_id: None,
            });
        }

        match provider.as_str() {
            "anthropic" => self.run_anthropic_agent(
                &prompt,
                &system,
                &model,
                max_tokens,
                max_turns,
                temperature,
                timeout,
                &compiled,
                line,
            ),
            "openai" => self.run_openai_agent(
                &prompt,
                &system,
                &model,
                max_tokens,
                max_turns,
                temperature,
                timeout,
                &compiled,
                line,
            ),
            other => Err(PerlError::runtime(
                format!("ai_agent: provider `{}` not supported in v0", other),
                line,
            )),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run_anthropic_agent(
        &mut self,
        prompt: &str,
        system: &str,
        model: &str,
        max_tokens: i64,
        max_turns: i64,
        temperature: f64,
        timeout: i64,
        tools: &[CompiledTool],
        line: usize,
    ) -> Result<PerlValue> {
        let key_env = config().lock().api_key_env.clone();
        let api_key = std::env::var(&key_env).map_err(|_| {
            PerlError::runtime(format!("ai_agent: ${} env var not set", key_env), line)
        })?;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(timeout.max(1) as u64))
            .build();

        let tool_specs: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.input_schema.clone(),
                })
            })
            .collect();

        let mut messages: Vec<serde_json::Value> =
            vec![serde_json::json!({"role": "user", "content": prompt})];

        for turn in 0..max_turns {
            // Cost ceiling check on every turn.
            let ceiling = config().lock().max_cost_run_usd;
            if ceiling > 0.0 && current_cost_usd() >= ceiling {
                return Err(PerlError::runtime(
                    format!(
                        "ai_agent: max_cost_run_usd={:.2} exceeded after turn {} (current ${:.4})",
                        ceiling,
                        turn,
                        current_cost_usd()
                    ),
                    line,
                ));
            }

            let mut body = serde_json::json!({
                "model": model,
                "max_tokens": max_tokens,
                "messages": messages,
                "tools": tool_specs,
            });
            if !system.is_empty() {
                body["system"] = serde_json::Value::String(system.to_string());
            }
            if temperature >= 0.0 {
                body["temperature"] = serde_json::Value::from(temperature);
            }

            let resp = agent
                .post("https://api.anthropic.com/v1/messages")
                .set("x-api-key", &api_key)
                .set("anthropic-version", "2023-06-01")
                .set("content-type", "application/json")
                .send_json(body)
                .map_err(|e| {
                    PerlError::runtime(format!("ai_agent: anthropic turn {}: {}", turn, e), line)
                })?;
            let json: serde_json::Value = resp
                .into_json()
                .map_err(|e| PerlError::runtime(format!("ai_agent: decode: {}", e), line))?;

            // Track tokens / cost for this turn.
            if let Some(input) = json["usage"]["input_tokens"].as_u64() {
                INPUT_TOKENS.fetch_add(input, Ordering::Relaxed);
                let (in_per_1k, _) = price_per_1k_tokens(model);
                add_cost(input as f64 / 1000.0 * in_per_1k);
            }
            if let Some(output) = json["usage"]["output_tokens"].as_u64() {
                OUTPUT_TOKENS.fetch_add(output, Ordering::Relaxed);
                let (_, out_per_1k) = price_per_1k_tokens(model);
                add_cost(output as f64 / 1000.0 * out_per_1k);
            }

            let stop_reason = json["stop_reason"].as_str().unwrap_or("");
            let content = json["content"].as_array().cloned().unwrap_or_default();

            // Append assistant turn to history so next turn's tool
            // results land in the right context.
            messages.push(serde_json::json!({
                "role": "assistant",
                "content": content.clone(),
            }));

            // Walk content blocks: collect text, dispatch tool_use.
            let mut tool_results: Vec<serde_json::Value> = Vec::new();
            let mut final_text = String::new();
            for block in &content {
                match block["type"].as_str() {
                    Some("text") => {
                        if let Some(t) = block["text"].as_str() {
                            final_text.push_str(t);
                        }
                    }
                    Some("tool_use") => {
                        let id = block["id"].as_str().unwrap_or("").to_string();
                        let name = block["name"].as_str().unwrap_or("").to_string();
                        let input = block["input"].clone();
                        let output = self.invoke_tool(tools, &name, input, line)?;
                        tool_results.push(serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": output,
                        }));
                    }
                    _ => {}
                }
            }

            if stop_reason == "end_turn" || tool_results.is_empty() {
                if !final_text.is_empty() {
                    return Ok(PerlValue::string(final_text));
                }
                if stop_reason == "end_turn" {
                    return Ok(PerlValue::string(String::new()));
                }
            }

            messages.push(serde_json::json!({
                "role": "user",
                "content": tool_results,
            }));
        }

        Err(PerlError::runtime(
            format!("ai_agent: hit max_turns={} without final answer", max_turns),
            line,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn run_openai_agent(
        &mut self,
        prompt: &str,
        system: &str,
        model: &str,
        max_tokens: i64,
        max_turns: i64,
        temperature: f64,
        timeout: i64,
        tools: &[CompiledTool],
        line: usize,
    ) -> Result<PerlValue> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| PerlError::runtime("ai_agent: $OPENAI_API_KEY not set", line))?;
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(timeout.max(1) as u64))
            .build();

        let tool_specs: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.input_schema.clone(),
                    }
                })
            })
            .collect();

        let mut messages: Vec<serde_json::Value> = Vec::new();
        if !system.is_empty() {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system,
            }));
        }
        messages.push(serde_json::json!({
            "role": "user",
            "content": prompt,
        }));

        for turn in 0..max_turns {
            let ceiling = config().lock().max_cost_run_usd;
            if ceiling > 0.0 && current_cost_usd() >= ceiling {
                return Err(PerlError::runtime(
                    format!(
                        "ai_agent: max_cost_run_usd={:.2} exceeded after turn {}",
                        ceiling, turn
                    ),
                    line,
                ));
            }

            let mut body = serde_json::json!({
                "model": model,
                "max_tokens": max_tokens,
                "messages": messages,
                "tools": tool_specs,
            });
            if temperature >= 0.0 {
                body["temperature"] = serde_json::Value::from(temperature);
            }

            let resp = agent
                .post("https://api.openai.com/v1/chat/completions")
                .set("authorization", &format!("Bearer {}", api_key))
                .set("content-type", "application/json")
                .send_json(body)
                .map_err(|e| {
                    PerlError::runtime(format!("ai_agent: openai turn {}: {}", turn, e), line)
                })?;
            let json: serde_json::Value = resp
                .into_json()
                .map_err(|e| PerlError::runtime(format!("ai_agent: decode: {}", e), line))?;

            if let Some(input) = json["usage"]["prompt_tokens"].as_u64() {
                INPUT_TOKENS.fetch_add(input, Ordering::Relaxed);
                let (in_per_1k, _) = price_per_1k_tokens(model);
                add_cost(input as f64 / 1000.0 * in_per_1k);
            }
            if let Some(output) = json["usage"]["completion_tokens"].as_u64() {
                OUTPUT_TOKENS.fetch_add(output, Ordering::Relaxed);
                let (_, out_per_1k) = price_per_1k_tokens(model);
                add_cost(output as f64 / 1000.0 * out_per_1k);
            }

            let choice = &json["choices"][0];
            let msg = &choice["message"];
            let finish = choice["finish_reason"].as_str().unwrap_or("");
            messages.push(msg.clone());

            if finish == "tool_calls" {
                let calls = msg["tool_calls"].as_array().cloned().unwrap_or_default();
                for c in &calls {
                    let id = c["id"].as_str().unwrap_or("").to_string();
                    let name = c["function"]["name"].as_str().unwrap_or("").to_string();
                    let raw_args = c["function"]["arguments"].as_str().unwrap_or("{}");
                    let parsed: serde_json::Value =
                        serde_json::from_str(raw_args).unwrap_or(serde_json::Value::Null);
                    let output = self.invoke_tool(tools, &name, parsed, line)?;
                    let output_str = match output {
                        serde_json::Value::String(s) => s,
                        v => v.to_string(),
                    };
                    messages.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": id,
                        "name": name,
                        "content": output_str,
                    }));
                }
                continue;
            }

            // Final answer.
            return Ok(PerlValue::string(
                msg["content"].as_str().unwrap_or("").to_string(),
            ));
        }

        Err(PerlError::runtime(
            format!("ai_agent: hit max_turns={} without final answer", max_turns),
            line,
        ))
    }

    fn invoke_tool(
        &mut self,
        tools: &[CompiledTool],
        name: &str,
        input: serde_json::Value,
        line: usize,
    ) -> Result<serde_json::Value> {
        let tool = tools.iter().find(|t| t.name == name).ok_or_else(|| {
            PerlError::runtime(
                format!("ai_agent: model called unknown tool `{}`", name),
                line,
            )
        })?;

        // Native built-in tool fast path.
        if let Some(id) = tool.native_id {
            let arg = json_to_perl(&input);
            let r = invoke_native_tool(id, arg, line)?;
            return Ok(perl_to_json(&r));
        }

        // MCP-routed: forward to the connected server.
        if let Some((handle_id, server_name)) = &tool.mcp_handle_id {
            return crate::mcp::call_attached_tool(*handle_id, server_name, input, line);
        }

        let run_sub = tool.run_sub.as_ref().ok_or_else(|| {
            PerlError::runtime(
                format!("ai_agent: tool `{}` has no implementation", name),
                line,
            )
        })?;
        let arg_hash = json_to_perl(&input);
        let result = match self.call_sub(run_sub, vec![arg_hash], WantarrayCtx::Scalar, line) {
            Ok(v) => v,
            Err(FlowOrError::Flow(_)) => PerlValue::UNDEF,
            Err(FlowOrError::Error(e)) => {
                let mut em = IndexMap::new();
                em.insert("error".to_string(), PerlValue::string(format!("{}", e)));
                PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(em)))
            }
        };

        Ok(perl_to_json(&result))
    }
}

struct CompiledTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
    /// Stryke coderef for explicit/registered tools.
    run_sub: Option<Arc<crate::value::PerlSub>>,
    /// (handle_id, server-side tool name) for MCP-routed tools.
    mcp_handle_id: Option<(u64, String)>,
    /// Built-in native tool registry id (web_search, fetch_url, etc.)
    native_id: Option<i64>,
}

fn compile_tool(v: &PerlValue, line: usize) -> Result<CompiledTool> {
    let map = v
        .as_hash_map()
        .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        .ok_or_else(|| {
            PerlError::runtime(
                "ai_agent: each tool must be a hashref +{name, description, parameters, run}",
                line,
            )
        })?;
    let name = map
        .get("name")
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_agent: tool missing `name`", line))?;
    let description = map
        .get("description")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let parameters = map.get("parameters").cloned().unwrap_or(PerlValue::UNDEF);
    let input_schema = parameters_to_json_schema(&parameters);

    // Built-in tool fast path: if `__native_tool_id__` is set, skip
    // the `run` coderef requirement and route through the native
    // registry instead.
    if let Some(native_id) = map.get("__native_tool_id__").map(|v| v.to_int()) {
        return Ok(CompiledTool {
            name,
            description,
            input_schema,
            run_sub: None,
            mcp_handle_id: None,
            native_id: Some(native_id),
        });
    }

    let run_v = map.get("run").ok_or_else(|| {
        PerlError::runtime(
            format!("ai_agent: tool `{}` missing `run` coderef", name),
            line,
        )
    })?;
    let run_sub = run_v.as_code_ref().ok_or_else(|| {
        PerlError::runtime(
            format!("ai_agent: tool `{}` has non-coderef `run`", name),
            line,
        )
    })?;

    Ok(CompiledTool {
        name,
        description,
        input_schema,
        run_sub: Some(run_sub),
        mcp_handle_id: None,
        native_id: None,
    })
}

fn parameters_to_json_schema(v: &PerlValue) -> serde_json::Value {
    // Accept either a plain hashref `{q => "string", limit => "int"}`
    // (treated as object schema with required fields) or a full
    // JSON-Schema-shaped hashref already (`{type: "object", properties:
    // {...}, required: [...]}`).
    let map = match v
        .as_hash_map()
        .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
    {
        Some(m) => m,
        None => return serde_json::json!({"type": "object", "properties": {}}),
    };

    if map.contains_key("type") && map.contains_key("properties") {
        // Looks like a real JSON schema — pass through.
        return perl_to_json_object(&map);
    }

    let mut props = serde_json::Map::new();
    let mut required: Vec<serde_json::Value> = Vec::new();
    for (k, type_v) in &map {
        let ty = type_v.to_string();
        let (json_ty, extra) = match ty.as_str() {
            "string" | "str" | "Str" => ("string", None),
            "int" | "integer" | "Int" => ("integer", None),
            "number" | "float" | "Float" | "Num" => ("number", None),
            "bool" | "boolean" | "Bool" => ("boolean", None),
            "array" | "list" => ("array", Some(serde_json::json!({"type": "string"}))),
            _ => ("string", None),
        };
        let mut p = serde_json::Map::new();
        p.insert("type".into(), serde_json::Value::String(json_ty.into()));
        if let Some(items) = extra {
            p.insert("items".into(), items);
        }
        props.insert(k.clone(), serde_json::Value::Object(p));
        required.push(serde_json::Value::String(k.clone()));
    }
    serde_json::json!({
        "type": "object",
        "properties": props,
        "required": required,
    })
}

fn perl_to_json_object(map: &IndexMap<String, PerlValue>) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for (k, v) in map {
        out.insert(k.clone(), perl_to_json(v));
    }
    serde_json::Value::Object(out)
}

fn perl_to_json(v: &PerlValue) -> serde_json::Value {
    if v.is_undef() {
        return serde_json::Value::Null;
    }
    if let Some(map) = v
        .as_hash_map()
        .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
    {
        return perl_to_json_object(&map);
    }
    if let Some(arr) = v.as_array_ref() {
        let items: Vec<serde_json::Value> = arr.read().iter().map(perl_to_json).collect();
        return serde_json::Value::Array(items);
    }
    if let Some(i) = v.as_integer() {
        return serde_json::Value::Number(serde_json::Number::from(i));
    }
    if let Some(f) = v.as_float() {
        return serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null);
    }
    if let Some(s) = v.as_str() {
        return serde_json::Value::String(s);
    }
    serde_json::Value::String(v.to_string())
}

fn json_to_perl(v: &serde_json::Value) -> PerlValue {
    match v {
        serde_json::Value::Null => PerlValue::UNDEF,
        serde_json::Value::Bool(b) => PerlValue::integer(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                PerlValue::integer(i)
            } else if let Some(f) = n.as_f64() {
                PerlValue::float(f)
            } else {
                PerlValue::UNDEF
            }
        }
        serde_json::Value::String(s) => PerlValue::string(s.clone()),
        serde_json::Value::Array(arr) => {
            let items: Vec<PerlValue> = arr.iter().map(json_to_perl).collect();
            PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(items)))
        }
        serde_json::Value::Object(obj) => {
            let mut m = IndexMap::new();
            for (k, v) in obj {
                m.insert(k.clone(), json_to_perl(v));
            }
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m)))
        }
    }
}

// ── Collection builtins (Phase 3) ─────────────────────────────────────
//
// Each one batches the input into a single LLM call where the prompt
// asks for a JSON array of judgments. Falls back to per-item calls
// when the batch parse fails (rare). Cost-conscious by construction.

pub(crate) fn ai_filter(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let (items, criterion, opts) = parse_collection_args(args, "ai_filter", line)?;
    let strs: Vec<String> = items.iter().map(|v| v.to_string()).collect();
    let prompt = format!(
        "Decide whether each item below matches this criterion: {}.\n\
         Return ONLY a JSON array of booleans, one per item, in order.\n\
         Items:\n{}",
        criterion,
        numbered_list(&strs)
    );
    let mut call_args = vec![PerlValue::string(prompt)];
    forward_opts(&mut call_args, &opts);
    let raw = ai_prompt(&call_args, line)?.to_string();
    let bools = parse_json_array_of_bools(&raw, items.len());
    let kept: Vec<PerlValue> = items
        .into_iter()
        .zip(bools.iter())
        .filter_map(|(v, b)| if *b { Some(v) } else { None })
        .collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        kept,
    ))))
}

pub(crate) fn ai_map(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let (items, instruction, opts) = parse_collection_args(args, "ai_map", line)?;
    let strs: Vec<String> = items.iter().map(|v| v.to_string()).collect();
    let prompt = format!(
        "Apply this instruction to each item: {}.\n\
         Return ONLY a JSON array of strings, one per item, in order.\n\
         Items:\n{}",
        instruction,
        numbered_list(&strs)
    );
    let mut call_args = vec![PerlValue::string(prompt)];
    forward_opts(&mut call_args, &opts);
    let raw = ai_prompt(&call_args, line)?.to_string();
    let strs = parse_json_array_of_strings(&raw, items.len());
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        strs.into_iter().map(PerlValue::string).collect(),
    ))))
}

pub(crate) fn ai_classify(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let (items, _criterion, opts) = parse_collection_args(args, "ai_classify", line)?;
    let labels_v = opts.get("into").or_else(|| opts.get("labels")).cloned();
    let labels: Vec<String> = labels_v
        .map(|v| {
            v.as_array_ref()
                .map(|a| a.read().iter().map(|x| x.to_string()).collect())
                .unwrap_or_else(|| vec![v.to_string()])
        })
        .unwrap_or_default();
    if labels.is_empty() {
        return Err(PerlError::runtime(
            "ai_classify: pass into => [\"label1\", \"label2\", ...]",
            line,
        ));
    }
    let strs: Vec<String> = items.iter().map(|v| v.to_string()).collect();
    let prompt = format!(
        "Classify each item below into exactly one of these labels: {}.\n\
         Return ONLY a JSON array of label strings, one per item, in order.\n\
         Items:\n{}",
        labels.join(", "),
        numbered_list(&strs)
    );
    let mut call_args = vec![PerlValue::string(prompt)];
    forward_opts(&mut call_args, &opts);
    let raw = ai_prompt(&call_args, line)?.to_string();
    let labs = parse_json_array_of_strings(&raw, items.len());
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        labs.into_iter().map(PerlValue::string).collect(),
    ))))
}

pub(crate) fn ai_match(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let item = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_match: item required", line))?;
    let criterion = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_match: criterion required", line))?;
    let opts = parse_opts(&args[2.min(args.len())..]);
    let prompt = format!(
        "Does this item match the criterion {:?}? Reply ONLY with `true` or `false`.\nItem: {}",
        criterion, item
    );
    let mut call_args = vec![PerlValue::string(prompt)];
    forward_opts(&mut call_args, &opts);
    let raw = ai_prompt(&call_args, line)?
        .to_string()
        .to_ascii_lowercase();
    Ok(PerlValue::integer(
        if raw.contains("true") && !raw.contains("false") {
            1
        } else if raw.starts_with('y') || raw.starts_with("\"y") {
            1
        } else {
            0
        },
    ))
}

pub(crate) fn ai_sort(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let (items, criterion, opts) = parse_collection_args(args, "ai_sort", line)?;
    let strs: Vec<String> = items.iter().map(|v| v.to_string()).collect();
    let prompt = format!(
        "Sort these items by: {}. Best/most-relevant first.\n\
         Return ONLY a JSON array of zero-based indexes (0..{}) describing the new order.\n\
         Items:\n{}",
        criterion,
        items.len() - 1,
        numbered_list(&strs)
    );
    let mut call_args = vec![PerlValue::string(prompt)];
    forward_opts(&mut call_args, &opts);
    let raw = ai_prompt(&call_args, line)?.to_string();
    let order = parse_json_array_of_ints(&raw);
    let mut out: Vec<PerlValue> = Vec::with_capacity(items.len());
    let mut seen = std::collections::HashSet::new();
    for idx in order {
        if idx >= 0 && (idx as usize) < items.len() && seen.insert(idx) {
            out.push(items[idx as usize].clone());
        }
    }
    // Append anything the model didn't index (defensive — model
    // sometimes drops items).
    for (i, v) in items.iter().enumerate() {
        if !seen.contains(&(i as i64)) {
            out.push(v.clone());
        }
    }
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        out,
    ))))
}

pub(crate) fn ai_dedupe(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let (items, hint, opts) = parse_collection_args(args, "ai_dedupe", line)?;
    let strs: Vec<String> = items.iter().map(|v| v.to_string()).collect();
    let prompt = format!(
        "Group duplicates among these items{}.\n\
         Return ONLY a JSON array where each entry is a list of zero-based\n\
         indexes that refer to the same underlying thing. Every index must\n\
         appear in exactly one group.\n\
         Items:\n{}",
        if hint.is_empty() {
            String::new()
        } else {
            format!(" ({})", hint)
        },
        numbered_list(&strs)
    );
    let mut call_args = vec![PerlValue::string(prompt)];
    forward_opts(&mut call_args, &opts);
    let raw = ai_prompt(&call_args, line)?.to_string();
    let groups = parse_json_array_of_int_arrays(&raw);
    let mut kept: Vec<PerlValue> = Vec::new();
    for g in groups {
        if let Some(&first) = g.first() {
            if first >= 0 && (first as usize) < items.len() {
                kept.push(items[first as usize].clone());
            }
        }
    }
    if kept.is_empty() {
        // Defensive — return originals if parse failed.
        kept = items;
    }
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        kept,
    ))))
}

fn parse_collection_args(
    args: &[PerlValue],
    name: &str,
    line: usize,
) -> Result<(Vec<PerlValue>, String, IndexMap<String, PerlValue>)> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            format!(
                "{}: usage: {}(\\@items, \"criterion\", opts...)",
                name, name
            ),
            line,
        ));
    }
    let items: Vec<PerlValue> = if let Some(arr) = args[0].as_array_ref() {
        arr.read().clone()
    } else {
        args[0].clone().to_list()
    };
    let criterion = args[1].to_string();
    let opts = parse_opts(&args[2..]);
    Ok((items, criterion, opts))
}

fn forward_opts(call_args: &mut Vec<PerlValue>, opts: &IndexMap<String, PerlValue>) {
    for k in &[
        "model",
        "system",
        "max_tokens",
        "temperature",
        "cache",
        "timeout",
    ] {
        if let Some(v) = opts.get(*k) {
            call_args.push(PerlValue::string(k.to_string()));
            call_args.push(v.clone());
        }
    }
}

fn numbered_list(items: &[String]) -> String {
    let mut out = String::new();
    for (i, s) in items.iter().enumerate() {
        out.push_str(&format!("[{}] {}\n", i, s));
    }
    out
}

fn extract_first_json_array(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut start = None;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate() {
        if esc {
            esc = false;
            continue;
        }
        if in_str {
            match b {
                b'\\' => esc = true,
                b'"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'[' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            b']' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s_idx) = start {
                        return Some(&s[s_idx..=i]);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_json_array_of_bools(raw: &str, expected: usize) -> Vec<bool> {
    let arr_str = match extract_first_json_array(raw) {
        Some(s) => s,
        None => return vec![false; expected],
    };
    let v: serde_json::Value = serde_json::from_str(arr_str).unwrap_or(serde_json::Value::Null);
    let mut out: Vec<bool> = v
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| match x {
                    serde_json::Value::Bool(b) => *b,
                    serde_json::Value::String(s) => {
                        matches!(s.to_ascii_lowercase().as_str(), "true" | "yes" | "1")
                    }
                    serde_json::Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
                    _ => false,
                })
                .collect()
        })
        .unwrap_or_default();
    out.resize(expected, false);
    out
}

fn parse_json_array_of_strings(raw: &str, expected: usize) -> Vec<String> {
    let arr_str = match extract_first_json_array(raw) {
        Some(s) => s,
        None => return vec![String::new(); expected],
    };
    let v: serde_json::Value = serde_json::from_str(arr_str).unwrap_or(serde_json::Value::Null);
    let mut out: Vec<String> = v
        .as_array()
        .map(|a| {
            a.iter()
                .map(|x| match x {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    out.resize(expected, String::new());
    out
}

fn parse_json_array_of_ints(raw: &str) -> Vec<i64> {
    let arr_str = match extract_first_json_array(raw) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let v: serde_json::Value = serde_json::from_str(arr_str).unwrap_or(serde_json::Value::Null);
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_i64()).collect())
        .unwrap_or_default()
}

fn parse_json_array_of_int_arrays(raw: &str) -> Vec<Vec<i64>> {
    let arr_str = match extract_first_json_array(raw) {
        Some(s) => s,
        None => return Vec::new(),
    };
    let v: serde_json::Value = serde_json::from_str(arr_str).unwrap_or(serde_json::Value::Null);
    v.as_array()
        .map(|a| {
            a.iter()
                .map(|inner| {
                    inner
                        .as_array()
                        .map(|b| b.iter().filter_map(|x| x.as_i64()).collect())
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default()
}

// ── Vector ops ────────────────────────────────────────────────────────

/// `vec_cosine(\@a, \@b)` — cosine similarity in `[-1, 1]`.
pub(crate) fn vec_cosine(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let a = floats_from(
        args.first()
            .ok_or_else(|| PerlError::runtime("vec_cosine: a required", line))?,
    );
    let b = floats_from(
        args.get(1)
            .ok_or_else(|| PerlError::runtime("vec_cosine: b required", line))?,
    );
    if a.len() != b.len() || a.is_empty() {
        return Err(PerlError::runtime(
            format!("vec_cosine: dim mismatch (a={}, b={})", a.len(), b.len()),
            line,
        ));
    }
    let mut dot = 0f64;
    let mut na = 0f64;
    let mut nb = 0f64;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return Ok(PerlValue::float(0.0));
    }
    Ok(PerlValue::float(dot / (na.sqrt() * nb.sqrt())))
}

/// `vec_search(\@query, \@candidates, top_k => N)` — returns arrayref
/// of `+{idx, score}` for the top-k cosine matches. Each candidate
/// is itself an arrayref of floats (typical embedding shape).
pub(crate) fn vec_search(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let q = floats_from(
        args.first()
            .ok_or_else(|| PerlError::runtime("vec_search: query required", line))?,
    );
    let cands_v = args
        .get(1)
        .ok_or_else(|| PerlError::runtime("vec_search: candidates required", line))?;
    let cands_arr = cands_v
        .as_array_ref()
        .map(|a| a.read().clone())
        .unwrap_or_else(|| cands_v.clone().to_list());
    let opts = parse_opts(&args[2.min(args.len())..]);
    let top_k = opt_int(&opts, "top_k", 10).max(1) as usize;

    let mut scored: Vec<(usize, f64)> = Vec::with_capacity(cands_arr.len());
    for (i, c) in cands_arr.iter().enumerate() {
        let v = floats_from(c);
        if v.len() != q.len() || v.is_empty() {
            continue;
        }
        let mut dot = 0f64;
        let mut nv = 0f64;
        let mut nq = 0f64;
        for j in 0..q.len() {
            dot += q[j] * v[j];
            nq += q[j] * q[j];
            nv += v[j] * v[j];
        }
        let denom = nq.sqrt() * nv.sqrt();
        let score = if denom > 0.0 { dot / denom } else { 0.0 };
        scored.push((i, score));
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);

    let out: Vec<PerlValue> = scored
        .into_iter()
        .map(|(i, s)| {
            let mut m = IndexMap::new();
            m.insert("idx".to_string(), PerlValue::integer(i as i64));
            m.insert("score".to_string(), PerlValue::float(s));
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m)))
        })
        .collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        out,
    ))))
}

/// `vec_topk(\@scores, $k)` — utility: return the indexes of the
/// k largest scalar scores.
pub(crate) fn vec_topk(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let scores = floats_from(
        args.first()
            .ok_or_else(|| PerlError::runtime("vec_topk: scores required", line))?,
    );
    let k = args.get(1).map(|v| v.to_int()).unwrap_or(10).max(1) as usize;
    let mut indexed: Vec<(usize, f64)> = scores.into_iter().enumerate().collect();
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    indexed.truncate(k);
    let out: Vec<PerlValue> = indexed
        .into_iter()
        .map(|(i, _)| PerlValue::integer(i as i64))
        .collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        out,
    ))))
}

fn floats_from(v: &PerlValue) -> Vec<f64> {
    if let Some(arr) = v.as_array_ref() {
        return arr.read().iter().map(|x| x.to_number()).collect();
    }
    v.clone().to_list().iter().map(|x| x.to_number()).collect()
}

// ── Cost estimate / budget / routing / history ───────────────────────

/// `ai_estimate($prompt, model => "...")` — pre-flight USD cost
/// estimate using the heuristic token count + the embedded price
/// table. Output side is assumed to be 25% of input by default;
/// override with `out_tokens => N`.
pub(crate) fn ai_estimate(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let prompt = args.first().map(|v| v.to_string()).unwrap_or_default();
    let opts = parse_opts(&args[1.min(args.len())..]);
    let model = opt_str(&opts, "model", &config().lock().model);
    let in_tokens = prompt.chars().count().div_ceil(4).max(1) as f64;
    let out_tokens = opts
        .get("out_tokens")
        .map(|v| v.to_number())
        .unwrap_or((in_tokens / 4.0).max(64.0));
    let (in_per_1k, out_per_1k) = price_per_1k_tokens(&model);
    let usd = in_tokens / 1000.0 * in_per_1k + out_tokens / 1000.0 * out_per_1k;
    let mut h = IndexMap::new();
    h.insert("usd".to_string(), PerlValue::float(usd));
    h.insert(
        "input_tokens".to_string(),
        PerlValue::integer(in_tokens as i64),
    );
    h.insert(
        "output_tokens".to_string(),
        PerlValue::integer(out_tokens as i64),
    );
    h.insert("model".to_string(), PerlValue::string(model));
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))))
}

static ROUTING: OnceLock<Mutex<IndexMap<String, String>>> = OnceLock::new();

fn routing() -> &'static Mutex<IndexMap<String, String>> {
    ROUTING.get_or_init(|| Mutex::new(IndexMap::new()))
}

/// `ai_routing_get("embed")` → provider name or undef.
pub(crate) fn ai_routing_get(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let op = args.first().map(|v| v.to_string()).unwrap_or_default();
    if op.is_empty() {
        let g = routing().lock();
        let mut m = IndexMap::new();
        for (k, v) in g.iter() {
            m.insert(k.clone(), PerlValue::string(v.clone()));
        }
        return Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m))));
    }
    Ok(routing()
        .lock()
        .get(&op)
        .cloned()
        .map(PerlValue::string)
        .unwrap_or(PerlValue::UNDEF))
}

/// `ai_routing_set("embed", "voyage")` — register a per-op provider
/// override. Currently advisory; embeddings honor it; the agent
/// loop uses the global default. Wires up in Phase 4.
pub(crate) fn ai_routing_set(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "ai_routing_set: usage: ai_routing_set(\"op\", \"provider\")",
            line,
        ));
    }
    let op = args[0].to_string();
    let provider = args[1].to_string();
    routing().lock().insert(op, provider);
    Ok(PerlValue::UNDEF)
}

#[derive(Clone, Debug)]
struct HistoryEntry {
    provider: String,
    model: String,
    prompt: String,
    response_chars: usize,
    input_tokens: u64,
    output_tokens: u64,
    usd: f64,
    cache_hit: bool,
    when_unix: i64,
}

static HISTORY: OnceLock<Mutex<std::collections::VecDeque<HistoryEntry>>> = OnceLock::new();
const HISTORY_CAP: usize = 100;

fn history_slot() -> &'static Mutex<std::collections::VecDeque<HistoryEntry>> {
    HISTORY.get_or_init(|| Mutex::new(std::collections::VecDeque::with_capacity(HISTORY_CAP)))
}

pub(crate) fn record_history(
    provider: &str,
    model: &str,
    prompt: &str,
    response_chars: usize,
    input_tokens: u64,
    output_tokens: u64,
    usd: f64,
    cache_hit: bool,
) {
    let when = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let mut g = history_slot().lock();
    if g.len() >= HISTORY_CAP {
        g.pop_front();
    }
    g.push_back(HistoryEntry {
        provider: provider.to_string(),
        model: model.to_string(),
        prompt: truncate(prompt, 200),
        response_chars,
        input_tokens,
        output_tokens,
        usd,
        cache_hit,
        when_unix: when,
    });
}

/// `ai_history()` → arrayref of last 100 calls, oldest first.
pub(crate) fn ai_history(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let g = history_slot().lock();
    let items: Vec<PerlValue> = g
        .iter()
        .map(|e| {
            let mut m = IndexMap::new();
            m.insert("provider".into(), PerlValue::string(e.provider.clone()));
            m.insert("model".into(), PerlValue::string(e.model.clone()));
            m.insert("prompt".into(), PerlValue::string(e.prompt.clone()));
            m.insert(
                "response_chars".into(),
                PerlValue::integer(e.response_chars as i64),
            );
            m.insert(
                "input_tokens".into(),
                PerlValue::integer(e.input_tokens as i64),
            );
            m.insert(
                "output_tokens".into(),
                PerlValue::integer(e.output_tokens as i64),
            );
            m.insert("usd".into(), PerlValue::float(e.usd));
            m.insert(
                "cache_hit".into(),
                PerlValue::integer(if e.cache_hit { 1 } else { 0 }),
            );
            m.insert("unix_time".into(), PerlValue::integer(e.when_unix));
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m)))
        })
        .collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        items,
    ))))
}

pub(crate) fn ai_history_clear(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    history_slot().lock().clear();
    Ok(PerlValue::UNDEF)
}

// ── Tool registry (Phase 1 sugar without `tool fn` parser keyword) ────
//
// `ai_register_tool(name, description, parameters, sub)` adds an
// always-on tool that the bare `ai($prompt)` agent loop will see.
// This sidesteps the parser extension while still letting users
// stand up agents incrementally.

#[derive(Clone)]
pub(crate) struct RegisteredTool {
    pub name: String,
    pub description: String,
    pub parameters: PerlValue,
    pub run_sub: Arc<crate::value::PerlSub>,
}

static REGISTERED: OnceLock<Mutex<Vec<RegisteredTool>>> = OnceLock::new();

pub(crate) fn registered_tools() -> &'static Mutex<Vec<RegisteredTool>> {
    REGISTERED.get_or_init(|| Mutex::new(Vec::new()))
}

pub(crate) fn ai_register_tool(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.len() < 4 {
        return Err(PerlError::runtime(
            "ai_register_tool: usage: ai_register_tool(\"name\", \"desc\", +{p=>\"type\"}, sub { ... })",
            line,
        ));
    }
    let name = args[0].to_string();
    let desc = args[1].to_string();
    let params = args[2].clone();
    let run_sub = args[3]
        .as_code_ref()
        .ok_or_else(|| PerlError::runtime("ai_register_tool: 4th arg must be a coderef", line))?;
    let mut g = registered_tools().lock();
    g.retain(|t| t.name != name); // idempotent re-register
    g.push(RegisteredTool {
        name,
        description: desc,
        parameters: params,
        run_sub,
    });
    Ok(PerlValue::UNDEF)
}

pub(crate) fn ai_unregister_tool(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let name = args.first().map(|v| v.to_string()).unwrap_or_default();
    let mut g = registered_tools().lock();
    let before = g.len();
    g.retain(|t| t.name != name);
    Ok(PerlValue::integer((before - g.len()) as i64))
}

pub(crate) fn ai_clear_tools(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    registered_tools().lock().clear();
    Ok(PerlValue::UNDEF)
}

pub(crate) fn ai_tools_list(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let g = registered_tools().lock();
    let items: Vec<PerlValue> = g
        .iter()
        .map(|t| {
            let mut m = IndexMap::new();
            m.insert("name".into(), PerlValue::string(t.name.clone()));
            m.insert(
                "description".into(),
                PerlValue::string(t.description.clone()),
            );
            m.insert("parameters".into(), t.parameters.clone());
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m)))
        })
        .collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        items,
    ))))
}

// ── Embedding memory / RAG (sqlite-backed) ────────────────────────────
//
// `ai_memory_save("doc-id", "text content")` embeds the text and stores
// it in a sqlite table. `ai_memory_recall("query", top_k => 3)` re-embeds
// the query and returns the best matches by cosine similarity. Storage
// is in-process for v0; pass `path => "memory.db"` to persist.

use rusqlite::Connection as SqliteConn;

struct MemoryStore {
    conn: SqliteConn,
    embed_dim: Option<usize>,
}

static MEMORY: OnceLock<Mutex<Option<MemoryStore>>> = OnceLock::new();

fn memory() -> &'static Mutex<Option<MemoryStore>> {
    MEMORY.get_or_init(|| Mutex::new(None))
}

fn ensure_memory(path: Option<&str>) -> Result<()> {
    let mut g = memory().lock();
    if g.is_some() {
        return Ok(());
    }
    let conn = match path {
        Some(p) => SqliteConn::open(p),
        None => SqliteConn::open_in_memory(),
    }
    .map_err(|e| PerlError::runtime(format!("ai_memory: open: {}", e), 0))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ai_memory (
            id        TEXT PRIMARY KEY,
            content   TEXT NOT NULL,
            embedding BLOB NOT NULL,
            metadata  TEXT,
            saved_at  TEXT
         );
         CREATE INDEX IF NOT EXISTS ai_memory_saved ON ai_memory(saved_at);",
    )
    .map_err(|e| PerlError::runtime(format!("ai_memory: schema: {}", e), 0))?;
    *g = Some(MemoryStore {
        conn,
        embed_dim: None,
    });
    Ok(())
}

fn vec_to_blob(vec: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vec.len() * 4);
    for f in vec {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

fn blob_to_vec(blob: &[u8]) -> Vec<f32> {
    let mut out = Vec::with_capacity(blob.len() / 4);
    let mut i = 0;
    while i + 4 <= blob.len() {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(&blob[i..i + 4]);
        out.push(f32::from_le_bytes(buf));
        i += 4;
    }
    out
}

fn embed_one(text: &str, line: usize) -> Result<Vec<f32>> {
    let v = ai_embed(&[PerlValue::string(text.to_string())], line)?;
    // Single-input embed returns a flat array_ref of floats.
    let arr = v
        .as_array_ref()
        .ok_or_else(|| PerlError::runtime("ai_memory: embed returned non-array", line))?;
    let read = arr.read();
    if read.is_empty() {
        return Err(PerlError::runtime("ai_memory: empty embedding", line));
    }
    // Detect collection-shaped response: arr-of-arrs.
    if read[0].as_array_ref().is_some() {
        // Flat single embedding inside a 1-element wrapper.
        let inner = read[0].as_array_ref().unwrap();
        let r = inner.read();
        return Ok(r.iter().map(|x| x.to_number() as f32).collect());
    }
    Ok(read.iter().map(|x| x.to_number() as f32).collect())
}

pub(crate) fn ai_memory_save(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    if args.len() < 2 {
        return Err(PerlError::runtime(
            "ai_memory_save: usage: ai_memory_save(\"id\", \"content\", metadata?, path?)",
            line,
        ));
    }
    let id = args[0].to_string();
    let content = args[1].to_string();
    let metadata = args
        .get(2)
        .map(|v| {
            if v.is_undef() {
                String::new()
            } else {
                v.to_string()
            }
        })
        .unwrap_or_default();
    let path = args.get(3).map(|v| v.to_string());
    ensure_memory(path.as_deref())?;

    // Mock-mode shortcut: when no real embed available, hash text into
    // a deterministic 8-dim vector so save/recall round-trips work in
    // tests without hitting the network.
    let embed_vec = if mock_embed_active() {
        mock_embedding(&content)
    } else {
        embed_one(&content, line)?
    };

    let blob = vec_to_blob(&embed_vec);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut g = memory().lock();
    let store = g.as_mut().expect("memory ensured above");
    if store.embed_dim.is_none() {
        store.embed_dim = Some(embed_vec.len());
    }
    store
        .conn
        .execute(
            "INSERT INTO ai_memory (id, content, embedding, metadata, saved_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
               content = excluded.content,
               embedding = excluded.embedding,
               metadata = excluded.metadata,
               saved_at = excluded.saved_at",
            rusqlite::params![id, content, blob, metadata, now.to_string()],
        )
        .map_err(|e| PerlError::runtime(format!("ai_memory: insert: {}", e), line))?;
    Ok(PerlValue::UNDEF)
}

pub(crate) fn ai_memory_recall(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let query = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_memory_recall: query required", line))?;
    let opts = parse_opts(&args[1.min(args.len())..]);
    let top_k = opt_int(&opts, "top_k", 5).max(1) as usize;
    let path = opts.get("path").map(|v| v.to_string());
    ensure_memory(path.as_deref())?;

    let q_vec = if mock_embed_active() {
        mock_embedding(&query)
    } else {
        embed_one(&query, line)?
    };

    let mut rows: Vec<(String, String, Vec<f32>, String)> = Vec::new();
    {
        let g = memory().lock();
        let store = g.as_ref().expect("memory ensured above");
        let mut stmt = store
            .conn
            .prepare("SELECT id, content, embedding, COALESCE(metadata, '') FROM ai_memory")
            .map_err(|e| PerlError::runtime(format!("ai_memory: prepare: {}", e), line))?;
        let iter = stmt
            .query_map([], |r| {
                let id: String = r.get(0)?;
                let content: String = r.get(1)?;
                let blob: Vec<u8> = r.get(2)?;
                let meta: String = r.get(3)?;
                Ok((id, content, blob_to_vec(&blob), meta))
            })
            .map_err(|e| PerlError::runtime(format!("ai_memory: query: {}", e), line))?;
        for row in iter.flatten() {
            rows.push(row);
        }
    }

    // Score by cosine similarity, take top-k.
    let mut scored: Vec<(usize, f64)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, (_, _, vec, _))| {
            if vec.len() != q_vec.len() || vec.is_empty() {
                return None;
            }
            let mut dot = 0f64;
            let mut nq = 0f64;
            let mut nv = 0f64;
            for j in 0..vec.len() {
                let a = q_vec[j] as f64;
                let b = vec[j] as f64;
                dot += a * b;
                nq += a * a;
                nv += b * b;
            }
            let denom = nq.sqrt() * nv.sqrt();
            Some((i, if denom > 0.0 { dot / denom } else { 0.0 }))
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);

    let out: Vec<PerlValue> = scored
        .into_iter()
        .map(|(i, score)| {
            let (id, content, _, meta) = &rows[i];
            let mut m = IndexMap::new();
            m.insert("id".into(), PerlValue::string(id.clone()));
            m.insert("content".into(), PerlValue::string(content.clone()));
            m.insert("score".into(), PerlValue::float(score));
            m.insert("metadata".into(), PerlValue::string(meta.clone()));
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m)))
        })
        .collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        out,
    ))))
}

pub(crate) fn ai_memory_forget(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let id = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_memory_forget: id required", line))?;
    ensure_memory(None)?;
    let g = memory().lock();
    let store = g.as_ref().expect("memory ensured above");
    let n = store
        .conn
        .execute("DELETE FROM ai_memory WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| PerlError::runtime(format!("ai_memory: delete: {}", e), line))?;
    Ok(PerlValue::integer(n as i64))
}

pub(crate) fn ai_memory_count(_args: &[PerlValue], line: usize) -> Result<PerlValue> {
    ensure_memory(None)?;
    let g = memory().lock();
    let store = g.as_ref().expect("memory ensured above");
    let n: i64 = store
        .conn
        .query_row("SELECT count(*) FROM ai_memory", [], |r| r.get(0))
        .map_err(|e| PerlError::runtime(format!("ai_memory: count: {}", e), line))?;
    Ok(PerlValue::integer(n))
}

pub(crate) fn ai_memory_clear(_args: &[PerlValue], line: usize) -> Result<PerlValue> {
    ensure_memory(None)?;
    let g = memory().lock();
    let store = g.as_ref().expect("memory ensured above");
    let n = store
        .conn
        .execute("DELETE FROM ai_memory", [])
        .map_err(|e| PerlError::runtime(format!("ai_memory: clear: {}", e), line))?;
    Ok(PerlValue::integer(n as i64))
}

// Mock-embed helper: deterministic 32-dim hash so tests work offline.
fn mock_embed_active() -> bool {
    mock_only_mode() || match_mock("embed:probe").is_some()
}

fn mock_embedding(text: &str) -> Vec<f32> {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    let bytes = h.finalize();
    let mut out: Vec<f32> = Vec::with_capacity(32);
    for chunk in bytes.chunks(8) {
        let mut acc: u64 = 0;
        for (i, b) in chunk.iter().enumerate() {
            acc |= (*b as u64) << (i * 8);
        }
        // Map u64 to [-1, 1] deterministically.
        let f = ((acc as f64) / (u64::MAX as f64)) * 2.0 - 1.0;
        out.push(f as f32);
    }
    // L2-normalize so cosine similarity ranges nicely.
    let mag: f32 = out.iter().map(|x| x * x).sum::<f32>().sqrt();
    if mag > 0.0 {
        for x in out.iter_mut() {
            *x /= mag;
        }
    }
    out
}

// ── Streaming with on_chunk callback ─────────────────────────────────
//
// Real SSE parsing for Anthropic. The user passes
// `stream_prompt($p, on_chunk => sub { print $_[0] })` and gets each
// text chunk as the model produces it. The full text is also returned
// at the end so existing buffered call sites still work.

impl Interpreter {
    pub(crate) fn ai_stream_with_callback(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> Result<PerlValue> {
        let prompt = args
            .first()
            .map(|v| v.to_string())
            .ok_or_else(|| PerlError::runtime("stream_prompt: prompt required", line))?;
        let opts = parse_opts(&args[1..]);
        let on_chunk = opts.get("on_chunk").cloned();
        let on_chunk_sub = on_chunk.as_ref().and_then(|v| v.as_code_ref());

        // No callback → buffered semantics, same as ai_prompt.
        if on_chunk_sub.is_none() {
            return ai_prompt(args, line);
        }

        // Mock-mode short-circuits streaming too: chunk the mocked
        // response by char so callbacks still fire deterministically.
        if let Some(resp) = match_mock(&prompt) {
            for c in resp.chars() {
                let arg = PerlValue::string(c.to_string());
                let _ = self.call_sub(
                    on_chunk_sub.as_ref().unwrap(),
                    vec![arg],
                    WantarrayCtx::Scalar,
                    line,
                );
            }
            return Ok(PerlValue::string(resp));
        }
        if mock_only_mode() {
            return Err(PerlError::runtime(
                "stream_prompt: STRYKE_AI_MODE=mock-only and no mock matched prompt",
                line,
            ));
        }

        let provider = opt_str(&opts, "provider", &config().lock().provider);
        if provider == "openai" {
            return self.ai_stream_openai(&prompt, opts, on_chunk_sub.as_ref().unwrap(), line);
        }
        if provider != "anthropic" {
            return ai_prompt(args, line);
        }
        let model = opt_str(&opts, "model", &config().lock().model);
        let system = opt_str(&opts, "system", "");
        let max_tokens = opt_int(&opts, "max_tokens", 1024);
        let temperature = opt_float(&opts, "temperature", -1.0);
        let timeout = opt_int(&opts, "timeout", 120);

        let key_env = config().lock().api_key_env.clone();
        let api_key = std::env::var(&key_env).map_err(|_| {
            PerlError::runtime(format!("stream_prompt: ${} env var not set", key_env), line)
        })?;
        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": [{ "role": "user", "content": prompt }],
            "stream": true,
        });
        if !system.is_empty() {
            body["system"] = serde_json::Value::String(system);
        }
        if temperature >= 0.0 {
            body["temperature"] = serde_json::Value::from(temperature);
        }

        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(timeout.max(1) as u64))
            .build();
        let resp = agent
            .post("https://api.anthropic.com/v1/messages")
            .set("x-api-key", &api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .set("accept", "text/event-stream")
            .send_json(body)
            .map_err(|e| PerlError::runtime(format!("stream_prompt: anthropic: {}", e), line))?;

        let reader = std::io::BufReader::new(resp.into_reader());
        let mut full = String::new();
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;

        use std::io::BufRead;
        for line_io in reader.lines() {
            let raw = match line_io {
                Ok(l) => l,
                Err(_) => break,
            };
            // SSE: blank line terminates an event; we only care about
            // `data: ...` lines.
            let Some(payload) = raw.strip_prefix("data: ") else {
                continue;
            };
            if payload == "[DONE]" {
                break;
            }
            let v: serde_json::Value = match serde_json::from_str(payload) {
                Ok(v) => v,
                Err(_) => continue,
            };
            match v["type"].as_str() {
                Some("content_block_delta") => {
                    if let Some(t) = v["delta"]["text"].as_str() {
                        full.push_str(t);
                        let _ = self.call_sub(
                            on_chunk_sub.as_ref().unwrap(),
                            vec![PerlValue::string(t.to_string())],
                            WantarrayCtx::Scalar,
                            line,
                        );
                    }
                }
                Some("message_start") => {
                    if let Some(n) = v["message"]["usage"]["input_tokens"].as_u64() {
                        input_tokens = n;
                    }
                }
                Some("message_delta") => {
                    if let Some(n) = v["usage"]["output_tokens"].as_u64() {
                        output_tokens = n;
                    }
                }
                _ => {}
            }
        }

        INPUT_TOKENS.fetch_add(input_tokens, Ordering::Relaxed);
        OUTPUT_TOKENS.fetch_add(output_tokens, Ordering::Relaxed);
        let (in_per_1k, out_per_1k) = price_per_1k_tokens(&model);
        add_cost(
            input_tokens as f64 / 1000.0 * in_per_1k + output_tokens as f64 / 1000.0 * out_per_1k,
        );
        record_history(
            &provider,
            &model,
            &prompt,
            full.chars().count(),
            input_tokens,
            output_tokens,
            current_cost_usd(),
            false,
        );
        Ok(PerlValue::string(full))
    }
}

impl Interpreter {
    fn ai_stream_openai(
        &mut self,
        prompt: &str,
        opts: IndexMap<String, PerlValue>,
        on_chunk_sub: &Arc<crate::value::PerlSub>,
        line: usize,
    ) -> Result<PerlValue> {
        let model = opt_str(&opts, "model", &config().lock().model);
        let system = opt_str(&opts, "system", "");
        let max_tokens = opt_int(&opts, "max_tokens", 1024);
        let temperature = opt_float(&opts, "temperature", -1.0);
        let timeout = opt_int(&opts, "timeout", 120);

        let api_key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| PerlError::runtime("stream_prompt: $OPENAI_API_KEY not set", line))?;
        let mut messages: Vec<serde_json::Value> = Vec::new();
        if !system.is_empty() {
            messages.push(serde_json::json!({"role":"system","content":system}));
        }
        messages.push(serde_json::json!({"role":"user","content":prompt}));
        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if temperature >= 0.0 {
            body["temperature"] = serde_json::Value::from(temperature);
        }
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(timeout.max(1) as u64))
            .build();
        let resp = agent
            .post("https://api.openai.com/v1/chat/completions")
            .set("authorization", &format!("Bearer {}", api_key))
            .set("content-type", "application/json")
            .set("accept", "text/event-stream")
            .send_json(body)
            .map_err(|e| PerlError::runtime(format!("stream_prompt: openai: {}", e), line))?;

        let reader = std::io::BufReader::new(resp.into_reader());
        let mut full = String::new();
        let mut input_tokens = 0u64;
        let mut output_tokens = 0u64;

        use std::io::BufRead;
        for line_io in reader.lines() {
            let raw = match line_io {
                Ok(l) => l,
                Err(_) => break,
            };
            let Some(payload) = raw.strip_prefix("data: ") else {
                continue;
            };
            if payload == "[DONE]" {
                break;
            }
            let v: serde_json::Value = match serde_json::from_str(payload) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(usage) = v.get("usage") {
                if let Some(n) = usage["prompt_tokens"].as_u64() {
                    input_tokens = n;
                }
                if let Some(n) = usage["completion_tokens"].as_u64() {
                    output_tokens = n;
                }
            }
            if let Some(choices) = v["choices"].as_array() {
                for ch in choices {
                    if let Some(delta) = ch["delta"]["content"].as_str() {
                        if !delta.is_empty() {
                            full.push_str(delta);
                            let _ = self.call_sub(
                                on_chunk_sub,
                                vec![PerlValue::string(delta.to_string())],
                                WantarrayCtx::Scalar,
                                line,
                            );
                        }
                    }
                }
            }
        }

        INPUT_TOKENS.fetch_add(input_tokens, Ordering::Relaxed);
        OUTPUT_TOKENS.fetch_add(output_tokens, Ordering::Relaxed);
        let (in_per_1k, out_per_1k) = price_per_1k_tokens(&model);
        add_cost(
            input_tokens as f64 / 1000.0 * in_per_1k + output_tokens as f64 / 1000.0 * out_per_1k,
        );
        record_history(
            "openai",
            &model,
            prompt,
            full.chars().count(),
            input_tokens,
            output_tokens,
            current_cost_usd(),
            false,
        );
        Ok(PerlValue::string(full))
    }
}

// ── Multimodal: image input ──────────────────────────────────────────
//
// `ai($prompt, image => $bytes_or_url)` builds an Anthropic vision
// content array. Bytes get base64-encoded inline; URLs get fetched
// (Anthropic doesn't support `url` source directly; URLs are fetched
// here and inlined as bytes).

pub(crate) fn ai_vision(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let prompt = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_vision: prompt required", line))?;
    let opts = parse_opts(&args[1..]);
    let image_v = opts.get("image").cloned().ok_or_else(|| {
        PerlError::runtime("ai_vision: pass image => $bytes | $url | $path", line)
    })?;
    let provider = opt_str(&opts, "provider", &config().lock().provider);
    if provider != "anthropic" {
        return Err(PerlError::runtime(
            format!(
                "ai_vision: provider `{}` not implemented (Anthropic only for v0)",
                provider
            ),
            line,
        ));
    }
    let model = opt_str(&opts, "model", &config().lock().model);
    let system = opt_str(&opts, "system", "");
    let max_tokens = opt_int(&opts, "max_tokens", 1024);
    let timeout = opt_int(&opts, "timeout", 60);

    if let Some(resp) = match_mock(&prompt) {
        return Ok(PerlValue::string(resp));
    }
    if mock_only_mode() {
        return Err(PerlError::runtime(
            "ai_vision: STRYKE_AI_MODE=mock-only and no mock matched",
            line,
        ));
    }

    let (b64, media_type) = resolve_image_input(&image_v, line)?;

    let key_env = config().lock().api_key_env.clone();
    let api_key = std::env::var(&key_env).map_err(|_| {
        PerlError::runtime(format!("ai_vision: ${} env var not set", key_env), line)
    })?;
    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": [{
            "role": "user",
            "content": [
                {
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": media_type,
                        "data": b64,
                    }
                },
                { "type": "text", "text": prompt }
            ]
        }],
    });
    if !system.is_empty() {
        body["system"] = serde_json::Value::String(system);
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post("https://api.anthropic.com/v1/messages")
        .set("x-api-key", &api_key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("ai_vision: anthropic: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_vision: decode: {}", e), line))?;

    if let Some(input) = json["usage"]["input_tokens"].as_u64() {
        INPUT_TOKENS.fetch_add(input, Ordering::Relaxed);
        let (in_per_1k, _) = price_per_1k_tokens(&model);
        add_cost(input as f64 / 1000.0 * in_per_1k);
    }
    if let Some(output) = json["usage"]["output_tokens"].as_u64() {
        OUTPUT_TOKENS.fetch_add(output, Ordering::Relaxed);
        let (_, out_per_1k) = price_per_1k_tokens(&model);
        add_cost(output as f64 / 1000.0 * out_per_1k);
    }

    let mut out = String::new();
    if let Some(arr) = json["content"].as_array() {
        for chunk in arr {
            if chunk["type"] == "text" {
                if let Some(t) = chunk["text"].as_str() {
                    out.push_str(t);
                }
            }
        }
    }
    Ok(PerlValue::string(out))
}

fn resolve_image_input(v: &PerlValue, line: usize) -> Result<(String, String)> {
    use base64::Engine;
    let s = v.to_string();
    let (bytes, media_type) = if s.starts_with("http://") || s.starts_with("https://") {
        // Fetch and inline.
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(30))
            .build();
        let resp = agent
            .get(&s)
            .call()
            .map_err(|e| PerlError::runtime(format!("ai_vision: fetch image: {}", e), line))?;
        let ct = resp
            .header("content-type")
            .unwrap_or("image/jpeg")
            .to_string();
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut resp.into_reader(), &mut buf)
            .map_err(|e| PerlError::runtime(format!("ai_vision: read image: {}", e), line))?;
        (
            buf,
            ct.split(';')
                .next()
                .unwrap_or("image/jpeg")
                .trim()
                .to_string(),
        )
    } else if std::path::Path::new(&s).exists() {
        let bytes = std::fs::read(&s)
            .map_err(|e| PerlError::runtime(format!("ai_vision: read {}: {}", s, e), line))?;
        let ct = guess_media_type(&s);
        (bytes, ct)
    } else if let Some(arc) = v.as_bytes_arc() {
        // Raw bytes from stryke.
        ((*arc).clone(), "image/jpeg".to_string())
    } else {
        return Err(PerlError::runtime(
            "ai_vision: image must be a URL, file path, or raw bytes",
            line,
        ));
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok((b64, media_type))
}

// ── Ollama (native generate API) ──────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn call_ollama(
    prompt: &str,
    system: &str,
    model: &str,
    max_tokens: i64,
    temperature: f64,
    timeout: i64,
    base: &str,
    line: usize,
) -> Result<String> {
    let url = format!("{}/api/generate", base.trim_end_matches('/'));
    let mut body = serde_json::json!({
        "model": if model.is_empty() || model.starts_with("claude") || model.starts_with("gpt") {
            // Ollama needs an Ollama-tagged model name; if the user didn't
            // override, default to a sensible small one.
            "llama3.2"
        } else {
            model
        },
        "prompt": prompt,
        "stream": false,
        "options": {
            "num_predict": max_tokens,
        },
    });
    if !system.is_empty() {
        body["system"] = serde_json::Value::String(system.to_string());
    }
    if temperature >= 0.0 {
        body["options"]["temperature"] = serde_json::Value::from(temperature);
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post(&url)
        .set("content-type", "application/json")
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("ai: ollama request: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai: ollama decode: {}", e), line))?;
    // Track tokens — Ollama returns prompt_eval_count + eval_count.
    let input = json["prompt_eval_count"].as_u64().unwrap_or(0);
    let output = json["eval_count"].as_u64().unwrap_or(0);
    INPUT_TOKENS.fetch_add(input, Ordering::Relaxed);
    OUTPUT_TOKENS.fetch_add(output, Ordering::Relaxed);
    // Local model = $0 — no add_cost call.
    let response = json["response"].as_str().unwrap_or("").to_string();
    if response.is_empty() {
        return Err(PerlError::runtime(
            format!(
                "ai: ollama returned empty response (raw: {})",
                truncate(&json.to_string(), 200)
            ),
            line,
        ));
    }
    Ok(response)
}

// ── Gemini (Google AI Studio) ────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn call_gemini(
    prompt: &str,
    system: &str,
    model: &str,
    max_tokens: i64,
    temperature: f64,
    timeout: i64,
    line: usize,
) -> Result<String> {
    let api_key = std::env::var("GOOGLE_API_KEY")
        .or_else(|_| std::env::var("GEMINI_API_KEY"))
        .map_err(|_| {
            PerlError::runtime(
                "ai: $GOOGLE_API_KEY (or $GEMINI_API_KEY) not set for gemini provider",
                line,
            )
        })?;
    // Model defaults: if user has Anthropic/OpenAI default still set,
    // map to Gemini's flagship.
    let model = if model.starts_with("claude") || model.starts_with("gpt") {
        "gemini-2.5-flash".to_string()
    } else {
        model.to_string()
    };
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
        model, api_key
    );
    let mut config = serde_json::json!({
        "maxOutputTokens": max_tokens,
    });
    if temperature >= 0.0 {
        config["temperature"] = serde_json::Value::from(temperature);
    }
    let mut body = serde_json::json!({
        "contents": [{ "parts": [{ "text": prompt }] }],
        "generationConfig": config,
    });
    if !system.is_empty() {
        body["systemInstruction"] = serde_json::json!({
            "parts": [{ "text": system }]
        });
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post(&url)
        .set("content-type", "application/json")
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("ai: gemini request: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai: gemini decode: {}", e), line))?;
    let usage = &json["usageMetadata"];
    let input = usage["promptTokenCount"].as_u64().unwrap_or(0);
    let output = usage["candidatesTokenCount"].as_u64().unwrap_or(0);
    INPUT_TOKENS.fetch_add(input, Ordering::Relaxed);
    OUTPUT_TOKENS.fetch_add(output, Ordering::Relaxed);
    let (in_per_1k, out_per_1k) = price_per_1k_tokens(&model);
    add_cost(input as f64 / 1000.0 * in_per_1k + output as f64 / 1000.0 * out_per_1k);

    let mut text = String::new();
    if let Some(cands) = json["candidates"].as_array() {
        if let Some(parts) = cands.first().and_then(|c| c["content"]["parts"].as_array()) {
            for p in parts {
                if let Some(t) = p["text"].as_str() {
                    text.push_str(t);
                }
            }
        }
    }
    if text.is_empty() {
        return Err(PerlError::runtime(
            format!(
                "ai: gemini returned no text (raw: {})",
                truncate(&json.to_string(), 200)
            ),
            line,
        ));
    }
    Ok(text)
}

// ── Whisper transcription (OpenAI Audio) ──────────────────────────────

/// `ai_transcribe($audio_path, model => "whisper-1", language => "en")`
/// — speech-to-text via OpenAI's audio transcription endpoint. Returns
/// a string. Accepts mp3/mp4/mpeg/mpga/m4a/wav/webm.
pub(crate) fn ai_transcribe(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let path = args.first().map(|v| v.to_string()).ok_or_else(|| {
        PerlError::runtime(
            "ai_transcribe: usage: ai_transcribe(\"path/to/audio.mp3\")",
            line,
        )
    })?;
    let opts = parse_opts(&args[1..]);
    let model = opt_str(&opts, "model", "whisper-1");
    let language = opt_str(&opts, "language", "");
    let timeout = opt_int(&opts, "timeout", 120);

    if let Some(resp) = match_mock(&format!("transcribe:{}", path)) {
        return Ok(PerlValue::string(resp));
    }
    if mock_only_mode() {
        return Err(PerlError::runtime(
            "ai_transcribe: STRYKE_AI_MODE=mock-only and no transcribe mock installed",
            line,
        ));
    }
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| PerlError::runtime("ai_transcribe: $OPENAI_API_KEY not set", line))?;
    let bytes = std::fs::read(&path)
        .map_err(|e| PerlError::runtime(format!("ai_transcribe: read {}: {}", path, e), line))?;

    let body = build_multipart(&[
        ("model", model.as_bytes(), None),
        (
            "language",
            language.as_bytes(),
            if language.is_empty() {
                None
            } else {
                Some("text/plain")
            },
        ),
        (
            "file",
            &bytes,
            Some(&format!(
                "{}:application/octet-stream",
                std::path::Path::new(&path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("audio.bin")
            )),
        ),
    ]);
    let boundary = "stryke_form_boundary_3f7a";
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post("https://api.openai.com/v1/audio/transcriptions")
        .set("authorization", &format!("Bearer {}", api_key))
        .set(
            "content-type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&body)
        .map_err(|e| PerlError::runtime(format!("ai_transcribe: request: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_transcribe: decode: {}", e), line))?;
    let text = json["text"].as_str().unwrap_or("").to_string();
    Ok(PerlValue::string(text))
}

// ── TTS (OpenAI Audio Speech) ────────────────────────────────────────

/// `ai_speak($text, voice => "alloy", model => "tts-1", output => "out.mp3")`
/// — text-to-speech via OpenAI. Returns audio bytes (and optionally
/// writes to `output` path).
pub(crate) fn ai_speak(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let text = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_speak: text required", line))?;
    let opts = parse_opts(&args[1..]);
    let model = opt_str(&opts, "model", "tts-1");
    let voice = opt_str(&opts, "voice", "alloy");
    let format = opt_str(&opts, "format", "mp3");
    let output = opt_str(&opts, "output", "");
    let timeout = opt_int(&opts, "timeout", 60);

    if mock_only_mode() {
        let fake = b"MOCK_TTS_AUDIO".to_vec();
        if !output.is_empty() {
            std::fs::write(&output, &fake).ok();
        }
        return Ok(PerlValue::bytes(Arc::new(fake)));
    }
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| PerlError::runtime("ai_speak: $OPENAI_API_KEY not set", line))?;
    let body = serde_json::json!({
        "model": model,
        "voice": voice,
        "input": text,
        "response_format": format,
    });
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post("https://api.openai.com/v1/audio/speech")
        .set("authorization", &format!("Bearer {}", api_key))
        .set("content-type", "application/json")
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("ai_speak: request: {}", e), line))?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut resp.into_reader(), &mut buf)
        .map_err(|e| PerlError::runtime(format!("ai_speak: read body: {}", e), line))?;
    if !output.is_empty() {
        std::fs::write(&output, &buf)
            .map_err(|e| PerlError::runtime(format!("ai_speak: write {}: {}", output, e), line))?;
    }
    Ok(PerlValue::bytes(Arc::new(buf)))
}

// ── Image generation (OpenAI Images) ─────────────────────────────────
//
// `ai_image($prompt, model => "gpt-image-1", size => "1024x1024",
//           quality => "high", output => "out.png")` generates an image and
// returns the raw bytes (PNG by default). Pass `n => N` for multiple images
// — returns an arrayref of byte buffers.
//
// For OpenAI, both `gpt-image-1` and `dall-e-3` use the same endpoint with
// different parameter sets. We default to `dall-e-3` because it doesn't
// require organization verification.
pub(crate) fn ai_image(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let prompt = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_image: prompt required", line))?;
    let opts = parse_opts(&args[1..]);
    let model = opt_str(&opts, "model", "dall-e-3");
    let size = opt_str(&opts, "size", "1024x1024");
    let quality = opt_str(&opts, "quality", "standard");
    let output = opt_str(&opts, "output", "");
    let n = opt_int(&opts, "n", 1).max(1);
    let style = opt_str(&opts, "style", "");
    let timeout = opt_int(&opts, "timeout", 120);

    if let Some(resp) = match_mock(&format!("image:{}", prompt)) {
        let bytes = resp.into_bytes();
        if !output.is_empty() {
            std::fs::write(&output, &bytes).ok();
        }
        return Ok(PerlValue::bytes(Arc::new(bytes)));
    }
    if mock_only_mode() {
        return Err(PerlError::runtime(
            "ai_image: STRYKE_AI_MODE=mock-only and no image mock installed",
            line,
        ));
    }
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| PerlError::runtime("ai_image: $OPENAI_API_KEY not set", line))?;

    let mut body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "n": n,
        "size": size,
        "response_format": "b64_json",
    });
    // dall-e-3 supports `quality` and `style`; gpt-image-1 has its own param surface.
    if model.starts_with("dall-e-3") {
        body["quality"] = serde_json::json!(quality);
        if !style.is_empty() {
            body["style"] = serde_json::json!(style);
        }
    } else if model.starts_with("gpt-image") {
        // gpt-image-1 uses `quality: "high"|"medium"|"low"`; pass through if set.
        if !quality.is_empty() && quality != "standard" {
            body["quality"] = serde_json::json!(quality);
        }
        // gpt-image-1 returns b64 by default; response_format is rejected.
        body.as_object_mut()
            .and_then(|m| m.remove("response_format"));
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post("https://api.openai.com/v1/images/generations")
        .set("authorization", &format!("Bearer {}", api_key))
        .set("content-type", "application/json")
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("ai_image: request: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_image: decode: {}", e), line))?;
    let data = json["data"].as_array().cloned().unwrap_or_default();
    if data.is_empty() {
        return Err(PerlError::runtime(
            format!("ai_image: empty response: {}", json),
            line,
        ));
    }

    let mut images: Vec<Vec<u8>> = Vec::new();
    for item in &data {
        if let Some(b64) = item["b64_json"].as_str() {
            let bytes = base64_decode_lenient(b64)
                .ok_or_else(|| PerlError::runtime("ai_image: invalid base64 in response", line))?;
            images.push(bytes);
        } else if let Some(url) = item["url"].as_str() {
            let r = agent
                .get(url)
                .call()
                .map_err(|e| PerlError::runtime(format!("ai_image: download: {}", e), line))?;
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut r.into_reader(), &mut buf)
                .map_err(|e| PerlError::runtime(format!("ai_image: read download: {}", e), line))?;
            images.push(buf);
        }
    }

    if images.len() == 1 {
        if !output.is_empty() {
            std::fs::write(&output, &images[0]).map_err(|e| {
                PerlError::runtime(format!("ai_image: write {}: {}", output, e), line)
            })?;
        }
        Ok(PerlValue::bytes(Arc::new(images.remove(0))))
    } else {
        if !output.is_empty() {
            for (i, b) in images.iter().enumerate() {
                let p = if let Some(dot) = output.rfind('.') {
                    format!("{}_{}{}", &output[..dot], i + 1, &output[dot..])
                } else {
                    format!("{}_{}", output, i + 1)
                };
                std::fs::write(&p, b).map_err(|e| {
                    PerlError::runtime(format!("ai_image: write {}: {}", p, e), line)
                })?;
            }
        }
        let arr: Vec<PerlValue> = images
            .into_iter()
            .map(|b| PerlValue::bytes(Arc::new(b)))
            .collect();
        Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
            arr,
        ))))
    }
}

// ── Image editing / variations (OpenAI Images) ───────────────────────
//
// `ai_image_edit($prompt, image => "in.png", mask => "mask.png", output => "out.png")`
// — edit an existing image given a prompt + optional mask. Uses
// OpenAI `/v1/images/edits` with multipart upload. The mask must be a
// PNG with transparent regions marking the area to edit.
pub(crate) fn ai_image_edit(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let prompt = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_image_edit: prompt required", line))?;
    let opts = parse_opts(&args[1..]);
    let image_path = opt_str(&opts, "image", "");
    if image_path.is_empty() {
        return Err(PerlError::runtime(
            "ai_image_edit: pass image => \"path/to/source.png\"",
            line,
        ));
    }
    let mask_path = opt_str(&opts, "mask", "");
    let model = opt_str(&opts, "model", "gpt-image-1");
    let size = opt_str(&opts, "size", "1024x1024");
    let n = opt_int(&opts, "n", 1).max(1);
    let output = opt_str(&opts, "output", "");
    let timeout = opt_int(&opts, "timeout", 180);

    if let Some(resp) = match_mock(&format!("image_edit:{}", prompt)) {
        let bytes = resp.into_bytes();
        if !output.is_empty() {
            std::fs::write(&output, &bytes).ok();
        }
        return Ok(PerlValue::bytes(Arc::new(bytes)));
    }
    if mock_only_mode() {
        return Err(PerlError::runtime(
            "ai_image_edit: STRYKE_AI_MODE=mock-only and no image_edit mock installed",
            line,
        ));
    }
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| PerlError::runtime("ai_image_edit: $OPENAI_API_KEY not set", line))?;
    let image_bytes = std::fs::read(&image_path).map_err(|e| {
        PerlError::runtime(format!("ai_image_edit: read {}: {}", image_path, e), line)
    })?;
    let image_filename = std::path::Path::new(&image_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image.png");

    let n_str = n.to_string();
    let mut fields: Vec<(&str, &[u8], Option<String>)> = vec![
        ("prompt", prompt.as_bytes(), None),
        ("model", model.as_bytes(), None),
        ("size", size.as_bytes(), None),
        ("n", n_str.as_bytes(), None),
        (
            "image",
            &image_bytes,
            Some(format!("{}:image/png", image_filename)),
        ),
    ];
    let mask_bytes;
    if !mask_path.is_empty() {
        mask_bytes = std::fs::read(&mask_path).map_err(|e| {
            PerlError::runtime(
                format!("ai_image_edit: read mask {}: {}", mask_path, e),
                line,
            )
        })?;
        let mask_filename = std::path::Path::new(&mask_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("mask.png")
            .to_string();
        fields.push((
            "mask",
            &mask_bytes,
            Some(format!("{}:image/png", mask_filename)),
        ));
    }
    let owned_fields = fields;
    let owned_refs: Vec<(&str, &[u8], Option<&str>)> = owned_fields
        .iter()
        .map(|(n, b, ct)| (*n, *b, ct.as_deref()))
        .collect();
    let body = build_multipart(&owned_refs);
    let boundary = "stryke_form_boundary_3f7a";

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post("https://api.openai.com/v1/images/edits")
        .set("authorization", &format!("Bearer {}", api_key))
        .set(
            "content-type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&body)
        .map_err(|e| PerlError::runtime(format!("ai_image_edit: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_image_edit: decode: {}", e), line))?;

    finalize_image_response(&json, &output, line)
}

/// `ai_image_variation(image => "in.png", n => 4, size => "1024x1024", output => "out.png")`
/// — generate variations of an existing image. OpenAI `/v1/images/variations`
/// (DALL-E 2 only — gpt-image-1 doesn't expose a variations endpoint).
pub(crate) fn ai_image_variation(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let opts = parse_opts(args);
    let image_path = opt_str(&opts, "image", "");
    if image_path.is_empty() {
        return Err(PerlError::runtime(
            "ai_image_variation: pass image => \"path/to/source.png\"",
            line,
        ));
    }
    let model = opt_str(&opts, "model", "dall-e-2");
    let size = opt_str(&opts, "size", "1024x1024");
    let n = opt_int(&opts, "n", 1).max(1);
    let output = opt_str(&opts, "output", "");
    let timeout = opt_int(&opts, "timeout", 180);

    if mock_only_mode() {
        let fake = b"MOCK_IMG_VAR".to_vec();
        return Ok(PerlValue::bytes(Arc::new(fake)));
    }
    let api_key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| PerlError::runtime("ai_image_variation: $OPENAI_API_KEY not set", line))?;
    let image_bytes = std::fs::read(&image_path).map_err(|e| {
        PerlError::runtime(
            format!("ai_image_variation: read {}: {}", image_path, e),
            line,
        )
    })?;
    let filename = std::path::Path::new(&image_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("image.png");
    let n_str = n.to_string();
    let body = build_multipart(&[
        ("model", model.as_bytes(), None),
        ("size", size.as_bytes(), None),
        ("n", n_str.as_bytes(), None),
        ("response_format", b"b64_json", None),
        (
            "image",
            &image_bytes,
            Some(&format!("{}:image/png", filename)),
        ),
    ]);
    let boundary = "stryke_form_boundary_3f7a";
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post("https://api.openai.com/v1/images/variations")
        .set("authorization", &format!("Bearer {}", api_key))
        .set(
            "content-type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&body)
        .map_err(|e| PerlError::runtime(format!("ai_image_variation: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_image_variation: decode: {}", e), line))?;

    finalize_image_response(&json, &output, line)
}

/// Shared response handler for image generation/edit/variation. Returns
/// PerlValue::bytes for n=1 or arrayref of bytes for n>1, optionally
/// writing to `output` path (with `_1`/`_2`/... suffix when n>1).
fn finalize_image_response(
    json: &serde_json::Value,
    output: &str,
    line: usize,
) -> Result<PerlValue> {
    let data = json["data"].as_array().cloned().unwrap_or_default();
    if data.is_empty() {
        return Err(PerlError::runtime(
            format!("image: empty response: {}", json),
            line,
        ));
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();
    let mut images: Vec<Vec<u8>> = Vec::new();
    for item in &data {
        if let Some(b64) = item["b64_json"].as_str() {
            let bytes = base64_decode_lenient(b64)
                .ok_or_else(|| PerlError::runtime("image: invalid base64", line))?;
            images.push(bytes);
        } else if let Some(url) = item["url"].as_str() {
            let r = agent
                .get(url)
                .call()
                .map_err(|e| PerlError::runtime(format!("image: download: {}", e), line))?;
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut r.into_reader(), &mut buf)
                .map_err(|e| PerlError::runtime(format!("image: read download: {}", e), line))?;
            images.push(buf);
        }
    }
    if images.len() == 1 {
        if !output.is_empty() {
            std::fs::write(output, &images[0])
                .map_err(|e| PerlError::runtime(format!("image: write {}: {}", output, e), line))?;
        }
        Ok(PerlValue::bytes(Arc::new(images.remove(0))))
    } else {
        if !output.is_empty() {
            for (i, b) in images.iter().enumerate() {
                let p = if let Some(dot) = output.rfind('.') {
                    format!("{}_{}{}", &output[..dot], i + 1, &output[dot..])
                } else {
                    format!("{}_{}", output, i + 1)
                };
                std::fs::write(&p, b)
                    .map_err(|e| PerlError::runtime(format!("image: write {}: {}", p, e), line))?;
            }
        }
        let arr: Vec<PerlValue> = images
            .into_iter()
            .map(|b| PerlValue::bytes(Arc::new(b)))
            .collect();
        Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
            arr,
        ))))
    }
}

/// Lenient base64 decoder — handles standard `A-Za-z0-9+/=` plus url-safe `-_`.
/// We don't pull a base64 crate just for this — image responses are large but
/// the table is tiny and well-defined.
fn base64_decode_lenient(s: &str) -> Option<Vec<u8>> {
    let mut buf: u32 = 0;
    let mut bits: u8 = 0;
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for ch in s.bytes() {
        let v = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' | b'\t' => continue,
            _ => return None,
        };
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

// ── Model listing ─────────────────────────────────────────────────────
//
// `ai_models($provider)` returns an arrayref of model IDs available from
// the given provider. Useful for autocompletion in tools and UIs that
// want to surface the live model catalog.
pub(crate) fn ai_models(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let provider = args
        .first()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "openai".into());
    let opts = parse_opts(&args[1..]);
    let timeout = opt_int(&opts, "timeout", 30);

    if mock_only_mode() {
        let mock = vec![
            PerlValue::string("mock-model-1".to_string()),
            PerlValue::string("mock-model-2".to_string()),
        ];
        return Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
            mock,
        ))));
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let ids: Vec<String> = match provider.as_str() {
        "openai" => {
            let key = std::env::var("OPENAI_API_KEY")
                .map_err(|_| PerlError::runtime("ai_models: $OPENAI_API_KEY not set", line))?;
            let resp = agent
                .get("https://api.openai.com/v1/models")
                .set("authorization", &format!("Bearer {}", key))
                .call()
                .map_err(|e| PerlError::runtime(format!("ai_models: openai: {}", e), line))?;
            let json: serde_json::Value = resp.into_json().map_err(|e| {
                PerlError::runtime(format!("ai_models: openai decode: {}", e), line)
            })?;
            json["data"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|o| o["id"].as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default()
        }
        "anthropic" | "claude" => {
            let key = std::env::var("ANTHROPIC_API_KEY")
                .map_err(|_| PerlError::runtime("ai_models: $ANTHROPIC_API_KEY not set", line))?;
            let resp = agent
                .get("https://api.anthropic.com/v1/models")
                .set("x-api-key", &key)
                .set("anthropic-version", "2023-06-01")
                .call()
                .map_err(|e| PerlError::runtime(format!("ai_models: anthropic: {}", e), line))?;
            let json: serde_json::Value = resp.into_json().map_err(|e| {
                PerlError::runtime(format!("ai_models: anthropic decode: {}", e), line)
            })?;
            json["data"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|o| o["id"].as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default()
        }
        "ollama" => {
            let base =
                std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".into());
            let url = format!("{}/api/tags", base.trim_end_matches('/'));
            let resp = agent
                .get(&url)
                .call()
                .map_err(|e| PerlError::runtime(format!("ai_models: ollama: {}", e), line))?;
            let json: serde_json::Value = resp.into_json().map_err(|e| {
                PerlError::runtime(format!("ai_models: ollama decode: {}", e), line)
            })?;
            json["models"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|o| o["name"].as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default()
        }
        "gemini" | "google" => {
            let key = std::env::var("GOOGLE_API_KEY")
                .or_else(|_| std::env::var("GEMINI_API_KEY"))
                .map_err(|_| PerlError::runtime("ai_models: $GOOGLE_API_KEY not set", line))?;
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models?key={}",
                key
            );
            let resp = agent
                .get(&url)
                .call()
                .map_err(|e| PerlError::runtime(format!("ai_models: gemini: {}", e), line))?;
            let json: serde_json::Value = resp.into_json().map_err(|e| {
                PerlError::runtime(format!("ai_models: gemini decode: {}", e), line)
            })?;
            json["models"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|o| {
                            o["name"]
                                .as_str()
                                .map(|s| s.trim_start_matches("models/").to_string())
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
        other => {
            return Err(PerlError::runtime(
                format!(
                    "ai_models: unknown provider `{}` (try openai|anthropic|ollama|gemini)",
                    other
                ),
                line,
            ));
        }
    };
    let arr: Vec<PerlValue> = ids.into_iter().map(PerlValue::string).collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        arr,
    ))))
}

/// Build a multipart/form-data body. Each field is `(name, bytes, content_type_filename)`.
/// When `content_type_filename` is `Some("name:type")` we treat it as a file part.
fn build_multipart(fields: &[(&str, &[u8], Option<&str>)]) -> Vec<u8> {
    let boundary = "stryke_form_boundary_3f7a";
    let mut out = Vec::new();
    for (name, bytes, ctf) in fields {
        if bytes.is_empty() && ctf.is_none() {
            continue; // skip empty plain fields like an unset `language`
        }
        out.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        match ctf {
            Some(ct) if ct.contains(':') => {
                let (filename, mime) = ct.split_once(':').unwrap();
                out.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                        name, filename, mime
                    )
                    .as_bytes(),
                );
            }
            Some(ct) => {
                out.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                        name, ct
                    )
                    .as_bytes(),
                );
            }
            None => {
                out.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", name).as_bytes(),
                );
            }
        }
        out.extend_from_slice(bytes);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());
    out
}

fn guess_media_type(path: &str) -> String {
    let lower = path.to_lowercase();
    if lower.ends_with(".png") {
        "image/png".into()
    } else if lower.ends_with(".gif") {
        "image/gif".into()
    } else if lower.ends_with(".webp") {
        "image/webp".into()
    } else {
        "image/jpeg".into()
    }
}

// ── Structured output (schema-validated JSON) ─────────────────────────
//
// `ai("extract names", $text, schema => +{name => "string", age => "int"})`
// asks the model to respond with strict JSON matching the schema, then
// parses + best-effort validates the result. Returns a hashref/arrayref
// of typed values rather than a raw string.

pub(crate) fn ai_extract(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let prompt = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_extract: prompt required", line))?;
    let opts = parse_opts(&args[1..]);
    let schema_v = opts.get("schema").cloned().ok_or_else(|| {
        PerlError::runtime("ai_extract: pass schema => +{field => \"type\"}", line)
    })?;
    let schema_str = describe_schema(&schema_v);

    let context = opts
        .get("context")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let body = if context.is_empty() {
        prompt.clone()
    } else {
        format!("{}\n\nContext:\n{}", prompt, context)
    };

    let full_prompt = format!(
        "{}\n\nReturn ONLY valid JSON matching this schema:\n{}\n\n\
         Output the JSON object directly with no surrounding prose.",
        body, schema_str
    );

    // Forward all opts except `schema` and `context` to the underlying call.
    let mut call_args = vec![PerlValue::string(full_prompt)];
    for (k, v) in &opts {
        if k == "schema" || k == "context" {
            continue;
        }
        call_args.push(PerlValue::string(k.clone()));
        call_args.push(v.clone());
    }
    let raw = ai_prompt(&call_args, line)?.to_string();
    let json_str = extract_first_json_object(&raw).unwrap_or(&raw);
    let parsed: serde_json::Value = serde_json::from_str(json_str).map_err(|e| {
        PerlError::runtime(
            format!(
                "ai_extract: parse: {} (raw: {})",
                e,
                truncate(json_str, 200)
            ),
            line,
        )
    })?;
    Ok(coerce_to_schema(&parsed, &schema_v))
}

fn describe_schema(v: &PerlValue) -> String {
    let map = match v
        .as_hash_map()
        .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
    {
        Some(m) => m,
        None => return v.to_string(),
    };
    let mut lines = Vec::with_capacity(map.len());
    lines.push("{".to_string());
    for (k, ty) in &map {
        let t = ty.to_string();
        lines.push(format!("  \"{}\": <{}>,", k, t));
    }
    if let Some(last) = lines.last_mut() {
        if let Some(stripped) = last.strip_suffix(',') {
            *last = stripped.to_string();
        }
    }
    lines.push("}".to_string());
    lines.join("\n")
}

fn extract_first_json_object(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut start = None;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate() {
        if esc {
            esc = false;
            continue;
        }
        if in_str {
            match b {
                b'\\' => esc = true,
                b'"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s_idx) = start {
                        return Some(&s[s_idx..=i]);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn coerce_to_schema(v: &serde_json::Value, schema: &PerlValue) -> PerlValue {
    let schema_map = schema
        .as_hash_map()
        .or_else(|| schema.as_hash_ref().map(|h| h.read().clone()));
    match (v, schema_map) {
        (serde_json::Value::Object(obj), Some(s)) => {
            let mut out = IndexMap::new();
            for (k, ty) in &s {
                let raw = obj.get(k).cloned().unwrap_or(serde_json::Value::Null);
                out.insert(k.clone(), coerce_value(&raw, &ty.to_string()));
            }
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(out)))
        }
        _ => json_to_perl_helper(v),
    }
}

fn coerce_value(v: &serde_json::Value, ty: &str) -> PerlValue {
    match ty {
        "int" | "integer" | "Int" => match v {
            serde_json::Value::Number(n) => PerlValue::integer(n.as_i64().unwrap_or(0)),
            serde_json::Value::String(s) => PerlValue::integer(s.parse::<i64>().unwrap_or(0)),
            serde_json::Value::Bool(b) => PerlValue::integer(if *b { 1 } else { 0 }),
            _ => PerlValue::integer(0),
        },
        "number" | "float" | "Float" | "Num" => match v {
            serde_json::Value::Number(n) => PerlValue::float(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::String(s) => PerlValue::float(s.parse::<f64>().unwrap_or(0.0)),
            _ => PerlValue::float(0.0),
        },
        "bool" | "boolean" | "Bool" => match v {
            serde_json::Value::Bool(b) => PerlValue::integer(if *b { 1 } else { 0 }),
            serde_json::Value::Number(n) => {
                PerlValue::integer(if n.as_i64().unwrap_or(0) != 0 { 1 } else { 0 })
            }
            serde_json::Value::String(s) => PerlValue::integer(
                if matches!(s.to_lowercase().as_str(), "true" | "yes" | "1") {
                    1
                } else {
                    0
                },
            ),
            _ => PerlValue::integer(0),
        },
        "array" | "list" => json_to_perl_helper(v),
        "object" | "hash" | "HashRef" => json_to_perl_helper(v),
        _ => match v {
            serde_json::Value::String(s) => PerlValue::string(s.clone()),
            serde_json::Value::Null => PerlValue::UNDEF,
            other => PerlValue::string(other.to_string()),
        },
    }
}

fn json_to_perl_helper(v: &serde_json::Value) -> PerlValue {
    match v {
        serde_json::Value::Null => PerlValue::UNDEF,
        serde_json::Value::Bool(b) => PerlValue::integer(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                PerlValue::integer(i)
            } else if let Some(f) = n.as_f64() {
                PerlValue::float(f)
            } else {
                PerlValue::UNDEF
            }
        }
        serde_json::Value::String(s) => PerlValue::string(s.clone()),
        serde_json::Value::Array(arr) => {
            let items: Vec<PerlValue> = arr.iter().map(json_to_perl_helper).collect();
            PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(items)))
        }
        serde_json::Value::Object(obj) => {
            let mut m = IndexMap::new();
            for (k, v) in obj {
                m.insert(k.clone(), json_to_perl_helper(v));
            }
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m)))
        }
    }
}

// ── Convenience wrappers ──────────────────────────────────────────────

pub(crate) fn ai_summarize(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let text = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_summarize: text required", line))?;
    let opts = parse_opts(&args[1..]);
    let words = opt_int(&opts, "words", 50);
    let prompt = format!(
        "Summarize the text below in roughly {} words. Keep it concise and factual.\n\n\
         Text:\n{}",
        words, text
    );
    let mut call_args = vec![PerlValue::string(prompt)];
    forward_opts(&mut call_args, &opts);
    ai_prompt(&call_args, line)
}

pub(crate) fn ai_translate(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let text = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_translate: text required", line))?;
    let opts = parse_opts(&args[1..]);
    let target = opt_str(&opts, "to", "English");
    let prompt = format!(
        "Translate the text below into {}. Output only the translation.\n\n\
         Text:\n{}",
        target, text
    );
    let mut call_args = vec![PerlValue::string(prompt)];
    forward_opts(&mut call_args, &opts);
    ai_prompt(&call_args, line)
}

// ── Scoped budget ─────────────────────────────────────────────────────

/// `ai_budget($usd_max, sub { ... })` — runs the block, fails the
/// program with a runtime error if cost during the block exceeds
/// `$usd_max`. Restores the prior global cap on exit.
impl Interpreter {
    pub(crate) fn ai_budget(&mut self, args: &[PerlValue], line: usize) -> Result<PerlValue> {
        let cap = args
            .first()
            .map(|v| v.to_number())
            .ok_or_else(|| PerlError::runtime("ai_budget: usd cap required", line))?;
        let body = args
            .get(1)
            .and_then(|v| v.as_code_ref())
            .ok_or_else(|| PerlError::runtime("ai_budget: second arg must be a coderef", line))?;

        let snapshot = current_cost_usd();
        let prev_cap = config().lock().max_cost_run_usd;
        config().lock().max_cost_run_usd = snapshot + cap;

        let result = self.call_sub(&body, vec![], WantarrayCtx::Scalar, line);

        config().lock().max_cost_run_usd = prev_cap;
        let spent = current_cost_usd() - snapshot;

        match result {
            Ok(v) => {
                if spent > cap {
                    return Err(PerlError::runtime(
                        format!("ai_budget: spent ${:.4} (cap ${:.2})", spent, cap),
                        line,
                    ));
                }
                Ok(v)
            }
            Err(crate::interpreter::FlowOrError::Flow(_)) => Ok(PerlValue::UNDEF),
            Err(crate::interpreter::FlowOrError::Error(e)) => Err(e),
        }
    }
}

pub(crate) fn ai_budget_dispatch(
    interp: &mut Interpreter,
    args: &[PerlValue],
    line: usize,
) -> Result<PerlValue> {
    interp.ai_budget(args, line)
}

// ── PDF / document input ──────────────────────────────────────────────

/// `ai($prompt, pdf => $path|$url|$bytes)` — same shape as image
/// vision, but builds an Anthropic `document` content block. Anthropic
/// supports up to 32MB / 100 pages per PDF.
pub(crate) fn ai_pdf(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let prompt = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_pdf: prompt required", line))?;
    let opts = parse_opts(&args[1..]);
    let pdf_v = opts
        .get("pdf")
        .cloned()
        .ok_or_else(|| PerlError::runtime("ai_pdf: pass pdf => $path|$url|$bytes", line))?;
    let provider = opt_str(&opts, "provider", &config().lock().provider);
    if provider != "anthropic" {
        return Err(PerlError::runtime(
            format!(
                "ai_pdf: provider `{}` not implemented (Anthropic only)",
                provider
            ),
            line,
        ));
    }
    let model = opt_str(&opts, "model", &config().lock().model);
    let system = opt_str(&opts, "system", "");
    let max_tokens = opt_int(&opts, "max_tokens", 1024);
    let timeout = opt_int(&opts, "timeout", 120);

    if let Some(resp) = match_mock(&prompt) {
        return Ok(PerlValue::string(resp));
    }
    if mock_only_mode() {
        return Err(PerlError::runtime(
            "ai_pdf: STRYKE_AI_MODE=mock-only and no mock matched",
            line,
        ));
    }

    let (b64, _) = resolve_pdf_input(&pdf_v, line)?;
    let key_env = config().lock().api_key_env.clone();
    let api_key = std::env::var(&key_env)
        .map_err(|_| PerlError::runtime(format!("ai_pdf: ${} env var not set", key_env), line))?;
    let want_citations = opt_int(&opts, "citations", 0) != 0;
    let title = opt_str(&opts, "title", "");
    let mut document = serde_json::json!({
        "type": "document",
        "source": {
            "type": "base64",
            "media_type": "application/pdf",
            "data": b64,
        }
    });
    if want_citations {
        document["citations"] = serde_json::json!({ "enabled": true });
    }
    if !title.is_empty() {
        document["title"] = serde_json::Value::String(title);
    }
    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": [{
            "role": "user",
            "content": [
                document,
                { "type": "text", "text": prompt }
            ]
        }],
    });
    if !system.is_empty() {
        body["system"] = serde_json::Value::String(system);
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post("https://api.anthropic.com/v1/messages")
        .set("x-api-key", &api_key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("ai_pdf: anthropic: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_pdf: decode: {}", e), line))?;

    if let Some(input) = json["usage"]["input_tokens"].as_u64() {
        INPUT_TOKENS.fetch_add(input, Ordering::Relaxed);
        let (in_per_1k, _) = price_per_1k_tokens(&model);
        add_cost(input as f64 / 1000.0 * in_per_1k);
    }
    if let Some(output) = json["usage"]["output_tokens"].as_u64() {
        OUTPUT_TOKENS.fetch_add(output, Ordering::Relaxed);
        let (_, out_per_1k) = price_per_1k_tokens(&model);
        add_cost(output as f64 / 1000.0 * out_per_1k);
    }

    let mut out = String::new();
    let mut citations: Vec<serde_json::Value> = Vec::new();
    if let Some(arr) = json["content"].as_array() {
        for chunk in arr {
            if chunk["type"] == "text" {
                if let Some(t) = chunk["text"].as_str() {
                    out.push_str(t);
                }
                if let Some(cs) = chunk["citations"].as_array() {
                    citations.extend(cs.iter().cloned());
                }
            }
        }
    }
    *last_citations().lock() = citations;
    Ok(PerlValue::string(out))
}

// ── Multi-document grounded responses ────────────────────────────────
//
// `ai_grounded($prompt, documents => [\@docs], titles => [\@titles])`
// — Anthropic-only convenience for grounding a single prompt against
// multiple reference documents with citations enabled. Each document
// is a path (PDF or text) or an inline string. The model's response
// carries citations referencing each document by index. Use
// `ai_citations()` to retrieve them after the call.
pub(crate) fn ai_grounded(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let prompt = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_grounded: prompt required", line))?;
    let opts = parse_opts(&args[1..]);
    let docs_v = opts
        .get("documents")
        .cloned()
        .ok_or_else(|| PerlError::runtime("ai_grounded: documents => [...] required", line))?;
    let docs: Vec<String> = match docs_v.as_array_ref() {
        Some(arr) => arr.read().iter().map(|v| v.to_string()).collect(),
        None => vec![docs_v.to_string()],
    };
    let titles: Vec<String> = match opts.get("titles").and_then(|v| v.as_array_ref()) {
        Some(arr) => arr.read().iter().map(|v| v.to_string()).collect(),
        None => Vec::new(),
    };
    let model = opt_str(&opts, "model", &config().lock().model);
    let system = opt_str(&opts, "system", "");
    let max_tokens = opt_int(&opts, "max_tokens", 1024);
    let timeout = opt_int(&opts, "timeout", 180);
    let provider = opt_str(&opts, "provider", &config().lock().provider);
    if provider != "anthropic" {
        return Err(PerlError::runtime(
            format!(
                "ai_grounded: provider `{}` not supported (Anthropic only)",
                provider
            ),
            line,
        ));
    }

    if let Some(resp) = match_mock(&prompt) {
        return Ok(PerlValue::string(resp));
    }
    if mock_only_mode() {
        return Err(PerlError::runtime(
            "ai_grounded: STRYKE_AI_MODE=mock-only and no mock matched",
            line,
        ));
    }
    let key_env = config().lock().api_key_env.clone();
    let api_key = std::env::var(&key_env)
        .map_err(|_| PerlError::runtime(format!("ai_grounded: ${} not set", key_env), line))?;

    let mut content_blocks: Vec<serde_json::Value> = Vec::with_capacity(docs.len() + 1);
    for (i, doc) in docs.iter().enumerate() {
        let block = build_document_block(doc, titles.get(i).map(|s| s.as_str()), line)?;
        content_blocks.push(block);
    }
    content_blocks.push(serde_json::json!({ "type": "text", "text": prompt }));
    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": max_tokens,
        "messages": [{
            "role": "user",
            "content": content_blocks,
        }],
    });
    if !system.is_empty() {
        body["system"] = serde_json::Value::String(system);
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post("https://api.anthropic.com/v1/messages")
        .set("x-api-key", &api_key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("ai_grounded: anthropic: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_grounded: decode: {}", e), line))?;

    if let Some(input) = json["usage"]["input_tokens"].as_u64() {
        INPUT_TOKENS.fetch_add(input, Ordering::Relaxed);
        let (in_per_1k, _) = price_per_1k_tokens(&model);
        add_cost(input as f64 / 1000.0 * in_per_1k);
    }
    if let Some(output) = json["usage"]["output_tokens"].as_u64() {
        OUTPUT_TOKENS.fetch_add(output, Ordering::Relaxed);
        let (_, out_per_1k) = price_per_1k_tokens(&model);
        add_cost(output as f64 / 1000.0 * out_per_1k);
    }

    let mut out = String::new();
    let mut citations: Vec<serde_json::Value> = Vec::new();
    if let Some(arr) = json["content"].as_array() {
        for chunk in arr {
            if chunk["type"] == "text" {
                if let Some(t) = chunk["text"].as_str() {
                    out.push_str(t);
                }
                if let Some(cs) = chunk["citations"].as_array() {
                    citations.extend(cs.iter().cloned());
                }
            }
        }
    }
    *last_citations().lock() = citations;
    Ok(PerlValue::string(out))
}

/// Build one Anthropic content block from a document spec. Auto-detects:
/// - PDF path (`.pdf` extension or magic bytes) → base64 document block
/// - Other file path → plain-text document block
/// - Inline string with no path-like shape → plain-text block
fn build_document_block(spec: &str, title: Option<&str>, line: usize) -> Result<serde_json::Value> {
    // Treat as a path if it points at an existing file.
    let p = std::path::Path::new(spec);
    let mut block = if p.is_file() {
        let bytes = std::fs::read(p)
            .map_err(|e| PerlError::runtime(format!("ai_grounded: read {}: {}", spec, e), line))?;
        let is_pdf = bytes.starts_with(b"%PDF-") || spec.to_lowercase().ends_with(".pdf");
        if is_pdf {
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            serde_json::json!({
                "type": "document",
                "source": {
                    "type": "base64",
                    "media_type": "application/pdf",
                    "data": b64,
                },
                "citations": { "enabled": true },
            })
        } else {
            let text = String::from_utf8_lossy(&bytes).to_string();
            serde_json::json!({
                "type": "document",
                "source": {
                    "type": "text",
                    "media_type": "text/plain",
                    "data": text,
                },
                "citations": { "enabled": true },
            })
        }
    } else {
        // Inline string → treat as raw text content.
        serde_json::json!({
            "type": "document",
            "source": {
                "type": "text",
                "media_type": "text/plain",
                "data": spec,
            },
            "citations": { "enabled": true },
        })
    };
    if let Some(t) = title {
        if !t.is_empty() {
            block["title"] = serde_json::Value::String(t.to_string());
        }
    }
    Ok(block)
}

// ── Anthropic Batch API (50% off, async) ────────────────────────────
//
// `ai_batch(\@prompts, model => "...", system => "...")` submits a
// list of prompts as one batch (cheaper at the cost of being async),
// polls until done, returns an arrayref of result strings in input
// order. Falls back to sequential calls when the batch endpoint is
// unavailable or `STRYKE_AI_BATCH=sync` is set.

pub(crate) fn ai_batch(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let prompts_v = args
        .first()
        .ok_or_else(|| PerlError::runtime("ai_batch: prompt list required", line))?;
    let prompts: Vec<String> = if let Some(arr) = prompts_v.as_array_ref() {
        arr.read().iter().map(|v| v.to_string()).collect()
    } else {
        vec![prompts_v.to_string()]
    };
    if prompts.is_empty() {
        return Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
            Vec::new(),
        ))));
    }
    let opts = parse_opts(&args[1..]);
    let provider = opt_str(&opts, "provider", &config().lock().provider);

    if matches!(
        std::env::var("STRYKE_AI_BATCH").as_deref(),
        Ok("sync") | Ok("seq")
    ) || provider != "anthropic"
    {
        return ai_batch_sequential(&prompts, &opts, line);
    }
    match ai_batch_anthropic(&prompts, &opts, line) {
        Ok(v) => Ok(v),
        Err(e) => {
            // If batch endpoint isn't available (org access, region,
            // etc.), gracefully fall back to sequential.
            eprintln!("ai_batch: batch failed ({}), falling back to sequential", e);
            ai_batch_sequential(&prompts, &opts, line)
        }
    }
}

fn ai_batch_sequential(
    prompts: &[String],
    opts: &IndexMap<String, PerlValue>,
    line: usize,
) -> Result<PerlValue> {
    let mut out: Vec<PerlValue> = Vec::with_capacity(prompts.len());
    for p in prompts {
        let mut call_args: Vec<PerlValue> = vec![PerlValue::string(p.clone())];
        for (k, v) in opts {
            call_args.push(PerlValue::string(k.clone()));
            call_args.push(v.clone());
        }
        out.push(ai_prompt(&call_args, line)?);
    }
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        out,
    ))))
}

fn ai_batch_anthropic(
    prompts: &[String],
    opts: &IndexMap<String, PerlValue>,
    line: usize,
) -> Result<PerlValue> {
    let model = opt_str(opts, "model", &config().lock().model);
    let system = opt_str(opts, "system", "");
    let max_tokens = opt_int(opts, "max_tokens", 1024);
    let poll_interval = opt_int(opts, "poll_secs", 5).max(1) as u64;
    let max_wait = opt_int(opts, "max_wait_secs", 24 * 3600).max(60) as u64;

    let key_env = config().lock().api_key_env.clone();
    let api_key = std::env::var(&key_env)
        .map_err(|_| PerlError::runtime(format!("ai_batch: ${} env var not set", key_env), line))?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();

    // Build per-prompt requests.
    let requests: Vec<serde_json::Value> = prompts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut params = serde_json::json!({
                "model": model,
                "max_tokens": max_tokens,
                "messages": [{ "role": "user", "content": p }],
            });
            if !system.is_empty() {
                params["system"] = serde_json::Value::String(system.clone());
            }
            serde_json::json!({
                "custom_id": format!("req-{}", i),
                "params": params,
            })
        })
        .collect();

    let create_resp = agent
        .post("https://api.anthropic.com/v1/messages/batches")
        .set("x-api-key", &api_key)
        .set("anthropic-version", "2023-06-01")
        .set("content-type", "application/json")
        .send_json(serde_json::json!({ "requests": requests }))
        .map_err(|e| PerlError::runtime(format!("ai_batch: create: {}", e), line))?;
    let create_json: serde_json::Value = create_resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_batch: create decode: {}", e), line))?;
    let batch_id = create_json["id"]
        .as_str()
        .ok_or_else(|| {
            PerlError::runtime(
                format!(
                    "ai_batch: no batch id in response (raw: {})",
                    truncate(&create_json.to_string(), 200)
                ),
                line,
            )
        })?
        .to_string();

    // Poll for completion.
    let started = std::time::Instant::now();
    let results_url: String;
    loop {
        if started.elapsed() > Duration::from_secs(max_wait) {
            return Err(PerlError::runtime(
                format!("ai_batch: max_wait_secs={} exceeded", max_wait),
                line,
            ));
        }
        let status_resp = agent
            .get(&format!(
                "https://api.anthropic.com/v1/messages/batches/{}",
                batch_id
            ))
            .set("x-api-key", &api_key)
            .set("anthropic-version", "2023-06-01")
            .call()
            .map_err(|e| PerlError::runtime(format!("ai_batch: status: {}", e), line))?;
        let status_json: serde_json::Value = status_resp
            .into_json()
            .map_err(|e| PerlError::runtime(format!("ai_batch: status decode: {}", e), line))?;
        let status = status_json["processing_status"].as_str().unwrap_or("");
        if status == "ended" {
            results_url = status_json["results_url"]
                .as_str()
                .ok_or_else(|| PerlError::runtime("ai_batch: no results_url after ended", line))?
                .to_string();
            break;
        }
        std::thread::sleep(Duration::from_secs(poll_interval));
    }

    // Fetch results — JSONL.
    let results_resp = agent
        .get(&results_url)
        .set("x-api-key", &api_key)
        .set("anthropic-version", "2023-06-01")
        .call()
        .map_err(|e| PerlError::runtime(format!("ai_batch: results: {}", e), line))?;
    let body = results_resp
        .into_string()
        .map_err(|e| PerlError::runtime(format!("ai_batch: read: {}", e), line))?;

    // Map by custom_id and reorder.
    let mut by_id: IndexMap<String, String> = IndexMap::new();
    let mut total_input = 0u64;
    let mut total_output = 0u64;
    for line_str in body.lines() {
        let v: serde_json::Value = match serde_json::from_str(line_str) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let cid = v["custom_id"].as_str().unwrap_or("").to_string();
        let result = &v["result"];
        let kind = result["type"].as_str().unwrap_or("");
        if kind == "succeeded" {
            let msg = &result["message"];
            if let Some(input) = msg["usage"]["input_tokens"].as_u64() {
                total_input += input;
            }
            if let Some(output) = msg["usage"]["output_tokens"].as_u64() {
                total_output += output;
            }
            let mut text = String::new();
            if let Some(arr) = msg["content"].as_array() {
                for chunk in arr {
                    if chunk["type"] == "text" {
                        if let Some(t) = chunk["text"].as_str() {
                            text.push_str(t);
                        }
                    }
                }
            }
            by_id.insert(cid, text);
        } else {
            by_id.insert(
                cid,
                format!(
                    "[batch error: {}]",
                    result["error"]["type"].as_str().unwrap_or("unknown")
                ),
            );
        }
    }

    INPUT_TOKENS.fetch_add(total_input, Ordering::Relaxed);
    OUTPUT_TOKENS.fetch_add(total_output, Ordering::Relaxed);
    let (in_per_1k, out_per_1k) = price_per_1k_tokens(&model);
    // Batch is 50% of normal price.
    let cost = total_input as f64 / 1000.0 * in_per_1k * 0.50
        + total_output as f64 / 1000.0 * out_per_1k * 0.50;
    add_cost(cost);

    let out: Vec<PerlValue> = (0..prompts.len())
        .map(|i| {
            let cid = format!("req-{}", i);
            PerlValue::string(by_id.get(&cid).cloned().unwrap_or_default())
        })
        .collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        out,
    ))))
}

// ── Cluster fanout (Phase 5) ─────────────────────────────────────────
//
// `ai_pmap(\@items, "instruction", cluster => $c)` runs `ai_map` shape
// across cluster nodes via the existing `pmap_on` plumbing. Each
// worker receives a slice of items and produces the corresponding
// summary/answer slice. Without a cluster, falls back to a local
// rayon parallel map.

pub(crate) fn ai_pmap(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let items_v = args
        .first()
        .ok_or_else(|| PerlError::runtime("ai_pmap: items array required", line))?;
    let items: Vec<PerlValue> = if let Some(arr) = items_v.as_array_ref() {
        arr.read().clone()
    } else {
        items_v.clone().to_list()
    };
    let instruction = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_pmap: instruction required", line))?;
    let opts = parse_opts(&args[2.min(args.len())..]);

    // For v0 we run sequentially via ai_map, which itself batches into
    // a single LLM call when called on a list; the cluster wiring is
    // a 1-call-per-shard split. Without an actual cluster handle, that
    // collapses to a single ai_map call.
    let cluster_v = opts.get("cluster").cloned();
    if cluster_v.is_none() {
        let mut call_args: Vec<PerlValue> = vec![items_v.clone(), PerlValue::string(instruction)];
        for (k, v) in &opts {
            if k == "cluster" {
                continue;
            }
            call_args.push(PerlValue::string(k.clone()));
            call_args.push(v.clone());
        }
        return ai_map(&call_args, line);
    }

    // Cluster mode: split items into N shards (N = slot count), run
    // ai_map on each shard via the cluster's pmap_on, concat results
    // back in order. Uses the existing cluster.rs run_cluster API
    // which handles serialization.
    let cluster_pv = cluster_v.unwrap();
    let cluster = cluster_pv
        .as_remote_cluster()
        .ok_or_else(|| PerlError::runtime("ai_pmap: cluster arg is not a cluster handle", line))?;
    let num_workers = cluster.slots.len().max(1);
    let shards = shard_items(&items, num_workers);

    // Each shard runs `ai_map(\@items_n, $instruction)` on the worker.
    let block_src = format!(
        r#"my @items = @_; ai_map(\@items, q{{{}}})"#,
        instruction.replace('}', "\\}")
    );
    let serialized: Vec<serde_json::Value> = shards
        .iter()
        .map(|shard| {
            let arr: Vec<serde_json::Value> = shard.iter().map(perl_value_to_json).collect();
            serde_json::Value::Array(arr)
        })
        .collect();

    let results =
        crate::cluster::run_cluster(&cluster, String::new(), block_src, Vec::new(), serialized)
            .map_err(|e| PerlError::runtime(format!("ai_pmap: cluster: {}", e), line))?;

    // Concat shards.
    let mut out: Vec<PerlValue> = Vec::with_capacity(items.len());
    for r in results {
        let arr = r
            .as_array_ref()
            .map(|a| a.read().clone())
            .unwrap_or_else(|| r.to_list());
        out.extend(arr);
    }
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        out,
    ))))
}

fn shard_items(items: &[PerlValue], n: usize) -> Vec<Vec<PerlValue>> {
    if n == 0 {
        return vec![items.to_vec()];
    }
    let chunk = items.len().div_ceil(n);
    let mut out = Vec::with_capacity(n);
    for c in items.chunks(chunk.max(1)) {
        out.push(c.to_vec());
    }
    while out.len() < n {
        out.push(Vec::new());
    }
    out
}

fn perl_value_to_json(v: &PerlValue) -> serde_json::Value {
    if v.is_undef() {
        return serde_json::Value::Null;
    }
    if let Some(i) = v.as_integer() {
        return serde_json::Value::Number(serde_json::Number::from(i));
    }
    if let Some(f) = v.as_float() {
        return serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null);
    }
    if let Some(s) = v.as_str() {
        return serde_json::Value::String(s);
    }
    serde_json::Value::String(v.to_string())
}

// ── Conversational sessions (multi-turn chat) ────────────────────────

#[derive(Clone)]
struct ChatSession {
    system: String,
    model: String,
    provider: String,
    messages: Vec<(String, String)>, // (role, content)
}

static SESSIONS: OnceLock<Mutex<IndexMap<u64, ChatSession>>> = OnceLock::new();
static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

fn sessions() -> &'static Mutex<IndexMap<u64, ChatSession>> {
    SESSIONS.get_or_init(|| Mutex::new(IndexMap::new()))
}

pub(crate) fn ai_session_new(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let opts = parse_opts(args);
    let cfg = config().lock().clone();
    let sess = ChatSession {
        system: opt_str(&opts, "system", ""),
        model: opt_str(&opts, "model", &cfg.model),
        provider: opt_str(&opts, "provider", &cfg.provider),
        messages: Vec::new(),
    };
    let id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    sessions().lock().insert(id, sess);
    let mut m = IndexMap::new();
    m.insert("__session_id__".into(), PerlValue::integer(id as i64));
    m.insert(
        "model".into(),
        PerlValue::string(opt_str(&opts, "model", &cfg.model)),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m))))
}

pub(crate) fn ai_session_send(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let id = args
        .first()
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .and_then(|m| m.get("__session_id__").map(|v| v.to_int() as u64))
        .ok_or_else(|| {
            PerlError::runtime("ai_session_send: first arg must be a session handle", line)
        })?;
    let user_msg = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_session_send: message required", line))?;
    let opts = parse_opts(&args[2.min(args.len())..]);

    // Snapshot session, append user message.
    let (system, model, provider, prior) = {
        let mut g = sessions().lock();
        let sess = g.get_mut(&id).ok_or_else(|| {
            PerlError::runtime(format!("ai_session_send: session {} not found", id), line)
        })?;
        sess.messages.push(("user".into(), user_msg.clone()));
        (
            sess.system.clone(),
            sess.model.clone(),
            sess.provider.clone(),
            sess.messages.clone(),
        )
    };

    // Build the messages array as PerlValues for the existing chat path.
    let msg_list: Vec<PerlValue> = prior
        .iter()
        .map(|(role, content)| {
            let mut m = IndexMap::new();
            m.insert("role".into(), PerlValue::string(role.clone()));
            m.insert("content".into(), PerlValue::string(content.clone()));
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m)))
        })
        .collect();
    let mut chat_args: Vec<PerlValue> = vec![PerlValue::array_ref(Arc::new(
        parking_lot::RwLock::new(msg_list),
    ))];
    if !system.is_empty() {
        chat_args.push(PerlValue::string("system".into()));
        chat_args.push(PerlValue::string(system));
    }
    chat_args.push(PerlValue::string("model".into()));
    chat_args.push(PerlValue::string(model));
    chat_args.push(PerlValue::string("provider".into()));
    chat_args.push(PerlValue::string(provider));
    for (k, v) in &opts {
        chat_args.push(PerlValue::string(k.clone()));
        chat_args.push(v.clone());
    }
    let resp = ai_chat(&chat_args, line)?;
    let resp_str = resp.to_string();

    // Append assistant turn to session.
    {
        let mut g = sessions().lock();
        if let Some(sess) = g.get_mut(&id) {
            sess.messages.push(("assistant".into(), resp_str.clone()));
        }
    }
    Ok(PerlValue::string(resp_str))
}

pub(crate) fn ai_session_history(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let id = args
        .first()
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .and_then(|m| m.get("__session_id__").map(|v| v.to_int() as u64))
        .ok_or_else(|| PerlError::runtime("ai_session_history: session handle required", line))?;
    let g = sessions().lock();
    let sess = g.get(&id).ok_or_else(|| {
        PerlError::runtime(
            format!("ai_session_history: session {} not found", id),
            line,
        )
    })?;
    let items: Vec<PerlValue> = sess
        .messages
        .iter()
        .map(|(role, content)| {
            let mut m = IndexMap::new();
            m.insert("role".into(), PerlValue::string(role.clone()));
            m.insert("content".into(), PerlValue::string(content.clone()));
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m)))
        })
        .collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        items,
    ))))
}

pub(crate) fn ai_session_close(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let id = args
        .first()
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .and_then(|m| m.get("__session_id__").map(|v| v.to_int() as u64))
        .ok_or_else(|| PerlError::runtime("ai_session_close: session handle required", line))?;
    sessions().lock().shift_remove(&id);
    Ok(PerlValue::UNDEF)
}

pub(crate) fn ai_session_reset(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let id = args
        .first()
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .and_then(|m| m.get("__session_id__").map(|v| v.to_int() as u64))
        .ok_or_else(|| PerlError::runtime("ai_session_reset: session handle required", line))?;
    let mut g = sessions().lock();
    if let Some(sess) = g.get_mut(&id) {
        sess.messages.clear();
    }
    Ok(PerlValue::UNDEF)
}

/// `ai_session_export($handle)` → JSON string capturing the session's
/// system prompt, model, provider, and full message log. Pair with
/// `ai_session_import($json)` to restore on a later run.
pub(crate) fn ai_session_export(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let id = args
        .first()
        .and_then(|v| {
            v.as_hash_map()
                .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        })
        .and_then(|m| m.get("__session_id__").map(|v| v.to_int() as u64))
        .ok_or_else(|| PerlError::runtime("ai_session_export: session handle required", line))?;
    let g = sessions().lock();
    let sess = g.get(&id).ok_or_else(|| {
        PerlError::runtime(format!("ai_session_export: session {} not found", id), line)
    })?;
    let json = serde_json::json!({
        "v": 1,
        "system": sess.system,
        "model": sess.model,
        "provider": sess.provider,
        "messages": sess.messages.iter().map(|(r, c)| serde_json::json!({"role": r, "content": c})).collect::<Vec<_>>(),
    });
    Ok(PerlValue::string(json.to_string()))
}

/// `ai_session_import($json)` → handle hashref. Inverse of
/// `ai_session_export`. Allocates a fresh session id, populates it from the
/// JSON body, returns a handle compatible with the rest of the
/// `ai_session_*` surface.
pub(crate) fn ai_session_import(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let s = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_session_import: json string required", line))?;
    let json: serde_json::Value = serde_json::from_str(&s)
        .map_err(|e| PerlError::runtime(format!("ai_session_import: parse: {}", e), line))?;
    let messages = json["messages"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let role = m["role"].as_str()?.to_string();
                    let content = m["content"].as_str()?.to_string();
                    Some((role, content))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let sess = ChatSession {
        system: json["system"].as_str().unwrap_or("").to_string(),
        model: json["model"].as_str().unwrap_or("").to_string(),
        provider: json["provider"].as_str().unwrap_or("").to_string(),
        messages,
    };
    let id = NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    sessions().lock().insert(id, sess);
    let mut m = IndexMap::new();
    m.insert("__session_id__".into(), PerlValue::integer(id as i64));
    m.insert(
        "model".into(),
        PerlValue::string(json["model"].as_str().unwrap_or("").to_string()),
    );
    m.insert(
        "imported".into(),
        PerlValue::integer(
            json["messages"]
                .as_array()
                .map(|a| a.len() as i64)
                .unwrap_or(0),
        ),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m))))
}

// ── Prompt templates ──────────────────────────────────────────────────

/// `ai_template("hello {name}, you are {age}", name => "world", age => 42)`
/// → substitutes `{key}` placeholders. No code execution; pure string
/// templating.
pub(crate) fn ai_template(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let tmpl = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_template: template required", line))?;
    let opts = parse_opts(&args[1..]);
    let mut out = String::with_capacity(tmpl.len());
    let bytes = tmpl.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            // `{{` escapes to a literal `{`.
            if i + 1 < bytes.len() && bytes[i + 1] == b'{' {
                out.push('{');
                i += 2;
                continue;
            }
            // Find matching `}`.
            if let Some(close) = (i + 1..bytes.len()).find(|&j| bytes[j] == b'}') {
                let key = &tmpl[i + 1..close];
                let trimmed = key.trim();
                if let Some(v) = opts.get(trimmed) {
                    out.push_str(&v.to_string());
                } else {
                    out.push('{');
                    out.push_str(key);
                    out.push('}');
                }
                i = close + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    Ok(PerlValue::string(out))
}

// ── Retry / backoff (used by the call_anthropic / call_openai paths) ─

fn should_retry(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504)
}

fn retry_delay(attempt: u32) -> Duration {
    // Exponential backoff with cap: 1s, 2s, 4s, 8s, 16s, 30s.
    let secs = (1u64 << attempt.min(5)).min(30);
    Duration::from_secs(secs)
}

fn call_anthropic_with_retry(
    agent: &ureq::Agent,
    api_key: &str,
    body: serde_json::Value,
    line: usize,
) -> Result<serde_json::Value> {
    let max_attempts: u32 = 4;
    let mut last_err: Option<PerlError> = None;
    for attempt in 0..max_attempts {
        match agent
            .post("https://api.anthropic.com/v1/messages")
            .set("x-api-key", api_key)
            .set("anthropic-version", "2023-06-01")
            .set("content-type", "application/json")
            .send_json(body.clone())
        {
            Ok(resp) => {
                return resp
                    .into_json()
                    .map_err(|e| PerlError::runtime(format!("ai: anthropic decode: {}", e), line));
            }
            Err(ureq::Error::Status(code, resp)) => {
                if attempt + 1 < max_attempts && should_retry(code) {
                    std::thread::sleep(retry_delay(attempt));
                    continue;
                }
                let body_text = resp.into_string().unwrap_or_default();
                last_err = Some(PerlError::runtime(
                    format!("ai: anthropic {}: {}", code, truncate(&body_text, 200)),
                    line,
                ));
                break;
            }
            Err(ureq::Error::Transport(t)) => {
                if attempt + 1 < max_attempts {
                    std::thread::sleep(retry_delay(attempt));
                    continue;
                }
                last_err = Some(PerlError::runtime(
                    format!("ai: anthropic transport: {}", t),
                    line,
                ));
                break;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| PerlError::runtime("ai: anthropic call failed", line)))
}

// ── Built-in tools (ready-to-pass tool specs) ─────────────────────────
//
// These return hashrefs in the same shape the agent loop wants:
//   +{ name, description, parameters, run => sub { ... } }
// so users can `ai($prompt, tools => [web_search_tool(), fetch_url_tool()])`.
// They are NOT registered globally — pass them in explicitly so apps
// stay in control of which tools any given agent has.

pub(crate) fn web_search_tool(_args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let mut m = IndexMap::new();
    m.insert("name".into(), PerlValue::string("web_search".into()));
    m.insert(
        "description".into(),
        PerlValue::string(
            "Search the public web. Returns top results with title + url + snippet.".into(),
        ),
    );
    let mut params = IndexMap::new();
    params.insert("q".into(), PerlValue::string("string".into()));
    params.insert("limit".into(), PerlValue::string("int".into()));
    m.insert(
        "parameters".into(),
        PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(params))),
    );
    m.insert(
        "__native_tool_id__".into(),
        wrap_native_tool(line, |args| {
            let q = args
                .as_hash_map()
                .or_else(|| args.as_hash_ref().map(|h| h.read().clone()))
                .and_then(|m| m.get("q").map(|v| v.to_string()))
                .unwrap_or_default();
            let limit = args
                .as_hash_map()
                .or_else(|| args.as_hash_ref().map(|h| h.read().clone()))
                .and_then(|m| m.get("limit").map(|v| v.to_int()))
                .unwrap_or(5)
                .clamp(1, 20);
            run_web_search(&q, limit)
        }),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m))))
}

pub(crate) fn fetch_url_tool(_args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let mut m = IndexMap::new();
    m.insert("name".into(), PerlValue::string("fetch_url".into()));
    m.insert(
        "description".into(),
        PerlValue::string("Fetch a URL via HTTP GET. Returns response body as text.".into()),
    );
    let mut params = IndexMap::new();
    params.insert("url".into(), PerlValue::string("string".into()));
    m.insert(
        "parameters".into(),
        PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(params))),
    );
    m.insert(
        "__native_tool_id__".into(),
        wrap_native_tool(line, |args| {
            let url = args
                .as_hash_map()
                .or_else(|| args.as_hash_ref().map(|h| h.read().clone()))
                .and_then(|m| m.get("url").map(|v| v.to_string()))
                .unwrap_or_default();
            run_fetch_url(&url)
        }),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m))))
}

pub(crate) fn read_file_tool(_args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let mut m = IndexMap::new();
    m.insert("name".into(), PerlValue::string("read_file".into()));
    m.insert(
        "description".into(),
        PerlValue::string("Read a file from disk. Returns text contents.".into()),
    );
    let mut params = IndexMap::new();
    params.insert("path".into(), PerlValue::string("string".into()));
    m.insert(
        "parameters".into(),
        PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(params))),
    );
    m.insert(
        "__native_tool_id__".into(),
        wrap_native_tool(line, |args| {
            let path = args
                .as_hash_map()
                .or_else(|| args.as_hash_ref().map(|h| h.read().clone()))
                .and_then(|m| m.get("path").map(|v| v.to_string()))
                .unwrap_or_default();
            match std::fs::read_to_string(&path) {
                Ok(s) => Ok(PerlValue::string(s)),
                Err(e) => Ok(PerlValue::string(format!("read_file error: {}", e))),
            }
        }),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m))))
}

pub(crate) fn run_code_tool(_args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let mut m = IndexMap::new();
    m.insert("name".into(), PerlValue::string("run_code".into()));
    m.insert(
        "description".into(),
        PerlValue::string(
            "Run a snippet of Python in a subprocess. 10s timeout. Returns stdout + stderr.".into(),
        ),
    );
    let mut params = IndexMap::new();
    params.insert("code".into(), PerlValue::string("string".into()));
    m.insert(
        "parameters".into(),
        PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(params))),
    );
    m.insert(
        "__native_tool_id__".into(),
        wrap_native_tool(line, |args| {
            let code = args
                .as_hash_map()
                .or_else(|| args.as_hash_ref().map(|h| h.read().clone()))
                .and_then(|m| m.get("code").map(|v| v.to_string()))
                .unwrap_or_default();
            run_python(&code)
        }),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m))))
}

/// Built-in tool specs carry a `__native_tool_id__` field instead of a
/// `run` coderef. The agent loop's `compile_tool` recognises that
/// marker and routes invocations through `NATIVE_TOOL_REGISTRY`
/// directly, skipping the coderef path.
fn wrap_native_tool<F>(_line: usize, f: F) -> PerlValue
where
    F: Fn(PerlValue) -> Result<PerlValue> + Send + Sync + 'static,
{
    let mut g = native_tool_registry().lock();
    let id = g.len() as i64;
    g.insert(id, Arc::new(f));
    drop(g);
    PerlValue::integer(id)
}

type NativeToolFn = Arc<dyn Fn(PerlValue) -> Result<PerlValue> + Send + Sync + 'static>;
static NATIVE_TOOL_REGISTRY: OnceLock<Mutex<IndexMap<i64, NativeToolFn>>> = OnceLock::new();

pub(crate) fn native_tool_registry() -> &'static Mutex<IndexMap<i64, NativeToolFn>> {
    NATIVE_TOOL_REGISTRY.get_or_init(|| Mutex::new(IndexMap::new()))
}

pub(crate) fn invoke_native_tool(id: i64, arg: PerlValue, line: usize) -> Result<PerlValue> {
    let f = native_tool_registry().lock().get(&id).cloned();
    let Some(f) = f else {
        return Err(PerlError::runtime(
            format!("ai_native_tool: id {} not registered", id),
            line,
        ));
    };
    f(arg)
}

fn run_web_search(q: &str, limit: i64) -> Result<PerlValue> {
    // Honor BRAVE_SEARCH_API_KEY if set; otherwise fall back to a
    // DuckDuckGo HTML scrape (best-effort, public-data only).
    if let Ok(key) = std::env::var("BRAVE_SEARCH_API_KEY") {
        return run_brave_search(q, limit, &key);
    }
    run_ddg_search(q, limit)
}

fn run_brave_search(q: &str, limit: i64, key: &str) -> Result<PerlValue> {
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
        urlencoding(q),
        limit.clamp(1, 20)
    );
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build();
    let resp = agent
        .get(&url)
        .set("X-Subscription-Token", key)
        .set("accept", "application/json")
        .call()
        .map_err(|e| PerlError::runtime(format!("web_search: brave: {}", e), 0))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("web_search: decode: {}", e), 0))?;
    let mut out: Vec<PerlValue> = Vec::new();
    if let Some(results) = json["web"]["results"].as_array() {
        for r in results.iter().take(limit as usize) {
            let mut m = IndexMap::new();
            m.insert(
                "title".into(),
                PerlValue::string(r["title"].as_str().unwrap_or("").to_string()),
            );
            m.insert(
                "url".into(),
                PerlValue::string(r["url"].as_str().unwrap_or("").to_string()),
            );
            m.insert(
                "snippet".into(),
                PerlValue::string(r["description"].as_str().unwrap_or("").to_string()),
            );
            out.push(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m))));
        }
    }
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        out,
    ))))
}

fn run_ddg_search(q: &str, limit: i64) -> Result<PerlValue> {
    let url = format!("https://duckduckgo.com/html/?q={}", urlencoding(q));
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build();
    let resp = agent
        .get(&url)
        .set("user-agent", "Mozilla/5.0 (stryke web_search)")
        .call()
        .map_err(|e| PerlError::runtime(format!("web_search: ddg: {}", e), 0))?;
    let body = resp
        .into_string()
        .map_err(|e| PerlError::runtime(format!("web_search: read: {}", e), 0))?;
    let re = regex::Regex::new(
        r#"<a class="result__a" href="(?P<url>[^"]+)"[^>]*>(?P<title>[^<]+)</a>"#,
    )
    .unwrap();
    let snip_re =
        regex::Regex::new(r#"<a class="result__snippet"[^>]*>(?P<snip>[^<]+)</a>"#).unwrap();
    let titles: Vec<(String, String)> = re
        .captures_iter(&body)
        .map(|c| {
            (
                c.name("url")
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
                c.name("title")
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default(),
            )
        })
        .collect();
    let snips: Vec<String> = snip_re
        .captures_iter(&body)
        .map(|c| {
            c.name("snip")
                .map(|m| m.as_str().to_string())
                .unwrap_or_default()
        })
        .collect();
    let mut out: Vec<PerlValue> = Vec::new();
    for (i, (url, title)) in titles.iter().take(limit as usize).enumerate() {
        let mut m = IndexMap::new();
        m.insert("title".into(), PerlValue::string(title.clone()));
        m.insert("url".into(), PerlValue::string(url.clone()));
        m.insert(
            "snippet".into(),
            PerlValue::string(snips.get(i).cloned().unwrap_or_default()),
        );
        out.push(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m))));
    }
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        out,
    ))))
}

fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
            out.push(c);
        } else {
            for b in c.to_string().as_bytes() {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

fn run_fetch_url(url: &str) -> Result<PerlValue> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    match agent.get(url).call() {
        Ok(r) => match r.into_string() {
            Ok(s) => Ok(PerlValue::string(s)),
            Err(e) => Ok(PerlValue::string(format!("fetch_url error: {}", e))),
        },
        Err(e) => Ok(PerlValue::string(format!("fetch_url error: {}", e))),
    }
}

fn run_python(code: &str) -> Result<PerlValue> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("python3")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| PerlError::runtime(format!("run_code: spawn python3: {}", e), 0))?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(code.as_bytes());
    }
    let timeout = std::time::Duration::from_secs(10);
    let start = std::time::Instant::now();
    loop {
        if let Some(_status) = child.try_wait().ok().flatten() {
            break;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            return Ok(PerlValue::string("run_code: timed out after 10s".into()));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let output = child
        .wait_with_output()
        .map_err(|e| PerlError::runtime(format!("run_code: wait: {}", e), 0))?;
    let mut combined = String::new();
    combined.push_str(&String::from_utf8_lossy(&output.stdout));
    if !output.stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    Ok(PerlValue::string(combined))
}

fn resolve_pdf_input(v: &PerlValue, line: usize) -> Result<(String, String)> {
    use base64::Engine;
    let s = v.to_string();
    let bytes = if s.starts_with("http://") || s.starts_with("https://") {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(60))
            .build();
        let resp = agent
            .get(&s)
            .call()
            .map_err(|e| PerlError::runtime(format!("ai_pdf: fetch: {}", e), line))?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut resp.into_reader(), &mut buf)
            .map_err(|e| PerlError::runtime(format!("ai_pdf: read body: {}", e), line))?;
        buf
    } else if std::path::Path::new(&s).exists() {
        std::fs::read(&s)
            .map_err(|e| PerlError::runtime(format!("ai_pdf: read {}: {}", s, e), line))?
    } else if let Some(arc) = v.as_bytes_arc() {
        (*arc).clone()
    } else {
        return Err(PerlError::runtime(
            "ai_pdf: pdf must be a URL, file path, or raw bytes",
            line,
        ));
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok((b64, "application/pdf".to_string()))
}

// ── Citations accessor ────────────────────────────────────────────────
//
// Anthropic's Citations feature surfaces grounded references for each
// content block: when the model uses a `document` content block with
// `citations.enabled = true`, the response carries `citations: [...]`
// entries pointing back into the source. We capture these into
// `LAST_CITATIONS_BUF` during call_anthropic and surface them via this
// builtin (analogous to `ai_last_thinking`).
pub(crate) fn ai_citations(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let cites = last_citations().lock().clone();
    let arr: Vec<PerlValue> = cites
        .iter()
        .map(|c| {
            let mut h = IndexMap::new();
            if let Some(t) = c["type"].as_str() {
                h.insert("type".to_string(), PerlValue::string(t.to_string()));
            }
            if let Some(t) = c["cited_text"].as_str() {
                h.insert("text".to_string(), PerlValue::string(t.to_string()));
            }
            if let Some(t) = c["document_title"].as_str() {
                h.insert("title".to_string(), PerlValue::string(t.to_string()));
            }
            if let Some(t) = c["document_index"].as_i64() {
                h.insert("document_index".to_string(), PerlValue::integer(t));
            }
            if let Some(t) = c["start_char_index"].as_i64() {
                h.insert("start".to_string(), PerlValue::integer(t));
            }
            if let Some(t) = c["end_char_index"].as_i64() {
                h.insert("end".to_string(), PerlValue::integer(t));
            }
            if let Some(t) = c["start_page_number"].as_i64() {
                h.insert("start_page".to_string(), PerlValue::integer(t));
            }
            if let Some(t) = c["end_page_number"].as_i64() {
                h.insert("end_page".to_string(), PerlValue::integer(t));
            }
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h)))
        })
        .collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        arr,
    ))))
}

// ── Files API (OpenAI) ────────────────────────────────────────────────
//
// `/v1/files` endpoint for uploading reference files (used by Whisper,
// vision, batch, assistants). Returns a file_id usable in subsequent
// API calls.

/// `ai_file_upload("path/to/file", purpose => "user_data")` →
/// hashref with `id`, `bytes`, `created_at`, `filename`, `purpose`.
pub(crate) fn ai_file_upload(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let path = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_file_upload: path required", line))?;
    let opts = parse_opts(&args[1..]);
    let purpose = opt_str(&opts, "purpose", "user_data");
    let timeout = opt_int(&opts, "timeout", 120);

    if mock_only_mode() {
        let mut h = IndexMap::new();
        h.insert(
            "id".to_string(),
            PerlValue::string("file-mock-123".to_string()),
        );
        h.insert("filename".to_string(), PerlValue::string(path.clone()));
        h.insert("purpose".to_string(), PerlValue::string(purpose.clone()));
        return Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))));
    }
    let key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| PerlError::runtime("ai_file_upload: $OPENAI_API_KEY not set", line))?;
    let bytes = std::fs::read(&path)
        .map_err(|e| PerlError::runtime(format!("ai_file_upload: read {}: {}", path, e), line))?;
    let filename = std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload.bin");
    let body = build_multipart(&[
        ("purpose", purpose.as_bytes(), None),
        (
            "file",
            &bytes,
            Some(&format!("{}:application/octet-stream", filename)),
        ),
    ]);
    let boundary = "stryke_form_boundary_3f7a";
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post("https://api.openai.com/v1/files")
        .set("authorization", &format!("Bearer {}", key))
        .set(
            "content-type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&body)
        .map_err(|e| PerlError::runtime(format!("ai_file_upload: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_file_upload: decode: {}", e), line))?;
    Ok(json_to_perl_hash(&json))
}

/// `ai_file_list()` → arrayref of file metadata hashrefs.
pub(crate) fn ai_file_list(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let opts = parse_opts(args);
    let purpose = opt_str(&opts, "purpose", "");
    let timeout = opt_int(&opts, "timeout", 30);

    if mock_only_mode() {
        return Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
            Vec::new(),
        ))));
    }
    let key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| PerlError::runtime("ai_file_list: $OPENAI_API_KEY not set", line))?;
    let mut url = "https://api.openai.com/v1/files".to_string();
    if !purpose.is_empty() {
        url.push_str(&format!("?purpose={}", purpose));
    }
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .get(&url)
        .set("authorization", &format!("Bearer {}", key))
        .call()
        .map_err(|e| PerlError::runtime(format!("ai_file_list: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_file_list: decode: {}", e), line))?;
    let data = json["data"].as_array().cloned().unwrap_or_default();
    let arr: Vec<PerlValue> = data.iter().map(json_to_perl_hash).collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        arr,
    ))))
}

/// `ai_file_delete($file_id)` → 1 if deleted, 0 otherwise.
pub(crate) fn ai_file_delete(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let id = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_file_delete: file_id required", line))?;
    let opts = parse_opts(&args[1..]);
    let timeout = opt_int(&opts, "timeout", 30);

    if mock_only_mode() {
        return Ok(PerlValue::integer(1));
    }
    let key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| PerlError::runtime("ai_file_delete: $OPENAI_API_KEY not set", line))?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .delete(&format!("https://api.openai.com/v1/files/{}", id))
        .set("authorization", &format!("Bearer {}", key))
        .call()
        .map_err(|e| PerlError::runtime(format!("ai_file_delete: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_file_delete: decode: {}", e), line))?;
    Ok(PerlValue::integer(
        if json["deleted"].as_bool().unwrap_or(false) {
            1
        } else {
            0
        },
    ))
}

/// `ai_file_get($file_id)` → metadata hashref.
pub(crate) fn ai_file_get(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let id = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_file_get: file_id required", line))?;
    let opts = parse_opts(&args[1..]);
    let timeout = opt_int(&opts, "timeout", 30);

    if mock_only_mode() {
        let mut h = IndexMap::new();
        h.insert("id".to_string(), PerlValue::string(id));
        return Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))));
    }
    let key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| PerlError::runtime("ai_file_get: $OPENAI_API_KEY not set", line))?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .get(&format!("https://api.openai.com/v1/files/{}", id))
        .set("authorization", &format!("Bearer {}", key))
        .call()
        .map_err(|e| PerlError::runtime(format!("ai_file_get: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_file_get: decode: {}", e), line))?;
    Ok(json_to_perl_hash(&json))
}

// ── Anthropic Files API ───────────────────────────────────────────────
//
// Anthropic's beta `/v1/files` endpoint mirrors OpenAI's: upload a file
// once, reference it by id from batch jobs and document blocks. Auth via
// `$ANTHROPIC_API_KEY`, requires the `files-api-2025-04-14` beta header.

const ANTHROPIC_FILES_BETA: &str = "files-api-2025-04-14";

/// `ai_file_anthropic_upload("path/to/file.pdf")` → metadata hashref with
/// `id`, `filename`, `mime_type`, `size_bytes`, `created_at`.
pub(crate) fn ai_file_anthropic_upload(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let path = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_file_anthropic_upload: path required", line))?;
    let opts = parse_opts(&args[1..]);
    let timeout = opt_int(&opts, "timeout", 120);

    if mock_only_mode() {
        let mut h = IndexMap::new();
        h.insert(
            "id".to_string(),
            PerlValue::string("file-anthropic-mock-1".to_string()),
        );
        h.insert("filename".to_string(), PerlValue::string(path.clone()));
        return Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))));
    }
    let key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        PerlError::runtime("ai_file_anthropic_upload: $ANTHROPIC_API_KEY not set", line)
    })?;
    let bytes = std::fs::read(&path).map_err(|e| {
        PerlError::runtime(
            format!("ai_file_anthropic_upload: read {}: {}", path, e),
            line,
        )
    })?;
    let filename = std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("upload.bin");
    let body = build_multipart(&[(
        "file",
        &bytes,
        Some(&format!("{}:application/octet-stream", filename)),
    )]);
    let boundary = "stryke_form_boundary_3f7a";
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post("https://api.anthropic.com/v1/files")
        .set("x-api-key", &key)
        .set("anthropic-version", "2023-06-01")
        .set("anthropic-beta", ANTHROPIC_FILES_BETA)
        .set(
            "content-type",
            &format!("multipart/form-data; boundary={}", boundary),
        )
        .send_bytes(&body)
        .map_err(|e| PerlError::runtime(format!("ai_file_anthropic_upload: {}", e), line))?;
    let json: serde_json::Value = resp.into_json().map_err(|e| {
        PerlError::runtime(format!("ai_file_anthropic_upload: decode: {}", e), line)
    })?;
    Ok(json_to_perl_hash(&json))
}

/// `ai_file_anthropic_list()` → arrayref of metadata hashrefs.
pub(crate) fn ai_file_anthropic_list(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let opts = parse_opts(args);
    let timeout = opt_int(&opts, "timeout", 30);
    if mock_only_mode() {
        return Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
            Vec::new(),
        ))));
    }
    let key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        PerlError::runtime("ai_file_anthropic_list: $ANTHROPIC_API_KEY not set", line)
    })?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .get("https://api.anthropic.com/v1/files")
        .set("x-api-key", &key)
        .set("anthropic-version", "2023-06-01")
        .set("anthropic-beta", ANTHROPIC_FILES_BETA)
        .call()
        .map_err(|e| PerlError::runtime(format!("ai_file_anthropic_list: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_file_anthropic_list: decode: {}", e), line))?;
    let data = json["data"].as_array().cloned().unwrap_or_default();
    let arr: Vec<PerlValue> = data.iter().map(json_to_perl_hash).collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        arr,
    ))))
}

/// `ai_file_anthropic_delete($file_id)` → 1 on success, 0 otherwise.
pub(crate) fn ai_file_anthropic_delete(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let id = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_file_anthropic_delete: file_id required", line))?;
    let opts = parse_opts(&args[1..]);
    let timeout = opt_int(&opts, "timeout", 30);
    if mock_only_mode() {
        return Ok(PerlValue::integer(1));
    }
    let key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        PerlError::runtime("ai_file_anthropic_delete: $ANTHROPIC_API_KEY not set", line)
    })?;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .delete(&format!("https://api.anthropic.com/v1/files/{}", id))
        .set("x-api-key", &key)
        .set("anthropic-version", "2023-06-01")
        .set("anthropic-beta", ANTHROPIC_FILES_BETA)
        .call();
    match resp {
        Ok(_) => Ok(PerlValue::integer(1)),
        Err(_) => Ok(PerlValue::integer(0)),
    }
}

// ── Moderation (OpenAI) ──────────────────────────────────────────────
//
// `ai_moderate($text, model => "omni-moderation-latest")` — content
// safety classifier. Returns `+{ flagged, categories => +{...},
// scores => +{...} }`. Free endpoint — no token cost. Use it before
// sending user-generated content to a generative model.
pub(crate) fn ai_moderate(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let input = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_moderate: input required", line))?;
    let opts = parse_opts(&args[1..]);
    let model = opt_str(&opts, "model", "omni-moderation-latest");
    let timeout = opt_int(&opts, "timeout", 30);

    if mock_only_mode() {
        let mut h = IndexMap::new();
        h.insert("flagged".to_string(), PerlValue::integer(0));
        h.insert(
            "categories".to_string(),
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(IndexMap::new()))),
        );
        h.insert(
            "scores".to_string(),
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(IndexMap::new()))),
        );
        return Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))));
    }
    let key = std::env::var("OPENAI_API_KEY")
        .map_err(|_| PerlError::runtime("ai_moderate: $OPENAI_API_KEY not set", line))?;
    let body = serde_json::json!({
        "model": model,
        "input": input,
    });
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(timeout.max(1) as u64))
        .build();
    let resp = agent
        .post("https://api.openai.com/v1/moderations")
        .set("authorization", &format!("Bearer {}", key))
        .set("content-type", "application/json")
        .send_json(body)
        .map_err(|e| PerlError::runtime(format!("ai_moderate: {}", e), line))?;
    let json: serde_json::Value = resp
        .into_json()
        .map_err(|e| PerlError::runtime(format!("ai_moderate: decode: {}", e), line))?;
    let result = json["results"]
        .get(0)
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let mut h = IndexMap::new();
    h.insert(
        "flagged".to_string(),
        PerlValue::integer(if result["flagged"].as_bool().unwrap_or(false) {
            1
        } else {
            0
        }),
    );
    h.insert(
        "categories".to_string(),
        json_to_perl_hash(&result["categories"]),
    );
    h.insert(
        "scores".to_string(),
        json_to_perl_hash(&result["category_scores"]),
    );
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))))
}

// ── Text chunking (for RAG) ──────────────────────────────────────────
//
// `ai_chunk($text, max_tokens => 500, overlap => 50, by => "tokens"|"chars"|"sentences")`
// → arrayref of strings. Pure local logic — no API call. Tokens are
// estimated as 4 chars each (matching `tokens_of`). Sentence mode
// splits on `.!?` followed by whitespace, keeping punctuation.
pub(crate) fn ai_chunk(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let text = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_chunk: text required", line))?;
    let opts = parse_opts(&args[1..]);
    let max_tokens = opt_int(&opts, "max_tokens", 500).max(1) as usize;
    let overlap = opt_int(&opts, "overlap", 50).max(0) as usize;
    let by = opt_str(&opts, "by", "tokens");

    let chunks = match by.as_str() {
        "chars" => chunk_by_chars(&text, max_tokens * 4, overlap * 4),
        "sentences" => chunk_by_sentences(&text, max_tokens * 4),
        _ => chunk_by_chars(&text, max_tokens * 4, overlap * 4),
    };
    let arr: Vec<PerlValue> = chunks.into_iter().map(PerlValue::string).collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        arr,
    ))))
}

/// Sliding-window chunker over chars. Each chunk is `max_chars` long; the
/// next chunk starts `max_chars - overlap` after the current one. Final
/// chunk may be shorter.
fn chunk_by_chars(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    if chars.is_empty() {
        return out;
    }
    let stride = max_chars.saturating_sub(overlap).max(1);
    let mut i = 0;
    while i < chars.len() {
        let end = (i + max_chars).min(chars.len());
        out.push(chars[i..end].iter().collect());
        if end >= chars.len() {
            break;
        }
        i += stride;
    }
    out
}

/// Sentence-aware chunker. Splits on `.!?` followed by whitespace, then
/// greedily packs consecutive sentences until the next one would push the
/// chunk over `max_chars`. Minimum one sentence per chunk.
fn chunk_by_sentences(text: &str, max_chars: usize) -> Vec<String> {
    let mut sents: Vec<&str> = Vec::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'.' | b'!' | b'?') {
            let mut end = i + 1;
            while end < bytes.len() && bytes[end].is_ascii_whitespace() {
                end += 1;
            }
            if end > i + 1 || end == bytes.len() {
                if let Ok(s) = std::str::from_utf8(&bytes[start..end]) {
                    sents.push(s.trim());
                }
                start = end;
            }
        }
        i += 1;
    }
    if start < bytes.len() {
        if let Ok(s) = std::str::from_utf8(&bytes[start..]) {
            let t = s.trim();
            if !t.is_empty() {
                sents.push(t);
            }
        }
    }
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    for s in sents {
        if !buf.is_empty() && buf.len() + s.len() + 1 > max_chars {
            out.push(std::mem::take(&mut buf));
        }
        if !buf.is_empty() {
            buf.push(' ');
        }
        buf.push_str(s);
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

// ── Warmup / auth check ──────────────────────────────────────────────
//
// `ai_warm(model => "...", provider => "...")` sends a 1-token "ping"
// request so the user finds out about auth or network issues at script
// start instead of mid-flow. Returns `+{ ok, latency_ms, model }`.
// Counts toward cost like any other call (typically <$0.001).
pub(crate) fn ai_warm(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let opts = parse_opts(args);
    let cfg = config().lock().clone();
    let provider = opt_str(&opts, "provider", &cfg.provider);
    let model = opt_str(&opts, "model", &cfg.model);
    let timeout = opt_int(&opts, "timeout", 15);

    let started = std::time::Instant::now();
    let mut warm_args: Vec<PerlValue> = vec![PerlValue::string("hi".to_string())];
    warm_args.push(PerlValue::string("max_tokens".to_string()));
    warm_args.push(PerlValue::integer(1));
    warm_args.push(PerlValue::string("model".to_string()));
    warm_args.push(PerlValue::string(model.clone()));
    warm_args.push(PerlValue::string("provider".to_string()));
    warm_args.push(PerlValue::string(provider.clone()));
    warm_args.push(PerlValue::string("timeout".to_string()));
    warm_args.push(PerlValue::integer(timeout));
    warm_args.push(PerlValue::string("cache".to_string()));
    warm_args.push(PerlValue::integer(0));

    let result = ai_prompt(&warm_args, 0);
    let elapsed_ms = started.elapsed().as_millis() as i64;
    let mut h = IndexMap::new();
    match result {
        Ok(_) => {
            h.insert("ok".to_string(), PerlValue::integer(1));
            h.insert("error".to_string(), PerlValue::string(String::new()));
        }
        Err(e) => {
            h.insert("ok".to_string(), PerlValue::integer(0));
            h.insert("error".to_string(), PerlValue::string(e.to_string()));
        }
    }
    h.insert("latency_ms".to_string(), PerlValue::integer(elapsed_ms));
    h.insert("model".to_string(), PerlValue::string(model));
    h.insert("provider".to_string(), PerlValue::string(provider));
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))))
}

// ── Semantic comparison ──────────────────────────────────────────────
//
// `ai_compare($a, $b, criteria => "factual accuracy", scale => 5)` →
// hashref `+{ winner, reason, scores => +{a, b} }`. The model picks a
// winner (`"a"`, `"b"`, or `"tie"`) and rates each on the given criteria.
// Single LLM call returns structured JSON.
pub(crate) fn ai_compare(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let a = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_compare: first input required", line))?;
    let b = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("ai_compare: second input required", line))?;
    let opts = parse_opts(&args[2.min(args.len())..]);
    let criteria = opt_str(&opts, "criteria", "overall quality");
    let scale = opt_int(&opts, "scale", 5).clamp(2, 100);

    let prompt = format!(
        "Compare these two inputs on \"{criteria}\". Score each from 1 to {scale}.\n\
         Pick a winner: \"a\", \"b\", or \"tie\". Briefly explain.\n\
         Respond with strict JSON: {{\"winner\":..., \"score_a\":..., \"score_b\":..., \"reason\":\"...\"}}.\n\n\
         A: {a}\n\nB: {b}",
        criteria = criteria,
        scale = scale,
        a = a,
        b = b,
    );

    if let Some(resp) = match_mock(&prompt) {
        let mut h = IndexMap::new();
        h.insert("raw".to_string(), PerlValue::string(resp));
        return Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))));
    }
    if mock_only_mode() {
        let mut h = IndexMap::new();
        h.insert("winner".to_string(), PerlValue::string("tie".to_string()));
        h.insert(
            "reason".to_string(),
            PerlValue::string("mock-only".to_string()),
        );
        let mut scores = IndexMap::new();
        scores.insert("a".to_string(), PerlValue::float(scale as f64 / 2.0));
        scores.insert("b".to_string(), PerlValue::float(scale as f64 / 2.0));
        h.insert(
            "scores".to_string(),
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(scores))),
        );
        return Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))));
    }

    let prompt_args: Vec<PerlValue> = vec![
        PerlValue::string(prompt),
        PerlValue::string("max_tokens".to_string()),
        PerlValue::integer(512),
    ];
    let resp = ai_prompt(&prompt_args, line)?;
    let resp_str = resp.to_string();

    // Best-effort JSON extraction — strip ```json fences and trim.
    let body = strip_json_fences(&resp_str);
    let json: serde_json::Value =
        serde_json::from_str(body.trim()).unwrap_or_else(|_| serde_json::json!({}));
    let mut h = IndexMap::new();
    h.insert(
        "winner".to_string(),
        PerlValue::string(json["winner"].as_str().unwrap_or("tie").to_string()),
    );
    h.insert(
        "reason".to_string(),
        PerlValue::string(json["reason"].as_str().unwrap_or("").to_string()),
    );
    let mut scores = IndexMap::new();
    scores.insert(
        "a".to_string(),
        PerlValue::float(json["score_a"].as_f64().unwrap_or(0.0)),
    );
    scores.insert(
        "b".to_string(),
        PerlValue::float(json["score_b"].as_f64().unwrap_or(0.0)),
    );
    h.insert(
        "scores".to_string(),
        PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(scores))),
    );
    h.insert("raw".to_string(), PerlValue::string(resp_str));
    Ok(PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h))))
}

/// Strip ```json fenced code blocks from a model response — best-effort
/// extractor for structured output that the model wraps in fences despite
/// the prompt asking for raw JSON.
fn strip_json_fences(s: &str) -> &str {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        if let Some(body) = rest.strip_suffix("```") {
            return body.trim();
        }
        return rest.trim();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        if let Some(body) = rest.strip_suffix("```") {
            return body.trim();
        }
        return rest.trim();
    }
    trimmed
}

/// Convert a JSON value into a PerlValue hashref for surfacing to user code.
/// Nested objects / arrays become hashrefs / arrayrefs recursively. Scalars
/// drop into PerlValue::{integer, float, string, true/false}.
fn json_to_perl_hash(v: &serde_json::Value) -> PerlValue {
    match v {
        serde_json::Value::Object(map) => {
            let mut h = IndexMap::new();
            for (k, val) in map {
                h.insert(k.clone(), json_to_perl_value(val));
            }
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(h)))
        }
        _ => json_to_perl_value(v),
    }
}

fn json_to_perl_value(v: &serde_json::Value) -> PerlValue {
    match v {
        serde_json::Value::Null => PerlValue::UNDEF,
        serde_json::Value::Bool(b) => PerlValue::integer(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                PerlValue::integer(i)
            } else if let Some(f) = n.as_f64() {
                PerlValue::float(f)
            } else {
                PerlValue::string(n.to_string())
            }
        }
        serde_json::Value::String(s) => PerlValue::string(s.clone()),
        serde_json::Value::Array(a) => {
            let arr: Vec<PerlValue> = a.iter().map(json_to_perl_value).collect();
            PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(arr)))
        }
        serde_json::Value::Object(_) => json_to_perl_hash(v),
    }
}
