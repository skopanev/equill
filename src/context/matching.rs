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
    if record
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

fn set_or_wildcard(actual: Option<&serde_json::Value>, expected: &serde_json::Value) -> bool {
    match actual {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::Array(values)) if !expected.is_array() => {
            values.iter().any(|value| value == expected)
        }
        Some(value) => value == expected,
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
