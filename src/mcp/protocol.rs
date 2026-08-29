use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Versions this adapter speaks, newest first. This is the 2025 line of the
/// protocol, not a later one: the list is taken from the official TypeScript
/// SDK's own SUPPORTED_PROTOCOL_VERSIONS, whose latest is 2025-11-25. Naming a
/// version no released client knows looks like compatibility and is not — a
/// real client disconnects on it, which is how this list was checked.
pub const SUPPORTED_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub fn negotiate(requested: Option<&str>) -> &'static str {
    requested
        .and_then(|wanted| {
            SUPPORTED_VERSIONS
                .iter()
                .find(|known| **known == wanted)
                .copied()
        })
        .unwrap_or(SUPPORTED_VERSIONS[0])
}

/// One JSON-RPC request as it arrives on stdin. `id` is absent for
/// notifications, which are acted on but never answered.
#[derive(Debug, Deserialize)]
pub struct Request {
    /// Present and equal to "2.0" in a well-formed call. A missing or wrong
    /// value is a malformed frame, not a question we can answer.
    #[serde(default)]
    pub jsonrpc: Option<String>,
    pub method: String,
    #[serde(default)]
    pub id: Option<Value>,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

impl Response {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    /// A tool that fails is not a protocol failure: the caller asked a valid
    /// question and got a real answer, which happens to be a refusal. Reporting
    /// it as a JSON-RPC error would make an ordinary denial look like a broken
    /// connection, so only malformed protocol uses this.
    pub fn failed(id: Value, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INVALID_REQUEST: i32 = -32600;
