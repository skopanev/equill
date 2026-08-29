use super::TypeDefinition;
use crate::kernel::error::Error;
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct CatalogReport {
    pub ok: bool,
    pub types: Vec<TypeSummary>,
}

#[derive(Debug, Serialize)]
pub struct TypeSummary {
    #[serde(rename = "type")]
    pub type_name: String,
    pub uri: String,
    pub owner: String,
    pub lifecycle: String,
    pub required: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TypeReport {
    pub ok: bool,
    #[serde(flatten)]
    pub summary: TypeSummary,
    pub fields: Vec<FieldSummary>,
}

/// What a lane needs before it can write or filter: the field name, whether it
/// is required, and — the part that cannot be guessed — the legal values of a
/// constrained field.
#[derive(Debug, Serialize)]
pub struct FieldSummary {
    pub name: String,
    pub kind: String,
    pub required: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<String>,
}

pub fn list(store_root: &Path) -> Result<CatalogReport, Error> {
    let mut types = names(store_root)?
        .iter()
        .map(|name| super::load(store_root, name).map(|definition| summary(&definition)))
        .collect::<Result<Vec<_>, _>>()?;
    types.sort_by(|left, right| left.type_name.cmp(&right.type_name));
    Ok(CatalogReport { ok: true, types })
}

pub fn show(store_root: &Path, type_name: &str) -> Result<TypeReport, Error> {
    let definition = super::load(store_root, type_name)?;
    Ok(TypeReport {
        ok: true,
        summary: summary(&definition),
        fields: fields(&definition),
    })
}

fn names(store_root: &Path) -> Result<Vec<String>, Error> {
    let directory = store_root.join("registry/types");
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().is_some_and(|value| value == "json")
            && let Some(stem) = path.file_stem().and_then(|value| value.to_str())
        {
            names.push(stem.to_owned());
        }
    }
    Ok(names)
}

fn summary(definition: &TypeDefinition) -> TypeSummary {
    TypeSummary {
        type_name: definition.type_name.clone(),
        uri: definition.uri.clone(),
        owner: definition.owner.clone(),
        lifecycle: format!("{:?}", definition.lifecycle.mode).to_lowercase(),
        required: required(&definition.payload_schema),
    }
}

fn fields(definition: &TypeDefinition) -> Vec<FieldSummary> {
    let required = required(&definition.payload_schema);
    let Some(properties) = definition
        .payload_schema
        .get("properties")
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    properties
        .iter()
        .map(|(name, schema)| FieldSummary {
            name: name.clone(),
            kind: kind(schema),
            required: required.contains(name),
            allowed: allowed(schema),
        })
        .collect()
}

fn kind(schema: &Value) -> String {
    match schema.get("type") {
        Some(Value::String(name)) => name.clone(),
        Some(Value::Array(names)) => names
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("|"),
        _ => "any".into(),
    }
}

/// Enum values are read through `items` as well, so the vocabulary of a list
/// field is as visible as that of a scalar one.
fn allowed(schema: &Value) -> Vec<String> {
    let source = schema.get("items").unwrap_or(schema);
    source
        .get("enum")
        .and_then(Value::as_array)
        .map(|values| values.iter().map(flatten).collect())
        .unwrap_or_default()
}

fn flatten(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn required(schema: &Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
