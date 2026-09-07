use super::{RecordDraft, StoredRecord};
use crate::kernel::error::Error;
use crate::kernel::identity::{self, WriteTarget};
use crate::kernel::store::{self, StoreConfig};
use std::path::Path;

pub(super) fn require_draft_writer(
    config: &StoreConfig,
    actor: &str,
    draft: &RecordDraft,
) -> Result<(), Error> {
    identity::require_record_writer(
        config,
        actor,
        WriteTarget {
            namespace: &draft.namespace,
            type_name: &draft.type_name,
            payload: &draft.payload,
        },
        None,
    )
}

/// Preserve the type-only recheck for legacy callers and its race regression.
#[cfg(test)]
pub(crate) fn require_current_writer(
    store: &Path,
    actor: &str,
    namespace: &str,
    type_name: &str,
) -> Result<(), Error> {
    identity::require_type_writer(&store::load(store)?, actor, namespace, type_name)
}

/// Re-read authority and bind both ends of a replacement to one grant.
pub(super) fn require_current_record_writer(
    store: &Path,
    actor: &str,
    record: &StoredRecord,
) -> Result<(), Error> {
    let config = store::load(store)?;
    let predecessor = record
        .supersedes
        .map(|id| {
            super::super::read_all(store)?
                .into_iter()
                .find(|item| item.id == id)
                .ok_or_else(|| Error::InvalidRecord(format!("supersedes target is unknown: {id}")))
        })
        .transpose()?;
    identity::require_record_writer(
        &config,
        actor,
        WriteTarget {
            namespace: &record.namespace,
            type_name: &record.type_name,
            payload: &record.payload,
        },
        predecessor.as_ref().map(|target| WriteTarget {
            namespace: &target.namespace,
            type_name: &target.type_name,
            payload: &target.payload,
        }),
    )
}
