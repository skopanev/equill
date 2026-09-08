//! The limit binds every actor and every way in, or it binds nobody.
//!
//! The rule it enforces was written as prose addressed to one role while four
//! others wrote to the same store, and half the stored lessons broke it. A
//! check that reaches only the path its author had in mind would reproduce
//! exactly that.
#[path = "../harness/mod.rs"]
mod harness;
#[path = "support.rs"]
mod support;

use std::fs;
use support::{
    LIMIT, batch, clear_settings, marked, record, run, sentence, set_limit, stderr, store,
};

/// The boundary through the real process: at the limit it is written, one word
/// over it is refused.
#[test]
fn the_limit_is_enforced_at_the_write() {
    let root = store("boundary", true);

    let inside = record(&root, &sentence(LIMIT), "owner");
    assert!(inside.status.success(), "{}", stderr(&inside));

    let over = record(&root, &sentence(LIMIT + 1), "owner");
    assert!(
        !over.status.success(),
        "one word over the limit was written"
    );
    let message = stderr(&over);
    assert!(message.contains("agent.lesson.v1"), "{message}");
    assert!(message.contains("rule"), "{message}");
    assert!(
        message.contains(&(LIMIT + 1).to_string()) && message.contains(&LIMIT.to_string()),
        "the refusal does not give both counts: {message}"
    );
    let _ = fs::remove_dir_all(&root);
}

/// Every actor, not just the one the written rule addressed.
#[test]
fn the_limit_binds_actors_the_written_rule_never_reached() {
    let root = store("actors", true);

    for actor in ["owner", "gate", "panel", "gm", "lane"] {
        let out = record(&root, &sentence(LIMIT + 1), actor);
        assert!(
            !out.status.success(),
            "{actor} wrote over the limit: {}",
            stderr(&out)
        );
    }
    let _ = fs::remove_dir_all(&root);
}

/// The batch path refuses the same record with the same words, because it is
/// the same check and not a second one written to match.
#[test]
fn the_batch_path_refuses_identically() {
    let root = store("batch", true);
    let single = record(&root, &sentence(LIMIT + 1), "owner");
    let many = batch(&root, &[&sentence(1), &sentence(LIMIT + 1)], "owner");

    assert!(!many.status.success(), "batch wrote over the limit");
    // The batch path answers with a receipt rather than a message on stderr,
    // so the comparison is on the reason, which is the part that has to match.
    let reason = stderr(&single);
    let reason = reason
        .trim()
        .trim_start_matches("equill: ")
        .trim_start_matches("invalid record: ");
    // The batch path answers with a receipt; the reason lives inside it.
    let receipt = String::from_utf8_lossy(&many.stdout);
    assert!(
        receipt.contains(reason),
        "the two ways in refuse differently:\n  one: {reason}\n  batch: {receipt}"
    );
    let _ = fs::remove_dir_all(&root);
}

/// A store that never asked for a limit is unchanged, and so is a settings
/// file written before the section existed.
#[test]
fn a_store_without_the_section_behaves_exactly_as_before() {
    let root = store("unlimited", false);
    clear_settings(&root);

    let out = record(&root, &sentence(200), "owner");
    assert!(out.status.success(), "{}", stderr(&out));
    let _ = fs::remove_dir_all(&root);
}

