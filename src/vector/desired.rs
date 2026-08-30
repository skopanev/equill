use super::model::vector_error;
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

const DESIRED: &str = "projections/qdrant/desired.json";
const SCHEMA: &str = "equill.qdrant-desired.v1";

/// What the ledger currently asks the index to cover. The writer moves this
/// after a durable commit; the drain chases it. Two numbers, no payload.
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Desired {
    schema: String,
    pub records: usize,
    pub corpus_sha256: String,
}

/// Records what the ledger holds now. Called by a writer that has already
/// committed, so a failure here can never unmake a record: the caller reports
/// it and the next write or an explicit sync picks the tail up.
pub fn publish(store: &Path, records: usize, corpus_sha256: &str) -> Result<(), Error> {
    let path = store.join(DESIRED);
    let directory = path
        .parent()
        .ok_or_else(|| vector_error("desired marker directory is invalid"))?;
    fs::create_dir_all(directory).map_err(|_| vector_error("desired marker staging failed"))?;
    let temporary = directory.join(format!(".desired-{}.json", Uuid::now_v7()));
    let marker = Desired {
        schema: SCHEMA.into(),
        records,
        corpus_sha256: corpus_sha256.to_owned(),
    };
    let bytes =
        serde_json::to_vec(&marker).map_err(|_| vector_error("desired marker serialization"))?;
    fs::write(&temporary, bytes).map_err(|_| vector_error("desired marker staging failed"))?;
    fs::rename(&temporary, &path).map_err(|_| {
        let _ = fs::remove_file(&temporary);
        vector_error("desired marker commit failed")
    })
}

/// Absent means nothing has published a target yet, which is not an error: a
/// store that predates this file simply has no recorded intent.
pub fn read(store: &Path) -> Result<Option<Desired>, Error> {
    let path = store.join(DESIRED);
    if !path.is_file() {
        return Ok(None);
    }
    let marker: Desired = serde_json::from_slice(&fs::read(path)?)?;
    if marker.schema != SCHEMA {
        return Err(vector_error("unsupported desired marker schema"));
    }
    Ok(Some(marker))
}
