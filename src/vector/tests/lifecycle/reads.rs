//! What a read costs in records read: nothing.
use super::{populated, request};
use crate::projection;
use crate::record::read_all;
use std::fs;

/// The property the change exists for. Both lifecycle questions together must
/// not read a single record.
#[test]
fn deciding_lifecycle_reads_no_records() {
    let root = populated("no-read");
    let ids = read_all(&root)
        .expect("ledger")
        .iter()
        .map(|record| record.id)
        .collect::<Vec<_>>();
    let mut hits = Vec::new();

    crate::record::hotpath::reset();
    crate::vector::current_only(&root, &mut hits).expect("current only");
    projection::historic(&root, &ids).expect("historic");
    crate::vector::history_slack(&root, &request(10)).expect("slack");
    let after = crate::record::hotpath::touched().ledger_reads;

    assert_eq!(after, 0, "deciding lifecycle read the ledger {after} times");
    fs::remove_dir_all(root).expect("cleanup");
}

/// The whole text-search path, counted end to end.
///
/// Before the watermark and the lifecycle columns this call made four full
/// passes over the ledger: one to read freshness, another inside the same
/// corpus digest, and two more to decide lifecycle. None of them produced a
/// row the caller saw.
#[test]
fn a_text_search_reads_no_records() {
    let root = populated("fts-no-read");

    crate::record::hotpath::reset();
    let report = crate::vector::search(&root, &request(10), crate::vector::SearchStrategy::Fts)
        .expect("search");
    let after = crate::record::hotpath::touched().ledger_reads;

    assert!(!report.hits.is_empty(), "the fixture matched nothing");
    assert_eq!(after, 0, "a text search read the ledger {after} times");
    fs::remove_dir_all(root).expect("cleanup");
}

/// A scope holding more history than the projection will scan used to be
/// refused outright, with a message telling the caller to narrow by namespace
/// or type — a refusal to serve an answer that existed, over a bound the caller
/// never chose. The pool is now sized and served.
#[test]
fn a_pool_past_the_old_scan_cap_is_served_rather_than_refused() {
    let root = populated("no-refusal");

    // 10_000 was the cap the old code compared against and refused past.
    let past_the_cap = crate::vector::history_slack(&root, &request(10_001)).expect("no refusal");
    let saturating = crate::vector::history_slack(&root, &request(u16::MAX)).expect("no refusal");

    assert!(past_the_cap > 10_000);
    // An index request is a u16, so that is where it stops — clamped, not an
    // error, and never a wrapped number smaller than what was asked for.
    assert_eq!(saturating, u16::MAX);
    fs::remove_dir_all(root).expect("cleanup");
}
