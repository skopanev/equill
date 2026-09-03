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
    if format == Format::Text {
        // Answered as a set: a role, its process and that process's steps
        // arrive as separate records, so the shape a reader wants exists only
        // across the answer.
        return Ok(text::answer(records, fields));
    }
    let mut lines = Vec::with_capacity(records.len());
    for record in records {
        lines.push(serde_json::to_string(&projected(record, fields))?);
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

/// Names resolve exactly as they do in a filter: a bare name is the payload
/// first, `payload.x` and `record.x` name a half outright. Printing and
/// filtering must agree, or the same word means two things in one command.
pub(super) fn lookup(record: &StoredRecord, field: &str) -> Option<Value> {
    let path = field.split('.').map(str::to_owned).collect::<Vec<_>>();
    let envelope = serde_json::to_value(record).ok()?;
    crate::filter::address(&record.payload, &envelope, &path).cloned()
}

mod classify;
mod label;
mod steps;
mod text;

#[cfg(test)]
mod process_tests;
#[cfg(test)]
mod shape_tests;
#[cfg(test)]
mod tests;
