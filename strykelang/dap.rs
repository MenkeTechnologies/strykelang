//! Debug Adapter Protocol (DAP) server for stryke.
//!
//! Started via `st --dap [file.stk]`. Speaks DAP over stdio using the same
//! `Content-Length`-framed JSON-RPC as LSP. Wraps [`crate::debugger::Debugger`]
//! so existing breakpoint / step / variable inspection logic is reused unchanged.
//!
//! ## Threading model
//!
//! * **Main thread** spawns a reader thread, then once the client sends
//!   `launch`, runs the VM in-place.
//! * **Reader thread** parses incoming DAP messages from stdin, mutates the
//!   shared [`DapShared`] state, and signals the VM thread via condvar when
//!   resuming.
//! * **VM thread** = main thread after `launch`. On each line stop it locks the
//!   shared state, captures a snapshot, emits a `stopped` event, and condvar-
//!   waits for a resume command.
//!
//! ## Stdout
//!
//! Both threads write to stdout through `DapShared.writer` (a `Mutex<Stdout>`).
//! All outgoing messages get a monotonic `seq`.
//!
//! ## What's supported (v1)
//!
//! * `initialize` / `configurationDone` / `disconnect` / `terminate`
//! * `launch` (with `program` + `noDebug` + `args` + `cwd`)
//! * `setBreakpoints` (line breakpoints; conditions ignored for now)
//! * `setFunctionBreakpoints` (sub-name breakpoints)
//! * `threads` (single thread)
//! * `stackTrace` / `scopes` / `variables` (locals + globals from snapshot)
//! * `continue` / `next` / `stepIn` / `stepOut` / `pause`
//! * `evaluate` (REPL — uses `Debugger::print_variable` for simple vars)
//!
//! Not yet: conditional / hit-count breakpoints, exception breakpoints, watch
//! expressions, sub-line stepping, remote attach, child process tracking.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use crate::debugger::DebugAction;

const MAX_VAR_REPR: usize = 200;

// ─── DAP wire types ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DapHeader {
    seq: u64,
    #[serde(rename = "type")]
    msg_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DapRequest {
    seq: u64,
    #[serde(rename = "type")]
    msg_type: String,
    command: String,
    #[serde(default)]
    arguments: Value,
}

// ─── Shared state ───────────────────────────────────────────────────────────

/// Snapshot of where the VM is paused. Captured by the VM thread *before*
/// blocking on the condvar so the reader thread can answer `stackTrace` /
/// `scopes` / `variables` requests without touching the VM.
#[derive(Default, Clone)]
pub(crate) struct PauseSnapshot {
    pub file: String,
    pub line: usize,
    pub reason: String, // "breakpoint" | "step" | "pause" | "entry"
    pub frames: Vec<FrameSnap>,
    pub locals: Vec<VarSnap>,
    pub globals: Vec<VarSnap>,
    /// Container varRef → children. Built by [`capture_locals`] by walking
    /// nested arrays / hashes recursively. The DAP `variables` request reads
    /// from this map for any non-scope varRef.
    pub var_ref_map: HashMap<u32, Vec<VarChild>>,
}

#[derive(Clone)]
pub(crate) struct FrameSnap {
    pub name: String,
    pub file: String,
    pub line: usize,
}

#[derive(Clone)]
pub(crate) struct VarSnap {
    pub name: String, // includes sigil: "$x", "@arr", "%h"
    pub repr: String,
    pub kind: String, // "scalar" | "array" | "hash"
    /// 0 = leaf. Non-zero = variablesReference the client should use to
    /// expand this entry into its key/value children. Resolved through
    /// [`PauseSnapshot::var_ref_map`].
    pub var_ref: u32,
}

/// One child row inside an expanded container, used recursively.
#[derive(Clone)]
pub(crate) struct VarChild {
    pub name: String,
    pub repr: String,
    /// 0 = leaf scalar/string. Non-zero = container with further children
    /// (looked up via [`PauseSnapshot::var_ref_map`]).
    pub var_ref: u32,
}

struct SharedInner {
    pending_action: Option<DebugAction>,
    is_paused: bool,
    snapshot: PauseSnapshot,
    pause_request: bool, // client asked us to pause asap
}

pub struct DapShared {
    inner: Mutex<SharedInner>,
    cv: Condvar,
    seq: AtomicU64,
    writer: Mutex<Box<dyn Write + Send>>,
    pub configuration_done: AtomicBool,
    pub disconnected: AtomicBool,
}

impl DapShared {
    fn new(writer: Box<dyn Write + Send>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(SharedInner {
                pending_action: None,
                is_paused: false,
                snapshot: PauseSnapshot::default(),
                pause_request: false,
                }),
            cv: Condvar::new(),
            seq: AtomicU64::new(1),
            writer: Mutex::new(writer),
            configuration_done: AtomicBool::new(false),
            disconnected: AtomicBool::new(false),
        })
    }

    /// Called by the VM thread when it has detected a stop. Captures the
    /// snapshot, sends a `stopped` event, then condvar-waits for resume.
    pub fn pause(&self, snap: PauseSnapshot) -> DebugAction {
        // Flush stdout/stderr so any `p`/`print`/`say` output the user
        // produced since the last pause is visible in the Console BEFORE the
        // suspend UI shows. Without this, stdout is block-buffered (piped to
        // OSProcessHandler) and `print "x"` followed by a breakpoint leaves
        // the Console looking empty until the buffer fills or the program
        // exits.
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let _ = std::io::Write::flush(&mut std::io::stderr());
        {
            let mut s = self.inner.lock().expect("dap lock");
            s.snapshot = snap.clone();
            s.is_paused = true;
            s.pending_action = None;
            s.pause_request = false;
        }
        self.emit_event(
            "stopped",
            json!({
                "reason": snap.reason,
                "threadId": 1,
                "allThreadsStopped": true,
                "preserveFocusHint": false,
                "description": snap.reason,
                "text": format!("{}:{}", snap.file, snap.line),
            }),
        );
        let mut guard = self.inner.lock().expect("dap lock");
        while guard.pending_action.is_none() && !self.disconnected.load(Ordering::SeqCst) {
            guard = self.cv.wait(guard).expect("dap cv");
        }
        let action = guard.pending_action.take().unwrap_or(DebugAction::Continue);
        guard.is_paused = false;
        action
    }

    pub fn was_disconnected(&self) -> bool {
        self.disconnected.load(Ordering::SeqCst)
    }

    pub fn want_pause(&self) -> bool {
        self.inner
            .lock()
            .map(|g| g.pause_request)
            .unwrap_or(false)
    }

    fn resume_with(&self, action: DebugAction) {
        let mut g = self.inner.lock().expect("dap lock");
        g.pending_action = Some(action);
        self.cv.notify_all();
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }

    fn write_message(&self, body: Value) {
        let s = serde_json::to_string(&body).unwrap_or_else(|_| "{}".to_string());
        let mut w = self.writer.lock().expect("dap writer");
        let _ = write!(w, "Content-Length: {}\r\n\r\n{}", s.len(), s);
        let _ = w.flush();
    }

    fn emit_response(&self, req: &DapRequest, success: bool, body: Value) {
        let seq = self.next_seq();
        let msg = json!({
            "seq": seq,
            "type": "response",
            "request_seq": req.seq,
            "success": success,
            "command": req.command,
            "body": body,
        });
        self.write_message(msg);
    }

    fn emit_error(&self, req: &DapRequest, message: &str) {
        let seq = self.next_seq();
        let msg = json!({
            "seq": seq,
            "type": "response",
            "request_seq": req.seq,
            "success": false,
            "command": req.command,
            "message": message,
            "body": { "error": { "format": message } },
        });
        self.write_message(msg);
    }

    pub fn emit_event(&self, event: &str, body: Value) {
        let seq = self.next_seq();
        let msg = json!({
            "seq": seq,
            "type": "event",
            "event": event,
            "body": body,
        });
        self.write_message(msg);
    }
}

