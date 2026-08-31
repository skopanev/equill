//! Two things a profile could not say before: which way a ranked list reads,
//! and how many records the answer depends on.
//!
//! Both are the store's business rather than the engine's. The engine knowing
//! that steps run in ascending order, or that a process must be unique, would
//! be the engine knowing one domain's vocabulary; a profile saying so is the
//! same knowledge kept where it belongs.
use super::super::{ContextRequest, assemble, register_profile, register_selector};
use super::fixtures::{records, support};
use crate::filter::Filter;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

/// A store whose one selector ranks by `/confidence` and expects what it is
/// told to expect.
fn store(name: &str, order: &str, expect: &str) -> PathBuf {
    let root = support::store(name);
    let selector = root.join("selector.json");
    fs::write(
        &selector,
        serde_json::to_vec(&json!({
            "id": "ranked.v1",
            "version": "1",
            "type": "agent.lesson.v1",
            "strategies": ["recency"],
            "rank_pointer": "/confidence",
            "rank_order": order,
            "expect": expect
        }))
        .expect("selector json"),
    )
    .expect("selector file");
    register_selector(&root, &selector, "test-owner").expect("register selector");
    let profile = root.join("profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&json!({
            "id": "ranked",
            "version": "1",
            "actors": [],
            "grants": [{ "namespace": "agent.memory", "types": ["agent.lesson.v1"] }],
            "selectors": ["ranked.v1"],
            "budget": {}
        }))
        .expect("profile json"),
    )
    .expect("profile file");
    register_profile(&root, &profile, "test-owner").expect("register profile");
    root
}

fn ask(root: &Path) -> Result<Vec<f64>, crate::kernel::error::Error> {
    let bundle = assemble(root, "ranked", request(), "test-owner", &Filter::default())?;
    let records = crate::record::read_all(root).expect("records");
    Ok(bundle
        .selected_record_ids
        .iter()
        .filter_map(|id| records.iter().find(|record| record.id == *id))
        .filter_map(|record| {
            record
                .payload
                .pointer("/confidence")
                .and_then(|v| v.as_f64())
        })
        .collect())
}

fn request() -> ContextRequest {
    ContextRequest {
        at: "2026-01-05T00:00:00Z".into(),
        ..support::request("")
    }
}

/// A list of steps is the case where lowest first is the only order that means
/// anything, and the alternative was to encode the order in the payload — which
/// makes the data carry a presentation choice.
#[test]
fn a_selector_can_ask_for_lowest_first() {
    let root = store("asc", "asc", "any");
    for confidence in [0.3, 0.1, 0.2] {
        records::append_ranked(&root, "step", confidence, "2026-01-01T00:00:00Z");
    }
    assert_eq!(ask(&root).expect("ordered"), [0.1, 0.2, 0.3]);
    let _ = fs::remove_dir_all(&root);
}

/// The control: not saying which way still means highest first, so no stored
/// profile changes behaviour because this field was added.
#[test]
fn saying_nothing_still_means_highest_first() {
    let root = support::store("desc-default");
    let selector = root.join("selector.json");
    fs::write(
        &selector,
        serde_json::to_vec(&json!({
            "id": "ranked.v1",
            "version": "1",
            "type": "agent.lesson.v1",
            "strategies": ["recency"],
            "rank_pointer": "/confidence"
        }))
        .expect("selector json"),
    )
    .expect("selector file");
    register_selector(&root, &selector, "test-owner").expect("register selector");
    let profile = root.join("profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&json!({
            "id": "ranked", "version": "1", "actors": [],
            "grants": [{ "namespace": "agent.memory", "types": ["agent.lesson.v1"] }],
            "selectors": ["ranked.v1"], "budget": {}
        }))
        .expect("profile json"),
    )
    .expect("profile file");
    register_profile(&root, &profile, "test-owner").expect("register profile");
    for confidence in [0.3, 0.1, 0.2] {
        records::append_ranked(&root, "step", confidence, "2026-01-01T00:00:00Z");
    }
    assert_eq!(ask(&root).expect("ordered"), [0.3, 0.2, 0.1]);
    let _ = fs::remove_dir_all(&root);
}

/// One record, or no answer. Nothing and two are the same failure seen from
/// either side: neither is a selection the caller could act on.
#[test]
fn expecting_exactly_one_refuses_both_none_and_several() {
    let empty = store("one-none", "desc", "one");
    let refused = ask(&empty).expect_err("nothing matched");
    let said = refused.to_string();
    assert!(
        said.contains("ranked.v1") && said.contains("agent.lesson.v1") && said.contains("found 0"),
        "the refusal does not say which selector expected what: {said}"
    );

    let several = store("one-several", "desc", "one");
    for confidence in [0.1, 0.2] {
        records::append_ranked(&several, "step", confidence, "2026-01-01T00:00:00Z");
    }
    let refused = ask(&several).expect_err("two matched");
    assert!(
        refused.to_string().contains("found 2"),
        "the refusal does not say how many it found: {refused}"
    );
    let _ = fs::remove_dir_all(&empty);
    let _ = fs::remove_dir_all(&several);
}

/// At least one is the weaker claim, and it has to stay weaker: two records
/// satisfy it.
#[test]
fn expecting_some_refuses_nothing_and_accepts_more_than_one() {
    let empty = store("some-none", "desc", "some");
    assert!(ask(&empty).is_err(), "an empty answer satisfied `some`");
    let several = store("some-several", "desc", "some");
    for confidence in [0.1, 0.2] {
        records::append_ranked(&several, "step", confidence, "2026-01-01T00:00:00Z");
    }
    assert_eq!(ask(&several).expect("two are enough").len(), 2);
    let _ = fs::remove_dir_all(&empty);
    let _ = fs::remove_dir_all(&several);
}

/// The compatibility control. Every profile written before this field existed
/// means `any`, and `any` has to keep meaning "including nothing".
#[test]
fn the_default_expectation_still_allows_an_empty_answer() {
    let root = store("any-none", "desc", "any");
    assert!(ask(&root).expect("no refusal").is_empty());
    let _ = fs::remove_dir_all(&root);
}
