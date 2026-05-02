//! MCP client (Model Context Protocol) — Phase 2 of AI_PRIMITIVES.md.
//!
//! Phase 2 ships the *client* side: connect to an external MCP server,
//! discover its tools / resources / prompts, call them. The *server*
//! side (`mcp_server "name" { ... }` declarative DSL) needs the parser
//! extension for `tool` / `resource` / `prompt` declarations and is
//! still pending.
//!
//! Transports supported here:
//!   * `stdio:CMD ARGS...` — spawn a subprocess, line-delimited JSON-RPC
//!     over stdin/stdout. The MCP spec calls this "stdio transport".
//!
//! Transports NOT yet wired:
//!   * `ws://...`   — WebSocket (needs tungstenite crate)
//!   * `http://...` — streaming HTTP (needs SSE parser)
//!
//! Builtins:
//!   * `mcp_connect("stdio:cmd args...")`           → handle hashref
//!   * `mcp_tools($h)`     → list the server exposes
//!   * `mcp_resources($h)` / `mcp_prompts($h)`
//!   * `mcp_call($h, "tool_name", +{...args})`      → tool result
//!   * `mcp_resource($h, "uri")`
//!   * `mcp_prompt($h, "name", +{...args})`
//!   * `mcp_close($h)`
//!   * `mcp_attach_to_ai($h)`                        — register a connected
//!     server's tools so the next `ai($prompt, auto_mcp => 1)` call
//!     auto-includes them
//!   * `mcp_detach_from_ai($h)`
//!   * `mcp_attached()`                              — list of attached IDs

use crate::error::PerlError;
use crate::value::PerlValue;
use indexmap::IndexMap;
use parking_lot::Mutex;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;

type Result<T> = std::result::Result<T, PerlError>;

enum McpTransport {
    Stdio {
        child: Child,
        stdin: ChildStdin,
        stdout: BufReader<ChildStdout>,
    },
    Http {
        url: String,
        bearer: Option<String>,
        agent: ureq::Agent,
        /// Some MCP servers issue a session-id on initialize; carry it
        /// back on subsequent requests as `mcp-session-id` header.
        session_id: Option<String>,
    },
}

struct McpHandle {
    name: String,
    transport: McpTransport,
    next_id: AtomicU64,
    closed: bool,
    /// Cached `tools/list` result so subsequent `mcp_tools` calls
    /// don't re-roundtrip.
    cached_tools: Option<Vec<serde_json::Value>>,
    cached_resources: Option<Vec<serde_json::Value>>,
    cached_prompts: Option<Vec<serde_json::Value>>,
}

static REGISTRY: OnceLock<Mutex<IndexMap<u64, Arc<Mutex<McpHandle>>>>> = OnceLock::new();
static NEXT_HID: AtomicU64 = AtomicU64::new(1);
static ATTACHED: OnceLock<Mutex<Vec<u64>>> = OnceLock::new();

fn registry() -> &'static Mutex<IndexMap<u64, Arc<Mutex<McpHandle>>>> {
    REGISTRY.get_or_init(|| Mutex::new(IndexMap::new()))
}

fn attached() -> &'static Mutex<Vec<u64>> {
    ATTACHED.get_or_init(|| Mutex::new(Vec::new()))
}

fn lookup(handle: &PerlValue, line: usize) -> Result<Arc<Mutex<McpHandle>>> {
    let map = handle
        .as_hash_map()
        .or_else(|| handle.as_hash_ref().map(|h| h.read().clone()))
        .ok_or_else(|| PerlError::runtime("mcp: handle must be a hashref", line))?;
    let id = map
        .get("__mcp_id__")
        .map(|v| v.to_int() as u64)
        .ok_or_else(|| PerlError::runtime("mcp: hashref missing `__mcp_id__`", line))?;
    registry()
        .lock()
        .get(&id)
        .cloned()
        .ok_or_else(|| PerlError::runtime(format!("mcp: handle id {} not found", id), line))
}

