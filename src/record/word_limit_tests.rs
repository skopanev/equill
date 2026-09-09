//! What counts as a word, and what the store does with the count.
use super::word_limit::{FieldLimit, Limits, check, count, validate};
use serde_json::json;

fn limits(max_words: usize) -> Limits {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("rule".to_string(), FieldLimit { max_words });
    let mut limits = Limits::new();
    limits.insert("agent.lesson.v2".to_string(), fields);
    limits
}

/// The boundary as a property, on one sentence grown by one word: N is
/// accepted and N+1 is refused. Two separate examples could pass while the
/// comparison is off by one in a way neither of them reaches.
#[test]
fn a_sentence_of_the_limit_is_accepted_and_one_word_more_is_not() {
    let limits = limits(20);
    for words in 1..=20 {
        let sentence = vec!["word"; words].join(" ");
        assert!(
            check(&limits, "agent.lesson.v2", &json!({ "rule": sentence })).is_ok(),
            "{words} words were refused under a limit of 20"
        );
    }
    let over = vec!["word"; 21].join(" ");
    assert!(
        check(&limits, "agent.lesson.v2", &json!({ "rule": over })).is_err(),
        "21 words were accepted under a limit of 20"
    );
}

/// Whitespace is a separator, not a word. Tabs, newlines and runs of spaces
/// change how a sentence looks and not what it counts.
#[test]
fn spacing_does_not_change_the_count() {
    assert_eq!(count("one two three"), 3);
    assert_eq!(count("  one\ttwo\n\nthree  "), 3);
    assert_eq!(count("one\u{a0}two"), 2);
    assert_eq!(count(""), 0);
    assert_eq!(count("   \t\n "), 0);
}

/// A hyphen inside a word does not split it: a writer counting their own
/// sentence counts "well-measured" once, and a limit that disagrees with the
/// person it constrains is a limit they cannot work to.
#[test]
fn an_internal_hyphen_does_not_split_a_word() {
    assert_eq!(count("well-measured claim"), 2);
    assert_eq!(count("state-of-the-art"), 1);
}

/// A configured field that is present but not text is refused rather than
/// waved through — otherwise the limit is bypassed by writing the same
/// sentence as a list.
#[test]
fn a_configured_field_that_is_not_text_is_refused() {
    let limits = limits(3);
    let listed = json!({ "rule": ["one", "two", "three", "four", "five"] });

    assert!(check(&limits, "agent.lesson.v2", &listed).is_err());
}

/// An absent field stays the schema's business. This policy bounds what is
/// written; whether a field is required is a different question, answered
/// somewhere else.
#[test]
fn an_absent_field_is_left_to_the_schema() {
    assert!(check(&limits(3), "agent.lesson.v2", &json!({ "other": "x" })).is_ok());
}

/// No configuration, no change: the behaviour of every store that has not
/// asked for a limit stays exactly what it was.
#[test]
fn a_store_without_limits_refuses_nothing() {
    let long = vec!["word"; 500].join(" ");

    assert!(check(&Limits::new(), "agent.lesson.v2", &json!({ "rule": long })).is_ok());
}

/// A limit of zero would refuse every record carrying the field. A store whose
/// settings silently reject everything is worse than one that will not load,
/// so the settings are refused instead of quietly disabling themselves.
#[test]
fn a_limit_of_zero_is_refused_as_settings_rather_than_ignored() {
    assert!(validate(&limits(0)).is_err());
    assert!(validate(&limits(1)).is_ok());
}

/// The refusal names the type, the field and the two numbers, and says nothing
/// about what was written: refusals travel into logs that the payload may not.
#[test]
fn a_refusal_names_the_counts_and_never_the_text() {
    let secret = format!("classified {}", vec!["word"; 30].join(" "));
    let error = check(&limits(20), "agent.lesson.v2", &json!({ "rule": &secret }))
        .expect_err("over the limit");
    let message = error.to_string();

    assert!(message.contains("agent.lesson.v2"), "{message}");
    assert!(message.contains("rule"), "{message}");
    assert!(
        message.contains("31") && message.contains("20"),
        "{message}"
    );
    assert!(
        !message.contains("classified"),
        "the payload leaked: {message}"
    );
}

/// Every configured field is checked, not just up to the first one missing.
///
/// A field the record does not carry is skipped, and skipping must mean "this
/// one", not "stop here": with two fields configured and the first absent, a
/// check that returned early would let the second one past unbounded.
#[test]
fn a_missing_field_does_not_stop_the_ones_after_it() {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert("absent".to_string(), FieldLimit { max_words: 3 });
    fields.insert("rule".to_string(), FieldLimit { max_words: 3 });
    let mut limits = Limits::new();
    limits.insert("agent.lesson.v2".to_string(), fields);

    let over = json!({ "rule": "one two three four" });
    assert!(
        check(&limits, "agent.lesson.v2", &over).is_err(),
        "the second field was left unchecked because the first was absent"
    );
}
