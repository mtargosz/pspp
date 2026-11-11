use std::{collections::HashMap, mem::MaybeUninit, time::Instant};

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

/// `sizeof(struct kinfo_proc)` on 64-bit macOS (x86_64 and arm64 share the layout).
const KINFO_PROC_SIZE: usize = 648;
/// Byte offsets inside `struct kinfo_proc` (`kp_proc.p_pid`, `kp_proc.p_comm`, `kp_eproc.e_ppid`).
const OFF_PID: usize = 40;
const OFF_COMM: usize = 243;
const COMM_LEN: usize = 17;
const OFF_PPID: usize = 560;

/// Identity of one process as reported by `sysctl(KERN_PROC_ALL)`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct KernProc {
    pid: i32,
    ppid: i32,
    comm: String,
}

fn read_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    let raw: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(i32::from_ne_bytes(raw))
}

/// Parses a raw `kinfo_proc` array; trailing partial entries are ignored.
fn parse_kinfo_procs(buf: &[u8]) -> Vec<KernProc> {
    buf.chunks_exact(KINFO_PROC_SIZE)
        .filter_map(|entry| {
            let pid = read_i32(entry, OFF_PID)?;
            let ppid = read_i32(entry, OFF_PPID)?;
            let comm_raw = entry.get(OFF_COMM..OFF_COMM + COMM_LEN)?;
            let end = comm_raw
                .iter()
                .position(|b| *b == 0)
                .unwrap_or(comm_raw.len());
            let comm = String::from_utf8_lossy(&comm_raw[..end]).into_owned();
            Some(KernProc { pid, ppid, comm })
        })
        .collect()
}

/// Lists every process (pid, ppid, short name) without privileges, like `ps` does.
fn read_kern_procs() -> Vec<KernProc> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_ALL];
    // The table can grow between the sizing call and the read, so retry on ENOMEM.
    for _ in 0..8 {
        let mut len: usize = 0;
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 || len == 0 {
            return Vec::new();
        }
        // Headroom for processes spawned between the two calls.
        let mut buf = vec![0u8; len.saturating_add(len / 8 + 16 * KINFO_PROC_SIZE)];
        let mut cap = buf.len();
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr().cast(),
                &mut cap,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc == 0 {
            buf.truncate(cap.min(buf.len()));
            return parse_kinfo_procs(&buf);
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOMEM) {
            return Vec::new();
        }
    }
    Vec::new()
}

pub(crate) fn take_snapshot() -> Snapshot {
    let mut system = System::new();
    system.refresh_memory();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );

    // sysinfo's parent() needs proc_pidinfo, which fails for other users' processes without root.
    let kern: HashMap<i32, KernProc> = read_kern_procs().into_iter().map(|k| (k.pid, k)).collect();

    let mut procs = Vec::with_capacity(system.processes().len());
    for (pid, process) in system.processes() {
        let pid = pid_to_i32(*pid);
        let known = kern.get(&pid);
        let ppid = known
            .map(|k| k.ppid)
            .or_else(|| process.parent().map(pid_to_i32))
            .unwrap_or_default();
        let footprint = read_phys_footprint(pid);
        let mut comm = process.name().to_string_lossy().into_owned();
        if comm.is_empty() {
            comm = known.map(|k| k.comm.clone()).unwrap_or_default();
        }

        procs.push(ProcRec {
            pid,
            ppid,
            comm,
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
        used_mem: system.used_memory().min(system.total_memory()),
        taken_at: Instant::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_ppid_matches_getppid() {
        let snapshot = take_snapshot();
        let me = i32::try_from(std::process::id()).unwrap_or(i32::MAX);
        let rec = snapshot.procs.iter().find(|p| p.pid == me);
        assert_eq!(rec.map(|p| p.ppid), Some(unsafe { libc::getppid() }));
    }

    #[test]
    fn kern_procs_include_self_and_launchd() {
        let procs = read_kern_procs();
        let me = i32::try_from(std::process::id()).unwrap_or(i32::MAX);
        assert!(procs.iter().any(|p| p.pid == me && !p.comm.is_empty()));
        assert!(procs.iter().any(|p| p.pid == 1 && p.ppid == 0));
    }

    #[test]
    fn few_processes_are_parentless() {
        let snapshot = take_snapshot();
        let orphans = snapshot.procs.iter().filter(|p| p.ppid == 0).count();
        assert!(orphans <= 4, "{orphans} processes have ppid 0");
    }

    #[test]
    fn parse_ignores_partial_trailing_entry() {
        let mut buf = vec![0u8; KINFO_PROC_SIZE + 100];
        buf[OFF_PID..OFF_PID + 4].copy_from_slice(&7i32.to_ne_bytes());
        buf[OFF_PPID..OFF_PPID + 4].copy_from_slice(&3i32.to_ne_bytes());
        buf[OFF_COMM..OFF_COMM + 3].copy_from_slice(b"abc");
        let parsed = parse_kinfo_procs(&buf);
        assert_eq!(
            parsed,
            vec![KernProc {
                pid: 7,
                ppid: 3,
                comm: "abc".into()
            }]
        );
        assert!(parse_kinfo_procs(&[]).is_empty());
    }
}
