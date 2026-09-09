use super::matching;
use super::model::{
    ContextProfile, ContextRequest, ExcludedCoordinate, ExclusionReason, Expectation, RankOrder,
    Selector, Strategy, Tier,
};
use crate::filter::Filter;
use crate::kernel::error::Error;
use crate::projection::{self, ProjectionState};
use crate::record::StoredRecord;
use std::collections::{BTreeSet, HashMap, HashSet};
mod fallback;
#[cfg(test)]
pub(crate) mod probe;
mod search;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SearchSource {
    Vector,
    Fts,
}

pub struct Candidate {
    pub record: StoredRecord,
    pub tier: Tier,
    pub strategies: Vec<Strategy>,
    pub score: usize,
    pub rank: Option<f64>,
    pub source: Option<SearchSource>,
    pub search_rank: Option<usize>,
}

pub struct Retrieval {
    pub unmatched_coordinates: Vec<super::model::UnmatchedCoordinate>,
    pub candidates: Vec<Candidate>,
    pub excluded: Vec<ExcludedCoordinate>,
    pub strategies: Vec<Strategy>,
    pub degraded_strategies: Vec<Strategy>,
    pub semantic: Option<super::model::SemanticAnswer>,
    pub projection: ProjectionState,
    /// The configured rule that suppressed the query path, if one did. The
    /// query itself is never carried: the receipt names the rule an operator
    /// wrote, not the line a caller sent.
    pub query_skipped_by: Option<String>,
}

/// Whether a selector's `expect` has anything to apply to.
///
/// `expect` describes the answer to a question: exactly one process, at least
/// one role. A health check asks nothing — it walks the registry to see whether
/// the profiles are well formed — so a store that happens to hold two of
/// something is not a broken profile there, it is a store the profile would
/// narrow if anybody asked. Enforcing cardinality with no request to enforce it
/// for turned a healthy store into a failing doctor.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    /// Somebody asked. The count is part of the answer being right.
    Answering,
    /// Nobody asked. Everything else about the profile is still checked.
    Diagnosing,
}

