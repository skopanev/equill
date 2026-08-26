use super::{AppendReport, RecordDraft, StoredRecord};
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::kernel::identity;
use crate::kernel::lock::StoreLock;
use crate::kernel::store;
use crate::schema;
use jiff::Timestamp;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use uuid::Uuid;

pub fn append_file(store_root: &Path, source: &Path, actor: &str) -> Result<AppendReport, Error> {
    let draft: RecordDraft = serde_json::from_slice(&fs::read(source)?)?;
    append(store_root, draft, actor)
}

pub fn append(store_root: &Path, draft: RecordDraft, actor: &str) -> Result<AppendReport, Error> {
    let config = store::load(store_root)?;
    identity::require_root(&config, actor)?;
    let definition = schema::load(store_root, &draft.type_name)?;
    super::validation::validate(&draft, &config, &definition)?;

    let recorded_at = Timestamp::now().to_string();
    let month = recorded_at
        .get(..7)
        .ok_or_else(|| Error::InvalidRecord("system clock is out of range".into()))?
        .to_owned();
    let valid_at = draft
        .valid_at
        .clone()
        .unwrap_or_else(|| draft.observed_at.clone());
    let record = StoredRecord {
        id: Uuid::now_v7(),
        namespace: draft.namespace,
        type_name: draft.type_name,
        actor: actor.to_owned(),
        recorded_at,
        observed_at: draft.observed_at,
        valid_at,
        payload: draft.payload,
        evidence: draft.evidence,
        tags: draft.tags,
        supersedes: draft.supersedes,
    };
    let mut line = serde_json::to_vec(&record)?;
    let digest = sha256_hex(&line);
    line.push(b'\n');
    let ledger = format!("records/{month}.jsonl");
    let path = store_root.join(&ledger);

    let _lock = StoreLock::exclusive(store_root)?;
    ensure_clean_tail(&path)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(&line)?;
    file.sync_data()?;

    Ok(AppendReport {
        ok: true,
        id: record.id,
        sha256: digest,
        ledger,
    })
}

fn ensure_clean_tail(path: &Path) -> Result<(), Error> {
    if !path.exists() || path.metadata()?.len() == 0 {
        return Ok(());
    }
    let mut file = OpenOptions::new().read(true).open(path)?;
    file.seek(SeekFrom::End(-1))?;
    let mut tail = [0_u8; 1];
    file.read_exact(&mut tail)?;
    if tail[0] != b'\n' {
        return Err(Error::Integrity(format!(
            "ledger has an incomplete final line: {}",
            path.display()
        )));
    }
    Ok(())
}
