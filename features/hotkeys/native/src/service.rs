//! Serialized feature configuration and lifecycle. Called only on the shared
//! bounded blocking persistence lane. No Tauri or OS types cross this boundary.
use crate::{
    FEATURE_ID,
    domain::Mapping,
    persistence::{HotkeysRepository, RepositoryError},
    runtime::MetricsSnapshot,
};
use feature_api::FeatureLifecycleState;
use persistence::CoreStore;
use std::path::PathBuf;
use uuid::Uuid;

pub const MAX_MAPPINGS: usize = 512;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceError {
    Validation,
    Conflict,
    NotFound,
    Persistence,
    Migration,
    PlatformUnsupported,
    Runtime(&'static str),
}
impl From<RepositoryError> for ServiceError {
    fn from(error: RepositoryError) -> Self {
        match error {
            RepositoryError::Shared(
                persistence::PersistenceError::Migration { .. }
                | persistence::PersistenceError::FutureSchema { .. }
                | persistence::PersistenceError::InconsistentSchema { .. },
            ) => Self::Migration,
            RepositoryError::NotFound { .. } => Self::NotFound,
            _ => Self::Persistence,
        }
    }
}
pub trait Backend: Send + 'static {
    type Candidate;
    fn compile(&self, mappings: &[Mapping]) -> Result<Self::Candidate, ServiceError>;
    fn start(&mut self, map: Self::Candidate) -> Result<(), ServiceError>;
    fn publish(&mut self, map: Self::Candidate);
    fn stop(&mut self) -> Result<(), ServiceError>;
    fn metrics(&self) -> MetricsSnapshot;
    fn analyze(
        &self,
        mappings: &[Mapping],
    ) -> Vec<(
        ipc_contracts::HotkeyConflictCode,
        ipc_contracts::HotkeyConflictSeverity,
    )> {
        if self.compile(mappings).is_err() {
            vec![(
                ipc_contracts::HotkeyConflictCode::InvalidMapping,
                ipc_contracts::HotkeyConflictSeverity::Error,
            )]
        } else {
            vec![]
        }
    }
}
pub struct HotkeysService<B: Backend> {
    root: PathBuf,
    backend: B,
    repository: Option<HotkeysRepository>,
    desired: bool,
    last_error: Option<ServiceError>,
    stopping: bool,
    reload_failures: usize,
    lifecycle: FeatureLifecycleState,
}
impl<B: Backend> HotkeysService<B> {
    pub fn new(root: PathBuf, backend: B) -> Self {
        Self {
            root,
            backend,
            repository: None,
            desired: false,
            last_error: None,
            stopping: false,
            reload_failures: 0,
            lifecycle: if cfg!(windows) {
                FeatureLifecycleState::Disabled
            } else {
                FeatureLifecycleState::Unavailable
            },
        }
    }
    pub fn desired(&self) -> bool {
        self.desired
    }
    pub fn last_error(&self) -> Option<ServiceError> {
        self.last_error
    }
    pub fn metrics(&self) -> MetricsSnapshot {
        self.backend.metrics()
    }
    pub fn reload_failures(&self) -> usize {
        self.reload_failures
    }
    pub fn lifecycle(&self) -> FeatureLifecycleState {
        if !cfg!(windows) {
            FeatureLifecycleState::Unavailable
        } else if self.last_error.is_some()
            || (self.lifecycle == FeatureLifecycleState::Running && !self.metrics().hook_active)
        {
            FeatureLifecycleState::Failed
        } else {
            self.lifecycle
        }
    }
    pub fn initialize(&mut self, core: &CoreStore) -> Result<(), ServiceError> {
        self.desired = core
            .feature_enabled(FEATURE_ID)
            .map_err(|_| ServiceError::Persistence)
            .inspect_err(|error| self.last_error = Some(*error))?
            .unwrap_or(false);
        if self.desired {
            let result = self.start();
            if let Err(error) = result {
                self.last_error = Some(error);
            }
            result
        } else {
            Ok(())
        }
    }
    fn start(&mut self) -> Result<(), ServiceError> {
        self.lifecycle = FeatureLifecycleState::Starting;
        let result = self.start_inner();
        self.lifecycle = if result.is_ok() {
            FeatureLifecycleState::Running
        } else {
            FeatureLifecycleState::Failed
        };
        if result.is_ok() {
            self.last_error = None;
        }
        result
    }
    fn start_inner(&mut self) -> Result<(), ServiceError> {
        if self.repository.is_some() && self.backend.metrics().hook_active {
            return Ok(());
        }
        self.backend.stop()?;
        self.repository = None;
        let repository = HotkeysRepository::open(&self.root)?;
        let mappings = repository.list_mappings()?;
        if mappings.len() > MAX_MAPPINGS {
            return Err(ServiceError::Validation);
        }
        let candidate = self.backend.compile(&mappings)?;
        self.backend.start(candidate)?;
        self.repository = Some(repository);
        self.last_error = None;
        Ok(())
    }
    pub fn set_enabled(&mut self, core: &CoreStore, enabled: bool) -> Result<(), ServiceError> {
        if self.stopping {
            return Err(ServiceError::Runtime("shutting_down"));
        }
        if enabled {
            self.start()
                .inspect_err(|error| self.last_error = Some(*error))?;
            if core.set_feature_enabled(FEATURE_ID, true).is_err() {
                // Compensate a failed desired-state write by stopping the new runtime.
                self.backend
                    .stop()
                    .inspect_err(|error| self.last_error = Some(*error))?;
                self.repository = None;
                self.last_error = Some(ServiceError::Persistence);
                return Err(ServiceError::Persistence);
            }
        } else {
            self.lifecycle = FeatureLifecycleState::Stopping;
            self.backend
                .stop()
                .inspect_err(|error| self.last_error = Some(*error))?;
            self.repository = None;
            if core.set_feature_enabled(FEATURE_ID, false).is_err() {
                // Runtime is safely stopped. Retain desired=true and expose the
                // contradiction explicitly; never claim Disabled on disk.
                self.last_error = Some(ServiceError::Persistence);
                return Err(ServiceError::Persistence);
            }
        }
        self.desired = enabled;
        self.lifecycle = if enabled {
            FeatureLifecycleState::Running
        } else {
            FeatureLifecycleState::Disabled
        };
        self.last_error = None;
        Ok(())
    }
    fn with_repository<R>(
        &mut self,
        operation: impl FnOnce(&mut HotkeysRepository, &mut B) -> Result<R, ServiceError>,
    ) -> Result<R, ServiceError> {
        if self.stopping {
            return Err(ServiceError::Runtime("shutting_down"));
        }
        if let Some(repository) = self.repository.as_mut() {
            operation(repository, &mut self.backend)
        } else {
            let mut temporary = HotkeysRepository::open(&self.root)?;
            operation(&mut temporary, &mut self.backend)
        }
    }
    pub fn list(&mut self) -> Result<Vec<Mapping>, ServiceError> {
        self.with_repository(|repository, _| repository.list_mappings().map_err(Into::into))
    }
    pub fn analyze(&mut self, candidate: Mapping) -> Result<(), ServiceError> {
        self.with_repository(|repository, backend| {
            let mut mappings = repository.list_mappings()?;
            mappings.retain(|mapping| mapping.id != candidate.id);
            mappings.push(candidate);
            if mappings.len() > MAX_MAPPINGS {
                return Err(ServiceError::Validation);
            }
            backend.compile(&mappings).map(|_| ())
        })
    }
    pub fn analyze_report(
        &mut self,
        candidate: Mapping,
    ) -> Result<
        Vec<(
            ipc_contracts::HotkeyConflictCode,
            ipc_contracts::HotkeyConflictSeverity,
        )>,
        ServiceError,
    > {
        self.with_repository(|repository, backend| {
            let mut mappings = repository.list_mappings()?;
            mappings.retain(|mapping| mapping.id != candidate.id);
            mappings.push(candidate);
            if mappings.len() > MAX_MAPPINGS {
                return Err(ServiceError::Validation);
            }
            Ok(backend.analyze(&mappings))
        })
    }
    pub fn save(&mut self, mapping: Mapping, create: bool) -> Result<Mapping, ServiceError> {
        self.with_repository(|repository, backend| {
            let mut mappings = repository.list_mappings()?;
            let exists = mappings.iter().any(|m| m.id == mapping.id);
            if create && exists {
                return Err(ServiceError::Conflict);
            }
            if !create && !exists {
                return Err(ServiceError::NotFound);
            }
            mappings.retain(|m| m.id != mapping.id);
            mappings.push(mapping.clone());
            if mappings.len() > MAX_MAPPINGS {
                return Err(ServiceError::Validation);
            }
            let compiled = backend.compile(&mappings)?;
            if create {
                repository.insert_mapping(&mapping)?;
            } else {
                repository.replace_mapping(&mapping)?;
            }
            backend.publish(compiled); // Infallible: never compile after commit.
            Ok(mapping)
        })
        .inspect_err(|_| self.reload_failures += 1)
    }
    pub fn delete(&mut self, id: Uuid) -> Result<(), ServiceError> {
        self.with_repository(|repository, backend| {
            let mut mappings = repository.list_mappings()?;
            if !mappings.iter().any(|m| m.id == id) {
                return Err(ServiceError::NotFound);
            }
            mappings.retain(|m| m.id != id);
            let compiled = backend.compile(&mappings)?;
            repository.delete_mapping(id)?;
            backend.publish(compiled);
            Ok(())
        })
        .inspect_err(|_| self.reload_failures += 1)
    }
    pub fn set_mapping_enabled(
        &mut self,
        id: Uuid,
        enabled: bool,
    ) -> Result<Mapping, ServiceError> {
        let mut mapping = self
            .list()?
            .into_iter()
            .find(|m| m.id == id)
            .ok_or(ServiceError::NotFound)?;
        mapping.enabled = enabled;
        self.save(mapping, false)
    }
    pub fn shutdown(&mut self) -> Result<(), ServiceError> {
        self.stopping = true;
        self.lifecycle = FeatureLifecycleState::Stopping;
        self.backend
            .stop()
            .inspect_err(|error| self.last_error = Some(*error))?;
        self.repository = None;
        self.lifecycle = FeatureLifecycleState::Disabled;
        self.last_error = None;
        Ok(())
    }
}
