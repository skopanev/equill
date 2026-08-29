//! Selector and profile registration for context tests: what a worker is
//! allowed to see and how much of it fits.
use super::super::{register_profile, register_selector};
use serde_json::json;
use std::fs;
use std::path::Path;

pub fn registry(
    root: &Path,
    total: usize,
    required_cap: usize,
    strategies: &[&str],
    grant_namespace: &str,
) {
    registry_with_modes(
        root,
        total,
        required_cap,
        strategies,
        grant_namespace,
        json!({}),
    );
}

pub fn registry_with_modes(
    root: &Path,
    total: usize,
    required_cap: usize,
    strategies: &[&str],
    grant_namespace: &str,
    coordinate_modes: serde_json::Value,
) {
    registry_with_options(
        root,
        total,
        required_cap,
        strategies,
        grant_namespace,
        coordinate_modes,
        None,
    );
}

pub fn registry_with_rank(
    root: &Path,
    total: usize,
    required_cap: usize,
    strategies: &[&str],
    rank_pointer: &str,
) {
    registry_with_options(
        root,
        total,
        required_cap,
        strategies,
        "agent.memory",
        json!({}),
        Some(rank_pointer),
    );
}

fn registry_with_options(
    root: &Path,
    total: usize,
    required_cap: usize,
    strategies: &[&str],
    grant_namespace: &str,
    coordinate_modes: serde_json::Value,
    rank_pointer: Option<&str>,
) {
    let core_cap = total.saturating_sub(20);
    let relevant_floor = (total / 4).min(500);
    let selector = root.join("selector.json");
    let mut definition = json!({
        "id": "agent.lesson.inject.v1",
        "version": "1",
        "type": "agent.lesson.v1",
        "strategies": strategies,
        "required_tags": ["must"],
        "core_tags": ["core"],
        "coordinate_pointers": { "scope": "/scope" },
        "coordinate_modes": coordinate_modes
    });
    if let Some(pointer) = rank_pointer {
        definition["rank_pointer"] = json!(pointer);
    }
    fs::write(
        &selector,
        serde_json::to_vec(&definition).expect("selector json"),
    )
    .expect("selector file");
    register_selector(root, &selector, "test-owner").expect("register selector");
    let profile = root.join("profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&json!({
            "id": "worker.v1",
            "version": "1",
            "actors": [],
            "grants": [{ "namespace": grant_namespace, "types": ["agent.lesson.v1"] }],
            "selectors": ["agent.lesson.inject.v1"],
            "budget": {
                "total": total,
                "required_cap": required_cap,
                "core_cap": core_cap,
                "relevant_floor": relevant_floor,
                "receipt_reserve": 20
            }
        }))
        .expect("profile json"),
    )
    .expect("profile file");
    register_profile(root, &profile, "test-owner").expect("register profile");
}

/// Profile with no budget block at all — every bound absent. Nothing is capped
/// and the required tier can never overflow.
pub fn registry_unbounded(root: &Path, strategies: &[&str], grant_namespace: &str) {
    let selector = root.join("selector.json");
    fs::write(
        &selector,
        serde_json::to_vec(&json!({
            "id": "agent.lesson.inject.v1",
            "version": "1",
            "type": "agent.lesson.v1",
            "strategies": strategies,
            "required_tags": ["must"],
            "core_tags": ["core"],
            "coordinate_pointers": { "scope": "/scope" },
            "coordinate_modes": {}
        }))
        .expect("selector json"),
    )
    .expect("selector file");
    register_selector(root, &selector, "test-owner").expect("register selector");
    let profile = root.join("profile.json");
    fs::write(
        &profile,
        serde_json::to_vec(&json!({
            "id": "worker.v1",
            "version": "1",
            "actors": [],
            "grants": [{ "namespace": grant_namespace, "types": ["agent.lesson.v1"] }],
            "selectors": ["agent.lesson.inject.v1"]
        }))
        .expect("profile json"),
    )
    .expect("profile file");
    register_profile(root, &profile, "test-owner").expect("register profile");
}
