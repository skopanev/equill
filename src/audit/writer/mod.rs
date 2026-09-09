mod directory;
pub(super) mod files;
mod reservation;
mod transaction;

#[cfg(test)]
use crate::audit::Event;
use crate::kernel::error::Error;
use std::fs::{self, File};
use std::path::Path;

pub(in crate::audit) use directory::Directory;
pub(super) use reservation::Reservation;

pub(in crate::audit) struct Locked {
    pub(in crate::audit) root: Directory,
    _lease: File,
}

pub(super) fn failure() -> Error {
    Error::Audit("request audit storage is unavailable or inconsistent".into())
}

pub(super) fn check_root(root: &Path) -> Result<(), Error> {
    let resolved = if root.exists() {
        fs::canonicalize(root)?
    } else {
        root.to_path_buf()
    };
    if resolved
        .ancestors()
        .any(|path| path.join("store.json").exists())
    {
        return Err(failure());
    }
    Ok(())
}

pub(super) fn lock(root: &Path) -> Result<Locked, Error> {
    let root = Directory::open(root)?;
    let lease = root.lock()?;
    Ok(Locked {
        root,
        _lease: lease,
    })
}

#[cfg(test)]
pub(super) fn append(root: &Path, event: &Event) -> Result<(), Error> {
    let lock = lock(root)?;
    recover(&lock.root)?;
    transaction::append(&lock.root, event, None)
}

pub(super) fn recover(root: &Directory) -> Result<(), Error> {
    transaction::recover(root)?;
    reservation::recover(root)
}

pub(super) fn month(time: &str) -> Result<String, Error> {
    let canonical = time
        .parse::<jiff::Timestamp>()
        .map_err(|_| failure())?
        .to_string();
    Ok(canonical[..7].into())
}
