mod budget;
mod matching;
mod model;
mod receipt;
mod registry;
mod retrieval;

use crate::filter::Filter;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::kernel::{identity, store};
use std::fs;
use std::path::Path;

pub use model::{ContextBundle, ContextRequest, RegistryReport};

pub fn register_profile(store: &Path, file: &Path, actor: &str) -> Result<RegistryReport, Error> {
    registry::register_profile(store, file, actor)
}

pub fn register_selector(store: &Path, file: &Path, actor: &str) -> Result<RegistryReport, Error> {
    registry::register_selector(store, file, actor)
}

pub fn assemble_file(
    store_root: &Path,
    profile_id: &str,
    request_file: &Path,
    actor: &str,
    filter: &Filter,
) -> Result<ContextBundle, Error> {
    let request: model::ContextRequest = serde_json::from_slice(&fs::read(request_file)?)?;
    assemble(store_root, profile_id, request, actor, filter)
}

pub fn assemble(
    store_root: &Path,
    profile_id: &str,
    request: model::ContextRequest,
    actor: &str,
    filter: &Filter,
) -> Result<ContextBundle, Error> {
    let config = store::load(store_root)?;
    if actor != config.root_owner {
        identity::require_root(&config, actor).or_else(|_| {
            let (profile, _) = registry::load_profile(store_root, profile_id)?;
            identity::permits(&profile.actors, actor)
                .then_some(())
                .ok_or(Error::PermissionDenied)
        })?;
    }
    let (profile, profile_coordinate) = registry::load_profile(store_root, profile_id)?;
    let mut selectors = Vec::new();
    let mut selector_coordinates = Vec::new();
    for id in &profile.selectors {
        let (selector, coordinate) = registry::load_selector(store_root, id)?;
        selectors.push(selector);
        selector_coordinates.push(coordinate);
    }
    selector_coordinates.sort_by(|left, right| left.id.cmp(&right.id));
    let request_digest = sha256_hex(&serde_json::to_vec(&request)?);
    // The filter is checked against the very types this profile can read, so a
    // typo names itself instead of quietly returning nothing.
    let scope = selectors
        .iter()
        .map(|selector| crate::schema::load(store_root, &selector.type_name))
        .collect::<Result<Vec<_>, _>>()?;
    crate::filter::validate(filter, &scope)?;
    let mut retrieved = retrieval::retrieve(store_root, &profile, &selectors, &request, filter)?;
    let budgeted = budget::apply(
        std::mem::take(&mut retrieved.candidates),
        &profile.budget,
        std::mem::take(&mut retrieved.excluded),
    )?;
    if budgeted.required_overflow > 0 {
        return Err(Error::Context(format!(
            "required context exceeds the {} unit limit: {} record(s) excluded",
            budgeted.required_limit, budgeted.required_overflow
        )));
    }
    receipt::bundle(
        store_root,
        profile_coordinate,
        selector_coordinates,
        request_digest,
        profile.budget,
        retrieved,
        budgeted,
    )
}

/// The profile this store nominates, or a refusal that says so plainly.
pub fn default_profile(store: &Path) -> Result<String, Error> {
    store::load(store)?.default_context_profile.ok_or_else(|| {
        Error::Context(
            "this store nominates no default context profile; name one with --profile".into(),
        )
    })
}

pub fn profile_faults(store_root: &Path) -> Result<usize, Error> {
    let mut faults = 0;
    for path in registry::profile_files(store_root)? {
        let profile: model::ContextProfile = serde_json::from_slice(&fs::read(path)?)?;
        let mut selectors = Vec::new();
        for id in &profile.selectors {
            match registry::load_selector(store_root, id) {
                Ok((selector, _)) => selectors.push(selector),
                Err(_) => {
                    faults += 1;
                    continue;
                }
            }
        }
        let request = model::ContextRequest {
            at: jiff::Timestamp::now().to_string(),
            query: String::new(),
            tags: Vec::new(),
            kinds: Vec::new(),
            coordinates: Default::default(),
            include_superseded: false,
        };
        let retrieved = retrieval::retrieve(
            store_root,
            &profile,
            &selectors,
            &request,
            &Filter::default(),
        )?;
        let budgeted = budget::apply(retrieved.candidates, &profile.budget, retrieved.excluded)?;
        if budgeted.required_overflow > 0 {
            faults += 1;
        }
    }
    Ok(faults)
}

#[cfg(test)]
mod tests;

/// Builds a request from flags instead of a file. `at` defaults to now, which
/// is what a person asking a question means, and coordinates arrive as
/// `key=value` because the coordinate names belong to the domain, not to this
/// executable — a fixed flag per name would have to be invented for every store.
pub fn inline_request(
    query: Option<String>,
    coordinates: Vec<String>,
    tags: Vec<String>,
    kinds: Vec<String>,
    at: Option<String>,
    include_superseded: bool,
) -> Result<model::ContextRequest, Error> {
    let mut parsed = std::collections::BTreeMap::new();
    for entry in &coordinates {
        let (key, value) = entry.split_once('=').ok_or_else(|| {
            Error::Context(format!("coordinate {entry} must be written as key=value"))
        })?;
        if key.trim().is_empty() || value.is_empty() {
            return Err(Error::Context(format!(
                "coordinate {entry} needs a name and a value"
            )));
        }
        // A comma lists alternatives, matching how a record holds several
        // values for one coordinate.
        let value = match value.split_once(',') {
            None => serde_json::Value::String(value.to_owned()),
            Some(_) => serde_json::Value::Array(
                value
                    .split(',')
                    .map(|item| serde_json::Value::String(item.to_owned()))
                    .collect(),
            ),
        };
        parsed.insert(key.to_owned(), value);
    }
    Ok(model::ContextRequest {
        at: at.unwrap_or_else(|| jiff::Timestamp::now().to_string()),
        query: query.unwrap_or_default(),
        tags,
        kinds,
        coordinates: parsed,
        include_superseded,
    })
}
