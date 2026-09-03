//! Which of the three a record is, decided by what it says it is and, failing
//! that, by what it carries.
//!
//! The type name is asked first because it is the record's own claim, and a
//! store may hold a v1, a v2 and a domain's own analogue of the same idea at
//! once. Shape is the fallback, not the rule: a record that never named itself
//! a step is only treated as one when it carries a step's required field.
use crate::record::StoredRecord;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
    Role,
    Process,
    Step,
    Other,
}

pub(super) fn classify(record: &StoredRecord) -> Kind {
    if let Some(kind) = by_name(&record.type_name) {
        return kind;
    }
    by_shape(record)
}

/// `agent.step.v2`, `agent.step.v1`, `review.step.v3` — the segment carries the
/// claim, the version does not. Matched as a whole segment so that a type named
/// `steps_taken` or `roleset` is not mistaken for one.
fn by_name(type_name: &str) -> Option<Kind> {
    type_name.split('.').find_map(|segment| match segment {
        "role" => Some(Kind::Role),
        "process" => Some(Kind::Process),
        "step" => Some(Kind::Step),
        _ => None,
    })
}

/// Required fields, in the order that keeps them from claiming each other: a
/// step is the only one of the three that must say what it does, a process the
/// only one that must say why it exists.
fn by_shape(record: &StoredRecord) -> Kind {
    let Some(payload) = record.payload.as_object() else {
        return Kind::Other;
    };
    let has = |name: &str| payload.get(name).is_some_and(|value| !value.is_null());
    if has("does") || (has("gate") && has("do")) {
        Kind::Step
    } else if has("purpose") {
        Kind::Process
    } else if has("do") && has("kind") {
        Kind::Role
    } else {
        Kind::Other
    }
}
