//! Shared runners for `stryke check` and `stryke test`.
//!
//! Exists so both the CLI subcommand handlers in `main.rs` and the in-process
//! `check` / `test` builtins call one implementation. The builtin path skips
//! the `Command::new("stryke")` fork that a user would otherwise need
//! (`system "stryke check $f"`), so calling `check("foo.stk")` from inside a
//! stryke session avoids the ~5ms exec cost per file.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::error::ErrorKind;
use crate::vm_helper::VMHelper;

/// RAII guard that pins **this thread** to a specific no-interop value for
/// the duration of a check run, then restores the prior thread-local
/// override on drop. Touches only thread-local state, so calling
/// `check_no_interop()` from N parallel `pmaps` workers does not race —
/// each worker flips its own override independently and other threads
/// keep reading whatever the parent set.
struct NoInteropGuard {
    saved: Option<bool>,
}

impl NoInteropGuard {
    fn new(target: bool) -> Self {
        let saved = crate::no_interop_mode_tls();
        crate::set_no_interop_mode_tls(Some(target));
        Self { saved }
    }
}

impl Drop for NoInteropGuard {
    fn drop(&mut self) {
        crate::set_no_interop_mode_tls(self.saved);
    }
}

/// Run a single test file in-process: read source → parse → fresh
/// `VMHelper` → execute. No fork, no dyld penalty. Per-test isolation
/// comes from:
///   * fresh `VMHelper` (own scope, sub registry, package globals,
///     test counters, `test_run_failed` AtomicBool)
///   * catchable `ErrorKind::Exit` so a test calling `exit($c)` from
///     the bytecode VM doesn't terminate the runner — exit code 0 is
///     treated as "test finished normally"; any non-zero is a failure
///   * thread-local `--no-interop` override pinned for the test only
///
/// Returns `(captured_output, passes, fails, file_failed, failure_detail)`
/// matching the fork-path return shape so the caller treats them the
/// same. Captured output is currently empty — stdio capture requires
/// VM-level stdout/stderr redirection (TODO); for now test output goes
/// to the real terminal interleaved with the runner's banners.
fn run_one_inproc(
    script_abs: &Path,
    no_interop: bool,
) -> (String, usize, usize, bool, Option<String>) {
    let _ni_guard = if no_interop {
        Some(NoInteropGuard::new(true))
    } else {
        None
    };
    let file_str = script_abs.to_string_lossy().to_string();
    let source = match std::fs::read_to_string(script_abs) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("read failed: {}", e);
            return (format!("  {}\n", msg), 0, 0, true, Some(msg));
        }
    };
    let program = match crate::parse_with_file(&source, &file_str) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("{}", e);
            return (format!("  {}\n", msg), 0, 0, true, Some(msg));
        }
    };
    let mut interp = VMHelper::new();
    interp.set_file(&file_str);
    // `$0` — tests like `test_narcissist.stk` slurp their own source via
    // `slurp($0)`, which works under fork mode because the OS sets argv[0]
    // when stryke is invoked as `stryke /path/to/test.stk`. In-process
    // we have to set it manually.
    interp.program_name = file_str.clone();
    let exec_result = interp.execute(&program);
    let _ = interp.run_global_teardown();

    // Sum totals (set by `test_run` before each reset) + any tail counts
    // that never reached a `test_run` (rare but supported pattern: a test
    // that asserts but never calls `test_run` to print its block summary).
    use std::sync::atomic::Ordering::Relaxed;
    let passes = interp.test_pass_total.load(Relaxed) + interp.test_pass_count.load(Relaxed);
    let fails = interp.test_fail_total.load(Relaxed) + interp.test_fail_count.load(Relaxed);
    let test_marked_failed = interp.test_run_failed.load(Relaxed);

    let (file_failed, detail): (bool, Option<String>) = match exec_result {
        Ok(_) => {
            let f = test_marked_failed || fails > 0;
            (
                f,
                if f {
                    Some(format!("{} assertion(s) failed", fails))
                } else {
                    None
                },
            )
        }
        Err(e) => match e.kind {
            // `exit(0)` from a test = clean exit; non-zero = failure.
            ErrorKind::Exit(0) => {
                let f = test_marked_failed || fails > 0;
                (
                    f,
                    if f {
                        Some(format!("{} assertion(s) failed", fails))
                    } else {
                        None
                    },
                )
            }
            ErrorKind::Exit(c) => (true, Some(format!("exited with code {}", c))),
            _ => (true, Some(format!("{}", e))),
        },
    };

    (String::new(), passes, fails, file_failed, detail)
}

