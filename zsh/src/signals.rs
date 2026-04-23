//! Signal handling for zshrs
//!
//! Direct port from zsh/Src/signals.c
//!
//! Manages signal handling including:
//! - Signal handlers for SIGINT, SIGCHLD, SIGHUP, etc.
//! - Signal queueing during critical sections
//! - Trap management (trap builtin)
//! - Job control signals

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use nix::sys::signal::{sigprocmask, SigmaskHow};
use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal as NixSignal};
use nix::unistd::getpid;

/// Maximum size of signal queue
const MAX_QUEUE_SIZE: usize = 128;

/// Signal trap flags
pub mod trap_flags {
    pub const ZSIG_TRAPPED: u32 = 1;    // Signal is trapped
    pub const ZSIG_IGNORED: u32 = 2;    // Signal is being ignored
    pub const ZSIG_FUNC: u32 = 4;       // Trap is a function (TRAPXXX)
    pub const ZSIG_SHIFT: u32 = 3;      // Bits to shift for local level
}

/// Well-known signal numbers (matching libc on most Unix systems)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Signal {
    SIGHUP = libc::SIGHUP,
    SIGINT = libc::SIGINT,
    SIGQUIT = libc::SIGQUIT,
    SIGILL = libc::SIGILL,
    SIGTRAP = libc::SIGTRAP,
    SIGABRT = libc::SIGABRT,
    SIGBUS = libc::SIGBUS,
    SIGFPE = libc::SIGFPE,
    SIGKILL = libc::SIGKILL,
    SIGUSR1 = libc::SIGUSR1,
    SIGSEGV = libc::SIGSEGV,
    SIGUSR2 = libc::SIGUSR2,
    SIGPIPE = libc::SIGPIPE,
    SIGALRM = libc::SIGALRM,
    SIGTERM = libc::SIGTERM,
    SIGCHLD = libc::SIGCHLD,
    SIGCONT = libc::SIGCONT,
    SIGSTOP = libc::SIGSTOP,
    SIGTSTP = libc::SIGTSTP,
    SIGTTIN = libc::SIGTTIN,
    SIGTTOU = libc::SIGTTOU,
    SIGURG = libc::SIGURG,
    SIGXCPU = libc::SIGXCPU,
    SIGXFSZ = libc::SIGXFSZ,
    SIGVTALRM = libc::SIGVTALRM,
    SIGPROF = libc::SIGPROF,
    SIGWINCH = libc::SIGWINCH,
    SIGIO = libc::SIGIO,
    SIGSYS = libc::SIGSYS,
}

/// Pseudo-signals for shell traps
pub const SIGEXIT: i32 = 0;
pub const SIGDEBUG: i32 = -1;
pub const SIGZERR: i32 = -2;

/// Signal names array
pub static SIGNAL_NAMES: &[(&str, i32)] = &[
    ("EXIT", SIGEXIT),
    ("HUP", libc::SIGHUP),
    ("INT", libc::SIGINT),
    ("QUIT", libc::SIGQUIT),
    ("ILL", libc::SIGILL),
    ("TRAP", libc::SIGTRAP),
    ("ABRT", libc::SIGABRT),
    ("BUS", libc::SIGBUS),
    ("FPE", libc::SIGFPE),
    ("KILL", libc::SIGKILL),
    ("USR1", libc::SIGUSR1),
    ("SEGV", libc::SIGSEGV),
    ("USR2", libc::SIGUSR2),
    ("PIPE", libc::SIGPIPE),
    ("ALRM", libc::SIGALRM),
    ("TERM", libc::SIGTERM),
    ("CHLD", libc::SIGCHLD),
    ("CONT", libc::SIGCONT),
    ("STOP", libc::SIGSTOP),
    ("TSTP", libc::SIGTSTP),
    ("TTIN", libc::SIGTTIN),
    ("TTOU", libc::SIGTTOU),
    ("URG", libc::SIGURG),
    ("XCPU", libc::SIGXCPU),
    ("XFSZ", libc::SIGXFSZ),
    ("VTALRM", libc::SIGVTALRM),
    ("PROF", libc::SIGPROF),
    ("WINCH", libc::SIGWINCH),
    ("IO", libc::SIGIO),
    ("SYS", libc::SIGSYS),
    ("DEBUG", SIGDEBUG),
    ("ZERR", SIGZERR),
    ("ERR", SIGZERR),  // Alias
];

