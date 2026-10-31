use super::{
    compiled::{self, NativeCompiledKeymap, PlatformCompileError, PublishedKeymap},
    engine::WindowsEngine,
};
use crate::{
    domain::Mapping,
    runtime::{Metrics, MetricsSnapshot},
    service::{Backend, ServiceError},
};
use std::sync::Arc;

#[derive(Default)]
pub struct WindowsBackend {
    engine: Option<WindowsEngine>,
    map: Arc<PublishedKeymap>,
    metrics: Arc<Metrics>,
}
impl Backend for WindowsBackend {
    type Candidate = Arc<NativeCompiledKeymap>;
    fn compile(&self, mappings: &[Mapping]) -> Result<Self::Candidate, ServiceError> {
        compiled::compile(mappings)
            .map(Arc::new)
            .map_err(|error| match error {
                PlatformCompileError::Semantic(_) | PlatformCompileError::AliasCollision { .. } => {
                    ServiceError::Conflict
                }
                _ => ServiceError::Validation,
            })
    }
    fn start(&mut self, map: Self::Candidate) -> Result<(), ServiceError> {
        if self.engine.is_some() {
            return Ok(());
        }
        self.metrics.set_mapping_count(map.mapping_count());
        self.map.publish(map);
        self.engine = Some(
            WindowsEngine::start(self.map.clone(), self.metrics.clone())
                .map_err(ServiceError::Runtime)?,
        );
        Ok(())
    }
    fn publish(&mut self, map: Self::Candidate) {
        self.metrics.set_mapping_count(map.mapping_count());
        self.map.publish(map);
    }
    fn stop(&mut self) -> Result<(), ServiceError> {
        if let Some(engine) = self.engine.as_mut() {
            engine.stop().map_err(ServiceError::Runtime)?;
        }
        self.engine = None;
        Ok(())
    }
    fn metrics(&self) -> MetricsSnapshot {
        self.metrics.snapshot()
    }
    fn analyze(
        &self,
        mappings: &[Mapping],
    ) -> Vec<(
        ipc_contracts::HotkeyConflictCode,
        ipc_contracts::HotkeyConflictSeverity,
    )> {
        use ipc_contracts::HotkeyConflictCode as Code;
        let semantic = crate::application::analyze(mappings);
        let mut issues: Vec<_> = semantic
            .into_iter()
            .map(|conflict| {
                (
                    match conflict.code {
                        crate::application::ConflictCode::DirectSelfMap => Code::SelfMap,
                        crate::application::ConflictCode::DuplicateEnabledTrigger => {
                            Code::DuplicateTrigger
                        }
                        crate::application::ConflictCode::EmitShortcutCycle => Code::Cycle,
                    },
                    match conflict.severity {
                        crate::application::ConflictSeverity::Error => {
                            ipc_contracts::HotkeyConflictSeverity::Error
                        }
                        crate::application::ConflictSeverity::Warning => {
                            ipc_contracts::HotkeyConflictSeverity::Warning
                        }
                    },
                )
            })
            .collect();
        if issues.is_empty()
            && let Err(error) = compiled::compile(mappings)
        {
            issues.push((
                match error {
                    PlatformCompileError::AliasCollision { .. } => Code::AliasCollision,
                    PlatformCompileError::SecureAttentionSequence { .. } => {
                        Code::SecureAttentionSequence
                    }
                    PlatformCompileError::Key(_) => Code::UnsupportedKey,
                    _ => Code::InvalidMapping,
                },
                ipc_contracts::HotkeyConflictSeverity::Error,
            ));
        }
        issues
    }
}
