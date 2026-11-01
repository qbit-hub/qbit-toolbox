#![deny(unsafe_code)]

//! Shared SQLite connection, durability, and migration policy, plus durable core state.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use feature_api::{FeatureId, FeatureIdError};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

pub mod lane;

const DATABASE_DIRECTORY: &str = "data";
const DATABASE_FILE: &str = "core.db";
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The durability guarantees requested for a SQLite store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurabilityProfile {
    Critical,
    Standard,
    Rebuildable,
}

/// Errors raised while opening or using a persistence store.
#[derive(Debug)]
pub enum PersistenceError {
    Io {
        source: io::Error,
    },
    Sqlite {
        source: rusqlite::Error,
    },
    InvalidDurabilityProfile {
        profile: DurabilityProfile,
    },
    Migration {
        version: i64,
        name: &'static str,
        source: rusqlite::Error,
    },
    InvalidMigrationPlan {
        source: MigrationPlanError,
    },
    FutureSchema {
        current_version: i64,
        latest_supported_version: i64,
    },
    InconsistentSchema {
        detail: String,
    },
    InvalidFeatureId {
        source: FeatureIdError,
    },
}

/// Errors in a migration plan supplied to [`run_migrations`].
#[derive(Debug, Eq, PartialEq)]
pub enum MigrationPlanError {
    NonPositiveVersion { version: i64 },
    VersionsNotStrictlyIncreasing { previous_version: i64, version: i64 },
    EmptyName { version: i64 },
}

impl fmt::Display for MigrationPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonPositiveVersion { version } => {
                write!(formatter, "migration version {version} must be positive")
            }
            Self::VersionsNotStrictlyIncreasing {
                previous_version,
                version,
            } => write!(
                formatter,
                "migration version {version} must be greater than previous version {previous_version}"
            ),
            Self::EmptyName { version } => {
                write!(formatter, "migration {version} has an empty name")
            }
        }
    }
}

impl Error for MigrationPlanError {}

impl fmt::Display for PersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { source } => write!(formatter, "persistence I/O error: {source}"),
            Self::Sqlite { source } => write!(formatter, "SQLite error: {source}"),
            Self::InvalidDurabilityProfile { profile } => {
                write!(
                    formatter,
                    "durability profile {profile:?} is not valid for this store"
                )
            }
            Self::Migration {
                version,
                name,
                source,
            } => {
                write!(formatter, "migration {version} ({name}) failed: {source}")
            }
            Self::InvalidMigrationPlan { source } => {
                write!(formatter, "invalid migration plan: {source}")
            }
            Self::FutureSchema {
                current_version,
                latest_supported_version,
            } => write!(
                formatter,
                "database schema version {current_version} is newer than supported version {latest_supported_version}"
            ),
            Self::InconsistentSchema { detail } => {
                write!(formatter, "inconsistent database schema: {detail}")
            }
            Self::InvalidFeatureId { source } => write!(formatter, "invalid feature id: {source}"),
        }
    }
}

impl Error for PersistenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source } => Some(source),
            Self::Sqlite { source } | Self::Migration { source, .. } => Some(source),
            Self::InvalidMigrationPlan { source } => Some(source),
            Self::InvalidFeatureId { source } => Some(source),
            Self::InvalidDurabilityProfile { .. }
            | Self::FutureSchema { .. }
            | Self::InconsistentSchema { .. } => None,
        }
    }
}

/// An opened feature-owned database. Features retain ownership of tables and migrations;
/// this type centralizes canonical paths and SQLite connection policy.
#[derive(Debug)]
pub struct FeatureDatabase {
    connection: Connection,
    database_path: PathBuf,
}

impl FeatureDatabase {
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    /// Transfers the configured connection to the feature repository that owns its schema.
    pub fn into_connection(self) -> Connection {
        self.connection
    }
}

/// Returns the canonical location for a feature's relational database.
pub fn feature_database_path(
    app_data_root: impl AsRef<Path>,
    feature_id: &str,
) -> Result<PathBuf, PersistenceError> {
    let id = FeatureId::new(feature_id)
        .map_err(|source| PersistenceError::InvalidFeatureId { source })?;
    Ok(app_data_root
        .as_ref()
        .join(DATABASE_DIRECTORY)
        .join("features")
        .join(id.as_str())
        .join("data.db"))
}

