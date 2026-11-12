use std::{collections::HashSet, mem::MaybeUninit};

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::collect::{MemSet, ProcRec, Snapshot};
use crate::model::MemKind;

/// Outcome of probing per-process Mach/rusage counters on macOS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProbeOutcome<T> {
    /// Readable counters, or exact `0` for an unreaped zombie (`SZOMB`).
    Value(T),
    /// Process still exists (or `pid <= 0`), but counters are privileged/unreadable (`EPERM`, etc.).
    Unreadable,
    /// Process exited and was reaped mid-scan (`ESRCH` with `kill(pid, 0)` `ESRCH`); skip record.
    Vanished,
}

/// Checks whether `pid` still has an entry in the kernel process table (including `SZOMB` zombies).
///
/// For `pid > 0`, `kill(pid, 0)` performs error checking without sending a signal:
/// - `0`: PID exists and caller can signal it (live process or own unreaped zombie).
/// - `-1` with `EPERM`: PID exists (owned by another user, live or zombie).
/// - `-1` with `ESRCH`: PID no longer exists (exited and reaped).
fn process_still_exists(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Pure classification for `proc_pidinfo(PROC_PIDTASKINFO)` results.
fn classify_taskinfo(
    pid: i32,
    ret: libc::c_int,
    expected: usize,
    counters: (u64, u64),
    errno: Option<i32>,
    process_still_exists: bool,
) -> ProbeOutcome<(u64, u64)> {
    if pid <= 0 {
        return ProbeOutcome::Unreadable;
    }
    if usize::try_from(ret).ok() == Some(expected) {
        return ProbeOutcome::Value(counters);
    }
    if ret <= 0 && errno == Some(libc::ESRCH) {
        return if process_still_exists {
            ProbeOutcome::Value((0, 0))
        } else {
            ProbeOutcome::Vanished
        };
    }
    ProbeOutcome::Unreadable
}

/// Pure classification for `proc_pid_rusage(RUSAGE_INFO_V2)` results.
fn classify_rusage(
    pid: i32,
    ret: libc::c_int,
    footprint: u64,
    errno: Option<i32>,
    process_still_exists: bool,
) -> ProbeOutcome<u64> {
    if pid <= 0 {
        return ProbeOutcome::Unreadable;
    }
    if ret == 0 {
        return ProbeOutcome::Value(footprint);
    }
    if errno == Some(libc::ESRCH) {
        return if process_still_exists {
            ProbeOutcome::Value(0)
        } else {
            ProbeOutcome::Vanished
        };
    }
    ProbeOutcome::Unreadable
}

/// Reads `(rss, vsz)` in bytes via `proc_pidinfo(PROC_PIDTASKINFO)`.
///
/// Unlike `proc_pid_rusage` (which returns `0` on success), `proc_pidinfo` returns the byte count
/// written (`<= 0` on `EPERM`/`ESRCH`). Unprivileged callers get `ProbeOutcome::Unreadable` on
/// `EPERM`, `ProbeOutcome::Value((0, 0))` for live `SZOMB` zombies (`ESRCH` while the PID still
/// exists in the process table), and `ProbeOutcome::Vanished` for processes reaped mid-scan.
fn read_task_memory(pid: i32) -> ProbeOutcome<(u64, u64)> {
    if pid <= 0 {
        return ProbeOutcome::Unreadable;
    }

    let expected = std::mem::size_of::<libc::proc_taskinfo>();
    let Ok(buf_size) = libc::c_int::try_from(expected) else {
        return ProbeOutcome::Unreadable;
    };
    let mut task_info = MaybeUninit::<libc::proc_taskinfo>::zeroed();
    unsafe {
        *libc::__error() = 0;
    }
    let ret = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTASKINFO,
            0,
            task_info.as_mut_ptr().cast(),
            buf_size,
        )
    };
    if usize::try_from(ret).ok() == Some(expected) {
        let task_info = unsafe { task_info.assume_init() };
        return ProbeOutcome::Value((task_info.pti_resident_size, task_info.pti_virtual_size));
    }

    let errno = std::io::Error::last_os_error().raw_os_error();
    let still_exists = ret <= 0 && errno == Some(libc::ESRCH) && process_still_exists(pid);
    classify_taskinfo(pid, ret, expected, (0, 0), errno, still_exists)
}

