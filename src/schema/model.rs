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
}