/// Opens a feature-owned database with the shared durability and connection policy.
pub fn open_feature_database(
    app_data_root: impl AsRef<Path>,
    feature_id: &str,
    profile: DurabilityProfile,
) -> Result<FeatureDatabase, PersistenceError> {
    if profile == DurabilityProfile::Rebuildable {
        return Err(PersistenceError::InvalidDurabilityProfile { profile });
    }
    let id = FeatureId::new(feature_id)
        .map_err(|source| PersistenceError::InvalidFeatureId { source })?;
    let feature_directory = app_data_root
        .as_ref()
        .join(DATABASE_DIRECTORY)
        .join("features")
        .join(id.as_str());
    fs::create_dir_all(&feature_directory).map_err(|source| PersistenceError::Io { source })?;
    let database_path = feature_directory.join("data.db");
    let connection =
        Connection::open(&database_path).map_err(|source| PersistenceError::Sqlite { source })?;
    configure_connection(&connection, profile)?;
    Ok(FeatureDatabase {
        connection,
        database_path,
    })
}

/// A durable, application-wide SQLite store.
#[derive(Debug)]
pub struct CoreStore {
    connection: Connection,
    database_path: PathBuf,
}

impl CoreStore {
    pub fn autostart_enabled(&self) -> Result<bool, PersistenceError> {
        self.connection
            .query_row(
                "SELECT enabled FROM app_settings WHERE name = 'autostart'",
                [],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .map(|value| value.unwrap_or(false))
            .map_err(|source| PersistenceError::Sqlite { source })
    }
    pub fn set_autostart_enabled(&self, enabled: bool) -> Result<(), PersistenceError> {
        self.connection.execute("INSERT INTO app_settings(name, enabled) VALUES ('autostart', ?1) ON CONFLICT(name) DO UPDATE SET enabled=excluded.enabled", params![enabled])
            .map(|_| ()).map_err(|source| PersistenceError::Sqlite { source })
    }
    /// Opens the durable core database below the caller-provided application data root.
    pub fn open(
        app_data_root: impl AsRef<Path>,
        profile: DurabilityProfile,
    ) -> Result<Self, PersistenceError> {
        if profile == DurabilityProfile::Rebuildable {
            return Err(PersistenceError::InvalidDurabilityProfile { profile });
        }

        let database_directory = app_data_root.as_ref().join(DATABASE_DIRECTORY);
        fs::create_dir_all(&database_directory)
            .map_err(|source| PersistenceError::Io { source })?;
        let database_path = database_directory.join(DATABASE_FILE);
        let mut connection = Connection::open(&database_path)
            .map_err(|source| PersistenceError::Sqlite { source })?;

        configure_connection(&connection, profile)?;
        run_migrations(&mut connection, MIGRATIONS)?;

        Ok(Self {
            connection,
            database_path,
        })
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn set_feature_enabled(
        &self,
        feature_id: &str,
        enabled: bool,
    ) -> Result<(), PersistenceError> {
        self.connection
            .execute(
                "INSERT INTO feature_state (feature_id, enabled) VALUES (?1, ?2) \
                 ON CONFLICT(feature_id) DO UPDATE SET enabled = excluded.enabled",
                params![feature_id, i64::from(enabled)],
            )
            .map(|_| ())
            .map_err(|source| PersistenceError::Sqlite { source })
    }

    pub fn feature_enabled(&self, feature_id: &str) -> Result<Option<bool>, PersistenceError> {
        self.connection
            .query_row(
                "SELECT enabled FROM feature_state WHERE feature_id = ?1",
                params![feature_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map(|value| value.map(|enabled| enabled != 0))
            .map_err(|source| PersistenceError::Sqlite { source })
    }

    pub fn schema_version(&self) -> Result<i64, PersistenceError> {
        database_schema_version(&self.connection)
    }
}

fn configure_connection(
    connection: &Connection,
    profile: DurabilityProfile,
) -> Result<(), PersistenceError> {
    let synchronous = match profile {
        DurabilityProfile::Critical => "FULL",
        DurabilityProfile::Standard => "NORMAL",
        DurabilityProfile::Rebuildable => {
            return Err(PersistenceError::InvalidDurabilityProfile { profile });
        }
    };

    connection
        .execute_batch(&format!(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = {synchronous}; PRAGMA foreign_keys = ON;"
        ))
        .map_err(|source| PersistenceError::Sqlite { source })?;
    connection
        .busy_timeout(BUSY_TIMEOUT)
        .map_err(|source| PersistenceError::Sqlite { source })
}

pub struct Migration {
    version: i64,
    name: &'static str,
    apply: fn(&Transaction<'_>) -> rusqlite::Result<()>,
}

impl Migration {
    pub const fn new(
        version: i64,
        name: &'static str,
        apply: fn(&Transaction<'_>) -> rusqlite::Result<()>,
    ) -> Self {
        Self {
            version,
            name,
            apply,
        }
    }
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "create_feature_state",
        apply: create_feature_state,
    },
    Migration::new(2, "create_app_settings", create_app_settings),
];

fn create_app_settings(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch("CREATE TABLE app_settings(name TEXT PRIMARY KEY NOT NULL, enabled INTEGER NOT NULL CHECK(enabled IN (0,1)));")
}

fn create_feature_state(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE feature_state (
            feature_id TEXT PRIMARY KEY,
            enabled INTEGER NOT NULL CHECK(enabled IN (0, 1))
        );",
    )
}

pub fn run_migrations(
    connection: &mut Connection,
    migrations: &[Migration],
) -> Result<(), PersistenceError> {
    validate_migration_plan(migrations)
        .map_err(|source| PersistenceError::InvalidMigrationPlan { source })?;
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );",
        )
        .map_err(|source| PersistenceError::Sqlite { source })?;

    let current_version = database_schema_version(connection)?;
    let latest_supported_version = migrations.last().map_or(0, |migration| migration.version);
    if current_version > latest_supported_version {
        return Err(PersistenceError::FutureSchema {
            current_version,
            latest_supported_version,
        });
    }

    for migration in migrations {
        let applied = connection
            .query_row(
                "SELECT name FROM schema_migrations WHERE version = ?1",
                params![migration.version],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|source| PersistenceError::Sqlite { source })?;

        if let Some(applied_name) = applied {
            if applied_name != migration.name {
                return Err(PersistenceError::InconsistentSchema {
                    detail: format!(
                        "migration {} is recorded as `{applied_name}`",
                        migration.version
                    ),
                });
            }
            continue;
        }
        if migration.version <= current_version {
            return Err(PersistenceError::InconsistentSchema {
                detail: format!(
                    "migration {} is missing below recorded version {current_version}",
                    migration.version
                ),
            });
        }

        let transaction =
            connection
                .transaction()
                .map_err(|source| PersistenceError::Migration {
                    version: migration.version,
                    name: migration.name,
                    source,
                })?;
        if let Err(source) = (migration.apply)(&transaction) {
            return Err(PersistenceError::Migration {
                version: migration.version,
                name: migration.name,
                source,
            });
        }
        if let Err(source) = transaction.execute(
            "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
            params![migration.version, migration.name],
        ) {
            return Err(PersistenceError::Migration {
                version: migration.version,
                name: migration.name,
                source,
            });
        }
        transaction
            .commit()
            .map_err(|source| PersistenceError::Migration {
                version: migration.version,
                name: migration.name,
                source,
            })?;
    }

    Ok(())
}

fn validate_migration_plan(migrations: &[Migration]) -> Result<(), MigrationPlanError> {
    let mut previous_version = None;
    for migration in migrations {
        if migration.version <= 0 {
            return Err(MigrationPlanError::NonPositiveVersion {
                version: migration.version,
            });
        }
        if migration.name.is_empty() {
            return Err(MigrationPlanError::EmptyName {
                version: migration.version,
            });
        }
        if let Some(previous_version) = previous_version
            && migration.version <= previous_version
        {
            return Err(MigrationPlanError::VersionsNotStrictlyIncreasing {
                previous_version,
                version: migration.version,
            });
        }
        previous_version = Some(migration.version);
    }
    Ok(())
}

pub fn database_schema_version(connection: &Connection) -> Result<i64, PersistenceError> {
    connection
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(|source| PersistenceError::Sqlite { source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fresh_open_creates_expected_schema_and_version() {
        let directory = tempdir().unwrap();
        let store = CoreStore::open(directory.path(), DurabilityProfile::Critical).unwrap();

        assert_eq!(store.schema_version().unwrap(), 2);
        assert!(store.database_path().ends_with("data/core.db"));
        let table: String = store
            .connection
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'feature_state'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table, "feature_state");
    }

    #[test]
    fn reopening_is_idempotent() {
        let directory = tempdir().unwrap();
        CoreStore::open(directory.path(), DurabilityProfile::Standard).unwrap();
        let reopened = CoreStore::open(directory.path(), DurabilityProfile::Standard).unwrap();

        assert_eq!(reopened.schema_version().unwrap(), 2);
        let count: i64 = reopened
            .connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn feature_enabled_state_roundtrips_and_updates() {
        let directory = tempdir().unwrap();
        let store = CoreStore::open(directory.path(), DurabilityProfile::Standard).unwrap();

        assert_eq!(store.feature_enabled("qbit.example").unwrap(), None);
        store.set_feature_enabled("qbit.example", true).unwrap();
        assert_eq!(store.feature_enabled("qbit.example").unwrap(), Some(true));
        store.set_feature_enabled("qbit.example", false).unwrap();
        assert_eq!(store.feature_enabled("qbit.example").unwrap(), Some(false));
    }

    #[test]
    fn rebuildable_is_rejected_before_creating_a_database() {
        let directory = tempdir().unwrap();
        assert!(matches!(
            CoreStore::open(directory.path(), DurabilityProfile::Rebuildable),
            Err(PersistenceError::InvalidDurabilityProfile { .. })
        ));
        assert!(
            !directory
                .path()
                .join(DATABASE_DIRECTORY)
                .join(DATABASE_FILE)
                .exists()
        );
    }

    #[test]
    fn failed_migration_rolls_back_schema_and_metadata() {
        fn failing_migration(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
            transaction.execute_batch("CREATE TABLE should_rollback (value TEXT); THIS IS NOT SQL;")
        }

        let directory = tempdir().unwrap();
        let database_path = directory.path().join("migration-test.db");
        let mut connection = Connection::open(&database_path).unwrap();
        let migrations = [Migration {
            version: 1,
            name: "fails",
            apply: failing_migration,
        }];

        assert!(matches!(
            run_migrations(&mut connection, &migrations),
            Err(PersistenceError::Migration { version: 1, .. })
        ));
        let table = connection
            .query_row(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'should_rollback'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .unwrap();
        assert_eq!(table, None);
        assert_eq!(database_schema_version(&connection).unwrap(), 0);
    }

    #[test]
    fn invalid_migration_plans_do_not_create_metadata_or_apply_sql() {
        fn creates_table(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
            transaction.execute_batch("CREATE TABLE should_not_exist (value TEXT);")
        }

        let cases = [
            (
                vec![Migration::new(0, "invalid", creates_table)],
                MigrationPlanError::NonPositiveVersion { version: 0 },
            ),
            (
                vec![
                    Migration::new(2, "first", creates_table),
                    Migration::new(2, "duplicate", creates_table),
                ],
                MigrationPlanError::VersionsNotStrictlyIncreasing {
                    previous_version: 2,
                    version: 2,
                },
            ),
            (
                vec![
                    Migration::new(2, "first", creates_table),
                    Migration::new(1, "descending", creates_table),
                ],
                MigrationPlanError::VersionsNotStrictlyIncreasing {
                    previous_version: 2,
                    version: 1,
                },
            ),
            (
                vec![Migration::new(1, "", creates_table)],
                MigrationPlanError::EmptyName { version: 1 },
            ),
        ];

        for (migrations, expected_error) in cases {
            let mut connection = Connection::open_in_memory().unwrap();
            let error = run_migrations(&mut connection, &migrations).unwrap_err();
            assert!(matches!(
                &error,
                PersistenceError::InvalidMigrationPlan { source } if source == &expected_error
            ));
            assert!(error.source().is_some());
            let table_count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('schema_migrations', 'should_not_exist')",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(table_count, 0);
        }
    }

    #[test]
    fn invalid_feature_id_does_not_create_a_feature_directory_or_database() {
        let directory = tempdir().unwrap();
        let result =
            open_feature_database(directory.path(), "invalid", DurabilityProfile::Critical);

        assert!(matches!(
            &result,
            Err(PersistenceError::InvalidFeatureId {
                source: FeatureIdError::MissingPrefix
            })
        ));
        assert!(result.unwrap_err().source().is_some());
        assert!(!directory.path().join("data/features").exists());
    }

    #[test]
    fn empty_migration_plan_is_allowed() {
        let mut connection = Connection::open_in_memory().unwrap();
        run_migrations(&mut connection, &[]).unwrap();
        assert_eq!(database_schema_version(&connection).unwrap(), 0);
    }
}