// ─── Reader / dispatch ──────────────────────────────────────────────────────

/// Spawn the DAP reader thread reading from an arbitrary `Read` source
/// (stdin for stdio mode; a TCP socket for socket mode). Returns a join
/// handle and a "launch parameters" channel that the main thread blocks on
/// before starting the VM.
pub fn spawn_reader_with_input(
    shared: Arc<DapShared>,
    bp_state: Arc<Mutex<BreakpointState>>,
    input: Box<dyn Read + Send>,
) -> (thread::JoinHandle<()>, std::sync::mpsc::Receiver<LaunchParams>) {
    let (tx, rx) = std::sync::mpsc::channel::<LaunchParams>();
    let h = thread::spawn(move || {
        let mut reader = BufReader::new(input);
        loop {
            let body = match read_message(&mut reader) {
                Ok(Some(b)) => b,
                Ok(None) => break,
                Err(_) => break,
            };
            let req: DapRequest = match serde_json::from_slice(&body) {
                Ok(r) => r,
                Err(_) => continue,
            };
            handle_request(&shared, &bp_state, &tx, &req);
            if shared.was_disconnected() {
                break;
            }
        }
        // Stream closed → release any waiting VM thread
        shared.resume_with(DebugAction::Quit);
        shared.disconnected.store(true, Ordering::SeqCst);
    });
    (h, rx)
}

fn read_message<R: Read>(reader: &mut BufReader<R>) -> io::Result<Option<Vec<u8>>> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(|c| c == '\r' || c == '\n');
        if line.is_empty() {
            break;
        }
        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().ok();
        }
    }
    let Some(len) = content_length else {
        return Ok(Some(Vec::new()));
    };
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

// ─── Launch + breakpoint state ──────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct LaunchParams {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub no_debug: bool,
    pub stop_on_entry: bool,
    pub interpreter_args: Vec<String>,
    pub no_interop: bool,
}

#[derive(Debug, Default)]
pub struct BreakpointState {
    /// Line breakpoints keyed by absolute file path → set of lines.
    pub line_breakpoints: HashMap<String, Vec<usize>>,
    pub function_breakpoints: Vec<String>,
    /// Step mode set by the reader thread; consumed by `Debugger::prompt`
    /// after it wakes from condvar.
    pub pending_step: Option<StepKind>,
}

// ─── Request handlers ───────────────────────────────────────────────────────

