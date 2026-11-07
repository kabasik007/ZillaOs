//! Read-only bounded process/thread snapshot contract.
//! Exposes real timer interrupt counts only when a CPU backend charges them.
//! Never invent CPU %, RSS, times, or unrelated GUI windows.
use crate::process::{Pid, ProcessState, ProcessTable, MAX_PROCESSES};
use crate::process::thread::{Tid, ThreadState, ThreadTable, MAX_THREADS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: Pid,
    pub name: &'static str,
    pub state: ProcessState,
    pub thread_count: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadInfo {
    pub tid: Tid,
    pub pid: Pid,
    pub state: ThreadState,
    pub assigned_cpu: Option<usize>,
    /// Uncalibrated IRQ tick samples; zero means none were recorded.
    pub timer_ticks: u64,
}

#[derive(Clone, Debug)]
pub struct TaskSnapshot {
    process_count: usize,
    thread_count: usize,
    processes: [Option<ProcessInfo>; MAX_PROCESSES],
    threads: [Option<ThreadInfo>; MAX_THREADS],
    pub online_cpu_mask: u64,
}

impl TaskSnapshot {
    pub fn collect(processes: &ProcessTable, threads: &ThreadTable) -> Self {
        let mut snapshot = Self {
            process_count: 0,
            thread_count: 0,
            processes: [None; MAX_PROCESSES],
            threads: [None; MAX_THREADS],
            online_cpu_mask: threads.online_cpu_mask(),
        };
        for p in processes.iter() {
            let n = threads.iter().filter(|t| t.pid == p.pid).count();
            snapshot.processes[snapshot.process_count] = Some(ProcessInfo {
                pid: p.pid,
                name: p.name,
                state: p.state,
                thread_count: n.min(u8::MAX as usize) as u8,
            });
            snapshot.process_count += 1;
        }
        for t in threads.iter() {
            snapshot.threads[snapshot.thread_count] = Some(ThreadInfo {
                tid: t.tid,
                pid: t.pid,
                state: t.state,
                assigned_cpu: t.assigned_cpu,
                timer_ticks: t.timer_ticks,
            });
            snapshot.thread_count += 1;
        }
        snapshot
    }

    pub const fn process_count(&self) -> usize { self.process_count }
    pub const fn thread_count(&self) -> usize { self.thread_count }
    pub fn processes(&self) -> impl Iterator<Item = ProcessInfo> + '_ {
        self.processes[..self.process_count].iter().flatten().copied()
    }
    pub fn threads(&self) -> impl Iterator<Item = ThreadInfo> + '_ {
        self.threads[..self.thread_count].iter().flatten().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::address_space::{Permissions, UserRegion, PAGE_SIZE};
    use crate::services::process_manager::ProcessManager;

    #[test]
    fn enumerates_only_registered_processes_and_owned_threads() {
        let mut pm=ProcessManager::new();
        let empty=pm.snapshot();
        assert_eq!((empty.process_count(),empty.thread_count()),(0,0));
        assert_eq!(empty.online_cpu_mask,1);
        let pid=pm.reserve_native("sample",0x400000,0x800000,4).unwrap();
        pm.map_native_region(pid,UserRegion::checked(
            0x400000,PAGE_SIZE,Permissions::READ_EXECUTE,
        ).unwrap()).unwrap();
        pm.prepare_native(pid).unwrap();
        let a=pm.reserve_thread(pid,0x400000,0x800000,1).unwrap();
        pm.prepare_thread(a).unwrap();
        let snap=pm.snapshot();
        assert_eq!(snap.process_count(),1);
        assert_eq!(snap.thread_count(),1);
        let p=snap.processes().next().unwrap();
        assert_eq!(p.pid,pid);
        assert_eq!(p.thread_count,1);
        assert_eq!(p.state,ProcessState::Ready);
        let t=snap.threads().next().unwrap();
        assert_eq!(t.tid,a);
        assert_eq!(t.pid,pid);
        assert_eq!(t.state,ThreadState::Ready);
        assert_eq!(t.assigned_cpu,None);
        assert_eq!(t.timer_ticks,0);
    }
}