// ── Worker-pool test runner ─────────────────────────────────────────────────
//
// Two halves:
//   * `run_test_worker_loop` — invoked as `stryke --test-worker`. Reads
//     test paths line-by-line from stdin, forks per path, child runs the
//     test in-process, writes one JSON result line to stdout, `_exit`s.
//     The worker process itself never executes test bytecode, so it can't
//     accumulate state across tests — each test runs in a fresh `fork()`
//     that COW-shares the worker's hot Rust runtime (no dyld, no crate
//     static-init, no reflection rebuild).
//
//   * `run_tests_pool` — parent runner. Pre-spawns N worker processes
//     once, dispatches test paths to idle workers via stdin, reads JSON
//     results from stdout. Worker reuse skips ~8ms per test vs the
//     `posix_spawn`-per-test default.
//
// Wire format (one JSON object per line, both directions):
//   request : {"path":"/abs/path/to/test.stk","no_interop":false,"chdir":"/abs/project_root"}
//   response: {"name":"test_foo.stk","passes":3,"fails":0,"failed":false,"detail":null}

#[derive(serde::Serialize, serde::Deserialize)]
struct WorkerRequest {
    path: String,
    no_interop: bool,
    chdir: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct WorkerResponse {
    name: String,
    passes: usize,
    fails: usize,
    failed: bool,
    detail: Option<String>,
    /// Captured stderr from the grandchild (the test's own `eprintln`s
    /// — the `✓`/`✗` checkmark lines from `test_run`, plus any
    /// diagnostic prints the test makes). Captured per-test in the
    /// child so concurrent grandchildren can't tear each other's lines
    /// at the terminal; the parent prints this verbatim under
    /// `print_lock` so each test's output stays a contiguous block.
    #[serde(default)]
    stderr: String,
}

/// `stryke --test-worker` entry point. Loops on stdin reading
/// `WorkerRequest` JSON lines. Per request: `fork()`. Child runs the test
/// in-process (fresh `VMHelper`), writes `WorkerResponse` JSON to stdout,
/// `_exit(0)`. Parent worker waits for the child, then loops.
///
/// Returns the exit code stryke should use when `--test-worker` mode
/// finishes (always 0 on a normal stdin EOF).
pub fn run_test_worker_loop() -> i32 {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break, // pipe closed
        };
        if line.is_empty() {
            continue;
        }
        let req: WorkerRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let resp = WorkerResponse {
                    name: line.chars().take(60).collect(),
                    passes: 0,
                    fails: 0,
                    failed: true,
                    detail: Some(format!("worker: malformed request: {}", e)),
                    stderr: String::new(),
                };
                let mut out = stdout.lock();
                let _ = writeln!(out, "{}", serde_json::to_string(&resp).unwrap());
                let _ = out.flush();
                continue;
            }
        };

        // SAFETY: stryke's main `--test-worker` mode is invoked before
        // any threads spawn or rayon pool inits in the worker process,
        // so the only thread alive at fork() is the main thread. That
        // makes raw `libc::fork()` safe here per the POSIX rule about
        // multi-threaded forks. If you change worker init to spin up
        // threads, you must switch to a thread-aware fork strategy.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            // Portable errno read — `libc::__error` is macOS-only;
            // Linux uses `__errno_location`. `Error::last_os_error`
            // routes to whichever the platform provides.
            let err = std::io::Error::last_os_error();
            let resp = WorkerResponse {
                name: req.path.clone(),
                passes: 0,
                fails: 0,
                failed: true,
                detail: Some(format!("worker: fork failed: {}", err)),
                stderr: String::new(),
            };
            let mut out = stdout.lock();
            let _ = writeln!(out, "{}", serde_json::to_string(&resp).unwrap());
            let _ = out.flush();
            continue;
        }
        if pid == 0 {
            // ── Child: run one test, write result, exit ────────────────
            //
            // Wire-protocol firewall + per-test stderr capture.
            //
            // (1) Stdout (fd 1) goes through the worker's pipe to the
            // parent runner's JSON line reader. Test code's
            // `print "hi"` would corrupt that wire, so we redirect
            // fd 1 to /dev/null in the child and save the original
            // pipe end to a saved fd for the JSON write at the end.
            //
            // (2) Stderr (fd 2) is where `test_run` emits its
            // `✓ assertion` checkmark lines. If we inherited the
            // worker's stderr to the terminal directly, 18 concurrent
            // grandchildren writing in parallel would tear each
            // other's lines (POSIX writes < PIPE_BUF are atomic but
            // line-aligned coalescing isn't). Instead: redirect
            // fd 2 to a per-test tmp file, run the test, slurp the
            // file back into the JSON response, parent prints it
            // verbatim under `print_lock` so each test's output stays
            // a contiguous block.
            let (saved_stdout, stderr_capture_fd, mut stderr_path): (
                libc::c_int,
                libc::c_int,
                [u8; 32],
            ) = unsafe {
                let saved = libc::dup(1);
                if saved >= 0 {
                    let devnull = libc::open(
                        b"/dev/null\0".as_ptr() as *const libc::c_char,
                        libc::O_WRONLY,
                    );
                    if devnull >= 0 {
                        libc::dup2(devnull, 1);
                        libc::close(devnull);
                    }
                }
                // mkstemp template — must be writable and end in XXXXXX.
                // Buffer is 24-byte template + NUL + 7 padding = 32 bytes;
                // mkstemp writes the resolved name back into it in place.
                let mut tmpl: [u8; 32] = *b"/tmp/stryke-test-XXXXXX\0\0\0\0\0\0\0\0\0";
                let cap_fd = libc::mkstemp(tmpl.as_mut_ptr() as *mut libc::c_char);
                if cap_fd >= 0 {
                    // Unlink immediately — file persists while the fd
                    // is open, then disappears on close. No /tmp clutter.
                    libc::unlink(tmpl.as_ptr() as *const libc::c_char);
                    libc::dup2(cap_fd, 2);
                }
                (saved, cap_fd, tmpl)
            };
            let _ = &mut stderr_path; // keep `stderr_path` alive through unsafe block lifetime

            // chdir for `require "./lib/foo.stk"` resolution, same as the
            // fork-per-test path's `Command::current_dir`.
            if let Some(cd) = req.chdir.as_deref() {
                let _ = std::env::set_current_dir(cd);
            }
            let script_abs = PathBuf::from(&req.path);
            let (_block, passes, fails, file_failed, detail) =
                run_one_inproc(&script_abs, req.no_interop);
            let name = script_abs
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| req.path.clone());
            // Slurp the captured stderr back. fd is at the end of the
            // file from the test's writes; rewind, read everything.
            let captured_stderr: String = unsafe {
                if stderr_capture_fd >= 0 {
                    libc::lseek(stderr_capture_fd, 0, libc::SEEK_SET);
                    let mut buf = Vec::with_capacity(4096);
                    let mut chunk = [0u8; 8192];
                    loop {
                        let n = libc::read(
                            stderr_capture_fd,
                            chunk.as_mut_ptr() as *mut libc::c_void,
                            chunk.len(),
                        );
                        if n <= 0 {
                            break;
                        }
                        buf.extend_from_slice(&chunk[..n as usize]);
                    }
                    libc::close(stderr_capture_fd);
                    String::from_utf8_lossy(&buf).into_owned()
                } else {
                    String::new()
                }
            };

            let resp = WorkerResponse {
                name,
                passes,
                fails,
                failed: file_failed,
                detail,
                stderr: captured_stderr,
            };
            let line = format!(
                "{}\n",
                serde_json::to_string(&resp).unwrap_or_else(|_| String::new())
            );
            unsafe {
                if saved_stdout >= 0 {
                    let _ = libc::write(
                        saved_stdout,
                        line.as_ptr() as *const libc::c_void,
                        line.len(),
                    );
                    libc::close(saved_stdout);
                }
            }
            // `_exit` skips Rust's atexit / Drop chain, which is correct
            // for a forked child — running parent destructors here
            // would close fds the parent still needs and double-free
            // any shared allocations.
            unsafe { libc::_exit(0) };
        }
        // ── Worker (parent of grandchild): wait, then loop ────────────
        let mut status: libc::c_int = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
    }
    0
}