fn handle_id(v: &PerlValue) -> Option<u64> {
    let map = v
        .as_hash_map()
        .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))?;
    map.get("__mcp_id__").map(|v| v.to_int() as u64)
}

fn make_handle_value(id: u64, name: &str) -> PerlValue {
    let mut m = IndexMap::new();
    m.insert("__mcp_id__".to_string(), PerlValue::integer(id as i64));
    m.insert("__mcp__".to_string(), PerlValue::integer(1));
    m.insert("name".to_string(), PerlValue::string(name.to_string()));
    PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m)))
}

// ── mcp_connect ───────────────────────────────────────────────────────

pub(crate) fn mcp_connect(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let url = args.first().map(|v| v.to_string()).ok_or_else(|| {
        PerlError::runtime("mcp_connect: usage: mcp_connect(\"stdio:CMD ARGS\")", line)
    })?;
    let name = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| url.split(':').nth(1).unwrap_or("mcp").to_string());

    // HTTP transport — streamable HTTP MCP servers (Anthropic spec).
    if url.starts_with("http://") || url.starts_with("https://") {
        return mcp_connect_http(&url, &name, line);
    }

    let cmd_str = url.strip_prefix("stdio:").ok_or_else(|| {
        PerlError::runtime(
            format!(
                "mcp_connect: unsupported transport `{}` — only `stdio:` and `http(s)://` wired",
                url
            ),
            line,
        )
    })?;
    let parts = shell_split(cmd_str);
    if parts.is_empty() {
        return Err(PerlError::runtime(
            "mcp_connect: empty command after stdio:",
            line,
        ));
    }
    let mut cmd = Command::new(&parts[0]);
    cmd.args(&parts[1..]);
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|e| PerlError::runtime(format!("mcp_connect: spawn {}: {}", parts[0], e), line))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| PerlError::runtime("mcp_connect: missing stdin", line))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| PerlError::runtime("mcp_connect: missing stdout", line))?;
    let stdout = BufReader::new(stdout);

    let mut handle = McpHandle {
        name: name.clone(),
        transport: McpTransport::Stdio {
            child,
            stdin,
            stdout,
        },
        next_id: AtomicU64::new(1),
        closed: false,
        cached_tools: None,
        cached_resources: None,
        cached_prompts: None,
    };

    let init_id = handle.next_id.fetch_add(1, Ordering::Relaxed);
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": init_id,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "roots": { "listChanged": false },
                "sampling": {}
            },
            "clientInfo": { "name": "stryke", "version": "0.1.0" }
        }
    });
    if let McpTransport::Stdio { stdin, stdout, .. } = &mut handle.transport {
        rpc_send(stdin, &init).map_err(|e| {
            PerlError::runtime(format!("mcp_connect: send initialize: {}", e), line)
        })?;
        rpc_recv(stdout, init_id)
            .map_err(|e| PerlError::runtime(format!("mcp_connect: initialize: {}", e), line))?;
        let initialized = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        });
        rpc_send(stdin, &initialized).map_err(|e| {
            PerlError::runtime(format!("mcp_connect: send initialized: {}", e), line)
        })?;
    }

    let id = NEXT_HID.fetch_add(1, Ordering::Relaxed);
    registry().lock().insert(id, Arc::new(Mutex::new(handle)));
    Ok(make_handle_value(id, &name))
}

// ── mcp_tools / mcp_resources / mcp_prompts ──────────────────────────

pub(crate) fn mcp_tools(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let h = lookup(
        args.first()
            .ok_or_else(|| PerlError::runtime("mcp_tools: handle required", line))?,
        line,
    )?;
    let cached = h.lock().cached_tools.clone();
    let list = match cached {
        Some(t) => t,
        None => {
            let v = call_method(&h, "tools/list", serde_json::json!({}), line)?;
            let arr = v["tools"].as_array().cloned().unwrap_or_default();
            h.lock().cached_tools = Some(arr.clone());
            arr
        }
    };
    Ok(json_array_to_perl(&list))
}

