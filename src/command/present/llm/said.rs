//! Saying a thing once, and telling two records apart when they say the same.
use super::Said;
use super::fallback;
use crate::record::StoredRecord;
use serde_json::Value;

/// Adds a block, distinguishing it from an existing one that reads the same.
///
/// Both sides get the field that differs, not just the newcomer: told only on
/// the second bullet, the reader learns that one of them is scoped and cannot
/// tell what the first one is.
pub(super) fn say(said: &mut Vec<Said>, record: &StoredRecord, mut lines: Vec<String>) -> bool {
    if lines.is_empty() {
        return false;
    }
    if let Some(existing) = said.iter_mut().find(|item| item.lines == lines) {
        let (mine, theirs) = differing(&record.payload, &existing.payload);
        if mine.is_empty() && theirs.is_empty() {
            // Same words, same payload, different envelope: one fact.
            return true;
        }
        existing.lines.extend(theirs);
        lines.extend(mine);
    }
    said.push(Said {
        payload: record.payload.clone(),
        lines,
    });
    true
}

/// The fields that tell two payloads apart, rendered for each side.
fn differing(mine: &Value, theirs: &Value) -> (Vec<String>, Vec<String>) {
    let (Some(mine), Some(theirs)) = (mine.as_object(), theirs.as_object()) else {
        return (Vec::new(), Vec::new());
    };
    let mut names: Vec<&String> = mine.keys().chain(theirs.keys()).collect();
    names.sort();
    names.dedup();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    for name in names {
        let (a, b) = (mine.get(name), theirs.get(name));
        if a == b {
            continue;
        }
        if let Some(value) = a {
            left.push(format!("{name}: {}", fallback::literal(value)));
        }
        if let Some(value) = b {
            right.push(format!("{name}: {}", fallback::literal(value)));
        }
    }
    (left, right)
}
