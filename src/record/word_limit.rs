//! An optional ceiling on how many words a payload field may hold.
//!
//! A contract can ask for short entries in prose, and prose binds only whoever
//! read it: a rule addressed to one role does not reach the other writers, and
//! nothing counts words on their behalf. A schema does not close the gap
//! either, since it bounds characters rather than words.
//!
//! Nor can it be closed by changing the schema. A registered type is immutable
//! by id, and a new version means a cascade: type, selectors, profile, and
//! every existing record re-appended as a pair, because one type may not
//! supersede another. That is a large, irreversible migration to express one
//! number.
//!
//! So this is store policy, checked at the write, and it applies to the payload
//! actually being appended — never to what is already stored. A limit that
//! changed what old records mean would make the ledger unreadable the moment
//! somebody edited a setting.
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Per type, per payload field. Absent means unbounded, which is what every
/// store means today.
pub type Limits = BTreeMap<String, BTreeMap<String, FieldLimit>>;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FieldLimit {
    pub max_words: usize,
}

/// A word is a non-empty run between Unicode whitespace. A hyphen inside a word
/// does not split it: "well-measured" is one word, because a writer counting
/// their own sentence counts it as one.
pub fn count(text: &str) -> usize {
    text.split_whitespace()
        .filter(|word| !word.is_empty())
        .count()
}

/// Checks the payload about to be appended against the store's limits.
///
/// A configured field that is present but not a string is refused rather than
/// waved through: silence there would let a limit be bypassed by writing the
/// same sentence as a list. A field that is absent stays the schema's business
/// — this policy bounds what is written, it does not decide what is required.
pub fn check(limits: &Limits, type_name: &str, payload: &Value) -> Result<(), Error> {
    let Some(fields) = limits.get(type_name) else {
        return Ok(());
    };
    for (name, limit) in fields {
        let Some(value) = payload.get(name) else {
            continue;
        };
        let Some(text) = value.as_str() else {
            return Err(refusal(format!(
                "{type_name}.{name} is limited to {} words but is not text",
                limit.max_words
            )));
        };
        let actual = count(text);
        if actual > limit.max_words {
            // The counts and the names, never the text: a refusal is written to
            // logs and read by people who are not entitled to the payload.
            return Err(refusal(format!(
                "{type_name}.{name} has {actual} words, limit is {}",
                limit.max_words
            )));
        }
    }
    Ok(())
}

/// A limit of zero would refuse every record with that field, and a store whose
/// settings silently reject everything is worse than one that will not load.
pub fn validate(limits: &Limits) -> Result<(), Error> {
    for (type_name, fields) in limits {
        for (name, limit) in fields {
            if limit.max_words == 0 {
                return Err(refusal(format!(
                    "{type_name}.{name} max_words must be at least 1"
                )));
            }
        }
    }
    Ok(())
}

fn refusal(message: String) -> Error {
    Error::InvalidRecord(message)
}
