//! Building a store the real binary accepts, and watching what it starts.
//!
//! What the unit tests cannot show: real `equill` child processes.
//!
//! Everything here drives the installed binary, so the handoff, the process
//! group, the kill and the recovery are the ones production would see rather
//! than a function pointer standing in for them. The provider is a socket
//! nobody listens on, which makes a child fail deterministically without any
//! network peer — and a directory of synthetic artifacts, so no model is loaded.
// Shared by two test binaries, and neither uses all of it: an integration
// harness is compiled once per target, so anything the other target needs looks
// dead here. Trimming to whatever one target happens to use would break the
// other.
#![allow(dead_code)]

pub mod fixture;
pub mod h2;
pub mod provider;
pub mod session;

pub use fixture::{write_json, write_line};

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub fn binary() -> PathBuf {
    // The integration harness builds the binary next to the test executable.
    let mut path = std::env::current_exe().expect("test binary");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join("equill")
}

pub fn equill(store: &Path, args: &[&str]) -> std::process::Output {
    Command::new(binary())
        .args(args)
        .arg("--store")
        .arg(store)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("run equill")
}

/// A store pointed at a provider the test controls, so a worker can be made to
/// stay alive. Everything else matches `store`.
pub fn store_against(name: &str, endpoint: &str) -> PathBuf {
    let root = store(name);
    fixture::configure_endpoint(&root, endpoint);
    root
}

pub fn store(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-process-{name}-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let out = Command::new(binary())
        .args(["init", "--owner", "owner", "--namespace", "agent.memory"])
        .arg("--store")
        .arg(&root)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("init");
    assert!(out.status.success(), "init failed");
    write_json(
        &root.join("schema.json"),
        &serde_json::json!({
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
    let out = Command::new(binary())
        .args(["schema", "register", "--file"])
        .arg(root.join("schema.json"))
        .arg("--store")
        .arg(&root)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("register");
    assert!(out.status.success(), "schema register failed");
    fixture::configure(&root);
    // Prove the fixture actually produced a store the binary considers enabled;
    // a silently disabled projection would make every assertion below vacuous.
    let out = Command::new(binary())
        .args(["status"])
        .arg("--store")
        .arg(&root)
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("status");
    assert!(
        out.status.success(),
        "fixture store is unusable: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let seen = String::from_utf8_lossy(&out.stdout);
    assert!(
        !seen.contains("disabled"),
        "the fixture left the projection disabled: {seen}"
    );
    root
}

/// One record, on ONE line. A pretty-printed draft is read as eight malformed
/// JSONL rows, which the writer reports as rejected while still exiting zero —
/// so this must be compact, and the caller must check more than the exit code.
pub fn draft(root: &Path, index: usize) -> PathBuf {
    let path = root.join(format!("draft-{index}.json"));
    write_line(
        &path,
        &serde_json::json!({
            "namespace": "agent.memory",
            "type": "agent.lesson.v1",
            "observed_at": "2026-01-01T00:00:00Z",
            "payload": { "rule": format!("process lesson number {index}") }
        }),
    );
    path
}

pub fn record(root: &Path, index: usize) -> Duration {
    record_with(&binary(), root, index)
}

/// Time one write through a named binary, so the current build and a released
/// one can be measured by identical means.
pub fn record_with(binary: &Path, root: &Path, index: usize) -> Duration {
    let input = draft(root, index);
    let started = Instant::now();
    let out = Command::new(binary)
        .args(["record", "--input"])
        .arg(&input)
        .arg("--store")
        .arg(root)
        .arg("--json")
        .env("EQUILL_ACTOR", "owner")
        .output()
        .expect("record");
    let elapsed = started.elapsed();
    assert!(
        out.status.success(),
        "record {index} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Exit code alone is not enough: a batch with every line rejected still
    // exits zero, which is how a broken fixture can look like a passing test.
    let seen = String::from_utf8_lossy(&out.stdout);
    assert!(
        !seen.contains("\"rejected\""),
        "record {index} rejected its input: {seen}"
    );
    elapsed
}

/// Wait for a file to appear, or give up. Polling a condition beats sleeping a
/// fixed span: the suite runs in parallel and a loaded machine is slower than a
/// quiet one, so a fixed sleep is either flaky or needlessly slow.
pub fn appears(path: &Path, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if path.is_file() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Wait for every worker to be gone.
pub fn settles(root: &Path, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if children(root) == 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Workers for ONE store. Counting every `vector drain` on the machine would
/// tally the other tests in this file, which run in parallel — a measurement
/// that says "single-flight is broken" when what broke is the measurement.
/// Serialize the measurements that must not run beside each other.
///
/// A file lock owned by the tests, in the temp directory — NOT a search for
/// other people's processes. An earlier version ran `pkill -f "vector drain"`,
/// which would have killed workers belonging to another test, another project,
/// or a live store, and its comment claimed it killed nothing. That was wrong
/// twice over and is gone.
pub fn exclusive_measurement() -> std::fs::File {
    let path = std::env::temp_dir().join("equill-benchmark.lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .expect("benchmark lock");
    // Blocking on purpose: the other measurement finishing is exactly what this
    // is waiting for.
    fs2::FileExt::lock_exclusive(&file).expect("lock");
    file
}

/// How long a worker can be waiting on a stalled provider before its own client
/// gives up: the Qdrant request timeout, ten seconds. Every assertion about a
/// worker being alive has to land inside this, because past it the worker is
/// SUPPOSED to exit — that is the product not hanging, not a failure.
pub const WORKER_PATIENCE: Duration = Duration::from_secs(10);

pub fn children(root: &Path) -> usize {
    let out = Command::new("pgrep")
        .args(["-f", &format!("vector drain --store {}", root.display())])
        .output()
        .expect("pgrep");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}
