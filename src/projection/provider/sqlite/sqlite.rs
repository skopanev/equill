use super::{queries, row::ProjectedRow, schema};
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::projection::ProjectionState;
use crate::record::StoredRecord;
use rusqlite::{Connection, OptionalExtension};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

const DATABASE: &str = "projections/sqlite/equill.sqlite3";
const DEGRADED: &str = "projections/sqlite/DEGRADED.json";

pub fn initialize(store_root: &Path) -> Result<(), Error> {
    let path = database(store_root);
    fs::create_dir_all(parent(&path)?)?;
    let connection = open(&path)?;
    create_schema(&connection)?;
    clear_degraded(store_root)
}

pub fn state(store_root: &Path) -> Result<ProjectionState, Error> {
    if store_root.join(DEGRADED).is_file() {
        Ok(ProjectionState::Degraded)
    } else if database(store_root).is_file() {
        Ok(ProjectionState::Ready)
    } else {
        Ok(ProjectionState::Missing)
    }
}

pub fn verify(store_root: &Path, truth: &[StoredRecord]) -> Result<usize, Error> {
    match state(store_root)? {
        ProjectionState::Missing => {
            return Err(Error::Projection("sqlite projection is missing".into()));
        }
        ProjectionState::Degraded => {
            return Err(Error::Projection(
                "sqlite projection is marked degraded".into(),
            ));
        }
        ProjectionState::Ready => {}
    }
    let connection = open(&database(store_root))?;
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .map_err(|error| projection_error("integrity check", error))?;
    if integrity != "ok" {
        return Err(Error::Projection(format!(
            "sqlite integrity check returned {integrity}"
        )));
    }
    verify_schema(&connection)?;
    let records = count(&connection, "records")?;
    let indexed = count(&connection, "records_fts")?;
    if records != indexed {
        return Err(Error::Projection(format!(
            "sqlite record count {records} differs from FTS count {indexed}"
        )));
    }
    if records != truth.len() {
        return Err(Error::Projection(format!(
            "sqlite record count {records} differs from log count {}",
            truth.len()
        )));
    }
    for record in truth {
        verify_record(&connection, record)?;
    }
    Ok(records)
}

fn verify_record(connection: &Connection, truth: &StoredRecord) -> Result<(), Error> {
    let projected = connection
        .query_row(
            queries::RECORD_BY_ID,
            [truth.id.to_string()],
            ProjectedRow::read,
        )
        .optional()
        .map_err(|error| projection_error("read projected record", error))?
        .ok_or_else(|| Error::Projection(format!("record {} is not indexed", truth.id)))?
        .record()?;
    let expected = sha256_hex(&serde_json::to_vec(truth)?);
    let actual = sha256_hex(&serde_json::to_vec(&projected)?);
    if actual != expected {
        return Err(Error::Projection(format!(
            "record {} differs from immutable log",
            truth.id
        )));
    }
    let indexed_content: String = connection
        .query_row(
            "SELECT content FROM records_fts WHERE id = ?1",
            [truth.id.to_string()],
            |row| row.get(0),
        )
        .map_err(|error| projection_error("read FTS content", error))?;
    if indexed_content != content(truth)? {
        return Err(Error::Projection(format!(
            "FTS content for record {} is stale",
            truth.id
        )));
    }
    Ok(())
}

pub fn mark_degraded(store_root: &Path, record_id: Uuid, reason: &str) -> Result<(), Error> {
    let path = store_root.join(DEGRADED);
    fs::create_dir_all(parent(&path)?)?;
    let body = serde_json::json!({
        "record_id": record_id,
        "reason": reason,
    });
    fs::write(path, serde_json::to_vec(&body)?)?;
    Ok(())
}

pub(super) fn open(path: &Path) -> Result<Connection, Error> {
    let connection = Connection::open(path).map_err(|error| projection_error("open", error))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| projection_error("set busy timeout", error))?;
    Ok(connection)
}

pub(super) fn create_schema(connection: &Connection) -> Result<(), Error> {
    connection
        .execute_batch(schema::CREATE)
        .map_err(|error| projection_error("initialize schema", error))?;
    verify_schema(connection)
}

fn verify_schema(connection: &Connection) -> Result<(), Error> {
    let version: Option<String> = connection
        .query_row(
            "SELECT value FROM equill_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| projection_error("read schema version", error))?;
    if version.as_deref() != Some(schema::VERSION) {
        return Err(Error::Projection(
            "unsupported sqlite schema version".into(),
        ));
    }
    Ok(())
}

pub(super) fn count(connection: &Connection, table: &str) -> Result<usize, Error> {
    let sql = format!("SELECT count(*) FROM {table}");
    connection
        .query_row(&sql, [], |row| row.get::<_, i64>(0))
        .map(|value| value as usize)
        .map_err(|error| projection_error("count rows", error))
}

pub(super) fn content(record: &StoredRecord) -> Result<String, Error> {
    Ok(format!(
        "{} {} {}",
        serde_json::to_string(&record.payload)?,
        serde_json::to_string(&record.evidence)?,
        serde_json::to_string(&record.tags)?
    ))
}

pub(super) fn clear_degraded(store_root: &Path) -> Result<(), Error> {
    let marker = store_root.join(DEGRADED);
    if marker.exists() {
        fs::remove_file(marker)?;
    }
    Ok(())
}

pub(super) fn database(store_root: &Path) -> PathBuf {
    store_root.join(DATABASE)
}

pub(super) fn parent(path: &Path) -> Result<&Path, Error> {
    path.parent()
        .ok_or_else(|| Error::Projection("sqlite path has no parent".into()))
}

pub(super) fn projection_error(action: &str, error: rusqlite::Error) -> Error {
    Error::Projection(format!("sqlite {action}: {error}"))
}
