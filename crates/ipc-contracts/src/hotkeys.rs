//! Transport contracts owned by Rust; no native keycodes or database errors.
use crate::{CommandErrorDto, FeatureLifecycleStateDto};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum HotkeyModifier {
    Ctrl,
    Shift,
    Alt,
    Meta,
    LeftCtrl,
    RightCtrl,
    LeftShift,
    RightShift,
    LeftAlt,
    RightAlt,
    LeftMeta,
    RightMeta,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum KeyIdentityKind {
    Logical,
    Physical,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct KeyIdentity {
    pub kind: KeyIdentityKind,
    pub token: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct HotkeyChord {
    pub modifiers: Vec<HotkeyModifier>,
    pub primary: KeyIdentity,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
#[serde(tag = "kind")]
#[ts(tag = "kind")]
pub enum HotkeyAction {
    EmitShortcut { chord: HotkeyChord },
    Disable,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct HotkeyMappingInput {
    pub enabled: bool,
    pub trigger: HotkeyChord,
    pub action: HotkeyAction,
    pub label: Option<String>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct HotkeyMapping {
    pub id: String,
    pub input: HotkeyMappingInput,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum HotkeyConflictCode {
    DuplicateTrigger,
    SelfMap,
    Cycle,
    AliasCollision,
    SecureAttentionSequence,
    UnsupportedKey,
    InvalidMapping,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub enum HotkeyConflictSeverity {
    Error,
    Warning,
    Info,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct HotkeyConflict {
    pub code: HotkeyConflictCode,
    pub severity: HotkeyConflictSeverity,
    pub message_key: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct HotkeysConflictReport {
    pub conflicts: Vec<HotkeyConflict>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct HotkeysMetrics {
    pub hook_active: bool,
    pub mapping_count: u32,
    pub events_seen: f64,
    pub events_matched: f64,
    pub actions_emitted: f64,
    pub injection_failures: f64,
    pub queue_overflows: f64,
    pub config_reload_failures: f64,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct HotkeysRuntimeStatus {
    pub desired_enabled: bool,
    pub lifecycle_state: FeatureLifecycleStateDto,
    pub metrics: HotkeysMetrics,
    pub last_engine_error: Option<CommandErrorDto>,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct AutostartStatus {
    pub desired_enabled: bool,
    pub registered: bool,
}
pub fn declarations() -> Vec<String> {
    vec![
        HotkeyModifier::decl(),
        KeyIdentityKind::decl(),
        KeyIdentity::decl(),
        HotkeyChord::decl(),
        HotkeyAction::decl(),
        HotkeyMappingInput::decl(),
        HotkeyMapping::decl(),
        HotkeyConflictCode::decl(),
        HotkeyConflictSeverity::decl(),
        HotkeyConflict::decl(),
        HotkeysConflictReport::decl(),
        HotkeysMetrics::decl(),
        HotkeysRuntimeStatus::decl(),
        AutostartStatus::decl(),
    ]
}
