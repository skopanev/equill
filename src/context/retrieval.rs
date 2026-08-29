use super::matching;
use super::model::{
    ContextProfile, ContextRequest, ExcludedCoordinate, ExclusionReason, Selector, Strategy, Tier,
};
use crate::filter::Filter;
use crate::kernel::error::Error;
use crate::projection::{self, ProjectionState, SearchRequest};
use crate::record::StoredRecord;
use std::collections::{BTreeSet, HashMap, HashSet};

pub struct Candidate {
    pub record: StoredRecord,
    pub tier: Tier,
    pub strategies: Vec<Strategy>,
    pub score: usize,
    pub rank: Option<f64>,
}

pub struct Retrieval {
    pub unmatched_coordinates: Vec<super::model::UnmatchedCoordinate>,
    pub candidates: Vec<Candidate>,
    pub excluded: Vec<ExcludedCoordinate>,
    pub strategies: Vec<Strategy>,
    pub degraded_strategies: Vec<Strategy>,
    pub projection: ProjectionState,
}

pub fn retrieve(
    store: &std::path::Path,
    profile: &ContextProfile,
    selectors: &[Selector],
    request: &ContextRequest,
    filter: &Filter,
) -> Result<Retrieval, Error> {
    let at: jiff::Timestamp = request
        .at
        .parse()
        .map_err(|_| Error::Context("request at must be RFC3339".into()))?;
    let mut records = crate::record::read_all(store)?;
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
    let fts = fts_hits(store, selectors, request, projection)?;
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
        if !crate::filter::matches(&record.payload, filter) {
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
        match matching::classify(&record, selector, request, &fts) {
            Some((tier, matched)) => candidates.push(Candidate {
                rank: selector
                    .rank_pointer
                    .as_ref()
                    .and_then(|pointer| record.payload.pointer(pointer))
                    .and_then(serde_json::Value::as_f64),
                record,
                tier,
                score: matched.len(),
                strategies: matched,
            }),
            None => excluded.push(matching::exclusion(
                &record,
                super::model::ExclusionReason::SelectorMismatch,
            )),
        }
    }
    candidates.sort_by(|left, right| {
        left.tier
            .cmp(&right.tier)
            .then_with(|| match (left.rank, right.rank) {
                (Some(left), Some(right)) => right.total_cmp(&left),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => right.score.cmp(&left.score),
            })
            .then_with(|| right.record.observed_at.cmp(&left.record.observed_at))
            .then_with(|| left.record.id.cmp(&right.record.id))
    });
    excluded.sort_by_key(|item| item.id);
    Ok(Retrieval {
        candidates,
        excluded,
        unmatched_coordinates,
        strategies,
        degraded_strategies,
        projection,
    })
}

fn fts_hits(
    store: &std::path::Path,
    selectors: &[Selector],
    request: &ContextRequest,
    state: ProjectionState,
) -> Result<HashSet<uuid::Uuid>, Error> {
    let mut ids = HashSet::new();
    if request.query.trim().is_empty() || state != ProjectionState::Ready {
        return Ok(ids);
    }
    for selector in selectors
        .iter()
        .filter(|item| item.strategies.contains(&Strategy::Fts))
    {
        let report = projection::search(
            store,
            &SearchRequest {
                query: request.query.clone(),
                namespace: None,
                type_name: Some(selector.type_name.clone()),
                limit: 100,
            },
        )?;
        ids.extend(report.hits.into_iter().map(|hit| hit.record.id));
    }
    Ok(ids)
}
