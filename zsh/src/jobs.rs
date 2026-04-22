//! Job control for zshrs
//!
//! Manages background processes, job table, and signals.
//! Based on patterns from fish-shell's job_group.rs, wait_handle.rs, and proc.rs,
//! adapted for zsh semantics.

use nix::sys::signal::{self, Signal};
use nix::sys::wait::{waitpid, WaitStatus};
use nix::unistd::Pid;
use std::cell::Cell;
use std::collections::HashMap;
use std::process::Child;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

// ============================================================================
// Job ID Management (ported from fish job_group.rs)
// ============================================================================

/// Thread-safe job ID allocator. Job IDs are recycled when jobs complete.
static NEXT_JOB_ID: AtomicUsize = AtomicUsize::new(1);
static CONSUMED_JOB_IDS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Acquire a new job ID greater than all currently used IDs.
fn acquire_job_id() -> usize {
    let mut consumed = CONSUMED_JOB_IDS.lock().expect("Poisoned mutex");
    let id = consumed.last().map_or(1, |&last| last + 1);
    consumed.push(id);
    id
}

/// Release a job ID back to the pool.
fn release_job_id(id: usize) {
    let mut consumed = CONSUMED_JOB_IDS.lock().expect("Poisoned mutex");
    if let Ok(pos) = consumed.binary_search(&id) {
        consumed.remove(pos);
    }
}

// ============================================================================
// Process Status (ported from fish proc.rs)
// ============================================================================

/// Encapsulates exit status logic (exited vs stopped vs signaled).
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcStatus(Option<i32>);

impl ProcStatus {
    /// Construct from a waitpid() status.
    pub fn from_waitpid(status: i32) -> Self {
        ProcStatus(Some(status))
    }

    /// Construct from an exit code (0-255).
    pub fn from_exit_code(code: i32) -> Self {
        // Encode as if WIFEXITED would return true
        ProcStatus(Some((code & 0xff) << 8))
    }

    /// Construct from a signal number.
    pub fn from_signal(sig: i32) -> Self {
        ProcStatus(Some(sig & 0x7f))
    }

    /// Empty status (e.g., for variable assignments).
    pub fn empty() -> Self {
        ProcStatus(None)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    fn status(&self) -> i32 {
        self.0.unwrap_or(0)
    }

    /// True if process exited normally.
    pub fn normal_exited(&self) -> bool {
        libc::WIFEXITED(self.status())
    }

    /// True if process was signaled.
    pub fn signaled(&self) -> bool {
        libc::WIFSIGNALED(self.status())
    }

    /// True if process is stopped.
    pub fn stopped(&self) -> bool {
        libc::WIFSTOPPED(self.status())
    }

    /// True if process continued.
    pub fn continued(&self) -> bool {
        libc::WIFCONTINUED(self.status())
    }

    /// Get the exit code (if normally exited).
    pub fn exit_code(&self) -> Option<i32> {
        if self.normal_exited() {
            Some(libc::WEXITSTATUS(self.status()))
        } else {
            None
        }
    }

    /// Get the signal (if signaled).
    pub fn signal(&self) -> Option<i32> {
        if self.signaled() {
            Some(libc::WTERMSIG(self.status()))
        } else {
            None
        }
    }

    /// Get the stop signal (if stopped).
    pub fn stop_signal(&self) -> Option<i32> {
        if self.stopped() {
            Some(libc::WSTOPSIG(self.status()))
        } else {
            None
        }
    }

    /// Return the status code for $? (zsh semantics).
    pub fn status_value(&self) -> i32 {
        if self.is_empty() {
            0
        } else if self.normal_exited() {
            self.exit_code().unwrap_or(0)
        } else if self.signaled() {
            128 + self.signal().unwrap_or(0)
        } else if self.stopped() {
            128 + self.stop_signal().unwrap_or(0)
        } else {
            0
        }
    }
}

// ============================================================================
// Wait Handle Store (ported from fish wait_handle.rs)
// ============================================================================

/// Internal job ID for tracking (never recycled, always increases).
pub type InternalJobId = u64;
static NEXT_INTERNAL_JOB_ID: AtomicUsize = AtomicUsize::new(1);

fn next_internal_job_id() -> InternalJobId {
    NEXT_INTERNAL_JOB_ID.fetch_add(1, Ordering::SeqCst) as InternalJobId
}

/// Tracks a process for the `wait` builtin even after the job completes.
pub struct WaitHandle {
    pub pid: u32,
    pub internal_job_id: InternalJobId,
    pub command: String,
    status: Cell<Option<i32>>,
}

impl WaitHandle {
    pub fn new(pid: u32, internal_job_id: InternalJobId, command: String) -> Self {
        WaitHandle {
            pid,
            internal_job_id,
            command,
            status: Cell::new(None),
        }
    }

