//! Store-level retrieval policy, separate from physical projection identity.
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const SETTINGS: &str = "settings.json";
pub const DEFAULT_QUERY_INSTRUCTION: &str =
    "Retrieve durable software-engineering knowledge directly applicable to the current task.";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Vector,
    Fts,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoreSettings {
    retrieval: RetrievalSettings,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RetrievalSettings {
    default_budget_records: usize,
    query_instruction: String,
    vector: VectorSettings,
    hybrid: HybridSettings,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VectorSettings {
    enabled: bool,
    score_threshold: f32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HybridSettings {
    order: [Source; 2],
    fill_remaining: bool,
    deduplicate: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Overrides {
    pub query_instruction: Option<String>,
    pub vector_enabled: Option<bool>,
    pub vector_score_threshold: Option<f32>,
    pub hybrid_order: Option<[Source; 2]>,
    pub hybrid_fill_remaining: Option<bool>,
    pub hybrid_deduplicate: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub default_budget_records: Option<usize>,
    pub query_instruction: String,
    pub vector_enabled: bool,
    pub vector_score_threshold: Option<f32>,
    pub hybrid_order: [Source; 2],
    pub hybrid_fill_remaining: bool,
    pub hybrid_deduplicate: bool,
}

impl Policy {
    fn defaults() -> Self {
        Self {
            default_budget_records: Some(30),
            query_instruction: DEFAULT_QUERY_INSTRUCTION.into(),
            vector_enabled: true,
            vector_score_threshold: Some(0.48),
            hybrid_order: [Source::Vector, Source::Fts],
            hybrid_fill_remaining: true,
            hybrid_deduplicate: true,
        }
    }

    fn configured(settings: RetrievalSettings) -> Self {
        Self {
            default_budget_records: Some(settings.default_budget_records),
            query_instruction: settings.query_instruction,
            vector_enabled: settings.vector.enabled,
            vector_score_threshold: Some(settings.vector.score_threshold),
            hybrid_order: settings.hybrid.order,
            hybrid_fill_remaining: settings.hybrid.fill_remaining,
            hybrid_deduplicate: settings.hybrid.deduplicate,
        }
    }
}

pub fn resolve(store: &Path, overrides: Overrides) -> Result<Policy, Error> {
    let path = store.join(SETTINGS);
    let mut policy = if path.is_file() {
        let settings: StoreSettings =
            serde_json::from_slice(&fs::read(path)?).map_err(|error| invalid(error.to_string()))?;
        validate(&settings.retrieval)?;
        Policy::configured(settings.retrieval)
    } else {
        Policy::defaults()
    };
    if let Some(value) = overrides.query_instruction {
        policy.query_instruction = value;
    }
    if let Some(value) = overrides.vector_enabled {
        policy.vector_enabled = value;
    }
    if let Some(value) = overrides.vector_score_threshold {
        policy.vector_score_threshold = Some(value);
    }
    if let Some(value) = overrides.hybrid_order {
        policy.hybrid_order = value;
    }
    if let Some(value) = overrides.hybrid_fill_remaining {
        policy.hybrid_fill_remaining = value;
    }
    if let Some(value) = overrides.hybrid_deduplicate {
        policy.hybrid_deduplicate = value;
    }
    validate_policy(&policy)?;
    Ok(policy)
}

fn validate(settings: &RetrievalSettings) -> Result<(), Error> {
    if !(1..=usize::from(u16::MAX)).contains(&settings.default_budget_records) {
        return Err(invalid(
            "default_budget_records must be between 1 and 65535",
        ));
    }
    validate_instruction(&settings.query_instruction)?;
    validate_threshold(settings.vector.score_threshold)?;
    validate_order(settings.hybrid.order)
}

fn validate_policy(policy: &Policy) -> Result<(), Error> {
    validate_instruction(&policy.query_instruction)?;
    if let Some(value) = policy.vector_score_threshold {
        validate_threshold(value)?;
    }
    validate_order(policy.hybrid_order)
}

fn validate_instruction(value: &str) -> Result<(), Error> {
    if value.trim().is_empty() || value.len() > 4_096 || value.chars().any(char::is_control) {
        return Err(invalid("query_instruction must be 1..4096 printable bytes"));
    }
    Ok(())
}

fn validate_threshold(value: f32) -> Result<(), Error> {
    if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
        return Err(invalid("vector.score_threshold must be between -1 and 1"));
    }
    Ok(())
}

fn validate_order(order: [Source; 2]) -> Result<(), Error> {
    if order[0] == order[1] {
        return Err(invalid(
            "hybrid.order must contain vector and fts once each",
        ));
    }
    Ok(())
}

fn invalid(reason: impl Into<String>) -> Error {
    Error::InvalidSettings(reason.into())
}

#[cfg(test)]
mod tests;
