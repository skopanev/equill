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
    let Checkpoint::Usable { indexed, digest } = checkpoint(store, config)? else {
        return Ok(Freshness {
            freshness: VectorFreshness::Unknown,
            indexed_records: None,
            pending_records: None,
        });
    };
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

/// What the last successful pass recorded, or why nothing it recorded can be
/// read as this store's position.
///
/// The unusable arm carries a reason rather than `None`, because every caller
/// that asked for a checkpoint has to say something to a reader, and "no
/// checkpoint" and "a checkpoint describing another model" are not the same
/// news. Reading a missing one as zero would report a fresh store as fully
/// behind and a foreign marker as fully caught up.
#[derive(Debug)]
pub enum Checkpoint {
    Usable { indexed: usize, digest: String },
    Unusable { reason: &'static str },
}

impl Checkpoint {
    pub fn indexed(&self) -> Option<usize> {
        match self {
            Checkpoint::Usable { indexed, .. } => Some(*indexed),
            Checkpoint::Unusable { .. } => None,
        }
    }
}

fn unusable(reason: &'static str) -> Checkpoint {
    Checkpoint::Unusable { reason }
}

/// Every test of whether the marker describes this store, in one place.
///
/// Freshness and the status report used to apply their own subsets, so a marker
/// claiming a revision the store never published was unknown to one and current
/// to the other — the same file, two answers, in one report.
pub fn checkpoint(store: &Path, config: Option<&VectorConfig>) -> Result<Checkpoint, Error> {
    let Some(config) = config.filter(|config| config.enabled) else {
        return Ok(unusable("no vector projection is configured"));
    };
    let path = store.join(STATE);
    if !path.is_file() {
        return Ok(unusable("the store has no vector state marker"));
    }
    let marker: StateFile = serde_json::from_slice(&fs::read(path)?)?;
    // A checkpoint that describes another store, alias or model is not a smaller
    // number — it is no answer at all.
    if !describes(&marker, config) {
        return Ok(unusable(
            "the marker describes another store, alias or model",
        ));
    }
    let (indexed, digest) = match (marker.indexed_records, marker.indexed_sha256) {
        (Some(indexed), Some(digest)) if valid_sha256(digest.as_str()) => (indexed, digest),
        // Half a checkpoint is a malformed one: refuse to read a count whose
        // snapshot is missing, rather than report freshness against nothing.
        (None, None) => return Ok(unusable("the marker predates checkpoints")),
        _ => {
            return Err(vector_error(
                "state marker carries an incomplete checkpoint",
            ));
        }
    };
    // A checkpoint cannot have covered a target that was never published.
    // Absent means revision zero, which is what the sync itself uses when there
    // is no target, so a checkpoint at zero against no target is level rather
    // than ahead.
    let target = super::desired::read(store)?.map_or(0, |desired| desired.revision);
    if marker
        .indexed_revision
        .is_some_and(|indexed| indexed > target)
    {
        return Ok(unusable(
            "the checkpoint claims a revision the store never published",
        ));
    }
    Ok(Checkpoint::Usable { indexed, digest })
}

/// One reading of where the index stands, for every part of a status report.
///
/// The store block and the component block used to read the marker separately
/// and could disagree inside a single JSON document. They are now two views of
/// this.
pub struct Position {
    /// Live records this store would embed. A property of the ledger, so it is
    /// counted whether or not a provider is configured.
    pub corpus: Vec<(crate::record::StoredRecord, String)>,
    pub corpus_digest: String,
    pub checkpoint: Checkpoint,
    /// Live records `embed_types` left out, and whether a filter is configured
    /// at all. Nothing skipped and nothing to skip are different answers, and a
    /// reader given only a zero cannot tell them apart.
    pub skipped_by_type: usize,
    pub filtered: bool,
}

impl Position {
    pub fn freshness(&self) -> VectorFreshness {
        match &self.checkpoint {
            Checkpoint::Unusable { .. } => VectorFreshness::Unknown,
            Checkpoint::Usable { digest, .. } if *digest == self.corpus_digest => {
                VectorFreshness::Current
            }
            Checkpoint::Usable { .. } => VectorFreshness::Lagging,
        }
    }
}

/// Reads that position without touching the provider: the corpus comes from the
/// ledger and the rules that decide what is embeddable, the checkpoint from the
/// marker file.
pub fn position(store: &Path) -> Result<Position, Error> {
    let config = super::config::load(store)?;
    let checkpoint = checkpoint(store, config.as_ref())?;
    let snapshot = super::operator::corpus_snapshot(store)?;
    Ok(Position {
        corpus: snapshot.records,
        corpus_digest: snapshot.digest,
        checkpoint,
        skipped_by_type: snapshot.skipped_by_type,
        filtered: !super::coverage::embed_types(store)?.is_empty(),
    })
}
