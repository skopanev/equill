use super::model::ContextReceipt;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub fn persist(store: &Path, receipt: &ContextReceipt) -> Result<String, Error> {
    let mut bytes = serde_json::to_vec(receipt)?;
    bytes.push(b'\n');
    let digest = sha256_hex(&bytes);
    let relative = format!("receipts/context/{digest}.json");
    let path = store.join(&relative);
    if path.exists() {
        if fs::read(&path)? != bytes {
            return Err(Error::Integrity("context receipt hash collision".into()));
        }
        return Ok(relative);
    }
    let directory = store.join("receipts/context");
    fs::create_dir_all(&directory)?;
    let temporary = directory.join(format!(".{digest}.tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(relative)
}
