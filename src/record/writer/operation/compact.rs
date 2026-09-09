use super::{COMMITTED, invalid, load, storage, verify_receipt};
use crate::kernel::{digest::sha256_hex, error::Error, path};
use crate::record::StoredRecord;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

/// Intentional physical compaction expires removed records' operation keys.
/// Surviving coordinates keep their keys, with offsets/digests rebased to the
/// rewritten ledger. No deleted coordinate or digest is retained as a tombstone.
pub(crate) fn reconcile(
    root: &Path,
    records: &[StoredRecord],
    apply: bool,
) -> Result<usize, Error> {
    let directory = path::within(root, COMMITTED)?;
    if !directory.exists() {
        return Ok(0);
    }
    let mut offsets = BTreeMap::<String, u64>::new();
    let mut retained = HashMap::new();
    for record in records {
        let month = record
            .recorded_at
            .get(..7)
            .ok_or_else(|| invalid("coordinate"))?;
        let ledger = format!("records/{month}.jsonl");
        let bytes = serde_json::to_vec(record)?;
        let offset = offsets.entry(ledger.clone()).or_default();
        retained.insert(
            record.id,
            (ledger, *offset, bytes.len() as u64 + 1, sha256_hex(&bytes)),
        );
        *offset += bytes.len() as u64 + 1;
    }
    let mut expired = 0;
    for entry in fs::read_dir(directory)? {
        let name = path::plain_name(&entry?.path())?;
        if storage::is_temporary(&name) {
            if apply {
                storage::remove(root, &format!("{COMMITTED}/{name}"))?;
            }
            continue;
        }
        let key = name
            .strip_suffix(".json")
            .ok_or_else(|| invalid("coordinate"))?;
        let relative = format!("{COMMITTED}/{name}");
        let mut outcome = load(root, &relative, key)?;
        match retained.get(&outcome.record_id) {
            None => {
                expired += 1;
                if apply {
                    storage::remove(root, &relative)?;
                }
            }
            Some((ledger, offset, length, digest)) if apply => {
                outcome.ledger = ledger.clone();
                outcome.offset = *offset;
                outcome.length = *length;
                outcome.record_sha256 = digest.clone();
                verify_receipt(root, &outcome)?;
                storage::publish(root, &relative, &outcome)?;
            }
            Some(_) => {}
        }
    }
    Ok(expired)
}
