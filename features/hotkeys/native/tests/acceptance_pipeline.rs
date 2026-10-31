#![cfg(windows)]

use qbit_hotkeys::{
    domain::{
        Action, Chord, KeyIdentity, KeyToken, Mapping, MappingBehavior, MappingMetadata, Modifier,
        Scope, Trigger,
    },
    platform::windows::{
        compiled::{self, ActionRequest, NativeAction, NativeCompiledKeymap, PublishedKeymap},
        engine::{Shared, Worker},
        hot_path::{
            Decision, HotPathState, InputOrigin, KeyEventKind, QBIT_INJECTION_TAG, RawEvent,
            classify_origin,
        },
        keys::{LogicalKey, PhysicalKey},
        output::{EncodedInput, Injector},
    },
    runtime::{ConcreteModifier, Metrics, ModifierState},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
use std::time::Duration;
use uuid::Uuid;

const F9_VK: u32 = 0x78;
const F9_SCAN: u32 = 0x43;
const F10_VK: u16 = 0x79;

fn chord(modifiers: impl IntoIterator<Item = Modifier>, key: &str) -> Chord {
    Chord::new(
        modifiers,
        KeyIdentity::Logical(KeyToken::new(key).expect("fixture key token is valid")),
    )
    .expect("fixture chord is valid")
}

fn mapping(trigger_modifiers: &[Modifier], action: Action) -> Mapping {
    Mapping::with_id(
        Uuid::from_u128(0x484f_544b_4559_5300_0000_0000_0000_0001),
        true,
        Trigger::Chord {
            chord: chord(trigger_modifiers.iter().copied(), "F9"),
        },
        action,
        Scope::Global,
        MappingBehavior::default(),
        MappingMetadata::default(),
    )
}

fn emit_mapping() -> Mapping {
    mapping(
        &[Modifier::Ctrl, Modifier::Shift],
        Action::EmitShortcut {
            chord: chord([Modifier::Ctrl, Modifier::Shift], "F10"),
        },
    )
}

fn compile(mappings: &[Mapping]) -> Arc<NativeCompiledKeymap> {
    Arc::new(compiled::compile(mappings).expect("acceptance fixture compiles"))
}

fn event(kind: KeyEventKind, vk_code: u32, scan_code: u32, extended: bool) -> RawEvent {
    RawEvent {
        kind,
        vk_code,
        scan_code,
        extended,
        origin: InputOrigin::Physical,
    }
}

fn modifier_event(kind: KeyEventKind, modifier: ConcreteModifier) -> RawEvent {
    let (vk, scan, extended) = match modifier {
        ConcreteModifier::LeftCtrl => (0xa2, 0x1d, false),
        ConcreteModifier::RightCtrl => (0xa3, 0x1d, true),
        ConcreteModifier::LeftShift => (0xa0, 0x2a, false),
        ConcreteModifier::RightShift => (0xa1, 0x36, false),
        ConcreteModifier::LeftAlt => (0xa4, 0x38, false),
        ConcreteModifier::RightAlt => (0xa5, 0x38, true),
        ConcreteModifier::LeftMeta => (0x5b, 0x5b, true),
        ConcreteModifier::RightMeta => (0x5c, 0x5c, true),
    };
    event(kind, vk, scan, extended)
}

fn process(state: &mut HotPathState, shared: &Shared, raw: RawEvent) -> Decision {
    let map = shared.map.load();
    let decision = state.process(raw, &map, &shared.queue, &shared.metrics);
    shared
        .physical
        .store(state.modifiers().bits(), Ordering::Release);
    decision
}

struct Recorder {
    sender: SyncSender<Vec<EncodedInput>>,
    sends: AtomicUsize,
    short_send: bool,
}

impl Injector for Recorder {
    fn send(&self, events: &[EncodedInput]) -> usize {
        self.sends.fetch_add(1, Ordering::SeqCst);
        self.sender
            .send(events.to_vec())
            .expect("acceptance recorder remains connected");
        events.len() - usize::from(self.short_send)
    }
}

struct WorkerFixture {
    shared: Arc<Shared>,
    worker: Option<Worker>,
    receiver: Receiver<Vec<EncodedInput>>,
    recorder: Arc<Recorder>,
}

impl WorkerFixture {
    fn start(shared: Arc<Shared>, short_send: bool) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        let recorder = Arc::new(Recorder {
            sender,
            sends: AtomicUsize::new(0),
            short_send,
        });
        let worker = Worker::start(shared.clone(), recorder.clone()).expect("worker starts");
        Self {
            shared,
            worker: Some(worker),
            receiver,
            recorder,
        }
    }

    fn receive(&self) -> Vec<EncodedInput> {
        self.receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("worker sends within the bounded acceptance timeout")
    }

    fn unpark(&self) {
        self.worker
            .as_ref()
            .expect("worker is owned")
            .thread
            .unpark();
    }

    fn stop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.as_mut() {
            worker.thread.unpark();
            worker.stop().expect("worker acknowledges stop");
        }
        self.worker = None;
    }
}

