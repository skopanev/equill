//! A long-lived session is where a store falls behind most easily: the server
//! boots once and serves for hours, so a hook that only ran at startup would
//! never notice a worker that died.
mod harness;

use harness::provider::SlowProvider;
use harness::{binary, children, record, settles, store_against};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A long-lived MCP session must recover too. It is the case where a store is
/// most likely to fall behind: the server boots once and then serves for hours,
/// so a hook that only ran at startup would never notice.
#[test]
fn a_long_lived_mcp_session_restarts_a_dead_worker() {
    let provider = SlowProvider::start();
    let root = store_against("mcp", &provider.endpoint());
    record(&root, 0);
    assert!(
        reaches_provider(&provider, Duration::from_secs(30)),
        "the first worker never sent a request; stage {:?}",
        provider.stage()
    );
    assert!(
        alive(&root, Duration::from_secs(30)),
        "a worker is running; stage {:?} requests={} {}",
        provider.stage(),
        provider.requests(),
        state(&root)
    );
    kill_workers(&root);
    assert!(settles(&root, Duration::from_secs(5)), "the worker is gone");

    // One session, two calls. The recovery must happen on the CALL, not at boot.
    let mut server = Command::new(binary())
        .args(["mcp"])
        .arg("--store")
        .arg(&root)
        .env("EQUILL_ACTOR", "owner")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("mcp");
    // Serve calls until a worker appears or the window closes. The claim is
    // that a LIVE session restarts the work, not that it does so on any
    // particular call — the server may still be starting when the first arrives.
    let restarted = {
        use std::io::Write;
        let stdin = server.stdin.as_mut().expect("stdin");
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut id = 1;
        let mut seen = false;
        while Instant::now() < deadline && !seen {
            let call = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": "tools/list"
            });
            if writeln!(stdin, "{call}").is_err() || stdin.flush().is_err() {
                break;
            }
            id += 1;
            seen = alive(&root, Duration::from_millis(500));
        }
        seen
    };
    let _ = server.kill();
    let _ = server.wait();
    assert!(
        restarted,
        "a live MCP session never restarted the outstanding work; stage {:?} requests={} {}",
        provider.stage(),
        provider.requests(),
        state(&root)
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

/// Wait until the provider is holding a request from a worker.
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

/// Test-owned state for one disposable store: counts and outcome words only.
fn state(root: &Path) -> String {
    let read = |name: &str| -> String {
        std::fs::read_to_string(root.join("projections/qdrant").join(name))
            .map(|text| text.chars().take(160).collect())
            .unwrap_or_else(|_| "absent".into())
    };
    format!(
        "desired={} cooldown={} last-drain={} handoff={} active={}",
        read("desired.json"),
        read("cooldown.json"),
        read("last-drain.json"),
        read("handoff.json"),
        read("handoff-active.json")
    )
}
