//! Where this store is allowed to write, proved component by component.
//!
//! Checking that a leaf is a regular file proves nothing about how it was
//! reached. `receipts/pending` can itself be a link, or `receipts`, or the
//! month directory a receipt is renamed into — and then every leaf inside is
//! an ordinary file that lives somewhere else entirely. Recovery renames and
//! deletes, so a directory that points outside the store turns those into
//! renames and deletions outside the store.
//!
//! So no path here is built by joining and hoping. Each component is walked
//! and checked as it is added, and a component that is a link ends the walk.
//! Nothing is canonicalized, because canonicalizing follows the links this
//! exists to refuse.
use crate::kernel::error::Error;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// A path inside the store, or an error.
///
/// Components that do not exist yet are allowed — they are about to be created
/// — but a component that exists and is a link is refused, and so is any
/// component that could climb out by name.
pub(super) fn within(root: &Path, relative: &str) -> Result<PathBuf, Error> {
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(escapes(relative));
        }
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(data) if data.file_type().is_symlink() => return Err(escapes(relative)),
            Ok(_) => {}
            // Not there yet is fine. Anything else is a directory that exists
            // and cannot be examined, which is not a directory this store can
            // vouch for.
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

/// The same walk, and the leaf must be a regular file this store alone holds.
///
/// The link count is checked as well as the type, because a hard link is not a
/// link as far as `symlink_metadata` is concerned — it is an ordinary file, and
/// the same file, reachable under a name outside the store. A staged receipt
/// this store wrote has exactly one name. Anything with more was given a second
/// one by somebody else, and its contents are not this store's to act on.
pub(super) fn file_within(root: &Path, relative: &str) -> Result<PathBuf, Error> {
    let path = within(root, relative)?;
    let data = fs::symlink_metadata(&path)?;
    if !data.is_file() {
        return Err(Error::Integrity(format!(
            "this store expects a regular file and found something else: {relative}"
        )));
    }
    if std::os::unix::fs::MetadataExt::nlink(&data) != 1 {
        return Err(Error::Integrity(format!(
            "a staged receipt is reachable under a name outside this store: {relative}"
        )));
    }
    Ok(path)
}

/// The name of an entry in a directory this store owns, with no path in it.
///
/// A directory listing hands back names, and a name containing a separator is
/// not a name. Callers turn these into paths, so the check belongs before that.
pub(super) fn plain_name(entry: &Path) -> Result<String, Error> {
    let name = entry
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| escapes(&entry.to_string_lossy()))?;
    if name.contains('/') || name == "." || name == ".." {
        return Err(escapes(name));
    }
    Ok(name.to_owned())
}

fn escapes(relative: &str) -> Error {
    Error::Integrity(format!(
        "a path this store must own leads outside it and is refused: {relative}"
    ))
}
