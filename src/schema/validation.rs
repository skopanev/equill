use super::{LifecycleMode, TypeDefinition};
use crate::kernel::error::Error;

pub fn validate(definition: &TypeDefinition) -> Result<(), Error> {
    validate_type_name(&definition.type_name)?;
    let segments: Vec<_> = definition.type_name.split('.').collect();
    let version = segments.last().copied().unwrap_or_default();
    let base = segments[..segments.len() - 1].join(".");
    let expected_uri = format!("equill://{base}/{version}");
    if definition.uri != expected_uri {
        return Err(Error::InvalidType(format!(
            "{} must use uri {expected_uri}",
            definition.type_name
        )));
    }
    if definition.owner.trim().is_empty() || definition.owner.chars().any(char::is_control) {
        return Err(Error::InvalidSchema(
            "owner must be a stable identity".into(),
        ));
    }
    jsonschema::draft202012::meta::validate(&definition.payload_schema)
        .map_err(|error| Error::InvalidSchema(error.to_string()))?;
    validate_lifecycle(definition)
}

fn validate_lifecycle(definition: &TypeDefinition) -> Result<(), Error> {
    let lifecycle = &definition.lifecycle;
    for predecessor in &lifecycle.allowed_predecessor_types {
        validate_type_name(predecessor)?;
        if predecessor == &definition.type_name {
            return Err(Error::InvalidSchema(
                "allowed_predecessor_types contains the current type".into(),
            ));
        }
    }
    let mut predecessors = lifecycle.allowed_predecessor_types.clone();
    predecessors.sort();
    predecessors.dedup();
    if predecessors.len() != lifecycle.allowed_predecessor_types.len() {
        return Err(Error::InvalidSchema(
            "allowed_predecessor_types contains duplicates".into(),
        ));
    }
    match lifecycle.mode {
        LifecycleMode::Linear => match lifecycle.key_pointer.as_deref() {
            Some(pointer) if pointer.starts_with('/') && pointer.len() <= 500 => Ok(()),
            _ => Err(Error::InvalidSchema(
                "linear lifecycle requires a JSON key_pointer".into(),
            )),
        },
        LifecycleMode::AppendOnly => {
            if lifecycle.key_pointer.is_some() || !lifecycle.allowed_predecessor_types.is_empty() {
                Err(Error::InvalidSchema(
                    "append_only lifecycle cannot declare replacement options".into(),
                ))
            } else {
                Ok(())
            }
        }
        LifecycleMode::Dag => {
            if lifecycle.key_pointer.is_some() {
                Err(Error::InvalidSchema(
                    "key_pointer is only valid for linear lifecycle".into(),
                ))
            } else {
                Ok(())
            }
        }
    }
}

pub fn validate_type_name(type_name: &str) -> Result<(), Error> {
    let segments: Vec<_> = type_name.split('.').collect();
    let version = segments.last().copied().unwrap_or_default();
    let valid_segments = segments.len() >= 3
        && segments.iter().all(|segment| {
            !segment.is_empty()
                && segment.chars().all(|character| {
                    character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
                })
        });
    if !valid_segments
        || !version.starts_with('v')
        || version.len() == 1
        || !version[1..]
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return Err(Error::InvalidType(type_name.to_owned()));
    }
    Ok(())
}
