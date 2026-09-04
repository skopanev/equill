use super::super::model::ExclusionReason;
use super::super::{assemble, assemble_with_budget, register_profile};
use super::fixtures::records::append;
use super::fixtures::registries::{registry, registry_unbounded};
use super::fixtures::support::{request, store};
use crate::command::doctor;
use crate::filter::Filter;
use std::fs;

#[test]
fn required_overflow_fails_context_and_doctor() {
    let root = store("overflow");
    registry(&root, 60, 10, &["exact", "tag"], "agent.memory");
    append(
        &root,
        "Mandatory policy too large for its cap",
        &["must"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let error = assemble(
        &root,
        "worker.v1",
        request(""),
        "test-owner",
        &Filter::default(),
    )
    .expect_err("required overflow must fail context");
    let report = doctor::report(Some(&root), true, false).expect("doctor");

    assert!(error.to_string().contains("CONTEXT_REQUIRED_OVERFLOW"));
    assert!(!report.ok);
    assert_eq!(report.context_profile_faults, 1);
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn context_budget_counts_and_emits_payload_only() {
    let root = store("payload-only");
    registry(&root, 500, 400, &["tag"], "agent.memory");
    append(
        &root,
        "Only payload enters context",
        &["must", "service-tag"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let bundle = assemble(
        &root,
        "worker.v1",
        request(""),
        "test-owner",
        &Filter::default(),
    )
    .expect("context");

    assert_eq!(bundle.content, r#"{"rule":"Only payload enters context"}"#);
    assert_eq!(
        bundle.receipt.usage.content,
        tiktoken_rs::o200k_base_singleton().count_ordinary(&bundle.content)
    );
    assert_eq!(bundle.receipt.schema, "equill.context-receipt.v3");
    assert_eq!(bundle.receipt.budget.tokenizer.id, "o200k_base");
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn relevant_floor_preserves_request_evidence_before_core() {
    let root = store("relevant-floor");
    registry(&root, 100, 50, &["exact", "tag"], "agent.memory");
    let core = append(
        &root,
        &"C".repeat(450),
        &["core"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let relevant = append(&root, "Needle evidence", &[], None, "2026-01-01T00:00:00Z");
    let bundle = assemble(
        &root,
        "worker.v1",
        request("needle"),
        "test-owner",
        &Filter::default(),
    )
    .expect("context");

    assert_eq!(bundle.selected_record_ids, vec![relevant]);
    assert!(
        bundle
            .receipt
            .excluded
            .iter()
            .any(|item| { item.id == core && item.reason == ExclusionReason::CoreCap })
    );
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn runtime_budget_lowers_but_never_raises_the_profile_cap() {
    let root = store("runtime-cap");
    registry(&root, 80, 60, &["tag"], "agent.memory");
    append(
        &root,
        "Required policy",
        &["must"],
        None,
        "2026-01-01T00:00:00Z",
    );
    let lowered = assemble_with_budget(
        &root,
        "worker.v1",
        request(""),
        "test-owner",
        &Filter::default(),
        Some(50),
    )
    .expect("lower runtime cap");
    let raised = assemble_with_budget(
        &root,
        "worker.v1",
        request(""),
        "test-owner",
        &Filter::default(),
        Some(500),
    )
    .expect("runtime cannot raise profile cap");

    assert_eq!(lowered.receipt.runtime_budget_tokens, Some(50));
    assert_eq!(lowered.receipt.effective_total_tokens, Some(50));
    assert_eq!(raised.receipt.effective_total_tokens, Some(80));
    assert!(lowered.receipt.usage.total <= 50);
    assert!(raised.receipt.usage.total <= 80);
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn absent_budget_returns_everything_and_never_overflows() {
    let root = store("unbounded");
    registry_unbounded(&root, &["exact", "tag"], "agent.memory");
    for index in 0..12 {
        append(
            &root,
            &format!("Mandatory policy {index}"),
            &["must"],
            None,
            "2026-01-01T00:00:00Z",
        );
    }
    let bundle = assemble(
        &root,
        "worker.v1",
        request(""),
        "test-owner",
        &Filter::default(),
    )
    .expect("a profile without caps must never fail on volume");
    let report = doctor::report(Some(&root), true, false).expect("doctor");

    assert_eq!(bundle.receipt.included.len(), 12);
    assert!(
        !bundle
            .receipt
            .excluded
            .iter()
            .any(|item| matches!(item.reason, ExclusionReason::RequiredOverflow))
    );
    assert!(!bundle.receipt.degraded);
    assert_eq!(report.context_profile_faults, 0);
    fs::remove_dir_all(root).expect("remove store");
}

#[test]
fn legacy_budget_names_load_as_tokens_with_the_pinned_default() {
    let budget: super::super::model::ContextBudget = serde_json::from_value(serde_json::json!({
        "total": 100,
        "required_cap": 80,
        "core_cap": 60,
        "relevant_floor": 20,
        "receipt_reserve": 10
    }))
    .expect("legacy budget");
    let canonical = serde_json::to_value(&budget).expect("canonical budget");

    assert_eq!(budget.total, Some(100));
    assert_eq!(budget.tokenizer.id, "o200k_base");
    assert_eq!(canonical["total_tokens"], 100);
    assert!(canonical.get("total").is_none());
}

#[test]
fn profile_registration_refuses_an_unpinned_tokenizer() {
    let root = store("unknown-tokenizer");
    let profile = root.join("bad-profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&serde_json::json!({
            "id": "bad.v1",
            "version": "1",
            "actors": [],
            "grants": [{"namespace": "agent.memory", "types": ["agent.lesson.v1"]}],
            "selectors": ["agent.lesson.inject.v1"],
            "budget": {
                "total_tokens": 100,
                "tokenizer": {"id": "unknown", "version": "1"}
            }
        }))
        .expect("profile json"),
    )
    .expect("profile file");

    let error = register_profile(&root, &profile, "test-owner").expect_err("tokenizer");
    assert!(error.to_string().contains("unsupported context tokenizer"));
    fs::remove_dir_all(root).expect("remove store");
}
