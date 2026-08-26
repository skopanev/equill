use crate::record::EvidenceRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyRecord {
    pub id: String,
    #[serde(rename = "ts")]
    pub legacy_recorded_at: String,
    pub namespace: String,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(rename = "actor")]
    pub legacy_actor: String,
    pub observed_at: String,
    #[serde(default)]
    pub valid_at: Option<String>,
    pub payload: Value,
    #[serde(default)]
    pub evidence: Vec<LegacyEvidence>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub supersedes: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum LegacyEvidence {
    Text(String),
    Typed(EvidenceRef),
}

#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub ok: bool,
    pub input_sha256: String,
    pub total: usize,
    pub imported: usize,
    pub skipped: usize,
    pub records: Vec<ImportItem>,
}

#[derive(Debug, Serialize)]
pub struct ImportItem {
    pub line: usize,
    pub legacy_id: String,
    pub record_id: Uuid,
    pub status: ImportStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportStatus {
    Imported,
    Skipped,
}
