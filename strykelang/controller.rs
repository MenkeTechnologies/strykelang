//! `stryke controller` — Interactive REPL for coordinating stress test agents.
//!
//! ## Usage
//!
//! ```sh
//! stryke controller                    # listen on 0.0.0.0:9999
//! stryke controller --port 8888        # custom port
//! stryke controller --bind 10.0.0.1    # specific interface
//! ```
//!
//! ## Commands
//!
//! - `status` — list connected agents
//! - `fire [duration]` — start stress test on all agents
//! - `fire node1,node2 [duration]` — specific agents
//! - `terminate` — stop stress test
//! - `shutdown` — disconnect all agents and exit
//! - `help` — show commands

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Instant;

use crate::agent::{
    frame_kind, AgentHello, AgentHelloAck, AgentState, EvalCommand, EvalResult, FireCommand,
    WorkloadType, AGENT_PROTO_VERSION,
};
use std::time::Duration;

/// Connected agent state
struct ConnectedAgent {
    stream: TcpStream,
    hostname: String,
    cores: usize,
    memory_bytes: u64,
    agent_name: Option<String>,
    state: AgentState,
    #[allow(dead_code)]
    session_id: u64,
    connected_at: Instant,
}

/// Controller state
pub struct Controller {
    /// `agents` field.
    agents: Arc<Mutex<HashMap<u64, ConnectedAgent>>>,
    /// `next_session_id` field.
    next_session_id: AtomicU64,
    /// `running` field.
    running: AtomicBool,
    /// Active chants — fired at each new agent registered by accept_loop.
    /// Lives on Controller (not ControllerHandle) so accept_loop can read
    /// it via `self` without a separate channel.
    chants: Arc<Mutex<HashMap<u64, String>>>,
    /// :cloistered mode — when true, accept_loop rejects agents whose
    /// AGENT_AUTH frame doesn't carry a token in `auth_tokens`. When
    /// false (default), agents bypass the AUTH check entirely.
    cloistered: AtomicBool,
    /// Valid AUTH tokens for cloistered mode. Populated by
    /// `set_cloistered(token)` and checked in accept_loop against the
    /// incoming AGENT_AUTH frame's token field.
    auth_tokens: Arc<Mutex<std::collections::HashSet<String>>>,
    /// When true, accept_loop suppresses the per-agent "[agent connected]"
    /// eprintln. Used during bulk-spawn (large `congregation(N)` /
    /// `anoint(N)`) where the main thread is in a tight fork loop and a
    /// concurrent eprintln from this background thread can leave the
    /// child with a borrowed `std::io::stderr` RefCell — guaranteed
    /// panic on the child's first stdio call. Toggled via
    /// [`ControllerHandle::set_quiet_accept`].
    quiet_accept: AtomicBool,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    /// `new` — see implementation.
    pub fn new() -> Self {
        Self {
            agents: Arc::new(Mutex::new(HashMap::new())),
            next_session_id: AtomicU64::new(1),
            running: AtomicBool::new(true),
            chants: Arc::new(Mutex::new(HashMap::new())),
            cloistered: AtomicBool::new(false),
            auth_tokens: Arc::new(Mutex::new(std::collections::HashSet::new())),
            quiet_accept: AtomicBool::new(false),
        }
    }

