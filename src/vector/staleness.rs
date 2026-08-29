use super::model::vector_error;
use crate::kernel::error::Error;
use serde_json::Value;
use std::fs;
use std::path::Path;

const STATE: &str = "projections/qdrant/state.json";

/// A durable record write always wins. Once a record is on disk the vector
/// index is behind by definition, so the marker is demoted from ready to
/// degraded and readers fall back to full-text search until a sync or rebuild.
///
/// The marker is rewritten as opaque JSON rather than through the state
/// structure: demoting must not depend on this module agreeing with the
/// provider about every field it stores.
///
/// Failure is reported, never propagated as a write failure: the caller has
/// already committed the ledger, and unwinding immutable history because a
/// disposable projection could not be annotated would be the worse outcome.
pub fn mark_stale(store_root: &Path) -> Result<(), Error> {
    let path = store_root.join(STATE);
    if !path.is_file() {
        return Ok(());
    }
    let mut marker: Value = serde_json::from_slice(&fs::read(&path)?)?;
    let object = marker
        .as_object_mut()
        .ok_or_else(|| vector_error("state marker is not an object"))?;
    if object.get("state").and_then(Value::as_str) == Some("degraded") {
        return Ok(());
    }
    object.insert("state".into(), Value::String("degraded".into()));
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec(&marker)?)?;
    fs::rename(&temporary, &path).map_err(|_| vector_error("state marker could not be demoted"))?;
    Ok(())
}

/// Convenience for write paths: the ledger is already durable, so a failure to
/// demote is surfaced as a diagnostic string instead of an error.
pub fn note_stale(store_root: &Path) -> Option<String> {
    mark_stale(store_root)
        .err()
        .map(|error| format!("vector projection was not demoted: {error}"))
}
