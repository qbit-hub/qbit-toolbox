#![cfg(windows)]

use qbit_hotkeys::domain::{
    Action, Chord, KeyIdentity, KeyToken, Mapping, MappingBehavior, MappingMetadata, Modifier,
    Scope, Trigger,
};
use qbit_hotkeys::platform::windows::compiled::{
    NativeAction, PlatformCompileError, PublishedKeymap, compile,
};
use qbit_hotkeys::platform::windows::keys::{
    KeyIdentityKind, KeyRole, KeyTranslationError, NativeKey, modifier_from_raw, translate,
};
use qbit_hotkeys::runtime::{ConcreteModifier, ModifierState};
use uuid::Uuid;
use windows::Win32::UI::Input::KeyboardAndMouse::*;

fn identity(token: &str, physical: bool) -> KeyIdentity {
    let token = KeyToken::new(token).unwrap();
    if physical {
        KeyIdentity::Physical(token)
    } else {
        KeyIdentity::Logical(token)
    }
}

fn chord(modifiers: impl IntoIterator<Item = Modifier>, token: &str, physical: bool) -> Chord {
    Chord::new(modifiers, identity(token, physical)).unwrap()
}

fn mapping(
    id: u128,
    enabled: bool,
    modifiers: impl IntoIterator<Item = Modifier>,
    token: &str,
    physical: bool,
    action: Action,
) -> Mapping {
    Mapping::with_id(
        Uuid::from_u128(id),
        enabled,
        Trigger::Chord {
            chord: chord(modifiers, token, physical),
        },
        action,
        Scope::Global,
        MappingBehavior::default(),
        MappingMetadata::default(),
    )
}

fn disable(id: u128, modifiers: impl IntoIterator<Item = Modifier>, key: &str) -> Mapping {
    mapping(id, true, modifiers, key, false, Action::Disable)
}

fn logical(token: &str) -> qbit_hotkeys::platform::windows::keys::LogicalKey {
    match translate(&identity(token, false), KeyRole::Trigger).unwrap() {
        NativeKey::Logical(key) => key,
        NativeKey::Physical(_) => unreachable!(),
    }
}

fn physical(token: &str) -> qbit_hotkeys::platform::windows::keys::PhysicalKey {
    match translate(&identity(token, true), KeyRole::Trigger).unwrap() {
        NativeKey::Physical(key) => key,
        NativeKey::Logical(_) => unreachable!(),
    }
}

#[test]
fn logical_translation_preserves_windows_distinctions() {
    assert_eq!(logical("KeyA").vk, VK_A.0);
    assert_eq!(logical("F24").vk, VK_F24.0);
    assert_eq!(logical("ArrowLeft").vk, VK_LEFT.0);
    assert!(!logical("Enter").extended);
    assert!(logical("NumpadEnter").extended);
    assert_eq!(logical("Enter").vk, logical("NumpadEnter").vk);
    assert_ne!(logical("Backslash").vk, logical("IntlBackslash").vk);
}

#[test]
fn physical_translation_uses_the_explicit_set_one_table() {
    assert_eq!(physical("KeyA").scan_code, 0x1e);
    assert_eq!(physical("KeyB").scan_code, 0x30);
    assert_eq!(physical("KeyQ").scan_code, 0x10);
    assert_eq!(physical("Numpad0").scan_code, 0x52);
    assert_eq!(physical("Numpad9").scan_code, 0x49);
    assert_eq!(physical("NumpadEnter").scan_code, 0x1c);
    assert!(physical("NumpadEnter").extended);
    assert_eq!(physical("NumpadDivide").scan_code, 0x35);
    assert!(physical("NumpadDivide").extended);
    assert_eq!(physical("ArrowUp").scan_code, 0x48);
    assert!(physical("ArrowUp").extended);
    assert_eq!(physical("Delete").scan_code, 0x53);
    assert!(physical("Delete").extended);
}

#[test]
fn unsupported_physical_keys_report_the_exact_token_and_role() {
    for token in ["PrintScreen", "Pause", "F13"] {
        assert_eq!(
            translate(&identity(token, true), KeyRole::Target),
            Err(KeyTranslationError::UnsupportedPhysicalKey {
                token: token.to_owned(),
                role: KeyRole::Target,
            })
        );
    }
}

#[test]
fn unsupported_tokens_preserve_identity_and_role() {
    assert_eq!(
        translate(&identity("Unknown", false), KeyRole::Trigger),
        Err(KeyTranslationError::UnsupportedToken {
            token: "Unknown".to_owned(),
            identity: KeyIdentityKind::Logical,
            role: KeyRole::Trigger,
        })
    );
    assert_eq!(
        translate(&identity("Unknown", true), KeyRole::Target),
        Err(KeyTranslationError::UnsupportedToken {
            token: "Unknown".to_owned(),
            identity: KeyIdentityKind::Physical,
            role: KeyRole::Target,
        })
    );
}

