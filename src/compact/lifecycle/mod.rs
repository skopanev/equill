use super::planner::SourceRecord;
use crate::ingest::model::LegacyEvidence;
use crate::kernel::error::Error;
use crate::record::{EvidenceRef, StoredRecord};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

pub fn validate(store: &Path, records: &[SourceRecord]) -> Result<(), Error> {
    let ids = records
        .iter()
        .map(|record| (record.id.as_str(), Uuid::now_v7()))
        .collect::<HashMap<_, _>>();
    let graph = records
        .iter()
        .map(|record| stored(record, &ids))
        .collect::<Result<Vec<_>, _>>()?;
    crate::record::lifecycle::validate_graph(store, &graph)
        .map_err(|error| Error::Compact(format!("record lifecycle: {error}")))
}

fn stored(record: &SourceRecord, ids: &HashMap<&str, Uuid>) -> Result<StoredRecord, Error> {
    let supersedes = record
        .supersedes
        .as_deref()
        .map(|target| resolve(record, target, ids))
        .transpose()?;
    Ok(StoredRecord {
        id: ids[record.id.as_str()],
        namespace: record.namespace.clone(),
        type_name: record.type_name.clone(),
        actor: record.actor.clone(),
        recorded_at: record.recorded_at.clone(),
        observed_at: record.observed_at.clone(),
        valid_at: record
            .valid_at
            .clone()
            .unwrap_or_else(|| record.observed_at.clone()),
        payload: record.payload.clone(),
        evidence: record.evidence.iter().map(evidence).collect(),
        tags: record.tags.clone(),
        supersedes,
    })
}

/// A plan is applied by importing the manifest into an empty shadow store, so the
/// manifest is the whole world an edge may point into. A raw uuid naming a record
/// the live store holds is still unknown here: accepting it would let a dry run
/// promise an apply that the shadow import then refuses.
fn resolve(record: &SourceRecord, target: &str, ids: &HashMap<&str, Uuid>) -> Result<Uuid, Error> {
    ids.get(target).copied().ok_or_else(|| {
        Error::Compact(format!(
            "record {} has unknown supersedes target {target}",
            record.id
        ))
    })
}

fn evidence(source: &LegacyEvidence) -> EvidenceRef {
    match source {
        LegacyEvidence::Text(reference) => EvidenceRef {
            kind: "legacy.evidence".into(),
            reference: reference.clone(),
            sha256: None,
        },
        LegacyEvidence::Typed(reference) => EvidenceRef {
            kind: reference.kind.clone(),
            reference: reference.reference.clone(),
            sha256: reference.sha256.clone(),
        },
    }
}

#[cfg(test)]
mod tests;
