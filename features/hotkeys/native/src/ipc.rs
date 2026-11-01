//! Explicit domain/transport conversion and safe error projection.
use crate::{
    domain as d,
    service::{Backend, HotkeysService, ServiceError},
};
use ipc_contracts::*;
use std::collections::BTreeMap;
use uuid::Uuid;

pub fn error(error: ServiceError) -> CommandErrorDto {
    let (code, category, key) = match error {
        ServiceError::Validation => (
            "hotkeys_invalid_mapping",
            CommandErrorCategoryDto::Validation,
            "hotkeys.error.validation",
        ),
        ServiceError::Conflict => (
            "hotkeys_conflict",
            CommandErrorCategoryDto::Conflict,
            "hotkeys.error.conflict",
        ),
        ServiceError::NotFound => (
            "hotkeys_not_found",
            CommandErrorCategoryDto::Validation,
            "hotkeys.error.notFound",
        ),
        ServiceError::Persistence => (
            "hotkeys_persistence_failed",
            CommandErrorCategoryDto::Persistence,
            "hotkeys.error.persistence",
        ),
        ServiceError::Migration => (
            "hotkeys_migration_failed",
            CommandErrorCategoryDto::Migration,
            "hotkeys.error.migration",
        ),
        ServiceError::PlatformUnsupported => (
            "hotkeys_platform_unsupported",
            CommandErrorCategoryDto::PlatformUnsupported,
            "hotkeys.error.platform",
        ),
        ServiceError::Runtime("modifiers_held_at_enable") => (
            "modifiers_held_at_enable",
            CommandErrorCategoryDto::Runtime,
            "hotkeys.error.modifiersHeld",
        ),
        ServiceError::Runtime(code) => (
            code,
            CommandErrorCategoryDto::Runtime,
            "hotkeys.error.runtime",
        ),
    };
    CommandErrorDto {
        code: code.into(),
        category,
        message_key: key.into(),
        recoverable: true,
        context: BTreeMap::new(),
    }
}
pub fn parse_id(id: &str) -> Result<Uuid, ServiceError> {
    Uuid::parse_str(id).map_err(|_| ServiceError::Validation)
}
fn modifier(value: HotkeyModifier) -> d::Modifier {
    match value {
        HotkeyModifier::Ctrl => d::Modifier::Ctrl,
        HotkeyModifier::Shift => d::Modifier::Shift,
        HotkeyModifier::Alt => d::Modifier::Alt,
        HotkeyModifier::Meta => d::Modifier::Meta,
        HotkeyModifier::LeftCtrl => d::Modifier::LeftCtrl,
        HotkeyModifier::RightCtrl => d::Modifier::RightCtrl,
        HotkeyModifier::LeftShift => d::Modifier::LeftShift,
        HotkeyModifier::RightShift => d::Modifier::RightShift,
        HotkeyModifier::LeftAlt => d::Modifier::LeftAlt,
        HotkeyModifier::RightAlt => d::Modifier::RightAlt,
        HotkeyModifier::LeftMeta => d::Modifier::LeftMeta,
        HotkeyModifier::RightMeta => d::Modifier::RightMeta,
    }
}
fn chord(value: HotkeyChord) -> Result<d::Chord, ServiceError> {
    if value.modifiers.len() > 8 || value.primary.token.len() > d::KeyToken::MAX_BYTES {
        return Err(ServiceError::Validation);
    }
    let token = d::KeyToken::new(value.primary.token).map_err(|_| ServiceError::Validation)?;
    let identity = match value.primary.kind {
        KeyIdentityKind::Logical => d::KeyIdentity::Logical(token),
        KeyIdentityKind::Physical => d::KeyIdentity::Physical(token),
    };
    d::Chord::new(value.modifiers.into_iter().map(modifier), identity)
        .map_err(|_| ServiceError::Validation)
}
pub fn mapping(id: Uuid, input: HotkeyMappingInput) -> Result<d::Mapping, ServiceError> {
    let trigger = d::Trigger::Chord {
        chord: chord(input.trigger)?,
    };
    let action = match input.action {
        HotkeyAction::Disable => d::Action::Disable,
        HotkeyAction::EmitShortcut { chord: value } => d::Action::EmitShortcut {
            chord: chord(value)?,
        },
    };
    let metadata = d::MappingMetadata::new(input.label).map_err(|_| ServiceError::Validation)?;
    Ok(d::Mapping::with_id(
        id,
        input.enabled,
        trigger,
        action,
        d::Scope::Global,
        d::MappingBehavior::default(),
        metadata,
    ))
}
fn chord_dto(value: &d::Chord) -> HotkeyChord {
    let (kind, token) = match value.primary() {
        d::KeyIdentity::Logical(t) => (KeyIdentityKind::Logical, t),
        d::KeyIdentity::Physical(t) => (KeyIdentityKind::Physical, t),
    };
    let modifiers = value
        .modifiers()
        .iter()
        .map(|m| match m {
            d::Modifier::Ctrl => HotkeyModifier::Ctrl,
            d::Modifier::Shift => HotkeyModifier::Shift,
            d::Modifier::Alt => HotkeyModifier::Alt,
            d::Modifier::Meta => HotkeyModifier::Meta,
            d::Modifier::LeftCtrl => HotkeyModifier::LeftCtrl,
            d::Modifier::RightCtrl => HotkeyModifier::RightCtrl,
            d::Modifier::LeftShift => HotkeyModifier::LeftShift,
            d::Modifier::RightShift => HotkeyModifier::RightShift,
            d::Modifier::LeftAlt => HotkeyModifier::LeftAlt,
            d::Modifier::RightAlt => HotkeyModifier::RightAlt,
            d::Modifier::LeftMeta => HotkeyModifier::LeftMeta,
            d::Modifier::RightMeta => HotkeyModifier::RightMeta,
        })
        .collect();
    HotkeyChord {
        modifiers,
        primary: KeyIdentity {
            kind,
            token: token.as_str().into(),
        },
    }
}
pub fn mapping_dto(value: d::Mapping) -> HotkeyMapping {
    let action = match &value.action {
        d::Action::Disable => HotkeyAction::Disable,
        d::Action::EmitShortcut { chord } => HotkeyAction::EmitShortcut {
            chord: chord_dto(chord),
        },
    };
    HotkeyMapping {
        id: value.id.to_string(),
        input: HotkeyMappingInput {
            enabled: value.enabled,
            trigger: chord_dto(value.trigger.chord()),
            action,
            label: value.metadata.label().map(str::to_owned),
        },
    }
}
pub fn status<B: Backend>(service: &HotkeysService<B>) -> HotkeysRuntimeStatus {
    let metrics = service.metrics();
    let last_error = service.last_error().or_else(|| {
        (service.desired() && !metrics.hook_active)
            .then_some(ServiceError::Runtime("hook_unavailable"))
    });
    let lifecycle_state = match service.lifecycle() {
        feature_api::FeatureLifecycleState::Unavailable => FeatureLifecycleStateDto::Unavailable,
        feature_api::FeatureLifecycleState::Disabled => FeatureLifecycleStateDto::Disabled,
        feature_api::FeatureLifecycleState::Starting => FeatureLifecycleStateDto::Starting,
        feature_api::FeatureLifecycleState::Running => FeatureLifecycleStateDto::Running,
        feature_api::FeatureLifecycleState::Degraded => FeatureLifecycleStateDto::Degraded,
        feature_api::FeatureLifecycleState::Stopping => FeatureLifecycleStateDto::Stopping,
        feature_api::FeatureLifecycleState::Failed => FeatureLifecycleStateDto::Failed,
    };
    HotkeysRuntimeStatus {
        desired_enabled: service.desired(),
        lifecycle_state,
        last_engine_error: last_error.map(error),
        metrics: HotkeysMetrics {
            hook_active: metrics.hook_active,
            mapping_count: u32::try_from(metrics.mapping_count).unwrap_or(u32::MAX),
            events_seen: metrics.events_seen as f64,
            events_matched: metrics.events_matched as f64,
            actions_emitted: metrics.actions_emitted as f64,
            injection_failures: metrics.injection_failures as f64,
            queue_overflows: metrics.queue_overflows as f64,
            config_reload_failures: service.reload_failures() as f64,
        },
    }
}
pub fn conflict_report(
    codes: Vec<(HotkeyConflictCode, HotkeyConflictSeverity)>,
) -> HotkeysConflictReport {
    HotkeysConflictReport {
        conflicts: codes
            .into_iter()
            .map(|(code, severity)| HotkeyConflict {
                code,
                severity,
                message_key: match code {
                    HotkeyConflictCode::DuplicateTrigger => "hotkeys.conflict.duplicateTrigger",
                    HotkeyConflictCode::SelfMap => "hotkeys.conflict.selfMap",
                    HotkeyConflictCode::Cycle => "hotkeys.conflict.cycle",
                    HotkeyConflictCode::AliasCollision => "hotkeys.conflict.aliasCollision",
                    HotkeyConflictCode::SecureAttentionSequence => {
                        "hotkeys.conflict.secureAttentionSequence"
                    }
                    HotkeyConflictCode::UnsupportedKey => "hotkeys.conflict.unsupportedKey",
                    HotkeyConflictCode::InvalidMapping => "hotkeys.conflict.invalidMapping",
                }
                .into(),
            })
            .collect(),
    }
}
