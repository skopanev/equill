use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Arguments {
    pub sha256: String,
    pub bytes: u64,
    pub items: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Output {
    pub durable: Option<bool>,
    pub sha256: String,
    pub bytes: u64,
    pub count: Option<u64>,
    pub ids: Vec<uuid::Uuid>,
    pub receipt_sha256: Option<String>,
}

/// Bounded structured coordinates stay readable; unsafe values are digest
/// references. The CLI applies the same rule to its filter values.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub schema_version: u8,
    pub id: uuid::Uuid,
    pub observed_at: String,
    pub duration_us: u64,
    pub surface: String,
    pub operation: String,
    pub project: Option<String>,
    pub role: Option<String>,
    pub process: Option<String>,
    pub pid: u32,
    pub actor_claimed: Option<String>,
    pub lane_claimed: Option<String>,
    pub instance: Option<String>,
    pub session: Option<String>,
    pub outcome: String,
    pub domain_outcome: String,
    pub error_class: Option<String>,
    pub arguments: Arguments,
    pub output: Output,
}

impl Event {
    pub(super) fn valid(&self) -> bool {
        let digest = |value: &str| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        };
        let coordinate = |value: &str| {
            value.strip_prefix("sha256:").is_some_and(digest)
                || super::capture::coordinate(value) == value
        };
        self.schema_version == 1
            && ["cli", "mcp"].contains(&self.surface.as_str())
            && self
                .operation
                .split('.')
                .all(|part| super::capture::operation(part) == part)
            && ["success", "error"].contains(&self.outcome.as_str())
            && ["success", "error", "unknown"].contains(&self.domain_outcome.as_str())
            && self
                .error_class
                .as_deref()
                .is_none_or(|value| super::capture::error_class(value) == value)
            && self.duration_us <= i64::MAX as u64
            && digest(&self.arguments.sha256)
            && digest(&self.output.sha256)
            && self.output.receipt_sha256.as_deref().is_none_or(digest)
            && self.output.ids.len() <= 16
            && [
                &self.project,
                &self.role,
                &self.process,
                &self.actor_claimed,
                &self.lane_claimed,
                &self.instance,
                &self.session,
            ]
            .iter()
            .all(|value| value.as_deref().is_none_or(coordinate))
    }
}
