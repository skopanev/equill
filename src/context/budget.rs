use super::model::{
    ContextBudget, ExcludedCoordinate, ExclusionReason, RuntimeBudget, SelectedCoordinate, Tier,
    TokenUsage,
};
use super::retrieval::Candidate;
use crate::kernel::error::Error;
use crate::record::StoredRecord;
mod report;

pub struct Budgeted {
    pub content: String,
    pub required_limit: usize,
    pub required_needed: usize,
    pub selected: Vec<SelectedCoordinate>,
    pub excluded: Vec<ExcludedCoordinate>,
    pub usage: TokenUsage,
    pub degraded: bool,
    pub required_overflow: usize,
    pub effective_total: Option<usize>,
    pub vector_selected_records: usize,
    pub fts_selected_records: usize,
}

pub(super) struct Picked {
    pub(super) candidate: Candidate,
    pub(super) tokens: usize,
}

pub fn apply(
    candidates: Vec<Candidate>,
    budget: &ContextBudget,
    runtime: RuntimeBudget,
    render: &dyn Fn(&[StoredRecord]) -> Result<String, Error>,
    mut excluded: Vec<ExcludedCoordinate>,
) -> Result<Budgeted, Error> {
    super::tokenizer::validate(&budget.tokenizer)?;
    let effective_total = budget.effective_total(runtime.tokens);
    let reserve = effective_total
        .map(|_| budget.receipt_reserve())
        .unwrap_or(0);
    let content_limit = effective_total
        .map(|total| total.saturating_sub(reserve))
        .unwrap_or(usize::MAX);
    let required_limit = budget.required_cap.unwrap_or(usize::MAX).min(content_limit);
    let (required, core, relevant) = tiers(candidates);
    let required_count = required.len();
    if let Some(limit) = runtime.records
        && required_count > limit
    {
        return Err(Error::Context(format!(
            "CONTEXT_REQUIRED_OVERFLOW: required context needs {required_count} records but the runtime record limit is {limit}; {} required record(s) would be excluded",
            required_count - limit
        )));
    }
    let mut picked = Vec::new();
    for candidate in required {
        push(candidate, &mut picked, render, &budget.tokenizer)?;
    }
    let required_needed = total(&picked);
    if required_needed > required_limit {
        excluded.extend(
            picked
                .iter()
                .map(|item| report::excluded_item(item, ExclusionReason::RequiredOverflow)),
        );
        return report::finish(
            picked,
            excluded,
            render,
            budget,
            required_limit,
            required_needed,
            required_count,
            effective_total,
            reserve,
        );
    }

    let relevant_cost = prospective_total(&picked, &relevant, render, &budget.tokenizer)?
        .saturating_sub(required_needed);
    let protected = budget
        .relevant_floor()
        .min(relevant_cost)
        .min(content_limit.saturating_sub(required_needed));
    let core_limit = required_needed
        .saturating_add(budget.core_cap())
        .min(content_limit.saturating_sub(protected));
    take(
        core,
        core_limit,
        &mut picked,
        &mut excluded,
        ExclusionReason::CoreCap,
        runtime.records,
        render,
        &budget.tokenizer,
    )?;
    take(
        relevant,
        content_limit,
        &mut picked,
        &mut excluded,
        ExclusionReason::TotalBudget,
        runtime.records,
        render,
        &budget.tokenizer,
    )?;
    report::finish(
        picked,
        excluded,
        render,
        budget,
        required_limit,
        required_needed,
        0,
        effective_total,
        reserve,
    )
}

fn tiers(candidates: Vec<Candidate>) -> (Vec<Candidate>, Vec<Candidate>, Vec<Candidate>) {
    let mut required = Vec::new();
    let mut core = Vec::new();
    let mut relevant = Vec::new();
    for candidate in candidates {
        match candidate.tier {
            Tier::Required => required.push(candidate),
            Tier::Core => core.push(candidate),
            Tier::Relevant => relevant.push(candidate),
        }
    }
    (required, core, relevant)
}

#[allow(clippy::too_many_arguments)]
fn take(
    source: Vec<Candidate>,
    limit: usize,
    picked: &mut Vec<Picked>,
    excluded: &mut Vec<ExcludedCoordinate>,
    reason: ExclusionReason,
    record_limit: Option<usize>,
    render: &dyn Fn(&[StoredRecord]) -> Result<String, Error>,
    tokenizer: &super::model::TokenizerCoordinate,
) -> Result<(), Error> {
    for candidate in source {
        if record_limit.is_some_and(|limit| picked.len() >= limit) {
            excluded.push(ExcludedCoordinate {
                id: candidate.record.id,
                namespace: candidate.record.namespace,
                type_name: candidate.record.type_name,
                reason: ExclusionReason::RecordBudget,
            });
            continue;
        }
        let next = prospective_total(picked, std::slice::from_ref(&candidate), render, tokenizer)?;
        if next <= limit {
            push(candidate, picked, render, tokenizer)?;
        } else {
            excluded.push(ExcludedCoordinate {
                id: candidate.record.id,
                namespace: candidate.record.namespace,
                type_name: candidate.record.type_name,
                reason,
            });
        }
    }
    Ok(())
}

fn push(
    candidate: Candidate,
    picked: &mut Vec<Picked>,
    render: &dyn Fn(&[StoredRecord]) -> Result<String, Error>,
    tokenizer: &super::model::TokenizerCoordinate,
) -> Result<(), Error> {
    let before = total(picked);
    let after = prospective_total(picked, std::slice::from_ref(&candidate), render, tokenizer)?;
    picked.push(Picked {
        candidate,
        tokens: after.saturating_sub(before),
    });
    Ok(())
}

fn prospective_total(
    picked: &[Picked],
    additions: &[Candidate],
    render: &dyn Fn(&[StoredRecord]) -> Result<String, Error>,
    tokenizer: &super::model::TokenizerCoordinate,
) -> Result<usize, Error> {
    let records = picked
        .iter()
        .map(|item| item.candidate.record.clone())
        .chain(additions.iter().map(|item| item.record.clone()))
        .collect::<Vec<_>>();
    super::tokenizer::count(&render(&records)?, tokenizer)
}

fn total(picked: &[Picked]) -> usize {
    picked.iter().map(|item| item.tokens).sum()
}
