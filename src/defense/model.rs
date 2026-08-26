use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DefenseMode {
    Block,
    Redact,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LiteralPattern {
    pub id: String,
    pub literal: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DefensePolicy {
    pub mode: DefenseMode,
    pub sensitive_keys: Vec<String>,
    pub literal_patterns: Vec<LiteralPattern>,
}

#[derive(Debug, Serialize)]
pub struct DefenseFinding {
    pub path: String,
    pub rule: String,
}

#[derive(Debug)]
pub struct DefenseResult {
    pub mode: DefenseMode,
    pub findings: Vec<DefenseFinding>,
}

impl DefenseResult {
    pub fn blocked(&self) -> bool {
        self.mode == DefenseMode::Block && !self.findings.is_empty()
    }

    pub fn redacted(&self) -> bool {
        self.mode == DefenseMode::Redact && !self.findings.is_empty()
    }
}
