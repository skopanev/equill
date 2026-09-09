//! Which half of a hybrid selector's answer survives when both were offered.
use super::{Candidate, Strategy, Tier};
use crate::context::matching;
use crate::context::model::{ExcludedCoordinate, ExclusionReason};

/// Drop the text half of a hybrid selector's answer when the vector half
/// answered, once it is known which candidates actually survived.
///
/// The contract is fallback, not top-up: text stands in for vectors, it does
/// not extend them. Applying it before the coordinates, grants, filter and
/// lifecycle had run would have read a vector half as non-empty and then
/// watched every one of its candidates be excluded, leaving nothing where the
/// text half could have answered.
///
/// Only matches made BY the hybrid strategy are affected. A record held by a
/// required or core tag keeps its place, and so does one a selector matched by
/// exact, tag, recency or its own `fts` strategy: those are not this decision.
pub(super) fn text_answers_only_when_vectors_did_not(
    candidates: &mut Vec<Candidate>,
    vectors: &std::collections::HashSet<uuid::Uuid>,
    excluded: &mut Vec<ExcludedCoordinate>,
) {
    let answered = candidates
        .iter()
        .any(|candidate| stood_in_for_vectors(candidate, vectors).is_some_and(|text| !text));
    if !answered {
        return;
    }
    let mut kept = Vec::with_capacity(candidates.len());
    for candidate in candidates.drain(..) {
        if stood_in_for_vectors(&candidate, vectors) == Some(true) {
            excluded.push(matching::exclusion(
                &candidate.record,
                ExclusionReason::SelectorMismatch,
            ));
        } else {
            kept.push(candidate);
        }
    }
    *candidates = kept;
}

/// `Some(true)` when this candidate is here only because the text half stood in
/// for the vector one, `Some(false)` when the vector half found it, and `None`
/// when the question does not apply to it.
fn stood_in_for_vectors(
    candidate: &Candidate,
    vectors: &std::collections::HashSet<uuid::Uuid>,
) -> Option<bool> {
    let only_hybrid =
        candidate.tier == Tier::Relevant && candidate.strategies == [Strategy::Hybrid];
    only_hybrid.then(|| !vectors.contains(&candidate.record.id))
}
