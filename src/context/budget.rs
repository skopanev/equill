use super::model::{ContextBudget, ExcludedCoordinate, ExclusionReason, SelectedCoordinate, Tier};
use super::retrieval::Candidate;
use crate::kernel::error::Error;

pub struct Budgeted {
    pub content: String,
    pub selected: Vec<SelectedCoordinate>,
    pub excluded: Vec<ExcludedCoordinate>,
    pub used: usize,
    pub degraded: bool,
    pub required_overflow: usize,
}

struct Rendered {
    candidate: Candidate,
    text: String,
    units: usize,
}

pub fn apply(
    candidates: Vec<Candidate>,
    budget: &ContextBudget,
    mut excluded: Vec<ExcludedCoordinate>,
) -> Result<Budgeted, Error> {
    let mut required = Vec::new();
    let mut core = Vec::new();
    let mut relevant = Vec::new();
    for candidate in candidates {
        let rendered = render(candidate)?;
        match rendered.candidate.tier {
            Tier::Required => required.push(rendered),
            Tier::Core => core.push(rendered),
            Tier::Relevant => relevant.push(rendered),
        }
    }
    let content_limit = budget.total - budget.receipt_reserve;
    let mut picked = Vec::new();
    let mut used = 0;
    let mut degraded = false;
    let required_overflow = take(
        &mut required,
        budget.required_cap.min(content_limit),
        &mut used,
        &mut picked,
        &mut excluded,
        ExclusionReason::RequiredOverflow,
        &mut degraded,
    );
    let relevant_units: usize = relevant.iter().map(|item| item.units).sum();
    let protected = budget
        .relevant_floor
        .min(relevant_units)
        .min(content_limit.saturating_sub(used));
    let core_limit = budget
        .core_cap
        .min(content_limit.saturating_sub(used + protected));
    let mut ignored = false;
    let _ = take(
        &mut core,
        used + core_limit,
        &mut used,
        &mut picked,
        &mut excluded,
        ExclusionReason::CoreCap,
        &mut ignored,
    );
    let _ = take(
        &mut relevant,
        content_limit,
        &mut used,
        &mut picked,
        &mut excluded,
        ExclusionReason::TotalBudget,
        &mut ignored,
    );
    excluded.sort_by_key(|item| item.id);
    let content = picked
        .iter()
        .map(|item| item.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let selected = picked
        .into_iter()
        .map(|item| SelectedCoordinate {
            id: item.candidate.record.id,
            namespace: item.candidate.record.namespace,
            type_name: item.candidate.record.type_name,
            tier: item.candidate.tier,
            units: item.units,
            strategies: item.candidate.strategies,
        })
        .collect();
    Ok(Budgeted {
        content,
        selected,
        excluded,
        used,
        degraded,
        required_overflow,
    })
}

fn render(candidate: Candidate) -> Result<Rendered, Error> {
    let text = serde_json::to_string(&candidate.record.payload)?;
    let units = text.chars().count();
    Ok(Rendered {
        candidate,
        text,
        units,
    })
}

fn take(
    source: &mut Vec<Rendered>,
    limit: usize,
    used: &mut usize,
    picked: &mut Vec<Rendered>,
    excluded: &mut Vec<ExcludedCoordinate>,
    reason: ExclusionReason,
    degraded: &mut bool,
) -> usize {
    let mut dropped = 0;
    for item in source.drain(..) {
        if *used + item.units <= limit {
            *used += item.units;
            picked.push(item);
        } else {
            *degraded = true;
            dropped += 1;
            excluded.push(excluded_item(&item, reason));
        }
    }
    dropped
}

fn excluded_item(item: &Rendered, reason: ExclusionReason) -> ExcludedCoordinate {
    ExcludedCoordinate {
        id: item.candidate.record.id,
        namespace: item.candidate.record.namespace.clone(),
        type_name: item.candidate.record.type_name.clone(),
        reason,
    }
}
