use super::{blocked::ensure_clean_tail, operation, prepare::Prepared, revocation::month};
use crate::kernel::{digest::sha256_hex, error::Error, path};
use crate::record::AppendReport;
use crate::record::lifecycle::{self, LifecycleState};
use crate::record::receipt::{self, WriteReceipt, WriteStatus};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

/// The only ledger append path, for one record and for preflighted batches.
pub(super) fn records(
    root: &Path,
    prepared: Vec<Prepared>,
    lifecycle: &mut LifecycleState,
    key: Option<&operation::Key>,
    atomic: bool,
) -> Result<Vec<AppendReport>, Error> {
    let Some(first) = prepared.first() else {
        return Ok(Vec::new());
    };
    let month = month(&first.record.recorded_at)?;
    let ledger = format!("records/{month}.jsonl");
    let target = path::within(root, &ledger)?;
    ensure_clean_tail(&target)?;
    let offset = if target.exists() {
        target.metadata()?.len()
    } else {
        0
    };
    let mut bytes = Vec::new();
    let mut reports = Vec::new();
    let mut staged = Vec::new();
    for item in &prepared {
        let record = &item.record;
        let line = serde_json::to_vec(record)?;
        let digest = sha256_hex(&line);
        bytes.extend_from_slice(&line);
        bytes.push(b'\n');
        let receipt = WriteReceipt {
            receipt_id: record.id,
            status: WriteStatus::Appended,
            record_id: Some(record.id),
            namespace: &record.namespace,
            type_name: &record.type_name,
            actor: &record.actor,
            recorded_at: &record.recorded_at,
            record_sha256: Some(&digest),
            durable: true,
            projection: crate::vector::projection_after_write(root),
            defense_findings: &item.defense.findings,
        };
        let stage = receipt::stage(root, &month, &receipt)?;
        reports.push(AppendReport {
            ok: true,
            durable: true,
            vector: Default::default(),
            similar: Vec::new(),
            id: record.id,
            sha256: digest,
            ledger: ledger.clone(),
            receipt: stage.relative().to_owned(),
            redacted: item.defense.redacted(),
            projection: crate::projection::ProjectionState::Queued,
        });
        staged.push(stage);
    }
    for stage in &mut staged {
        stage.preserve();
    }
    let revision = if atomic || key.is_some() {
        crate::vector::desired::reserve(root, reports.len() as u64)?
    } else {
        None
    };
    let operation = key
        .map(|key| operation::reserve(root, key, &reports[0], offset, bytes.len() as u64, revision))
        .transpose()?;
    let fresh = !target.exists();
    // Keyed single writes need the same verified bytes for a torn append to
    // recover without guessing whether a partial tail belongs to this request.
    let journal = if atomic || key.is_some() {
        Some(super::transaction::stage(
            root, &reports, &bytes, offset, revision,
        )?)
    } else {
        None
    };
    #[cfg(test)]
    super::seam::at(super::seam::Step::BeforeAppend)?;
    let mut file = OpenOptions::new().create(true).append(true).open(&target)?;
    #[cfg(test)]
    if super::seam::failing(super::seam::Step::PartialAppend) {
        file.write_all(&bytes[..bytes.len() / 2])?;
        file.sync_data()?;
        super::seam::at(super::seam::Step::PartialAppend)?;
    }
    file.write_all(&bytes)?;
    #[cfg(test)]
    super::seam::at(super::seam::Step::BeforeLedgerSync)?;
    file.sync_data()?;
    #[cfg(test)]
    crate::record::hotpath::ledger_sync();
    if fresh {
        File::open(path::within(root, "records")?)?.sync_all()?;
    }
    #[cfg(test)]
    super::seam::at(super::seam::Step::AfterAppend)?;
    for (index, (stage, report)) in staged.into_iter().zip(&reports).enumerate() {
        let handle = stage.handle().to_owned();
        stage.commit().map_err(|error| Error::PostCommit(format!(
            "record {} at append position {index} is durable but its receipt is not committed: {error}; recovery handle: {handle}", report.id
        )))?;
        #[cfg(test)]
        super::seam::at(super::seam::Step::AfterReceipt(index))?;
    }
    #[cfg(test)]
    super::seam::at(super::seam::Step::AfterReceipts)?;
    if let Some(operation) = operation {
        #[cfg(test)]
        super::seam::at(super::seam::Step::BeforeOutcome)?;
        operation::complete(root, &operation).map_err(|_| {
            Error::PostCommit(format!(
                "record {} is durable; operation outcome requires recovery",
                reports[0].id
            ))
        })?;
    }
    if let Some(journal) = journal {
        journal.committed(root).map_err(|_| {
            Error::PostCommit(format!(
                "batch beginning at record {} is durable; transaction cleanup requires recovery",
                reports[0].id
            ))
        })?;
    }
    // These projections can be reconstructed; their failure cannot undo truth.
    let _ = lifecycle::save_state(root, lifecycle);
    let _ =
        crate::projection::publish_target(root, lifecycle.entries.len(), lifecycle.watermark.bytes);
    Ok(reports)
}
