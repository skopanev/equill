use crate::projection::ProjectionState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;
mod selection;

pub use selection::{CoordinateMode, Expectation, RankOrder, Selector, Strategy, Tier};

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
    /// A superseded record is hidden because a later one replaced it. Reading
    /// the chain is a different question from reading current state, so it is
    /// asked for explicitly rather than inferred.
    #[serde(default)]
    pub include_superseded: bool,
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
    #[serde(default)]
    pub budget: ContextBudget,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadGrant {
    pub namespace: String,
    pub types: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Every bound is optional. An absent cap means "do not bound this tier"; an
/// absent floor or reserve means zero. A profile with no budget at all returns
/// everything the selectors matched.
pub struct ContextBudget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_cap: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub core_cap: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevant_floor: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_reserve: Option<usize>,
}

impl ContextBudget {
    /// Total content space once the receipt reserve is set aside.
    pub fn content_limit(&self) -> usize {
        match self.total {
            Some(total) => total.saturating_sub(self.receipt_reserve()),
            None => usize::MAX,
        }
    }

    pub fn receipt_reserve(&self) -> usize {
        self.receipt_reserve.unwrap_or(0)
    }

    pub fn relevant_floor(&self) -> usize {
        self.relevant_floor.unwrap_or(0)
    }

    /// Hard ceiling on the required tier. Exceeding it is fatal, so an absent
    /// cap is the difference between "bounded" and "never fails".
    pub fn required_limit(&self) -> usize {
        self.required_cap
            .unwrap_or(usize::MAX)
            .min(self.content_limit())
    }

    pub fn core_cap(&self) -> usize {
        self.core_cap.unwrap_or(usize::MAX)
    }
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
    FilterMismatch,
    RequiredOverflow,
    CoreCap,
    TotalBudget,
}

/// A coordinate the request carried that narrowed the result to nothing.
/// `declared` false means no selector in this profile knows the name at all;
/// `exact_only` means the name is known but compared exactly, so records
/// holding null for it were not treated as universal.
#[derive(Clone, Debug, Serialize)]
pub struct UnmatchedCoordinate {
    pub key: String,
    pub declared: bool,
    pub exact_only: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContextReceipt {
    pub schema: &'static str,
    pub profile: VersionCoordinate,
    pub selectors: Vec<VersionCoordinate>,
    pub request_digest: String,
    pub included: Vec<SelectedCoordinate>,
    pub excluded: Vec<ExcludedCoordinate>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unmatched_coordinates: Vec<UnmatchedCoordinate>,
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
    /// Whether the receipt was written to the store as well as returned.
    ///
    /// Reading is not a privilege that depends on being able to write. A store
    /// mounted read-only still answers questions, and it says plainly that the
    /// receipt it handed back is the only copy — rather than refusing the
    /// answer it had already assembled because it could not file a copy of it.
    pub receipt_persisted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RegistryReport {
    pub ok: bool,
    pub created: bool,
    pub id: String,
    pub version: String,
    pub digest: String,
}
