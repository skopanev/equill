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
    // And the receipt names what actually answered. A caller citing this as
    // evidence is citing a text search, and has to be able to see that.
    assert_eq!(report.answered_by, "fts");
    assert!(
        report
            .fallback
            .as_deref()
            .is_some_and(|reason| reason.contains("vector")),
        "the stand-in was not reported: {:?}",
        report.fallback
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// `fill_remaining: false`, the setting this case is about.
pub(super) fn fallback_only() -> crate::retrieval::Policy {
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

/// What the report says about which half answered.
///
/// The receipt is the evidence a caller cites. A text answer reported as a
/// hybrid one is not a smaller truth, it is the wrong one — and an empty
/// vector half reported that way has been cited as proof that nothing existed.
mod answered {
    use super::super::super::SearchStrategy;
    use super::super::search::{add, store};
    use super::fallback_only;
    use crate::projection::SearchRequest;
    use crate::record::StoredRecord;
    use crate::retrieval::{Policy, Source};
    use std::fs;
    use std::path::Path;

    type Half =
        Result<(Vec<StoredRecord>, Vec<crate::vector::RejectedHit>), crate::kernel::error::Error>;

    fn nothing(_: &Path, _: &SearchRequest) -> Half {
        Ok((Vec::new(), Vec::new()))
    }

    fn everything(store_root: &Path, _: &SearchRequest) -> Half {
        Ok((crate::record::read_all(store_root)?, Vec::new()))
    }

    fn report(
        root: &Path,
        half: fn(&Path, &SearchRequest) -> Half,
        policy: &Policy,
    ) -> crate::vector::StrategySearchReport {
        let request = SearchRequest {
            query: Some("merging".into()),
            namespace: None,
            type_name: None,
            limit: 10,
        };
        crate::vector::with_semantic_half(half, || {
            crate::vector::search_with_policy(
                root,
                &request,
                SearchStrategy::Hybrid,
                policy,
                &|_| true,
            )
            .expect("hybrid search")
        })
    }

    fn text_first() -> Policy {
        Policy {
            hybrid_order: [Source::Fts, Source::Vector],
            ..fallback_only()
        }
    }

    #[test]
    fn a_text_answer_to_a_hybrid_question_says_text_answered() {
        let root = store("answered-empty-vector");
        add(&root, "Always run the build checks before merging");

        let report = report(&root, nothing, &fallback_only());

        assert_eq!(report.returned_count, 1);
        assert_eq!(report.answered_by, "fts");
        let stood_in = report
            .fallback
            .expect("the text half stood in and the receipt owes that");
        assert!(stood_in.contains("vector"), "{stood_in}");
        assert_eq!(
            report.total_matches, None,
            "a hybrid answer must not be reported as the exhaustive total"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_vector_answer_says_so_and_names_no_stand_in() {
        let root = store("answered-vector");
        add(&root, "Always run the build checks before merging");

        let report = report(&root, everything, &fallback_only());

        assert_eq!(report.returned_count, 1);
        assert_eq!(report.answered_by, "hybrid");
        assert!(
            report.fallback.is_none(),
            "nothing stood in, so nothing may be reported as having done"
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    /// Both halves empty. Nobody stood in, and the receipt says nothing beyond
    /// that — which is honest but still cannot separate "found nothing" from
    /// "could never have found anything". That needs a number this change was
    /// asked not to add.
    #[test]
    fn two_empty_halves_report_no_stand_in() {
        let root = store("answered-both-empty");
        add(&root, "An unrelated note");

        let report = report(&root, nothing, &fallback_only());

        assert_eq!(report.returned_count, 0);
        assert_eq!(report.answered_by, "hybrid");
        assert!(report.fallback.is_none());
        fs::remove_dir_all(root).expect("cleanup");
    }

    /// The mirror image: text is configured first, text finds nothing, and the
    /// vector half stands in for it.
    #[test]
    fn with_text_configured_first_a_vector_answer_is_the_stand_in() {
        let root = store("answered-text-first");
        add(&root, "A note the query cannot reach");

        let report = report(&root, everything, &text_first());

        assert_eq!(report.answered_by, "vector");
        let stood_in = report.fallback.expect("the vector half stood in");
        assert!(stood_in.contains("fts"), "{stood_in}");
        fs::remove_dir_all(root).expect("cleanup");
    }
}
