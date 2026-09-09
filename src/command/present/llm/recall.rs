//! How a lesson and a finding read once they reach the answer.
use super::render;
use crate::record::StoredRecord;
use serde_json::Value;

pub(super) fn lesson(record: &StoredRecord) -> Vec<String> {
    field(record, "rule")
}

pub(super) fn finding(record: &StoredRecord) -> Vec<String> {
    let mut lines = field(record, "claim");
    if let Some(evidence) = record.payload.get("evidence").and_then(Value::as_object) {
        append(&mut lines, "Evidence", evidence.get("how"), true);
        append(&mut lines, "Result", evidence.get("result"), false);
        append(&mut lines, "Location", evidence.get("where"), false);
    }
    append(
        &mut lines,
        "Boundary",
        record.payload.get("not_proven"),
        false,
    );
    lines
}

pub(super) fn field(record: &StoredRecord, name: &str) -> Vec<String> {
    record
        .payload
        .get(name)
        .map(|value| render::content(value, false))
        .unwrap_or_default()
}

pub(super) fn append(lines: &mut Vec<String>, label: &str, value: Option<&Value>, code: bool) {
    if let Some(value) = value {
        lines.extend(
            render::content(value, code)
                .into_iter()
                .map(|value| format!("{label}: {value}")),
        );
    }
}
