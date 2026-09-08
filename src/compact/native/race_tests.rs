//! A write that lands in the middle of a compaction.
use super::run::{run, with_pause};
use crate::command::init;
use crate::record::{RecordDraft, append, read_all};
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn store(name: &str) -> PathBuf {
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

fn add(root: &Path, rule: &str, supersedes: Option<uuid::Uuid>) -> uuid::Uuid {
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

/// A compaction whose provider is unreachable is not a successful compaction.
///
/// Swallowing the failure would report success while leaving points behind
/// forever: the ledger no longer names those records, so no later sync can
/// find them. The work has to stay recoverable instead.
#[test]
fn an_unreachable_provider_fails_the_compaction_and_leaves_the_work_pending() {
    let root = store("unreachable");
    let first = add(&root, "older", None);
    add(&root, "newer", Some(first));
    // A configured provider that cannot be reached. The config has to be one
    // the loader accepts, or the failure under test is a parse error wearing
    // the right name — which is what an earlier version of this test measured.
    let models = root.join("models");
    std::fs::create_dir_all(&models).expect("models");
    for (name, body) in [
        ("model.onnx", &b"synthetic model"[..]),
        ("tokenizer.json", &b"synthetic tokenizer"[..]),
        ("config.json", &b"synthetic model config"[..]),
    ] {
        std::fs::write(models.join(name), body).expect("artifact");
    }
    std::fs::create_dir_all(root.join("registry/vector")).expect("registry");
    std::fs::write(
        root.join("registry/vector/qdrant.json"),
        serde_json::json!({
            "schema": "equill.qdrant-config.v1",
            "enabled": true,
            "endpoint": "http://127.0.0.1:9",
            "collection_alias": "equill_records_test",
            "store_id": uuid::Uuid::now_v7(),
            "dimensions": 3,
            "distance": "cosine",
            "embedding": {
                "model_id": "synthetic-embedding-v1",
                "input_schema": "equill.record.embedding.v1",
                "model": {
                    "path": "models/model.onnx",
                    "sha256": crate::kernel::digest::sha256_hex(b"synthetic model")
                },
                "tokenizer": {
                    "path": "models/tokenizer.json",
                    "sha256": crate::kernel::digest::sha256_hex(b"synthetic tokenizer")
                },
                "config": {
                    "path": "models/config.json",
                    "sha256": crate::kernel::digest::sha256_hex(b"synthetic model config")
                }
            }
        })
        .to_string(),
    )
    .expect("vector config");

    let outcome = run(&root, true, "owner");

    assert!(
        outcome.is_err(),
        "an unreachable provider was reported as a successful compaction"
    );
    assert!(
        !super::projections::unfinished(&root)
            .expect("pending")
            .is_empty(),
        "the list of points to remove was lost, so no later run can finish the job"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The second run finishes what the first could not.
///
/// Once the ledger has been swapped, it no longer names the removed records —
/// so a second compaction plans nothing and would exit, leaving the points
/// behind. It finishes the stashed work first, which is the only reason the
/// interruption is recoverable at all.
#[test]
fn a_second_run_finishes_the_cleanup_an_interrupted_one_left() {
    let root = store("recovery");
    let first = add(&root, "older", None);
    add(&root, "newer", Some(first));

    // What an interrupted run leaves behind: the ledger already compacted, the
    // work still pending.
    run(&root, true, "owner").expect("first compaction");
    super::projections::stash(&root, &[first]).expect("stash");

    let again = run(&root, true, "owner").expect("second compaction");

    assert_eq!(again.removed, 0, "the ledger was compacted twice");
    assert!(
        super::projections::unfinished(&root)
            .expect("pending")
            .is_empty(),
        "the second run did not finish the interrupted cleanup"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The reconciliation is reached from the command, not only reachable.
///
/// A direct test of the removal logic proves the decision is right; it says
/// nothing about whether compaction ever asks. This one fails if the call is
/// taken out of `run`.
#[test]
fn compaction_leaves_nothing_pending_when_it_succeeds() {
    let root = store("wired");
    let first = add(&root, "older", None);
    add(&root, "newer", Some(first));

    let report = run(&root, true, "owner").expect("compaction");

    assert_eq!(report.removed, 1);
    assert!(
        super::projections::unfinished(&root)
            .expect("pending")
            .is_empty(),
        "compaction finished without reconciling: the pending list survived"
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
