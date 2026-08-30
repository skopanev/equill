use super::StoredRecord;
use crate::kernel::error::Error;
use crate::kernel::identity;
use crate::kernel::store;
use crate::schema;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use uuid::Version;

pub fn verify_all(store_root: &Path) -> Result<usize, Error> {
    Ok(read_all(store_root)?.len())
}

pub fn read_all(store_root: &Path) -> Result<Vec<StoredRecord>, Error> {
    let config = store::load(store_root)?;
    let directory = store_root.join("records");
    if !directory.is_dir() {
        return Err(Error::Integrity(format!(
            "required directory is missing: {}",
            directory.display()
        )));
    }
    let mut ids = HashSet::new();
    let mut records = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let is_ledger = path.extension().is_some_and(|value| value == "jsonl");
        if !is_ledger {
            return Err(Error::Integrity(format!(
                "unexpected record ledger entry: {}",
                path.display()
            )));
        }
        let contents = fs::read_to_string(&path)?;
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
            let location = format!("{}:{}", path.display(), index + 1);
            let record: StoredRecord = serde_json::from_str(line)
                .map_err(|error| Error::Integrity(format!("{location}: {error}")))?;
            verify_record(store_root, &config, &record)
                .map_err(|error| Error::Integrity(format!("{location}: {error}")))?;
            if !ids.insert(record.id) {
                return Err(Error::Integrity(format!(
                    "{location}: duplicate record identifier"
                )));
            }
            records.push(record);
        }
    }
    super::lifecycle::validate_graph(store_root, &records)
        .map_err(|error| Error::Integrity(format!("record lifecycle: {error}")))?;
    Ok(records)
}

fn verify_record(
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