/// Get signal number from name
pub fn sig_by_name(name: &str) -> Option<i32> {
    let name_upper = name.to_uppercase();
    let lookup = if name_upper.starts_with("SIG") {
        &name_upper[3..]
    } else {
        &name_upper
    };
    
    for (sig_name, sig_num) in SIGNAL_NAMES {
        if *sig_name == lookup {
            return Some(*sig_num);
        }
    }
    
    // Try parsing as number
    lookup.parse().ok()
}

/// Get signal name from number
pub fn sig_name(sig: i32) -> Option<&'static str> {
    for (name, num) in SIGNAL_NAMES {
        if *num == sig {
            return Some(name);
        }
    }
    None
}

/// Signal state for queueing
struct SignalQueue {
    enabled: AtomicBool,
    front: AtomicUsize,
    rear: AtomicUsize,
    signals: [AtomicI32; MAX_QUEUE_SIZE],
}

impl SignalQueue {
    const fn new() -> Self {
        const INIT: AtomicI32 = AtomicI32::new(0);
        SignalQueue {
            enabled: AtomicBool::new(false),
            front: AtomicUsize::new(0),
            rear: AtomicUsize::new(0),
            signals: [INIT; MAX_QUEUE_SIZE],
        }
    }

    fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    fn enable(&self) {
        self.enabled.store(true, Ordering::SeqCst);
    }

    fn disable(&self) {
        self.enabled.store(false, Ordering::SeqCst);
    }

    fn push(&self, sig: i32) -> bool {
        let rear = self.rear.load(Ordering::SeqCst);
        let new_rear = (rear + 1) % MAX_QUEUE_SIZE;
        let front = self.front.load(Ordering::SeqCst);
        
        if new_rear == front {
            return false; // Queue full
        }
        
        self.signals[new_rear].store(sig, Ordering::SeqCst);
        self.rear.store(new_rear, Ordering::SeqCst);
        true
    }

    fn pop(&self) -> Option<i32> {
        let front = self.front.load(Ordering::SeqCst);
        let rear = self.rear.load(Ordering::SeqCst);
        
        if front == rear {
            return None; // Queue empty
        }
        
        let new_front = (front + 1) % MAX_QUEUE_SIZE;
        let sig = self.signals[new_front].load(Ordering::SeqCst);
        self.front.store(new_front, Ordering::SeqCst);
        Some(sig)
    }
}

static SIGNAL_QUEUE: SignalQueue = SignalQueue::new();
static TRAP_QUEUE: SignalQueue = SignalQueue::new();

/// Last signal received
static LAST_SIGNAL: AtomicI32 = AtomicI32::new(0);

/// Trap handler storage
pub struct TrapHandler {
    /// Trap code/function for each signal
    traps: Mutex<HashMap<i32, TrapAction>>,
    /// Flags for each trapped signal
    flags: Mutex<HashMap<i32, u32>>,
    /// Number of trapped signals
    pub num_trapped: AtomicUsize,
    /// Currently in a trap?
    pub in_trap: AtomicBool,
    /// Running exit trap?
    pub in_exit_trap: AtomicBool,
}

/// What action to take for a trap
#[derive(Debug, Clone)]
pub enum TrapAction {
    /// Ignore the signal
    Ignore,
    /// Execute this code string
    Code(String),
    /// Call function TRAPXXX
    Function(String),
    /// Default action
    Default,
}

impl Default for TrapHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl TrapHandler {
    pub fn new() -> Self {
        TrapHandler {
            traps: Mutex::new(HashMap::new()),
            flags: Mutex::new(HashMap::new()),
            num_trapped: AtomicUsize::new(0),
            in_trap: AtomicBool::new(false),
            in_exit_trap: AtomicBool::new(false),
        }
    }

