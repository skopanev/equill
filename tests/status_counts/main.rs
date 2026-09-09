//! What `status` reports, and what it refuses to do while reporting it.
#[path = "../harness/mod.rs"]
mod harness;

use harness::{binary, write_json};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

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
///
/// The premise — a quiescent store — is not free: every append starts catch-up
/// work that finishes on its own schedule, and the setup's `rebuild` does not
/// wait for the appends' worker. A worker mid-flight leaves exactly the files
/// this test once raced on: a handoff ticket with its active claim while the
/// worker is claimed or starting, and a sqlite rollback journal while the text
/// index catches up inside it. Fingerprinting while any of that is live makes
/// "before" a picture of work in progress, and the worker finishing between the
/// two snapshots reads as `status` rewriting the store.
///
/// The guard waits, bounded, for every observable of that work to be gone:
/// both handoff markers, every sqlite sidecar file, a free drain lock, and the
/// text index level with its published target — the product's own freshness
/// comparison, read from the two markers it publishes. `settles` alone is not
/// enough: it probes the drain lock, and a worker between claiming the handoff
/// and taking the lock (or after dropping either) is invisible to it.
#[test]
fn status_changes_no_file_in_the_store() {
    let root = store("read-only");
    let first = record(&root, "one", None);
    record(&root, "two", Some(&first));
    assert!(run(&root, &["rebuild"]).status.success());
    assert!(
        setup_settles(&root),
        "the setup's catch-up work never finished; refusing to fingerprint a \
         store that is still changing for reasons unrelated to status"
    );

    let before = fingerprint(&root);
    assert!(run(&root, &["status"]).status.success());
    assert!(run(&root, &["status", "--json"]).status.success());
    let after = fingerprint(&root);

    assert_eq!(before, after, "status rewrote something while reporting");
    let _ = std::fs::remove_dir_all(&root);
}

/// Bounded observation of the setup's own background work, by store artifacts
/// only: no process scans, no arbitrary sleep, no product behaviour relied on
/// beyond the markers it already publishes.
fn setup_settles(root: &Path) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let handoffs_gone = [
            "projections/qdrant/handoff.json",
            "projections/qdrant/handoff-active.json",
        ]
        .iter()
        .all(|marker| !root.join(marker).exists());
        let sidecars_gone = std::fs::read_dir(root.join("projections/sqlite"))
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .all(|entry| !entry.file_name().to_string_lossy().ends_with("-journal"))
            })
            .unwrap_or(false);
        let text_current = text_level_with_target(root);
        let drain_idle = harness::settles(root, std::time::Duration::from_millis(1));
        if handoffs_gone && sidecars_gone && text_current && drain_idle {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The text index's own freshness condition: the watermark it published says it
/// covered every record and every byte the ledger target claims. Absent
/// markers mean the pass has not finished, which is exactly what the guard is
/// waiting for.
fn text_level_with_target(root: &Path) -> bool {
    let read = |path: &str| -> Option<serde_json::Value> {
        serde_json::from_slice(&std::fs::read(root.join(path)).ok()?).ok()
    };
    let Some(watermark) = read("projections/sqlite/watermark.json") else {
        return false;
    };
    let Some(target) = read("projections/target.json") else {
        return false;
    };
    watermark.get("indexed_records") == target.get("records")
        && watermark.get("ledger_bytes") == target.get("ledger_bytes")
}
