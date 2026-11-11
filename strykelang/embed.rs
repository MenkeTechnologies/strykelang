//! Uniform embedding entry point: run a whole program against a string stdin
//! and get its stdout back.

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::error::ErrorKind;
use crate::vm_helper::VMHelper;

/// Stack for the interpreter thread; matches the CLI test workers.
const INTERPRETER_STACK: usize = 16 * 1024 * 1024;

/// Run `src` as a complete program with `input` as its stdin and return
/// everything it wrote to stdout.
///
/// Each call parses into a fresh [`VMHelper`] on its own thread, so nothing
/// leaks between calls and concurrent calls never share a buffer. STDOUT
/// writes (`p`, `print`, `say`, `printf`) are captured via
/// [`VMHelper::begin_stdout_capture`]; STDERR still reaches the process.
/// STDIN reads (`<STDIN>`, `<>`, `input`, `getc`, `read`) consume `input`.
///
/// Parse errors, `die`, runtime errors, `exit N` with N != 0 and panics inside
/// the interpreter return `Err`. `exit 0` returns `Ok` with the output so far.
pub fn run_program(src: &str, input: &str) -> Result<String, String> {
    let (src, input) = (src.to_owned(), input.to_owned());
    std::thread::Builder::new()
        .name("stryke-embed".into())
        .stack_size(INTERPRETER_STACK)
        .spawn(move || {
            catch_unwind(AssertUnwindSafe(|| run_captured(&src, &input)))
                .unwrap_or_else(|p| Err(panic_message(p.as_ref())))
        })
        .map_err(|e| format!("stryke: cannot spawn interpreter thread: {e}"))?
        .join()
        .unwrap_or_else(|p| Err(panic_message(p.as_ref())))
}

fn run_captured(src: &str, input: &str) -> Result<String, String> {
    let program = crate::parse(src).map_err(|e| e.to_string())?;
    let mut interp = VMHelper::new();
    interp.begin_stdout_capture();
    interp.set_stdin(input);
    let result = interp
        .execute(&program)
        .and_then(|_| interp.run_global_teardown());
    let output = interp.end_capture();
    match result {
        Ok(()) => Ok(output),
        Err(e) => match e.kind {
            ErrorKind::Exit(0) => Ok(output),
            ErrorKind::Exit(code) => Err(format!("exit status {code}")),
            _ => Err(e.to_string()),
        },
    }
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    let msg = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_string());
    format!("stryke: interpreter panicked: {msg}")
}
