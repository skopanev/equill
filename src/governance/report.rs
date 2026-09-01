use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct OwnerReport {
    pub ok: bool,
    pub previous_owner: String,
    pub owner: String,
    /// Every form of append the previous owner lost: the store-wide writer
    /// entry, and how many scoped grants named them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub revoked_writers: Vec<String>,
    pub audit_record: Uuid,
    pub store_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct GrantReport {
    pub ok: bool,
    pub actor: String,
    pub grants: usize,
    /// False when the grant was already present: the call is idempotent, and
    /// saying so is more useful than pretending something changed.
    pub changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit_record: Option<Uuid>,
    pub store_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct AuthorityReport {
    pub ok: bool,
    pub owner: String,
    pub writers: Vec<String>,
    /// Actors the store holds to reading, sorted. Listed beside the writers
    /// because a reader of this report is asking "who may change this", and an
    /// answer that shows only the grants would be answering half of it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub read_only: Vec<String>,
    pub grants: Vec<GrantView>,
    pub store_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct GrantView {
    pub actors: Vec<String>,
    pub namespace: String,
    pub types: Vec<String>,
}

/// The outcome of naming, or unnaming, an actor that may only read.
#[derive(Debug, Serialize)]
pub struct ReaderReport {
    pub ok: bool,
    pub actor: String,
    pub readers: usize,
    /// False when the store already said this: the call is idempotent, and
    /// saying so is more useful than pretending something changed.
    pub changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audit_record: Option<Uuid>,
    pub store_sha256: String,
}
