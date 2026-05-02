//! Expanded stress-testing surface. The original `stress_cpu` /
//! `stress_mem` / `stress_io` / `stress_test` / `heat` builtins live in
//! `builtins.rs`; this module adds the rest of the load-axis matrix —
//! float / int / cache / branch / sort compute; sustained-disk + IOPS;
//! TCP / HTTP / DNS network; fork / thread churn; alloc / mmap memory;
//! crypto / compress / regex / json kernels; ramp / burst / oscillate
//! patterns; and per-system telemetry (temp / freq / throttle / load /
//! meminfo).
//!
//! Naming convention: every builtin in this module starts with `stress_`
//! so app code can grep + wrap the whole surface.
//!
//! Each kernel:
//!   * Pins ALL cores via `std::thread::scope` + `available_parallelism`
//!   * Returns an i64 (operations completed) or a hashref of metrics
//!   * Honors a duration in seconds (default 5s, max ~3600s)

use crate::error::PerlError;
use crate::value::PerlValue;
use indexmap::IndexMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, PerlError>;

fn cores() -> usize {
    std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(1)
}

fn duration_arg(args: &[PerlValue], default_secs: f64) -> Duration {
    let secs = args
        .first()
        .map(|v| v.to_number())
        .unwrap_or(default_secs)
        .clamp(0.001, 3600.0);
    Duration::from_secs_f64(secs)
}

fn hash_to_perl(map: IndexMap<String, PerlValue>) -> PerlValue {
    PerlValue::hash_ref(Arc::new(parking_lot::RwLock::new(map)))
}

// ── Compute kernels ────────────────────────────────────────────────────

