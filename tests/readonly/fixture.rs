//! The store these tests ask questions of, and how they look at it.
//!
//! A store that grants everyone with a wildcard and then names the one actor
//! that may not append: without the wildcard the refusal under test would never
//! be reached, and the tests would pass on an actor who simply was not a writer.
use crate::harness;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const READER: &str = "pm";
pub fn existing_record(root: &Path) -> String {
    let contents = fs::read_dir(root.join("records"))
        .expect("records")
        .filter_map(Result::ok)
        .find_map(|entry| fs::read_to_string(entry.path()).ok())
        .expect("a ledger");
    let line = contents.lines().next().expect("a record");
    serde_json::from_str::<serde_json::Value>(line).expect("json")["id"]
        .as_str()
        .expect("id")
        .to_owned()
}

pub fn run(root: &Path, actor: &str, args: &[&str]) -> Output {
    Command::new(harness::binary())
        .args(args)
        .arg("--store")
        .arg(root)
        .env("EQUILL_ACTOR", actor)
        .output()
        .expect("command")
}

pub fn write(root: &Path, name: &str, value: serde_json::Value) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, serde_json::to_vec(&value).expect("json")).expect("write");
    path
}

/// A store with a schema, a profile, one record — and no grants at all.
pub fn plain_store() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-read-only-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    let _ = fs::remove_dir_all(&root);
    let out = run(
        &root,
        "owner",
        &["init", "--owner", "owner", "--namespace", "agent.memory"],
    );
    assert!(out.status.success(), "init failed");
    let schema = write(
        &root,
        "schema.json",
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "equill://agent.lesson/v1",
            "type": "object",
            "required": ["rule"],
            "additionalProperties": false,
            "properties": { "rule": { "type": "string" } },
            "x-equill-envelope": { "namespace": "agent.memory", "type": "agent.lesson.v1" }
        }),
    );
    let out = run(
        &root,
        "owner",
        &[
            "schema",
            "register",
            "--file",
            schema.to_str().expect("path"),
        ],
    );
    assert!(out.status.success(), "schema register failed");
    let seed = write(
        &root,
        "seed.json",
        json!({
            "namespace": "agent.memory",
            "type": "agent.lesson.v1",
            "observed_at": "2026-01-01T00:00:00Z",
            "payload": { "rule": "a lesson that already exists" }
        }),
    );
    let out = run(
        &root,
        "owner",
        &["record", "--input", seed.to_str().expect("path")],
    );
    assert!(out.status.success(), "seed failed");

    // A profile, so that reading through context can be shown to still work.
    let selector = write(
        &root,
        "selector.json",
        json!({
            "id": "lessons.v1", "version": "1", "type": "agent.lesson.v1",
            "strategies": ["recency"]
        }),
    );
    let out = run(
        &root,
        "owner",
        &[
            "selector",
            "register",
            "--file",
            selector.to_str().expect("path"),
        ],
    );
    assert!(out.status.success(), "selector register failed");
    let profile = write(
        &root,
        "profile.json",
        json!({
            // Named, because a non-root actor may only use a profile that
            // lists it — which is the store's own rule and nothing to do with
            // the refusal under test.
            "id": "reading", "version": "1", "actors": [READER],
            "grants": [{ "namespace": "agent.memory", "types": ["agent.lesson.v1"] }],
            "selectors": ["lessons.v1"], "budget": {}
        }),
    );
    let out = run(
        &root,
        "owner",
        &[
            "profile",
            "register",
            "--file",
            profile.to_str().expect("path"),
        ],
    );
    assert!(out.status.success(), "profile register failed");

    root
}

/// The same store, opened to everyone and holding one actor to reading.
///
/// Some refusals have to be proved on the plain store instead: one carrying a
/// wildcard grant refuses an ownership handover for a reason of its own, and a
/// test that ran there would pass while the door it names stood open.
pub fn store() -> PathBuf {
    let root = plain_store();
    // The store opens itself to everyone. Without the wildcard the refusal
    // under test would never be reached, and the tests would pass on an actor
    // who simply was not a writer.
    let out = run(
        &root,
        "owner",
        &[
            "grant",
            "add",
            "--actor",
            "*",
            "--namespace",
            "*",
            "--types",
            "*",
        ],
    );
    assert!(
        out.status.success(),
        "wildcard grant failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // And then names the one actor held to reading — through the governed
    // command, not by editing the file, because a test that configures by hand
    // proves nothing about whether the store can be configured at all.
    let out = run(
        &root,
        "owner",
        &["reader", "add", "--actor", READER, "--comment", "read-only"],
    );
    assert!(
        out.status.success(),
        "reader add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    root
}
