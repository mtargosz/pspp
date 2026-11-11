//! Non-interactive query execution, size parsing/formatting, and table/JSON rendering.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::collect::{ProcRec, Snapshot};
use crate::model::{AggMode, MemKind, ProcTree, SortBy};

#[derive(Clone, Debug)]
pub struct QueryOpts {
    pub pattern: Option<String>,
    pub pid: Option<i32>,
    pub mem_kind: MemKind,
    pub agg_mode: AggMode,
    pub min_bytes: Option<u64>,
    pub sort_by: SortBy,
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
}

#[derive(Debug, Serialize)]
struct JsonRow {
    pid: i32,
    ppid: i32,
    comm: String,
    own: u64,
    agg: u64,
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

pub fn run_query(snapshot: &Snapshot, opts: &QueryOpts) -> QueryResult {
    let (rows, total) = matching_rows(snapshot, opts);
    let row_count = rows.len();
    let output = if opts.json {
        render_json(rows, total, opts)
    } else {
        render_plain(&rows, total, opts)
    };
    QueryResult { output, row_count }
}

fn matching_rows(snapshot: &Snapshot, opts: &QueryOpts) -> (Vec<RenderRow>, Total) {
    let tree = ProcTree::build(snapshot);
    let aggregates = tree.aggregate(opts.mem_kind, opts.agg_mode);
    let by_pid: HashMap<i32, &ProcRec> = snapshot
        .procs
        .iter()
        .map(|proc_rec| (proc_rec.pid, proc_rec))
        .collect();
    let allowed = allowed_pid_set(snapshot, opts.pid);
    let lowered_pattern = opts.pattern.as_ref().map(|value| value.to_lowercase());

    let mut rows: Vec<RenderRow> = snapshot
        .procs
        .iter()
        .filter(|proc_rec| allowed.contains(&proc_rec.pid))
        .filter(|proc_rec| pattern_matches(&proc_rec.comm, lowered_pattern.as_deref()))
        .filter_map(|proc_rec| {
            let aggregate = aggregates.get(&proc_rec.pid)?;
            if opts.min_bytes.is_some_and(|min| aggregate.aggregated < min) {
                return None;
            }

            let parent_comm = by_pid
                .get(&proc_rec.ppid)
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

    rows.sort_by(|left, right| compare_rows(left, right, opts.sort_by));
    if let Some(top) = opts.top {
        rows.truncate(top);
    }
    let total = distinct_total(&tree, &rows, opts.agg_mode);
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

fn allowed_pid_set(snapshot: &Snapshot, pid: Option<i32>) -> HashSet<i32> {
    match pid {
        None => snapshot.procs.iter().map(|proc_rec| proc_rec.pid).collect(),
        Some(root_pid) => collect_subtree_pids(snapshot, root_pid),
    }
}

fn collect_subtree_pids(snapshot: &Snapshot, root_pid: i32) -> HashSet<i32> {
    let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
    let mut known = HashSet::new();
    for proc_rec in &snapshot.procs {
        known.insert(proc_rec.pid);
        children
            .entry(proc_rec.ppid)
            .or_default()
            .push(proc_rec.pid);
    }

    if !known.contains(&root_pid) {
        return HashSet::new();
    }

    let mut seen = HashSet::new();
    let mut stack = vec![root_pid];

    while let Some(pid) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if let Some(child_pids) = children.get(&pid) {
            for child in child_pids {
                stack.push(*child);
            }
        }
    }

    seen
}

fn pattern_matches(comm: &str, lowered_pattern: Option<&str>) -> bool {
    let Some(pattern) = lowered_pattern else {
        return true;
    };
    comm.to_lowercase().contains(pattern)
}

fn compare_rows(left: &RenderRow, right: &RenderRow, sort_by: SortBy) -> Ordering {
    match sort_by {
        SortBy::Aggregated => right
            .agg
            .cmp(&left.agg)
            .then_with(|| left.pid.cmp(&right.pid)),
        SortBy::Own => right
            .own
            .cmp(&left.own)
            .then_with(|| left.pid.cmp(&right.pid)),
        SortBy::Pid => left.pid.cmp(&right.pid),
        SortBy::Comm => left
            .comm
            .to_lowercase()
            .cmp(&right.comm.to_lowercase())
            .then_with(|| left.pid.cmp(&right.pid)),
    }
}

fn render_plain(rows: &[RenderRow], total: Total, opts: &QueryOpts) -> String {
    let agg_header = format!("{} Σ ({})", opts.mem_kind.label(), agg_name(opts.agg_mode));
    let header = [
        "PID",
        "PARENT",
        "COMM",
        "OWN",
        agg_header.as_str(),
        "CHILDREN",
    ];

    let rendered_rows: Vec<[String; 6]> = rows
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
                row.children.to_string(),
            ]
        })
        .collect();

    let mut total_formatted = format_size(total.bytes, opts.base10);
    if total.approximate {
        total_formatted.push('~');
    }
    let total_row = [
        String::new(),
        String::new(),
        "TOTAL".to_owned(),
        String::new(),
        total_formatted,
        String::new(),
    ];