    /// Accept incoming agent connections
    fn accept_loop(&self, listener: TcpListener) {
        for stream in listener.incoming() {
            if !self.running.load(Ordering::Relaxed) {
                break;
            }

            match stream {
                Ok(mut stream) => {
                    let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);

                    // Read AGENT_HELLO
                    let (kind, payload) = match read_frame(&mut stream) {
                        Ok(f) => f,
                        Err(e) => {
                            eprintln!("controller: failed to read hello: {}", e);
                            continue;
                        }
                    };

                    if kind != frame_kind::AGENT_HELLO {
                        eprintln!("controller: expected AGENT_HELLO, got {}", kind);
                        continue;
                    }

                    let hello: AgentHello = match bincode::deserialize(&payload) {
                        Ok(h) => h,
                        Err(e) => {
                            eprintln!("controller: invalid hello: {}", e);
                            continue;
                        }
                    };

                    if hello.proto_version != AGENT_PROTO_VERSION {
                        let ack = AgentHelloAck {
                            session_id: 0,
                            accepted: false,
                            message: format!(
                                "protocol version mismatch: got {}, expected {}",
                                hello.proto_version, AGENT_PROTO_VERSION
                            ),
                        };
                        let ack_bytes = bincode::serialize(&ack).unwrap();
                        let _ = write_frame(&mut stream, frame_kind::AGENT_HELLO_ACK, &ack_bytes);
                        continue;
                    }

                    let name = hello
                        .agent_name
                        .clone()
                        .unwrap_or_else(|| hello.hostname.clone());

                    // :cloistered mode — require an AGENT_AUTH frame with
                    // a valid token within 500ms of HELLO. Agents in open
                    // mode don't send AUTH; we'd block forever waiting if
                    // we required it unconditionally, so this read only
                    // happens when cloistered is true. The check happens
                    // BEFORE we send the success ACK so rejected agents
                    // get a single accepted=false ACK, not an accepted=true
                    // followed by a rejection.
                    if self.cloistered.load(Ordering::Relaxed) {
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                        let auth_result = read_frame(&mut stream);
                        let _ = stream.set_read_timeout(None);
                        let auth_token: Option<String> = match auth_result {
                            Ok((frame_kind::AGENT_AUTH, payload)) => {
                                bincode::deserialize::<crate::agent::AgentAuth>(&payload)
                                    .ok()
                                    .map(|a| a.token)
                            }
                            _ => None,
                        };
                        let valid = auth_token
                            .as_ref()
                            .map(|tok| self.auth_tokens.lock().unwrap().contains(tok))
                            .unwrap_or(false);
                        if !valid {
                            eprintln!("[cloistered] rejected agent {} — no/bad AUTH token", name);
                            let rej = AgentHelloAck {
                                session_id: 0,
                                accepted: false,
                                message: "cloistered: missing or invalid AUTH token".into(),
                            };
                            if let Ok(rb) = bincode::serialize(&rej) {
                                let _ = write_frame(&mut stream, frame_kind::AGENT_HELLO_ACK, &rb);
                            }
                            continue;
                        }
                    }

                    // Send accepted-HELLO_ACK now that any cloistered check
                    // has passed.
                    let ack = AgentHelloAck {
                        session_id,
                        accepted: true,
                        message: "connected".to_string(),
                    };
                    let ack_bytes = bincode::serialize(&ack).unwrap();
                    if let Err(e) =
                        write_frame(&mut stream, frame_kind::AGENT_HELLO_ACK, &ack_bytes)
                    {
                        eprintln!("controller: failed to send hello ack: {}", e);
                        continue;
                    }

                    if !self.quiet_accept.load(Ordering::Relaxed) {
                        eprintln!(
                            "[agent connected] {} (cores={}, session={})",
                            name, hello.cores, session_id
                        );
                    }

                    let agent = ConnectedAgent {
                        stream,
                        hostname: hello.hostname,
                        cores: hello.cores,
                        memory_bytes: hello.memory_bytes,
                        agent_name: hello.agent_name,
                        state: AgentState::Idle,
                        session_id,
                        connected_at: Instant::now(),
                    };

                    // Insert into roster, then fire any active chants at
                    // this new joiner so late-comers receive the same
                    // ongoing prayers as everyone else.
                    {
                        let mut agents = self.agents.lock().unwrap();
                        agents.insert(session_id, agent);
                        // Drop the lock before issuing chants — fire_chant
                        // reacquires it.
                    }
                    self.fire_chants_at(session_id);
                }
                Err(e) => {
                    if self.running.load(Ordering::Relaxed) {
                        eprintln!("controller: accept error: {}", e);
                    }
                }
            }
        }
    }

    /// Fire every currently-active chant at the named agent. Called by
    /// `accept_loop` after a new agent is registered so late joiners
    /// receive the same continuous prayers as everyone else. Errors
    /// (write failures from a disconnecting agent) are silently swallowed
    /// — same convention as `scatter`.
    fn fire_chants_at(&self, session_id: u64) {
        // Snapshot the chant codes so we don't hold the chants lock while
        // doing IO under the agents lock.
        let chant_codes: Vec<String> = {
            let chants = self.chants.lock().unwrap();
            chants.values().cloned().collect()
        };
        if chant_codes.is_empty() {
            return;
        }
        let mut agents = self.agents.lock().unwrap();
        let agent = match agents.get_mut(&session_id) {
            Some(a) => a,
            None => return,
        };
        for code in chant_codes {
            let cmd = EvalCommand { code };
            if let Ok(bytes) = bincode::serialize(&cmd) {
                let _ = write_frame(&mut agent.stream, frame_kind::EVAL, &bytes);
            }
        }
    }

    /// Send FIRE to all agents
    fn fire_all(&self, duration_secs: f64) {
        let cmd = FireCommand {
            workload: WorkloadType::Cpu,
            duration_secs,
            intensity: 1.0,
        };
        let cmd_bytes = bincode::serialize(&cmd).unwrap();

        let mut agents = self.agents.lock().unwrap();
        let mut fired = 0;

        for agent in agents.values_mut() {
            if write_frame(&mut agent.stream, frame_kind::FIRE, &cmd_bytes).is_ok() {
                agent.state = AgentState::Firing;
                fired += 1;
            }
        }

        eprintln!("[fire] {} agents, duration={}s", fired, duration_secs);
    }

    /// Send EVAL to every connected agent, then synchronously collect each agent's
    /// `EvalResult` and print it to stdout. Per-agent the path is request/response:
    /// stale frames from previous commands (`METRICS` after a long-running `FIRE`,
    /// etc.) are quietly skipped so the next visible line is always the eval result.
    /// A 30-second read timeout guards against agents that ignore the frame entirely
    /// (e.g. an old agent version with no EVAL handler).
    ///
    /// Output ordering: agents are visited in **stable alphabetical order by display
    /// name** (agent_name if set, else hostname) so successive controllers and
    /// successive `@eval` calls within one controller produce comparable transcripts.
    /// HashMap iteration order would shuffle per controller run otherwise (Rust
    /// randomizes the hash seed per process).
    ///
    /// Multi-line output is prefixed **per line**: each `\n`-separated line of an
    /// agent's stringified result carries its own `[name/ok|ERR]` tag so grepping
    /// or diffing transcripts by agent stays trivial regardless of result shape.
    ///
    /// **Concurrent execution.** Done as a two-pass loop with no threading or
    /// concurrency primitives:
    ///
    ///   * **Pass 1** writes the EVAL frame to every agent in rapid succession
    ///     (each `write_frame` is just a kernel send, no waiting for the reply).
    ///     By the end of pass 1 every agent is already executing in parallel.
    ///   * **Pass 2** reads the EVAL_RESULT back from each agent in the same
    ///     sorted order.
    ///
    /// Total wall time = max(per-agent latency), not sum — three agents that
    /// each take 5 s now finish in ~5 s wall, not 15. Output stays alphabetical
    /// because pass 2 reads in the same order pass 1 wrote.
    fn eval_all(&self, code: &str) {
        let cmd = EvalCommand {
            code: code.to_string(),
        };
        let cmd_bytes = bincode::serialize(&cmd).expect("serialize EvalCommand");

        let mut agents = self.agents.lock().unwrap();
        if agents.is_empty() {
            println!("[eval] no agents connected");
            return;
        }

        // Build a stable visit order by display name. Done inside the mutex guard
        // so the (id → name) snapshot can't be invalidated by an accept thread.
        let mut order: Vec<(u64, String)> = agents
            .iter()
            .map(|(id, a)| {
                let name = a.agent_name.clone().unwrap_or_else(|| a.hostname.clone());
                (*id, name)
            })
            .collect();
        order.sort_by(|a, b| a.1.cmp(&b.1));

        // Pass 1 — fan out: write EVAL to every agent, set its read timeout.
        // Tracks (id, name) pairs we successfully dispatched to, so pass 2 only
        // tries to read from agents that actually received the frame.
        let mut dispatched: Vec<(u64, String)> = Vec::with_capacity(order.len());
        for (id, name) in &order {
            let agent = match agents.get_mut(id) {
                Some(a) => a,
                None => continue,
            };
            if let Err(e) = write_frame(&mut agent.stream, frame_kind::EVAL, &cmd_bytes) {
                print_tagged(name, "ERR", &format!("write error: {}", e));
                continue;
            }
            let _ = agent.stream.set_read_timeout(Some(Duration::from_secs(30)));
            dispatched.push((*id, name.clone()));
        }
        // At this point every dispatched agent is executing concurrently.

        // Pass 2 — collect: read EVAL_RESULT from each agent in sorted order.
        for (id, name) in &dispatched {
            let agent = match agents.get_mut(id) {
                Some(a) => a,
                None => continue,
            };
            loop {
                match read_frame(&mut agent.stream) {
                    Ok((frame_kind::EVAL_RESULT, payload)) => {
                        match bincode::deserialize::<EvalResult>(&payload) {
                            Ok(r) => {
                                let tag = if r.ok { "ok" } else { "ERR" };
                                print_tagged(name, tag, &r.output);
                            }
                            Err(e) => {
                                print_tagged(name, "ERR", &format!("malformed EVAL_RESULT: {}", e))
                            }
                        }
                        break;
                    }
                    Ok((other_kind, _)) => {
                        eprintln!(
                            "[{}] (dropped stale frame kind 0x{:02X} while awaiting EVAL_RESULT)",
                            name, other_kind
                        );
                    }
                    Err(e) => {
                        print_tagged(name, "ERR", &format!("read error: {}", e));
                        break;
                    }
                }
            }
            let _ = agent.stream.set_read_timeout(None);
        }
    }

    /// Send TERMINATE to all agents
    fn terminate_all(&self) {
        let mut agents = self.agents.lock().unwrap();
        let mut terminated = 0;

        for agent in agents.values_mut() {
            if write_frame(&mut agent.stream, frame_kind::TERMINATE, &[]).is_ok() {
                agent.state = AgentState::Terminated;
                terminated += 1;
            }
        }

        eprintln!("[terminate] {} agents", terminated);
    }

    /// Print status of all agents
    fn print_status(&self) {
        let agents = self.agents.lock().unwrap();

        if agents.is_empty() {
            println!("No agents connected.");
            return;
        }

        println!(
            "{:<20} {:>6} {:>10} {:>12} {:>10}",
            "AGENT", "CORES", "MEMORY", "STATE", "UPTIME"
        );
        println!("{}", "-".repeat(62));

        for agent in agents.values() {
            let name = agent
                .agent_name
                .clone()
                .unwrap_or_else(|| agent.hostname.clone());
            let mem_gb = agent.memory_bytes / (1024 * 1024 * 1024);
            let state = match agent.state {
                AgentState::Idle => "idle",
                AgentState::Armed => "armed",
                AgentState::Firing => "FIRING",
                AgentState::Terminated => "terminated",
            };
            let uptime = agent.connected_at.elapsed().as_secs();

            println!(
                "{:<20} {:>6} {:>8}GB {:>12} {:>8}s",
                name, agent.cores, mem_gb, state, uptime
            );
        }

        let total_cores: usize = agents.values().map(|a| a.cores).sum();
        let firing_count = agents
            .values()
            .filter(|a| a.state == AgentState::Firing)
            .count();

        println!();
        println!(
            "Total: {} agents, {} cores, {} firing",
            agents.len(),
            total_cores,
            firing_count
        );
    }

    /// Send SHUTDOWN to all agents
    fn shutdown_all(&self) {
        let mut agents = self.agents.lock().unwrap();

        for agent in agents.values_mut() {
            let _ = write_frame(&mut agent.stream, frame_kind::SHUTDOWN, &[]);
        }

        agents.clear();
        self.running.store(false, Ordering::Relaxed);
        eprintln!("[shutdown] all agents disconnected");
    }

    /// Run the REPL
    fn run_repl(&self) {
        use std::io::{stdin, BufRead};

        println!("stryke controller v{}", env!("CARGO_PKG_VERSION"));
        println!("Type 'help' for commands, Ctrl-C to exit\n");

        let stdin = stdin();
        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };

            // `@` prefix: ship the rest of the line as stryke source to every
            // agent. Matches the sigil the user already associates with `@` in
            // the language, and saves four keystrokes vs the explicit `eval`
            // verb. `@   code`, `@code`, `@code with args` all work — the `@`
            // is stripped and the remainder is sent verbatim.
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix('@') {
                let code = rest.trim();
                if code.is_empty() {
                    println!("usage: @CODE  (alias for `eval CODE`)");
                } else {
                    self.eval_all(code);
                }
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            match parts[0] {
                "status" | "s" => self.print_status(),
                "fire" | "f" => {
                    let duration = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(10.0);
                    self.fire_all(duration);
                }
                "eval" | "e" => {
                    // Everything after the verb (preserving inner whitespace) is the source.
                    let after = line
                        .trim_start()
                        .splitn(2, char::is_whitespace)
                        .nth(1)
                        .unwrap_or("")
                        .trim();
                    if after.is_empty() {
                        println!("usage: eval CODE  (sends CODE to every connected agent for execution against its persistent VM)");
                    } else {
                        self.eval_all(after);
                    }
                }
                "terminate" | "t" | "stop" => self.terminate_all(),
                "shutdown" | "quit" | "exit" | "q" => {
                    self.shutdown_all();
                    break;
                }
                "help" | "h" | "?" => {
                    println!("Commands:");
                    println!("  status (s)           List connected agents");
                    println!("  fire [SECS] (f)      Start stress test (default: 10s)");
                    println!("  eval CODE (e)        Run arbitrary stryke source on every agent (state persists across calls)");
                    println!("  @CODE                Shorthand for `eval CODE` — `@<source>` ships <source> to every agent");
                    println!("  terminate (t)        Stop stress test");
                    println!("  shutdown (q)         Disconnect all and exit");
                    println!("  help (h)             Show this help");
                }
                _ => println!("Unknown command: {}. Type 'help' for commands.", parts[0]),
            }
        }
    }
}

