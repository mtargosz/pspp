use std::{mem::MaybeUninit, time::Instant};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::collect::{MemSet, ProcRec, Snapshot};

fn pid_to_i32(pid: Pid) -> i32 {
    i32::try_from(pid.as_u32()).unwrap_or_default()
}

fn read_phys_footprint(pid: i32) -> Option<u64> {
    // The kernel writes a full `rusage_info_v2`, so the buffer must use libc's complete layout.
    let mut usage = MaybeUninit::<libc::rusage_info_v2>::zeroed();
    let result =
        unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, usage.as_mut_ptr().cast()) };
    if result != 0 {
        return None;
    }

    let usage = unsafe { usage.assume_init() };
    Some(usage.ri_phys_footprint)
}

pub(crate) fn take_snapshot() -> Snapshot {
    let mut system = System::new();
    system.refresh_memory();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );

    let mut procs = Vec::with_capacity(system.processes().len());
    for (pid, process) in system.processes() {
        let pid = pid_to_i32(*pid);
        let ppid = process.parent().map(pid_to_i32).unwrap_or_default();
        let footprint = read_phys_footprint(pid);

        procs.push(ProcRec {
            pid,
            ppid,
            comm: process.name().to_string_lossy().into_owned(),
            mem: MemSet {
                rss: process.memory(),
                vsz: process.virtual_memory(),
                footprint,
                pss: None,
                uss: None,
                swap: None,
                approximate: footprint.is_none(),
            },
        });
    }

    Snapshot {
        procs,
        total_mem: system.total_memory(),
        taken_at: Instant::now(),
    }
}
