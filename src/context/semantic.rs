//! Which records a hybrid selector considers, and what actually answered.
//!
//! Kept apart from the text path on purpose. Text search over the projection is
//! enumerable and repeatable: the same store and the same query give the same
//! set, today and tomorrow. A semantic half does not promise that — an
//! approximate index returns what it has indexed so far — so a profile has to
//! ask for it by name, and the receipt has to say what it got.
use super::model::Selector;
use super::model::{ContextRequest, SemanticAnswer};
use crate::kernel::error::Error;
use crate::projection::SearchRequest;
use crate::vector::{self, SearchStrategy};
use std::collections::HashSet;

/// How many candidates one selector may contribute. The same bound the text
/// path uses: this is a membership question, not a ranking one — the bundle is
/// ordered afterwards by tier, score and coordinate, never by search position.
const PER_SELECTOR: u16 = 100;

/// What a hybrid pass found, and how honestly it can be described.
pub struct SemanticHits {
    pub ids: HashSet<uuid::Uuid>,
    /// What the bundle may claim about its own assembly. `None` when no hybrid
    /// selector ran at all, which is what keeps a text-only receipt the shape
    /// it has always been.
    pub answer: Option<SemanticAnswer>,
}

impl SemanticHits {
    fn empty() -> Self {
        SemanticHits {
            ids: HashSet::new(),
            answer: None,
        }
    }
}

/// Run the hybrid selectors, if any asked for it.
///
/// An empty query is not a search: with nothing to be similar to, the semantic
/// half has no question, and the selector falls through to whatever its other
/// strategies say.
pub fn hits(
    store: &std::path::Path,
    selectors: &[Selector],
    request: &ContextRequest,
) -> Result<SemanticHits, Error> {
    let wanted = selectors
        .iter()
        .filter(|item| item.strategies.contains(&super::model::Strategy::Hybrid))
        .collect::<Vec<_>>();
    if request.query.trim().is_empty() || wanted.is_empty() {
        return Ok(SemanticHits::empty());
    }
    let mut found = SemanticHits::empty();
    for selector in wanted {
        let report = vector::search(
            store,
            &SearchRequest {
                query: Some(request.query.clone()),
                namespace: None,
                type_name: Some(selector.type_name.clone()),
                limit: PER_SELECTOR,
            },
            SearchStrategy::Hybrid,
        )?;
        // The weakest answer wins the label. One selector served by text alone
        // makes the bundle partly text-answered, and saying `hybrid` because
        // another selector managed it would overstate what the caller holds.
        let degraded = report.answered_by == "fts";
        let answer = found.answer.get_or_insert(SemanticAnswer {
            answered_by: report.answered_by.to_owned(),
            fallback: report.fallback.clone(),
            vector_freshness: report.vector_freshness,
            vector_indexed_records: report.vector_indexed_records,
            vector_pending_records: report.vector_pending_records,
        });
        if degraded {
            answer.answered_by = "fts".to_owned();
            answer.fallback = answer.fallback.take().or(report.fallback);
        }
        found
            .ids
            .extend(report.hits.into_iter().map(|hit| hit.record.id));
    }
    Ok(found)
}
