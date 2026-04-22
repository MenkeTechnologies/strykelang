//! Signal handling for zshrs
//!
//! Based on patterns from fish-shell's signal.rs, adapted for zsh semantics.

use nix::sys::signal::{sigprocmask, SigmaskHow};
use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal as NixSignal};
use nix::unistd::getpid;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Mutex;

// ============================================================================
// Global State
// ============================================================================

/// Store the main shell PID to detect forked children.
static MAIN_PID: AtomicI32 = AtomicI32::new(0);

/// The cancellation signal (SIGINT).
static CANCELLATION_SIGNAL: AtomicI32 = AtomicI32::new(0);

/// Whether we received SIGCHLD.
static SIGCHLD_RECEIVED: AtomicBool = AtomicBool::new(false);

/// Whether we received SIGWINCH.
static SIGWINCH_RECEIVED: AtomicBool = AtomicBool::new(false);

/// Pending signals queue for trap handlers.
static PENDING_SIGNALS: Mutex<Vec<i32>> = Mutex::new(Vec::new());

// ============================================================================
// Signal Checking
// ============================================================================

/// Clear the cancellation signal.
pub fn signal_clear_cancel() {
    CANCELLATION_SIGNAL.store(0, Ordering::SeqCst);
}

/// Check if a cancellation signal (SIGINT) was received.
pub fn signal_check_cancel() -> i32 {
    CANCELLATION_SIGNAL.load(Ordering::SeqCst)
}

/// Check and clear SIGCHLD flag.
pub fn signal_check_sigchld() -> bool {
    SIGCHLD_RECEIVED.swap(false, Ordering::SeqCst)
}

/// Check and clear SIGWINCH flag.
pub fn signal_check_sigwinch() -> bool {
    SIGWINCH_RECEIVED.swap(false, Ordering::SeqCst)
}

/// Get and clear pending signals for trap processing.
pub fn signal_get_pending() -> Vec<i32> {
    let mut pending = PENDING_SIGNALS.lock().expect("Poisoned mutex");
    std::mem::take(&mut *pending)
}

// ============================================================================
// Signal Handler
// ============================================================================

/// Check if we're in a forked child and re-raise signal if so.
fn reraise_if_forked_child(sig: i32) -> bool {
    if getpid().as_raw() == MAIN_PID.load(Ordering::Relaxed) {
        return false;
    }
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
    true
}

/// Central signal handler for zshrs.
extern "C" fn zshrs_signal_handler(sig: i32) {
    // Preserve errno
    let saved_errno = unsafe { *libc::__error() };

    // Check if we're a forked child
    if reraise_if_forked_child(sig) {
        unsafe { *libc::__error() = saved_errno };
        return;
    }

    match sig {
        libc::SIGINT => {
            CANCELLATION_SIGNAL.store(libc::SIGINT, Ordering::SeqCst);
        }
        libc::SIGCHLD => {
            SIGCHLD_RECEIVED.store(true, Ordering::SeqCst);
        }
        libc::SIGWINCH => {
            SIGWINCH_RECEIVED.store(true, Ordering::SeqCst);
        }
        _ => {}
    }

    // Queue signal for trap handlers
    if let Ok(mut pending) = PENDING_SIGNALS.try_lock() {
        if !pending.contains(&sig) {
            pending.push(sig);
        }
    }

    unsafe { *libc::__error() = saved_errno };
}

// ============================================================================
// Signal Setup
// ============================================================================

/// Set up signal handlers for the shell.
pub fn signal_set_handlers(interactive: bool) {
    MAIN_PID.store(getpid().as_raw(), Ordering::Relaxed);

    // Ignore SIGPIPE - we handle broken pipes ourselves
    let ignore = SigAction::new(SigHandler::SigIgn, SaFlags::empty(), SigSet::empty());
    unsafe {
        let _ = nix::sys::signal::sigaction(NixSignal::SIGPIPE, &ignore);
        let _ = nix::sys::signal::sigaction(NixSignal::SIGQUIT, &ignore);
    }

    // Set up our handler for key signals
    let handler = SigAction::new(
        SigHandler::Handler(zshrs_signal_handler),
        SaFlags::SA_RESTART,
        SigSet::empty(),
    );

    unsafe {
        let _ = nix::sys::signal::sigaction(NixSignal::SIGINT, &handler);
        let _ = nix::sys::signal::sigaction(NixSignal::SIGCHLD, &handler);
    }

    if interactive {
        // Ignore job control signals in interactive mode
        unsafe {
            let _ = nix::sys::signal::sigaction(NixSignal::SIGTSTP, &ignore);
            let _ = nix::sys::signal::sigaction(NixSignal::SIGTTOU, &ignore);
        }

        // Handle SIGWINCH for terminal resize
        unsafe {
            let _ = nix::sys::signal::sigaction(NixSignal::SIGWINCH, &handler);
        }

        // Handle SIGHUP and SIGTERM
        unsafe {
            let _ = nix::sys::signal::sigaction(NixSignal::SIGHUP, &handler);
            let _ = nix::sys::signal::sigaction(NixSignal::SIGTERM, &handler);
        }
    }
}

