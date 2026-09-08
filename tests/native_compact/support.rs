//! A store written record by record, with a chain to collapse.
use crate::harness::{binary, write_json};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .arg("--store")
        .arg(root)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("command")
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Writes one record and returns the id the writer reports.
pub fn record(root: &Path, rule: &str, supersedes: Option<&str>) -> String {
    let path = root.join(format!("draft-{}.json", uuid::Uuid::now_v7().simple()));
    let mut draft = json!({
        "namespace": "agent.memory",
        "type": "agent.lesson.v1",
        "observed_at": "2026-01-01T00:00:00Z",
        "payload": { "rule": rule }
    });
    if let Some(target) = supersedes {
        draft["supersedes"] = json!(target);
    }
    write_json(&path, &draft);
    let out = run(root, &["record", "--input", path.to_str().expect("path")]);
    assert!(out.status.success(), "record failed: {}", stderr(&out));
    stdout(&out)
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("Recorded "))
        .map(str::to_owned)
        .expect("id")
}

pub fn ledger_lines(root: &Path) -> Vec<Value> {
    let directory = root.join("records");
    let mut lines = Vec::new();
    for entry in std::fs::read_dir(directory).expect("records") {
        let path = entry.expect("entry").path();
        for line in std::fs::read_to_string(&path).expect("ledger").lines() {
            if !line.trim().is_empty() {
                lines.push(serde_json::from_str(line).expect("record"));
            }
        }
    }
    lines
}

pub fn store(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-native-compact-{name}-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let init = run(
        &root,
        &["init", "--owner", "owner", "--namespace", "agent.memory"],
    );
    assert!(init.status.success(), "init failed: {}", stderr(&init));
    let schema = root.join("schema.json");
    write_json(
        &schema,
        &json!({
            "type": "agent.lesson.v1",
            "uri": "equill://agent.lesson/v1",
            "owner": "owner",
            "payload_schema": {
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }
        }),
    );
    let registered = run(
        &root,
        &[
            "schema",
            "register",
            "--file",
            schema.to_str().expect("path"),
        ],
    );
    assert!(
        registered.status.success(),
        "schema failed: {}",
        stderr(&registered)
    );
    root
}
