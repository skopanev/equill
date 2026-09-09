use super::StoredRecord;
use crate::kernel::error::Error;
use crate::kernel::identity;
use crate::kernel::store;
use crate::schema;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::Path;
use uuid::Version;

pub fn verify_all(store_root: &Path) -> Result<usize, Error> {
    Ok(read_all(store_root)?.len())
}

pub fn read_all(store_root: &Path) -> Result<Vec<StoredRecord>, Error> {
    Ok(read_snapshot(store_root)?.records)
}

pub(crate) struct VerifiedSnapshot {
    pub records: Vec<StoredRecord>,
    pub digests: HashMap<uuid::Uuid, String>,
    pub bytes: u64,
}

pub(crate) fn read_snapshot(store_root: &Path) -> Result<VerifiedSnapshot, Error> {
    read_captured(store_root, super::snapshot::capture(store_root, None)?)
}

/// Only for callers already holding writer.lock, after write recovery.
pub(crate) fn read_all_exclusive(store_root: &Path) -> Result<Vec<StoredRecord>, Error> {
    Ok(read_captured(
        store_root,
        super::snapshot::capture_exclusive(store_root, None)?,
    )?
    .records)
}

pub(crate) fn read_captured(
    store_root: &Path,
    snapshot: super::snapshot::Snapshot,
) -> Result<VerifiedSnapshot, Error> {
    #[cfg(test)]
    super::snapshot::after_capture(store_root);
    #[cfg(test)]
    super::hotpath::ledger_read();
    let config = store::load(store_root)?;
    let mut ids = HashSet::new();
    let mut records = Vec::new();
    let mut digests = HashMap::new();
    for mut shard in snapshot.shards {
        let mut contents = String::new();
        shard.reader.read_to_string(&mut contents)?;
        // A reader running beside an append-only writer can catch the last line
        // mid-write. Only completed lines are records: a trailing fragment with
        // no newline is a write in progress, not corruption, and refusing to
        // read the store until it finishes would make every reader wait on
        // every writer.
        let complete = match contents.rfind('\n') {
            Some(end) => &contents[..=end],
            None if contents.trim().is_empty() => &contents[..],
            // No newline at all: nothing has been completely written yet.
            None => "",
        };
        for (index, line) in complete.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let location = format!("records/{}:{}", shard.name, index + 1);
            let record: StoredRecord = serde_json::from_str(line)
                .map_err(|error| Error::Integrity(format!("{location}: {error}")))?;
            verify_record(store_root, &config, &record)
                .map_err(|error| Error::Integrity(format!("{location}: {error}")))?;
            if !ids.insert(record.id) {
                return Err(Error::Integrity(format!(
                    "{location}: duplicate record identifier"
                )));
            }
            digests.insert(
                record.id,
                crate::kernel::digest::sha256_hex(line.as_bytes()),
            );
            records.push(record);
        }
    }
    super::lifecycle::validate_graph(store_root, &records)
        .map_err(|error| Error::Integrity(format!("record lifecycle: {error}")))?;
    Ok(VerifiedSnapshot {
        records,
        digests,
        bytes: snapshot.bytes,
    })
}

/// The ledgers of a store in the order their records were written.
///
/// `read_dir` promises no order at all, and the one it happens to give is a
/// property of the filesystem rather than of the store. Ledgers are named for
/// the month they hold, so sorting by name is sorting by time.
///
/// Callers depend on this in ways that stay invisible until a store spans two
/// months. The text catch-up carries a count as its cursor into this sequence,
/// and a count is only a cursor if the sequence grows at the end: a new month
/// landing anywhere else means every record the cursor steps over is skipped
/// for good. The vector corpus digest is taken over the same sequence, and a
/// reordering would change the digest without a single record changing —
/// reporting an index that is exactly right as behind.
///
/// A function rather than a line, because a test can hand it a reversed list
/// and see what it does. Handing `read_dir` a reversed list is not something a
/// test can do.
pub(super) fn in_month_order<T: Ord>(mut ledgers: Vec<T>) -> Vec<T> {
    ledgers.sort();
    ledgers
}

pub(crate) fn verify_record(
    store_root: &Path,
    config: &store::StoreConfig,
    record: &StoredRecord,
) -> Result<(), Error> {
    if record.id.get_version() != Some(Version::SortRand) {
        return Err(Error::InvalidRecord("id must be UUIDv7".into()));
    }
    if !identity::valid(&record.actor) {
        return Err(Error::InvalidActor);
    }
    let definition = schema::load(store_root, &record.type_name)?;
    super::validation::validate_stored(record, config, &definition)
}

#[cfg(test)]
mod order {
    /// Whatever order the directory gives, the answer is the same.
    #[test]
    fn ledgers_are_read_oldest_month_first() {
        let scrambled: Vec<std::path::PathBuf> = ["2099-01", "1999-01", "2026-08", "2026-01"]
            .iter()
            .map(|month| std::path::PathBuf::from(format!("/store/records/{month}.jsonl")))
            .collect();
        let ordered: Vec<String> = super::in_month_order(scrambled)
            .iter()
            .filter_map(|path| {
                path.file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
            })
            .collect();
        assert_eq!(ordered, ["1999-01", "2026-01", "2026-08", "2099-01"]);
    }
}
