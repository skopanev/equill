use super::super::assembly;
use super::super::model::{ContextBundle, ContextRequest, RuntimeBudget};
use super::super::payload;
use crate::filter::Filter;
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use std::fs;
use std::path::Path;

#[allow(clippy::too_many_arguments)]
pub fn assemble_file_with_renderer_and_options(
    store: &Path,
    profile: &str,
    request: &Path,
    actor: &str,
    filter: &Filter,
    budget: RuntimeBudget,
    retrieval: crate::retrieval::Overrides,
    render: &dyn Fn(&[StoredRecord]) -> Result<String, Error>,
) -> Result<ContextBundle, Error> {
    let request: ContextRequest = serde_json::from_slice(&fs::read(request)?)?;
    assemble_with_renderer_and_options(
        store, profile, request, actor, filter, budget, retrieval, render,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn assemble_with_options(
    store: &Path,
    profile: &str,
    request: ContextRequest,
    actor: &str,
    filter: &Filter,
    budget: RuntimeBudget,
    retrieval: crate::retrieval::Overrides,
) -> Result<ContextBundle, Error> {
    assembly::assemble(
        store, profile, request, actor, filter, budget, retrieval, &payload,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn assemble_with_renderer_and_options(
    store: &Path,
    profile: &str,
    request: ContextRequest,
    actor: &str,
    filter: &Filter,
    budget: RuntimeBudget,
    retrieval: crate::retrieval::Overrides,
    render: &dyn Fn(&[StoredRecord]) -> Result<String, Error>,
) -> Result<ContextBundle, Error> {
    assembly::assemble(
        store, profile, request, actor, filter, budget, retrieval, render,
    )
}
