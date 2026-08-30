use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

const MARKER: &str = "projections/lifecycle/watermark.json";
const SCHEMA: &str = "equill.lifecycle-state.v2";

/// The ledger position a lifecycle state describes: the total size of the
/// ledger files.
///
/// Size alone, because it is the only thing readable without opening them —
/// which is the point. An append-only ledger only ever grows, so a state whose
/// watermark matches the current size was built from exactly these bytes. If
/// the ledger is ever rewritten rather than appended to, the size changes and
/// the state is refused, which is the safe direction to be wrong in.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Watermark {
    pub(crate) bytes: u64,
}

#[derive(Debug, Deserialize, Serialize)]
struct Marker {
    schema: String,
    watermark: Watermark,
}

/// Where the ledger stands now, from file metadata alone.
pub(crate) fn observe(store: &Path) -> Result<Watermark, Error> {
    let directory = store.join("records");
    let mut bytes = 0;
    if directory.is_dir() {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if path.extension().is_some_and(|value| value == "jsonl") {
                bytes += fs::metadata(&path)?.len();
            }
        }
    }
    Ok(Watermark { bytes })
}

/// The position the stored state claims, if there is one this version can read.
///
/// An unreadable or foreign marker reports absence rather than an error: the
/// caller's fallback is to rebuild from the ledger, which is the only source
/// that can settle the question anyway.
pub(crate) fn read(store: &Path) -> Result<Option<Watermark>, Error> {
    let path = store.join(MARKER);
    if !path.is_file() {
        return Ok(None);
    }
    let Ok(marker) = serde_json::from_slice::<Marker>(&fs::read(&path)?) else {
        return Ok(None);
    };
    if marker.schema != SCHEMA {
        return Ok(None);
    }
    Ok(Some(marker.watermark))
}

/// Withdraw the claim. Until a marker is written again there is no state, which
/// is the safe thing to be interrupted as.
pub(crate) fn discard(store: &Path) {
    let _ = fs::remove_file(store.join(MARKER));
}

/// Stamp the ledger position the stored lines now describe, atomically.
///
/// Written after those lines, never before: a crash in between leaves a
/// watermark that no longer matches the ledger, so the next write rebuilds —
/// one scan, rather than a state that claims to cover records it does not hold.
pub(crate) fn commit(store: &Path, directory: &Path) -> Result<Watermark, Error> {
    let watermark = observe(store)?;
    let bytes = serde_json::to_vec(&Marker {
        schema: SCHEMA.into(),
        watermark,
    })?;
    let temporary = directory.join(format!(".watermark-{}.json", Uuid::now_v7()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    if file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(Error::Integrity(
            "lifecycle watermark staging failed".into(),
        ));
    }
    drop(file);
    if fs::rename(&temporary, store.join(MARKER)).is_err() {
        let _ = fs::remove_file(&temporary);
        return Err(Error::Integrity("lifecycle watermark commit failed".into()));
    }
    let _ = File::open(directory).and_then(|handle| handle.sync_all());
    Ok(watermark)
}
