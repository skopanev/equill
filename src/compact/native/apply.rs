//! Rewriting a native ledger without its dead records.
//!
//! The store is the source here, so the new ledger is built beside the old one
//! and swapped in whole — the same staging the manifest path already uses. A
//! crash before the swap leaves the original untouched; a crash during it is
//! undone by the same rollback, because the swap is a rename and a rename is
//! the only step that is not free to fail halfway.
use super::projections::Plan;
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
        durable_write(&directory.join(format!("{month}.jsonl")), &bytes)?;
    }
    crate::compact::native::journal::sync_directory(&directory)?;
    Ok(())
}

/// A staged file the journal will point at has to survive the crash the
/// journal exists for. Writing it into the page cache and recording that it is
/// ready are two different claims.
pub fn durable_write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let file = fs::File::create(path)?;
    {
        use std::io::Write as _;
        let mut writer = std::io::BufWriter::new(&file);
        writer.write_all(bytes)?;
        writer.flush()?;
    }
    file.sync_all()?;
    Ok(())
}

/// Receipts for records that no longer exist go with them: a receipt that
/// verifies nothing is not evidence, it is an orphan that makes `doctor` green
/// for the wrong reason.
/// A retained record whose link was cut has a new hash, and its receipt still
/// carries the old one.
///
/// Left alone, every one of those receipts would disagree with the record it
/// attests to — and a verification comparing them would report corruption for
/// records nobody touched. The receipt is updated to the bytes that are now in
/// the ledger, which is what it was always meant to describe.
pub fn reconcile_receipts(shadow: &Path, kept: &[StoredRecord]) -> Result<(), Error> {
    let writes = shadow.join("receipts/writes");
    if !writes.is_dir() {
        return Ok(());
    }
    let mut current = std::collections::HashMap::new();
    for record in kept {
        current.insert(
            record.id.to_string(),
            crate::kernel::digest::sha256_hex(&serde_json::to_vec(record)?),
        );
    }
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
                .unwrap_or_default()
                .to_owned();
            let Some(digest) = current.get(&named) else {
                continue;
            };
            let mut receipt: serde_json::Value = serde_json::from_slice(&fs::read(&path)?)?;
            if receipt["record_sha256"].as_str() == Some(digest.as_str()) {
                continue;
            }
            receipt["record_sha256"] = serde_json::json!(digest);
            durable_write(&path, &serde_json::to_vec(&receipt)?)?;
        }
    }
    Ok(())
}

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

/// A swallowed failure here reads as a finished transaction while leaving the
/// staged copy behind, and the next run would find a store it cannot explain.
pub fn remove(path: &Path) -> Result<(), Error> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(Error::Compact(format!(
            "could not remove {}: {error}",
            path.display()
        ))),
    }
}
