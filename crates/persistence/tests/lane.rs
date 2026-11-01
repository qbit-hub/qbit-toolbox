use persistence::lane::{BlockingLane, LaneError, ShutdownPhase};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;
#[test]
fn lane_bounds_acceptance_serializes_mutations_and_rejects_after_close() {
    let lane = BlockingLane::new(0, 1).unwrap();
    let (started_tx, started) = mpsc::channel();
    let (release_tx, release) = mpsc::channel();
    let first = lane
        .submit(move |state| {
            started_tx.send(()).unwrap();
            release.recv().unwrap();
            *state += 1;
            *state
        })
        .unwrap();
    started.recv().unwrap();
    let second = lane
        .submit(|state| {
            *state += 1;
            *state
        })
        .unwrap();
    assert!(matches!(lane.submit(|_| ()), Err(LaneError::Busy)));
    release_tx.send(()).unwrap();
    assert_eq!(first.recv().unwrap(), 1);
    assert_eq!(second.recv().unwrap(), 2);
    lane.close().unwrap();
    lane.close().unwrap();
    assert!(matches!(lane.submit(|_| ()), Err(LaneError::Closed)));
}
#[test]
fn shutdown_failure_is_reported_instead_of_claiming_cleanup() {
    struct Owned(Arc<AtomicBool>);
    impl Drop for Owned {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicBool::new(false));
    let mut attempts = 0;
    let lane = BlockingLane::new_with_shutdown(Owned(dropped.clone()), 1, move |_| {
        attempts += 1;
        if attempts == 1 {
            Err(LaneError::CleanupFailed)
        } else {
            Ok(())
        }
    })
    .unwrap();
    assert_eq!(lane.close(), Err(LaneError::CleanupFailed));
    assert_eq!(
        lane.shutdown_status().unwrap().phase,
        ShutdownPhase::Incomplete
    );
    assert!(!dropped.load(Ordering::SeqCst));
    assert!(matches!(lane.submit(|_| ()), Err(LaneError::Closed)));
    lane.close().unwrap();
    assert_eq!(
        lane.shutdown_status().unwrap().phase,
        ShutdownPhase::Complete
    );
    assert!(dropped.load(Ordering::SeqCst));
    lane.close().unwrap();
}

#[test]
fn timeout_and_concurrent_close_retain_one_in_flight_cleanup() {
    let (started_tx, started) = mpsc::channel();
    let (release_tx, release) = mpsc::channel();
    let attempts = Arc::new(AtomicUsize::new(0));
    let count = attempts.clone();
    let lane = Arc::new(
        BlockingLane::new_with_shutdown((), 1, move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            started_tx.send(()).unwrap();
            release.recv().unwrap();
            Ok(())
        })
        .unwrap(),
    );
    let closing = lane.clone();
    let waiter = std::thread::spawn(move || closing.close_with_timeout(Duration::from_secs(2)));
    started.recv().unwrap();
    assert_eq!(lane.close(), Err(LaneError::Busy));
    assert_eq!(
        lane.shutdown_status().unwrap().phase,
        ShutdownPhase::Cleaning
    );
    assert!(matches!(lane.submit(|_| ()), Err(LaneError::Closed)));
    // The first bounded caller times out, but cleanup still owns its state.
    assert_eq!(waiter.join().unwrap(), Err(LaneError::Timeout));
    assert_eq!(
        lane.close_with_timeout(Duration::ZERO),
        Err(LaneError::Timeout)
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    release_tx.send(()).unwrap();
    lane.close().unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(
        lane.shutdown_status().unwrap().phase,
        ShutdownPhase::Complete
    );
}

#[test]
fn closing_busy_lane_drains_accepted_fifo_before_cleanup_and_never_reopens() {
    let (started_tx, started) = mpsc::channel();
    let (release_tx, release) = mpsc::channel();
    let lane = BlockingLane::new_with_shutdown(Vec::new(), 1, |state| {
        assert_eq!(*state, vec![1, 2]);
        Ok(())
    })
    .unwrap();
    let first = lane
        .submit(move |state| {
            started_tx.send(()).unwrap();
            release.recv().unwrap();
            state.push(1);
        })
        .unwrap();
    started.recv().unwrap();
    let second = lane.submit(|state| state.push(2)).unwrap();
    assert!(matches!(lane.submit(|_| ()), Err(LaneError::Busy)));
    lane.stop_accepting().unwrap();
    assert_eq!(
        lane.shutdown_status().unwrap().phase,
        ShutdownPhase::Draining
    );
    assert!(matches!(lane.submit(|_| ()), Err(LaneError::Closed)));
    assert_eq!(
        lane.close_with_timeout(Duration::ZERO),
        Err(LaneError::Timeout)
    );
    release_tx.send(()).unwrap();
    first.recv().unwrap();
    second.recv().unwrap();
    lane.close().unwrap();
    assert!(matches!(lane.submit(|_| ()), Err(LaneError::Closed)));
}

#[test]
fn cleanup_panic_is_incomplete_and_keeps_state_available_for_deliberate_retry() {
    let mut first = true;
    let lane = BlockingLane::new_with_shutdown(42, 1, move |state| {
        assert_eq!(*state, 42);
        if std::mem::take(&mut first) {
            panic!("deterministic cleanup failure");
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(lane.close(), Err(LaneError::Panicked));
    assert_eq!(
        lane.shutdown_status().unwrap().phase,
        ShutdownPhase::Incomplete
    );
    lane.close().unwrap();
}
