use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct DeepFinding {
    pub record_id: Uuid,
    pub ledger: String,
    pub ledger_line: usize,
    pub rule: String,
    pub content_line: usize,
    pub content_column: usize,
}

#[derive(Debug, Serialize)]
pub struct DeepReport {
    pub records: usize,
    pub findings: usize,
    pub receipt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alert: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuditStatus {
    Clean,
    AttentionRequired,
}

#[derive(Serialize)]
pub struct AuditReceipt<'a> {
    pub scan_id: &'a str,
    pub status: AuditStatus,
    pub catalog: &'static str,
    pub scanned_at: &'a str,
    pub corpus_sha256: &'a str,
    pub records: usize,
    pub findings: &'a [DeepFinding],
}

#[derive(Serialize)]
pub struct AuditAlert<'a> {
    pub scan_id: &'a str,
    pub receipt: &'a str,
    pub findings: usize,
    pub action: &'static str,
}
