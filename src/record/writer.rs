use super::{AppendReport, RecordDraft, StoredRecord};
use crate::defense;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::kernel::identity;
use crate::kernel::lock::StoreLock;
use crate::kernel::store;
use crate::projection::{self, ProjectionState};
use crate::schema;
use jiff::Timestamp;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use uuid::Uuid;

use super::receipt::{self, WriteReceipt, WriteStatus};

pub fn append_file(store_root: &Path, source: &Path, actor: &str) -> Result<AppendReport, Error> {
    let draft: RecordDraft = serde_json::from_slice(&fs::read(source)?)?;
    append(store_root, draft, actor)
}

/// One record, then a catch-up. A batch wants exactly one catch-up for the
/// whole set rather than one per line — loading the model forty times to index
/// forty records is the difference between a usable import and an unusable one
/// — so batch callers use `append_only` and drain once at the end.
pub fn append(store_root: &Path, draft: RecordDraft, actor: &str) -> Result<AppendReport, Error> {
    let mut report = append_only(store_root, draft, actor)?;
    report.vector = crate::vector::after_commit(store_root, actor);
    Ok(report)
}

pub fn append_only(
    store_root: &Path,
    mut draft: RecordDraft,
    actor: &str,
) -> Result<AppendReport, Error> {
    let config = store::load(store_root)?;
    identity::require_type_writer(&config, actor, &draft.namespace, &draft.type_name)?;
    let recorded_at = Timestamp::now().to_string();
    let month = month(&recorded_at)?;
    let defense = defense::apply(store_root, &mut draft)?;
    if defense.blocked() {
        return block_write(store_root, &draft, actor, &recorded_at, &month, defense);
    }
    let definition = schema::load(store_root, &draft.type_name)?;
    super::validation::validate(&draft, &config, &definition)?;

    let redacted = defense.redacted();
    let valid_at = draft
        .valid_at
        .clone()
        .unwrap_or_else(|| draft.observed_at.clone());
    let record = StoredRecord {
        id: Uuid::now_v7(),
        namespace: draft.namespace,
        type_name: draft.type_name,
        actor: actor.to_owned(),
        recorded_at,
        observed_at: draft.observed_at,
        valid_at,
        payload: draft.payload,
        evidence: draft.evidence,
        tags: draft.tags,
        supersedes: draft.supersedes,
    };
    let mut line = serde_json::to_vec(&record)?;
    let digest = sha256_hex(&line);
    line.push(b'\n');
    let ledger = format!("records/{month}.jsonl");
    let path = store_root.join(&ledger);
    let receipt = WriteReceipt {
        receipt_id: record.id,
        status: WriteStatus::Appended,
        record_id: Some(record.id),
        namespace: &record.namespace,
        type_name: &record.type_name,
        actor,
        recorded_at: &record.recorded_at,
        record_sha256: Some(&digest),
        defense_findings: &defense.findings,
    };

    let _lock = StoreLock::exclusive(store_root)?;
    let records = super::read_all(store_root)?;
    super::lifecycle::validate_append(store_root, records, &record, &definition)?;
    ensure_clean_tail(&path)?;
    let staged = receipt::stage(store_root, &month, &receipt)?;
    let receipt_path = staged.relative().to_owned();
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&line)?;
    file.sync_data()?;
    staged.commit().map_err(|error| {
        Error::PostCommit(format!(
            "record {} was appended but its receipt failed: {error}",
            record.id
        ))
    })?;
    let projection = match projection::index(store_root, &record, &digest, &ledger) {
        Ok(()) => ProjectionState::Ready,
        Err(error) => {
            projection::mark_degraded(store_root, &record, &error.to_string());
            ProjectionState::Degraded
        }
    };

    // Advisory only, and computed after the record is durable: a similarity
    // check that could fail a write would be a worse trade than a missed hint.
    let similar = super::find_similar(store_root, &record).unwrap_or_default();
    Ok(AppendReport {
        ok: true,
        vector: crate::vector::DrainReport::default(),
        similar,
        id: record.id,
        sha256: digest,
        ledger,
        receipt: receipt_path,
        redacted,
        projection,
    })
}

fn month(timestamp: &str) -> Result<String, Error> {
    timestamp
        .get(..7)
        .map(str::to_owned)
        .ok_or_else(|| Error::InvalidRecord("system clock is out of range".into()))
}

fn block_write(
    store_root: &Path,
    draft: &RecordDraft,
    actor: &str,
    recorded_at: &str,
    month: &str,
    defense: defense::DefenseResult,
) -> Result<AppendReport, Error> {
    let receipt = WriteReceipt {
        receipt_id: Uuid::now_v7(),
        status: WriteStatus::BlockedByMemoryDefense,
        record_id: None,
        namespace: &draft.namespace,
        type_name: &draft.type_name,
        actor,
        recorded_at,
        record_sha256: None,
        defense_findings: &defense.findings,
    };
    let matches = defense.findings.len();
    let _lock = StoreLock::exclusive(store_root)?;
    let staged = receipt::stage(store_root, month, &receipt)?;
    let path = staged.relative().to_owned();
    staged.commit()?;
    Err(Error::MemoryDefense(format!(
        "blocked {matches} match(es); receipt: {path}"
    )))
}

fn ensure_clean_tail(path: &Path) -> Result<(), Error> {
    if !path.exists() || path.metadata()?.len() == 0 {
        return Ok(());
    }
    let mut file = OpenOptions::new().read(true).open(path)?;
    file.seek(SeekFrom::End(-1))?;
    let mut tail = [0_u8; 1];
    file.read_exact(&mut tail)?;
    if tail[0] != b'\n' {
        return Err(Error::Integrity(format!(
            "ledger has an incomplete final line: {}",
            path.display()
        )));
    }
    Ok(())
}
