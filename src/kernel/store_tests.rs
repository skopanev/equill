//! What a store's metadata is allowed to say.
use super::{StoreConfig, WriteGrant, validate, validate_write_grants};
use serde_json::json;

fn metadata() -> serde_json::Value {
    json!({
        "format_version": 1,
        "root_owner": "owner",
        "namespaces": ["agent.memory"],
        "writers": [],
        "created_at_unix_ms": 1
    })
}

#[test]
fn legacy_metadata_defaults_to_no_scoped_grants() {
    let config: StoreConfig = serde_json::from_value(metadata()).expect("legacy metadata");
    // Metadata written by a newer equill must still open in this one: the
    // grant shape is strict, the envelope around it deliberately is not.
    let mut forward = metadata();
    forward["future_field"] = json!("unknown to this version");

    assert!(config.write_grants.is_empty());
    serde_json::from_value::<StoreConfig>(forward).expect("unknown top-level field");
}

#[test]
fn scoped_grants_reject_unknown_fields() {
    let mut value = metadata();
    value["write_grants"] = json!([{
        "actors": ["agent"],
        "namespace": "agent.memory",
        "types": ["agent.finding.v1"],
        "typo": true
    }]);

    assert!(serde_json::from_value::<StoreConfig>(value).is_err());
}

#[test]
fn scoped_grants_reject_empty_or_control_dimensions() {
    let mut config: StoreConfig = serde_json::from_value(metadata()).expect("metadata");
    config.write_grants = vec![WriteGrant {
        actors: vec!["agent\n".into()],
        namespace: "agent.memory".into(),
        types: vec!["agent.finding.v1".into()],
    }];
    assert!(validate_write_grants(&config).is_err());

    config.write_grants[0].actors = vec!["agent".into()];
    config.write_grants[0].types.clear();
    assert!(validate_write_grants(&config).is_err());
}
/// The command that writes this list already refuses these. A config can be
/// edited by hand, so the store refuses them again when it opens — a
/// written policy it cannot enforce, or cannot undo, must not be loadable.
#[test]
fn a_hold_list_that_could_not_be_enforced_or_undone_is_refused() {
    for (held, why) in [
        (
            json!(["*"]),
            "a wildcard holds everyone, the owner included",
        ),
        (
            json!(["owner"]),
            "holding the owner leaves nobody able to lift it",
        ),
        (
            json!(["pm", "pm"]),
            "a name twice makes the count disagree with itself",
        ),
        (
            json!([" "]),
            "a blank name holds nobody and reads as if it did",
        ),
        (
            json!(["bad\u{7}name"]),
            "a control character is not a stable identity",
        ),
    ] {
        let mut metadata = metadata();
        metadata["read_only"] = held.clone();
        let config: StoreConfig =
            serde_json::from_value(metadata).expect("metadata parses either way");
        assert!(validate(&config).is_err(), "{why}: {held} was accepted");
    }
}

/// The control: an ordinary hold loads, so the rejections above are about
/// what they name and not about the field existing at all.
#[test]
fn an_ordinary_hold_loads() {
    let mut metadata = metadata();
    metadata["read_only"] = json!(["pm", "lane"]);
    let config: StoreConfig = serde_json::from_value(metadata).expect("metadata");
    validate(&config).expect("an ordinary hold is refused");
}
/// The question both surfaces ask before they resume a lagging index.
#[test]
fn a_store_says_which_actors_it_holds_to_reading() {
    let mut metadata = metadata();
    metadata["read_only"] = json!(["pm"]);
    let config: StoreConfig = serde_json::from_value(metadata).expect("metadata");
    assert!(config.read_only.iter().any(|item| item == "pm"));
    assert!(!config.read_only.iter().any(|item| item == "lane"));
}
