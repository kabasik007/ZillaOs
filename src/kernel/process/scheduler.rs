//! Cooperative run-queue selection policy only; no CPU context switch yet.
use super::{Pid, ProcessState, ProcessTable};

#[derive(Clone, Copy, Debug, Default)]
pub struct RoundRobin {
    last: u64,
}

impl RoundRobin {
    pub const fn new() -> Self { Self { last: 0 } }

    /// Return the next ready PID without changing execution state.
    pub fn next(&mut self, processes: &ProcessTable) -> Option<Pid> {
        let mut first: Option<Pid> = None;
        let mut after: Option<Pid> = None;
        for process in processes.iter().filter(|p| p.state == ProcessState::Ready) {
            let pid = process.pid;
            if first.is_none_or(|old| pid < old) {
                first = Some(pid);
            }
            if pid.0 > self.last && after.is_none_or(|old| pid < old) {
                after = Some(pid);
            }
        }
        let chosen = after.or(first)?;
        self.last = chosen.0;
        Some(chosen)
    }
}
