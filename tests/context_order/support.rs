use crate::harness;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const WRITTEN: [f64; 3] = [0.3, 0.1, 0.2];

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

pub fn store() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-context-order-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    let _ = fs::remove_dir_all(&root);
    run(
        &root,
        &["init", "--owner", "owner", "--namespace", "agent.memory"],
    );
    register(&root);
    for confidence in WRITTEN {
        let draft = write(
            &root,
            &format!("draft-{confidence}.json"),
            json!({
                "namespace": "agent.memory",
                "type": "agent.lesson.v1",
                "observed_at": "2026-01-01T00:00:00Z",
                "payload": { "rule": format!("step {confidence}"), "confidence": confidence }
            }),
        );
        run(&root, &["record", "--input", draft.to_str().expect("path")]);
    }
    root
}

fn register(root: &Path) {
    for (command, name, body) in [
        (
            "schema",
            "schema.json",
            json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$id": "equill://agent.lesson/v1",
                "type": "object",
                "required": ["rule"],
                "additionalProperties": false,
                "properties": { "rule": { "type": "string" }, "confidence": { "type": "number" } },
                "x-equill-envelope": { "namespace": "agent.memory", "type": "agent.lesson.v1" }
            }),
        ),
        (
            "selector",
            "selector.json",
            json!({
                "id": "ranked.v1", "version": "1", "type": "agent.lesson.v1",
                "strategies": ["recency"], "rank_pointer": "/confidence", "rank_order": "asc"
            }),
        ),
        (
            "profile",
            "profile.json",
            json!({
                "id": "ranked", "version": "1", "actors": [],
                "grants": [{ "namespace": "agent.memory", "types": ["agent.lesson.v1"] }],
                "selectors": ["ranked.v1"], "budget": {}
            }),
        ),
    ] {
        let path = write(root, name, body);
        run(
            root,
            &[command, "register", "--file", path.to_str().expect("path")],
        );
    }
}
