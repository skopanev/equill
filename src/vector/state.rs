use super::config::VectorConfig;
use super::model::{VectorState, valid_collection_name, valid_sha256, vector_error};
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const STATE: &str = "projections/qdrant/state.json";
const SCHEMA: &str = "equill.qdrant-state.v2";
const SCHEMA_V1: &str = "equill.qdrant-state.v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StateFile {
    schema: String,
    state: StoredState,
    store_id: Uuid,
    collection_alias: String,
    physical_collection: String,
    model_sha256: String,
    /// The snapshot this index was built from: how many records it covered and
    /// their digest. A v1 marker has neither, which is not a failure — it is a
    /// checkpoint whose freshness simply cannot be computed until the next sync
    /// writes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    indexed_records: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    indexed_sha256: Option<String>,
}

/// Whether the index reflects the ledger as it is now. This is not health: an
/// index can be entirely healthy and still behind, which is the ordinary state
/// of any store that is being written to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VectorFreshness {
    /// The indexed snapshot matches the ledger.
    Current,
    /// The index is healthy and behind by a known number of records.
    Lagging,
    /// A pre-v2 checkpoint: searchable, but its snapshot was never recorded.
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
pub struct Freshness {
    pub freshness: VectorFreshness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_records: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_records: Option<usize>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum StoredState {
    Ready,
    Degraded,
}

pub(crate) struct StagedReady {
    temporary: PathBuf,
    final_path: PathBuf,
}

impl StagedReady {
    pub(crate) fn commit(mut self) -> Result<(), Error> {
        fs::rename(&self.temporary, &self.final_path)
            .map_err(|_| vector_error("ready marker commit failed"))?;
        self.temporary = PathBuf::new();
        if let Some(directory) = self.final_path.parent()
            && File::open(directory)
                .and_then(|file| file.sync_all())
                .is_err()
        {
            let _ = fs::remove_file(&self.final_path);
            return Err(vector_error("ready marker sync failed"));
        }
        Ok(())
    }
}

impl Drop for StagedReady {
    fn drop(&mut self) {
        if !self.temporary.as_os_str().is_empty() {
            let _ = fs::remove_file(&self.temporary);
        }
    }
}

pub(crate) fn stage_ready(
    store: &Path,
    config: &VectorConfig,
    physical: &str,
    snapshot: Option<(usize, &str)>,
) -> Result<StagedReady, Error> {
    stage(store, config, physical, StoredState::Ready, snapshot)
}

fn stage(
    store: &Path,
    config: &VectorConfig,
    physical: &str,
    state: StoredState,
    snapshot: Option<(usize, &str)>,
) -> Result<StagedReady, Error> {
    if !valid_collection_name(physical) {
        return Err(vector_error("invalid collection name"));
    }
    let final_path = store.join(STATE);
    let directory = final_path
        .parent()
        .ok_or_else(|| vector_error("ready marker directory is invalid"))?;
    fs::create_dir_all(directory).map_err(|_| vector_error("ready marker staging failed"))?;
    let temporary = directory.join(format!(".state-{}.json", Uuid::now_v7()));
    let marker = StateFile {
        schema: SCHEMA.into(),
        state,
        store_id: config.store_id,
        collection_alias: config.collection_alias.clone(),
        physical_collection: physical.into(),
        model_sha256: config.embedding.model.sha256.clone(),
        indexed_records: snapshot.map(|(count, _)| count),
        indexed_sha256: snapshot.map(|(_, digest)| digest.to_owned()),
    };
    let bytes = serde_json::to_vec(&marker)
        .map_err(|_| vector_error("ready marker serialization failed"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| vector_error("ready marker staging failed"))?;
    if file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .is_err()
    {
        drop(file);
        let _ = fs::remove_file(&temporary);
        return Err(vector_error("ready marker staging failed"));
    }
    Ok(StagedReady {
        temporary,
        final_path,
    })
}

pub(crate) fn read(store: &Path, config: Option<&VectorConfig>) -> Result<VectorState, Error> {
    let Some(config) = config.filter(|config| config.enabled) else {
        return Ok(VectorState::Disabled);
    };
    let path = store.join(STATE);
    if !path.is_file() {
        return Ok(VectorState::Missing);
    }
    let marker: StateFile = serde_json::from_slice(&fs::read(path)?)?;
    // A v1 marker is a valid checkpoint written by an older build; refusing it
    // would take a working index offline for a schema change it never made.
    if (marker.schema != SCHEMA && marker.schema != SCHEMA_V1)
        || !valid_collection_name(&marker.physical_collection)
        || !valid_sha256(&marker.model_sha256)
    {
        return Err(vector_error("invalid state marker"));
    }
    if marker.store_id != config.store_id
        || marker.collection_alias != config.collection_alias
        || marker.model_sha256 != config.embedding.model.sha256
    {
        return Ok(VectorState::Degraded);
    }
    Ok(match marker.state {
        StoredState::Ready => VectorState::Ready,
        StoredState::Degraded => VectorState::Degraded,
    })
}

/// How far behind the index is, read without loading a model or touching the
/// provider. A store nobody has written to since the last sync is `Current`; a
/// store that has moved on is `Lagging` by a countable number of records; a
/// pre-v2 checkpoint is `Unknown`, because its snapshot was never recorded.
///
/// Freshness is never an error: a lagging index still answers, and saying so
/// honestly is the point.
/// Whether a marker describes the index this config points at. The same
/// question the health read asks, asked once more before believing a number.
fn describes(marker: &StateFile, config: &VectorConfig) -> bool {
    (marker.schema == SCHEMA || marker.schema == SCHEMA_V1)
        && marker.store_id == config.store_id
        && marker.collection_alias == config.collection_alias
        && marker.model_sha256 == config.embedding.model.sha256
        && valid_collection_name(&marker.physical_collection)
}

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
        (Some(indexed), Some(digest)) if valid_sha256(&digest) => (indexed, digest),
        // Half a checkpoint is a malformed one: refuse to read a count whose
        // snapshot is missing, rather than report freshness against nothing.
        (None, None) => return Ok(unknown),
        _ => {
            return Err(vector_error(
                "state marker carries an incomplete checkpoint",
            ));
        }
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
