//! Text as a fallback for the vector half, not as a top-up.
//!
//! The fixtures live here; the cases are next door.
//!
//! The contract the owner asked for: when the vector half answers, the text
//! half adds nothing; when it does not, the text half answers instead. The
//! second clause is the one that was broken — "no top-up" had been read as
//! "stop after the first source", so an empty or unreachable index produced an
//! empty answer where text could have produced a real one.
//!
//! These run through `assemble`, so the decision is exercised where it now
//! lives: after the coordinates, grants, filter and lifecycle have had their
//! say, which is the only point at which "the vector half answered" can be
//! told from "the vector half found things that were all excluded".
mod cases;
mod order;

use super::super::assemble;
use super::fixtures::records::{append, append_scoped};
use super::fixtures::registries::registry;
use super::fixtures::support::{request, store};
use crate::filter::Filter;
use crate::projection::SearchRequest;
use crate::record::StoredRecord;
use crate::vector::{RejectedHit, with_semantic_half};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(super) type Half = Result<(Vec<StoredRecord>, Vec<RejectedHit>), crate::kernel::error::Error>;

/// One record the text index can find by the query word, and one it cannot.
/// Keeping them apart is what makes "which half answered" observable at all.
pub(super) const TEXT_FINDS: &str = "Searchable needle in the ledger";
pub(super) const VECTOR_ONLY: &str = "A thought about compaction";

pub(super) fn seed(name: &str) -> (PathBuf, Uuid, Uuid) {
    let root = store(name);
    registry(&root, 4_000, 1_000, &["hybrid"], "agent.memory");
    let textual = append(&root, TEXT_FINDS, &[], None, "2026-01-01T00:00:00Z");
    // Given a scope so a filter can exclude exactly this one, which is how the
    // "vectors found things, all of them excluded" case is built.
    let semantic = append_scoped(
        &root,
        VECTOR_ONLY,
        &[],
        None,
        "2026-01-02T00:00:00Z",
        Some("private"),
    );
    fallback_only(&root);
    (root, textual, semantic)
}

/// `fill_remaining: false` — the setting under test.
pub(super) fn fallback_only(root: &Path) {
    ordered_fallback(root, ["vector", "fts"]);
}

/// The same, with the preference the store actually configured.
pub(super) fn ordered_fallback(root: &Path, order: [&str; 2]) {
    fs::write(
        root.join("settings.json"),
        serde_json::to_vec(&json!({
            "retrieval": {
                "default_budget_records": 30,
                "query_instruction": "Retrieve durable memory directly applicable to the current request.",
                "vector": { "enabled": true, "score_threshold": 0.48 },
                "hybrid": { "order": order, "fill_remaining": false, "deduplicate": true }
            }
        }))
        .expect("settings json"),
    )
    .expect("settings file");
}

/// A semantic half that returns only the record the text index cannot find.
pub(super) fn vector_answers(store_root: &Path, _request: &SearchRequest) -> Half {
    let records = crate::record::read_all(store_root)?
        .into_iter()
        .filter(|record| record.payload["rule"] == VECTOR_ONLY)
        .collect();
    Ok((records, Vec::new()))
}

pub(super) fn vector_is_empty(_store_root: &Path, _request: &SearchRequest) -> Half {
    Ok((Vec::new(), Vec::new()))
}

pub(super) fn vector_is_unavailable(_store_root: &Path, _request: &SearchRequest) -> Half {
    Err(crate::kernel::error::Error::Projection(
        "index unreachable".into(),
    ))
}

pub(super) fn assembled(
    root: &Path,
    half: fn(&Path, &SearchRequest) -> Half,
    filter: &Filter,
) -> Vec<Uuid> {
    with_semantic_half(half, || {
        assemble(root, "worker.v1", request("needle"), "test-owner", filter)
            .expect("context")
            .selected_record_ids
    })
}
