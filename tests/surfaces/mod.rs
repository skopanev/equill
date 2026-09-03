//! The three doors onto one selection, and the store they are asked about.
//!
//! Kept apart from the assertions so each file stays readable and under the
//! line limit: what a surface returns is one job, what the fixture holds is
//! another, and the tests are a third.
#![allow(dead_code)]

pub mod fixture;

use crate::harness::equill;
use crate::harness::session::Session;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

/// What one surface answered: which records it chose, and the digest it
/// published for the bundle it built from them.
///
/// The digest is carried because agreeing on a set of ids is not the same as
/// agreeing on an answer. Two surfaces can name the same records and still
/// disagree about what they assembled, and a receipt nobody compares is a
/// receipt nobody is holding to anything.
#[derive(Debug, PartialEq, Eq)]
pub struct Answer {
    pub ids: BTreeSet<String>,
    pub bundle_digest: String,
}

impl Answer {
    fn read(answer: &Value) -> Self {
        Self {
            ids: answer["selected_record_ids"]
                .as_array()
                .expect("selected ids")
                .iter()
                .map(|id| id.as_str().expect("id").to_owned())
                .collect(),
            bundle_digest: answer["bundle_digest"]
                .as_str()
                .expect("bundle digest")
                .to_owned(),
        }
    }
}

/// What the CLI answers in JSON: the records it chose and the digest it
/// published for the bundle.
/// The receipt exactly as the CLI prints it, for assertions about the document
/// itself rather than about the two fields `Answer` keeps.
pub fn cli_json_value(root: &Path, coordinates: &[&str]) -> serde_json::Value {
    let mut args = vec!["context", "--profile", "roles", "--json"];
    for entry in coordinates {
        args.push("--coordinate");
        args.push(entry);
    }
    let out = equill(root, &args);
    assert!(
        out.status.success(),
        "cli json failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("cli json")
}

pub fn cli_json(root: &Path, coordinates: &[&str]) -> Answer {
    let mut args = vec!["context", "--profile", "roles", "--json"];
    for entry in coordinates {
        args.push("--coordinate");
        args.push(entry);
    }
    let out = equill(root, &args);
    assert!(
        out.status.success(),
        "cli json failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Answer::read(&serde_json::from_slice(&out.stdout).expect("cli json"))
}

/// What a text answer names. The text surface no longer prints a record id —
/// a reader was being handed a UUID they could not use — so the comparison
/// runs on the title each synthetic record carries, which is unique per record
/// in this fixture and therefore names it as exactly as the id did.
pub fn cli_text(root: &Path, coordinates: &[&str]) -> BTreeSet<String> {
    let mut args = vec!["context", "--profile", "roles", "--format", "text"];
    for entry in coordinates {
        args.push("--coordinate");
        args.push(entry);
    }
    let out = equill(root, &args);
    assert!(
        out.status.success(),
        "cli text failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("Title: "))
        .map(|title| title.trim().to_owned())
        .collect()
}

/// The same records the receipt names, read back by the title that identifies
/// them — so "the two surfaces chose the same records" stays a claim about the
/// records themselves and not about a rendering.
pub fn titles_of(root: &Path, ids: &BTreeSet<String>) -> BTreeSet<String> {
    ids.iter()
        .map(|id| {
            let out = equill(
                root,
                &["get", "--id", id, "--format", "jsonl", "--fields", "title"],
            );
            assert!(
                out.status.success(),
                "get {id} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let value: serde_json::Value =
                serde_json::from_slice(out.stdout.trim_ascii()).expect("record json");
            value["title"].as_str().expect("title").to_owned()
        })
        .collect()
}

/// The same, through a real MCP session rather than a helper.
pub fn mcp(root: &Path, coordinates: &[&str]) -> Answer {
    let mut session = Session::open(root);
    let (_, response) = session.tool(
        "context",
        json!({
            "profile": "roles",
            "coordinates": coordinates.iter().map(|item| json!(item)).collect::<Vec<_>>()
        }),
    );
    assert!(
        response["error"].is_null(),
        "mcp context failed: {response}"
    );
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("mcp text");
    Answer::read(&serde_json::from_str(text).expect("mcp json"))
}
