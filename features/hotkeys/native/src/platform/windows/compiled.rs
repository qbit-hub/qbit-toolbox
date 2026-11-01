use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use arc_swap::ArcSwap;
use uuid::Uuid;

use crate::application;
use crate::domain::{Action, Mapping, Modifier};
use crate::runtime::ModifierState;

use super::keys::{KeyRole, KeyTranslationError, LogicalKey, NativeKey, PhysicalKey, translate};

const LEFT_CTRL: u8 = 0x01;
const RIGHT_CTRL: u8 = 0x02;
const LEFT_SHIFT: u8 = 0x04;
const RIGHT_SHIFT: u8 = 0x08;
const LEFT_ALT: u8 = 0x10;
const RIGHT_ALT: u8 = 0x20;
const LEFT_META: u8 = 0x40;
const RIGHT_META: u8 = 0x80;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeOutput {
    pub primary: NativeKey,
    modifiers: [Option<Modifier>; 8],
    modifier_len: u8,
}

impl NativeOutput {
    pub fn modifiers(&self) -> impl Iterator<Item = Modifier> + '_ {
        self.modifiers[..usize::from(self.modifier_len)]
            .iter()
            .flatten()
            .copied()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeAction {
    Disable,
    EmitShortcut(NativeOutput),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActionRequest {
    pub mapping_id: Uuid,
    pub action: NativeAction,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct Trigger<K> {
    key: K,
    modifiers: u8,
}

#[derive(Clone, Debug, Default)]
pub struct NativeCompiledKeymap {
    logical: HashMap<Trigger<LogicalKey>, ActionRequest>,
    physical: HashMap<Trigger<PhysicalKey>, ActionRequest>,
    active_mapping_count: usize,
}

impl NativeCompiledKeymap {
    pub fn lookup(
        &self,
        logical: LogicalKey,
        physical: PhysicalKey,
        state: ModifierState,
    ) -> Option<ActionRequest> {
        self.lookup_event(Some(logical), Some(physical), state)
    }

    pub fn lookup_event(
        &self,
        logical: Option<LogicalKey>,
        physical: Option<PhysicalKey>,
        state: ModifierState,
    ) -> Option<ActionRequest> {
        let modifiers = state.bits() as u8;
        physical
            .and_then(|key| self.physical.get(&Trigger { key, modifiers }).copied())
            .or_else(|| {
                logical.and_then(|key| self.logical.get(&Trigger { key, modifiers }).copied())
            })
    }
    pub fn mapping_count(&self) -> usize {
        self.active_mapping_count
    }

    pub fn alias_count(&self) -> usize {
        self.logical.len() + self.physical.len()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlatformCompileError {
    Semantic(application::CompileError),
    Key(KeyTranslationError),
    AliasCollision { first: Uuid, second: Uuid },
    SecureAttentionSequence { mapping_id: Uuid },
}
impl fmt::Display for PlatformCompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Semantic(error) => write!(f, "semantic hotkey compilation failed: {error:?}"),
            Self::Key(error) => write!(f, "Windows key translation failed: {error}"),
            Self::AliasCollision { first, second } => write!(
                f,
                "Windows trigger alias collision between {first} and {second}"
            ),
            Self::SecureAttentionSequence { mapping_id } => {
                write!(f, "mapping {mapping_id} uses Ctrl+Alt+Delete")
            }
        }
    }
}
impl std::error::Error for PlatformCompileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Key(error) => Some(error),
            _ => None,
        }
    }
}
impl From<KeyTranslationError> for PlatformCompileError {
    fn from(error: KeyTranslationError) -> Self {
        Self::Key(error)
    }
}

pub fn compile(mappings: &[Mapping]) -> Result<NativeCompiledKeymap, PlatformCompileError> {
    application::compile(mappings).map_err(PlatformCompileError::Semantic)?;
    let mut keymap = NativeCompiledKeymap::default();
    for mapping in mappings.iter().filter(|mapping| mapping.enabled) {
        let chord = mapping.trigger.chord();
        if is_secure_attention_sequence(chord.primary(), chord.modifiers()) {
            return Err(PlatformCompileError::SecureAttentionSequence {
                mapping_id: mapping.id,
            });
        }
        let trigger = translate(chord.primary(), KeyRole::Trigger)?;
        let request = ActionRequest {
            mapping_id: mapping.id,
            action: compile_action(mapping.id, &mapping.action)?,
        };
        for modifiers in aliases(chord.modifiers()) {
            match trigger {
                NativeKey::Logical(key) => {
                    insert(&mut keymap.logical, Trigger { key, modifiers }, request)?
                }
                NativeKey::Physical(key) => {
                    insert(&mut keymap.physical, Trigger { key, modifiers }, request)?
                }
            }
        }
        keymap.active_mapping_count += 1;
    }
    Ok(keymap)
}

