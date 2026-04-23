//! Job control for zshrs
//!
//! Port from zsh/Src/jobs.c
//!
//! Provides job control, process management, and signal handling for jobs.

use std::process::Child;
use std::time::{Duration, Instant};

/// Job status flags
pub mod stat {
    pub const STOPPED: u32 = 1 << 0;      // Job is stopped
    pub const DONE: u32 = 1 << 1;         // Job is finished
    pub const SUBJOB: u32 = 1 << 2;       // Job is a subjob
    pub const CURSH: u32 = 1 << 3;        // Last pipeline elem in current shell
    pub const SUPERJOB: u32 = 1 << 4;     // Job is a superjob
    pub const WASSUPER: u32 = 1 << 5;     // Was a superjob
    pub const INUSE: u32 = 1 << 6;        // Entry in use
    pub const BUILTIN: u32 = 1 << 7;      // Job has builtin
    pub const DISOWN: u32 = 1 << 8;       // Disowned
    pub const NOTIFY: u32 = 1 << 9;       // Notify when done
    pub const ATTACH: u32 = 1 << 10;      // Attached to tty
}

/// Special process status values
pub const SP_RUNNING: i32 = -1;

/// Maximum pipestats
pub const MAX_PIPESTATS: usize = 256;

/// Process timing information
#[derive(Clone, Debug, Default)]
pub struct TimeInfo {
    pub user_time: Duration,
    pub sys_time: Duration,
}

/// A single process in a pipeline
#[derive(Clone, Debug)]
pub struct Process {
    pub pid: i32,
    pub status: i32,
    pub start_time: Option<Instant>,
    pub end_time: Option<Instant>,
    pub ti: TimeInfo,
    pub text: String,
}

impl Process {
    pub fn new(pid: i32) -> Self {
        Process {
            pid,
            status: SP_RUNNING,
            start_time: Some(Instant::now()),
            end_time: None,
            ti: TimeInfo::default(),
            text: String::new(),
        }
    }

    pub fn is_running(&self) -> bool {
        self.status == SP_RUNNING
    }

    pub fn is_stopped(&self) -> bool {
        // WIFSTOPPED equivalent
        self.status & 0xff == 0x7f
    }

    pub fn is_signaled(&self) -> bool {
        // WIFSIGNALED equivalent  
        (self.status & 0x7f) > 0 && (self.status & 0x7f) < 0x7f
    }

    pub fn exit_status(&self) -> i32 {
        // WEXITSTATUS equivalent
        (self.status >> 8) & 0xff
    }

    pub fn term_sig(&self) -> i32 {
        // WTERMSIG equivalent
        self.status & 0x7f
    }

    pub fn stop_sig(&self) -> i32 {
        // WSTOPSIG equivalent
        (self.status >> 8) & 0xff
    }
}

/// A job (pipeline)
#[derive(Clone, Debug)]
pub struct Job {
    pub stat: u32,
    pub gleader: i32,           // Process group leader
    pub procs: Vec<Process>,    // Processes in job
    pub auxprocs: Vec<Process>, // Auxiliary processes
    pub other: usize,           // For superjobs: subjob index
    pub filelist: Vec<String>,  // Temp files to delete
    pub text: String,           // Job text for display
}

impl Job {
    pub fn new() -> Self {
        Job {
            stat: 0,
            gleader: 0,
            procs: Vec::new(),
            auxprocs: Vec::new(),
            other: 0,
            filelist: Vec::new(),
            text: String::new(),
        }
    }

    pub fn is_done(&self) -> bool {
        (self.stat & stat::DONE) != 0
    }

    pub fn is_stopped(&self) -> bool {
        (self.stat & stat::STOPPED) != 0
    }

    pub fn is_superjob(&self) -> bool {
        (self.stat & stat::SUPERJOB) != 0
    }

    pub fn is_subjob(&self) -> bool {
        (self.stat & stat::SUBJOB) != 0
    }

    pub fn is_inuse(&self) -> bool {
        (self.stat & stat::INUSE) != 0
    }

    pub fn has_procs(&self) -> bool {
        !self.procs.is_empty() || !self.auxprocs.is_empty()
    }

