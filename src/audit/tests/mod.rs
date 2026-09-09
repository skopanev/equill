use super::{Arguments, Event, Output, capture, writer};
mod confinement;
mod query;
mod reservations;
mod root_swap;
use crate::kernel::digest::sha256_hex;
use std::fs;
use std::path::PathBuf;

pub(super) fn root() -> PathBuf {
    let path = std::env::temp_dir().join(format!("equill-audit-test-{}", uuid::Uuid::now_v7()));
    fs::create_dir_all(&path).expect("directory");
    path
}

pub(super) fn event(at: &str) -> Event {
    Event {
        schema_version: 1,
        id: uuid::Uuid::now_v7(),
        observed_at: at.into(),
        duration_us: 120,
        surface: "cli".into(),
        operation: "status".into(),
        project: Some("demo".into()),
        role: Some("reviewer".into()),
        process: Some("equill".into()),
        pid: 7,
        actor_claimed: Some("owner".into()),
        lane_claimed: Some("lane-a".into()),
        instance: Some("instance-a".into()),
        session: Some("session-a".into()),
        outcome: "success".into(),
        domain_outcome: "success".into(),
        error_class: None,
        arguments: Arguments {
            sha256: sha256_hex(b"[]"),
            bytes: 2,
            items: 0,
        },
        output: Output {
            sha256: sha256_hex(b"{}"),
            bytes: 2,
            ..Output::default()
        },
    }
}

#[test]
fn durable_rotation_and_concurrent_appends_keep_every_event_once() {
    let root = root();
    let start = std::sync::Arc::new(std::sync::Barrier::new(12));
    let threads: Vec<_> = (0..12)
        .map(|index| {
            let root = root.clone();
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                let at = if index % 2 == 0 {
                    "2026-01-01T00:00:00Z"
                } else {
                    "2026-02-01T00:00:00Z"
                };
                let event = event(at);
                writer::append(&root, &event).expect("durable");
                event.id
            })
        })
        .collect();
    let mut expected: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().expect("thread"))
        .collect();
    let mut actual = Vec::new();
    for month in ["2026-01", "2026-02"] {
        let data = fs::read_to_string(root.join(format!("{month}.jsonl"))).expect("ledger");
        assert!(data.ends_with('\n'));
        actual.extend(
            data.lines()
                .map(|line| serde_json::from_str::<Event>(line).expect("event").id),
        );
    }
    expected.sort();
    actual.sort();
    assert_eq!(actual, expected);
    assert!(!root.join("pending.json").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn interrupted_append_recovers_at_every_byte_boundary_without_duplicate() {
    let record = event("2026-01-01T00:00:00Z");
    let line = format!("{}\n", serde_json::to_string(&record).expect("json"));
    for cut in [0, 1, line.len() / 2, line.len() - 1, line.len()] {
        let root = root();
        let journal = serde_json::json!({
            "month": "2026-01", "offset": 0, "sha256": sha256_hex(line.as_bytes()), "line": line,
        });
        fs::write(
            root.join("pending.json"),
            serde_json::to_vec(&journal).expect("journal"),
        )
        .expect("stage");
        fs::write(root.join("2026-01.jsonl"), &line.as_bytes()[..cut]).expect("interrupted ledger");
        let lock = writer::lock(&root).expect("lock");
        writer::recover(&lock.root).expect("recover");
        writer::recover(&lock.root).expect("idempotent recovery");
        drop(lock);
        assert_eq!(
            fs::read_to_string(root.join("2026-01.jsonl")).expect("read"),
            line
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}

#[test]
fn refuses_project_store_and_unjournaled_partial_ledger() {
    let root = root();
    fs::write(root.join("store.json"), b"{}").expect("project marker");
    assert!(writer::append(&root, &event("2026-01-01T00:00:00Z")).is_err());
    assert!(writer::append(&root.join("audit"), &event("2026-01-01T00:00:00Z")).is_err());
    fs::remove_file(root.join("store.json")).expect("remove fixture marker");
    fs::write(root.join("2026-01.jsonl"), b"partial").expect("bad tail");
    assert!(writer::append(&root, &event("2026-01-01T00:00:00Z")).is_err());
    assert_eq!(
        fs::read(root.join("2026-01.jsonl")).expect("tail"),
        b"partial"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn capture_keeps_coordinates_readable_and_arbitrary_values_out() {
    assert_eq!(capture::coordinate("demo-project"), "demo-project");
    for value in [
        "/private/example",
        "large free text",
        &"x".repeat(1000),
        &format!("{}{}", "ghp_", "a".repeat(36)),
    ] {
        let coordinate = capture::coordinate(value);
        assert!(coordinate.starts_with("sha256:"));
        assert!(!coordinate.contains(value));
    }
    let payload = "private body ".repeat(10000);
    let result = capture::output(
        serde_json::json!({ "payload": payload, "receipt": "/private/receipt", "id": "not-an-id" })
            .to_string()
            .as_bytes(),
    );
    let json = serde_json::to_string(&result).expect("output");
    assert!(json.len() < 400);
    assert!(!json.contains("private"));
    assert!(result.ids.is_empty());
}

#[test]
fn numeric_result_counts_and_import_references_are_bounded() {
    let id = uuid::Uuid::now_v7();
    for (body, count) in [
        (serde_json::json!({"records":42}), 42),
        (serde_json::json!({"exported":3,"current":2,"legacy":1}), 3),
        (
            serde_json::json!({"imported":1,"records":[{"record_id":id}]}),
            1,
        ),
        (serde_json::json!({"stored":0,"records":[{"id":id}]}), 0),
    ] {
        let output = capture::output(&serde_json::to_vec(&body).expect("JSON"));
        assert_eq!(output.count, Some(count));
        if body["records"].is_array() {
            assert_eq!(output.ids, vec![id]);
        }
    }
}
