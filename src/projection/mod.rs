mod model;
mod provider;

use crate::kernel::error::Error;
use crate::kernel::lock::StoreLock;
use crate::kernel::store;
use crate::record::StoredRecord;
use std::path::Path;

pub use model::{ProjectionState, RebuildReport, SearchHit, SearchReport, SearchRequest};
pub use provider::sqlite::MAX_SCAN;

pub fn initialize(store_root: &Path) -> Result<(), Error> {
    provider::sqlite::initialize(store_root)
}

pub fn state(store_root: &Path) -> Result<ProjectionState, Error> {
    provider::sqlite::state(store_root)
}

pub fn index(
    store_root: &Path,
    record: &StoredRecord,
    sha256: &str,
    ledger: &str,
) -> Result<(), Error> {
    provider::sqlite::index(store_root, record, sha256, ledger)
}

pub fn mark_degraded(store_root: &Path, record: &StoredRecord, reason: &str) {
    let _ = provider::sqlite::mark_degraded(store_root, record.id, reason);
}

pub fn search(store_root: &Path, request: &SearchRequest) -> Result<SearchReport, Error> {
    store::load(store_root)?;
    provider::sqlite::search(store_root, request)
}

pub fn verify(store_root: &Path, records: &[StoredRecord]) -> Result<usize, Error> {
    provider::sqlite::verify(store_root, records)
}

pub fn rebuild(store_root: &Path) -> Result<RebuildReport, Error> {
    store::load(store_root)?;
    let _lock = StoreLock::exclusive(store_root)?;
    let records = crate::record::read_all(store_root)?;
    provider::sqlite::rebuild(store_root, &records)?;
    Ok(RebuildReport {
        ok: true,
        projection: "sqlite-fts",
        records: records.len(),
    })
}
