//! Non-interactive query execution, size parsing/formatting, and table/JSON rendering.

use std::cmp::Ordering;
use std::collections::HashSet;

use serde::Serialize;
use unicode_width::UnicodeWidthStr;

use crate::collect::Snapshot;
use crate::model::{AggMode, MemKind, ProcTree, SortBy};

#[derive(Clone, Debug)]
pub struct QueryOpts {
    pub pattern: Option<String>,
    pub pid: Option<i32>,
    pub mem_kind: MemKind,
    pub agg_mode: AggMode,
    pub min_bytes: Option<u64>,
    pub sort_by: SortBy,
    pub reverse: bool,
    pub top: Option<usize>,
    pub json: bool,
    pub base10: bool,
}

impl Default for QueryOpts {
    fn default() -> Self {
        Self {
            pattern: None,
            pid: None,
            mem_kind: MemKind::Rss,
            agg_mode: AggMode::Rollup,
            min_bytes: None,
            sort_by: SortBy::Aggregated,
            reverse: false,
            top: None,
            json: false,
            base10: false,
        }
    }
}

#[derive(Clone, Debug)]
struct RenderRow {
    pid: i32,
    ppid: i32,
    parent_comm: String,
    comm: String,
    own: u64,
    own_approximate: bool,
    agg: u64,
    approximate: bool,
    children: usize,
    descendants: usize,
}

#[derive(Debug, Serialize)]
struct JsonOutput {
    mem: String,
    agg: &'static str,
    rows: Vec<JsonRow>,
    total: u64,
    total_approximate: bool,
}

#[derive(Debug, Serialize)]
struct JsonRow {
    pid: i32,
    ppid: i32,
    comm: String,
    own: u64,
    own_approximate: bool,
    agg: u64,
    pct: f64,
    children: usize,
    descendants: usize,
    approximate: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Total {
    bytes: u64,
    approximate: bool,
}

#[derive(Debug)]
pub struct QueryResult {
    pub output: String,
    pub row_count: usize,
}

pub fn run_query(snapshot: Snapshot, opts: &QueryOpts) -> QueryResult {
    let tree = ProcTree::build(snapshot);
    let ram = tree.total_mem();
    let (rows, total) = matching_rows(&tree, opts);
    let row_count = rows.len();
    let output = if opts.json {
        render_json(rows, total, opts, ram)
    } else {
        render_plain(&rows, total, opts, ram)
    };
    QueryResult { output, row_count }
}

fn matching_rows(tree: &ProcTree, opts: &QueryOpts) -> (Vec<RenderRow>, Total) {
    let aggregates = tree.aggregate(opts.mem_kind, opts.agg_mode);
    let allowed = allowed_pid_set(tree, opts.pid);
    let lowered_pattern = opts.pattern.as_ref().map(|value| value.to_lowercase());

    let mut rows: Vec<RenderRow> = tree
        .procs()
        .filter(|proc_rec| allowed.contains(&proc_rec.pid))
        .filter(|proc_rec| pattern_matches(&proc_rec.comm, lowered_pattern.as_deref()))
        .filter_map(|proc_rec| {
            let aggregate = aggregates.get(&proc_rec.pid)?;
            if opts.min_bytes.is_some_and(|min| aggregate.aggregated < min) {
                return None;
            }

            let parent_comm = tree
                .proc(proc_rec.ppid)
                .map(|parent| parent.comm.clone())
                .unwrap_or_else(|| "-".to_owned());

            Some(RenderRow {
                pid: proc_rec.pid,
                ppid: proc_rec.ppid,
                parent_comm,
                comm: proc_rec.comm.clone(),
                own: aggregate.own,
                own_approximate: aggregate.own_approximate,
                agg: aggregate.aggregated,
                approximate: aggregate.aggregated_approximate,
                children: aggregate.child_count,
                descendants: aggregate.descendant_count,
            })
        })
        .collect();

    rows.sort_by(|left, right| compare_rows(left, right, opts.sort_by, opts.reverse));
    if let Some(top) = opts.top {
        rows.truncate(top);
    }
    let total = distinct_total(tree, &rows, opts.agg_mode);
    (rows, total)
}

/// Sums the returned rows without counting any process twice.
///
/// Rollup rows nest (an ancestor's Σ already contains its descendants), so only rows with no
/// returned ancestor contribute. Flat Σ covers direct children only, and each process has one
/// parent, so flat rows are already disjoint.
fn distinct_total(tree: &ProcTree, rows: &[RenderRow], agg_mode: AggMode) -> Total {
    let returned: HashSet<i32> = rows.iter().map(|row| row.pid).collect();
    let has_returned_ancestor = |pid: i32| {
        let mut current = tree.parent_pid(pid);
        while let Some(parent) = current {
            if returned.contains(&parent) {
                return true;
            }
            current = tree.parent_pid(parent);
        }
        false
    };

    rows.iter()
        .filter(|row| agg_mode == AggMode::Flat || !has_returned_ancestor(row.pid))
        .fold(Total::default(), |acc, row| Total {
            bytes: acc.bytes.saturating_add(row.agg),
            approximate: acc.approximate || row.approximate,
        })
}

fn allowed_pid_set(tree: &ProcTree, pid: Option<i32>) -> HashSet<i32> {
    match pid {
        None => tree.procs().map(|proc_rec| proc_rec.pid).collect(),
        Some(root_pid) => collect_subtree_pids(tree, root_pid),
    }
}

/// `root_pid` plus its descendants in `tree` (`tree.child_pids`), matching `ProcTree` semantics.
fn collect_subtree_pids(tree: &ProcTree, root_pid: i32) -> HashSet<i32> {
    if tree.proc(root_pid).is_none() {
        return HashSet::new();
    }

    let mut seen = HashSet::new();
    let mut stack = vec![root_pid];
    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        stack.extend(tree.child_pids(pid).iter().copied());
    }
    seen
}