    /// Set a trap for a signal
    pub fn set_trap(&self, sig: i32, action: TrapAction) -> Result<(), String> {
        // Can't trap SIGKILL or SIGSTOP
        if sig == libc::SIGKILL || sig == libc::SIGSTOP {
            return Err(format!("can't trap SIG{}", sig_name(sig).unwrap_or("?")));
        }

        let mut traps = self.traps.lock().unwrap();
        let mut flags = self.flags.lock().unwrap();

        let was_trapped = flags.get(&sig).map(|f| f & trap_flags::ZSIG_TRAPPED != 0).unwrap_or(false);

        match &action {
            TrapAction::Ignore => {
                traps.insert(sig, action);
                flags.insert(sig, trap_flags::ZSIG_IGNORED);
                if sig > 0 {
                    self.ignore_signal(sig);
                }
            }
            TrapAction::Code(code) if code.is_empty() => {
                traps.insert(sig, TrapAction::Ignore);
                flags.insert(sig, trap_flags::ZSIG_IGNORED);
                if sig > 0 {
                    self.ignore_signal(sig);
                }
            }
            TrapAction::Code(_) => {
                if !was_trapped {
                    self.num_trapped.fetch_add(1, Ordering::SeqCst);
                }
                traps.insert(sig, action);
                flags.insert(sig, trap_flags::ZSIG_TRAPPED);
                if sig > 0 {
                    self.install_handler(sig);
                }
            }
            TrapAction::Function(name) => {
                if !was_trapped {
                    self.num_trapped.fetch_add(1, Ordering::SeqCst);
                }
                traps.insert(sig, TrapAction::Function(name.clone()));
                flags.insert(sig, trap_flags::ZSIG_TRAPPED | trap_flags::ZSIG_FUNC);
                if sig > 0 {
                    self.install_handler(sig);
                }
            }
            TrapAction::Default => {
                if was_trapped {
                    self.num_trapped.fetch_sub(1, Ordering::SeqCst);
                }
                traps.remove(&sig);
                flags.remove(&sig);
                if sig > 0 {
                    self.default_signal(sig);
                }
            }
        }

        Ok(())
    }

    /// Remove a trap
    pub fn unset_trap(&self, sig: i32) {
        let _ = self.set_trap(sig, TrapAction::Default);
    }

    /// Get the trap action for a signal
    pub fn get_trap(&self, sig: i32) -> Option<TrapAction> {
        self.traps.lock().unwrap().get(&sig).cloned()
    }

    /// Check if a signal is trapped
    pub fn is_trapped(&self, sig: i32) -> bool {
        self.flags.lock().unwrap()
            .get(&sig)
            .map(|f| f & trap_flags::ZSIG_TRAPPED != 0)
            .unwrap_or(false)
    }

    /// Check if a signal is ignored
    pub fn is_ignored(&self, sig: i32) -> bool {
        self.flags.lock().unwrap()
            .get(&sig)
            .map(|f| f & trap_flags::ZSIG_IGNORED != 0)
            .unwrap_or(false)
    }

    /// Install signal handler
    fn install_handler(&self, sig: i32) {
        unsafe {
            libc::signal(sig, handler as *const () as usize);
        }
    }

    /// Ignore a signal
    fn ignore_signal(&self, sig: i32) {
        unsafe {
            libc::signal(sig, libc::SIG_IGN);
        }
    }

    /// Reset to default handler
    fn default_signal(&self, sig: i32) {
        unsafe {
            libc::signal(sig, libc::SIG_DFL);
        }
    }

    /// List all traps
    pub fn list_traps(&self) -> Vec<(i32, TrapAction)> {
        self.traps.lock().unwrap()
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }
}

/// Global trap handler
static TRAPS: OnceLock<TrapHandler> = OnceLock::new();

/// Get the global trap handler
pub fn traps() -> &'static TrapHandler {
    TRAPS.get_or_init(TrapHandler::new)
}

