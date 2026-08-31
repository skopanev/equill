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
pub(crate) fn within(root: &Path, relative: &str) -> Result<PathBuf, Error> {
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
pub(crate) fn file_within(root: &Path, relative: &str) -> Result<PathBuf, Error> {
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
pub(crate) fn plain_name(entry: &Path) -> Result<String, Error> {
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

/// Walk, create, walk again.
///
/// The order is the whole point. Walking only after creating cannot protect an
/// ANCESTOR: by then `create_dir_all` has already used it, and it is content
/// with a link that resolves to a directory. Walking only before cannot protect
/// against a swap during the create. Both, and the window closes.
///
/// The first walk is what stops a directory being made outside the store on a
/// path that is about to be refused — a refusal with a side effect is still a
/// side effect.
pub(crate) fn prepare(root: &Path, relative: &str) -> Result<PathBuf, Error> {
    within(root, relative)?;
    fs::create_dir_all(root.join(relative))?;
    within(root, relative)
}

/// Make a directory's contents — its names, not its files — survive a crash.
///
/// A renamed or created file whose directory was never synced can be gone
/// after a power loss while whatever it replaced is also gone. Every step that
/// something later depends on is published before that something happens.
pub(crate) fn publish(directory: &Path, step: Step) -> Result<(), Error> {
    note(step);
    fs::File::open(directory)?.sync_all()?;
    failure(step)
}

/// The durability steps, in the order a write performs them.
///
/// Named rather than counted so a test can assert the ORDER, which is the part
/// that matters: a rename published after the thing that depends on it is not
/// published at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The name of the staging directory, the first time a store has one.
    PendingCreated,
    /// The staged receipt's own name, before the ledger holds the record it
    /// describes. A crash after the append with no stage on disk leaves a
    /// durable record that recovery has nothing to finish.
    Staged,
    /// The name of a month directory that did not exist before this write.
    ///
    /// Published before the receipt inside it, because a receipt whose month
    /// directory did not survive is a receipt that did not survive — while the
    /// removal of its stage, published afterwards, would have. That leaves a
    /// durable record with neither a receipt nor the stage to rebuild one from,
    /// which is the outcome the whole mechanism exists to prevent.
    MonthCreated,
    /// The receipt at its final coordinate.
    Committed,
    /// The staging directory, after the receipt left it.
    Drained,
}

#[cfg(test)]
pub(crate) use seam::{fail, reset, steps};

fn note(step: Step) {
    #[cfg(test)]
    seam::note(step);
    #[cfg(not(test))]
    let _ = step;
}

fn failure(step: Step) -> Result<(), Error> {
    #[cfg(test)]
    if seam::failing(step) {
        return Err(Error::Integrity(format!(
            "a directory could not be published: {step:?}"
        )));
    }
    #[cfg(not(test))]
    let _ = step;
    Ok(())
}

#[cfg(test)]
mod seam {
    use super::Step;
    use std::cell::RefCell;

    thread_local! {
        static STEPS: RefCell<Vec<Step>> = const { RefCell::new(Vec::new()) };
        static FAILING: RefCell<Option<Step>> = const { RefCell::new(None) };
    }

    pub(super) fn note(step: Step) {
        STEPS.with(|steps| steps.borrow_mut().push(step));
    }

    pub(super) fn failing(step: Step) -> bool {
        FAILING.with(|failing| *failing.borrow() == Some(step))
    }

    pub(crate) fn reset() {
        STEPS.with(|steps| steps.borrow_mut().clear());
        FAILING.with(|failing| *failing.borrow_mut() = None);
    }

    pub(crate) fn steps() -> Vec<Step> {
        STEPS.with(|steps| steps.borrow().clone())
    }

    /// Make one publication fail, to prove success is not reported before it.
    pub(crate) fn fail(step: Step) {
        FAILING.with(|failing| *failing.borrow_mut() = Some(step));
    }
}
