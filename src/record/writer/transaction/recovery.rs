use super::{DATA, Journal, MARKER, Phase, damaged, storage};
use crate::kernel::{error::Error, path};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub(in crate::record::writer) fn recover(root: &Path) -> Result<(), Error> {
    if !path::within(root, MARKER)?.exists() {
        return clean_orphans(root);
    }
    let bytes = fs::read(path::file_within(root, MARKER)?)?;
    let mut journal: Journal = serde_json::from_slice(&bytes).map_err(|_| damaged())?;
    if journal.schema != "equill.atomic-batch.v1"
        || journal.records.is_empty()
        || journal.id.get_version() != Some(uuid::Version::SortRand)
    {
        return Err(damaged());
    }
    let ledger = path::within(root, &journal.ledger)?;
    let mut tail = Vec::new();
    if ledger.exists() {
        let mut file = File::open(path::file_within(root, &journal.ledger)?)?;
        if file.metadata()?.len() < journal.offset {
            return Err(damaged());
        }
        file.seek(SeekFrom::Start(journal.offset))?;
        file.read_to_end(&mut tail)?;
    } else if journal.offset != 0 {
        return Err(damaged());
    }
    if journal.phase == Phase::Aborted {
        if !tail.is_empty() {
            return Err(damaged());
        }
        // A previous recovery may have removed some staged receipts already.
        for coordinate in &journal.records {
            let pending = format!("receipts/pending/{}.json", coordinate.id);
            if path::within(root, &coordinate.receipt)?.exists() {
                return Err(damaged());
            }
            if path::within(root, &pending)?.exists() {
                let bytes = fs::read(path::file_within(root, &pending)?)?;
                if crate::kernel::digest::sha256_hex(&bytes) != coordinate.receipt_sha256 {
                    return Err(damaged());
                }
                storage::remove(root, &pending)?;
            }
        }
        return journal.clear(root);
    }
    if journal.phase == Phase::Committed {
        journal.verify(root, &tail)?;
        journal.receipts(root, true)?;
        journal.publish_target(root)?;
        return journal.clear(root);
    }
    let staged = fs::read(path::file_within(root, &journal.data())?)?;
    journal.verify(root, &staged)?;
    // Only this exact staged prefix is eligible to be rolled back. An unrelated
    // or corrupt tail remains untouched and blocks every subsequent writer.
    if !staged.starts_with(&tail) {
        return Err(damaged());
    }
    if tail.len() == staged.len() {
        File::open(&ledger)?.sync_all()?;
        File::open(path::within(root, "records")?)?.sync_all()?;
        journal.receipts(root, true)?;
        journal.committed(root)
    } else {
        if ledger.exists() {
            let file = OpenOptions::new().write(true).open(&ledger)?;
            file.set_len(journal.offset)?;
            file.sync_all()?;
        }
        journal.phase = Phase::Aborted;
        storage::publish(root, MARKER, &journal)?;
        journal.receipts(root, false)?;
        journal.clear(root)
    }
}

fn clean_orphans(root: &Path) -> Result<(), Error> {
    storage::clean_temporary(root, "transactions")?;
    let directory = path::within(root, DATA)?;
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let name = path::plain_name(&entry?.path())?;
        let valid = name
            .strip_suffix(".jsonl")
            .and_then(|stem| uuid::Uuid::parse_str(stem).ok())
            .is_some_and(|id| id.get_version() == Some(uuid::Version::SortRand));
        if !valid {
            return Err(damaged());
        }
        storage::remove(root, &format!("{DATA}/{name}"))?;
    }
    Ok(())
}
