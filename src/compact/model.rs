use crate::ingest::manifest::ManifestEntry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorState {
    Alive,
    Dead,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactReason {
    Superseded,
    Expired,
    DeadAnchor,
    ActiveDescendant,
    ExpiryWarningWindow,
    ActiveAnchor,
    UnknownAnchor,
}

impl Display for CompactReason {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Superseded => "superseded",
            Self::Expired => "expired",
            Self::DeadAnchor => "dead_anchor",
            Self::ActiveDescendant => "active_descendant",
            Self::ExpiryWarningWindow => "expiry_warning_window",
            Self::ActiveAnchor => "active_anchor",
            Self::UnknownAnchor => "unknown_anchor",
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RecordDecision {
    pub id: String,
    pub reason: CompactReason,
}

#[derive(Clone, Debug, Serialize)]
pub struct InputPlan {
    pub path: String,
    pub before_sha256: String,
    pub after_sha256: String,
    pub removals: Vec<RecordDecision>,
    pub retained: Vec<RecordDecision>,
}

#[derive(Debug, Serialize)]
pub struct CompactReport {
    pub ok: bool,
    pub applied: bool,
    pub manifest_sha256: String,
    pub inputs: Vec<InputPlan>,
    pub removed: usize,
    pub retained_with_reason: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CompactReceipt {
    pub schema: &'static str,
    pub manifest_sha256: String,
    pub actor: String,
    pub timestamp: String,
    pub inputs: Vec<InputPlan>,
    pub removed: usize,
    pub records: usize,
    pub projection_records: usize,
    pub import_set_sha256: String,
    pub doctor_ok: bool,
}

pub(crate) struct Plan {
    pub manifest_bytes: Vec<u8>,
    pub manifest_sha256: String,
    pub entries: Vec<ManifestEntry>,
    pub inputs: Vec<PlannedInput>,
}

pub(crate) struct PlannedInput {
    pub declared: PathBuf,
    pub source: PathBuf,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
    pub records_after: usize,
    pub public: InputPlan,
}

pub(crate) struct Decision {
    pub id: String,
    pub remove: bool,
    pub reason: Option<CompactReason>,
    pub replacement: Option<Vec<u8>>,
}

pub(crate) type Decisions = HashMap<(usize, usize), Decision>;