impl Drop for WorkerFixture {
    fn drop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.as_mut() {
            worker.thread.unpark();
            let _ = worker.stop();
        }
        self.worker = None;
    }
}

fn expected(vk: u16, scan: u16, scan_mode: bool, up: bool) -> EncodedInput {
    EncodedInput {
        vk,
        scan,
        extended: false,
        scan_mode,
        up,
        tag: QBIT_INJECTION_TAG,
    }
}

#[test]
fn ctrl_shift_f9_left_sides_to_ctrl_shift_f10_has_exact_consumption_and_output_plan() {
    let published = Arc::new(PublishedKeymap::default());
    published.publish(compile(&[emit_mapping()]));
    let metrics = Arc::new(Metrics::default());
    let shared = Arc::new(Shared::new(published, metrics.clone()));
    let mut worker = WorkerFixture::start(shared.clone(), false);
    let mut state = HotPathState::default();

    assert_eq!(
        process(
            &mut state,
            &shared,
            modifier_event(KeyEventKind::Down, ConcreteModifier::LeftCtrl)
        ),
        Decision::Pass
    );
    assert_eq!(
        process(
            &mut state,
            &shared,
            modifier_event(KeyEventKind::Down, ConcreteModifier::LeftShift)
        ),
        Decision::Pass
    );
    let f9_down = event(KeyEventKind::Down, F9_VK, F9_SCAN, false);
    assert_eq!(process(&mut state, &shared, f9_down), Decision::Consume);
    assert_eq!(shared.queue.len(), 1);
    assert_eq!(process(&mut state, &shared, f9_down), Decision::Consume);
    assert_eq!(
        shared.queue.len(),
        1,
        "repeat must not enqueue another action"
    );
    assert_eq!(
        process(
            &mut state,
            &shared,
            event(KeyEventKind::Up, F9_VK, F9_SCAN, false)
        ),
        Decision::Consume
    );
    assert_eq!(shared.queue.len(), 1);

    // Model the physical modifiers being released before the parked worker runs.
    assert_eq!(
        process(
            &mut state,
            &shared,
            modifier_event(KeyEventKind::Up, ConcreteModifier::LeftShift)
        ),
        Decision::Pass
    );
    assert_eq!(
        process(
            &mut state,
            &shared,
            modifier_event(KeyEventKind::Up, ConcreteModifier::LeftCtrl)
        ),
        Decision::Pass
    );
    assert_eq!(shared.physical.load(Ordering::Acquire), 0);
    worker.unpark();

    let batch = worker.receive();
    assert_eq!(
        batch,
        vec![
            expected(0, 0x1d, true, false),
            expected(0, 0x2a, true, false),
            expected(F10_VK, 0, false, false),
            expected(F10_VK, 0, false, true),
            expected(0, 0x2a, true, true),
            expected(0, 0x1d, true, true),
        ]
    );
    assert_eq!(worker.recorder.sends.load(Ordering::SeqCst), 1);
    worker.stop();
    assert_eq!(
        metrics.snapshot(),
        qbit_hotkeys::runtime::MetricsSnapshot {
            mapping_count: 0,
            events_seen: 7,
            events_matched: 1,
            queue_overflows: 0,
            actions_emitted: 1,
            injection_failures: 0,
            hook_active: false,
        }
    );
}

