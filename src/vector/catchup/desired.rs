use super::super::model::vector_error;
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

const DESIRED: &str = "projections/qdrant/desired.json";
const SCHEMA: &str = "equill.qdrant-desired.v2";
const SCHEMA_V1: &str = "equill.qdrant-desired.v1";

/// What the ledger has asked the index to cover, as a number that only goes up.
///
/// Deliberately NOT a corpus digest. Computing one means reading and hashing the
/// whole ledger, and a write cannot pay that: it is linear in store size on the
/// path of every append. A revision is a counter — bumping it is O(1) — and the
/// worker does the enumeration and hashing on its own time.
#[derive(Debug, Deserialize, Serialize)]
pub struct Desired {
    schema: String,
    pub revision: u64,
    /// Carried from the v1 format so an upgraded store is not forced through a
    /// full scan on its first write. Never written by this build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records: Option<usize>,
}

/// Move the target forward by `appended` records. Called after a durable commit,
/// under the writer lock, so two writers cannot both read the same revision.
///
/// A failure here can never unmake a record: the caller reports it and the next
/// write or an explicit sync picks the tail up.
pub fn advance(store: &Path, appended: u64) -> Result<u64, Error> {
    let current = read(store)?.map_or(0, |desired| desired.revision);
    let revision = current.saturating_add(appended.max(1));
    publish(store, revision)?;
    Ok(revision)
}

pub fn publish(store: &Path, revision: u64) -> Result<(), Error> {
    // A target only ever moves forward. Callers publish under the writer lock,
    // so this should never trigger — but a watermark that can go backwards turns
    // a drain into a loop that never agrees with itself.
    if let Some(current) = read(store)?
        && current.revision > revision
    {
        return Ok(());
    }
    let path = store.join(DESIRED);
    let directory = path
        .parent()
        .ok_or_else(|| vector_error("desired marker directory is invalid"))?;
    fs::create_dir_all(directory).map_err(|_| vector_error("desired marker staging failed"))?;
    let temporary = directory.join(format!(".desired-{}.json", Uuid::now_v7()));
    let marker = Desired {
        schema: SCHEMA.into(),
        revision,
        records: None,
    };
    let bytes =
        serde_json::to_vec(&marker).map_err(|_| vector_error("desired marker serialization"))?;
    // The target is the only durable statement of what the index still owes the
    // ledger. A plain write leaves it in the page cache, so a crash could lose
    // the record of what was outstanding while the ledger entry that caused it
    // survived.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| vector_error("desired marker staging failed"))?;
    if file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(vector_error("desired marker staging failed"));
    }
    drop(file);
    if fs::rename(&temporary, &path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(vector_error("desired marker commit failed"));
    }
    File::open(directory)
        .and_then(|handle| handle.sync_all())
        .map_err(|_| vector_error("desired marker directory sync failed"))
}

/// Absent means nothing has published a target yet, which is not an error.
///
/// A v1 marker is read, not rejected: its record count becomes the revision, so
/// an upgraded store carries on from where it was without a scan.
pub fn read(store: &Path) -> Result<Option<Desired>, Error> {
    let path = store.join(DESIRED);
    if !path.is_file() {
        return Ok(None);
    }
    let raw: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
    let schema = raw.get("schema").and_then(|value| value.as_str());
    match schema {
        Some(SCHEMA) => Ok(Some(serde_json::from_value(raw)?)),
        Some(SCHEMA_V1) => {
            let records = raw
                .get("records")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            Ok(Some(Desired {
                schema: SCHEMA.into(),
                revision: records,
                records: Some(records as usize),
            }))
        }
        _ => Err(vector_error("unsupported desired marker schema")),
    }
}
