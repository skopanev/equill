//! A selector that asked for an order means it for every way of printing.
//!
//! The JSON path returns the bundle's own content and kept the order for free.
//! The text path rebuilt the list by filtering the ledger, which returns
//! whatever order the ledger happens to hold — so a store whose records were
//! written out of order printed them out of order, while the same call in JSON
//! printed them correctly. Two answers to one question.
mod harness;
#[path = "context_order/support.rs"]
mod support;

use std::fs;
use std::process::Command;
use support::{run, store};

/// Written 0.3, 0.1, 0.2, so ledger order and rank order cannot agree by luck.
const WRITTEN: [f64; 3] = [0.3, 0.1, 0.2];
const ASCENDING: [&str; 3] = ["0.1", "0.2", "0.3"];

#[test]
fn default_context_is_strict_jsonl_without_blank_lines() {
    let root = store();
    let printed = run(&root, &["context", "--profile", "ranked"]);
    let lines = printed.lines().collect::<Vec<_>>();

    assert_eq!(lines.len(), WRITTEN.len());
    assert!(lines.iter().all(|line| !line.is_empty()));
    assert!(
        lines
            .iter()
            .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok())
    );
    let _ = fs::remove_dir_all(&root);
}

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

#[test]
fn context_and_search_accept_prompt_ready_output() {
    let root = store();
    let context = run(
        &root,
        &["context", "--profile", "ranked", "--format", "llm"],
    );
    let search = run(
        &root,
        &[
            "search",
            "--query",
            "step",
            "--strategy",
            "fts",
            "--format",
            "llm",
        ],
    );

    for printed in [context, search] {
        assert!(printed.starts_with("## RETRIEVED MEMORY\n- step"));
        assert!(!printed.contains("agent.lesson.v1"));
        assert!(!printed.contains("\"id\""));
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn runtime_budget_counts_the_exact_llm_stdout() {
    let root = store();
    let args = [
        "context",
        "--profile",
        "ranked",
        "--format",
        "llm",
        "--budget",
        "16",
    ];
    let printed = run(&root, &args);
    let body: serde_json::Value = serde_json::from_str(&run(
        &root,
        &[
            "context",
            "--profile",
            "ranked",
            "--format",
            "llm",
            "--budget",
            "16",
            "--json",
        ],
    ))
    .expect("json");
    let stdout = printed.trim_end();
    let tokens = tiktoken_rs::o200k_base_singleton().count_ordinary(stdout);

    assert!(tokens <= 16, "{tokens} tokens: {stdout}");
    assert_eq!(body["content"], stdout);
    assert_eq!(body["receipt"]["usage"]["content"], tokens);
    assert_eq!(body["receipt"]["effective_total_tokens"], 16);
    assert_eq!(body["receipt"]["budget"]["tokenizer"]["id"], "o200k_base");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn zero_runtime_budget_is_rejected_before_context_assembly() {
    let root = store();
    for flag in ["--budget", "--budget-records"] {
        let out = Command::new(harness::binary())
            .args(["context", "--profile", "ranked", flag, "0"])
            .arg("--store")
            .arg(&root)
            .env("EQUILL_ACTOR", "owner")
            .output()
            .expect("command");

        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains(flag));
    }
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