fn read_phys_footprint(pid: i32) -> ProbeOutcome<u64> {
    if pid <= 0 {
        return ProbeOutcome::Unreadable;
    }

    // The kernel writes a full `rusage_info_v2`, so the buffer must use libc's complete layout.
    let mut usage = MaybeUninit::<libc::rusage_info_v2>::zeroed();
    unsafe {
        *libc::__error() = 0;
    }
    let ret =
        unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V2, usage.as_mut_ptr().cast()) };
    if ret == 0 {
        let usage = unsafe { usage.assume_init() };
        return ProbeOutcome::Value(usage.ri_phys_footprint);
    }

    let errno = std::io::Error::last_os_error().raw_os_error();
    let still_exists = errno == Some(libc::ESRCH) && process_still_exists(pid);
    classify_rusage(pid, ret, 0, errno, still_exists)
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

pub(crate) struct MacCollector {
    system: System,
}

impl MacCollector {
    pub(crate) fn new() -> Self {
        Self {
            system: System::new(),
        }
    }

    pub(crate) fn take_snapshot(&mut self, kind: MemKind) -> Snapshot {
        self.system.refresh_memory();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );

        let kern_procs = read_kern_procs();
        self.build_snapshot(kern_procs, kind)
    }

    fn build_snapshot(&mut self, kern_procs: Vec<KernProc>, kind: MemKind) -> Snapshot {
        let mut seen_pids = HashSet::with_capacity(kern_procs.len());
        let mut procs = Vec::with_capacity(kern_procs.len());

        for kern_proc in kern_procs {
            if !seen_pids.insert(kern_proc.pid) {
                continue;
            }

            let (rss, vsz) = match read_task_memory(kern_proc.pid) {
                ProbeOutcome::Value((rss, vsz)) => (Some(rss), Some(vsz)),
                ProbeOutcome::Unreadable => (None, None),
                ProbeOutcome::Vanished => continue,
            };
            let footprint = if kind == MemKind::Footprint {
                match read_phys_footprint(kern_proc.pid) {
                    ProbeOutcome::Value(fp) => Some(fp),
                    ProbeOutcome::Unreadable => None,
                    ProbeOutcome::Vanished => continue,
                }
            } else {
                None
            };

            let comm = (kern_proc.pid > 0)
                .then(|| u32::try_from(kern_proc.pid).ok())
                .flatten()
                .and_then(|raw_pid| self.system.process(Pid::from_u32(raw_pid)))
                .map(|process| process.name().to_string_lossy().into_owned())
                .filter(|name| !name.is_empty())
                .unwrap_or(kern_proc.comm);

            procs.push(ProcRec {
                pid: kern_proc.pid,
                ppid: kern_proc.ppid,
                comm,
                mem: MemSet {
                    rss,
                    vsz,
                    footprint,
                    pss: None,
                    uss: None,
                    swap: None,
                },
            });
        }

        let total_mem = self.system.total_memory();
        let used_mem = self.system.used_memory().min(total_mem);
        Snapshot {
            procs,
            total_mem,
            used_mem,
        }
    }
}

