//! The STEPS section: which steps survive, and how each one reads.
use super::Step;
use super::render::{commands, content, first};
use serde_json::Value;

/// Whether this value will survive `steps` below.
///
/// The same predicate the renderer applies, asked before the fact so that a
/// step dropped here is a step the record still gets credit for elsewhere. Two
/// copies of this rule would drift apart, and the drift would be silent: a step
/// counted as printed and then not printed is exactly the shape of the bug this
/// fixes.
pub(super) fn renders_as_step(value: &Value) -> bool {
    match value.as_object() {
        Some(fields) => first(fields, &["does", "do", "instruction", "text"]).is_some(),
        None => !content(value, false).join(" ").is_empty(),
    }
}

pub(super) fn steps(blocks: &mut Vec<String>, steps: Vec<Step>) {
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
