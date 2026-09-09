use super::support::Fixture;
use std::fs;
use std::io::Write;
use std::process::Stdio;

#[test]
fn revoke_surfaces_and_interrupted_outcomes_keep_the_committed_tombstone() {
    for surface in ["human", "json", "mcp"] {
        for interrupted in [false, true] {
            let fixture = Fixture::new();
            if interrupted {
                fs::create_dir_all(fixture.audit.join("pending.tmp")).expect("audit fault");
            }
            let output = if surface == "mcp" {
                let mut child = fixture
                    .command()
                    .args(["mcp", "--store"])
                    .arg(&fixture.store)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .expect("MCP");
                let request = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"revoke","arguments":{"id":fixture.record}}});
                writeln!(child.stdin.take().expect("input"), "{request}").expect("request");
                child.wait_with_output().expect("MCP result")
            } else {
                let mut command = fixture.command();
                if surface == "json" {
                    command.arg("--json");
                }
                command
                    .args(["revoke", "--id"])
                    .arg(fixture.record.to_string())
                    .arg("--store")
                    .arg(&fixture.store)
                    .output()
                    .expect("CLI result")
            };
            assert!(
                output.status.success(),
                "{surface}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let records = equill::record::read_all(&fixture.store).expect("truth");
            let tombstone = records
                .iter()
                .find(|record| record.supersedes == Some(fixture.record))
                .expect("durable tombstone")
                .id;
            if surface == "human" {
                assert!(String::from_utf8_lossy(&output.stdout).contains(&tombstone.to_string()));
            } else {
                let response: serde_json::Value =
                    serde_json::from_slice(&output.stdout).expect("JSON");
                let body = response
                    .pointer("/result/structuredContent")
                    .unwrap_or(&response);
                assert_eq!(body["tombstone"], tombstone.to_string());
            }
            if interrupted {
                fs::remove_dir(fixture.audit.join("pending.tmp")).expect("restore audit");
            }
            let scope = equill::audit::Scope {
                operation: Some("revoke".into()),
                ..Default::default()
            };
            let events = equill::audit::list_at(&fixture.audit, &scope, 10)
                .expect("recover")
                .events;
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].output.ids, vec![tombstone]);
            assert_eq!(events[0].output.count, Some(1));
            assert_eq!(events[0].output.durable, Some(true));
            assert_eq!(events[0].domain_outcome, "success");
            assert_eq!(
                events[0].error_class.as_deref(),
                if interrupted {
                    Some("interrupted")
                } else {
                    None
                }
            );
            assert_eq!(
                equill::audit::list_at(&fixture.audit, &scope, 10)
                    .expect("again")
                    .events,
                events
            );
        }
    }
}
