mod compact;
mod recovery;
pub(super) mod storage;
pub(crate) use compact::reconcile;
pub(crate) use recovery::recover;

use crate::kernel::{digest::sha256_hex, error::Error, path};
use crate::record::{AppendReport, RecordDraft, StoredRecord};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use uuid::Uuid;

const PENDING: &str = "receipts/operations/pending";
const COMMITTED: &str = "receipts/operations/committed";
const SCHEMA: &str = "equill.append-operation.v1";

pub(super) struct Key {
    digest: String,
    request: String,
}

impl Key {
    pub(super) fn new(actor: &str, key: &str, draft: &RecordDraft) -> Result<Self, Error> {
        let mut canonical = serde_json::to_value(draft)?;
        canonical.sort_all_objects();
        Ok(Self {
            // The selected store owns this directory: key scope is per store.
            digest: sha256_hex(&serde_json::to_vec(&(actor, key))?),
            request: sha256_hex(&serde_json::to_vec(&canonical)?),
        })
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Outcome {
    schema: String,
    key_sha256: String,
    request_sha256: String,
    record_id: Uuid,
    record_sha256: String,
    ledger: String,
    receipt: String,
    offset: u64,
    length: u64,
    redacted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vector_revision: Option<u64>,
}

pub(super) fn reserve(
    root: &Path,
    key: &Key,
    report: &AppendReport,
    offset: u64,
    length: u64,
    vector_revision: Option<u64>,
) -> Result<Outcome, Error> {
    let outcome = Outcome {
        schema: SCHEMA.into(),
        key_sha256: key.digest.clone(),
        request_sha256: key.request.clone(),
        record_id: report.id,
        record_sha256: report.sha256.clone(),
        ledger: report.ledger.clone(),
        receipt: report.receipt.clone(),
        offset,
        length,
        redacted: report.redacted,
        vector_revision,
    };
    storage::publish(root, &format!("{PENDING}/{}.json", key.digest), &outcome)?;
    Ok(outcome)
}

pub(super) fn complete(root: &Path, outcome: &Outcome) -> Result<(), Error> {
    crate::vector::desired::publish_reserved(root, outcome.vector_revision).map_err(|_| {
        Error::PostCommit(format!(
            "record {} is durable; target publication requires recovery",
            outcome.record_id
        ))
    })?;
    storage::publish(
        root,
        &format!("{COMMITTED}/{}.json", outcome.key_sha256),
        outcome,
    )?;
    storage::remove(root, &format!("{PENDING}/{}.json", outcome.key_sha256))
}

pub(super) fn lookup(
    root: &Path,
    key: &Key,
) -> Result<Option<(AppendReport, StoredRecord)>, Error> {
    let relative = format!("{COMMITTED}/{}.json", key.digest);
    if !path::within(root, &relative)?.exists() {
        return Ok(None);
    }
    let outcome = load(root, &relative, &key.digest)?;
    if outcome.request_sha256 != key.request {
        return Err(Error::InvalidRecord(format!(
            "idempotency conflict for {}",
            key.digest
        )));
    }
    let record = record(root, &outcome)?.ok_or_else(|| invalid(&key.digest))?;
    verify_receipt(root, &outcome)?;
    Ok(Some((report(&outcome), record)))
}

fn load(root: &Path, relative: &str, digest: &str) -> Result<Outcome, Error> {
    let bytes = fs::read(path::file_within(root, relative)?)?;
    let value: Outcome = serde_json::from_slice(&bytes).map_err(|_| invalid(digest))?;
    let valid = |sha: &str| {
        sha.len() == 64
            && sha
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };
    if value.schema != SCHEMA
        || value.key_sha256 != digest
        || !valid(digest)
        || !valid(&value.request_sha256)
        || !valid(&value.record_sha256)
        || value.length == 0
        || value.length > 64 * 1024 * 1024
    {
        return Err(invalid(digest));
    }
    Ok(value)
}

fn record(root: &Path, outcome: &Outcome) -> Result<Option<StoredRecord>, Error> {
    let ledger = path::within(root, &outcome.ledger)?;
    if !ledger.exists() && outcome.offset == 0 {
        return Ok(None);
    }
    let mut file = File::open(path::file_within(root, &outcome.ledger)?)?;
    let size = file.metadata()?.len();
    if size == outcome.offset {
        return Ok(None);
    }
    if size < outcome.offset.saturating_add(outcome.length) {
        return Err(invalid(&outcome.key_sha256));
    }
    file.seek(SeekFrom::Start(outcome.offset))?;
    let mut bytes = vec![0; outcome.length as usize];
    file.read_exact(&mut bytes)?;
    if bytes.pop() != Some(b'\n') || sha256_hex(&bytes) != outcome.record_sha256 {
        return Err(invalid(&outcome.key_sha256));
    }
    let record: StoredRecord =
        serde_json::from_slice(&bytes).map_err(|_| invalid(&outcome.key_sha256))?;
    if record.id != outcome.record_id
        || outcome.ledger
            != format!(
                "records/{}.jsonl",
                record.recorded_at.get(..7).unwrap_or_default()
            )
        || outcome.receipt
            != format!(
                "receipts/writes/{}/{}.json",
                record.recorded_at.get(..7).unwrap_or_default(),
                record.id
            )
    {
        return Err(invalid(&outcome.key_sha256));
    }
    crate::record::verify::verify_record(root, &crate::kernel::store::load(root)?, &record)
        .map_err(|_| invalid(&outcome.key_sha256))?;
    Ok(Some(record))
}

fn verify_receipt(root: &Path, outcome: &Outcome) -> Result<(), Error> {
    let bytes = fs::read(path::file_within(root, &outcome.receipt)?)?;
    let receipt: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| invalid(&outcome.key_sha256))?;
    if receipt["record_id"] != outcome.record_id.to_string()
        || receipt["record_sha256"] != outcome.record_sha256
        || receipt["durable"] != true
        || receipt["status"] != "appended"
    {
        return Err(invalid(&outcome.key_sha256));
    }
    Ok(())
}

fn report(outcome: &Outcome) -> AppendReport {
    AppendReport {
        ok: true,
        durable: true,
        vector: Default::default(),
        similar: Vec::new(),
        id: outcome.record_id,
        sha256: outcome.record_sha256.clone(),
        ledger: outcome.ledger.clone(),
        receipt: outcome.receipt.clone(),
        redacted: outcome.redacted,
        projection: crate::projection::ProjectionState::Queued,
    }
}

fn invalid(digest: &str) -> Error {
    let digest = if digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        digest
    } else {
        "metadata"
    };
    Error::Integrity(format!("append operation {digest} is inconsistent"))
}
