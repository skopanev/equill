//! What the revocation path is, and is not, allowed to wave past the store's
//! write policy.
use super::tests::{add, store};
use crate::record::read_all;
use crate::schema::LifecyclePolicy;
use serde_json::json;
use std::fs;

/// The exemption belongs to the payload already stored, not to the path.
///
/// `revoke` copies the target's payload, so from outside the store this can
/// never differ — which means the guard is invisible to every test that goes
/// through a command, and a later caller could hand this path a rewritten
/// claim and have it accepted. Asserted here, where the path is reachable,
/// because a guard nothing exercises is a guard that quietly stops working.
#[test]
fn the_revocation_path_does_not_exempt_a_rewritten_claim() {
    let root = store("rewritten", LifecyclePolicy::default());
    let long = vec!["word"; 30].join(" ");
    let id = add(&root, &long);
    let second = add(&root, &long);
    let target = read_all(&root)
        .expect("records")
        .into_iter()
        .find(|record| record.id == id)
        .expect("target");
    fs::write(
        root.join("settings.json"),
        json!({ "records": { "agent.lesson.v1": { "rule": { "max_words": 5 } } } }).to_string(),
    )
    .expect("settings");

    // The claim as stored: exempt, because retracting it is not new writing.
    let faithful = crate::record::writer::append_revocation(
        &root,
        super::tombstone(&target, None),
        "owner",
        &target,
    );
    assert!(faithful.is_ok(), "a faithful retraction was refused");

    // The same call with the claim rewritten, against a target that has not
    // been revoked yet — otherwise a stale-head refusal would satisfy the
    // assertion and the cap would never be exercised at all.
    let fresh = read_all(&root)
        .expect("records")
        .into_iter()
        .find(|record| record.id == second)
        .expect("second target");
    let mut rewritten = super::tombstone(&fresh, None);
    rewritten.payload = json!({ "rule": vec!["other"; 30].join(" ") });
    let refused = crate::record::writer::append_revocation(&root, rewritten, "owner", &fresh)
        .expect_err("a rewritten claim was exempted on the revocation path");
    let message = refused.to_string();
    assert!(
        message.contains("30 words") && message.contains("limit is 5"),
        "refused for the wrong reason, so the cap was never reached: {message}"
    );
    let _ = fs::remove_dir_all(&root);
}

/// A revocation owes the projection the same nudge every other append gives it.
///
/// Withdrawing a claim changes what a search should return, so a retraction
/// that skipped the signal would leave the withdrawn record sitting in the
/// index until something unrelated happened to wake the worker — visible to
/// readers, and gone from the answer only by luck of timing.
#[test]
fn a_revocation_signals_the_projection_like_any_other_append() {
    let root = store("signal", LifecyclePolicy::default());
    let id = add(&root, "a short rule");
    let target = read_all(&root)
        .expect("records")
        .into_iter()
        .find(|record| record.id == id)
        .expect("target");

    let ordinary = crate::record::append(
        &root,
        crate::record::RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": "another short rule" }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: None,
        },
        "owner",
    )
    .expect("ordinary append");

    let revoked = crate::record::writer::append_revocation(
        &root,
        super::tombstone(&target, None),
        "owner",
        &target,
    )
    .expect("revocation");

    assert_eq!(
        revoked.vector.spawned, ordinary.vector.spawned,
        "a revocation handed the projection a different signal than an append"
    );
    assert_eq!(
        serde_json::to_value(revoked.vector.projection).expect("projection"),
        serde_json::to_value(ordinary.vector.projection).expect("projection"),
        "the two paths report different projection state"
    );
    let _ = fs::remove_dir_all(&root);
}
