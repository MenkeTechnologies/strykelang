//! Synchronization primitives — basic exclusive mutex + counting semaphore.
//!
//! These match the POSIX `pthread_mutex_t` / `sem_t` contract and the
//! Python `threading.Lock` / `threading.Semaphore` surface; nothing novel,
//! just the workhorse primitives users expect from a concurrent language.
//!
//! Blocking variants (`mutex_lock`, `semaphore_acquire`) park on
//! `parking_lot::Condvar` rather than busy-spinning; non-blocking
//! variants (`mutex_try_lock`, `semaphore_try_acquire`) test and return
//! `1` / `0`. Guards never leave a builtin call — the storage is a `held`
//! flag (mutex) or a `permits` counter (semaphore) protected by the
//! handle's own `parking_lot::Mutex`, so `MutexGuard` lifetimes never have
//! to cross a VM dispatch boundary.

use crate::error::{StrykeError, StrykeResult};
use crate::value::StrykeValue;

/// `mutex()` → fresh unlocked mutex value.
pub fn mutex_new(_args: &[StrykeValue], _line: usize) -> StrykeResult<StrykeValue> {
    Ok(StrykeValue::mutex())
}

/// `mutex_lock($m)` — block until `$m` is acquired; sets held = true.
/// Returns UNDEF (the lock-acquired action is a side effect).
pub fn mutex_lock(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let Some(m) = args.first().and_then(|v| v.as_mutex()) else {
        return Err(StrykeError::runtime(
            "mutex_lock: argument must be a mutex",
            line,
        ));
    };
    let mut held = m.held.lock();
    while *held {
        m.condvar.wait(&mut held);
    }
    *held = true;
    Ok(StrykeValue::UNDEF)
}

/// `mutex_unlock($m)` — release the lock and wake one waiter.
/// Unlocking a mutex that isn't held is a no-op (matches Python `Lock.release`
/// raising vs POSIX undefined — we take the more forgiving stance and
/// simply notify; user-visible state stays consistent).
pub fn mutex_unlock(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let Some(m) = args.first().and_then(|v| v.as_mutex()) else {
        return Err(StrykeError::runtime(
            "mutex_unlock: argument must be a mutex",
            line,
        ));
    };
    {
        let mut held = m.held.lock();
        *held = false;
    }
    m.condvar.notify_one();
    Ok(StrykeValue::UNDEF)
}

/// `mutex_try_lock($m)` — non-blocking. Returns `1` if acquired, `0` if
/// already held by someone else.
pub fn mutex_try_lock(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let Some(m) = args.first().and_then(|v| v.as_mutex()) else {
        return Err(StrykeError::runtime(
            "mutex_try_lock: argument must be a mutex",
            line,
        ));
    };
    let mut held = m.held.lock();
    if *held {
        Ok(StrykeValue::integer(0))
    } else {
        *held = true;
        Ok(StrykeValue::integer(1))
    }
}

/// `mutex_is_locked($m)` → `1` if currently held, else `0`.
pub fn mutex_is_locked(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let Some(m) = args.first().and_then(|v| v.as_mutex()) else {
        return Err(StrykeError::runtime(
            "mutex_is_locked: argument must be a mutex",
            line,
        ));
    };
    let held = *m.held.lock();
    Ok(StrykeValue::integer(i64::from(held)))
}

/// `semaphore($n)` / `sem($n)` — create a counting semaphore with `n`
/// permits. `n` must be `>= 0` (defaults to `0` if omitted, matching
/// the "empty semaphore, waiters block until a release" idiom).
pub fn semaphore_new(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let n = args.first().map(|v| v.to_int()).unwrap_or(0);
    if n < 0 {
        return Err(StrykeError::runtime(
            format!("semaphore: permit count must be >= 0 (got {n})"),
            line,
        ));
    }
    Ok(StrykeValue::semaphore(n))
}

