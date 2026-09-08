use super::{SearchSource, Strategy};
use crate::context::model::{ContextRequest, Selector};
use crate::kernel::error::Error;
use crate::projection::{self, ProjectionState, SearchRequest};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const DEFAULT_LIMIT: usize = 100;

pub(super) struct Hits {
    pub(super) fts: HashSet<Uuid>,
    pub(super) semantic: HashSet<Uuid>,
    pub(super) vector: HashSet<Uuid>,
    pub(super) ranks: HashMap<Uuid, usize>,
    pub(super) answer: Option<crate::context::model::SemanticAnswer>,
    order: [crate::retrieval::Source; 2],
    mixed: bool,
}

impl Hits {
    pub(super) fn source(&self, id: &Uuid) -> (Option<SearchSource>, Option<usize>) {
        if !self.mixed {
            return (None, None);
        }
        let source = self.order.iter().find_map(|source| match source {
            crate::retrieval::Source::Vector if self.vector.contains(id) => {
                Some(SearchSource::Vector)
            }
            crate::retrieval::Source::Fts if self.fts.contains(id) => Some(SearchSource::Fts),
            _ => None,
        });
        (source, self.ranks.get(id).copied())
    }
}

pub(super) fn collect(
    store: &std::path::Path,
    selectors: &[Selector],
    request: &ContextRequest,
    state: ProjectionState,
    record_limit: Option<usize>,
    policy: &crate::retrieval::Policy,
) -> Result<Hits, Error> {
    let text = text(store, selectors, request, state, record_limit)?;
    let semantic = crate::context::semantic::hits(store, selectors, request, record_limit, policy)?;
    let mixed = record_limit.is_some() && semantic.answer.is_some();
    if !mixed {
        return Ok(Hits {
            fts: text.ids,
            semantic: semantic.ids,
            vector: HashSet::new(),
            ranks: HashMap::new(),
            answer: semantic.answer,
            order: policy.hybrid_order,
            mixed: false,
        });
    }
    let vector = semantic.ids;
    let (primary, secondary) = if policy.hybrid_order[0] == crate::retrieval::Source::Vector {
        (&vector, &text.ids)
    } else {
        (&text.ids, &vector)
    };
    let mut hybrid = primary.clone();
    if policy.hybrid_fill_remaining {
        hybrid.extend(secondary.iter().copied());
    }
    let mut ranks = HashMap::new();
    for source in policy.hybrid_order {
        let ordered = if source == crate::retrieval::Source::Vector {
            &semantic.ordered
        } else {
            &text.ordered
        };
        for (rank, id) in ordered.iter().copied().enumerate() {
            ranks.entry(id).or_insert(rank);
        }
    }
    Ok(Hits {
        fts: text.ids,
        semantic: hybrid,
        vector,
        ranks,
        answer: semantic.answer,
        order: policy.hybrid_order,
        mixed: true,
    })
}

struct RankedIds {
    ids: HashSet<Uuid>,
    ordered: Vec<Uuid>,
}

fn text(
    store: &std::path::Path,
    selectors: &[Selector],
    request: &ContextRequest,
    state: ProjectionState,
    record_limit: Option<usize>,
) -> Result<RankedIds, Error> {
    let mut found = RankedIds {
        ids: HashSet::new(),
        ordered: Vec::new(),
    };
    if request.query.trim().is_empty() || state != ProjectionState::Ready {
        return Ok(found);
    }
    for selector in selectors.iter().filter(|item| {
        item.strategies.contains(&Strategy::Fts)
            || (record_limit.is_some() && item.strategies.contains(&Strategy::Hybrid))
    }) {
        let report = projection::search(
            store,
            &SearchRequest {
                query: Some(request.query.clone()),
                namespace: None,
                type_name: Some(selector.type_name.clone()),
                limit: u16::try_from(record_limit.unwrap_or(DEFAULT_LIMIT)).unwrap_or(u16::MAX),
            },
        )?;
        for id in report.hits.into_iter().map(|hit| hit.record.id) {
            if found.ids.insert(id) {
                found.ordered.push(id);
            }
        }
    }
    Ok(found)
}