fn handle_request(
    shared: &Arc<DapShared>,
    bp_state: &Arc<Mutex<BreakpointState>>,
    launch_tx: &std::sync::mpsc::Sender<LaunchParams>,
    req: &DapRequest,
) {
    match req.command.as_str() {
        "initialize" => {
            shared.emit_response(
                req,
                true,
                json!({
                    "supportsConfigurationDoneRequest": true,
                    "supportsFunctionBreakpoints": true,
                    "supportsConditionalBreakpoints": false,
                    "supportsHitConditionalBreakpoints": false,
                    "supportsEvaluateForHovers": true,
                    "supportsTerminateRequest": true,
                    "supportsRestartRequest": false,
                    "supportsStepInTargetsRequest": false,
                    "supportsSetVariable": false,
                    "supportsCompletionsRequest": false,
                    "supportsLoadedSourcesRequest": false,
                    "supportsExceptionInfoRequest": false,
                    "supportsExceptionOptions": false,
                    "supportsValueFormattingOptions": false,
                    "supportsLogPoints": false,
                    "supportsModulesRequest": false,
                    "supportsRestartFrame": false,
                    "supportsGotoTargetsRequest": false,
                    "supportsStepBack": false,
                }),
            );
            shared.emit_event("initialized", json!({}));
        }
        "setBreakpoints" => {
            let path = req
                .arguments
                .get("source")
                .and_then(|s| s.get("path"))
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            let bps = req
                .arguments
                .get("breakpoints")
                .and_then(|b| b.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|b| b.get("line").and_then(|l| l.as_u64()))
                        .map(|l| l as usize)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            {
                let mut bp = bp_state.lock().expect("bp lock");
                bp.line_breakpoints.insert(path.clone(), bps.clone());
            }
            let verified: Vec<Value> = bps
                .iter()
                .map(|l| {
                    json!({
                        "verified": true,
                        "line": *l,
                        "source": { "path": path }
                    })
                })
                .collect();
            shared.emit_response(req, true, json!({ "breakpoints": verified }));
        }
        "setFunctionBreakpoints" => {
            let fbps: Vec<String> = req
                .arguments
                .get("breakpoints")
                .and_then(|b| b.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|b| b.get("name").and_then(|n| n.as_str()).map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            {
                let mut bp = bp_state.lock().expect("bp lock");
                bp.function_breakpoints = fbps.clone();
            }
            let body: Vec<Value> = fbps.iter().map(|_| json!({ "verified": true })).collect();
            shared.emit_response(req, true, json!({ "breakpoints": body }));
        }
        "setExceptionBreakpoints" => {
            shared.emit_response(req, true, json!({ "breakpoints": [] }));
        }
        "configurationDone" => {
            shared.configuration_done.store(true, Ordering::SeqCst);
            shared.emit_response(req, true, json!({}));
        }
        "launch" => {
            let lp = LaunchParams {
                program: req
                    .arguments
                    .get("program")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                args: req
                    .arguments
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                cwd: req
                    .arguments
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                no_debug: req
                    .arguments
                    .get("noDebug")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                stop_on_entry: req
                    .arguments
                    .get("stopOnEntry")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                interpreter_args: req
                    .arguments
                    .get("interpreterArgs")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|s| s.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                no_interop: req
                    .arguments
                    .get("noInterop")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            };
            let _ = launch_tx.send(lp);
            shared.emit_response(req, true, json!({}));
        }
        "threads" => {
            shared.emit_response(
                req,
                true,
                json!({
                    "threads": [
                        { "id": 1, "name": "main" }
                    ]
                }),
            );
        }
        "stackTrace" => {
            let snap = shared.inner.lock().expect("dap lock").snapshot.clone();
            let frames: Vec<Value> = snap
                .frames
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    json!({
                        "id": i + 1,
                        "name": f.name,
                        "line": f.line,
                        "column": 1,
                        "source": { "name": leaf(&f.file), "path": f.file }
                    })
                })
                .collect();
            shared.emit_response(
                req,
                true,
                json!({
                    "stackFrames": frames,
                    "totalFrames": frames.len(),
                }),
            );
        }
        "scopes" => {
            shared.emit_response(
                req,
                true,
                json!({
                    "scopes": [
                        { "name": "Locals",  "variablesReference": 1000, "expensive": false },
                        { "name": "Globals", "variablesReference": 2000, "expensive": false }
                    ]
                }),
            );
        }
        "variables" => {
            let var_ref = req
                .arguments
                .get("variablesReference")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let snap = shared.inner.lock().expect("dap lock").snapshot.clone();
            let vars: Vec<Value> = match var_ref {
                1000 => snap
                    .locals
                    .iter()
                    .map(|v| json!({
                        "name": v.name,
                        "value": v.repr,
                        "type": v.kind,
                        "variablesReference": v.var_ref,
                    }))
                    .collect(),
                2000 => snap
                    .globals
                    .iter()
                    .map(|v| json!({
                        "name": v.name,
                        "value": v.repr,
                        "type": v.kind,
                        "variablesReference": v.var_ref,
                    }))
                    .collect(),
                _ => snap
                    .var_ref_map
                    .get(&var_ref)
                    .map(|children| {
                        children
                            .iter()
                            .map(|c| json!({
                                "name": c.name,
                                "value": c.repr,
                                "type": "",
                                "variablesReference": c.var_ref,
                            }))
                            .collect::<Vec<Value>>()
                    })
                    .unwrap_or_default(),
            };
            shared.emit_response(req, true, json!({ "variables": vars }));
        }
        "continue" => {
            shared.resume_with(DebugAction::Continue);
            shared.emit_response(req, true, json!({ "allThreadsContinued": true }));
        }
        "next" => {
            // CRITICAL ORDER: set pending_step BEFORE resume_with. The VM
            // thread reads `pending_step` immediately after cv.wait returns,
            // so the step kind must be in place before we notify the cv.
            request_step(bp_state, StepKind::Over);
            shared.resume_with(DebugAction::Continue);
            shared.emit_response(req, true, json!({}));
        }
        "stepIn" => {
            request_step(bp_state, StepKind::Into);
            shared.resume_with(DebugAction::Continue);
            shared.emit_response(req, true, json!({}));
        }
        "stepOut" => {
            request_step(bp_state, StepKind::Out);
            shared.resume_with(DebugAction::Continue);
            shared.emit_response(req, true, json!({}));
        }
        "pause" => {
            let mut g = shared.inner.lock().expect("dap lock");
            g.pause_request = true;
            shared.emit_response(req, true, json!({}));
        }
        "evaluate" => {
            let expr = req
                .arguments
                .get("expression")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let snap = shared.inner.lock().expect("dap lock").snapshot.clone();
            let result = evaluate_expression(&expr, &snap);
            shared.emit_response(
                req,
                true,
                json!({
                    "result": result,
                    "variablesReference": 0,
                }),
            );
        }
        "terminate" | "disconnect" => {
            shared.disconnected.store(true, Ordering::SeqCst);
            shared.resume_with(DebugAction::Quit);
            shared.emit_response(req, true, json!({}));
            shared.emit_event("terminated", json!({}));
        }
        _ => {
            shared.emit_response(req, true, json!({}));
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum StepKind {
    Over,
    Into,
    Out,
}

/// Step requests are routed to the VM via the shared `BreakpointState`. The
/// debugger picks them up the next time it wakes from condvar.
fn request_step(bp_state: &Arc<Mutex<BreakpointState>>, kind: StepKind) {
    if let Ok(mut g) = bp_state.lock() {
        g.pending_step = Some(kind);
    }
}

fn leaf(path: &str) -> String {
    path.rsplit_once('/').map(|(_, t)| t.to_string()).unwrap_or_else(|| path.to_string())
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

/// Evaluate a debugger expression.
///
/// 1. **Name lookup** — if the expression is literally a captured variable
///    name (`$foo`, `@bar`, `%baz`), return its repr from the snapshot. Fast,
///    no subprocess.
/// 2. **Expression evaluation** — for anything else (`55 + 3`, `len(@arr)`,
///    `sqrt(2)`, etc.), spawn a fresh `st -e 'p (<expr>)'` and return the
///    captured stdout. Cannot reference the paused frame's local variables
///    (no scope injection yet), but constant/library expressions work.
fn evaluate_expression(expr: &str, snap: &PauseSnapshot) -> String {
    let needle = expr.trim();
    if needle.is_empty() {
        return String::new();
    }
    // (1) Direct name lookup
    for src in [&snap.locals, &snap.globals] {
        for v in src.iter() {
            if v.name == needle {
                return v.repr.clone();
            }
        }
    }
    // (2) Fall back to subprocess evaluation
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return format!("eval: cannot locate stryke binary: {e}"),
    };
    // Build a prelude that re-declares the captured user scalars so expressions
    // like `$a * $b` see the paused-frame values. Skip stryke built-ins; their
    // names would shadow real specials and `my $!` etc. don't compile.
    let mut prelude = String::new();
    for v in &snap.locals {
        if v.kind != "scalar" { continue; }
        if !v.name.starts_with('$') { continue; }
        let bare = &v.name[1..];
        if bare.is_empty() || is_builtin_like(bare) { continue; }
        // The repr is already in stryke-source form (e.g. `42`, `"hello"`, `undef`)
        // produced by `crate::debugger::format_value`.
        let repr = if v.repr.is_empty() { "undef" } else { v.repr.as_str() };
        prelude.push_str(&format!("my ${bare} = {repr};\n"));
    }
    // Wrap the expression so its value is printed. `p (EXPR)` prints any
    // scalar/list/hash representation.
    let wrapped = format!("{prelude}p ({needle})");
    let output = std::process::Command::new(&exe)
        .arg("-e")
        .arg(&wrapped)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output();
    match output {
        Ok(out) => {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout)
                    .trim_end_matches('\n')
                    .to_string();
                if s.is_empty() { "(no output)".to_string() } else { s }
            } else {
                let err = String::from_utf8_lossy(&out.stderr);
                let msg = err.lines().next().unwrap_or("").trim();
                format!("error: {msg}")
            }
        }
        Err(e) => format!("eval spawn failed: {e}"),
    }
}

// ─── Snapshot capture helpers ───────────────────────────────────────────────

/// Base variablesReference for user-defined and built-in arrays/hashes. The
/// scopes themselves use 1000 (Locals) and 2000 (Globals), so we start
/// container refs at 10_000 and increment per container. Refs are stable
/// within a single pause; reset on each `pause()`.
const CONTAINER_REF_BASE: u32 = 10_000;

/// Walker state shared across the recursive capture.
struct CaptureCtx<'a> {
    next_ref: u32,
    map: &'a mut HashMap<u32, Vec<VarChild>>,
}

