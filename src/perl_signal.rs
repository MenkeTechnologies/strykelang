//! Minimal `%SIG` delivery for common signals (Unix). Handlers run between statements.
//!
//! Signal hooks install lazily the first time Perl code assigns `$SIG{NAME}` (or the perlrs
//! runtime asks for a specific signal). Until then we leave the POSIX default in place so that
//! **Ctrl-C terminates immediately** on scripts that do not trap `SIGINT` — otherwise `signal_hook`
//! would hijack the default action the moment the first statement polled. A second `SIGINT` that
//! arrives before Perl has consumed the first flips an escape hatch and `libc::_exit(130)` kills
//! the process outright — this is the guarantee the user needs when a parallel primitive (e.g.
//! `pfor { sleep N }`) leaves the main thread stuck inside a rayon call that cannot poll.

use crate::error::PerlResult;
use crate::interpreter::Interpreter;

/// Ask the signal runtime to install a hook for `name` (Perl signal name without `SIG` prefix).
/// Idempotent — repeated calls are no-ops. Called from `%SIG` assignment paths in [`crate::scope`].
pub fn install(name: &str) {
    #[cfg(unix)]
    {
        unix::install(name);
    }
    #[cfg(not(unix))]
    {
        let _ = name;
    }
}

/// Check whether a pending signal has been observed for `name` (Perl signal name). Used by the
/// blocking builtins (`sleep`, `pwatch`) that cannot rely on the per-statement poll.
pub fn pending(name: &str) -> bool {
    #[cfg(unix)]
    {
        unix::pending(name)
    }
    #[cfg(not(unix))]
    {
        let _ = name;
        false
    }
}

/// Call between statements to run pending `%SIG` hooks.
pub fn poll(interp: &mut Interpreter) -> PerlResult<()> {
    #[cfg(unix)]
    {
        unix::poll(interp)
    }
    #[cfg(not(unix))]
    {
        let _ = interp;
        Ok(())
    }
}

#[cfg(unix)]
mod unix {
    use std::sync::atomic::{AtomicBool, Ordering};

    use signal_hook::consts::{SIGALRM, SIGCHLD, SIGINT, SIGTERM};

    use super::*;

    static SIGINT_P: AtomicBool = AtomicBool::new(false);
    static SIGTERM_P: AtomicBool = AtomicBool::new(false);
    static SIGALRM_P: AtomicBool = AtomicBool::new(false);
    static SIGCHLD_P: AtomicBool = AtomicBool::new(false);

    static SIGINT_INSTALLED: AtomicBool = AtomicBool::new(false);
    static SIGTERM_INSTALLED: AtomicBool = AtomicBool::new(false);
    static SIGALRM_INSTALLED: AtomicBool = AtomicBool::new(false);
    static SIGCHLD_INSTALLED: AtomicBool = AtomicBool::new(false);

    pub(super) fn install(name: &str) {
        match name {
            "INT" => install_sigint(),
            "TERM" => install_sigterm(),
            "ALRM" => install_sigalrm(),
            "CHLD" => install_sigchld(),
            _ => {}
        }
    }

    pub(super) fn pending(name: &str) -> bool {
        match name {
            "INT" => SIGINT_P.load(Ordering::SeqCst),
            "TERM" => SIGTERM_P.load(Ordering::SeqCst),
            "ALRM" => SIGALRM_P.load(Ordering::SeqCst),
            "CHLD" => SIGCHLD_P.load(Ordering::SeqCst),
            _ => false,
        }
    }

    fn install_sigint() {
        if SIGINT_INSTALLED.swap(true, Ordering::SeqCst) {
            return;
        }
        unsafe {
            let _ = signal_hook::low_level::register(SIGINT, || {
                // Second Ctrl-C before the first was consumed — main thread is likely stuck
                // inside a blocking parallel call (rayon, channel recv, sleep). Escape hatch:
                // kill the process right from the signal handler. `libc::_exit` is
                // async-signal-safe; `std::process::exit` is not.
                if SIGINT_P.swap(true, Ordering::SeqCst) {
                    libc::_exit(130);
                }
            });
        }
    }

    fn install_sigterm() {
        if SIGTERM_INSTALLED.swap(true, Ordering::SeqCst) {
            return;
        }
        unsafe {
            let _ = signal_hook::low_level::register(SIGTERM, || {
                if SIGTERM_P.swap(true, Ordering::SeqCst) {
                    libc::_exit(143);
                }
            });
        }
    }

    fn install_sigalrm() {
        if SIGALRM_INSTALLED.swap(true, Ordering::SeqCst) {
            return;
        }
        unsafe {
            let _ = signal_hook::low_level::register(SIGALRM, || {
                SIGALRM_P.store(true, Ordering::SeqCst);
            });
        }
    }

    fn install_sigchld() {
        if SIGCHLD_INSTALLED.swap(true, Ordering::SeqCst) {
            return;
        }
        unsafe {
            let _ = signal_hook::low_level::register(SIGCHLD, || {
                SIGCHLD_P.store(true, Ordering::SeqCst);
            });
        }
    }

    pub(super) fn poll(interp: &mut Interpreter) -> PerlResult<()> {
        if SIGINT_INSTALLED.load(Ordering::Relaxed) && SIGINT_P.swap(false, Ordering::SeqCst) {
            interp.sigint_pending_caret.set(true);
            interp.invoke_sig_handler("INT")?;
        }
        if SIGTERM_INSTALLED.load(Ordering::Relaxed) && SIGTERM_P.swap(false, Ordering::SeqCst) {
            interp.invoke_sig_handler("TERM")?;
        }
        if SIGALRM_INSTALLED.load(Ordering::Relaxed) && SIGALRM_P.swap(false, Ordering::SeqCst) {
            interp.invoke_sig_handler("ALRM")?;
        }
        if SIGCHLD_INSTALLED.load(Ordering::Relaxed) && SIGCHLD_P.swap(false, Ordering::SeqCst) {
            interp.invoke_sig_handler("CHLD")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_returns_ok_with_fresh_interpreter() {
        let mut interp = Interpreter::new();
        assert!(poll(&mut interp).is_ok());
    }

    #[test]
    fn install_is_idempotent() {
        install("INT");
        install("INT");
        install("TERM");
        install("ALRM");
        install("CHLD");
        install("UNKNOWN_SIG_NAME");
        // Still polls cleanly after installs.
        let mut interp = Interpreter::new();
        assert!(poll(&mut interp).is_ok());
    }
}
