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

fn total_mem_bytes() -> u64 {
    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        for line in meminfo.lines() {
            if let Some(total) = parse_kib_field(line, "MemTotal:") {
                return total;
            }
        }
    }

    let mut system = System::new();
    system.refresh_memory();
    system.total_memory()
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

    Snapshot {
        procs,
        total_mem: total_mem_bytes(),
        taken_at: Instant::now(),
    }
}
