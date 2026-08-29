use super::{Absent, Condition, Filter, Term, envelope_path};
use crate::record::StoredRecord;
use serde_json::Value;

/// A record passes when every flag passes; a flag passes when any of its values
/// does. Absence is decided once, by the filter's own policy, so the answer
/// never depends on which condition happened to be evaluated first.
/// A record is filtered as a whole. Its payload carries the domain's own
/// fields, while namespace, type, actor, tags and evidence sit on the envelope
/// — and a caller asking about `tags` or `evidence.kind` is asking an ordinary
/// question, not reaching into internals. Payload names win a collision,
/// because they belong to the schema the caller was reading when they typed it.
pub fn matches(record: &StoredRecord, filter: &Filter) -> bool {
    let envelope = serde_json::to_value(record).unwrap_or(Value::Null);
    filter
        .conditions()
        .iter()
        .all(|condition| holds(&record.payload, &envelope, condition, filter.absent()))
}

/// A bare name means the payload first, because that is what the caller was
/// reading in the schema when they typed it. `payload.x` and `record.x` say
/// which half explicitly, which is the only way to reach a payload field that
/// an envelope name shadows — or the reverse.
pub fn address<'a>(payload: &'a Value, envelope: &'a Value, path: &[String]) -> Option<&'a Value> {
    match path[0].as_str() {
        "payload" => resolve(payload, &path[1..]),
        "record" => resolve(envelope, &path[1..]),
        _ => resolve(payload, path).or_else(|| {
            envelope_path(path)
                .unwrap_or(false)
                .then(|| resolve(envelope, path))
                .flatten()
        }),
    }
}

fn holds(payload: &Value, envelope: &Value, condition: &Condition, absent: Absent) -> bool {
    let actual = address(payload, envelope, &condition.path);
    let missing = matches!(actual, None | Some(Value::Null));
    // Presence is asked about directly rather than through the absence policy:
    // `field=null` and `field=!null` are questions about presence itself, and a
    // list that offers null as one alternative keeps that meaning for the
    // records that have nothing there.
    let asks_for_null = condition.values.contains(&Term::Null);
    if condition.values == [Term::Null] {
        return missing != condition.negated;
    }
    if missing {
        if asks_for_null {
            return !condition.negated;
        }
        return match absent {
            // An absent field is the record saying "this does not narrow me",
            // which is the same reading a selector gives a null coordinate.
            Absent::Wildcard => true,
            Absent::Exclude => false,
        };
    }
    let found = condition
        .values
        .iter()
        .any(|value| contains(actual.expect("checked above"), value));
    found != condition.negated
}

/// Scalars compare by value; arrays compare by membership, so a record whose
/// field holds several values matches when any of them is asked for.
fn contains(actual: &Value, term: &Term) -> bool {
    match (actual, term) {
        (Value::Array(items), _) => items.iter().any(|item| contains(item, term)),
        (_, Term::Null) => actual.is_null(),
        (Value::String(text), Term::Literal(literal)) => text == literal,
        (Value::Bool(_) | Value::Number(_), Term::Literal(literal)) => {
            &actual.to_string() == literal
        }
        _ => false,
    }
}

/// Dotted paths address nested objects. A segment that meets an array descends
/// into every element and keeps walking the rest of the path, so
/// `evidence.kind` works whether evidence is one object or a list of them, and
/// so does a deeper path through a list.
fn resolve<'a>(payload: &'a Value, path: &[String]) -> Option<&'a Value> {
    let Some((segment, rest)) = path.split_first() else {
        return Some(payload);
    };
    match payload {
        Value::Object(fields) => resolve(fields.get(segment)?, rest),
        // Every element is tried against the whole remaining path, not just the
        // current segment: truncating it here silently answered the wrong
        // question for anything nested below a list.
        Value::Array(items) => items.iter().find_map(|item| resolve(item, path)),
        _ => None,
    }
}
