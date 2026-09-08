//! Rewriting a native ledger without its dead records.
//!
//! The store is the source here, so the new ledger is built beside the old one
//! and swapped in whole — the same staging the manifest path already uses. A
//! crash before the swap leaves the original untouched; a crash during it is
//! undone by the same rollback, because the swap is a rename and a rename is
//! the only step that is not free to fail halfway.
use super::plan::Plan;
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use uuid::Uuid;

/// The ledger as it will be after compaction: dead records gone, and the links
/// that pointed at them cut.
///
/// Cutting is not cosmetic — it changes the envelope and therefore the record's
/// hash. It is done because lifecycle validation refuses a `supersedes` it
/// cannot resolve, and after this there is nothing left to resolve it to.
pub fn rewrite(records: &[StoredRecord], plan: &Plan) -> Vec<StoredRecord> {
    let removed = plan
        .removed
        .iter()
        .map(|item| item.id)
        .collect::<HashSet<Uuid>>();
    records
        .iter()
        .filter(|record| !removed.contains(&record.id))
        .map(|record| {
            let mut kept = record.clone();
            if kept.supersedes.is_some_and(|id| removed.contains(&id)) {
                kept.supersedes = None;
            }
            kept
        })
        .collect()
}

/// One ledger file per month, exactly as the writer lays them out, so an
/// ordinary append after compaction lands where it always would.
pub fn stage_records(shadow: &Path, records: &[StoredRecord]) -> Result<(), Error> {
    let directory = shadow.join("records");
    fs::create_dir_all(&directory)?;
    let mut by_month: std::collections::BTreeMap<String, Vec<u8>> =
        std::collections::BTreeMap::new();
    for record in records {
        let month = record
            .recorded_at
            .get(..7)
            .ok_or_else(|| Error::Compact("record has no usable month".into()))?
            .to_owned();
        let line = by_month.entry(month).or_default();
        line.extend_from_slice(&serde_json::to_vec(record)?);
        line.push(b'\n');
    }
    for (month, bytes) in by_month {
        fs::write(directory.join(format!("{month}.jsonl")), bytes)?;
    }
    Ok(())
}

/// Receipts for records that no longer exist go with them: a receipt that
/// verifies nothing is not evidence, it is an orphan that makes `doctor` green
/// for the wrong reason.
pub fn drop_receipts(shadow: &Path, plan: &Plan) -> Result<(), Error> {
    let writes = shadow.join("receipts/writes");
    if !writes.is_dir() {
        return Ok(());
    }
    let removed = plan
        .removed
        .iter()
        .map(|item| item.id.to_string())
        .collect::<HashSet<_>>();
    for month in fs::read_dir(&writes)? {
        let month = month?.path();
        if !month.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&month)? {
            let path = entry?.path();
            let named = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if removed.contains(named) {
                fs::remove_file(&path)?;
            }
        }
    }
    Ok(())
}
