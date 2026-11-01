#![cfg(windows)]
use crate::{
    FEATURE_ID,
    domain::*,
    persistence::HotkeysRepository,
    platform::windows::compiled::{self, NativeCompiledKeymap, PublishedKeymap},
    runtime::MetricsSnapshot,
    service::{Backend, HotkeysService, ServiceError},
};
use persistence::{CoreStore, DurabilityProfile};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};
use tempfile::tempdir;

#[derive(Default)]
struct Trace {
    active: bool,
    published: usize,
    fail_start: bool,
    fail_stop: bool,
    stop_attempts: usize,
}
struct Fake(Arc<Mutex<Trace>>);
impl Backend for Fake {
    type Candidate = compiled::NativeCompiledKeymap;
    fn compile(&self, mappings: &[Mapping]) -> Result<Self::Candidate, ServiceError> {
        compiled::compile(mappings).map_err(|_| ServiceError::Conflict)
    }
    fn start(&mut self, map: Self::Candidate) -> Result<(), ServiceError> {
        let mut trace = self.0.lock().unwrap();
        if trace.fail_start {
            return Err(ServiceError::Runtime("test_install_failed"));
        }
        trace.active = true;
        trace.published = map.mapping_count();
        Ok(())
    }
    fn publish(&mut self, map: Self::Candidate) {
        self.0.lock().unwrap().published = map.mapping_count();
    }
    fn stop(&mut self) -> Result<(), ServiceError> {
        let mut trace = self.0.lock().unwrap();
        trace.stop_attempts += 1;
        if trace.fail_stop {
            return Err(ServiceError::Runtime("hook_stop_timeout"));
        }
        trace.active = false;
        Ok(())
    }
    fn metrics(&self) -> MetricsSnapshot {
        let trace = self.0.lock().unwrap();
        MetricsSnapshot {
            hook_active: trace.active,
            mapping_count: trace.published,
            ..Default::default()
        }
    }
}

#[derive(Default)]
struct PublicationTrace {
    active: bool,
    starts: usize,
    publications: usize,
    drops: usize,
}

struct AcceptanceBackend {
    map: Arc<PublishedKeymap>,
    trace: Arc<Mutex<PublicationTrace>>,
}

impl AcceptanceBackend {
    fn new(map: Arc<PublishedKeymap>, trace: Arc<Mutex<PublicationTrace>>) -> Self {
        Self { map, trace }
    }
}

impl Drop for AcceptanceBackend {
    fn drop(&mut self) {
        self.trace.lock().unwrap().drops += 1;
    }
}

impl Backend for AcceptanceBackend {
    type Candidate = Arc<NativeCompiledKeymap>;

    fn compile(&self, mappings: &[Mapping]) -> Result<Self::Candidate, ServiceError> {
        compiled::compile(mappings)
            .map(Arc::new)
            .map_err(|_| ServiceError::Validation)
    }

    fn start(&mut self, candidate: Self::Candidate) -> Result<(), ServiceError> {
        self.publish(candidate);
        let mut trace = self.trace.lock().unwrap();
        trace.active = true;
        trace.starts += 1;
        Ok(())
    }

    fn publish(&mut self, candidate: Self::Candidate) {
        self.map.publish(candidate);
        self.trace.lock().unwrap().publications += 1;
    }

    fn stop(&mut self) -> Result<(), ServiceError> {
        self.trace.lock().unwrap().active = false;
        Ok(())
    }

    fn metrics(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            hook_active: self.trace.lock().unwrap().active,
            mapping_count: self.map.load().mapping_count(),
            ..Default::default()
        }
    }
}

