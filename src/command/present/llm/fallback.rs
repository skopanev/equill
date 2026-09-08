//! What a record says when no section claimed it.
use super::{Sections, fresh};
use crate::record::StoredRecord;
use serde_json::Value;

/// What a record says when this formatter knows no shape for it, or knew one
/// and it came out empty.
///
/// Deliberately not `label::pair`, which the text surface uses: that one drops
/// nested nulls and renders a null scalar as an empty string, so a limit of
/// `null` and a limit of nothing would look the same here. In a contract handed
/// to an agent those are different facts — one says the limit is deliberately
/// unset, the other says the record never mentioned it. Zero, `false` and empty
/// containers are facts for the same reason.
///
/// The text surface keeps its own rendering unchanged; this is a second reader
/// with a different need, not a replacement.
pub(super) fn fallback(sections: &mut Sections, record: &StoredRecord) {
    let mut lines = Vec::new();
    if let Some(fields) = record.payload.as_object() {
        for (name, value) in fields {
            lines.push(format!("{name}: {}", literal(value)));
        }
    } else if !record.payload.is_null() {
        lines.push(literal(&record.payload));
    }
    if lines.is_empty() {
        return;
    }
    if fresh(&mut sections.seen, record) {
        sections.records.push(lines);
    }
}

/// A value written so that what it is stays readable: a string as itself, and
/// everything else — including `null`, `false`, `0` and the empty list — as the
/// literal it is.
pub(super) fn literal(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) if items.iter().all(|item| !item.is_object()) => format!(
            "[{}]",
            items.iter().map(literal).collect::<Vec<_>>().join(", ")
        ),
        other => other.to_string(),
    }
}
