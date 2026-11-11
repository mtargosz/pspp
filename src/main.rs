mod collect;
pub mod model;

use model::{MemKind, ProcTree, SortBy};

fn main() {
    let snapshot = collect::take_snapshot();
    let tree = ProcTree::build(&snapshot);
    let rollup = tree.rollup(MemKind::Rss);
    let sorted = tree.sorted_pids(&rollup, SortBy::Aggregated);

    println!(
        "processes: {}, total_mem: {} bytes, top rollup parents by rss:",
        snapshot.procs.len(),
        snapshot.total_mem
    );
    for pid in sorted.into_iter().take(5) {
        let Some(proc_rec) = tree.proc(pid) else {
            continue;
        };
        let Some(row) = rollup.get(&pid) else {
            continue;
        };
        println!(
            "pid={} comm={} own={} agg={} children={}",
            pid, proc_rec.comm, row.own, row.aggregated, row.child_count
        );
    }
}
