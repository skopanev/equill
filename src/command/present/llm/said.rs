//! Saying a thing once, and telling records apart when they say the same.
use super::Said;
use super::fallback;
use crate::record::StoredRecord;

/// Adds a block and re-annotates every block that reads the same.
///
/// The whole group, not just the newcomer: annotate only the arrival and the
/// third record finds no match, because the first two no longer read the way
/// they were written. Matching runs on the text as first rendered — kept
/// unchanged for exactly this reason — while what is displayed carries the
/// annotations on top.
///
/// Nothing coalesces here. Whether two records are one fact was decided once,
/// above, on namespace, type and full payload; deciding it again on a weaker
/// signal is how a record of a different type silently disappeared behind an
/// identical sentence.
pub(super) fn say(said: &mut Vec<Said>, record: &StoredRecord, lines: Vec<String>) -> bool {
    if lines.is_empty() {
        return false;
    }
    let key = lines.clone();
    said.push(Said {
        key: key.clone(),
        payload: record.payload.clone(),
        namespace: record.namespace.clone(),
        type_name: record.type_name.clone(),
        lines,
    });
    annotate(said, &key);
    true
}

/// Rewrites the display lines of one group: the text as written, plus whatever
/// distinguishes its members from each other.
///
/// A group of one is left exactly as it was — a lone record has nothing to be
/// told apart from, and an annotation there would be metadata nobody asked for.
fn annotate(said: &mut [Said], key: &[String]) {
    let members: Vec<usize> = said
        .iter()
        .enumerate()
        .filter(|(_, item)| item.key == key)
        .map(|(index, _)| index)
        .collect();
    if members.len() < 2 {
        return;
    }
    let names = distinguishing(said, &members);
    for index in &members {
        let mut lines = said[*index].key.clone();
        for name in &names {
            if let Some(value) = said[*index].payload.get(name) {
                lines.push(format!("{name}: {}", fallback::literal(value)));
            }
        }
        said[*index].lines = lines;
    }
    label_remaining_duplicates(said, &members);
}

/// Gives a namespace and type to the blocks that still read alike.
///
/// The payload fields settle most of a group, but not the members that differ
/// only in what they are — and in a mixed group they are the minority. Adding
/// the label to everyone would bury the compact answer under provenance the
/// reader did not ask for, so it goes only to the blocks that would otherwise
/// be indistinguishable from another record's.
fn label_remaining_duplicates(said: &mut [Said], members: &[usize]) {
    let mut collisions = Vec::new();
    for (position, index) in members.iter().enumerate() {
        let clashes = members.iter().enumerate().any(|(other_position, other)| {
            other_position != position && said[*other].lines == said[*index].lines
        });
        if clashes {
            collisions.push(*index);
        }
    }
    for index in collisions {
        let label = format!("type: {}/{}", said[index].namespace, said[index].type_name);
        said[index].lines.push(label);
    }
}

/// The payload field names on which the group's members disagree.
fn distinguishing(said: &[Said], members: &[usize]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for index in members {
        if let Some(fields) = said[*index].payload.as_object() {
            for name in fields.keys() {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
    }
    names.sort();
    names.retain(|name| {
        let first = said[members[0]].payload.get(name);
        members
            .iter()
            .any(|index| said[*index].payload.get(name) != first)
    });
    names
}
