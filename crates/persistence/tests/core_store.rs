use persistence::{CoreStore, DurabilityProfile, PersistenceError};
use tempfile::tempdir;

#[test]
fn fresh_open_creates_current_schema_at_the_core_database_path() {
    let directory = tempdir().unwrap();
    let store = CoreStore::open(directory.path(), DurabilityProfile::Critical).unwrap();

    assert_eq!(store.schema_version().unwrap(), 2);
    assert_eq!(
        store.database_path(),
        directory.path().join("data").join("core.db")
    );
    assert!(store.database_path().is_file());
}

#[test]
fn reopening_is_idempotent_and_preserves_schema_and_state() {
    let directory = tempdir().unwrap();
    let database_path;

    {
        let store = CoreStore::open(directory.path(), DurabilityProfile::Standard).unwrap();
        store.set_feature_enabled("qbit.example", true).unwrap();
        database_path = store.database_path().to_owned();
    }

    let reopened = CoreStore::open(directory.path(), DurabilityProfile::Standard).unwrap();

    assert_eq!(reopened.database_path(), database_path);
    assert_eq!(reopened.schema_version().unwrap(), 2);
    assert_eq!(
        reopened.feature_enabled("qbit.example").unwrap(),
        Some(true)
    );
}

#[test]
fn feature_enabled_insert_and_update_roundtrip() {
    let directory = tempdir().unwrap();
    let store = CoreStore::open(directory.path(), DurabilityProfile::Standard).unwrap();

    assert_eq!(store.feature_enabled("qbit.example").unwrap(), None);
    store.set_feature_enabled("qbit.example", true).unwrap();
    assert_eq!(store.feature_enabled("qbit.example").unwrap(), Some(true));
    store.set_feature_enabled("qbit.example", false).unwrap();
    assert_eq!(store.feature_enabled("qbit.example").unwrap(), Some(false));
}

#[test]
fn v1_upgrade_preserves_feature_intent_and_autostart_defaults_off() {
    let root = tempdir().unwrap();
    std::fs::create_dir(root.path().join("data")).unwrap();
    let connection = rusqlite::Connection::open(root.path().join("data/core.db")).unwrap();
    connection.execute_batch("CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL); INSERT INTO schema_migrations VALUES(1,'create_feature_state'); CREATE TABLE feature_state(feature_id TEXT PRIMARY KEY, enabled INTEGER NOT NULL CHECK(enabled IN (0,1))); INSERT INTO feature_state VALUES('qbit.hotkeys',1);").unwrap();
    drop(connection);
    let store = CoreStore::open(root.path(), DurabilityProfile::Critical).unwrap();
    assert_eq!(store.schema_version().unwrap(), 2);
    assert_eq!(store.feature_enabled("qbit.hotkeys").unwrap(), Some(true));
    assert!(!store.autostart_enabled().unwrap());
    store.set_autostart_enabled(true).unwrap();
    drop(store);
    assert!(
        CoreStore::open(root.path(), DurabilityProfile::Critical)
            .unwrap()
            .autostart_enabled()
            .unwrap()
    );
}

#[test]
fn rebuildable_is_rejected_without_creating_core_database() {
    let directory = tempdir().unwrap();
    let database_path = directory.path().join("data").join("core.db");

    assert!(matches!(
        CoreStore::open(directory.path(), DurabilityProfile::Rebuildable),
        Err(PersistenceError::InvalidDurabilityProfile {
            profile: DurabilityProfile::Rebuildable
        })
    ));
    assert!(!database_path.exists());
}
