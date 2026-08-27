use super::model::CompactReceipt;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct StagedReceipt {
    temporary: Option<PathBuf>,
    final_path: PathBuf,
    relative: String,
}

impl StagedReceipt {
    pub fn relative(&self) -> &str {
        &self.relative
    }

    pub fn commit(mut self) -> Result<(), Error> {
        if let Some(temporary) = self.temporary.take() {
            fs::rename(temporary, &self.final_path)?;
        }
        Ok(())
    }
}

impl Drop for StagedReceipt {
    fn drop(&mut self) {
        if let Some(temporary) = &self.temporary {
            let _ = fs::remove_file(temporary);
        }
    }
}

pub fn stage(store: &Path, receipt: &CompactReceipt) -> Result<StagedReceipt, Error> {
    let mut bytes = serde_json::to_vec(receipt)?;
    bytes.push(b'\n');
    let digest = sha256_hex(&bytes);
    let directory = store.join("receipts/compacts");
    fs::create_dir_all(&directory)?;
    let relative = format!("receipts/compacts/{digest}.json");
    let final_path = store.join(&relative);
    if final_path.exists() {
        if fs::read(&final_path)? != bytes {
            return Err(Error::Integrity("compact receipt hash collision".into()));
        }
        return Ok(StagedReceipt {
            temporary: None,
            final_path,
            relative,
        });
    }
    let temporary = directory.join(format!(".{digest}.tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(StagedReceipt {
        temporary: Some(temporary),
        final_path,
        relative,
    })
}