/// Store the main shell PID to detect forked children.
static MAIN_PID: AtomicI32 = AtomicI32::new(0);

/// Whether we received SIGCHLD.
static SIGCHLD_RECEIVED: AtomicBool = AtomicBool::new(false);

/// Whether we received SIGWINCH.
static SIGWINCH_RECEIVED: AtomicBool = AtomicBool::new(false);

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

/// Signal handler function
extern "C" fn handler(sig: i32) {
    // Preserve errno
    #[cfg(target_os = "macos")]
    let saved_errno = unsafe { *libc::__error() };
    #[cfg(not(target_os = "macos"))]
    let saved_errno = unsafe { *libc::__errno_location() };

    // Check if we're a forked child
    if reraise_if_forked_child(sig) {
        #[cfg(target_os = "macos")]
        unsafe { *libc::__error() = saved_errno };
        #[cfg(not(target_os = "macos"))]
        unsafe { *libc::__errno_location() = saved_errno };
        return;
    }

    LAST_SIGNAL.store(sig, Ordering::SeqCst);

    // Track specific signals
    if sig == libc::SIGCHLD {
        SIGCHLD_RECEIVED.store(true, Ordering::SeqCst);
    } else if sig == libc::SIGWINCH {
        SIGWINCH_RECEIVED.store(true, Ordering::SeqCst);
    }

    // If queueing is enabled, queue the signal
    if SIGNAL_QUEUE.is_enabled() {
        SIGNAL_QUEUE.push(sig);
        #[cfg(target_os = "macos")]
        unsafe { *libc::__error() = saved_errno };
        #[cfg(not(target_os = "macos"))]
        unsafe { *libc::__errno_location() = saved_errno };
        return;
    }

    // Handle the signal directly
    handle_signal(sig);

    #[cfg(target_os = "macos")]
    unsafe { *libc::__error() = saved_errno };
    #[cfg(not(target_os = "macos"))]
    unsafe { *libc::__errno_location() = saved_errno };
}

/// Handle a signal
fn handle_signal(sig: i32) {
    match sig {
        s if s == libc::SIGCHLD => {
            // Child process status change - handled by job control
        }
        s if s == libc::SIGINT => {
            // Interrupt - set error flag
            if let Some(action) = traps().get_trap(s) {
                run_trap(s, &action);
            }
        }
        s if s == libc::SIGHUP => {
            // Hangup
            if let Some(action) = traps().get_trap(s) {
                run_trap(s, &action);
            }
        }
        s if s == libc::SIGWINCH => {
            // Window size change
            if let Some(action) = traps().get_trap(s) {
                run_trap(s, &action);
            }
        }
        s if s == libc::SIGALRM => {
            // Alarm
            if let Some(action) = traps().get_trap(s) {
                run_trap(s, &action);
            }
        }
        s if s == libc::SIGPIPE => {
            // Broken pipe
            if let Some(action) = traps().get_trap(s) {
                run_trap(s, &action);
            }
        }
        _ => {
            // Other signals
            if let Some(action) = traps().get_trap(sig) {
                run_trap(sig, &action);
            }
        }
    }
}

/// Run a trap action
fn run_trap(sig: i32, action: &TrapAction) {
    match action {
        TrapAction::Ignore => {}
        TrapAction::Code(_code) => {
            // Would execute the code - needs executor integration
            traps().in_trap.store(true, Ordering::SeqCst);
            if sig == SIGEXIT {
                traps().in_exit_trap.store(true, Ordering::SeqCst);
            }
            // Execute code here...
            if sig == SIGEXIT {
                traps().in_exit_trap.store(false, Ordering::SeqCst);
            }
            traps().in_trap.store(false, Ordering::SeqCst);
        }
        TrapAction::Function(_name) => {
            // Would call the function - needs executor integration
            traps().in_trap.store(true, Ordering::SeqCst);
            // Call function here...
            traps().in_trap.store(false, Ordering::SeqCst);
        }
        TrapAction::Default => {}
    }
}

/// Enable signal queueing
pub fn queue_signals() {
    SIGNAL_QUEUE.enable();
}

