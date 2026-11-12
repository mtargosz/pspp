//! Background snapshot collection so the UI loop never blocks on process-table I/O.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;

use crate::collect::{self, Snapshot};
use crate::model::MemKind;

/// Owns a worker thread that takes one snapshot per request, for the requested [`MemKind`].
///
/// At most one request is in flight: [`Collector::request`] is a no-op while the worker is busy,
/// so a slow machine never builds a backlog. Dropping the collector stops the worker.
pub struct Collector {
    requests: Sender<MemKind>,
    results: Receiver<(MemKind, Snapshot)>,
    in_flight: bool,
}

impl Collector {
    /// Starts a worker that collects real snapshots.
    pub fn spawn() -> Self {
        Self::spawn_with(collect::make_snapshot_source())
    }

    /// Starts a worker around an arbitrary snapshot source (used by tests).
    pub fn spawn_with<F>(mut source: F) -> Self
    where
        F: FnMut(MemKind) -> Snapshot + Send + 'static,
    {
        let (request_tx, request_rx) = mpsc::channel::<MemKind>();
        let (result_tx, result_rx) = mpsc::channel::<(MemKind, Snapshot)>();
        thread::spawn(move || {
            for kind in request_rx {
                if result_tx.send((kind, source(kind))).is_err() {
                    break;
                }
            }
        });
        Self {
            requests: request_tx,
            results: result_rx,
            in_flight: false,
        }
    }

    /// Asks for a snapshot; returns whether a new request was actually sent.
    pub fn request(&mut self, kind: MemKind) -> bool {
        if self.in_flight {
            return false;
        }
        if self.requests.send(kind).is_ok() {
            self.in_flight = true;
            true
        } else {
            false
        }
    }

    /// True while a requested snapshot has not arrived yet.
    pub fn is_busy(&self) -> bool {
        self.in_flight
    }

    /// Returns a finished snapshot (with the kind it was taken for), if one is ready.
    pub fn try_take(&mut self) -> Option<(MemKind, Snapshot)> {
        match self.results.try_recv() {
            Ok(result) => {
                self.in_flight = false;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.in_flight = false;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::collect::{MemSet, ProcRec};

    fn snapshot_with(pid: i32) -> Snapshot {
        Snapshot {
            procs: vec![ProcRec {
                pid,
                ppid: 0,
                comm: "t".into(),
                mem: MemSet::default(),
            }],
            total_mem: 1,
            used_mem: 0,
        }
    }

    fn wait_for(collector: &mut Collector) -> (MemKind, Snapshot) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(result) = collector.try_take() {
                return result;
            }
            assert!(Instant::now() < deadline, "worker never answered");
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn request_returns_immediately_and_result_arrives_later() {
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let mut collector = Collector::spawn_with(move |_| {
            thread::sleep(Duration::from_millis(80));
            let n = seen.fetch_add(1, Ordering::SeqCst);
            snapshot_with(i32::try_from(n).unwrap_or(0) + 1)
        });

        let started = Instant::now();
        assert!(collector.request(MemKind::Vsz));
        assert!(
            started.elapsed() < Duration::from_millis(50),
            "must not block"
        );
        assert!(collector.is_busy());
        assert!(collector.try_take().is_none());

        let (kind, snap) = wait_for(&mut collector);
        assert_eq!(kind, MemKind::Vsz);
        assert_eq!(snap.procs[0].pid, 1);
        assert!(!collector.is_busy());
    }

    #[test]
    fn only_one_request_is_in_flight() {
        let mut collector = Collector::spawn_with(|_| {
            thread::sleep(Duration::from_millis(40));
            snapshot_with(7)
        });
        assert!(collector.request(MemKind::Rss));
        assert!(
            !collector.request(MemKind::Rss),
            "second request is dropped"
        );
        wait_for(&mut collector);
        assert!(
            collector.request(MemKind::Rss),
            "free again after the result"
        );
        wait_for(&mut collector);
    }

    #[test]
    fn worker_receives_the_requested_kind() {
        let mut collector = Collector::spawn_with(|kind| snapshot_with(kind as i32 + 100));
        assert!(collector.request(MemKind::Rss));
        let (kind, _) = wait_for(&mut collector);
        assert_eq!(kind, MemKind::Rss);
        assert!(collector.request(MemKind::Vsz));
        let (kind, _) = wait_for(&mut collector);
        assert_eq!(kind, MemKind::Vsz);
    }
}
