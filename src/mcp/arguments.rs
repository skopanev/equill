//! Reading one JSON-RPC argument object.
//!
//! A missing argument is a refusal with the key named, and a malformed one
//! reads as absent rather than as an error: the adapter answers about the
//! store, and a client that sent the wrong shape learns which key it was.
use crate::kernel::error::Error;
use serde_json::Value;

pub(super) fn value<T: serde::Serialize>(report: &T) -> Result<Value, Error> {
    Ok(serde_json::to_value(report)?)
}

pub(super) fn text<'a>(arguments: &'a Value, key: &str) -> Result<&'a str, Error> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::InvalidRecord(format!("{key} is required")))
}

pub(super) fn optional(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

pub(super) fn strings(arguments: &Value, key: &str) -> Vec<String> {
    arguments
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn flag(arguments: &Value, key: &str) -> bool {
    arguments.get(key).and_then(Value::as_bool).unwrap_or(false)
}
