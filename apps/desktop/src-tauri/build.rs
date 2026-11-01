fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_core_status",
            "hotkeys_list_mappings",
            "hotkeys_create_mapping",
            "hotkeys_update_mapping",
            "hotkeys_delete_mapping",
            "hotkeys_set_mapping_enabled",
            "hotkeys_set_feature_enabled",
            "hotkeys_get_runtime_status",
            "hotkeys_analyze_candidate",
            "get_autostart",
            "set_autostart",
        ]),
    ))
    .expect("Tauri application command manifest must generate during the desktop build");
}
