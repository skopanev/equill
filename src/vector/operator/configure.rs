use super::super::model::vector_error;
use crate::kernel::error::Error;
use crate::kernel::{identity, store};
use serde::Serialize;
use serde_json::Value;
use std::fs;
use std::path::Path;

const CONFIG: &str = "registry/vector/qdrant.json";

#[derive(Debug, Serialize)]
pub struct VectorConfigReport {
    pub ok: bool,
    pub projection: &'static str,
    pub enabled: bool,
    pub collection_alias: String,
}

/// Governance, so root only. The candidate is written first and then loaded
/// through the ordinary reader: validation, artifact hashes and all. Anything
/// the reader rejects is rolled back, so a store never keeps a config that its
/// own loader would refuse — the alternative is a second, drifting validator.
pub fn configure(store_root: &Path, file: &Path, actor: &str) -> Result<VectorConfigReport, Error> {
    let config = store::load(store_root)?;
    identity::require_root(&config, actor)?;
    let candidate: Value = serde_json::from_slice(&fs::read(file)?)?;
    write(store_root, &candidate, previous(store_root)?)
}

/// Disabling keeps the descriptor so a later enable does not have to rebuild
/// the file, and immediately makes the projection report Disabled.
pub fn disable(store_root: &Path, actor: &str) -> Result<VectorConfigReport, Error> {
    let config = store::load(store_root)?;
    identity::require_root(&config, actor)?;
    let mut current =
        previous(store_root)?.ok_or_else(|| vector_error("vector projection is not configured"))?;
    current
        .as_object_mut()
        .ok_or_else(|| vector_error("stored config is not an object"))?
        .insert("enabled".into(), Value::Bool(false));
    let restore = previous(store_root)?;
    write(store_root, &current, restore)
}

fn write(
    store_root: &Path,
    candidate: &Value,
    restore: Option<Value>,
) -> Result<VectorConfigReport, Error> {
    let path = store_root.join(CONFIG);
    let directory = path
        .parent()
        .ok_or_else(|| vector_error("config directory is invalid"))?;
    fs::create_dir_all(directory)?;
    fs::write(&path, serde_json::to_vec_pretty(candidate)?)?;
    match super::super::config::load(store_root) {
        Ok(Some(loaded)) => Ok(VectorConfigReport {
            ok: true,
            projection: "vector-qdrant",
            enabled: loaded.enabled,
            collection_alias: loaded.collection_alias,
        }),
        Ok(None) => Err(vector_error("config did not persist")),
        Err(error) => {
            rollback(&path, restore);
            Err(error)
        }
    }
}

fn rollback(path: &Path, restore: Option<Value>) {
    match restore.and_then(|value| serde_json::to_vec_pretty(&value).ok()) {
        Some(bytes) => {
            let _ = fs::write(path, bytes);
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
}

fn previous(store_root: &Path) -> Result<Option<Value>, Error> {
    let path = store_root.join(CONFIG);
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
}