#[test]
fn generic_ctrl_shift_trigger_covers_left_right_both_and_absent_modifier_sides() {
    let published = Arc::new(PublishedKeymap::default());
    published.publish(compile(&[emit_mapping()]));

    for ctrl_left in [false, true] {
        for ctrl_right in [false, true] {
            for shift_left in [false, true] {
                for shift_right in [false, true] {
                    let shared =
                        Arc::new(Shared::new(published.clone(), Arc::new(Metrics::default())));
                    let mut state = HotPathState::default();
                    for (present, modifier) in [
                        (ctrl_left, ConcreteModifier::LeftCtrl),
                        (ctrl_right, ConcreteModifier::RightCtrl),
                        (shift_left, ConcreteModifier::LeftShift),
                        (shift_right, ConcreteModifier::RightShift),
                    ] {
                        if present {
                            assert_eq!(
                                process(
                                    &mut state,
                                    &shared,
                                    modifier_event(KeyEventKind::Down, modifier)
                                ),
                                Decision::Pass
                            );
                        }
                    }
                    let expected_decision =
                        if (ctrl_left || ctrl_right) && (shift_left || shift_right) {
                            Decision::Consume
                        } else {
                            Decision::Pass
                        };
                    let down = event(KeyEventKind::Down, F9_VK, F9_SCAN, false);
                    assert_eq!(process(&mut state, &shared, down), expected_decision);
                    assert_eq!(process(&mut state, &shared, down), expected_decision);
                    assert_eq!(
                        shared.queue.len(),
                        usize::from(expected_decision == Decision::Consume),
                        "one action only for each present Ctrl+Shift chord"
                    );
                    assert_eq!(
                        process(
                            &mut state,
                            &shared,
                            event(KeyEventKind::Up, F9_VK, F9_SCAN, false)
                        ),
                        expected_decision
                    );
                    if expected_decision == Decision::Consume {
                        shared
                            .queue
                            .pop()
                            .expect("matched press has one queued action");
                    }
                    for (present, modifier) in [
                        (ctrl_left, ConcreteModifier::LeftCtrl),
                        (ctrl_right, ConcreteModifier::RightCtrl),
                        (shift_left, ConcreteModifier::LeftShift),
                        (shift_right, ConcreteModifier::RightShift),
                    ] {
                        if present {
                            assert_eq!(
                                process(
                                    &mut state,
                                    &shared,
                                    modifier_event(KeyEventKind::Up, modifier)
                                ),
                                Decision::Pass
                            );
                        }
                    }
                    assert_eq!(state.modifiers().bits(), 0);
                }
            }
        }
    }
}

#[test]
fn disable_mapping_consumes_down_repeat_up_without_queuing_output() {
    let mapping = mapping(&[Modifier::Ctrl, Modifier::Shift], Action::Disable);
    let shared = Arc::new(Shared::new(
        Arc::new(PublishedKeymap::default()),
        Arc::new(Metrics::default()),
    ));
    shared.map.publish(compile(&[mapping]));
    let mut state = HotPathState::default();
    for modifier in [ConcreteModifier::LeftCtrl, ConcreteModifier::LeftShift] {
        assert_eq!(
            process(
                &mut state,
                &shared,
                modifier_event(KeyEventKind::Down, modifier)
            ),
            Decision::Pass
        );
    }
    let down = event(KeyEventKind::Down, F9_VK, F9_SCAN, false);
    assert_eq!(process(&mut state, &shared, down), Decision::Consume);
    assert_eq!(process(&mut state, &shared, down), Decision::Consume);
    assert_eq!(
        process(
            &mut state,
            &shared,
            event(KeyEventKind::Up, F9_VK, F9_SCAN, false)
        ),
        Decision::Consume
    );
    assert!(shared.queue.is_empty());
    assert_eq!(shared.metrics.snapshot().events_matched, 1);
    assert_eq!(shared.metrics.snapshot().actions_emitted, 0);
}