pub(crate) fn mcp_resources(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let h = lookup(
        args.first()
            .ok_or_else(|| PerlError::runtime("mcp_resources: handle required", line))?,
        line,
    )?;
    let cached = h.lock().cached_resources.clone();
    let list = match cached {
        Some(r) => r,
        None => {
            let v = call_method(&h, "resources/list", serde_json::json!({}), line)?;
            let arr = v["resources"].as_array().cloned().unwrap_or_default();
            h.lock().cached_resources = Some(arr.clone());
            arr
        }
    };
    Ok(json_array_to_perl(&list))
}

pub(crate) fn mcp_prompts(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let h = lookup(
        args.first()
            .ok_or_else(|| PerlError::runtime("mcp_prompts: handle required", line))?,
        line,
    )?;
    let cached = h.lock().cached_prompts.clone();
    let list = match cached {
        Some(p) => p,
        None => {
            let v = call_method(&h, "prompts/list", serde_json::json!({}), line)?;
            let arr = v["prompts"].as_array().cloned().unwrap_or_default();
            h.lock().cached_prompts = Some(arr.clone());
            arr
        }
    };
    Ok(json_array_to_perl(&list))
}

// ── mcp_call ──────────────────────────────────────────────────────────

pub(crate) fn mcp_call(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let h = lookup(
        args.first()
            .ok_or_else(|| PerlError::runtime("mcp_call: handle required", line))?,
        line,
    )?;
    let tool_name = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("mcp_call: tool name required", line))?;
    let tool_args_v = args.get(2).cloned().unwrap_or(PerlValue::UNDEF);
    let arguments = perl_to_json(&tool_args_v);
    let v = call_method(
        &h,
        "tools/call",
        serde_json::json!({
            "name": tool_name,
            "arguments": arguments,
        }),
        line,
    )?;
    Ok(json_to_perl(&v))
}

pub(crate) fn mcp_resource(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let h = lookup(
        args.first()
            .ok_or_else(|| PerlError::runtime("mcp_resource: handle required", line))?,
        line,
    )?;
    let uri = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("mcp_resource: uri required", line))?;
    let v = call_method(
        &h,
        "resources/read",
        serde_json::json!({ "uri": uri }),
        line,
    )?;
    Ok(json_to_perl(&v))
}

pub(crate) fn mcp_prompt(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let h = lookup(
        args.first()
            .ok_or_else(|| PerlError::runtime("mcp_prompt: handle required", line))?,
        line,
    )?;
    let name = args
        .get(1)
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("mcp_prompt: name required", line))?;
    let prompt_args_v = args.get(2).cloned().unwrap_or(PerlValue::UNDEF);
    let arguments = perl_to_json(&prompt_args_v);
    let v = call_method(
        &h,
        "prompts/get",
        serde_json::json!({
            "name": name,
            "arguments": arguments,
        }),
        line,
    )?;
    Ok(json_to_perl(&v))
}

// ── mcp_close ─────────────────────────────────────────────────────────

pub(crate) fn mcp_close(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let h_v = args
        .first()
        .ok_or_else(|| PerlError::runtime("mcp_close: handle required", line))?;
    let h = lookup(h_v, line)?;
    let id = handle_id(h_v).unwrap_or(0);
    {
        let mut g = h.lock();
        if !g.closed {
            if let McpTransport::Stdio { child, .. } = &mut g.transport {
                let _ = child.kill();
                let _ = child.wait();
            }
            g.closed = true;
        }
    }
    if id != 0 {
        registry().lock().shift_remove(&id);
        attached().lock().retain(|x| *x != id);
    }
    Ok(PerlValue::UNDEF)
}

// ── Auto-attach to ai ─────────────────────────────────────────────────