#[test]
fn failed_shutdown_retains_native_owner_rejects_mutations_and_retry_clears_error() {
    let root = tempdir().unwrap();
    let core = core(root.path());
    let trace = Arc::new(Mutex::new(Trace::default()));
    let mut service = HotkeysService::new(root.path().into(), Fake(trace.clone()));
    service.save(mapping(), true).unwrap();
    service.set_enabled(&core, true).unwrap();
    trace.lock().unwrap().fail_stop = true;
    assert_eq!(
        service.shutdown(),
        Err(ServiceError::Runtime("hook_stop_timeout"))
    );
    assert!(service.metrics().hook_active);
    assert_eq!(
        service.lifecycle(),
        feature_api::FeatureLifecycleState::Failed
    );
    assert_eq!(service.list(), Err(ServiceError::Runtime("shutting_down")));
    assert_eq!(
        service.set_enabled(&core, true),
        Err(ServiceError::Runtime("shutting_down"))
    );
    assert_eq!(core.feature_enabled(FEATURE_ID).unwrap(), Some(true));
    let attempts = trace.lock().unwrap().stop_attempts;
    trace.lock().unwrap().fail_stop = false;
    service.shutdown().unwrap();
    assert_eq!(trace.lock().unwrap().stop_attempts, attempts + 1);
    assert!(!service.metrics().hook_active);
    assert_eq!(service.last_error(), None);
    assert_eq!(
        service.lifecycle(),
        feature_api::FeatureLifecycleState::Disabled
    );
    // Quit cleanup retains persisted intent for the next launch.
    assert_eq!(core.feature_enabled(FEATURE_ID).unwrap(), Some(true));
}
fn mapping() -> Mapping {
    Mapping::new(
        true,
        Trigger::Chord {
            chord: Chord::new(
                [Modifier::Ctrl],
                KeyIdentity::Logical(KeyToken::new("KeyA").unwrap()),
            )
            .unwrap(),
        },
        Action::Disable,
        Scope::Global,
        MappingBehavior::default(),
        MappingMetadata::default(),
    )
}

fn acceptance_mapping(trigger_key: &str) -> Mapping {
    Mapping::with_id(
        uuid::Uuid::from_u128(if trigger_key == "F9" { 0x9001 } else { 0x9002 }),
        true,
        Trigger::Chord {
            chord: Chord::new(
                [Modifier::Ctrl, Modifier::Shift],
                KeyIdentity::Logical(KeyToken::new(trigger_key).unwrap()),
            )
            .unwrap(),
        },
        Action::EmitShortcut {
            chord: Chord::new(
                [Modifier::Ctrl, Modifier::Shift],
                KeyIdentity::Logical(KeyToken::new("F10").unwrap()),
            )
            .unwrap(),
        },
        Scope::Global,
        MappingBehavior::default(),
        MappingMetadata::default(),
    )
}