pub(crate) fn take_snapshot(kind: MemKind) -> Snapshot {
    MacCollector::new().take_snapshot(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ProcTree;

    #[test]
    fn own_ppid_matches_getppid() {
        let snapshot = take_snapshot(MemKind::Footprint);
        let me = i32::try_from(std::process::id()).unwrap_or(i32::MAX);
        let rec = snapshot.procs.iter().find(|p| p.pid == me);
        assert_eq!(rec.map(|p| p.ppid), Some(unsafe { libc::getppid() }));
    }

    #[test]
    fn own_process_populates_rss_vsz_and_lazy_footprint() {
        let me = i32::try_from(std::process::id()).unwrap_or(i32::MAX);

        let rss_snap = take_snapshot(MemKind::Rss);
        let rss_me = rss_snap
            .procs
            .iter()
            .find(|p| p.pid == me)
            .expect("self process in RSS snapshot");
        assert!(rss_me.mem.rss.is_some_and(|b| b > 0));
        assert!(rss_me.mem.vsz.is_some_and(|b| b > 0));
        assert!(
            rss_me.mem.footprint.is_none(),
            "RSS snapshot must skip proc_pid_rusage"
        );

        let fp_snap = take_snapshot(MemKind::Footprint);
        let fp_me = fp_snap
            .procs
            .iter()
            .find(|p| p.pid == me)
            .expect("self process in Footprint snapshot");
        assert!(fp_me.mem.rss.is_some_and(|b| b > 0));
        assert!(fp_me.mem.vsz.is_some_and(|b| b > 0));
        assert!(fp_me.mem.footprint.is_some_and(|b| b > 0));
    }

    #[test]
    fn unprivileged_launchd_reports_unknown_rss_and_vsz() {
        if unsafe { libc::geteuid() } == 0 {
            return;
        }

        let snap = take_snapshot(MemKind::Rss);
        let launchd = snap
            .procs
            .iter()
            .find(|p| p.pid == 1)
            .expect("launchd (pid 1) must be present from KERN_PROC_ALL");
        assert!(launchd.mem.rss.is_none(), "expected None RSS on EPERM");
        assert!(launchd.mem.vsz.is_none(), "expected None VSZ on EPERM");

        let tree = ProcTree::build(snap);
        assert_eq!(tree.parent_pid(1), None);
        assert!(tree.child_pids(0).is_empty());
        for kind in [MemKind::Rss, MemKind::Vsz] {
            let rollup = tree.rollup(kind);
            if let Some(kernel_row) = rollup.get(&0) {
                assert_eq!(kernel_row.aggregated, 0);
            }
            let row = rollup.get(&1).expect("launchd rollup row");
            assert_eq!(row.own, 0);
            assert!(row.own_approximate, "{kind:?}: launchd own must be ~");
            assert!(
                row.aggregated_approximate,
                "{kind:?}: launchd rollup must be ~"
            );
        }
    }

    #[test]
    fn kernel_task_and_launchd_are_separate_roots_in_snapshot() {
        let snap = take_snapshot(MemKind::Footprint);
        let kernel_task = snap
            .procs
            .iter()
            .find(|p| p.pid == 0)
            .expect("kernel_task (pid 0) must be present from KERN_PROC_ALL");
        assert_eq!(kernel_task.ppid, 0);
        assert_eq!(
            (
                kernel_task.mem.rss,
                kernel_task.mem.vsz,
                kernel_task.mem.footprint
            ),
            (None, None, None)
        );
        assert!(snap.procs.iter().any(|p| p.pid == 1 && p.ppid == 0));

        let tree = ProcTree::build(snap);
        assert!(tree.root_children().contains(&0));
        assert!(tree.root_children().contains(&1));
        assert_eq!(tree.parent_pid(0), None);
        assert_eq!(tree.parent_pid(1), None);
        assert!(tree.child_pids(0).is_empty());

        for kind in [MemKind::Rss, MemKind::Vsz, MemKind::Footprint] {
            let rollup = tree.rollup(kind);
            assert_eq!(rollup[&0].own, 0);
            assert!(rollup[&0].own_approximate);
            assert_eq!(rollup[&0].aggregated, 0);
            assert_eq!(rollup[&0].descendant_count, 0);
        }
    }

    #[test]
    fn classify_probes_distinguish_reaped_zombie_eperm_and_nonpositive_pid() {
        let expected = std::mem::size_of::<libc::proc_taskinfo>();
        let ok_ret = libc::c_int::try_from(expected).expect("proc_taskinfo size fits c_int");

        // Non-positive PIDs (`kernel_task` pid 0, negative sentinels) always map to Unreadable (`None`).
        for bad_pid in [0, -1] {
            assert_eq!(
                classify_taskinfo(
                    bad_pid,
                    ok_ret,
                    expected,
                    (4096, 8192),
                    Some(libc::ESRCH),
                    true
                ),
                ProbeOutcome::Unreadable
            );
            assert_eq!(
                classify_rusage(bad_pid, 0, 4096, Some(libc::ESRCH), true),
                ProbeOutcome::Unreadable
            );
        }

        // Successful syscalls yield exact counters.
        assert_eq!(
            classify_taskinfo(42, ok_ret, expected, (4096, 8192), None, true),
            ProbeOutcome::Value((4096, 8192))
        );
        assert_eq!(
            classify_rusage(42, 0, 16384, None, true),
            ProbeOutcome::Value(16384)
        );

        // Reaped ESRCH (`process_still_exists == false`) -> Vanished (drop row).
        assert_eq!(
            classify_taskinfo(42, 0, expected, (0, 0), Some(libc::ESRCH), false),
            ProbeOutcome::Vanished
        );
        assert_eq!(
            classify_rusage(42, -1, 0, Some(libc::ESRCH), false),
            ProbeOutcome::Vanished
        );

        // Live zombie `SZOMB` (`ESRCH` while `process_still_exists == true`) -> exact `0`.
        assert_eq!(
            classify_taskinfo(42, 0, expected, (0, 0), Some(libc::ESRCH), true),
            ProbeOutcome::Value((0, 0))
        );
        assert_eq!(
            classify_rusage(42, -1, 0, Some(libc::ESRCH), true),
            ProbeOutcome::Value(0)
        );

        // EPERM and other errors / truncated reads -> Unreadable (`None` / `0 B~`).
        for exists in [true, false] {
            assert_eq!(
                classify_taskinfo(42, 0, expected, (0, 0), Some(libc::EPERM), exists),
                ProbeOutcome::Unreadable
            );
            assert_eq!(
                classify_rusage(42, -1, 0, Some(libc::EPERM), exists),
                ProbeOutcome::Unreadable
            );
            assert_eq!(
                classify_taskinfo(42, 0, expected, (0, 0), Some(libc::EINVAL), exists),
                ProbeOutcome::Unreadable
            );
            assert_eq!(
                classify_rusage(42, -1, 0, Some(libc::EINVAL), exists),
                ProbeOutcome::Unreadable
            );
        }
        assert_eq!(
            classify_taskinfo(42, 4, expected, (4096, 8192), Some(libc::ESRCH), false),
            ProbeOutcome::Unreadable
        );
    }

    #[test]
    fn zombie_child_reports_exact_zero_and_reaped_child_is_dropped() {
        use std::process::{Child, Command};
        use std::time::Duration;

        struct ReapGuard(Option<Child>);
        impl Drop for ReapGuard {
            fn drop(&mut self) {
                if let Some(mut child) = self.0.take() {
                    let _ = child.wait();
                }
            }
        }

        let me = i32::try_from(std::process::id()).expect("self pid fits i32");
        let child = Command::new("/usr/bin/true")
            .spawn()
            .expect("spawn /usr/bin/true");
        let child_pid = i32::try_from(child.id()).expect("child pid fits i32");
        let mut guard = ReapGuard(Some(child));

        // Wait until the child exits and tears down its Mach task (entering SZOMB) without reaping.
        let expected = std::mem::size_of::<libc::proc_taskinfo>();
        let buf_size = libc::c_int::try_from(expected).expect("proc_taskinfo size fits c_int");
        let mut became_zombie = false;
        for _ in 0..400 {
            let mut info = MaybeUninit::<libc::proc_taskinfo>::zeroed();
            let ret = unsafe {
                libc::proc_pidinfo(
                    child_pid,
                    libc::PROC_PIDTASKINFO,
                    0,
                    info.as_mut_ptr().cast(),
                    buf_size,
                )
            };
            if ret <= 0 {
                became_zombie = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(became_zombie, "child {child_pid} did not reach SZOMB");
        assert!(process_still_exists(child_pid));
        assert_eq!(read_task_memory(child_pid), ProbeOutcome::Value((0, 0)));
        assert_eq!(read_phys_footprint(child_pid), ProbeOutcome::Value(0));

        let pre_reap_kern_procs = read_kern_procs();
        assert!(pre_reap_kern_procs.iter().any(|p| p.pid == child_pid));

        for kind in [MemKind::Rss, MemKind::Footprint] {
            let snap = take_snapshot(kind);
            let rec = snap
                .procs
                .iter()
                .find(|p| p.pid == child_pid)
                .unwrap_or_else(|| panic!("{kind:?}: unreaped zombie {child_pid} must be present"));
            assert_eq!(rec.ppid, me);
            assert_eq!(rec.mem.rss, Some(0));
            assert_eq!(rec.mem.vsz, Some(0));
            if kind == MemKind::Footprint {
                assert_eq!(rec.mem.footprint, Some(0));
            }

            let tree = ProcTree::build(snap);
            let rollup = tree.rollup(kind);
            assert_eq!(rollup[&child_pid].own, 0);
            assert!(!rollup[&child_pid].own_approximate);
            assert!(
                !rollup[&me].aggregated_approximate,
                "{kind:?}: zombie child must not mark parent approximate"
            );
        }

        // Reap the zombie and verify direct probes + mid-scan snapshots drop it cleanly.
        let exit_status = guard
            .0
            .take()
            .expect("child still held")
            .wait()
            .expect("wait on child");
        assert!(exit_status.success());

        assert!(!process_still_exists(child_pid));
        assert_eq!(read_task_memory(child_pid), ProbeOutcome::Vanished);
        assert_eq!(read_phys_footprint(child_pid), ProbeOutcome::Vanished);

        let mut collector = MacCollector::new();
        collector.system.refresh_memory();
        for kind in [MemKind::Rss, MemKind::Footprint] {
            let snap = collector.build_snapshot(pre_reap_kern_procs.clone(), kind);
            assert!(
                snap.procs.iter().all(|p| p.pid != child_pid),
                "{kind:?}: reaped pid {child_pid} from stale KERN_PROC_ALL must be dropped"
            );
            let tree = ProcTree::build(snap);
            let rollup = tree.rollup(kind);
            assert!(
                !rollup[&me].aggregated_approximate,
                "{kind:?}: vanished child must not leave a 0 B~ phantom under parent"
            );
        }

        let fresh = take_snapshot(MemKind::Rss);
        assert!(fresh.procs.iter().all(|p| p.pid != child_pid));
    }

    #[test]
    fn mac_collector_reuse_across_kinds_produces_valid_snapshots() {
        let me = i32::try_from(std::process::id()).unwrap_or(i32::MAX);
        let mut collector = MacCollector::new();

        for (kind, expect_footprint) in [
            (MemKind::Rss, false),
            (MemKind::Footprint, true),
            (MemKind::Vsz, false),
        ] {
            let snap = collector.take_snapshot(kind);
            assert!(!snap.procs.is_empty(), "{kind:?}: empty snapshot");
            assert!(snap.total_mem > 0, "{kind:?}: total_mem");
            assert!(
                snap.used_mem > 0 && snap.used_mem <= snap.total_mem,
                "{kind:?}: used_mem {} / {}",
                snap.used_mem,
                snap.total_mem
            );
            let rec = snap
                .procs
                .iter()
                .find(|p| p.pid == me)
                .expect("own pid present on reused collector");
            assert!(!rec.comm.is_empty(), "{kind:?}: own comm empty");
            assert!(rec.mem.rss.is_some_and(|b| b > 0), "{kind:?}: own rss");
            assert!(rec.mem.vsz.is_some_and(|b| b > 0), "{kind:?}: own vsz");
            assert_eq!(
                rec.mem.footprint.is_some_and(|b| b > 0),
                expect_footprint,
                "{kind:?}: footprint presence mismatch"
            );
        }
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
        let snapshot = take_snapshot(MemKind::Footprint);
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
