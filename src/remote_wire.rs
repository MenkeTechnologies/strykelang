//! Framed bincode over stdin/stdout for `pe --remote-worker` (distributed `pmap_on`).
//!
//! ## Wire protocol
//!
//! Every message is a length-prefixed frame: `[u64 LE length][u8 kind][bincode payload]`.
//! The single-byte `kind` discriminator lets future revisions add message types without
//! breaking older workers — an unknown kind is a hard error so version skew is loud.
//!
//! ### Message flow (v3 — persistent session)
//!
//! ```text
//! dispatcher                    worker
//!     │                            │
//!     │── HELLO ─────────────────►│   (proto version, build id)
//!     │◄───────────── HELLO_ACK ──│   (worker pe version, hostname)
//!     │── SESSION_INIT ──────────►│   (subs prelude, block source, captured lexicals)
//!     │◄────────── SESSION_ACK ───│   (or ERROR)
//!     │── JOB(seq=0) ────────────►│   (item)
//!     │◄────────── JOB_RESP(0) ───│
//!     │── JOB(seq=1) ────────────►│
//!     │◄────────── JOB_RESP(1) ───│
//!     │           ...             │
//!     │── SHUTDOWN ──────────────►│
//!     │                            └─ exit 0
//! ```
//!
//! Why this beats the basic v1 protocol: subs prelude + block source ship **once** per
//! session instead of once per item, the parser+compiler runs once per worker instead of
//! once per job, and one ssh handshake amortizes across the whole map.
//!
//! Dynamic [`serde_json::Value`] fields are embedded as JSON UTF-8 bytes inside the bincode
//! envelope (v3+). Bincode cannot deserialize `Value` directly (`deserialize_any`); nested
//! JSON keeps the on-wire type self-describing.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::ast::Block;
use crate::interpreter::{FlowOrError, Interpreter};
use crate::value::{PerlSub, PerlValue};

/// Frame-kind discriminator. Stored as the first byte of every wire payload after the
/// length prefix. Sub-byte values are reserved (anything outside the documented set is
/// rejected with a clean error rather than silently misparsed).
#[allow(dead_code)]
pub mod frame_kind {
    pub const HELLO: u8 = 0x01;
    pub const HELLO_ACK: u8 = 0x02;
    pub const SESSION_INIT: u8 = 0x03;
    pub const SESSION_ACK: u8 = 0x04;
    pub const JOB: u8 = 0x05;
    pub const JOB_RESP: u8 = 0x06;
    pub const SHUTDOWN: u8 = 0x07;
    pub const ERROR: u8 = 0xFF;
}

/// Wire protocol version. Bumped whenever the layout of an existing message changes in a
/// backwards-incompatible way. The HELLO handshake fails fast on version mismatch so
/// dispatcher and worker never silently disagree on layout.
pub const PROTO_VERSION: u32 = 3;

mod json_value_bincode {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &serde_json::Value, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let buf = serde_json::to_vec(value).map_err(serde::ser::Error::custom)?;
        buf.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<serde_json::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let buf: Vec<u8> = Vec::deserialize(deserializer)?;
        serde_json::from_slice(&buf).map_err(serde::de::Error::custom)
    }
}

mod capture_json_bincode {
    use serde::{de::Deserializer, ser::SerializeSeq, Deserialize, Serializer};

