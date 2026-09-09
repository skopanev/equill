//! What `status` reports, and what it refuses to do while reporting it.
#[path = "../harness/mod.rs"]
mod harness;

use harness::{binary, write_json};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .arg("--store")
        .arg(root)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("command")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Every file under the store, by path and content hash.
fn fingerprint(root: &Path) -> Vec<(String, String)> {
    let mut seen = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).expect("read") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let bytes = std::fs::read(&path).expect("file");
                seen.push((
                    path.strip_prefix(root)
                        .expect("relative")
                        .display()
                        .to_string(),
                    equill::kernel::digest::sha256_hex(&bytes),
                ));
            }
        }
    }
    seen.sort();
    seen
}

fn store(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-status-{name}-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let init = run(
        &root,
        &["init", "--owner", "owner", "--namespace", "agent.memory"],
    );
    assert!(init.status.success());
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
    assert!(
        run(
            &root,
            &[
                "schema",
                "register",
                "--file",
                schema.to_str().expect("path")
            ]
        )
        .status
        .success()
    );
    root
}

fn record(root: &Path, rule: &str, supersedes: Option<&str>) -> String {
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
    assert!(out.status.success());
    stdout(&out)
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("Recorded "))
        .map(str::to_owned)
        .expect("id")
}

/// A store with a chain and a withdrawal, read back through the command.
#[test]
fn the_counts_describe_the_ledger_a_reader_would_find() {
    let root = store("counts");
    let first = record(&root, "one", None);
    record(&root, "two", Some(&first));
    let third = record(&root, "three", None);
    assert!(run(&root, &["revoke", "--id", &third]).status.success());

    let text = stdout(&run(&root, &["status"]));

    assert!(
        text.contains("Records: 4 total, 1 live, 3 dead"),
        "unexpected ledger line:\n{text}"
    );
    assert!(
        text.contains("Dead: 2 superseded, 1 revoked, 0 both"),
        "unexpected dead line:\n{text}"
    );

    let body: serde_json::Value =
        serde_json::from_str(&stdout(&run(&root, &["status", "--json"]))).expect("json");
    assert_eq!(body["store"]["ledger_records"], 4);
    assert_eq!(body["store"]["ledger_live"], 1);
    assert_eq!(body["store"]["ledger_dead"], 3);
    let _ = std::fs::remove_dir_all(&root);
}

/// Reporting is not doing. Nothing under the store changes, including the
/// projections it describes.
#[test]
fn status_changes_no_file_in_the_store() {
    let root = store("read-only");
    let first = record(&root, "one", None);
    record(&root, "two", Some(&first));
    assert!(run(&root, &["rebuild"]).status.success());

    let before = fingerprint(&root);
    assert!(run(&root, &["status"]).status.success());
    assert!(run(&root, &["status", "--json"]).status.success());
    let after = fingerprint(&root);

    assert_eq!(before, after, "status rewrote something while reporting");
    let _ = std::fs::remove_dir_all(&root);
}
