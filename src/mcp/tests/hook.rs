//! Context for an editor lifecycle hook: the envelope, and nothing else.
use super::support::*;
use crate::record::{RecordDraft, append_indexed};
use serde_json::{Value, json};
use std::fs;

/// The hook answers for the event it was asked about, and only in that shape.
///
/// Both editors are supported by carrying the caller's own event name back
/// out. Substituting one editor's event for another's is the failure this
/// guards: a harness that gets the wrong `hookEventName` ignores the result.
#[test]
fn a_hook_answers_in_the_envelope_of_the_event_it_was_given() {
    let root = store();
    seed(&root, "Run the checks before merging.");

    for event in ["UserPromptSubmit", "PostToolBatch", "PostToolUse"] {
        let response = one(
            &root,
            json!({ "hook_event_name": event, "profile": "hook.v1", "query": "checks before merging" }),
        );
        let output = &response["result"]["structuredContent"]["hookSpecificOutput"];

        assert_eq!(output["hookEventName"], event, "{event}: {response}");
        assert!(
            output["additionalContext"]
                .as_str()
                .expect("additionalContext")
                .contains("Run the checks"),
            "{event}: context did not carry the seeded record"
        );
        assert!(
            response["result"]["structuredContent"]
                .as_object()
                .expect("one key")
                .len()
                == 1,
            "{event}: the hook returned more than hookSpecificOutput"
        );
    }
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn an_unknown_hook_event_is_refused_by_name() {
    let root = store();
    let response = one(
        &root,
        json!({ "hook_event_name": "PreCompact", "query": "anything" }),
    );

    let message = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
        + response["error"]["message"].as_str().unwrap_or_default();
    assert!(
        message.contains("PreCompact"),
        "the refusal does not name what was sent: {response}"
    );
    fs::remove_dir_all(root).expect("remove store");
}

/// The hook is an adapter, not a second retrieval path.
///
/// This is the test that keeps it thin: the same store and the same arguments
/// asked through `context` and through `hook_context` must produce the same
/// text, byte for byte. Any policy the hook decided for itself — its own skip
/// rules, its own cap, its own way of composing a question — would show up
/// here as a difference, and the only thing the hook is allowed to add is the
/// envelope around it.
#[test]
fn the_envelope_carries_exactly_what_the_context_tool_would_have_answered() {
    let root = store();
    seed(&root, "Run the checks before merging.");
    let arguments = json!({ "profile": "hook.v1", "query": "checks before merging" });

    let mut hooked = arguments.clone();
    hooked["hook_event_name"] = json!("UserPromptSubmit");
    let replies = exchange(
        &root,
        "owner",
        &[
            call("context", arguments, 1),
            call("hook_context", hooked, 2),
        ],
    );

    let plain = structured(&replies[0])["content"]
        .as_str()
        .expect("context content");
    let wrapped = structured(&replies[1])["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect("additionalContext");
    assert!(!plain.is_empty(), "the plain context answered nothing");
    assert_eq!(wrapped, plain);
    fs::remove_dir_all(root).expect("remove store");
}

/// A profile the hook can name by default, and one selector that answers it.
/// Minimal on purpose: the hook is being tested, not the retrieval policy.
fn profile(root: &std::path::Path) {
    let selector = root.join("hook-selector.json");
    fs::write(
        &selector,
        serde_json::to_vec(&json!({
            "id": "agent.lesson.hook.v1",
            "version": "1",
            "type": "agent.lesson.v1",
            "strategies": ["fts", "recency"],
            "expect": "any"
        }))
        .expect("selector json"),
    )
    .expect("selector file");
    crate::context::register_selector(root, &selector, "owner").expect("register selector");
    let profile = root.join("hook-profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&json!({
            "id": "hook.v1",
            "version": "1",
            "actors": [],
            "grants": [{ "namespace": "agent.memory", "types": ["agent.lesson.v1"] }],
            "selectors": ["agent.lesson.hook.v1"],
            "budget": {
                "total_tokens": 4_000,
                "required_cap_tokens": 1_000,
                "core_cap_tokens": 2_000,
                "relevant_floor_tokens": 500,
                "receipt_reserve_tokens": 20,
                "tokenizer": { "id": "o200k_base", "version": "tiktoken-rs-0.12.0" }
            }
        }))
        .expect("profile json"),
    )
    .expect("profile file");
    crate::context::register_profile(root, &profile, "owner").expect("register profile");
}

fn seed(root: &std::path::Path, rule: &str) {
    profile(root);
    append_indexed(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": rule }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: None,
        },
        "owner",
    )
    .expect("seed");
}

/// One `hook_context` call, with the query log off.
fn one(root: &std::path::Path, arguments: Value) -> Value {
    exchange(root, "owner", &[call("hook_context", arguments, 1)])
        .pop()
        .expect("one reply")
}
