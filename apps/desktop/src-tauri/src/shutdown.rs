//! Host intent is distinct from lane cleanup: a bounded wait can fail while
//! cleanup is still in flight. Admission never reopens after the first Quit.
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Phase {
    Running,
    Attempting,
    Incomplete,
    Complete,
}
pub struct Shutdown(AtomicU8);
impl Default for Shutdown {
    fn default() -> Self {
        Self(AtomicU8::new(Phase::Running as u8))
    }
}
impl Shutdown {
    pub fn phase(&self) -> Phase {
        match self.0.load(Ordering::Acquire) {
            0 => Phase::Running,
            1 => Phase::Attempting,
            2 => Phase::Incomplete,
            _ => Phase::Complete,
        }
    }
    pub fn begin(&self) -> bool {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |phase| {
                (phase == Phase::Running as u8 || phase == Phase::Incomplete as u8)
                    .then_some(Phase::Attempting as u8)
            })
            .is_ok()
    }
    /// Returns true exactly once, authorizing exit only on acknowledged cleanup.
    pub fn finish(&self, cleaned: bool) -> bool {
        self.0
            .compare_exchange(
                Phase::Attempting as u8,
                if cleaned {
                    Phase::Complete
                } else {
                    Phase::Incomplete
                } as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
            && cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use persistence::lane::{BlockingLane, LaneError, ShutdownPhase};
    use std::sync::{Arc, atomic::AtomicUsize, mpsc};
    use std::time::Duration;

    fn quit(shutdown: &Shutdown, lane: &BlockingLane<usize>, exits: &AtomicUsize) {
        if shutdown.begin() {
            lane.stop_accepting().unwrap();
            if shutdown.finish(lane.close().is_ok()) {
                exits.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
    #[test]
    fn failed_quit_retains_service_and_retry_exits_exactly_once() {
        let lane = BlockingLane::new_with_shutdown(0, 1, |attempts| {
            *attempts += 1;
            if *attempts == 1 {
                Err(LaneError::CleanupFailed)
            } else {
                Ok(())
            }
        })
        .unwrap();
        let shutdown = Shutdown::default();
        let exits = AtomicUsize::new(0);
        quit(&shutdown, &lane, &exits);
        assert_eq!(shutdown.phase(), Phase::Incomplete);
        assert_eq!(exits.load(Ordering::SeqCst), 0);
        assert_eq!(
            lane.shutdown_status().unwrap().phase,
            ShutdownPhase::Incomplete
        );
        assert!(matches!(lane.submit(|_| ()), Err(LaneError::Closed)));
        quit(&shutdown, &lane, &exits);
        quit(&shutdown, &lane, &exits);
        assert_eq!(shutdown.phase(), Phase::Complete);
        assert_eq!(exits.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn concurrent_quit_cannot_overlap_cleanup_or_double_exit() {
        let (started_tx, started) = mpsc::channel();
        let (release_tx, release) = mpsc::channel();
        let lane = Arc::new(
            BlockingLane::new_with_shutdown(0, 1, move |attempts| {
                *attempts += 1;
                assert_eq!(*attempts, 1);
                started_tx.send(()).unwrap();
                release.recv().unwrap();
                Ok(())
            })
            .unwrap(),
        );
        let shutdown = Arc::new(Shutdown::default());
        let exits = Arc::new(AtomicUsize::new(0));
        let worker = {
            let (shutdown, lane, exits) = (shutdown.clone(), lane.clone(), exits.clone());
            std::thread::spawn(move || quit(&shutdown, &lane, &exits))
        };
        started.recv().unwrap();
        let requests: Vec<_> = (0..16)
            .map(|_| {
                let (shutdown, lane, exits) = (shutdown.clone(), lane.clone(), exits.clone());
                std::thread::spawn(move || quit(&shutdown, &lane, &exits))
            })
            .collect();
        for request in requests {
            request.join().unwrap();
        }
        assert_eq!(shutdown.phase(), Phase::Attempting);
        assert_eq!(exits.load(Ordering::SeqCst), 0);
        release_tx.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(exits.load(Ordering::SeqCst), 1);
        assert!(!shutdown.finish(true));
    }
    #[test]
    fn timeout_is_recoverable_without_starting_overlapping_cleanup() {
        let (started_tx, started) = mpsc::channel();
        let (release_tx, release) = mpsc::channel();
        let lane = BlockingLane::new_with_shutdown(0, 1, move |attempts| {
            *attempts += 1;
            assert_eq!(*attempts, 1);
            started_tx.send(()).unwrap();
            release.recv().unwrap();
            Ok(())
        })
        .unwrap();
        let shutdown = Shutdown::default();
        assert!(shutdown.begin());
        assert_eq!(
            lane.close_with_timeout(Duration::ZERO),
            Err(LaneError::Timeout)
        );
        started.recv().unwrap();
        assert!(!shutdown.finish(false));
        assert_eq!(shutdown.phase(), Phase::Incomplete);
        assert!(shutdown.begin());
        assert_eq!(
            lane.close_with_timeout(Duration::ZERO),
            Err(LaneError::Timeout)
        );
        assert!(!shutdown.finish(false));
        release_tx.send(()).unwrap();
        assert!(shutdown.begin());
        assert!(shutdown.finish(lane.close().is_ok()));
        assert_eq!(shutdown.phase(), Phase::Complete);
    }
}