pub(crate) fn mcp_attach_to_ai(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let h_v = args
        .first()
        .ok_or_else(|| PerlError::runtime("mcp_attach_to_ai: handle required", line))?;
    let id = handle_id(h_v)
        .ok_or_else(|| PerlError::runtime("mcp_attach_to_ai: handle id missing", line))?;
    let mut g = attached().lock();
    if !g.contains(&id) {
        g.push(id);
    }
    Ok(PerlValue::UNDEF)
}

pub(crate) fn mcp_detach_from_ai(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let h_v = args
        .first()
        .ok_or_else(|| PerlError::runtime("mcp_detach_from_ai: handle required", line))?;
    let id = handle_id(h_v)
        .ok_or_else(|| PerlError::runtime("mcp_detach_from_ai: handle id missing", line))?;
    attached().lock().retain(|x| *x != id);
    Ok(PerlValue::UNDEF)
}

pub(crate) fn mcp_attached(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let g = attached().lock();
    let items: Vec<PerlValue> = g
        .iter()
        .filter_map(|id| {
            let reg = registry().lock();
            reg.get(id).map(|h| {
                let g = h.lock();
                make_handle_value(*id, &g.name)
            })
        })
        .collect();
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        items,
    ))))
}

/// Return a list of `(handle_id, server_name, tool_spec)` for every
/// attached server's tools — used by the agent loop to auto-include
/// MCP tools alongside user-supplied ones.
pub(crate) fn collect_attached_tools(line: usize) -> Vec<AttachedTool> {
    let mut out = Vec::new();
    let ids: Vec<u64> = attached().lock().clone();
    for id in ids {
        let h = match registry().lock().get(&id).cloned() {
            Some(h) => h,
            None => continue,
        };
        let tools = h.lock().cached_tools.clone();
        let tools = match tools {
            Some(t) => t,
            None => match call_method(&h, "tools/list", serde_json::json!({}), line) {
                Ok(v) => {
                    let arr = v["tools"].as_array().cloned().unwrap_or_default();
                    h.lock().cached_tools = Some(arr.clone());
                    arr
                }
                Err(_) => continue,
            },
        };
        let server_name = h.lock().name.clone();
        for t in tools {
            out.push(AttachedTool {
                handle_id: id,
                server_name: server_name.clone(),
                spec: t,
            });
        }
    }
    out
}

pub struct AttachedTool {
    pub handle_id: u64,
    pub server_name: String,
    pub spec: serde_json::Value,
}

/// Invoke a tool on an attached MCP server by name. Used by the agent
/// loop's `invoke_tool` fallback when a tool isn't found among the
/// user-supplied list.
pub(crate) fn call_attached_tool(
    handle_id: u64,
    name: &str,
    arguments: serde_json::Value,
    line: usize,
) -> Result<serde_json::Value> {
    let h = registry().lock().get(&handle_id).cloned().ok_or_else(|| {
        PerlError::runtime(format!("mcp: attached handle {} gone", handle_id), line)
    })?;
    call_method(
        &h,
        "tools/call",
        serde_json::json!({
            "name": name,
            "arguments": arguments,
        }),
        line,
    )
}

// ── HTTP MCP transport ───────────────────────────────────────────────

