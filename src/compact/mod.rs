mod anchor;
mod apply;
mod lifecycle;
mod model;
mod planner;
mod receipt;
mod rewrite;
mod transaction;

use crate::kernel::error::Error;
use crate::kernel::governance::RootGuard;
use std::path::Path;

pub use model::CompactReport;

pub fn run(
    store_root: &Path,
    manifest: &Path,
    apply_changes: bool,
    actor: &str,
) -> Result<CompactReport, Error> {
    let (_guard, _config) = RootGuard::acquire(store_root, actor)?;
    let plan = planner::build(store_root, manifest, jiff::Timestamp::now())?;
    if apply_changes {
        return apply::execute(store_root, plan, actor);
    }
    let removed = plan
        .inputs
        .iter()
        .map(|input| input.public.removals.len())
        .sum();
    let retained_with_reason = plan
        .inputs
        .iter()
        .map(|input| input.public.retained.len())
        .sum();
    Ok(CompactReport {
        ok: true,
        applied: false,
        manifest_sha256: plan.manifest_sha256,
        inputs: plan.inputs.into_iter().map(|input| input.public).collect(),
        removed,
        retained_with_reason,
        receipt: None,
    })
}

#[cfg(test)]
mod tests;
