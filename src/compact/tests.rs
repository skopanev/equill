use super::model::CompactReason;
use super::{planner, run};
use crate::command::init;
use crate::{ingest, integrity, schema};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

#[test]
fn dry_run_is_read_only_and_apply_rebuilds_the_manifest() {
    let root = store("apply");
    register_schema(&root);
    let source = root.join("source");
    fs::create_dir(&source).expect("source directory");
    let one = source.join("one.jsonl");
    let two = source.join("two.jsonl");
    fs::write(
        &one,
        format!(
            "{}{}",
            line("old", None, None, &[]),
            line("live", Some("old"), None, &[])
        ),
    )
    .expect("first input");
    fs::write(
        &two,
        format!(
            "{}{}{}{}{}",
            line("expired", None, Some("2020-01-01T00:00:00Z"), &[]),
            line("warning", None, Some("2026-08-10T00:00:00Z"), &[]),
            line("dead", None, None, &["anchor:ticket:closed-ticket"]),
            line("active", None, None, &["anchor:ticket:open-ticket"]),
            line("unknown", None, None, &["anchor:ticket:unknown-ticket"]),
        ),
    )
    .expect("second input");
    fs::write(
        source.join("anchors.jsonl"),
        concat!(
            "{\"kind\":\"anchor:ticket\",\"target\":\"closed-ticket\",\"state\":\"dead\"}\n",
            "{\"kind\":\"anchor:ticket\",\"target\":\"open-ticket\",\"state\":\"alive\"}\n"
        ),
    )
    .expect("anchors");
    let manifest = source.join("inputs.jsonl");
    let policy = "\"expiry\":{\"pointer\":\"/expires_at\",\"warning_days\":30},\"anchor_resolver\":\"anchors.jsonl\"";
    fs::write(&manifest, format!(
        "{{\"path\":\"one.jsonl\",\"role\":\"rules\",{policy}}}\n{{\"path\":\"two.jsonl\",\"role\":\"lessons\",{policy}}}\n"
    )).expect("manifest");
    ingest::import_manifest(&root, &manifest, "test-owner").expect("initial import");
    let before = snapshot(&root, &[&one, &two]);

    let dry = planner::build(
        &root,
        &manifest,
        "2026-08-27T00:00:00Z".parse().expect("time"),
    )
    .expect("dry plan");
    assert_eq!(
        dry.inputs
            .iter()
            .map(|input| input.public.removals.len())
            .sum::<usize>(),
        3
    );
    assert!(
        dry.inputs[0]
            .public
            .retained
            .iter()
            .any(|item| item.reason == CompactReason::ActiveDescendant)
    );
    assert!(
        dry.inputs[1]
            .public
            .retained
            .iter()
            .any(|item| item.reason == CompactReason::UnknownAnchor)
    );
    assert!(
        dry.inputs[1]
            .public
            .retained
            .iter()
            .any(|item| item.reason == CompactReason::ActiveAnchor)
    );
    assert!(
        dry.inputs[1]
            .public
            .retained
            .iter()
            .any(|item| item.reason == CompactReason::ExpiryWarningWindow)
    );
    assert_eq!(before, snapshot(&root, &[&one, &two]));

    let applied = run(&root, &manifest, true, "test-owner").expect("apply");
    let scan = integrity::scan(&root).expect("rebuilt store");
    let repeat = run(&root, &manifest, false, "test-owner").expect("repeat dry run");
    assert_eq!(applied.removed, 3);
    assert_eq!((scan.records, scan.projection_records), (4, 4));
    assert_eq!(repeat.removed, 0);
    assert!(root.join(applied.receipt.expect("receipt")).is_file());
    fs::remove_dir_all(root).expect("remove store");
}

fn store(name: &str) -> PathBuf {
    let suffix = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "equill-compact-{name}-{}-{suffix}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    init::create(&root, "test-owner", "agent.memory").expect("initialize");
    root
}

fn register_schema(root: &Path) {
    let path = root.join("lesson.schema.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "equill://agent.lesson/v1",
            "type": "object",
            "required": ["rule"],
            "additionalProperties": false,
            "properties": {
                "rule": { "type": "string" },
                "expires_at": { "type": "string" }
            },
            "x-equill-envelope": { "namespace": "agent.memory", "type": "agent.lesson.v1" }
        }))
        .expect("schema json"),
    )
    .expect("schema file");
    schema::register_file(root, &path, "test-owner").expect("register schema");
}

fn line(id: &str, supersedes: Option<&str>, expires_at: Option<&str>, tags: &[&str]) -> String {
    let mut payload = json!({ "rule": format!("Synthetic rule {id}") });
    if let Some(expires_at) = expires_at {
        payload["expires_at"] = json!(expires_at);
    }
    format!(
        "{}\n",
        serde_json::to_string(&json!({
            "id": id,
            "ts": "2026-01-01T00:00:00Z",
            "namespace": "agent.memory",
            "type": "agent.lesson.v1",
            "actor": "legacy-writer",
            "observed_at": "2026-01-01T00:00:00Z",
            "payload": payload,
            "evidence": [],
            "tags": tags,
            "supersedes": supersedes
        }))
        .expect("record json")
    )
}

fn snapshot(root: &Path, inputs: &[&Path]) -> Vec<Vec<u8>> {
    let mut values = inputs
        .iter()
        .map(|path| fs::read(path).expect("input bytes"))
        .collect::<Vec<_>>();
    let mut ledgers = fs::read_dir(root.join("records"))
        .expect("record directory")
        .map(|entry| fs::read(entry.expect("ledger entry").path()).expect("ledger bytes"))
        .collect::<Vec<_>>();
    ledgers.sort();
    values.extend(ledgers);
    values
}
