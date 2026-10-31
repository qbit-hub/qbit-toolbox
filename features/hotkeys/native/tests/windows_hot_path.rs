#![cfg(windows)]

use crossbeam_queue::ArrayQueue;
use qbit_hotkeys::{
    domain::{
        Action, Chord, KeyIdentity, KeyToken, Mapping, MappingBehavior, MappingMetadata, Scope,
        Trigger,
    },
    platform::windows::{
        compiled::{ActionRequest, NativeAction, NativeCompiledKeymap, compile},
        hot_path::{
            Decision, HotPathState, InputOrigin, KeyEventKind, QBIT_INJECTION_TAG, RawEvent,
            classify_origin,
        },
    },
    runtime::Metrics,
};
use uuid::Uuid;

const KEY_A_VK: u32 = 0x41;
const KEY_A_SCAN: u32 = 0x1e;

fn event(kind: KeyEventKind, vk_code: u32, scan_code: u32) -> RawEvent {
    RawEvent {
        kind,
        vk_code,
        scan_code,
        extended: false,
        origin: InputOrigin::Physical,
    }
}

fn mapping(id: &str, physical: bool, disable: bool) -> Mapping {
    let token = KeyToken::new("KeyA").unwrap();
    let primary = if physical {
        KeyIdentity::Physical(token)
    } else {
        KeyIdentity::Logical(token)
    };
    Mapping::with_id(
        Uuid::parse_str(id).unwrap(),
        true,
        Trigger::Chord {
            chord: Chord::new([], primary).unwrap(),
        },
        if disable {
            Action::Disable
        } else {
            Action::EmitShortcut {
                chord: Chord::new([], KeyIdentity::Logical(KeyToken::new("KeyB").unwrap()))
                    .unwrap(),
            }
        },
        Scope::Global,
        MappingBehavior::default(),
        MappingMetadata::default(),
    )
}

fn compiled(id: &str, physical: bool, disable: bool) -> NativeCompiledKeymap {
    compile(&[mapping(id, physical, disable)]).expect("test mapping must compile")
}

fn process(
    state: &mut HotPathState,
    event: RawEvent,
    keymap: &NativeCompiledKeymap,
    queue: &ArrayQueue<ActionRequest>,
    metrics: &Metrics,
) -> Decision {
    state.process(event, keymap, queue, metrics)
}

#[test]
fn classifies_origins_with_qbit_precedence() {
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
}

#[test]
fn injected_events_bypass_mapping_state_and_queue_but_are_seen() {
    let keymap = compiled("00000000-0000-0000-0000-000000000001", false, false);
    let queue = ArrayQueue::new(1);
    let metrics = Metrics::default();
    let mut state = HotPathState::default();
    let mut injected = event(KeyEventKind::Down, KEY_A_VK, KEY_A_SCAN);
    injected.origin = InputOrigin::QbitInjected;
    assert_eq!(
        process(&mut state, injected, &keymap, &queue, &metrics),
        Decision::Pass
    );
    injected.origin = InputOrigin::ThirdPartyInjected;
    assert_eq!(
        process(&mut state, injected, &keymap, &queue, &metrics),
        Decision::Pass
    );
    assert_eq!(state.modifiers().bits(), 0);
    for origin in [InputOrigin::QbitInjected, InputOrigin::ThirdPartyInjected] {
        let mut modifier = event(KeyEventKind::Down, 0x11, 0x1d);
        modifier.origin = origin;
        modifier.extended = true;
        assert_eq!(
            process(&mut state, modifier, &keymap, &queue, &metrics),
            Decision::Pass
        );
        assert_eq!(state.modifiers().bits(), 0);
    }
    assert!(queue.is_empty());
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.events_seen, 4);
    assert_eq!(snapshot.events_matched, 0);
}

