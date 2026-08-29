use super::super::model::{EmbeddingDocument, vector_error};
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use serde_json::Value;
use std::fmt::Write as _;

/// Canonical record -> embedding input, version 1.
///
/// The input carries what a reader would call the meaning of a record: its
/// namespace, its type, its stable tags, and the domain payload. It carries no
/// provenance — no actor, no evidence, no ledger coordinate, no timestamp, no
/// id. Provenance changes when a record is re-imported or compacted while the
/// meaning does not, so including it would churn every embedding for nothing
/// and would leak write-side identity into a similarity index.
///
/// Ordering is total and explicit, so the same record always produces the same
/// bytes and therefore the same `input_sha256`.
pub fn canonical(record: &StoredRecord, record_sha256: &str) -> Result<EmbeddingDocument, Error> {
    if record.namespace.trim().is_empty() || record.type_name.trim().is_empty() {
        return Err(vector_error("record has no embeddable coordinates"));
    }
    let text = canonical_text(record);
    Ok(EmbeddingDocument {
        record_id: record.id,
        namespace: record.namespace.clone(),
        type_name: record.type_name.clone(),
        record_sha256: record_sha256.to_owned(),
        input_sha256: sha256_hex(text.as_bytes()),
        text,
    })
}

fn canonical_text(record: &StoredRecord) -> String {
    let mut text = format!(
        "namespace {}\ntype {}\n",
        record.namespace, record.type_name
    );
    let mut tags = record.tags.clone();
    tags.sort();
    tags.dedup();
    for tag in tags {
        let _ = writeln!(&mut text, "tag {tag}");
    }
    let mut leaves = Vec::new();
    walk(String::new(), &record.payload, &mut leaves);
    leaves.sort();
    for (pointer, value) in leaves {
        let _ = writeln!(&mut text, "{pointer} {value}");
    }
    text
}

/// Leaves are addressed by JSON pointer so that two payloads differing only in
/// nesting never collapse to the same input. Null is skipped: an absent field
/// and an explicit null mean the same thing to a reader.
fn walk(pointer: String, value: &Value, leaves: &mut Vec<(String, String)>) {
    match value {
        Value::Null => {}
        Value::Bool(item) => leaves.push((pointer, item.to_string())),
        Value::Number(item) => leaves.push((pointer, item.to_string())),
        Value::String(item) => leaves.push((pointer, item.clone())),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk(format!("{pointer}/{index}"), item, leaves);
            }
        }
        Value::Object(entries) => {
            for (key, item) in entries {
                walk(format!("{pointer}/{}", escape(key)), item, leaves);
            }
        }
    }
}

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}
