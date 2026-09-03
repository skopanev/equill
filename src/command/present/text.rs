//! What a person reads when they ask a question in their own terminal.
//!
//! The tab-separated line this replaces was written for `cut`, not for a
//! reader: it led with a UUID nobody types, put every value in a column whose
//! header was somewhere else, and printed anything nested as escaped JSON — so
//! the one field most worth reading arrived least readable.
//!
//! A role, the process it obeys and the steps of that process arrive as
//! separate records, so the shape a reader wants exists only across the answer,
//! never inside one record. That is why this reads the whole set: the grouping
//! is the presentation.
use super::classify::{Kind, classify};
use crate::record::StoredRecord;
use serde_json::Value;

/// Named outright because these three carry the sentence; everything else the
/// record holds follows under its own name.
const ROLE_LEAD: &str = "role";
const PROCESS_LEAD: [&str; 2] = ["title", "name"];

/// The order a reader needs, fixed here rather than taken from the record.
/// A payload's own key order does not survive being stored — the map sorts its
/// keys — so a record written role, process, steps comes back process, role,
/// steps. Reading the order off the payload would print the alphabet and call
/// it the author's intent.
const ORDER: [&str; 3] = ["role", "process", "steps"];

/// What a step is read for, first. `does` is the step; `actor` and `project`
/// are bookkeeping about it, and bookkeeping printed above the instruction is
/// how a reader loses the instruction.
const STEP_ORDER: [&str; 4] = ["does", "do", "gate", "on_fail"];

/// Fields in a named order, with everything unnamed keeping the order it
/// arrived in. Used for both step shapes, because a step that arrives inside a
/// record and a step that arrives as one are the same thing to a reader — and
/// sorting only one of them leaves the other reading backwards.
fn in_order<'a>(
    fields: impl Iterator<Item = (&'a String, &'a Value)>,
    order: &[&str],
) -> Vec<(String, Value)> {
    let mut out: Vec<(String, Value)> = fields
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    out.sort_by_key(|(name, _)| {
        order
            .iter()
            .position(|named| named == name)
            .unwrap_or(order.len())
    });
    out
}

