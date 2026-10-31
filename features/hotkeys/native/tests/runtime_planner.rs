use proptest::prelude::*;
use qbit_hotkeys::domain::{Modifier, ModifierFamily};
use qbit_hotkeys::runtime::{
    ConcreteModifier, Metrics, MetricsSnapshot, ModifierState, PlanStep, PlannerError,
    plan_keystroke,
};

use ConcreteModifier::*;
use PlanStep::*;

fn state(modifiers: &[ConcreteModifier]) -> ModifierState {
    let mut state = ModifierState::empty();
    for &modifier in modifiers {
        state.press(modifier);
    }
    state
}

fn steps(physical: ModifierState, target: &[Modifier]) -> Vec<PlanStep> {
    plan_keystroke(physical, target).unwrap().steps().collect()
}

#[test]
fn modifier_state_masks_and_iterates_in_runtime_order() {
    let mut state = ModifierState::from_bits(0xff03);
    assert_eq!(state.bits(), 0x03);
    assert!(state.contains(LeftCtrl));
    assert!(state.contains(RightCtrl));
    assert!(state.contains_family(ModifierFamily::Ctrl));
    assert!(!state.contains_family(ModifierFamily::Shift));
    state.release(LeftCtrl);
    state.press(RightMeta);
    assert_eq!(state.iter().collect::<Vec<_>>(), vec![RightCtrl, RightMeta]);
}

#[test]
fn ctrl_to_meta_releases_presses_and_restores() {
    assert_eq!(
        steps(state(&[LeftCtrl]), &[Modifier::Meta]),
        vec![
            ReleaseModifier(LeftCtrl),
            PressModifier(LeftMeta),
            PrimaryDown,
            PrimaryUp,
            ReleaseModifier(LeftMeta),
            PressModifier(LeftCtrl),
        ]
    );
}

#[test]
fn generic_target_reuses_held_ctrl_sides() {
    assert_eq!(
        steps(state(&[LeftCtrl]), &[Modifier::Ctrl]),
        vec![PrimaryDown, PrimaryUp]
    );
    assert_eq!(
        steps(state(&[RightCtrl]), &[Modifier::Ctrl]),
        vec![PrimaryDown, PrimaryUp]
    );
    assert_eq!(
        steps(state(&[LeftCtrl, RightCtrl]), &[Modifier::Ctrl]),
        vec![PrimaryDown, PrimaryUp]
    );
}

#[test]
fn sided_target_replaces_the_other_held_side() {
    assert_eq!(
        steps(state(&[LeftCtrl]), &[Modifier::RightCtrl]),
        vec![
            ReleaseModifier(LeftCtrl),
            PressModifier(RightCtrl),
            PrimaryDown,
            PrimaryUp,
            ReleaseModifier(RightCtrl),
            PressModifier(LeftCtrl),
        ]
    );
}

#[test]
fn unrelated_modifiers_are_released_and_restored() {
    assert_eq!(
        steps(state(&[LeftShift, RightAlt]), &[]),
        vec![
            ReleaseModifier(RightAlt),
            ReleaseModifier(LeftShift),
            PrimaryDown,
            PrimaryUp,
            PressModifier(LeftShift),
            PressModifier(RightAlt),
        ]
    );
}

#[test]
fn supports_both_sides_no_target_and_left_default() {
    assert_eq!(
        steps(
            ModifierState::empty(),
            &[Modifier::LeftCtrl, Modifier::RightCtrl]
        ),
        vec![
            PressModifier(LeftCtrl),
            PressModifier(RightCtrl),
            PrimaryDown,
            PrimaryUp,
            ReleaseModifier(RightCtrl),
            ReleaseModifier(LeftCtrl),
        ]
    );
    assert_eq!(
        steps(ModifierState::empty(), &[]),
        vec![PrimaryDown, PrimaryUp]
    );
    assert_eq!(
        steps(ModifierState::empty(), &[Modifier::Ctrl]),
        vec![
            PressModifier(LeftCtrl),
            PrimaryDown,
            PrimaryUp,
            ReleaseModifier(LeftCtrl),
        ]
    );
}

#[test]
fn invalid_targets_are_typed_errors() {
    assert_eq!(
        plan_keystroke(ModifierState::empty(), &[Modifier::Ctrl, Modifier::Ctrl]),
        Err(PlannerError::DuplicateTargetModifier(Modifier::Ctrl))
    );
    assert_eq!(
        plan_keystroke(
            ModifierState::empty(),
            &[Modifier::Ctrl, Modifier::LeftCtrl]
        ),
        Err(PlannerError::AmbiguousTargetModifierFamily(
            ModifierFamily::Ctrl
        ))
    );
}

#[test]
fn plans_reach_target_at_primary_and_restore_physical_state() {
    for bits in 0u16..=0xff {
        let physical = ModifierState::from_bits(bits);
        for requirements in 0u8..=80 {
            let target = target_for(requirements);
            let plan = plan_keystroke(physical, &target).unwrap();
            let (at_primary, final_state) =
                execute_plan(physical, &plan.steps().collect::<Vec<_>>());
            assert_eq!(final_state, physical);
            assert_eq!(at_primary, expected_target(physical, &target));
            assert_phase_steps_are_unique(&plan.steps().collect::<Vec<_>>());
        }
    }
}

