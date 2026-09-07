use super::harness::session::Session;
use super::{run, write};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Output;

pub const PM: &str = "project-a-pm";

pub fn store(name: &str) -> PathBuf {
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
                    "project": {
                        "type": ["array", "null"],
                        "items": { "type": "string" }
                    },
                    "scope": { "type": ["string", "null"] }
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

pub fn grant(root: &Path, actor: &str) {
    grant_one(
        root,
        actor,
        "agent.lesson.v1",
        &["/project=[\"project-a\"]", "/scope=\"project\""],
    );
    grant_one(
        root,
        actor,
        "agent.finding.v1",
        &["/project=[\"project-a\"]"],
    );
}

fn grant_one(root: &Path, actor: &str, type_name: &str, constraints: &[&str]) {
    let mut args = vec![
        "grant",
        "add",
        "--actor",
        actor,
        "--namespace",
        "agent.memory",
        "--types",
        type_name,
    ];
    for constraint in constraints {
        args.push("--payload-equals-json");
        args.push(constraint);
    }
    let out = run(root, "owner", &args);
    assert!(out.status.success(), "grant failed: {}", stderr(&out));
}

pub fn draft(type_name: &str, project: Option<Value>, scope: Option<Value>) -> Value {
    let mut value = json!({
        "namespace": "agent.memory",
        "type": type_name,
        "observed_at": "2026-01-01T00:00:00Z",
        "payload": { "rule": "synthetic scoped fact" }
    });
    if let Some(project) = project {
        value["payload"]["project"] = project;
    }
    if let Some(scope) = scope {
        value["payload"]["scope"] = scope;
    }
    value
}

pub fn record(root: &Path, actor: &str, name: &str, draft: &Value) -> Output {
    let input = write(root, name, draft.clone());
    run(
        root,
        actor,
        &["record", "--json", "--input", input.to_str().expect("path")],
    )
}

pub fn mcp(root: &Path, actor: &str, arguments: Value) -> Value {
    let mut session = Session::open_as(root, actor);
    let (_, response) = session.tool("record", arguments);
    response
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
