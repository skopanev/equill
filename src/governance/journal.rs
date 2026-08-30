use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Durable authority state, deliberately NOT under `projections/`: everything
/// there is rebuildable from the ledger, and this is not — it is the only
/// record of a transaction that was decided but may not have landed.
const JOURNAL: &str = "governance/pending.json";

/// A governance change that has been decided but may not yet have landed.
///
/// The exact bytes are kept, not just their digest: recovery has to be able to
/// finish the commit, and re-deriving the metadata later could produce a
/// different serialization than the one the audit record attested.
#[derive(Deserialize, Serialize)]
pub(super) struct Pending {
    pub(super) tx_id: Uuid,
    pub(super) action: String,
    pub(super) subject: String,
    pub(super) before_sha256: String,
    pub(super) after_sha256: String,
    /// Store metadata is JSON, so the bytes are kept as the text they are
    /// rather than an encoding of it — the journal stays readable by a person
    /// looking at an interrupted store.
    pub(super) after_bytes: String,
}

impl Pending {
    /// Only tests need this: a journal whose bytes were swapped while its
    /// declared digests were left alone, which is exactly the forgery recovery
    /// has to catch.
    #[cfg(test)]
    pub(super) fn tampered_with(&self, after_bytes: &str) -> Self {
        Self {
            tx_id: self.tx_id,
            action: self.action.clone(),
            subject: self.subject.clone(),
            before_sha256: self.before_sha256.clone(),
            after_sha256: self.after_sha256.clone(),
            after_bytes: after_bytes.to_owned(),
        }
    }
}

pub(super) fn path(store_root: &Path) -> PathBuf {
    store_root.join(JOURNAL)
}

pub(super) fn read(store_root: &Path) -> Result<Option<Pending>, Error> {
    let path = path(store_root);
    if !path.is_file() {
        return Ok(None);
    }
    serde_json::from_slice(&fs::read(path)?)
        .map(Some)
        .map_err(|_| Error::Integrity("governance journal is unreadable".into()))
}

/// Written before the audit record, so every crash after this point is one the
/// recovery contract can name.
pub(super) fn write(store_root: &Path, pending: &Pending) -> Result<(), Error> {
    let path = path(store_root);
    let directory = path
        .parent()
        .ok_or_else(|| Error::Integrity("governance journal path is invalid".into()))?;
    fs::create_dir_all(directory)?;
    let temporary = directory.join(format!(".pending-{}.json", pending.tx_id));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let bytes = serde_json::to_vec(pending)?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(Error::Io(error));
    }
    drop(file);
    fs::rename(&temporary, &path)?;
    File::open(directory).and_then(|handle| handle.sync_all())?;
    Ok(())
}

pub(super) fn clear(store_root: &Path) -> Result<(), Error> {
    let path = path(store_root);
    if path.is_file() {
        fs::remove_file(&path)?;
        if let Some(directory) = path.parent() {
            File::open(directory).and_then(|handle| handle.sync_all())?;
        }
    }
    Ok(())
}