    pub fn serialize<S>(v: &[(String, serde_json::Value)], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(v.len()))?;
        for (k, val) in v {
            let enc = serde_json::to_vec(val).map_err(serde::ser::Error::custom)?;
            seq.serialize_element(&(k, enc))?;
        }
        seq.end()
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<Vec<(String, serde_json::Value)>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw: Vec<(String, Vec<u8>)> = Vec::deserialize(deserializer)?;
        let mut out = Vec::with_capacity(raw.len());
        for (k, enc) in raw {
            let val = serde_json::from_slice(&enc).map_err(serde::de::Error::custom)?;
            out.push((k, val));
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloMsg {
    pub proto_version: u32,
    pub pe_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloAck {
    pub proto_version: u32,
    pub pe_version: String,
    pub hostname: String,
}

/// Sent **once** per worker session. Carries everything that doesn't change between jobs:
/// the user's named subs, the `pmap_on` block source, and the captured-lexical snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInit {
    pub subs_prelude: String,
    pub block_src: String,
    #[serde(with = "capture_json_bincode")]
    pub capture: Vec<(String, serde_json::Value)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAck {
    pub ok: bool,
    pub err_msg: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobMsg {
    pub seq: u64,
    #[serde(with = "json_value_bincode")]
    pub item: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRespMsg {
    pub seq: u64,
    pub ok: bool,
    #[serde(with = "json_value_bincode")]
    pub result: serde_json::Value,
    pub err_msg: String,
}

/// Read a typed frame: returns `(kind, body)` where `body` is the bincode payload after
/// the kind byte. Caller decides how to interpret based on `kind`.
pub fn read_typed_frame<R: Read>(r: &mut R) -> std::io::Result<(u8, Vec<u8>)> {
    let raw = read_framed(r)?;
    if raw.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "remote frame: empty payload (missing kind byte)",
        ));
    }
    let kind = raw[0];
    Ok((kind, raw[1..].to_vec()))
}

/// Write a typed frame: prepends the `kind` byte to `payload` and writes one length-prefixed
/// frame.
pub fn write_typed_frame<W: Write>(w: &mut W, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut framed = Vec::with_capacity(payload.len() + 1);
    framed.push(kind);
    framed.extend_from_slice(payload);
    write_framed(w, &framed)
}

/// Bincode + write helper. The two-step `bincode::serialize` + `write_typed_frame` pattern
/// is the same in every send site so it lives here once.
pub fn send_msg<W: Write, T: Serialize>(w: &mut W, kind: u8, msg: &T) -> Result<(), String> {
    let payload = bincode::serialize(msg).map_err(|e| format!("bincode encode: {e}"))?;
    write_typed_frame(w, kind, &payload).map_err(|e| format!("write frame: {e}"))
}

/// Bincode + read helper. Returns the deserialized message and verifies the kind matches.
pub fn recv_msg<R: Read, T: for<'de> Deserialize<'de>>(
    r: &mut R,
    expected_kind: u8,
) -> Result<T, String> {
    let (kind, body) = read_typed_frame(r).map_err(|e| format!("read frame: {e}"))?;
    if kind != expected_kind {
        return Err(format!(
            "wire: expected frame kind {:#04x}, got {:#04x}",
            expected_kind, kind
        ));
    }
    bincode::deserialize(&body).map_err(|e| format!("bincode decode: {e}"))
}

/// One unit of work executed on a remote `pe --remote-worker`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteJobV1 {
    pub seq: u64,
    pub subs_prelude: String,
    pub block_src: String,
    #[serde(with = "capture_json_bincode")]
    pub capture: Vec<(String, serde_json::Value)>,
    #[serde(with = "json_value_bincode")]
    pub item: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteRespV1 {
    pub seq: u64,
    pub ok: bool,
    #[serde(with = "json_value_bincode")]
    pub result: serde_json::Value,
    pub err_msg: String,
}

const MAX_FRAME: usize = 256 * 1024 * 1024;

pub fn write_framed<W: Write>(w: &mut W, payload: &[u8]) -> std::io::Result<()> {
    w.write_all(&(payload.len() as u64).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()?;
    Ok(())
}

pub fn read_framed<R: Read>(r: &mut R) -> std::io::Result<Vec<u8>> {
    let mut h = [0u8; 8];
    r.read_exact(&mut h)?;
    let n = u64::from_le_bytes(h) as usize;
    if n > MAX_FRAME {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("remote frame too large: {n}"),
        ));
    }
    let mut v = vec![0u8; n];
    r.read_exact(&mut v)?;
    Ok(v)
}

pub fn encode_job(job: &RemoteJobV1) -> Result<Vec<u8>, String> {
    bincode::serialize(job).map_err(|e| e.to_string())
}

pub fn decode_job(bytes: &[u8]) -> Result<RemoteJobV1, String> {
    bincode::deserialize(bytes).map_err(|e| e.to_string())
}

pub fn encode_resp(resp: &RemoteRespV1) -> Result<Vec<u8>, String> {
    bincode::serialize(resp).map_err(|e| e.to_string())
}

pub fn decode_resp(bytes: &[u8]) -> Result<RemoteRespV1, String> {
    bincode::deserialize(bytes).map_err(|e| e.to_string())
}

pub fn perl_to_json_value(v: &PerlValue) -> Result<serde_json::Value, String> {
    if v.is_undef() {
        return Ok(serde_json::Value::Null);
    }
    if let Some(i) = v.as_integer() {
        return Ok(serde_json::json!(i));
    }
    if let Some(f) = v.as_float() {
        return Ok(serde_json::json!(f));
    }
    if v.is_string_like() {
        return Ok(serde_json::Value::String(v.to_string()));
    }
    if let Some(a) = v.as_array_vec() {
        let mut out = Vec::with_capacity(a.len());
        for x in a {
            out.push(perl_to_json_value(&x)?);
        }
        return Ok(serde_json::Value::Array(out));
    }
    if let Some(h) = v.as_hash_map() {
        let mut m = serde_json::Map::new();
        for (k, val) in h {
            m.insert(k.clone(), perl_to_json_value(&val)?);
        }
        return Ok(serde_json::Value::Object(m));
    }
    Err(format!(
        "value not supported for remote pmap (need null, bool/int/float/string/array/hash): {}",
        v.type_name()
    ))
}

pub fn json_to_perl(v: &serde_json::Value) -> Result<PerlValue, String> {
    Ok(match v {
        serde_json::Value::Null => PerlValue::UNDEF,
        serde_json::Value::Bool(b) => PerlValue::integer(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                PerlValue::integer(i)
            } else if let Some(u) = n.as_u64() {
                PerlValue::integer(u as i64)
            } else {
                PerlValue::float(n.as_f64().unwrap_or(0.0))
            }
        }
        serde_json::Value::String(s) => PerlValue::string(s.clone()),
        serde_json::Value::Array(a) => {
            let mut items = Vec::with_capacity(a.len());
            for x in a {
                items.push(json_to_perl(x)?);
            }
            PerlValue::array(items)
        }
        serde_json::Value::Object(o) => {
            let mut map = indexmap::IndexMap::new();
            for (k, val) in o {
                map.insert(k.clone(), json_to_perl(val)?);
            }
            PerlValue::hash(map)
        }
    })
}

