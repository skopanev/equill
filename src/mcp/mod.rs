mod protocol;
#[cfg(test)]
mod tests;
mod tools;

use crate::kernel::error::Error;
use protocol::{INVALID_PARAMS, METHOD_NOT_FOUND, PROTOCOL_VERSION, Request, Response};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::Path;

/// A local adapter over the operations the CLI already exposes, spoken over
/// stdio. No socket is opened and nothing is listened on: the transport is the
/// pipe the caller already handed us, so running this cannot expose a store to
/// anything the caller has not already given access to.
pub fn serve(
    store: &Path,
    actor: &str,
    input: impl BufRead,
    mut output: impl Write,
) -> Result<(), Error> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Some(response) = handle(store, actor, &line) else {
            continue;
        };
        output.write_all(&serde_json::to_vec(&response)?)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

/// Returns `None` for a notification, which by protocol gets no answer.
fn handle(store: &Path, actor: &str, line: &str) -> Option<Response> {
    let request: Request = match serde_json::from_str(line) {
        Ok(request) => request,
        Err(error) => {
            return Some(Response::failed(
                Value::Null,
                INVALID_PARAMS,
                error.to_string(),
            ));
        }
    };
    let id = request.id?;
    Some(match request.method.as_str() {
        "initialize" => Response::ok(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "equill", "version": env!("CARGO_PKG_VERSION") }
            }),
        ),
        "tools/list" => Response::ok(id, tools::catalog()),
        "tools/call" => Response::ok(id, invoke(store, actor, &request.params)),
        "ping" => Response::ok(id, json!({})),
        other => Response::failed(id, METHOD_NOT_FOUND, format!("unknown method {other}")),
    })
}

/// A refused write is a real answer to a valid question, so it comes back as
/// tool content with `isError`, not as a transport failure. Errors carry
/// coordinates and reasons, never record payloads.
fn invoke(store: &Path, actor: &str, params: &Value) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    match tools::call(store, actor, name, &arguments) {
        Ok(result) => json!({
            "content": [{ "type": "text", "text": result.to_string() }],
            "structuredContent": result,
            "isError": false
        }),
        Err(error) => json!({
            "content": [{ "type": "text", "text": error.to_string() }],
            "isError": true
        }),
    }
}
