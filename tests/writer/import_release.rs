//! Finite synthetic acceptance fixture; timing gates run only in release.
#[cfg(not(debug_assertions))]
#[path = "../harness/mod.rs"]
mod harness;

#[cfg(not(debug_assertions))]
#[test]
fn nine_hundred_forty_four_records_commit_within_sixty_seconds() {
    use serde_json::json;
    use std::fs;
    let root = std::env::temp_dir().join(format!("equill-import-release-{}", uuid::Uuid::now_v7()));
    equill::command::init::create(&root, "owner", "agent.memory").unwrap();
    equill::schema::register(&root, equill::schema::TypeDefinition {
        type_name: "agent.lesson.v1".into(), uri: "equill://agent.lesson/v1".into(), owner: "owner".into(),
        payload_schema: json!({"type":"object","properties":{"rule":{"type":"string"}},"required":["rule"]}), lifecycle: Default::default(),
    }, "owner").unwrap();
    let input = root.join("synthetic.jsonl");
    let lines = (0..944).map(|index| format!("{}\n", json!({
        "id":format!("legacy-{index}"),"ts":"2026-01-01T00:00:00Z","actor":"legacy-owner",
        "namespace":"agent.memory","type":"agent.lesson.v1","observed_at":"2026-01-01T00:00:00Z",
        "payload":{"rule":format!("Synthetic benchmark record {index}")}
    }))).collect::<String>();
    fs::write(&input, lines).unwrap();
    let started = std::time::Instant::now();
    let report = equill::ingest::import_jsonl(&root, &input, "owner").unwrap();
    let duration = started.elapsed();
    eprintln!(
        "atomic import: records={} total={duration:?} records_per_second={:.2}",
        report.imported,
        944.0 / duration.as_secs_f64()
    );
    assert!(duration <= std::time::Duration::from_secs(60));
    assert_eq!(report.imported, 944);
    let records = equill::record::read_all(&root).unwrap();
    assert_eq!(records.len(), 944);
    assert_eq!(equill::projection::verify(&root, &records).unwrap(), 944);
    let month = &records[0].recorded_at[..7];
    assert_eq!(
        fs::read_dir(root.join(format!("receipts/writes/{month}")))
            .unwrap()
            .count(),
        944
    );
    let health = equill::command::doctor::report(Some(&root), true, false).unwrap();
    assert!(health.ok, "{health:?}");
    let mut session = harness::session::Session::open(&root);
    let mut timings = Vec::new();
    for (index, predecessor) in records.iter().take(16).enumerate() {
        let (elapsed, response) = session.tool(
            "record",
            json!({
                "idempotency_key":format!("synthetic replacement {index}"),
                "draft":{"namespace":"agent.memory","type":"agent.lesson.v1",
                    "observed_at":"2026-01-01T00:00:00Z","supersedes":predecessor.id,
                    "payload":{"rule":format!("Synthetic replacement {index}")}}
            }),
        );
        assert!(response["error"].is_null(), "{response}");
        assert_ne!(response["result"]["isError"], true, "{response}");
        assert!(
            elapsed <= std::time::Duration::from_millis(250),
            "supersedes MCP call {index}: {elapsed:?}"
        );
        timings.push(elapsed);
    }
    let cold = timings.remove(0);
    timings.sort();
    eprintln!(
        "944-record supersedes MCP: cold={cold:?} warm_p50={:?} warm_p95={:?} max={:?}",
        timings[timings.len() / 2],
        timings[timings.len() - 1],
        timings[timings.len() - 1]
    );
    drop(session);
    fs::remove_dir_all(root).unwrap();
}
