use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use crate::collect::{ProcRec, Snapshot};
use crate::model::MemKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AggMode {
    Rollup,
    Flat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SortBy {
    #[default]
    Aggregated,
    Own,
    Pid,
    Comm,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AggregateRow {
    pub pid: i32,
    pub own: u64,
    pub own_approximate: bool,
    pub aggregated: u64,
    pub aggregated_approximate: bool,
    pub child_count: usize,
    pub descendant_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleRow {
    pub pid: i32,
    pub depth: usize,
    pub has_children: bool,
    pub is_last_child: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct FilterMatchSet {
    pub visible: HashSet<i32>,
    pub direct: HashSet<i32>,
}

#[derive(Clone, Debug)]
struct ProcNode {
    rec: ProcRec,
    parent: Option<i32>,
    children: Vec<i32>,
}

#[derive(Clone, Debug, Default)]
pub struct ProcTree {
    nodes: HashMap<i32, ProcNode>,
    ordered_pids: Vec<i32>,
    root_children: Vec<i32>,
    total_mem: u64,
    used_mem: u64,
}

impl ProcTree {
    /// Builds the forest, taking ownership of the snapshot's process records (no copies).
    /// Duplicate pids keep the first record.
    pub fn build(snapshot: Snapshot) -> ProcTree {
        let Snapshot {
            procs,
            total_mem,
            used_mem,
        } = snapshot;
        let mut nodes = HashMap::with_capacity(procs.len());
        for rec in procs {
            if let std::collections::hash_map::Entry::Vacant(slot) = nodes.entry(rec.pid) {
                slot.insert(ProcNode {
                    rec,
                    parent: None,
                    children: Vec::new(),
                });
            }
        }

        let mut ordered_pids: Vec<i32> = nodes.keys().copied().collect();
        ordered_pids.sort_unstable();

        let pid_set: HashSet<i32> = ordered_pids.iter().copied().collect();
        let mut parents: HashMap<i32, Option<i32>> = HashMap::with_capacity(ordered_pids.len());
        for pid in &ordered_pids {
            let ppid = nodes.get(pid).map(|node| node.rec.ppid).unwrap_or_default();
            let parent = if *pid == 0 || ppid == *pid || !pid_set.contains(&ppid) {
                None
            } else {
                Some(ppid)
            };
            parents.insert(*pid, parent);
        }

        Self::break_cycles(&ordered_pids, &mut parents);

        let mut children_map: HashMap<i32, Vec<i32>> = HashMap::new();
        for pid in &ordered_pids {
            let parent = parents.get(pid).copied().flatten();
            if let Some(node) = nodes.get_mut(pid) {
                node.parent = parent;
            }
            if let Some(ppid) = parent {
                children_map.entry(ppid).or_default().push(*pid);
            }
        }

        for pid in &ordered_pids {
            if let Some(node) = nodes.get_mut(pid) {
                let mut children = children_map.remove(pid).unwrap_or_default();
                children.sort_unstable();
                node.children = children;
            }
        }

        let mut root_children: Vec<i32> = ordered_pids
            .iter()
            .copied()
            .filter(|pid| parents.get(pid).copied().flatten().is_none())
            .collect();
        root_children.sort_unstable();

        ProcTree {
            nodes,
            ordered_pids,
            root_children,
            total_mem,
            used_mem,
        }
    }

    /// Every process record, in ascending pid order.
    pub fn procs(&self) -> impl Iterator<Item = &ProcRec> {
        self.ordered_pids
            .iter()
            .filter_map(|pid| self.nodes.get(pid).map(|node| &node.rec))
    }

    pub fn proc_count(&self) -> usize {
        self.nodes.len()
    }

    /// Largest pid present (0 when empty); used to size the PID column.
    pub fn max_pid(&self) -> i32 {
        self.ordered_pids.last().copied().unwrap_or(0)
    }

    /// Total physical memory at snapshot time, in bytes.
    pub fn total_mem(&self) -> u64 {
        self.total_mem
    }

    /// System memory in use at snapshot time, in bytes.
    pub fn used_mem(&self) -> u64 {
        self.used_mem
    }

    pub fn root_children(&self) -> &[i32] {
        &self.root_children
    }

    pub fn proc(&self, pid: i32) -> Option<&ProcRec> {
        self.nodes.get(&pid).map(|node| &node.rec)
    }

    pub fn parent_pid(&self, pid: i32) -> Option<i32> {
        self.nodes.get(&pid).and_then(|node| node.parent)
    }

    pub fn child_pids(&self, pid: i32) -> &[i32] {
        self.nodes
            .get(&pid)
            .map(|node| node.children.as_slice())
            .unwrap_or(&[])
    }

    pub fn aggregate(&self, kind: MemKind, mode: AggMode) -> HashMap<i32, AggregateRow> {
        match mode {
            AggMode::Rollup => self.rollup(kind),
            AggMode::Flat => self.flat(kind),
        }
    }

    pub fn rollup(&self, kind: MemKind) -> HashMap<i32, AggregateRow> {
        let mut rows: HashMap<i32, AggregateRow> = HashMap::with_capacity(self.nodes.len());
        let postorder = self.postorder();

        for pid in postorder {
            let Some(node) = self.nodes.get(&pid) else {
                continue;
            };

            let (own, own_approximate) = Self::value_for(kind, node);
            let mut aggregated = own;
            let mut aggregated_approximate = own_approximate;
            let mut descendant_count = 0_usize;

            for child in &node.children {
                let Some(child_row) = rows.get(child) else {
                    aggregated_approximate = true;
                    descendant_count = descendant_count.saturating_add(1);
                    continue;
                };

                aggregated = aggregated.saturating_add(child_row.aggregated);
                aggregated_approximate |= child_row.aggregated_approximate;
                descendant_count =
                    descendant_count.saturating_add(child_row.descendant_count.saturating_add(1));
            }

            rows.insert(
                pid,
                AggregateRow {
                    pid,
                    own,
                    own_approximate,
                    aggregated,
                    aggregated_approximate,
                    child_count: node.children.len(),
                    descendant_count,
                },
            );
        }

        rows
    }

    /// Flat mode sums direct children only.
    /// The parent's own memory is not included in `aggregated` and remains in `own`.
    pub fn flat(&self, kind: MemKind) -> HashMap<i32, AggregateRow> {
        let mut rows = HashMap::with_capacity(self.nodes.len());
        let descendant_counts = self.descendant_counts();

        for pid in &self.ordered_pids {
            let Some(node) = self.nodes.get(pid) else {
                continue;
            };

            let (own, own_approximate) = Self::value_for(kind, node);
            let mut aggregated = 0_u64;
            let mut aggregated_approximate = false;

            if !node.children.is_empty() {
                for child in &node.children {
                    let Some(child_node) = self.nodes.get(child) else {
                        aggregated_approximate = true;
                        continue;
                    };

                    let (child_own, child_own_approximate) = Self::value_for(kind, child_node);
                    aggregated = aggregated.saturating_add(child_own);
                    aggregated_approximate |= child_own_approximate;
                }
            }

            rows.insert(
                *pid,
                AggregateRow {
                    pid: *pid,
                    own,
                    own_approximate,
                    aggregated,
                    aggregated_approximate,
                    child_count: node.children.len(),
                    descendant_count: *descendant_counts.get(pid).unwrap_or(&0),
                },
            );
        }

        rows
    }

    pub fn sorted_pids(&self, rows: &HashMap<i32, AggregateRow>, sort_by: SortBy) -> Vec<i32> {
        let mut pids = self.ordered_pids.clone();
        match sort_by {
            SortBy::Aggregated => {
                pids.sort_by(|a, b| Self::cmp_desc(rows.get(a), rows.get(b), |row| row.aggregated))
            }
            SortBy::Own => {
                pids.sort_by(|a, b| Self::cmp_desc(rows.get(a), rows.get(b), |row| row.own))
            }
            SortBy::Pid => pids.sort(),
            SortBy::Comm => pids.sort_by(|a, b| {
                let a_comm = self
                    .nodes
                    .get(a)
                    .map(|node| node.rec.comm.as_str())
                    .unwrap_or_default();
                let b_comm = self
                    .nodes
                    .get(b)
                    .map(|node| node.rec.comm.as_str())
                    .unwrap_or_default();
                a_comm.cmp(b_comm)
            }),
        }
        pids
    }

    pub fn flatten_visible(&self, expanded: &HashSet<i32>) -> Vec<VisibleRow> {
        let mut rows = Vec::with_capacity(self.nodes.len());
        let mut visited = HashSet::with_capacity(self.nodes.len());
        let mut stack: Vec<(i32, usize, bool)> = Vec::new();

        for (idx, pid) in self.root_children.iter().enumerate().rev() {
            stack.push((*pid, 0, idx == self.root_children.len().saturating_sub(1)));
        }

        while let Some((pid, depth, is_last_child)) = stack.pop() {
            if !visited.insert(pid) {
                continue;
            }

            let Some(node) = self.nodes.get(&pid) else {
                continue;
            };

            let has_children = !node.children.is_empty();
            rows.push(VisibleRow {
                pid,
                depth,
                has_children,
                is_last_child,
            });

            if has_children && expanded.contains(&pid) {
                for (idx, child) in node.children.iter().enumerate().rev() {
                    stack.push((
                        *child,
                        depth.saturating_add(1),
                        idx == node.children.len().saturating_sub(1),
                    ));
                }
            }
        }

        rows
    }

    pub fn flatten_visible_sorted(
        &self,
        expanded: &HashSet<i32>,
        rows: &HashMap<i32, AggregateRow>,
        sort_by: SortBy,
        descending: bool,
    ) -> Vec<VisibleRow> {
        if sort_by == SortBy::Pid && !descending {
            return self.flatten_visible(expanded);
        }

        let mut flattened = Vec::with_capacity(self.nodes.len());
        let mut visited = HashSet::with_capacity(self.nodes.len());
        let mut stack: Vec<(i32, usize, bool)> = Vec::new();

        let roots = self.sorted_pid_list(&self.root_children, rows, sort_by, descending);
        for (idx, pid) in roots.iter().enumerate().rev() {
            stack.push((*pid, 0, idx == roots.len().saturating_sub(1)));
        }

        while let Some((pid, depth, is_last_child)) = stack.pop() {
            if !visited.insert(pid) {
                continue;
            }

            let Some(node) = self.nodes.get(&pid) else {
                continue;
            };

            let has_children = !node.children.is_empty();
            flattened.push(VisibleRow {
                pid,
                depth,
                has_children,
                is_last_child,
            });

            if has_children && expanded.contains(&pid) {
                let children = self.sorted_pid_list(&node.children, rows, sort_by, descending);
                for (idx, child) in children.iter().enumerate().rev() {
                    stack.push((
                        *child,
                        depth.saturating_add(1),
                        idx == children.len().saturating_sub(1),
                    ));
                }
            }
        }

        flattened
    }

    pub fn filter_match_set(&self, filter: &str) -> FilterMatchSet {
        if filter.is_empty() {
            let all: HashSet<i32> = self.ordered_pids.iter().copied().collect();
            return FilterMatchSet {
                visible: all.clone(),
                direct: all,
            };
        }

        let needle = filter.to_lowercase();
        let mut direct = HashSet::new();
        for pid in &self.ordered_pids {
            let matches = self
                .nodes
                .get(pid)
                .map(|node| node.rec.comm.to_lowercase().contains(&needle))
                .unwrap_or(false);
            if matches {
                direct.insert(*pid);
            }
        }

        let mut visible = HashSet::new();
        for pid in self.postorder() {
            if direct.contains(&pid) {
                visible.insert(pid);
                continue;
            }

            let has_matching_descendant = self
                .nodes
                .get(&pid)
                .map(|node| node.children.iter().any(|child| visible.contains(child)))
                .unwrap_or(false);
            if has_matching_descendant {
                visible.insert(pid);
            }
        }

        FilterMatchSet { visible, direct }
    }

    fn break_cycles(ordered_pids: &[i32], parents: &mut HashMap<i32, Option<i32>>) {
        let mut state: HashMap<i32, u8> = ordered_pids.iter().map(|pid| (*pid, 0_u8)).collect();

        for start in ordered_pids {
            if state.get(start).copied().unwrap_or(2) != 0 {
                continue;
            }

            let mut path = Vec::new();
            let mut path_index = HashMap::new();
            let mut current = *start;

            loop {
                if state.get(&current).copied().unwrap_or(2) == 2 {
                    break;
                }

                if let Some(cycle_start) = path_index.get(&current).copied() {
                    if let Some(cycle_break_pid) = path[cycle_start..].iter().min().copied() {
                        parents.insert(cycle_break_pid, None);
                    }
                    break;
                }

                path_index.insert(current, path.len());
                path.push(current);
                state.insert(current, 1);

                let Some(next) = parents.get(&current).copied().flatten() else {
                    break;
                };
                if !state.contains_key(&next) {
                    break;
                }
                current = next;
            }

            for pid in path {
                state.insert(pid, 2);
            }
        }
    }

    fn postorder(&self) -> Vec<i32> {
        let mut order = Vec::with_capacity(self.nodes.len());
        let mut seen = HashSet::with_capacity(self.nodes.len());
        let mut stack: Vec<(i32, bool)> = Vec::new();

        for pid in self.root_children.iter().rev() {
            stack.push((*pid, false));
        }

        while let Some((pid, expanded)) = stack.pop() {
            if expanded {
                order.push(pid);
                continue;
            }
            if !seen.insert(pid) {
                continue;
            }

            stack.push((pid, true));
            if let Some(node) = self.nodes.get(&pid) {
                for child in node.children.iter().rev() {
                    if seen.contains(child) {
                        continue;
                    }
                    stack.push((*child, false));
                }
            }
        }

        for pid in &self.ordered_pids {
            if seen.contains(pid) {
                continue;
            }
            stack.push((*pid, false));
            while let Some((walk_pid, expanded)) = stack.pop() {
                if expanded {
                    order.push(walk_pid);
                    continue;
                }
                if !seen.insert(walk_pid) {
                    continue;
                }
                stack.push((walk_pid, true));
                if let Some(node) = self.nodes.get(&walk_pid) {
                    for child in node.children.iter().rev() {
                        if seen.contains(child) {
                            continue;
                        }
                        stack.push((*child, false));
                    }
                }
            }
        }

        order
    }

    fn descendant_counts(&self) -> HashMap<i32, usize> {
        let mut counts: HashMap<i32, usize> = HashMap::with_capacity(self.nodes.len());
        for pid in self.postorder() {
            let mut total = 0_usize;
            if let Some(node) = self.nodes.get(&pid) {
                for child in &node.children {
                    let child_desc = counts.get(child).copied().unwrap_or(0);
                    total = total.saturating_add(child_desc.saturating_add(1));
                }
            }
            counts.insert(pid, total);
        }
        counts
    }

    fn value_for(kind: MemKind, node: &ProcNode) -> (u64, bool) {
        match kind.get(&node.rec.mem) {
            Some(value) => (value, false),
            None => (0, true),
        }
    }

    fn cmp_desc(
        a: Option<&AggregateRow>,
        b: Option<&AggregateRow>,
        value: impl Fn(&AggregateRow) -> u64,
    ) -> Ordering {
        let a_val = a.map(&value).unwrap_or(0);
        let b_val = b.map(value).unwrap_or(0);
        b_val.cmp(&a_val)
    }

    fn sorted_pid_list(
        &self,
        pids: &[i32],
        rows: &HashMap<i32, AggregateRow>,
        sort_by: SortBy,
        descending: bool,
    ) -> Vec<i32> {
        let mut sorted = pids.to_vec();
        if sort_by == SortBy::Comm {
            // Lowercase each name once instead of twice per comparison.
            sorted.sort_by_cached_key(|pid| {
                let comm = self.nodes.get(pid).map(|node| node.rec.comm.to_lowercase());
                (comm.unwrap_or_default(), *pid)
            });
            if descending {
                sorted.reverse();
            }
            return sorted;
        }
        sorted.sort_by(|a, b| self.compare_pid_for_sort(*a, *b, rows, sort_by, descending));
        sorted
    }

    fn compare_pid_for_sort(
        &self,
        left: i32,
        right: i32,
        rows: &HashMap<i32, AggregateRow>,
        sort_by: SortBy,
        descending: bool,
    ) -> Ordering {
        let compare = match sort_by {
            SortBy::Aggregated => rows
                .get(&left)
                .map(|row| row.aggregated)
                .unwrap_or_default()
                .cmp(
                    &rows
                        .get(&right)
                        .map(|row| row.aggregated)
                        .unwrap_or_default(),
                ),
            SortBy::Own => rows
                .get(&left)
                .map(|row| row.own)
                .unwrap_or_default()
                .cmp(&rows.get(&right).map(|row| row.own).unwrap_or_default()),
            SortBy::Pid => left.cmp(&right),
            SortBy::Comm => self
                .nodes
                .get(&left)
                .map(|node| node.rec.comm.to_lowercase())
                .unwrap_or_default()
                .cmp(
                    &self
                        .nodes
                        .get(&right)
                        .map(|node| node.rec.comm.to_lowercase())
                        .unwrap_or_default(),
                ),
        };

        let compare = compare.then_with(|| left.cmp(&right));
        if descending {
            compare.reverse()
        } else {
            compare
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::collect::{MemSet, ProcRec, Snapshot};
    use crate::model::{AggMode, MemKind, ProcTree, SortBy};

    fn mem(rss: u64) -> MemSet {
        MemSet {
            rss,
            vsz: rss,
            footprint: None,
            pss: None,
            uss: None,
            swap: None,
            approximate: false,
        }
    }

    fn proc(pid: i32, ppid: i32, comm: &str, rss: u64) -> ProcRec {
        ProcRec {
            pid,
            ppid,
            comm: comm.to_owned(),
            mem: mem(rss),
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
    fn deep_chain_rollup_and_flat_match_requirements() {
        let tree = ProcTree::build(snapshot(vec![
            proc(1, 0, "a", 10),
            proc(2, 1, "b", 20),
            proc(3, 2, "c", 30),
            proc(4, 3, "d", 40),
        ]));

        let rollup = tree.rollup(MemKind::Rss);
        let flat = tree.flat(MemKind::Rss);
        let via_mode = tree.aggregate(MemKind::Rss, AggMode::Rollup);

        assert_eq!(rollup.get(&1).map(|row| row.aggregated), Some(100));
        assert_eq!(rollup.get(&1).map(|row| row.descendant_count), Some(3));
        assert_eq!(flat.get(&1).map(|row| row.aggregated), Some(20));
        assert_eq!(flat.get(&1).map(|row| row.own), Some(10));
        assert_eq!(rollup.get(&1), via_mode.get(&1));
    }

    #[test]
    fn build_attaches_orphans_self_links_and_cycles_to_root_safely() {
        let tree = ProcTree::build(snapshot(vec![
            proc(0, 1, "real-zero", 9),
            proc(1, 0, "root", 1),
            proc(2, 1, "child-a", 2),
            proc(3, 1, "child-b", 3),
            proc(4, 9999, "orphan", 4),
            proc(5, 5, "self-link", 5),
            proc(6, 7, "cycle-a", 6),
            proc(7, 6, "cycle-b", 7),
        ]));

        assert_eq!(tree.root_children(), &[0, 4, 5, 6]);
        assert_eq!(
            tree.nodes
                .get(&1)
                .map(|node| node.children.clone())
                .unwrap_or_default(),
            vec![2, 3]
        );
        assert_eq!(
            tree.nodes
                .get(&6)
                .map(|node| node.children.clone())
                .unwrap_or_default(),
            vec![7]
        );
        assert_eq!(
            tree.flatten_visible(&HashSet::from([0, 1, 6]))
                .iter()
                .map(|row| row.pid)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3, 4, 5, 6, 7]
        );
    }

    #[test]
    fn rollup_uses_saturating_add() {
        let tree = ProcTree::build(snapshot(vec![
            proc(1, 0, "a", u64::MAX),
            proc(2, 1, "b", 10),
        ]));
        let rollup = tree.rollup(MemKind::Rss);
        assert_eq!(rollup.get(&1).map(|row| row.aggregated), Some(u64::MAX));
    }

    #[test]
    fn missing_values_mark_aggregate_as_approximate() {
        let mut root = proc(1, 0, "a", 10);
        root.mem.pss = Some(42);
        let mut child = proc(2, 1, "b", 20);
        child.mem.pss = None;

        let tree = ProcTree::build(snapshot(vec![root, child]));
        let rollup = tree.rollup(MemKind::Pss);

        assert_eq!(rollup.get(&1).map(|row| row.aggregated), Some(42));
        assert_eq!(
            rollup.get(&1).map(|row| row.aggregated_approximate),
            Some(true)
        );
    }

    #[test]
    fn flatten_visible_respects_expanded_state() {
        let tree = ProcTree::build(snapshot(vec![
            proc(1, 0, "a", 1),
            proc(2, 1, "b", 2),
            proc(3, 1, "c", 3),
            proc(4, 2, "d", 4),
        ]));

        let collapsed = tree.flatten_visible(&HashSet::new());
        assert_eq!(
            collapsed.iter().map(|row| row.pid).collect::<Vec<_>>(),
            vec![1]
        );

        let expanded_root = tree.flatten_visible(&HashSet::from([1]));
        assert_eq!(
            expanded_root.iter().map(|row| row.pid).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );

        let expanded_two_levels = tree.flatten_visible(&HashSet::from([1, 2]));
        assert_eq!(
            expanded_two_levels
                .iter()
                .map(|row| (row.pid, row.depth, row.is_last_child))
                .collect::<Vec<_>>(),
            vec![(1, 0, true), (2, 1, false), (4, 2, true), (3, 1, true)]
        );
    }

    #[test]
    fn sorting_helpers_are_stable() {
        let tree = ProcTree::build(snapshot(vec![
            proc(1, 0, "z", 5),
            proc(2, 0, "a", 5),
            proc(3, 0, "b", 1),
        ]));
        let rows = tree.rollup(MemKind::Rss);

        assert_eq!(tree.sorted_pids(&rows, SortBy::Aggregated), vec![1, 2, 3]);
        assert_eq!(tree.sorted_pids(&rows, SortBy::Own), vec![1, 2, 3]);
        assert_eq!(tree.sorted_pids(&rows, SortBy::Pid), vec![1, 2, 3]);
        assert_eq!(tree.sorted_pids(&rows, SortBy::Comm), vec![2, 3, 1]);
    }

    #[test]
    fn tree_owns_snapshot_records_and_exposes_system_memory() {
        let mut snap = snapshot(vec![
            proc(30, 1, "c", 3),
            proc(1, 0, "a", 1),
            proc(30, 1, "dup", 99),
            proc(7, 1, "b", 2),
        ]);
        snap.total_mem = 1000;
        snap.used_mem = 400;
        let tree = ProcTree::build(snap);

        let pids: Vec<i32> = tree.procs().map(|p| p.pid).collect();
        assert_eq!(pids, vec![1, 7, 30], "ascending, duplicates dropped");
        assert_eq!(
            tree.proc(30).map(|p| p.comm.as_str()),
            Some("c"),
            "first record wins"
        );
        assert_eq!(tree.proc_count(), 3);
        assert_eq!(tree.max_pid(), 30);
        assert_eq!((tree.total_mem(), tree.used_mem()), (1000, 400));
        assert_eq!(ProcTree::default().max_pid(), 0);
    }

    #[test]
    fn filter_match_set_keeps_matching_leaf_ancestors() {
        let tree = ProcTree::build(snapshot(vec![
            proc(1, 0, "root", 1),
            proc(2, 1, "middle", 1),
            proc(3, 2, "TargetLeaf", 1),
        ]));

        let matches = tree.filter_match_set("target");
        assert_eq!(matches.direct, HashSet::from([3]));
        assert_eq!(matches.visible, HashSet::from([1, 2, 3]));
    }

    #[test]
    fn filter_match_set_returns_empty_when_nothing_matches() {
        let tree = ProcTree::build(snapshot(vec![
            proc(1, 0, "alpha", 1),
            proc(2, 1, "beta", 1),
        ]));
        let matches = tree.filter_match_set("zzz");
        assert!(matches.direct.is_empty());
        assert!(matches.visible.is_empty());
    }

    #[test]
    fn filter_match_set_with_empty_filter_returns_all_rows() {
        let tree = ProcTree::build(snapshot(vec![
            proc(10, 0, "one", 1),
            proc(11, 10, "two", 1),
            proc(12, 11, "three", 1),
        ]));
        let matches = tree.filter_match_set("");
        assert_eq!(matches.visible, HashSet::from([10, 11, 12]));
    }

    #[test]
    fn filter_match_set_folds_unicode_case() {
        let tree = ProcTree::build(snapshot(vec![
            proc(1, 0, "CAFÉ", 1),
            proc(2, 1, "école", 1),
            proc(3, 0, "other", 1),
        ]));
        let accent = tree.filter_match_set("café");
        assert_eq!(accent.direct, HashSet::from([1]));
        assert_eq!(accent.visible, HashSet::from([1]));

        let upper = tree.filter_match_set("ÉCOLE");
        assert_eq!(upper.direct, HashSet::from([2]));
        assert_eq!(upper.visible, HashSet::from([1, 2]));
    }

    #[test]
    fn single_process_is_its_own_root_without_children() {
        let tree = ProcTree::build(snapshot(vec![proc(1, 0, "init", 42)]));
        assert_eq!(tree.root_children(), &[1]);
        let rollup = tree.rollup(MemKind::Rss);
        let row = rollup.get(&1).expect("row for lone process");
        assert_eq!((row.own, row.aggregated, row.descendant_count), (42, 42, 0));
        let flat = tree.flat(MemKind::Rss);
        assert_eq!(flat.get(&1).map(|row| row.aggregated), Some(0));
    }

    #[test]
    fn comm_sort_is_case_insensitive_with_pid_tiebreak_in_both_directions() {
        let tree = ProcTree::build(snapshot(vec![
            proc(5, 0, "beta", 1),
            proc(3, 0, "Alpha", 1),
            proc(4, 0, "alpha", 1),
            proc(1, 0, "Gamma", 1),
        ]));
        let rows = tree.aggregate(MemKind::Rss, AggMode::Rollup);
        let order = |descending| -> Vec<i32> {
            tree.flatten_visible_sorted(&HashSet::new(), &rows, SortBy::Comm, descending)
                .iter()
                .map(|row| row.pid)
                .collect()
        };
        assert_eq!(order(false), vec![3, 4, 5, 1]);
        assert_eq!(order(true), vec![1, 5, 4, 3]);
    }
}
