use super::config::VectorConfig;
use super::model::{VectorState, valid_collection_name, valid_sha256, vector_error};
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(super) const STATE: &str = "projections/qdrant/state.json";
const SCHEMA: &str = "equill.qdrant-state.v2";
const SCHEMA_V1: &str = "equill.qdrant-state.v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StateFile {
    pub(super) schema: String,
    pub(super) state: StoredState,
    pub(super) store_id: Uuid,
    pub(super) collection_alias: String,
    pub(super) physical_collection: String,
    pub(super) model_sha256: String,
    /// The snapshot this index was built from: how many records it covered and
    /// their digest. A v1 marker has neither, which is not a failure — it is a
    /// checkpoint whose freshness simply cannot be computed until the next sync
    /// writes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) indexed_records: Option<usize>,
    /// The desired revision this pass covered. Bound to the checkpoint so a
    /// write that lands mid-pass leaves the numbers apart and forces another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) indexed_revision: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) indexed_sha256: Option<String>,
    /// The `embed_types` filter the indexed corpus was taken under, as a set.
    ///
    /// Absent means no filter, which is what every marker written before the
    /// field existed says and what a store configuring none still writes. It is
    /// part of the marker's identity rather than of its freshness: a checkpoint
    /// taken under a different filter does not describe a smaller corpus, it
    /// describes a different one, and reading it as a position in this store is
    /// how a narrowed index stayed current over records it had never dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) embed_types_sha256: Option<String>,
    /// The document cap the indexed inputs were built under — identity, not
    /// freshness: a checkpoint under another cap describes different inputs,
    /// and the corpus digest cannot see a cap because it hashes ledger lines.
    /// Absent matches no cap, so a pre-cap marker earns one pass, cheap where
    /// nothing was truncated. The QUERY cap never touches a document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) max_document_chars: Option<usize>,
}

/// Whether the index reflects the ledger as it is now. This is not health: an
/// index can be entirely healthy and still behind, which is the ordinary state
/// of any store that is being written to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VectorFreshness {
    /// The indexed snapshot matches the ledger.
    Current,
    /// The index is healthy and behind; pending record count may be unknown.
    Lagging,
    /// A pre-v2 checkpoint: searchable, but its snapshot was never recorded.
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
pub struct Freshness {
    pub freshness: VectorFreshness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_records: Option<usize>,
    /// Absent means unknown, not zero. Revision gaps are not document counts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_records: Option<usize>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum StoredState {
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

/// `snapshot` is (records, digest, revision, the filter it was taken under).
/// The filter travels with the rest of the boundary instead of being re-read
/// here: a descriptor that changes while a pass runs would otherwise stamp the
/// checkpoint with a filter the pass never applied.
pub(crate) fn stage_ready(
    store: &Path,
    config: &VectorConfig,
    physical: &str,
    snapshot: Option<(usize, &str, u64, Option<&str>)>,
) -> Result<StagedReady, Error> {
    stage(store, config, physical, StoredState::Ready, snapshot)
}

fn stage(
    store: &Path,
    config: &VectorConfig,
    physical: &str,
    state: StoredState,
    snapshot: Option<(usize, &str, u64, Option<&str>)>,
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
        model_sha256: config.embedding.model_sha256().to_owned(),
        indexed_records: snapshot.map(|(count, _, _, _)| count),
        indexed_revision: snapshot.map(|(_, _, revision, _)| revision),
        indexed_sha256: snapshot.map(|(_, digest, _, _)| digest.to_owned()),
        embed_types_sha256: snapshot.and_then(|(_, _, _, filter)| filter.map(str::to_owned)),
        max_document_chars: snapshot.map(|_| config.max_document_chars),
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
        || marker.model_sha256 != config.embedding.model_sha256()
    {
        return Ok(VectorState::Degraded);
    }
    Ok(match marker.state {
        StoredState::Ready => VectorState::Ready,
        StoredState::Degraded => VectorState::Degraded,
    })
}

/// Whether a marker describes the index this config points at. The same
/// question the health read asks, asked once more before believing a number.
pub(super) fn describes(marker: &StateFile, config: &VectorConfig) -> bool {
    (marker.schema == SCHEMA || marker.schema == SCHEMA_V1)
        && marker.store_id == config.store_id
        && marker.collection_alias == config.collection_alias
        && marker.model_sha256 == config.embedding.model_sha256()
        // A checkpoint taken under another filter is not this store's position.
        // Without this, a configure that stored the descriptor and then died
        // before publishing its target left a marker that still looked current:
        // the retry saw an unchanged filter, published nothing, and the vectors
        // of an excluded type answered searches nothing in the ledger accounts
        // for. Both absent means no filter on either side, which is what every
        // marker written before this field says.
        && marker.embed_types_sha256 == super::coverage::fingerprint(&config.embed_types)
        && marker.max_document_chars == Some(config.max_document_chars)
        && valid_collection_name(&marker.physical_collection)
}

/// The checkpoint as recorded, without touching the ledger.
///
/// `freshness` hashes the whole corpus to say how far behind the index is. That
/// is right for a report and unacceptable for a gate that runs before every
/// command, so this reads the marker and nothing else.
///
/// `None` means there is no usable checkpoint: no config, no marker, a marker
/// describing a different store, or half a checkpoint. Each of those means the
/// index cannot be shown to be current — a reason to look closer, never a
/// reason to assume all is well.
pub(crate) fn checkpoint(store: &Path, config: Option<&VectorConfig>) -> Option<u64> {
    let config = config.filter(|config| config.enabled)?;
    let path = store.join(STATE);
    if !path.is_file() {
        return None;
    }
    let marker: StateFile = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    if !describes(&marker, config) {
        return None;
    }
    // The revision the worker actually covered. A write during a pass leaves
    // this behind the target, which is what forces another pass.
    marker.indexed_revision
}