/// Reset all signal handlers to default (called after fork).
pub fn signal_reset_handlers() {
    let default = SigAction::new(SigHandler::SigDfl, SaFlags::empty(), SigSet::empty());

    let signals = [
        NixSignal::SIGHUP,
        NixSignal::SIGINT,
        NixSignal::SIGQUIT,
        NixSignal::SIGTERM,
        NixSignal::SIGCHLD,
        NixSignal::SIGTSTP,
        NixSignal::SIGTTIN,
        NixSignal::SIGTTOU,
        NixSignal::SIGPIPE,
    ];

    for sig in signals {
        unsafe {
            let _ = nix::sys::signal::sigaction(sig, &default);
        }
    }
}

/// Unblock all signals.
pub fn signal_unblock_all() {
    let _ = sigprocmask(SigmaskHow::SIG_SETMASK, Some(&SigSet::empty()), None);
}

/// Block SIGCHLD temporarily.
pub fn signal_block_sigchld() -> SigSet {
    let mut mask = SigSet::empty();
    mask.add(NixSignal::SIGCHLD);
    let mut old = SigSet::empty();
    let _ = sigprocmask(SigmaskHow::SIG_BLOCK, Some(&mask), Some(&mut old));
    old
}

/// Restore previous signal mask.
pub fn signal_restore_mask(mask: &SigSet) {
    let _ = sigprocmask(SigmaskHow::SIG_SETMASK, Some(mask), None);
}

// ============================================================================
// Signal Info
// ============================================================================

/// Signal name/description table entry.
struct SignalInfo {
    name: &'static str,
    desc: &'static str,
}

