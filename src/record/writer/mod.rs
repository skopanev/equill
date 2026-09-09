mod atomic;
mod authority;
mod blocked;
mod commit;
mod operation;
mod prepare;
mod revocation;
#[cfg(test)]
mod seam;
#[cfg(test)]
mod tests;
mod transaction;

use super::{AppendReport, AppendRequest, RecordDraft, StoredRecord};
use crate::kernel::{error::Error, lock::StoreLock};
use crate::schema;
use jiff::Timestamp;
use prepare::Preparation;
use std::fs;
use std::path::Path;
use uuid::Uuid;

pub(crate) use atomic::{AtomicDraft, AtomicScope, append_atomic};
#[cfg(test)]
pub(crate) use authority::require_current_writer;
pub(crate) use operation::reconcile as reconcile_operations;
pub use revocation::append_only;
pub(crate) use revocation::append_revocation;

pub fn append_file(root: &Path, source: &Path, actor: &str) -> Result<AppendReport, Error> {
    let draft: RecordDraft = serde_json::from_slice(&fs::read(source)?)?;
    append(root, draft, actor)
}

pub fn append(root: &Path, draft: RecordDraft, actor: &str) -> Result<AppendReport, Error> {
    append_request(
        root,
        AppendRequest {
            draft,
            idempotency_key: None,
        },
        actor,
    )
}

pub fn append_request(
    root: &Path,
    request: AppendRequest,
    actor: &str,
) -> Result<AppendReport, Error> {
    let key = request
        .idempotency_key
        .as_deref()
        .map(|key| operation::Key::new(actor, key, &request.draft))
        .transpose()?;
    let (mut report, _, replayed) = confirm_inner(root, request.draft, actor, None, key.as_ref())?;
    report.vector = crate::vector::after_commit(root, u64::from(key.is_none() && !replayed));
    Ok(report)
}

pub fn append_only_request(
    root: &Path,
    request: AppendRequest,
    actor: &str,
) -> Result<AppendReport, Error> {
    let key = request
        .idempotency_key
        .as_deref()
        .map(|key| operation::Key::new(actor, key, &request.draft))
        .transpose()?;
    confirm_inner(root, request.draft, actor, None, key.as_ref()).map(|(report, _, _)| report)
}

fn confirm(
    root: &Path,
    draft: RecordDraft,
    actor: &str,
    revoking: Option<&StoredRecord>,
) -> Result<(AppendReport, StoredRecord), Error> {
    confirm_inner(root, draft, actor, revoking, None).map(|(report, record, _)| (report, record))
}

fn confirm_inner(
    root: &Path,
    draft: RecordDraft,
    actor: &str,
    revoking: Option<&StoredRecord>,
    key: Option<&operation::Key>,
) -> Result<(AppendReport, StoredRecord, bool), Error> {
    // The request digest above excludes generated coordinates and timestamps.
    let recorded_at = Timestamp::now().to_string();
    let prepared = match prepare::prepare(
        root,
        draft,
        actor,
        revoking,
        Uuid::now_v7(),
        recorded_at.clone(),
    )? {
        Preparation::Allowed(prepared) => *prepared,
        Preparation::Blocked(blocked) => {
            let (draft, defense) = *blocked;
            return blocked::block_write(
                root,
                &draft,
                actor,
                &recorded_at,
                &revocation::month(&recorded_at)?,
                defense,
            )
            .map(|_| unreachable!("a blocked draft is refused"));
        }
    };
    #[cfg(test)]
    seam::before_lock(root);
    let _lock = StoreLock::exclusive(root)?;
    authority::require_current_record_scope(root, actor, &prepared.record)?;
    recover(root)?;
    let mut lifecycle = match super::lifecycle::load_state(root)? {
        Some(state) => state,
        None => super::lifecycle::rebuild_state(root)?,
    };
    // Reauthorization precedes a cached outcome as well as a new append.
    if let Some(key) = key
        && let Some((report, record)) = operation::lookup(root, key)?
    {
        authority::require_current_record_writer(root, actor, &record, &lifecycle)?;
        return Ok((report, record, true));
    }
    authority::require_current_record_writer(root, actor, &prepared.record, &lifecycle)?;
    let claiming = super::lifecycle::registered_types(root)?;
    validate_lifecycle(root, &prepared, &lifecycle, &claiming)?;
    lifecycle.record(
        &prepared.record,
        super::lifecycle::keys_of(&prepared.record, &claiming),
    )?;
    let record = prepared.record.clone();
    let report = commit::records(root, vec![prepared], &mut lifecycle, key, false)?.remove(0);
    Ok((report, record, false))
}

pub(crate) fn recover(root: &Path) -> Result<(), Error> {
    transaction::recover(root)?;
    operation::recover(root)?;
    super::receipt::resolve_pending(root)
}

fn validate_lifecycle(
    root: &Path,
    prepared: &prepare::Prepared,
    state: &super::lifecycle::LifecycleState,
    claiming: &[(String, schema::TypeDefinition)],
) -> Result<(), Error> {
    let target = prepared
        .record
        .supersedes
        .and_then(|id| state.entries.get(&id))
        .map(|entry| schema::load(root, &entry.type_name))
        .transpose()?;
    super::lifecycle::validate_append_against(
        state,
        &prepared.record,
        &prepared.definition,
        target.as_ref(),
        claiming,
    )
}
