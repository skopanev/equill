//! Stable section ordering and Markdown details for the LLM presentation.
use super::{Rule, Sections, Step};
use serde_json::Value;

pub(super) fn sections(mut sections: Sections) -> String {
    sections
        .roles
        .sort_by_key(|item| (item.order.unwrap_or(i64::MAX), item.text.clone(), item.id));
    sections
        .steps
        .sort_by_key(|item| (item.number.unwrap_or(i64::MAX), item.id));
    sort_rules(&mut sections.communication);
    sort_rules(&mut sections.ticketing);
    let mut blocks = Vec::new();
    bullets(
        &mut blocks,
        "ROLE",
        sections.roles.into_iter().map(|item| item.text).collect(),
    );
    prose(&mut blocks, "GOAL", sections.goals);
    prose(&mut blocks, "FINISH", sections.finishes);
    steps(&mut blocks, sections.steps);
    bullets(
        &mut blocks,
        "COMMUNICATION RULES",
        sections
            .communication
            .into_iter()
            .map(|item| item.text)
            .collect(),
    );
    bullets(
        &mut blocks,
        "TICKETING RULES",
        sections
            .ticketing
            .into_iter()
            .map(|item| item.text)
            .collect(),
    );
    memories(&mut blocks, sections.memory);
    blocks.join("\n\n")
}

fn sort_rules(rules: &mut [Rule]) {
    rules.sort_by_key(|item| (item.key.clone(), item.text.clone(), item.id));
}

fn prose(blocks: &mut Vec<String>, heading: &str, values: Vec<String>) {
    if !values.is_empty() {
        let values = values
            .into_iter()
            .map(|value| commands(&value))
            .collect::<Vec<_>>();
        blocks.push(format!("## {heading}\n{}", values.join("\n")));
    }
}

fn bullets(blocks: &mut Vec<String>, heading: &str, values: Vec<String>) {
    if !values.is_empty() {
        let body = values
            .into_iter()
            .map(|value| format!("- {}", commands(&value)))
            .collect::<Vec<_>>()
            .join("\n");
        blocks.push(format!("## {heading}\n{body}"));
    }
}

fn steps(blocks: &mut Vec<String>, steps: Vec<Step>) {
    let mut lines = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        let Some(fields) = step.value.as_object() else {
            let text = content(&step.value, false).join(" ");
            if !text.is_empty() {
                lines.push(format!("{}. {}", index + 1, commands(&text)));
            }
            continue;
        };
        let instruction = first(fields, &["does", "do", "instruction", "text"]);
        let Some(instruction) = instruction else {
            continue;
        };
        lines.push(format!("{}. {}", index + 1, commands(&instruction)));
        detail(&mut lines, fields.get("gate"), "Gate");
        detail(&mut lines, fields.get("on_fail"), "On fail");
    }
    if !lines.is_empty() {
        blocks.push(format!("## STEPS\n{}", lines.join("\n")));
    }
}

fn detail(lines: &mut Vec<String>, value: Option<&Value>, label: &str) {
    let values = value.map(|value| content(value, false)).unwrap_or_default();
    if !values.is_empty() {
        lines.push(format!("   {label}: {}", commands(&values.join("; "))));
    }
}

fn memories(blocks: &mut Vec<String>, memories: Vec<Vec<String>>) {
    if memories.is_empty() {
        return;
    }
    let mut lines = Vec::new();
    for memory in memories {
        let mut values = memory.into_iter();
        if let Some(first) = values.next() {
            lines.push(format!("- {}", commands(&first)));
        }
        lines.extend(values.map(|value| format!("  {}", commands(&value))));
    }
    blocks.push(format!("## RETRIEVED MEMORY\n{}", lines.join("\n")));
}

fn first(fields: &serde_json::Map<String, Value>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        fields
            .get(*name)
            .and_then(|value| content(value, false).into_iter().next())
    })
}

pub(super) fn content(value: &Value, code: bool) -> Vec<String> {
    match value {
        Value::String(text) if code => vec![fence(text)],
        Value::String(text) => vec![text.clone()],
        Value::Number(number) => vec![number.to_string()],
        Value::Bool(value) => vec![value.to_string()],
        Value::Array(values) => values
            .iter()
            .flat_map(|value| content(value, code))
            .collect(),
        Value::Object(fields) => fields
            .values()
            .flat_map(|value| content(value, code))
            .collect(),
        Value::Null => Vec::new(),
    }
}

pub(super) fn commands(text: &str) -> String {
    text.lines()
        .map(command_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn command_line(line: &str) -> String {
    let mut output = String::new();
    let mut cursor = 0;
    while let Some((start, command)) = find_command(line, cursor) {
        let end = command_end(line, start, command);
        output.push_str(&line[cursor..start]);
        output.push('`');
        output.push_str(&line[start..end]);
        output.push('`');
        cursor = end;
    }
    output.push_str(&line[cursor..]);
    output
}

fn find_command(line: &str, cursor: usize) -> Option<(usize, &'static str)> {
    const STARTS: [&str; 10] = [
        "EQUILL_",
        "agentbus ",
        "ntk ",
        "rtk ",
        "equill ",
        "cargo ",
        "git ",
        "~/",
        "./",
        "<",
    ];
    STARTS
        .iter()
        .filter_map(|command| {
            let mut offset = cursor;
            loop {
                let position = offset + line[offset..].find(command)?;
                let valid = !inside_code(line, position)
                    && (position == 0
                        || line[..position]
                            .chars()
                            .last()
                            .is_some_and(|ch| ch.is_whitespace() || matches!(ch, ':' | '(')))
                    && valid_start(&line[position..], command);
                if valid {
                    break Some((position, *command));
                }
                offset = position + command.len();
            }
        })
        .min_by_key(|(position, _)| *position)
}

fn valid_start(tail: &str, command: &str) -> bool {
    if command == "EQUILL_" {
        return tail
            .split_whitespace()
            .next()
            .is_some_and(|word| word.contains('='));
    }
    if matches!(command, "~/" | "./" | "<") {
        return tail
            .split_whitespace()
            .next()
            .is_some_and(|word| word.ends_with(".sh"));
    }
    true
}

fn inside_code(line: &str, position: usize) -> bool {
    line[..position].matches('`').count() % 2 == 1
}

fn command_end(line: &str, start: usize, command: &str) -> usize {
    let tail = &line[start..];
    for compact in ["agentbus drain", "ntk ls"] {
        if tail.starts_with(compact) && !tail[compact.len()..].trim_start().starts_with('-') {
            return start + compact.len();
        }
    }
    let end = [", ", "; ", ". ", " — ", " then ", " and then "]
        .iter()
        .filter_map(|boundary| tail.find(boundary))
        .min()
        .unwrap_or(tail.len());
    let end = tail[..end].trim_end_matches('.').len();
    // A recognized executable always contributes at least its own token.
    start + end.max(command.trim().len())
}

fn fence(text: &str) -> String {
    if text.starts_with('`') && text.ends_with('`') {
        text.to_owned()
    } else {
        format!("`{text}`")
    }
}
