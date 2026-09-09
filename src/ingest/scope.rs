use super::jsonl::ParsedLine;
use crate::record::AtomicScope;

/// Keep skipped lines in the authorization request: a replay can repair
/// projections or unfinished writes, and that is still a store mutation.
pub(super) fn requested(lines: &[ParsedLine]) -> Vec<AtomicScope> {
    lines
        .iter()
        .map(|line| AtomicScope {
            line: line.number,
            namespace: line.record.namespace.clone(),
            type_name: line.record.type_name.clone(),
            payload: line.record.payload.clone(),
        })
        .collect()
}