    let mut widths = [
        header[0].len(),
        header[1].len(),
        header[2].len(),
        header[3].len(),
        header[4].len(),
        header[5].len(),
    ];

    for row in &rendered_rows {
        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(cell.len());
        }
    }
    for (idx, cell) in total_row.iter().enumerate() {
        widths[idx] = widths[idx].max(cell.len());
    }

    let mut lines = Vec::with_capacity(rendered_rows.len().saturating_add(3));
    lines.push(format!(
        "{:<pid_w$}  {:<parent_w$}  {:<comm_w$}  {:>own_w$}  {:>agg_w$}  {:>children_w$}",
        header[0],
        header[1],
        header[2],
        header[3],
        header[4],
        header[5],
        pid_w = widths[0],
        parent_w = widths[1],
        comm_w = widths[2],
        own_w = widths[3],
        agg_w = widths[4],
        children_w = widths[5],
    ));
    let full_width = widths.iter().sum::<usize>() + 10;
    lines.push("-".repeat(full_width));

    for row in &rendered_rows {
        lines.push(format!(
            "{:<pid_w$}  {:<parent_w$}  {:<comm_w$}  {:>own_w$}  {:>agg_w$}  {:>children_w$}",
            row[0],
            row[1],
            row[2],
            row[3],
            row[4],
            row[5],
            pid_w = widths[0],
            parent_w = widths[1],
            comm_w = widths[2],
            own_w = widths[3],
            agg_w = widths[4],
            children_w = widths[5],
        ));
    }
    lines.push(format!(
        "{:<pid_w$}  {:<parent_w$}  {:<comm_w$}  {:>own_w$}  {:>agg_w$}  {:>children_w$}",
        total_row[0],
        total_row[1],
        total_row[2],
        total_row[3],
        total_row[4],
        total_row[5],
        pid_w = widths[0],
        parent_w = widths[1],
        comm_w = widths[2],
        own_w = widths[3],
        agg_w = widths[4],
        children_w = widths[5],
    ));

    lines.join("\n")
}

fn agg_name(mode: AggMode) -> &'static str {
    match mode {
        AggMode::Rollup => "rollup",
        AggMode::Flat => "flat",
    }
}

fn render_json(rows: Vec<RenderRow>, total: Total, opts: &QueryOpts) -> String {
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
                agg: row.agg,
                children: row.children,
                descendants: row.descendants,
                approximate: row.approximate,
            })
            .collect(),
        total: total.bytes,
    };

    match serde_json::to_string_pretty(&payload) {
        Ok(rendered) => rendered,
        Err(_) => format!(
            r#"{{"mem":"{}","agg":"{}","rows":[],"total":0}}"#,
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
    use std::time::Instant;

    use serde_json::Value;

    use super::{format_size, parse_size, run_query, QueryOpts};
    use crate::collect::{MemSet, ProcRec, Snapshot};
    use crate::model::{AggMode, MemKind, SortBy};

    fn proc(pid: i32, ppid: i32, comm: &str, rss: u64) -> ProcRec {
        ProcRec {
            pid,
            ppid,
            comm: comm.to_owned(),
            mem: MemSet {
                rss,
                vsz: rss.saturating_mul(2),
                footprint: None,
                pss: None,
                uss: None,
                swap: None,
                approximate: false,
            },
        }
    }

    fn snapshot(procs: Vec<ProcRec>) -> Snapshot {
        Snapshot {
            procs,
            total_mem: 0,
            taken_at: Instant::now(),
        }
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

        let rendered = run_query(&snap, &opts).output;
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
            top: None,
            json: false,
            base10: false,
        };

        let rendered = run_query(&snap, &opts).output;
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
        let rendered = run_query(&snap, &accent_lower).output;
        assert!(rendered.contains("CAFÉ-worker"));
        assert!(!rendered.contains("école-daemon"));

        let accent_upper = QueryOpts {
            pattern: Some("ÉCOLE".to_owned()),
            ..QueryOpts::default()
        };
        let rendered = run_query(&snap, &accent_upper).output;
        assert!(rendered.contains("école-daemon"));
        assert!(!rendered.contains("CAFÉ-worker"));
    }

    #[test]
    fn run_query_handles_empty_and_single_process_snapshots() {
        let json = QueryOpts {
            json: true,
            ..QueryOpts::default()
        };

        let empty: Value = serde_json::from_str(&run_query(&snapshot(Vec::new()), &json).output)
            .expect("empty query json should parse");
        assert_eq!(empty["rows"].as_array().map(Vec::len), Some(0));
        assert_eq!(empty["total"].as_u64(), Some(0));

        let single: Value = serde_json::from_str(
            &run_query(&snapshot(vec![proc(1, 0, "init", 500)]), &json).output,
        )
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

        let parsed: Value = serde_json::from_str(&run_query(&snap, &opts).output)
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

        let parsed: Value =
            serde_json::from_str(&run_query(&snap, &opts).output).expect("json should parse");
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

        let parsed: Value =
            serde_json::from_str(&run_query(&snap, &opts).output).expect("json should parse");
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
            let result = run_query(&snap, &opts);
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
}
