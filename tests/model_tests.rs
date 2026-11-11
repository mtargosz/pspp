//! Model-layer tests driven by the JSON fixtures in `tests/fixtures/`.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use pspp::collect::{MemSet, ProcRec, Snapshot};
use pspp::model::{MemKind, ProcTree};
use serde_json::Value;

/// Loads `tests/fixtures/<name>.json` into a `Snapshot` (rss/vsz/footprint per process).
fn load(name: &str) -> Snapshot {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.json"));
    let text = fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap_or_else(|err| panic!("{name}: {err}"));

    let int = |value: &Value, key: &str| {
        value[key]
            .as_i64()
            .unwrap_or_else(|| panic!("{name}: {key}"))
    };
    let procs = doc["procs"]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: procs array"))
        .iter()
        .map(|p| ProcRec {
            pid: i32::try_from(int(p, "pid")).expect("pid fits i32"),
            ppid: i32::try_from(int(p, "ppid")).expect("ppid fits i32"),
            comm: p["comm"].as_str().unwrap_or("?").to_owned(),
            mem: MemSet {
                rss: u64::try_from(int(p, "rss")).expect("rss"),
                vsz: u64::try_from(int(p, "vsz")).expect("vsz"),
                footprint: p["footprint"].as_u64(),
                ..MemSet::default()
            },
        })
        .collect();
    Snapshot {
        procs,
        total_mem: doc["total_mem"].as_u64().unwrap_or(0),
        used_mem: doc["used_mem"].as_u64().unwrap_or(0),
    }
}

const ALL: [&str; 6] = [
    "deep_chain",
    "fan_out",
    "orphans",
    "pid_reuse",
    "cycle",
    "duplicate_pid",
];

fn rss_total(tree: &ProcTree) -> u64 {
    tree.procs().map(|p| p.mem.rss).sum()
}

fn root_set(tree: &ProcTree) -> HashSet<i32> {
    tree.root_children().iter().copied().collect()
}

#[test]
fn every_fixture_counts_each_process_exactly_once_in_rollup() {
    for name in ALL {
        let tree = ProcTree::build(load(name));
        let rollup = tree.rollup(MemKind::Rss);
        let from_roots: u64 = tree
            .root_children()
            .iter()
            .map(|pid| rollup[pid].aggregated)
            .sum();
        assert_eq!(
            from_roots,
            rss_total(&tree),
            "{name}: roots' Σ must equal Σ of all processes"
        );
        assert_eq!(
            rollup.len(),
            tree.proc_count(),
            "{name}: one row per process"
        );
    }
}

#[test]
fn every_fixture_flat_sums_cover_each_non_root_exactly_once() {
    for name in ALL {
        let tree = ProcTree::build(load(name));
        let flat = tree.flat(MemKind::Rss);
        let roots = root_set(&tree);
        let non_root_own: u64 = tree
            .procs()
            .filter(|p| !roots.contains(&p.pid))
            .map(|p| p.mem.rss)
            .sum();
        let flat_total: u64 = flat.values().map(|row| row.aggregated).sum();
        assert_eq!(
            flat_total, non_root_own,
            "{name}: flat Σ is the sum over direct children"
        );
    }
}

#[test]
fn deep_chain_rolls_up_cumulatively_and_flat_is_one_level() {
    let tree = ProcTree::build(load("deep_chain"));
    assert_eq!(root_set(&tree), HashSet::from([1]));

    let rollup = tree.rollup(MemKind::Rss);
    let expected = [(1, 50, 4), (2, 40, 3), (3, 30, 2), (4, 20, 1), (5, 10, 0)];
    for (pid, aggregated, descendants) in expected {
        assert_eq!(rollup[&pid].aggregated, aggregated, "rollup pid {pid}");
        assert_eq!(
            rollup[&pid].descendant_count, descendants,
            "descendants pid {pid}"
        );
    }

    let flat = tree.flat(MemKind::Rss);
    for pid in 1..=4 {
        assert_eq!(flat[&pid].aggregated, 10, "flat pid {pid} = its one child");
        assert_eq!(flat[&pid].own, 10);
    }
    assert_eq!(flat[&5].aggregated, 0);
}

#[test]
fn fan_out_parent_sums_all_children_in_both_modes() {
    let tree = ProcTree::build(load("fan_out"));
    assert_eq!(tree.child_pids(1), &[2, 3, 4, 5, 6]);
    assert_eq!(tree.rollup(MemKind::Rss)[&1].aggregated, 5 + 15);
    let flat = tree.flat(MemKind::Rss);
    assert_eq!(
        flat[&1].aggregated, 15,
        "flat excludes the parent's own memory"
    );
    assert_eq!(flat[&1].own, 5);
    assert_eq!(flat[&1].child_count, 5);
}

#[test]
fn orphans_missing_parents_and_self_links_become_roots() {
    let tree = ProcTree::build(load("orphans"));
    assert_eq!(root_set(&tree), HashSet::from([10, 20, 30]));
    assert_eq!(tree.child_pids(20), &[21]);
    let rollup = tree.rollup(MemKind::Rss);
    assert_eq!(rollup[&20].aggregated, 6);
    assert_eq!(rollup[&30].aggregated, 8, "self-link is not its own child");
    assert_eq!(rollup[&30].descendant_count, 0);
}

#[test]
fn pid_reuse_links_follow_the_recorded_ppid() {
    let tree = ProcTree::build(load("pid_reuse"));
    // The snapshot cannot tell that pid 100 is a newer process, so its "child" attaches to it.
    assert_eq!(tree.parent_pid(200), Some(100));
    assert_eq!(tree.rollup(MemKind::Rss)[&100].aggregated, 2 + 4);
    // A parent that no longer exists leaves the child as a root rather than dropping it.
    assert_eq!(tree.parent_pid(300), None);
    assert_eq!(root_set(&tree), HashSet::from([1, 300]));
}

#[test]
fn cycle_is_broken_without_losing_or_duplicating_processes() {
    let tree = ProcTree::build(load("cycle"));
    assert_eq!(tree.proc_count(), 3);
    let roots = root_set(&tree);
    assert!(roots.contains(&1));
    assert_eq!(
        roots.len(),
        2,
        "exactly one member of the 2<->3 cycle becomes a root: {roots:?}"
    );

    let rollup = tree.rollup(MemKind::Rss);
    let from_roots: u64 = roots.iter().map(|pid| rollup[pid].aggregated).sum();
    assert_eq!(from_roots, 1 + 2 + 4);
    // Traversal terminates and every process is reachable from the roots exactly once.
    let visible = tree.flatten_visible(&tree.procs().map(|p| p.pid).collect());
    assert_eq!(visible.len(), 3);
}

#[test]
fn duplicate_pid_keeps_the_first_record() {
    let tree = ProcTree::build(load("duplicate_pid"));
    assert_eq!(tree.proc_count(), 4 - 1);
    assert_eq!(tree.proc(5).map(|p| p.comm.as_str()), Some("first"));
    let rollup = tree.rollup(MemKind::Rss);
    assert_eq!(rollup[&5].aggregated, 10 + 20);
    assert_eq!(rollup[&1].aggregated, 1 + 10 + 20);
}

#[test]
fn fixtures_expose_system_memory_figures() {
    let tree = ProcTree::build(load("deep_chain"));
    assert_eq!((tree.total_mem(), tree.used_mem()), (1_000_000, 400_000));
}
