//! Minimal `%SIG` delivery for common signals (Unix). Handlers run between statements.

use crate::error::PerlResult;
use crate::interpreter::Interpreter;

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
    use std::sync::Once;

    use signal_hook::consts::{SIGALRM, SIGCHLD, SIGINT, SIGTERM};

    use super::*;

    static INIT: Once = Once::new();
    static SIGINT_P: AtomicBool = AtomicBool::new(false);
    static SIGTERM_P: AtomicBool = AtomicBool::new(false);
    static SIGALRM_P: AtomicBool = AtomicBool::new(false);
    static SIGCHLD_P: AtomicBool = AtomicBool::new(false);

    pub fn poll(interp: &mut Interpreter) -> PerlResult<()> {
        INIT.call_once(|| unsafe {
            let _ = signal_hook::low_level::register(SIGINT, || {
                SIGINT_P.store(true, Ordering::SeqCst);
            });
            let _ = signal_hook::low_level::register(SIGTERM, || {
                SIGTERM_P.store(true, Ordering::SeqCst);
            });
            let _ = signal_hook::low_level::register(SIGALRM, || {
                SIGALRM_P.store(true, Ordering::SeqCst);
            });
            let _ = signal_hook::low_level::register(SIGCHLD, || {
                SIGCHLD_P.store(true, Ordering::SeqCst);
            });
        });
        if SIGINT_P.swap(false, Ordering::SeqCst) {
            interp.sigint_pending_caret.set(true);
            interp.invoke_sig_handler("INT")?;
        }
        if SIGTERM_P.swap(false, Ordering::SeqCst) {
            interp.invoke_sig_handler("TERM")?;
        }
        if SIGALRM_P.swap(false, Ordering::SeqCst) {
            interp.invoke_sig_handler("ALRM")?;
        }
        if SIGCHLD_P.swap(false, Ordering::SeqCst) {
            interp.invoke_sig_handler("CHLD")?;
        }
        Ok(())
    }
}