#[test]
fn physical_left_and_right_modifiers_update_state_and_pass() {
    let keymap = NativeCompiledKeymap::default();
    let queue = ArrayQueue::new(1);
    let metrics = Metrics::default();
    let mut state = HotPathState::default();
    assert_eq!(
        process(
            &mut state,
            event(KeyEventKind::Down, 0x11, 0x1d),
            &keymap,
            &queue,
            &metrics
        ),
        Decision::Pass
    );
    let mut right = event(KeyEventKind::Down, 0x11, 0x1d);
    right.extended = true;
    assert_eq!(
        process(&mut state, right, &keymap, &queue, &metrics),
        Decision::Pass
    );
    assert_eq!(state.modifiers().bits(), 3);
    right.kind = KeyEventKind::Up;
    assert_eq!(
        process(&mut state, right, &keymap, &queue, &metrics),
        Decision::Pass
    );
    assert_eq!(state.modifiers().bits(), 1);
    assert_eq!(
        process(
            &mut state,
            event(KeyEventKind::Down, 0x11, 0x1d),
            &keymap,
            &queue,
            &metrics
        ),
        Decision::Pass
    );
    assert_ne!(state.modifiers().bits(), 0);
    assert_eq!(
        process(
            &mut state,
            event(KeyEventKind::Up, 0x11, 0x1d),
            &keymap,
            &queue,
            &metrics
        ),
        Decision::Pass
    );
    assert_eq!(
        process(
            &mut state,
            event(KeyEventKind::Down, 0x11, 0x1d),
            &keymap,
            &queue,
            &metrics
        ),
        Decision::Pass
    );
}

#[test]
fn emit_lifecycle_enqueues_once_per_press_and_worker_owns_emission_metric() {
    let keymap = compiled("00000000-0000-0000-0000-000000000002", false, false);
    let queue = ArrayQueue::new(2);
    let metrics = Metrics::default();
    let mut state = HotPathState::default();
    let down = event(KeyEventKind::Down, KEY_A_VK, KEY_A_SCAN);
    let up = event(KeyEventKind::Up, KEY_A_VK, KEY_A_SCAN);
    assert_eq!(
        process(&mut state, down, &keymap, &queue, &metrics),
        Decision::Consume
    );
    assert_eq!(queue.len(), 1);
    assert_eq!(metrics.snapshot().events_matched, 1);
    assert_eq!(metrics.snapshot().actions_emitted, 0);
    assert_eq!(
        process(&mut state, down, &keymap, &queue, &metrics),
        Decision::Consume
    );
    assert_eq!(queue.len(), 1);
    assert_eq!(metrics.snapshot().events_matched, 1);
    assert_eq!(
        process(&mut state, up, &keymap, &queue, &metrics),
        Decision::Consume
    );
    queue.pop();
    assert_eq!(
        process(&mut state, down, &keymap, &queue, &metrics),
        Decision::Consume
    );
    assert_eq!(queue.len(), 1);
    assert_eq!(metrics.snapshot().events_matched, 2);
    assert_eq!(metrics.snapshot().actions_emitted, 0);
}

#[test]
fn disable_lifecycle_consumes_down_repeat_and_up_without_queueing() {
    let keymap = compiled("00000000-0000-0000-0000-000000000003", false, true);
    let queue = ArrayQueue::new(1);
    let metrics = Metrics::default();
    let mut state = HotPathState::default();
    for kind in [KeyEventKind::Down, KeyEventKind::Down, KeyEventKind::Up] {
        assert_eq!(
            process(
                &mut state,
                event(kind, KEY_A_VK, KEY_A_SCAN),
                &keymap,
                &queue,
                &metrics
            ),
            Decision::Consume
        );
    }
    assert!(queue.is_empty());
    assert_eq!(metrics.snapshot().events_matched, 1);
}

#[test]
fn full_queue_fails_open_and_retries_only_after_a_new_press() {
    let mapping_id = Uuid::from_u128(4);
    let keymap = compiled("00000000-0000-0000-0000-000000000004", false, false);
    let queue = ArrayQueue::new(1);
    queue
        .push(ActionRequest {
            mapping_id,
            action: NativeAction::Disable,
        })
        .unwrap();
    let metrics = Metrics::default();
    let mut state = HotPathState::default();
    let down = event(KeyEventKind::Down, KEY_A_VK, KEY_A_SCAN);
    let up = event(KeyEventKind::Up, KEY_A_VK, KEY_A_SCAN);
    assert_eq!(
        process(&mut state, down, &keymap, &queue, &metrics),
        Decision::Pass
    );
    assert_eq!(metrics.snapshot().queue_overflows, 1);
    assert_eq!(metrics.snapshot().events_matched, 1);
    // Drain while held: autorepeat must still pass rather than retry enqueue.
    queue.pop();
    assert_eq!(
        process(&mut state, down, &keymap, &queue, &metrics),
        Decision::Pass
    );
    assert_eq!(metrics.snapshot().queue_overflows, 1);
    assert_eq!(
        process(&mut state, up, &keymap, &queue, &metrics),
        Decision::Pass
    );
    queue.pop();
    assert_eq!(
        process(&mut state, down, &keymap, &queue, &metrics),
        Decision::Consume
    );
    assert_eq!(queue.len(), 1);
}

