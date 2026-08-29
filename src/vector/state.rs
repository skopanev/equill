use super::config::VectorConfig;
use super::model::{VectorState, valid_collection_name, valid_sha256, vector_error};
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const STATE: &str = "projections/qdrant/state.json";
const SCHEMA: &str = "equill.qdrant-state.v1";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StateFile {
    schema: String,
    state: StoredState,
    store_id: Uuid,
    collection_alias: String,
    physical_collection: String,
    model_sha256: String,
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
) -> Result<StagedReady, Error> {
    stage(store, config, physical, StoredState::Ready)
}

pub(crate) fn write_degraded(
    store: &Path,
    config: &VectorConfig,
    physical: &str,
) -> Result<(), Error> {
    stage(store, config, physical, StoredState::Degraded)?.commit()
}

fn stage(
    store: &Path,
    config: &VectorConfig,
    physical: &str,
    state: StoredState,
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
    if marker.schema != SCHEMA
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