fn pattern_matches(comm: &str, lowered_pattern: Option<&str>) -> bool {
    let Some(pattern) = lowered_pattern else {
        return true;
    };
    comm.to_lowercase().contains(pattern)
}

fn compare_rows(left: &RenderRow, right: &RenderRow, sort_by: SortBy, reverse: bool) -> Ordering {
    let primary = match sort_by {
        SortBy::Aggregated => right.agg.cmp(&left.agg),
        SortBy::Own => right.own.cmp(&left.own),
        SortBy::Pid => left.pid.cmp(&right.pid),
        SortBy::Comm => left.comm.to_lowercase().cmp(&right.comm.to_lowercase()),
    };
    let primary = if reverse { primary.reverse() } else { primary };
    primary.then_with(|| left.pid.cmp(&right.pid))
}

fn render_plain(rows: &[RenderRow], total: Total, opts: &QueryOpts, ram: u64) -> String {
    const COLS: usize = 7;
    // PID, PARENT, COMM are left-aligned; the numeric columns are right-aligned.
    const LEFT: [bool; COLS] = [true, true, true, false, false, false, false];

    let agg_header = format!("{} Σ ({})", opts.mem_kind.label(), agg_name(opts.agg_mode));
    let header: [String; COLS] = [
        "PID".to_owned(),
        "PARENT".to_owned(),
        "COMM".to_owned(),
        "OWN".to_owned(),
        agg_header,
        "%".to_owned(),
        "CHILDREN".to_owned(),
    ];

    let rendered_rows: Vec<[String; COLS]> = rows
        .iter()
        .map(|row| {
            let parent = if row.ppid == 0 {
                "-".to_owned()
            } else {
                format!("{}({})", row.ppid, row.parent_comm)
            };
            let own = format_size(row.own, opts.base10);
            let own = if row.own_approximate {
                format!("{own}~")
            } else {
                own
            };
            let mut agg = format_size(row.agg, opts.base10);
            if row.approximate {
                agg.push('~');
            }

            [
                row.pid.to_string(),
                parent,
                row.comm.clone(),
                own,
                agg,
                format_pct(row.agg, ram),
                row.children.to_string(),
            ]
        })
        .collect();

    let mut total_formatted = format_size(total.bytes, opts.base10);
    if total.approximate {
        total_formatted.push('~');
    }
    let total_row: [String; COLS] = [
        String::new(),
        String::new(),
        "TOTAL".to_owned(),
        String::new(),
        total_formatted,
        format_pct(total.bytes, ram),
        String::new(),
    ];

    let mut widths = [0_usize; COLS];
    for row in std::iter::once(&header)
        .chain(rendered_rows.iter())
        .chain(std::iter::once(&total_row))
    {
        for (col_width, cell) in widths.iter_mut().zip(row.iter()) {
            *col_width = (*col_width).max(UnicodeWidthStr::width(cell.as_str()));
        }
    }

    let format_line = |cells: &[String; COLS]| -> String {
        let line = cells
            .iter()
            .zip(widths.iter().copied())
            .zip(LEFT.iter().copied())
            .map(|((cell, col_width), left_align)| {
                let pad = col_width.saturating_sub(UnicodeWidthStr::width(cell.as_str()));
                if left_align {
                    format!("{cell}{}", " ".repeat(pad))
                } else {
                    format!("{}{cell}", " ".repeat(pad))
                }
            })
            .collect::<Vec<_>>()
            .join("  ");
        line.trim_end().to_owned()
    };

    let mut lines = Vec::with_capacity(rendered_rows.len().saturating_add(3));
    lines.push(format_line(&header));
    lines.push("-".repeat(widths.iter().sum::<usize>() + 2 * (COLS - 1)));
    lines.extend(rendered_rows.iter().map(&format_line));
    lines.push(format_line(&total_row));
    lines.join("\n")
}

