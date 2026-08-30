use crate::defense::DefenseFinding;
use crate::kernel::error::Error;
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WriteStatus {
    Appended,
    BlockedByMemoryDefense,
}

#[derive(Debug, Serialize)]
pub struct WriteReceipt<'a> {
    pub receipt_id: Uuid,
    pub status: WriteStatus,
    pub record_id: Option<Uuid>,
    pub namespace: &'a str,
    #[serde(rename = "type")]
    pub type_name: &'a str,
    pub actor: &'a str,
    pub recorded_at: &'a str,
    pub record_sha256: Option<&'a str>,
    /// The canonical claim, written into the receipt so it survives the process
    /// that made it: this record is in the immutable ledger. It says nothing
    /// about the index, which is reported beside it and separately.
    pub durable: bool,
    /// Where the vector projection stood when the record was written. A receipt
    /// that only recorded durability left a reader unable to tell a store that
    /// was current from one that was still catching up.
    pub projection: crate::vector::Projection,
    pub defense_findings: &'a [DefenseFinding],
}

pub struct StagedReceipt {
    temporary: PathBuf,
    final_path: PathBuf,
    relative: String,
    committed: bool,
}

impl StagedReceipt {
    pub fn relative(&self) -> &str {
        &self.relative
    }

    pub fn commit(mut self) -> Result<(), Error> {
        fs::rename(&self.temporary, &self.final_path)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for StagedReceipt {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.temporary);
        }
    }
}

pub fn stage(
    store_root: &Path,
    month: &str,
    receipt: &WriteReceipt<'_>,
) -> Result<StagedReceipt, Error> {
    let directory = store_root.join("receipts/writes").join(month);
    fs::create_dir_all(&directory)?;
    let relative = format!("receipts/writes/{month}/{}.json", receipt.receipt_id);
    let final_path = store_root.join(&relative);
    let temporary = directory.join(format!(
        ".{}.tmp-{}",
        receipt.receipt_id,
        std::process::id()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    serde_json::to_writer(&mut file, receipt)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(StagedReceipt {
        temporary,
        final_path,
        relative,
        committed: false,
    })
}