/// `semaphore_acquire($s)` / `sem_acquire($s)` — block until a permit is
/// available, then decrement. Returns UNDEF.
pub fn semaphore_acquire(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let Some(s) = args.first().and_then(|v| v.as_semaphore()) else {
        return Err(StrykeError::runtime(
            "semaphore_acquire: argument must be a semaphore",
            line,
        ));
    };
    let mut permits = s.permits.lock();
    while *permits <= 0 {
        s.condvar.wait(&mut permits);
    }
    *permits -= 1;
    Ok(StrykeValue::UNDEF)
}

/// `semaphore_release($s)` / `sem_release($s)` — increment the permit
/// count and wake one waiter. Returns UNDEF.
pub fn semaphore_release(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let Some(s) = args.first().and_then(|v| v.as_semaphore()) else {
        return Err(StrykeError::runtime(
            "semaphore_release: argument must be a semaphore",
            line,
        ));
    };
    {
        let mut permits = s.permits.lock();
        *permits += 1;
    }
    s.condvar.notify_one();
    Ok(StrykeValue::UNDEF)
}

/// `semaphore_try_acquire($s)` — non-blocking; returns `1` if a permit
/// was acquired, `0` if none available.
pub fn semaphore_try_acquire(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let Some(s) = args.first().and_then(|v| v.as_semaphore()) else {
        return Err(StrykeError::runtime(
            "semaphore_try_acquire: argument must be a semaphore",
            line,
        ));
    };
    let mut permits = s.permits.lock();
    if *permits > 0 {
        *permits -= 1;
        Ok(StrykeValue::integer(1))
    } else {
        Ok(StrykeValue::integer(0))
    }
}

/// `semaphore_permits($s)` → current number of available permits.
pub fn semaphore_permits(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let Some(s) = args.first().and_then(|v| v.as_semaphore()) else {
        return Err(StrykeError::runtime(
            "semaphore_permits: argument must be a semaphore",
            line,
        ));
    };
    let n = *s.permits.lock();
    Ok(StrykeValue::integer(n))
}

