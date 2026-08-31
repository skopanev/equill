use super::model::{
    ContextProfile, ContextRequest, CoordinateMode, ExcludedCoordinate, ExclusionReason, Selector,
    Strategy, Tier,
};
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use jiff::Timestamp;
use std::collections::{HashMap, HashSet};

pub(super) fn gate(
    record: &StoredRecord,
    profile: &ContextProfile,
    selectors: &HashMap<&str, &Selector>,
    request: &ContextRequest,
    at: Timestamp,
    superseded: &HashSet<uuid::Uuid>,
) -> Result<Option<ExclusionReason>, Error> {
    if !read_authorized(record, profile) {
        return Ok(Some(ExclusionReason::Unauthorized));
    }
    if superseded.contains(&record.id) {
        return Ok(Some(ExclusionReason::Superseded));
    }
    if !request.include_superseded
        && record
            .tags
            .iter()
            .any(|tag| tag == "equill:revoked" || tag == "status:revoked")
    {
        return Ok(Some(ExclusionReason::Revoked));
    }
    let valid_at: Timestamp = record
        .valid_at
        .parse()
        .map_err(|_| Error::Context(format!("record {} has invalid valid_at", record.id)))?;
    if valid_at > at {
        return Ok(Some(ExclusionReason::InvalidAtRequestTime));
    }
    let Some(selector) = selectors.get(record.type_name.as_str()).copied() else {
        return Ok(Some(ExclusionReason::SelectorMismatch));
    };
    if expired(record, selector, at)? {
        return Ok(Some(ExclusionReason::Expired));
    }
    if !coordinates_match(record, selector, request) {
        return Ok(Some(ExclusionReason::SelectorMismatch));
    }
    if !request.kinds.is_empty() && !kind_matches(record, selector, &request.kinds) {
        return Ok(Some(ExclusionReason::SelectorMismatch));
    }
    Ok(None)
}

pub(super) fn read_authorized(record: &StoredRecord, profile: &ContextProfile) -> bool {
    profile.grants.iter().any(|grant| {
        grant.namespace == record.namespace
            && grant.types.iter().any(|item| item == &record.type_name)
    })
}

pub(super) fn classify(
    record: &StoredRecord,
    selector: &Selector,
    request: &ContextRequest,
    fts: &HashSet<uuid::Uuid>,
) -> Option<(Tier, Vec<Strategy>)> {
    if has_any(&record.tags, &selector.required_tags) {
        return Some((Tier::Required, vec![Strategy::Tag]));
    }
    if has_any(&record.tags, &selector.core_tags) {
        return Some((Tier::Core, vec![Strategy::Tag]));
    }
    let matched = selector
        .strategies
        .iter()
        .copied()
        .filter(|strategy| match strategy {
            Strategy::Exact => exact(record, &request.query),
            Strategy::Tag => has_any(&record.tags, &request.tags),
            Strategy::Recency => true,
            Strategy::Fts => fts.contains(&record.id),
        })
        .collect::<Vec<_>>();
    (!matched.is_empty()).then_some((Tier::Relevant, matched))
}

pub(super) fn exclusion(record: &StoredRecord, reason: ExclusionReason) -> ExcludedCoordinate {
    ExcludedCoordinate {
        id: record.id,
        namespace: record.namespace.clone(),
        type_name: record.type_name.clone(),
        reason,
    }
}

fn coordinates_match(record: &StoredRecord, selector: &Selector, request: &ContextRequest) -> bool {
    selector.coordinate_pointers.iter().all(|(key, pointer)| {
        request.coordinates.get(key).is_none_or(|expected| {
            let actual = record.payload.pointer(pointer);
            match selector.coordinate_modes.get(key) {
                Some(CoordinateMode::SetOrWildcard) => set_or_wildcard(actual, expected),
                _ => actual == Some(expected),
            }
        })
    })
}

