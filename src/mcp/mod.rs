mod arguments;
mod catalog;
mod environment;
mod protocol;
mod retrieval;
#[cfg(test)]
mod tests;
mod tools;

#[cfg(test)]
pub(crate) use tools::call as tools_call;

use crate::kernel::error::Error;
use protocol::{INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, Request, Response, negotiate};
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
    log_queries: bool,
    input: impl BufRead,
    mut output: impl Write,
) -> Result<(), Error> {
    for line in input.lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                crate::audit::Invocation::mcp(b"", actor)?.finish(b"", Some("transport"))?;
                return Err(error.into());
            }
        };
        if line.trim().is_empty() {
            continue;
        }
        let mut audit = crate::audit::Invocation::mcp(line.as_bytes(), actor)?;
        let response = handle(store, actor, log_queries, &line);
        let mut bytes = response
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()?
            .unwrap_or_default();
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let class = if value.get("error").is_some() {
            Some("validation")
        } else if value.pointer("/result/isError").and_then(Value::as_bool) == Some(true) {
            Some("execution")
        } else {
            None
        };
        let id = audit.id();
        if audit.prepare(&bytes, class).is_err() && response.is_some() {
            let mut annotated = value;
            let notice =
                json!({"state":"pending","invocation":id,"operation_result_preserved":true});
            if annotated.get("result").is_some() {
                annotated["result"]["audit"] = notice;
            } else {
                annotated["error"]["data"]["audit"] = notice;
            }
            bytes = serde_json::to_vec(&annotated)?;
            // The notice changes the delivered response, not the domain result.
            // Persist its final fingerprint even when the same sink fault remains.
            let _ = audit.prepare(&bytes, class);
        }
        let sent = if response.is_some() {
            output
                .write_all(&bytes)
                .and_then(|_| output.write_all(b"\n"))
                .and_then(|_| output.flush())
        } else {
            Ok(())
        };
        if audit.settle(sent.is_err()).is_err() {
            eprintln!(
                "equill: audit pending for invocation {id}; the operation result is preserved"
            );
        }
        sent?;
    }
    Ok(())
}

/// Returns `None` for a notification, which by protocol gets no answer.
fn handle(store: &Path, actor: &str, log_queries: bool, line: &str) -> Option<Response> {
    // Per call, not once at startup. A long-lived server that only nudged the
    // index when it booted would let a store fall behind for the whole session,
    // which is exactly the case a long-lived server makes likely. The check is
    // two small file reads when there is nothing to do.
    // Not for an actor this store holds to reading: catching a lagging index
    // up is a write, and a call that is about to be refused would otherwise
    // have changed the store on its way to being refused. The same rule the
    // CLI applies, through the same helper, so the two surfaces cannot drift.
    if !crate::kernel::store::holds_to_reading(store, actor) {
        crate::vector::resume(store);
    }
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
    if request.jsonrpc.as_deref() != Some("2.0") {
        return Some(Response::failed(
            id,
            INVALID_REQUEST,
            "jsonrpc must be \"2.0\"",
        ));
    }
    Some(match request.method.as_str() {
        "initialize" => Response::ok(
            id,
            json!({
                "protocolVersion": negotiate(
                    request.params.get("protocolVersion").and_then(Value::as_str)
                ),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "equill", "version": env!("CARGO_PKG_VERSION") }
            }),
        ),
        "tools/list" => Response::ok(id, catalog::catalog()),
        "tools/call" => match request.params.get("name").and_then(Value::as_str) {
            // An unknown tool name is a malformed call, not a failed one: the
            // client asked for something this server never advertised.
            None => Response::failed(id, INVALID_PARAMS, "tools/call needs a tool name"),
            Some(name) if !catalog::exists(name) => {
                Response::failed(id, INVALID_PARAMS, format!("unknown tool {name}"))
            }
            Some(_) => Response::ok(id, invoke(store, actor, log_queries, &request.params)),
        },
        "ping" => Response::ok(id, json!({})),
        other => Response::failed(id, METHOD_NOT_FOUND, format!("unknown method {other}")),
    })
}

/// A refused write is a real answer to a valid question, so it comes back as
/// tool content with `isError`, not as a transport failure. Errors carry
/// coordinates and reasons, never record payloads.
fn invoke(store: &Path, actor: &str, log_queries: bool, params: &Value) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    match tools::call(store, actor, log_queries, name, &arguments) {
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
