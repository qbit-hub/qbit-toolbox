#![deny(unsafe_code)]

//! In-process registration and lifecycle tracking for compile-time features.

use std::collections::HashMap;
use std::fmt;

use feature_api::{
    FeatureDescriptor, FeatureId, FeatureLifecycleState, RuntimeMode, StartupPolicy,
};

/// A core-owned request to activate a feature surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivationIntent {
    OpenControlWindow,
}

/// A stable, display-safe projection of a registered feature and its current status.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FeatureSummary {
    pub id: FeatureId,
    pub display_name_key: &'static str,
    pub description_key: &'static str,
    pub runtime_mode: RuntimeMode,
    pub startup_policy: StartupPolicy,
    pub lifecycle_state: FeatureLifecycleState,
    pub route: Option<&'static str>,
}

impl FeatureSummary {
    fn from_entry(entry: &FeatureEntry) -> Self {
        Self {
            id: entry.descriptor.id.clone(),
            display_name_key: entry.descriptor.display_name_key,
            description_key: entry.descriptor.description_key,
            runtime_mode: entry.descriptor.runtime_mode,
            startup_policy: entry.descriptor.startup_policy,
            lifecycle_state: entry.state,
            route: entry.descriptor.ui.map(|ui| ui.route),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FeatureEntry {
    descriptor: FeatureDescriptor,
    state: FeatureLifecycleState,
}

/// Errors raised while registering feature metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RegistrationError {
    DuplicateFeatureId { id: FeatureId },
    InvalidInitialState { state: FeatureLifecycleState },
}

impl fmt::Display for RegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateFeatureId { id } => {
                write!(formatter, "feature `{id}` is already registered")
            }
            Self::InvalidInitialState { state } => {
                write!(
                    formatter,
                    "feature cannot be registered in initial state {state:?}"
                )
            }
        }
    }
}

impl std::error::Error for RegistrationError {}

/// Errors raised while updating a feature lifecycle state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FeatureStateError {
    UnknownFeature { id: FeatureId },
}

impl fmt::Display for FeatureStateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownFeature { id } => write!(formatter, "feature `{id}` is not registered"),
        }
    }
}

impl std::error::Error for FeatureStateError {}

/// An ordered registry of feature metadata and independently managed lifecycle state.
#[derive(Debug, Default)]
pub struct FeatureRegistry {
    entries: Vec<FeatureEntry>,
    indices: HashMap<FeatureId, usize>,
}

