//! Running stryke inside somebody else's process.
//!
//! The `stryke` binary owns its process: it can `exit()` from any of the
//! eighty-seven places its command line layer does, set the process SIGPIPE
//! disposition to suit a Unix filter, and let the kernel reclaim the rest. A
//! host that dispatches `stryke` as one command among many — the `stryke` shell
//! builtin in zshrs-native, where there is no fork and no exec — can afford
//! none of that. This module is the difference.
//!
//! [`exit`] keeps `process::exit` when stryke owns the process and unwinds when
//! it does not; [`run`] catches the unwind and turns it back into the status the
//! exit asked for. Unwinding costs nothing on the path that does not take it,
//! and destructors still run, so buffered output is flushed on the way out.
//!
//! arb and zvcs carry a module of the same shape for the same reason
//! (`arb/src/hosted.rs`, `zvcs/src/extensions/src/hosted.rs`). They are
//! deliberately separate copies: the flag is per-crate state, each crate's
//! payload type is its own, and none of the three should grow a dependency on
//! another just to be embeddable.

use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::Once;

thread_local! {
    /// True while this thread is running a hosted invocation.
    static HOSTED: Cell<bool> = const { Cell::new(false) };
}

/// The payload [`exit`] unwinds with, carrying the status the caller wanted.
struct HostedExit(i32);

/// Whether this thread is inside [`run`].
pub fn is_hosted() -> bool {
    HOSTED.with(Cell::get)
}

/// `exit(code)`, in a form a host process survives.
///
/// Outside a host this is `std::process::exit` exactly. Inside one it unwinds
/// to the [`run`] that started the invocation, which returns `code`.
pub fn exit(code: i32) -> ! {
    if is_hosted() {
        panic::panic_any(HostedExit(code));
    }
    std::process::exit(code)
}

/// Teach the panic hook to stay quiet about [`HostedExit`], and to leave every
/// other panic to whatever hook the host installed.
fn install_quiet_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if info.payload().is::<HostedExit>() {
                return;
            }
            previous(info);
        }));
    });
}

/// Run one hosted stryke invocation and return the status it left with.
///
/// A panic that is not a [`HostedExit`] reports through the host's hook and
/// yields 1 — stryke's own status for a failed run.
pub fn run<F>(f: F) -> i32
where
    F: FnOnce() -> i32,
{
    install_quiet_hook();

    let cwd: Option<PathBuf> = std::env::current_dir().ok();
    let previously = HOSTED.replace(true);

    let outcome = panic::catch_unwind(AssertUnwindSafe(f));

    HOSTED.set(previously);
    if let Some(dir) = cwd {
        let _ = std::env::set_current_dir(dir);
    }

    match outcome {
        Ok(code) => code,
        Err(payload) => match payload.downcast::<HostedExit>() {
            Ok(exit) => exit.0,
            Err(_) => 1,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_from_inside_becomes_a_return_value() {
        assert_eq!(run(|| exit(2)), 2);
    }

    #[test]
    fn a_normal_return_is_untouched() {
        assert_eq!(run(|| 0), 0);
    }

    #[test]
    fn a_panic_becomes_a_failed_run() {
        assert_eq!(run(|| panic!("boom")), 1);
    }

    #[test]
    fn the_flag_is_off_again_afterwards() {
        assert!(!is_hosted());
        assert_eq!(run(|| if is_hosted() { 1 } else { 0 }), 1);
        assert!(!is_hosted());
    }

    #[test]
    fn the_working_directory_is_restored() {
        let before = std::env::current_dir().unwrap();
        assert_eq!(
            run(|| {
                std::env::set_current_dir(std::env::temp_dir()).unwrap();
                0
            }),
            0
        );
        assert_eq!(std::env::current_dir().unwrap(), before);
    }
}