/// Read a framed message
/// Print `output` to stdout with every line prefixed `[name/tag] `. Empty
/// output produces a single bare `[name/tag]` line so the caller always sees
/// a row per agent even when the eval returned void.
fn print_tagged(name: &str, tag: &str, output: &str) {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let _ = write_tagged(&mut handle, name, tag, output);
}

/// Inner workhorse for [`print_tagged`], generic over `Write` so tests can
/// observe the exact bytes that go to stdout.
fn write_tagged<W: Write>(w: &mut W, name: &str, tag: &str, output: &str) -> std::io::Result<()> {
    if output.is_empty() {
        writeln!(w, "[{}/{}]", name, tag)?;
        return Ok(());
    }
    for ln in output.lines() {
        writeln!(w, "[{}/{}] {}", name, tag, ln)?;
    }
    // Preserve a trailing newline in the source output (e.g. `p "x"` ends with \n)
    // by emitting a bare prefixed line, so successive evals don't visually run
    // into each other.
    if output.ends_with('\n') {
        writeln!(w, "[{}/{}]", name, tag)?;
    }
    Ok(())
}

fn read_frame<R: Read>(r: &mut R) -> std::io::Result<(u8, Vec<u8>)> {
    let mut len_buf = [0u8; 8];
    r.read_exact(&mut len_buf)?;
    let len = u64::from_le_bytes(len_buf) as usize;
    if len < 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "empty frame",
        ));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    let kind = payload[0];
    Ok((kind, payload[1..].to_vec()))
}

