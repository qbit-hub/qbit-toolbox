//! All direct Win32 calls and the no-unwind callback boundary live here.
use super::{
    engine::Shared,
    hot_path::{Decision, HotPathState, KeyEventKind, RawEvent, classify_origin},
    output::{CAPACITY, EncodedInput, Injector},
};
use std::{
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, atomic::Ordering, mpsc},
    thread::{self, JoinHandle, Thread},
    time::Duration,
};
use windows::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

struct Context {
    shared: Arc<Shared>,
    state: HotPathState,
    worker: Thread,
}
thread_local! { static CONTEXT: RefCell<Option<Context>> = const { RefCell::new(None) }; }

unsafe extern "system" fn callback(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
    if code < 0 {
        // SAFETY: forwarding original hook parameters; no pointer dereference.
        return unsafe { CallNextHookEx(None, code, message, data) };
    }
    let kind = match u32::try_from(message.0) {
        Ok(WM_KEYDOWN | WM_SYSKEYDOWN) => KeyEventKind::Down,
        Ok(WM_KEYUP | WM_SYSKEYUP) => KeyEventKind::Up,
        _ => {
            // SAFETY: unhandled messages are forwarded unchanged.
            return unsafe { CallNextHookEx(None, code, message, data) };
        }
    };
    let handled = catch_unwind(AssertUnwindSafe(|| {
        if data.0 == 0 || code != HC_ACTION as i32 {
            return false;
        }
        // SAFETY: WH_KEYBOARD_LL HC_ACTION supplies a live KBDLLHOOKSTRUCT
        // for this callback duration. read_unaligned avoids alignment assumptions;
        // it is copied, never retained or exposed outside this boundary.
        let raw = unsafe { (data.0 as *const KBDLLHOOKSTRUCT).read_unaligned() };
        CONTEXT
            .try_with(|cell| {
                let Ok(mut context) = cell.try_borrow_mut() else {
                    return false;
                };
                let Some(context) = context.as_mut() else {
                    return false;
                };
                if context.shared.cancelled.load(Ordering::Acquire) {
                    return false;
                }
                let event = RawEvent {
                    kind,
                    vk_code: raw.vkCode,
                    scan_code: raw.scanCode,
                    extended: raw.flags.contains(LLKHF_EXTENDED),
                    origin: classify_origin(
                        raw.flags.contains(LLKHF_INJECTED),
                        raw.flags.contains(LLKHF_LOWER_IL_INJECTED),
                        raw.dwExtraInfo,
                    ),
                };
                let map = context.shared.map.load();
                let decision = context.state.process(
                    event,
                    &map,
                    &context.shared.queue,
                    &context.shared.metrics,
                );
                context
                    .shared
                    .physical
                    .store(context.state.modifiers().bits(), Ordering::Release);
                if decision == Decision::Consume {
                    context.worker.unpark();
                }
                decision == Decision::Consume
            })
            .unwrap_or(false)
    }))
    .unwrap_or(false);
    if handled {
        LRESULT(1)
    } else {
        // SAFETY: no consumed event; preserve the OS hook chain.
        unsafe { CallNextHookEx(None, code, message, data) }
    }
}

pub struct SystemInjector;
impl Injector for SystemInjector {
    fn send(&self, events: &[EncodedInput]) -> usize {
        if events.len() > CAPACITY {
            return 0;
        }
        let mut inputs = [INPUT::default(); CAPACITY];
        for (slot, event) in inputs.iter_mut().zip(events) {
            let mut flags = KEYBD_EVENT_FLAGS(0);
            if event.up {
                flags |= KEYEVENTF_KEYUP;
            }
            if event.extended {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            if event.scan_mode {
                flags |= KEYEVENTF_SCANCODE;
            }
            *slot = INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(event.vk),
                        wScan: event.scan,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: event.tag,
                    },
                },
            };
        }
        let Ok(size) = i32::try_from(std::mem::size_of::<INPUT>()) else {
            return 0;
        };
        // SAFETY: initialized keyboard INPUTs in a live bounded slice; correct
        // ABI size checked above. Never retry partial sends or infer UIPI.
        unsafe { SendInput(&inputs[..events.len()], size) as usize }
    }
}

