use super::{Filter, invalid};
use crate::kernel::error::Error;
use crate::schema::TypeDefinition;
use serde_json::Value;

/// A filter is checked against the schemas it will run over, so a typo fails
/// the command instead of returning an empty result. An empty result and "no
/// such field" look identical to a caller, and only one of them is worth
/// retrying.
pub fn validate(filter: &Filter, definitions: &[TypeDefinition]) -> Result<(), Error> {
    if filter.is_empty() {
        return Ok(());
    }
    if definitions.is_empty() {
        return Err(invalid(
            "filters need at least one registered type to check against",
        ));
    }
    for condition in filter.conditions() {
        let known = definitions
            .iter()
            .any(|definition| declared(&definition.payload_schema, &condition.path));
        if !known {
            let field = condition.path.join(".");
            let types = definitions
                .iter()
                .map(|definition| definition.type_name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(invalid(format!(
                "unknown field {field}: no type in scope declares it ({types})"
            )));
        }
    }
    Ok(())
}

/// Walks a JSON Schema the way the payload will be walked: through `properties`,
/// and through `items` wherever the schema describes a list of objects.
fn declared(schema: &Value, path: &[String]) -> bool {
    let mut current = schema;
    for segment in path {
        match descend(current, segment) {
            Some(next) => current = next,
            None => return false,
        }
    }
    true
}

fn descend<'a>(schema: &'a Value, segment: &str) -> Option<&'a Value> {
    if let Some(items) = schema.get("items") {
        return descend(items, segment);
    }
    schema.get("properties")?.get(segment)
}

/// The types a filter will be checked against: one when the caller named it,
/// otherwise every type the store has registered.
pub fn in_scope(
    store: &std::path::Path,
    type_name: Option<&str>,
) -> Result<Vec<TypeDefinition>, Error> {
    if let Some(name) = type_name {
        return Ok(vec![crate::schema::load(store, name)?]);
    }
    let directory = store.join("registry/types");
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
    names.sort();
    names
        .iter()
        .map(|name| crate::schema::load(store, name))
        .collect()
}
