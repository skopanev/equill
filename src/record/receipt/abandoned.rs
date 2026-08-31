//! What is kept when a staged receipt turns out to describe nothing.
//!
//! A stage that no ledger record answers for belonged to a write that died
//! before its append. Keeping the file itself would preserve a document that
//! reads like a receipt — namespace, type, actor, findings, all of it — for a
//! record that does not exist, in a directory a person will later go looking
//! through. What is worth keeping is only that this happened, to which
//! transaction, and to what exact bytes.
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

const ABANDONED: &str = "receipts/abandoned";
const PARENT: &str = "receipts";
const SCHEMA: &str = "equill.receipt-quarantine.v1";

/// The whole of what an abandoned stage leaves behind.
///
/// Five fields and no others: which transaction, by the path its receipt would
/// have occupied; the digest of the stage as it stood; why it was set aside;
/// and when. The digest is what makes the note checkable — a person holding a
/// copy of the stage can tell whether it is the one this note describes —
/// without the note itself asserting anything about a record that was never
/// written.
///
/// The coordinate is the receipt path rather than a bare id because that is
/// the coordinate the store already speaks in: it is what a successful write
/// reports back. A reader comparing this note against the receipts is then
/// comparing like with like.
#[derive(Serialize)]
struct Note {
    schema: &'static str,
    coordinate: String,
    stage_sha256: String,
    reason: &'static str,
    recovered_at: String,
}

/// Replace a stage with a note about it.
///
/// The note is durable before the stage is removed, and durable means the
/// rename is durable, not merely that the bytes were synced: a renamed file
/// whose directory was never synced can be gone after a power loss while the
/// stage it replaced is also gone. So each step is published before the next
/// one depends on it — parent, then the note's directory, then the removal.
///
/// A crash anywhere in that order leaves the stage in place with, at worst, a
/// note beside it. The next run recomputes the same note from the same bytes
/// and removes the stage again: the operation converges rather than depending
/// on having completed. Every failure is returned rather than swallowed,
/// because a stage removed on the strength of a note that was not written is
/// the one outcome this must never produce.
pub(super) fn quarantine(
    store_root: &Path,
    stage: &Path,
    receipt_id: Uuid,
    coordinate: String,
    bytes: &[u8],
) -> Result<(), Error> {
    let fresh = !super::path::within(store_root, ABANDONED)?.is_dir();
    fs::create_dir_all(store_root.join(ABANDONED))?;
    // Created, then walked: create_dir_all accepts a link that already resolves
    // to a directory, and this is where a file is about to be renamed into.
    let directory = super::path::within(store_root, ABANDONED)?;
    if fresh {
        // A directory entry is no more durable than a file's. If this is the
        // first abandonment the store has ever had, the directory holding the
        // note has to survive too.
        publish(&super::path::within(store_root, PARENT)?)?;
    }
    let note = serde_json::to_vec(&Note {
        schema: SCHEMA,
        coordinate,
        stage_sha256: sha256_hex(bytes),
        reason: "pre_append_crash",
        recovered_at: jiff::Timestamp::now().to_string(),
    })?;
    // A name nothing can have anticipated, claimed with create_new. A
    // predictable temporary name is a place to leave a link and wait: the write
    // would follow it out of the store, and the rename would publish whatever
    // it found there as this store's own record of what happened.
    let temporary =
        super::path::within(store_root, &format!("{ABANDONED}/.{}.json", Uuid::now_v7()))?;
    if let Err(error) = write_durably(&temporary, &note) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    let target = super::path::within(store_root, &format!("{ABANDONED}/{receipt_id}.json"))?;
    if fs::rename(&temporary, &target).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(Error::Integrity(
            "an abandoned stage could not be recorded".into(),
        ));
    }
    // The rename, published. Until this returns, the note is a name in a
    // directory that may not survive, and the stage must not be removed on the
    // strength of it.
    publish(&directory)?;
    fs::remove_file(stage)?;
    // And the removal, published, so the next run does not find the stage again
    // and do this a second time.
    publish(stage.parent().unwrap_or(store_root))
}

fn write_durably(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    Ok(file.sync_all()?)
}

/// Make a directory's contents — its names, not its files — survive a crash.
fn publish(directory: &Path) -> Result<(), Error> {
    Ok(File::open(directory)?.sync_all()?)
}
