use super::TokenizerCoordinate;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
/// Every bound is optional. An absent cap means "do not bound this tier"; an
/// absent floor or reserve means zero. A profile with no budget at all returns
/// everything the selectors matched.
pub struct ContextBudget {
    #[serde(
        default,
        rename = "total_tokens",
        alias = "total",
        skip_serializing_if = "Option::is_none"
    )]
    pub total: Option<usize>,
    #[serde(
        default,
        rename = "required_cap_tokens",
        alias = "required_cap",
        skip_serializing_if = "Option::is_none"
    )]
    pub required_cap: Option<usize>,
    #[serde(
        default,
        rename = "core_cap_tokens",
        alias = "core_cap",
        skip_serializing_if = "Option::is_none"
    )]
    pub core_cap: Option<usize>,
    #[serde(
        default,
        rename = "relevant_floor_tokens",
        alias = "relevant_floor",
        skip_serializing_if = "Option::is_none"
    )]
    pub relevant_floor: Option<usize>,
    #[serde(
        default,
        rename = "receipt_reserve_tokens",
        alias = "receipt_reserve",
        skip_serializing_if = "Option::is_none"
    )]
    pub receipt_reserve: Option<usize>,
    #[serde(default)]
    pub tokenizer: TokenizerCoordinate,
}

impl ContextBudget {
    pub fn receipt_reserve(&self) -> usize {
        self.receipt_reserve.unwrap_or(0)
    }

    pub fn relevant_floor(&self) -> usize {
        self.relevant_floor.unwrap_or(0)
    }

    pub fn core_cap(&self) -> usize {
        self.core_cap.unwrap_or(usize::MAX)
    }

    pub fn effective_total(&self, runtime: Option<usize>) -> Option<usize> {
        match (self.total, runtime) {
            (Some(profile), Some(runtime)) => Some(profile.min(runtime)),
            (Some(profile), None) => Some(profile),
            (None, Some(runtime)) => Some(runtime),
            (None, None) => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RuntimeBudget {
    pub tokens: Option<usize>,
    pub records: Option<usize>,
}