    pub fn make_running(&mut self) {
        self.stat &= !stat::STOPPED;
        for proc in &mut self.procs {
            if proc.is_stopped() {
                proc.status = SP_RUNNING;
            }
        }
    }
}

impl Default for Job {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple job info for exec.rs compatibility
#[derive(Debug)]
pub struct JobInfo {
    pub id: usize,
    pub pid: i32,
    pub child: Option<Child>,
    pub command: String,
    pub state: JobState,
    pub is_current: bool,
}

/// Job table compatible with exec.rs
pub struct JobTable {
    jobs: Vec<Option<JobInfo>>,
    current_id: Option<usize>,
    next_id: usize,
}

impl Default for JobTable {
    fn default() -> Self {
        Self::new()
    }
}

impl JobTable {
    pub fn new() -> Self {
        JobTable {
            jobs: Vec::with_capacity(16),
            current_id: None,
            next_id: 1,
        }
    }

    /// Add a job with a Child process
    pub fn add_job(&mut self, child: Child, command: String, state: JobState) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        
        let pid = child.id() as i32;
        let job = JobInfo {
            id,
            pid,
            child: Some(child),
            command,
            state,
            is_current: true,
        };

        // Mark previous current as not current
        if let Some(cur_id) = self.current_id {
            if let Some(j) = self.get_mut_internal(cur_id) {
                j.is_current = false;
            }
        }

        // Add new job
        let slot = self.get_free_slot();
        if slot >= self.jobs.len() {
            self.jobs.resize_with(slot + 1, || None);
        }
        self.jobs[slot] = Some(job);
        self.current_id = Some(id);
        
        id
    }

    fn get_free_slot(&self) -> usize {
        for (i, slot) in self.jobs.iter().enumerate() {
            if slot.is_none() {
                return i;
            }
        }
        self.jobs.len()
    }

    fn get_mut_internal(&mut self, id: usize) -> Option<&mut JobInfo> {
        for job in self.jobs.iter_mut().flatten() {
            if job.id == id {
                return Some(job);
            }
        }
        None
    }

    /// Get a job by ID
    pub fn get(&self, id: usize) -> Option<&JobInfo> {
        for job in self.jobs.iter().flatten() {
            if job.id == id {
                return Some(job);
            }
        }
        None
    }

    /// Get a mutable job by ID
    pub fn get_mut(&mut self, id: usize) -> Option<&mut JobInfo> {
        self.get_mut_internal(id)
    }

    /// Remove a job by ID
    pub fn remove(&mut self, id: usize) -> Option<JobInfo> {
        for slot in self.jobs.iter_mut() {
            if slot.as_ref().map(|j| j.id == id).unwrap_or(false) {
                let job = slot.take();
                if self.current_id == Some(id) {
                    self.current_id = None;
                }
                return job;
            }
        }
        None
    }

    /// List all active jobs
    pub fn list(&self) -> Vec<&JobInfo> {
        self.jobs.iter().filter_map(|j| j.as_ref()).collect()
    }

    /// Iterate over jobs with their IDs (for compatibility)
    pub fn iter(&self) -> impl Iterator<Item = (usize, &JobInfo)> {
        self.jobs.iter().filter_map(|j| j.as_ref().map(|job| (job.id, job)))
    }

    /// Count number of active jobs
    pub fn count(&self) -> usize {
        self.jobs.iter().filter(|j| j.is_some()).count()
    }

    /// Check if there are any jobs
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// Get current job
    pub fn current(&self) -> Option<&JobInfo> {
        self.current_id.and_then(|id| self.get(id))
    }

    /// Reap finished jobs (check for completed processes)
    pub fn reap_finished(&mut self) -> Vec<JobInfo> {
        let mut finished = Vec::new();
        
        for slot in self.jobs.iter_mut() {
            if let Some(job) = slot {
                if let Some(ref mut child) = job.child {
                    // Try to check if child has finished without blocking
                    match child.try_wait() {
                        Ok(Some(_status)) => {
                            // Child finished
                            job.state = JobState::Done;
                        }
                        Ok(None) => {
                            // Still running
                        }
                        Err(_) => {
                            // Error checking, assume done
                            job.state = JobState::Done;
                        }
                    }
                }
            }
        }

        // Remove done jobs
        for slot in self.jobs.iter_mut() {
            if slot.as_ref().map(|j| j.state == JobState::Done).unwrap_or(false) {
                if let Some(job) = slot.take() {
                    finished.push(job);
                }
            }
        }

        finished
    }
}

