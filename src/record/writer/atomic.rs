use super::{commit, prepare, recover, validate_lifecycle};
use crate::kernel::{error::Error, identity, lock::StoreLock, store};
use crate::record::{RecordDraft, StoredRecord, lifecycle};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

pub(crate) struct AtomicDraft {
    pub id: Uuid,
    pub line: usize,
    pub draft: RecordDraft,
}

/// Every requested line, including operations that immutable truth may skip.
pub(crate) struct AtomicScope {
    pub line: usize,
    pub namespace: String,
    pub type_name: String,
    pub payload: serde_json::Value,
}

/// Import plans against the writer's one verified snapshot. All preparation and
/// lifecycle/grant checks complete before any receipt, journal or record exists.
pub(crate) fn append_atomic<T>(
    root: &Path,
    actor: &str,
    requested: &[AtomicScope],
    build: impl FnOnce(&[StoredRecord]) -> Result<(Vec<AtomicDraft>, T), Error>,
) -> Result<T, Error> {
    authorize(root, actor, requested)?;
    let lock = StoreLock::exclusive(root)?;
    authorize(root, actor, requested)?;
    recover(root)?;
    let mut truth = crate::record::read_all_exclusive(root)?;
    let (drafts, report) = build(&truth)?;
    if drafts.is_empty() {
        if crate::projection::watermark(root)
            .is_none_or(|watermark| watermark.indexed_records != truth.len())
        {
            publish(root, &truth);
        }
        drop(lock);
        crate::vector::after_commit(root, 0);
        return Ok(report);
    }
    let config = store::load(root)?;
    let claiming = lifecycle::registered_types(root)?;
    let mut state = lifecycle::from_records(&truth, &claiming)?;
    let mut known = truth
        .iter()
        .map(|record| (record.id, record.clone()))
        .collect::<HashMap<_, _>>();
    let recorded_at = jiff::Timestamp::now().to_string();
    let mut prepared = Vec::with_capacity(drafts.len());
    for candidate in drafts {
        let prepare_one = || {
            if known.contains_key(&candidate.id)
                || candidate.id.get_version() != Some(uuid::Version::SortRand)
            {
                return Err(Error::InvalidRecord(
                    "duplicate or invalid batch coordinate".into(),
                ));
            }
            let prepared = match prepare::prepare(
                root,
                candidate.draft,
                actor,
                None,
                candidate.id,
                recorded_at.clone(),
            )? {
                prepare::Preparation::Allowed(prepared) => *prepared,
                prepare::Preparation::Blocked(_) => {
                    return Err(Error::MemoryDefense("batch draft blocked".into()));
                }
            };
            let record = &prepared.record;
            let predecessor = record
                .supersedes
                .map(|id| {
                    known.get(&id).ok_or_else(|| {
                        Error::InvalidRecord(format!("unknown supersedes coordinate {id}"))
                    })
                })
                .transpose()?;
            identity::require_record_writer(
                &config,
                actor,
                target(record),
                predecessor.map(target),
            )?;
            validate_lifecycle(root, &prepared, &state, &claiming)?;
            Ok(prepared)
        };
        let item = prepare_one()
            .map_err(|error| Error::Import(format!("line {}: {error}", candidate.line)))?;
        state.record(&item.record, lifecycle::keys_of(&item.record, &claiming))?;
        known.insert(item.record.id, item.record.clone());
        truth.push(item.record.clone());
        prepared.push(item);
    }
    commit::records(root, prepared, &mut state, None, true)?;
    #[cfg(test)]
    super::seam::at(super::seam::Step::BeforeProjection)?;
    // One provider transaction covers the snapshot already in memory. A stale
    // earlier projection is repaired without a second read of immutable truth.
    publish(root, &truth);
    drop(lock);
    crate::vector::after_commit(root, 0);
    Ok(report)
}

fn authorize(root: &Path, actor: &str, requested: &[AtomicScope]) -> Result<(), Error> {
    let config = store::load(root)?;
    if requested.is_empty() {
        // No input scope exists to establish a scoped recovery authorization.
        return identity::require_writer(&config, actor);
    }
    for scope in requested {
        identity::require_record_writer(
            &config,
            actor,
            identity::WriteTarget {
                namespace: &scope.namespace,
                type_name: &scope.type_name,
                payload: &scope.payload,
            },
            None,
        )
        .map_err(|error| Error::Import(format!("line {}: {error}", scope.line)))?;
    }
    Ok(())
}

fn publish(root: &Path, truth: &[StoredRecord]) {
    if crate::projection::index_batch(root, truth).is_err()
        && let Some(last) = truth.last()
    {
        crate::projection::mark_degraded(root, last, "atomic batch projection failed");
    }
}

fn target(record: &StoredRecord) -> identity::WriteTarget<'_> {
    identity::WriteTarget {
        namespace: &record.namespace,
        type_name: &record.type_name,
        payload: &record.payload,
    }
}