impl<'a> CaptureCtx<'a> {
    fn alloc_ref(&mut self) -> u32 {
        let r = self.next_ref;
        self.next_ref += 1;
        r
    }
}

/// Maximum recursion depth for nested hash/array drill-down. Cyclic refs
/// will bottom out; deeper than this is shown as the inline `format_value`
/// repr without an expand triangle.
const MAX_VAR_DEPTH: u32 = 12;

/// Build a child row for one value (scalar, array, or hash). For containers
/// at depth < [`MAX_VAR_DEPTH`] this allocates a varRef and recursively
/// populates [`CaptureCtx::map`] so the IDE can expand it.
fn build_child(name: String, value: &crate::value::StrykeValue, depth: u32, ctx: &mut CaptureCtx) -> VarChild {
    // Opaque sketch objects (TDigest, Bloom, HLL, CMS, TopK, ...) — render a
    // useful summary instead of the bare type name, and make the row
    // expandable so the user can drill into the actual stats (count, p50,
    // p99, fpr, ...). Falls through to the generic scalar/container paths
    // when the value is not a sketch.
    if let Some(rich) = try_sketch_child(&name, value, depth, ctx) {
        return rich;
    }
    // Hash or hashref — handle both `HeapObject::Hash` and `HeapObject::HashRef`.
    let hash_contents: Option<indexmap::IndexMap<String, crate::value::StrykeValue>> =
        value.as_hash_map().or_else(|| {
            value.as_hash_ref().map(|arc| arc.read().clone())
        });
    if let Some(map) = hash_contents {
        if depth >= MAX_VAR_DEPTH || map.is_empty() {
            // At depth limit, use the non-recursive short repr — format_value
            // would recurse the whole subtree and can stack-overflow on
            // moderately-nested data ($config with `flags => {...}` etc.).
            return VarChild {
                name,
                repr: truncate(&short_scalar_repr(value), MAX_VAR_REPR),
                var_ref: 0,
            };
        }
        let preview: Vec<String> = map.iter().take(4).map(|(k, v)| {
            format!("{k} => {}", short_scalar_repr(v))
        }).collect();
        let repr = format!(
            "[{}] ({}{})",
            map.len(),
            preview.join(", "),
            if map.len() > 4 { format!(", … {} more", map.len() - 4) } else { String::new() },
        );
        let var_ref = ctx.alloc_ref();
        let children: Vec<VarChild> = map.iter().take(2000).map(|(k, v)| {
            build_child(k.clone(), v, depth + 1, ctx)
        }).collect();
        ctx.map.insert(var_ref, children);
        return VarChild {
            name,
            repr: truncate(&repr, MAX_VAR_REPR),
            var_ref,
        };
    }
    // Array or arrayref. For `HeapObject::ArrayRef` we MUST read through the
    // `Arc<RwLock<Vec>>` — `value.to_list()` hits the catch-all `vec![self]`
    // arm for ArrayRef, returning the same ref wrapped, which produces a
    // self-referential descent that wastes stack until MAX_VAR_DEPTH and can
    // overflow on otherwise small data.
    let array_contents: Option<Vec<crate::value::StrykeValue>> = if let Some(arc) = value.as_array_ref() {
        Some(arc.read().clone())
    } else if value.as_array_vec().is_some() {
        Some(value.to_list())
    } else {
        None
    };
    if let Some(list) = array_contents {
        if depth >= MAX_VAR_DEPTH || list.is_empty() {
            return VarChild {
                name,
                repr: truncate(&short_scalar_repr(value), MAX_VAR_REPR),
                var_ref: 0,
            };
        }
        let preview: Vec<String> = list.iter().take(6).map(short_scalar_repr).collect();
        let repr = format!(
            "[{}] ({}{})",
            list.len(),
            preview.join(", "),
            if list.len() > 6 { format!(", … {} more", list.len() - 6) } else { String::new() },
        );
        let var_ref = ctx.alloc_ref();
        let children: Vec<VarChild> = list.iter().take(2000).enumerate().map(|(i, v)| {
            build_child(format!("[{i}]"), v, depth + 1, ctx)
        }).collect();
        ctx.map.insert(var_ref, children);
        return VarChild {
            name,
            repr: truncate(&repr, MAX_VAR_REPR),
            var_ref,
        };
    }
    // Plain scalar leaf
    VarChild {
        name,
        repr: truncate(&crate::debugger::format_value(value), MAX_VAR_REPR),
        var_ref: 0,
    }
}