fn mcp_connect_http(url: &str, name: &str, line: usize) -> Result<PerlValue> {
    let bearer = std::env::var("MCP_BEARER_TOKEN").ok();
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(30))
        .build();
    let mut handle = McpHandle {
        name: name.to_string(),
        transport: McpTransport::Http {
            url: url.to_string(),
            bearer,
            agent,
            session_id: None,
        },
        next_id: AtomicU64::new(1),
        closed: false,
        cached_tools: None,
        cached_resources: None,
        cached_prompts: None,
    };

    // initialize handshake.
    let init_id = handle.next_id.fetch_add(1, Ordering::Relaxed);
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": init_id,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": { "roots": { "listChanged": false }, "sampling": {} },
            "clientInfo": { "name": "stryke", "version": "0.1.0" }
        }
    });
    if let McpTransport::Http {
        url,
        bearer,
        agent,
        session_id,
    } = &mut handle.transport
    {
        let resp = http_rpc(agent, url, bearer.as_deref(), session_id, &init, line)?;
        let _ = resp;
        // Anthropic streamable-HTTP MCP returns mcp-session-id; carry
        // it on subsequent requests if present.
        // (rpc_send sets it via the helper.)
    }

    // notifications/initialized (no response expected for HTTP either).
    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
    });
    if let McpTransport::Http {
        url,
        bearer,
        agent,
        session_id,
    } = &mut handle.transport
    {
        // Best-effort post; ignore body.
        let mut req_builder = agent
            .post(url)
            .set("content-type", "application/json")
            .set("accept", "application/json, text/event-stream");
        if let Some(tok) = bearer {
            req_builder = req_builder.set("authorization", &format!("Bearer {}", tok));
        }
        if let Some(sid) = session_id {
            req_builder = req_builder.set("mcp-session-id", sid);
        }
        let _ = req_builder.send_json(initialized);
    }

    let id = NEXT_HID.fetch_add(1, Ordering::Relaxed);
    registry().lock().insert(id, Arc::new(Mutex::new(handle)));
    Ok(make_handle_value(id, name))
}

/// One JSON-RPC round-trip over HTTP. Handles the streamable-HTTP
/// shape: server responds with either `application/json` (single
/// envelope) or `text/event-stream` (chunked SSE; we collect the
/// final response message only).
fn http_rpc(
    agent: &ureq::Agent,
    url: &str,
    bearer: Option<&str>,
    session_id: &mut Option<String>,
    req: &serde_json::Value,
    line: usize,
) -> Result<serde_json::Value> {
    let want_id = req.get("id").and_then(|v| v.as_u64());
    let mut req_builder = agent
        .post(url)
        .set("content-type", "application/json")
        .set("accept", "application/json, text/event-stream");
    if let Some(tok) = bearer {
        req_builder = req_builder.set("authorization", &format!("Bearer {}", tok));
    }
    if let Some(sid) = session_id.as_deref() {
        req_builder = req_builder.set("mcp-session-id", sid);
    }
    let resp = req_builder
        .send_json(req.clone())
        .map_err(|e| PerlError::runtime(format!("mcp http: {}", e), line))?;

    // Stash session id if server set one.
    if let Some(sid) = resp.header("mcp-session-id") {
        *session_id = Some(sid.to_string());
    }

    let ct = resp
        .header("content-type")
        .unwrap_or("application/json")
        .to_string();

    if ct.starts_with("text/event-stream") {
        // Walk SSE looking for a `data:` line whose JSON has the
        // matching id. Stop at the first match.
        use std::io::BufRead;
        let body = std::io::BufReader::new(resp.into_reader());
        for line_io in body.lines() {
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
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(payload) {
                if v.get("id").and_then(|x| x.as_u64()) == want_id {
                    return Ok(v);
                }
            }
        }
        Err(PerlError::runtime(
            "mcp http: SSE ended without matching response",
            line,
        ))
    } else {
        // Plain JSON response.
        let v: serde_json::Value = resp
            .into_json()
            .map_err(|e| PerlError::runtime(format!("mcp http: decode: {}", e), line))?;
        Ok(v)
    }
}

// ── JSON-RPC plumbing ─────────────────────────────────────────────────

fn call_method(
    h: &Arc<Mutex<McpHandle>>,
    method: &str,
    params: serde_json::Value,
    line: usize,
) -> Result<serde_json::Value> {
    let mut g = h.lock();
    if g.closed {
        return Err(PerlError::runtime(
            format!("mcp: handle is closed for method {}", method),
            line,
        ));
    }
    let id = g.next_id.fetch_add(1, Ordering::Relaxed);
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    let resp = match &mut g.transport {
        McpTransport::Stdio { stdin, stdout, .. } => {
            rpc_send(stdin, &req)
                .map_err(|e| PerlError::runtime(format!("mcp: {}: send: {}", method, e), line))?;
            rpc_recv(stdout, id)
                .map_err(|e| PerlError::runtime(format!("mcp: {}: recv: {}", method, e), line))?
        }
        McpTransport::Http {
            url,
            bearer,
            agent,
            session_id,
        } => http_rpc(agent, url, bearer.as_deref(), session_id, &req, line)?,
    };
    if let Some(err) = resp.get("error") {
        return Err(PerlError::runtime(
            format!("mcp: {} returned error: {}", method, err),
            line,
        ));
    }
    Ok(resp["result"].clone())
}

