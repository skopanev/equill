//! Which records a hybrid selector considers, and what actually answered.
//!
//! Kept apart from the text path on purpose. Text search over the projection is
//! enumerable and repeatable: the same store and the same query give the same
//! set, today and tomorrow. A semantic half does not promise that — an
//! approximate index returns what it has indexed so far — so a profile has to
//! ask for it by name, and the receipt has to say what it got.
use super::model::ContextRequest;
use super::model::Selector;
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
    /// What answered: `hybrid` when both halves ran, `fts` when the index could
    /// not and text stood in. Recorded rather than inferred, because a caller
    /// comparing two bundles needs to know which of them saw semantics at all.
    pub answered_by: Option<&'static str>,
    /// Present when the semantic half was unavailable and text answered for it.
    pub fallback: Option<String>,
}

impl SemanticHits {
    fn empty() -> Self {
        SemanticHits {
            ids: HashSet::new(),
            answered_by: None,
            fallback: None,
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
        if report.answered_by == "fts" {
            found.answered_by = Some("fts");
            found.fallback = found.fallback.or(report.fallback);
        } else if found.answered_by.is_none() {
            found.answered_by = Some("hybrid");
        }
        found
            .ids
            .extend(report.hits.into_iter().map(|hit| hit.record.id));
    }
    Ok(found)
}
