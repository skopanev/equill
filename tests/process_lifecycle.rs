//! What only a worker that STAYS ALIVE can show: that the writer did not wait
//! for it, that a signal aimed at the writer does not reach it, that killing it
//! is recoverable, and that a long-lived MCP session notices.
//!
//! Everything here runs against a provider this test controls — a listener that
//! accepts and then holds the connection — so a child lives exactly as long as
//! the assertion needs.
mod harness;

use harness::provider::SlowProvider;
use harness::{children, equill, record, settles, store_against};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn binary() -> std::path::PathBuf {
    harness::binary()
}

/// With a provider that never answers, the worker stays alive — which is the
/// only way to tell "the writer did not wait" from "the worker finished first".
#[test]
fn a_write_does_not_wait_for_a_worker_that_keeps_running() {
    let provider = SlowProvider::start();
    let root = store_against("slow", &provider.endpoint());

    let elapsed = record(&root, 0);

    assert!(
        elapsed < Duration::from_millis(400),
        "the write took {elapsed:?} while the worker was still going"
    );
    // The premise first, in the order the comment claims: the worker has to
    // have sent a request and be waiting on it. Checking that it is merely
    // alive before knowing it reached the provider proves nothing — a worker
    // that is about to fail is also alive. An earlier harness held the TCP connection
    // without completing the HTTP/2 handshake, so the worker exited with
    // provider-unavailable in milliseconds and every "still running" assertion
    // was measuring a race it usually lost.
    // Waited for, not sampled: a freshly forked worker takes a moment to reach
    // the network, and on a loaded machine that moment is longer than the
    // assertion that follows it.
    assert!(
        reaches_provider(&provider, harness::WORKER_PATIENCE / 2),
        "the worker never reached the provider"
    );
    assert!(
        !failed_early(&root),
        "the worker exited with a provider error instead of waiting"
    );
    provider.release();
    settles(&root, Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(&root);
}

/// An interrupt aimed at the writer's process group must not take the worker
/// with it. That is what process_group(0) is for, and it can only be shown with
/// a worker that lives long enough to be signalled.
///
/// The writer is launched in a group of its OWN so the signal can be delivered
/// to exactly that group — signalling this test's group would kill the test
/// runner, which is a way to fail rather than a way to prove anything.
#[test]
fn an_interrupt_to_the_writers_group_leaves_the_worker_running() {
    let provider = SlowProvider::start();
    let root = store_against("interrupt", &provider.endpoint());
    let input = harness::draft(&root, 0);

    let mut writer = Command::new(binary())
        .args(["record", "--input"])
        .arg(&input)
        .arg("--store")
        .arg(&root)
        .env("EQUILL_ACTOR", "owner")
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("writer");
    let group = writer.id() as i32;
    // The worker must be genuinely waiting on the provider before the signal.
    // A worker that has not got that far may exit on its own, and the assertion
    // afterwards would blame the interrupt for it.
    //
    // The window is generous because a debug build starts slowly: the binary is
    // 70MB against 13MB released, and the writer, the fork and the worker each
    // pay for that before a request is on the wire. What is being tested is
    // whether the signal reaches the worker, not how fast the worker arrives.
    assert!(
        reaches_provider(&provider, harness::WORKER_PATIENCE / 2),
        "the writer's worker never sent a request; handshake reached {:?}, requests={}, {}",
        provider.stage(),
        provider.requests(),
        diagnostics(&root)
    );
    assert!(
        alive(&root, harness::WORKER_PATIENCE / 2),
        "the writer never started a worker"
    );
    // Ctrl-C, as a terminal would deliver it: to the writer's whole group.
    unsafe { libc_killpg(group, 2) };
    let _ = writer.wait();
    std::thread::sleep(Duration::from_millis(400));

    assert_eq!(
        children(&root),
        1,
        "the interrupt reached the worker, so it is not in a group of its own"
    );
    provider.release();
    settles(&root, Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(&root);
}

/// Killing the worker outright releases ownership, and the next ordinary
/// command starts a fresh one.
#[test]
fn killing_the_worker_lets_the_next_command_start_another() {
    let provider = SlowProvider::start();
    let root = store_against("kill", &provider.endpoint());
    record(&root, 0);
    assert!(
        reaches_provider(&provider, harness::WORKER_PATIENCE / 2),
        "the worker never sent a request"
    );
    assert!(
        alive(&root, harness::WORKER_PATIENCE / 2),
        "a worker is running"
    );

    kill_workers(&root);
    assert!(settles(&root, Duration::from_secs(5)), "the worker is gone");

    // An ordinary read must find the work outstanding and start another.
    let out = equill(&root, &["search", "--query", "lesson", "--limit", "1"]);
    assert!(out.status.success(), "the read failed");
    assert!(
        alive(&root, harness::WORKER_PATIENCE / 2),
        "an ordinary command did not restart the work after a kill"
    );
    provider.release();
    settles(&root, Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(&root);
}

fn alive(root: &Path, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if children(root) > 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn kill_workers(root: &Path) {
    let _ = Command::new("pkill")
        .args([
            "-9",
            "-f",
            &format!("vector drain --store {}", root.display()),
        ])
        .status();
}

unsafe extern "C" {
    #[link_name = "killpg"]
    fn libc_killpg(group: i32, signal: i32) -> i32;
}

/// Whether a worker has already filed a failed outcome. Used to prove a stalled
/// provider is stalling rather than refusing.
fn failed_early(root: &Path) -> bool {
    let path = root.join("projections/qdrant/last-drain.json");
    let Ok(bytes) = std::fs::read(&path) else {
        return false;
    };
    let state: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    state["outcome"] == "failed"
}

/// Wait until the provider has accepted a connection from the worker.
fn reaches_provider(provider: &SlowProvider, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if provider.requests() > 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Test-owned state for one disposable store, read when an assertion fails.
/// Counts and outcome words only — no payloads, no identifiers.
fn diagnostics(root: &Path) -> String {
    let read = |name: &str| -> String {
        std::fs::read_to_string(root.join("projections/qdrant").join(name))
            .map(|text| text.chars().take(200).collect())
            .unwrap_or_else(|_| "absent".into())
    };
    format!(
        "desired={} last-drain={} handoff={} active={} workers={}",
        read("desired.json"),
        read("last-drain.json"),
        read("handoff.json"),
        read("handoff-active.json"),
        children(root)
    )
}
