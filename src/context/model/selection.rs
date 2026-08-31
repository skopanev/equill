//! How a profile says which records it wants and how many.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank_pointer: Option<String>,
    /// Which way `rank_pointer` reads. Highest first unless the selector says
    /// otherwise — a list of steps is the case where lowest first is the only
    /// order that means anything, and encoding that in the payload instead
    /// would make the data carry a presentation choice.
    #[serde(default)]
    pub rank_order: RankOrder,
    /// How many records this selector has to find for the answer to be an
    /// answer.
    ///
    /// Named for what it bounds — the size of the selection — and deliberately
    /// not `required`, which this type already uses on a different axis:
    /// `required_tags` decides which records the budget serves first, not
    /// whether there is an answer at all. One word on two axes in one type
    /// would read wrong on every later review.
    #[serde(default)]
    pub expect: Expectation,
    #[serde(default)]
    pub coordinate_pointers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub coordinate_modes: BTreeMap<String, CoordinateMode>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RankOrder {
    #[default]
    Desc,
    Asc,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Expectation {
    /// However many there are, including none. What every profile written
    /// before this existed means, which is why it is the default: adding a
    /// field must not change what a stored profile does.
    #[default]
    Any,
    /// Not fewer than one. A selector the answer depends on.
    Some,
    /// Exactly one. A second record answering to the same name is not a choice
    /// the caller can make — picking either would be picking for them — and
    /// none at all is the same absence, whether the record was never written
    /// or belongs to somebody else.
    One,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinateMode {
    Exact,
    SetOrWildcard,
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
