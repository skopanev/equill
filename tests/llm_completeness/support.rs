//! A synthetic store whose records exercise every way the formatter used to
//! lose one. Permissive schemas on purpose: the shapes that vanished are
//! shapes a real domain schema forbids, so a fixture bound by those schemas
//! could not reach the defect.
use crate::harness;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run(root: &Path, args: &[&str]) -> String {
    let out = Command::new(harness::binary())
        .args(args)
        .arg("--store")
        .arg(root)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("command");
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write(root: &Path, name: &str, value: serde_json::Value) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, serde_json::to_vec(&value).expect("json")).expect("write");
    path
}

/// One record per way of disappearing, each carrying a marker that appears
/// nowhere else, plus a pair that reads identically and means different things.
pub fn records() -> Vec<serde_json::Value> {
    vec![
        // A type the formatter has no shape for.
        json!({ "type": "sample.registry.v1", "payload": { "project": "sample-alpha", "lane_limit": 4 } }),
        // The same, with facts that render as nothing if absence is mishandled.
        json!({ "type": "sample.registry.v1", "payload": { "project": null, "lane_limit": 0, "active": false, "lanes": [], "marker": "sample-beta" } }),
        // Recognized category, but reached only if an unknown module stops
        // vetoing a known `rules`.
        json!({ "type": "sample.rule.v1", "payload": { "module": "process", "rules": "comm", "rule": "sample-gamma" } }),
        // Recognized branches that produce nothing.
        json!({ "type": "sample.role.v1", "payload": { "role": "reviewer", "why": "sample-delta" } }),
        json!({ "type": "sample.process.v1", "payload": { "title": "sample-epsilon" } }),
        // A step the renderer drops for having no instruction.
        json!({ "type": "sample.step.v1", "payload": { "step": 7, "gate": "sample-zeta" } }),
        // Two lessons that read the same and hold for different projects.
        // Their scopes appear nowhere else in this fixture on purpose: with
        // shared values, an unrelated row carrying the same word satisfies the
        // assertion while the two lessons stay indistinguishable, and the test
        // passes having measured nothing.
        json!({ "type": "sample.lesson.v1", "payload": { "rule": "Measure before claiming.", "project": "scope-only-one" } }),
        json!({ "type": "sample.lesson.v1", "payload": { "rule": "Measure before claiming.", "project": "scope-only-two" } }),
    ]
}

pub const MARKERS: [&str; 6] = [
    "sample-alpha",
    "sample-beta",
    "sample-gamma",
    "sample-delta",
    "sample-epsilon",
    "sample-zeta",
];

pub fn store() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-llm-completeness-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    let _ = fs::remove_dir_all(&root);
    run(
        &root,
        &["init", "--owner", "owner", "--namespace", "agent.memory"],
    );
    register(&root);
    for (index, record) in records().into_iter().enumerate() {
        let mut draft = record;
        draft["namespace"] = json!("agent.memory");
        draft["observed_at"] = json!("2026-01-01T00:00:00Z");
        let path = write(&root, &format!("draft-{index}.json"), draft);
        run(&root, &["record", "--input", path.to_str().expect("path")]);
    }
    root
}

fn register(root: &Path) {
    let types = [
        "sample.registry.v1",
        "sample.rule.v1",
        "sample.role.v1",
        "sample.process.v1",
        "sample.step.v1",
        "sample.lesson.v1",
    ];
    for name in types {
        let short = name.trim_end_matches(".v1");
        let path = write(
            root,
            &format!("{name}.schema.json"),
            json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$id": format!("equill://{short}/v1"),
                "type": "object",
                "additionalProperties": true,
                "x-equill-envelope": { "namespace": "agent.memory", "type": name }
            }),
        );
        run(
            root,
            &["schema", "register", "--file", path.to_str().expect("path")],
        );
        let selector = write(
            root,
            &format!("{name}.selector.json"),
            json!({ "id": name, "version": "1", "type": name, "strategies": ["recency"] }),
        );
        run(
            root,
            &[
                "selector",
                "register",
                "--file",
                selector.to_str().expect("path"),
            ],
        );
    }
    let profile = write(
        root,
        "profile.json",
        json!({
            "id": "sample", "version": "1", "actors": [],
            "grants": [{ "namespace": "agent.memory", "types": types }],
            "selectors": types, "budget": {}
        }),
    );
    run(
        root,
        &[
            "profile",
            "register",
            "--file",
            profile.to_str().expect("path"),
        ],
    );
}