/// Disable signal queueing and process queued signals
pub fn unqueue_signals() {
    SIGNAL_QUEUE.disable();
    while let Some(sig) = SIGNAL_QUEUE.pop() {
        handle_signal(sig);
    }
}

/// Check if signal queueing is enabled
pub fn queueing_enabled() -> bool {
    SIGNAL_QUEUE.is_enabled()
}

/// Enable trap queueing
pub fn queue_traps() {
    TRAP_QUEUE.enable();
}

/// Disable trap queueing and run queued traps
pub fn unqueue_traps() {
    TRAP_QUEUE.disable();
    while let Some(sig) = TRAP_QUEUE.pop() {
        if let Some(action) = traps().get_trap(sig) {
            run_trap(sig, &action);
        }
    }
}

/// Block a signal
pub fn signal_block(sig: i32) {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, sig);
        libc::sigprocmask(libc::SIG_BLOCK, &set, std::ptr::null_mut());
    }
}

/// Unblock a signal
pub fn signal_unblock(sig: i32) {
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigaddset(&mut set, sig);
        libc::sigprocmask(libc::SIG_UNBLOCK, &set, std::ptr::null_mut());
    }
}

/// Block SIGINT for interactive shells
pub fn hold_intr() {
    signal_block(libc::SIGINT);
}

/// Unblock SIGINT
pub fn release_intr() {
    signal_unblock(libc::SIGINT);
}

/// Install default interrupt handler for interactive shells
pub fn setup_intr() {
    unsafe {
        libc::signal(libc::SIGINT, handler as *const () as usize);
    }
}

/// Get last received signal
pub fn last_signal() -> i32 {
    LAST_SIGNAL.load(Ordering::SeqCst)
}

/// Kill a process group
pub fn killpg(pgrp: i32, sig: i32) -> i32 {
    unsafe { libc::killpg(pgrp, sig) }
}

/// Kill a process
pub fn kill(pid: i32, sig: i32) -> i32 {
    unsafe { libc::kill(pid, sig) }
}

/// Check and clear SIGCHLD flag.
pub fn signal_check_sigchld() -> bool {
    SIGCHLD_RECEIVED.swap(false, Ordering::SeqCst)
}

/// Check and clear SIGWINCH flag.
pub fn signal_check_sigwinch() -> bool {
    SIGWINCH_RECEIVED.swap(false, Ordering::SeqCst)
}

/// Clear the cancellation signal.
pub fn signal_clear_cancel() {
    LAST_SIGNAL.store(0, Ordering::SeqCst);
}