#[test]
fn nonmatched_repeat_and_up_pass_without_matching() {
    let keymap = NativeCompiledKeymap::default();
    let queue = ArrayQueue::new(1);
    let metrics = Metrics::default();
    let mut state = HotPathState::default();
    for kind in [KeyEventKind::Down, KeyEventKind::Down, KeyEventKind::Up] {
        assert_eq!(
            process(
                &mut state,
                event(kind, KEY_A_VK, KEY_A_SCAN),
                &keymap,
                &queue,
                &metrics
            ),
            Decision::Pass
        );
    }
    assert_eq!(metrics.snapshot().events_matched, 0);
}

#[test]
fn consumed_keyup_survives_mapping_removal() {
    let keymap = compiled("00000000-0000-0000-0000-000000000005", false, true);
    let queue = ArrayQueue::new(1);
    let metrics = Metrics::default();
    let mut state = HotPathState::default();
    assert_eq!(
        process(
            &mut state,
            event(KeyEventKind::Down, KEY_A_VK, KEY_A_SCAN),
            &keymap,
            &queue,
            &metrics
        ),
        Decision::Consume
    );
    assert_eq!(
        process(
            &mut state,
            event(KeyEventKind::Up, KEY_A_VK, KEY_A_SCAN),
            &NativeCompiledKeymap::default(),
            &queue,
            &metrics
        ),
        Decision::Consume
    );
}

#[test]
fn physical_mapping_wins_over_logical_through_hot_path_state() {
    let physical_id = "00000000-0000-0000-0000-000000000006";
    let logical_id = "00000000-0000-0000-0000-000000000007";
    let keymap = compile(&[
        mapping(logical_id, false, false),
        mapping(physical_id, true, false),
    ])
    .unwrap();
    let queue = ArrayQueue::new(1);
    let metrics = Metrics::default();
    let mut state = HotPathState::default();
    assert_eq!(
        process(
            &mut state,
            event(KeyEventKind::Down, KEY_A_VK, KEY_A_SCAN),
            &keymap,
            &queue,
            &metrics
        ),
        Decision::Consume
    );
    assert_eq!(
        queue.pop().unwrap().mapping_id,
        Uuid::parse_str(physical_id).unwrap()
    );
}

#[test]
fn zero_scan_vk_fallback_has_a_clean_press_cycle() {
    let keymap = compiled("00000000-0000-0000-0000-000000000008", false, false);
    let queue = ArrayQueue::new(2);
    let metrics = Metrics::default();
    let mut state = HotPathState::default();
    for kind in [KeyEventKind::Down, KeyEventKind::Down, KeyEventKind::Up] {
        assert_eq!(
            process(
                &mut state,
                event(kind, KEY_A_VK, 0),
                &keymap,
                &queue,
                &metrics
            ),
            Decision::Consume
        );
    }
    queue.pop();
    assert_eq!(
        process(
            &mut state,
            event(KeyEventKind::Down, KEY_A_VK, 0),
            &keymap,
            &queue,
            &metrics
        ),
        Decision::Consume
    );
}

#[test]
fn malformed_raw_codes_fail_open_without_poisoning_normal_input() {
    let keymap = compiled("00000000-0000-0000-0000-000000000009", false, false);
    let queue = ArrayQueue::new(1);
    let metrics = Metrics::default();
    let mut state = HotPathState::default();
    assert_eq!(
        process(
            &mut state,
            event(KeyEventKind::Down, 0x11, 256),
            &keymap,
            &queue,
            &metrics
        ),
        Decision::Pass
    );
    assert_eq!(state.modifiers().bits(), 0);
    for kind in [KeyEventKind::Down, KeyEventKind::Down, KeyEventKind::Up] {
        assert_eq!(
            process(&mut state, event(kind, 256, 256), &keymap, &queue, &metrics),
            Decision::Pass
        );
    }
    assert_eq!(
        process(
            &mut state,
            event(KeyEventKind::Down, KEY_A_VK, KEY_A_SCAN),
            &keymap,
            &queue,
            &metrics
        ),
        Decision::Consume
    );
}
