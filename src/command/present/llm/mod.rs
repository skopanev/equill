//! Prompt-ready Markdown made only from selected domain content.
mod fallback;
mod kind;
mod recall;
mod render;
mod render_steps;
mod said;

use crate::record::StoredRecord;
use serde_json::Value;

#[derive(Default)]
struct Sections {
    roles: Vec<Ordered>,
    goals: Vec<Said>,
    finishes: Vec<Said>,
    steps: Vec<Step>,
    communication: Vec<Rule>,
    ticketing: Vec<Rule>,
    memory: Vec<Said>,
    /// What has already been said, keyed by the record's namespace, type and
    /// whole payload. Two records that differ only in scope render the same
    /// sentence, and collapsing on that sentence would silently answer one
    /// question where two were asked; envelope identity is excluded so that a
    /// genuine re-record of the same fact still coalesces.
    seen: Vec<(String, String, Value)>,
    /// Everything the specialized sections did not say. A record the selection
    /// returned is part of the contract whether or not this formatter knows a
    /// shape for it, and dropping it makes the receipt lie: selection and
    /// receipt stay green while the agent never sees what it was given.
    records: Vec<Vec<String>>,
}

/// A block that has been said, kept with the payload that said it.
///
/// The payload is here for one reason: when a second record renders to the
/// same words, the reader needs to see which is which. Keeping only the words
/// would leave two identical bullets, and keeping the whole payload always
/// would bury the compact answer under metadata nobody asked for — so the
/// difference is added when, and only when, there is one.
pub(super) struct Said {
    /// The text as first rendered, never annotated. Matching runs on this:
    /// annotating what is matched on breaks the group the moment it grows past
    /// two, because the later arrivals no longer recognize the earlier ones.
    pub(super) key: Vec<String>,
    pub(super) payload: Value,
    pub(super) namespace: String,
    pub(super) type_name: String,
    pub(super) lines: Vec<String>,
}

struct Ordered {
    order: Option<i64>,
    text: String,
    id: uuid::Uuid,
}

struct Step {
    number: Option<i64>,
    value: Value,
    id: uuid::Uuid,
}

struct Rule {
    key: String,
    text: String,
    id: uuid::Uuid,
}

pub(super) fn answer(records: &[StoredRecord]) -> String {
    let mut sections = Sections::default();
    for record in records {
        // Asked once per record, here. A record recorded twice unchanged is one
        // fact and is said once; everything below then decides only where the
        // records that remain are said.
        if !fresh(&mut sections.seen, record) {
            continue;
        }
        // Whether the specialized branch actually said anything, not whether one
        // exists. A role with no `do`, a process with neither purpose nor
        // ends_when, a rule with no text: all correctly recognized and all
        // silently dropped, which no check against the unknown-type case can
        // catch.
        let rendered = match kind::classify_llm(record) {
            kind::LlmKind::Role => role(&mut sections, record),
            kind::LlmKind::Process => process(&mut sections, record),
            kind::LlmKind::Step => step(&mut sections, record),
            kind::LlmKind::Communication => rule(&mut sections.communication, record),
            kind::LlmKind::Ticketing => rule(&mut sections.ticketing, record),
            kind::LlmKind::Lesson => memory(&mut sections, record, recall::lesson(record)),
            kind::LlmKind::Finding => memory(&mut sections, record, recall::finding(record)),
            kind::LlmKind::Note => memory(&mut sections, record, recall::field(record, "text")),
            kind::LlmKind::Other => false,
        };
        // Counted separately: steps carried inside a record are content, but
        // they are not the rest of that record's payload, so finding them is
        // not a reason to consider the record said.
        inline_steps(&mut sections, record);
        if !rendered {
            fallback::fallback(&mut sections, record);
        }
    }
    render::sections(sections)
}

fn role(sections: &mut Sections, record: &StoredRecord) -> bool {
    if let Some(text) = string(record.payload.get("do")) {
        sections.roles.push(Ordered {
            order: number(record.payload.get("order")),
            text,
            id: record.id,
        });
        return true;
    }
    false
}

/// Same rule as `memory`, for the sections that hold bare sentences: a repeat
/// of a sentence is not an answer for a different record, only for the same one.
fn process(sections: &mut Sections, record: &StoredRecord) -> bool {
    let purpose = say_one(
        &mut sections.goals,
        record,
        string(record.payload.get("purpose")),
    );
    let ends = say_one(
        &mut sections.finishes,
        record,
        string(record.payload.get("ends_when")),
    );
    purpose || ends
}

fn say_one(said: &mut Vec<Said>, record: &StoredRecord, value: Option<String>) -> bool {
    match value {
        Some(value) => said::say(said, record, vec![value]),
        None => false,
    }
}

/// Whether the step will actually be printed, asked with the renderer's own
/// predicate rather than assumed from having pushed one.
///
/// A step with no instruction is dropped at render time, so a record counted as
/// covered because a `Step` was pushed would disappear anyway — the same silent
/// loss, one layer down.
fn step(sections: &mut Sections, record: &StoredRecord) -> bool {
    let rendered = render_steps::renders_as_step(&record.payload);
    sections.steps.push(Step {
        number: number(record.payload.get("step")),
        value: record.payload.clone(),
        id: record.id,
    });
    rendered
}

fn inline_steps(sections: &mut Sections, record: &StoredRecord) {
    let Some(Value::Array(values)) = record.payload.get("steps") else {
        return;
    };
    for (index, value) in values.iter().enumerate() {
        sections.steps.push(Step {
            number: Some(index as i64 + 1),
            value: value.clone(),
            id: record.id,
        });
    }
}

fn rule(out: &mut Vec<Rule>, record: &StoredRecord) -> bool {
    let Some(text) = string(record.payload.get("rule")) else {
        return false;
    };
    out.push(Rule {
        key: string(record.payload.get("key")).unwrap_or_default(),
        text,
        id: record.id,
    });
    true
}

/// Identical rendered text from two records collapses, because printing the
/// same sentence twice tells a reader nothing. Content that differs never
/// collapses — the comparison is over the whole rendered block, so two lessons
/// sharing a rule but differing in scope stay two.
/// Adds the block, and says whether this record is answered by it.
///
/// A record whose rendering is already on the page is only answered by it when
/// the record itself is the same one. Two lessons sharing a sentence and
/// differing in scope are two facts: printing the sentence twice shows the
/// reader nothing, and printing it once answers one of the two questions. So
/// the second is not claimed here — it goes to RECORDS, where the field that
/// makes it different is visible.
fn memory(sections: &mut Sections, record: &StoredRecord, lines: Vec<String>) -> bool {
    said::say(&mut sections.memory, record, lines)
}

fn fresh(seen: &mut Vec<(String, String, Value)>, record: &StoredRecord) -> bool {
    let key = (
        record.namespace.clone(),
        record.type_name.clone(),
        record.payload.clone(),
    );
    if seen.contains(&key) {
        return false;
    }
    seen.push(key);
    true
}

fn string(value: Option<&Value>) -> Option<String> {
    value?
        .as_str()
        .map(str::to_owned)
        .filter(|text| !text.is_empty())
}

fn number(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    value.as_i64().or_else(|| value.as_str()?.parse().ok())
}

#[cfg(test)]
mod category_tests;
#[cfg(test)]
mod coverage_tests;
#[cfg(test)]
mod tests;
