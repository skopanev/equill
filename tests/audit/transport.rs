use super::support::{Fixture, draft};
use std::io::Write;
use std::process::Stdio;

#[test]
fn broken_response_pipes_preserve_known_domain_success_in_one_audit_event() {
    for mcp in [false, true] {
        let fixture = Fixture::new();
        let mut command = fixture.command();
        if mcp {
            command.args(["mcp", "--store"]).arg(&fixture.store);
        } else {
            command
                .args(["record", "--store"])
                .arg(&fixture.store)
                .arg("--input")
                .arg(fixture.root.join("draft.json"));
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("child");
        drop(child.stdout.take()); // A deterministic closed reader, before response delivery.
        if mcp {
            let request = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"record","arguments":{"draft":draft()}}});
            writeln!(child.stdin.take().expect("stdin"), "{request}").expect("request");
        } else {
            drop(child.stdin.take());
        }
        assert!(!child.wait().expect("exit").success());
        let events = fixture.events();
        let matching: Vec<_> = events
            .iter()
            .filter(|event| event.operation == "record")
            .collect();
        assert_eq!(matching.len(), 1);
        let event = matching[0];
        assert_eq!(event.error_class.as_deref(), Some("transport"));
        assert_eq!(event.outcome, "error");
        assert_eq!(event.domain_outcome, "success");
        assert_eq!(event.output.durable, Some(true));
        assert_eq!(event.output.ids.len(), 1);
        assert!(
            equill::record::read_all(&fixture.store)
                .expect("ledger")
                .iter()
                .any(|record| event.output.ids.contains(&record.id))
        );
    }
}

#[test]
fn help_and_error_digests_describe_the_bytes_actually_emitted() {
    let fixture = Fixture::new();
    let help = fixture.cli(&["--help"]);
    assert!(help.status.success());
    let help_event = fixture.events().pop().expect("help event");
    assert_eq!(
        help_event.output.sha256,
        equill::kernel::digest::sha256_hex(&help.stdout)
    );
    let error = fixture.scoped(&["get", "--id", "invalid"]);
    assert!(!error.status.success());
    let error_event = fixture.events().pop().expect("error event");
    assert_eq!(
        error_event.output.sha256,
        equill::kernel::digest::sha256_hex(&error.stderr)
    );
}