proptest! {
    #[test]
    fn planner_property_reaches_target_and_restores(
        physical_bits in any::<u8>(),
        selector in 0u8..81,
    ) {
        let physical = ModifierState::from_bits(physical_bits.into());
        let target = target_for(selector);
        let plan = plan_keystroke(physical, &target).unwrap();
        let steps = plan.steps().collect::<Vec<_>>();
        let (at_primary, final_state) = execute_plan(physical, &steps);

        prop_assert_eq!(at_primary, expected_target(physical, &target));
        prop_assert_eq!(final_state, physical);
        assert_phase_steps_are_unique(&steps);
    }
}

fn target_for(selector: u8) -> Vec<Modifier> {
    let generic = [
        Modifier::Ctrl,
        Modifier::Shift,
        Modifier::Alt,
        Modifier::Meta,
    ];
    let left = [
        Modifier::LeftCtrl,
        Modifier::LeftShift,
        Modifier::LeftAlt,
        Modifier::LeftMeta,
    ];
    let right = [
        Modifier::RightCtrl,
        Modifier::RightShift,
        Modifier::RightAlt,
        Modifier::RightMeta,
    ];
    let mut target = Vec::new();
    for family in 0..4 {
        match (selector / 3u8.pow(family as u32)) % 3 {
            1 => target.push(generic[family]),
            2 => target.extend([left[family], right[family]]),
            _ => {}
        }
    }
    target
}

fn execute_plan(physical: ModifierState, steps: &[PlanStep]) -> (ModifierState, ModifierState) {
    let mut execution = physical;
    let mut at_primary = None;
    for &step in steps {
        match step {
            ReleaseModifier(modifier) => execution.release(modifier),
            PressModifier(modifier) => execution.press(modifier),
            PrimaryDown => at_primary = Some(execution),
            PrimaryUp => {}
        }
    }
    (
        at_primary.expect("every plan includes primary input"),
        execution,
    )
}

fn assert_phase_steps_are_unique(steps: &[PlanStep]) {
    let primary_down = steps.iter().position(|step| *step == PrimaryDown).unwrap();
    let primary_up = steps.iter().position(|step| *step == PrimaryUp).unwrap();
    let mut before_releases = [false; 8];
    let mut before_presses = [false; 8];
    let mut after_releases = [false; 8];
    let mut after_presses = [false; 8];

    for (index, &step) in steps.iter().enumerate() {
        let (seen, modifier) = match step {
            ReleaseModifier(modifier) if index < primary_down => (&mut before_releases, modifier),
            PressModifier(modifier) if index < primary_down => (&mut before_presses, modifier),
            ReleaseModifier(modifier) if index > primary_up => (&mut after_releases, modifier),
            PressModifier(modifier) if index > primary_up => (&mut after_presses, modifier),
            _ => continue,
        };
        let slot = modifier as usize;
        assert!(
            !seen[slot],
            "modifier {modifier:?} repeated within one planner phase"
        );
        seen[slot] = true;
    }
}

fn expected_target(physical: ModifierState, target: &[Modifier]) -> ModifierState {
    let mut expected = ModifierState::empty();
    for family in [
        ModifierFamily::Ctrl,
        ModifierFamily::Shift,
        ModifierFamily::Alt,
        ModifierFamily::Meta,
    ] {
        let generic = target.iter().any(|modifier| {
            matches!(
                (family, modifier),
                (ModifierFamily::Ctrl, Modifier::Ctrl)
                    | (ModifierFamily::Shift, Modifier::Shift)
                    | (ModifierFamily::Alt, Modifier::Alt)
                    | (ModifierFamily::Meta, Modifier::Meta)
            )
        });
        if generic {
            for modifier in ConcreteModifier::ORDER {
                if modifier.family() == family && physical.contains(modifier) {
                    expected.press(modifier);
                }
            }
            if !expected.contains_family(family) {
                expected.press(match family {
                    ModifierFamily::Ctrl => LeftCtrl,
                    ModifierFamily::Shift => LeftShift,
                    ModifierFamily::Alt => LeftAlt,
                    ModifierFamily::Meta => LeftMeta,
                });
            }
        } else {
            for &modifier in target {
                if let Some(concrete) = ConcreteModifier::from_domain_modifier(modifier)
                    && concrete.family() == family
                {
                    expected.press(concrete);
                }
            }
        }
    }
    expected
}

#[test]
fn metrics_snapshot_is_observational() {
    let metrics = Metrics::default();
    metrics.set_mapping_count(4);
    metrics.record_event_seen();
    metrics.record_event_matched();
    metrics.record_queue_overflow();
    metrics.record_action_emitted();
    metrics.record_injection_failure();
    metrics.set_hook_active(true);
    assert_eq!(
        metrics.snapshot(),
        MetricsSnapshot {
            mapping_count: 4,
            events_seen: 1,
            events_matched: 1,
            queue_overflows: 1,
            actions_emitted: 1,
            injection_failures: 1,
            hook_active: true,
        }
    );
}
