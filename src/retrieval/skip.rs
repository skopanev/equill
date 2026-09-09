//! Queries an operator has declared are not questions.
//!
//! Our own harness forwards bus notifications into the prompt, and each one
//! used to fire a full hybrid retrieval over a line that asks nothing. The
//! pattern that recognised them lived in a shell script on one consumer, so
//! every other reader of the same store either repeated it or paid the cost.
//!
//! A match suppresses the query-driven half of retrieval and nothing else. The
//! coordinate and recency selectors never look at the query, so a role, a
//! process and its steps are assembled exactly as before — which is the point:
//! an agent that matched a pattern must still know who it is.
use crate::kernel::error::Error;
use regex::Regex;

/// The compiled patterns, beside the text an operator wrote.
///
/// Both are kept because the receipt names the rule that matched, and a
/// compiled regex prints its own normalisation rather than the line in the
/// settings file. An operator has to be able to find what matched by searching
/// their own configuration.
#[derive(Clone, Debug, Default)]
pub struct SkipRules {
    rules: Vec<(String, Regex)>,
}

impl SkipRules {
    /// Compiles every pattern, refusing the whole set if one will not compile.
    ///
    /// A filter that silently never matches is worse than no filter: the cost
    /// stays and the operator believes it is gone. So this is a load failure,
    /// not a warning, and the store does not open until it is fixed.
    pub fn compile(patterns: &[String]) -> Result<Self, Error> {
        let mut rules = Vec::with_capacity(patterns.len());
        for pattern in patterns {
            match Regex::new(pattern) {
                Ok(compiled) => rules.push((pattern.clone(), compiled)),
                Err(error) => {
                    return Err(Error::InvalidSettings(format!(
                        "retrieval.skip_query_patterns holds a pattern that will not compile: {}",
                        first_line(&error.to_string())
                    )));
                }
            }
        }
        Ok(Self { rules })
    }

    /// The first rule that matches, as written in the settings file.
    ///
    /// Matched against the raw query, before any normalisation, so what an
    /// operator writes is what they can predict from the line they see.
    pub fn matched(&self, raw_query: &str) -> Option<&str> {
        self.rules
            .iter()
            .find(|(_, compiled)| compiled.is_match(raw_query))
            .map(|(pattern, _)| pattern.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// A compile error spans several lines with a caret diagram under the pattern.
/// The settings error is one line, and the reader still has the pattern itself.
fn first_line(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("invalid regular expression")
        .to_owned()
}
