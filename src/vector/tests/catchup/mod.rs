mod drain_races;
mod enumeration;

use crate::command::init;
use crate::record::{RecordDraft, append};
use crate::schema::{self, TypeDefinition};
use crate::vector::after_commit;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(super) fn store(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("equill-drain-{name}-{}", Uuid::now_v7()));
    init::create(&root, "owner", "agent.memory").expect("initialize");
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
    .expect("register schema");
    root
}

pub(super) fn add(root: &Path, rule: &str) {
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
            supersedes: None,
        },
        "owner",
    )
    .expect("append");
}

/// A store with the projection off must never open a connection, so the drain
/// has to decide from local config before it can reach anything.
#[test]
fn a_disabled_projection_neither_publishes_nor_connects() {
    let root = store("disabled");

    add(&root, "a rule written with no vector configured");
    let report = after_commit(&root);

    assert!(!report.ran);
    assert!(report.attempt_error.is_none());
    assert!(
        !root.join("projections/qdrant/desired.json").is_file(),
        "nothing is published for a store that has no vector projection"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// The write is already durable when the catch-up runs, so an unreachable
/// provider costs a report line and nothing else.
#[test]
fn an_unreachable_provider_leaves_the_record_written() {
    let root = store("unreachable");
    configure_unreachable(&root);

    add(&root, "a rule written while the index is unreachable");
    let report = after_commit(&root);

    // The record is in the ledger regardless of what the provider did.
    assert_eq!(crate::record::read_all(&root).expect("records").len(), 1);
    assert!(report.attempt_error.is_some(), "the attempt is reported");
    // And what the ledger wants indexed is recorded, so the next attempt knows.
    let target = crate::vector::desired::read(&root)
        .expect("desired")
        .expect("published");
    assert_eq!(target.records, 1);
    fs::remove_dir_all(root).expect("cleanup");
}

/// Writing configures a target the drain can chase. Pointing it at a port
/// nothing listens on keeps the test hermetic: no container, no network peer.
pub(super) fn configure_unreachable(root: &Path) {
    let models = root.join("models");
    fs::create_dir_all(&models).expect("model directory");
    let artifact = |name: &str, body: &str| {
        fs::write(models.join(name), body).expect("artifact");
        json!({
            "path": format!("models/{name}"),
            "sha256": crate::kernel::digest::sha256_hex(body.as_bytes())
        })
    };
    let config = json!({
        "schema": "equill.qdrant-config.v1",
        "enabled": true,
        "endpoint": "http://127.0.0.1:1",
        "collection_alias": "equill_drain_test",
        "store_id": Uuid::now_v7(),
        "dimensions": 1024,
        "distance": "cosine",
        "embedding": {
            "model_id": "Qwen/Qwen3-Embedding-0.6B",
            "input_schema": "equill.record.embedding.v1",
            "model": artifact("model.safetensors", "synthetic weights"),
            "tokenizer": artifact("tokenizer.json", "synthetic tokenizer"),
            "config": artifact("config.json", "synthetic config")
        }
    });
    let directory = root.join("registry/vector");
    fs::create_dir_all(&directory).expect("registry directory");
    fs::write(
        directory.join("qdrant.json"),
        serde_json::to_vec_pretty(&config).expect("config json"),
    )
    .expect("config file");
}

/// The race worth naming: a writer commits while somebody is already draining.
/// It must not wait and must not spawn, but its tail must not be lost either.
/// What makes that safe is the order — publish the watermark, then try the lock
/// — so a holder doing its final comparison sees the tail, and a holder that
/// has already left leaves the lock free for the writer to take.
#[test]
fn a_writer_that_finds_the_drain_busy_still_publishes_its_tail() {
    let root = store("busy");
    configure_unreachable(&root);
    add(&root, "first");
    after_commit(&root);
    let first = crate::vector::desired::read(&root)
        .expect("desired")
        .expect("published")
        .records;

    // Somebody else is draining for the whole of the next write.
    let held = crate::kernel::lock::TryLock::acquire(&root, "vector-drain.lock")
        .expect("lock")
        .expect("free to take");
    add(&root, "second");
    let report = after_commit(&root);
    let published = crate::vector::desired::read(&root)
        .expect("desired")
        .expect("published");

    // The writer did not run the drain and did not block on it.
    assert!(!report.ran);
    assert!(report.attempt_error.is_none());
    // But it did record what it wants indexed, which is what the holder will
    // see when it checks whether it has caught up.
    assert_eq!(published.records, first + 1);
    drop(held);

    // With the lock free again, the next write is free to drain itself.
    add(&root, "third");
    let after = after_commit(&root);
    assert!(
        after.ran,
        "the lock is no longer held, so this writer drains"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// The same handoff, proved across a real process boundary: the lock is an
/// advisory file lock, so a test that only opened it twice inside one process
/// would prove the protocol but not the isolation. Here another process holds
/// it, and this one must still commit, publish and decline to wait.
#[test]
fn the_handoff_holds_when_another_process_owns_the_lock() {
    let root = store("cross-process");
    configure_unreachable(&root);
    let lock_path = root.join("locks/vector-drain.lock");
    fs::create_dir_all(root.join("locks")).expect("locks directory");
    let ready = root.join("locks/.held");
    let mut holder = std::process::Command::new("python3")
        .arg("-c")
        .arg(format!(
            "import fcntl,pathlib,time\n\
             f=open({lock:?},'a+')\n\
             fcntl.flock(f,fcntl.LOCK_EX)\n\
             pathlib.Path({ready:?}).write_text('held')\n\
             time.sleep(30)\n",
            lock = lock_path.to_string_lossy(),
            ready = ready.to_string_lossy()
        ))
        .spawn()
        .expect("holder process");
    for _ in 0..200 {
        if ready.is_file() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    assert!(ready.is_file(), "the other process took the lock");

    add(&root, "written while another process drains");
    let report = after_commit(&root);
    let published = crate::vector::desired::read(&root)
        .expect("desired")
        .expect("published");

    holder.kill().expect("stop the holder");
    holder.wait().expect("reap the holder");

    // Did not wait for the other process and did not start one of its own.
    assert!(!report.ran);
    assert!(report.attempt_error.is_none());
    // The tail is on record for whoever is draining to pick up.
    assert_eq!(published.records, 1);
    // Once that process is gone the lock is free again — the kernel releases it
    // even if the holder never got to, which is why no lease timeout is needed.
    let reclaimed = crate::kernel::lock::TryLock::acquire(&root, "vector-drain.lock")
        .expect("lock")
        .is_some();
    assert!(reclaimed, "a dead holder does not strand the lock");
    fs::remove_dir_all(root).expect("cleanup");
}
