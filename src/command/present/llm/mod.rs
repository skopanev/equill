//! Prompt-ready Markdown made only from selected domain content.
mod render;

use super::classify::{Kind, classify};
use crate::record::StoredRecord;
use serde_json::Value;

#[derive(Default)]
struct Sections {
    roles: Vec<Ordered>,
    goals: Vec<String>,
    finishes: Vec<String>,
    steps: Vec<Step>,
    communication: Vec<Rule>,
    ticketing: Vec<Rule>,
    memory: Vec<Vec<String>>,
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
        match classify_llm(record) {
            LlmKind::Role => role(&mut sections, record),
            LlmKind::Process => process(&mut sections, record),
            LlmKind::Step => step(&mut sections, record),
            LlmKind::Communication => rule(&mut sections.communication, record),
            LlmKind::Ticketing => rule(&mut sections.ticketing, record),
            LlmKind::Lesson => memory(&mut sections, lesson(record)),
            LlmKind::Finding => memory(&mut sections, finding(record)),
            LlmKind::Note => memory(&mut sections, field(record, "text")),
            LlmKind::Other => {}
        }
        inline_steps(&mut sections, record);
    }
    render::sections(sections)
}

#[derive(Clone, Copy)]
enum LlmKind {
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

fn classify_llm(record: &StoredRecord) -> LlmKind {
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

fn rule_kind(record: &StoredRecord) -> Option<LlmKind> {
    let category = ["module", "rules"]
        .iter()
        .find_map(|name| record.payload.get(*name).and_then(Value::as_str))?;
    match category {
        "communication" | "comm" => Some(LlmKind::Communication),
        "tickets" | "ticketing" => Some(LlmKind::Ticketing),
        _ => None,
    }
}

fn role(sections: &mut Sections, record: &StoredRecord) {
    if let Some(text) = string(record.payload.get("do")) {
        sections.roles.push(Ordered {
            order: number(record.payload.get("order")),
            text,
            id: record.id,
        });
    }
}

fn process(sections: &mut Sections, record: &StoredRecord) {
    push_unique(&mut sections.goals, string(record.payload.get("purpose")));
    push_unique(
        &mut sections.finishes,
        string(record.payload.get("ends_when")),
    );
}

fn step(sections: &mut Sections, record: &StoredRecord) {
    sections.steps.push(Step {
        number: number(record.payload.get("step")),
        value: record.payload.clone(),
        id: record.id,
    });
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

fn rule(out: &mut Vec<Rule>, record: &StoredRecord) {
    let Some(text) = string(record.payload.get("rule")) else {
        return;
    };
    out.push(Rule {
        key: string(record.payload.get("key")).unwrap_or_default(),
        text,
        id: record.id,
    });
}

fn lesson(record: &StoredRecord) -> Vec<String> {
    field(record, "rule")
}

fn finding(record: &StoredRecord) -> Vec<String> {
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

fn field(record: &StoredRecord, name: &str) -> Vec<String> {
    record
        .payload
        .get(name)
        .map(|value| render::content(value, false))
        .unwrap_or_default()
}

fn append(lines: &mut Vec<String>, label: &str, value: Option<&Value>, code: bool) {
    if let Some(value) = value {
        lines.extend(
            render::content(value, code)
                .into_iter()
                .map(|value| format!("{label}: {value}")),
        );
    }
}

fn memory(sections: &mut Sections, lines: Vec<String>) {
    if !lines.is_empty() && !sections.memory.contains(&lines) {
        sections.memory.push(lines);
    }
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

fn push_unique(out: &mut Vec<String>, value: Option<String>) {
    if let Some(value) = value
        && !out.contains(&value)
    {
        out.push(value);
    }
}

#[cfg(test)]
mod tests;
