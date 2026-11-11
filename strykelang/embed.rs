//! Uniform embedding entry point: run a whole program against a string stdin
//! and get its stdout back.

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::error::ErrorKind;
use crate::vm_helper::{EmbedIo, VMHelper};

/// Stack for the interpreter thread; matches the CLI test workers.
const INTERPRETER_STACK: usize = 16 * 1024 * 1024;

/// Run `src` as a complete program with `input` as its stdin and return
/// everything it wrote to stdout.
///
/// Each call runs a fresh [`VMHelper`] on its own OS thread (16 MiB stack)
/// with this call's [`EmbedIo`] installed. Worker interpreters inherit it:
/// rayon tasks (`pmap`, `fan`, `preduce`, ...) through [`EmbedIo::for_task`],
/// OS-thread workers (`async`, pipelines, `ppool`, ...) through
/// [`EmbedIo::carry`]. So every interpreter of the run writes STDOUT (`p`,
/// `print`, `say`, `printf`) into one per-call buffer and reads STDIN
/// (`<STDIN>`, `<>`, `input`, `getc`, `read`) from `input`. Parallel work runs
/// on rayon's global pool, as in the CLI. Nothing leaks between calls and
/// concurrent calls never share a buffer. STDERR still reaches the process.
///
/// Parse errors, `die`, runtime errors, `exit N` with N != 0 and panics inside
/// the interpreter return `Err`. `exit 0` returns `Ok` with the output so far.
pub fn run_program(src: &str, input: &str) -> Result<String, String> {
    let io = EmbedIo::stdout_capture_with_stdin(input);
    let (src, thread_io) = (src.to_owned(), io.clone());
    let result = std::thread::Builder::new()
        .name("stryke-embed".into())
        .stack_size(INTERPRETER_STACK)
        .spawn(move || {
            thread_io.install();
            catch_unwind(AssertUnwindSafe(|| run(&src)))
                .unwrap_or_else(|p| Err(panic_message(p.as_ref())))
        })
        .map_err(|e| format!("stryke: cannot spawn interpreter thread: {e}"))?
        .join()
        .unwrap_or_else(|p| Err(panic_message(p.as_ref())));
    let output = io.take_output();
    match result {
        Ok(()) => Ok(output),
        Err(Exit::Success) => Ok(output),
        Err(Exit::Failure(msg)) => Err(msg),
    }
}

enum Exit {
    /// `exit 0`: stop early, but the run succeeded.
    Success,
    Failure(String),
}

/// Parse and run `src` on the current thread; the interpreter picks up
/// the thread's installed [`EmbedIo`] in [`VMHelper::new`].
fn run(src: &str) -> Result<(), Exit> {
    let program = crate::parse(src).map_err(|e| Exit::Failure(e.to_string()))?;
    let mut interp = VMHelper::new();
    interp
        .execute(&program)
        .and_then(|_| interp.run_global_teardown())
        .map_err(|e| match e.kind {
            ErrorKind::Exit(0) => Exit::Success,
            ErrorKind::Exit(code) => Exit::Failure(format!("exit status {code}")),
            _ => Exit::Failure(e.to_string()),
        })
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> Exit {
    let msg = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string());
    Exit::Failure(format!("stryke: interpreter panicked: {msg}"))
}
