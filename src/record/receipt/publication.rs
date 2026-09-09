//! Only atomically published UUID.json files are recovery claims. A recognized
//! temporary name is uncommitted staging, never evidence of a durable append.
use super::{PENDING, WriteReceipt};
use crate::kernel::{error::Error, path};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::Path,
};

pub(super) fn stage(root: &Path, relative: &str, receipt: &WriteReceipt<'_>) -> Result<(), Error> {
    let target = path::within(root, relative)?;
    if target.exists() {
        return Err(Error::Integrity(
            "receipt stage coordinate already exists".into(),
        ));
    }
    let temporary = format!("{PENDING}/.receipt-stage-{}.tmp", uuid::Uuid::now_v7());
    let temporary = path::within(root, &temporary)?;
    let mut bytes = serde_json::to_vec(receipt)?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        #[cfg(test)]
        if INTERRUPTION.with(std::cell::Cell::get) != Interruption::None {
            file.write_all(&bytes[..bytes.len() / 2])?;
            file.sync_all()?;
            if INTERRUPTION.with(std::cell::Cell::get) == Interruption::Kill {
                std::process::abort();
            }
            return Err(Error::Io(std::io::Error::from_raw_os_error(libc::ENOSPC)));
        }
        file.write_all(&bytes)?;
        file.sync_all()?;
        // All callers hold the writer lock; an existing UUID was refused above.
        // The complete synced file replaces only this writer's absent name.
        fs::rename(&temporary, target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(super) fn clean_orphans(root: &Path) -> Result<(), Error> {
    let directory = path::within(root, PENDING)?;
    if !directory.exists() {
        return Ok(());
    }
    let mut removed = false;
    for entry in fs::read_dir(&directory)? {
        let name = path::plain_name(&entry?.path())?;
        let owned = name
            .strip_prefix(".receipt-stage-")
            .and_then(|name| name.strip_suffix(".tmp"))
            .and_then(|name| uuid::Uuid::parse_str(name).ok())
            .is_some_and(|id| id.get_version() == Some(uuid::Version::SortRand));
        if owned {
            // Linked/nonregular temporary paths are refused, never followed.
            fs::remove_file(path::file_within(root, &format!("{PENDING}/{name}"))?)?;
            removed = true;
        }
    }
    if removed {
        File::open(directory)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Interruption {
    None,
    Error,
    Kill,
}
#[cfg(test)]
thread_local! { static INTERRUPTION: std::cell::Cell<Interruption> = const { std::cell::Cell::new(Interruption::None) }; }
#[cfg(test)]
pub(crate) fn interrupt(value: Interruption) {
    INTERRUPTION.with(|slot| slot.set(value));
}
