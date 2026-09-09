use super::support::Fixture;

#[test]
fn human_and_json_batch_dispatch_preserve_summary_after_writer_extraction() {
    for json in [false, true] {
        let fixture = Fixture::new();
        let input = fixture.root.join("batch.jsonl");
        let draft = super::support::draft();
        std::fs::write(&input, format!("{draft}\n{draft}\n")).expect("batch");
        let mut command = fixture.command();
        if json {
            command.arg("--json");
        }
        let output = command
            .args(["record", "--store"])
            .arg(&fixture.store)
            .arg("--input")
            .arg(&input)
            .output()
            .expect("record batch");
        assert!(output.status.success(), "{output:?}");
        let events = fixture.events();
        let records: Vec<_> = events
            .iter()
            .filter(|event| event.operation == "record")
            .collect();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].output.count, Some(2));
        assert_eq!(records[0].output.ids.len(), 2);
    }
}

#[test]
fn human_and_json_schema_export_preserve_manifest_count_without_payloads() {
    for json in [false, true] {
        let fixture = Fixture::new();
        let destination = fixture.root.join("portable-types");
        let mut command = fixture.command();
        if json {
            command.arg("--json");
        }
        let output = command
            .args(["schema", "export", "--store"])
            .arg(&fixture.store)
            .arg("--output")
            .arg(&destination)
            .output()
            .expect("schema export");
        assert!(output.status.success(), "{output:?}");
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(destination.join("manifest.json")).expect("manifest"),
        )
        .expect("manifest JSON");
        let count = manifest["entries"].as_array().expect("entries").len() as u64;
        assert!(count > 0);
        let events = fixture.events();
        let exports: Vec<_> = events
            .iter()
            .filter(|event| event.operation == "schema.export")
            .collect();
        assert_eq!(exports.len(), 1);
        assert_eq!(exports[0].output.count, Some(count));
        assert!(exports[0].output.ids.is_empty());
    }
}

#[test]
fn human_record_and_jsonl_llm_search_preserve_structured_ids_and_counts() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args(["record", "--store"])
        .arg(&fixture.store)
        .arg("--input")
        .arg(fixture.root.join("draft.json"))
        .output()
        .expect("record");
    assert!(output.status.success());
    let events = fixture.events();
    let event = events
        .iter()
        .find(|event| event.operation == "record")
        .expect("record event");
    assert_eq!(event.output.ids.len(), 1);
    assert_eq!(event.output.count, Some(1));
    assert_eq!(event.output.durable, Some(true));
    assert!(event.output.receipt_sha256.is_some());
    assert!(String::from_utf8_lossy(&output.stdout).contains(&event.output.ids[0].to_string()));
    equill::projection::rebuild(&fixture.store).expect("settle index");
    for format in ["jsonl", "llm", "text"] {
        let output = fixture
            .command()
            .args([
                "search",
                "--query",
                "synthetic",
                "--strategy",
                "fts",
                "--format",
                format,
                "--store",
            ])
            .arg(&fixture.store)
            .output()
            .expect("search");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let events = fixture.events();
        let event = events
            .iter()
            .rev()
            .find(|event| event.operation == "search")
            .expect("search event");
        assert_eq!(event.output.ids.len(), 2, "{format}");
        assert_eq!(event.output.count, Some(2), "{format}");
    }
}

#[test]
fn human_and_json_import_and_rebuild_retain_numeric_counts() {
    for json in [false, true] {
        let fixture = Fixture::new();
        let input = fixture.root.join("legacy.jsonl");
        std::fs::write(
            &input,
            format!(
                "{}\n",
                serde_json::json!({
                    "id":"synthetic-summary", "ts":"2026-01-01T00:00:00Z",
                    "namespace":"agent.memory", "type":"agent.note.v1", "actor":"source-writer",
                    "observed_at":"2026-01-01T00:00:00Z", "payload":{"text":"synthetic summary"}
                })
            ),
        )
        .expect("legacy source");
        for (operation, count) in [("import", 1), ("rebuild", 2)] {
            let mut command = fixture.command();
            if json {
                command.arg("--json");
            }
            command.arg(operation).arg("--store").arg(&fixture.store);
            if operation == "import" {
                command.arg("--input").arg(&input);
            }
            let output = command.output().expect("command");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let events = fixture.events();
            let event = events
                .iter()
                .find(|event| event.operation == operation)
                .expect("event");
            assert_eq!(event.output.count, Some(count), "{operation} json={json}");
            if operation == "import" {
                assert_eq!(event.output.ids.len(), 1);
            }
        }
    }
}
