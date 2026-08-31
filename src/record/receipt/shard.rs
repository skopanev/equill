//! Asking a ledger shard whether it holds a record, under the writer lock.
use super::super::StoredRecord;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use uuid::Uuid;

pub(super) enum Held {
    Once,
    Never,
    Twice,
    Mismatched,
}

/// Ask the one ledger shard the receipt names, and no other.
///
/// This runs under the store's writer lock, which is what lets it be strict.
/// The ledger reader used elsewhere tolerates an unterminated final line
/// because it runs beside a live writer and that line is a write in progress.
/// Here there is no live writer: nothing else can be appending, so an
/// unterminated line is not in progress, it is what a crash left behind. The
/// same goes for a blank line and for a line that is not a record — this
/// writer emits neither, so their presence is damage.
///
/// Damage is not absence. Absence sends the stage to quarantine; reading
/// damage as absence would file the receipt for a record that is really there
/// as an abandoned stage. So anything short of a shard this store could have
/// written refuses the next write instead of answering.
pub(super) fn ledger_holds(
    store_root: &Path,
    month: &str,
    record_id: Uuid,
    digest: &str,
) -> Result<Held, Error> {
    let path = store_root.join("records").join(format!("{month}.jsonl"));
    let contents = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        // No shard at all is a real answer: the append never created one.
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Held::Never),
        Err(error) => return Err(error.into()),
    };
    if !contents.is_empty() && !contents.ends_with('\n') {
        return Err(damaged(&path, "its final line was never finished"));
    }
    let mut found = 0;
    let mut matched = 0;
    for line in contents.lines() {
        if line.trim().is_empty() {
            return Err(damaged(&path, "it holds a blank line"));
        }
        // Typed, not inspected: a shard whose line parses as JSON but not as a
        // record is not a shard this store wrote. `{}` is valid JSON and is not
        // an answer to anything.
        let record: StoredRecord = serde_json::from_str(line).map_err(|error| {
            damaged(
                &path,
                &format!("it holds a line that is not a record: {error}"),
            )
        })?;
        if record.id != record_id {
            continue;
        }
        found += 1;
        // The digest is taken over the serialized record exactly as the writer
        // hashed it: the ledger line without its newline.
        if sha256_hex(line.as_bytes()) == digest {
            matched += 1;
        }
    }
    Ok(match (found, matched) {
        (0, _) => Held::Never,
        (1, 1) => Held::Once,
        (1, _) => Held::Mismatched,
        _ => Held::Twice,
    })
}

/// Lowercase, sixty-four hexadecimal characters, and nothing else.
pub(super) fn canonical_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn damaged(path: &Path, because: &str) -> Error {
    Error::Integrity(format!(
        "the ledger shard an unfinished receipt names is damaged — {because}: {}",
        path.display()
    ))
}
