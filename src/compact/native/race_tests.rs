//! A write that lands in the middle of a compaction.
use super::run::{run, with_pause};
use crate::command::init;
use crate::record::{RecordDraft, append, read_all};
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) fn store(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-compact-race-{name}-{}",
        uuid::Uuid::now_v7()
    ));
    init::create(&root, "owner", "agent.memory").expect("init");
    schema::register(
        &root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "owner".into(),
            payload_schema: json!({
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        "owner",
    )
    .expect("schema");
    root
}

pub(super) fn add(root: &Path, rule: &str, supersedes: Option<uuid::Uuid>) -> uuid::Uuid {
    append(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": rule }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes,
        },
        "owner",
    )
    .expect("append")
    .id
}

/// Governance and the writer hold different locks, so holding the governance
/// one says nothing about appends. Without the writer's lock a record landing
/// between the read and the swap is dropped by a rewrite that never saw it —
/// gone from an immutable ledger.
///
/// The window is opened deliberately rather than hoped for: a race test that
/// waits on luck comes out green whether or not the lock is held, which is the
/// failure this test exists to avoid in itself.
#[test]
fn a_record_written_inside_the_window_survives() {
    let root = store("window");
    let first = add(&root, "older", None);
    add(&root, "newer", Some(first));

    let racing = root.clone();
    let writer = std::thread::spawn(move || {
        // Late enough that compaction has read the ledger, early enough that it
        // has not published.
        std::thread::sleep(Duration::from_millis(120));
        add(&racing, "written during compaction", None)
    });
    let report =
        with_pause(Duration::from_millis(400), || run(&root, true, "owner")).expect("compaction");
    let landed = writer.join().expect("writer thread");

    assert_eq!(report.removed, 1);
    let after = read_all(&root).expect("ledger");
    assert!(
        after.iter().any(|record| record.id == landed),
        "a record written during compaction was dropped from the ledger"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A survivor's receipt describes the bytes that are now in the ledger.
///
/// Cutting a link rewrites the record, so its stored hash moves. A receipt
/// still carrying the old one would make a verification report corruption for
/// a record nobody touched — the store accusing itself of damage it did
/// deliberately.
#[test]
fn a_rewritten_survivor_and_its_receipt_agree() {
    let root = store("receipts");
    let first = add(&root, "older", None);
    let survivor = add(&root, "newer", Some(first));

    run(&root, true, "owner").expect("compaction");

    let record = read_all(&root)
        .expect("ledger")
        .into_iter()
        .find(|record| record.id == survivor)
        .expect("survivor");
    let digest = crate::kernel::digest::sha256_hex(&serde_json::to_vec(&record).expect("bytes"));
    let month = &record.recorded_at[..7];
    let path = root
        .join("receipts/writes")
        .join(month)
        .join(format!("{survivor}.json"));
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("receipt")).expect("json");

    assert_eq!(
        receipt["record_sha256"].as_str(),
        Some(digest.as_str()),
        "the receipt still attests to bytes the ledger no longer holds"
    );
    let _ = std::fs::remove_dir_all(&root);
}
