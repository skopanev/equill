//! Capture committed ledger descriptors and byte bounds; never hold a lock
//! while parsing, hashing, indexing, or embedding their contents.
use crate::kernel::{error::Error, lock::ReadLock};
use std::fs::{self, File};
use std::io::{Read, Take};
use std::path::Path;

pub(crate) struct Shard {
    pub name: String,
    pub reader: Take<File>,
}

pub(crate) struct Snapshot {
    pub shards: Vec<Shard>,
    pub bytes: u64,
}

pub(crate) fn capture(root: &Path, names: Option<&[&str]>) -> Result<Snapshot, Error> {
    let lock = ReadLock::acquire(root)?;
    let snapshot = capture_exclusive(root, names)?;
    if lock.is_none() && root.join("locks/writer.lock").try_exists()? {
        // The first canonical writer creates this persistent file before it
        // touches the ledger. Recapture once if it appeared during our opens.
        let _lock = ReadLock::acquire(root)?.ok_or_else(|| {
            Error::Integrity("writer lock disappeared during snapshot capture".into())
        })?;
        return capture_exclusive(root, names);
    }
    Ok(snapshot)
}

/// Caller already owns writer.lock. Recovery must have finished first; even
/// this path refuses unresolved journal bytes instead of exposing a prefix.
pub(crate) fn capture_exclusive(root: &Path, names: Option<&[&str]>) -> Result<Snapshot, Error> {
    ensure_settled(root)?;
    let directory = super::located::open::directory(root)?;
    let names = match names {
        Some(names) => names.iter().map(|name| (*name).to_owned()).collect(),
        None => ledger_names(root)?,
    };
    let mut shards = Vec::new();
    let mut bytes = 0;
    for name in names {
        let file = super::located::open::shard(&directory, &name)?;
        let length = file.metadata()?.len();
        bytes += length;
        shards.push(Shard {
            name,
            reader: file.take(length),
        });
    }
    Ok(Snapshot { shards, bytes })
}

/// Also used by existing writer-locked raw diagnostics, before they read rows.
pub(crate) fn ensure_settled(root: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(root.join("transactions/batch.json")) {
        Ok(_) => {
            return Err(Error::Integrity(
                "write recovery pending; committed snapshot unavailable".into(),
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn ledger_names(root: &Path) -> Result<Vec<String>, Error> {
    let mut names = Vec::new();
    for entry in fs::read_dir(root.join("records"))? {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| Error::Integrity("invalid record ledger name".into()))?;
        if !name.ends_with(".jsonl") {
            return Err(Error::Integrity("unexpected record ledger entry".into()));
        }
        names.push(name);
    }
    Ok(super::verify::in_month_order(names))
}

#[cfg(test)]
type AfterCapture = fn(&Path);
#[cfg(test)]
thread_local! {
    static AFTER_CAPTURE: std::cell::Cell<Option<AfterCapture>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn after_capture(root: &Path) {
    if let Some(action) = AFTER_CAPTURE.with(std::cell::Cell::take) {
        action(root);
    }
}

#[cfg(test)]
pub(crate) fn with_after_capture<T>(action: AfterCapture, body: impl FnOnce() -> T) -> T {
    let _restore = crate::vector::catchup::seam::Restore::install(&AFTER_CAPTURE, action);
    body()
}
