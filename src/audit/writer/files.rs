//! All audit child files are regular, private, single-link, and never followed
//! through a symlink. Check the opened inode before any truncation or append.
use super::failure;
use crate::kernel::error::Error;
use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

pub(in crate::audit) fn open(path: &Path, create: bool) -> Result<File, Error> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(failure());
    }
    Ok(file)
}

pub(in crate::audit) fn remove(path: &Path) -> Result<(), Error> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => {
            drop(open(path, false)?);
            fs::remove_file(path)?;
            Ok(())
        }
    }
}
