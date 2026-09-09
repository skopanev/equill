//! Which half of a hybrid selector's answer survives when both were offered.
use super::{Candidate, Strategy, Tier};
use crate::context::matching;
use crate::context::model::{ExcludedCoordinate, ExclusionReason};
use crate::retrieval::Source;
use std::collections::HashSet;
use uuid::Uuid;

/// Keep the answer of the configured first source, and the other one only when
/// the first has nothing left — decided once it is known which candidates
/// actually survived.
///
/// The contract is fallback, not top-up: the second source stands in for the
/// first, it does not extend it. Applying that before the coordinates, the
/// grants, the filter and the lifecycle had run would read a preferred half as
/// non-empty and then watch every one of its candidates be excluded, leaving
/// nothing where the other half could have answered.
///
/// The preference is the store's, not this function's: `hybrid_order` may name
/// text first, and a setting that is allowed to be written has to be allowed to
/// take effect.
///
/// Only matches made BY the hybrid strategy are affected. A record held by a
/// required or core tag keeps its place, and so does one a selector matched by
/// exact, tag, recency or its own `fts` strategy: those are not this decision.
pub(super) fn keep_the_first_source_that_answered(
    candidates: &mut Vec<Candidate>,
    found: &Found<'_>,
    excluded: &mut Vec<ExcludedCoordinate>,
) {
    let answered = candidates
        .iter()
        .any(|candidate| stood_in(candidate, found).is_some_and(|stood_in| !stood_in));
    if !answered {
        return;
    }
    let mut kept = Vec::with_capacity(candidates.len());
    for candidate in candidates.drain(..) {
        if stood_in(&candidate, found) == Some(true) {
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

/// What each half found, and which of them the store asked for first.
pub(super) struct Found<'a> {
    pub(super) vector: &'a HashSet<Uuid>,
    pub(super) fts: &'a HashSet<Uuid>,
    pub(super) preferred: Source,
}

impl<'a> Found<'a> {
    pub(super) fn new(
        vector: &'a HashSet<Uuid>,
        fts: &'a HashSet<Uuid>,
        policy: &crate::retrieval::Policy,
    ) -> Self {
        Self {
            vector,
            fts,
            preferred: policy.hybrid_order[0],
        }
    }

    fn preferred_holds(&self, id: &Uuid) -> bool {
        match self.preferred {
            Source::Vector => self.vector.contains(id),
            Source::Fts => self.fts.contains(id),
        }
    }
}

/// `Some(true)` when this candidate is here only because the second source
/// stood in for the first, `Some(false)` when the preferred source found it,
/// and `None` when the question does not apply to it.
fn stood_in(candidate: &Candidate, found: &Found<'_>) -> Option<bool> {
    let only_hybrid =
        candidate.tier == Tier::Relevant && candidate.strategies == [Strategy::Hybrid];
    only_hybrid.then(|| !found.preferred_holds(&candidate.record.id))
}
