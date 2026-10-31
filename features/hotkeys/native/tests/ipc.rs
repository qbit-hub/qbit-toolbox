use ipc_contracts::*;
use qbit_hotkeys::{ipc, service::ServiceError};
#[test]
fn camel_case_action_domain_roundtrip_and_safe_error_categories() {
    let value = serde_json::json!({"enabled":true,"trigger":{"modifiers":["ctrl"],"primary":{"kind":"logical","token":"KeyA"}},"action":{"kind":"emitShortcut","chord":{"modifiers":["meta"],"primary":{"kind":"logical","token":"KeyB"}}},"label":null});
    let dto: HotkeyMappingInput = serde_json::from_value(value.clone()).unwrap();
    let domain = ipc::mapping(uuid::Uuid::new_v4(), dto).unwrap();
    let output = ipc::mapping_dto(domain);
    assert_eq!(serde_json::to_value(output.input).unwrap(), value);
    for (error, category) in [
        (ServiceError::Conflict, CommandErrorCategoryDto::Conflict),
        (
            ServiceError::Persistence,
            CommandErrorCategoryDto::Persistence,
        ),
        (ServiceError::Migration, CommandErrorCategoryDto::Migration),
        (
            ServiceError::Validation,
            CommandErrorCategoryDto::Validation,
        ),
        (
            ServiceError::PlatformUnsupported,
            CommandErrorCategoryDto::PlatformUnsupported,
        ),
    ] {
        let dto = ipc::error(error);
        assert_eq!(dto.category, category);
        assert!(dto.context.is_empty());
        assert!(dto.message_key.starts_with("hotkeys.error."));
    }
}
#[test]
fn duplicated_modifiers_and_oversize_label_are_rejected_at_boundary() {
    let mut input = HotkeyMappingInput {
        enabled: true,
        trigger: HotkeyChord {
            modifiers: vec![HotkeyModifier::Ctrl, HotkeyModifier::Ctrl],
            primary: KeyIdentity {
                kind: KeyIdentityKind::Logical,
                token: "KeyA".into(),
            },
        },
        action: HotkeyAction::Disable,
        label: None,
    };
    assert_eq!(
        ipc::mapping(uuid::Uuid::new_v4(), input.clone()),
        Err(ServiceError::Validation)
    );
    input.trigger.modifiers.clear();
    input.label = Some("a".repeat(121));
    assert_eq!(
        ipc::mapping(uuid::Uuid::new_v4(), input),
        Err(ServiceError::Validation)
    );
}
