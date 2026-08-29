//! Who still sees a withdrawn claim, and who must not.
use super::revoke;
use super::tests::{add, store};
use crate::record::read_all;
use crate::schema::LifecyclePolicy;
use std::fs;

/// An ordinary search answers with what is current. Handing back a claim its
/// author already withdrew — or the tombstone that withdrew it — would make the
/// retraction decorative.
#[test]
fn an_ordinary_search_stops_serving_a_withdrawn_claim() {
    let root = store("search", LifecyclePolicy::default());
    let target = add(&root, "Run the build checks");
    let kept = add(&root, "Rotate credentials often");

    let before = crate::vector::search(
        &root,
        &crate::projection::SearchRequest {
            query: Some("checks credentials".into()),
            namespace: None,
            type_name: None,
            limit: 10,
        },
        crate::vector::SearchStrategy::Fts,
    )
    .expect("search");
    let report = revoke(&root, target, Some("no longer true"), "owner").expect("revoke");
    let after = crate::vector::search(
        &root,
        &crate::projection::SearchRequest {
            query: Some("checks credentials".into()),
            namespace: None,
            type_name: None,
            limit: 10,
        },
        crate::vector::SearchStrategy::Fts,
    )
    .expect("search again");
    let ids = |report: &crate::vector::StrategySearchReport| {
        report
            .hits
            .iter()
            .map(|hit| hit.record.id)
            .collect::<Vec<_>>()
    };

    assert!(ids(&before).contains(&target));
    // Neither the withdrawn claim nor its tombstone answers an ordinary search.
    assert!(!ids(&after).contains(&target));
    assert!(!ids(&after).contains(&report.tombstone));
    assert!(
        ids(&after).contains(&kept),
        "an untouched record still answers"
    );
    // Auditing still works: get reaches the tombstone and its reason.
    let stone = read_all(&root)
        .expect("records")
        .into_iter()
        .find(|record| record.id == report.tombstone)
        .expect("tombstone is still stored");
    assert!(
        stone
            .evidence
            .iter()
            .any(|item| item.reference == "no longer true")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// History has to be excluded before the page is cut. Filtering a limited
/// result set instead returns nothing when the top hit happens to be a record a
/// later one replaced, while a live match waits one row below it.
#[test]
fn a_page_of_one_returns_the_live_match_not_an_empty_page() {
    let root = store("paging", LifecyclePolicy::default());
    let withdrawn = add(&root, "deployment checklist alpha");
    let live = add(&root, "deployment checklist beta");
    revoke(&root, withdrawn, None, "owner").expect("revoke");

    let ask = |limit: u16| {
        crate::vector::search(
            &root,
            &crate::projection::SearchRequest {
                query: Some("deployment".into()),
                namespace: None,
                type_name: None,
                limit,
            },
            crate::vector::SearchStrategy::Fts,
        )
        .expect("search")
        .hits
        .into_iter()
        .map(|hit| hit.record.id)
        .collect::<Vec<_>>()
    };

    assert_eq!(ask(1), vec![live], "one row must be the live one");
    assert_eq!(ask(10), vec![live], "and it is the only one either way");
    // The same holds when the filter alone selects, with no text to rank by.
    let scanned = crate::vector::search(
        &root,
        &crate::projection::SearchRequest {
            query: None,
            namespace: None,
            type_name: None,
            limit: 1,
        },
        crate::vector::SearchStrategy::Fts,
    )
    .expect("scan")
    .hits;
    assert_eq!(scanned.len(), 1);
    assert_eq!(scanned[0].record.id, live);
    fs::remove_dir_all(root).expect("cleanup");
}

/// Five withdrawn records ahead of one live match is exactly the case a guessed
/// overfetch multiple gets wrong. The slack has to be counted, not assumed.
#[test]
fn a_page_of_one_survives_more_history_than_any_fixed_multiple() {
    let root = store("slack", LifecyclePolicy::default());
    for index in 0..6 {
        let doomed = add(&root, &format!("deployment note {index}"));
        revoke(&root, doomed, None, "owner").expect("revoke");
    }
    let live = add(&root, "deployment note that stands");

    let ask = |strategy| {
        crate::vector::search(
            &root,
            &crate::projection::SearchRequest {
                query: Some("deployment".into()),
                namespace: None,
                type_name: None,
                limit: 1,
            },
            strategy,
        )
        .expect("search")
        .hits
        .into_iter()
        .map(|hit| hit.record.id)
        .collect::<Vec<_>>()
    };

    // Twelve history records — six withdrawn claims and six tombstones — sit
    // between the query and the one record that still stands.
    assert_eq!(read_all(&root).expect("records").len(), 13);
    assert_eq!(ask(crate::vector::SearchStrategy::Fts), vec![live]);
    fs::remove_dir_all(root).expect("cleanup");
}

/// Lifecycle is the ledger's answer, not the text projection's. A semantic
/// search must keep excluding history when full text is unavailable — otherwise
/// a healthy vector index would start serving withdrawn claims the moment
/// SQLite went missing.
#[test]
fn the_slack_is_exact_and_does_not_depend_on_the_text_projection() {
    let root = store("independent", LifecyclePolicy::default());
    for index in 0..6 {
        let doomed = add(&root, &format!("deployment note {index}"));
        revoke(&root, doomed, None, "owner").expect("revoke");
    }
    let live = add(&root, "deployment note that stands");
    // Twelve history records: six withdrawn claims and six tombstones.
    assert_eq!(read_all(&root).expect("records").len(), 13);

    // Remove the text projection entirely; the ledger still knows the answer.
    fs::remove_dir_all(root.join("projections")).expect("drop the projection");
    let replaced = read_all(&root)
        .expect("records")
        .iter()
        .filter_map(|record| record.supersedes)
        .collect::<std::collections::HashSet<_>>();
    let current = read_all(&root)
        .expect("records")
        .into_iter()
        .filter(|record| !replaced.contains(&record.id))
        .filter(|record| {
            !record
                .tags
                .iter()
                .any(|tag| tag == crate::record::REVOKED_TAG)
        })
        .map(|record| record.id)
        .collect::<Vec<_>>();

    assert_eq!(replaced.len(), 6, "six claims were replaced");
    assert_eq!(current, vec![live], "one record still stands");
    fs::remove_dir_all(root).expect("cleanup");
}
