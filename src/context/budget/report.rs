use super::{Budgeted, Picked};
use crate::context::model::{
    ContextBudget, ExcludedCoordinate, ExclusionReason, SelectedCoordinate, Tier, TokenUsage,
};
use crate::kernel::error::Error;
use crate::record::StoredRecord;

#[allow(clippy::too_many_arguments)]
pub(super) fn finish(
    picked: Vec<Picked>,
    mut excluded: Vec<ExcludedCoordinate>,
    render: &dyn Fn(&[StoredRecord]) -> Result<String, Error>,
    budget: &ContextBudget,
    required_limit: usize,
    required_needed: usize,
    required_overflow: usize,
    effective_total: Option<usize>,
    reserve: usize,
) -> Result<Budgeted, Error> {
    excluded.sort_by_key(|item| item.id);
    let degraded = required_overflow > 0
        || excluded.iter().any(|item| {
            matches!(
                item.reason,
                ExclusionReason::RequiredOverflow
                    | ExclusionReason::CoreCap
                    | ExclusionReason::TotalBudget
                    | ExclusionReason::RecordBudget
            )
        });
    let records = picked
        .iter()
        .map(|item| item.candidate.record.clone())
        .collect::<Vec<_>>();
    let content = render(&records)?;
    let tier_tokens = |tier| {
        picked
            .iter()
            .filter(|item| item.candidate.tier == tier)
            .map(|item| item.tokens)
            .sum()
    };
    let content_tokens = crate::context::tokenizer::count(&content, &budget.tokenizer)?;
    let usage = TokenUsage {
        required: tier_tokens(Tier::Required),
        core: tier_tokens(Tier::Core),
        relevant: tier_tokens(Tier::Relevant),
        content: content_tokens,
        receipt_reserved: reserve,
        total: content_tokens.saturating_add(reserve),
    };
    let selected = picked
        .into_iter()
        .map(|item| SelectedCoordinate {
            id: item.candidate.record.id,
            namespace: item.candidate.record.namespace,
            type_name: item.candidate.record.type_name,
            tier: item.candidate.tier,
            tokens: item.tokens,
            strategies: item.candidate.strategies,
        })
        .collect();
    Ok(Budgeted {
        content,
        required_limit,
        required_needed,
        selected,
        excluded,
        usage,
        degraded,
        required_overflow,
        effective_total,
    })
}

pub(super) fn excluded_item(item: &Picked, reason: ExclusionReason) -> ExcludedCoordinate {
    ExcludedCoordinate {
        id: item.candidate.record.id,
        namespace: item.candidate.record.namespace.clone(),
        type_name: item.candidate.record.type_name.clone(),
        reason,
    }
}
