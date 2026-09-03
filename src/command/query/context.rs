//! Assembling context from a profile — named by the caller, or the one the
//! store nominates.
use crate::kernel::error::Error;
use crate::{command, context, filter, kernel, record, telemetry};
use std::path::PathBuf;

#[allow(clippy::too_many_arguments)]
pub fn context(
    json: bool,
    store: PathBuf,
    profile: Option<String>,
    request: Option<PathBuf>,
    query: Option<String>,
    mut coordinates: Vec<String>,
    project: Option<String>,
    role: Option<String>,
    phase: Option<String>,
    harness: Option<String>,
    process: Option<String>,
    tags: Vec<String>,
    kinds: Vec<String>,
    at: Option<String>,
    include_superseded: bool,
    filters: Vec<String>,
    strict: bool,
    format: command::cli::FormatArg,
    fields: Vec<String>,
) -> Result<String, Error> {
    let actor = kernel::identity::actor_from_env()?;
    let filter = filter::Filter::parse(&filters, strict)?;
    // Which profile answers is the store's decision, not the caller's memory
    // of one. A store that names a default lets an agent ask its question
    // without repeating the store's own configuration back to it.
    let profile = match profile {
        Some(named) => named,
        None => context::default_profile(&store)?,
    };
    // `--process` is a coordinate like any other. Naming it in the engine as
    // anything more would put one domain's vocabulary in the reader.
    for (key, value) in [
        ("project", project),
        ("role", role),
        ("phase", phase),
        ("harness", harness),
        ("process", process),
    ] {
        if let Some(value) = value {
            coordinates.push(format!("{key}={value}"));
        }
    }
    let bundle = match request {
        Some(path) => context::assemble_file(&store, &profile, &path, &actor, &filter)?,
        None => {
            let request =
                context::inline_request(query, coordinates, tags, kinds, at, include_superseded)?;
            context::assemble(&store, &profile, request, &actor, &filter)?
        }
    };
    // In the order the selection made, not the order the ledger holds. A
    // selector that asked for a particular order means it for every way of
    // printing the answer; filtering the ledger by a set of ids throws that
    // order away and hands back whatever the ledger happened to keep.
    let selected =
        if json || !(fields.is_empty() && matches!(format, command::cli::FormatArg::Jsonl)) {
            let mut by_id: std::collections::HashMap<_, _> = record::read_all(&store)?
                .into_iter()
                .map(|item| (item.id, item))
                .collect();
            bundle
                .selected_record_ids
                .iter()
                .filter_map(|id| by_id.remove(id))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
    let text = if fields.is_empty() && matches!(format, command::cli::FormatArg::Jsonl) {
        bundle.content.clone()
    } else {
        command::present::records(&selected, super::shape(format), &fields)?
    };
    telemetry::record_query(
        &store,
        "context",
        &bundle.receipt.request_digest,
        bundle
            .receipt
            .unmatched_coordinates
            .iter()
            .map(|item| item.key.as_str())
            .collect(),
        bundle.selected_record_ids.len(),
        telemetry::enabled(),
    );
    if json {
        // The receipt gains the records it already named, as objects rather
        // than as a string holding JSON: `content` carried them escaped, so a
        // caller had to parse a field out of a parsed document to reach them.
        // Everything the receipt said before it still says, byte for byte —
        // this adds a field, it does not reshape one.
        return command::output::render(json, &with_records(&bundle, &selected)?, text);
    }
    command::output::render(json, &bundle, text)
}

/// The receipt as it was, plus `records`: the selected records themselves, in
/// the order the receipt names them. The bundle's own type is untouched —
/// this is how the answer is presented, not what the store holds.
fn with_records(
    bundle: &context::ContextBundle,
    selected: &[record::StoredRecord],
) -> Result<serde_json::Value, Error> {
    let mut value = serde_json::to_value(bundle)?;
    if let serde_json::Value::Object(fields) = &mut value {
        fields.insert("records".into(), serde_json::to_value(selected)?);
    }
    Ok(value)
}