/// Worker-pool test runner. Spawns `n_workers` persistent stryke
/// `--test-worker` processes once, then dispatches test paths to them
/// via stdin, reads results from stdout. Workers stay alive across all
/// tests — they fork per request, so no state accumulates in them.
pub fn run_tests_pool(
    targets: &[String],
    j_threads: Option<&str>,
    no_interop: bool,
    quiet: bool,
) -> i32 {
    // Discovery / target normalization mirrors `run_tests_with_mode`.
    let targets: Vec<String> = if targets.is_empty() {
        if Path::new("t").is_dir() {
            vec!["t".to_string()]
        } else if Path::new("tests").is_dir() {
            vec!["tests".to_string()]
        } else {
            eprintln!("stryke test: no t/ or tests/ directory found");
            return 1;
        }
    } else {
        targets
            .iter()
            .filter(|t| Path::new(t.as_str()).exists())
            .cloned()
            .collect()
    };
    if targets.is_empty() {
        eprintln!("stryke test: no valid paths found");
        return 1;
    }
    let mut test_files: Vec<String> = Vec::new();
    for target in &targets {
        let target_path = Path::new(target);
        if target_path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(target_path) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path().to_string_lossy().to_string();
                    let name = entry.file_name().to_string_lossy().to_string();
                    if (name.starts_with("test_") || name.starts_with("t_"))
                        && (name.ends_with(".stk")
                            || name.ends_with(".st")
                            || name.ends_with(".pl"))
                    {
                        test_files.push(path);
                    }
                }
            }
        } else {
            test_files.push(target.clone());
        }
    }
    test_files.sort();
    test_files.dedup();
    if test_files.is_empty() {
        eprintln!("stryke test: no test files found");
        return 1;
    }
    let total = test_files.len();

    let n_workers = j_threads
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        });

    let exe: PathBuf = std::env::current_exe()
        .ok()
        .filter(|p| p.exists())
        .or_else(|| {
            std::env::args()
                .next()
                .and_then(|a| std::fs::canonicalize(&a).ok())
        })
        .unwrap_or_else(|| {
            PathBuf::from(std::env::args().next().unwrap_or_else(|| "stryke".into()))
        });

    if !quiet {
        eprintln!(
            "\x1b[36mRunning {} test file{} ({} workers)\x1b[0m\n",
            total,
            if total == 1 { "" } else { "s" },
            n_workers
        );
    }

    // Pre-resolve canonical paths + project_root per file (the worker
    // child will chdir to project_root so `require "./lib/..."` works).
    let jobs: Vec<(String, String, String)> = test_files
        .iter()
        .map(|f| {
            let abs = std::fs::canonicalize(f)
                .unwrap_or_else(|_| PathBuf::from(f))
                .to_string_lossy()
                .to_string();
            let chdir = Path::new(&abs)
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".to_string());
            let name = Path::new(&abs)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| abs.clone());
            (name, abs, chdir)
        })
        .collect();

    let (job_tx, job_rx) = crossbeam::channel::unbounded::<(String, String, String)>();
    for j in jobs {
        job_tx.send(j).expect("send job");
    }
    drop(job_tx);

    let total_pass = Arc::new(AtomicUsize::new(0));
    let total_fail = Arc::new(AtomicUsize::new(0));
    let failed_count = Arc::new(AtomicUsize::new(0));
    let failure_details: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let print_lock: Arc<Mutex<()>> = Arc::new(Mutex::new(()));

    let mut handles = Vec::with_capacity(n_workers);
    for _ in 0..n_workers {
        let job_rx = job_rx.clone();
        let total_pass = Arc::clone(&total_pass);
        let total_fail = Arc::clone(&total_fail);
        let failed_count = Arc::clone(&failed_count);
        let failure_details = Arc::clone(&failure_details);
        let print_lock = Arc::clone(&print_lock);
        let exe = exe.clone();
        let h = thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let mut child = match process::Command::new(&exe)
                    .arg("--test-worker")
                    .stdin(process::Stdio::piped())
                    .stdout(process::Stdio::piped())
                    .stderr(process::Stdio::inherit())
                    .spawn()
                {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("stryke test: failed to spawn worker: {}", e);
                        return;
                    }
                };
                let mut stdin = child.stdin.take().expect("worker stdin");
                let stdout = child.stdout.take().expect("worker stdout");
                let mut reader = BufReader::new(stdout);
                let mut line_buf = String::new();

                while let Ok((_short_name, abs, chdir)) = job_rx.recv() {
                    let req = WorkerRequest {
                        path: abs.clone(),
                        no_interop,
                        chdir: Some(chdir),
                    };
                    let req_json = serde_json::to_string(&req).expect("serialize");
                    if writeln!(stdin, "{}", req_json).is_err() {
                        break;
                    }
                    if stdin.flush().is_err() {
                        break;
                    }

                    line_buf.clear();
                    if reader.read_line(&mut line_buf).is_err() || line_buf.is_empty() {
                        break;
                    }
                    let resp: WorkerResponse = match serde_json::from_str(line_buf.trim()) {
                        Ok(r) => r,
                        Err(e) => WorkerResponse {
                            name: abs.clone(),
                            passes: 0,
                            fails: 0,
                            failed: true,
                            detail: Some(format!("malformed worker response: {}", e)),
                            stderr: String::new(),
                        },
                    };

                    total_pass.fetch_add(resp.passes, Ordering::Relaxed);
                    total_fail.fetch_add(resp.fails, Ordering::Relaxed);
                    if resp.failed {
                        failed_count.fetch_add(1, Ordering::Relaxed);
                        if let Some(d) = &resp.detail {
                            failure_details
                                .lock()
                                .unwrap()
                                .push((resp.name.clone(), d.clone()));
                        }
                    }

                    if !quiet {
                        let _g = print_lock.lock().unwrap();
                        eprintln!("\x1b[1m── {} ──\x1b[0m", resp.name);
                        // Captured stderr from the grandchild — the
                        // `✓`/`✗` checkmark lines plus any test
                        // diagnostic prints. Print verbatim so each
                        // test's output is a contiguous block (no
                        // line-tearing across concurrent workers).
                        if !resp.stderr.is_empty() {
                            eprint!("{}", resp.stderr);
                        }
                        eprintln!(
                            "  {} passed, {} failed{}",
                            resp.passes,
                            resp.fails,
                            if resp.failed { " (file FAILED)" } else { "" }
                        );
                        eprintln!();
                    }
                }
                drop(stdin); // close worker stdin → worker exits its loop
                let _ = child.wait();
            })
            .expect("spawn worker thread");
        handles.push(h);
    }
    for h in handles {
        let _ = h.join();
    }

    let failed = failed_count.load(Ordering::Relaxed);
    let total_pass = total_pass.load(Ordering::Relaxed);
    let total_fail = total_fail.load(Ordering::Relaxed);
    let failure_details = Arc::try_unwrap(failure_details)
        .expect("workers dropped failure_details Arc")
        .into_inner()
        .unwrap();
    let grand_total = total_pass + total_fail;
    if !quiet {
        eprintln!("═══════════════════════════════");
    }
    if failed == 0 {
        if !quiet {
            eprintln!(
                "\x1b[32m✓ All {} test file{} passed ({} assertions)\x1b[0m",
                total,
                if total == 1 { "" } else { "s" },
                grand_total
            );
        }
        0
    } else {
        if !quiet {
            eprintln!();
            eprintln!(
                "\x1b[1;31m════════════════════════════════════════════════════════════════\x1b[0m"
            );
            eprintln!("\x1b[1;31m                        FAILURES SUMMARY\x1b[0m");
            eprintln!(
                "\x1b[1;31m════════════════════════════════════════════════════════════════\x1b[0m"
            );
            for (file_name, details) in &failure_details {
                eprintln!();
                eprintln!("\x1b[1;33m── {} ──\x1b[0m", file_name);
                for line in details.lines().take(20) {
                    eprintln!("  {}", line);
                }
                if details.lines().count() > 20 {
                    eprintln!("  ... ({} more lines)", details.lines().count() - 20);
                }
            }
            eprintln!();
            eprintln!(
                "\x1b[1;31m════════════════════════════════════════════════════════════════\x1b[0m"
            );
            eprintln!(
                "\x1b[31m✗ {} of {} test file{} failed ({} passed, {} failed)\x1b[0m",
                failed,
                total,
                if total == 1 { "" } else { "s" },
                total_pass,
                total_fail
            );
        }
        1
    }
}

