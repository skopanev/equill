use super::*;
use serde_json::json;
use std::fs;
use uuid::Uuid;

fn store() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("equill-settings-{}", Uuid::now_v7()));
    fs::create_dir_all(&path).expect("store");
    path
}

fn configured() -> serde_json::Value {
    json!({
        "retrieval": {
            "default_budget_records": 30,
            "query_instruction": "Retrieve durable memory directly applicable to the current request.",
            "vector": { "enabled": true, "score_threshold": 0.48 },
            "hybrid": {
                "order": ["vector", "fts"],
                "fill_remaining": true,
                "deduplicate": true
            }
        },
        "telemetry": { "query_log": true }
    })
}

#[test]
fn complete_settings_resolve_the_approved_policy() {
    let root = store();
    fs::write(
        root.join("settings.json"),
        serde_json::to_vec(&configured()).expect("json"),
    )
    .expect("settings");

    let policy = resolve(&root, Overrides::default()).expect("policy");
    assert_eq!(policy.default_budget_records, Some(30));
    assert_eq!(policy.vector_score_threshold, Some(0.48));
    assert_eq!(policy.hybrid_order, [Source::Vector, Source::Fts]);
    assert!(policy.vector_enabled && policy.hybrid_fill_remaining);
    assert!(policy.hybrid_deduplicate);
    assert!(query_log(&root).expect("query log setting"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn absent_settings_use_the_approved_defaults() {
    let root = store();
    let policy = resolve(&root, Overrides::default()).expect("policy");

    assert_eq!(policy.default_budget_records, Some(30));
    assert_eq!(policy.vector_score_threshold, Some(0.48));
    assert_eq!(policy.query_instruction, DEFAULT_QUERY_INSTRUCTION);
    assert!(!query_log(&root).expect("query log default"));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn explicit_values_override_the_store_policy() {
    let root = store();
    fs::write(
        root.join("settings.json"),
        serde_json::to_vec(&configured()).expect("json"),
    )
    .expect("settings");
    let policy = resolve(
        &root,
        Overrides {
            query_instruction: Some("Find the exact runbook".into()),
            vector_enabled: Some(false),
            vector_score_threshold: Some(0.75),
            hybrid_order: Some([Source::Fts, Source::Vector]),
            hybrid_fill_remaining: Some(false),
            hybrid_deduplicate: Some(false),
        },
    )
    .expect("policy");

    assert_eq!(policy.query_instruction, "Find the exact runbook");
    assert!(!policy.vector_enabled);
    assert_eq!(policy.vector_score_threshold, Some(0.75));
    assert_eq!(policy.hybrid_order, [Source::Fts, Source::Vector]);
    assert!(!policy.hybrid_fill_remaining && !policy.hybrid_deduplicate);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn malformed_or_partial_settings_are_refused() {
    let root = store();
    for bad in [
        json!({ "retrieval": { "default_budget_records": 30 } }),
        {
            let mut value = configured();
            value["retrieval"]["vector"]["score_threshold"] = json!(1.5);
            value
        },
        {
            let mut value = configured();
            value["retrieval"]["hybrid"]["order"] = json!(["vector", "vector"]);
            value
        },
        {
            let mut value = configured();
            value["unexpected"] = json!(true);
            value
        },
    ] {
        fs::write(
            root.join("settings.json"),
            serde_json::to_vec(&bad).expect("json"),
        )
        .expect("settings");
        assert!(resolve(&root, Overrides::default()).is_err(), "{bad}");
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn telemetry_can_be_enabled_without_repeating_retrieval_defaults() {
    let root = store();
    fs::write(
        root.join("settings.json"),
        br#"{"telemetry":{"query_log":true}}"#,
    )
    .expect("settings");

    assert!(query_log(&root).expect("query log setting"));
    assert_eq!(
        resolve(&root, Overrides::default())
            .expect("default retrieval")
            .default_budget_records,
        Some(30)
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// A pattern that will not compile is a load failure, not a warning.
///
/// The alternative is a filter that never fires: the cost an operator meant to
/// remove stays exactly where it was, and they have every reason to believe it
/// is gone. Refusing the store is the only outcome they can act on.
#[test]
fn a_pattern_that_will_not_compile_fails_the_load() {
    let root = store();
    let mut settings = configured();
    settings["retrieval"]["skip_query_patterns"] = json!(["^\\[BUS\\]", "("]);
    fs::write(
        root.join("settings.json"),
        serde_json::to_vec(&settings).expect("json"),
    )
    .expect("settings");

    let error =
        resolve(&root, Overrides::default()).expect_err("an unclosed group must be a load failure");
    let message = error.to_string();
    assert!(
        message.contains("skip_query_patterns"),
        "the error does not say which setting is wrong: {message}"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// Settings written before the key existed keep loading, and keep behaving the
/// way they did.
#[test]
fn settings_without_the_key_configure_no_skipping() {
    let root = store();
    fs::write(
        root.join("settings.json"),
        serde_json::to_vec(&configured()).expect("json"),
    )
    .expect("settings");

    let policy = resolve(&root, Overrides::default()).expect("policy");
    assert!(policy.skip_query_patterns.is_empty());
    assert_eq!(policy.skip_query_patterns.matched("[BUS] unread: 3"), None);
    fs::remove_dir_all(root).expect("cleanup");
}

/// The rule handed back is the line the operator wrote, not the compiled
/// regex's own rendering of it: they have to be able to find it by searching
/// their settings file.
#[test]
fn the_matched_rule_is_returned_as_written() {
    let root = store();
    let mut settings = configured();
    settings["retrieval"]["skip_query_patterns"] =
        json!(["^\\[BUS\\] unread:", "^\\[BUS\\] reply required"]);
    fs::write(
        root.join("settings.json"),
        serde_json::to_vec(&settings).expect("json"),
    )
    .expect("settings");

    let policy = resolve(&root, Overrides::default()).expect("policy");
    assert_eq!(
        policy
            .skip_query_patterns
            .matched("[BUS] unread: 3. To read: agentbus drain"),
        Some("^\\[BUS\\] unread:")
    );
    assert_eq!(
        policy
            .skip_query_patterns
            .matched("what did we decide about compaction?"),
        None
    );
    fs::remove_dir_all(root).expect("cleanup");
}