#[test]
fn acceptance_service_persists_publishes_reloads_and_keeps_enable_idempotent() {
    let root = tempdir().unwrap();
    let core = core(root.path());
    let published = Arc::new(PublishedKeymap::default());
    let trace = Arc::new(Mutex::new(PublicationTrace::default()));
    let backend = AcceptanceBackend::new(published.clone(), trace.clone());
    let mut service = HotkeysService::new(root.path().into(), backend);
    let mapping = acceptance_mapping("F9");

    service.save(mapping.clone(), true).unwrap();
    assert_eq!(service.list().unwrap(), vec![mapping.clone()]);
    assert_eq!(published.load().mapping_count(), 1);
    assert_eq!(trace.lock().unwrap().publications, 1);
    assert!(!trace.lock().unwrap().active);

    service.set_enabled(&core, true).unwrap();
    assert_eq!(core.feature_enabled(FEATURE_ID).unwrap(), Some(true));
    assert_eq!(published.load().mapping_count(), 1);
    assert!(trace.lock().unwrap().active);
    assert_eq!(trace.lock().unwrap().starts, 1);
    service.set_enabled(&core, true).unwrap();
    assert_eq!(
        trace.lock().unwrap().starts,
        1,
        "re-enabling an active runtime is idempotent"
    );

    // Exercise dispatch from the actual service-published map, rather than
    // inferring it from a status metric. No OS hook or injector is involved.
    use crate::platform::windows::{
        engine::Shared,
        hot_path::{Decision, HotPathState, InputOrigin, KeyEventKind, RawEvent},
    };
    let shared = Shared::new(
        published.clone(),
        Arc::new(crate::runtime::Metrics::default()),
    );
    let dispatch = |vk_code, scan_code| {
        let mut state = HotPathState::default();
        for (vk_code, scan_code) in [(0xa2, 0x1d), (0xa0, 0x2a)] {
            state.process(
                RawEvent {
                    kind: KeyEventKind::Down,
                    vk_code,
                    scan_code,
                    extended: false,
                    origin: InputOrigin::Physical,
                },
                &published.load(),
                &shared.queue,
                &shared.metrics,
            );
        }
        state.process(
            RawEvent {
                kind: KeyEventKind::Down,
                vk_code,
                scan_code,
                extended: false,
                origin: InputOrigin::Physical,
            },
            &published.load(),
            &shared.queue,
            &shared.metrics,
        )
    };
    assert_eq!(dispatch(0x78, 0x43), Decision::Consume);
    assert!(shared.queue.pop().is_some());
    service.set_mapping_enabled(mapping.id, false).unwrap();
    assert_eq!(published.load().mapping_count(), 0);
    assert_eq!(dispatch(0x78, 0x43), Decision::Pass);
    assert!(shared.queue.pop().is_none());
    service.set_mapping_enabled(mapping.id, true).unwrap();
    assert_eq!(dispatch(0x78, 0x43), Decision::Consume);
    assert!(shared.queue.pop().is_some());
    assert_eq!(
        trace.lock().unwrap().starts,
        1,
        "row hot reload never restarts the backend"
    );
    service.set_enabled(&core, false).unwrap();
    assert!(
        !trace.lock().unwrap().active,
        "off removes native ingress ownership"
    );
    assert!(
        service.list().unwrap()[0].enabled,
        "feature off preserves enabled mapping"
    );
    service.set_enabled(&core, true).unwrap();
    assert_eq!(dispatch(0x78, 0x43), Decision::Consume);
    assert!(shared.queue.pop().is_some());

    // A real trigger/action edit must remove old dispatch and preserve the ID.
    let mut edited = mapping.clone();
    edited.trigger = acceptance_mapping("F8").trigger;
    edited.action = Action::Disable;
    let starts_before_edit = trace.lock().unwrap().starts;
    service.save(edited.clone(), false).unwrap();
    assert_eq!(service.list().unwrap(), vec![edited]);
    assert_eq!(
        dispatch(0x78, 0x43),
        Decision::Pass,
        "stale F9 trigger is gone"
    );
    assert!(shared.queue.pop().is_none());
    assert_eq!(
        dispatch(0x77, 0x42),
        Decision::Consume,
        "new F8 Disable trigger is published"
    );
    assert!(
        shared.queue.pop().is_none(),
        "Disable action never queues emission"
    );
    assert_eq!(trace.lock().unwrap().starts, starts_before_edit);
    service.save(mapping.clone(), false).unwrap();
    assert_eq!(dispatch(0x78, 0x43), Decision::Consume);
    assert!(shared.queue.pop().is_some());

    service.shutdown().unwrap();
    assert!(!trace.lock().unwrap().active);
    assert_eq!(core.feature_enabled(FEATURE_ID).unwrap(), Some(true));

    let reloaded_map = Arc::new(PublishedKeymap::default());
    let reloaded_trace = Arc::new(Mutex::new(PublicationTrace::default()));
    let mut restarted = HotkeysService::new(
        root.path().into(),
        AcceptanceBackend::new(reloaded_map.clone(), reloaded_trace.clone()),
    );
    restarted.initialize(&core).unwrap();
    assert!(reloaded_trace.lock().unwrap().active);
    assert_eq!(reloaded_map.load().mapping_count(), 1);
    assert_eq!(restarted.list().unwrap(), vec![mapping.clone()]);
    restarted.delete(mapping.id).unwrap();
    assert_eq!(reloaded_map.load().mapping_count(), 0);
    assert!(restarted.list().unwrap().is_empty());
    restarted.set_enabled(&core, false).unwrap();
    assert_eq!(core.feature_enabled(FEATURE_ID).unwrap(), Some(false));
    assert!(!reloaded_trace.lock().unwrap().active);
}

