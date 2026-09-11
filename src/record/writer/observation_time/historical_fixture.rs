//! Valid pre-fix ledger bytes, constructed without weakening the fresh writer.
use super::super::operation;
use crate::kernel::digest::sha256_hex;
use crate::record::receipt::{self, WriteReceipt, WriteStatus};
use crate::record::{AppendReport, RecordDraft, StoredRecord};
use std::fs;
use std::io::Write;
use std::path::Path;

pub(super) fn seed(
    root: &Path,
    draft: RecordDraft,
    actor: &str,
    key: Option<&str>,
) -> AppendReport {
    let key = key.map(|key| operation::Key::new(actor, key, &draft).unwrap());
    let record = StoredRecord {
        id: uuid::Uuid::now_v7(),
        namespace: draft.namespace,
        type_name: draft.type_name,
        actor: actor.into(),
        recorded_at: "2000-01-01T00:00:00Z".into(),
        valid_at: draft.valid_at.unwrap_or_else(|| draft.observed_at.clone()),
        observed_at: draft.observed_at,
        payload: draft.payload,
        evidence: draft.evidence,
        tags: draft.tags,
        supersedes: draft.supersedes,
    };
    let bytes = serde_json::to_vec(&record).unwrap();
    let hash = sha256_hex(&bytes);
    let staged = receipt::stage(
        root,
        "2000-01",
        &WriteReceipt {
            receipt_id: record.id,
            status: WriteStatus::Appended,
            record_id: Some(record.id),
            namespace: &record.namespace,
            type_name: &record.type_name,
            actor,
            recorded_at: &record.recorded_at,
            record_sha256: Some(&hash),
            durable: true,
            projection: crate::vector::projection_after_write(root),
            defense_findings: &[],
        },
    )
    .unwrap();
    let report = AppendReport {
        ok: true,
        durable: true,
        vector: Default::default(),
        similar: Vec::new(),
        id: record.id,
        sha256: hash,
        ledger: "records/2000-01.jsonl".into(),
        receipt: staged.relative().into(),
        redacted: false,
        projection: crate::projection::ProjectionState::Queued,
    };
    fs::create_dir_all(root.join("records")).unwrap();
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join(&report.ledger))
        .unwrap();
    let offset = file.metadata().unwrap().len();
    file.write_all(&bytes).unwrap();
    file.write_all(b"\n").unwrap();
    staged.commit().unwrap();
    if let Some(key) = key {
        let outcome =
            operation::reserve(root, &key, &report, offset, bytes.len() as u64 + 1, None).unwrap();
        // Leave this as a completed durable append whose operation publication
        // was interrupted. Recovery, then normal retry, must both accept it.
        let _ = outcome;
    }
    report
}