    pub fn is_completed(&self) -> bool {
        self.status.get().is_some()
    }

    pub fn set_status(&self, status: i32) {
        self.status.set(Some(status));
    }

    pub fn status(&self) -> Option<i32> {
        self.status.get()
    }
}

/// LRU cache of wait handles for completed processes.
pub struct WaitHandleStore {
    handles: Vec<WaitHandle>,
    capacity: usize,
}

impl Default for WaitHandleStore {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl WaitHandleStore {
    pub fn new(capacity: usize) -> Self {
        WaitHandleStore {
            handles: Vec::new(),
            capacity,
        }
    }

    pub fn add(&mut self, handle: WaitHandle) {
        // Remove existing handle with same PID
        self.handles.retain(|h| h.pid != handle.pid);

        // Evict oldest if at capacity
        if self.handles.len() >= self.capacity {
            self.handles.remove(0);
        }

        self.handles.push(handle);
    }

    pub fn get_by_pid(&self, pid: u32) -> Option<&WaitHandle> {
        self.handles.iter().rev().find(|h| h.pid == pid)
    }

    pub fn remove_by_pid(&mut self, pid: u32) {
        self.handles.retain(|h| h.pid != pid);
    }

    pub fn iter(&self) -> impl Iterator<Item = &WaitHandle> {
        self.handles.iter().rev()
    }

    pub fn len(&self) -> usize {
        self.handles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }
}

// ============================================================================
// Job State
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Stopped,
    Done,
}

impl std::fmt::Display for JobState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobState::Running => write!(f, "running"),
            JobState::Stopped => write!(f, "suspended"),
            JobState::Done => write!(f, "done"),
        }
    }
}

// ============================================================================
// Job
// ============================================================================

pub struct Job {
    pub id: usize,
    pub internal_id: InternalJobId,
    pub pid: u32,
    pub pgid: u32,
    pub command: String,
    pub state: JobState,
    pub is_current: bool,
    pub child: Option<Child>,
    pub nohup: bool,
    pub status: ProcStatus,
}

impl Job {
    /// Check if this job should be reported to the user.
    pub fn is_reportable(&self) -> bool {
        self.state == JobState::Done || self.state == JobState::Stopped
    }
}

// ============================================================================
// Job Table
// ============================================================================

pub struct JobTable {
    jobs: HashMap<usize, Job>,
    current_job: Option<usize>,
    previous_job: Option<usize>,
    wait_handles: WaitHandleStore,
}

