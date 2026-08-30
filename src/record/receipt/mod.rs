mod recovery;

use crate::defense::DefenseFinding;
use crate::kernel::error::Error;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WriteStatus {
    Appended,
    BlockedByMemoryDefense,
}

#[derive(Debug, Serialize)]
pub struct WriteReceipt<'a> {
    pub receipt_id: Uuid,
    pub status: WriteStatus,
    pub record_id: Option<Uuid>,
    pub namespace: &'a str,
    #[serde(rename = "type")]
    pub type_name: &'a str,
    pub actor: &'a str,
    pub recorded_at: &'a str,
    pub record_sha256: Option<&'a str>,
    /// The canonical claim, written into the receipt so it survives the process
    /// that made it: this record is in the immutable ledger. It says nothing
    /// about the index, which is reported beside it and separately.
    pub durable: bool,
    /// Where the vector projection stood when the record was written. A receipt
    /// that only recorded durability left a reader unable to tell a store that
    /// was current from one that was still catching up.
    pub projection: crate::vector::Projection,
    pub defense_findings: &'a [DefenseFinding],
}

pub(super) const PENDING: &str = "receipts/pending";

pub use recovery::resolve_pending;

pub struct StagedReceipt {
    pending: PathBuf,
    final_path: PathBuf,
    relative: String,
    handle: String,
    /// Whether the pending file is somebody else's problem now — either because
    /// it was renamed into place, or because it was deliberately left for
    /// recovery. Only an abandoned staging that nothing durable depends on gets
    /// deleted.
    settled: bool,
}

impl StagedReceipt {
    pub fn relative(&self) -> &str {
        &self.relative
    }

    /// Where the unfinished receipt is, if committing it fails. Stable across
    /// processes and restarts, because it is a path rather than a handle to
    /// anything in memory.
    pub fn handle(&self) -> &str {
        &self.handle
    }

    /// Move the staged receipt into place.
    ///
    /// Settled either way. On success the file is where it belongs; on failure
    /// it stays in `receipts/pending`, which is where recovery finishes it from.
    /// Deleting it on failure would destroy the only material a durable record's
    /// receipt can be rebuilt from, and the record would remain durable
    /// regardless — an accepted write with no receipt is precisely what must not
    /// be allowed to exist.
    pub fn commit(mut self) -> Result<(), Error> {
        self.settled = true;
        if let Some(directory) = self.final_path.parent() {
            fs::create_dir_all(directory)?;
        }
        Ok(fs::rename(&self.pending, &self.final_path)?)
    }
}

impl Drop for StagedReceipt {
    fn drop(&mut self) {
        if !self.settled {
            let _ = fs::remove_file(&self.pending);
        }
    }
}

pub fn stage(
    store_root: &Path,
    month: &str,
    receipt: &WriteReceipt<'_>,
) -> Result<StagedReceipt, Error> {
    // Staged in its own directory rather than beside the receipts it will join.
    // Recovery has to find unfinished work without reading a month that may
    // hold every receipt the store has ever written, and a directory that is
    // empty except during a failure is the cheapest possible place to look.
    let directory = store_root.join(PENDING);
    fs::create_dir_all(&directory)?;
    let relative = format!("receipts/writes/{month}/{}.json", receipt.receipt_id);
    let final_path = store_root.join(&relative);
    let handle = format!("{PENDING}/{}.json", receipt.receipt_id);
    let pending = store_root.join(&handle);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)?;
    serde_json::to_writer(&mut file, receipt)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(StagedReceipt {
        pending,
        final_path,
        relative,
        handle,
        settled: false,
    })
}
