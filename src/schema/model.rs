use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TypeDefinition {
    #[serde(rename = "type")]
    pub type_name: String,
    pub uri: String,
    pub owner: String,
    pub payload_schema: Value,
    #[serde(default, skip_serializing_if = "LifecyclePolicy::is_default")]
    pub lifecycle: LifecyclePolicy,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleMode {
    AppendOnly,
    Linear,
    #[default]
    Dag,
}

#[derive(Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LifecyclePolicy {
    #[serde(default)]
    pub mode: LifecycleMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_pointer: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_predecessor_types: Vec<String>,
}

impl LifecyclePolicy {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}
