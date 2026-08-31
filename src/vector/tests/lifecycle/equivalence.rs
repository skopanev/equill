//! The projection is only allowed to be faster than the ledger walk it
//! replaced, never different.
use super::{history_by_ledger, populated};
use crate::projection::{self, LifecycleScope, SearchHit, SearchRequest};
use crate::record::read_all;
use std::collections::HashSet;
use std::fs;

/// The projection and the ledger must name exactly the same records as history,
/// over every scope and for every hit set.
#[test]
fn the_projection_names_the_same_history_the_ledger_does() {
    let root = populated("agree");
    let all = read_all(&root).expect("ledger");
    let expected = history_by_ledger(&root);

    let ids = all.iter().map(|record| record.id).collect::<Vec<_>>();
    let projected = projection::historic(&root, &ids).expect("historic").history;

    assert_eq!(projected, expected);
    // And through the read path itself: what survives filtering is what the
    // ledger would have left standing.
    let mut hits = all
        .iter()
        .cloned()
        .map(|record| SearchHit { record })
        .collect::<Vec<_>>();
    crate::vector::current_only(&root, &mut hits).expect("current only");
    let surviving = hits.iter().map(|hit| hit.record.id).collect::<HashSet<_>>();
    let by_ledger = all
        .iter()
        .map(|record| record.id)
        .filter(|id| !expected.contains(id))
        .collect::<HashSet<_>>();
    assert_eq!(surviving, by_ledger);
    assert!(!surviving.is_empty(), "the fixture left nothing current");
    fs::remove_dir_all(root).expect("cleanup");
}

/// The count a page sizes its overfetch from, scoped the way a request scopes
/// it, compared against the same count taken from the ledger.
#[test]
fn history_in_scope_matches_the_ledger_scope_for_scope() {
    let root = populated("scope");
    let all = read_all(&root).expect("ledger");
    let history = history_by_ledger(&root);
    let scopes = [
        (None, None),
        (Some("agent.memory"), None),
        (Some("agent.memory"), Some("agent.lesson.v1")),
        (Some("agent.memory"), Some("agent.note.v1")),
        (Some("no.such.namespace"), None),
    ];

    for (namespace, type_name) in scopes {
        let expected = all
            .iter()
            .filter(|record| namespace.is_none_or(|value| record.namespace == value))
            .filter(|record| type_name.is_none_or(|value| record.type_name == value))
            .filter(|record| history.contains(&record.id))
            .count();
        let counted = projection::history_in_scope(
            &root,
            &LifecycleScope {
                namespace: namespace.map(str::to_owned),
                type_name: type_name.map(str::to_owned),
            },
        )
        .expect("history in scope")
        .history;
        assert_eq!(counted, expected, "scope {namespace:?}/{type_name:?}");
    }
    fs::remove_dir_all(root).expect("cleanup");
}

/// Text search excludes the same history it always did, including the legacy
/// tag, now that the query reads columns rather than matching tag text.
#[test]
fn text_search_still_excludes_every_shape_of_history() {
    let root = populated("fts");
    let expected = history_by_ledger(&root);

    let hits = projection::search(
        &root,
        &SearchRequest {
            query: Some("rule note".into()),
            namespace: None,
            type_name: None,
            limit: 50,
        },
    )
    .expect("search")
    .hits;

    assert!(!hits.is_empty(), "the fixture matched nothing");
    for hit in &hits {
        assert!(
            !expected.contains(&hit.record.id),
            "search returned history: {}",
            hit.record.id
        );
    }
    fs::remove_dir_all(root).expect("cleanup");
}

/// A rebuild sees the ledger in whatever order it holds, so a record can be
/// indexed after the one that replaced it. The flags must come out the same.
#[test]
fn a_rebuild_reproduces_lifecycle_whatever_the_order() {
    let root = populated("rebuild");
    let expected = history_by_ledger(&root);
    let ids = read_all(&root)
        .expect("ledger")
        .iter()
        .map(|record| record.id)
        .collect::<Vec<_>>();

    projection::rebuild(&root).expect("rebuild");

    let projected = projection::historic(&root, &ids).expect("historic").history;
    assert_eq!(projected, expected);
    fs::remove_dir_all(root).expect("cleanup");
}
