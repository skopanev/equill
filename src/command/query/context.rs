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
    let text = if fields.is_empty() && matches!(format, command::cli::FormatArg::Jsonl) {
        bundle.content.clone()
    } else {
        let selected = record::read_all(&store)?
            .into_iter()
            .filter(|item| bundle.selected_record_ids.contains(&item.id))
            .collect::<Vec<_>>();
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
    command::output::render(json, &bundle, text)
}
