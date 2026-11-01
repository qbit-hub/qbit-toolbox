//! Feature-owned SQLite persistence for Hotkeys configuration.
//!
//! This synchronous repository is configuration-plane work only. Callers must keep it off
//! latency-sensitive keyboard callbacks; future IPC wiring provides that execution boundary.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use persistence::{
    DurabilityProfile, Migration, PersistenceError, open_feature_database, run_migrations,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::Error as JsonError;
use uuid::Uuid;
#[cfg(all(test, windows))]
mod service_tests;

use crate::domain::Mapping;
use crate::feature::{FEATURE_ID, FEATURE_SCHEMA_VERSION};

pub const SCHEMA_VERSION: i64 = FEATURE_SCHEMA_VERSION as i64;
pub const PAYLOAD_FORMAT_VERSION: i64 = 1;

#[derive(Debug)]
pub enum RepositoryError {
    Shared(PersistenceError),
    Sqlite(rusqlite::Error),
    Serialization(JsonError),
    DuplicateMapping {
        id: Uuid,
    },
    NotFound {
        id: Uuid,
    },
    UnsupportedPayloadVersion {
        id: Uuid,
        version: i64,
    },
    InvalidStoredMappingId {
        stored_id: String,
        source: uuid::Error,
    },
    InvalidPayload {
        id: Uuid,
        detail: String,
    },
    OrderingOverflow,
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shared(error) => write!(formatter, "Hotkeys database error: {error}"),
            Self::Sqlite(error) => write!(formatter, "Hotkeys SQLite error: {error}"),
            Self::Serialization(error) => {
                write!(formatter, "Hotkeys payload serialization error: {error}")
            }
            Self::DuplicateMapping { id } => write!(formatter, "mapping {id} already exists"),
            Self::NotFound { id } => write!(formatter, "mapping {id} does not exist"),
            Self::UnsupportedPayloadVersion { id, version } => {
                write!(
                    formatter,
                    "mapping {id} has unsupported payload version {version}"
                )
            }
            Self::InvalidStoredMappingId { stored_id, source } => {
                write!(
                    formatter,
                    "invalid stored mapping id `{stored_id}`: {source}"
                )
            }
            Self::InvalidPayload { id, detail } => {
                write!(formatter, "invalid persisted mapping {id}: {detail}")
            }
            Self::OrderingOverflow => {
                write!(formatter, "mapping ordering has reached its maximum value")
            }
        }
    }
}

impl Error for RepositoryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Shared(error) => Some(error),
            Self::Sqlite(error) => Some(error),
            Self::Serialization(error) => Some(error),
            Self::InvalidStoredMappingId { source, .. } => Some(source),
            Self::DuplicateMapping { .. }
            | Self::NotFound { .. }
            | Self::UnsupportedPayloadVersion { .. }
            | Self::InvalidPayload { .. }
            | Self::OrderingOverflow => None,
        }
    }
}

impl From<PersistenceError> for RepositoryError {
    fn from(error: PersistenceError) -> Self {
        Self::Shared(error)
    }
}

const MIGRATIONS: &[Migration] = &[Migration::new(
    SCHEMA_VERSION,
    "create_mappings",
    create_mappings,
)];

fn create_mappings(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    transaction.execute_batch(
        "CREATE TABLE mappings (
            id TEXT PRIMARY KEY NOT NULL,
            sort_order INTEGER NOT NULL UNIQUE CHECK(sort_order >= 0),
            enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
            payload_version INTEGER NOT NULL,
            payload TEXT NOT NULL
        );",
    )
}

/// Owns the Hotkeys schema and its single SQLite connection.
#[derive(Debug)]
pub struct HotkeysRepository {
    connection: Connection,
    database_path: PathBuf,
}

