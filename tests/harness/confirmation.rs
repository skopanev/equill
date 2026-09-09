//! Confirmation evidence is checked outside the timed MCP round trip.
use super::{children, provider::SlowProvider, session::Session};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

pub const CALLS: usize = 50;
pub const CEILING: Duration = Duration::from_millis(250);

pub struct Timings {
    pub p50: Duration,
    pub p95: Duration,
    pub max: Duration,
    pub count: usize,
}

impl Timings {
    pub fn new(mut values: Vec<Duration>) -> Self {
        assert!(!values.is_empty());
        values.sort();
        Self {
            p50: values[values.len() / 2],
            p95: values[(values.len() * 95).div_ceil(100) - 1],
            max: *values.last().expect("measurements"),
            count: values.len(),
        }
    }

    pub fn report(&self, label: &str) {
        let quality =
            self.p95 <= Duration::from_millis(50) && self.max <= Duration::from_millis(100);
        eprintln!(
            "{label}: n={} p50={:?} p95={:?} max={:?}; 50/100ms quality={quality}; 250ms ceiling={}",
            self.count,
            self.p50,
            self.p95,
            self.max,
            self.max <= CEILING
        );
    }

    pub fn check(&self) {
        assert!(self.count >= CALLS, "fewer than {CALLS} measured calls");
        assert!(
            self.max <= CEILING,
            "max {:?} exceeds {CEILING:?}",
            self.max
        );
    }
}

pub fn draft(index: usize) -> serde_json::Value {
    serde_json::json!({
        "namespace":"agent.memory", "type":"agent.lesson.v1",
        "observed_at":"2026-01-01T00:00:00Z",
        "payload":{"rule":format!("confirmation lesson number {index}")}
    })
}

pub fn write(session: &mut Session, root: &Path, index: usize) -> Duration {
    let (elapsed, response) = session.tool("record", serde_json::json!({"draft":draft(index)}));
    assert!(response["error"].is_null(), "RPC failure: {response}");
    assert_eq!(
        response["result"]["isError"], false,
        "tool failure: {response}"
    );
    let body = &response["result"]["structuredContent"];
    assert_eq!(body["ok"], true);
    assert_eq!(body["durable"], true);
    assert_eq!(body["text_index"], "queued");
    assert_eq!(body["vector"]["projection"], "queued");
    assert!(body["vector"]["attempt_error"].is_null());
    // Not deferred until catch-up. Fault injection separately proves fsync order.
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join(body["receipt"].as_str().expect("receipt path")))
            .expect("receipt exists at confirmation"),
    )
    .expect("receipt JSON");
    assert_eq!(receipt["durable"], true);
    assert_eq!(receipt["record_id"], body["id"]);
    assert_eq!(receipt["record_sha256"], body["sha256"]);
    let ledger = std::fs::read_to_string(root.join(body["ledger"].as_str().expect("ledger")))
        .expect("ledger exists at confirmation");
    let matching = ledger.lines().filter(|line| {
        let record: serde_json::Value = serde_json::from_str(line).expect("complete record");
        if record["id"] != body["id"] {
            return false;
        }
        assert_eq!(super::fixture::sha256(line.as_bytes()), body["sha256"]);
        true
    });
    assert_eq!(matching.count(), 1, "exactly one confirmed ledger record");
    elapsed
}

pub fn failure_mark(root: &Path) -> Option<SystemTime> {
    std::fs::metadata(root.join("projections/qdrant/last-drain.json"))
        .and_then(|metadata| metadata.modified())
        .ok()
}

pub fn assert_worker(root: &Path, mark: Option<SystemTime>) {
    assert_eq!(
        children(root),
        1,
        "stalled worker exited during measurement"
    );
    if failure_mark(root) != mark {
        let bytes = std::fs::read(root.join("projections/qdrant/last-drain.json"))
            .expect("new worker outcome");
        let report: serde_json::Value = serde_json::from_slice(&bytes).expect("worker outcome");
        assert_ne!(
            report["outcome"], "failed",
            "worker failed during measurement"
        );
    }
}

/// Observe startup outside the record timer; never release the pending request
/// before measured writes return. A synchronous provider wait fails the gate.
pub fn stalled(root: &Path, provider: &SlowProvider, mark: Option<SystemTime>) {
    let until = Instant::now() + super::WORKER_PATIENCE / 2;
    while provider.requests() == 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        provider.requests() > 0,
        "no HTTP/2 request: {:?}",
        provider.stage()
    );
    assert_worker(root, mark);
}

pub fn warm(session: &mut Session, root: &Path, mark: Option<SystemTime>, start: usize) -> Timings {
    Timings::new(
        (start..start + CALLS)
            .map(|index| {
                assert_worker(root, mark);
                let elapsed = write(session, root, index);
                assert_worker(root, mark);
                elapsed
            })
            .collect(),
    )
}
