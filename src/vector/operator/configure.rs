use super::super::model::vector_error;
use crate::kernel::error::Error;
use crate::kernel::governance::RootGuard;
use crate::kernel::lock::StoreLock;

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
    let (_guard, _config) = RootGuard::acquire(store_root, actor)?;
    let candidate: Value = serde_json::from_slice(&fs::read(file)?)?;
    embed_types_are_registered(store_root, &candidate)?;
    let before = previous(store_root)?;
    let filter_changed = embed_types_of(before.as_ref()) != embed_types_of(Some(&candidate));
    let report = write(store_root, &candidate, before)?;
    if filter_changed {
        announce_outstanding_work(store_root)?;
    }
    Ok(report)
}

/// Changing the filter changes the corpus, and nothing else says so.
///
/// The target is what tells the catch-up machinery the index owes the ledger
/// something; a corpus digest alone does not, because the gate answers from
/// markers and never scans the ledger. Without this, narrowing `embed_types`
/// left the old checkpoint reading as current and no pass ever ran — so the
/// vectors of a type the store had just stopped embedding stayed in the
/// collection, answering searches nothing in the ledger accounts for.
///
/// The same contract an ordinary append uses, and the same lock, so a write
/// landing at the same moment cannot read and republish the target underneath
/// this one.
fn announce_outstanding_work(store_root: &Path) -> Result<(), Error> {
    let _writers = StoreLock::exclusive(store_root)?;
    super::super::desired::advance(store_root, 1)?;
    Ok(())
}

/// The filter as the file states it, in order. A reordering is not a change of
/// what gets embedded, so the comparison is over the set.
fn embed_types_of(config: Option<&Value>) -> std::collections::BTreeSet<String> {
    config
        .and_then(|value| value.get("embed_types"))
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// A type name nobody registered is a filter that will never match, and the
/// saving it promises never happens while the cost stays. Checked here rather
/// than in the loader: types are immutable once registered, but a store must
/// still open when the list was written against a registry it can read, and a
/// misspelling is something only the author of this file can fix.
fn embed_types_are_registered(store_root: &Path, candidate: &Value) -> Result<(), Error> {
    let Some(wanted) = candidate.get("embed_types").and_then(Value::as_array) else {
        return Ok(());
    };
    let registered = crate::schema::list(store_root)?
        .types
        .into_iter()
        .map(|summary| summary.type_name)
        .collect::<Vec<_>>();
    for name in wanted.iter().filter_map(Value::as_str) {
        if !registered.iter().any(|known| known == name) {
            return Err(vector_error(&format!(
                "embed_types names a type this store has not registered: {name}"
            )));
        }
    }
    Ok(())
}

/// Disabling keeps the descriptor so a later enable does not have to rebuild
/// the file, and immediately makes the projection report Disabled.
pub fn disable(store_root: &Path, actor: &str) -> Result<VectorConfigReport, Error> {
    let (_guard, _config) = RootGuard::acquire(store_root, actor)?;
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
