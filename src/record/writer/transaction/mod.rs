mod recovery;

use super::operation::storage;
use crate::kernel::{digest::sha256_hex, error::Error, path};
use crate::record::{AppendReport, StoredRecord};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

const MARKER: &str = "transactions/batch.json";
const DATA: &str = "transactions/batches";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Journal {
    schema: String,
    id: Uuid,
    ledger: String,
    offset: u64,
    length: u64,
    sha256: String,
    phase: Phase,
    records: Vec<Coordinate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vector_revision: Option<u64>,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Phase {
    Prepared,
    Committed,
    Aborted,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Coordinate {
    id: Uuid,
    record_sha256: String,
    receipt: String,
    receipt_sha256: String,
}

pub(super) fn stage(
    root: &Path,
    reports: &[AppendReport],
    bytes: &[u8],
    offset: u64,
    vector_revision: Option<u64>,
) -> Result<Journal, Error> {
    let mut journal = Journal {
        schema: "equill.atomic-batch.v1".into(),
        id: Uuid::now_v7(),
        ledger: reports[0].ledger.clone(),
        offset,
        length: bytes.len() as u64,
        sha256: sha256_hex(bytes),
        phase: Phase::Prepared,
        records: Vec::new(),
        vector_revision,
    };
    for report in reports {
        let stage = path::file_within(root, &format!("receipts/pending/{}.json", report.id))?;
        journal.records.push(Coordinate {
            id: report.id,
            record_sha256: report.sha256.clone(),
            receipt: report.receipt.clone(),
            receipt_sha256: sha256_hex(&fs::read(stage)?),
        });
    }
    storage::directory(root, DATA)?;
    let data = path::within(root, &journal.data())?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(data)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    File::open(path::within(root, DATA)?)?.sync_all()?;
    storage::publish(root, MARKER, &journal)?;
    Ok(journal)
}

impl Journal {
    fn data(&self) -> String {
        format!("{DATA}/{}.jsonl", self.id)
    }

    pub(super) fn committed(mut self, root: &Path) -> Result<(), Error> {
        self.publish_target(root)?;
        self.phase = Phase::Committed;
        storage::publish(root, MARKER, &self)?;
        self.clear(root)
    }

    fn publish_target(&self, root: &Path) -> Result<(), Error> {
        crate::vector::desired::publish_reserved(root, self.vector_revision).map_err(|_| {
            Error::PostCommit(format!(
                "batch {} is durable; target publication requires recovery",
                self.id
            ))
        })
    }

    fn clear(&self, root: &Path) -> Result<(), Error> {
        let data = self.data();
        if path::within(root, &data)?.exists() {
            storage::remove(root, &data)?;
        }
        #[cfg(test)]
        super::seam::at(super::seam::Step::AfterBatchDataRemoval)?;
        storage::remove(root, MARKER)
    }

    fn verify(&self, root: &Path, bytes: &[u8]) -> Result<Vec<StoredRecord>, Error> {
        if bytes.len() as u64 != self.length
            || sha256_hex(bytes) != self.sha256
            || bytes.last() != Some(&b'\n')
        {
            return Err(damaged());
        }
        let mut records = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let config = crate::kernel::store::load(root)?;
        for (line, coordinate) in bytes
            .split_inclusive(|byte| *byte == b'\n')
            .zip(&self.records)
        {
            let line = &line[..line.len() - 1];
            if sha256_hex(line) != coordinate.record_sha256 {
                return Err(damaged());
            }
            let record: StoredRecord = serde_json::from_slice(line).map_err(|_| damaged())?;
            let month = record.recorded_at.get(..7).ok_or_else(damaged)?;
            if record.id != coordinate.id
                || !seen.insert(record.id)
                || self.ledger != format!("records/{month}.jsonl")
                || coordinate.receipt != format!("receipts/writes/{month}/{}.json", record.id)
            {
                return Err(damaged());
            }
            crate::record::verify::verify_record(root, &config, &record).map_err(|_| damaged())?;
            records.push(record);
        }
        if records.len() != self.records.len()
            || bytes.iter().filter(|byte| **byte == b'\n').count() != records.len()
        {
            return Err(damaged());
        }
        Ok(records)
    }

    fn receipts(&self, root: &Path, committed: bool) -> Result<(), Error> {
        for coordinate in &self.records {
            let pending = format!("receipts/pending/{}.json", coordinate.id);
            let final_path = path::within(root, &coordinate.receipt)?;
            let staged = path::within(root, &pending)?;
            let relative = if final_path.exists() {
                if !committed {
                    return Err(damaged());
                }
                &coordinate.receipt
            } else if staged.exists() {
                &pending
            } else {
                return Err(damaged());
            };
            let bytes = fs::read(path::file_within(root, relative)?)?;
            if sha256_hex(&bytes) != coordinate.receipt_sha256 {
                return Err(damaged());
            }
            if committed && relative == &pending {
                let month = self
                    .ledger
                    .strip_prefix("records/")
                    .and_then(|name| name.strip_suffix(".jsonl"))
                    .ok_or_else(damaged)?;
                crate::record::receipt::finalize(root, &staged, month, coordinate.id)?;
            } else if !committed {
                storage::remove(root, &pending)?;
                #[cfg(test)]
                super::seam::at(super::seam::Step::AfterAbortReceiptRemoval)?;
            }
        }
        Ok(())
    }
}

pub(super) use recovery::recover;

fn damaged() -> Error {
    Error::Integrity("atomic batch transaction coordinates or hashes disagree".into())
}
