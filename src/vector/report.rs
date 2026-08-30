//! How far behind the index is. A report, not a gate: it re-reads and hashes the
//! whole ledger, which answers the question honestly and would be ruinous on the
//! path of every command.
use super::config::VectorConfig;
use super::model::{valid_sha256, vector_error};
use super::state::{Freshness, STATE, StateFile, VectorFreshness, describes};
use crate::kernel::error::Error;
use std::fs;
use std::path::Path;

/// How far behind the index is, read without loading a model or touching the
/// provider. A store nobody has written to since the last sync is `Current`; a
/// store that has moved on is `Lagging` by a countable number of records; a
/// pre-v2 checkpoint is `Unknown`, because its snapshot was never recorded.
///
/// Freshness is never an error: a lagging index still answers, and saying so
/// honestly is the point.
pub fn freshness(store: &Path, config: Option<&VectorConfig>) -> Result<Freshness, Error> {
    let unknown = Freshness {
        freshness: VectorFreshness::Unknown,
        indexed_records: None,
        pending_records: None,
    };
    let Some(config) = config.filter(|config| config.enabled) else {
        return Ok(unknown);
    };
    let path = store.join(STATE);
    if !path.is_file() {
        return Ok(unknown);
    }
    let marker: StateFile = serde_json::from_slice(&fs::read(path)?)?;
    // Freshness is only meaningful for a marker that describes this store, this
    // alias and this model. A checkpoint that describes something else is not a
    // smaller number — it is no answer at all.
    if !describes(&marker, config) {
        return Ok(unknown);
    }
    let (indexed, digest) = match (marker.indexed_records, marker.indexed_sha256) {
        (Some(indexed), Some(digest)) if valid_sha256(digest.as_str()) => (indexed, digest),
        // Half a checkpoint is a malformed one: refuse to read a count whose
        // snapshot is missing, rather than report freshness against nothing.
        (None, None) => return Ok(unknown),
        _ => {
            return Err(vector_error(
                "state marker carries an incomplete checkpoint",
            ));
        }
    };
    // A checkpoint cannot have covered a target that was never published, nor
    // one older than the revision it claims. When it says otherwise the marker
    // is not describing this store's history, and the honest answer is that its
    // position is unknown — not that it is current, which is what a digest
    // comparison alone would say. Absent means revision zero, which is what the
    // sync itself uses when there is no target, so a checkpoint at zero against
    // no target is level rather than ahead.
    let target = super::desired::read(store)?.map_or(0, |desired| desired.revision);
    if marker
        .indexed_revision
        .is_some_and(|indexed| indexed > target)
    {
        return Ok(unknown);
    }
    let (records, current) = super::corpus(store)?;
    Ok(Freshness {
        freshness: if current == digest {
            VectorFreshness::Current
        } else {
            VectorFreshness::Lagging
        },
        indexed_records: Some(indexed),
        // Records the snapshot did not cover. Never negative and never falsely
        // zero: a shrinking corpus reports nothing pending rather than a lie.
        pending_records: Some(records.len().saturating_sub(indexed)),
    })
}
