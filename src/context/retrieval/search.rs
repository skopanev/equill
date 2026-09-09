use super::{SearchSource, Strategy};
use crate::context::model::{ContextRequest, Selector};
use crate::kernel::error::Error;
use crate::projection::{self, ProjectionState, SearchRequest};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

const DEFAULT_LIMIT: usize = 100;

pub(super) struct Hits {
    /// The configured rule that suppressed the query path, as an operator
    /// wrote it. `None` when no rule applied, which is every store that
    /// configures none.
    pub(super) skipped_by: Option<String>,
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
    // Before either half runs. The promise is zero calls to the projection and
    // zero to the embedder, and the only way to keep it is not to reach them.
    //
    // A request with no query is not a skippable one. Both halves already treat
    // a blank query as no search at all, so there is nothing here for a pattern
    // to suppress — and `.*` matches the empty string, which would have stamped
    // a rule into the receipt of every SessionStart and changed a digest that
    // has to stay what it was. The blank test is the one the query path itself
    // uses; a query that is not blank is matched exactly as written.
    if !request.query.trim().is_empty()
        && let Some(rule) = policy.skip_query_patterns.matched(&request.query)
    {
        return Ok(skipped(rule, policy));
    }
    let text = text(store, selectors, request, state, record_limit)?;
    let semantic = crate::context::semantic::hits(store, selectors, request, record_limit, policy)?;
    let mixed = record_limit.is_some() && semantic.answer.is_some();
    if !mixed {
        return Ok(Hits {
            skipped_by: None,
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
    // Both sources are offered to the classifier even when the second is only
    // a fallback. Deciding here would decide too early: these ids have not yet
    // met the coordinates, the grants, the filter or the lifecycle, so a
    // vector half that looks non-empty now can be empty by the time anything
    // is selected — and that is exactly when the text half is needed. The
    // choice is made in `retrieve`, once survival is known.
    let mut hybrid = primary.clone();
    hybrid.extend(secondary.iter().copied());
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
        skipped_by: None,
        fts: text.ids,
        semantic: hybrid,
        vector,
        ranks,
        answer: semantic.answer,
        order: policy.hybrid_order,
        mixed: true,
    })
}

/// What the rest of retrieval sees when a rule matched: no hits from either
/// half, and the name of the rule to put in the receipt. Not an empty bundle —
/// the coordinate and recency selectors have not run yet and are untouched.
fn skipped(rule: &str, policy: &crate::retrieval::Policy) -> Hits {
    Hits {
        skipped_by: Some(rule.to_owned()),
        fts: HashSet::new(),
        semantic: HashSet::new(),
        vector: HashSet::new(),
        ranks: HashMap::new(),
        answer: None,
        order: policy.hybrid_order,
        mixed: false,
    }
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
    #[cfg(test)]
    super::probe::entered_fts();
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
