//! A store that declares a word limit, and the paths a record can arrive by.
use crate::harness::{binary, write_json};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const LIMIT: usize = 5;

pub fn run(root: &Path, args: &[&str], actor: &str) -> Output {
    Command::new(binary())
        .args(args)
        .arg("--store")
        .arg(root)
        .env("EQUILL_ACTOR", actor)
        .output()
        .expect("command")
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

pub fn sentence(words: usize) -> String {
    vec!["word"; words].join(" ")
}

/// A sentence of `words` words carrying a marker that appears nowhere else, so
/// a search for it can only find this record.
pub fn marked(marker: &str, words: usize) -> String {
    let mut parts = vec![marker.to_string()];
    parts.extend(std::iter::repeat_n("word".to_string(), words - 1));
    parts.join(" ")
}

pub fn draft(rule: &str) -> Value {
    json!({
        "namespace": "agent.memory",
        "type": "agent.lesson.v1",
        "observed_at": "2026-01-01T00:00:00Z",
        "payload": { "rule": rule }
    })
}

/// A store whose settings declare the limit. `limited` false gives the same
/// store with no `records` section at all — the shape every settings file
/// written before today has.
pub fn store(name: &str, limited: bool) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-word-limits-{name}-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    let init = run(
        &root,
        &["init", "--owner", "owner", "--namespace", "agent.memory"],
        "owner",
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
    let register = run(
        &root,
        &[
            "schema",
            "register",
            "--file",
            schema.to_str().expect("path"),
        ],
        "owner",
    );
    assert!(
        register.status.success(),
        "schema failed: {}",
        stderr(&register)
    );
    if limited {
        set_limit(&root, LIMIT);
    }
    root
}

pub fn set_limit(root: &Path, max_words: usize) {
    write_json(
        &root.join("settings.json"),
        &json!({ "records": { "agent.lesson.v1": { "rule": { "max_words": max_words } } } }),
    );
}

pub fn clear_settings(root: &Path) {
    let _ = fs::remove_file(root.join("settings.json"));
}

/// Writes one record through `record --input` and returns the process result.
pub fn record(root: &Path, rule: &str, actor: &str) -> Output {
    let path = root.join(format!("draft-{}.json", uuid::Uuid::now_v7().simple()));
    write_json(&path, &draft(rule));
    run(
        root,
        &["record", "--input", path.to_str().expect("path")],
        actor,
    )
}

/// The same record through the batch path, which is the other way in: one
/// draft per line rather than one document.
pub fn batch(root: &Path, rules: &[&str], actor: &str) -> Output {
    let path = root.join(format!("batch-{}.jsonl", uuid::Uuid::now_v7().simple()));
    let lines = rules
        .iter()
        .map(|rule| draft(rule).to_string())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&path, format!("{lines}\n")).expect("batch input");
    run(
        root,
        &["record", "--input", path.to_str().expect("path"), "--json"],
        actor,
    )
}

/// The id the writer reports, so a test can act on the record it just wrote.
/// The plain output names it on the first line; asking for JSON here would
/// change the path under test.
pub fn written_id(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("Recorded "))
        .map(str::to_owned)
        .expect("the writer did not report an id")
}

/// A hand-built record wearing the revocation tag and pointing at a target:
/// what a caller could write if the exemption were reachable from a draft.
pub fn forged_revocation(root: &Path, target: &str, rule: &str, actor: &str) -> Output {
    let path = root.join(format!("forged-{}.json", uuid::Uuid::now_v7().simple()));
    let mut body = draft(rule);
    body["tags"] = json!(["equill:revoked"]);
    body["supersedes"] = json!(target);
    write_json(&path, &body);
    run(
        root,
        &["record", "--input", path.to_str().expect("path")],
        actor,
    )
}

/// An ordinary replacement: supersedes a record and says something new.
pub fn superseding(root: &Path, target: &str, rule: &str, actor: &str) -> Output {
    let path = root.join(format!("replace-{}.json", uuid::Uuid::now_v7().simple()));
    let mut body = draft(rule);
    body["supersedes"] = json!(target);
    write_json(&path, &body);
    run(
        root,
        &["record", "--input", path.to_str().expect("path")],
        actor,
    )
}
