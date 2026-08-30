mod graph;
mod state;

mod watermark;

#[cfg(test)]
pub(crate) use graph::validate_append;
pub(crate) use graph::validate_graph;
pub(crate) use state::{LifecycleState, load as load_state, save as save_state};

use super::StoredRecord;
use crate::kernel::error::Error;
use crate::schema::{self, LifecycleMode, TypeDefinition};
use std::path::Path;
fn invalid(reason: impl Into<String>) -> Error {
    Error::InvalidRecord(reason.into())
}
/// Validate a candidate against compact state instead of the whole ledger.
///
/// The rules are the ones above, unchanged. What changes is what they consult:
/// the full-graph revalidation that `validate_append` performs on every write is
/// checking what every earlier write already checked, so the invariant is
/// maintained inductively rather than rediscovered. What a new record can break
/// is only ever local — its own edge, and the head rules for its own key — and
/// that is exactly what this checks.
///
/// A caller with no usable state must fall back to the ledger. Refusing to
/// validate at all would be safe and useless; validating against state that no
/// longer describes the ledger would be neither.
pub(crate) fn validate_append_against(
    state: &LifecycleState,
    candidate: &StoredRecord,
    definition: &TypeDefinition,
    target_definition: Option<&TypeDefinition>,
    claiming: &[(String, TypeDefinition)],
) -> Result<(), Error> {
    let target = candidate
        .supersedes
        .map(|id| {
            state
                .entries
                .get(&id)
                .ok_or_else(|| invalid(format!("supersedes target is unknown: {id}")))
                .map(|entry| (id, entry))
        })
        .transpose()?;

    if let Some((id, entry)) = target {
        if candidate.id == id {
            return Err(invalid(format!(
                "record {} supersedes itself",
                candidate.id
            )));
        }
        if definition.lifecycle.mode == LifecycleMode::AppendOnly {
            return Err(invalid(format!(
                "type {} is append_only and cannot supersede records",
                candidate.type_name
            )));
        }
        if candidate.namespace != entry.namespace {
            return Err(invalid("supersedes cannot cross namespaces"));
        }
        if candidate.type_name != entry.type_name
            && !definition
                .lifecycle
                .allowed_predecessor_types
                .contains(&entry.type_name)
        {
            return Err(invalid(format!(
                "type {} cannot supersede predecessor type {}",
                candidate.type_name, entry.type_name
            )));
        }
        // A type declares append_only for its own records. Reading that as a
        // rule about successors only would let any other type erase it.
        if target_definition
            .is_some_and(|target| target.lifecycle.mode == LifecycleMode::AppendOnly)
        {
            return Err(invalid(format!(
                "type {} is append_only and cannot be superseded",
                entry.type_name
            )));
        }
        let keys = (
            key(candidate, definition),
            entry.keys.get(&candidate.type_name),
        );
        if definition.lifecycle.mode == LifecycleMode::Linear
            && matches!(keys, (Some(left), Some(right)) if left != right)
        {
            return Err(invalid("linear supersedes requires the same lifecycle key"));
        }
    }

    if definition.lifecycle.mode == LifecycleMode::Linear {
        let candidate_key = require_key(candidate, definition)?;
        if let Some((id, _)) = target
            && !state.head(&id)
        {
            return Err(invalid("linear supersedes target is not the current head"));
        }
        let heads = state.heads_claiming(&candidate.type_name, &candidate.namespace, candidate_key);
        match target {
            None if !heads.is_empty() => {
                return Err(invalid(
                    "linear lifecycle key already has a current head; supersedes is required",
                ));
            }
            Some((id, _)) if heads.iter().any(|head| head != &id) => {
                return Err(invalid(
                    "linear supersedes would leave more than one current head",
                ));
            }
            _ => {}
        }
    }

    // A record of any mode can break a linear type's head uniqueness, because a
    // linear type counts heads across every type it accepts as a predecessor.
    // Migrating an old record to a new linear type and then writing another of
    // the old type is exactly that, and checking only the candidate's own mode
    // would let it through.
    for (linear_type, linear) in claimants(candidate, definition, claiming) {
        let Some(candidate_key) = key(candidate, linear) else {
            continue;
        };
        let heads = state.heads_claiming(linear_type, &candidate.namespace, candidate_key);
        let superseding = target.map(|(id, _)| id);
        if heads.iter().any(|head| Some(*head) != superseding) {
            return Err(invalid("linear lifecycle key has multiple current heads"));
        }
    }
    Ok(())
}