#[allow(clippy::too_many_arguments)]
pub fn retrieve(
    store: &std::path::Path,
    profile: &ContextProfile,
    selectors: &[Selector],
    request: &ContextRequest,
    filter: &Filter,
    cardinality: Cardinality,
    record_limit: Option<usize>,
    policy: &crate::retrieval::Policy,
    mut records: Vec<StoredRecord>,
) -> Result<Retrieval, Error> {
    let at: jiff::Timestamp = request
        .at
        .parse()
        .map_err(|_| Error::Context("request at must be RFC3339".into()))?;
    records.sort_by_key(|record| record.id);
    let selector_map = selectors
        .iter()
        .map(|selector| (selector.type_name.as_str(), selector))
        .collect::<HashMap<_, _>>();
    let superseded = if request.include_superseded {
        HashSet::new()
    } else {
        records
            .iter()
            .filter(|record| matching::read_authorized(record, profile))
            .filter(|record| selector_map.contains_key(record.type_name.as_str()))
            .filter_map(|record| record.supersedes)
            .collect::<HashSet<_>>()
    };
    let projection = projection::state(store)?;
    let search = search::collect(store, selectors, request, projection, record_limit, policy)?;
    let strategies: Vec<Strategy> = selectors
        .iter()
        .flat_map(|selector| selector.strategies.iter().copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let degraded_strategies =
        if projection == ProjectionState::Ready || !strategies.contains(&Strategy::Fts) {
            Vec::new()
        } else {
            vec![Strategy::Fts]
        };
    let readable = records
        .iter()
        .filter(|record| matching::read_authorized(record, profile))
        .filter(|record| selector_map.contains_key(record.type_name.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let unmatched_coordinates =
        matching::coordinate_diagnosis(&readable, &selectors.iter().collect::<Vec<_>>(), request);
    let mut candidates = Vec::new();
    let mut excluded = Vec::new();
    for record in records {
        // Filtering here, before a candidate is ever built, keeps excluded
        // records from consuming the budget that the caller asked to spend on
        // what it actually wanted.
        if !crate::filter::matches(&record, filter) {
            excluded.push(matching::exclusion(
                &record,
                ExclusionReason::FilterMismatch,
            ));
            continue;
        }
        let reason = matching::gate(&record, profile, &selector_map, request, at, &superseded)?;
        if let Some(reason) = reason {
            excluded.push(matching::exclusion(&record, reason));
            continue;
        }
        let selector = selector_map[record.type_name.as_str()];
        match matching::classify(&record, selector, request, &search.fts, &search.semantic) {
            Some((tier, matched)) => {
                let (source, search_rank) = if tier == Tier::Relevant {
                    search.source(&record.id)
                } else {
                    (None, None)
                };
                candidates.push(Candidate {
                    // Negated for an ascending selector so that one comparator
                    // still orders every candidate. The number is never shown; it
                    // exists to sort by, and sorting is all it is used for.
                    rank: selector
                        .rank_pointer
                        .as_ref()
                        .and_then(|pointer| record.payload.pointer(pointer))
                        .and_then(serde_json::Value::as_f64)
                        .map(|value| match selector.rank_order {
                            RankOrder::Asc => -value,
                            RankOrder::Desc => value,
                        }),
                    record,
                    tier,
                    score: matched.len(),
                    strategies: matched,
                    source,
                    search_rank,
                })
            }
            None => excluded.push(matching::exclusion(
                &record,
                super::model::ExclusionReason::SelectorMismatch,
            )),
        }
    }
    if !policy.hybrid_fill_remaining {
        fallback::text_answers_only_when_vectors_did_not(
            &mut candidates,
            &search.vector,
            &mut excluded,
        );
    }
    candidates.sort_by(|left, right| {
        left.tier
            .cmp(&right.tier)
            .then_with(|| {
                source_order(left.source, policy).cmp(&source_order(right.source, policy))
            })
            .then_with(|| left.search_rank.cmp(&right.search_rank))
            .then_with(|| match (left.rank, right.rank) {
                (Some(left), Some(right)) => right.total_cmp(&left),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => right.score.cmp(&left.score),
            })
            .then_with(|| right.record.observed_at.cmp(&left.record.observed_at))
            .then_with(|| left.record.id.cmp(&right.record.id))
    });
    if cardinality == Cardinality::Answering {
        require(&candidates, selectors)?;
    }
    excluded.sort_by_key(|item| item.id);
    Ok(Retrieval {
        candidates,
        excluded,
        unmatched_coordinates,
        strategies,
        degraded_strategies,
        semantic: search.answer,
        projection,
        query_skipped_by: search.skipped_by,
    })
}

fn source_order(source: Option<SearchSource>, policy: &crate::retrieval::Policy) -> u8 {
    let wanted = match source {
        Some(SearchSource::Vector) => crate::retrieval::Source::Vector,
        Some(SearchSource::Fts) => crate::retrieval::Source::Fts,
        None => return 2,
    };
    if policy.hybrid_order[0] == wanted {
        0
    } else {
        1
    }
}

/// Hold each selector to what the profile said it must find.
///
/// Checked here, where the candidates are, and before anything is budgeted or
/// rendered: a refusal has to happen instead of an answer, not alongside a
/// partial one.
fn require(candidates: &[Candidate], selectors: &[Selector]) -> Result<(), Error> {
    for selector in selectors {
        let found = candidates
            .iter()
            .filter(|candidate| candidate.record.type_name == selector.type_name)
            .count();
        let wanted = match selector.expect {
            Expectation::Any => continue,
            Expectation::Some if found >= 1 => continue,
            Expectation::One if found == 1 => continue,
            Expectation::Some => "at least one record",
            Expectation::One => "exactly one record",
        };
        // Coordinates, not just a complaint: which selector, over which type,
        // what it expected and what it found. An error that says only that
        // something went wrong sends the reader looking in the wrong place.
        return Err(Error::Context(format!(
            "selector {} over {} expected {wanted} and found {found}",
            selector.id, selector.type_name
        )));
    }
    Ok(())
}
