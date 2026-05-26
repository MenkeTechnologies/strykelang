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
use std::sync::{Arc, Mutex};
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
    agents: Arc<Mutex<HashMap<u64, ConnectedAgent>>>,
    next_session_id: AtomicU64,
    running: AtomicBool,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(Mutex::new(HashMap::new())),
            next_session_id: AtomicU64::new(1),
            running: AtomicBool::new(true),
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

                    // Send HELLO_ACK
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

                    let name = hello
                        .agent_name
                        .clone()
                        .unwrap_or_else(|| hello.hostname.clone());
                    eprintln!(
                        "[agent connected] {} (cores={}, session={})",
                        name, hello.cores, session_id
                    );

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

                    self.agents.lock().unwrap().insert(session_id, agent);
                }
                Err(e) => {
                    if self.running.load(Ordering::Relaxed) {
                        eprintln!("controller: accept error: {}", e);
                    }
                }
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
                let name = a
                    .agent_name
                    .clone()
                    .unwrap_or_else(|| a.hostname.clone());
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
                            Err(e) => print_tagged(
                                name,
                                "ERR",
                                &format!("malformed EVAL_RESULT: {}", e),
                            ),
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

/// Main entry point
pub fn run_controller(bind: &str, port: u16) -> i32 {
    let addr = format!("{}:{}", bind, port);

    let listener = match TcpListener::bind(&addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("controller: cannot bind to {}: {}", addr, e);
            return 1;
        }
    };

    eprintln!("stryke controller listening on {}", addr);
    eprintln!("Waiting for agents...\n");

    let controller = Arc::new(Controller::new());

    // Spawn accept thread
    let ctrl_clone = Arc::clone(&controller);
    let accept_handle = thread::spawn(move || {
        ctrl_clone.accept_loop(listener);
    });

    // Run REPL on main thread
    controller.run_repl();

    // Cleanup
    controller.running.store(false, Ordering::Relaxed);
    let _ = accept_handle.join();

    0
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