fn rpc_send(stdin: &mut ChildStdin, msg: &serde_json::Value) -> std::io::Result<()> {
    let line = serde_json::to_string(msg)?;
    stdin.write_all(line.as_bytes())?;
    stdin.write_all(b"\n")?;
    stdin.flush()
}

fn rpc_recv(
    stdout: &mut BufReader<ChildStdout>,
    want_id: u64,
) -> std::io::Result<serde_json::Value> {
    // Read line-delimited JSON-RPC. Skip notifications (no `id`) and
    // any responses with a different id.
    loop {
        let mut line = String::new();
        let n = stdout.read_line(&mut line)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "mcp: server closed stdout",
            ));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(trimmed).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}: {}", e, trimmed),
            )
        })?;
        if let Some(id) = v.get("id").and_then(|x| x.as_u64()) {
            if id == want_id {
                return Ok(v);
            }
        }
        // notification or unrelated response — keep reading.
    }
}

// ── JSON ↔ PerlValue helpers (duplicated locally to avoid pulling in
// ai.rs internals). ───────────────────────────────────────────────────

fn json_array_to_perl(arr: &[serde_json::Value]) -> PerlValue {
    let items: Vec<PerlValue> = arr.iter().map(json_to_perl).collect();
    PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(items)))
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
        serde_json::Value::Array(arr) => json_array_to_perl(arr),
        serde_json::Value::Object(obj) => {
            let mut m = IndexMap::new();
            for (k, v) in obj {
                m.insert(k.clone(), json_to_perl(v));
            }
            PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(m)))
        }
    }
}