/// Format a job for display
pub fn format_job(job: &Job, job_num: usize, cur_job: Option<usize>, prev_job: Option<usize>) -> String {
    let marker = if Some(job_num) == cur_job {
        '+'
    } else if Some(job_num) == prev_job {
        '-'
    } else {
        ' '
    };

    let status = if job.is_done() {
        "done"
    } else if job.is_stopped() {
        "suspended"
    } else {
        "running"
    };

    format!("[{}]{} {:10}  {}", job_num, marker, status, job.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_new() {
        let proc = Process::new(1234);
        assert_eq!(proc.pid, 1234);
        assert!(proc.is_running());
    }

    #[test]
    fn test_job_new() {
        let job = Job::new();
        assert_eq!(job.stat, 0);
        assert!(!job.is_done());
        assert!(!job.is_stopped());
    }

    #[test]
    fn test_job_table_new() {
        let table = JobTable::new();
        assert!(table.is_empty());
    }

    #[test]
    fn test_job_table_remove() {
        // This test would require spawning a real process, skipping for now
    }

    #[test]
    fn test_job_make_running() {
        let mut job = Job::new();
        job.stat |= stat::STOPPED;
        job.procs.push(Process { status: 0x007f, ..Process::new(1234) }); // Stopped

        job.make_running();
        assert!(!job.is_stopped());
        assert!(job.procs[0].is_running());
    }

    #[test]
    fn test_format_job() {
        let mut job = Job::new();
        job.text = "vim file.txt".to_string();
        job.stat |= stat::STOPPED;

        let formatted = format_job(&job, 1, Some(1), None);
        assert!(formatted.contains("[1]+"));
        assert!(formatted.contains("suspended"));
        assert!(formatted.contains("vim file.txt"));
    }

    #[test]
    fn test_job_state_enum() {
        let state = JobState::Running;
        assert_eq!(state, JobState::Running);
        assert_ne!(state, JobState::Stopped);
        assert_ne!(state, JobState::Done);
    }
}

/// Job state for simpler tracking
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Running,
    Stopped,
    Done,
}

/// Simple job entry for executor compatibility
#[derive(Debug)]
pub struct JobEntry {
    pub pid: i32,
    pub child: Option<Child>,
    pub command: String,
    pub state: JobState,
    pub is_current: bool,
}

/// Send a signal to a process
#[cfg(unix)]
pub fn send_signal(pid: i32, sig: nix::sys::signal::Signal) -> Result<(), String> {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    
    kill(Pid::from_raw(pid), sig).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
pub fn send_signal(_pid: i32, _sig: i32) -> Result<(), String> {
    Err("Signal sending not supported on this platform".to_string())
}

/// Continue a stopped job
#[cfg(unix)]
pub fn continue_job(pid: i32) -> Result<(), String> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    
    kill(Pid::from_raw(pid), Signal::SIGCONT).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
pub fn continue_job(_pid: i32) -> Result<(), String> {
    Err("Job control not supported on this platform".to_string())
}

/// Wait for a job to complete
#[cfg(unix)]
pub fn wait_for_job(pid: i32) -> Result<i32, String> {
    use nix::sys::wait::{waitpid, WaitStatus};
    use nix::unistd::Pid;
    
    loop {
        match waitpid(Pid::from_raw(pid), None) {
            Ok(WaitStatus::Exited(_, code)) => return Ok(code),
            Ok(WaitStatus::Signaled(_, sig, _)) => return Ok(128 + sig as i32),
            Ok(WaitStatus::Stopped(_, _)) => return Ok(128),
            Ok(_) => continue,
            Err(nix::errno::Errno::ECHILD) => return Ok(0),
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(not(unix))]
pub fn wait_for_job(_pid: i32) -> Result<i32, String> {
    Err("Job waiting not supported on this platform".to_string())
}

/// Wait for a child process
pub fn wait_for_child(child: &mut Child) -> Result<i32, String> {
    match child.wait() {
        Ok(status) => Ok(status.code().unwrap_or(0)),
        Err(e) => Err(e.to_string()),
    }
}
