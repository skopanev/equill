//! Canonical reads bounded to named ledger shards. No directory enumeration,
//! projection payload or lifecycle rebuild. Snapshot capture never waits for a writer.
pub(crate) mod open;

use super::StoredRecord;
use crate::kernel::{digest::sha256_hex, error::Error, store};
use crate::projection::LedgerLocator;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Resolve every coordinate against immutable truth, preserving input order.
/// Missing or duplicate coordinates, invalid rows and digest mismatches refuse
/// the whole result. This function does not establish projection freshness.
pub fn read(root: &Path, locators: &[LedgerLocator]) -> Result<Vec<StoredRecord>, Error> {
    read_prefix(root, locators, false)
}

/// For a caller already holding the writer lock: an incomplete tail is crash
/// damage here, not a concurrent append. This function does not take the lock.
pub fn read_exclusive(root: &Path, locators: &[LedgerLocator]) -> Result<Vec<StoredRecord>, Error> {
    read_prefix(root, locators, true)
}

fn read_prefix(
    root: &Path,
    locators: &[LedgerLocator],
    exclusive: bool,
) -> Result<Vec<StoredRecord>, Error> {
    if locators.is_empty() {
        return Ok(Vec::new());
    }
    let mut wanted = HashMap::new();
    let mut shards = BTreeMap::<&str, HashSet<uuid::Uuid>>::new();
    for locator in locators {
        let name = shard_name(&locator.ledger)?;
        if locator.record_id.get_version() != Some(uuid::Version::SortRand)
            || !valid_digest(&locator.record_sha256)
            || wanted.insert(locator.record_id, locator).is_some()
        {
            return Err(failure("invalid or duplicate locator coordinate"));
        }
        shards.entry(name).or_default().insert(locator.record_id);
    }
    let config = store::load(root).map_err(|_| failure("invalid store metadata"))?;
    let names = shards.keys().copied().collect::<Vec<_>>();
    let snapshot = if exclusive {
        super::snapshot::capture_exclusive(root, Some(&names))?
    } else {
        super::snapshot::capture(root, Some(&names))?
    };
    let mut seen = HashSet::new();
    let mut found = HashMap::new();
    for shard in snapshot.shards {
        let name = shard.name.as_str();
        let ids = &shards[name];
        #[cfg(test)]
        SHARDS.with(|count| count.set(count.get() + 1));
        let mut reader = BufReader::new(shard.reader);
        let mut line = String::new();
        loop {
            line.clear();
            if reader
                .read_line(&mut line)
                .map_err(|_| failure("unreadable ledger row"))?
                == 0
            {
                break;
            }
            if !line.ends_with('\n') {
                if exclusive {
                    return Err(failure("incomplete ledger row under writer lock"));
                }
                break;
            }
            if line.trim().is_empty() {
                return Err(failure("blank ledger row"));
            }
            let record: StoredRecord =
                serde_json::from_str(&line).map_err(|_| failure("invalid ledger row"))?;
            super::verify::verify_record(root, &config, &record)
                .map_err(|_| failure("invalid stored record"))?;
            if !seen.insert(record.id) {
                return Err(failure("duplicate ledger coordinate"));
            }
            if !ids.contains(&record.id) {
                continue;
            }
            if record.recorded_at.get(..7) != name.strip_suffix(".jsonl") {
                return Err(failure("record and ledger month disagree"));
            }
            let actual = sha256_hex(
                &serde_json::to_vec(&record).map_err(|_| failure("record digest unavailable"))?,
            );
            if actual != wanted[&record.id].record_sha256 {
                return Err(failure("locator record SHA-256 mismatch"));
            }
            found.insert(record.id, record);
        }
    }
    locators
        .iter()
        .map(|locator| {
            found
                .remove(&locator.record_id)
                .ok_or_else(|| failure("coordinate absent from named ledger"))
        })
        .collect()
}

fn shard_name(ledger: &str) -> Result<&str, Error> {
    let name = ledger
        .strip_prefix("records/")
        .ok_or_else(|| failure("invalid ledger path"))?;
    let month = name
        .strip_suffix(".jsonl")
        .ok_or_else(|| failure("invalid ledger path"))?;
    if month.len() != 7
        || month.as_bytes()[4] != b'-'
        || !month
            .bytes()
            .enumerate()
            .all(|(i, b)| i == 4 || b.is_ascii_digit())
        || !("01"..="12").contains(&&month[5..])
    {
        return Err(failure("invalid ledger path"));
    }
    Ok(name)
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn failure(reason: &str) -> Error {
    Error::Integrity(format!("candidate ledger verification: {reason}"))
}

#[cfg(test)]
thread_local! {
    static SHARDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn shard_reads() -> usize {
    SHARDS.with(|count| count.replace(0))
}
