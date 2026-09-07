use super::model::{ContextBundle, ContextRequest, RuntimeBudget};
use super::{budget, receipt, registry, retrieval};
use crate::filter::Filter;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::kernel::{identity, store};
use crate::record::StoredRecord;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub fn assemble(
    store_root: &Path,
    profile_id: &str,
    request: ContextRequest,
    actor: &str,
    filter: &Filter,
    runtime_budget: RuntimeBudget,
    render: &dyn Fn(&[StoredRecord]) -> Result<String, Error>,
) -> Result<ContextBundle, Error> {
    if runtime_budget.tokens == Some(0) {
        return Err(Error::Context(
            "runtime token budget must be positive".into(),
        ));
    }
    if runtime_budget.records == Some(0) {
        return Err(Error::Context(
            "runtime record budget must be positive".into(),
        ));
    }
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
    let scope = selectors
        .iter()
        .map(|selector| crate::schema::load(store_root, &selector.type_name))
        .collect::<Result<Vec<_>, _>>()?;
    crate::filter::validate(filter, &scope)?;
    let mut retrieved = retrieval::retrieve(
        store_root,
        &profile,
        &selectors,
        &request,
        filter,
        retrieval::Cardinality::Answering,
        runtime_budget.records,
    )?;
    let budgeted = budget::apply(
        std::mem::take(&mut retrieved.candidates),
        &profile.budget,
        runtime_budget,
        render,
        std::mem::take(&mut retrieved.excluded),
    )?;
    if budgeted.required_overflow > 0 {
        return Err(Error::Context(format!(
            "CONTEXT_REQUIRED_OVERFLOW: required context needs {} tokens but the effective limit is {}; {} required record(s) would be excluded",
            budgeted.required_needed, budgeted.required_limit, budgeted.required_overflow
        )));
    }
    receipt::bundle(
        store_root,
        profile_coordinate,
        selector_coordinates,
        request_digest,
        profile.budget,
        runtime_budget,
        retrieved,
        budgeted,
    )
}
