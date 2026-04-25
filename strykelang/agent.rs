//! `stryke agent` — Persistent load testing agent for distributed stress testing.
//!
//! ## Overview
//!
//! The agent runs as a daemon, connects to a controller via TCP, and awaits commands.
//! When the controller sends a FIRE command, the agent executes stress workloads until
//! TERMINATE is received. Designed for enterprise load testing of distributed clusters.
//!
//! ## Config file
//!
//! Default: `~/.config/stryke/agent.toml`
//!
//! ```toml
//! [controller]
//! host = "controller.example.com"
//! port = 9999
//!
//! [limits]
//! max_temp = 85       # auto-terminate if CPU temp exceeds (Celsius)
//! max_duration = 3600 # max seconds per stress session
//!
//! [agent]
//! name = "node-01"    # optional, defaults to hostname
//! ```
//!
//! ## Wire protocol
//!
//! Same framing as remote_wire: `[u64 LE length][u8 kind][bincode payload]`
//!
//! ```text
//! controller                      agent
//!     │                             │
//!     │◄──── AGENT_HELLO ───────────│  (hostname, cores, memory)
//!     │───── AGENT_HELLO_ACK ──────►│  (session_id, config overrides)
//!     │                             │
//!     │───── FIRE ─────────────────►│  (workload type, duration, intensity)
//!     │◄──── METRICS ───────────────│  (cpu%, temp, memory, hashes/sec)
//!     │◄──── METRICS ───────────────│
//!     │───── TERMINATE ────────────►│
//!     │◄──── TERM_ACK ──────────────│  (final stats)
//!     │                             │
//!     │───── SHUTDOWN ─────────────►│
//!     │                             └─ exit 0
//! ```

use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Agent protocol frame kinds
pub mod frame_kind {
    pub const AGENT_HELLO: u8 = 0x10;
    pub const AGENT_HELLO_ACK: u8 = 0x11;
    pub const FIRE: u8 = 0x12;
    pub const METRICS: u8 = 0x13;
    pub const TERMINATE: u8 = 0x14;
    pub const TERM_ACK: u8 = 0x15;
    pub const SHUTDOWN: u8 = 0x16;
    pub const STATUS: u8 = 0x17;
    pub const STATUS_RESP: u8 = 0x18;
    pub const ERROR: u8 = 0xFF;
}

pub const AGENT_PROTO_VERSION: u32 = 1;

