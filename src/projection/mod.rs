#[cfg(test)]
mod catchup_tests;
mod marker;
mod model;
mod provider;

pub use marker::{ProjectionTarget, TextWatermark, publish_target, target, watermark};

use crate::kernel::error::Error;
use crate::kernel::lock::StoreLock;
use crate::kernel::store;
use crate::record::StoredRecord;
use std::path::Path;

pub use model::{
    HistoricRecords, HistoryCount, LifecycleScope, ProjectionState, RebuildReport, SearchHit,
    SearchReport, SearchRequest,
};
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
    #[cfg(test)]
    crate::record::hotpath::projection_write();
    provider::sqlite::index(store_root, record, sha256, ledger)
}

pub fn mark_degraded(store_root: &Path, record: &StoredRecord, reason: &str) {
    let _ = provider::sqlite::mark_degraded(store_root, record.id, reason);
}

pub fn search(store_root: &Path, request: &SearchRequest) -> Result<SearchReport, Error> {
    store::load(store_root)?;
    provider::sqlite::search(store_root, request)
}

/// Which of these records the projection knows to be history. Answered from
/// indexed lifecycle rather than from the ledger.
pub fn historic(store_root: &Path, ids: &[uuid::Uuid]) -> Result<HistoricRecords, Error> {
    provider::sqlite::historic(store_root, ids)
}

/// How much of a scope is history. What a semantic page has to be prepared to
/// skip, counted without reading a record.
pub fn history_in_scope(store_root: &Path, scope: &LifecycleScope) -> Result<HistoryCount, Error> {
    provider::sqlite::history_in_scope(store_root, scope)
}

pub fn verify(store_root: &Path, records: &[StoredRecord]) -> Result<usize, Error> {
    provider::sqlite::verify(store_root, records)
}

/// Bring the text index level with the ledger, without being asked which
/// records are missing.
///
/// Called after confirmation, not inside it. Indexing is an upsert, so covering
/// records the index already holds costs a little and risks nothing — which is
/// why the watermark below is allowed to be an optimisation rather than a
/// correctness boundary. A burst of writes coalesces into one pass, the same
/// way the vector catch-up does.
///
/// No writer lock. An earlier version took `StoreLock::exclusive` to snapshot
/// the ledger, which made every concurrent write wait for a full scan and an
/// index pass — the exact contention that was removed from the vector path for
/// the exact same reason. Reading without it is sound because the ledger is
/// append-only and `read_all` stops at the last completed line, so the worst a
/// concurrent append can do is be missed and picked up by the next pass.
pub fn catch_up_text(store_root: &Path) -> Result<usize, Error> {
    let reached = marker::ledger_bytes(store_root)?;
    let (at, covered) = watermark(store_root)
        .map(|mark| (mark.ledger_bytes, mark.indexed_records))
        .unwrap_or((0, 0));
    if at == reached {
        // Nothing has been appended since the last pass. A wake that finds the
        // ledger where it left it should cost a stat, not a scan.
        return Ok(0);
    }
    let records = crate::record::read_all(store_root)?;
    // Only what the last pass did not cover. Re-indexing the whole store on
    // every wake is harmless in the sense that upserts are idempotent, and
    // ruinous in the sense that a few hundred fsyncs land on the same disk a
    // concurrent write is trying to commit to. The ledger is append-only and
    // read in order, so a count is a valid cursor into it.
    let fresh = records
        .get(covered.min(records.len())..)
        .unwrap_or_default();
    let mut indexed = 0;
    let mut complete = true;
    for record in fresh {
        let digest = crate::kernel::digest::sha256_hex(&serde_json::to_vec(record)?);
        let ledger = format!("records/{}.jsonl", &record.recorded_at[..7]);
        match provider::sqlite::index(store_root, record, &digest, &ledger) {
            Ok(()) => indexed += 1,
            Err(error) => {
                provider::sqlite::mark_degraded(store_root, record.id, &error.to_string())?;
                complete = false;
            }
        }
    }
    if !complete {
        // A record failed to index. The cursor is a count, so moving it would
        // step over that record and no later pass would ever come back for it —
        // only a full rebuild could. Leaving the watermark where it was costs a
        // repeat of the records that did succeed, which is an upsert and cheap,
        // and it means the failed one is retried on the next wake. It also
        // keeps the published position honest: this pass did not reach the end
        // of the ledger, so it does not say it did, and freshness reads as
        // behind rather than current.
        return Ok(indexed);
    }
    // Every record the ledger holds is in the index. Whatever put this store
    // into a degraded state has been indexed since — degradation is recorded
    // per record and cleared by covering them, so leaving the marker up would
    // report a store as broken for a failure it has already recovered from.
    provider::sqlite::clear_degraded(store_root)?;
    // After the pass, never before: a watermark written first would let a
    // failed pass claim coverage it does not have.
    marker::record_watermark(store_root, reached, records.len());
    Ok(indexed)
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
