//! Descriptor-relative opens keep every internal component inside the store,
//! even if an ancestor is replaced with a symlink between validation and open.
use super::failure;
use crate::kernel::error::Error;
use std::ffi::CStr;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

pub(crate) fn directory(root: &Path) -> Result<File, Error> {
    let root = File::open(root).map_err(|_| failure("store cannot be opened"))?;
    at(&root, c"records", libc::O_DIRECTORY)
}

pub(crate) fn shard(directory: &File, name: &str) -> Result<File, Error> {
    let name = std::ffi::CString::new(name).map_err(|_| failure("invalid ledger path"))?;
    let file = at(directory, &name, 0)?;
    let metadata = file
        .metadata()
        .map_err(|_| failure("ledger metadata unavailable"))?;
    if !metadata.is_file() || metadata.nlink() != 1 {
        return Err(failure("ledger must be a confined regular file"));
    }
    Ok(file)
}

fn at(parent: &File, name: &CStr, flags: i32) -> Result<File, Error> {
    // SAFETY: parent remains open, name is NUL-terminated, and a successful
    // descriptor is transferred exactly once to File's ownership below.
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
        )
    };
    if descriptor < 0 {
        return Err(failure("ledger path is missing, linked, or unreadable"));
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}
