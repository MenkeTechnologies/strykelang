//! Job control for zshrs
//!
//! Manages background processes, job table, and signals.

use nix::sys::signal::{self, Signal};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::process::Child;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Stopped,
    Done,
}

pub struct Job {
    pub id: usize,
    pub pid: u32,
    pub pgid: u32,
    pub command: String,
    pub state: JobState,
    pub is_current: bool,
    pub child: Option<Child>,
}

pub struct JobTable {
    jobs: HashMap<usize, Job>,
    next_id: usize,
    current_job: Option<usize>,
    previous_job: Option<usize>,
}

impl JobTable {
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            next_id: 1,
            current_job: None,
            previous_job: None,
        }
    }

    pub fn add_job(&mut self, child: Child, command: String, state: JobState) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let pid = child.id();

        let job = Job {
            id,
            pid,
            pgid: pid,
            command,
            state,
            is_current: true,
            child: Some(child),
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

    pub fn list(&self) -> Vec<&Job> {
        let mut jobs: Vec<_> = self.jobs.values().collect();
        jobs.sort_by_key(|j| j.id);
        jobs
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

pub fn send_signal(pid: u32, sig: Signal) -> Result<(), String> {
    signal::kill(Pid::from_raw(pid as i32), sig).map_err(|e| format!("kill: {}: {}", pid, e))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_table_add() {
        let mut table = JobTable::new();
        let id = table.add_job(1234, "sleep 100".to_string(), JobState::Running);
        assert_eq!(id, 1);
        assert!(table.get(1).is_some());
    }

    #[test]
    fn test_job_table_current() {
        let mut table = JobTable::new();
        table.add_job(1234, "cmd1".to_string(), JobState::Running);
        table.add_job(5678, "cmd2".to_string(), JobState::Running);

        let current = table.current().unwrap();
        assert_eq!(current.pid, 5678);

        let previous = table.previous().unwrap();
        assert_eq!(previous.pid, 1234);
    }

    #[test]
    fn test_job_table_remove() {
        let mut table = JobTable::new();
        table.add_job(1234, "cmd".to_string(), JobState::Running);
        assert!(table.remove(1).is_some());
        assert!(table.get(1).is_none());
    }
}