#[test]
fn unmatched_and_injected_origins_pass_without_changing_physical_state() {
    let shared = Arc::new(Shared::new(
        Arc::new(PublishedKeymap::default()),
        Arc::new(Metrics::default()),
    ));
    let mut state = HotPathState::default();
    assert_eq!(
        process(
            &mut state,
            &shared,
            event(KeyEventKind::Down, F9_VK, F9_SCAN, false)
        ),
        Decision::Pass
    );

    assert_eq!(classify_origin(false, false, 0), InputOrigin::Physical);
    assert_eq!(
        classify_origin(true, false, 0),
        InputOrigin::ThirdPartyInjected
    );
    assert_eq!(
        classify_origin(false, true, 0),
        InputOrigin::ThirdPartyInjected
    );
    assert_eq!(
        classify_origin(true, true, QBIT_INJECTION_TAG),
        InputOrigin::QbitInjected
    );
    for origin in [InputOrigin::QbitInjected, InputOrigin::ThirdPartyInjected] {
        let mut injected_modifier = modifier_event(KeyEventKind::Down, ConcreteModifier::RightCtrl);
        injected_modifier.origin = origin;
        assert_eq!(
            process(&mut state, &shared, injected_modifier),
            Decision::Pass
        );
        let mut injected_trigger = event(KeyEventKind::Down, F9_VK, F9_SCAN, false);
        injected_trigger.origin = origin;
        assert_eq!(
            process(&mut state, &shared, injected_trigger),
            Decision::Pass
        );
        assert_eq!(state.modifiers().bits(), 0);
    }
    assert!(shared.queue.is_empty());
    assert_eq!(shared.metrics.snapshot().events_matched, 0);
}

#[test]
fn map_disable_or_removal_preserves_held_key_up_consumption_then_passes_fresh_press() {
    for replacement in [
        vec![Mapping {
            enabled: false,
            ..emit_mapping()
        }],
        vec![],
    ] {
        let published = Arc::new(PublishedKeymap::default());
        published.publish(compile(&[emit_mapping()]));
        let shared = Arc::new(Shared::new(published.clone(), Arc::new(Metrics::default())));
        let mut state = HotPathState::default();
        for modifier in [ConcreteModifier::LeftCtrl, ConcreteModifier::LeftShift] {
            process(
                &mut state,
                &shared,
                modifier_event(KeyEventKind::Down, modifier),
            );
        }
        let down = event(KeyEventKind::Down, F9_VK, F9_SCAN, false);
        assert_eq!(process(&mut state, &shared, down), Decision::Consume);
        assert_eq!(shared.queue.len(), 1);
        assert_eq!(
            published.replace_mappings(&replacement).unwrap(),
            0,
            "disabled or removed mapping must publish an empty active map"
        );
        assert_eq!(
            process(
                &mut state,
                &shared,
                event(KeyEventKind::Up, F9_VK, F9_SCAN, false)
            ),
            Decision::Consume
        );
        assert_eq!(process(&mut state, &shared, down), Decision::Pass);
        assert_eq!(process(&mut state, &shared, down), Decision::Pass);
        assert_eq!(
            process(
                &mut state,
                &shared,
                event(KeyEventKind::Up, F9_VK, F9_SCAN, false)
            ),
            Decision::Pass
        );
        assert_eq!(
            shared.queue.len(),
            1,
            "a fresh unmapped press queues nothing"
        );
    }
}