#[test]
fn modifier_primaries_are_rejected_for_triggers_and_targets() {
    for (token, role) in [("Ctrl", KeyRole::Trigger), ("LeftShift", KeyRole::Target)] {
        assert_eq!(
            translate(&identity(token, false), role),
            Err(KeyTranslationError::UnsupportedPrimaryModifier {
                token: token.to_owned(),
                role,
            })
        );
    }
}

#[test]
fn raw_modifier_classification_is_sided_without_guessing_shift() {
    assert_eq!(
        modifier_from_raw(VK_SHIFT.0 as u32, 0x2a, false),
        Some(ConcreteModifier::LeftShift)
    );
    assert_eq!(
        modifier_from_raw(VK_SHIFT.0 as u32, 0x36, false),
        Some(ConcreteModifier::RightShift)
    );
    assert_eq!(modifier_from_raw(VK_SHIFT.0 as u32, 0, false), None);
    assert_eq!(
        modifier_from_raw(VK_LSHIFT.0 as u32, 0, false),
        Some(ConcreteModifier::LeftShift)
    );
    assert_eq!(
        modifier_from_raw(VK_RSHIFT.0 as u32, 0, false),
        Some(ConcreteModifier::RightShift)
    );
    assert_eq!(
        modifier_from_raw(VK_CONTROL.0 as u32, 0, false),
        Some(ConcreteModifier::LeftCtrl)
    );
    assert_eq!(
        modifier_from_raw(VK_CONTROL.0 as u32, 0, true),
        Some(ConcreteModifier::RightCtrl)
    );
    assert_eq!(
        modifier_from_raw(VK_LCONTROL.0 as u32, 0, false),
        Some(ConcreteModifier::LeftCtrl)
    );
    assert_eq!(
        modifier_from_raw(VK_RCONTROL.0 as u32, 0, false),
        Some(ConcreteModifier::RightCtrl)
    );
    assert_eq!(
        modifier_from_raw(VK_MENU.0 as u32, 0, false),
        Some(ConcreteModifier::LeftAlt)
    );
    assert_eq!(
        modifier_from_raw(VK_MENU.0 as u32, 0, true),
        Some(ConcreteModifier::RightAlt)
    );
    assert_eq!(
        modifier_from_raw(VK_LMENU.0 as u32, 0, false),
        Some(ConcreteModifier::LeftAlt)
    );
    assert_eq!(
        modifier_from_raw(VK_RMENU.0 as u32, 0, false),
        Some(ConcreteModifier::RightAlt)
    );
    assert_eq!(
        modifier_from_raw(VK_LWIN.0 as u32, 0, false),
        Some(ConcreteModifier::LeftMeta)
    );
    assert_eq!(
        modifier_from_raw(VK_RWIN.0 as u32, 0, false),
        Some(ConcreteModifier::RightMeta)
    );
}

#[test]
fn generic_ctrl_expands_to_only_concrete_ctrl_masks() {
    let compiled = compile(&[disable(1, [Modifier::Ctrl], "KeyA")]).unwrap();
    let key = logical("KeyA");
    for bits in [0x01, 0x02, 0x03] {
        assert_eq!(
            compiled
                .lookup_event(Some(key), None, ModifierState::from_bits(bits))
                .unwrap()
                .mapping_id,
            Uuid::from_u128(1)
        );
    }
    assert!(
        compiled
            .lookup_event(Some(key), None, ModifierState::from_bits(0))
            .is_none()
    );
    assert!(
        compiled
            .lookup_event(Some(key), None, ModifierState::from_bits(0x05))
            .is_none()
    );
}

#[test]
fn same_identity_alias_collisions_are_sorted() {
    let error = compile(&[
        disable(2, [Modifier::Ctrl], "KeyA"),
        disable(1, [Modifier::LeftCtrl], "KeyA"),
    ])
    .unwrap_err();
    assert_eq!(
        error,
        PlatformCompileError::AliasCollision {
            first: Uuid::from_u128(1),
            second: Uuid::from_u128(2)
        }
    );
}

#[test]
fn ctrl_alt_delete_is_rejected_for_logical_and_physical_triggers() {
    for (id, physical_trigger, modifiers) in [
        (1, false, vec![Modifier::Ctrl, Modifier::Alt]),
        (2, true, vec![Modifier::LeftCtrl, Modifier::RightAlt]),
    ] {
        assert_eq!(
            compile(&[mapping(
                id,
                true,
                modifiers,
                "Delete",
                physical_trigger,
                Action::Disable,
            )])
            .unwrap_err(),
            PlatformCompileError::SecureAttentionSequence {
                mapping_id: Uuid::from_u128(id)
            }
        );
    }
}