/// `stryke check FILE...` — parse + compile + lint without executing.
/// Returns 0 on success, 1 on errors, 2 on usage error.
///
/// `no_interop` overrides the global `--no-interop` flag for the duration
/// of the check (saved and restored via RAII). `None` inherits whatever the
/// parent process already has set, `Some(true)` enforces no-interop, and
/// `Some(false)` temporarily lifts it.
pub fn run_check(
    files: &[String],
    quiet: bool,
    json_output: bool,
    no_interop: Option<bool>,
) -> i32 {
    let _guard = no_interop.map(NoInteropGuard::new);
    if files.is_empty() {
        eprintln!("stryke check: no files specified");
        return 2;
    }

    let mut errors = 0;
    for file in files {
        let source = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                if json_output {
                    println!(
                        r#"{{"file":"{}","line":0,"col":0,"severity":"error","message":"{}"}}"#,
                        file,
                        e.to_string().replace('"', "\\\"")
                    );
                } else {
                    eprintln!("{}:0:0: error: {}", file, e);
                }
                errors += 1;
                continue;
            }
        };

        let program = match crate::parse_with_file(&source, file) {
            Ok(p) => p,
            Err(e) => {
                if json_output {
                    println!(
                        r#"{{"file":"{}","line":{},"col":0,"severity":"error","message":"{}"}}"#,
                        file,
                        e.line,
                        e.to_string().replace('"', "\\\"").replace('\n', "\\n")
                    );
                } else {
                    eprintln!("{}:{}:0: error: {}", file, e.line, e);
                }
                errors += 1;
                continue;
            }
        };

        let mut interp = VMHelper::new();
        interp.set_file(file);
        match crate::lint_program(&program, &mut interp) {
            Ok(()) => {
                if !quiet && !json_output {
                    eprintln!("{}: OK", file);
                }
            }
            Err(e) => {
                if json_output {
                    println!(
                        r#"{{"file":"{}","line":{},"col":0,"severity":"error","message":"{}"}}"#,
                        file,
                        e.line,
                        e.to_string().replace('"', "\\\"").replace('\n', "\\n")
                    );
                } else {
                    eprintln!("{}:{}:0: error: {}", file, e.line, e);
                }
                errors += 1;
            }
        }
    }

    if errors > 0 {
        if !quiet && !json_output {
            eprintln!();
            eprintln!(
                "{} error{} in {} file{}",
                errors,
                if errors == 1 { "" } else { "s" },
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            );
        }
        1
    } else {
        if !quiet && !json_output && files.len() > 1 {
            eprintln!();
            eprintln!("All {} files OK", files.len());
        }
        0
    }
}