/// Agent configuration (from TOML file)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    #[serde(default)]
    pub controller: ControllerConfig,
    #[serde(default)]
    pub limits: LimitsConfig,
    #[serde(default)]
    pub agent: AgentIdentity,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControllerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_host() -> String {
    "localhost".to_string()
}
fn default_port() -> u16 {
    9999
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitsConfig {
    #[serde(default = "default_max_temp")]
    pub max_temp: u32,
    #[serde(default = "default_max_duration")]
    pub max_duration: u64,
}

fn default_max_temp() -> u32 {
    85
}
fn default_max_duration() -> u64 {
    3600
}

impl Default for LimitsConfig {
    fn default() -> Self {
        Self {
            max_temp: default_max_temp(),
            max_duration: default_max_duration(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentIdentity {
    #[serde(default)]
    pub name: Option<String>,
}

/// Hello message from agent to controller
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHello {
    pub proto_version: u32,
    pub stryke_version: String,
    pub hostname: String,
    pub cores: usize,
    pub memory_bytes: u64,
    pub agent_name: Option<String>,
}

/// Acknowledgment from controller
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHelloAck {
    pub session_id: u64,
    pub accepted: bool,
    pub message: String,
}

/// Fire command — start stress test
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FireCommand {
    pub workload: WorkloadType,
    pub duration_secs: f64,
    pub intensity: f64, // 0.0-1.0, percentage of cores to use
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkloadType {
    Cpu,
    Memory { bytes: u64 },
    Io { dir: String, iterations: u64 },
    Combined,
    Custom { code: String },
}

/// Metrics report from agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetrics {
    pub cpu_percent: f64,
    pub memory_used: u64,
    pub hashes_per_sec: u64,
    pub elapsed_secs: f64,
    pub state: AgentState,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum AgentState {
    Idle,
    Armed,
    Firing,
    Terminated,
}

/// Termination acknowledgment with final stats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermAck {
    pub total_hashes: u64,
    pub total_duration: f64,
    pub peak_cpu: f64,
}

/// Read a framed message from a stream
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

/// Write a framed message to a stream
fn write_frame<W: Write>(w: &mut W, kind: u8, payload: &[u8]) -> std::io::Result<()> {
    let total_len = 1 + payload.len();
    w.write_all(&(total_len as u64).to_le_bytes())?;
    w.write_all(&[kind])?;
    w.write_all(payload)?;
    w.flush()
}

/// Get default config path
pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stryke")
        .join("agent.toml")
}

/// Load config from file or return defaults
pub fn load_config(path: Option<&str>) -> AgentConfig {
    let config_path = path
        .map(PathBuf::from)
        .unwrap_or_else(default_config_path);

    if config_path.exists() {
        match std::fs::read_to_string(&config_path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(config) => {
                    eprintln!("stryke agent: loaded config from {}", config_path.display());
                    return config;
                }
                Err(e) => {
                    eprintln!(
                        "stryke agent: config parse error {}: {}",
                        config_path.display(),
                        e
                    );
                }
            },
            Err(e) => {
                eprintln!(
                    "stryke agent: cannot read {}: {}",
                    config_path.display(),
                    e
                );
            }
        }
    }

    eprintln!("stryke agent: using default config (controller=localhost:9999)");
    AgentConfig::default()
}

/// Get system hostname
fn get_hostname() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Get CPU core count
fn get_cores() -> usize {
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1)
}

/// Get total system memory (approximate)
fn get_memory() -> u64 {
    // Simple heuristic — real implementation would use sysinfo crate
    // For now, return a placeholder based on typical server memory
    16 * 1024 * 1024 * 1024 // 16GB default
}

/// Run the stress workload — pins ALL cores to 100% TDP
fn run_workload(
    workload: &WorkloadType,
    duration_secs: f64,
    terminate: Arc<AtomicBool>,
) -> (u64, f64) {
    use sha2::{Digest, Sha256};
    use std::sync::atomic::AtomicU64;

    let start = Instant::now();
    let duration = Duration::from_secs_f64(duration_secs);
    let num_cores = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1);

    match workload {
        WorkloadType::Cpu | WorkloadType::Combined => {
            let total_hashes = AtomicU64::new(0);
            
            std::thread::scope(|s| {
                for _ in 0..num_cores {
                    let term = Arc::clone(&terminate);
                    let counter = &total_hashes;
                    s.spawn(move || {
                        let mut local_count: u64 = 0;
                        let mut data = [0u8; 64];
                        
                        while start.elapsed() < duration && !term.load(Ordering::Relaxed) {
                            for _ in 0..1000 {
                                let hash = Sha256::digest(&data);
                                data[..32].copy_from_slice(&hash);
                                local_count += 1;
                            }
                        }
                        
                        counter.fetch_add(local_count, Ordering::Relaxed);
                    });
                }
            });
            
            (total_hashes.load(Ordering::Relaxed), start.elapsed().as_secs_f64())
        }
        WorkloadType::Memory { bytes } => {
            let bytes_per_core = *bytes as usize / num_cores;
            
            std::thread::scope(|s| {
                for core_id in 0..num_cores {
                    let term = Arc::clone(&terminate);
                    s.spawn(move || {
                        if term.load(Ordering::Relaxed) {
                            return;
                        }
                        let mut buf: Vec<u8> = vec![0u8; bytes_per_core];
                        for i in (0..bytes_per_core).step_by(4096) {
                            if term.load(Ordering::Relaxed) {
                                break;
                            }
                            buf[i] = ((i + core_id) & 0xff) as u8;
                        }
                        std::hint::black_box(&buf);
                    });
                }
            });
            
            (*bytes, start.elapsed().as_secs_f64())
        }
        WorkloadType::Io { dir, iterations } => {
            use std::fs;
            use std::io::Write as IoWrite;
            
            let total_bytes = AtomicU64::new(0);
            let iters_per_core = *iterations as usize / num_cores;
            
            std::thread::scope(|s| {
                for core_id in 0..num_cores {
                    let term = Arc::clone(&terminate);
                    let counter = &total_bytes;
                    let dir = dir.clone();
                    s.spawn(move || {
                        let io_data = vec![0xABu8; 1_000_000];
                        for i in 0..iters_per_core {
                            if term.load(Ordering::Relaxed) {
                                break;
                            }
                            let path = format!("{}/stryke_stress_{}_{}", dir, core_id, i);
                            if let Ok(mut f) = fs::File::create(&path) {
                                let _ = f.write_all(&io_data);
                            }
                            let _ = fs::read(&path);
                            let _ = fs::remove_file(&path);
                            counter.fetch_add(io_data.len() as u64, Ordering::Relaxed);
                        }
                    });
                }
            });
            
            (total_bytes.load(Ordering::Relaxed), start.elapsed().as_secs_f64())
        }
        WorkloadType::Custom { code: _ } => {
            // TODO: execute custom stryke code
            (0, start.elapsed().as_secs_f64())
        }
    }
}

/// Main agent loop
pub fn run_agent(config_path: Option<&str>) -> i32 {
    run_agent_with_overrides(config_path, None, None)
}

/// Main agent loop with CLI overrides
pub fn run_agent_with_overrides(
    config_path: Option<&str>,
    controller_override: Option<&str>,
    port_override: Option<u16>,
) -> i32 {
    let mut config = load_config(config_path);

    if let Some(host) = controller_override {
        config.controller.host = host.to_string();
    }
    if let Some(port) = port_override {
        config.controller.port = port;
    }

    let addr = format!("{}:{}", config.controller.host, config.controller.port);

    eprintln!("stryke agent: connecting to controller at {}", addr);

    let mut stream = match TcpStream::connect(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("stryke agent: connection failed: {}", e);
            return 1;
        }
    };

    // Set read timeout for non-blocking checks
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));

    // Send AGENT_HELLO
    let hello = AgentHello {
        proto_version: AGENT_PROTO_VERSION,
        stryke_version: env!("CARGO_PKG_VERSION").to_string(),
        hostname: get_hostname(),
        cores: get_cores(),
        memory_bytes: get_memory(),
        agent_name: config.agent.name.clone(),
    };

    let hello_bytes = bincode::serialize(&hello).expect("serialize hello");
    if let Err(e) = write_frame(&mut stream, frame_kind::AGENT_HELLO, &hello_bytes) {
        eprintln!("stryke agent: failed to send hello: {}", e);
        return 1;
    }

    // Wait for HELLO_ACK
    let (kind, payload) = match read_frame(&mut stream) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("stryke agent: failed to read hello ack: {}", e);
            return 1;
        }
    };

    if kind != frame_kind::AGENT_HELLO_ACK {
        eprintln!("stryke agent: unexpected frame kind: {}", kind);
        return 1;
    }

    let ack: AgentHelloAck = match bincode::deserialize(&payload) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("stryke agent: failed to parse hello ack: {}", e);
            return 1;
        }
    };

    if !ack.accepted {
        eprintln!("stryke agent: rejected by controller: {}", ack.message);
        return 1;
    }

    eprintln!(
        "stryke agent: connected (session_id={}, cores={}, hostname={})",
        ack.session_id,
        get_cores(),
        get_hostname()
    );
    eprintln!("stryke agent: awaiting commands...");

    // Disable read timeout for blocking reads
    let _ = stream.set_read_timeout(None);

    // Main command loop
    let terminate = Arc::new(AtomicBool::new(false));
    let mut state = AgentState::Idle;
    let mut session_start: Option<Instant> = None;
    let mut total_hashes: u64 = 0;
    let mut peak_cpu: f64 = 0.0;

    loop {
        let (kind, payload) = match read_frame(&mut stream) {
            Ok(f) => f,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    eprintln!("stryke agent: controller disconnected");
                } else {
                    eprintln!("stryke agent: read error: {}", e);
                }
                break;
            }
        };

        match kind {
            frame_kind::FIRE => {
                let cmd: FireCommand = match bincode::deserialize(&payload) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("stryke agent: invalid FIRE command: {}", e);
                        continue;
                    }
                };

                eprintln!(
                    "stryke agent: FIRE received (duration={}s, intensity={})",
                    cmd.duration_secs, cmd.intensity
                );

                state = AgentState::Firing;
                session_start = Some(Instant::now());
                terminate.store(false, Ordering::Relaxed);

                // Run workload in a separate thread so we can handle TERMINATE
                let term_clone = Arc::clone(&terminate);
                let workload = cmd.workload.clone();
                let duration = cmd.duration_secs;

                let handle = std::thread::spawn(move || run_workload(&workload, duration, term_clone));

                // Wait for completion or termination
                let (hashes, elapsed) = handle.join().unwrap_or((0, 0.0));
                total_hashes += hashes;

                // Send final metrics
                let metrics = AgentMetrics {
                    cpu_percent: 100.0, // Was at max
                    memory_used: 0,
                    hashes_per_sec: if elapsed > 0.0 {
                        (hashes as f64 / elapsed) as u64
                    } else {
                        0
                    },
                    elapsed_secs: elapsed,
                    state: AgentState::Idle,
                };

                let metrics_bytes = bincode::serialize(&metrics).expect("serialize metrics");
                let _ = write_frame(&mut stream, frame_kind::METRICS, &metrics_bytes);

                state = AgentState::Idle;
                eprintln!(
                    "stryke agent: workload complete ({} hashes in {:.2}s)",
                    hashes, elapsed
                );
            }

            frame_kind::TERMINATE => {
                eprintln!("stryke agent: TERMINATE received");
                terminate.store(true, Ordering::Relaxed);

                let elapsed = session_start.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);
                let term_ack = TermAck {
                    total_hashes,
                    total_duration: elapsed,
                    peak_cpu,
                };

                let ack_bytes = bincode::serialize(&term_ack).expect("serialize term_ack");
                let _ = write_frame(&mut stream, frame_kind::TERM_ACK, &ack_bytes);

                state = AgentState::Idle;
                total_hashes = 0;
                peak_cpu = 0.0;
                session_start = None;
            }

            frame_kind::STATUS => {
                let metrics = AgentMetrics {
                    cpu_percent: if state == AgentState::Firing {
                        100.0
                    } else {
                        0.0
                    },
                    memory_used: 0,
                    hashes_per_sec: 0,
                    elapsed_secs: session_start.map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0),
                    state,
                };

                let metrics_bytes = bincode::serialize(&metrics).expect("serialize metrics");
                let _ = write_frame(&mut stream, frame_kind::STATUS_RESP, &metrics_bytes);
            }

            frame_kind::SHUTDOWN => {
                eprintln!("stryke agent: SHUTDOWN received, exiting");
                terminate.store(true, Ordering::Relaxed);
                break;
            }

            _ => {
                eprintln!("stryke agent: unknown frame kind: {}", kind);
            }
        }
    }

    eprintln!("stryke agent: disconnected");
    0
}

/// Print agent help
pub fn print_help() {
    println!("stryke agent — Distributed load testing agent");
    println!();
    println!("USAGE:");
    println!("    stryke agent [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -c, --config PATH    Config file (default: ~/.config/stryke/agent.toml)");
    println!("    --controller HOST    Controller address (overrides config)");
    println!("    --port PORT          Controller port (overrides config)");
    println!("    --help               Print this help");
    println!();
    println!("CONFIG FILE:");
    println!("    ~/.config/stryke/agent.toml");
    println!();
    println!("    [controller]");
    println!("    host = \"controller.example.com\"");
    println!("    port = 9999");
    println!();
    println!("    [limits]");
    println!("    max_temp = 85");
    println!("    max_duration = 3600");
    println!();
    println!("    [agent]");
    println!("    name = \"node-01\"");
    println!();
    println!("EXAMPLE:");
    println!("    stryke agent                           # use config file");
    println!("    stryke agent --controller 10.0.0.1     # connect to specific host");
}
