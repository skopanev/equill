use super::support::{Fixture, draft};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn audit_failure_preserves_mcp_committed_result_and_exposes_pending_coordinate() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.audit.join("pending.tmp")).expect("audit fault");
    let mut child = fixture
        .command()
        .args(["mcp", "--store"])
        .arg(&fixture.store)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("MCP");
    let request = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"record","arguments":{"draft":draft()}}});
    writeln!(child.stdin.take().expect("input"), "{request}").expect("request");
    let response = child.wait_with_output().expect("response");
    assert!(response.status.success());
    let body = response
        .stdout
        .strip_suffix(b"\n")
        .expect("response line")
        .to_vec();
    let response: Value = serde_json::from_slice(&body).expect("JSON response");
    let result = &response["result"];
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"]["durable"], true);
    assert_eq!(result["audit"]["operation_result_preserved"], true);
    assert_eq!(result["audit"]["state"], "pending");
    let id: uuid::Uuid = serde_json::from_value(result["structuredContent"]["id"].clone())
        .expect("original record coordinate");
    let invocation: uuid::Uuid = serde_json::from_value(result["audit"]["invocation"].clone())
        .expect("recoverable invocation coordinate");
    let receipt = result["structuredContent"]["receipt"]
        .as_str()
        .expect("receipt");
    assert!(fixture.store.join(receipt).is_file());
    fs::remove_dir(fixture.audit.join("pending.tmp")).expect("restore audit");
    let scope = equill::audit::Scope {
        surface: Some("mcp".into()),
        operation: Some("record".into()),
        ..Default::default()
    };
    let events = equill::audit::list_at(&fixture.audit, &scope, 100)
        .expect("recover")
        .events;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].id, invocation);
    assert_eq!(events[0].domain_outcome, "success");
    assert_eq!(events[0].output.ids, vec![id]);
    assert_eq!(events[0].output.durable, Some(true));
    assert_eq!(
        events[0].output.sha256,
        equill::kernel::digest::sha256_hex(&body)
    );
    assert_eq!(events[0].output.bytes, body.len() as u64);
    assert_eq!(
        equill::audit::list_at(&fixture.audit, &scope, 100)
            .expect("again")
            .events,
        events
    );
}

#[test]
fn audit_failure_preserves_cli_committed_record_and_receipt_then_recovers_once() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.audit.join("pending.tmp"))
        .expect("injected audit publication failure");
    let output = fixture.scoped(&[
        "record",
        "--input",
        fixture.root.join("draft.json").to_str().expect("path"),
    ]);
    assert!(
        output.status.success(),
        "domain success must not invite a blind retry"
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("original record report");
    assert_eq!(report["durable"], true);
    let id: uuid::Uuid = report["id"]
        .as_str()
        .expect("record coordinate")
        .parse()
        .expect("UUID");
    let receipt = report["receipt"].as_str().expect("receipt coordinate");
    assert!(fixture.store.join(receipt).is_file());
    assert!(String::from_utf8_lossy(&output.stderr).contains("audit pending for invocation"));
    fs::remove_dir(fixture.audit.join("pending.tmp")).expect("restore audit sink");
    let scope = equill::audit::Scope {
        operation: Some("record".into()),
        ..Default::default()
    };
    let events = equill::audit::list_at(&fixture.audit, &scope, 100)
        .expect("recovery")
        .events;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].domain_outcome, "success");
    assert_eq!(events[0].output.ids, vec![id]);
    assert_eq!(events[0].output.durable, Some(true));
    assert_eq!(
        events[0].output.receipt_sha256.as_deref(),
        Some(equill::kernel::digest::sha256_hex(receipt.as_bytes()).as_str())
    );
    assert_eq!(
        equill::audit::list_at(&fixture.audit, &scope, 100)
            .expect("again")
            .events,
        events
    );
}

#[test]
fn abrupt_process_exit_before_and_after_outcome_checkpoint_keeps_one_honest_event() {
    for checkpoint in [false, true] {
        let fixture = Fixture::new();
        let status = Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "recovery::crash_child", "--nocapture"])
            .env("EQUILL_AUDIT_CRASH_FIXTURE", &fixture.store)
            .env("EQUILL_AUDIT_DIR", &fixture.audit)
            .env("EQUILL_AUDIT_CRASH_CHECKPOINT", checkpoint.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("crash helper");
        assert_eq!(status.code(), Some(73));
        let events = equill::audit::list_at(&fixture.audit, &Default::default(), 100)
            .expect("recover process death")
            .events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].error_class.as_deref(), Some("interrupted"));
        assert_eq!(
            events[0].domain_outcome,
            if checkpoint { "success" } else { "unknown" }
        );
        let records = fixture.truth();
        assert_eq!(
            records.len(),
            2,
            "the domain append survived the process exit"
        );
        if checkpoint {
            assert!(
                records
                    .iter()
                    .any(|record| events[0].output.ids.contains(&record.id))
            );
        }
        assert_eq!(
            equill::audit::list_at(&fixture.audit, &Default::default(), 100)
                .expect("again")
                .events,
            events
        );
    }
}

#[test]
fn crash_child() {
    let Some(store) = std::env::var_os("EQUILL_AUDIT_CRASH_FIXTURE") else {
        return;
    };
    let mut invocation = equill::audit::Invocation::cli(&["equill".into(), "record".into()])
        .expect("reserve")
        .expect("audited");
    let report = equill::record::append_only(
        std::path::Path::new(&store),
        serde_json::from_value(draft()).expect("draft"),
        "owner",
    )
    .expect("domain append");
    if std::env::var("EQUILL_AUDIT_CRASH_CHECKPOINT").as_deref() == Ok("true") {
        invocation
            .prepare(&serde_json::to_vec(&report).expect("report"), None)
            .expect("checkpoint");
    }
    std::process::exit(73); // Deliberately skips Rust destructors, as a killed process would.
}
