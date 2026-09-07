use super::assembly;
use super::model::{ContextBundle, ContextRequest, RuntimeBudget};
use super::payload;
use crate::filter::Filter;
use crate::kernel::error::Error;
use crate::record::StoredRecord;
use std::fs;
use std::path::Path;

pub fn assemble_file(
    store: &Path,
    profile: &str,
    request: &Path,
    actor: &str,
    filter: &Filter,
) -> Result<ContextBundle, Error> {
    assemble_file_with_limits(
        store,
        profile,
        request,
        actor,
        filter,
        RuntimeBudget::default(),
    )
}

pub fn assemble_file_with_budget(
    store: &Path,
    profile: &str,
    request: &Path,
    actor: &str,
    filter: &Filter,
    tokens: Option<usize>,
) -> Result<ContextBundle, Error> {
    assemble_file_with_limits(
        store,
        profile,
        request,
        actor,
        filter,
        RuntimeBudget {
            tokens,
            records: None,
        },
    )
}

pub fn assemble_file_with_limits(
    store: &Path,
    profile: &str,
    request: &Path,
    actor: &str,
    filter: &Filter,
    budget: RuntimeBudget,
) -> Result<ContextBundle, Error> {
    assemble_file_with_renderer_and_limits(store, profile, request, actor, filter, budget, &payload)
}

#[allow(clippy::too_many_arguments)]
pub fn assemble_file_with_renderer(
    store: &Path,
    profile: &str,
    request: &Path,
    actor: &str,
    filter: &Filter,
    tokens: Option<usize>,
    render: &dyn Fn(&[StoredRecord]) -> Result<String, Error>,
) -> Result<ContextBundle, Error> {
    assemble_file_with_renderer_and_limits(
        store,
        profile,
        request,
        actor,
        filter,
        RuntimeBudget {
            tokens,
            records: None,
        },
        render,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn assemble_file_with_renderer_and_limits(
    store: &Path,
    profile: &str,
    request: &Path,
    actor: &str,
    filter: &Filter,
    budget: RuntimeBudget,
    render: &dyn Fn(&[StoredRecord]) -> Result<String, Error>,
) -> Result<ContextBundle, Error> {
    let request: ContextRequest = serde_json::from_slice(&fs::read(request)?)?;
    assemble_with_renderer_and_limits(store, profile, request, actor, filter, budget, render)
}

pub fn assemble(
    store: &Path,
    profile: &str,
    request: ContextRequest,
    actor: &str,
    filter: &Filter,
) -> Result<ContextBundle, Error> {
    assemble_with_limits(
        store,
        profile,
        request,
        actor,
        filter,
        RuntimeBudget::default(),
    )
}

pub fn assemble_with_budget(
    store: &Path,
    profile: &str,
    request: ContextRequest,
    actor: &str,
    filter: &Filter,
    tokens: Option<usize>,
) -> Result<ContextBundle, Error> {
    assemble_with_limits(
        store,
        profile,
        request,
        actor,
        filter,
        RuntimeBudget {
            tokens,
            records: None,
        },
    )
}

pub fn assemble_with_limits(
    store: &Path,
    profile: &str,
    request: ContextRequest,
    actor: &str,
    filter: &Filter,
    budget: RuntimeBudget,
) -> Result<ContextBundle, Error> {
    assembly::assemble(store, profile, request, actor, filter, budget, &payload)
}

#[allow(clippy::too_many_arguments)]
pub fn assemble_with_renderer(
    store: &Path,
    profile: &str,
    request: ContextRequest,
    actor: &str,
    filter: &Filter,
    tokens: Option<usize>,
    render: &dyn Fn(&[StoredRecord]) -> Result<String, Error>,
) -> Result<ContextBundle, Error> {
    assemble_with_renderer_and_limits(
        store,
        profile,
        request,
        actor,
        filter,
        RuntimeBudget {
            tokens,
            records: None,
        },
        render,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn assemble_with_renderer_and_limits(
    store: &Path,
    profile: &str,
    request: ContextRequest,
    actor: &str,
    filter: &Filter,
    budget: RuntimeBudget,
    render: &dyn Fn(&[StoredRecord]) -> Result<String, Error>,
) -> Result<ContextBundle, Error> {
    assembly::assemble(store, profile, request, actor, filter, budget, render)
}
