//! Thin IPC adapters. Feature rules live in qbit-hotkeys; every operation uses
//! the same bounded blocking lane, including read/analysis and shutdown.
use ipc_contracts::*;
use persistence::{CoreStore, lane::BlockingLane};
use qbit_hotkeys::{
    ipc as wire,
    platform::backend::SystemBackend,
    service::{HotkeysService, ServiceError},
};
use std::sync::Arc;
use tauri::Manager;
pub struct Services {
    pub core: Option<CoreStore>,
    pub hotkeys: HotkeysService<SystemBackend>,
}
pub struct ConfigurationState {
    pub lane: Arc<BlockingLane<Services>>,
}
async fn run<R: Send + 'static>(
    app: tauri::AppHandle,
    task: impl FnOnce(&mut Services) -> Result<R, ServiceError> + Send + 'static,
) -> Result<R, CommandErrorDto> {
    if app.state::<super::ApplicationState>().shutdown.phase() != super::shutdown::Phase::Running {
        return Err(wire::error(ServiceError::Runtime("shutting_down")));
    }
    let lane = app.state::<ConfigurationState>().lane.clone();
    let owner = app.clone();
    let response = lane
        .submit(move |services| {
            if services.core.is_none() {
                return Err(ServiceError::Runtime("core_recovery_required"));
            }
            let result = task(services);
            if let Ok(mut registry) = owner.state::<super::ApplicationState>().registry.lock() {
                let state = match wire::status(&services.hotkeys).lifecycle_state {
                    FeatureLifecycleStateDto::Running => {
                        feature_api::FeatureLifecycleState::Running
                    }
                    FeatureLifecycleStateDto::Failed => feature_api::FeatureLifecycleState::Failed,
                    FeatureLifecycleStateDto::Unavailable => {
                        feature_api::FeatureLifecycleState::Unavailable
                    }
                    _ => feature_api::FeatureLifecycleState::Disabled,
                };
                if let Ok(id) = feature_api::FeatureId::new(qbit_hotkeys::FEATURE_ID) {
                    let _ = registry.set_lifecycle_state(&id, state);
                }
            }
            result
        })
        .map_err(|_| wire::error(ServiceError::Runtime("configuration_busy")))?;
    tauri::async_runtime::spawn_blocking(move || response.recv())
        .await
        .map_err(|_| wire::error(ServiceError::Runtime("configuration_failed")))?
        .map_err(|_| wire::error(ServiceError::Runtime("configuration_closed")))?
        .map_err(wire::error)
}
#[tauri::command]
pub async fn hotkeys_list_mappings(
    app: tauri::AppHandle,
) -> Result<Vec<HotkeyMapping>, CommandErrorDto> {
    run(app, move |s| {
        s.hotkeys
            .list()
            .map(|items| items.into_iter().map(wire::mapping_dto).collect())
    })
    .await
}
#[tauri::command]
pub async fn hotkeys_create_mapping(
    app: tauri::AppHandle,
    input: HotkeyMappingInput,
) -> Result<HotkeyMapping, CommandErrorDto> {
    run(app, move |s| {
        s.hotkeys
            .save(wire::mapping(uuid::Uuid::new_v4(), input)?, true)
            .map(wire::mapping_dto)
    })
    .await
}
#[tauri::command]
pub async fn hotkeys_update_mapping(
    app: tauri::AppHandle,
    id: String,
    input: HotkeyMappingInput,
) -> Result<HotkeyMapping, CommandErrorDto> {
    run(app, move |s| {
        s.hotkeys
            .save(wire::mapping(wire::parse_id(&id)?, input)?, false)
            .map(wire::mapping_dto)
    })
    .await
}
#[tauri::command]
pub async fn hotkeys_delete_mapping(
    app: tauri::AppHandle,
    id: String,
) -> Result<(), CommandErrorDto> {
    run(app, move |s| s.hotkeys.delete(wire::parse_id(&id)?)).await
}
#[tauri::command]
pub async fn hotkeys_set_mapping_enabled(
    app: tauri::AppHandle,
    id: String,
    enabled: bool,
) -> Result<HotkeyMapping, CommandErrorDto> {
    run(app, move |s| {
        s.hotkeys
            .set_mapping_enabled(wire::parse_id(&id)?, enabled)
            .map(wire::mapping_dto)
    })
    .await
}
#[tauri::command]
pub async fn hotkeys_set_feature_enabled(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<HotkeysRuntimeStatus, CommandErrorDto> {
    let owner = app.clone();
    run(app, move |s| {
        let core = s
            .core
            .as_ref()
            .ok_or(ServiceError::Runtime("core_recovery_required"))?;
        if let Ok(mut registry) = owner.state::<super::ApplicationState>().registry.lock()
            && let Ok(id) = feature_api::FeatureId::new(qbit_hotkeys::FEATURE_ID)
        {
            let _ = registry.set_lifecycle_state(
                &id,
                if enabled {
                    feature_api::FeatureLifecycleState::Starting
                } else {
                    feature_api::FeatureLifecycleState::Stopping
                },
            );
        }
        s.hotkeys.set_enabled(core, enabled)?;
        Ok(wire::status(&s.hotkeys))
    })
    .await
}
#[tauri::command]
pub async fn hotkeys_get_runtime_status(
    app: tauri::AppHandle,
) -> Result<HotkeysRuntimeStatus, CommandErrorDto> {
    run(app, move |s| Ok(wire::status(&s.hotkeys))).await
}
#[tauri::command]
pub async fn hotkeys_analyze_candidate(
    app: tauri::AppHandle,
    id: Option<String>,
    input: HotkeyMappingInput,
) -> Result<HotkeysConflictReport, CommandErrorDto> {
    run(app, move |s| {
        let id = id
            .as_deref()
            .map(wire::parse_id)
            .transpose()?
            .unwrap_or_else(uuid::Uuid::new_v4);
        s.hotkeys
            .analyze_report(wire::mapping(id, input)?)
            .map(wire::conflict_report)
    })
    .await
}
#[tauri::command]
pub async fn get_autostart(app: tauri::AppHandle) -> Result<AutostartStatus, CommandErrorDto> {
    let os = app.clone();
    run(app, move |s| {
        // A transient startup failure must not permanently disable recovery.
        // Re-read OS truth on Retry; the DTO exposes saved/actual disagreement.
        super::autostart::status(s.core.as_ref().ok_or(ServiceError::Persistence)?, &os)
    })
    .await
}
#[tauri::command]
pub async fn set_autostart(
    app: tauri::AppHandle,
    enabled: bool,
) -> Result<AutostartStatus, CommandErrorDto> {
    let os = app.clone();
    run(app, move |s| {
        super::autostart::set(
            s.core.as_ref().ok_or(ServiceError::Persistence)?,
            &os,
            enabled,
        )
    })
    .await
}
