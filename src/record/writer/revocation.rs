//! The one write that may exceed the store's word limit, and why nothing else
//! can reach it.
use super::confirm;
use crate::kernel::error::Error;
use crate::record::{AppendReport, RecordDraft, StoredRecord};
use std::path::Path;

/// Writing a tombstone for a record that predates the store's word limit.
///
/// A retraction repeats the claim it withdraws, so under a cap the store would
/// refuse to retract exactly the records the cap was introduced to discourage —
/// and the older the store, the more of them there would be. The exemption is
/// carried here, on a path no caller can reach, rather than by a tag or a flag
/// in a draft: anything a writer can set is something a writer can set to
/// bypass the limit.
///
/// It applies only when the payload is byte-identical to what is already
/// stored. A retraction that changed the claim is not a retraction, and it is
/// held to the cap like any other new writing.
/// Equality is decided on the payload as it will be stored, not as it arrived.
/// The defense may rewrite a draft on its way in, so comparing before that runs
/// answers about a payload the store never keeps — and the exemption would then
/// be granted, or withheld, on the strength of text nobody will ever read back.
pub(crate) fn append_revocation(
    store_root: &Path,
    draft: RecordDraft,
    actor: &str,
    target: &StoredRecord,
) -> Result<AppendReport, Error> {
    let (mut report, _record) = confirm(store_root, draft, actor, Some(target))?;
    // A revocation changes what a search should return, so it owes the
    // projection the same nudge every other append gives it. Returning the
    // confirmation directly left withdrawn records sitting in the index until
    // something unrelated happened to wake the worker.
    report.vector = crate::vector::after_commit(store_root, 1);
    Ok(report)
}

pub fn append_only(
    store_root: &Path,
    draft: RecordDraft,
    actor: &str,
) -> Result<AppendReport, Error> {
    confirm(store_root, draft, actor, None).map(|(report, _)| report)
}

pub(super) fn month(timestamp: &str) -> Result<String, Error> {
    timestamp
        .get(..7)
        .map(str::to_owned)
        .ok_or_else(|| Error::InvalidRecord("system clock is out of range".into()))
}
