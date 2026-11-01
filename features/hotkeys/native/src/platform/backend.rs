//! Platform selection. Unsupported platforms expose availability, never a dummy runtime.
#[cfg(windows)]
pub use super::windows::backend::WindowsBackend as SystemBackend;

#[cfg(not(windows))]
#[derive(Default)]
pub struct SystemBackend;
#[cfg(not(windows))]
impl crate::service::Backend for SystemBackend {
    type Candidate = std::convert::Infallible;
    fn compile(
        &self,
        _: &[crate::domain::Mapping],
    ) -> Result<Self::Candidate, crate::service::ServiceError> {
        Err(crate::service::ServiceError::PlatformUnsupported)
    }
    fn start(&mut self, candidate: Self::Candidate) -> Result<(), crate::service::ServiceError> {
        match candidate {}
    }
    fn publish(&mut self, candidate: Self::Candidate) {
        match candidate {}
    }
    fn stop(&mut self) -> Result<(), crate::service::ServiceError> {
        Ok(())
    }
    fn metrics(&self) -> crate::runtime::MetricsSnapshot {
        Default::default()
    }
}
