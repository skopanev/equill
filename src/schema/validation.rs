use super::TypeDefinition;
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
    Ok(())
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
