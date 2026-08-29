use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// One JSON-RPC request as it arrives on stdin. `id` is absent for
/// notifications, which are acted on but never answered.
#[derive(Debug, Deserialize)]
pub struct Request {
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
