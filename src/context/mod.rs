mod budget;
mod matching;
mod model;
mod receipt;
mod registry;
mod retrieval;

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
) -> Result<ContextBundle, Error> {
    let request: model::ContextRequest = serde_json::from_slice(&fs::read(request_file)?)?;
    assemble(store_root, profile_id, request, actor)
}

pub fn assemble(
    store_root: &Path,
    profile_id: &str,
    request: model::ContextRequest,
    actor: &str,
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
    let retrieved = retrieval::retrieve(store_root, &profile, &selectors, &request)?;
    let budgeted = budget::apply(retrieved.candidates, &profile.budget, retrieved.excluded)?;
    if budgeted.required_overflow > 0 {
        return Err(Error::Context(format!(
            "required context exceeds the {} unit limit: {} record(s) excluded",
            budgeted.required_limit, budgeted.required_overflow
        )));
    }
    let bundle_digest = sha256_hex(budgeted.content.as_bytes());
    let degraded = budgeted.degraded || !retrieved.degraded_strategies.is_empty();
    let empty = budgeted.selected.is_empty();
    let receipt = model::ContextReceipt {
        schema: "equill.context-receipt.v1",
        profile: profile_coordinate,
        selectors: selector_coordinates,
        request_digest,
        included: budgeted.selected.clone(),
        excluded: budgeted.excluded,
        strategies: retrieved.strategies,
        budget: profile.budget,
        used: budgeted.used,
        bundle_digest: bundle_digest.clone(),
        projection: retrieved.projection,
        degraded_strategies: retrieved.degraded_strategies,
        degraded,
        empty,
    };
    let receipt_path = receipt::persist(store_root, &receipt)?;
    let selected_record_ids = receipt.included.iter().map(|item| item.id).collect();
    Ok(ContextBundle {
        ok: true,
        content: budgeted.content,
        bundle_digest,
        selected_record_ids,
        receipt,
        receipt_path,
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
        };
        let retrieved = retrieval::retrieve(store_root, &profile, &selectors, &request)?;
        let budgeted = budget::apply(retrieved.candidates, &profile.budget, retrieved.excluded)?;
        if budgeted.required_overflow > 0 {
            faults += 1;
        }
    }
    Ok(faults)
}

#[cfg(test)]
mod tests;
