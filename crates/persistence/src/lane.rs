//! One bounded, blocking configuration lane shared by application services.
//! SQL never executes on an async/UI executor or an input callback.
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, TryLockError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;
type Job<S> = Box<dyn FnOnce(&mut S) + Send>;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LaneError {
    Busy,
    Closed,
    Panicked,
    Timeout,
    CleanupFailed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShutdownPhase {
    Open,
    Draining,
    Cleaning,
    Incomplete,
    Complete,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ShutdownStatus {
    pub phase: ShutdownPhase,
    pub last_error: Option<LaneError>,
}
struct Progress {
    status: ShutdownStatus,
    in_flight: bool,
}
pub struct BlockingLane<S: Send + 'static> {
    sender: Mutex<Option<mpsc::SyncSender<Job<S>>>>,
    join: Mutex<Option<JoinHandle<()>>>,
    cleanup: mpsc::SyncSender<()>,
    progress: Arc<(Mutex<Progress>, Condvar)>,
    close_attempt: Mutex<()>,
}
impl<S: Send + 'static> BlockingLane<S> {
    pub fn new(state: S, capacity: usize) -> std::io::Result<Self> {
        Self::new_with_shutdown(state, capacity, |_| Ok(()))
    }
    pub fn new_with_shutdown(
        state: S,
        capacity: usize,
        mut shutdown: impl FnMut(&mut S) -> Result<(), LaneError> + Send + 'static,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<Job<S>>(capacity);
        let (cleanup, attempts) = mpsc::sync_channel(1);
        let progress = Arc::new((
            Mutex::new(Progress {
                status: ShutdownStatus {
                    phase: ShutdownPhase::Open,
                    last_error: None,
                },
                in_flight: false,
            }),
            Condvar::new(),
        ));
        let completion = progress.clone();
        let join = thread::Builder::new()
            .name("configuration-persistence".into())
            .spawn(move || {
                let mut state = state;
                for job in receiver {
                    job(&mut state);
                }
                // Admission is disconnected before this loop can start. All
                // accepted jobs have drained in FIFO order. Failed cleanup keeps
                // the entire state here, parked until a deliberate close retry.
                for () in attempts {
                    {
                        let mut progress = completion.0.lock().unwrap();
                        progress.status.phase = ShutdownPhase::Cleaning;
                    }
                    let result = catch_unwind(AssertUnwindSafe(|| shutdown(&mut state)))
                        .unwrap_or(Err(LaneError::Panicked));
                    if result.is_ok() {
                        // Acknowledge only after all lane-owned connections and
                        // services have been released on their owning thread.
                        drop(state);
                        let mut progress = completion.0.lock().unwrap();
                        progress.status = ShutdownStatus {
                            phase: ShutdownPhase::Complete,
                            last_error: None,
                        };
                        progress.in_flight = false;
                        completion.1.notify_all();
                        return;
                    }
                    let mut progress = completion.0.lock().unwrap();
                    progress.status = ShutdownStatus {
                        phase: ShutdownPhase::Incomplete,
                        last_error: result.err(),
                    };
                    progress.in_flight = false;
                    completion.1.notify_all();
                }
            })?;
        Ok(Self {
            sender: Mutex::new(Some(sender)),
            join: Mutex::new(Some(join)),
            cleanup,
            progress,
            close_attempt: Mutex::new(()),
        })
    }
    pub fn submit<R: Send + 'static>(
        &self,
        task: impl FnOnce(&mut S) -> R + Send + 'static,
    ) -> Result<mpsc::Receiver<R>, LaneError> {
        let (reply, receiver) = mpsc::sync_channel(1);
        let guard = self.sender.lock().map_err(|_| LaneError::Panicked)?;
        let sender = guard.as_ref().ok_or(LaneError::Closed)?;
        sender
            .try_send(Box::new(move |state| {
                let _ = reply.send(task(state));
            }))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => LaneError::Busy,
                mpsc::TrySendError::Disconnected(_) => LaneError::Closed,
            })?;
        Ok(receiver)
    }
    /// Close admission immediately, without waiting on jobs or cleanup. Admission
    /// stays closed after failure; only cleanup, never mutations, may be retried.
    pub fn stop_accepting(&self) -> Result<(), LaneError> {
        self.sender.lock().map_err(|_| LaneError::Panicked)?.take();
        let mut progress = self.progress.0.lock().map_err(|_| LaneError::Panicked)?;
        if progress.status.phase == ShutdownPhase::Open {
            progress.status.phase = ShutdownPhase::Draining;
        }
        Ok(())
    }
    pub fn shutdown_status(&self) -> Result<ShutdownStatus, LaneError> {
        Ok(self
            .progress
            .0
            .lock()
            .map_err(|_| LaneError::Panicked)?
            .status)
    }
    /// Drain accepted operations, then attempt cleanup. Failure retains state for
    /// an explicit retry. Timeout retains the same in-flight attempt; a retry may
    /// wait for it but cannot overlap it. Never call on the UI/async executor.
    pub fn close(&self) -> Result<(), LaneError> {
        self.close_with_timeout(Duration::from_secs(10))
    }
    pub fn close_with_timeout(&self, timeout: Duration) -> Result<(), LaneError> {
        let _attempt = self.close_attempt.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => LaneError::Busy,
            TryLockError::Poisoned(_) => LaneError::Panicked,
        })?;
        self.stop_accepting()?;
        let mut progress = self.progress.0.lock().map_err(|_| LaneError::Panicked)?;
        if progress.status.phase != ShutdownPhase::Complete && !progress.in_flight {
            self.cleanup.try_send(()).map_err(|_| LaneError::Panicked)?;
            progress.in_flight = true;
            progress.status.last_error = None;
            if progress.status.phase == ShutdownPhase::Incomplete {
                progress.status.phase = ShutdownPhase::Cleaning;
            }
        }
        let (mut progress, _) = self
            .progress
            .1
            .wait_timeout_while(progress, timeout, |progress| progress.in_flight)
            .map_err(|_| LaneError::Panicked)?;
        if progress.in_flight {
            progress.status.last_error = Some(LaneError::Timeout);
            return Err(LaneError::Timeout);
        }
        if progress.status.phase != ShutdownPhase::Complete {
            return Err(progress
                .status
                .last_error
                .unwrap_or(LaneError::CleanupFailed));
        }
        drop(progress);
        if let Some(join) = self.join.lock().map_err(|_| LaneError::Panicked)?.take() {
            join.join().map_err(|_| LaneError::Panicked)?;
        }
        Ok(())
    }
}
