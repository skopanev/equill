//! Which section a record belongs to, decided by what it says it is.
use super::super::classify::{Kind, classify};
use crate::record::StoredRecord;
use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum LlmKind {
    Role,
    Process,
    Step,
    Communication,
    Ticketing,
    Lesson,
    Finding,
    Note,
    Other,
}

pub(super) fn classify_llm(record: &StoredRecord) -> LlmKind {
    if let Some(kind) = rule_kind(record) {
        return kind;
    }
    let tokens = record
        .type_name
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .collect::<Vec<_>>();
    if tokens.contains(&"lesson") {
        return LlmKind::Lesson;
    }
    if tokens.contains(&"finding") {
        return LlmKind::Finding;
    }
    if tokens.contains(&"note") {
        return LlmKind::Note;
    }
    match classify(record) {
        Kind::Role => LlmKind::Role,
        Kind::Process => LlmKind::Process,
        Kind::Step => LlmKind::Step,
        Kind::Other => LlmKind::Other,
    }
}

/// The first key that names a category this formatter knows, rather than the
/// first key that happens to be present.
///
/// `module` is still asked first, so a record where both are recognized keeps
/// the section it has always had — changing that is a different decision than
/// this fix. What changes is the case where `module` holds something this
/// formatter does not know: it used to stop there and answer "no category",
/// which sent the record to the discard pile even when `rules` named one. A
/// free-form field was acting as a whitelist for the renderer.
fn rule_kind(record: &StoredRecord) -> Option<LlmKind> {
    ["module", "rules"].iter().find_map(|name| {
        match record.payload.get(*name).and_then(Value::as_str)? {
            "communication" | "comm" => Some(LlmKind::Communication),
            "tickets" | "ticketing" => Some(LlmKind::Ticketing),
            _ => None,
        }
    })
}
