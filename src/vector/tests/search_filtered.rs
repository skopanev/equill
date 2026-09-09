//! What a caller's filter does to the choice between the two halves.
use super::super::SearchStrategy;
use super::search::{add, store};
use crate::projection::SearchRequest;
use std::fs;

/// The surface case: vectors answer, the caller's `--where` excludes every one
/// of them, and text could have answered.
///
/// Both surfaces reach this through `search_with_policy` and hand it their
/// filter, so this exercises the shared point rather than either binary. What
/// it does not exercise is the process boundary itself — a non-empty vector
/// half needs a provider, and there is none in a test — so what is proven here
/// is that the filter narrows before the merge, not that each binary spells its
/// own call correctly. That part is one line in each and is read, not measured.
#[test]
fn a_filter_that_empties_the_vector_half_still_leaves_the_text_answer() {
    let root = store("filtered-vector-fallback");
    let private = add(&root, "Private note about merging");
    add(&root, "Always run the build checks before merging");
    let request = SearchRequest {
        query: Some("merging".into()),
        namespace: None,
        type_name: None,
        limit: 10,
    };
    let excluding =
        crate::filter::Filter::parse(&["rule=!Private note about merging".into()], false)
            .expect("filter");

    let report = crate::vector::with_semantic_half(
        |store_root, _| {
            let found = crate::record::read_all(store_root)?
                .into_iter()
                .filter(|record| record.payload["rule"] == "Private note about merging")
                .collect();
            Ok((found, Vec::new()))
        },
        || {
            crate::vector::search_with_policy(
                &root,
                &request,
                SearchStrategy::Hybrid,
                &fallback_only(),
                &|record| crate::filter::matches(record, &excluding),
            )
            .expect("hybrid search")
        },
    );

    let ids = report
        .hits
        .iter()
        .map(|hit| hit.record.id)
        .collect::<Vec<_>>();
    assert!(
        !ids.contains(&private),
        "the excluded record was returned anyway"
    );
    assert_eq!(
        ids.len(),
        1,
        "every vector hit was excluded and the text half was suppressed with it"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// `fill_remaining: false`, the setting this case is about.
fn fallback_only() -> crate::retrieval::Policy {
    crate::retrieval::Policy {
        default_budget_records: Some(30),
        query_instruction: crate::retrieval::DEFAULT_QUERY_INSTRUCTION.into(),
        vector_enabled: true,
        vector_score_threshold: Some(0.48),
        hybrid_order: [
            crate::retrieval::Source::Vector,
            crate::retrieval::Source::Fts,
        ],
        hybrid_fill_remaining: false,
        hybrid_deduplicate: true,
        skip_query_patterns: Default::default(),
    }
}