/// Table of signal names and descriptions.
const SIGNAL_TABLE: &[(i32, SignalInfo)] = &[
    (
        libc::SIGHUP,
        SignalInfo {
            name: "HUP",
            desc: "Hangup",
        },
    ),
    (
        libc::SIGINT,
        SignalInfo {
            name: "INT",
            desc: "Interrupt",
        },
    ),
    (
        libc::SIGQUIT,
        SignalInfo {
            name: "QUIT",
            desc: "Quit",
        },
    ),
    (
        libc::SIGILL,
        SignalInfo {
            name: "ILL",
            desc: "Illegal instruction",
        },
    ),
    (
        libc::SIGTRAP,
        SignalInfo {
            name: "TRAP",
            desc: "Trace trap",
        },
    ),
    (
        libc::SIGABRT,
        SignalInfo {
            name: "ABRT",
            desc: "Abort",
        },
    ),
    (
        libc::SIGBUS,
        SignalInfo {
            name: "BUS",
            desc: "Bus error",
        },
    ),
    (
        libc::SIGFPE,
        SignalInfo {
            name: "FPE",
            desc: "Floating point exception",
        },
    ),
    (
        libc::SIGKILL,
        SignalInfo {
            name: "KILL",
            desc: "Killed",
        },
    ),
    (
        libc::SIGUSR1,
        SignalInfo {
            name: "USR1",
            desc: "User signal 1",
        },
    ),
    (
        libc::SIGSEGV,
        SignalInfo {
            name: "SEGV",
            desc: "Segmentation fault",
        },
    ),
    (
        libc::SIGUSR2,
        SignalInfo {
            name: "USR2",
            desc: "User signal 2",
        },
    ),
    (
        libc::SIGPIPE,
        SignalInfo {
            name: "PIPE",
            desc: "Broken pipe",
        },
    ),
    (
        libc::SIGALRM,
        SignalInfo {
            name: "ALRM",
            desc: "Alarm clock",
        },
    ),
    (
        libc::SIGTERM,
        SignalInfo {
            name: "TERM",
            desc: "Terminated",
        },
    ),
    (
        libc::SIGCHLD,
        SignalInfo {
            name: "CHLD",
            desc: "Child status changed",
        },
    ),
    (
        libc::SIGCONT,
        SignalInfo {
            name: "CONT",
            desc: "Continued",
        },
    ),
    (
        libc::SIGSTOP,
        SignalInfo {
            name: "STOP",
            desc: "Stopped (signal)",
        },
    ),
    (
        libc::SIGTSTP,
        SignalInfo {
            name: "TSTP",
            desc: "Stopped",
        },
    ),
    (
        libc::SIGTTIN,
        SignalInfo {
            name: "TTIN",
            desc: "Stopped (tty input)",
        },
    ),
    (
        libc::SIGTTOU,
        SignalInfo {
            name: "TTOU",
            desc: "Stopped (tty output)",
        },
    ),
    (
        libc::SIGURG,
        SignalInfo {
            name: "URG",
            desc: "Urgent I/O condition",
        },
    ),
    (
        libc::SIGXCPU,
        SignalInfo {
            name: "XCPU",
            desc: "CPU time limit exceeded",
        },
    ),
    (
        libc::SIGXFSZ,
        SignalInfo {
            name: "XFSZ",
            desc: "File size limit exceeded",
        },
    ),
    (
        libc::SIGVTALRM,
        SignalInfo {
            name: "VTALRM",
            desc: "Virtual timer expired",
        },
    ),
    (
        libc::SIGPROF,
        SignalInfo {
            name: "PROF",
            desc: "Profiling timer expired",
        },
    ),
    (
        libc::SIGWINCH,
        SignalInfo {
            name: "WINCH",
            desc: "Window size changed",
        },
    ),
    (
        libc::SIGIO,
        SignalInfo {
            name: "IO",
            desc: "I/O possible",
        },
    ),
    (
        libc::SIGSYS,
        SignalInfo {
            name: "SYS",
            desc: "Bad system call",
        },
    ),
];

/// Get signal name from number.
pub fn signal_name(sig: i32) -> &'static str {
    SIGNAL_TABLE
        .iter()
        .find(|(s, _)| *s == sig)
        .map(|(_, info)| info.name)
        .unwrap_or("UNKNOWN")
}

/// Get signal description from number.
pub fn signal_desc(sig: i32) -> &'static str {
    SIGNAL_TABLE
        .iter()
        .find(|(s, _)| *s == sig)
        .map(|(_, info)| info.desc)
        .unwrap_or("Unknown signal")
}

/// Parse signal name to number. Accepts "HUP", "SIGHUP", "1", etc.
pub fn signal_parse(name: &str) -> Option<i32> {
    // Try parsing as number first
    if let Ok(num) = name.parse::<i32>() {
        if num > 0 && num < 32 {
            return Some(num);
        }
    }

    // Strip SIG prefix if present (case-insensitive)
    let name_upper = name.to_ascii_uppercase();
    let name = name_upper.strip_prefix("SIG").unwrap_or(&name_upper);

    SIGNAL_TABLE
        .iter()
        .find(|(_, info)| info.name.eq_ignore_ascii_case(name))
        .map(|(sig, _)| *sig)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_name() {
        assert_eq!(signal_name(libc::SIGINT), "INT");
        assert_eq!(signal_name(libc::SIGHUP), "HUP");
        assert_eq!(signal_name(libc::SIGKILL), "KILL");
    }

    #[test]
    fn test_signal_parse() {
        assert_eq!(signal_parse("HUP"), Some(libc::SIGHUP));
        assert_eq!(signal_parse("SIGHUP"), Some(libc::SIGHUP));
        assert_eq!(signal_parse("sighup"), Some(libc::SIGHUP));
        // Test numeric parsing with actual SIGHUP value
        assert_eq!(signal_parse(&libc::SIGHUP.to_string()), Some(libc::SIGHUP));
        assert_eq!(signal_parse("INT"), Some(libc::SIGINT));
        assert_eq!(signal_parse("SIGINT"), Some(libc::SIGINT));
        assert_eq!(signal_parse("INVALID"), None);
    }

    #[test]
    fn test_signal_desc() {
        assert_eq!(signal_desc(libc::SIGINT), "Interrupt");
        assert_eq!(signal_desc(libc::SIGKILL), "Killed");
    }
}