/// Check if a cancellation signal (SIGINT) was received.
pub fn signal_check_cancel() -> i32 {
    let sig = LAST_SIGNAL.load(Ordering::SeqCst);
    if sig == libc::SIGINT {
        sig
    } else {
        0
    }
}

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
    let sa_handler = SigAction::new(
        SigHandler::Handler(handler),
        SaFlags::SA_RESTART,
        SigSet::empty(),
    );

    unsafe {
        let _ = nix::sys::signal::sigaction(NixSignal::SIGINT, &sa_handler);
        let _ = nix::sys::signal::sigaction(NixSignal::SIGCHLD, &sa_handler);
    }

    if interactive {
        // Ignore job control signals in interactive mode
        unsafe {
            let _ = nix::sys::signal::sigaction(NixSignal::SIGTSTP, &ignore);
            let _ = nix::sys::signal::sigaction(NixSignal::SIGTTOU, &ignore);
        }

        // Handle SIGWINCH for terminal resize
        unsafe {
            let _ = nix::sys::signal::sigaction(NixSignal::SIGWINCH, &sa_handler);
        }

        // Handle SIGHUP and SIGTERM
        unsafe {
            let _ = nix::sys::signal::sigaction(NixSignal::SIGHUP, &sa_handler);
            let _ = nix::sys::signal::sigaction(NixSignal::SIGTERM, &sa_handler);
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

/// Get signal description from number.
pub fn signal_desc(sig: i32) -> &'static str {
    match sig {
        s if s == libc::SIGHUP => "Hangup",
        s if s == libc::SIGINT => "Interrupt",
        s if s == libc::SIGQUIT => "Quit",
        s if s == libc::SIGILL => "Illegal instruction",
        s if s == libc::SIGTRAP => "Trace trap",
        s if s == libc::SIGABRT => "Abort",
        s if s == libc::SIGBUS => "Bus error",
        s if s == libc::SIGFPE => "Floating point exception",
        s if s == libc::SIGKILL => "Killed",
        s if s == libc::SIGUSR1 => "User signal 1",
        s if s == libc::SIGSEGV => "Segmentation fault",
        s if s == libc::SIGUSR2 => "User signal 2",
        s if s == libc::SIGPIPE => "Broken pipe",
        s if s == libc::SIGALRM => "Alarm clock",
        s if s == libc::SIGTERM => "Terminated",
        s if s == libc::SIGCHLD => "Child status changed",
        s if s == libc::SIGCONT => "Continued",
        s if s == libc::SIGSTOP => "Stopped (signal)",
        s if s == libc::SIGTSTP => "Stopped",
        s if s == libc::SIGTTIN => "Stopped (tty input)",
        s if s == libc::SIGTTOU => "Stopped (tty output)",
        s if s == libc::SIGURG => "Urgent I/O condition",
        s if s == libc::SIGXCPU => "CPU time limit exceeded",
        s if s == libc::SIGXFSZ => "File size limit exceeded",
        s if s == libc::SIGVTALRM => "Virtual timer expired",
        s if s == libc::SIGPROF => "Profiling timer expired",
        s if s == libc::SIGWINCH => "Window size changed",
        s if s == libc::SIGIO => "I/O possible",
        s if s == libc::SIGSYS => "Bad system call",
        _ => "Unknown signal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sig_by_name() {
        assert_eq!(sig_by_name("INT"), Some(libc::SIGINT));
        assert_eq!(sig_by_name("SIGINT"), Some(libc::SIGINT));
        assert_eq!(sig_by_name("int"), Some(libc::SIGINT));
        assert_eq!(sig_by_name("HUP"), Some(libc::SIGHUP));
        assert_eq!(sig_by_name("TERM"), Some(libc::SIGTERM));
        assert_eq!(sig_by_name("EXIT"), Some(SIGEXIT));
        assert_eq!(sig_by_name("9"), Some(9));
    }

    #[test]
    fn test_sig_name() {
        assert_eq!(sig_name(libc::SIGINT), Some("INT"));
        assert_eq!(sig_name(libc::SIGHUP), Some("HUP"));
        assert_eq!(sig_name(SIGEXIT), Some("EXIT"));
    }

    #[test]
    fn test_trap_handler() {
        let handler = TrapHandler::new();
        
        // Initially not trapped
        assert!(!handler.is_trapped(libc::SIGUSR1));
        
        // Set a trap
        handler.set_trap(libc::SIGUSR1, TrapAction::Code("echo trapped".to_string())).unwrap();
        assert!(handler.is_trapped(libc::SIGUSR1));
        
        // Unset trap
        handler.unset_trap(libc::SIGUSR1);
        assert!(!handler.is_trapped(libc::SIGUSR1));
    }

    #[test]
    fn test_ignore_trap() {
        let handler = TrapHandler::new();
        
        handler.set_trap(libc::SIGUSR1, TrapAction::Ignore).unwrap();
        assert!(handler.is_ignored(libc::SIGUSR1));
        assert!(!handler.is_trapped(libc::SIGUSR1));
    }

    #[test]
    fn test_signal_queue() {
        // Enable queueing
        queue_signals();
        assert!(queueing_enabled());
        
        // Disable queueing
        unqueue_signals();
        assert!(!queueing_enabled());
    }

    #[test]
    fn test_cant_trap_sigkill() {
        let handler = TrapHandler::new();
        let result = handler.set_trap(libc::SIGKILL, TrapAction::Code("echo".to_string()));
        assert!(result.is_err());
    }
}
