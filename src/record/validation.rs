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
    // The writer is usually appending JSONL by hand and cannot see the registered
    // contract, so a bare "does not match" costs them a hunt through the schema.
    // Name the field and the constraint instead.
    let mut faults = validator
        .iter_errors(payload)
        .map(|error| {
            let pointer = error.instance_path().to_string();
            let at = if pointer.is_empty() {
                "payload"
            } else {
                &pointer
            };
            // The offending value is quoted back in full by the validator; a
            // rule that is too long would then bury its own error message.
            format!("{at}: {}", shorten(&error.to_string()))
        })
        .collect::<Vec<_>>();
    if faults.is_empty() {
        return Ok(());
    }
    faults.sort();
    const SHOWN: usize = 5;
    let hidden = faults.len().saturating_sub(SHOWN);
    faults.truncate(SHOWN);
    let mut message = format!("payload does not match {type_name}: {}", faults.join("; "));
    if hidden > 0 {
        message.push_str(&format!(" (and {hidden} more)"));
    }
    Err(Error::InvalidRecord(message))
}

/// The validator quotes the offending value before stating the reason, so a
/// value that is itself too long would push its own explanation out of view.
/// Keep both ends: enough of the value to recognise it, all of the reason.
fn shorten(text: &str) -> String {
    const HEAD: usize = 60;
    const TAIL: usize = 90;
    let indices = text
        .char_indices()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if indices.len() <= HEAD + TAIL {
        return text.to_owned();
    }
    let head = &text[..indices[HEAD]];
    let tail = &text[indices[indices.len() - TAIL]..];
    format!("{head}… …{tail}")
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