fn agg_name(mode: AggMode) -> &'static str {
    match mode {
        AggMode::Rollup => "rollup",
        AggMode::Flat => "flat",
    }
}

fn render_json(rows: Vec<RenderRow>, total: Total, opts: &QueryOpts, ram: u64) -> String {
    let payload = JsonOutput {
        mem: opts.mem_kind.label().to_lowercase(),
        agg: agg_name(opts.agg_mode),
        rows: rows
            .into_iter()
            .map(|row| JsonRow {
                pid: row.pid,
                ppid: row.ppid,
                comm: row.comm,
                own: row.own,
                own_approximate: row.own_approximate,
                agg: row.agg,
                pct: pct_tenths(row.agg, ram) as f64 / 10.0,
                children: row.children,
                descendants: row.descendants,
                approximate: row.approximate,
            })
            .collect(),
        total: total.bytes,
        total_approximate: total.approximate,
    };

    match serde_json::to_string_pretty(&payload) {
        Ok(rendered) => rendered,
        Err(_) => format!(
            r#"{{"mem":"{}","agg":"{}","rows":[],"total":0,"total_approximate":false}}"#,
            opts.mem_kind.label().to_lowercase(),
            agg_name(opts.agg_mode)
        ),
    }
}

pub fn parse_size(value: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("size cannot be empty".to_owned());
    }

    let split_at = trimmed
        .find(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .unwrap_or(trimmed.len());
    let (num_part, suffix_part) = trimmed.split_at(split_at);
    if num_part.is_empty() {
        return Err(format!(
            "invalid size '{value}'; expected formats like 512K, 100M, 1.5G, or bytes"
        ));
    }

    let number = num_part
        .parse::<f64>()
        .map_err(|_| format!("invalid size '{value}'; '{num_part}' is not a valid number"))?;
    if !number.is_finite() || number.is_sign_negative() {
        return Err(format!(
            "invalid size '{value}'; number must be finite and positive"
        ));
    }

    let multiplier = match suffix_part.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1_f64,
        "k" | "kb" | "ki" | "kib" => 1024_f64,
        "m" | "mb" | "mi" | "mib" => 1024_f64.powi(2),
        "g" | "gb" | "gi" | "gib" => 1024_f64.powi(3),
        "t" | "tb" | "ti" | "tib" => 1024_f64.powi(4),
        "p" | "pb" | "pi" | "pib" => 1024_f64.powi(5),
        other => {
            return Err(format!(
                "invalid size suffix '{other}' in '{value}'; expected B, K, M, G, T, or P"
            ));
        }
    };

    let bytes = number * multiplier;
    if !bytes.is_finite() || bytes < 0.0 || bytes > u64::MAX as f64 {
        return Err(format!("size '{value}' is out of range"));
    }

    Ok(bytes.round() as u64)
}

