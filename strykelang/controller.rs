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
    frame_kind, AgentHello, AgentHelloAck, AgentMetrics, AgentState, FireCommand, TermAck,
    WorkloadType, AGENT_PROTO_VERSION,
};

/// Connected agent state
struct ConnectedAgent {
    stream: TcpStream,
    hostname: String,
    cores: usize,
    memory_bytes: u64,
    agent_name: Option<String>,
    state: AgentState,
    session_id: u64,
    connected_at: Instant,
}

/// Controller state
pub struct Controller {
    agents: Arc<Mutex<HashMap<u64, ConnectedAgent>>>,
    next_session_id: AtomicU64,
    running: AtomicBool,
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
                    if let Err(e) = write_frame(&mut stream, frame_kind::AGENT_HELLO_ACK, &ack_bytes)
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

            let parts: Vec<&str> = line.trim().split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            match parts[0] {
                "status" | "s" => self.print_status(),
                "fire" | "f" => {
                    let duration = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(10.0);
                    self.fire_all(duration);
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
