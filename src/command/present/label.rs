//! Turning a field name into something a person reads, and a value into text
//! rather than into escaped JSON.
use serde_json::Value;

/// `on_fail` is read aloud as "on fail"; the underscore is a key's business,
/// not a reader's. A name that already arrived capitalised is left alone.
pub(super) fn name(field: &str) -> String {
    let spaced = field.replace(['_', '-'], " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => {
            first.to_uppercase().collect::<String>() + chars.as_str()
        }
        _ => spaced,
    }
}

/// A label and its value. Anything nested arrives indented under its own name
/// instead of on one line as escaped JSON — the shape is what makes it
/// readable, and quoting it away is what made the old output unreadable.
pub(super) fn pair(field: &str, value: &Value) -> String {
    match value {
        Value::Object(fields) => {
            let mut out = vec![format!("{}:", name(field))];
            for (inner, value) in fields {
                if !value.is_null() {
                    out.push(indent(&pair(inner, value)));
                }
            }
            out.join("\n")
        }
        Value::Array(items) if items.iter().any(Value::is_object) => {
            let mut out = vec![format!("{}:", name(field))];
            for item in items {
                out.push(indent(&format!("- {}", scalar(item))));
            }
            out.join("\n")
        }
        other => format!("{}: {}", name(field), scalar(other)),
    }
}

fn indent(block: &str) -> String {
    block
        .lines()
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Values as text: a string is itself, a list reads as a list, and a number or
/// a boolean keeps the spelling it had. Never a quoted blob.
pub(super) fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        Value::Array(items) => items.iter().map(scalar).collect::<Vec<_>>().join(", "),
        Value::Object(fields) => fields
            .iter()
            .filter(|(_, value)| !value.is_null())
            .map(|(field, value)| format!("{}: {}", name(field), scalar(value)))
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}