/// `stryke test [FILE|DIR...]` — run test files. Returns 0 on all-pass,
/// 1 on any failure.
///
/// **In-process by default**: each test gets a fresh `VMHelper` on a
/// worker thread, no fork. ~20× faster than spawning a stryke subprocess
/// per file because we skip the dyld load + crate static-init cost
/// (~9ms warm × N tests). Tests stay isolated via the per-VM scope,
/// per-VM `test_run_failed` / `test_pass_count` counters, and catchable
/// `ErrorKind::Exit` (so `exit($c)` from a test doesn't terminate the
/// runner). Pass `force_fork=true` to opt back into the legacy
/// process-per-test model when a test needs real OS-level isolation
/// (mutating `%ENV`, `chdir`, signal handlers, `fork()`, etc.).
///
/// `j_threads` controls outer parallelism. `None` defaults to all
/// logical CPUs; `Some("1")` runs tests serially.
///
/// `no_interop=true` runs tests under stryke's bot-firewall mode
/// (rejects Perl-isms). In-process: pins each worker thread's TLS
/// no-interop flag for the duration of the test. Forked: forwards
/// `--no-interop` to each child.
///
/// `quiet=true` suppresses per-file headers, captured output, and the
/// failure summary so callers iterating over many test dirs can render
/// their own one-line summary per dir.
pub fn run_tests(
    targets: &[String],
    j_threads: Option<&str>,
    no_interop: bool,
    quiet: bool,
) -> i32 {
    // Default = **worker pool**. Pre-forks N persistent stryke worker
    // processes; each worker fork-on-receive per test so the worker
    // process itself never runs test bytecode (zero corruption risk),
    // while every test still runs in its own fresh forked child (full
    // isolation). ~5× faster than `posix_spawn`-per-test on big corpora
    // because the forked child COW-shares its hot worker's Rust runtime
    // (no dyld, no crate static-init, no reflection rebuild).
    run_tests_pool(targets, j_threads, no_interop, quiet)
}