impl FeatureRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Registers a feature in the disabled state.
    pub fn register(&mut self, descriptor: FeatureDescriptor) -> Result<(), RegistrationError> {
        self.register_with_state(descriptor, FeatureLifecycleState::Disabled)
    }

    /// Registers a feature with an explicitly resolved initial availability state.
    pub fn register_with_state(
        &mut self,
        descriptor: FeatureDescriptor,
        initial_state: FeatureLifecycleState,
    ) -> Result<(), RegistrationError> {
        if !matches!(
            initial_state,
            FeatureLifecycleState::Disabled | FeatureLifecycleState::Unavailable
        ) {
            return Err(RegistrationError::InvalidInitialState {
                state: initial_state,
            });
        }

        let id = descriptor.id.clone();
        if self.indices.contains_key(&id) {
            return Err(RegistrationError::DuplicateFeatureId { id });
        }

        let index = self.entries.len();
        self.entries.push(FeatureEntry {
            descriptor,
            state: initial_state,
        });
        self.indices.insert(id, index);
        Ok(())
    }

    pub fn summary(&self, id: &FeatureId) -> Option<FeatureSummary> {
        self.indices
            .get(id)
            .map(|&index| FeatureSummary::from_entry(&self.entries[index]))
    }

    /// Returns summaries in deterministic registration order.
    pub fn summaries(&self) -> Vec<FeatureSummary> {
        self.entries
            .iter()
            .map(FeatureSummary::from_entry)
            .collect()
    }

    pub fn lifecycle_state(&self, id: &FeatureId) -> Option<FeatureLifecycleState> {
        self.indices.get(id).map(|&index| self.entries[index].state)
    }

    /// Updates a registered feature's lifecycle state.
    ///
    /// Lifecycle policy is intentionally owned by the future Runtime Supervisor.
    pub fn set_lifecycle_state(
        &mut self,
        id: &FeatureId,
        state: FeatureLifecycleState,
    ) -> Result<(), FeatureStateError> {
        let Some(&index) = self.indices.get(id) else {
            return Err(FeatureStateError::UnknownFeature { id: id.clone() });
        };
        self.entries[index].state = state;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feature_api::{
        BackgroundRequirement, CapabilityRequirement, FeatureDependency, FeatureDependencyKind,
        FeatureDiagnosticsMetadata, FeatureUiMetadata, FeatureVersion, StorageClass,
        StorageRequirement, SupportedPlatform,
    };

    fn descriptor(id: &str) -> FeatureDescriptor {
        FeatureDescriptor {
            id: FeatureId::new(id).unwrap(),
            display_name_key: "features.example.display-name",
            description_key: "features.example.description",
            version: Some(FeatureVersion {
                implementation: "1.0.0",
                schema: Some(1),
            }),
            supported_platforms: &[SupportedPlatform::Windows],
            runtime_mode: RuntimeMode::EmbeddedBackground,
            startup_policy: StartupPolicy::Manual,
            capabilities: &[CapabilityRequirement {
                name: "test",
                required: false,
            }],
            storage: &[StorageRequirement {
                class: StorageClass::Cache,
                required: false,
                purpose: "test data",
            }],
            background_requirement: BackgroundRequirement::OnDemand,
            dependencies: &[FeatureDependency {
                kind: FeatureDependencyKind::CoreCapability,
                id: "test",
            }],
            ui: Some(FeatureUiMetadata {
                route: "/features/example",
            }),
            diagnostics: FeatureDiagnosticsMetadata {
                component: "feature.example",
            },
        }
    }

    #[test]
    fn empty_registry_is_valid() {
        let registry = FeatureRegistry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.summaries().is_empty());
    }

    #[test]
    fn duplicate_registration_has_a_typed_error() {
        let mut registry = FeatureRegistry::new();
        registry.register(descriptor("qbit.example")).unwrap();

        assert_eq!(
            registry.register(descriptor("qbit.example")),
            Err(RegistrationError::DuplicateFeatureId {
                id: FeatureId::new("qbit.example").unwrap(),
            })
        );
    }

    #[test]
    fn active_initial_state_is_rejected_without_mutating_the_registry() {
        let mut registry = FeatureRegistry::new();

        assert_eq!(
            registry
                .register_with_state(descriptor("qbit.example"), FeatureLifecycleState::Running),
            Err(RegistrationError::InvalidInitialState {
                state: FeatureLifecycleState::Running
            })
        );
        assert!(registry.is_empty());
    }

    #[test]
    fn summaries_preserve_registration_order() {
        let mut registry = FeatureRegistry::new();
        registry.register(descriptor("qbit.first")).unwrap();
        registry.register(descriptor("qbit.second")).unwrap();

        let ids: Vec<_> = registry
            .summaries()
            .into_iter()
            .map(|summary| summary.id)
            .collect();
        assert_eq!(
            ids,
            vec![
                FeatureId::new("qbit.first").unwrap(),
                FeatureId::new("qbit.second").unwrap()
            ]
        );
    }

    #[test]
    fn summary_preserves_localization_keys() {
        let mut registry = FeatureRegistry::new();
        let id = FeatureId::new("qbit.example").unwrap();
        registry.register(descriptor("qbit.example")).unwrap();

        assert_eq!(
            registry.summary(&id),
            Some(FeatureSummary {
                route: Some("/features/example"),
                id,
                display_name_key: "features.example.display-name",
                description_key: "features.example.description",
                runtime_mode: RuntimeMode::EmbeddedBackground,
                startup_policy: StartupPolicy::Manual,
                lifecycle_state: FeatureLifecycleState::Disabled,
            })
        );
    }

    #[test]
    fn open_control_window_is_a_stable_typed_activation_intent() {
        assert_eq!(
            ActivationIntent::OpenControlWindow,
            ActivationIntent::OpenControlWindow
        );
    }

    #[test]
    fn lifecycle_state_can_be_updated_without_transition_policy() {
        let mut registry = FeatureRegistry::new();
        let id = FeatureId::new("qbit.example").unwrap();
        registry.register(descriptor("qbit.example")).unwrap();
        registry
            .set_lifecycle_state(&id, FeatureLifecycleState::Running)
            .unwrap();
        assert_eq!(
            registry.lifecycle_state(&id),
            Some(FeatureLifecycleState::Running)
        );
    }

    #[test]
    fn unknown_feature_state_update_does_not_mutate_registry() {
        let mut registry = FeatureRegistry::new();
        registry.register(descriptor("qbit.example")).unwrap();
        let unknown_id = FeatureId::new("qbit.unknown").unwrap();

        assert_eq!(
            registry.set_lifecycle_state(&unknown_id, FeatureLifecycleState::Running),
            Err(FeatureStateError::UnknownFeature { id: unknown_id })
        );
        assert_eq!(registry.len(), 1);
        assert_eq!(
            registry.lifecycle_state(&FeatureId::new("qbit.example").unwrap()),
            Some(FeatureLifecycleState::Disabled)
        );
    }
}