#[test]
fn queue_overflow_fails_open_and_a_repeat_does_not_retry_enqueue() {
    let published = Arc::new(PublishedKeymap::default());
    let keymap = compile(&[emit_mapping()]);
    published.publish(keymap.clone());
    let shared = Arc::new(Shared::new(published, Arc::new(Metrics::default())));
    let request = keymap
        .lookup_event(
            Some(LogicalKey {
                vk: F9_VK as u16,
                extended: false,
            }),
            Some(PhysicalKey {
                scan_code: F9_SCAN as u16,
                extended: false,
            }),
            ModifierState::from_bits(
                ConcreteModifier::LeftCtrl.bit() | ConcreteModifier::LeftShift.bit(),
            ),
        )
        .expect("compiled Ctrl+Shift mapping matches its fixture");
    for _ in 0..64 {
        shared
            .queue
            .push(request)
            .expect("fixed queue has capacity 64");
    }
    assert_eq!(shared.queue.len(), 64);
    let mut state = HotPathState::default();
    for modifier in [ConcreteModifier::LeftCtrl, ConcreteModifier::LeftShift] {
        assert_eq!(
            process(
                &mut state,
                &shared,
                modifier_event(KeyEventKind::Down, modifier)
            ),
            Decision::Pass
        );
    }
    let down = event(KeyEventKind::Down, F9_VK, F9_SCAN, false);
    assert_eq!(process(&mut state, &shared, down), Decision::Pass);
    assert_eq!(shared.queue.len(), 64);
    assert_eq!(process(&mut state, &shared, down), Decision::Pass);
    assert_eq!(shared.queue.len(), 64, "overflowed repeat is not retried");
    assert_eq!(
        process(
            &mut state,
            &shared,
            event(KeyEventKind::Up, F9_VK, F9_SCAN, false)
        ),
        Decision::Pass
    );
    assert_eq!(shared.metrics.snapshot().queue_overflows, 1);
    assert_eq!(shared.metrics.snapshot().events_matched, 1);
}

#[test]
fn physical_priority_and_modifier_release_restoration_remain_exact() {
    let logical = Mapping {
        id: Uuid::from_u128(2),
        ..Mapping::with_id(
            Uuid::from_u128(2),
            true,
            Trigger::Chord {
                chord: chord([], "F9"),
            },
            Action::Disable,
            Scope::Global,
            MappingBehavior::default(),
            MappingMetadata::default(),
        )
    };
    let physical = Mapping::with_id(
        Uuid::from_u128(3),
        true,
        Trigger::Chord {
            chord: Chord::new([], KeyIdentity::Physical(KeyToken::new("F9").unwrap())).unwrap(),
        },
        Action::Disable,
        Scope::Global,
        MappingBehavior::default(),
        MappingMetadata::default(),
    );
    let keymap = compiled::compile(&[logical, physical]).unwrap();
    let request = keymap
        .lookup_event(
            Some(LogicalKey {
                vk: F9_VK as u16,
                extended: false,
            }),
            Some(PhysicalKey {
                scan_code: F9_SCAN as u16,
                extended: false,
            }),
            ModifierState::empty(),
        )
        .unwrap();
    assert_eq!(request.mapping_id, Uuid::from_u128(3));

    let NativeAction::EmitShortcut(output) = compiled::compile(&[emit_mapping()])
        .unwrap()
        .lookup_event(
            Some(LogicalKey {
                vk: F9_VK as u16,
                extended: false,
            }),
            Some(PhysicalKey {
                scan_code: F9_SCAN as u16,
                extended: false,
            }),
            ModifierState::from_bits(
                ConcreteModifier::LeftCtrl.bit() | ConcreteModifier::LeftShift.bit(),
            ),
        )
        .unwrap()
        .action
    else {
        panic!("fixture must emit a shortcut");
    };
    let plan = qbit_hotkeys::platform::windows::output::encode(
        output,
        ModifierState::from_bits(
            ConcreteModifier::LeftCtrl.bit() | ConcreteModifier::LeftAlt.bit(),
        ),
    )
    .unwrap();
    assert_eq!(
        plan.events(),
        &[
            expected(0, 0x38, true, true),
            expected(0, 0x2a, true, false),
            expected(F10_VK, 0, false, false),
            expected(F10_VK, 0, false, true),
            expected(0, 0x2a, true, true),
            expected(0, 0x38, true, false),
        ]
    );
}

