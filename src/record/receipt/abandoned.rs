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
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

const ABANDONED: &str = "receipts/abandoned";
const SCHEMA: &str = "equill.abandoned-stage.v1";

/// The whole of what an abandoned stage leaves behind.
///
/// Five fields and no others: which transaction, the digest of the stage as it
/// stood, why it was set aside, and when. The digest is what makes the note
/// checkable — a person holding a copy of the stage can tell whether it is the
/// one this note describes — without the note itself asserting anything about a
/// record that was never written.
#[derive(Serialize)]
struct Note {
    schema: &'static str,
    receipt_id: Uuid,
    stage_sha256: String,
    reason: &'static str,
    recovered_at: String,
}

/// Replace a stage with a note about it.
///
/// The note is written and made durable first, then the stage is removed. A
/// crash in between leaves both, and the next run recomputes the same note from
/// the same bytes and removes the stage again — the operation converges rather
/// than depending on having completed.
pub(super) fn quarantine(
    store_root: &Path,
    stage: &Path,
    receipt_id: Uuid,
    bytes: &[u8],
) -> Result<(), Error> {
    let directory = store_root.join(ABANDONED);
    fs::create_dir_all(&directory)?;
    let note = serde_json::to_vec(&Note {
        schema: SCHEMA,
        receipt_id,
        stage_sha256: sha256_hex(bytes),
        reason: "pre_append_crash",
        recovered_at: jiff::Timestamp::now().to_string(),
    })?;
    let final_path = directory.join(format!("{receipt_id}.json"));
    let temporary = directory.join(format!(".{receipt_id}.tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)?;
    if file
        .write_all(&note)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(Error::Integrity(
            "an abandoned stage could not be recorded".into(),
        ));
    }
    drop(file);
    if fs::rename(&temporary, &final_path).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(Error::Integrity(
            "an abandoned stage could not be recorded".into(),
        ));
    }
    // Only now: until the note is on disk, the stage is the only evidence there
    // is, and removing it first would lose the transaction entirely.
    Ok(fs::remove_file(stage)?)
}
