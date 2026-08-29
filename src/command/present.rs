use crate::kernel::error::Error;
use crate::record::StoredRecord;
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    Jsonl,
    Text,
}

/// Presentation of a result set. A caller reading a rule wants the sentence,
/// not eighteen fields around it; a script wants one object per line. Both are
/// the same records, so the choice belongs here rather than in each command.
pub fn records(
    records: &[StoredRecord],
    format: Format,
    fields: &[String],
) -> Result<String, Error> {
    let mut lines = Vec::with_capacity(records.len());
    for record in records {
        lines.push(match format {
            Format::Jsonl => serde_json::to_string(&projected(record, fields))?,
            Format::Text => text(record, fields),
        });
    }
    Ok(lines.join("\n"))
}

/// With `--fields` the selection is exactly what was asked for, in the order it
/// was asked for. Without it the whole record is kept, which is today's shape.
fn projected(record: &StoredRecord, fields: &[String]) -> Value {
    if fields.is_empty() {
        return serde_json::to_value(record).unwrap_or(Value::Null);
    }
    let mut selected = Map::new();
    for field in fields {
        if let Some(value) = lookup(record, field) {
            selected.insert(field.clone(), value);
        }
    }
    Value::Object(selected)
}

/// One record per line: the requested fields in order, or the payload's own
/// values followed by the coordinates that place them.
fn text(record: &StoredRecord, fields: &[String]) -> String {
    if !fields.is_empty() {
        return fields
            .iter()
            .map(|field| lookup(record, field).map_or(String::new(), flatten))
            .collect::<Vec<_>>()
            .join("\t");
    }
    let mut parts = payload_values(&record.payload);
    parts.push(format!("[{} {}]", record.namespace, record.type_name));
    parts.join("\t")
}

fn payload_values(payload: &Value) -> Vec<String> {
    match payload {
        Value::Object(fields) => fields.values().cloned().map(flatten).collect(),
        other => vec![flatten(other.clone())],
    }
}

/// Envelope coordinates are addressable by name so a caller can ask for `id`
/// or `type` beside a payload field; dots reach into the payload.
fn lookup(record: &StoredRecord, field: &str) -> Option<Value> {
    match field {
        "id" => Some(Value::String(record.id.to_string())),
        "namespace" => Some(Value::String(record.namespace.clone())),
        "type" => Some(Value::String(record.type_name.clone())),
        "actor" => Some(Value::String(record.actor.clone())),
        "observed_at" => Some(Value::String(record.observed_at.clone())),
        "valid_at" => Some(Value::String(record.valid_at.clone())),
        "tags" => Some(serde_json::to_value(&record.tags).ok()?),
        _ => {
            let pointer = format!("/{}", field.replace('.', "/"));
            record.payload.pointer(&pointer).cloned()
        }
    }
}

fn flatten(value: Value) -> String {
    match value {
        Value::String(text) => text,
        Value::Array(items) => items.into_iter().map(flatten).collect::<Vec<_>>().join(","),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}
