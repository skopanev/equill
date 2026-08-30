//! The two edges of the write path that are not the write: a draft the memory
//! defense refuses, and a ledger whose last line never finished.
//!
//! Both belong beside confirmation rather than inside it. A blocked draft never
//! becomes a record, so it takes a receipt and no ledger line; an unfinished
//! tail means the previous writer died mid-line, and appending after it would
//! bury the damage under valid data.
use super::super::receipt::{self, WriteReceipt, WriteStatus};
use super::super::{AppendReport, RecordDraft, StoredRecord};
use crate::defense;
use crate::kernel::error::Error;
use crate::kernel::lock::StoreLock;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use uuid::Uuid;

/// `block_write` always returns an error for a blocked draft; this exists only
/// so the types line up if that ever changes, and says loudly what it assumes.
pub(super) fn unreachable_report(_report: AppendReport) -> (AppendReport, StoredRecord) {
    unreachable!("a blocked write returns an error rather than a report")
}

pub(super) fn block_write(
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
        // Blocked before it reached the ledger. Nothing is durable, so nothing
        // is queued either: reporting a projection state would describe work
        // that will never happen for a record that does not exist.
        durable: false,
        projection: crate::vector::Projection::NotApplicable,
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

pub(super) fn ensure_clean_tail(path: &Path) -> Result<(), Error> {
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
