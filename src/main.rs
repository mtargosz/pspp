mod collect;

fn main() {
    let snapshot = collect::take_snapshot();
    let total_rss = snapshot
        .procs
        .iter()
        .fold(0_u64, |acc, proc_rec| acc.saturating_add(proc_rec.mem.rss));
    let _taken_at = snapshot.taken_at;

    println!(
        "processes: {}, total_rss: {} bytes, total_mem: {} bytes",
        snapshot.procs.len(),
        total_rss,
        snapshot.total_mem
    );
}
