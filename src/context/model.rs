use crate::projection::ProjectionState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextRequest {
    pub at: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub kinds: Vec<String>,
    #[serde(default)]
    pub coordinates: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextProfile {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub actors: Vec<String>,
    pub grants: Vec<ReadGrant>,
    pub selectors: Vec<String>,
    pub budget: ContextBudget,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadGrant {
    pub namespace: String,
    pub types: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextBudget {
    pub total: usize,
    pub required_cap: usize,
    pub core_cap: usize,
    pub relevant_floor: usize,
    pub receipt_reserve: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Selector {
    pub id: String,
    pub version: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub strategies: Vec<Strategy>,
    #[serde(default)]
    pub required_tags: Vec<String>,
    #[serde(default)]
    pub core_tags: Vec<String>,
    #[serde(default)]
    pub kind_pointer: Option<String>,
    #[serde(default)]
    pub expires_at_pointer: Option<String>,
    #[serde(default)]
    pub coordinate_pointers: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    Exact,
    Tag,
    Recency,
    Fts,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Required,
    Core,
    Relevant,
}

#[derive(Clone, Debug, Serialize)]
pub struct SelectedCoordinate {
    pub id: Uuid,
    pub namespace: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub tier: Tier,
    pub units: usize,
    pub strategies: Vec<Strategy>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExcludedCoordinate {
    pub id: Uuid,
    pub namespace: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub reason: ExclusionReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    Unauthorized,
    Superseded,
    Revoked,
    InvalidAtRequestTime,
    Expired,
    SelectorMismatch,
    RequiredOverflow,
    CoreCap,
    TotalBudget,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContextReceipt {
    pub schema: &'static str,
    pub profile: VersionCoordinate,
    pub selectors: Vec<VersionCoordinate>,
    pub request_digest: String,
    pub included: Vec<SelectedCoordinate>,
    pub excluded: Vec<ExcludedCoordinate>,
    pub strategies: Vec<Strategy>,
    pub budget: ContextBudget,
    pub used: usize,
    pub bundle_digest: String,
    pub projection: ProjectionState,
    pub degraded_strategies: Vec<Strategy>,
    pub degraded: bool,
    pub empty: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct VersionCoordinate {
    pub id: String,
    pub version: String,
    pub digest: String,
}

#[derive(Debug, Serialize)]
pub struct ContextBundle {
    pub ok: bool,
    pub content: String,
    pub bundle_digest: String,
    pub selected_record_ids: Vec<Uuid>,
    pub receipt: ContextReceipt,
    pub receipt_path: String,
}

#[derive(Debug, Serialize)]
pub struct RegistryReport {
    pub ok: bool,
    pub created: bool,
    pub id: String,
    pub version: String,
    pub digest: String,
}
