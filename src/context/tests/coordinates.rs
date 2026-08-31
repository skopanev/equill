//! One coordinate, matched from both sides.
//!
//! A record says which roles it applies to; a request says which roles it cares
//! about. Either can be written as one value or as several, and the four
//! combinations are the same question asked four ways. Only one of them used to
//! work: a record holding a set against a request holding a scalar. The other
//! three compared an array to a string, found them unequal, and dropped the row
//! without saying anything — which is the part that made it expensive to find.
use super::super::matching::set_or_wildcard;
use serde_json::{Value, json};

/// The combination that already worked, kept so a fix cannot quietly lose it.
#[test]
fn a_record_holding_a_set_answers_a_request_holding_one_value() {
    assert!(held(json!(["lane", "backend"])).is_answered_by(&json!("lane")));
    assert!(!held(json!(["lane", "backend"])).is_answered_by(&json!("kyc")));
}

/// The one the live store was actually asking, and the one that was broken.
#[test]
fn a_record_holding_one_value_answers_a_request_holding_a_set() {
    assert!(held(json!("pm")).is_answered_by(&json!(["pm", "gm"])));
    assert!(held(json!("lane")).is_answered_by(&json!(["lane", "backend"])));
    assert!(!held(json!("pm")).is_answered_by(&json!(["lane", "backend"])));
}

/// Two sets meet if they share anything. Equality would be the wrong test:
/// a record for two roles answers a request about one of them and something
/// else, and the order they are written in means nothing.
#[test]
fn two_sets_meet_when_they_share_anything() {
    assert!(held(json!(["lane", "backend"])).is_answered_by(&json!(["backend", "kyc"])));
    assert!(held(json!(["lane", "backend"])).is_answered_by(&json!(["backend", "lane"])));
    assert!(!held(json!(["lane", "backend"])).is_answered_by(&json!(["kyc", "ops"])));
}

#[test]
fn one_value_answers_the_same_value_and_no_other() {
    assert!(held(json!("pm")).is_answered_by(&json!("pm")));
    assert!(!held(json!("pm")).is_answered_by(&json!("gm")));
}

/// A record that names no role applies to every role.
///
/// This is the record side, and it is universal in every shape the request can
/// take — that is what makes the fix a widening of the shapes handled rather
/// than of the answer.
#[test]
fn a_record_that_names_no_role_answers_anything() {
    assert!(set_or_wildcard(None, &json!("pm")));
    assert!(set_or_wildcard(None, &json!(["pm", "gm"])));
    assert!(held(Value::Null).is_answered_by(&json!("pm")));
    assert!(held(Value::Null).is_answered_by(&json!(["pm", "gm"])));
}

/// The request side is NOT universal, and deliberately so.
///
/// A caller that is not asking about roles leaves the coordinate out, and that
/// never reaches this function. Writing null in the request is a different
/// statement — it asks for the records that name no role — and it has always
/// meant that. Making it universal too would have been symmetric and would have
/// quietly widened every stored query that used it.
#[test]
fn an_explicit_null_request_still_asks_for_the_records_that_name_nothing() {
    assert!(held(Value::Null).is_answered_by(&Value::Null));
    assert!(set_or_wildcard(None, &Value::Null));
    assert!(!held(json!("pm")).is_answered_by(&Value::Null));
    assert!(!held(json!(["lane", "backend"])).is_answered_by(&Value::Null));
}

/// The controls: a role nobody asked about is not pulled in, in any of the four
/// shapes. A matcher that widened instead of narrowing would pass every test
/// above and fail every one of these.
#[test]
fn a_role_that_was_not_asked_for_is_not_pulled_in() {
    assert!(!held(json!("ops")).is_answered_by(&json!("pm")));
    assert!(!held(json!("ops")).is_answered_by(&json!(["pm", "gm"])));
    assert!(!held(json!(["ops"])).is_answered_by(&json!("pm")));
    assert!(!held(json!(["ops", "audit"])).is_answered_by(&json!(["pm", "gm"])));
}

/// An empty set is a statement, not an absence: it names no roles, so it meets
/// none. Absence is spelled null or nothing at all.
#[test]
fn an_empty_set_meets_nothing() {
    assert!(!held(json!([])).is_answered_by(&json!("pm")));
    assert!(!held(json!([])).is_answered_by(&json!(["pm"])));
    assert!(!held(json!("pm")).is_answered_by(&json!([])));
    assert!(!held(json!(["pm"])).is_answered_by(&json!([])));
}

/// Values that are not strings are compared the same way, because nothing in
/// the rule is about strings.
#[test]
fn the_rule_does_not_care_what_the_values_are() {
    assert!(held(json!(3)).is_answered_by(&json!([1, 2, 3])));
    assert!(held(json!([true])).is_answered_by(&json!(true)));
    assert!(!held(json!(3)).is_answered_by(&json!([1, 2])));
}

/// What a record holds at a coordinate, phrased so the assertions read as the
/// question being asked rather than as argument order.
struct Held(Value);

fn held(value: Value) -> Held {
    Held(value)
}

impl Held {
    fn is_answered_by(&self, request: &Value) -> bool {
        set_or_wildcard(Some(&self.0), request)
    }
}
