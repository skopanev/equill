use crate::compact::model::AnchorState;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnchorFact {
    pub kind: String,
    pub target: String,
    pub state: AnchorState,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Anchor {
    pub kind: String,
    pub target: Option<String>,
}