/// Does what a record holds at a coordinate satisfy what the request asked for?
///
/// Both sides can be one value or several, and the rule is symmetric in that:
/// a record listing the roles it applies to and a request naming the roles it
/// cares about are the same kind of statement, and either can be written either
/// way. The previous version handled only one of the four combinations — a
/// record holding a set against a request holding a scalar — so a request for
/// `["lane", "backend"]` compared an array to a string, found them unequal, and
/// silently dropped every record whose role was a plain string. Nothing said
/// so; the rows were simply absent.
///
/// Absence is universal on the RECORD side only: a record that does not name a
/// role applies to all of them. A request that is not asking about roles says
/// so by leaving the coordinate out, which never reaches here. An explicit null
/// in the request is a different statement — it asks for the records that name
/// no role — and it keeps meaning that.
pub(super) fn set_or_wildcard(
    actual: Option<&serde_json::Value>,
    expected: &serde_json::Value,
) -> bool {
    use serde_json::Value::{Array, Null};
    match (actual, expected) {
        (None | Some(Null), _) => true,
        // Two sets meet if they share anything at all. Not equality: a record
        // for ["lane", "backend"] answers a request about ["backend", "kyc"].
        (Some(Array(held)), Array(wanted)) => held.iter().any(|value| wanted.contains(value)),
        (Some(Array(held)), wanted) => held.contains(wanted),
        (Some(held), Array(wanted)) => wanted.contains(held),
        (Some(held), wanted) => held == wanted,
    }
}

fn exact(record: &StoredRecord, query: &str) -> bool {
    if query.trim().is_empty() {
        return false;
    }
    let needle = query.to_lowercase();
    serde_json::to_string(&record.payload).is_ok_and(|value| value.to_lowercase().contains(&needle))
        || record
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(&needle))
}

fn has_any(values: &[String], expected: &[String]) -> bool {
    !expected.is_empty() && values.iter().any(|value| expected.contains(value))
}

fn kind_matches(record: &StoredRecord, selector: &Selector, kinds: &[String]) -> bool {
    selector
        .kind_pointer
        .as_ref()
        .and_then(|pointer| record.payload.pointer(pointer))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| kinds.iter().any(|item| item == kind))
}

fn expired(record: &StoredRecord, selector: &Selector, at: Timestamp) -> Result<bool, Error> {
    let value = selector
        .expires_at_pointer
        .as_ref()
        .and_then(|pointer| record.payload.pointer(pointer))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            record
                .tags
                .iter()
                .find_map(|tag| tag.strip_prefix("expires:"))
        });
    let Some(value) = value else {
        return Ok(false);
    };
    let expiry: Timestamp = value
        .parse()
        .map_err(|_| Error::Context(format!("record {} has invalid expiry", record.id)))?;
    Ok(expiry < at)
}

/// Why a requested coordinate matched nothing. An empty bundle and a
/// misunderstood coordinate look identical to a caller, and only one of them is
/// worth retrying — so the receipt says which happened.
pub(super) fn coordinate_diagnosis(
    records: &[StoredRecord],
    selectors: &[&Selector],
    request: &ContextRequest,
) -> Vec<super::model::UnmatchedCoordinate> {
    let mut unmatched = Vec::new();
    for (key, expected) in &request.coordinates {
        let mut declared = false;
        let mut matched = false;
        let mut exact_only = true;
        for selector in selectors {
            let Some(pointer) = selector.coordinate_pointers.get(key) else {
                continue;
            };
            declared = true;
            let wildcard = matches!(
                selector.coordinate_modes.get(key),
                Some(CoordinateMode::SetOrWildcard)
            );
            if wildcard {
                exact_only = false;
            }
            matched = matched
                || records.iter().any(|record| {
                    let actual = record.payload.pointer(pointer);
                    if wildcard {
                        set_or_wildcard(actual, expected)
                    } else {
                        actual == Some(expected)
                    }
                });
        }
        if !matched {
            unmatched.push(super::model::UnmatchedCoordinate {
                key: key.clone(),
                declared,
                exact_only: declared && exact_only,
            });
        }
    }
    unmatched
}
