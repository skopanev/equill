//! Reading the registry: which types a store holds, and what one of them says.
use crate::kernel::error::Error;
use crate::{command, schema};
use std::path::Path;

pub fn list(json: bool, store: &Path) -> Result<String, Error> {
    let report = schema::list(store)?;
    let text = report
        .types
        .iter()
        .map(|item| {
            let required = if item.required.is_empty() {
                "none".to_string()
            } else {
                item.required.join(", ")
            };
            format!(
                "{} ({}) required: {required}",
                item.type_name, item.lifecycle
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    command::output::render(json, &report, text)
}

pub fn show(json: bool, store: &Path, type_name: &str) -> Result<String, Error> {
    let report = schema::show(store, type_name)?;
    let text = report
        .fields
        .iter()
        .map(|field| {
            let mark = if field.required {
                "required"
            } else {
                "optional"
            };
            // The vocabulary of a constrained field is the part a caller cannot
            // guess, so it is printed rather than left to the JSON form.
            let allowed = if field.allowed.is_empty() {
                String::new()
            } else {
                format!(" one of: {}", field.allowed.join(", "))
            };
            format!("{} {} {mark}{allowed}", field.name, field.kind)
        })
        .collect::<Vec<_>>()
        .join("\n");
    command::output::render(json, &report, text)
}
