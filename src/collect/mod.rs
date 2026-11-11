//! Cross-platform process and memory snapshot collection (macOS and Linux).

// Compiled in tests on every platform so the Linux parsers can be unit-tested on macOS.
#[cfg(any(target_os = "linux", test))]
mod linux;
mod worker;

pub use worker::Collector;

use crate::model::MemKind;
#[cfg(target_os = "macos")]
mod macos;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemSet {
    pub rss: u64,
    pub vsz: u64,
    pub footprint: Option<u64>,
    pub pss: Option<u64>,
    pub uss: Option<u64>,
    pub swap: Option<u64>,
    pub approximate: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcRec {
    pub pid: i32,
    pub ppid: i32,
    pub comm: String,
    pub mem: MemSet,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub procs: Vec<ProcRec>,
    pub total_mem: u64,
    /// System-wide memory in use (macOS: `used_memory()`; Linux: `MemTotal - MemAvailable`).
    pub used_mem: u64,
}

#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LinuxStatRec {
    pub(crate) pid: i32,
    pub(crate) ppid: i32,
    pub(crate) comm: String,
    pub(crate) vsz: u64,
    pub(crate) rss: u64,
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn parse_linux_stat_line(line: &str, page_size: u64) -> Option<LinuxStatRec> {
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    if close <= open {
        return None;
    }

    let pid = line[..open].trim().parse::<i32>().ok()?;
    let comm = line.get(open + 1..close)?.to_owned();
    let fields: Vec<&str> = line.get(close + 1..)?.split_whitespace().collect();
    if fields.len() <= 21 {
        return None;
    }

    let ppid = fields.get(1)?.parse::<i32>().ok()?;
    let vsz = fields.get(20)?.parse::<u64>().ok()?;
    let rss_pages = fields.get(21)?.parse::<i64>().ok()?;
    let rss_pages_u64 = u64::try_from(rss_pages).ok()?;
    let rss = rss_pages_u64.saturating_mul(page_size);

    Some(LinuxStatRec {
        pid,
        ppid,
        comm,
        vsz,
        rss,
    })
}

/// Takes one snapshot. `kind` is the memory metric the caller will display: expensive
/// per-process reads (Linux `smaps_rollup`) only happen for the kinds that need them, and the
/// remaining fields stay `None` without flagging the process as approximate.
#[cfg(target_os = "macos")]
pub fn take_snapshot(kind: MemKind) -> Snapshot {
    macos::take_snapshot(kind)
}

#[cfg(target_os = "linux")]
pub fn take_snapshot(kind: MemKind) -> Snapshot {
    linux::take_snapshot(kind)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn take_snapshot(_kind: MemKind) -> Snapshot {
    Snapshot {
        procs: Vec::new(),
        total_mem: 0,
        used_mem: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linux_stat_line_with_parens_in_name() {
        let line = "123 (a) (b) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 4096 77";
        let parsed = parse_linux_stat_line(line, 4096).expect("valid stat line should parse");

        assert_eq!(parsed.pid, 123);
        assert_eq!(parsed.ppid, 1);
        assert_eq!(parsed.comm, "a) (b");
        assert_eq!(parsed.vsz, 4096);
        assert_eq!(parsed.rss, 77 * 4096);
    }

    #[test]
    fn rejects_malformed_linux_stat_line() {
        assert!(parse_linux_stat_line("not-a-stat-line", 4096).is_none());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn snapshot_contains_self_process() {
        let snapshot = take_snapshot(MemKind::Rss);
        let self_pid = i32::try_from(std::process::id()).unwrap_or(i32::MAX);

        assert!(!snapshot.procs.is_empty());
        assert!(snapshot.total_mem > 0);
        assert!(snapshot.used_mem > 0 && snapshot.used_mem <= snapshot.total_mem);
        assert!(snapshot
            .procs
            .iter()
            .any(|proc_rec| proc_rec.pid == self_pid));
    }
}