fn perl_to_json(v: &PerlValue) -> serde_json::Value {
    if v.is_undef() {
        return serde_json::Value::Null;
    }
    if let Some(map) = v
        .as_hash_map()
        .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
    {
        let mut out = serde_json::Map::new();
        for (k, val) in map {
            out.insert(k, perl_to_json(&val));
        }
        return serde_json::Value::Object(out);
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

// ── Programmatic MCP server (no parser DSL) ───────────────────────────
//
// `mcp_server_start("filesystem", +{
//     tools => [
//         +{ name => "read_file", description => "...",
//            parameters => +{ path => "string" },
//            run => sub { slurp $_[0]->{path} } },
//         ...
//     ]
// })` runs a stdio JSON-RPC loop on stdin/stdout that exposes the
// tools list and dispatches calls back into stryke. Used for
// `s_web build --mcp-server`-style usage where the binary IS the
// MCP server.
//
// Caveat: the server takes over stdin/stdout for the rest of the
// process. Calling this from a web app or CLI prints corrupts the
// binary's interactive state — only call it from a dedicated MCP
// server entry point.

use crate::interpreter::{FlowOrError, Interpreter, WantarrayCtx};
use crate::value::{PerlSub, PerlValue as PV};

struct ServerTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
    run_sub: Arc<PerlSub>,
}

impl crate::interpreter::Interpreter {
    pub(crate) fn mcp_server_start(
        &mut self,
        args: &[PerlValue],
        line: usize,
    ) -> Result<PerlValue> {
        let name = args.first().map(|v| v.to_string()).ok_or_else(|| {
            PerlError::runtime(
                "mcp_server_start: usage: mcp_server_start(\"name\", +{tools => [...]})",
                line,
            )
        })?;
        let opts_v = args.get(1).cloned().unwrap_or(PerlValue::UNDEF);
        let opts = opts_v
            .as_hash_map()
            .or_else(|| opts_v.as_hash_ref().map(|h| h.read().clone()))
            .unwrap_or_default();
        let tools_v = opts.get("tools").cloned().unwrap_or(PerlValue::UNDEF);
        let tools_list = tools_v
            .as_array_ref()
            .map(|a| a.read().clone())
            .unwrap_or_else(|| tools_v.to_list());

        let mut tools: Vec<ServerTool> = Vec::with_capacity(tools_list.len());
        for t in &tools_list {
            tools.push(compile_server_tool(t, line)?);
        }

        run_stdio_server(self, &name, &tools, line)
    }
}

pub(crate) fn mcp_server_start_dispatch(
    interp: &mut Interpreter,
    args: &[PerlValue],
    line: usize,
) -> Result<PerlValue> {
    interp.mcp_server_start(args, line)
}

/// `mcp_serve_registered_tools($server_name)` — convert every tool
/// registered via `ai_register_tool` (or `tool fn` desugar) into the
/// MCP server schema and start the stdio JSON-RPC loop. This is the
/// runtime side of `s build --mcp-server` / `stryke --mcp-server` —
/// both flags eventually call this after the user's script has had a
/// chance to register its tools.
pub(crate) fn mcp_serve_registered_tools(
    interp: &mut Interpreter,
    args: &[PerlValue],
    line: usize,
) -> Result<PerlValue> {
    let name = args
        .first()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "stryke-mcp".to_string());
    let registered = crate::ai::registered_tools().lock().clone();
    if registered.is_empty() {
        return Err(PerlError::runtime(
            "mcp_serve_registered_tools: no tools registered — call `tool fn name { ... }` or `ai_register_tool(...)` before serving",
            line,
        ));
    }
    let tools: Vec<ServerTool> = registered
        .into_iter()
        .map(|rt| ServerTool {
            input_schema: server_params_to_schema(&rt.parameters),
            name: rt.name,
            description: rt.description,
            run_sub: rt.run_sub,
        })
        .collect();
    run_stdio_server(interp, &name, &tools, line)
}

pub(crate) fn mcp_serve_registered_tools_dispatch(
    interp: &mut Interpreter,
    args: &[PerlValue],
    line: usize,
) -> Result<PerlValue> {
    mcp_serve_registered_tools(interp, args, line)
}

fn compile_server_tool(v: &PerlValue, line: usize) -> Result<ServerTool> {
    let map = v
        .as_hash_map()
        .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
        .ok_or_else(|| {
            PerlError::runtime(
                "mcp_server_start: each tool must be +{name, description, parameters, run}",
                line,
            )
        })?;
    let name = map
        .get("name")
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("mcp_server_start: tool missing name", line))?;
    let description = map
        .get("description")
        .map(|v| v.to_string())
        .unwrap_or_default();
    let parameters = map.get("parameters").cloned().unwrap_or(PerlValue::UNDEF);
    let run_v = map.get("run").ok_or_else(|| {
        PerlError::runtime(
            format!("mcp_server_start: tool `{}` missing run coderef", name),
            line,
        )
    })?;
    let run_sub = run_v.as_code_ref().ok_or_else(|| {
        PerlError::runtime(
            format!("mcp_server_start: tool `{}` run must be a coderef", name),
            line,
        )
    })?;
    Ok(ServerTool {
        name,
        description,
        input_schema: server_params_to_schema(&parameters),
        run_sub,
    })
}

