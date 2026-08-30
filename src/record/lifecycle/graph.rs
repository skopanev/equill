//! The canonical full-ledger validation.
//!
//! Not the write path — the definition the write path is measured against. A
//! test can run this and the compact check over the same case and see them
//! agree, and doctor validates a whole store this way, where paying for the
//! entire ledger is the entire point rather than an accident.
use super::super::StoredRecord;
use super::{eligible, invalid, key, require_key};
use crate::kernel::error::Error;
use crate::schema::{self, LifecycleMode, TypeDefinition};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use uuid::Uuid;

/// The canonical full-ledger validation, kept as the reference the compact path
/// is judged against rather than as the path itself.
///
/// The writer no longer calls this: revalidating the whole graph on every append
/// re-checks what every earlier append already checked, and the cost of that
/// grows with the store. It stays because it is the definition of correct — a
/// test can run both over the same case and see them agree — and because doctor
/// still verifies a store this way, where paying for the whole ledger is the
/// entire point.
#[cfg(test)]
/// A candidate is judged against the graph it would create, not against itself:
/// the writer holds the store lock, so validating `records + candidate` here is
/// what keeps an append from storing something a canonical read would reject.
/// The edge checks run first because they name the offending coordinate.
pub fn validate_append(
    store: &Path,
    mut records: Vec<StoredRecord>,
    candidate: &StoredRecord,
    definition: &TypeDefinition,
) -> Result<(), Error> {
    #[cfg(test)]
    super::super::hotpath::lifecycle_walk();
    let target = candidate
        .supersedes
        .map(|id| find(&records, id))
        .transpose()?;
    validate_edge(candidate, target, definition)?;
    if definition.lifecycle.mode == LifecycleMode::Linear {
        validate_linear_append(&records, candidate, target, definition)?;
    }
    records.push(candidate.clone());
    validate_graph(store, &records)
}

pub fn validate_graph(store: &Path, records: &[StoredRecord]) -> Result<(), Error> {
    let by_id = records
        .iter()
        .map(|record| (record.id, record))
        .collect::<HashMap<_, _>>();
    let mut definitions = HashMap::new();
    for record in records {
        if !definitions.contains_key(&record.type_name) {
            definitions.insert(
                record.type_name.clone(),
                schema::load(store, &record.type_name)?,
            );
        }
    }
    let mut children = HashMap::<Uuid, usize>::new();
    for record in records {
        let definition = &definitions[&record.type_name];
        let target = record
            .supersedes
            .map(|id| {
                by_id
                    .get(&id)
                    .copied()
                    .ok_or_else(|| invalid(format!("supersedes target is unknown: {id}")))
            })
            .transpose()?;
        validate_edge(record, target, definition)?;
        if definition.lifecycle.mode == LifecycleMode::Linear {
            require_key(record, definition)?;
        }
        if let Some(target) = target {
            // A type declares append_only for its own records. Reading that as a
            // rule about successors only would let any other type erase it.
            if definitions[&target.type_name].lifecycle.mode == LifecycleMode::AppendOnly {
                return Err(invalid(format!(
                    "type {} is append_only and cannot be superseded",
                    target.type_name
                )));
            }
            *children.entry(target.id).or_default() += 1;
        }
    }
    reject_cycles(records, &by_id)?;
    validate_linear_graph(records, &definitions, &children)
}

fn validate_edge(
    record: &StoredRecord,
    target: Option<&StoredRecord>,
    definition: &TypeDefinition,
) -> Result<(), Error> {
    let Some(target) = target else {
        return Ok(());
    };
    if record.id == target.id {
        return Err(invalid(format!("record {} supersedes itself", record.id)));
    }
    if definition.lifecycle.mode == LifecycleMode::AppendOnly {
        return Err(invalid(format!(
            "type {} is append_only and cannot supersede records",
            record.type_name
        )));
    }
    if record.namespace != target.namespace {
        return Err(invalid("supersedes cannot cross namespaces"));
    }
    if record.type_name != target.type_name
        && !definition
            .lifecycle
            .allowed_predecessor_types
            .contains(&target.type_name)
    {
        return Err(invalid(format!(
            "type {} cannot supersede predecessor type {}",
            record.type_name, target.type_name
        )));
    }
    let keys = (key(record, definition), key(target, definition));
    if definition.lifecycle.mode == LifecycleMode::Linear
        && matches!(keys, (Some(left), Some(right)) if left != right)
    {
        return Err(invalid("linear supersedes requires the same lifecycle key"));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn validate_linear_append(
    records: &[StoredRecord],
    candidate: &StoredRecord,
    target: Option<&StoredRecord>,
    definition: &TypeDefinition,
) -> Result<(), Error> {
    let candidate_key = require_key(candidate, definition)?;
    let superseded = records
        .iter()
        .filter_map(|record| record.supersedes)
        .collect::<HashSet<_>>();
    if target.is_some_and(|record| superseded.contains(&record.id)) {
        return Err(invalid("linear supersedes target is not the current head"));
    }
    let matching_heads = records
        .iter()
        .filter(|record| !superseded.contains(&record.id))
        .filter(|record| eligible(candidate, record, definition))
        .filter(|record| key(record, definition) == Some(candidate_key))
        .map(|record| record.id)
        .collect::<Vec<_>>();
    match target {
        None if !matching_heads.is_empty() => Err(invalid(
            "linear lifecycle key already has a current head; supersedes is required",
        )),
        Some(target) if matching_heads.iter().any(|head| head != &target.id) => Err(invalid(
            "linear supersedes would leave more than one current head",
        )),
        _ => Ok(()),
    }
}

fn validate_linear_graph(
    records: &[StoredRecord],
    definitions: &HashMap<String, TypeDefinition>,
    children: &HashMap<Uuid, usize>,
) -> Result<(), Error> {
    let superseded = children.keys().copied().collect::<HashSet<_>>();
    let mut checked = HashSet::new();
    for record in records {
        let definition = &definitions[&record.type_name];
        if definition.lifecycle.mode != LifecycleMode::Linear {
            continue;
        }
        if record
            .supersedes
            .is_some_and(|target| children.get(&target).copied().unwrap_or(0) != 1)
        {
            return Err(invalid(
                "linear lifecycle has multiple children of one head",
            ));
        }
        if !checked.insert((record.namespace.as_str(), record.type_name.as_str())) {
            continue;
        }
        let mut keys = HashSet::new();
        for head in records
            .iter()
            .filter(|item| !superseded.contains(&item.id))
            .filter(|item| eligible(record, item, definition))
        {
            if key(head, definition).is_some_and(|value| !keys.insert(value.to_string())) {
                return Err(invalid("linear lifecycle key has multiple current heads"));
            }
        }
    }
    Ok(())
}

fn reject_cycles(
    records: &[StoredRecord],
    by_id: &HashMap<Uuid, &StoredRecord>,
) -> Result<(), Error> {
    for origin in records {
        let mut seen = HashSet::new();
        let mut current = origin;
        while let Some(parent) = current.supersedes {
            if !seen.insert(current.id) {
                return Err(invalid(format!("supersedes cycle contains {}", current.id)));
            }
            current = by_id[&parent];
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn find(records: &[StoredRecord], id: Uuid) -> Result<&StoredRecord, Error> {
    records
        .iter()
        .find(|record| record.id == id)
        .ok_or_else(|| invalid(format!("supersedes target is unknown: {id}")))
}
