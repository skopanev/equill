use super::{TypeDefinition, failure};
use crate::kernel::error::Error;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) fn current(
    definitions: &BTreeMap<String, TypeDefinition>,
) -> Result<BTreeSet<String>, Error> {
    let mut successors: BTreeMap<&str, Vec<&str>> = definitions
        .keys()
        .map(|name| (name.as_str(), Vec::new()))
        .collect();
    let mut pending = BTreeMap::new();
    for (name, definition) in definitions {
        let predecessors = &definition.lifecycle.allowed_predecessor_types;
        pending.insert(name.as_str(), predecessors.len());
        for predecessor in predecessors {
            successors
                .get_mut(predecessor.as_str())
                .ok_or_else(|| failure("lineage references a missing definition"))?
                .push(name);
        }
    }
    let mut queue: VecDeque<_> = pending
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(name, _)| *name)
        .collect();
    let mut visited = 0;
    while let Some(name) = queue.pop_front() {
        visited += 1;
        for successor in &successors[name] {
            let count = pending.get_mut(successor).unwrap();
            *count -= 1;
            if *count == 0 {
                queue.push_back(*successor);
            }
        }
    }
    if visited != definitions.len() {
        return Err(failure("lineage contains a cycle"));
    }
    let mut seen = BTreeSet::new();
    let mut current = BTreeSet::new();
    for name in definitions.keys() {
        if seen.contains(name.as_str()) {
            continue;
        }
        let mut component = vec![name.as_str()];
        let mut leaves = Vec::new();
        while let Some(node) = component.pop() {
            if !seen.insert(node) {
                continue;
            }
            if successors[node].is_empty() {
                leaves.push(node);
            }
            component.extend(successors[node].iter().copied());
            component.extend(
                definitions[node]
                    .lifecycle
                    .allowed_predecessor_types
                    .iter()
                    .map(String::as_str),
            );
        }
        if leaves.len() != 1 {
            return Err(failure("lineage has multiple effective definitions"));
        }
        current.insert(leaves[0].to_owned());
    }
    Ok(current)
}
