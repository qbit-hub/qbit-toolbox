#![cfg(windows)]
use qbit_hotkeys::{
    domain::*,
    platform::windows::{
        compiled::{self, NativeAction, PublishedKeymap},
        engine::{Shared, Worker},
        hot_path::QBIT_INJECTION_TAG,
        keys::{LogicalKey, PhysicalKey},
        output::{self, EncodedInput, Injector},
    },
    runtime::{Metrics, ModifierState},
};
use std::sync::{Arc, Mutex, atomic::Ordering, mpsc};
use std::time::Duration;

fn mapping(identity: KeyIdentity, modifiers: Vec<Modifier>) -> Mapping {
    Mapping::new(
        true,
        Trigger::Chord {
            chord: Chord::new([], KeyIdentity::Logical(KeyToken::new("KeyA").unwrap())).unwrap(),
        },
        Action::EmitShortcut {
            chord: Chord::new(modifiers, identity).unwrap(),
        },
        Scope::Global,
        MappingBehavior::default(),
        MappingMetadata::default(),
    )
}
fn request(mapping: Mapping) -> compiled::ActionRequest {
    compiled::compile(&[mapping])
        .unwrap()
        .lookup(
            LogicalKey {
                vk: 0x41,
                extended: false,
            },
            PhysicalKey {
                scan_code: 0x1e,
                extended: false,
            },
            ModifierState::empty(),
        )
        .unwrap()
}
struct Recorder {
    sender: Mutex<mpsc::Sender<Vec<EncodedInput>>>,
    short: bool,
}
impl Injector for Recorder {
    fn send(&self, events: &[EncodedInput]) -> usize {
        self.sender.lock().unwrap().send(events.to_vec()).unwrap();
        events.len() - usize::from(self.short)
    }
}
#[test]
fn all_sided_modifiers_have_exact_scan_encoding_and_tags() {
    for (modifier, scan, extended) in [
        (Modifier::LeftCtrl, 0x1d, false),
        (Modifier::RightCtrl, 0x1d, true),
        (Modifier::LeftShift, 0x2a, false),
        (Modifier::RightShift, 0x36, false),
        (Modifier::LeftAlt, 0x38, false),
        (Modifier::RightAlt, 0x38, true),
        (Modifier::LeftMeta, 0x5b, true),
        (Modifier::RightMeta, 0x5c, true),
    ] {
        let NativeAction::EmitShortcut(output) = request(mapping(
            KeyIdentity::Physical(KeyToken::new("Enter").unwrap()),
            vec![modifier],
        ))
        .action
        else {
            panic!()
        };
        let batch = output::encode(output, ModifierState::empty()).unwrap();
        assert_eq!(batch.events().len(), 4);
        assert!(
            batch
                .events()
                .iter()
                .all(|e| e.tag == QBIT_INJECTION_TAG && e.vk == 0 && e.scan_mode)
        );
        assert_eq!(
            (
                batch.events()[0].scan,
                batch.events()[0].extended,
                batch.events()[0].up
            ),
            (scan, extended, false)
        );
        assert_eq!(
            (
                batch.events()[3].scan,
                batch.events()[3].extended,
                batch.events()[3].up
            ),
            (scan, extended, true)
        );
        assert_eq!(batch.events()[1].scan, 0x1c);
        assert!(!batch.events()[1].up);
        assert!(batch.events()[2].up);
    }
}
#[test]
fn logical_encoding_and_partial_send_never_report_success_or_retry() {
    let NativeAction::EmitShortcut(output) = request(mapping(
        KeyIdentity::Logical(KeyToken::new("NumpadEnter").unwrap()),
        vec![],
    ))
    .action
    else {
        panic!()
    };
    for short in [false, true] {
        let (tx, rx) = mpsc::channel();
        let metrics = Metrics::default();
        output::execute(
            &Recorder {
                sender: Mutex::new(tx),
                short,
            },
            output,
            ModifierState::empty(),
            &metrics,
        );
        let events = rx.recv().unwrap();
        assert_eq!(
            (
                events[0].vk,
                events[0].scan,
                events[0].extended,
                events[0].scan_mode
            ),
            (0x0d, 0, true, false)
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(metrics.snapshot().actions_emitted, usize::from(!short));
        assert_eq!(metrics.snapshot().injection_failures, usize::from(short));
    }
}
#[test]
fn parked_worker_uses_execution_time_modifiers_and_stops_repeatedly() {
    for _ in 0..32 {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared::new(
            Arc::new(PublishedKeymap::default()),
            Arc::new(Metrics::default()),
        ));
        let mut worker = Worker::start(
            shared.clone(),
            Arc::new(Recorder {
                sender: Mutex::new(tx),
                short: false,
            }),
        )
        .unwrap();
        let request = request(mapping(
            KeyIdentity::Logical(KeyToken::new("KeyB").unwrap()),
            vec![Modifier::Ctrl],
        ));
        // Physical Ctrl is released after trigger time and before execution.
        shared.physical.store(0, Ordering::Release);
        shared.queue.push(request).unwrap();
        worker.thread.unpark();
        let batch = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(batch.len(), 4); // Must press and release Ctrl at execution.
        assert_eq!(batch[0].scan, 0x1d);
        shared.cancelled.store(true, Ordering::Release);
        worker.stop().unwrap();
        worker.stop().unwrap();
        assert_eq!(shared.metrics.snapshot().actions_emitted, 1);
    }
}

#[test]
#[ignore = "interactive opt-in only: installs a global hook; run on an isolated Windows desktop"]
fn real_hook_lifecycle_opt_in() {
    assert_eq!(
        std::env::var("QBIT_INTERACTIVE_HOTKEYS_TEST").as_deref(),
        Ok("1")
    );
    let metrics = Arc::new(Metrics::default());
    let mut engine = qbit_hotkeys::platform::windows::engine::WindowsEngine::start(
        Arc::new(PublishedKeymap::default()),
        metrics.clone(),
    )
    .unwrap();
    assert!(metrics.snapshot().hook_active);
    engine.stop().unwrap();
    engine.stop().unwrap();
    assert!(!metrics.snapshot().hook_active);
}