struct GatedRecorder {
    sender: SyncSender<Vec<EncodedInput>>,
    release: Mutex<Option<Receiver<()>>>,
    sends: AtomicUsize,
}

impl Injector for GatedRecorder {
    fn send(&self, events: &[EncodedInput]) -> usize {
        self.sends.fetch_add(1, Ordering::SeqCst);
        self.sender
            .send(events.to_vec())
            .expect("bounded recorder is open");
        if let Some(release) = self.release.lock().expect("gate lock is healthy").take() {
            release
                .recv_timeout(Duration::from_secs(2))
                .expect("test releases blocked fake injection within its bound");
        }
        events.len()
    }
}

struct GatedWorker {
    shared: Arc<Shared>,
    worker: Option<Worker>,
    receiver: Receiver<Vec<EncodedInput>>,
    recorder: Arc<GatedRecorder>,
    release: SyncSender<()>,
}

impl GatedWorker {
    fn start(shared: Arc<Shared>) -> Self {
        let (sender, receiver) = mpsc::sync_channel(2);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let recorder = Arc::new(GatedRecorder {
            sender,
            release: Mutex::new(Some(release_rx)),
            sends: AtomicUsize::new(0),
        });
        let worker = Worker::start(shared.clone(), recorder.clone()).unwrap();
        Self {
            shared,
            worker: Some(worker),
            receiver,
            recorder,
            release: release_tx,
        }
    }

    fn receive(&self) -> Vec<EncodedInput> {
        self.receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("worker reaches recorder")
    }

    fn release_gate(&self) {
        let _ = self.release.try_send(());
    }

    fn stop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Release);
        self.release_gate();
        if let Some(worker) = self.worker.as_mut() {
            worker.thread.unpark();
            worker.stop().expect("gated worker stops");
        }
        self.worker = None;
    }
}

impl Drop for GatedWorker {
    fn drop(&mut self) {
        self.shared.cancelled.store(true, Ordering::Release);
        self.release_gate();
        if let Some(worker) = self.worker.as_mut() {
            worker.thread.unpark();
            let _ = worker.stop();
        }
        self.worker = None;
    }
}

#[test]
fn worker_uses_execution_time_modifiers_for_each_queued_action() {
    let mapping = mapping(
        &[],
        Action::EmitShortcut {
            chord: chord([Modifier::Ctrl, Modifier::Shift], "F10"),
        },
    );
    let keymap = compile(&[mapping]);
    let published = Arc::new(PublishedKeymap::default());
    published.publish(keymap.clone());
    let shared = Arc::new(Shared::new(published, Arc::new(Metrics::default())));
    let mut state = HotPathState::default();
    assert_eq!(
        process(
            &mut state,
            &shared,
            event(KeyEventKind::Down, F9_VK, F9_SCAN, false)
        ),
        Decision::Consume
    );

    let request: ActionRequest = keymap
        .lookup_event(
            Some(LogicalKey {
                vk: F9_VK as u16,
                extended: false,
            }),
            Some(PhysicalKey {
                scan_code: F9_SCAN as u16,
                extended: false,
            }),
            ModifierState::empty(),
        )
        .expect("compiled trigger exists");
    let mut worker = GatedWorker::start(shared.clone());
    worker.worker.as_ref().unwrap().thread.unpark();
    let first = worker.receive();
    assert_eq!(
        first,
        vec![
            expected(0, 0x1d, true, false),
            expected(0, 0x2a, true, false),
            expected(F10_VK, 0, false, false),
            expected(F10_VK, 0, false, true),
            expected(0, 0x2a, true, true),
            expected(0, 0x1d, true, true),
        ]
    );

    shared
        .queue
        .push(request)
        .expect("second accepted request fits");
    shared
        .physical
        .store(ConcreteModifier::RightCtrl.bit(), Ordering::Release);
    worker.release_gate();
    let second = worker.receive();
    assert_eq!(
        second,
        vec![
            expected(0, 0x2a, true, false),
            expected(F10_VK, 0, false, false),
            expected(F10_VK, 0, false, true),
            expected(0, 0x2a, true, true),
        ],
        "worker preserves right Ctrl that is physically held at execution time"
    );
    assert_eq!(worker.recorder.sends.load(Ordering::SeqCst), 2);
    worker.stop();
}

