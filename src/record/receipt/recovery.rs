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
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use serde::Deserialize;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use uuid::Uuid;

const ABANDONED: &str = "receipts/abandoned";

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
    let directory = store_root.join(PENDING);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        // Only absence means there is nothing pending. A directory that exists
        // and cannot be read is not an empty one, and reading it as empty would
        // let a write proceed over an unresolved transaction it never saw.
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        resolve_one(store_root, &entry?.path())?;
    }
    Ok(())
}

fn resolve_one(store_root: &Path, path: &Path) -> Result<(), Error> {
    let staged: Pending = serde_json::from_slice(&fs::read(path)?)
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

    match ledger_holds(store_root, &month, record_id, digest)? {
        Held::Once => finalize(store_root, path, &month, staged.receipt_id),
        Held::Never => {
            // The crash landed between staging and the append. Nothing is
            // durable, so no receipt may say otherwise. The file is moved aside
            // rather than deleted: it is evidence of an interrupted write, and
            // the next write is entitled to proceed once it is out of the way.
            quarantine(store_root, path, staged.receipt_id)
        }
        Held::Mismatched => Err(refuse(
            path,
            "the record it names is in the ledger with different contents",
        )),
        Held::Twice => Err(refuse(path, "the record it names is in the ledger twice")),
    }
}

enum Held {
    Once,
    Never,
    Twice,
    Mismatched,
}

/// Ask the one ledger shard the receipt names, and no other.
fn ledger_holds(
    store_root: &Path,
    month: &str,
    record_id: Uuid,
    digest: &str,
) -> Result<Held, Error> {
    let path = store_root.join("records").join(format!("{month}.jsonl"));
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Held::Never),
        Err(error) => return Err(error.into()),
    };
    let wanted = record_id.to_string();
    let mut found = 0;
    let mut matched = 0;
    for line in contents.lines() {
        let Ok(record) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if record["id"].as_str() != Some(wanted.as_str()) {
            continue;
        }
        found += 1;
        // The digest is taken over the serialized record exactly as the writer
        // hashed it: the ledger line without its newline.
        if sha256_hex(line.as_bytes()) == digest {
            matched += 1;
        }
    }
    Ok(match (found, matched) {
        (0, _) => Held::Never,
        (1, 1) => Held::Once,
        (1, _) => Held::Mismatched,
        _ => Held::Twice,
    })
}

fn finalize(store_root: &Path, path: &Path, month: &str, receipt_id: Uuid) -> Result<(), Error> {
    let target = store_root
        .join("receipts/writes")
        .join(month)
        .join(format!("{receipt_id}.json"));
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(fs::rename(path, &target)?)
}

fn quarantine(store_root: &Path, path: &Path, receipt_id: Uuid) -> Result<(), Error> {
    let directory = store_root.join(ABANDONED);
    fs::create_dir_all(&directory)?;
    Ok(fs::rename(
        path,
        directory.join(format!("{receipt_id}.json")),
    )?)
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