/// Underlying entry point with explicit fork/in-process selection. The
/// public `run_tests` always picks in-process; the CLI's `--fork` flag
/// (and any other caller that explicitly wants subprocess isolation)
/// goes through this with `force_fork=true`.
pub fn run_tests_with_mode(
    targets: &[String],
    j_threads: Option<&str>,
    no_interop: bool,
    quiet: bool,
    force_fork: bool,
) -> i32 {
    let targets: Vec<String> = if targets.is_empty() {
        if Path::new("t").is_dir() {
            vec!["t".to_string()]
        } else if Path::new("tests").is_dir() {
            vec!["tests".to_string()]
        } else {
            eprintln!("stryke test: no t/ or tests/ directory found");
            return 1;
        }
    } else {
        targets
            .iter()
            .filter(|t| Path::new(t.as_str()).exists())
            .cloned()
            .collect()
    };
    if targets.is_empty() {
        eprintln!("stryke test: no valid paths found");
        return 1;
    }
    let mut test_files: Vec<String> = Vec::new();
    for target in &targets {
        let target_path = Path::new(target);
        if target_path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(target_path) {
                for entry in entries.filter_map(|e| e.ok()) {
                    let path = entry.path().to_string_lossy().to_string();
                    let name = entry.file_name().to_string_lossy().to_string();
                    if (name.starts_with("test_") || name.starts_with("t_"))
                        && (name.ends_with(".stk")
                            || name.ends_with(".st")
                            || name.ends_with(".pl"))
                    {
                        test_files.push(path);
                    }
                }
            }
        } else {
            test_files.push(target.clone());
        }
    }
    test_files.sort();
    test_files.dedup();
    if test_files.is_empty() {
        eprintln!("stryke test: no test files found");
        return 1;
    }
    let total = test_files.len();
    if !quiet {
        eprintln!(
            "\x1b[36mRunning {} test file{}\x1b[0m\n",
            total,
            if total == 1 { "" } else { "s" }
        );
    }

    // Resolve the stryke exe once on the parent thread so every child
    // worker reuses it without re-doing the `current_exe` / `canonicalize`
    // lookup. `std::env::args` and `current_exe` are safe to call from
    // worker threads but pointless to repeat 200 times.
    let exe: PathBuf = std::env::current_exe()
        .ok()
        .filter(|p| p.exists())
        .or_else(|| {
            std::env::args()
                .next()
                .and_then(|a| std::fs::canonicalize(&a).ok())
        })
        .unwrap_or_else(|| {
            PathBuf::from(std::env::args().next().unwrap_or_else(|| "stryke".into()))
        });

    // Streaming-parallel: a fixed pool of `n_threads` worker threads pull
    // test paths from a crossbeam channel, run each in a child process,
    // then flush that child's full stderr block under `print_lock` the
    // moment it finishes. We use **explicit `std::thread`s + a channel**
    // instead of `rayon::par_iter` because the inner `Command::output()`
    // is a blocking syscall — rayon's work-stealing scheduler is tuned
    // for CPU-bound chunks and starves under blocking I/O (~3 cores
    // active out of 18 in benches), while a dumb thread-per-worker queue
    // saturates all cores (xargs hits 1200% CPU on the same workload).
    let total_pass = AtomicUsize::new(0);
    let total_fail = AtomicUsize::new(0);
    let failed_count = AtomicUsize::new(0);
    let failure_details: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
    let print_lock: Mutex<()> = Mutex::new(());

    // Outer-parallelism comes from `-j N` (same flag the user types to
    // control parallel builtins). When unset we default to all logical
    // CPUs. `s -j 1 t …` correctly serializes; `s -j 8 t …` runs 8 test
    // files concurrently.
    let n_threads = j_threads
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        });

    // Pre-canonicalize every path on the main thread so workers don't
    // each redo the syscalls in parallel (filesystem stat() is one of
    // the per-spawn bottlenecks).
    let raw_jobs: Vec<(String, PathBuf, PathBuf)> = test_files
        .iter()
        .map(|f| {
            let name = Path::new(f)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| f.clone());
            let script_abs = std::fs::canonicalize(f).unwrap_or_else(|_| PathBuf::from(f));
            let project_root = script_abs
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            (name, script_abs, project_root)
        })
        .collect();

    // Group by project_root so we can `chdir` once per group and run
    // every test in that group concurrently. Process cwd is process-
    // wide (libc has no per-thread cwd on macOS), so cross-group
    // parallelism would race; cross-group is therefore serial, intra-
    // group is parallel. Most invocations pass a single dir so this is
    // one group with full parallelism. Forked path doesn't need this
    // (each child gets its own `current_dir(...)`), but we still group
    // for output ordering consistency.
    let mut by_root: std::collections::BTreeMap<PathBuf, Vec<(String, PathBuf)>> =
        std::collections::BTreeMap::new();
    for (name, abs, root) in raw_jobs {
        by_root.entry(root).or_default().push((name, abs));
    }

    let exe = Arc::new(exe);
    let total_pass = Arc::new(total_pass);
    let total_fail = Arc::new(total_fail);
    let failed_count = Arc::new(failed_count);
    let failure_details = Arc::new(failure_details);
    let print_lock = Arc::new(print_lock);

    for (root, group) in by_root {
        // Chdir once per group so `require "./lib/foo.stk"` resolves
        // the way the test author expects (relative to project_root).
        // Skip when fork mode is in effect — children get cwd via
        // `Command::current_dir`. Failure to chdir is logged but not
        // fatal (rare; might fire if root was deleted between glob and now).
        if !force_fork {
            if let Err(e) = std::env::set_current_dir(&root) {
                eprintln!(
                    "stryke test: cannot chdir to {}: {} (skipping group)",
                    root.display(),
                    e
                );
                continue;
            }
        }

        let (tx, rx) = crossbeam::channel::unbounded::<(String, PathBuf, PathBuf)>();
        for (name, abs) in group {
            tx.send((name, abs, root.clone())).expect("send job");
        }
        drop(tx);

        let mut handles = Vec::with_capacity(n_threads);
        for _ in 0..n_threads {
            let rx = rx.clone();
            let exe = Arc::clone(&exe);
            let total_pass = Arc::clone(&total_pass);
            let total_fail = Arc::clone(&total_fail);
            let failed_count = Arc::clone(&failed_count);
            let failure_details = Arc::clone(&failure_details);
            let print_lock = Arc::clone(&print_lock);
            let j_threads_owned: Option<String> = j_threads.map(|s| s.to_string());
            let force_inner_one = n_threads > 1;
            let no_interop = no_interop;
            let quiet = quiet;
            // 16 MB stack per worker — stryke's VM can recurse deeply on
            // heavy tests (the default 2 MB std-thread stack overflows on
            // some recursion-heavy ones). Matches the main stryke binary's
            // primary-thread stack size.
            let h = thread::Builder::new()
                .stack_size(16 * 1024 * 1024)
                .spawn(move || {
                    while let Ok((name, script_abs, project_root)) = rx.recv() {
                        // ── In-process path ────────────────────────────────────
                        if !force_fork {
                            let (block, passes, fails, file_failed, detail) =
                                run_one_inproc(&script_abs, no_interop);
                            total_pass.fetch_add(passes, Ordering::Relaxed);
                            total_fail.fetch_add(fails, Ordering::Relaxed);
                            if file_failed {
                                failed_count.fetch_add(1, Ordering::Relaxed);
                                if let Some(d) = detail {
                                    failure_details.lock().unwrap().push((name.clone(), d));
                                }
                            }
                            if !quiet {
                                let _g = print_lock.lock().unwrap();
                                eprintln!("\x1b[1m── {} ──\x1b[0m", name);
                                eprint!("{}", block);
                                eprintln!();
                            }
                            continue;
                        }

                        // ── Fork-per-test path (legacy / `--fork`) ─────────────
                        let mut cmd = process::Command::new(&*exe);
                        // Anti-oversubscription: when the runner is parallel,
                        // force every child to `-j 1` so we don't end up with
                        // `n_threads × num_cpus` threads fighting for the cores.
                        if force_inner_one {
                            cmd.arg("-j").arg("1");
                        } else if let Some(n) = j_threads_owned.as_deref() {
                            cmd.arg("-j").arg(n);
                        }
                        if no_interop {
                            cmd.arg("--no-interop");
                        }
                        let output = cmd
                            .arg(&script_abs)
                            .current_dir(&project_root)
                            .stderr(process::Stdio::piped())
                            .output();
                        let (block, passes, fails, file_failed, detail) = match output {
                            Ok(out) => {
                                let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
                                let mut passes = 0usize;
                                let mut fails = 0usize;
                                let mut file_failures: Vec<String> = Vec::new();
                                for line in stderr.lines() {
                                    let trimmed = line.trim_start();
                                    if trimmed.starts_with("\x1b[32m✓\x1b[0m")
                                        || trimmed.starts_with("✓")
                                    {
                                        if !trimmed.contains("All ") && !trimmed.contains(" passed")
                                        {
                                            passes += 1;
                                        }
                                    } else if (trimmed.starts_with("\x1b[31m✗\x1b[0m")
                                        || trimmed.starts_with("✗"))
                                        && (!trimmed.contains(" of ")
                                            || !trimmed.contains(" failed"))
                                    {
                                        fails += 1;
                                        file_failures.push(line.to_string());
                                    }
                                }
                                let failed = !out.status.success() || !file_failures.is_empty();
                                let detail = if failed {
                                    let error_output: String = stderr
                                        .lines()
                                        .filter(|l| {
                                            let t = l.trim_start();
                                            t.starts_with("\x1b[31m✗")
                                                || t.starts_with("✗")
                                                || t.contains("error:")
                                                || t.contains("Error:")
                                                || t.contains("panicked")
                                                || t.contains("FAILED")
                                                || t.contains(" at ")
                                                    && (t.contains(" line ") || t.contains(".stk"))
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    Some(if !error_output.is_empty() {
                                        error_output
                                    } else if !file_failures.is_empty() {
                                        file_failures.join("\n")
                                    } else {
                                        stderr.clone()
                                    })
                                } else {
                                    None
                                };
                                (stderr, passes, fails, failed, detail)
                            }
                            Err(e) => {
                                let msg = format!("failed to run: {}", e);
                                (format!("  {}\n", msg), 0, 0, true, Some(msg))
                            }
                        };

                        total_pass.fetch_add(passes, Ordering::Relaxed);
                        total_fail.fetch_add(fails, Ordering::Relaxed);
                        if file_failed {
                            failed_count.fetch_add(1, Ordering::Relaxed);
                            if let Some(d) = detail {
                                failure_details.lock().unwrap().push((name.clone(), d));
                            }
                        }

                        if !quiet {
                            // Per-block lock so concurrent finishers don't tear
                            // each other's lines.
                            let _g = print_lock.lock().unwrap();
                            eprintln!("\x1b[1m── {} ──\x1b[0m", name);
                            eprint!("{}", block);
                            eprintln!();
                        }
                    }
                })
                .expect("spawn worker thread");
            handles.push(h);
        }
        for h in handles {
            let _ = h.join();
        }
    }

    let failed = failed_count.load(Ordering::Relaxed);
    let total_pass = total_pass.load(Ordering::Relaxed);
    let total_fail = total_fail.load(Ordering::Relaxed);
    let failure_details = Arc::try_unwrap(failure_details)
        .expect("workers dropped Arc<failure_details>")
        .into_inner()
        .unwrap();
    let grand_total = total_pass + total_fail;
    if !quiet {
        eprintln!("═══════════════════════════════");
    }
    if failed == 0 {
        if !quiet {
            eprintln!(
                "\x1b[32m✓ All {} test file{} passed ({} assertions)\x1b[0m",
                total,
                if total == 1 { "" } else { "s" },
                grand_total
            );
        }
        0
    } else {
        if !quiet {
            eprintln!();
            eprintln!(
                "\x1b[1;31m════════════════════════════════════════════════════════════════\x1b[0m"
            );
            eprintln!("\x1b[1;31m                        FAILURES SUMMARY\x1b[0m");
            eprintln!(
                "\x1b[1;31m════════════════════════════════════════════════════════════════\x1b[0m"
            );
            for (file_name, details) in &failure_details {
                eprintln!();
                eprintln!("\x1b[1;33m── {} ──\x1b[0m", file_name);
                for line in details.lines().take(20) {
                    eprintln!("  {}", line);
                }
                if details.lines().count() > 20 {
                    eprintln!("  ... ({} more lines)", details.lines().count() - 20);
                }
            }
            eprintln!();
            eprintln!(
                "\x1b[1;31m════════════════════════════════════════════════════════════════\x1b[0m"
            );
            eprintln!(
                "\x1b[31m✗ {} of {} test file{} failed ({} passed, {} failed)\x1b[0m",
                failed,
                total,
                if total == 1 { "" } else { "s" },
                total_pass,
                total_fail
            );
        }
        1
    }
}

/// Extract `-j N` from the current process's argv (the parent CLI args).
/// Used by the `test` builtin to forward the parallel-pool size to spawned
/// child stryke processes the same way `stryke -j N test t/` does.
pub fn parent_j_threads() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let pos = args.iter().position(|a| a == "-j")?;
    args.get(pos + 1).cloned()
}
