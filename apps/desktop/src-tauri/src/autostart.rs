//! App-global startup setting with explicit OS/persistence compensation.
use ipc_contracts::AutostartStatus;
use persistence::CoreStore;
use qbit_hotkeys::service::ServiceError;
use tauri_plugin_autostart::ManagerExt;
pub trait Preferences {
    fn desired(&self) -> Result<bool, ServiceError>;
    fn persist(&self, enabled: bool) -> Result<(), ServiceError>;
}
impl Preferences for CoreStore {
    fn desired(&self) -> Result<bool, ServiceError> {
        self.autostart_enabled()
            .map_err(|_| ServiceError::Persistence)
    }
    fn persist(&self, enabled: bool) -> Result<(), ServiceError> {
        self.set_autostart_enabled(enabled)
            .map_err(|_| ServiceError::Persistence)
    }
}
pub trait Registration {
    fn registered(&self) -> Result<bool, ServiceError>;
    fn set(&self, enabled: bool) -> Result<(), ServiceError>;
}
impl Registration for tauri::AppHandle {
    fn registered(&self) -> Result<bool, ServiceError> {
        self.autolaunch()
            .is_enabled()
            .map_err(|_| ServiceError::Runtime("autostart_read_failed"))
    }
    fn set(&self, enabled: bool) -> Result<(), ServiceError> {
        let manager = self.autolaunch();
        (if enabled {
            manager.enable()
        } else {
            manager.disable()
        })
        .map_err(|_| ServiceError::Runtime("autostart_write_failed"))
    }
}
pub fn status(
    core: &impl Preferences,
    os: &impl Registration,
) -> Result<AutostartStatus, ServiceError> {
    if !cfg!(windows) {
        return Err(ServiceError::PlatformUnsupported);
    }
    Ok(AutostartStatus {
        desired_enabled: core.desired().map_err(|_| ServiceError::Persistence)?,
        registered: os.registered()?,
    })
}
pub fn set(
    core: &impl Preferences,
    os: &impl Registration,
    enabled: bool,
) -> Result<AutostartStatus, ServiceError> {
    if !cfg!(windows) {
        return Err(ServiceError::PlatformUnsupported);
    }
    let previous = os.registered()?;
    os.set(enabled)?;
    if os.registered() != Ok(enabled) {
        os.set(previous)
            .map_err(|_| ServiceError::Runtime("autostart_compensation_failed"))?;
        return Err(ServiceError::Runtime("autostart_verification_failed"));
    }
    if core.persist(enabled).is_err() {
        os.set(previous)
            .map_err(|_| ServiceError::Runtime("autostart_compensation_failed"))?;
        return Err(ServiceError::Persistence);
    }
    status(core, os)
}
/// Persisted user intent wins on startup; errors remain visible to the caller.
pub fn reconcile(core: &impl Preferences, os: &impl Registration) -> Result<(), ServiceError> {
    if cfg!(windows) {
        let desired = core.desired().map_err(|_| ServiceError::Persistence)?;
        if os.registered()? != desired {
            os.set(desired)?;
        }
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Fake {
        registered: Cell<bool>,
        fail: bool,
    }
    impl Registration for Fake {
        fn registered(&self) -> Result<bool, ServiceError> {
            Ok(self.registered.get())
        }
        fn set(&self, enabled: bool) -> Result<(), ServiceError> {
            if self.fail {
                Err(ServiceError::Runtime("test_os_failed"))
            } else {
                self.registered.set(enabled);
                Ok(())
            }
        }
    }
    #[test]
    fn fake_registration_reconciles_and_failed_os_write_preserves_preference() {
        let root = tempfile::tempdir().unwrap();
        let core = CoreStore::open(root.path(), persistence::DurabilityProfile::Critical).unwrap();
        let os = Fake {
            registered: Cell::new(false),
            fail: false,
        };
        assert!(!status(&core, &os).unwrap().desired_enabled);
        set(&core, &os, true).unwrap();
        assert!(os.registered.get());
        os.registered.set(false);
        reconcile(&core, &os).unwrap();
        assert!(os.registered.get());
        let failing = Fake {
            registered: Cell::new(true),
            fail: true,
        };
        assert!(set(&core, &failing, false).is_err());
        assert!(core.desired().unwrap());
    }
    #[test]
    fn persisted_failure_compensates_os_registration() {
        let core = FailingStore;

        let os = Fake {
            registered: Cell::new(false),
            fail: false,
        };
        assert!(set(&core, &os, true).is_err());
        assert!(!os.registered.get());
        assert!(!core.desired().unwrap());
    }
    #[test]
    fn failed_post_change_verification_restores_previous_registration() {
        struct Flaky {
            value: Cell<bool>,
            reads: Cell<u8>,
        }
        impl Registration for Flaky {
            fn registered(&self) -> Result<bool, ServiceError> {
                self.reads.set(self.reads.get() + 1);
                if self.reads.get() == 2 {
                    Err(ServiceError::Runtime("test_read_failed"))
                } else {
                    Ok(self.value.get())
                }
            }
            fn set(&self, enabled: bool) -> Result<(), ServiceError> {
                self.value.set(enabled);
                Ok(())
            }
        }
        let root = tempfile::tempdir().unwrap();
        let core = CoreStore::open(root.path(), persistence::DurabilityProfile::Critical).unwrap();
        let os = Flaky {
            value: Cell::new(false),
            reads: Cell::new(0),
        };
        assert_eq!(
            set(&core, &os, true),
            Err(ServiceError::Runtime("autostart_verification_failed"))
        );
        assert!(!os.value.get());
        assert!(!core.autostart_enabled().unwrap());
    }
    struct FailingStore;
    impl Preferences for FailingStore {
        fn desired(&self) -> Result<bool, ServiceError> {
            Ok(false)
        }
        fn persist(&self, _: bool) -> Result<(), ServiceError> {
            Err(ServiceError::Persistence)
        }
    }
}