impl JobTable {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            current_job: None,
            previous_job: None,
            wait_handles: WaitHandleStore::default(),
        }
    }

    pub fn add_job(&mut self, child: Child, command: String, state: JobState) -> usize {
        let id = acquire_job_id();
        let internal_id = next_internal_job_id();
        let pid = child.id();

        let job = Job {
            id,
            internal_id,
            pid,
            pgid: pid,
            command,
            state,
            is_current: true,
            child: Some(child),
            nohup: false,
            status: ProcStatus::empty(),
        };

        // Update current/previous
        if let Some(curr) = self.current_job {
            if let Some(j) = self.jobs.get_mut(&curr) {
                j.is_current = false;
            }
            self.previous_job = Some(curr);
        }
        self.current_job = Some(id);

        self.jobs.insert(id, job);
        id
    }

    pub fn get(&self, id: usize) -> Option<&Job> {
        self.jobs.get(&id)
    }

    pub fn count(&self) -> usize {
        self.jobs.len()
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut Job> {
        self.jobs.get_mut(&id)
    }

    pub fn get_by_pid(&self, pid: u32) -> Option<&Job> {
        self.jobs.values().find(|j| j.pid == pid)
    }

    pub fn get_by_pid_mut(&mut self, pid: u32) -> Option<&mut Job> {
        self.jobs.values_mut().find(|j| j.pid == pid)
    }

    pub fn current(&self) -> Option<&Job> {
        self.current_job.and_then(|id| self.jobs.get(&id))
    }

    pub fn previous(&self) -> Option<&Job> {
        self.previous_job.and_then(|id| self.jobs.get(&id))
    }

    pub fn remove(&mut self, id: usize) -> Option<Job> {
        let job = self.jobs.remove(&id);

        if let Some(ref j) = job {
            release_job_id(id);

            // Create wait handle for completed job
            let handle = WaitHandle::new(j.pid, j.internal_id, j.command.clone());
            if let Some(code) = j.status.exit_code() {
                handle.set_status(code);
            }
            self.wait_handles.add(handle);
        }

        if self.current_job == Some(id) {
            self.current_job = self.previous_job.take();
        }
        if self.previous_job == Some(id) {
            self.previous_job = None;
        }

        job
    }

    pub fn update_state(&mut self, pid: u32, state: JobState) {
        if let Some(job) = self.get_by_pid_mut(pid) {
            job.state = state;
        }
    }

    /// Mark a job to not receive SIGHUP on shell exit.
    pub fn mark_nohup(&mut self, id: usize) {
        if let Some(job) = self.jobs.get_mut(&id) {
            job.nohup = true;
        }
    }

    pub fn list(&self) -> Vec<&Job> {
        let mut jobs: Vec<_> = self.jobs.values().collect();
        jobs.sort_by_key(|j| j.id);
        jobs
    }

    /// Get wait handle by PID (for `wait` builtin).
    pub fn get_wait_handle(&self, pid: u32) -> Option<&WaitHandle> {
        self.wait_handles.get_by_pid(pid)
    }

    /// Iterate over all wait handles.
    pub fn wait_handles(&self) -> impl Iterator<Item = &WaitHandle> {
        self.wait_handles.iter()
    }

    pub fn reap_finished(&mut self) -> Vec<Job> {
        let mut finished = Vec::new();
        let ids: Vec<usize> = self.jobs.keys().copied().collect();

        for id in ids {
            if let Some(job) = self.jobs.get_mut(&id) {
                if let Some(ref mut child) = job.child {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            job.state = JobState::Done;
                            job.status = ProcStatus::from_exit_code(status.code().unwrap_or(0));
                        }
                        Ok(None) => {
                            // Still running
                        }
                        Err(_) => {
                            job.state = JobState::Done;
                        }
                    }
                }
            }
        }

        // Collect and remove done jobs
        let done_ids: Vec<usize> = self
            .jobs
            .iter()
            .filter(|(_, j)| j.state == JobState::Done)
            .map(|(id, _)| *id)
            .collect();

        for id in done_ids {
            if let Some(job) = self.remove(id) {
                finished.push(job);
            }
        }

        finished
    }
}

