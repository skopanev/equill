//! Finishing a write that did not finish itself.
//!
//! A receipt is staged in `receipts/pending` and renamed into its month on
//! commit. Anything still in that directory belongs to a transaction that
//! stopped somewhere in the middle — and the middle has two halves. Staging
//! happens BEFORE the ledger append, so a process that dies between them leaves
//! a staged receipt for a record that was never written. Treating the file's
//! survival as proof the append happened would manufacture a committed receipt
//! attesting to a record the ledger does not hold, which is a worse failure
//! than the one being recovered from.
//!
//! So nothing here is inferred. The ledger shard the receipt names is the only
//! thing that settles it, and it is asked directly.
use super::PENDING;
use super::shard::{Held, canonical_digest, ledger_holds};
use crate::kernel::error::Error;
use serde::Deserialize;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use uuid::Uuid;

/// The fields of a staged receipt that recovery is allowed to act on.
#[derive(Deserialize)]
struct Pending {
    receipt_id: Uuid,
    status: String,
    #[serde(default)]
    record_id: Option<Uuid>,
    #[serde(default)]
    record_sha256: Option<String>,
    recorded_at: String,
    durable: bool,
}

/// Finish any receipt a previous write left unfinished.
///
/// Called under the writer lock at the start of every write. The directory is
/// empty in the ordinary case, which is what makes this affordable there: one
/// listing of an empty directory, never a scan of the receipts a store has
/// accumulated.
///
/// Every outcome except "finished" or "there was nothing" refuses the write.
/// The store already holds an unresolved transaction; accepting another write
/// would bury it under one more.
pub fn resolve_pending(store_root: &Path) -> Result<(), Error> {
    let directory = crate::kernel::path::within(store_root, PENDING)?;
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        // Only absence means there is nothing pending. A directory that exists
        // and cannot be read is not an empty one, and reading it as empty would
        // let a write proceed over an unresolved transaction it never saw.
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        // A listing hands back names. Turning one into a path is where a name
        // that is not a name would do its damage, so it is checked there, and
        // the whole chain down to it is walked rather than assumed.
        let name = crate::kernel::path::plain_name(&entry?.path())?;
        let stage = crate::kernel::path::file_within(store_root, &format!("{PENDING}/{name}"))?;
        resolve_one(store_root, &stage)?;
    }
    Ok(())
}

fn resolve_one(store_root: &Path, path: &Path) -> Result<(), Error> {
    let bytes = fs::read(path)?;
    let staged: Pending = serde_json::from_slice(&bytes)
        .map_err(|error| refuse(path, &format!("it cannot be read: {error}")))?;
    // The name is part of the claim. A receipt whose filename disagrees with
    // its contents names two different transactions, and neither can be trusted.
    if path
        .file_name()
        .is_none_or(|name| *name != *format!("{}.json", staged.receipt_id))
    {
        return Err(refuse(path, "its name and its contents disagree"));
    }
    let month = month_of(&staged.recorded_at)
        .ok_or_else(|| refuse(path, "its recorded_at is not a timestamp this store writes"))?;

    if staged.status == "blocked-by-memory-defense" {
        // A refused draft never reached the ledger and never claimed to. Its
        // receipt is the whole record of what happened, so finishing it is the
        // honest outcome — there is nothing to check it against.
        if staged.durable || staged.record_id.is_some() {
            return Err(refuse(path, "it is blocked and durable at once"));
        }
        return finalize(store_root, path, &month, staged.receipt_id);
    }
    if staged.status != "appended" {
        return Err(refuse(path, "its status is not one this store writes"));
    }
    let (Some(record_id), Some(digest), true) = (
        staged.record_id,
        staged.record_sha256.as_deref(),
        staged.durable,
    ) else {
        return Err(refuse(
            path,
            "it claims an append without saying what was appended",
        ));
    };
    // The writer uses the record's own id as the receipt id, so a receipt
    // naming a different record is not one this store produced.
    if record_id != staged.receipt_id {
        return Err(refuse(path, "it names a record other than its own"));
    }
    // The digest decides whether the ledger's answer is the right one, so a
    // digest that cannot be a digest makes the question unanswerable. Checked
    // before the shard is opened: otherwise a stage carrying nonsense here and
    // naming a record that is genuinely absent would come out as an ordinary
    // pre-append crash, and be quarantined as though it had been understood.
    if !canonical_digest(digest) {
        return Err(refuse(path, "the digest it states is not a sha256"));
    }

    match ledger_holds(store_root, &month, record_id, digest)? {
        Held::Once => finalize(store_root, path, &month, staged.receipt_id),
        Held::Never => {
            // The crash landed between staging and the append. Nothing is
            // durable, so no receipt may say otherwise. The file is moved aside
            // rather than deleted: it is evidence of an interrupted write, and
            // the next write is entitled to proceed once it is out of the way.
            super::abandoned::quarantine(
                store_root,
                path,
                staged.receipt_id,
                coordinate(&month, staged.receipt_id),
                &bytes,
            )
        }
        Held::Mismatched => Err(refuse(
            path,
            "the record it names is in the ledger with different contents",
        )),
    }
}

/// Where a receipt belongs: the path the store reports on a successful write.
fn coordinate(month: &str, receipt_id: Uuid) -> String {
    format!("receipts/writes/{month}/{receipt_id}.json")
}

fn finalize(store_root: &Path, path: &Path, month: &str, receipt_id: Uuid) -> Result<(), Error> {
    // The same construction the quarantine note records, so that "where this
    // receipt belongs" has one definition rather than two that agree by habit.
    let directory = crate::kernel::path::prepare(store_root, &format!("receipts/writes/{month}"))?;
    let target = crate::kernel::path::within(store_root, &coordinate(month, receipt_id))?;
    fs::rename(path, &target)?;
    crate::kernel::path::publish(&directory, crate::kernel::path::Step::Committed)?;
    crate::kernel::path::publish(
        &crate::kernel::path::within(store_root, PENDING)?,
        crate::kernel::path::Step::Drained,
    )
}

/// The month a timestamp belongs to, or nothing.
///
/// Parsed rather than sliced: a month is used to build a path, and the first
/// seven characters of an arbitrary string are not a month. Anything that is
/// not a timestamp this store could have written is refused before it names a
/// directory.
fn month_of(recorded_at: &str) -> Option<String> {
    let timestamp: jiff::Timestamp = recorded_at.parse().ok()?;
    let month = timestamp.to_string().get(..7)?.to_owned();
    let (year, rest) = month.split_at(4);
    let usable = year.chars().all(|value| value.is_ascii_digit())
        && rest.starts_with('-')
        && rest[1..].chars().all(|value| value.is_ascii_digit());
    usable.then_some(month)
}

fn refuse(path: &Path, because: &str) -> Error {
    Error::Integrity(format!(
        "an unfinished receipt must be resolved before this store accepts another write — {because}: {}",
        path.display()
    ))
}
