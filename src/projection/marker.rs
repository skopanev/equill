//! What each side of a projection publishes about where it stands.
//!
//! Two small files and nothing else: the writer says where the ledger is, and
//! the text index says how far it has caught up. They live together because
//! they are one contract read from two ends — a reader compares them to decide
//! whether what it is about to search is current — and apart from the code that
//! searches, because publishing a position is not searching.
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::path::Path;

const WATERMARK: &str = "projections/sqlite/watermark.json";
const WATERMARK_SCHEMA: &str = "equill.sqlite-watermark.v1";
const TARGET: &str = "projections/target.json";
const TARGET_SCHEMA: &str = "equill.projection-target.v1";

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
    let bytes = std::fs::read(path(store_root)).ok()?;
    let mark: TextWatermark = serde_json::from_slice(&bytes).ok()?;
    (mark.schema == WATERMARK_SCHEMA).then_some(mark)
}

fn path(store_root: &Path) -> std::path::PathBuf {
    store_root.join(WATERMARK)
}

/// Total ledger size, from file metadata alone — the same cheap signal the
/// lifecycle state uses to decide whether it still describes the store.
pub(super) fn ledger_bytes(store_root: &Path) -> Result<u64, Error> {
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

/// Say how far the text index has caught up.
///
/// Best effort by design: the position is an optimisation, and a store that
/// will not take the file still holds every record the pass indexed. What it
/// must never do is claim coverage a pass did not reach, which is why the
/// caller writes it only after a complete pass.
pub(super) fn record_watermark(store_root: &Path, reached: u64, indexed_records: usize) {
    let _ = std::fs::create_dir_all(store_root.join("projections/sqlite"));
    let Ok(bytes) = serde_json::to_vec(&TextWatermark {
        schema: WATERMARK_SCHEMA.into(),
        ledger_bytes: reached,
        indexed_records,
    }) else {
        return;
    };
    let _ = std::fs::write(path(store_root), bytes);
}
