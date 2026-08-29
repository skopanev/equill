use super::{EvidenceRef, RecordDraft, StoredRecord, append, read_all};
use crate::kernel::error::Error;
use serde::Serialize;
use std::path::Path;
use uuid::Uuid;

pub const REVOKED_TAG: &str = "equill:revoked";
const COMMENT_KIND: &str = "equill.revocation.comment";

#[derive(Debug, Serialize)]
pub struct RevokeReport {
    pub ok: bool,
    pub revoked: Uuid,
    pub tombstone: Uuid,
    pub ledger: String,
    pub receipt: String,
}

/// Withdrawing a claim was possible but not offered: a caller had to hand-build
/// a record that repeated the original payload, remembered the tag and pointed
/// `supersedes` at the right id. Getting any of that wrong produced something
/// that looked like a retraction and did not suppress anything.
///
/// Nothing is deleted. The tombstone is an ordinary record — same namespace,
/// same type, the target's own payload — written through the same
/// grant-checked immutable writer, so every rule that governs an append
/// governs a revocation: an append_only type refuses it, a stale head refuses
/// it, and an actor the store does not allow is refused here too.
pub fn revoke(
    store_root: &Path,
    id: Uuid,
    comment: Option<&str>,
    actor: &str,
) -> Result<RevokeReport, Error> {
    let records = read_all(store_root)?;
    let target = records
        .iter()
        .find(|record| record.id == id)
        .ok_or_else(|| Error::InvalidRecord(format!("no record with id {id}")))?;
    if let Some(later) = records.iter().find(|record| record.supersedes == Some(id)) {
        return Err(Error::InvalidRecord(format!(
            "record {id} was already superseded by {}; revoke the current head instead",
            later.id
        )));
    }
    let report = append(store_root, tombstone(target, comment), actor)?;
    Ok(RevokeReport {
        ok: true,
        revoked: id,
        tombstone: report.id,
        ledger: report.ledger.clone(),
        receipt: report.receipt.clone(),
    })
}

/// The payload is copied rather than rewritten: a retraction says "no longer",
/// not "instead", and inventing a replacement claim would put words in the
/// author's mouth. The reason, when given, is evidence about the revocation —
/// it never touches the payload the type declared.
fn tombstone(target: &StoredRecord, comment: Option<&str>) -> RecordDraft {
    let mut tags = target.tags.clone();
    if !tags.iter().any(|tag| tag == REVOKED_TAG) {
        tags.push(REVOKED_TAG.to_owned());
    }
    let mut evidence = target.evidence.clone();
    if let Some(reason) = comment.map(str::trim).filter(|text| !text.is_empty()) {
        evidence.push(EvidenceRef {
            kind: COMMENT_KIND.into(),
            reference: reason.to_owned(),
            sha256: None,
        });
    }
    RecordDraft {
        namespace: target.namespace.clone(),
        type_name: target.type_name.clone(),
        observed_at: jiff::Timestamp::now().to_string(),
        valid_at: None,
        payload: target.payload.clone(),
        evidence,
        tags,
        supersedes: Some(target.id),
    }
}

#[cfg(test)]
mod tests;
