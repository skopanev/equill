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

/// Fields in a named order, with everything unnamed keeping the order it
/// arrived in. Used for both step shapes, because a step that arrives inside a
/// record and a step that arrives as one are the same thing to a reader — and
/// sorting only one of them leaves the other reading backwards.
pub(super) fn in_order<'a>(
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
    // Two sources for the heading, kept apart on purpose. A record that
    // describes the process names it; a record that merely belongs to one only
    // refers to it. Held in one slot, whichever arrived first would win, so a
    // bundle listing `process: "premerge"` before the process record itself
    // would leave the answer headed by the code instead of the title.
    let (mut roles, mut named, mut referred, mut steps, mut rest) =
        (Vec::<String>::new(), None, None, Vec::new(), Vec::new());
    for (position, record) in records.iter().enumerate() {
        let kind = classify(record);
        // A role, its process and that process's steps arrive as separate
        // records — and sometimes as one record carrying all three. Both are
        // read the same way, so the parts are looked for in the fields rather
        // than assumed from the record's type.
        if kind == Kind::Role {
            // One heading, every role. Several records can describe the same
            // role — a revision, a narrower grant, the same role seen by two
            // profiles — and a heading per record makes one role look like
            // several. Repeats collapse; genuinely different roles are all
            // named on that one line, because hiding one would be hiding a
            // fact rather than tidying a heading.
            if let Some(named) = record
                .payload
                .get(ROLE_LEAD)
                .filter(|value| !value.is_null())
                .map(super::label::scalar)
                .filter(|named| !named.is_empty())
                && !roles.contains(&named)
            {
                roles.push(named);
            }
        }
        if kind == Kind::Process {
            named.get_or_insert_with(|| heading_of("Process", &PROCESS_LEAD, record));
        } else if kind != Kind::Step
            && let Some(reference) = record.payload.get("process").filter(|it| it.is_string())
        {
            // Only when the record is describing a process, never when it is
            // naming the one it belongs to: a step carries `process` as a
            // reference, and reading that as a heading invents a process block
            // out of a step's bookkeeping.
            referred.get_or_insert_with(|| format!("Process: {}", super::label::scalar(reference)));
        }
        match record.payload.get("steps") {
            Some(Value::Array(carried)) => {
                // Steps written inside one record are numbered by where they
                // sit in the list: the order they were written in is the only
                // order they have.
                for (index, step) in carried.iter().enumerate() {
                    steps.push((Some(index as i64 + 1), position, step.clone()));
                }
            }
            _ if kind == Kind::Step => {
                steps.push((step_number(record), position, record.payload.clone()));
            }
            _ if kind == Kind::Other => rest.push(plain(record)),
            _ => {}
        }
    }
    let mut blocks: Vec<String> = Vec::new();
    if !roles.is_empty() {
        blocks.push(format!("Role: {}", roles.join(", ")));
    }
    // The record that describes the process wins over the one that only names
    // it, whichever arrived first.
    blocks.extend(named.or(referred));
    if !steps.is_empty() {
        blocks.push(super::steps::numbered(&mut steps));
    }
    blocks.append(&mut rest);
    blocks.join("\n\n")
}

/// The heading and nothing else. What a role is for and how it is graded lives
/// in the record; an answer a person reads at a glance is not the place to
/// unload it, and printing everything is what made the old output unreadable
/// in the first place.
fn heading_of(heading: &str, leads: &[&str], record: &StoredRecord) -> String {
    let fields = record.payload.as_object();
    let named = leads.iter().find_map(|lead| {
        fields
            .and_then(|fields| fields.get(*lead))
            .filter(|value| !value.is_null())
    });
    match named {
        Some(value) => format!("{heading}: {}", super::label::scalar(value)),
        None => format!("{heading}:"),
    }
}

/// A lesson, a finding, a note: no shape to lean on, so every field it does
/// carry is printed under its own name rather than dropped.
fn plain(record: &StoredRecord) -> String {
    let mut out = Vec::new();
    for (name, value) in payload(record) {
        if !value.is_null() {
            out.push(super::label::pair(super::steps::rename(&name), &value));
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

fn step_number(record: &StoredRecord) -> Option<i64> {
    match record.payload.get("step")? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}
