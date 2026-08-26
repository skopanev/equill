use super::{EvidenceRef, RecordDraft, StoredRecord};
use crate::kernel::error::Error;
use crate::kernel::store::StoreConfig;
use crate::schema::TypeDefinition;
use jiff::Timestamp;

pub fn validate(
    draft: &RecordDraft,
    config: &StoreConfig,
    definition: &TypeDefinition,
) -> Result<(), Error> {
    if !config
        .namespaces
        .iter()
        .any(|namespace| namespace == &draft.namespace)
    {
        return Err(Error::InvalidRecord("namespace is not registered".into()));
    }
    parse_time(&draft.observed_at, "observed_at")?;
    if let Some(value) = &draft.valid_at {
        parse_time(value, "valid_at")?;
    }
    for evidence in &draft.evidence {
        validate_evidence(evidence)?;
    }
    validate_tags(&draft.tags)?;
    validate_payload(&draft.payload, &draft.type_name, definition)
}

pub fn validate_stored(
    record: &StoredRecord,
    config: &StoreConfig,
    definition: &TypeDefinition,
) -> Result<(), Error> {
    if !config
        .namespaces
        .iter()
        .any(|namespace| namespace == &record.namespace)
    {
        return Err(Error::InvalidRecord("namespace is not registered".into()));
    }
    parse_time(&record.recorded_at, "recorded_at")?;
    parse_time(&record.observed_at, "observed_at")?;
    parse_time(&record.valid_at, "valid_at")?;
    for evidence in &record.evidence {
        validate_evidence(evidence)?;
    }
    validate_tags(&record.tags)?;
    validate_payload(&record.payload, &record.type_name, definition)
}

fn validate_payload(
    payload: &serde_json::Value,
    type_name: &str,
    definition: &TypeDefinition,
) -> Result<(), Error> {
    let validator = jsonschema::draft202012::new(&definition.payload_schema)
        .map_err(|_| Error::InvalidSchema("payload validator could not compile".into()))?;
    if !validator.is_valid(payload) {
        return Err(Error::InvalidRecord(format!(
            "payload does not match {type_name}"
        )));
    }
    Ok(())
}

fn validate_tags(tags: &[String]) -> Result<(), Error> {
    if tags
        .iter()
        .any(|tag| tag.trim().is_empty() || tag.chars().any(char::is_control))
    {
        return Err(Error::InvalidRecord(
            "tags must be non-empty stable labels".into(),
        ));
    }
    Ok(())
}

fn parse_time(value: &str, field: &str) -> Result<Timestamp, Error> {
    value
        .parse()
        .map_err(|_| Error::InvalidRecord(format!("{field} must be an RFC3339 timestamp")))
}

fn validate_evidence(evidence: &EvidenceRef) -> Result<(), Error> {
    if evidence.kind.trim().is_empty()
        || evidence.reference.trim().is_empty()
        || evidence.kind.chars().any(char::is_control)
        || evidence.reference.chars().any(char::is_control)
    {
        return Err(Error::InvalidRecord(
            "evidence requires stable kind and reference".into(),
        ));
    }
    if let Some(digest) = &evidence.sha256 {
        let valid = digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !valid {
            return Err(Error::InvalidRecord(
                "evidence sha256 must contain 64 hexadecimal characters".into(),
            ));
        }
    }
    Ok(())
}
