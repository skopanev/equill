//! An actor the store lists as read-only cannot append, whatever else allows it.
//!
//! Every other rule in the store grants: `writers` says who may append, and a
//! `*` there says everyone. A store that has opened itself with a wildcard has
//! no way to take one actor back out — removing a name from a list that holds
//! no names does nothing, and revoking the wildcard takes access from everybody
//! at once. So the refusal is written down separately and checked first.
//!
//! The fixture opens the store with that wildcard on purpose. A test where the
//! actor simply was not a writer would pass without the refusal existing.
mod harness;
mod readonly;

use readonly::{READER, existing_record, readback, run, state, store, write};
use serde_json::json;
use std::fs;

#[test]
fn a_read_only_actor_cannot_append_by_any_route() {
    let root = store();
    let target = existing_record(&root);
    let (before_ledger, before_index) = state(&root);
    let before_answers = readback(&root, &target);

    let draft = write(
        &root,
        "draft.json",
        json!({
            "namespace": "agent.memory",
            "type": "agent.lesson.v1",
            "observed_at": "2026-01-01T00:00:00Z",
            "payload": { "rule": "a lesson the reader may not write" }
        }),
    );
    // A complete legacy envelope. An incomplete one is refused by the parser
    // before the write boundary is reached, which would make this test pass
    // without the boundary refusing anything.
    let legacy = root.join("import.jsonl");
    fs::write(
        &legacy,
        format!(
            "{}\n",
            json!({
                "id": "legacy-1",
                "ts": "2026-01-01T00:00:00Z",
                "namespace": "agent.memory",
                "type": "agent.lesson.v1",
                "actor": "legacy-writer",
                "observed_at": "2026-01-01T00:00:00Z",
                "payload": { "rule": "a lesson the reader may not import" }
            })
        ),
    )
    .expect("import file");

    for (route, args) in [
        (
            "record",
            vec!["record", "--input", draft.to_str().expect("path")],
        ),
        (
            "import",
            vec!["import", "--input", legacy.to_str().expect("path")],
        ),
        ("revoke", vec!["revoke", "--id", &target, "--comment", "no"]),
    ] {
        let out = run(&root, READER, &args);
        assert!(
            !out.status.success(),
            "{route} succeeded for a read-only actor"
        );
        // The exact contract, not a paraphrase: a caller keys on the token,
        // and the escalation path is what the refused actor needs to read.
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(
            said.contains("PM_WRITE_DENIED"),
            "{route} refused without the stable token: {said}"
        );
        assert!(
            said.contains("Escalate to GM."),
            "{route} refused without saying where to go: {said}"
        );
        assert!(said.contains(READER), "{route} refused without naming who");
    }

    // Nothing moved. Not "no new records" — the same bytes, because a refusal
    // that still touched the ledger would be a different kind of failure. And
    // the whole projection tree, because catching an index up writes three
    // files and hashing one of them would miss the other two.
    let (after_ledger, after_index) = state(&root);
    assert_eq!(before_ledger, after_ledger, "the ledger changed");
    assert_eq!(before_index, after_index, "the projection tree changed");
    // The answers too: a store that reads differently afterwards changed,
    // whatever its bytes say.
    assert_eq!(
        before_answers,
        readback(&root, &target),
        "the store answers differently after a refused write"
    );
    let _ = fs::remove_dir_all(&root);
}

/// Reading is untouched, which is the point of the word.
#[test]
fn a_read_only_actor_can_still_read() {
    let root = store();
    let target = existing_record(&root);

    for (route, args) in [
        ("get", vec!["get", "--id", &target]),
        ("search", vec!["search", "--query", "lesson"]),
        ("context", vec!["context", "--profile", "reading", "--json"]),
    ] {
        let out = run(&root, READER, &args);
        assert!(
            out.status.success(),
            "{route} failed for a read-only actor: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let _ = fs::remove_dir_all(&root);
}

/// The control: the refusal names one actor and does not close the store.
#[test]
fn other_actors_still_write_through_the_same_wildcard() {
    let root = store();
    let (before, _) = state(&root);
    let draft = write(
        &root,
        "other.json",
        json!({
            "namespace": "agent.memory",
            "type": "agent.lesson.v1",
            "observed_at": "2026-01-01T00:00:00Z",
            "payload": { "rule": "written by somebody who may" }
        }),
    );
    let out = run(
        &root,
        "lane",
        &["record", "--input", draft.to_str().expect("path")],
    );
    assert!(
        out.status.success(),
        "the wildcard stopped granting: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_ne!(before, state(&root).0, "the accepted write did not land");
    let _ = fs::remove_dir_all(&root);
}

/// Governance is the thing that lifts the restriction, so an actor held to
/// reading must not reach it — otherwise it could let itself write.
#[test]
fn a_read_only_actor_cannot_govern() {
    let root = store();
    let out = run(&root, READER, &["reader", "revoke", "--actor", READER]);
    assert!(
        !out.status.success(),
        "a read-only actor lifted its own restriction"
    );
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        said.contains("PM_WRITE_DENIED"),
        "governance refused without the stable token: {said}"
    );

    // The control on the same command: the owner still governs, so the refusal
    // names one actor rather than closing the door.
    let out = run(
        &root,
        "owner",
        &["reader", "add", "--actor", "someone-else"],
    );
    assert!(
        out.status.success(),
        "the owner lost governance: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = fs::remove_dir_all(&root);
}
