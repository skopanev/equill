#[cfg(test)]
mod catchup_tests;
mod model;
mod provider;

use crate::kernel::error::Error;
use crate::kernel::lock::StoreLock;
use crate::kernel::store;
use crate::record::StoredRecord;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub use model::{ProjectionState, RebuildReport, SearchHit, SearchReport, SearchRequest};
pub use provider::sqlite::MAX_SCAN;

const WATERMARK: &str = "projections/sqlite/watermark.json";
const WATERMARK_SCHEMA: &str = "equill.sqlite-watermark.v1";
const TARGET: &str = "projections/target.json";
const TARGET_SCHEMA: &str = "equill.projection-target.v1";

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
    let reached = ledger_bytes(store_root)?;
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
    let _ = std::fs::create_dir_all(store_root.join("projections/sqlite"));
    // After the pass, never before: a watermark written first would let a
    // failed pass claim coverage it does not have.
    let _ = std::fs::write(
        marker(store_root),
        serde_json::to_vec(&TextWatermark {
            schema: WATERMARK_SCHEMA.into(),
            ledger_bytes: reached,
            indexed_records: records.len(),
        })?,
    );
    Ok(indexed)
}

/// Where the ledger stands, published by the writer.
///
/// Not derived by the reader from file sizes: a reader that stats the ledger to
/// learn the target has made freshness depend on the store's contents, and the
/// whole point of these markers is that neither side touches the ledger to
/// answer the question. The writer knows both numbers at the moment it commits,
/// so it says them.
///
/// `records` counts every immutable record the ledger holds — governance and
/// audit included. It is the same population the text index takes, which is
/// what makes it comparable to `TextWatermark::indexed_records`. The vector
/// corpus filters audit records out and therefore counts differently; its
/// target is its own.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProjectionTarget {
    #[serde(default)]
    schema: String,
    pub ledger_bytes: u64,
    pub records: usize,
}

/// Publish where the ledger now stands. Called inside confirmation, because a
/// target that lags the record just accepted would report a stale projection as
/// current. Two numbers the caller already holds and one small atomic write —
/// no scan, no stat of the ledger.
pub fn publish_target(store_root: &Path, records: usize, ledger_bytes: u64) -> Result<(), Error> {
    let bytes = serde_json::to_vec(&ProjectionTarget {
        schema: TARGET_SCHEMA.into(),
        ledger_bytes,
        records,
    })?;
    let path = store_root.join(TARGET);
    let directory = path
        .parent()
        .ok_or_else(|| Error::Integrity("projection target path is invalid".into()))?;
    std::fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(".target-{records}-{ledger_bytes}.json"));
    std::fs::write(&temporary, &bytes)?;
    // Renamed rather than written in place: a reader must see the whole target
    // or the previous one, never half of it. Not fsynced — a target lost to a
    // crash is rebuilt by the next write, and until then it reads as missing,
    // which is reported as unknown rather than as fresh.
    if std::fs::rename(&temporary, &path).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(Error::Integrity("projection target commit failed".into()));
    }
    Ok(())
}

/// Where the ledger stands, as last published. `None` for missing, unreadable
/// or foreign-schema targets alike: all three mean freshness is unknown.
pub fn target(store_root: &Path) -> Option<ProjectionTarget> {
    let bytes = std::fs::read(store_root.join(TARGET)).ok()?;
    let mark: ProjectionTarget = serde_json::from_slice(&bytes).ok()?;
    (mark.schema == TARGET_SCHEMA).then_some(mark)
}

/// How far the text index has been caught up.
///
/// Published rather than private because the read side has to report projection
/// freshness at its own boundary, and a second marker written by that side would
/// be a second answer to the same question. `indexed_records` counts EVERY
/// record the ledger holds, governance and audit included — the text index takes
/// them all, unlike the vector corpus, which filters.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TextWatermark {
    #[serde(default)]
    schema: String,
    pub ledger_bytes: u64,
    pub indexed_records: usize,
}

/// The position the text index last caught up to, if it has ever said.
///
/// `None` covers never-written, unreadable and foreign-schema markers alike:
/// all three mean this store cannot say how fresh its text index is, which a
/// caller should report as unknown rather than guess at. Reading it costs one
/// small file and never a ledger scan.
pub fn watermark(store_root: &Path) -> Option<TextWatermark> {
    let bytes = std::fs::read(marker(store_root)).ok()?;
    let mark: TextWatermark = serde_json::from_slice(&bytes).ok()?;
    (mark.schema == WATERMARK_SCHEMA).then_some(mark)
}

fn marker(store_root: &Path) -> std::path::PathBuf {
    store_root.join(WATERMARK)
}

/// Total ledger size, from file metadata alone — the same cheap signal the
/// lifecycle state uses to decide whether it still describes the store.
fn ledger_bytes(store_root: &Path) -> Result<u64, Error> {
    let directory = store_root.join("records");
    let mut bytes = 0;
    if directory.is_dir() {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if path.extension().is_some_and(|value| value == "jsonl") {
                bytes += std::fs::metadata(&path)?.len();
            }
        }
    }
    Ok(bytes)
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