#[test]
fn acceptance_lane_quit_releases_service_and_databases_then_off_restart_preserves_mapping() {
    use persistence::lane::{BlockingLane, ShutdownPhase};
    use std::time::Duration;
    let root = tempdir().unwrap();
    let core = core(root.path());
    let published = Arc::new(PublishedKeymap::default());
    let trace = Arc::new(Mutex::new(PublicationTrace::default()));
    let mut service = HotkeysService::new(
        root.path().into(),
        AcceptanceBackend::new(published, trace.clone()),
    );
    let saved = acceptance_mapping("F9");
    service.save(saved.clone(), true).unwrap();
    service.set_enabled(&core, true).unwrap();
    let core_path = core.database_path().to_owned();
    let feature_path = HotkeysRepository::open(root.path())
        .unwrap()
        .database_path()
        .to_owned();
    let lane = BlockingLane::new_with_shutdown((core, service), 2, |(_, service)| {
        service
            .shutdown()
            .map_err(|_| persistence::lane::LaneError::CleanupFailed)
    })
    .unwrap();
    // Accepted work must complete before cleanup can drop its service/backend.
    let changed = lane
        .submit(|(core, service)| service.set_enabled(core, false))
        .unwrap();
    lane.close().unwrap();
    changed
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(
        lane.shutdown_status().unwrap().phase,
        ShutdownPhase::Complete
    );
    assert!(!trace.lock().unwrap().active);
    assert_eq!(
        trace.lock().unwrap().drops,
        1,
        "backend ownership released before Complete"
    );
    // These are owned temporary files only. Windows sharing denial would expose
    // a retained SQLite connection; WAL/SHM presence is deliberately irrelevant.
    for path in [&core_path, &feature_path] {
        use std::os::windows::fs::OpenOptionsExt;
        let exclusive = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(path)
            .unwrap();
        drop(exclusive);
        let db =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        assert_eq!(
            db.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    }
    let reopened_core = CoreStore::open(root.path(), DurabilityProfile::Critical).unwrap();
    assert_eq!(
        reopened_core.feature_enabled(FEATURE_ID).unwrap(),
        Some(false)
    );
    let new_trace = Arc::new(Mutex::new(PublicationTrace::default()));
    let mut restarted = HotkeysService::new(
        root.path().into(),
        AcceptanceBackend::new(Arc::new(PublishedKeymap::default()), new_trace.clone()),
    );
    restarted.initialize(&reopened_core).unwrap();
    assert!(!restarted.desired());
    assert_eq!(
        new_trace.lock().unwrap().starts,
        0,
        "off startup never starts native ingress"
    );
    assert_eq!(restarted.list().unwrap(), vec![saved]);
    restarted.shutdown().unwrap();
}

#[test]
fn acceptance_compile_and_sqlite_failures_do_not_publish_or_change_persisted_mappings() {
    let root = tempdir().unwrap();
    let published = Arc::new(PublishedKeymap::default());
    let trace = Arc::new(Mutex::new(PublicationTrace::default()));
    let mut service = HotkeysService::new(
        root.path().into(),
        AcceptanceBackend::new(published.clone(), trace.clone()),
    );
    let original = acceptance_mapping("F9");
    service.save(original.clone(), true).unwrap();
    let prior_map = published.load();

    let unsupported = Mapping::with_id(
        uuid::Uuid::from_u128(0x9003),
        true,
        Trigger::Chord {
            chord: Chord::new([], KeyIdentity::Logical(KeyToken::new("NotAKey").unwrap())).unwrap(),
        },
        Action::Disable,
        Scope::Global,
        MappingBehavior::default(),
        MappingMetadata::default(),
    );
    assert_eq!(
        service.save(unsupported, true),
        Err(ServiceError::Validation)
    );
    assert!(Arc::ptr_eq(&prior_map, &published.load()));
    assert_eq!(service.list().unwrap(), vec![original.clone()]);

    let database = HotkeysRepository::open(root.path())
        .unwrap()
        .database_path()
        .to_owned();
    let connection = rusqlite::Connection::open(database).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_update BEFORE UPDATE ON mappings BEGIN SELECT RAISE(ABORT,'acceptance update failure'); END;",
        )
        .unwrap();
    assert_eq!(
        service.set_mapping_enabled(original.id, false),
        Err(ServiceError::Persistence)
    );
    assert!(Arc::ptr_eq(&prior_map, &published.load()));
    assert_eq!(service.list().unwrap(), vec![original.clone()]);

    connection
        .execute_batch(
            "CREATE TRIGGER reject_insert BEFORE INSERT ON mappings BEGIN SELECT RAISE(ABORT,'acceptance insert failure'); END;",
        )
        .unwrap();
    assert_eq!(
        service.save(acceptance_mapping("F8"), true),
        Err(ServiceError::Persistence)
    );
    assert!(Arc::ptr_eq(&prior_map, &published.load()));
    assert_eq!(service.list().unwrap(), vec![original]);
    assert_eq!(published.load().mapping_count(), 1);
    assert_eq!(trace.lock().unwrap().publications, 1);
    service.shutdown().unwrap();
}
fn core(path: &Path) -> CoreStore {
    CoreStore::open(path, DurabilityProfile::Critical).unwrap()
}
#[test]
fn default_disabled_transient_crud_compile_before_commit_and_restart() {
    let root = tempdir().unwrap();
    let core = core(root.path());
    let trace = Arc::new(Mutex::new(Trace::default()));
    let mut service = HotkeysService::new(root.path().into(), Fake(trace.clone()));
    service.initialize(&core).unwrap();
    assert!(!service.desired());
    assert!(!trace.lock().unwrap().active);
    assert!(!root.path().join("data/features").exists());
    let mapping = service.save(mapping(), true).unwrap();
    let mut conflict = mapping.clone();
    conflict.id = uuid::Uuid::new_v4();
    assert_eq!(service.save(conflict, true), Err(ServiceError::Conflict));
    assert_eq!(service.list().unwrap(), vec![mapping.clone()]);
    assert!(!trace.lock().unwrap().active);
    service.set_enabled(&core, true).unwrap();
    assert!(trace.lock().unwrap().active);
    service.set_mapping_enabled(mapping.id, false).unwrap();
    assert_eq!(trace.lock().unwrap().published, 0);
    service.set_mapping_enabled(mapping.id, true).unwrap();
    assert_eq!(trace.lock().unwrap().published, 1);
    service.shutdown().unwrap();
    assert!(!trace.lock().unwrap().active);
    let mut restarted = HotkeysService::new(root.path().into(), Fake(trace.clone()));
    restarted.initialize(&core).unwrap();
    assert!(trace.lock().unwrap().active);
    assert_eq!(restarted.list().unwrap(), vec![mapping.clone()]);
    restarted.delete(mapping.id).unwrap();
    assert_eq!(trace.lock().unwrap().published, 0);
    restarted.set_enabled(&core, false).unwrap();
    assert_eq!(core.feature_enabled(FEATURE_ID).unwrap(), Some(false));
    assert!(!trace.lock().unwrap().active);
}
#[test]
fn failed_db_commit_does_not_publish_and_failed_start_retains_desired_state() {
    let root = tempdir().unwrap();
    let core = core(root.path());
    let trace = Arc::new(Mutex::new(Trace::default()));
    let mut service = HotkeysService::new(root.path().into(), Fake(trace.clone()));
    let original = service.save(mapping(), true).unwrap();
    service.set_enabled(&core, true).unwrap();
    let path = HotkeysRepository::open(root.path())
        .unwrap()
        .database_path()
        .to_owned();
    let connection = rusqlite::Connection::open(path).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_update BEFORE UPDATE ON mappings BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert_eq!(
        service.set_mapping_enabled(original.id, false),
        Err(ServiceError::Persistence)
    );
    assert_eq!(trace.lock().unwrap().published, 1);
    assert_eq!(service.list().unwrap(), vec![original]);
    service.shutdown().unwrap();
    trace.lock().unwrap().fail_start = true;
    let mut restarted = HotkeysService::new(root.path().into(), Fake(trace.clone()));
    assert!(restarted.initialize(&core).is_err());
    assert!(restarted.desired());
    assert_eq!(core.feature_enabled(FEATURE_ID).unwrap(), Some(true));
    assert!(!trace.lock().unwrap().active);
    assert_eq!(
        restarted.last_error(),
        Some(ServiceError::Runtime("test_install_failed"))
    );
}
#[test]
fn failed_core_state_write_compensates_new_runtime() {
    let root = tempdir().unwrap();
    let core = core(root.path());
    let trace = Arc::new(Mutex::new(Trace::default()));
    let mut service = HotkeysService::new(root.path().into(), Fake(trace.clone()));
    let connection = rusqlite::Connection::open(core.database_path()).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_core BEFORE INSERT ON feature_state BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert_eq!(
        service.set_enabled(&core, true),
        Err(ServiceError::Persistence)
    );
    assert!(!trace.lock().unwrap().active);
    assert!(!service.desired());
    assert_eq!(core.feature_enabled(FEATURE_ID).unwrap(), None);
}