/// `semaphore_limit($s)` → the initial max permit count (`N` from
/// `semaphore(N)`).
pub fn semaphore_limit(args: &[StrykeValue], line: usize) -> StrykeResult<StrykeValue> {
    let Some(s) = args.first().and_then(|v| v.as_semaphore()) else {
        return Err(StrykeError::runtime(
            "semaphore_limit: argument must be a semaphore",
            line,
        ));
    };
    Ok(StrykeValue::integer(s.limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── mutex_new / mutex_is_locked ─────────────────────────────────────

    #[test]
    fn mutex_new_starts_unlocked() {
        let m = mutex_new(&[], 0).unwrap();
        assert_eq!(mutex_is_locked(&[m], 0).unwrap().to_int(), 0);
    }

    #[test]
    fn mutex_is_locked_wrong_type_returns_runtime_err() {
        let r = mutex_is_locked(&[StrykeValue::integer(7)], 42);
        assert!(r.is_err());
    }

    // ─── mutex_try_lock / mutex_unlock ───────────────────────────────────

    #[test]
    fn mutex_try_lock_acquires_first_then_fails_second() {
        let m = mutex_new(&[], 0).unwrap();
        // First try succeeds.
        assert_eq!(mutex_try_lock(&[m.clone()], 0).unwrap().to_int(), 1);
        // Now held → state reflects.
        assert_eq!(mutex_is_locked(&[m.clone()], 0).unwrap().to_int(), 1);
        // Second try fails (returns 0, not error).
        assert_eq!(mutex_try_lock(&[m], 0).unwrap().to_int(), 0);
    }

    #[test]
    fn mutex_unlock_releases_so_try_lock_succeeds_again() {
        let m = mutex_new(&[], 0).unwrap();
        mutex_try_lock(&[m.clone()], 0).unwrap();
        mutex_unlock(&[m.clone()], 0).unwrap();
        assert_eq!(mutex_is_locked(&[m.clone()], 0).unwrap().to_int(), 0);
        assert_eq!(mutex_try_lock(&[m], 0).unwrap().to_int(), 1);
    }

    #[test]
    fn mutex_unlock_when_unheld_is_idempotent() {
        // Contract documented in the source: unlocking an unheld mutex is a no-op
        // (we take the forgiving stance).
        let m = mutex_new(&[], 0).unwrap();
        let r = mutex_unlock(&[m.clone()], 0);
        assert!(r.is_ok());
        assert_eq!(mutex_is_locked(&[m], 0).unwrap().to_int(), 0);
    }

    #[test]
    fn mutex_lock_returns_undef() {
        let m = mutex_new(&[], 0).unwrap();
        // Lock on a fresh mutex should not block.
        let r = mutex_lock(&[m], 0).unwrap();
        assert!(r.is_undef());
    }

    #[test]
    fn mutex_try_lock_no_args_errors() {
        // First arg missing → no mutex → runtime error.
        assert!(mutex_try_lock(&[], 0).is_err());
    }

    // ─── semaphore_new ───────────────────────────────────────────────────

    #[test]
    fn semaphore_new_default_zero_permits() {
        let s = semaphore_new(&[], 0).unwrap();
        assert_eq!(semaphore_permits(&[s.clone()], 0).unwrap().to_int(), 0);
        assert_eq!(semaphore_limit(&[s], 0).unwrap().to_int(), 0);
    }

    #[test]
    fn semaphore_new_rejects_negative_permits() {
        let r = semaphore_new(&[StrykeValue::integer(-1)], 0);
        assert!(r.is_err());
    }

    #[test]
    fn semaphore_new_preserves_initial_count() {
        let s = semaphore_new(&[StrykeValue::integer(3)], 0).unwrap();
        assert_eq!(semaphore_permits(&[s.clone()], 0).unwrap().to_int(), 3);
        assert_eq!(semaphore_limit(&[s], 0).unwrap().to_int(), 3);
    }

    // ─── semaphore acquire / release ─────────────────────────────────────

    #[test]
    fn semaphore_try_acquire_decrements_permits() {
        let s = semaphore_new(&[StrykeValue::integer(2)], 0).unwrap();
        assert_eq!(semaphore_try_acquire(&[s.clone()], 0).unwrap().to_int(), 1);
        assert_eq!(semaphore_permits(&[s.clone()], 0).unwrap().to_int(), 1);
        assert_eq!(semaphore_try_acquire(&[s.clone()], 0).unwrap().to_int(), 1);
        // Empty now.
        assert_eq!(semaphore_try_acquire(&[s], 0).unwrap().to_int(), 0);
    }

    #[test]
    fn semaphore_release_increments_above_initial_limit() {
        // Contract: release does not cap at limit (semaphore_limit is just the
        // initial value, not a max).
        let s = semaphore_new(&[StrykeValue::integer(1)], 0).unwrap();
        semaphore_release(&[s.clone()], 0).unwrap();
        semaphore_release(&[s.clone()], 0).unwrap();
        assert_eq!(semaphore_permits(&[s.clone()], 0).unwrap().to_int(), 3);
        // Limit unchanged.
        assert_eq!(semaphore_limit(&[s], 0).unwrap().to_int(), 1);
    }

    #[test]
    fn semaphore_acquire_on_available_does_not_block() {
        let s = semaphore_new(&[StrykeValue::integer(1)], 0).unwrap();
        let r = semaphore_acquire(&[s.clone()], 0).unwrap();
        assert!(r.is_undef());
        assert_eq!(semaphore_permits(&[s], 0).unwrap().to_int(), 0);
    }

    #[test]
    fn semaphore_wrong_type_errors_on_each_op() {
        let bad = StrykeValue::integer(7);
        assert!(semaphore_permits(&[bad.clone()], 0).is_err());
        assert!(semaphore_limit(&[bad.clone()], 0).is_err());
        assert!(semaphore_acquire(&[bad.clone()], 0).is_err());
        assert!(semaphore_release(&[bad.clone()], 0).is_err());
        assert!(semaphore_try_acquire(&[bad], 0).is_err());
    }
}
