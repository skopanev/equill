use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::projection::ProjectionState;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordDraft {
    pub namespace: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub observed_at: String,
    #[serde(default)]
    pub valid_at: Option<String>,
    pub payload: Value,
    #[serde(default)]
    pub evidence: Vec<EvidenceRef>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub supersedes: Option<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub kind: String,
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoredRecord {
    pub id: Uuid,
    pub namespace: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub actor: String,
    pub recorded_at: String,
    pub observed_at: String,
    pub valid_at: String,
    pub payload: Value,
    pub evidence: Vec<EvidenceRef>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct AppendReport {
    pub ok: bool,
    /// What the vector catch-up managed after this write. Never a reason for
    /// the write itself to fail.
    #[serde(skip_serializing_if = "quiet")]
    pub vector: crate::vector::DrainReport,
    /// Records the store already holds that look like this one. Advisory: the
    /// write succeeded, and the author decides what to do about it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub similar: Vec<super::SimilarRecord>,
    pub id: Uuid,
    pub sha256: String,
    pub ledger: String,
    pub receipt: String,
    pub redacted: bool,
    pub projection: ProjectionState,
}

/// A drain that did nothing and had nothing to say stays out of the report.
fn quiet(report: &crate::vector::DrainReport) -> bool {
    !report.ran && report.attempt_error.is_none()
}
