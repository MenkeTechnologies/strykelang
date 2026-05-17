//! Structured shell output: `capture("cmd")`.

use std::process::Command;
use std::sync::Arc;

use crate::error::{StrykeError, StrykeResult};
use crate::perl_decode::decode_utf8_or_latin1;
use crate::value::{CaptureResult, StrykeValue};
use crate::vm_helper::VMHelper;

/// Run `cmd` through `sh -c` and return stdout as a string (Perl `` `...` `` / `qx`).
/// Updates [`VMHelper::child_exit_status`] (`$?`) like [`run_capture`] and `system`.
pub fn run_readpipe(interp: &mut VMHelper, cmd: &str, line: usize) -> StrykeResult<StrykeValue> {
    let output = match Command::new("sh").arg("-c").arg(cmd).output() {
        Ok(o) => o,
        Err(e) => {
            interp.errno = e.to_string();
            interp.child_exit_status = -1;
            return Err(StrykeError::runtime(format!("readpipe: {}", e), line));
        }
    };
    interp.record_child_exit_status(output.status);
    Ok(StrykeValue::string(decode_utf8_or_latin1(&output.stdout)))
}

/// Run `cmd` through `sh -c` and return stdout, stderr, and exit code.
/// Updates [`VMHelper::child_exit_status`] (`$?`) like `system` and backticks.
pub fn run_capture(interp: &mut VMHelper, cmd: &str, line: usize) -> StrykeResult<StrykeValue> {
    let output = match Command::new("sh").arg("-c").arg(cmd).output() {
        Ok(o) => o,
        Err(e) => {
            interp.errno = e.to_string();
            interp.child_exit_status = -1;
            return Err(StrykeError::runtime(format!("capture: {}", e), line));
        }
    };
    interp.record_child_exit_status(output.status);
    let exitcode = output.status.code().unwrap_or(-1) as i64;
    let stdout = decode_utf8_or_latin1(&output.stdout);
    let stderr = decode_utf8_or_latin1(&output.stderr);
    Ok(StrykeValue::capture(Arc::new(CaptureResult {
        stdout,
        stderr,
        exitcode,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_readpipe_echo_stdout_string() {
        let mut interp = VMHelper::new();
        let v = run_readpipe(&mut interp, "echo stryke_readpipe_ok", 1).expect("readpipe");
        assert_eq!(v.to_string(), "stryke_readpipe_ok\n");
    }

    #[test]
    fn run_capture_echo_stdout_exit_zero() {
        let mut interp = VMHelper::new();
        let v = run_capture(&mut interp, "echo stryke_capture_ok", 1).expect("capture");
        let c = v.as_capture().expect("capture StrykeValue");
        assert_eq!(c.exitcode, 0, "stderr={:?}", c.stderr);
        assert!(
            c.stdout.contains("stryke_capture_ok"),
            "stdout={:?}",
            c.stdout
        );
    }

    #[test]
    fn run_capture_false_nonzero_exit() {
        let mut interp = VMHelper::new();
        let v = run_capture(&mut interp, "false", 1).expect("capture");
        let c = v.as_capture().expect("capture StrykeValue");
        assert_ne!(c.exitcode, 0);
    }
}
