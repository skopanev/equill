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

    pub fn read(store_root: &Path) -> Result<Option<Self>, Error> {
        match fs::read(store_root.join(JOURNAL)) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
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
