use super::{Directory, failure, month};
use crate::audit::Event;
use crate::kernel::{digest::sha256_hex, error::Error};
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom, Write};

const MAX_EVENT: usize = 8192;

#[derive(Deserialize, Serialize)]
struct Pending {
    month: String,
    offset: u64,
    sha256: String,
    line: String,
    #[serde(default)]
    reservation: Option<uuid::Uuid>,
}

pub(super) fn append(
    root: &Directory,
    event: &Event,
    reservation: Option<uuid::Uuid>,
) -> Result<(), Error> {
    if !event.valid() {
        return Err(failure());
    }
    recover(root)?;
    let month = month(&event.observed_at)?;
    let mut line = serde_json::to_string(event)?;
    line.push('\n');
    if line.len() > MAX_EVENT {
        return Err(failure());
    }
    let ledger = format!("{month}.jsonl");
    let mut file = root.file(&ledger, true)?;
    let offset = file.metadata()?.len();
    if offset > 0 {
        file.seek(SeekFrom::End(-1))?;
        let mut tail = [0];
        file.read_exact(&mut tail)?;
        if tail != *b"\n" {
            return Err(failure());
        }
    }
    let pending = Pending {
        month,
        offset,
        sha256: sha256_hex(line.as_bytes()),
        line,
        reservation,
    };
    let mut file = root.file("pending.tmp", true)?;
    file.set_len(0)?;
    file.write_all(&serde_json::to_vec(&pending)?)?;
    file.sync_all()?;
    root.rename("pending.tmp", "pending.json")?;
    root.sync()?;
    recover(root)
}

/// The pending journal survives a partial append or a crash after fsync. Its
/// prefix must match the ledger exactly; recovery only appends missing bytes.
pub(super) fn recover(root: &Directory) -> Result<(), Error> {
    if !root.exists("pending.json")? {
        return Ok(());
    }
    let pending: Pending = serde_json::from_slice(&root.read("pending.json")?)?;
    if pending.line.len() > MAX_EVENT
        || !pending.line.ends_with('\n')
        || month(&format!("{}-01T00:00:00Z", pending.month))? != pending.month
        || sha256_hex(pending.line.as_bytes()) != pending.sha256
    {
        return Err(failure());
    }
    let event: Event = serde_json::from_str(&pending.line)?;
    if !event.valid()
        || month(&event.observed_at)? != pending.month
        || pending.reservation.is_some_and(|id| id != event.id)
    {
        return Err(failure());
    }
    let mut file = root.file(&format!("{}.jsonl", pending.month), true)?;
    let size = file.metadata()?.len();
    if size < pending.offset || size > pending.offset + pending.line.len() as u64 {
        return Err(failure());
    }
    file.seek(SeekFrom::Start(pending.offset))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    if !pending.line.as_bytes().starts_with(&tail) {
        return Err(failure());
    }
    file.write_all(&pending.line.as_bytes()[tail.len()..])?;
    file.sync_all()?;
    root.sync()?;
    if let Some(id) = pending.reservation {
        super::reservation::remove(root, id)?;
    }
    root.remove("pending.json")?;
    root.sync()?;
    Ok(())
}