pub(super) fn answer(records: &[StoredRecord], fields: &[String]) -> String {
    if !fields.is_empty() {
        return records
            .iter()
            .map(|record| selected(record, fields))
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    // Grouped, not interleaved: the reader's order is Role, then Process, then
    // the steps of it. Printing these in the order the records happened to
    // arrive would make the answer's shape depend on the ledger, which is the
    // same mistake as reading a payload's key order off the store.
    let (mut roles, mut processes, mut steps, mut rest) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for (position, record) in records.iter().enumerate() {
        match classify(record) {
            Kind::Role => roles.push(headed("Role", ROLE_LEAD, record)),
            Kind::Process => processes.push(led("Process", &PROCESS_LEAD, record)),
            Kind::Step => steps.push((step_number(record), position, record)),
            // Records with no shape keep the order they came in, which is the
            // order the caller asked for.
            Kind::Other => rest.push(plain(record)),
        }
    }
    let mut blocks = roles;
    blocks.append(&mut processes);
    if !steps.is_empty() {
        blocks.push(numbered(&mut steps));
    }
    blocks.append(&mut rest);
    blocks.join("\n\n")
}

/// Steps carry their own number, and a gap in it is information: step 5 missing
/// from a process that has 4 and 6 is a reader's question, not something to
/// paper over by renumbering from one.
fn numbered(steps: &mut [(Option<i64>, usize, &StoredRecord)]) -> String {
    steps.sort_by_key(|(number, position, _)| (number.unwrap_or(i64::MAX), *position));
    let mut out = vec!["Steps:".to_string()];
    for (index, (number, _, record)) in steps.iter().enumerate() {
        let position = number.unwrap_or((index + 1) as i64);
        let mut lines = Vec::new();
        let fields = record.payload.as_object();
        for (name, value) in fields
            .map(|fields| in_order(fields.iter(), &STEP_ORDER))
            .unwrap_or_default()
        {
            if value.is_null() || matches!(name.as_str(), "step" | "process") {
                continue;
            }
            lines.push(super::label::pair(rename(&name), &value));
        }
        let body = lines.join("\n   ");
        out.push(format!("{position}. {body}"));
    }
    out.join("\n")
}

/// `does` is what the step does; a reader asked for `Do`. The record keeps its
/// own field names, the answer speaks the reader's.
fn rename(name: &str) -> &str {
    match name {
        "does" | "do" => "Do",
        "gate" => "Gate",
        "on_fail" | "on-fail" => "On fail",
        other => other,
    }
}

/// A heading whose value is one named field, with the rest beneath it.
fn headed(heading: &str, lead: &str, record: &StoredRecord) -> String {
    led(heading, &[lead], record)
}

fn led(heading: &str, leads: &[&str], record: &StoredRecord) -> String {
    let fields = payload(record);
    let named = leads
        .iter()
        .find_map(|lead| fields.iter().find(|(name, _)| name == lead));
    let mut out = match named {
        Some((_, value)) => vec![format!("{heading}: {}", super::label::scalar(value))],
        None => vec![format!("{heading}:")],
    };
    let taken = named.map(|(name, _)| name.clone());
    for (name, value) in fields {
        if value.is_null() || Some(&name) == taken.as_ref() {
            continue;
        }
        out.push(field(&name, &value));
    }
    out.join("\n")
}

/// A lesson, a finding, a note: no shape to lean on, so every field it does
/// carry is printed under its own name rather than dropped.
fn plain(record: &StoredRecord) -> String {
    let mut out = Vec::new();
    for (name, value) in payload(record) {
        if !value.is_null() {
            out.push(field(&name, &value));
        }
    }
    if out.is_empty() {
        out.push(super::label::scalar(&record.payload));
    }
    out.push(format!("Type: {}/{}", record.namespace, record.type_name));
    out.join("\n")
}

/// An explicit selection answers with exactly what was asked for, under the
/// names it was asked for by. A field the record does not carry still gets its
/// label: dropping the line would silently shorten the answer, and the reader
/// would not learn that what they asked for is missing.
fn selected(record: &StoredRecord, fields: &[String]) -> String {
    fields
        .iter()
        .map(|field| match super::lookup(record, field) {
            Some(value) if !value.is_null() => super::label::pair(field, &value),
            _ => format!("{}:", super::label::name(field)),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn payload(record: &StoredRecord) -> Vec<(String, Value)> {
    let Some(fields) = record.payload.as_object() else {
        return Vec::new();
    };
    // The named three first, in the order a reader needs them; the rest keep
    // the order the store gives back.
    in_order(fields.iter(), &ORDER)
}

/// Steps also arrive as an array inside one record, not only as records of
/// their own. Same reader, same shape: numbered, one named part per line.
fn inline_steps(steps: &[Value]) -> String {
    let mut out = vec!["Steps:".to_string()];
    for (index, step) in steps.iter().enumerate() {
        let position = index + 1;
        match step.as_object() {
            Some(fields) => {
                let body = in_order(fields.iter(), &STEP_ORDER)
                    .into_iter()
                    .filter(|(_, value)| !value.is_null())
                    .map(|(name, value)| super::label::pair(rename(&name), &value))
                    .collect::<Vec<_>>()
                    .join("\n   ");
                out.push(format!("{position}. {body}"));
            }
            None => out.push(format!("{position}. {}", super::label::scalar(step))),
        }
    }
    out.join("\n")
}

/// One field of a record, printed the way that field deserves.
fn field(name: &str, value: &Value) -> String {
    match (name, value) {
        ("steps", Value::Array(steps)) if !steps.is_empty() => inline_steps(steps),
        _ => super::label::pair(rename(name), value),
    }
}

fn step_number(record: &StoredRecord) -> Option<i64> {
    match record.payload.get("step")? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}
