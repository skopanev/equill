//! What a compaction wrote down before it started moving directories.
//!
//! The journal states an intent, not a fact. A phase is recorded after the
//! rename it describes, so there is always a window where the directory has
//! already moved and the journal has not caught up — which means recovery can
//! never trust the phase alone. It reads the directories and uses the journal
//! to know what was being attempted and with which paths.
//!
//! It is a step in a transaction, not a record of anything: it names local
//! paths, the transaction, the phase, and the ids whose points still have to
//! go. It is deleted once the work is done.
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const JOURNAL: &str = "compact-journal.json";

/// The order the directories are published in. Recovery walks the same list.
pub const STEPS: [&str; 2] = ["records", "receipts/writes"];

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Everything is prepared beside the store and nothing has moved.
    Staged,
    /// The ledger is published; the receipts may or may not be.
    RecordsSwapped,
    /// Both directories are published; the projections are not yet agreed.
    ReceiptsSwapped,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Journal {
    pub transaction: String,
    pub shadow: PathBuf,
    pub phase: Phase,
    /// The points that still have to be dropped. After the swap the ledger no
    /// longer names these records, so this is the only thing that can.
    pub condemned: Vec<Uuid>,
    /// What the live directories held when the staged copy was built, by path
    /// and length.
    ///
    /// The store keeps taking writes the moment the process dies, and an append
    /// lands in the directory that is still current. Publishing the prepared
    /// copy over it would carry that record away in the backup. The staged copy
    /// cannot answer this by itself, since compaction makes it shorter on
    /// purpose, so what it replaced is measured here.
    pub source: BTreeMap<String, u64>,
}

/// The lengths of every file under one directory, keyed by path relative to the
/// store, so the same map can be compared against the same directory later.
pub fn measure(store_root: &Path, relative: &str) -> Result<BTreeMap<String, u64>, Error> {
    let mut sizes = BTreeMap::new();
    walk(store_root, &store_root.join(relative), &mut sizes)?;
    Ok(sizes)
}

fn walk(store_root: &Path, at: &Path, sizes: &mut BTreeMap<String, u64>) -> Result<(), Error> {
    if !at.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(at)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(store_root, &path, sizes)?;
        } else if let Ok(key) = path.strip_prefix(store_root) {
            sizes.insert(
                key.to_string_lossy().into_owned(),
                fs::metadata(&path)?.len(),
            );
        }
    }
    Ok(())
}

impl Journal {
    /// Written after staging succeeds and before the first rename, durably: a
    /// journal that is not on disk when the process dies is a journal that was
    /// never written.
    pub fn write(&self, store_root: &Path) -> Result<(), Error> {
        let path = store_root.join(JOURNAL);
        let staging = store_root.join(format!(".{JOURNAL}.{}", self.transaction));
        let file = fs::File::create(&staging)?;
        serde_json::to_writer(&file, self)?;
        file.sync_all()?;
        fs::rename(&staging, &path)?;
        sync_directory(store_root)?;
        Ok(())
    }

    /// Reads the journal and checks that its paths are the ones this store
    /// would have produced.
    ///
    /// The paths travel through a JSON file, and they are used for renames and
    /// recursive deletes. A journal naming somewhere else — corrupted, copied
    /// from another store, or written by something that is not this — must not
    /// be able to point those operations at it. The transaction name is
    /// checked, and the staged path is recomputed from it rather than taken.
    pub fn read(store_root: &Path) -> Result<Option<Self>, Error> {
        let bytes = match fs::read(store_root.join(JOURNAL)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let journal: Self = serde_json::from_slice(&bytes)?;
        if journal.transaction.len() != 32
            || !journal
                .transaction
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return Err(Error::Compact(
                "compaction journal names a transaction this store did not write".into(),
            ));
        }
        let expected =
            super::super::transaction::sibling(store_root, "native", &journal.transaction)?;
        if journal.shadow != expected {
            return Err(Error::Compact(
                "compaction journal points outside the store it was found in".into(),
            ));
        }
        Ok(Some(journal))
    }

    pub fn advance(&mut self, store_root: &Path, phase: Phase) -> Result<(), Error> {
        self.phase = phase;
        self.write(store_root)
    }

    pub fn clear(store_root: &Path) -> Result<(), Error> {
        match fs::remove_file(store_root.join(JOURNAL)) {
            Ok(()) => sync_directory(store_root),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            // A cleanup that failed is not a completed purge, and saying so is
            // the difference between a finished transaction and one that looks
            // finished.
            Err(error) => Err(error.into()),
        }
    }
}

/// A rename is only durable once the directory holding the name is.
pub fn sync_directory(path: &Path) -> Result<(), Error> {
    let directory = fs::File::open(path)?;
    directory.sync_all()?;
    Ok(())
}

// Where a test asks the compaction to stop. Compiled out of a release build.
#[cfg(test)]
thread_local! {
    static INTERRUPT: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn interrupting(point: &str) -> bool {
    INTERRUPT.with(|slot| slot.borrow().as_deref() == Some(point))
}

#[cfg(test)]
pub(super) fn interrupt_at(point: &str) -> Result<(), Error> {
    if interrupting(point) {
        return Err(crate::kernel::error::Error::Compact(format!(
            "interrupted at {point}"
        )));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn with_interrupt<T>(point: &str, body: impl FnOnce() -> T) -> T {
    struct Restore(Option<String>);
    impl Drop for Restore {
        fn drop(&mut self) {
            INTERRUPT.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(INTERRUPT.with(|slot| slot.borrow_mut().take()));
    INTERRUPT.with(|slot| *slot.borrow_mut() = Some(point.to_owned()));
    body()
}

/// Where a test child is asked to stop dead.
///
/// Compiled out of the product entirely: a release binary must not carry a
/// switch that ends a compaction halfway, however useful it is to a test. The
/// crash test runs the test binary as its own child, which is built with this.
#[cfg(test)]
pub(super) fn asked_to_halt(point: &str) -> bool {
    std::env::var("EQUILL_TEST_COMPACT_HALT").ok().as_deref() == Some(point)
}

/// Death, not an error: an error unwinds and puts things back, a crash does
/// neither.
#[cfg(test)]
pub(super) fn halt_if_asked(point: &str) {
    if asked_to_halt(point) {
        std::process::abort();
    }
}
