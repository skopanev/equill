use super::{Absent, Condition, Filter, Term};
use serde_json::Value;

/// A record passes when every flag passes; a flag passes when any of its values
/// does. Absence is decided once, by the filter's own policy, so the answer
/// never depends on which condition happened to be evaluated first.
pub fn matches(payload: &Value, filter: &Filter) -> bool {
    filter
        .conditions()
        .iter()
        .all(|condition| holds(payload, condition, filter.absent()))
}

fn holds(payload: &Value, condition: &Condition, absent: Absent) -> bool {
    let actual = resolve(payload, &condition.path);
    let missing = matches!(actual, None | Some(Value::Null));
    // `field=null` and `field=!null` are questions about presence itself, so
    // they answer directly instead of going through the absence policy.
    if condition.values == [Term::Null] {
        return missing != condition.negated;
    }
    if missing {
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
/// into every element, so `evidence.kind=x` works whether evidence is one
/// object or a list of them.
fn resolve<'a>(payload: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut current = payload;
    for segment in path {
        current = match current {
            Value::Object(fields) => fields.get(segment)?,
            Value::Array(items) => {
                return items
                    .iter()
                    .find_map(|item| resolve(item, std::slice::from_ref(segment)));
            }
            _ => return None,
        };
    }
    Some(current)
}