fn compile_action(mapping_id: Uuid, action: &Action) -> Result<NativeAction, PlatformCompileError> {
    match action {
        Action::Disable => Ok(NativeAction::Disable),
        Action::EmitShortcut { chord } => {
            if is_secure_attention_sequence(chord.primary(), chord.modifiers()) {
                return Err(PlatformCompileError::SecureAttentionSequence { mapping_id });
            }
            let mut modifiers = [None; 8];
            for (index, modifier) in chord.modifiers().iter().enumerate() {
                modifiers[index] = Some(*modifier);
            }
            Ok(NativeAction::EmitShortcut(NativeOutput {
                primary: translate(chord.primary(), KeyRole::Target)?,
                modifiers,
                modifier_len: chord.modifiers().len() as u8,
            }))
        }
    }
}

fn insert<K: Eq + std::hash::Hash>(
    map: &mut HashMap<Trigger<K>, ActionRequest>,
    trigger: Trigger<K>,
    request: ActionRequest,
) -> Result<(), PlatformCompileError> {
    if let Some(previous) = map.get(&trigger) {
        let (first, second) = sorted_ids(previous.mapping_id, request.mapping_id);
        return Err(PlatformCompileError::AliasCollision { first, second });
    }
    map.insert(trigger, request);
    Ok(())
}

fn sorted_ids(left: Uuid, right: Uuid) -> (Uuid, Uuid) {
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

#[derive(Clone, Copy)]
struct FamilyRequirement {
    generic: Modifier,
    left: Modifier,
    right: Modifier,
    left_bit: u8,
    right_bit: u8,
}

const MODIFIER_FAMILIES: [FamilyRequirement; 4] = [
    FamilyRequirement {
        generic: Modifier::Ctrl,
        left: Modifier::LeftCtrl,
        right: Modifier::RightCtrl,
        left_bit: LEFT_CTRL,
        right_bit: RIGHT_CTRL,
    },
    FamilyRequirement {
        generic: Modifier::Shift,
        left: Modifier::LeftShift,
        right: Modifier::RightShift,
        left_bit: LEFT_SHIFT,
        right_bit: RIGHT_SHIFT,
    },
    FamilyRequirement {
        generic: Modifier::Alt,
        left: Modifier::LeftAlt,
        right: Modifier::RightAlt,
        left_bit: LEFT_ALT,
        right_bit: RIGHT_ALT,
    },
    FamilyRequirement {
        generic: Modifier::Meta,
        left: Modifier::LeftMeta,
        right: Modifier::RightMeta,
        left_bit: LEFT_META,
        right_bit: RIGHT_META,
    },
];

fn aliases(modifiers: &[Modifier]) -> Vec<u8> {
    let mut results = vec![0];
    for family in MODIFIER_FAMILIES {
        let choices = if modifiers.contains(&family.generic) {
            vec![
                family.left_bit,
                family.right_bit,
                family.left_bit | family.right_bit,
            ]
        } else {
            let mut exact = 0;
            if modifiers.contains(&family.left) {
                exact |= family.left_bit;
            }
            if modifiers.contains(&family.right) {
                exact |= family.right_bit;
            }
            vec![exact]
        };
        results = results
            .into_iter()
            .flat_map(|prefix| choices.iter().map(move |choice| prefix | choice))
            .collect();
    }
    results
}

fn is_secure_attention_sequence(
    primary: &crate::domain::KeyIdentity,
    modifiers: &[Modifier],
) -> bool {
    let delete = matches!(primary, crate::domain::KeyIdentity::Logical(token) | crate::domain::KeyIdentity::Physical(token) if token.as_str() == "Delete");
    delete
        && modifiers.iter().any(|modifier| {
            matches!(
                modifier,
                Modifier::Ctrl | Modifier::LeftCtrl | Modifier::RightCtrl
            )
        })
        && modifiers.iter().any(|modifier| {
            matches!(
                modifier,
                Modifier::Alt | Modifier::LeftAlt | Modifier::RightAlt
            )
        })
}

pub struct PublishedKeymap {
    map: ArcSwap<NativeCompiledKeymap>,
}
impl Default for PublishedKeymap {
    fn default() -> Self {
        Self {
            map: ArcSwap::from_pointee(NativeCompiledKeymap::default()),
        }
    }
}
impl PublishedKeymap {
    /// Publishes a candidate that was fully compiled before its database commit.
    pub fn publish(&self, compiled: Arc<NativeCompiledKeymap>) {
        self.map.store(compiled);
    }
    pub fn load(&self) -> Arc<NativeCompiledKeymap> {
        self.map.load_full()
    }
    pub fn replace_mappings(&self, mappings: &[Mapping]) -> Result<usize, PlatformCompileError> {
        let compiled = compile(mappings)?;
        let count = compiled.mapping_count();
        self.map.store(Arc::new(compiled));
        Ok(count)
    }
}
