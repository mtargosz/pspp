// Compiled (for its parsers' tests) on every platform; only used at runtime on Linux.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::fs;

use sysinfo::System;

use crate::collect::{parse_linux_stat_line, MemSet, ProcRec, Snapshot};
use crate::model::MemKind;

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

/// Name from `/proc/<pid>/cmdline`: the basename of argv[0] (comm is truncated to 15 bytes).
/// Returns `None` for empty cmdlines (kernel threads) so callers fall back to `comm`.
fn parse_cmdline_name(raw: &[u8]) -> Option<String> {
    let argv0 = raw.split(|byte| *byte == 0).next()?;
    let argv0 = String::from_utf8_lossy(argv0);
    let argv0 = argv0.trim();
    if argv0.is_empty() {
        return None;
    }
    // Only a path-like first token is reduced to its basename; titles such as
    // "nginx: worker process" are kept whole.
    let head = argv0.split(' ').next().unwrap_or(argv0);
    let name = if head.contains('/') {
        head.rsplit('/').next().unwrap_or(head)
    } else {
        argv0
    };
    (!name.is_empty()).then(|| name.to_owned())
}

/// Whether the selected metric needs the (expensive) `smaps_rollup` read.
fn needs_smaps(kind: MemKind) -> bool {
    matches!(kind, MemKind::Pss | MemKind::Uss | MemKind::Swap)
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

pub(crate) fn take_snapshot(kind: MemKind) -> Snapshot {
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

            let rollup = if needs_smaps(kind) {
                fs::read_to_string(proc_dir.join("smaps_rollup"))
                    .ok()
                    .map(|content| parse_smaps_rollup(&content))
            } else {
                None
            };

            let pss = rollup.as_ref().and_then(|value| value.pss);
            let uss = rollup.as_ref().and_then(|value| value.uss);
            let swap = rollup.as_ref().and_then(|value| value.swap);

            let comm = fs::read(proc_dir.join("cmdline"))
                .ok()
                .and_then(|raw| parse_cmdline_name(&raw))
                .unwrap_or(stat.comm);

            procs.push(ProcRec {
                pid: stat.pid,
                ppid: stat.ppid,
                comm,
                mem: MemSet {
                    rss: Some(stat.rss),
                    vsz: Some(stat.vsz),
                    footprint: None,
                    pss,
                    uss,
                    swap,
                },
            });
        }
    }

    let (total_mem, used_mem) = system_memory();
    Snapshot {
        procs,
        total_mem,
        used_mem,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMAPS: &str = "\
Rss:                1000 kB
Pss:                 600 kB
Pss_Dirty:            50 kB
Pss_Anon:            111 kB
Pss_File:            222 kB
Pss_Shmem:           333 kB
Shared_Clean:        100 kB
Shared_Dirty:         10 kB
Private_Clean:       200 kB
Private_Dirty:       300 kB
Swap:                 40 kB
SwapPss:              25 kB
";

    #[test]
    fn smaps_rollup_happy_path() {
        let parsed = parse_smaps_rollup(SMAPS);
        assert_eq!(parsed.pss, Some(600 * 1024));
        assert_eq!(parsed.uss, Some((200 + 300) * 1024));
        assert_eq!(parsed.swap, Some(40 * 1024));
    }

    #[test]
    fn smaps_prefixed_fields_do_not_match_pss_or_swap() {
        // Only the prefixed fields are present: they must not be mistaken for Pss:/Swap:.
        let only_prefixed = "Pss_Anon: 111 kB\nPss_File: 222 kB\nSwapPss: 25 kB\n\
                             Private_Clean: 1 kB\nPrivate_Dirty: 2 kB\n";
        let parsed = parse_smaps_rollup(only_prefixed);
        assert_eq!(parsed.pss, None);
        assert_eq!(parsed.swap, None);
        assert_eq!(parsed.uss, Some(3 * 1024));
    }

    #[test]
    fn smaps_missing_private_fields_leave_uss_unknown() {
        let parsed = parse_smaps_rollup("Pss: 10 kB\nPrivate_Clean: 5 kB\nSwap: 0 kB\n");
        assert_eq!(parsed.pss, Some(10 * 1024));
        assert_eq!(parsed.swap, Some(0));
        assert_eq!(parsed.uss, None, "Private_Dirty is missing");
        let empty = parse_smaps_rollup("");
        assert_eq!((empty.pss, empty.uss, empty.swap), (None, None, None));
        let junk = parse_smaps_rollup("Pss: lots kB\nSwap:\nPrivate_Clean 3 kB\n");
        assert_eq!((junk.pss, junk.uss, junk.swap), (None, None, None));
    }

    #[test]
    fn cmdline_name_uses_argv0_basename() {
        let name = |raw: &[u8]| parse_cmdline_name(raw);
        assert_eq!(
            name(b"/usr/lib/systemd/systemd-journald\0--flag\0").as_deref(),
            Some("systemd-journald")
        );
        assert_eq!(
            name(b"nginx: worker process\0").as_deref(),
            Some("nginx: worker process")
        );
        assert_eq!(
            name(b"/usr/bin/python3 -m http.server\0").as_deref(),
            Some("python3")
        );
        assert_eq!(name(b"bash\0").as_deref(), Some("bash"));
        assert_eq!(name(b""), None, "kernel threads have no cmdline");
        assert_eq!(name(b"\0\0"), None);
        assert_eq!(name(b"/usr/bin/\0"), None);
        assert_eq!(
            name(&[0xff, 0xfe, b'x']).as_deref(),
            Some("\u{fffd}\u{fffd}x")
        );
    }

    #[test]
    fn skipped_smaps_is_not_approximate_for_rss_vsz_but_missing_smaps_fields_are() {
        use crate::model::ProcTree;

        let build_mem = |rollup: Option<SmapsRollup>| MemSet {
            rss: Some(4096),
            vsz: Some(8192),
            footprint: None,
            pss: rollup.as_ref().and_then(|r| r.pss),
            uss: rollup.as_ref().and_then(|r| r.uss),
            swap: rollup.as_ref().and_then(|r| r.swap),
        };
        let check_own_approx = |mem: MemSet, kind: MemKind| -> bool {
            let tree = ProcTree::build(Snapshot {
                procs: vec![ProcRec {
                    pid: 1,
                    ppid: 0,
                    comm: "t".into(),
                    mem,
                }],
                total_mem: 0,
                used_mem: 0,
            });
            tree.rollup(kind)[&1].own_approximate
        };

        // RSS/VSZ skip smaps_rollup (`None`), yet RSS and VSZ remain exact (`Some(...)`).
        assert!(!check_own_approx(build_mem(None), MemKind::Rss));
        assert!(!check_own_approx(build_mem(None), MemKind::Vsz));

        // Full smaps_rollup satisfies Pss, Uss, and Swap without approximation.
        assert!(!check_own_approx(
            build_mem(Some(parse_smaps_rollup(SMAPS))),
            MemKind::Pss
        ));
        assert!(!check_own_approx(
            build_mem(Some(parse_smaps_rollup(SMAPS))),
            MemKind::Uss
        ));
        assert!(!check_own_approx(
            build_mem(Some(parse_smaps_rollup(SMAPS))),
            MemKind::Swap
        ));

        // Unreadable or partial smaps_rollup marks only the missing smaps metrics approximate.
        assert!(check_own_approx(build_mem(None), MemKind::Pss));
        let partial = parse_smaps_rollup("Pss: 1 kB\n");
        assert!(!check_own_approx(
            build_mem(Some(SmapsRollup { ..partial })),
            MemKind::Pss
        ));
        assert!(check_own_approx(build_mem(Some(partial)), MemKind::Uss));
    }

    #[test]
    fn only_smaps_kinds_request_the_expensive_read() {
        assert!(!needs_smaps(MemKind::Rss) && !needs_smaps(MemKind::Vsz));
        assert!(!needs_smaps(MemKind::Footprint));
        assert!(
            needs_smaps(MemKind::Pss) && needs_smaps(MemKind::Uss) && needs_smaps(MemKind::Swap)
        );
    }

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
