use feature_api::{
    BackgroundRequirement, CapabilityRequirement, FeatureDescriptor, FeatureDiagnosticsMetadata,
    FeatureId, FeatureUiMetadata, FeatureVersion, RuntimeMode, StartupPolicy, StorageClass,
    StorageRequirement, SupportedPlatform,
};

pub const FEATURE_ID: &str = "qbit.hotkeys";
pub const FEATURE_SCHEMA_VERSION: u32 = 1;

/// Static metadata; the host starts this feature only when persisted enabled.
pub fn descriptor() -> FeatureDescriptor {
    FeatureDescriptor {
        id: FeatureId::new(FEATURE_ID).expect("static feature id is valid"),
        display_name_key: "features.hotkeys.name",
        description_key: "features.hotkeys.description",
        version: Some(FeatureVersion {
            implementation: env!("CARGO_PKG_VERSION"),
            schema: Some(FEATURE_SCHEMA_VERSION),
        }),
        supported_platforms: &[SupportedPlatform::Windows],
        runtime_mode: RuntimeMode::EmbeddedBackground,
        startup_policy: StartupPolicy::OnApplicationStart,
        capabilities: &[
            CapabilityRequirement {
                name: "keyboard.global-interception",
                required: true,
            },
            CapabilityRequirement {
                name: "input.emission",
                required: true,
            },
        ],
        storage: &[StorageRequirement {
            class: StorageClass::FeatureRelationalState,
            required: true,
            purpose: "hotkey mappings and settings",
        }],
        background_requirement: BackgroundRequirement::Continuous,
        dependencies: &[],
        ui: Some(FeatureUiMetadata {
            route: "/features/hotkeys",
        }),
        diagnostics: FeatureDiagnosticsMetadata {
            component: "feature.hotkeys",
        },
    }
}
