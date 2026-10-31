use super::{
    compiled::{ActionRequest, NativeAction, NativeCompiledKeymap, PublishedKeymap},
    ffi,
    output::{Injector, execute},
};
use crate::runtime::{Metrics, ModifierState};
use crossbeam_queue::ArrayQueue;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU16, Ordering},
    mpsc,
};
use std::thread::{self, JoinHandle, Thread};
use std::time::Duration;

const STOP_TIMEOUT: Duration = Duration::from_secs(3);
// At most 64 pending user actions. Queue overflow passes the original key.
pub const QUEUE_CAPACITY: usize = 64;
pub struct Shared {
    pub map: Arc<PublishedKeymap>,
    pub metrics: Arc<Metrics>,
    pub physical: AtomicU16,
    pub cancelled: AtomicBool,
    pub queue: ArrayQueue<ActionRequest>,
}
impl Shared {
    pub fn new(map: Arc<PublishedKeymap>, metrics: Arc<Metrics>) -> Self {
        Self {
            map,
            metrics,
            physical: AtomicU16::new(0),
            cancelled: AtomicBool::new(false),
            queue: ArrayQueue::new(QUEUE_CAPACITY),
        }
    }
}
pub struct Worker {
    join: Option<JoinHandle<()>>,
    done: mpsc::Receiver<()>,
    pub thread: Thread,
    terminal_error: Option<&'static str>,
}
impl Worker {
    pub fn start(shared: Arc<Shared>, injector: Arc<dyn Injector>) -> Result<Self, &'static str> {
        let (done_tx, done) = mpsc::channel();
        let join = thread::Builder::new()
            .name("hotkeys-output".into())
            .spawn(move || {
                loop {
                    if shared.cancelled.load(Ordering::Acquire) {
                        break;
                    }
                    if let Some(request) = shared.queue.pop() {
                        if let NativeAction::EmitShortcut(output) = request.action {
                            let physical =
                                ModifierState::from_bits(shared.physical.load(Ordering::Acquire));
                            execute(injector.as_ref(), output, physical, &shared.metrics);
                        }
                    } else {
                        thread::park();
                    }
                }
                let _ = done_tx.send(());
            })
            .map_err(|_| "worker_start_failed")?;
        let thread = join.thread().clone();
        Ok(Self {
            join: Some(join),
            done,
            thread,
            terminal_error: None,
        })
    }
    pub fn stop(&mut self) -> Result<(), &'static str> {
        self.stop_with_timeout(STOP_TIMEOUT)
    }
    fn stop_with_timeout(&mut self, timeout: Duration) -> Result<(), &'static str> {
        if let Some(error) = self.terminal_error {
            return Err(error);
        }
        self.thread.unpark();
        if self.join.is_some() {
            self.done
                .recv_timeout(timeout)
                .map_err(|error| match error {
                    mpsc::RecvTimeoutError::Timeout => "worker_stop_timeout",
                    mpsc::RecvTimeoutError::Disconnected => "worker_cleanup_unknown",
                })?;
            if let Some(join) = self.join.take()
                && join.join().is_err()
            {
                self.terminal_error = Some("worker_panicked");
                return Err("worker_panicked");
            }
        }
        Ok(())
    }
}

pub struct WindowsEngine {
    shared: Arc<Shared>,
    worker: Worker,
    hook: ffi::HookThread,
}
impl WindowsEngine {
    pub fn start(map: Arc<PublishedKeymap>, metrics: Arc<Metrics>) -> Result<Self, &'static str> {
        let shared = Arc::new(Shared::new(map, metrics));
        let mut worker = Worker::start(shared.clone(), Arc::new(ffi::SystemInjector))?;
        match ffi::HookThread::start(shared.clone(), worker.thread.clone()) {
            Ok(hook) => Ok(Self {
                shared,
                worker,
                hook,
            }),
            Err(error) => {
                shared.cancelled.store(true, Ordering::Release);
                worker.stop()?;
                Err(error)
            }
        }
    }
    pub fn publish(&self, map: Arc<NativeCompiledKeymap>) {
        self.shared.metrics.set_mapping_count(map.mapping_count());
        self.shared.map.publish(map);
    }
    pub fn stop(&mut self) -> Result<(), &'static str> {
        self.shared.cancelled.store(true, Ordering::Release);
        self.worker.thread.unpark();
        // Hook completion is acknowledged after unhook and TLS destruction.
        let hook = self.hook.stop();
        let worker = self.worker.stop();
        hook.and(worker)
    }
}
impl Drop for WindowsEngine {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_timeout_retains_owned_thread_until_late_completion() {
        let (release_tx, release) = mpsc::channel();
        let (done_tx, done) = mpsc::channel();
        let join = thread::spawn(move || {
            release.recv().unwrap();
            done_tx.send(()).unwrap();
        });
        let mut worker = Worker {
            thread: join.thread().clone(),
            join: Some(join),
            done,
            terminal_error: None,
        };
        assert_eq!(
            worker.stop_with_timeout(Duration::ZERO),
            Err("worker_stop_timeout")
        );
        assert!(worker.join.is_some());
        release_tx.send(()).unwrap();
        worker.stop_with_timeout(Duration::from_secs(1)).unwrap();
        assert!(worker.join.is_none());
        worker.stop().unwrap();
    }
    #[test]
    fn missing_worker_ack_is_unknown_and_never_becomes_a_successful_stop() {
        let (done_tx, done) = mpsc::channel();
        let join = thread::spawn(move || drop(done_tx));
        let mut worker = Worker {
            thread: join.thread().clone(),
            join: Some(join),
            done,
            terminal_error: None,
        };
        assert_eq!(worker.stop(), Err("worker_cleanup_unknown"));
        assert_eq!(worker.stop(), Err("worker_cleanup_unknown"));
        assert!(worker.join.is_some());
    }
}
