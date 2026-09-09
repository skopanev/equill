use super::{Arguments, Event, Output};
use crate::kernel::digest::sha256_hex;
use serde_json::Value;
use std::ffi::OsString;

pub(super) fn arguments(bytes: &[u8], items: usize) -> Arguments {
    Arguments {
        sha256: sha256_hex(bytes),
        bytes: bytes.len() as u64,
        items: items as u64,
    }
}

pub(super) fn cli(args: &[OsString]) -> Arguments {
    let mut canonical = Vec::new();
    for arg in args.iter().skip(1) {
        let bytes = arg.as_encoded_bytes();
        canonical.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        canonical.extend_from_slice(bytes);
    }
    arguments(&canonical, args.len().saturating_sub(1))
}

pub(super) fn output(bytes: &[u8]) -> Output {
    let value = serde_json::from_slice::<Value>(bytes).ok();
    let canonical = value.as_ref().and_then(|v| serde_json::to_vec(v).ok());
    let mut result = Output {
        sha256: sha256_hex(canonical.as_deref().unwrap_or(bytes)),
        bytes: bytes.len() as u64,
        ..Output::default()
    };
    if let Some(value) = value {
        let body = value.pointer("/result/structuredContent").unwrap_or(&value);
        result.durable = body["durable"].as_bool();
        for key in ["id", "record_id", "receipt_id", "tombstone"] {
            if let Some(id) = body[key].as_str().and_then(|v| v.parse().ok()) {
                result.ids.push(id);
            }
        }
        if body["tombstone"]
            .as_str()
            .and_then(|v| v.parse::<uuid::Uuid>().ok())
            .is_some()
        {
            result.count = Some(1);
            result.durable = Some(true);
        }
        for key in ["selected_record_ids", "hits", "records"] {
            if let Some(items) = body[key].as_array() {
                result.count = Some(items.len() as u64);
                for item in items.iter().take(16) {
                    let id = item
                        .as_str()
                        .or_else(|| item["id"].as_str())
                        .or_else(|| item["record_id"].as_str())
                        .or_else(|| item.pointer("/record/id").and_then(Value::as_str));
                    if let Some(id) = id.and_then(|v| v.parse().ok()) {
                        result.ids.push(id);
                    }
                }
            }
        }
        result.count = body["stored"]
            .as_u64()
            .or_else(|| body["imported"].as_u64())
            .or_else(|| body["exported"].as_u64())
            .or_else(|| body["records"].as_u64())
            .or(result.count);
        result.receipt_sha256 = body["receipt"]
            .as_str()
            .or_else(|| body["receipt_path"].as_str())
            .map(|value| sha256_hex(value.as_bytes()));
    }
    result.ids.sort();
    result.ids.dedup();
    result.ids.truncate(16);
    if result.count.is_none() && !result.ids.is_empty() {
        result.count = Some(result.ids.len() as u64);
    }
    result
}

pub(super) fn coordinates(event: &mut Event, value: impl Fn(&str) -> Option<String>) {
    let hashed = |key: &str| value(key).map(|value| coordinate(&value));
    event.project = hashed("project");
    event.role = hashed("role");
    event.process = hashed("process");
    event.actor_claimed = hashed("actor");
    event.lane_claimed = hashed("lane");
    event.instance = hashed("instance");
    event.session = hashed("session");
}

pub(super) fn overlay(event: &mut Event, value: impl Fn(&str) -> Option<String>) {
    for (key, target) in [
        ("project", &mut event.project),
        ("role", &mut event.role),
        ("process", &mut event.process),
        ("lane", &mut event.lane_claimed),
        ("instance", &mut event.instance),
        ("session", &mut event.session),
    ] {
        if let Some(value) = value(key) {
            *target = Some(coordinate(&value));
        }
    }
}

pub(super) fn cli_coordinate(words: &[&str], key: &str) -> Option<String> {
    let flag = format!("--{key}");
    for (index, word) in words.iter().enumerate() {
        if *word == flag {
            return words.get(index + 1).map(|v| (*v).to_owned());
        }
        if let Some(value) = word.strip_prefix(&format!("{flag}=")) {
            return Some(value.into());
        }
        let coordinate = if *word == "--coordinate" {
            words.get(index + 1).copied()
        } else {
            word.strip_prefix("--coordinate=")
        };
        if let Some(value) = coordinate.and_then(|value| value.strip_prefix(&format!("{key}="))) {
            return Some(value.into());
        }
    }
    None
}

pub(super) fn mcp_coordinate(arguments: &Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            arguments
                .get("coordinates")
                .and_then(Value::as_array)?
                .iter()
                .filter_map(Value::as_str)
                .find_map(|value| value.strip_prefix(&format!("{key}=")).map(str::to_owned))
        })
}

pub(super) fn coordinate(value: &str) -> String {
    if !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        && !["sk-", "ghp_", "github_pat_", "xox", "AKIA", "ASIA", "eyJ"]
            .iter()
            .any(|prefix| value.starts_with(prefix))
        && crate::defense::coordinate_is_public(value)
    {
        value.into()
    } else {
        format!("sha256:{}", sha256_hex(value.as_bytes()))
    }
}

pub(super) fn operation(value: &str) -> String {
    // Unknown input is a classification, never a user-controlled log string.
    const KNOWN: &[&str] = &[
        "init",
        "record",
        "import",
        "compact",
        "doctor",
        "schema",
        "profile",
        "selector",
        "context",
        "status",
        "search",
        "vector",
        "get",
        "revoke",
        "mcp",
        "owner",
        "grant",
        "reader",
        "rebuild",
        "list",
        "show",
        "register",
        "export",
        "configure",
        "disable",
        "sync",
        "drain",
        "transfer",
        "add",
        "schema_list",
        "schema_show",
        "initialize",
        "ping",
        "tools/list",
        "tools/call",
        "notifications/initialized",
    ];
    if KNOWN.contains(&value) {
        value.into()
    } else {
        "invalid".into()
    }
}

pub(super) fn error_class(value: &str) -> &str {
    if [
        "io",
        "validation",
        "authorization",
        "post_commit",
        "audit",
        "integrity",
        "execution",
        "transport",
        "interrupted",
    ]
    .contains(&value)
    {
        value
    } else {
        "execution"
    }
}
