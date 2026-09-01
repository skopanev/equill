//! What the store refuses to be talked into, and what it says it holds.
use crate::harness::session::Session;
use crate::{READER, plain_store, run, state, store};
use std::fs;

/// Two ways to end governance, both refused where they are written.
#[test]
fn the_store_refuses_the_two_moves_it_could_not_undo() {
    let root = plain_store();

    // On a store with no grants: one carrying a wildcard grant refuses an
    // ownership handover for a reason of its own, and this test would pass
    // while the door it names stood open.

    // `*` is a valid identity everywhere it grants. Here it would read as
    // "hold everyone to reading", the owner included, and nothing could lift it.
    let out = run(&root, "owner", &["reader", "add", "--actor", "*"]);
    assert!(!out.status.success(), "the wildcard was accepted");

    // Handing the store to an actor it holds to reading would leave a store
    // whose owner cannot govern it and whose restriction nobody can lift. The
    // hold has to exist first — a plain store has nothing to refuse, and the
    // refusal would look like it fired when it had nothing to fire at.
    let out = run(&root, "owner", &["reader", "add", "--actor", READER]);
    assert!(out.status.success(), "reader add failed");
    let out = run(&root, "owner", &["owner", "transfer", "--to", READER]);
    assert!(
        !out.status.success(),
        "the store was handed to an actor it holds to reading"
    );

    // The controls: both commands still work for what they are for.
    let out = run(&root, "owner", &["reader", "add", "--actor", "another"]);
    assert!(
        out.status.success(),
        "reader add stopped working: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = run(&root, "owner", &["owner", "transfer", "--to", "lane"]);
    assert!(
        out.status.success(),
        "owner transfer stopped working: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = fs::remove_dir_all(&root);
}

/// A list that does not name them is a list that hides them.
#[test]
fn the_authority_listing_names_who_is_held_to_reading() {
    let root = store();
    let out = run(&root, "owner", &["reader", "add", "--actor", "another"]);
    assert!(out.status.success(), "reader add failed");

    let printed =
        String::from_utf8_lossy(&run(&root, "owner", &["reader", "list"]).stdout).into_owned();
    assert!(
        printed.contains(READER) && printed.contains("another"),
        "the listing omits actors held to reading: {printed}"
    );
    // Sorted, so two stores configured the same way report the same way.
    let body: serde_json::Value = serde_json::from_str(&String::from_utf8_lossy(
        &run(&root, "owner", &["reader", "list", "--json"]).stdout,
    ))
    .expect("json");
    let held: Vec<&str> = body["read_only"]
        .as_array()
        .expect("read_only")
        .iter()
        .filter_map(|item| item.as_str())
        .collect();
    assert_eq!(held, ["another", READER], "the listing is not sorted");
    let _ = fs::remove_dir_all(&root);
}

/// A store that already ran governance under the old vocabulary keeps running.
///
/// The registered audit definition is compared byte for byte, so widening it in
/// place would stop every such store from governing at all — including with the
/// command that would put it right. The old definition is left exactly as it
/// was and a second one is registered beside it.
#[test]
fn a_store_that_governed_under_the_old_vocabulary_still_governs() {
    let root = plain_store();
    let types = root.join("registry/types");
    let v1 = types.join("equill.governance.v1.json");

    // What an older build left behind: the type registered with the three
    // actions it knew about.
    fs::create_dir_all(&types).expect("types");
    fs::write(
        &v1,
        serde_json::to_vec(&serde_json::json!({
            "type": "equill.governance.v1",
            "uri": "equill://equill.governance/v1",
            "owner": "owner",
            "payload_schema": {
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": {
                    "action": { "enum": ["grant-add", "grant-revoke", "owner-transfer"] },
                    "subject": { "type": "string", "minLength": 1 },
                    "tx_id": { "type": "string", "minLength": 1 },
                    "store_sha256_before": { "type": "string", "pattern": "^[0-9a-f]{64}$" },
                    "store_sha256_after": { "type": "string", "pattern": "^[0-9a-f]{64}$" },
                    "comment_sha256": { "type": "string", "pattern": "^[0-9a-f]{64}$" }
                },
                "required": [
                    "action", "subject", "tx_id", "store_sha256_before", "store_sha256_after"
                ],
                "additionalProperties": false
            }
        }))
        .expect("json"),
    )
    .expect("v1 definition");
    let before = fs::read(&v1).expect("v1");

    // Both the old operations and the new one, because the failure this closes
    // took out every one of them at once.
    for args in [
        vec![
            "grant",
            "add",
            "--actor",
            "probe",
            "--namespace",
            "*",
            "--types",
            "*",
        ],
        vec!["reader", "add", "--actor", READER],
    ] {
        let out = run(&root, "owner", &args);
        assert!(
            out.status.success(),
            "{args:?} failed on a store carrying the old definition: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    assert_eq!(
        before,
        fs::read(&v1).expect("v1"),
        "the old definition was rewritten"
    );
    assert!(
        types.join("equill.governance.v2.json").is_file(),
        "the new definition was never registered"
    );
    let _ = fs::remove_dir_all(&root);
}

/// The other door into the store answers the same way.
///
/// A session is where a long-lived agent lives, and it resumes a lagging index
/// on every call. For a call about to be refused that resume is a write the
/// refusal did not prevent — the same fault as on the command line, reached
/// through a different door.
#[test]
fn a_session_refuses_the_held_actor_and_changes_nothing() {
    let root = store();
    // Behind, and configured, so there is a catch-up for the guard to prevent.
    crate::lagging::prepare(&root);
    let before = state(&root);

    let mut session = Session::open_as(&root, READER);
    let (_, response) = session.tool(
        "record",
        serde_json::json!({ "draft": {
            "namespace": "agent.memory",
            "type": "agent.lesson.v1",
            "observed_at": "2026-01-01T00:00:00Z",
            "payload": { "rule": "a lesson the session may not write" }
        }}),
    );
    let said = response.to_string();
    assert!(
        said.contains("PM_WRITE_DENIED") && said.contains("Escalate to GM."),
        "the session refused without the contract: {said}"
    );

    assert!(
        !crate::lagging::starts(&root),
        "the refused call started a catch-up on its way to being refused"
    );
    drop(session);
    assert_eq!(before, state(&root), "the refused call changed the store");

    // The control: the same store, in the same state, an actor that may.
    crate::lagging::unmute(&root);
    let mut session = Session::open_as(&root, "lane");
    let (_, response) = session.tool(
        "record",
        serde_json::json!({ "draft": {
            "namespace": "agent.memory",
            "type": "agent.lesson.v1",
            "observed_at": "2026-01-01T00:00:00Z",
            "payload": { "rule": "a lesson the session may write" }
        }}),
    );
    assert!(
        !response.to_string().contains("PM_WRITE_DENIED"),
        "the control was refused: {response}"
    );
    drop(session);
    assert!(
        crate::lagging::starts(&root),
        "nothing resumes for anybody here, so the refusal proved nothing"
    );
    let _ = fs::remove_dir_all(&root);
}