pub fn capture_entries_to_json(
    entries: &[(String, PerlValue)],
) -> Result<Vec<(String, serde_json::Value)>, String> {
    let mut out = Vec::with_capacity(entries.len());
    for (k, v) in entries {
        out.push((k.clone(), perl_to_json_value(v)?));
    }
    Ok(out)
}

pub fn build_subs_prelude(subs: &HashMap<String, Arc<PerlSub>>) -> String {
    let mut names: Vec<_> = subs.keys().cloned().collect();
    names.sort();
    let mut s = String::new();
    for name in names {
        let sub = &subs[&name];
        if sub.closure_env.is_some() {
            continue;
        }
        let sig = if !sub.params.is_empty() {
            format!(
                " ({})",
                sub.params
                    .iter()
                    .map(crate::fmt::format_sub_sig_param)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else if let Some(ref p) = sub.prototype {
            format!(" ({})", p)
        } else {
            String::new()
        };
        let body = crate::fmt::format_block(&sub.body);
        s.push_str(&format!("sub {}{} {{\n{}\n}}\n", name, sig, body));
    }
    s
}

/// Run one job in-process (for tests / local debugging).
pub fn run_job_local(job: &RemoteJobV1) -> RemoteRespV1 {
    let mut interp = Interpreter::new();
    let cap: Vec<(String, PerlValue)> = match job
        .capture
        .iter()
        .map(|(k, v)| json_to_perl(v).map(|pv| (k.clone(), pv)))
        .collect()
    {
        Ok(c) => c,
        Err(e) => {
            return RemoteRespV1 {
                seq: job.seq,
                ok: false,
                result: serde_json::Value::Null,
                err_msg: e,
            };
        }
    };
    interp.scope_push_hook();
    interp.scope.restore_capture(&cap);
    let item_pv = match json_to_perl(&job.item) {
        Ok(v) => v,
        Err(e) => {
            interp.scope_pop_hook();
            return RemoteRespV1 {
                seq: job.seq,
                ok: false,
                result: serde_json::Value::Null,
                err_msg: e,
            };
        }
    };
    interp.scope.set_topic(item_pv);
    let full_src = format!("{}\n{}", job.subs_prelude, job.block_src);
    let prog = match crate::parse(&full_src) {
        Ok(p) => p,
        Err(e) => {
            interp.scope_pop_hook();
            return RemoteRespV1 {
                seq: job.seq,
                ok: false,
                result: serde_json::Value::Null,
                err_msg: e.message,
            };
        }
    };
    let block: Block = prog.statements;
    let r = match interp.exec_block_smart(&block) {
        Ok(v) => v,
        Err(e) => {
            interp.scope_pop_hook();
            let msg = match e {
                FlowOrError::Error(pe) => pe.to_string(),
                FlowOrError::Flow(f) => format!("unexpected control flow: {:?}", f),
            };
            return RemoteRespV1 {
                seq: job.seq,
                ok: false,
                result: serde_json::Value::Null,
                err_msg: msg,
            };
        }
    };
    interp.scope_pop_hook();
    match perl_to_json_value(&r) {
        Ok(j) => RemoteRespV1 {
            seq: job.seq,
            ok: true,
            result: j,
            err_msg: String::new(),
        },
        Err(e) => RemoteRespV1 {
            seq: job.seq,
            ok: false,
            result: serde_json::Value::Null,
            err_msg: e,
        },
    }
}

/// Persistent v3 worker session: handles many jobs over a single stdin/stdout pair, with
/// one Interpreter and one parsed block shared across the whole session.
///
/// Protocol order: HELLO → HELLO_ACK → SESSION_INIT → SESSION_ACK → JOB / JOB_RESP loop
/// → SHUTDOWN → exit. Any wire error or unknown frame kind causes a clean non-zero exit so
/// the dispatcher can re-route in-flight jobs to a different slot.
///
/// Why this beats the basic v1 [`run_remote_worker_stdio`]: subs prelude + block source
/// ship **once** per session instead of per-item, parser+compiler runs once per worker,
/// and one ssh handshake amortizes across the whole map.
pub fn run_remote_worker_session() -> i32 {
    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();
    let mut stdout = std::io::stdout();

    // 1. HELLO handshake. Dispatcher sends first; we reply with our build info.
    let hello: HelloMsg = match recv_msg(&mut stdin, frame_kind::HELLO) {
        Ok(h) => h,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "remote-worker: hello: {e}");
            return 1;
        }
    };
    if hello.proto_version != PROTO_VERSION {
        let _ = writeln!(
            std::io::stderr(),
            "remote-worker: proto version mismatch (dispatcher {} vs worker {})",
            hello.proto_version,
            PROTO_VERSION
        );
        return 1;
    }
    let ack = HelloAck {
        proto_version: PROTO_VERSION,
        pe_version: env!("CARGO_PKG_VERSION").to_string(),
        hostname: hostname_or_unknown(),
    };
    if let Err(e) = send_msg(&mut stdout, frame_kind::HELLO_ACK, &ack) {
        let _ = writeln!(std::io::stderr(), "remote-worker: hello ack: {e}");
        return 1;
    }

    // 2. SESSION_INIT: subs prelude + block source + captured lexicals.
    let init: SessionInit = match recv_msg(&mut stdin, frame_kind::SESSION_INIT) {
        Ok(i) => i,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "remote-worker: session init: {e}");
            return 1;
        }
    };

    // Parse subs prelude ONCE so they're registered for every JOB; parse block ONCE so we
    // can hand the same `Block` to `exec_block_smart` per item without re-parsing.
    let mut interp = Interpreter::new();
    let prelude_program = match crate::parse(&init.subs_prelude) {
        Ok(p) => p,
        Err(e) => {
            let nack = SessionAck {
                ok: false,
                err_msg: format!("parse subs prelude: {}", e.message),
            };
            let _ = send_msg(&mut stdout, frame_kind::SESSION_ACK, &nack);
            return 2;
        }
    };
    let block_program = match crate::parse(&init.block_src) {
        Ok(p) => p,
        Err(e) => {
            let nack = SessionAck {
                ok: false,
                err_msg: format!("parse block: {}", e.message),
            };
            let _ = send_msg(&mut stdout, frame_kind::SESSION_ACK, &nack);
            return 2;
        }
    };

    // Restore captured lexicals once per session — they don't change across jobs.
    let cap_pv: Vec<(String, PerlValue)> = match init
        .capture
        .iter()
        .map(|(k, v)| json_to_perl(v).map(|pv| (k.clone(), pv)))
        .collect()
    {
        Ok(c) => c,
        Err(e) => {
            let nack = SessionAck {
                ok: false,
                err_msg: format!("decode capture: {e}"),
            };
            let _ = send_msg(&mut stdout, frame_kind::SESSION_ACK, &nack);
            return 2;
        }
    };
    interp.scope_push_hook();
    interp.scope.restore_capture(&cap_pv);

    // Run the prelude (sub decls) once. After this every JOB has the user's named subs in
    // scope without re-parsing or re-executing the prelude per item.
    if let Err(e) = interp.execute(&prelude_program) {
        let nack = SessionAck {
            ok: false,
            err_msg: format!("session prelude: {e}"),
        };
        let _ = send_msg(&mut stdout, frame_kind::SESSION_ACK, &nack);
        return 2;
    }

    let ack = SessionAck {
        ok: true,
        err_msg: String::new(),
    };
    if let Err(e) = send_msg(&mut stdout, frame_kind::SESSION_ACK, &ack) {
        let _ = writeln!(std::io::stderr(), "remote-worker: session ack: {e}");
        return 1;
    }

    let block: Block = block_program.statements;

    // 3. JOB loop. Each iteration sets `$_ = item`, re-evaluates the cached block, and
    // sends back the result. The Interpreter is reused — sub registrations, package state,
    // anything mutated by SESSION_INIT persists across jobs.
    loop {
        let (kind, body) = match read_typed_frame(&mut stdin) {
            Ok(p) => p,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return 0,
            Err(e) => {
                let _ = writeln!(std::io::stderr(), "remote-worker: read job: {e}");
                return 1;
            }
        };
        match kind {
            frame_kind::JOB => {
                let job: JobMsg = match bincode::deserialize(&body) {
                    Ok(j) => j,
                    Err(e) => {
                        let resp = JobRespMsg {
                            seq: 0,
                            ok: false,
                            result: serde_json::Value::Null,
                            err_msg: format!("decode job: {e}"),
                        };
                        let _ = send_msg(&mut stdout, frame_kind::JOB_RESP, &resp);
                        continue;
                    }
                };
                let resp = run_one_session_job(&mut interp, &block, &job);
                if let Err(e) = send_msg(&mut stdout, frame_kind::JOB_RESP, &resp) {
                    let _ = writeln!(std::io::stderr(), "remote-worker: write resp: {e}");
                    return 1;
                }
            }
            frame_kind::SHUTDOWN => return 0,
            other => {
                let _ = writeln!(
                    std::io::stderr(),
                    "remote-worker: unexpected frame kind {:#04x} in JOB loop",
                    other
                );
                return 1;
            }
        }
    }
}

