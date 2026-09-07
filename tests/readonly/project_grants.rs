//! Payload constraints are write authority, while launch metadata is not.
use super::harness;
use super::harness::session::Session;
use super::{run, write};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PM: &str = "project-a-pm";

fn store(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "equill-project-grant-{name}-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ));
    assert!(
        run(
            &root,
            "owner",
            &["init", "--owner", "owner", "--namespace", "agent.memory"]
        )
        .status
        .success()
    );
    for (type_name, uri) in [
        ("agent.lesson.v1", "equill://agent.lesson/v1"),
        ("agent.finding.v1", "equill://agent.finding/v1"),
        ("agent.process.v1", "equill://agent.process/v1"),
    ] {
        let schema = write(
            &root,
            &format!("{type_name}.json"),
            json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "$id": uri,
                "type": "object",
                "additionalProperties": false,
                "required": ["rule"],
                "properties": {
                    "rule": { "type": "string" },
                    "project": { "type": ["string", "null"] }
                },
                "x-equill-envelope": { "namespace": "agent.memory", "type": type_name }
            }),
        );
        let registered = run(
            &root,
            "owner",
            &[
                "schema",
                "register",
                "--file",
                schema.to_str().expect("path"),
            ],
        );
        assert!(
            registered.status.success(),
            "schema {type_name} failed: {}",
            stderr(&registered)
        );
    }
    grant(&root, PM);
    root
}

fn grant(root: &Path, actor: &str) {
    let out = run(
        root,
        "owner",
        &[
            "grant",
            "add",
            "--actor",
            actor,
            "--namespace",
            "agent.memory",
            "--types",
            "agent.lesson.v1,agent.finding.v1",
            "--payload-equals",
            "/project=project-a",
        ],
    );
    assert!(out.status.success(), "grant failed: {}", stderr(&out));
}

fn draft(type_name: &str, project: Option<Value>) -> Value {
    let mut value = json!({
        "namespace": "agent.memory",
        "type": type_name,
        "observed_at": "2026-01-01T00:00:00Z",
        "payload": { "rule": "synthetic scoped fact" }
    });
    if let Some(project) = project {
        value["payload"]["project"] = project;
    }
    value
}

fn record(root: &Path, actor: &str, name: &str, draft: &Value) -> Output {
    let input = write(root, name, draft.clone());
    run(
        root,
        actor,
        &["record", "--json", "--input", input.to_str().expect("path")],
    )
}

fn mcp(root: &Path, actor: &str, arguments: Value) -> Value {
    let mut session = Session::open_as(root, actor);
    let (_, response) = session.tool("record", arguments);
    response
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn project_grant_accepts_only_its_exact_payload_through_cli_and_mcp() {
    let root = store("exact");
    let own = draft("agent.lesson.v1", Some(json!("project-a")));
    let cli = record(&root, PM, "own-cli.json", &own);
    assert!(
        cli.status.success(),
        "own project refused: {}",
        stderr(&cli)
    );
    let finding = draft("agent.finding.v1", Some(json!("project-a")));
    let response = mcp(&root, PM, json!({ "draft": finding }));
    assert_eq!(response["result"]["isError"], false, "{response}");
    let accepted = equill::record::read_all(&root).expect("ledger").len();

    for (name, value) in [
        ("other", draft("agent.lesson.v1", Some(json!("other")))),
        ("missing", draft("agent.lesson.v1", None)),
        ("null", draft("agent.lesson.v1", Some(Value::Null))),
        ("global", draft("agent.lesson.v1", Some(json!("global")))),
        ("type", draft("agent.process.v1", Some(json!("project-a")))),
    ] {
        let out = record(&root, PM, &format!("refused-{name}.json"), &value);
        assert!(!out.status.success(), "{name} escaped the project grant");
    }
    assert_eq!(
        equill::record::read_all(&root).expect("ledger").len(),
        accepted
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn routing_metadata_cannot_spoof_scope_or_cross_project_history() {
    let root = store("spoof");
    let other = draft("agent.lesson.v1", Some(json!("other")));
    let seeded = record(&root, "owner", "other.json", &other);
    let id = serde_json::from_slice::<Value>(&seeded.stdout).expect("receipt")["id"]
        .as_str()
        .expect("id")
        .to_owned();
    let unchanged = equill::record::read_all(&root).expect("ledger").len();

    let input = write(&root, "env-spoof.json", other.clone());
    let spoof = Command::new(harness::binary())
        .args(["record", "--input"])
        .arg(input)
        .arg("--store")
        .arg(&root)
        .env("EQUILL_ACTOR", PM)
        .env("EQUILL_PROJECT", "project-a")
        .env("EQUILL_ROLE", "gm")
        .output()
        .expect("record");
    assert!(
        !spoof.status.success(),
        "routing environment granted authority"
    );
    assert_eq!(
        equill::record::read_all(&root).expect("ledger").len(),
        unchanged
    );
    let response = mcp(
        &root,
        PM,
        json!({ "project": "project-a", "role": "gm", "draft": other }),
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        equill::record::read_all(&root).expect("ledger").len(),
        unchanged
    );

    let mut replacement = draft("agent.lesson.v1", Some(json!("project-a")));
    replacement["supersedes"] = json!(id);
    let out = record(&root, PM, "cross-project.json", &replacement);
    assert!(!out.status.success(), "cross-project supersede succeeded");
    assert_eq!(
        equill::record::read_all(&root).expect("ledger").len(),
        unchanged
    );
    let out = run(&root, PM, &["revoke", "--id", &id]);
    assert!(!out.status.success(), "cross-project revoke succeeded");
    assert_eq!(
        equill::record::read_all(&root).expect("ledger").len(),
        unchanged
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn read_only_actor_stays_rejected_with_a_matching_project_grant() {
    let root = store("read-only");
    grant(&root, "lane");
    assert!(
        run(&root, "owner", &["reader", "add", "--actor", "lane"])
            .status
            .success()
    );
    let own = draft("agent.lesson.v1", Some(json!("project-a")));

    let cli = record(&root, "lane", "lane.json", &own);
    assert!(!cli.status.success());
    let response = mcp(&root, "lane", json!({ "draft": own }));
    assert_eq!(response["result"]["isError"], true, "{response}");
    let _ = fs::remove_dir_all(root);
}
