use crate::kernel::error::Error;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;

pub struct StoreLock {
    file: File,
}

impl StoreLock {
    pub fn exclusive(store: &Path) -> Result<Self, Error> {
        Self::named(store, "writer.lock")
    }

    /// A lock on one named file under `locks/`. Governance holds its own for a
    /// whole transaction while the writer lock is taken and released several
    /// times inside it, which only works because they are different files.
    pub fn named(store: &Path, name: &str) -> Result<Self, Error> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(store.join("locks").join(name))?;
        file.lock_exclusive()?;
        Ok(Self { file })
    }
}

/// A lock a caller may decline to wait for. The vector drain uses it so a
/// second writer never queues behind an embedding run: it commits, records what
/// it wants indexed, and leaves the catch-up to whoever already holds this.
pub struct TryLock {
    file: File,
}

impl TryLock {
    /// `None` means somebody else is already doing the work.
    pub fn acquire(store: &Path, name: &str) -> Result<Option<Self>, Error> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(store.join("locks").join(name))?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self { file })),
            Err(_) => Ok(None),
        }
    }

    /// Releasing before the writer lock is dropped is what closes the lost
    /// wake-up: a writer that has already committed will find this free.
    pub fn release(self) {}
}

impl Drop for TryLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod probe {
    use super::TryLock;

    /// Does this lock exclude a second acquisition inside the same process?
    /// The answer decides whether the drain claim can work at all.
    #[test]
    fn a_second_acquisition_in_this_process_is_refused() {
        let root = std::env::temp_dir().join(format!("equill-lockprobe-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(root.join("locks")).expect("locks");
        let first = TryLock::acquire(&root, "probe.lock")
            .expect("acquire")
            .expect("free");
        let second = TryLock::acquire(&root, "probe.lock").expect("acquire");
        let excluded = second.is_none();
        drop(first);
        std::fs::remove_dir_all(&root).ok();
        assert!(
            excluded,
            "same-process re-acquisition succeeded; the claim cannot rely on it"
        );
    }
}