/// Write a framed message
fn write_frame<W: Write>(w: &mut W, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    let total_len = 1 + payload.len();
    w.write_all(&(total_len as u64).to_le_bytes())?;
    w.write_all(&[kind])?;
    w.write_all(payload)?;
    w.flush()
}

/// Main entry point — back-compat wrapper that delegates to
/// [`spawn_controller`] + [`ControllerHandle::run_repl_blocking`]. Preserves
/// the historical CLI behaviour: bind, accept agents in a background thread,
/// run the interactive REPL on the main thread, cleanly join the accept thread
/// on REPL exit. Scripts that want non-REPL programmatic access use
/// [`spawn_controller`] directly.
pub fn run_controller(bind: &str, port: u16) -> i32 {
    let handle = match spawn_controller(bind, port) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("controller: cannot bind to {}:{}: {}", bind, port, e);
            return 1;
        }
    };

    eprintln!("stryke controller listening on {}", handle.listen_addr());
    eprintln!("Waiting for agents...\n");

    handle.run_repl_blocking();
    0
}

// ============================================================================
//                  Programmatic / Scriptable API (Tier 0)
// ============================================================================
//
// Lets `.stk` scripts drive the controller without the REPL. Builtins in
// `builtins.rs` (`ordain`, `muster`, `pray`, `annex`) wrap these methods and
// expose opaque integer IDs to script code via the registries below.
//
// Semantics:
//   * `spawn_controller(bind, port)` — bind listener, start accept thread,
//     return `Arc<ControllerHandle>`. Non-blocking.
//   * `ControllerHandle::muster()` — current session IDs of connected agents.
//   * `ControllerHandle::welcome(n, timeout)` — block until >= n agents have
//     connected, or the timeout elapses.
//   * `ControllerHandle::scatter(code, &[id])` — write EVAL to each agent in
//     parallel (fan-out pass), return a `petition_id`. Agents start executing
//     immediately; results are NOT collected here.
//   * `ControllerHandle::gather(petition_id, timeout)` — read EVAL_RESULT
//     from every agent dispatched in that scatter, in parallel, return a
//     HashMap<session_id, EvalResult>. Removes the divination from the
//     pending table once consumed (so the same petition_id can't be
//     gathered twice).
//   * `ControllerHandle::shutdown()` — send SHUTDOWN to all agents, mark
//     accept loop done, wake it with a self-connect, join the accept thread.
//
// Threading note: scatter + gather both lock the `agents` map for the duration
// of the operation. This serializes against the accept loop briefly, but lets
// us reuse the same TcpStream for both write and read passes without cloning
// or per-agent reader threads. Multi-outstanding-petition concurrency is a
// Tier 1 problem.

/// One outstanding scatter. Records the session IDs the EVAL was actually
/// written to so `gather` only reads from agents that received the frame.
struct DivinationState {
    dispatched: Vec<u64>,
}

/// Non-blocking handle to a running [`Controller`]. Returned by
/// [`spawn_controller`]; used by the scriptable builtins to drive the
/// distributed compute fabric from `.stk` code.
pub struct ControllerHandle {
    /// `controller` field.
    controller: Arc<Controller>,
    /// `listen_addr` field.
    listen_addr: std::net::SocketAddr,
    /// `accept_handle` field.
    accept_handle: Mutex<Option<thread::JoinHandle<()>>>,
    /// `next_petition_id` field.
    next_petition_id: AtomicU64,
    /// `pending_divinations` field.
    pending_divinations: Mutex<HashMap<u64, DivinationState>>,
}

impl ControllerHandle {
    /// Returns the address the listener is actually bound to (port may have
    /// been auto-assigned if 0 was passed in).
    pub fn listen_addr(&self) -> std::net::SocketAddr {
        self.listen_addr
    }

    /// Current count of connected agents.
    pub fn agent_count(&self) -> usize {
        self.controller.agents.lock().unwrap().len()
    }

