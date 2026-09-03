//! A selector that asked for an order means it for every way of printing.
//!
//! The JSON path returns the bundle's own content and kept the order for free.
//! The text path rebuilt the list by filtering the ledger, which returns
//! whatever order the ledger happens to hold — so a store whose records were
//! written out of order printed them out of order, while the same call in JSON
//! printed them correctly. Two answers to one question.
mod harness;

use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Written 0.3, 0.1, 0.2, so ledger order and rank order cannot agree by luck.
const WRITTEN: [f64; 3] = [0.3, 0.1, 0.2];
const ASCENDING: [&str; 3] = ["0.1", "0.2", "0.3"];

#[test]
fn text_output_follows_the_selector_order_not_the_ledger() {
    let root = store();
    let printed = run(
        &root,
        &["context", "--profile", "ranked", "--format", "text"],
    );
    // Read back by content, because the text answer no longer prints an
    // identifier: a reader was being handed a UUID they could not use, and the
    // line it led was unreadable for it. Confidence is unique per record in
    // this fixture, so it names the record as exactly as the id did.
    let order = confidences(&printed);
    assert_eq!(
        order, ASCENDING,
        "text printed the ledger's order, not the selector's"
    );

    // And the two surfaces still agree, which is the point of fixing the one
    // that was wrong rather than loosening the assertion. Compared through the
    // same records rather than through ids, which only one surface now shows.
    let lines = run(
        &root,
        &[
            "context",
            "--profile",
            "ranked",
            "--format",
            "jsonl",
            "--fields",
            "confidence",
        ],
    );
    let structured: Vec<String> = lines
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|record| {
            record["confidence"]
                .as_f64()
                .or_else(|| record["payload"]["confidence"].as_f64())
        })
        .map(|value| value.to_string())
        .collect();
    assert_eq!(structured, order, "text and jsonl disagree on the order");

    let body: serde_json::Value =
        serde_json::from_str(&run(&root, &["context", "--profile", "ranked", "--json"]))
            .expect("json");
    assert_eq!(
        body["selected_record_ids"].as_array().expect("ids").len(),
        order.len(),
        "the receipt and the printed answer disagree on how many records there are"
    );
    let _ = fs::remove_dir_all(&root);
}

/// The value each record is known by in this fixture, in the order printed.
fn confidences(printed: &str) -> Vec<String> {
    printed
        .lines()
        .filter_map(|line| line.strip_prefix("Confidence: "))
        .map(str::to_owned)
        .collect()
}

fn run(root: &Path, args: &[&str]) -> String {
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

fn store() -> PathBuf {
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
    let schema = write(
        &root,
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
    );
    run(
        &root,
        &[
            "schema",
            "register",
            "--file",
            schema.to_str().expect("path"),
        ],
    );
    let selector = write(
        &root,
        "selector.json",
        json!({
            "id": "ranked.v1", "version": "1", "type": "agent.lesson.v1",
            "strategies": ["recency"], "rank_pointer": "/confidence", "rank_order": "asc"
        }),
    );
    run(
        &root,
        &[
            "selector",
            "register",
            "--file",
            selector.to_str().expect("path"),
        ],
    );
    let profile = write(
        &root,
        "profile.json",
        json!({
            "id": "ranked", "version": "1", "actors": [],
            "grants": [{ "namespace": "agent.memory", "types": ["agent.lesson.v1"] }],
            "selectors": ["ranked.v1"], "budget": {}
        }),
    );
    run(
        &root,
        &[
            "profile",
            "register",
            "--file",
            profile.to_str().expect("path"),
        ],
    );
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