/// Run one JOB inside an active session. Sets `$_` to the item, evaluates the cached block,
/// returns the JSON-marshalled result. Preserves Interpreter state across jobs so anything
/// the prelude installed (named subs, package vars) stays live.
fn run_one_session_job(interp: &mut Interpreter, block: &Block, job: &JobMsg) -> JobRespMsg {
    let item_pv = match json_to_perl(&job.item) {
        Ok(v) => v,
        Err(e) => {
            return JobRespMsg {
                seq: job.seq,
                ok: false,
                result: serde_json::Value::Null,
                err_msg: e,
            };
        }
    };
    interp.scope.set_topic(item_pv);
    let r = match interp.exec_block_smart(block) {
        Ok(v) => v,
        Err(FlowOrError::Error(pe)) => {
            return JobRespMsg {
                seq: job.seq,
                ok: false,
                result: serde_json::Value::Null,
                err_msg: pe.to_string(),
            };
        }
        Err(FlowOrError::Flow(f)) => {
            return JobRespMsg {
                seq: job.seq,
                ok: false,
                result: serde_json::Value::Null,
                err_msg: format!("unexpected control flow: {:?}", f),
            };
        }
    };
    match perl_to_json_value(&r) {
        Ok(j) => JobRespMsg {
            seq: job.seq,
            ok: true,
            result: j,
            err_msg: String::new(),
        },
        Err(e) => JobRespMsg {
            seq: job.seq,
            ok: false,
            result: serde_json::Value::Null,
            err_msg: e,
        },
    }
}

