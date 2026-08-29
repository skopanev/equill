mod apply;
mod schema;
#[cfg(test)]
mod tests;

use crate::kernel::error::Error;
pub use apply::matches;

/// Envelope names a caller may filter on, with the sub-names each one allows.
/// These are the coordinates every record carries whatever its type, so no
/// schema declares them — but a typo inside one still has to be caught, which
/// is why the nested names are listed rather than waved through.
pub const ENVELOPE_FIELDS: [(&str, &[&str]); 9] = [
    ("id", &[]),
    ("namespace", &[]),
    ("type", &[]),
    ("actor", &[]),
    ("recorded_at", &[]),
    ("observed_at", &[]),
    ("valid_at", &[]),
    ("tags", &[]),
    ("evidence", &["kind", "reference", "sha256"]),
];

/// Whether a path addresses the envelope, and if so whether it names something
/// the envelope actually has.
pub(crate) fn envelope_path(path: &[String]) -> Option<bool> {
    let (_, nested) = ENVELOPE_FIELDS.iter().find(|(name, _)| *name == path[0])?;
    Some(match path.len() {
        1 => true,
        2 => nested.contains(&path[1].as_str()),
        _ => false,
    })
}
pub use schema::{in_scope, validate};

/// One `--where` flag. Values inside a single flag are alternatives; separate
/// flags are conjunctions, so `--where a=1,2 --where b=3` reads as
/// "(a is 1 or 2) and b is 3".
#[derive(Clone, Debug, PartialEq)]
pub struct Condition {
    pub path: Vec<String>,
    pub negated: bool,
    pub values: Vec<Term>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    Null,
    Literal(String),
}

/// How a record that does not carry the field at all should be treated. A
/// selector already reads an absent or null coordinate as "applies to
/// everything", and a filter that silently disagreed with retrieval would be a
/// second, invisible policy. `--strict` is the way to ask for the other reading.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Absent {
    Wildcard,
    Exclude,
}

#[derive(Clone, Debug, Default)]
pub struct Filter {
    conditions: Vec<Condition>,
    strict: bool,
}

impl Filter {
    pub fn parse(flags: &[String], strict: bool) -> Result<Self, Error> {
        let conditions = flags
            .iter()
            .map(|flag| condition(flag))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { conditions, strict })
    }

    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
    }

    pub fn conditions(&self) -> &[Condition] {
        &self.conditions
    }

    pub fn absent(&self) -> Absent {
        if self.strict {
            Absent::Exclude
        } else {
            Absent::Wildcard
        }
    }
}

fn condition(flag: &str) -> Result<Condition, Error> {
    let (field, raw) = flag
        .split_once('=')
        .ok_or_else(|| invalid(format!("filter {flag} must be written as field=value")))?;
    let path = field
        .split('.')
        .map(str::trim)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if field.trim().is_empty() || path.iter().any(String::is_empty) {
        return Err(invalid(format!("filter {flag} has an empty field name")));
    }
    // A leading `!` negates the whole flag, including a list of alternatives:
    // `kind=!a,b` means "kind is neither a nor b".
    let (negated, raw) = match raw.strip_prefix('!') {
        Some(rest) => (true, rest),
        None => (false, raw),
    };
    let values = raw
        .split(',')
        .map(|value| match value {
            "null" => Term::Null,
            other => Term::Literal(other.to_owned()),
        })
        .collect::<Vec<_>>();
    if values
        .iter()
        .any(|value| matches!(value, Term::Literal(literal) if literal.is_empty()))
    {
        return Err(invalid(format!("filter {flag} has an empty value")));
    }
    if values.len() > 1 && values.contains(&Term::Null) {
        return Err(invalid(format!(
            "filter {flag} mixes null with other values; ask for them separately"
        )));
    }
    Ok(Condition {
        path,
        negated,
        values,
    })
}

pub(crate) fn invalid(reason: impl Into<String>) -> Error {
    Error::Filter(reason.into())
}

pub(crate) fn candidate_limit(records: usize, requested: u16) -> Result<u16, Error> {
    u16::try_from(records.max(usize::from(requested))).map_err(|_| {
        invalid("filtered search supports at most 65535 records; narrow the type or namespace")
    })
}
