use crate::kernel::error::Error;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;

pub struct StoreLock {
    file: File,
}

impl StoreLock {
    pub fn exclusive(store: &Path) -> Result<Self, Error> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(store.join("locks/writer.lock"))?;
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
