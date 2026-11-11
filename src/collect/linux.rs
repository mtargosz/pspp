// Compiled (for its parsers' tests) on every platform; only used at runtime on Linux.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::{fs, time::Instant};

use sysinfo::System;

use crate::collect::{parse_linux_stat_line, MemSet, ProcRec, Snapshot};

const PROC_DIR: &str = "/proc";

#[derive(Default)]
struct SmapsRollup {
    pss: Option<u64>,
    uss: Option<u64>,
    swap: Option<u64>,
}

fn parse_kib_field(line: &str, field: &str) -> Option<u64> {
    if !line.starts_with(field) {
        return None;
    }

    let kib = line
        .get(field.len()..)?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?;
    Some(kib.saturating_mul(1024))
}

fn parse_smaps_rollup(contents: &str) -> SmapsRollup {
    let mut pss = None;
    let mut private_clean = None;
    let mut private_dirty = None;
    let mut swap = None;

    for line in contents.lines() {
        if pss.is_none() {
            pss = parse_kib_field(line, "Pss:");
        }
        if private_clean.is_none() {
            private_clean = parse_kib_field(line, "Private_Clean:");
        }
        if private_dirty.is_none() {
            private_dirty = parse_kib_field(line, "Private_Dirty:");
        }
        if swap.is_none() {
            swap = parse_kib_field(line, "Swap:");
        }
    }

    let uss = match (private_clean, private_dirty) {
        (Some(clean), Some(dirty)) => Some(clean.saturating_add(dirty)),
        _ => None,
    };

    SmapsRollup { pss, uss, swap }
}

fn page_size_bytes() -> u64 {
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size > 0 {
        u64::try_from(page_size).unwrap_or(4096)
    } else {
        4096
    }
}

/// `(total, used)` in bytes from `/proc/meminfo` text; used = `MemTotal - MemAvailable`.
fn parse_meminfo(contents: &str) -> Option<(u64, u64)> {
    let mut total = None;
    let mut available = None;
    for line in contents.lines() {
        if total.is_none() {
            total = parse_kib_field(line, "MemTotal:");
        }
        if available.is_none() {
            available = parse_kib_field(line, "MemAvailable:");
        }
    }
    let total = total?;
    Some((total, total.saturating_sub(available?)))
}

/// `(total, used)` system memory in bytes, falling back to sysinfo.
fn system_memory() -> (u64, u64) {
    if let Some(parsed) = fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|contents| parse_meminfo(&contents))
    {
        return parsed;
    }

    let mut system = System::new();
    system.refresh_memory();
    let total = system.total_memory();
    (total, system.used_memory().min(total))
}

pub(crate) fn take_snapshot() -> Snapshot {
    let page_size = page_size_bytes();
    let mut procs = Vec::new();

    if let Ok(entries) = fs::read_dir(PROC_DIR) {
        for entry in entries.flatten() {
            let pid_name = entry.file_name();
            if pid_name.to_string_lossy().parse::<i32>().is_err() {
                continue;
            }

            let proc_dir = entry.path();
            let stat_line = match fs::read_to_string(proc_dir.join("stat")) {
                Ok(stat_line) => stat_line,
                Err(_) => continue,
            };

            let stat = match parse_linux_stat_line(&stat_line, page_size) {
                Some(stat) => stat,
                None => continue,
            };

            let rollup = fs::read_to_string(proc_dir.join("smaps_rollup"))
                .ok()
                .map(|content| parse_smaps_rollup(&content));

            let pss = rollup.as_ref().and_then(|value| value.pss);
            let uss = rollup.as_ref().and_then(|value| value.uss);
            let swap = rollup.as_ref().and_then(|value| value.swap);

            let approximate = rollup.is_none() || pss.is_none() || uss.is_none() || swap.is_none();

            procs.push(ProcRec {
                pid: stat.pid,
                ppid: stat.ppid,
                comm: stat.comm,
                mem: MemSet {
                    rss: stat.rss,
                    vsz: stat.vsz,
                    footprint: None,
                    pss,
                    uss,
                    swap,
                    approximate,
                },
            });
        }
    }

    let (total_mem, used_mem) = system_memory();
    Snapshot {
        procs,
        total_mem,
        used_mem,
        taken_at: Instant::now(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo_used_is_total_minus_available() {
        let text = "MemTotal:       32768 kB\nMemFree:         1000 kB\nMemAvailable:   12288 kB\n";
        assert_eq!(parse_meminfo(text), Some((32768 * 1024, 20480 * 1024)));
    }

    #[test]
    fn meminfo_without_available_is_rejected() {
        assert_eq!(parse_meminfo("MemTotal: 100 kB\n"), None);
        assert_eq!(parse_meminfo(""), None);
    }

    #[test]
    fn meminfo_available_above_total_saturates() {
        assert_eq!(
            parse_meminfo("MemTotal: 1 kB\nMemAvailable: 5 kB\n"),
            Some((1024, 0))
        );
    }
}