impl Default for JobTable {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Signal Utilities
// ============================================================================

pub fn send_signal(pid: u32, sig: Signal) -> Result<(), String> {
    signal::kill(Pid::from_raw(pid as i32), sig).map_err(|e| format!("kill: {}: {}", pid, e))
}

pub fn send_signal_to_group(pgid: u32, sig: Signal) -> Result<(), String> {
    signal::killpg(Pid::from_raw(pgid as i32), sig).map_err(|e| format!("kill: -{}: {}", pgid, e))
}

pub fn continue_job(pid: u32) -> Result<(), String> {
    send_signal(pid, Signal::SIGCONT)
}

pub fn stop_job(pid: u32) -> Result<(), String> {
    send_signal(pid, Signal::SIGSTOP)
}

pub fn terminate_job(pid: u32) -> Result<(), String> {
    send_signal(pid, Signal::SIGTERM)
}

pub fn kill_job(pid: u32) -> Result<(), String> {
    send_signal(pid, Signal::SIGKILL)
}

pub fn wait_for_child(child: &mut Child) -> Result<i32, String> {
    match child.wait() {
        Ok(status) => Ok(status.code().unwrap_or(0)),
        Err(e) => Err(format!("wait: {}", e)),
    }
}

pub fn wait_for_job(pid: u32) -> Result<i32, String> {
    match waitpid(Pid::from_raw(pid as i32), None) {
        Ok(WaitStatus::Exited(_, code)) => Ok(code),
        Ok(WaitStatus::Signaled(_, sig, _)) => Ok(128 + sig as i32),
        Ok(WaitStatus::Stopped(_, _)) => Ok(128 + Signal::SIGSTOP as i32),
        Ok(_) => Ok(0),
        Err(e) => Err(format!("wait: {}", e)),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn spawn_sleep() -> Child {
        Command::new("sleep")
            .arg("0.001")
            .spawn()
            .expect("Failed to spawn sleep")
    }

    #[test]
    fn test_job_table_add() {
        let mut table = JobTable::new();
        let child = spawn_sleep();
        let id = table.add_job(child, "sleep 0.001".to_string(), JobState::Running);
        assert!(id >= 1);
        assert!(table.get(id).is_some());
    }

    #[test]
    fn test_job_table_current() {
        let mut table = JobTable::new();
        let child1 = spawn_sleep();
        let child2 = spawn_sleep();
        let pid1 = child1.id();
        let pid2 = child2.id();
        table.add_job(child1, "cmd1".to_string(), JobState::Running);
        table.add_job(child2, "cmd2".to_string(), JobState::Running);

        let current = table.current().unwrap();
        assert_eq!(current.pid, pid2);

        let previous = table.previous().unwrap();
        assert_eq!(previous.pid, pid1);
    }

    #[test]
    fn test_job_table_remove() {
        let mut table = JobTable::new();
        let child = spawn_sleep();
        let id = table.add_job(child, "cmd".to_string(), JobState::Running);
        assert!(table.remove(id).is_some());
        assert!(table.get(id).is_none());
    }

    #[test]
    fn test_proc_status() {
        let status = ProcStatus::from_exit_code(42);
        assert!(status.normal_exited());
        assert_eq!(status.exit_code(), Some(42));
        assert_eq!(status.status_value(), 42);

        let empty = ProcStatus::empty();
        assert!(empty.is_empty());
        assert_eq!(empty.status_value(), 0);
    }

    #[test]
    fn test_wait_handle_store() {
        let mut store = WaitHandleStore::new(3);

        store.add(WaitHandle::new(100, 1, "cmd1".to_string()));
        store.add(WaitHandle::new(200, 2, "cmd2".to_string()));
        store.add(WaitHandle::new(300, 3, "cmd3".to_string()));

        assert_eq!(store.len(), 3);
        assert!(store.get_by_pid(100).is_some());

        // Adding a 4th should evict the oldest
        store.add(WaitHandle::new(400, 4, "cmd4".to_string()));
        assert_eq!(store.len(), 3);
        assert!(store.get_by_pid(100).is_none());
        assert!(store.get_by_pid(400).is_some());
    }
}