fn server_params_to_schema(v: &PerlValue) -> serde_json::Value {
    let map = match v
        .as_hash_map()
        .or_else(|| v.as_hash_ref().map(|h| h.read().clone()))
    {
        Some(m) => m,
        None => return serde_json::json!({"type": "object", "properties": {}}),
    };
    if map.contains_key("type") && map.contains_key("properties") {
        // Already a JSON Schema.
        let mut out = serde_json::Map::new();
        for (k, val) in &map {
            out.insert(k.clone(), perl_to_json(val));
        }
        return serde_json::Value::Object(out);
    }
    let mut props = serde_json::Map::new();
    let mut required: Vec<serde_json::Value> = Vec::new();
    for (k, ty) in &map {
        let t = ty.to_string();
        let json_ty = match t.as_str() {
            "int" | "integer" | "Int" => "integer",
            "number" | "float" | "Float" | "Num" => "number",
            "bool" | "boolean" | "Bool" => "boolean",
            "array" | "list" => "array",
            _ => "string",
        };
        let mut p = serde_json::Map::new();
        p.insert("type".into(), serde_json::Value::String(json_ty.into()));
        props.insert(k.clone(), serde_json::Value::Object(p));
        required.push(serde_json::Value::String(k.clone()));
    }
    serde_json::json!({
        "type": "object",
        "properties": props,
        "required": required,
    })
}

fn run_stdio_server(
    interp: &mut Interpreter,
    name: &str,
    tools: &[ServerTool],
    line: usize,
) -> Result<PerlValue> {
    use std::io::{stdin, stdout, BufRead, BufWriter, Write};

    let stdin = stdin();
    let mut out = BufWriter::new(stdout().lock());
    let mut buf = String::new();
    let mut reader = stdin.lock();

    loop {
        buf.clear();
        let n = reader
            .read_line(&mut buf)
            .map_err(|e| PerlError::runtime(format!("mcp_server: read: {}", e), line))?;
        if n == 0 {
            break;
        }
        let trimmed = buf.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = req.get("id").cloned();
        let method = req["method"].as_str().unwrap_or("").to_string();
        let params = req.get("params").cloned().unwrap_or(serde_json::json!({}));

        let result = match method.as_str() {
            "initialize" => Ok(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": { "name": name, "version": "0.1.0" },
                "capabilities": { "tools": {} },
            })),
            "notifications/initialized" => {
                continue;
            }
            "tools/list" => {
                let arr: Vec<serde_json::Value> = tools
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "name": t.name,
                            "description": t.description,
                            "inputSchema": t.input_schema.clone(),
                        })
                    })
                    .collect();
                Ok(serde_json::json!({ "tools": arr }))
            }
            "tools/call" => {
                let tool_name = params["name"].as_str().unwrap_or("").to_string();
                let args_json = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or(serde_json::json!({}));
                match tools.iter().find(|t| t.name == tool_name) {
                    Some(t) => {
                        let arg_perl = json_to_perl(&args_json);
                        match interp.call_sub(
                            &t.run_sub,
                            vec![arg_perl],
                            WantarrayCtx::Scalar,
                            line,
                        ) {
                            Ok(v) => {
                                let s = match perl_to_json(&v) {
                                    serde_json::Value::String(s) => s,
                                    other => other.to_string(),
                                };
                                Ok(serde_json::json!({
                                    "content": [{ "type": "text", "text": s }]
                                }))
                            }
                            Err(FlowOrError::Flow(_)) => Ok(serde_json::json!({
                                "content": [{ "type": "text", "text": "" }]
                            })),
                            Err(FlowOrError::Error(e)) => {
                                Err(format!("tool `{}`: {}", tool_name, e))
                            }
                        }
                    }
                    None => Err(format!("unknown tool: {}", tool_name)),
                }
            }
            other => Err(format!("method not implemented: {}", other)),
        };

        let envelope = match result {
            Ok(r) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": r,
            }),
            Err(msg) => serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32000, "message": msg },
            }),
        };
        let _ = writeln!(out, "{}", envelope);
        let _ = out.flush();
    }
    let _ = PV::UNDEF;
    Ok(PerlValue::UNDEF)
}

// Minimal shell-split for `stdio:CMD ARGS` — same shape as perl_pty.
fn shell_split(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut esc = false;
    for c in s.chars() {
        if esc {
            cur.push(c);
            esc = false;
            continue;
        }
        if c == '\\' && quote != Some('\'') {
            esc = true;
            continue;
        }
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                cur.push(c);
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            c if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            other => cur.push(other),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