#[test]
fn full_and_short_worker_sends_are_counted_once_without_retry() {
    let keymap = compile(&[mapping(
        &[],
        Action::EmitShortcut {
            chord: chord([], "F10"),
        },
    )]);
    let request = keymap
        .lookup_event(
            Some(LogicalKey {
                vk: F9_VK as u16,
                extended: false,
            }),
            Some(PhysicalKey {
                scan_code: F9_SCAN as u16,
                extended: false,
            }),
            ModifierState::empty(),
        )
        .unwrap();

    for short_send in [false, true] {
        let published = Arc::new(PublishedKeymap::default());
        published.publish(keymap.clone());
        let metrics = Arc::new(Metrics::default());
        let shared = Arc::new(Shared::new(published, metrics.clone()));
        let mut worker = WorkerFixture::start(shared.clone(), short_send);
        shared.queue.push(request).unwrap();
        worker.unpark();
        let batch = worker.receive();
        assert_eq!(
            batch,
            vec![
                expected(F10_VK, 0, false, false),
                expected(F10_VK, 0, false, true)
            ]
        );
        assert_eq!(worker.recorder.sends.load(Ordering::SeqCst), 1);
        worker.stop();
        assert!(shared.queue.is_empty());
        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.actions_emitted, usize::from(!short_send));
        assert_eq!(snapshot.injection_failures, usize::from(short_send));
        assert_eq!(
            worker.recorder.sends.load(Ordering::SeqCst),
            1,
            "short output is never retried"
        );
    }
}

#[test]
fn worker_shutdown_finishes_current_send_then_leaves_later_queued_work_unexecuted() {
    let keymap = compile(&[mapping(
        &[],
        Action::EmitShortcut {
            chord: chord([], "F10"),
        },
    )]);
    let request = keymap
        .lookup_event(
            Some(LogicalKey {
                vk: F9_VK as u16,
                extended: false,
            }),
            Some(PhysicalKey {
                scan_code: F9_SCAN as u16,
                extended: false,
            }),
            ModifierState::empty(),
        )
        .unwrap();
    let published = Arc::new(PublishedKeymap::default());
    published.publish(keymap);
    let shared = Arc::new(Shared::new(published, Arc::new(Metrics::default())));
    let mut worker = GatedWorker::start(shared.clone());
    shared.queue.push(request).unwrap();
    worker.worker.as_ref().unwrap().thread.unpark();
    let _in_flight = worker.receive();
    shared.queue.push(request).unwrap();
    shared.cancelled.store(true, Ordering::Release);
    worker.release_gate();
    worker
        .worker
        .as_mut()
        .unwrap()
        .stop()
        .expect("current send completes and stop is acknowledged");
    worker.worker = None;
    assert_eq!(worker.recorder.sends.load(Ordering::SeqCst), 1);
    assert_eq!(
        shared.queue.len(),
        1,
        "the shutdown contract does not drain queued output actions"
    );
}