/// The linear types that would count this record as one of their heads: the
/// record's own type when it is linear, and any linear type that accepts it as
/// a predecessor.
fn claimants<'a>(
    candidate: &StoredRecord,
    definition: &TypeDefinition,
    claiming: &'a [(String, TypeDefinition)],
) -> Vec<(&'a str, &'a TypeDefinition)> {
    claiming
        .iter()
        .filter(|(_, linear)| linear.lifecycle.mode == LifecycleMode::Linear)
        .filter(|(name, linear)| {
            *name == candidate.type_name
                || linear
                    .lifecycle
                    .allowed_predecessor_types
                    .contains(&candidate.type_name)
        })
        // The candidate's own type, when linear, was already checked above with
        // the rules that know about supersedes; checking it again here would
        // reject a legitimate replacement as a second head.
        .filter(|(name, _)| {
            !(*name == candidate.type_name && definition.lifecycle.mode == LifecycleMode::Linear)
        })
        .map(|(name, linear)| (name.as_str(), linear))
        .collect()
}

/// Every key this record presents, one per linear type that could claim it.
pub(crate) fn keys_of(
    record: &StoredRecord,
    claiming: &[(String, TypeDefinition)],
) -> std::collections::BTreeMap<String, serde_json::Value> {
    claiming
        .iter()
        .filter(|(_, linear)| linear.lifecycle.mode == LifecycleMode::Linear)
        .filter(|(name, linear)| {
            *name == record.type_name
                || linear
                    .lifecycle
                    .allowed_predecessor_types
                    .contains(&record.type_name)
        })
        .filter_map(|(name, linear)| key(record, linear).map(|value| (name.clone(), value.clone())))
        .collect()
}

/// Every registered type and its definition. Reads the type registry, which is
/// a handful of small files — not the ledger.
pub(crate) fn registered_types(store: &Path) -> Result<Vec<(String, TypeDefinition)>, Error> {
    let directory = store.join("registry/types");
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Ok(found);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|value| value == "json")
            && let Some(name) = path.file_stem().and_then(|stem| stem.to_str())
            && let Ok(definition) = schema::load(store, name)
        {
            found.push((name.to_owned(), definition));
        }
    }
    Ok(found)
}

/// Build state from the ledger. The one place a full read is still correct —
/// a store with no usable state has nothing else to build from.
pub(crate) fn rebuild_state(store: &Path) -> Result<LifecycleState, Error> {
    let claiming = registered_types(store)?;
    let mut built = state::empty();
    for record in super::read_all(store)? {
        let keys = keys_of(&record, &claiming);
        built.record(&record, keys);
    }
    Ok(built)
}

fn eligible(candidate: &StoredRecord, record: &StoredRecord, definition: &TypeDefinition) -> bool {
    candidate.namespace == record.namespace
        && (candidate.type_name == record.type_name
            || definition
                .lifecycle
                .allowed_predecessor_types
                .contains(&record.type_name))
}

/// A record of an allowed predecessor type need not carry the successor's
/// lifecycle key. Such a record claims no linear head, so it is skipped rather
/// than rejected; only the linear type itself must resolve its own key.
fn key<'a>(record: &'a StoredRecord, definition: &TypeDefinition) -> Option<&'a serde_json::Value> {
    definition
        .lifecycle
        .key_pointer
        .as_deref()
        .and_then(|pointer| record.payload.pointer(pointer))
}

fn require_key<'a>(
    record: &'a StoredRecord,
    definition: &TypeDefinition,
) -> Result<&'a serde_json::Value, Error> {
    let pointer = definition.lifecycle.key_pointer.as_deref().unwrap_or("/");
    key(record, definition).ok_or_else(|| {
        invalid(format!(
            "record {} is missing lifecycle key {pointer}",
            record.id
        ))
    })
}
