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

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}