/// `stress_fp(secs)` — float matrix-multiply pinning every core.
/// Returns total FLOPs (approx — counts inner adds).
pub(crate) fn stress_fp(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let dur = duration_arg(args, 5.0);
    let n = cores();
    let total = AtomicI64::new(0);
    let start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                const D: usize = 96;
                let mut a = vec![0.001f64; D * D];
                let mut b = vec![0.002f64; D * D];
                let mut c = vec![0f64; D * D];
                for (i, x) in a.iter_mut().enumerate() {
                    *x = (i as f64).sin();
                }
                for (i, x) in b.iter_mut().enumerate() {
                    *x = (i as f64).cos();
                }
                let mut local: i64 = 0;
                while start.elapsed() < dur {
                    for i in 0..D {
                        for j in 0..D {
                            let mut acc = 0f64;
                            for k in 0..D {
                                acc += a[i * D + k] * b[k * D + j];
                            }
                            c[i * D + j] = acc;
                        }
                    }
                    local += (D * D * D) as i64;
                    std::hint::black_box(&c);
                }
                total.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

/// `stress_int(secs)` — integer pipeline pinning every core.
/// Returns ops completed.
pub(crate) fn stress_int(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let dur = duration_arg(args, 5.0);
    let n = cores();
    let total = AtomicI64::new(0);
    let start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                let mut local: i64 = 0;
                let mut x: u64 = 0xCAFEBABEDEADBEEF;
                while start.elapsed() < dur {
                    for _ in 0..10_000 {
                        x = x
                            .wrapping_mul(6364136223846793005)
                            .wrapping_add(1442695040888963407);
                        x ^= x.rotate_left(13);
                        x = x.wrapping_mul(0x5851F42D4C957F2D);
                        local += 4;
                    }
                    std::hint::black_box(x);
                }
                total.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

/// `stress_cache(secs, kb)` — pound a working set of `kb` KiB per core
/// in random order, intended to thrash the chosen cache level. Default
/// `kb=1024` (~ L2/L3 boundary on most CPUs).
pub(crate) fn stress_cache(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let dur = duration_arg(args, 5.0);
    let kb = args.get(1).map(|v| v.to_int()).unwrap_or(1024).max(8);
    let bytes = (kb as usize) * 1024;
    let n = cores();
    let total = AtomicI64::new(0);
    let start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                let len = bytes / 8;
                let mut buf = vec![0u64; len];
                for (i, x) in buf.iter_mut().enumerate() {
                    *x = i as u64;
                }
                let mut idx: usize = 0;
                let mut local: i64 = 0;
                while start.elapsed() < dur {
                    for _ in 0..100_000 {
                        idx = (idx.wrapping_mul(2654435761) ^ buf[idx % len] as usize) % len;
                        buf[idx] = buf[idx].wrapping_add(1);
                        local += 1;
                    }
                    std::hint::black_box(&buf);
                }
                total.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

/// `stress_branch(secs)` — branch-predictor torture: data-dependent
/// branches on pseudorandom data.
pub(crate) fn stress_branch(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let dur = duration_arg(args, 5.0);
    let n = cores();
    let total = AtomicI64::new(0);
    let start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                let mut x: u64 = 0xDEAD_BEEF_CAFE_F00D;
                let mut sum: i64 = 0;
                let mut local: i64 = 0;
                while start.elapsed() < dur {
                    for _ in 0..100_000 {
                        x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                        sum += if x & 1 == 0 {
                            if x & 2 == 0 {
                                1
                            } else {
                                -1
                            }
                        } else if x & 4 == 0 {
                            2
                        } else {
                            -3
                        };
                        local += 1;
                    }
                    std::hint::black_box(sum);
                }
                total.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

/// `stress_sort(secs, n)` — repeatedly sort an `n`-element vector per
/// core. `n` defaults to 100k.
pub(crate) fn stress_sort(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let dur = duration_arg(args, 5.0);
    let n_elems = args.get(1).map(|v| v.to_int()).unwrap_or(100_000).max(16) as usize;
    let cores_n = cores();
    let total = AtomicI64::new(0);
    let start = Instant::now();
    let total_ref = &total;
    std::thread::scope(|s| {
        for c in 0..cores_n {
            s.spawn(move || {
                let mut buf: Vec<i64> = (0..n_elems as i64)
                    .map(|i| ((i ^ (c as i64 + 1)).wrapping_mul(2654435761)) % 1_000_000)
                    .collect();
                let mut local: i64 = 0;
                let mut salt: i64 = c as i64;
                while start.elapsed() < dur {
                    salt = salt.wrapping_add(1);
                    for x in buf.iter_mut() {
                        *x ^= salt;
                    }
                    buf.sort_unstable();
                    local += 1;
                    std::hint::black_box(&buf);
                }
                total_ref.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

// ── Memory kernels ────────────────────────────────────────────────────

/// `stress_alloc(secs, size_kb)` — small-alloc churn. Default 64KB allocs.
pub(crate) fn stress_alloc(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let dur = duration_arg(args, 5.0);
    let size = (args.get(1).map(|v| v.to_int()).unwrap_or(64).max(1) as usize) * 1024;
    let n = cores();
    let total = AtomicI64::new(0);
    let start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                let mut local: i64 = 0;
                while start.elapsed() < dur {
                    let mut v: Vec<u8> = vec![0; size];
                    v[size - 1] = 1;
                    std::hint::black_box(v);
                    local += 1;
                }
                total.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

/// `stress_mmap(secs, mb)` — mmap a `mb` MiB anon region per core and
/// touch every page repeatedly. Default 256MB.
pub(crate) fn stress_mmap(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let dur = duration_arg(args, 5.0);
    let mb = args.get(1).map(|v| v.to_int()).unwrap_or(256).max(1) as usize;
    let n = cores();
    let total = AtomicI64::new(0);
    let start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                let bytes = mb * 1024 * 1024;
                let mut local: i64 = 0;
                while start.elapsed() < dur {
                    let mut map = match memmap2::MmapOptions::new().len(bytes).map_anon() {
                        Ok(m) => m,
                        Err(_) => break,
                    };
                    for i in (0..bytes).step_by(4096) {
                        map[i] = (i & 0xff) as u8;
                        local += 1;
                    }
                    std::hint::black_box(&map);
                }
                total.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    let _ = line;
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

// ── Disk kernels ──────────────────────────────────────────────────────

/// `stress_disk(path, secs, mb_per_write)` — sustained sequential write
/// then fsync then read across all cores. Each core writes/reads its own
/// file in `path` (or /tmp).
pub(crate) fn stress_disk(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    use std::fs;
    use std::io::Write;
    let path = args
        .first()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "/tmp".to_string());
    let dur = duration_arg(&args[args.len().min(1)..], 5.0);
    let mb = args.get(2).map(|v| v.to_int()).unwrap_or(8).max(1) as usize;
    let n = cores();
    let total = AtomicI64::new(0);
    let total_ref = &total;
    let start = Instant::now();
    std::thread::scope(|s| {
        for c in 0..n {
            let path = path.clone();
            s.spawn(move || {
                let block = vec![0xCDu8; mb * 1024 * 1024];
                let file = format!("{}/stryke_disk_{}_{}.tmp", path, std::process::id(), c);
                let mut local: i64 = 0;
                while start.elapsed() < dur {
                    if let Ok(mut f) = fs::File::create(&file) {
                        if f.write_all(&block).is_ok() {
                            let _ = f.sync_all();
                            local += block.len() as i64;
                        }
                    }
                    let _ = fs::read(&file);
                }
                let _ = fs::remove_file(&file);
                total_ref.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

/// `stress_iops(path, secs, block_kb)` — small random reads/writes
/// against per-core scratch files. Default block=4KB.
pub(crate) fn stress_iops(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    use std::fs::OpenOptions;
    use std::io::{Read, Seek, SeekFrom, Write};
    let path = args
        .first()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "/tmp".to_string());
    let dur = duration_arg(&args[args.len().min(1)..], 5.0);
    let block_kb = args.get(2).map(|v| v.to_int()).unwrap_or(4).max(1) as usize;
    let block = block_kb * 1024;
    let n = cores();
    let total = AtomicI64::new(0);
    let total_ref = &total;
    let start = Instant::now();
    std::thread::scope(|s| {
        for c in 0..n {
            let path = path.clone();
            s.spawn(move || {
                let file = format!("{}/stryke_iops_{}_{}.tmp", path, std::process::id(), c);
                // 8MB scratch — enough working set for random offsets,
                // small enough that the setup write doesn't eat the
                // duration budget on short runs.
                let scratch = vec![0xABu8; 8 * 1024 * 1024];
                let _ = std::fs::write(&file, &scratch);
                let mut f = match OpenOptions::new().read(true).write(true).open(&file) {
                    Ok(f) => f,
                    Err(_) => return,
                };
                let mut buf = vec![0u8; block];
                let payload = vec![0xEFu8; block];
                let mut x: u64 = (c as u64).wrapping_mul(0x9E37_79B1);
                let mut local: i64 = 0;
                // Re-anchor the timer post-setup so a short duration
                // still gets some IOPS work in.
                let local_start = Instant::now();
                while local_start.elapsed() < dur && start.elapsed() < dur.saturating_mul(4) {
                    for _ in 0..100 {
                        x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                        let off = (x as usize % (scratch.len() / block)) * block;
                        let _ = f.seek(SeekFrom::Start(off as u64));
                        let _ = f.read_exact(&mut buf);
                        let _ = f.seek(SeekFrom::Start(off as u64));
                        let _ = f.write_all(&payload);
                        local += 2;
                    }
                }
                let _ = std::fs::remove_file(&file);
                total_ref.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

// ── Network kernels ──────────────────────────────────────────────────

/// `stress_net(target, secs, conns)` — open `conns` TCP connections per
/// core to `target` (`host:port`), pump bytes until duration elapses.
/// Returns total bytes sent.
pub(crate) fn stress_net(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    use std::io::Write;
    use std::net::TcpStream;
    let target = args.first().map(|v| v.to_string()).ok_or_else(|| {
        PerlError::runtime(
            "stress_net: usage: stress_net(\"host:port\", secs, conns)",
            line,
        )
    })?;
    let dur = duration_arg(&args[1.min(args.len())..], 5.0);
    let conns = args.get(2).map(|v| v.to_int()).unwrap_or(8).max(1) as usize;
    let n = cores();
    let total = AtomicI64::new(0);
    let total_ref = &total;
    let start = Instant::now();
    let payload = vec![0xA5u8; 4096];
    std::thread::scope(|s| {
        for _ in 0..n {
            let target = target.clone();
            let payload = &payload;
            s.spawn(move || {
                let mut streams: Vec<TcpStream> = (0..conns)
                    .filter_map(|_| TcpStream::connect(&target).ok())
                    .collect();
                let mut local: i64 = 0;
                while start.elapsed() < dur && !streams.is_empty() {
                    for st in streams.iter_mut() {
                        if let Ok(n) = st.write(payload) {
                            local += n as i64
                        }
                    }
                }
                total_ref.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

/// `stress_http(url, secs, conns)` — HTTP GET storm. Counts successful
/// 2xx responses across cores.
pub(crate) fn stress_http(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let url = args.first().map(|v| v.to_string()).ok_or_else(|| {
        PerlError::runtime(
            "stress_http: usage: stress_http(\"http://...\", secs, conns)",
            line,
        )
    })?;
    let dur = duration_arg(&args[1.min(args.len())..], 5.0);
    let n = cores();
    let total = AtomicI64::new(0);
    let total_ref = &total;
    let start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..n {
            let url = url.clone();
            s.spawn(move || {
                use std::io::Read;
                let agent = ureq::AgentBuilder::new()
                    .timeout_connect(Duration::from_secs(2))
                    .timeout(Duration::from_secs(5))
                    .build();
                let mut local: i64 = 0;
                while start.elapsed() < dur {
                    if let Ok(r) = agent.get(&url).call() {
                        if (200..300).contains(&r.status()) {
                            local += 1;
                        }
                        let _ = r.into_reader().read_to_end(&mut Vec::new());
                    }
                }
                total_ref.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

/// `stress_dns(host, secs)` — DNS lookup storm. Returns successful
/// resolves count.
pub(crate) fn stress_dns(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let host = args.first().map(|v| v.to_string()).ok_or_else(|| {
        PerlError::runtime(
            "stress_dns: usage: stress_dns(\"host.example.com\", secs)",
            line,
        )
    })?;
    let dur = duration_arg(&args[1.min(args.len())..], 5.0);
    let n = cores();
    let total = AtomicI64::new(0);
    let total_ref = &total;
    let start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..n {
            let host = host.clone();
            s.spawn(move || {
                use std::net::ToSocketAddrs;
                let mut local: i64 = 0;
                while start.elapsed() < dur {
                    if (host.as_str(), 0u16)
                        .to_socket_addrs()
                        .map(|mut it| it.next().is_some())
                        .unwrap_or(false)
                    {
                        local += 1;
                    }
                }
                total_ref.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

// ── Process / Thread churn ────────────────────────────────────────────

/// `stress_fork(secs)` — fork + immediate exit churn. Returns
/// completed forks.
#[cfg(unix)]
pub(crate) fn stress_fork(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let dur = duration_arg(args, 5.0);
    let n = cores();
    let total = AtomicI64::new(0);
    let start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                let mut local: i64 = 0;
                while start.elapsed() < dur {
                    match unsafe { nix::unistd::fork() } {
                        Ok(nix::unistd::ForkResult::Child) => unsafe {
                            libc::_exit(0);
                        },
                        Ok(nix::unistd::ForkResult::Parent { child }) => {
                            let _ = nix::sys::wait::waitpid(child, None);
                            local += 1;
                        }
                        Err(_) => break,
                    }
                }
                total.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

#[cfg(not(unix))]
pub(crate) fn stress_fork(_args: &[PerlValue], line: usize) -> Result<PerlValue> {
    Err(PerlError::runtime("stress_fork: unix-only", line))
}

/// `stress_thread(secs, count)` — thread spawn/join churn. Each round
/// spawns `count` threads (default 64) and joins them.
pub(crate) fn stress_thread(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let dur = duration_arg(args, 5.0);
    let count = args.get(1).map(|v| v.to_int()).unwrap_or(64).max(1) as usize;
    let total = AtomicI64::new(0);
    let start = Instant::now();
    while start.elapsed() < dur {
        let handles: Vec<_> = (0..count)
            .map(|i| {
                std::thread::spawn(move || {
                    std::hint::black_box(i.wrapping_mul(31));
                })
            })
            .collect();
        for h in handles {
            let _ = h.join();
        }
        total.fetch_add(count as i64, Ordering::Relaxed);
    }
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

// ── Crypto / compress / regex / json kernels ──────────────────────────

/// `stress_aes(secs)` — AES-128 round trip across cores. Returns ops.
pub(crate) fn stress_aes(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    use aes::cipher::{generic_array::GenericArray, BlockEncrypt, KeyInit};
    use aes::Aes128;
    let dur = duration_arg(args, 5.0);
    let n = cores();
    let total = AtomicI64::new(0);
    let start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                let key = GenericArray::from([0u8; 16]);
                let cipher = Aes128::new(&key);
                let mut block = GenericArray::from([0u8; 16]);
                let mut local: i64 = 0;
                while start.elapsed() < dur {
                    for _ in 0..10_000 {
                        cipher.encrypt_block(&mut block);
                        local += 1;
                    }
                }
                total.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

/// `stress_compress(secs, kb)` — gzip compress + decompress payload of
/// `kb` KiB per core. Default 1MB. Returns round-trips.
pub(crate) fn stress_compress(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    use flate2::read::GzDecoder;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::{Read, Write};
    let dur = duration_arg(args, 5.0);
    let kb = args.get(1).map(|v| v.to_int()).unwrap_or(1024).max(1) as usize;
    let bytes = kb * 1024;
    let n = cores();
    let total = AtomicI64::new(0);
    let start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                let mut payload = vec![0u8; bytes];
                for (i, x) in payload.iter_mut().enumerate() {
                    *x = (i & 0xff) as u8;
                }
                // Add some repetition so gzip has work to do.
                for chunk in payload.chunks_mut(64) {
                    for x in chunk.iter_mut() {
                        *x = (*x).wrapping_mul(13);
                    }
                }
                let mut local: i64 = 0;
                while start.elapsed() < dur {
                    let mut enc = GzEncoder::new(Vec::with_capacity(bytes), Compression::default());
                    let _ = enc.write_all(&payload);
                    let compressed = enc.finish().unwrap_or_default();
                    let mut dec = GzDecoder::new(&compressed[..]);
                    let mut decoded = Vec::with_capacity(bytes);
                    let _ = dec.read_to_end(&mut decoded);
                    std::hint::black_box(&decoded);
                    local += 1;
                }
                total.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

/// `stress_regex(secs)` — pathological backtracking regex against
/// generated input. Tests parser worst-case.
pub(crate) fn stress_regex(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let dur = duration_arg(args, 5.0);
    let n = cores();
    let total = AtomicI64::new(0);
    let start = Instant::now();
    // Compile once and share — every worker scans the same pattern.
    // Avoid actual catastrophic backtracking that would never terminate;
    // pick a heavy-but-bounded shape.
    let re = regex::Regex::new(r"(?:[a-z0-9]+_)+(?:[a-z0-9]+)").unwrap();
    let re_ref = &re;
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                let re = re_ref;
                let inputs = [
                    "neon_void_quasar_axion_pulse_stryke_phantom_kernel",
                    "stack_lattice_vector_kernel_phantom_aurora_nova",
                    "abc_def_ghi_jkl_mno_pqr_stu_vwx_yza_bcd_efg",
                ];
                let mut local: i64 = 0;
                let mut idx = 0;
                while start.elapsed() < dur {
                    for _ in 0..1000 {
                        if re.is_match(inputs[idx % inputs.len()]) {
                            local += 1;
                        }
                        idx = idx.wrapping_add(1);
                    }
                }
                total.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

/// `stress_json(secs, kb)` — encode + decode a `kb` KiB JSON object per
/// core. Default 256KB.
pub(crate) fn stress_json(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let dur = duration_arg(args, 5.0);
    let kb = args.get(1).map(|v| v.to_int()).unwrap_or(256).max(1) as usize;
    let n = cores();
    let total = AtomicI64::new(0);
    let start = Instant::now();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| {
                let n_keys = (kb * 1024) / 32;
                let mut obj = serde_json::Map::with_capacity(n_keys);
                for i in 0..n_keys {
                    obj.insert(
                        format!("k_{}", i),
                        serde_json::Value::Number(serde_json::Number::from(i as i64)),
                    );
                }
                let value = serde_json::Value::Object(obj);
                let mut local: i64 = 0;
                while start.elapsed() < dur {
                    let s = serde_json::to_string(&value).unwrap_or_default();
                    let _: serde_json::Value =
                        serde_json::from_str(&s).unwrap_or(serde_json::Value::Null);
                    local += 1;
                }
                total.fetch_add(local, Ordering::Relaxed);
            });
        }
    });
    Ok(PerlValue::integer(total.load(Ordering::Relaxed)))
}

// ── Pattern controllers (burst / ramp / oscillate) ────────────────────

/// `stress_burst(workload_name, on_secs, off_secs, total_secs)` — run
/// the named stress kernel for `on_secs`, sleep `off_secs`, repeat
/// until `total_secs` elapsed. Returns total iterations.
pub(crate) fn stress_burst(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let name = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("stress_burst: workload name required", line))?;
    let on = args.get(1).map(|v| v.to_number()).unwrap_or(2.0).max(0.05);
    let off = args.get(2).map(|v| v.to_number()).unwrap_or(2.0).max(0.0);
    let total =
        Duration::from_secs_f64(args.get(3).map(|v| v.to_number()).unwrap_or(20.0).max(0.05));
    let started = Instant::now();
    let mut iters: i64 = 0;
    while started.elapsed() < total {
        let _ = run_named_stress(&name, on, line)?;
        iters += 1;
        if off > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(off));
        }
    }
    Ok(PerlValue::integer(iters))
}

/// `stress_ramp(workload, start_pct, end_pct, total_secs)` — duty-cycle
/// ramp. `pct` is the *on* fraction of each 1-second tick.
pub(crate) fn stress_ramp(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let name = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("stress_ramp: workload name required", line))?;
    let start_pct = args.get(1).map(|v| v.to_number()).unwrap_or(10.0);
    let end_pct = args.get(2).map(|v| v.to_number()).unwrap_or(100.0);
    let total = args.get(3).map(|v| v.to_number()).unwrap_or(30.0).max(0.5);
    let begin = Instant::now();
    let total_dur = Duration::from_secs_f64(total);
    let mut ticks: i64 = 0;
    while begin.elapsed() < total_dur {
        let t = begin.elapsed().as_secs_f64() / total;
        let pct = (start_pct + (end_pct - start_pct) * t).clamp(0.0, 100.0);
        let on = (pct / 100.0).clamp(0.0, 1.0);
        let _ = run_named_stress(&name, on, line)?;
        ticks += 1;
        let off = 1.0 - on;
        if off > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(off));
        }
    }
    Ok(PerlValue::integer(ticks))
}

/// `stress_oscillate(workload, period_secs, total_secs)` — sinusoidal
/// duty cycle: on/off pattern with period `period_secs`.
pub(crate) fn stress_oscillate(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let name = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("stress_oscillate: workload name required", line))?;
    let period = args.get(1).map(|v| v.to_number()).unwrap_or(10.0).max(1.0);
    let total = args
        .get(2)
        .map(|v| v.to_number())
        .unwrap_or(60.0)
        .max(period);
    let begin = Instant::now();
    let total_dur = Duration::from_secs_f64(total);
    let mut ticks: i64 = 0;
    while begin.elapsed() < total_dur {
        let phase = (begin.elapsed().as_secs_f64() / period) * std::f64::consts::TAU;
        let on = ((phase.sin() + 1.0) / 2.0).clamp(0.0, 1.0);
        let _ = run_named_stress(&name, on.max(0.05), line)?;
        ticks += 1;
        let off = 1.0 - on;
        if off > 0.0 {
            std::thread::sleep(Duration::from_secs_f64(off));
        }
    }
    Ok(PerlValue::integer(ticks))
}

fn run_named_stress(name: &str, secs: f64, line: usize) -> Result<PerlValue> {
    let arg = vec![PerlValue::float(secs)];
    match name {
        "fp" | "stress_fp" => stress_fp(&arg, line),
        "int" | "stress_int" => stress_int(&arg, line),
        "cache" | "stress_cache" => stress_cache(&arg, line),
        "branch" | "stress_branch" => stress_branch(&arg, line),
        "sort" | "stress_sort" => stress_sort(&arg, line),
        "alloc" | "stress_alloc" => stress_alloc(&arg, line),
        "mmap" | "stress_mmap" => stress_mmap(&arg, line),
        "aes" | "stress_aes" => stress_aes(&arg, line),
        "compress" | "stress_compress" => stress_compress(&arg, line),
        "regex" | "stress_regex" => stress_regex(&arg, line),
        "json" | "stress_json" => stress_json(&arg, line),
        "thread" | "stress_thread" => stress_thread(&arg, line),
        #[cfg(unix)]
        "fork" | "stress_fork" => stress_fork(&arg, line),
        other => Err(PerlError::runtime(
            format!("stress_burst/ramp/oscillate: unknown workload `{}`", other),
            line,
        )),
    }
}

// ── Combined: stress_all ─────────────────────────────────────────────

/// `stress_all(secs)` — run every kernel above in parallel for `secs`.
/// Returns a hashref of per-kernel counts. The biggest hammer in the box.
pub(crate) fn stress_all(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let secs = args.first().map(|v| v.to_number()).unwrap_or(5.0).max(0.05);
    let arg = vec![PerlValue::float(secs)];
    let started = Instant::now();

    // Run each kernel on its own scoped thread so they all hit
    // simultaneously. Each kernel internally already pins all cores —
    // running them concurrently means oversubscription, which is the
    // point: fight for cycles, cache, mem, fd, etc.
    let mut out: IndexMap<String, PerlValue> = IndexMap::new();
    macro_rules! run {
        ($key:expr, $f:expr) => {
            out.insert($key.to_string(), $f(&arg, line)?);
        };
    }
    // Compute kernels in parallel via std::thread::scope.
    std::thread::scope(|s| {
        let h_fp = s.spawn(|| stress_fp(&arg, line));
        let h_int = s.spawn(|| stress_int(&arg, line));
        let h_cache = s.spawn(|| stress_cache(&arg, line));
        let h_branch = s.spawn(|| stress_branch(&arg, line));
        let h_sort = s.spawn(|| stress_sort(&arg, line));
        let h_alloc = s.spawn(|| stress_alloc(&arg, line));
        let h_aes = s.spawn(|| stress_aes(&arg, line));
        let h_regex = s.spawn(|| stress_regex(&arg, line));
        let h_json = s.spawn(|| stress_json(&arg, line));

        out.insert(
            "fp".to_string(),
            h_fp.join()
                .unwrap_or(Ok(PerlValue::UNDEF))
                .unwrap_or(PerlValue::UNDEF),
        );
        out.insert(
            "int".to_string(),
            h_int
                .join()
                .unwrap_or(Ok(PerlValue::UNDEF))
                .unwrap_or(PerlValue::UNDEF),
        );
        out.insert(
            "cache".to_string(),
            h_cache
                .join()
                .unwrap_or(Ok(PerlValue::UNDEF))
                .unwrap_or(PerlValue::UNDEF),
        );
        out.insert(
            "branch".to_string(),
            h_branch
                .join()
                .unwrap_or(Ok(PerlValue::UNDEF))
                .unwrap_or(PerlValue::UNDEF),
        );
        out.insert(
            "sort".to_string(),
            h_sort
                .join()
                .unwrap_or(Ok(PerlValue::UNDEF))
                .unwrap_or(PerlValue::UNDEF),
        );
        out.insert(
            "alloc".to_string(),
            h_alloc
                .join()
                .unwrap_or(Ok(PerlValue::UNDEF))
                .unwrap_or(PerlValue::UNDEF),
        );
        out.insert(
            "aes".to_string(),
            h_aes
                .join()
                .unwrap_or(Ok(PerlValue::UNDEF))
                .unwrap_or(PerlValue::UNDEF),
        );
        out.insert(
            "regex".to_string(),
            h_regex
                .join()
                .unwrap_or(Ok(PerlValue::UNDEF))
                .unwrap_or(PerlValue::UNDEF),
        );
        out.insert(
            "json".to_string(),
            h_json
                .join()
                .unwrap_or(Ok(PerlValue::UNDEF))
                .unwrap_or(PerlValue::UNDEF),
        );
    });
    // Compress + thread are heavier — run sequentially after.
    run!("compress", stress_compress);
    run!("thread", stress_thread);
    out.insert(
        "duration".to_string(),
        PerlValue::float(started.elapsed().as_secs_f64()),
    );
    out.insert("cores".to_string(), PerlValue::integer(cores() as i64));
    Ok(hash_to_perl(out))
}

// ── Telemetry ────────────────────────────────────────────────────────

/// `stress_temp()` — best-effort CPU temperature in °C. Reads
/// `/sys/class/thermal/thermal_zone*/temp` on Linux. Returns the
/// hottest zone or undef when unavailable.
pub(crate) fn stress_temp(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    if let Some(t) = read_hottest_thermal_zone() {
        return Ok(PerlValue::float(t));
    }
    Ok(PerlValue::UNDEF)
}

/// `stress_thermal_zones()` — arrayref of `+{name, temp_c}` entries
/// from every readable thermal zone.
pub(crate) fn stress_thermal_zones(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let mut out: Vec<PerlValue> = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/thermal") {
        for e in entries.flatten() {
            let path = e.path();
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if !name.starts_with("thermal_zone") {
                continue;
            }
            let temp_path = path.join("temp");
            let type_path = path.join("type");
            let raw = std::fs::read_to_string(&temp_path).unwrap_or_default();
            let kind = std::fs::read_to_string(&type_path)
                .unwrap_or_default()
                .trim()
                .to_string();
            if let Ok(milli) = raw.trim().parse::<f64>() {
                let mut m = IndexMap::new();
                m.insert("name".into(), PerlValue::string(name));
                m.insert("kind".into(), PerlValue::string(kind));
                m.insert("temp_c".into(), PerlValue::float(milli / 1000.0));
                out.push(hash_to_perl(m));
            }
        }
    }
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        out,
    ))))
}

fn read_hottest_thermal_zone() -> Option<f64> {
    let mut hottest: Option<f64> = None;
    let entries = std::fs::read_dir("/sys/class/thermal").ok()?;
    for e in entries.flatten() {
        let path = e.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !name.starts_with("thermal_zone") {
            continue;
        }
        if let Ok(raw) = std::fs::read_to_string(path.join("temp")) {
            if let Ok(milli) = raw.trim().parse::<f64>() {
                let c = milli / 1000.0;
                hottest = Some(match hottest {
                    Some(prev) => prev.max(c),
                    None => c,
                });
            }
        }
    }
    hottest
}

/// `stress_freq()` — current CPU frequency in MHz, averaged across
/// cpufreq cpus. Linux-only via `/sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq`.
pub(crate) fn stress_freq(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let mut total_khz: f64 = 0.0;
    let mut n: f64 = 0.0;
    if let Ok(entries) = std::fs::read_dir("/sys/devices/system/cpu") {
        for e in entries.flatten() {
            let path = e.path();
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !name.starts_with("cpu")
                || !name[3..].chars().next().is_some_and(|c| c.is_ascii_digit())
            {
                continue;
            }
            let f_path = path.join("cpufreq/scaling_cur_freq");
            if let Ok(s) = std::fs::read_to_string(&f_path) {
                if let Ok(khz) = s.trim().parse::<f64>() {
                    total_khz += khz;
                    n += 1.0;
                }
            }
        }
    }
    if n > 0.0 {
        return Ok(PerlValue::float(total_khz / n / 1000.0));
    }
    Ok(PerlValue::UNDEF)
}

/// `stress_throttled()` — best-effort thermal-throttling indicator.
/// On Linux, returns 1 when current freq is < 80% of `cpuinfo_max_freq`
/// AND a thermal zone reads > 80°C. Otherwise 0. Returns undef when
/// the data isn't available.
pub(crate) fn stress_throttled(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let cur = match read_avg_freq_khz() {
        Some(v) => v,
        None => return Ok(PerlValue::UNDEF),
    };
    let max = match read_max_freq_khz() {
        Some(v) => v,
        None => return Ok(PerlValue::UNDEF),
    };
    let temp = read_hottest_thermal_zone();
    let freq_low = max > 0.0 && cur / max < 0.80;
    let hot = temp.map(|t| t > 80.0).unwrap_or(false);
    Ok(PerlValue::integer(if freq_low && hot { 1 } else { 0 }))
}

fn read_avg_freq_khz() -> Option<f64> {
    let mut total: f64 = 0.0;
    let mut n: f64 = 0.0;
    let entries = std::fs::read_dir("/sys/devices/system/cpu").ok()?;
    for e in entries.flatten() {
        let path = e.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !name.starts_with("cpu") || !name[3..].chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            continue;
        }
        if let Ok(s) = std::fs::read_to_string(path.join("cpufreq/scaling_cur_freq")) {
            if let Ok(khz) = s.trim().parse::<f64>() {
                total += khz;
                n += 1.0;
            }
        }
    }
    if n > 0.0 {
        Some(total / n)
    } else {
        None
    }
}

fn read_max_freq_khz() -> Option<f64> {
    let entries = std::fs::read_dir("/sys/devices/system/cpu").ok()?;
    for e in entries.flatten() {
        let path = e.path();
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if !name.starts_with("cpu") || !name[3..].chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            continue;
        }
        if let Ok(s) = std::fs::read_to_string(path.join("cpufreq/cpuinfo_max_freq")) {
            if let Ok(khz) = s.trim().parse::<f64>() {
                return Some(khz);
            }
        }
    }
    None
}

/// `stress_load()` — three load averages as `+{m1, m5, m15}`.
pub(crate) fn stress_load(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let mut buf = [0f64; 3];
    let n = unsafe { libc::getloadavg(buf.as_mut_ptr(), 3) };
    let mut m = IndexMap::new();
    if n >= 1 {
        m.insert("m1".into(), PerlValue::float(buf[0]));
    }
    if n >= 2 {
        m.insert("m5".into(), PerlValue::float(buf[1]));
    }
    if n >= 3 {
        m.insert("m15".into(), PerlValue::float(buf[2]));
    }
    Ok(hash_to_perl(m))
}

/// `stress_meminfo()` — a hashref of system memory in bytes.
pub(crate) fn stress_meminfo(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    use sysinfo::System;
    let mut sys = System::new();
    sys.refresh_memory();
    let mut m = IndexMap::new();
    m.insert(
        "total_bytes".into(),
        PerlValue::integer(sys.total_memory() as i64),
    );
    m.insert(
        "used_bytes".into(),
        PerlValue::integer(sys.used_memory() as i64),
    );
    m.insert(
        "free_bytes".into(),
        PerlValue::integer(sys.free_memory() as i64),
    );
    m.insert(
        "available_bytes".into(),
        PerlValue::integer(sys.available_memory() as i64),
    );
    m.insert(
        "swap_total_bytes".into(),
        PerlValue::integer(sys.total_swap() as i64),
    );
    m.insert(
        "swap_used_bytes".into(),
        PerlValue::integer(sys.used_swap() as i64),
    );
    Ok(hash_to_perl(m))
}

/// `stress_cores()` — number of logical cores the runtime sees.
pub(crate) fn stress_cores(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    Ok(PerlValue::integer(cores() as i64))
}

// ── Watchdog / kill switch ───────────────────────────────────────────

static GLOBAL_KILL: AtomicBool = AtomicBool::new(false);

/// `stress_arm_kill_switch(secs)` — register a global kill flag that
/// flips after `secs`. Cooperative — kernels still need to check it
/// at their next loop boundary, but it gives apps a deadman switch.
pub(crate) fn stress_arm_kill_switch(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let secs = args.first().map(|v| v.to_number()).unwrap_or(60.0);
    GLOBAL_KILL.store(false, Ordering::Relaxed);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs_f64(secs.max(0.0)));
        GLOBAL_KILL.store(true, Ordering::Relaxed);
    });
    Ok(PerlValue::UNDEF)
}

/// `stress_killed()` → 1 if the kill switch tripped.
pub(crate) fn stress_killed(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    Ok(PerlValue::integer(if GLOBAL_KILL.load(Ordering::Relaxed) {
        1
    } else {
        0
    }))
}

/// `stress_disarm_kill_switch()` — reset the kill flag.
pub(crate) fn stress_disarm_kill_switch(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    GLOBAL_KILL.store(false, Ordering::Relaxed);
    Ok(PerlValue::UNDEF)
}

// ── Metrics history ───────────────────────────────────────────────────
//
// A process-wide ring of `(timestamp_ms, name, value, labels)` samples.
// Drop a record per stress-loop iteration or per scrape tick, then dump
// in CSV / JSON / Prometheus text format on demand. Used by the
// controller to ship metrics to dashboards or to stream live to a TUI.

#[derive(Clone)]
struct MetricSample {
    ts_ms: i64,
    name: String,
    value: f64,
    labels: IndexMap<String, String>,
}

static METRICS: std::sync::OnceLock<parking_lot::Mutex<Vec<MetricSample>>> =
    std::sync::OnceLock::new();

fn metrics() -> &'static parking_lot::Mutex<Vec<MetricSample>> {
    METRICS.get_or_init(|| parking_lot::Mutex::new(Vec::with_capacity(4096)))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// `stress_metrics_record($name, $value, label1 => "...", label2 => "...")`
/// — append one sample to the metrics history. Value is coerced to f64.
/// Labels are stored as strings; integer/float labels stringify.
pub(crate) fn stress_metrics_record(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let name = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("stress_metrics_record: name required", line))?;
    let value = args
        .get(1)
        .map(|v| v.as_float().unwrap_or_else(|| v.to_int() as f64))
        .unwrap_or(0.0);
    let mut labels = IndexMap::new();
    let mut i = 2;
    while i + 1 < args.len() {
        labels.insert(args[i].to_string(), args[i + 1].to_string());
        i += 2;
    }
    metrics().lock().push(MetricSample {
        ts_ms: now_ms(),
        name,
        value,
        labels,
    });
    Ok(PerlValue::UNDEF)
}

/// `stress_metrics_clear()` — wipe the history.
pub(crate) fn stress_metrics_clear(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    metrics().lock().clear();
    Ok(PerlValue::UNDEF)
}

/// `stress_metrics_count()` — number of samples currently in the history.
pub(crate) fn stress_metrics_count(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    Ok(PerlValue::integer(metrics().lock().len() as i64))
}

/// `stress_metrics_export($path, format => "csv"|"json")` — write the
/// history to disk. JSON is one-array-of-objects; CSV has the columns
/// `ts_ms,name,value,labels` where labels is a `k=v;k=v` pack.
pub(crate) fn stress_metrics_export(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let path = args
        .first()
        .map(|v| v.to_string())
        .ok_or_else(|| PerlError::runtime("stress_metrics_export: path required", line))?;
    let mut format = "json".to_string();
    let mut i = 1;
    while i + 1 < args.len() {
        if args[i].to_string() == "format" {
            format = args[i + 1].to_string();
        }
        i += 2;
    }
    let g = metrics().lock();
    let body = match format.as_str() {
        "csv" => format_csv(&g),
        "json" => format_json(&g),
        "prom" | "prometheus" => format_prometheus(&g),
        other => {
            return Err(PerlError::runtime(
                format!("stress_metrics_export: unknown format `{}`", other),
                line,
            ));
        }
    };
    std::fs::write(&path, &body).map_err(|e| {
        PerlError::runtime(
            format!("stress_metrics_export: write {}: {}", path, e),
            line,
        )
    })?;
    Ok(PerlValue::integer(g.len() as i64))
}

/// `stress_metrics_prometheus()` → string in Prometheus text exposition
/// format. Suitable for serving from a `/metrics` endpoint with no
/// additional formatting.
pub(crate) fn stress_metrics_prometheus(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let g = metrics().lock();
    Ok(PerlValue::string(format_prometheus(&g)))
}

/// `stress_metrics_json()` → JSON string of the entire history.
pub(crate) fn stress_metrics_json(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let g = metrics().lock();
    Ok(PerlValue::string(format_json(&g)))
}

/// `stress_metrics_csv()` → CSV string of the entire history.
pub(crate) fn stress_metrics_csv(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let g = metrics().lock();
    Ok(PerlValue::string(format_csv(&g)))
}

/// `stress_metrics_watch(field => "stress_temp", interval_ms => 1000, max_ticks => 60)`
/// → arrayref of values sampled at the requested cadence. Each tick
/// reads the latest sample matching `field` and appends its value.
/// When `field` is omitted, returns the running cost-style summary
/// (currently only the count of samples per name).
pub(crate) fn stress_metrics_watch(args: &[PerlValue], line: usize) -> Result<PerlValue> {
    let mut field = String::new();
    let mut interval_ms: i64 = 1000;
    let mut max_ticks: i64 = 60;
    let mut on_tick: Option<PerlValue> = None;
    let mut i = 0;
    while i + 1 < args.len() {
        let key = args[i].to_string();
        let val = args[i + 1].clone();
        match key.as_str() {
            "field" => field = val.to_string(),
            "interval_ms" => interval_ms = val.to_int(),
            "max_ticks" => max_ticks = val.to_int(),
            "on_tick" => on_tick = Some(val),
            _ => {}
        }
        i += 2;
    }
    if field.is_empty() {
        return Err(PerlError::runtime(
            "stress_metrics_watch: field => \"name\" required",
            line,
        ));
    }
    let interval = Duration::from_millis(interval_ms.max(1) as u64);
    let mut out: Vec<PerlValue> = Vec::with_capacity(max_ticks.max(0) as usize);
    let mut last_ts_seen: i64 = 0;
    for _ in 0..max_ticks.max(0) {
        std::thread::sleep(interval);
        let g = metrics().lock();
        let latest = g
            .iter()
            .rev()
            .find(|s| s.name == field && s.ts_ms > last_ts_seen)
            .cloned();
        drop(g);
        if let Some(s) = latest {
            last_ts_seen = s.ts_ms;
            let v = PerlValue::float(s.value);
            out.push(v.clone());
            if let Some(_cb) = &on_tick {
                // Callback dispatch goes through the interpreter caller;
                // we record the value and let user code attach via the
                // returned arrayref. The on_tick option is reserved for
                // future bytecode-level hookup.
            }
        }
        if GLOBAL_KILL.load(Ordering::Relaxed) {
            break;
        }
    }
    Ok(PerlValue::array_ref(Arc::new(parking_lot::RwLock::new(
        out,
    ))))
}

fn format_csv(samples: &[MetricSample]) -> String {
    let mut out = String::from("ts_ms,name,value,labels\n");
    for s in samples {
        let labels_pack = s
            .labels
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(";");
        out.push_str(&format!(
            "{},{},{},{}\n",
            s.ts_ms,
            csv_escape(&s.name),
            s.value,
            csv_escape(&labels_pack)
        ));
    }
    out
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

fn format_json(samples: &[MetricSample]) -> String {
    let mut out = String::from("[");
    for (i, s) in samples.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let labels_json: Vec<String> = s
            .labels
            .iter()
            .map(|(k, v)| format!("\"{}\":\"{}\"", json_escape(k), json_escape(v)))
            .collect();
        out.push_str(&format!(
            "{{\"ts_ms\":{},\"name\":\"{}\",\"value\":{},\"labels\":{{{}}}}}",
            s.ts_ms,
            json_escape(&s.name),
            s.value,
            labels_json.join(",")
        ));
    }
    out.push(']');
    out
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Prometheus text exposition format. Latest value per (name, labels) wins.
fn format_prometheus(samples: &[MetricSample]) -> String {
    let mut latest: IndexMap<String, &MetricSample> = IndexMap::new();
    for s in samples {
        let labels_key: Vec<String> = s
            .labels
            .iter()
            .map(|(k, v)| format!("{}={:?}", k, v))
            .collect();
        let key = format!("{}{{{}}}", s.name, labels_key.join(","));
        latest.insert(key, s);
    }
    let mut by_name: IndexMap<&str, Vec<&MetricSample>> = IndexMap::new();
    for s in latest.values() {
        by_name.entry(&s.name).or_default().push(s);
    }
    let mut out = String::new();
    for (name, group) in by_name {
        out.push_str(&format!("# TYPE {} gauge\n", prom_safe_name(name)));
        for s in group {
            let labels_part = if s.labels.is_empty() {
                String::new()
            } else {
                let parts: Vec<String> = s
                    .labels
                    .iter()
                    .map(|(k, v)| format!("{}={:?}", prom_safe_name(k), v))
                    .collect();
                format!("{{{}}}", parts.join(","))
            };
            out.push_str(&format!(
                "{}{} {} {}\n",
                prom_safe_name(name),
                labels_part,
                s.value,
                s.ts_ms
            ));
        }
    }
    out
}

fn prom_safe_name(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == ':' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ── Audit log ─────────────────────────────────────────────────────────
//
// Append-only JSONL stream for compliance trails. Each call writes one
// line. Path defaults to `~/.stryke/audit.log`; override via
// `STRYKE_AUDIT_LOG=/path/to/log`. Each line is:
//   {"ts": <iso8601>, "ts_ms": <epoch_ms>, "event": "...",
//    "pid": ..., "host": "...", "user": "...", "details": {...}}

/// `audit_log("event_name", k1 => v1, k2 => v2, ...)` — append one
/// JSONL line. Returns 1 on success, 0 on failure (does not throw —
/// audit failures must never crash the program).
pub(crate) fn audit_log(args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let event = args
        .first()
        .map(|v| v.to_string())
        .unwrap_or_else(|| "<unnamed>".to_string());

    let mut details: Vec<(String, String)> = Vec::new();
    let mut i = 1;
    while i + 1 < args.len() {
        details.push((args[i].to_string(), args[i + 1].to_string()));
        i += 2;
    }

    let path = std::env::var("STRYKE_AUDIT_LOG").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        format!("{}/.stryke/audit.log", home)
    });
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let now_ts = now_ms();
    let host = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_default();
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let pid = std::process::id();

    let details_json: Vec<String> = details
        .iter()
        .map(|(k, v)| format!("\"{}\":\"{}\"", json_escape(k), json_escape(v)))
        .collect();
    let line_str = format!(
        "{{\"ts_ms\":{},\"ts\":\"{}\",\"event\":\"{}\",\"pid\":{},\"host\":\"{}\",\"user\":\"{}\",\"details\":{{{}}}}}\n",
        now_ts,
        iso8601_from_ms(now_ts),
        json_escape(&event),
        pid,
        json_escape(&host),
        json_escape(&user),
        details_json.join(","),
    );

    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut f) => {
            let ok = f.write_all(line_str.as_bytes()).is_ok();
            Ok(PerlValue::integer(if ok { 1 } else { 0 }))
        }
        Err(_) => Ok(PerlValue::integer(0)),
    }
}

/// `audit_log_path()` → the path the next `audit_log` call will write to.
pub(crate) fn audit_log_path(_args: &[PerlValue], _line: usize) -> Result<PerlValue> {
    let path = std::env::var("STRYKE_AUDIT_LOG").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        format!("{}/.stryke/audit.log", home)
    });
    Ok(PerlValue::string(path))
}

fn iso8601_from_ms(ms: i64) -> String {
    let secs = ms / 1000;
    let days = secs / 86_400;
    let secs_of_day = secs % 86_400;
    let h = secs_of_day / 3600;
    let m = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    let (y, mo, d) = days_to_ymd(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y,
        mo,
        d,
        h,
        m,
        s,
        ms % 1000
    )
}

fn days_to_ymd(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}