/// What is already written stays readable when the limit arrives, and stays
/// readable when it changes. A policy that reached backwards would make the
/// ledger unreadable the moment somebody edited a setting.
#[test]
fn records_written_before_a_limit_still_read_and_reindex() {
    let root = store("historical", false);
    let long = marked("beforelimit", 50);
    let written = record(&root, &long, "owner");
    assert!(written.status.success(), "{}", stderr(&written));

    set_limit(&root, LIMIT);

    // Rebuilding replays every stored record through validation and rebuilds
    // the projection from the ledger. A write policy that reached backwards
    // would fail here the moment the setting changed, taking the projection
    // with it — so this is the check that the past stays readable, not a
    // search, whose index is built lazily and would answer about timing
    // instead.
    let rebuilt = run(&root, &["rebuild"], "owner");
    assert!(
        rebuilt.status.success(),
        "rebuilding stopped working under a new limit: {}",
        stderr(&rebuilt)
    );

    let read = run(
        &root,
        &["search", "--query", "word", "--format", "jsonl"],
        "owner",
    );
    assert!(read.status.success(), "reading failed: {}", stderr(&read));
    let found = String::from_utf8_lossy(&read.stdout);
    assert!(
        found.contains(&long),
        "a record written before the limit stopped being readable:\n{found}"
    );
    let _ = fs::remove_dir_all(&root);
}

/// A retraction repeats the claim it withdraws, so under a cap the store would
/// refuse to retract exactly the records the cap exists to discourage. The
/// older the store, the more of them there would be.
#[test]
fn a_record_written_before_the_limit_can_still_be_revoked() {
    let root = store("revoke", false);
    let long = marked("historical", 50);
    let written = record(&root, &long, "owner");
    assert!(written.status.success(), "{}", stderr(&written));
    let id = support::written_id(&written);

    set_limit(&root, LIMIT);

    let revoked = run(&root, &["revoke", "--id", &id], "owner");
    assert!(
        revoked.status.success(),
        "a historical record became impossible to retract: {}",
        stderr(&revoked)
    );
    let _ = fs::remove_dir_all(&root);
}

/// The exemption belongs to the retraction path, not to anything a writer can
/// put in a draft. A record carrying the revocation tag and pointing
/// `supersedes` at a target is an ordinary append, and the cap holds.
#[test]
fn a_forged_revocation_through_the_public_path_is_still_capped() {
    let root = store("forged", false);
    let long = marked("historical", 50);
    let written = record(&root, &long, "owner");
    assert!(written.status.success(), "{}", stderr(&written));
    let id = support::written_id(&written);

    set_limit(&root, LIMIT);

    let forged = support::forged_revocation(&root, &id, &sentence(LIMIT + 1), "owner");
    assert!(
        !forged.status.success(),
        "a hand-written record wearing the revocation tag bypassed the limit"
    );
    let _ = fs::remove_dir_all(&root);
}

/// A retraction that changes the claim is not a retraction. Only the payload
/// already stored is exempt; anything else is new writing and is capped.
#[test]
fn an_ordinary_long_replacement_is_still_refused() {
    let root = store("replacement", false);
    let written = record(&root, &marked("historical", 50), "owner");
    let id = support::written_id(&written);

    set_limit(&root, LIMIT);

    let replacement = support::superseding(&root, &id, &sentence(LIMIT + 1), "owner");
    assert!(
        !replacement.status.success(),
        "a long replacement was accepted because it superseded something"
    );
    let _ = fs::remove_dir_all(&root);
}

/// The adapter is a third way in, and a limit that binds two of three binds
/// none of them in practice — the writer that matters is whichever one an
/// agent happens to use.
#[test]
fn the_mcp_adapter_refuses_with_the_same_reason() {
    let root = store("mcp", true);
    let single = record(&root, &sentence(LIMIT + 1), "owner");
    let reason = stderr(&single);
    let reason = reason
        .trim()
        .trim_start_matches("equill: ")
        .trim_start_matches("invalid record: ");

    let mut session = harness::session::Session::open(&root);
    let (_, response) = session.tool(
        "record",
        serde_json::json!({
            "draft": {
                "namespace": "agent.memory",
                "type": "agent.lesson.v1",
                "observed_at": "2026-01-01T00:00:00Z",
                "payload": { "rule": sentence(LIMIT + 1) }
            }
        }),
    );

    let text = response.to_string();
    assert!(
        text.contains(reason),
        "the adapter refuses differently from the command line:\n  cli: {reason}\n  mcp: {text}"
    );
    let _ = fs::remove_dir_all(&root);
}
