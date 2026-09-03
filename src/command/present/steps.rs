//! Printing a process's steps: the order they are in, the numbers they carry,
//! and the three parts a reader acts on.
use super::label;
use serde_json::Value;

/// What a step is read for, first. `does` is the step; everything else is
/// bookkeeping about it.
const STEP_ORDER: [&str; 4] = ["does", "do", "gate", "on_fail"];

/// The whole of a step, as far as a reader is concerned: what to do, what says
/// it is done, and what to do when it is not. Who owns it, why it exists,
/// which project it belongs to, whether it can be undone, its own number —
/// all bookkeeping about the step rather than the step, and printing it turns
/// three lines a person can act on into a wall they have to search.
const STEP_PARTS: [&str; 4] = ["does", "do", "gate", "on_fail"];

pub(super) fn is_part_of_a_step(name: &str) -> bool {
    STEP_PARTS.contains(&name)
}

/// `does` is what the step does; a reader asked for `Do`. The record keeps its
/// own field names, the answer speaks the reader's.
pub(super) fn rename(name: &str) -> &str {
    match name {
        "does" | "do" => "Do",
        "gate" => "Gate",
        "on_fail" | "on-fail" => "On fail",
        other => other,
    }
}

/// Steps carry their own number, and a gap in it is information: step 5 missing
/// from a process that has 4 and 6 is a reader's question, not something to
/// paper over by renumbering from one.
pub(super) fn numbered(steps: &mut [(Option<i64>, usize, Value)]) -> String {
    steps.sort_by_key(|(number, position, _)| (number.unwrap_or(i64::MAX), *position));
    let mut out = vec!["Steps:".to_string()];
    for (index, (number, _, step)) in steps.iter().enumerate() {
        let position = number.unwrap_or((index + 1) as i64);
        let mut parts = match step.as_object() {
            Some(fields) => super::text::in_order(fields.iter(), &STEP_ORDER)
                .into_iter()
                .filter(|(name, value)| !value.is_null() && is_part_of_a_step(name))
                .map(|(name, value)| label::pair(rename(&name), &value))
                .collect::<Vec<_>>(),
            // A step written as a sentence rather than as parts is that
            // sentence; there is nothing to select from it.
            None => vec![format!("Do: {}", label::scalar(step))],
        };
        // The schema requires an instruction, so a step without one is a
        // record that broke its own contract. It is still printed — an empty
        // `Do:` says a step is there and says what it does not say, where
        // dropping it would quietly shorten the process.
        if !parts.iter().any(|line| line.starts_with("Do:")) {
            parts.insert(0, "Do:".to_string());
        }
        out.push(format!("{position}. {}", parts.join("\n   ")));
    }
    out.join("\n")
}
