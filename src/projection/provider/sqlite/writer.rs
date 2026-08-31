use super::{queries, sqlite};
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use rusqlite::{Connection, params};
use std::fs;
use std::path::Path;

pub fn index(
    store_root: &Path,
    record: &StoredRecord,
    sha256: &str,
    ledger: &str,
) -> Result<(), Error> {
    let mut connection = sqlite::open(&sqlite::database(store_root))?;
    sqlite::create_schema(&connection)?;
    index_record(&mut connection, record, sha256, ledger)
}

pub fn rebuild(store_root: &Path, records: &[StoredRecord]) -> Result<(), Error> {
    let final_path = sqlite::database(store_root);
    let directory = sqlite::parent(&final_path)?;
    fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(".rebuild-{}.sqlite3", std::process::id()));
    if temporary.exists() {
        return Err(Error::Projection("stale sqlite rebuild file exists".into()));
    }
    {
        let mut connection = sqlite::open(&temporary)?;
        sqlite::create_schema(&connection)?;
        for record in records {
            let digest = sha256_hex(&serde_json::to_vec(record)?);
            let month = record.recorded_at.get(..7).ok_or_else(|| {
                Error::Projection("record timestamp cannot identify its ledger".into())
            })?;
            let ledger = format!("records/{month}.jsonl");
            index_record(&mut connection, record, &digest, &ledger)?;
        }
        verify_counts(&connection, records.len())?;
    }
    replace(&temporary, &final_path)?;
    sqlite::clear_degraded(store_root)
}

fn index_record(
    connection: &mut Connection,
    record: &StoredRecord,
    sha256: &str,
    ledger: &str,
) -> Result<(), Error> {
    let payload = serde_json::to_string(&record.payload)?;
    let evidence = serde_json::to_string(&record.evidence)?;
    let tags = serde_json::to_string(&record.tags)?;
    let content = sqlite::content(record)?;
    let revoked = i64::from(withdrawn(record));
    let transaction = connection
        .transaction()
        .map_err(|error| sqlite::projection_error("start index transaction", error))?;
    let inserted = transaction
        .execute(
            queries::INSERT_RECORD,
            params![
                record.id.to_string(),
                record.namespace,
                record.type_name,
                record.actor,
                record.recorded_at,
                record.observed_at,
                record.valid_at,
                payload,
                evidence,
                tags,
                record.supersedes.map(|id| id.to_string()),
                revoked,
                sha256,
                ledger,
            ],
        )
        .map_err(|error| sqlite::projection_error("index record", error))?;
    if inserted == 0 {
        verify_existing(&transaction, record, sha256)?;
    }
    mark_lifecycle(&transaction, record)?;
    transaction
        .execute(
            "DELETE FROM records_fts WHERE id = ?1",
            [record.id.to_string()],
        )
        .map_err(|error| sqlite::projection_error("refresh FTS record", error))?;
    transaction
        .execute(
            "INSERT INTO records_fts(id, content) VALUES (?1, ?2)",
            params![record.id.to_string(), content],
        )
        .map_err(|error| sqlite::projection_error("write FTS record", error))?;
    transaction
        .commit()
        .map_err(|error| sqlite::projection_error("commit index transaction", error))
}

/// Lifecycle, written where the fact appears rather than derived on every read.
///
/// A record is history when a later record replaced it or a tombstone withdrew
/// it. Both are decided by records already in the store, so this is a
/// projection of the ledger and a rebuild reproduces it exactly. Reads used to
/// answer the same question by walking the whole ledger, which made the cost of
/// a bounded page a function of how much history the store held.
fn mark_lifecycle(
    transaction: &rusqlite::Transaction<'_>,
    record: &StoredRecord,
) -> Result<(), Error> {
    if let Some(replaced) = record.supersedes {
        transaction
            .execute(queries::MARK_SUPERSEDED, [replaced.to_string()])
            .map_err(|error| sqlite::projection_error("mark superseded record", error))?;
    }
    transaction
        .execute(queries::MARK_IF_ALREADY_REPLACED, [record.id.to_string()])
        .map_err(|error| sqlite::projection_error("mark replaced record", error))?;
    Ok(())
}

/// The tags a tombstone leaves. The legacy spelling is still honoured: a store
/// written before the namespaced tag must not come back to life on a rebuild.
fn withdrawn(record: &StoredRecord) -> bool {
    record
        .tags
        .iter()
        .any(|tag| tag == crate::record::REVOKED_TAG || tag == "status:revoked")
}

fn verify_existing(
    transaction: &rusqlite::Transaction<'_>,
    record: &StoredRecord,
    sha256: &str,
) -> Result<(), Error> {
    let existing: String = transaction
        .query_row(
            "SELECT record_sha256 FROM records WHERE id = ?1",
            [record.id.to_string()],
            |row| row.get(0),
        )
        .map_err(|error| sqlite::projection_error("read indexed digest", error))?;
    if existing != sha256 {
        return Err(Error::Projection(format!(
            "record {} is indexed with a different digest",
            record.id
        )));
    }
    Ok(())
}

fn verify_counts(connection: &Connection, expected: usize) -> Result<(), Error> {
    let records = sqlite::count(connection, "records")?;
    let indexed = sqlite::count(connection, "records_fts")?;
    if records != expected || indexed != expected {
        return Err(Error::Projection(
            "rebuilt sqlite counts do not match".into(),
        ));
    }
    Ok(())
}

fn replace(temporary: &Path, final_path: &Path) -> Result<(), Error> {
    let backup = final_path.with_extension(format!("previous-{}", std::process::id()));
    if final_path.exists() {
        fs::rename(final_path, &backup)?;
    }
    if let Err(error) = fs::rename(temporary, final_path) {
        if backup.exists() {
            let _ = fs::rename(&backup, final_path);
        }
        return Err(error.into());
    }
    if backup.exists() {
        fs::remove_file(backup)?;
    }
    Ok(())
}