fn hostname_or_unknown() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| {
        std::process::Command::new("hostname")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    })
}

/// stdin/stdout worker loop: one framed request → one framed response, then exit 0.
pub fn run_remote_worker_stdio() -> i32 {
    let stdin = std::io::stdin();
    let mut stdin = stdin.lock();
    let mut stdout = std::io::stdout();
    let payload = match read_framed(&mut stdin) {
        Ok(p) => p,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "remote-worker: read frame: {e}");
            return 1;
        }
    };
    let job = match decode_job(&payload) {
        Ok(j) => j,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "remote-worker: decode job: {e}");
            return 1;
        }
    };
    let resp = run_job_local(&job);
    let out = match encode_resp(&resp) {
        Ok(b) => b,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "remote-worker: encode resp: {e}");
            return 1;
        }
    };
    if let Err(e) = write_framed(&mut stdout, &out) {
        let _ = writeln!(std::io::stderr(), "remote-worker: write frame: {e}");
        return 1;
    }
    if resp.ok {
        0
    } else {
        let _ = writeln!(std::io::stderr(), "remote-worker: {}", resp.err_msg);
        2
    }
}

pub fn ssh_invoke_remote_worker(
    host: &str,
    pe_bin: &str,
    job: &RemoteJobV1,
) -> Result<RemoteRespV1, String> {
    let payload = encode_job(job)?;
    let mut child = Command::new("ssh")
        .arg(host)
        .arg(pe_bin)
        .arg("--remote-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("ssh: {e}"))?;
    let mut stdin = child.stdin.take().ok_or_else(|| "ssh: stdin".to_string())?;
    write_framed(&mut stdin, &payload).map_err(|e| format!("ssh stdin: {e}"))?;
    drop(stdin);
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "ssh: stdout".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "ssh: stderr".to_string())?;
    let stderr_task = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    let out_bytes = read_framed(&mut stdout).map_err(|e| format!("ssh read frame: {e}"))?;
    let status = child.wait().map_err(|e| format!("ssh wait: {e}"))?;
    let stderr_text = stderr_task.join().unwrap_or_default();
    if !status.success() {
        return Err(format!(
            "ssh remote pe exited {:?}: {}",
            status.code(),
            stderr_text.trim()
        ));
    }
    decode_resp(&out_bytes).map_err(|e| {
        format!(
            "decode remote response: {e}; stderr: {}",
            stderr_text.trim()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_resp_msg_bincode_roundtrip() {
        let msg = JobRespMsg {
            seq: 1,
            ok: true,
            result: serde_json::json!(42i64),
            err_msg: String::new(),
        };
        let bytes = bincode::serialize(&msg).unwrap();
        let back: JobRespMsg = bincode::deserialize(&bytes).unwrap();
        assert_eq!(back.seq, msg.seq);
        assert_eq!(back.ok, msg.ok);
        assert_eq!(back.result, msg.result);
        assert_eq!(back.err_msg, msg.err_msg);
    }

    #[test]
    fn local_roundtrip_doubles() {
        let job = RemoteJobV1 {
            seq: 0,
            subs_prelude: String::new(),
            block_src: "$_ * 2;".to_string(),
            capture: vec![],
            item: serde_json::json!(21),
        };
        let r = run_job_local(&job);
        assert!(r.ok, "{}", r.err_msg);
        assert_eq!(r.result, serde_json::json!(42));
    }
}
