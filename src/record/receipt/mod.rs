mod abandoned;
mod recovery;
mod shard;

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
pub(super) const WRITES: &str = "receipts/writes";
const RECEIPTS: &str = "receipts";

#[cfg(test)]
pub(crate) use abandoned::seam as quarantine_seam;
pub use recovery::resolve_pending;

pub struct StagedReceipt {
    root: PathBuf,
    pending: PathBuf,
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
        let month = self
            .relative
            .rsplit_once('/')
            .map(|(head, _)| head)
            .unwrap_or_default();
        let fresh = !crate::kernel::path::within(&self.root, month)?.is_dir();
        let directory = crate::kernel::path::prepare(&self.root, month)?;
        let target = crate::kernel::path::within(&self.root, &self.relative)?;
        fs::rename(&self.pending, &target)?;
        if fresh {
            crate::kernel::path::publish(
                &crate::kernel::path::within(&self.root, WRITES)?,
                crate::kernel::path::Step::MonthCreated,
            )?;
        }
        // A rename is a name in a directory, and a name is no more durable than
        // the directory holding it. Published here rather than left to chance:
        // until this returns, the receipt is not committed, and this call must
        // not report that it is.
        crate::kernel::path::publish(&directory, crate::kernel::path::Step::Committed)?;
        crate::kernel::path::publish(
            &crate::kernel::path::within(&self.root, PENDING)?,
            crate::kernel::path::Step::Drained,
        )
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
    // Walked before it is created, not only after. Walking afterwards cannot
    // protect an ancestor — by then `create_dir_all` has used it — and a
    // refusal that has already made a directory outside the store is still a
    // side effect outside the store.
    let fresh = !crate::kernel::path::within(store_root, PENDING)?.is_dir();
    let staging = crate::kernel::path::prepare(store_root, PENDING)?;
    if fresh {
        crate::kernel::path::publish(
            &crate::kernel::path::within(store_root, RECEIPTS)?,
            crate::kernel::path::Step::PendingCreated,
        )?;
    }
    let relative = format!("receipts/writes/{month}/{}.json", receipt.receipt_id);
    crate::kernel::path::within(store_root, &relative)?;
    let handle = format!("{PENDING}/{}.json", receipt.receipt_id);
    let pending = crate::kernel::path::within(store_root, &handle)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)?;
    serde_json::to_writer(&mut file, receipt)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    // The staged receipt's own name, published before the ledger is touched.
    // The append comes after this returns, so a crash between them must leave a
    // stage with no record — which recovery reads as a pre-append crash — and
    // never a record with no stage, which it cannot finish at all.
    crate::kernel::path::publish(&staging, crate::kernel::path::Step::Staged)?;
    Ok(StagedReceipt {
        root: store_root.to_owned(),
        pending,
        relative,
        handle,
        settled: false,
    })
}