/// Format a `f64` for the Variables panel: drop trailing zeros, prefer
/// fixed notation for "normal" magnitudes, fall back to scientific for very
/// big / small numbers. Keeps p99 = 87.3 readable instead of 87.30000000000001.
fn fmt_f(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    let av = v.abs();
    if av != 0.0 && (av < 1e-3 || av >= 1e15) {
        return format!("{:e}", v);
    }
    // Trim a {:.6} representation: "12.300000" -> "12.3", "5.000000" -> "5".
    let s = format!("{:.6}", v);
    if !s.contains('.') {
        return s;
    }
    let trimmed = s.trim_end_matches('0').trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "-" {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Helper: produce a leaf row for a sketch sub-stat (e.g. `count`, `p50`).
fn sketch_leaf(name: &str, repr: String) -> VarChild {
    VarChild { name: name.to_string(), repr: truncate(&repr, MAX_VAR_REPR), var_ref: 0 }
}

/// When `value` is a sketch heap object, return a rich [`VarChild`] with a
/// one-line summary repr AND an expandable `var_ref` whose children are the
/// useful stats (count, percentiles, fpr, ...). Returns `None` for any
/// non-sketch value so the caller falls through to the generic logic.
fn try_sketch_child(
    name: &str,
    value: &crate::value::StrykeValue,
    depth: u32,
    ctx: &mut CaptureCtx,
) -> Option<VarChild> {
    if depth >= MAX_VAR_DEPTH {
        return None;
    }

    if let Some(arc) = value.as_tdigest_sketch() {
        let mut g = arc.lock();
        let n = g.count();
        let (repr, children) = if n == 0 {
            (
                "TDigestSketch(empty)".to_string(),
                vec![sketch_leaf("count", "0".to_string()), sketch_leaf("compression", g.compression().to_string())],
            )
        } else {
            let (mn, mx) = (g.min(), g.max());
            let (mean, sum) = (g.mean(), g.sum());
            let (p50, p90, p95, p99) = (g.quantile(0.50), g.quantile(0.90), g.quantile(0.95), g.quantile(0.99));
            let compression = g.compression();
            let repr = format!(
                "TDigestSketch(n={}, min={}, max={}, p50={}, p99={})",
                n, fmt_f(mn), fmt_f(mx), fmt_f(p50), fmt_f(p99)
            );
            let kids = vec![
                sketch_leaf("count", n.to_string()),
                sketch_leaf("min", fmt_f(mn)),
                sketch_leaf("max", fmt_f(mx)),
                sketch_leaf("mean", fmt_f(mean)),
                sketch_leaf("sum", fmt_f(sum)),
                sketch_leaf("p50", fmt_f(p50)),
                sketch_leaf("p90", fmt_f(p90)),
                sketch_leaf("p95", fmt_f(p95)),
                sketch_leaf("p99", fmt_f(p99)),
                sketch_leaf("compression", compression.to_string()),
            ];
            (repr, kids)
        };
        let var_ref = ctx.alloc_ref();
        ctx.map.insert(var_ref, children);
        return Some(VarChild { name: name.to_string(), repr: truncate(&repr, MAX_VAR_REPR), var_ref });
    }

    if let Some(arc) = value.as_bloom_filter() {
        let g = arc.lock();
        let n = g.inserted();
        let bits = g.bit_count();
        let k = g.k();
        let fpr = g.estimated_fpr();
        let repr = format!("BloomFilter(n={}, bits={}, k={}, fpr={})", n, bits, k, fmt_f(fpr));
        let children = vec![
            sketch_leaf("inserted", n.to_string()),
            sketch_leaf("bit_count", bits.to_string()),
            sketch_leaf("k", k.to_string()),
            sketch_leaf("estimated_fpr", fmt_f(fpr)),
        ];
        let var_ref = ctx.alloc_ref();
        ctx.map.insert(var_ref, children);
        return Some(VarChild { name: name.to_string(), repr: truncate(&repr, MAX_VAR_REPR), var_ref });
    }

    if let Some(arc) = value.as_hll_sketch() {
        let g = arc.lock();
        let card = g.count();
        let p = g.precision();
        let m = g.registers_len();
        let repr = format!("HllSketch(cardinality={}, p={}, m={})", fmt_f(card), p, m);
        let children = vec![
            sketch_leaf("cardinality", fmt_f(card)),
            sketch_leaf("precision", p.to_string()),
            sketch_leaf("registers", m.to_string()),
        ];
        let var_ref = ctx.alloc_ref();
        ctx.map.insert(var_ref, children);
        return Some(VarChild { name: name.to_string(), repr: truncate(&repr, MAX_VAR_REPR), var_ref });
    }

    if let Some(arc) = value.as_cms_sketch() {
        let g = arc.lock();
        let repr = format!("CmsSketch(width={}, depth={})", g.width(), g.depth());
        let children = vec![
            sketch_leaf("width", g.width().to_string()),
            sketch_leaf("depth", g.depth().to_string()),
        ];
        let var_ref = ctx.alloc_ref();
        ctx.map.insert(var_ref, children);
        return Some(VarChild { name: name.to_string(), repr: truncate(&repr, MAX_VAR_REPR), var_ref });
    }

    if let Some(arc) = value.as_topk_sketch() {
        let g = arc.lock();
        let k = g.k();
        let n = g.size();
        let heavies = g.heavies(k.min(10));
        let preview: Vec<String> = heavies.iter().take(3).map(|(key, count, _err)| {
            let key_s = String::from_utf8_lossy(key);
            format!("({}, {})", key_s, count)
        }).collect();
        let repr = format!("TopKSketch(k={}, n={}, top=[{}])", k, n, preview.join(", "));
        let mut children = vec![
            sketch_leaf("k", k.to_string()),
            sketch_leaf("size", n.to_string()),
        ];
        for (i, (key, count, err)) in heavies.iter().enumerate() {
            let key_s = String::from_utf8_lossy(key);
            children.push(sketch_leaf(
                &format!("top[{}]", i),
                format!("({}, count={}, err={})", key_s, count, err),
            ));
        }
        let var_ref = ctx.alloc_ref();
        ctx.map.insert(var_ref, children);
        return Some(VarChild { name: name.to_string(), repr: truncate(&repr, MAX_VAR_REPR), var_ref });
    }

    // ─── User-defined record/object types ──────────────────────────────────
    // Struct: `struct Point { x: Num, y: Num }` → drill into named fields.
    if let Some(inst) = value.as_struct_inst() {
        let values = inst.values.read().clone();
        let preview: Vec<String> = inst.def.fields.iter().zip(values.iter()).take(4).map(|(f, v)| {
            format!("{}={}", f.name, short_scalar_repr(v))
        }).collect();
        let repr = format!(
            "{}({}{})",
            inst.def.name,
            preview.join(", "),
            if inst.def.fields.len() > 4 { format!(", … {} more", inst.def.fields.len() - 4) } else { String::new() },
        );
        let var_ref = ctx.alloc_ref();
        let children: Vec<VarChild> = inst.def.fields.iter().zip(values.iter()).map(|(f, v)| {
            build_child(f.name.clone(), v, depth + 1, ctx)
        }).collect();
        ctx.map.insert(var_ref, children);
        return Some(VarChild { name: name.to_string(), repr: truncate(&repr, MAX_VAR_REPR), var_ref });
    }

    // Enum: `enum Maybe { Just(T), Nothing }` → show `Type::Variant(data…)`,
    // expand to show variant name + carried data (which may itself be a
    // struct/array/scalar that further drills down through `build_child`).
    if let Some(inst) = value.as_enum_inst() {
        let variant = inst.variant_name();
        let data_preview = if inst.data.is_undef() {
            String::new()
        } else {
            format!("({})", short_scalar_repr(&inst.data))
        };
        let repr = format!("{}::{}{}", inst.def.name, variant, data_preview);
        let mut children = vec![
            sketch_leaf("__variant", variant.to_string()),
            sketch_leaf("__variant_idx", inst.variant_idx.to_string()),
        ];
        if !inst.data.is_undef() {
            children.push(build_child("data".to_string(), &inst.data, depth + 1, ctx));
        }
        let var_ref = ctx.alloc_ref();
        ctx.map.insert(var_ref, children);
        return Some(VarChild { name: name.to_string(), repr: truncate(&repr, MAX_VAR_REPR), var_ref });
    }

    // Class instance: `class Point { pub x: Num, pub y: Num }`. Drill into
    // named fields; expose the ISA chain (parent classes) and the class
    // name as synthetic `__class` / `__isa` rows so the debugger can show
    // the dispatch target without forcing the user to evaluate `ref $obj`.
    if let Some(inst) = value.as_class_inst() {
        let values = inst.values.read().clone();
        let preview: Vec<String> = inst.def.fields.iter().zip(values.iter()).take(4).map(|(f, v)| {
            format!("{}={}", f.name, short_scalar_repr(v))
        }).collect();
        let repr = format!(
            "{}({}{})",
            inst.def.name,
            preview.join(", "),
            if inst.def.fields.len() > 4 { format!(", … {} more", inst.def.fields.len() - 4) } else { String::new() },
        );
        let mut children = vec![sketch_leaf("__class", inst.def.name.clone())];
        if !inst.isa_chain.is_empty() {
            children.push(sketch_leaf("__isa", format!("[{}]", inst.isa_chain.join(", "))));
        }
        for (f, v) in inst.def.fields.iter().zip(values.iter()) {
            let vis_marker = match f.visibility {
                crate::ast::Visibility::Private => "-",
                crate::ast::Visibility::Protected => "#",
                crate::ast::Visibility::Public => "+",
            };
            children.push(build_child(format!("{}{}", vis_marker, f.name), v, depth + 1, ctx));
        }
        let var_ref = ctx.alloc_ref();
        ctx.map.insert(var_ref, children);
        return Some(VarChild { name: name.to_string(), repr: truncate(&repr, MAX_VAR_REPR), var_ref });
    }

    // Set: ordered set of distinct elements. `PerlSet = IndexMap<String, StrykeValue>`
    // where keys are the element string repr. Drill rows show each element
    // value (the IndexMap's value side, which is the original `StrykeValue`).
    if let Some(arc) = crate::value::set_payload(value) {
        let len = arc.len();
        let preview: Vec<String> = arc.values().take(6).map(short_scalar_repr).collect();
        let repr = format!(
            "Set({}){}",
            len,
            if arc.is_empty() { String::new() } else { format!(" {{{}{}}}",
                preview.join(", "),
                if len > 6 { format!(", … {} more", len - 6) } else { String::new() }
            ) },
        );
        let var_ref = if arc.is_empty() { 0 } else { ctx.alloc_ref() };
        if var_ref != 0 {
            let children: Vec<VarChild> = arc.values().take(2000).enumerate().map(|(i, v)| {
                build_child(format!("[{}]", i), v, depth + 1, ctx)
            }).collect();
            ctx.map.insert(var_ref, children);
        }
        return Some(VarChild { name: name.to_string(), repr: truncate(&repr, MAX_VAR_REPR), var_ref });
    }

    None
}

/// One-line scalar repr used inside preview strings — avoids recursing into
/// nested containers (use `HASH(?)`/`ARRAY(?)` placeholders there).
fn short_scalar_repr(v: &crate::value::StrykeValue) -> String {
    if v.as_hash_ref().is_some() || v.as_hash_map().is_some() {
        return "{…}".to_string();
    }
    if v.as_array_ref().is_some() || v.as_array_vec().is_some() {
        return "[…]".to_string();
    }
    crate::debugger::format_value(v)
}

pub(crate) fn capture_locals_with_map(
    scope: &crate::scope::Scope,
    map: &mut HashMap<u32, Vec<VarChild>>,
) -> Vec<VarSnap> {
    let mut ctx = CaptureCtx { next_ref: CONTAINER_REF_BASE, map };
    let mut user: Vec<VarSnap> = Vec::new();
    let mut topic: Vec<VarSnap> = Vec::new();
    let mut builtin: Vec<VarSnap> = Vec::new();

    for name in scope.all_scalar_names().into_iter().take(256) {
        if should_hide(&name) { continue; }
        let v = scope.get_scalar(&name);
        // A scalar might HOLD a hashref/arrayref (refs in stryke).
        let child = build_child(format!("${name}"), &v, 0, &mut ctx);
        let snap = VarSnap {
            name: child.name,
            repr: child.repr,
            kind: "scalar".into(),
            var_ref: child.var_ref,
        };
        if is_magic_block_param(&name) { topic.push(snap); }
        else if is_builtin_like(&name) { builtin.push(snap); }
        else { user.push(snap); }
    }
    for name in scope.all_array_names().into_iter().take(64) {
        if should_hide(&name) { continue; }
        let arr = scope.get_array(&name);
        let preview: Vec<String> = arr.iter().take(8).map(short_scalar_repr).collect();
        let repr = format!(
            "[{}]{}",
            arr.len(),
            if arr.is_empty() { String::new() } else { format!(" ({}{})",
                preview.join(", "),
                if arr.len() > 8 { format!(", … {} more", arr.len() - 8) } else { String::new() }
            ) },
        );
        let var_ref = if arr.is_empty() { 0 } else { ctx.alloc_ref() };
        if var_ref != 0 {
            let children: Vec<VarChild> = arr.iter().take(2000).enumerate().map(|(i, v)| {
                build_child(format!("[{i}]"), v, 1, &mut ctx)
            }).collect();
            ctx.map.insert(var_ref, children);
        }
        let snap = VarSnap {
            name: format!("@{name}"),
            repr: truncate(&repr, MAX_VAR_REPR),
            kind: "array".into(),
            var_ref,
        };
        if is_builtin_like(&name) { builtin.push(snap); } else { user.push(snap); }
    }
    for name in scope.all_hash_names().into_iter().take(64) {
        if should_hide(&name) { continue; }
        let h = scope.get_hash(&name);
        let preview: Vec<String> = h.iter().take(6).map(|(k, v)| {
            format!("{k} => {}", short_scalar_repr(v))
        }).collect();
        let repr = format!(
            "[{}]{}",
            h.len(),
            if h.is_empty() { String::new() } else { format!(" ({}{})",
                preview.join(", "),
                if h.len() > 6 { format!(", … {} more", h.len() - 6) } else { String::new() }
            ) },
        );
        let var_ref = if h.is_empty() { 0 } else { ctx.alloc_ref() };
        if var_ref != 0 {
            let children: Vec<VarChild> = h.iter().take(2000).map(|(k, v)| {
                build_child(k.clone(), v, 1, &mut ctx)
            }).collect();
            ctx.map.insert(var_ref, children);
        }
        let snap = VarSnap {
            name: format!("%{name}"),
            repr: truncate(&repr, MAX_VAR_REPR),
            kind: "hash".into(),
            var_ref,
        };
        if is_builtin_like(&name) { builtin.push(snap); } else { user.push(snap); }
    }

    let by_sigil_name = |a: &VarSnap, b: &VarSnap| a.name.cmp(&b.name);
    user.sort_by(by_sigil_name);
    builtin.sort_by(by_sigil_name);
    // Topic variants sort numerically ($_, $_0, $_1, $_2 ...) — not lex
    // (which would put $_10 before $_2). Strip the `$_` prefix; an empty
    // tail (the bare `$_`) sorts before any digit suffix.
    // Order: `$_`, `$_0`, `$_1`, ..., `$a`, `$b`. Underscore family first
    // (sorted numerically so `$_2` precedes `$_10`), then sort/reduce
    // params last.
    topic.sort_by(|a, b| {
        let key = |n: &str| -> (u8, usize, String) {
            // Bucket 0 = underscore topics, bucket 1 = $a/$b.
            if n == "$a" || n == "$b" {
                (1, 0, n.to_string())
            } else {
                let bare = n.strip_prefix("$_").unwrap_or(n);
                (0, bare.parse::<usize>().unwrap_or(0), n.to_string())
            }
        };
        key(&a.name).cmp(&key(&b.name))
    });
    let mut out = user;
    out.extend(topic);
    out.extend(builtin);
    out
}

/// Magic block-param scalars: `$_`, `$_0`, `$_1`, ... (topic + implicit
/// closure positionals) plus `$a` / `$b` (Perl-5 sort/reduce holdovers).
/// They belong below user-defined `my` variables but above compiler/runtime
/// builtins, so the Variables panel reads as: my-vars first, magic block
/// params in the middle, builtins at the bottom.
fn is_magic_block_param(name: &str) -> bool {
    if name == "_" || name == "a" || name == "b" {
        return true;
    }
    if let Some(rest) = name.strip_prefix('_') {
        return !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit());
    }
    false
}

/// Backwards-compat shim — call sites that don't want the map yet.
pub(crate) fn capture_locals(scope: &crate::scope::Scope) -> Vec<VarSnap> {
    let mut map = HashMap::new();
    capture_locals_with_map(scope, &mut map)
}

fn should_hide(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    // Compiler-internal `__foreach_i__`, `__foreach_list__`,
    // `__INTERCEPT_NAME__`, `__INTERCEPT_ARGS__`, etc. — anything wrapped
    // in double underscores is reserved synthetic state, never user-facing.
    if name.starts_with("__") && name.ends_with("__") && name.len() > 4 {
        return true;
    }
    false
}

/// True for compiler-generated / stryke-built-in names that should sort to
/// the bottom of the Variables panel so the user's own `my $foo` ones float
/// to the top.
fn is_builtin_like(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.starts_with('^') {
        return true;
    }
    // Pipeline outer-chain topic vars: `_<`, `_<<`, `_<<<`, `_0<`, etc.
    if name.starts_with('_') && name.len() > 1 && name[1..].contains('<') {
        return true;
    }
    // Common stryke built-in arrays/hashes
    matches!(
        name,
        "INC" | "ARGV" | "ENV" | "INC" | "SIG"
            | "path" | "p" | "fpath" | "f"
            | "term" | "uname" | "limits"
            | "-" | "+" | "~" | "/" | "\\" | "\"" | "," | "!" | "@" | "&" | "'" | "`" | "?" | "$"
    ) || name.starts_with("stryke::")
}

// ─── Public entrypoint ──────────────────────────────────────────────────────

/// Run `st --dap [HOST:PORT]`.
///
/// * **Stdio mode** (`st --dap`) — DAP traffic uses stdio. Fine for manual
///   shell testing; *broken* under IntelliJ because `OSProcessHandler` reads
///   the same stdout stream and steals bytes from `DapClient`.
/// * **TCP mode** (`st --dap HOST:PORT`) — connects to the given address and
///   runs DAP over that socket. Stdio is left alone so the program's `p` /
///   `print` output flows normally to the Console. This is the path the
///   IntelliJ plugin uses.
pub fn run() -> i32 {
    run_with_args(&[])
}

pub fn run_with_args(args: &[String]) -> i32 {
    let connect_addr = args.iter().find(|a| a.contains(':')).cloned();
    let (reader, writer): (Box<dyn Read + Send>, Box<dyn Write + Send>) = match connect_addr {
        Some(addr) => {
            // TCP mode: connect to the spawner's server socket.
            match std::net::TcpStream::connect(&addr) {
                Ok(s) => {
                    let r = s.try_clone().expect("dap: tcp clone");
                    (Box::new(r), Box::new(s))
                }
                Err(e) => {
                    eprintln!("stryke --dap: connect {addr}: {e}");
                    return 2;
                }
            }
        }
        None => (Box::new(io::stdin()), Box::new(io::stdout())),
    };

    let shared = DapShared::new(writer);
    let bp_state = Arc::new(Mutex::new(BreakpointState::default()));
    let (_reader_handle, launch_rx) = spawn_reader_with_input(shared.clone(), bp_state.clone(), reader);

    // Wait for `launch` request before starting the VM.
    let lp = match launch_rx.recv() {
        Ok(p) => p,
        Err(_) => return 1,
    };

    // Send `process` event for prettiness in client UIs.
    shared.emit_event(
        "process",
        json!({
            "name": lp.program,
            "isLocalProcess": true,
            "startMethod": "launch",
        }),
    );
    shared.emit_event("thread", json!({ "reason": "started", "threadId": 1 }));

    // Read the program source.
    let source = match std::fs::read_to_string(&lp.program) {
        Ok(s) => s,
        Err(e) => {
            shared.emit_event(
                "output",
                json!({ "category": "stderr", "output": format!("stryke --dap: cannot read {}: {}\n", lp.program, e) }),
            );
            shared.emit_event("terminated", json!({}));
            return 1;
        }
    };

    // Build interpreter + debugger.
    let mut interp = crate::vm_helper::VMHelper::new();
    if let Some(cwd) = &lp.cwd {
        let _ = std::env::set_current_dir(cwd);
    }
    interp.file = lp.program.clone();
    // Pre-populate @ARGV
    let argv_vals: Vec<crate::value::StrykeValue> = lp
        .args
        .iter()
        .map(|s| crate::value::StrykeValue::string(s.clone()))
        .collect();
    interp.scope.declare_array("ARGV", argv_vals);

    // Pre-populate @INC the same way `configure_interpreter` does for CLI:
    // vendor perl modules, system perl's INC, the script's directory,
    // `STRYKE_INC`, then ".".
    let mut inc_paths: Vec<String> = Vec::new();
    let vendor = crate::vendor_perl_inc_path();
    if vendor.is_dir() {
        crate::perl_inc::push_unique_string_paths(
            &mut inc_paths,
            vec![vendor.to_string_lossy().into_owned()],
        );
    }
    crate::perl_inc::push_unique_string_paths(
        &mut inc_paths,
        crate::perl_inc::paths_from_system_perl(),
    );
    if let Some(parent) = std::path::Path::new(&lp.program).parent() {
        if !parent.as_os_str().is_empty() {
            crate::perl_inc::push_unique_string_paths(
                &mut inc_paths,
                vec![parent.to_string_lossy().into_owned()],
            );
        }
    }
    if let Ok(extra) = std::env::var("STRYKE_INC") {
        let extra: Vec<String> = std::env::split_paths(&extra)
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        crate::perl_inc::push_unique_string_paths(&mut inc_paths, extra);
    }
    crate::perl_inc::push_unique_string_paths(&mut inc_paths, vec![".".to_string()]);
    let inc_dirs: Vec<crate::value::StrykeValue> = inc_paths
        .into_iter()
        .map(crate::value::StrykeValue::string)
        .collect();
    interp.scope.declare_array("INC", inc_dirs);

    // Eagerly populate %ENV so the Variables panel shows the process env on
    // first stop. Normal CLI mode defers this to first `$ENV{KEY}` access;
    // for an inspect-only debugger surface we want it visible immediately.
    interp.materialize_env_if_needed();

    // Force `$| = 1` so user `p` / `print` / `printf` calls flush stdout
    // immediately. Without this, stdout is block-buffered (piped to the
    // IDE's OSProcessHandler) and output doesn't appear in the Console
    // until the buffer fills or the program exits. Real-time tracing
    // through a debugger needs immediate output.
    interp.output_autoflush = true;

    // NOTE: NOT calling `ensure_reflection_hashes()` here. Doing so triggers
    // a stack overflow during the VM's main dispatch (probably some path in
    // hash-lookup recursion when 10k+ entries are eagerly inserted). The
    // hashes get populated lazily on first user access via the
    // `touch_env_hash` hook in vm_helper, so they'll be visible *after* the
    // script accesses one. Eagerly installing them needs more investigation.

    // Configure debugger with DAP backend.
    let mut dbg = crate::debugger::Debugger::new();
    dbg.set_file(&lp.program);
    dbg.load_source(&source);
    // Pre-set breakpoints
    {
        let bp = bp_state.lock().expect("bp lock");
        if let Some(lines) = bp.line_breakpoints.get(&lp.program) {
            for l in lines {
                dbg.add_breakpoint_line(*l);
            }
        }
        for name in &bp.function_breakpoints {
            dbg.add_breakpoint_sub(name);
        }
    }
    dbg.set_dap_backend(shared.clone(), bp_state.clone());
    if !lp.stop_on_entry {
        dbg.set_step_mode(false);
    }
    interp.debugger = Some(dbg);

    // Parse + run. `no_interop` flag is exposed but we always go through the
    // standard parser in v1; honoring the flag is a follow-on once
    // `parse_with_file_no_interop` is exposed publicly.
    let _ = lp.no_interop;
    let program = match crate::parse_with_file(&source, &lp.program) {
        Ok(p) => p,
        Err(e) => {
            shared.emit_event(
                "output",
                json!({
                    "category": "stderr",
                    "output": format!("stryke: parse error: {}\n", e.message),
                }),
            );
            shared.emit_event(
                "stopped",
                json!({ "reason": "exception", "threadId": 1, "description": e.message }),
            );
            shared.emit_event("terminated", json!({}));
            return 1;
        }
    };

    let result = interp.execute(&program);

    let exit_code = match result {
        Ok(_) => 0,
        Err(e) => {
            // Client-initiated disconnect propagates back as "debugger: quit".
            // That's expected shutdown, not a user-visible runtime error.
            if e.message != "debugger: quit" && !shared.was_disconnected() {
                shared.emit_event(
                    "output",
                    json!({
                        "category": "stderr",
                        "output": format!("stryke: runtime error: {}\n", e.message),
                    }),
                );
            }
            if shared.was_disconnected() { 0 } else { 1 }
        }
    };

    shared.emit_event("exited", json!({ "exitCode": exit_code }));
    shared.emit_event("terminated", json!({}));
    exit_code
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_magic_block_param classifier ─────────────────────────────────
    //
    // Drives the Variables panel's three-tier sort: user `my` vars first,
    // then magic block params (`$_`, `$_N`, `$a`, `$b`), then builtins.
    // Misclassifying a user variable as magic would silently push it to
    // the wrong bucket.

    #[test]
    fn magic_block_param_matches_underscore_topic_and_sort_pair() {
        assert!(is_magic_block_param("_"), "$_ topic");
        assert!(is_magic_block_param("_0"), "$_0 first positional");
        assert!(is_magic_block_param("_1"), "$_1 second positional");
        assert!(is_magic_block_param("_42"), "$_N N-th positional");
        assert!(is_magic_block_param("a"), "$a sort/reduce");
        assert!(is_magic_block_param("b"), "$b sort/reduce");
    }

    #[test]
    fn magic_block_param_rejects_user_names() {
        assert!(!is_magic_block_param("name"));
        assert!(
            !is_magic_block_param("_name"),
            "underscore-prefix is not a topic alias"
        );
        assert!(!is_magic_block_param("a1"));
        assert!(
            !is_magic_block_param("ab"),
            "$ab is a user var, not a magic block param"
        );
        assert!(!is_magic_block_param(""));
    }

    // ── should_hide — synthetic compiler vars ───────────────────────────
    //
    // The Variables panel hides anything wrapped in double underscores
    // (`__foreach_i__`, `__INTERCEPT_NAME__`, etc.) — those are compiler-
    // internal synthetic names, never user-facing. The check is "starts
    // and ends with `__` AND length > 4" so we don't accidentally hide
    // `__` itself or `__x__` style user names that happen to have the
    // marker shape.

    #[test]
    fn should_hide_dunder_synthetic_names() {
        assert!(should_hide("__foreach_i__"));
        assert!(should_hide("__INTERCEPT_NAME__"));
        assert!(should_hide("__list_assign_tmp__"));
    }

    #[test]
    fn should_hide_keeps_user_visible_names() {
        assert!(!should_hide("x"));
        assert!(!should_hide("my_var"));
        assert!(!should_hide("_"));
        assert!(!should_hide("_0"));
        assert!(!should_hide("__"));
        // Single-leading-underscore is a user var (per
        // is_magic_block_param), not a synthetic.
        assert!(!should_hide("_foo"));
    }

    #[test]
    fn should_hide_empty_name() {
        assert!(should_hide(""));
    }

    // ── fmt_f — number formatting for sketch panel rows ─────────────────

    #[test]
    fn fmt_f_trims_trailing_zeros() {
        assert_eq!(fmt_f(1.0), "1");
        assert_eq!(fmt_f(1.5), "1.5");
        assert_eq!(fmt_f(0.0), "0");
        assert_eq!(fmt_f(-2.5), "-2.5");
    }

    #[test]
    fn fmt_f_uses_scientific_for_extremes() {
        // Very small.
        assert!(fmt_f(1e-10).contains('e'));
        // Very large.
        assert!(fmt_f(1e20).contains('e'));
    }

    #[test]
    fn fmt_f_handles_non_finite() {
        // NaN / inf round-trip through Rust's default Display.
        assert_eq!(fmt_f(f64::NAN), "NaN");
        assert_eq!(fmt_f(f64::INFINITY), "inf");
    }

    // ── is_builtin_like — bottom-bucket sort classifier ─────────────────

    #[test]
    fn is_builtin_like_matches_stryke_builtin_arrays_and_hashes() {
        assert!(is_builtin_like("INC"));
        assert!(is_builtin_like("ARGV"));
        assert!(is_builtin_like("ENV"));
        assert!(is_builtin_like("path"));
        assert!(is_builtin_like("p"));
        assert!(is_builtin_like("term"));
    }

    #[test]
    fn is_builtin_like_matches_caret_prefixed_specials() {
        // `$^O`, `$^X`, etc. — the `^` prefix marks them as Perl special
        // variables visible only via the caret-name form.
        assert!(is_builtin_like("^O"));
        assert!(is_builtin_like("^X"));
        assert!(is_builtin_like("^HOOK"));
    }

    #[test]
    fn is_builtin_like_matches_pipeline_outer_topic_chains() {
        // `_<`, `_<<`, `_0<`, ... — outer-topic chain naming.
        assert!(is_builtin_like("_<"));
        assert!(is_builtin_like("_<<"));
        assert!(is_builtin_like("_0<"));
        assert!(is_builtin_like("_5<<<"));
    }

    #[test]
    fn is_builtin_like_rejects_plain_user_names() {
        assert!(!is_builtin_like("x"));
        assert!(!is_builtin_like("my_var"));
        assert!(!is_builtin_like("_5"));
        assert!(!is_builtin_like(""));
    }
}
