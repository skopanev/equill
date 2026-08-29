mod apply;
mod schema;
#[cfg(test)]
mod tests;

use crate::kernel::error::Error;
pub use apply::{address, matches};

/// Envelope names a caller may filter on, with the sub-names each one allows.
/// These are the coordinates every record carries whatever its type, so no
/// schema declares them — but a typo inside one still has to be caught, which
/// is why the nested names are listed rather than waved through.
pub const ENVELOPE_FIELDS: [(&str, &[&str]); 10] = [
    ("id", &[]),
    ("namespace", &[]),
    ("type", &[]),
    ("actor", &[]),
    ("recorded_at", &[]),
    ("observed_at", &[]),
    ("valid_at", &[]),
    ("tags", &[]),
    ("evidence", &["kind", "reference", "sha256"]),
    ("supersedes", &[]),
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
    // `role=backend,null` reads as "backend, or nothing said about role" — one
    // question, not two. Negating that mixture is the ambiguous case: it is
    // unclear whether the absent records are being excluded or kept, so it is
    // refused by name rather than guessed at.
    if negated && values.len() > 1 && values.contains(&Term::Null) {
        return Err(invalid(format!(
            "filter {flag} negates a list containing null; ask for the values and \
             the absence separately so the intent is explicit"
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

/// A filtered search must look at the whole in-scope corpus, not at the
/// caller's page: a match that sits past the page boundary is still a match.
/// The scan is bounded, and hitting that bound says so precisely instead of
/// silently returning less.
/// How many records a filtered search must be able to look at. Counted inside
/// the namespace and type the caller narrowed to, because advising them to
/// narrow is useless if narrowing does not change the number.
pub(crate) fn scope_size(
    store: &std::path::Path,
    namespace: Option<&str>,
    type_name: Option<&str>,
) -> Result<usize, Error> {
    Ok(crate::record::read_all(store)?
        .iter()
        .filter(|record| namespace.is_none_or(|value| record.namespace == value))
        .filter(|record| type_name.is_none_or(|value| record.type_name == value))
        .count())
}

pub(crate) fn candidate_limit(records: usize, requested: u16) -> Result<u16, Error> {
    let wanted = records.max(usize::from(requested));
    u16::try_from(wanted)
        .ok()
        .filter(|scan| *scan <= crate::projection::MAX_SCAN)
        .ok_or_else(|| {
            invalid(format!(
                "a filtered search scans everything in scope, and this scope holds {records} \
                 records, past the {} the engine will scan; narrow it with --type or --namespace",
                crate::projection::MAX_SCAN
            ))
        })
}
