use std::{mem::MaybeUninit, time::Instant};

use sysinfo::{Pid, System};

use crate::collect::{MemSet, ProcRec, Snapshot};

const RUSAGE_INFO_V2: libc::c_int = 2;

#[repr(C)]
struct RusageInfoV2 {
    ri_uuid: [u8; 16],
    ri_user_time: u64,
    ri_system_time: u64,
    ri_pkg_idle_wkups: u64,
    ri_interrupt_wkups: u64,
    ri_pageins: u64,
    ri_wired_size: u64,
    ri_resident_size: u64,
    ri_phys_footprint: u64,
    ri_proc_start_abstime: u64,
    ri_proc_exit_abstime: u64,
}

#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pid_rusage(
        pid: libc::c_int,
        flavor: libc::c_int,
        buffer: *mut libc::c_void,
    ) -> libc::c_int;
}

fn pid_to_i32(pid: Pid) -> i32 {
    pid.to_string().parse::<i32>().unwrap_or_default()
}

fn read_phys_footprint(pid: i32) -> Option<u64> {
    let mut usage = MaybeUninit::<RusageInfoV2>::zeroed();
    let result = unsafe { proc_pid_rusage(pid, RUSAGE_INFO_V2, usage.as_mut_ptr().cast()) };
    if result != 0 {
        return None;
    }

    let usage = unsafe { usage.assume_init() };
    Some(usage.ri_phys_footprint)
}

pub(crate) fn take_snapshot() -> Snapshot {
    let mut system = System::new_all();
    system.refresh_all();

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
