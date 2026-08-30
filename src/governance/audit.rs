use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::record::{RecordDraft, append_only};
use crate::schema::{self, TypeDefinition};
use jiff::Timestamp;
use serde_json::{Value, json};
use std::path::Path;
use uuid::Uuid;

pub const TYPE: &str = "equill.governance.v1";
const URI: &str = "equill://equill.governance/v1";

/// Hash-only by construction: an action, who it was about, the transaction it
/// belongs to, and the metadata digests it moved between. The reason is
/// attested by digest rather than quoted, so the ledger records that a reason
/// was given without carrying its text.
fn payload_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "action": { "enum": ["grant-add", "grant-revoke", "owner-transfer"] },
            "subject": { "type": "string", "minLength": 1 },
            "tx_id": { "type": "string", "minLength": 1 },
            "store_sha256_before": { "type": "string", "pattern": "^[0-9a-f]{64}$" },
            "store_sha256_after": { "type": "string", "pattern": "^[0-9a-f]{64}$" },
            "comment_sha256": { "type": "string", "pattern": "^[0-9a-f]{64}$" }
        },
        "required": [
            "action", "subject", "tx_id", "store_sha256_before", "store_sha256_after"
        ],
        "additionalProperties": false
    })
}

/// The namespace the audit is written to: the store's first, which every store
/// has because `init` requires one. Governance does not invent a namespace of
/// its own — adding one would mutate the very metadata it is about to change,
/// before the transaction that is supposed to describe the change has begun.
pub(super) fn namespace(store_root: &Path) -> Result<String, Error> {
    crate::kernel::store::load(store_root)?
        .namespaces
        .into_iter()
        .next()
        .ok_or(Error::InvalidNamespace)
}

/// Register the engine-owned audit type if it is absent. Idempotent, and it
/// touches no metadata: a schema lives in the registry, not in store.json.
pub(super) fn prepare(store_root: &Path, owner: &str) -> Result<(), Error> {
    let path = store_root.join(format!("registry/types/{TYPE}.json"));
    if path.is_file() {
        // Present is not the same as correct. A file under this type name that
        // does not match the built-in definition means something else is using
        // the engine's audit type, and writing governance history against a
        // schema we did not author would make that history unverifiable.
        let existing: TypeDefinition = serde_json::from_slice(&std::fs::read(&path)?)
            .map_err(|_| Error::Integrity("governance audit schema is unreadable".into()))?;
        if existing.type_name != TYPE
            || existing.uri != URI
            || existing.payload_schema != payload_schema()
            || existing.lifecycle != Default::default()
        {
            return Err(Error::Integrity(
                "governance audit schema does not match the built-in definition".into(),
            ));
        }
        return Ok(());
    }
    schema::register_authorized(
        store_root,
        TypeDefinition {
            type_name: TYPE.into(),
            uri: URI.into(),
            owner: owner.to_owned(),
            payload_schema: payload_schema(),
            lifecycle: Default::default(),
        },
    )
    .map(|_| ())
}

/// Written after the journal and before the metadata swap. A transfer strips
/// the old owner of both root and writer access, so once the swap lands the
/// only identity that could still explain it no longer has permission to write.
pub(super) fn record(
    store_root: &Path,
    owner: &str,
    tx_id: Uuid,
    action: &str,
    subject: &str,
    digests: (&str, &str),
    comment: Option<&str>,
) -> Result<Uuid, Error> {
    let mut payload = json!({
        "action": action,
        "subject": subject,
        "tx_id": tx_id.to_string(),
        "store_sha256_before": digests.0,
        "store_sha256_after": digests.1
    });
    if let Some(comment) = comment {
        payload["comment_sha256"] = json!(sha256_hex(comment.as_bytes()));
    }
    let report = append_only(
        store_root,
        RecordDraft {
            namespace: namespace(store_root)?,
            type_name: TYPE.into(),
            observed_at: Timestamp::now().to_string(),
            valid_at: None,
            payload,
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: None,
        },
        owner,
    )?;
    Ok(report.id)
}

/// Every governance record in the ledger that names this transaction.
///
/// Recovery needs the records themselves, not a count: what the ledger says
/// about a transaction is the authority on what it was, and the journal is only
/// a convenience copy. Cardinality still matters, so this returns all matches
/// rather than the first.
pub(super) fn for_transaction(store_root: &Path, tx_id: Uuid) -> Result<Vec<Value>, Error> {
    let wanted = tx_id.to_string();
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(store_root.join("records")) else {
        return Ok(found);
    };
    for entry in entries.flatten() {
        let text = std::fs::read_to_string(entry.path())?;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let value: Value = serde_json::from_str(line)
                .map_err(|_| Error::Integrity("ledger line is unreadable".into()))?;
            if value["type"] == TYPE && value["payload"]["tx_id"] == wanted.as_str() {
                found.push(value);
            }
        }
    }
    Ok(found)
}