    /// Return session IDs of all currently connected agents, in numerically
    /// sorted order (deterministic for tests and scripts).
    pub fn muster(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .controller
            .agents
            .lock()
            .unwrap()
            .keys()
            .copied()
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Block until at least `target_count` agents are connected, or `timeout`
    /// elapses. Returns `true` if the count was reached. Polls every 50ms.
    pub fn welcome(&self, target_count: usize, timeout: Duration) -> bool {
        let start = Instant::now();
        loop {
            if self.agent_count() >= target_count {
                return true;
            }
            if start.elapsed() >= timeout {
                return self.agent_count() >= target_count;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Fan EVAL out to `agent_ids` in parallel. Returns a `petition_id` used
    /// later with [`gather`](Self::gather) to collect results. Agents that
    /// don't exist in the roster or whose write fails are silently dropped
    /// from the dispatched set, so a stale agent_id in `agent_ids` does NOT
    /// cause the whole scatter to fail.
    ///
    /// Returns `Err` only if the EvalCommand fails to bincode-serialize
    /// (which should never happen for plain strings).
    pub fn scatter(&self, code: &str, agent_ids: &[u64]) -> std::io::Result<u64> {
        use rayon::prelude::*;

        let cmd = EvalCommand {
            code: code.to_string(),
        };
        let cmd_bytes = bincode::serialize(&cmd)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // Tier 2: parallel fanout. Take the mutex once to grab per-agent
        // TcpStream clones (sharing the underlying socket), drop the mutex,
        // then write_frame on each clone in parallel via Rayon. The N
        // sequential write_frames in the prior impl scaled O(N) at ~10μs
        // per write; parallel scaling brings 10k-agent fanout from ~100ms
        // down to ~1ms on an 8-core box.
        //
        // Stream clones share the underlying fd — writing to a clone IS
        // writing to the agent's socket. Reads in `gather` go through the
        // original (which we kept in the HashMap), so writer and reader
        // don't conflict.
        let stream_clones: Vec<(u64, TcpStream)> = {
            let agents = self.controller.agents.lock().unwrap();
            agent_ids
                .iter()
                .filter_map(|id| {
                    agents
                        .get(id)
                        .and_then(|a| a.stream.try_clone().ok().map(|s| (*id, s)))
                })
                .collect()
        };

        let cmd_bytes = Arc::new(cmd_bytes);
        let dispatched: Vec<u64> = stream_clones
            .into_par_iter()
            .filter_map(|(id, mut stream)| {
                if write_frame(&mut stream, frame_kind::EVAL, cmd_bytes.as_slice()).is_ok() {
                    Some(id)
                } else {
                    None
                }
            })
            .collect();

        let petition_id = self.next_petition_id.fetch_add(1, Ordering::Relaxed);
        self.pending_divinations
            .lock()
            .unwrap()
            .insert(petition_id, DivinationState { dispatched });
        Ok(petition_id)
    }

    /// Block for results of `petition_id` up to `timeout` (per-agent read
    /// timeout, not total wall time). Returns a HashMap of session_id →
    /// EvalResult for every agent that replied with a valid EVAL_RESULT
    /// frame in time. Agents that timed out, errored, or disconnected are
    /// omitted from the map.
    ///
    /// Stale frames in the per-agent socket buffer (e.g. METRICS left over
    /// from a prior FIRE) are silently skipped — same loop pattern as
    /// `eval_all` so manual REPL behaviour stays consistent with scripted.
    ///
    /// Removes the divination from the pending table on return, so a
    /// second `gather` on the same petition_id is an error.
    pub fn gather(
        &self,
        petition_id: u64,
        timeout: Duration,
    ) -> std::io::Result<HashMap<u64, EvalResult>> {
        let state = self
            .pending_divinations
            .lock()
            .unwrap()
            .remove(&petition_id)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("no pending divination for petition_id {}", petition_id),
                )
            })?;

        let mut results: HashMap<u64, EvalResult> = HashMap::new();
        let mut agents = self.controller.agents.lock().unwrap();
        for id in &state.dispatched {
            let agent = match agents.get_mut(id) {
                Some(a) => a,
                None => continue, // agent disconnected between scatter and gather
            };
            let _ = agent.stream.set_read_timeout(Some(timeout));
            loop {
                match read_frame(&mut agent.stream) {
                    Ok((frame_kind::EVAL_RESULT, payload)) => {
                        if let Ok(r) = bincode::deserialize::<EvalResult>(&payload) {
                            results.insert(*id, r);
                        }
                        break;
                    }
                    Ok(_) => continue, // stale frame, skip
                    Err(_) => break,   // timeout or disconnect
                }
            }
            let _ = agent.stream.set_read_timeout(None);
        }
        Ok(results)
    }

    /// Register an active chant — a prayer that fires at every current
    /// agent now AND at every new agent that joins later (via the
    /// `accept_loop` → `fire_chants_at` path). Returns a chant_id used
    /// by `amen_chant` to stop the rescatter.
    ///
    /// Fire-and-forget: chants don't accumulate replies. Use for state
    /// distribution (`bestow`-like push to current + future workers).
    pub fn chant(&self, code: &str, agent_ids: &[u64]) -> std::io::Result<u64> {
        // Fan out to current agents using the regular scatter machinery
        // — we just don't register a divination since there's no gather.
        let cmd = EvalCommand {
            code: code.to_string(),
        };
        let cmd_bytes = bincode::serialize(&cmd)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut agents = self.controller.agents.lock().unwrap();
        for id in agent_ids {
            if let Some(agent) = agents.get_mut(id) {
                let _ = write_frame(&mut agent.stream, frame_kind::EVAL, &cmd_bytes);
            }
        }
        drop(agents);

        // Record in active_chants so late joiners get it too.
        let chant_id = NEXT_CHANT_ID.fetch_add(1, Ordering::Relaxed);
        self.controller
            .chants
            .lock()
            .unwrap()
            .insert(chant_id, code.to_string());
        Ok(chant_id)
    }

    /// Stop an active chant. Late joiners after this call won't receive it.
    /// Returns true if the chant was active and removed, false otherwise.
    pub fn amen_chant(&self, chant_id: u64) -> bool {
        self.controller
            .chants
            .lock()
            .unwrap()
            .remove(&chant_id)
            .is_some()
    }

    /// Silence the accept_loop's per-agent "[agent connected]" eprintln.
    /// Set to true before a bulk-spawn loop (`congregation(N)` / `anoint(N)`)
    /// to prevent the fork-thread/stdio RefCell race that loses 1-3
    /// children at N>~50 on macOS Rust stdio. Set back to false after
    /// the spawn loop completes if you want the REPL UX back.
    pub fn set_quiet_accept(&self, quiet: bool) {
        self.controller.quiet_accept.store(quiet, Ordering::Relaxed);
    }

    /// Turn :cloistered mode on (with a single accepted token) or off
    /// (with an empty token). Cloistered accept_loop reads an AGENT_AUTH
    /// frame after HELLO and rejects agents that don't present a valid
    /// token. Open mode bypasses the AUTH read entirely.
    pub fn set_cloistered(&self, token: Option<&str>) {
        match token {
            Some(t) if !t.is_empty() => {
                self.controller
                    .auth_tokens
                    .lock()
                    .unwrap()
                    .insert(t.to_string());
                self.controller.cloistered.store(true, Ordering::Relaxed);
            }
            _ => {
                self.controller.cloistered.store(false, Ordering::Relaxed);
                self.controller.auth_tokens.lock().unwrap().clear();
            }
        }
    }

    /// Send SHUTDOWN to a specific subset of agents (the `excommunicate` verb).
    /// Each agent receives a SHUTDOWN frame and exits its loop; the agent's
    /// TCP connection is dropped. Returns the count of agents that the frame
    /// was successfully written to (write failures from disconnected agents
    /// are silently swallowed — same convention as `scatter`).
    pub fn excommunicate(&self, agent_ids: &[u64]) -> usize {
        let mut agents = self.controller.agents.lock().unwrap();
        let mut count = 0;
        for id in agent_ids {
            if let Some(agent) = agents.get_mut(id) {
                if write_frame(&mut agent.stream, frame_kind::SHUTDOWN, &[]).is_ok() {
                    count += 1;
                }
            }
        }
        // Best-effort removal from the roster. The accept thread won't see
        // the disconnect until the OS notices the dropped connection, so we
        // proactively drop them here.
        for id in agent_ids {
            agents.remove(id);
        }
        count
    }

    /// Pilgrimage barrier — scatter `barrier_code` to all `agent_ids` and
    /// block until every agent that received the frame replies, OR `timeout`
    /// elapses. Returns `true` if every dispatched agent replied in time.
    ///
    /// `barrier_code` is the prayer the agents execute at the barrier; for a
    /// pure rendezvous, pass `"1"` and the agent's reply is the synchronization
    /// signal. For computational barriers, pass code that does the work and
    /// returns when done.
    pub fn pilgrimage(&self, barrier_code: &str, agent_ids: &[u64], timeout: Duration) -> bool {
        let petition_id = match self.scatter(barrier_code, agent_ids) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let expected = self
            .pending_divinations
            .lock()
            .unwrap()
            .get(&petition_id)
            .map(|d| d.dispatched.len())
            .unwrap_or(0);
        match self.gather(petition_id, timeout) {
            Ok(results) => results.len() == expected,
            Err(_) => false,
        }
    }

    /// SHUTDOWN every agent, stop the accept loop, join the accept thread.
    /// Wakes the blocking `listener.incoming()` call by self-connecting to
    /// the bound address (the connection's HELLO read will fail and the
    /// accept thread will exit its loop now that `running` is false).
    pub fn shutdown(&self) {
        self.controller.shutdown_all();
        // Wake the accept loop by self-connecting; it'll see running=false
        // and break. We swallow any connect error — the listener may already
        // be gone if shutdown_all races with another shutdown call.
        let _ = TcpStream::connect(self.listen_addr);
        if let Some(h) = self.accept_handle.lock().unwrap().take() {
            let _ = h.join();
        }
    }

    /// Run the existing REPL on the calling thread, then clean shutdown.
    /// Used by [`run_controller`] for back-compat with the CLI subcommand.
    pub fn run_repl_blocking(&self) {
        self.controller.run_repl();
        self.controller.running.store(false, Ordering::Relaxed);
        let _ = TcpStream::connect(self.listen_addr);
        if let Some(h) = self.accept_handle.lock().unwrap().take() {
            let _ = h.join();
        }
    }
}

/// Bind a listener and start the accept thread, return a non-blocking handle.
/// Pass `port = 0` to let the OS pick a free port; recover the chosen one via
/// [`ControllerHandle::listen_addr`].
pub fn spawn_controller(bind: &str, port: u16) -> std::io::Result<Arc<ControllerHandle>> {
    let addr = format!("{}:{}", bind, port);
    let listener = TcpListener::bind(&addr)?;
    let listen_addr = listener.local_addr()?;

    let controller = Arc::new(Controller::new());
    let ctrl_clone = Arc::clone(&controller);
    let accept_handle = thread::spawn(move || {
        ctrl_clone.accept_loop(listener);
    });

    Ok(Arc::new(ControllerHandle {
        controller,
        listen_addr,
        accept_handle: Mutex::new(Some(accept_handle)),
        next_petition_id: AtomicU64::new(1),
        pending_divinations: Mutex::new(HashMap::new()),
    }))
}

// ─── Global handle registries (script ↔ Rust bridge) ────────────────────────
//
// Stryke scripts can't hold `Arc<ControllerHandle>` directly — they only see
// `StrykeValue`s. So we stash the live handle in a process-global registry
// and hand the script an opaque integer ID. Same pattern for divination
// handles (each `pray` call returns a divination ID).
//
// Both registries are `OnceLock<Mutex<HashMap>>` — initialised on first use,
// shared across all threads. The script-visible IDs are monotonic atomics
// so they never collide within a single process.

static CONTROLLER_REGISTRY: OnceLock<Mutex<HashMap<u64, Arc<ControllerHandle>>>> = OnceLock::new();
static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

fn controller_registry() -> &'static Mutex<HashMap<u64, Arc<ControllerHandle>>> {
    CONTROLLER_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a controller handle; returns a script-visible u64 ID.
pub fn register_controller(handle: Arc<ControllerHandle>) -> u64 {
    let id = NEXT_HANDLE_ID.fetch_add(1, Ordering::Relaxed);
    controller_registry().lock().unwrap().insert(id, handle);
    id
}

/// Look up a controller by its script-visible ID. Returns `None` if the ID
/// was never registered or has been unregistered.
pub fn get_controller(handle_id: u64) -> Option<Arc<ControllerHandle>> {
    controller_registry()
        .lock()
        .unwrap()
        .get(&handle_id)
        .map(Arc::clone)
}

/// Remove a controller from the registry. Caller typically also calls
/// `shutdown()` on the returned Arc before dropping it.
pub fn unregister_controller(handle_id: u64) -> Option<Arc<ControllerHandle>> {
    controller_registry().lock().unwrap().remove(&handle_id)
}

// ─── Chant ID atomic + registry ──────────────────────────────────────────────
//
// Chants get their own ID space (separate from divinations) so amen() can
// dispatch correctly. The CHANT_REGISTRY maps script-visible chant_id →
// controller_id so `amen` can find the right controller's active-chant table.

static NEXT_CHANT_ID: AtomicU64 = AtomicU64::new(1);
static CHANT_REGISTRY: OnceLock<Mutex<HashMap<u64, (u64, u64)>>> = OnceLock::new();

fn chant_registry() -> &'static Mutex<HashMap<u64, (u64, u64)>> {
    CHANT_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a chant in the global registry; returns the script-visible
/// chant_id. The pair stored is (controller_id, controller_local_chant_id).
pub fn register_chant(controller_id: u64, local_chant_id: u64) -> u64 {
    let id = NEXT_CHANT_ID.fetch_add(1, Ordering::Relaxed);
    chant_registry()
        .lock()
        .unwrap()
        .insert(id, (controller_id, local_chant_id));
    id
}
/// `get_chant` — see implementation.
pub fn get_chant(chant_id: u64) -> Option<(u64, u64)> {
    chant_registry().lock().unwrap().get(&chant_id).copied()
}
/// `unregister_chant` — see implementation.
pub fn unregister_chant(chant_id: u64) -> Option<(u64, u64)> {
    chant_registry().lock().unwrap().remove(&chant_id)
}

// ─── Cathedral — in-process named congregation registry ─────────────────────
//
// Maps "congregation name" → controller endpoint (host:port). Masters
// register themselves at `ordain("name", ...)`; slaves look up the
// endpoint at `profess("name")` and connect.
//
// In-process registry only (not a separate daemon). Tier 5+ work would
// promote this to a `stryked` standalone binary so cross-host congregations
// can resolve names; for now everything is within one shared OS-image
// process address space.

static CATHEDRAL: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn cathedral() -> &'static Mutex<HashMap<String, String>> {
    CATHEDRAL.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a congregation name → endpoint binding. Returns the previous
/// binding if any (so caller can detect collisions if it cares).
pub fn cathedral_register(name: &str, endpoint: &str) -> Option<String> {
    cathedral()
        .lock()
        .unwrap()
        .insert(name.to_string(), endpoint.to_string())
}

/// Look up a congregation name → endpoint. Returns None if unregistered.
pub fn cathedral_lookup(name: &str) -> Option<String> {
    cathedral().lock().unwrap().get(name).cloned()
}

/// Remove a name from the registry. Returns the endpoint that was bound.
pub fn cathedral_unregister(name: &str) -> Option<String> {
    cathedral().lock().unwrap().remove(name)
}

/// Enumerate registered congregation names (sorted).
pub fn cathedral_names() -> Vec<String> {
    let mut names: Vec<String> = cathedral().lock().unwrap().keys().cloned().collect();
    names.sort();
    names
}

// ─── Divination registry: divination_id → (controller_id, petition_id) ──────

static DIVINATION_REGISTRY: OnceLock<Mutex<HashMap<u64, (u64, u64)>>> = OnceLock::new();
static NEXT_DIVINATION_ID: AtomicU64 = AtomicU64::new(1);

fn divination_registry() -> &'static Mutex<HashMap<u64, (u64, u64)>> {
    DIVINATION_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a divination; returns a script-visible u64 ID that resolves back
/// to (controller_id, petition_id) via [`get_divination`].
pub fn register_divination(controller_id: u64, petition_id: u64) -> u64 {
    let id = NEXT_DIVINATION_ID.fetch_add(1, Ordering::Relaxed);
    divination_registry()
        .lock()
        .unwrap()
        .insert(id, (controller_id, petition_id));
    id
}

/// Look up a divination's (controller_id, petition_id) pair.
pub fn get_divination(divination_id: u64) -> Option<(u64, u64)> {
    divination_registry()
        .lock()
        .unwrap()
        .get(&divination_id)
        .copied()
}

/// Remove a divination from the registry. Returns the (controller_id,
/// petition_id) pair so the caller can route the actual gather request.
pub fn unregister_divination(divination_id: u64) -> Option<(u64, u64)> {
    divination_registry().lock().unwrap().remove(&divination_id)
}

// ─── Current-controller tracking for ergonomic script API ───────────────────
//
// Scripts that work with a single congregation (the overwhelmingly common
// case) shouldn't have to thread a controller handle through every `pray` /
// `muster` / `annex` call. We stash the most-recently-created controller
// here and `pray` / `muster` / `annex` fall back to it when no explicit
// controller is named.
//
// Scripts juggling multiple concurrent congregations pass the controller
// handle explicitly via the low-level `ordain` return value.

static CURRENT_CONTROLLER_ID: AtomicU64 = AtomicU64::new(0);

/// Make `controller_id` the implicit target for subsequent `pray` / `muster`
/// / `annex` calls that don't name a controller.
pub fn set_current_controller(controller_id: u64) {
    CURRENT_CONTROLLER_ID.store(controller_id, Ordering::Relaxed);
}

/// Return the current implicit controller, or `None` if no congregation /
/// ordination has happened yet in this process.
pub fn get_current_controller() -> Option<u64> {
    let id = CURRENT_CONTROLLER_ID.load(Ordering::Relaxed);
    if id == 0 {
        None
    } else {
        Some(id)
    }
}

/// Print controller help
pub fn print_help() {
    println!("stryke controller — Distributed load testing controller");
    println!();
    println!("USAGE:");
    println!("    stryke controller [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    --bind ADDR          Bind address (default: 0.0.0.0)");
    println!("    --port PORT          Listen port (default: 9999)");
    println!("    --help               Print this help");
    println!();
    println!("COMMANDS (in REPL):");
    println!("    status               List connected agents");
    println!("    fire [SECS]          Start stress test (default: 10 seconds)");
    println!("    terminate            Stop stress test");
    println!("    shutdown             Disconnect all agents and exit");
    println!();
    println!("EXAMPLE:");
    println!("    stryke controller --port 9999");
    println!();
    println!("    controller> status");
    println!("    controller> fire 60      # 60 second stress test");
    println!("    controller> terminate");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{frame_kind, handle_eval_frame, read_frame};
    use crate::vm_helper::VMHelper;
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    fn s(name: &str, tag: &str, output: &str) -> String {
        let mut buf = Vec::new();
        write_tagged(&mut buf, name, tag, output).unwrap();
        String::from_utf8(buf).unwrap()
    }

    /// Spawn a synthetic agent on a fresh loopback port that:
    ///   1. accepts one connection,
    ///   2. reads one EVAL frame,
    ///   3. sleeps `delay`,
    ///   4. replies with an EVAL_RESULT computed against a real `VMHelper`.
    /// Returns the controller-side `TcpStream` and the handle for the worker.
    fn spawn_synthetic_agent(delay: Duration) -> (TcpStream, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut server, _) = listener.accept().expect("accept");
            let mut interp = VMHelper::new();
            let (kind, payload) = read_frame(&mut server).expect("read EVAL");
            assert_eq!(kind, frame_kind::EVAL);
            thread::sleep(delay);
            handle_eval_frame(&mut server, &mut interp, &payload).expect("reply");
        });
        let client = TcpStream::connect(addr).expect("connect");
        (client, handle)
    }

    /// Build a `Controller` populated with the supplied mock agents (one
    /// `ConnectedAgent` per entry, ids 1..N, names "agent-NN").
    fn controller_with_agents(streams: Vec<TcpStream>) -> Controller {
        let controller = Controller::new();
        let mut agents = controller.agents.lock().unwrap();
        for (i, stream) in streams.into_iter().enumerate() {
            let id = (i + 1) as u64;
            agents.insert(
                id,
                ConnectedAgent {
                    stream,
                    hostname: "localhost".to_string(),
                    cores: 1,
                    memory_bytes: 0,
                    agent_name: Some(format!("agent-{:02}", id)),
                    state: AgentState::Idle,
                    session_id: id,
                    connected_at: Instant::now(),
                },
            );
        }
        drop(agents);
        controller
    }

    /// Single-line output: one tagged row, one trailing newline (from writeln!).
    #[test]
    fn single_line_output_emits_one_tagged_row() {
        assert_eq!(s("node-01", "ok", "42"), "[node-01/ok] 42\n");
    }

    /// Multi-line output: EVERY line carries the prefix — the wart this commit fixes.
    /// Without per-line prefixing, only the first line would have `[name/tag]` and a
    /// pipeline like `@p "a"; p "b"; p "c"; 0` would print three orphan lines.
    #[test]
    fn multi_line_output_prefixes_every_line() {
        let got = s("node-02", "ok", "alpha\nbeta\ngamma");
        assert_eq!(
            got, "[node-02/ok] alpha\n[node-02/ok] beta\n[node-02/ok] gamma\n",
            "every line must carry the [name/tag] prefix"
        );
    }

    /// Empty output is NOT swallowed — the caller still sees one bare prefixed line
    /// so void returns ("undef" stringifies to "") produce visible per-agent rows.
    #[test]
    fn empty_output_still_emits_a_row() {
        assert_eq!(s("node-03", "ok", ""), "[node-03/ok]\n");
    }

    /// Trailing newline in source (e.g. `p "x"` returns `"x\n"`) emits the visible
    /// content's tagged line plus one bare prefixed line so successive evals don't
    /// visually run into each other.
    #[test]
    fn trailing_newline_emits_blank_prefixed_terminator() {
        let got = s("node-04", "ok", "x\n");
        assert_eq!(got, "[node-04/ok] x\n[node-04/ok]\n");
    }

    /// Error tag formatting parallels the ok tag — no special casing.
    #[test]
    fn error_tag_format_matches_ok() {
        assert_eq!(
            s("node-05", "ERR", "Division by zero at -e line 1"),
            "[node-05/ERR] Division by zero at -e line 1\n"
        );
    }

    /// The two-pass fan-out in `eval_all` must execute agents **in parallel**, not
    /// serially. Three mock agents each sleep 250 ms before replying; with the
    /// previous serial loop the wall-clock would be ≥750 ms. With the parallel
    /// fan-out the writes go out in microseconds and the reads block on the
    /// slowest agent — total wall ≈ 250 ms. We assert well under the 750 ms serial
    /// bound to keep the test non-flaky under CI load while still failing loudly
    /// if anyone reintroduces serial dispatch.
    #[test]
    fn eval_all_executes_agents_in_parallel_not_serially() {
        const N: usize = 3;
        const DELAY: Duration = Duration::from_millis(250);
        const SERIAL_BOUND: Duration = Duration::from_millis(750); // N * DELAY

        let (s1, h1) = spawn_synthetic_agent(DELAY);
        let (s2, h2) = spawn_synthetic_agent(DELAY);
        let (s3, h3) = spawn_synthetic_agent(DELAY);
        let controller = controller_with_agents(vec![s1, s2, s3]);

        let start = Instant::now();
        controller.eval_all("1 + 1");
        let elapsed = start.elapsed();

        for h in [h1, h2, h3] {
            h.join().expect("agent thread");
        }

        assert!(
            elapsed < Duration::from_millis(600),
            "eval_all must run {} agents in parallel (each delay {:?}); elapsed {:?} \
             is too close to the serial bound {:?}",
            N,
            DELAY,
            elapsed,
            SERIAL_BOUND
        );
    }

    /// Empty controller (no agents connected) prints a notice and returns without
    /// panic / hang. Regression guard for the empty-agents branch.
    #[test]
    fn eval_all_with_no_agents_is_a_noop() {
        let controller = Controller::new();
        let start = Instant::now();
        controller.eval_all("anything");
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(100),
            "no-agents eval_all should return instantly, took {:?}",
            elapsed
        );
    }

    /// Stale frames left over in the TCP buffer from a prior FIRE (METRICS) or
    /// STATUS (STATUS_RESP) must be silently skipped so `eval_all` finds the
    /// EVAL_RESULT it's actually waiting for. Pins the behaviour at controller.rs's
    /// pass-2 inner loop.
    #[test]
    fn eval_all_skips_stale_frames_before_eval_result() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut server, _) = listener.accept().expect("accept");
            let mut interp = VMHelper::new();
            let (kind, payload) = read_frame(&mut server).expect("read EVAL");
            assert_eq!(kind, frame_kind::EVAL);
            // Send an unrelated frame FIRST (simulating a leftover METRICS from a
            // prior FIRE). Controller must drop it and keep waiting for EVAL_RESULT.
            super::write_frame(&mut server, frame_kind::METRICS, &[0u8; 4]).unwrap();
            handle_eval_frame(&mut server, &mut interp, &payload).expect("reply");
        });
        let client = TcpStream::connect(addr).unwrap();
        let controller = controller_with_agents(vec![client]);

        // If the stale-frame skipping is broken, eval_all hangs or deserializes
        // METRICS bytes as EvalResult and prints garbage. Either way the test fails
        // via the timeout / agent panic.
        let start = Instant::now();
        controller.eval_all("42");
        assert!(start.elapsed() < Duration::from_secs(5));
        handle.join().expect("agent thread");
    }

    /// Multiple `eval_all` calls against the same controller reuse the persistent
    /// per-agent `VMHelper`, so package globals set in call N are visible in N+1.
    /// Smoke test for the REPL-style semantics from the user's perspective.
    #[test]
    fn successive_eval_all_calls_share_per_agent_vm_state() {
        // Single agent that handles TWO EVAL frames against the same VM.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let (mut server, _) = listener.accept().expect("accept");
            let mut interp = VMHelper::new();
            for _ in 0..2 {
                let (kind, payload) = read_frame(&mut server).expect("read EVAL");
                assert_eq!(kind, frame_kind::EVAL);
                handle_eval_frame(&mut server, &mut interp, &payload).expect("reply");
            }
        });
        let client = TcpStream::connect(addr).unwrap();
        let controller = controller_with_agents(vec![client]);

        // Frame 1: define a package global.
        controller.eval_all("$main::tally = 10; $main::tally");
        // Frame 2: read it back. If state didn't persist, we'd see "" / undef.
        controller.eval_all("$main::tally + 32");
        // We don't have stdout capture here, so the assertion is via the synthetic
        // agent thread's panics — it errors if frame deserialisation fails.
        handle.join().expect("agent thread");
        // Force a real connection close so the agent thread above wakes.
        // (Controlled by the controller dropping at end of scope.)
        // The test passes if the agent handled exactly 2 frames without panicking.
    }

    /// `Arc<Controller>` clones share the agents map — the accept loop holds one
    /// clone and the REPL thread holds another. Sanity check that eval_all on a
    /// cloned controller sees the same agents as the original.
    #[test]
    fn arc_clone_shares_agents_map() {
        let (s, h) = spawn_synthetic_agent(Duration::from_millis(0));
        let controller = Arc::new(controller_with_agents(vec![s]));
        let clone = Arc::clone(&controller);
        clone.eval_all("99");
        h.join().expect("agent thread");
        // Sanity: the agent map carries one entry after the eval.
        assert_eq!(controller.agents.lock().unwrap().len(), 1);
    }
}
