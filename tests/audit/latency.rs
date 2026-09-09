//! Release measurements run explicitly on an otherwise idle host. Audit-only
//! wall time includes metadata capture, lock acquisition, journal and fsync.
use super::support::{Fixture, draft};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::time::{Duration, Instant};

fn report(label: &str, mut values: Vec<Duration>) -> bool {
    values.sort();
    let p95 = values[(values.len() * 95).div_ceil(100) - 1];
    let max = *values.last().expect("measurements");
    eprintln!(
        "audit release {label}: p95={p95:?} max={max:?}; 50/100ms advisory={}",
        p95 <= Duration::from_millis(50) && max <= Duration::from_millis(100)
    );
    max <= Duration::from_millis(250)
}

#[test]
fn audit_only_overhead_and_total_finite_session_latency() {
    assert!(
        std::env::var_os("EQUILL_AUDIT_DIR").is_some(),
        "release measurements need an explicit disposable audit destination"
    );
    let mut overhead = Vec::new();
    for _ in 0..50 {
        let start = Instant::now();
        equill::audit::Invocation::mcp(br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#, "owner")
            .expect("audit begin")
            .finish(b"{}", None)
            .expect("audit commit");
        overhead.push(start.elapsed());
    }
    report("audit-only", overhead);
    let fixture = Fixture::new();
    let startup = Instant::now();
    let mut child = fixture
        .command()
        .args(["mcp", "--store"])
        .arg(&fixture.store)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("session");
    let mut input = child.stdin.take().expect("stdin");
    let mut output = BufReader::new(child.stdout.take().expect("stdout"));
    // Match real clients and the existing session harness: negotiate before
    // tool timing, but do not warm up any record/search/context operation.
    writeln!(
        input,
        "{}",
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{
            "protocolVersion":"2025-06-18", "capabilities":{},
            "clientInfo":{"name":"synthetic-audit-benchmark","version":"0"}
        }})
    )
    .expect("initialize");
    input.flush().expect("initialize flush");
    let mut initialized = String::new();
    output
        .read_line(&mut initialized)
        .expect("initialize response");
    let response: Value = serde_json::from_str(&initialized).expect("initialize JSON");
    assert!(response.get("result").is_some(), "{response}");
    eprintln!(
        "audit release MCP startup+initialize: {:?}",
        startup.elapsed()
    );
    let mut id = 0;
    let mut call = |name: &str, args: Value| {
        id += 1;
        let request = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}});
        let start = Instant::now();
        writeln!(input, "{request}").expect("write");
        input.flush().expect("flush");
        let mut line = String::new();
        output.read_line(&mut line).expect("read");
        let elapsed = start.elapsed();
        let result: Value = serde_json::from_str(&line).expect("JSON");
        assert_eq!(result["result"]["isError"], false, "{result}");
        elapsed
    };
    let mut within_ceiling = true;
    for (name, args) in [
        ("record", json!({"draft":draft()})),
        (
            "search",
            json!({"query":"synthetic","vector_enabled":false}),
        ),
        ("context", json!({"profile":"reader","query":"synthetic"})),
    ] {
        let values: Vec<_> = (0..50).map(|_| call(name, args.clone())).collect();
        eprintln!("audit release {name} first call: {:?}", values[0]);
        within_ceiling &= report(name, values);
    }
    drop(input);
    assert!(child.wait().expect("exit").success());
    assert!(
        within_ceiling,
        "one or more operations exceed 250ms owner ceiling"
    );
}