#[test]
fn ctrl_alt_delete_targets_are_rejected_with_the_actual_mapping_id() {
    for physical_target in [false, true] {
        let invalid = mapping(
            77,
            true,
            [],
            "KeyA",
            false,
            Action::EmitShortcut {
                chord: chord([Modifier::Ctrl, Modifier::Alt], "Delete", physical_target),
            },
        );
        assert_eq!(
            compile(&[invalid]).unwrap_err(),
            PlatformCompileError::SecureAttentionSequence {
                mapping_id: Uuid::from_u128(77)
            }
        );
    }
}

#[test]
fn physical_lookup_wins_when_both_identities_match() {
    let logical_mapping = disable(1, [], "KeyA");
    let physical_mapping = mapping(2, true, [], "KeyA", true, Action::Disable);
    for mappings in [
        vec![logical_mapping.clone(), physical_mapping.clone()],
        vec![physical_mapping, logical_mapping],
    ] {
        let compiled = compile(&mappings).unwrap();
        assert_eq!(
            compiled
                .lookup(logical("KeyA"), physical("KeyA"), ModifierState::empty())
                .unwrap()
                .mapping_id,
            Uuid::from_u128(2)
        );
    }
}

#[test]
fn mapping_count_tracks_active_mappings_not_aliases() {
    let mut disabled = disable(2, [], "KeyB");
    disabled.enabled = false;
    let compiled = compile(&[disable(1, [Modifier::Ctrl], "KeyA"), disabled]).unwrap();
    assert_eq!(compiled.mapping_count(), 1);
    assert_eq!(compiled.alias_count(), 3);
}

#[test]
fn published_keymap_preserves_the_last_successful_snapshot() {
    let published = PublishedKeymap::default();
    let invalid = mapping(
        3,
        true,
        [],
        "KeyC",
        false,
        Action::EmitShortcut {
            chord: chord([], "Unknown", false),
        },
    );
    assert!(
        published
            .replace_mappings(std::slice::from_ref(&invalid))
            .is_err()
    );
    assert_eq!(published.load().mapping_count(), 0);

    published
        .replace_mappings(&[disable(1, [], "KeyA")])
        .unwrap();
    published
        .replace_mappings(&[disable(2, [], "KeyB")])
        .unwrap();
    assert_eq!(
        published
            .load()
            .lookup_event(Some(logical("KeyB")), None, ModifierState::empty())
            .unwrap()
            .mapping_id,
        Uuid::from_u128(2)
    );
    assert!(published.replace_mappings(&[invalid]).is_err());
    let loaded = published.load();
    assert_eq!(loaded.mapping_count(), 1);
    assert_eq!(
        loaded
            .lookup_event(Some(logical("KeyB")), None, ModifierState::empty())
            .unwrap()
            .mapping_id,
        Uuid::from_u128(2)
    );
}

#[test]
fn publication_and_events_observe_complete_immutable_maps_under_contention() {
    use std::sync::{Arc, Barrier};
    let published = Arc::new(PublishedKeymap::default());
    let barrier = Arc::new(Barrier::new(2));
    let reader = published.clone();
    let ready = barrier.clone();
    let thread = std::thread::spawn(move || {
        ready.wait();
        for _ in 0..10_000 {
            let snapshot = reader.load();
            let count = snapshot.mapping_count();
            let a = snapshot.lookup_event(Some(logical("KeyA")), None, ModifierState::empty());
            let b = snapshot.lookup_event(Some(logical("KeyB")), None, ModifierState::empty());
            assert!(count == 0 || (count == 1 && a.is_some() != b.is_some()));
        }
    });
    barrier.wait();
    for index in 0..1000 {
        let key = if index % 2 == 0 { "KeyA" } else { "KeyB" };
        published.replace_mappings(&[disable(1, [], key)]).unwrap();
    }
    thread.join().unwrap();
}

#[test]
fn native_output_exposes_only_its_initialized_domain_modifiers() {
    let action = Action::EmitShortcut {
        chord: chord([Modifier::LeftAlt, Modifier::RightMeta], "KeyB", false),
    };
    let compiled = compile(&[mapping(1, true, [], "KeyA", false, action)]).unwrap();
    let request = compiled
        .lookup_event(Some(logical("KeyA")), None, ModifierState::empty())
        .unwrap();
    let NativeAction::EmitShortcut(output) = request.action else {
        panic!("expected shortcut output")
    };
    assert_eq!(
        output.modifiers().collect::<Vec<_>>(),
        vec![Modifier::LeftAlt, Modifier::RightMeta]
    );
}