/// `value / total` as a percentage with one decimal (`12.3%`); not capped at 100 because Σ of
/// per-process values can exceed physical RAM. Capped at `999.9%` for column width.
pub fn format_pct(value: u64, total: u64) -> String {
    let tenths = pct_tenths(value, total);
    format!("{}.{}%", tenths / 10, tenths % 10)
}

fn pct_tenths(value: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    let tenths = (u128::from(value) * 1000 + u128::from(total) / 2) / u128::from(total);
    u64::try_from(tenths.min(9999)).unwrap_or(9999)
}

pub fn format_size(bytes: u64, base10: bool) -> String {
    let (base, units) = if base10 {
        (
            1000_f64,
            ["B", "KB", "MB", "GB", "TB", "PB", "EB"].as_slice(),
        )
    } else {
        (
            1024_f64,
            ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"].as_slice(),
        )
    };

    if bytes < base as u64 {
        return format!("{bytes} B");
    }

    let mut value = bytes as f64;
    let mut idx = 0_usize;
    while idx + 1 < units.len() && (value * 10.0).round() / 10.0 >= base {
        value /= base;
        idx += 1;
    }

    format!("{value:.1} {}", units[idx])
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::{format_pct, format_size, parse_size, run_query, QueryOpts};
    use crate::collect::{MemSet, ProcRec, Snapshot};
    use crate::model::{AggMode, MemKind, SortBy};

    fn proc(pid: i32, ppid: i32, comm: &str, rss: u64) -> ProcRec {
        ProcRec {
            pid,
            ppid,
            comm: comm.to_owned(),
            mem: MemSet {
                rss: Some(rss),
                vsz: Some(rss.saturating_mul(2)),
                footprint: None,
                pss: None,
                uss: None,
                swap: None,
            },
        }
    }

    fn snapshot(procs: Vec<ProcRec>) -> Snapshot {
        Snapshot {
            procs,
            total_mem: 0,
            used_mem: 0,
        }
    }

    #[test]
    fn format_pct_rounds_to_one_decimal_and_does_not_cap_at_100() {
        assert_eq!(format_pct(0, 100), "0.0%");
        assert_eq!(format_pct(1, 3), "33.3%");
        assert_eq!(format_pct(1, 8), "12.5%");
        assert_eq!(format_pct(3, 2), "150.0%");
        assert_eq!(format_pct(5, 0), "0.0%");
        assert_eq!(format_pct(u64::MAX, 1), "999.9%");
    }

    #[test]
    fn plain_and_json_outputs_carry_pct_of_total_ram() {
        let mut snap = snapshot(vec![proc(1, 0, "init", 250), proc(2, 1, "child", 100)]);
        snap.total_mem = 1000;
        let plain = run_query(snap.clone(), &QueryOpts::default()).output;
        let header = plain.lines().next().unwrap_or_default();
        assert!(
            header.contains("RSS Σ (rollup)") && header.contains('%'),
            "{header}"
        );
        assert!(plain.contains("35.0%"), "{plain}");
        assert!(plain.contains("10.0%"), "{plain}");

        let json = run_query(
            snap.clone(),
            &QueryOpts {
                json: true,
                ..QueryOpts::default()
            },
        )
        .output;
        let parsed: Value = serde_json::from_str(&json).expect("json");
        assert_eq!(parsed["rows"][0]["pct"], 35.0);
        assert_eq!(parsed["rows"][1]["pct"], 10.0);
    }

    #[test]
    fn parse_size_accepts_expected_formats() {
        assert_eq!(parse_size("512K"), Ok(512 * 1024));
        assert_eq!(parse_size("1.5G"), Ok(1_610_612_736));
        assert_eq!(parse_size("42"), Ok(42));
    }

    #[test]
    fn parse_size_rejects_bad_inputs() {
        assert!(parse_size("abc").is_err());
        assert!(parse_size("1Q").is_err());
        assert!(parse_size("").is_err());
    }

    #[test]
    fn format_size_uses_binary_and_decimal_units() {
        assert_eq!(format_size(1536, false), "1.5 KiB");
        assert_eq!(format_size(1500, true), "1.5 KB");
    }

    #[test]
    fn format_size_promotes_unit_when_rounding_reaches_base() {
        assert_eq!(format_size(1_048_550, false), "1.0 MiB");
        assert_eq!(format_size(999_960, true), "1.0 MB");
        assert_eq!(format_size(1_048_000, false), "1023.4 KiB");
    }

    #[test]
    fn run_query_json_emits_rows_and_total() {
        let snap = snapshot(vec![
            proc(1, 0, "launchd", 10_000),
            proc(2, 1, "shell", 8_000),
            proc(3, 2, "cargo", 4_000),
        ]);
        let opts = QueryOpts {
            json: true,
            top: Some(2),
            ..QueryOpts::default()
        };

        let rendered = run_query(snap.clone(), &opts).output;
        let parsed: Value = serde_json::from_str(&rendered).expect("query json should parse");
        let rows = parsed["rows"].as_array().expect("rows should be array");
        let total = parsed["total"].as_u64().expect("total should be u64");

        assert_eq!(rows.len(), 2);
        assert!(total > 0);
    }

    #[test]
    fn run_query_plain_filters_by_pattern_and_pid_subtree() {
        let snap = snapshot(vec![
            proc(10, 0, "root", 100),
            proc(20, 10, "worker-alpha", 80),
            proc(21, 10, "worker-beta", 60),
            proc(30, 0, "other", 40),
        ]);
        let opts = QueryOpts {
            pattern: Some("worker".to_owned()),
            pid: Some(10),
            mem_kind: MemKind::Rss,
            agg_mode: AggMode::Rollup,
            min_bytes: None,
            sort_by: SortBy::Pid,
            reverse: false,
            top: None,
            json: false,
            base10: false,
        };

        let rendered = run_query(snap.clone(), &opts).output;
        assert!(rendered.contains("worker-alpha"));
        assert!(rendered.contains("worker-beta"));
        assert!(!rendered.contains("other"));
    }

    #[test]
    fn pattern_match_folds_unicode_case() {
        let snap = snapshot(vec![
            proc(1, 0, "CAFÉ-worker", 10),
            proc(2, 0, "école-daemon", 20),
        ]);

        let accent_lower = QueryOpts {
            pattern: Some("café".to_owned()),
            ..QueryOpts::default()
        };
        let rendered = run_query(snap.clone(), &accent_lower).output;
        assert!(rendered.contains("CAFÉ-worker"));
        assert!(!rendered.contains("école-daemon"));

        let accent_upper = QueryOpts {
            pattern: Some("ÉCOLE".to_owned()),
            ..QueryOpts::default()
        };
        let rendered = run_query(snap.clone(), &accent_upper).output;
        assert!(rendered.contains("école-daemon"));
        assert!(!rendered.contains("CAFÉ-worker"));
    }

    #[test]
    fn run_query_handles_empty_and_single_process_snapshots() {
        let json = QueryOpts {
            json: true,
            ..QueryOpts::default()
        };

        let empty: Value = serde_json::from_str(&run_query(snapshot(Vec::new()), &json).output)
            .expect("empty query json should parse");
        assert_eq!(empty["rows"].as_array().map(Vec::len), Some(0));
        assert_eq!(empty["total"].as_u64(), Some(0));

        let single: Value =
            serde_json::from_str(&run_query(snapshot(vec![proc(1, 0, "init", 500)]), &json).output)
                .expect("single query json should parse");
        let rows = single["rows"].as_array().expect("rows should be array");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["children"].as_u64(), Some(0));
        assert_eq!(rows[0]["agg"].as_u64(), Some(500));
        assert_eq!(single["total"].as_u64(), Some(500));
    }

    #[test]
    fn run_query_pid_subtree_terminates_on_cycle() {
        let snap = snapshot(vec![
            proc(6, 7, "cycle-a", 6),
            proc(7, 6, "cycle-b", 7),
            proc(8, 0, "outside", 8),
        ]);
        let opts = QueryOpts {
            pid: Some(6),
            json: true,
            ..QueryOpts::default()
        };

        let parsed: Value = serde_json::from_str(&run_query(snap.clone(), &opts).output)
            .expect("cycle query json should parse");
        let pids: Vec<i64> = parsed["rows"]
            .as_array()
            .expect("rows should be array")
            .iter()
            .filter_map(|row| row["pid"].as_i64())
            .collect();
        assert!(pids.contains(&6));
        assert!(pids.contains(&7));
        assert!(!pids.contains(&8));
    }

    #[test]
    fn run_query_unknown_pid_yields_no_rows() {
        let snap = snapshot(vec![proc(1, 0, "init", 100)]);
        let opts = QueryOpts {
            pid: Some(424_242),
            json: true,
            ..QueryOpts::default()
        };

        let parsed: Value = serde_json::from_str(&run_query(snap.clone(), &opts).output)
            .expect("json should parse");
        assert_eq!(parsed["rows"].as_array().map(Vec::len), Some(0));
        assert_eq!(parsed["total"].as_u64(), Some(0));
    }

    #[test]
    fn run_query_total_saturates_on_overflow() {
        let snap = snapshot(vec![
            proc(1, 0, "big-a", u64::MAX),
            proc(2, 0, "big-b", u64::MAX),
        ]);
        let opts = QueryOpts {
            json: true,
            ..QueryOpts::default()
        };

        let parsed: Value = serde_json::from_str(&run_query(snap.clone(), &opts).output)
            .expect("json should parse");
        assert_eq!(parsed["total"].as_u64(), Some(u64::MAX));
    }

    #[test]
    fn run_query_total_counts_each_process_once() {
        let snap = snapshot(vec![
            proc(1, 0, "launchd", 100),
            proc(2, 1, "shell", 10),
            proc(3, 2, "cargo", 1),
            proc(4, 0, "other", 1_000),
        ]);
        let total_for = |agg_mode, pattern: Option<&str>| {
            let opts = QueryOpts {
                json: true,
                agg_mode,
                pattern: pattern.map(str::to_owned),
                ..QueryOpts::default()
            };
            let result = run_query(snap.clone(), &opts);
            let parsed: Value = serde_json::from_str(&result.output).expect("json should parse");
            (result.row_count, parsed["total"].as_u64())
        };

        // Rollup rows nest: launchd's Σ (111) already covers shell and cargo.
        assert_eq!(total_for(AggMode::Rollup, None), (4, Some(1_111)));
        // Without the ancestor in the result, nested rows count on their own.
        assert_eq!(total_for(AggMode::Rollup, Some("cargo")), (1, Some(1)));
        assert_eq!(total_for(AggMode::Rollup, Some("l")), (2, Some(111)));
        // Flat Σ is direct children only, which never overlap across rows.
        assert_eq!(total_for(AggMode::Flat, None), (4, Some(11)));
    }

    #[test]
    fn parse_size_edge_inputs() {
        assert_eq!(parse_size("0"), Ok(0));
        assert_eq!(parse_size(".5G"), Ok(512 * 1024 * 1024));
        assert!(parse_size("-1").is_err());
        assert!(parse_size("1..5G").is_err());
        assert!(parse_size("1.5.5G").is_err());
        assert!(parse_size(".").is_err());
        assert!(parse_size("1e3").is_err());
        assert!(parse_size("inf").is_err());
        assert!(parse_size("99999999999999999999").is_err());
    }

    #[test]
    fn render_plain_aligns_cjk_and_emoji_process_names_by_display_width() {
        use unicode_width::UnicodeWidthStr;

        let mut snap = snapshot(vec![
            proc(1, 0, "日本語プロセス", 2048),
            proc(2, 1, "🚀worker", 1024),
            proc(3, 0, "bash", 512),
        ]);
        snap.total_mem = 10_240;

        let rendered = run_query(snap, &QueryOpts::default()).output;
        let lines: Vec<&str> = rendered.lines().collect();
        assert_eq!(lines.len(), 6, "{rendered}");

        // Separator line width equals the exact display width of every full data row.
        let sep_width = UnicodeWidthStr::width(lines[1]);
        for data_line in &lines[2..=4] {
            assert_eq!(
                UnicodeWidthStr::width(*data_line),
                sep_width,
                "misaligned row: {data_line}\n{rendered}"
            );
        }

        // Right edge of OWN, RSS Σ, %, and CHILDREN columns align across CJK, emoji, and ASCII rows.
        let pct_end_col = |line: &str| -> usize {
            let idx = line.find('%').expect("pct");
            UnicodeWidthStr::width(&line[..=idx])
        };
        assert_eq!(pct_end_col(lines[2]), pct_end_col(lines[3]));
        assert_eq!(pct_end_col(lines[2]), pct_end_col(lines[4]));
        assert_eq!(pct_end_col(lines[2]), pct_end_col(lines[5]));
    }

    #[test]
    fn reverse_option_reverses_all_sort_keys_with_deterministic_pid_tie_break() {
        // Roots:
        // pid 1: comm "beta",  own 10, child 2 (own 90) -> agg 100
        // pid 2: comm "alpha", own 90, leaf             -> agg 90
        // pid 3: comm "gamma", own 50, leaf             -> agg 50
        // pid 4: comm "alpha", own 50, leaf             -> agg 50  (ties own/agg with 3, comm with 2)
        let snap = snapshot(vec![
            proc(1, 0, "beta", 10),
            proc(2, 1, "alpha", 90),
            proc(3, 0, "gamma", 50),
            proc(4, 0, "alpha", 50),
        ]);

        let sorted_pids = |sort_by: SortBy, reverse: bool| -> Vec<i64> {
            let opts = QueryOpts {
                json: true,
                sort_by,
                reverse,
                ..QueryOpts::default()
            };
            let parsed: Value =
                serde_json::from_str(&run_query(snap.clone(), &opts).output).expect("json");
            parsed["rows"]
                .as_array()
                .expect("rows")
                .iter()
                .filter_map(|row| row["pid"].as_i64())
                .collect()
        };

        assert_eq!(sorted_pids(SortBy::Aggregated, false), vec![1, 2, 3, 4]);
        assert_eq!(sorted_pids(SortBy::Aggregated, true), vec![3, 4, 2, 1]);

        assert_eq!(sorted_pids(SortBy::Own, false), vec![2, 3, 4, 1]);
        assert_eq!(sorted_pids(SortBy::Own, true), vec![1, 3, 4, 2]);

        assert_eq!(sorted_pids(SortBy::Pid, false), vec![1, 2, 3, 4]);
        assert_eq!(sorted_pids(SortBy::Pid, true), vec![4, 3, 2, 1]);

        assert_eq!(sorted_pids(SortBy::Comm, false), vec![2, 4, 1, 3]);
        assert_eq!(sorted_pids(SortBy::Comm, true), vec![3, 1, 2, 4]);
    }

    #[test]
    fn collect_subtree_pids_matches_proctree_on_pid_zero_ppid_zero_and_cycles() {
        use std::collections::HashSet;

        let tree = super::ProcTree::build(snapshot(vec![
            proc(0, 0, "kernel_task", 0),
            proc(1, 0, "launchd", 10),
            proc(2, 1, "child", 20),
            proc(6, 7, "cycle-a", 6),
            proc(7, 6, "cycle-b", 7),
        ]));

        // pid 0 is an independent root; launchd (ppid == 0) must not be treated as its child.
        assert_eq!(super::collect_subtree_pids(&tree, 0), HashSet::from([0]));
        assert_eq!(super::collect_subtree_pids(&tree, 1), HashSet::from([1, 2]));
        // Cycle 6 <-> 7 is broken at min pid (6 is root, 7 is leaf child of 6).
        assert_eq!(super::collect_subtree_pids(&tree, 6), HashSet::from([6, 7]));
        assert_eq!(super::collect_subtree_pids(&tree, 7), HashSet::from([7]));
    }

    #[test]
    fn json_emits_own_approximate_and_total_approximate_including_empty_matches() {
        let unreadable = MemSet {
            rss: None,
            vsz: None,
            ..MemSet::default()
        };
        let snap = snapshot(vec![
            ProcRec {
                pid: 1,
                ppid: 0,
                comm: "launchd".into(),
                mem: unreadable,
            },
            proc(2, 1, "readable_child", 4096),
        ]);

        // Rollup: pid 1 has unreadable own (own_approximate = true) and approximate rollup;
        // pid 2 is exact (own_approximate = false, approximate = false); total_approximate = true.
        let rollup_json: Value = serde_json::from_str(
            &run_query(
                snap.clone(),
                &QueryOpts {
                    json: true,
                    agg_mode: AggMode::Rollup,
                    sort_by: SortBy::Pid,
                    ..QueryOpts::default()
                },
            )
            .output,
        )
        .expect("rollup json");
        assert_eq!(rollup_json["total_approximate"], Value::Bool(true));
        assert_eq!(rollup_json["rows"][0]["own_approximate"], Value::Bool(true));
        assert_eq!(rollup_json["rows"][0]["approximate"], Value::Bool(true));
        assert_eq!(
            rollup_json["rows"][1]["own_approximate"],
            Value::Bool(false)
        );
        assert_eq!(rollup_json["rows"][1]["approximate"], Value::Bool(false));

        // Flat: pid 1's own is approximate, but its direct child (pid 2) is exact, so
        // own_approximate = true while aggregated approximate = false and total_approximate = false.
        let flat_json: Value = serde_json::from_str(
            &run_query(
                snap.clone(),
                &QueryOpts {
                    json: true,
                    agg_mode: AggMode::Flat,
                    sort_by: SortBy::Pid,
                    ..QueryOpts::default()
                },
            )
            .output,
        )
        .expect("flat json");
        assert_eq!(flat_json["total_approximate"], Value::Bool(false));
        assert_eq!(flat_json["rows"][0]["own_approximate"], Value::Bool(true));
        assert_eq!(flat_json["rows"][0]["approximate"], Value::Bool(false));

        // Empty match: rows = [], total = 0, total_approximate = false.
        let unmatched_json: Value = serde_json::from_str(
            &run_query(
                snap,
                &QueryOpts {
                    json: true,
                    pattern: Some("no-such-proc".into()),
                    ..QueryOpts::default()
                },
            )
            .output,
        )
        .expect("unmatched json");
        assert_eq!(unmatched_json["rows"].as_array().map(Vec::len), Some(0));
        assert_eq!(unmatched_json["total"], 0);
        assert_eq!(unmatched_json["total_approximate"], Value::Bool(false));
    }
}
