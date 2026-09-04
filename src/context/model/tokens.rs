use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TokenizerCoordinate {
    pub id: String,
    pub version: String,
}

impl Default for TokenizerCoordinate {
    fn default() -> Self {
        Self {
            id: "o200k_base".into(),
            version: "tiktoken-rs-0.12.0".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct TokenUsage {
    pub required: usize,
    pub core: usize,
    pub relevant: usize,
    pub content: usize,
    pub receipt_reserved: usize,
    pub total: usize,
}