pub struct HookThread {
    id: u32,
    join: Option<JoinHandle<()>>,
    done: mpsc::Receiver<Result<(), &'static str>>,
    requests: mpsc::SyncSender<()>,
    request_pending: bool,
    terminal_error: Option<&'static str>,
}
impl HookThread {
    pub fn start(shared: Arc<Shared>, worker: Thread) -> Result<Self, &'static str> {
        let (ready_tx, ready) = mpsc::sync_channel(1);
        let (done_tx, done) = mpsc::sync_channel(1);
        let (requests, stop_requests) = mpsc::sync_channel(1);
        let join = thread::Builder::new()
            .name("hotkeys-hook".into())
            .spawn(move || {
                // SAFETY: no handles/pointers are supplied; ID is of this thread.
                let id = unsafe { GetCurrentThreadId() };
                let mut message = MSG::default();
                // SAFETY: valid writable MSG; creates this thread's message queue.
                unsafe {
                    let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
                }
                CONTEXT.with(|cell| {
                    *cell.borrow_mut() = Some(Context {
                        shared: shared.clone(),
                        state: HotPathState::default(),
                        worker,
                    })
                });
                // SAFETY: current executable module remains loaded throughout hook
                // lifetime; callback has the exact windows 0.61.3 HOOKPROC ABI.
                let hook = unsafe {
                    GetModuleHandleW(None).and_then(|module| {
                        SetWindowsHookExW(WH_KEYBOARD_LL, Some(callback), Some(module.into()), 0)
                    })
                };
                let Ok(hook) = hook else {
                    CONTEXT.with(|cell| cell.borrow_mut().take());
                    let _ = ready_tx.send(Err("hook_install_failed"));
                    let _ = done_tx.send(Ok(()));
                    return;
                };
                // Require an all-released activation boundary rather than
                // guessing physical state for modifiers held before the hook.
                let held = [
                    VK_LCONTROL,
                    VK_RCONTROL,
                    VK_LSHIFT,
                    VK_RSHIFT,
                    VK_LMENU,
                    VK_RMENU,
                    VK_LWIN,
                    VK_RWIN,
                ]
                .into_iter()
                .any(|key| {
                    // SAFETY: scalar VK query with no pointers or handles;
                    // activation only, never modifier discovery in a callback.
                    unsafe { GetAsyncKeyState(i32::from(key.0)) < 0 }
                });
                if held {
                    // SAFETY: installing thread owns this fresh hook; remove
                    // it before reporting failed activation and exiting thread.
                    let _ = unsafe { UnhookWindowsHookEx(hook) };
                    CONTEXT.with(|cell| cell.borrow_mut().take());
                    let _ = ready_tx.send(Err("modifiers_held_at_enable"));
                    let _ = done_tx.send(Ok(()));
                    return;
                }
                shared.metrics.set_hook_active(true);
                let acknowledged = ready_tx.send(Ok(id)).is_ok();
                let mut retry_unhook = false;
                loop {
                    let requested = if acknowledged {
                        let message_result = loop {
                            // SAFETY: valid MSG on installing thread; blocks without
                            // polling, returns 0 on WM_QUIT and -1 on failure.
                            let result = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
                            if result <= 0 {
                                break result;
                            }
                        };
                        // An unexpected message-loop failure gets one immediate
                        // cleanup attempt. Retrying failed cleanup always needs
                        // a deliberate request, so a failed loop cannot spin.
                        if message_result == 0 || retry_unhook {
                            stop_requests.recv().is_ok()
                        } else {
                            true
                        }
                    } else {
                        false
                    };
                    shared.cancelled.store(true, Ordering::Release);
                    CONTEXT.with(|cell| {
                        if let Some(context) = cell.borrow().as_ref() {
                            context.worker.unpark();
                        }
                    });
                    // SAFETY: this thread owns the hook. Retry the same handle only
                    // after failure; a successful unhook immediately exits the loop.
                    let stopped = unsafe { UnhookWindowsHookEx(hook) };
                    if stopped.is_ok() {
                        shared.metrics.set_hook_active(false);
                        CONTEXT.with(|cell| cell.borrow_mut().take());
                        let _ = done_tx.send(Ok(()));
                        return;
                    }
                    // Keep the hook/TLS/thread owned and pump messages until
                    // another requested stop. Failed unhook is never inactive.
                    retry_unhook = true;
                    let _ = done_tx.send(Err("hook_uninstall_failed"));
                    if !requested {
                        return;
                    }
                }
            })
            .map_err(|_| "hook_thread_start_failed")?;
        // Hook installation is synchronous on its own thread. The handshake
        // ensures callers never report Running before successful installation.
        match ready.recv() {
            Ok(Ok(id)) => Ok(Self {
                id,
                join: Some(join),
                done,
                requests,
                request_pending: false,
                terminal_error: None,
            }),
            Ok(Err(error)) => {
                let _ = join.join();
                Err(error)
            }
            Err(_) => {
                let _ = join.join();
                Err("hook_install_failed")
            }
        }
    }
    pub fn stop(&mut self) -> Result<(), &'static str> {
        self.stop_with(Duration::from_secs(3), |id| {
            // SAFETY: id from acknowledged hook thread; posts only WM_QUIT,
            // with no pointer payload. Failure is reported, never assumed sent.
            unsafe { PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0)) }
                .map_err(|_| "hook_stop_signal_failed")
        })
    }
    fn stop_with(
        &mut self,
        timeout: Duration,
        post: impl FnOnce(u32) -> Result<(), &'static str>,
    ) -> Result<(), &'static str> {
        if let Some(error) = self.terminal_error {
            return Err(error);
        }
        if self.join.is_none() {
            return Ok(());
        }
        match self.done.try_recv() {
            Ok(result) => return self.acknowledge(result),
            Err(mpsc::TryRecvError::Disconnected) => return Err("hook_cleanup_unknown"),
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if !self.request_pending {
            post(self.id)?;
            self.requests
                .try_send(())
                .map_err(|_| "hook_stop_signal_failed")?;
            self.request_pending = true;
        }
        // Timeout retains both join ownership and the pending acknowledgement.
        // A later call waits for that same request instead of posting another.
        let result = self
            .done
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => "hook_stop_timeout",
                mpsc::RecvTimeoutError::Disconnected => "hook_cleanup_unknown",
            })?;
        self.acknowledge(result)
    }
    fn acknowledge(&mut self, result: Result<(), &'static str>) -> Result<(), &'static str> {
        self.request_pending = false;
        result?; // Failure keeps the live thread and hook available for retry.
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            self.terminal_error = Some("hook_thread_panicked");
            return Err("hook_thread_panicked");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fake(
        run: impl FnOnce(mpsc::Receiver<()>, mpsc::SyncSender<Result<(), &'static str>>)
        + Send
        + 'static,
    ) -> HookThread {
        let (requests, receiver) = mpsc::sync_channel(1);
        let (sender, done) = mpsc::sync_channel(1);
        HookThread {
            id: 7,
            join: Some(thread::spawn(move || run(receiver, sender))),
            done,
            requests,
            request_pending: false,
            terminal_error: None,
        }
    }
    #[test]
    fn failed_stop_signal_keeps_hook_owned_and_next_attempt_can_succeed() {
        let mut hook = fake(|requests, done| {
            requests.recv().unwrap();
            done.send(Ok(())).unwrap();
        });
        assert_eq!(
            hook.stop_with(Duration::ZERO, |_| Err("hook_stop_signal_failed")),
            Err("hook_stop_signal_failed")
        );
        assert!(hook.join.is_some());
        assert!(!hook.request_pending);
        hook.stop_with(Duration::from_secs(1), |_| Ok(())).unwrap();
        assert!(hook.join.is_none());
    }
    #[test]
    fn failed_unhook_ack_keeps_thread_and_allows_a_deliberate_retry() {
        let mut hook = fake(|requests, done| {
            requests.recv().unwrap();
            done.send(Err("hook_uninstall_failed")).unwrap();
            requests.recv().unwrap();
            done.send(Ok(())).unwrap();
        });
        assert_eq!(
            hook.stop_with(Duration::from_secs(1), |_| Ok(())),
            Err("hook_uninstall_failed")
        );
        assert!(hook.join.is_some());
        hook.stop_with(Duration::from_secs(1), |_| Ok(())).unwrap();
        hook.stop_with(Duration::ZERO, |_| panic!("already cleaned"))
            .unwrap();
    }
    #[test]
    fn hook_timeout_preserves_pending_request_and_late_ack_is_consumed_once() {
        let (started_tx, started) = mpsc::channel();
        let (release_tx, release) = mpsc::channel();
        let mut hook = fake(move |requests, done| {
            requests.recv().unwrap();
            started_tx.send(()).unwrap();
            release.recv().unwrap();
            done.send(Ok(())).unwrap();
            assert!(requests.try_recv().is_err());
        });
        assert_eq!(
            hook.stop_with(Duration::ZERO, |_| Ok(())),
            Err("hook_stop_timeout")
        );
        started.recv().unwrap();
        assert!(hook.join.is_some());
        assert!(hook.request_pending);
        assert_eq!(
            hook.stop_with(Duration::ZERO, |_| panic!("no duplicate stop signal")),
            Err("hook_stop_timeout")
        );
        release_tx.send(()).unwrap();
        hook.stop_with(Duration::from_secs(1), |_| {
            panic!("wait for the existing acknowledgement")
        })
        .unwrap();
    }
    #[test]
    fn missing_hook_ack_remains_unknown_and_never_authorizes_success() {
        let mut hook = fake(|requests, done| {
            requests.recv().unwrap();
            drop(done);
        });
        assert_eq!(
            hook.stop_with(Duration::from_secs(1), |_| Ok(())),
            Err("hook_cleanup_unknown")
        );
        assert_eq!(
            hook.stop_with(Duration::ZERO, |_| panic!(
                "terminated owner cannot acknowledge"
            )),
            Err("hook_cleanup_unknown")
        );
        assert!(hook.join.is_some());
    }
}