impl HotkeysRepository {
    /// Lazily opens, configures, and forward-migrates the feature-owned database.
    pub fn open(app_data_root: impl AsRef<Path>) -> Result<Self, RepositoryError> {
        let database =
            open_feature_database(app_data_root, FEATURE_ID, DurabilityProfile::Critical)?;
        let database_path = database.database_path().to_owned();
        let mut connection = database.into_connection();
        run_migrations(&mut connection, MIGRATIONS)?;
        Ok(Self {
            connection,
            database_path,
        })
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    pub fn schema_version(&self) -> Result<i64, RepositoryError> {
        persistence::database_schema_version(&self.connection).map_err(Into::into)
    }

    pub fn list_mappings(&self) -> Result<Vec<Mapping>, RepositoryError> {
        let mut statement = self.connection.prepare(
            "SELECT id, enabled, payload_version, payload FROM mappings ORDER BY sort_order ASC, id ASC",
        ).map_err(RepositoryError::Sqlite)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(RepositoryError::Sqlite)?;
        rows.map(|row| row.map_err(RepositoryError::Sqlite).and_then(Self::hydrate))
            .collect()
    }

    pub fn get_mapping(&self, id: Uuid) -> Result<Option<Mapping>, RepositoryError> {
        self.connection
            .query_row(
                "SELECT id, enabled, payload_version, payload FROM mappings WHERE id = ?1",
                params![id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(RepositoryError::Sqlite)?
            .map(Self::hydrate)
            .transpose()
    }

    pub fn insert_mapping(&mut self, mapping: &Mapping) -> Result<(), RepositoryError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(RepositoryError::Sqlite)?;
        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM mappings WHERE id = ?1)",
                params![mapping.id.to_string()],
                |row| row.get(0),
            )
            .map_err(RepositoryError::Sqlite)?;
        if exists {
            return Err(RepositoryError::DuplicateMapping { id: mapping.id });
        }
        let max_order: Option<i64> = transaction
            .query_row("SELECT MAX(sort_order) FROM mappings", [], |row| row.get(0))
            .map_err(RepositoryError::Sqlite)?;
        let next_order = match max_order {
            Some(order) => order
                .checked_add(1)
                .ok_or(RepositoryError::OrderingOverflow)?,
            None => 0,
        };
        let payload = serde_json::to_string(mapping).map_err(RepositoryError::Serialization)?;
        transaction
            .execute(
            "INSERT INTO mappings (id, sort_order, enabled, payload_version, payload) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![mapping.id.to_string(), next_order, i64::from(mapping.enabled), PAYLOAD_FORMAT_VERSION, payload],
        )
            .map_err(RepositoryError::Sqlite)?;
        transaction.commit().map_err(RepositoryError::Sqlite)
    }

    pub fn replace_mapping(&mut self, mapping: &Mapping) -> Result<(), RepositoryError> {
        let payload = serde_json::to_string(mapping).map_err(RepositoryError::Serialization)?;
        let updated = self.connection.execute(
            "UPDATE mappings SET enabled = ?2, payload_version = ?3, payload = ?4 WHERE id = ?1",
            params![mapping.id.to_string(), i64::from(mapping.enabled), PAYLOAD_FORMAT_VERSION, payload],
        ).map_err(RepositoryError::Sqlite)?;
        if updated == 0 {
            Err(RepositoryError::NotFound { id: mapping.id })
        } else {
            Ok(())
        }
    }

    pub fn delete_mapping(&mut self, id: Uuid) -> Result<bool, RepositoryError> {
        self.connection
            .execute(
                "DELETE FROM mappings WHERE id = ?1",
                params![id.to_string()],
            )
            .map(|affected| affected != 0)
            .map_err(RepositoryError::Sqlite)
    }

    pub fn set_mapping_enabled(&mut self, id: Uuid, enabled: bool) -> Result<(), RepositoryError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(RepositoryError::Sqlite)?;
        let record: Option<(String, i64, i64, String)> = transaction
            .query_row(
                "SELECT id, enabled, payload_version, payload FROM mappings WHERE id = ?1",
                params![id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(RepositoryError::Sqlite)?;
        let Some(record) = record else {
            return Err(RepositoryError::NotFound { id });
        };
        let mut mapping = Self::hydrate(record)?;
        mapping.enabled = enabled;
        let payload = serde_json::to_string(&mapping).map_err(RepositoryError::Serialization)?;
        transaction
            .execute(
                "UPDATE mappings SET enabled = ?2, payload_version = ?3, payload = ?4 WHERE id = ?1",
                params![id.to_string(), i64::from(enabled), PAYLOAD_FORMAT_VERSION, payload],
            )
            .map_err(RepositoryError::Sqlite)?;
        transaction.commit().map_err(RepositoryError::Sqlite)
    }

    fn hydrate(record: (String, i64, i64, String)) -> Result<Mapping, RepositoryError> {
        let (stored_id, enabled, payload_version, payload) = record;
        let id = Uuid::parse_str(&stored_id).map_err(|source| {
            RepositoryError::InvalidStoredMappingId {
                stored_id: stored_id.clone(),
                source,
            }
        })?;
        if payload_version != PAYLOAD_FORMAT_VERSION {
            return Err(RepositoryError::UnsupportedPayloadVersion {
                id,
                version: payload_version,
            });
        }
        if !(0..=1).contains(&enabled) {
            return Err(RepositoryError::InvalidPayload {
                id,
                detail: "invalid enabled state".to_owned(),
            });
        }
        let mapping: Mapping =
            serde_json::from_str(&payload).map_err(|error| RepositoryError::InvalidPayload {
                id,
                detail: error.to_string(),
            })?;
        if mapping.id != id || mapping.enabled != (enabled != 0) {
            return Err(RepositoryError::InvalidPayload {
                id,
                detail: "row metadata does not match payload".to_owned(),
            });
        }
        Ok(mapping)
    }
}
