//! Payload constraints are write authority, while launch metadata is not.
use super::harness;
use super::project_grant_fixture::{PM, draft, grant, mcp, record, stderr, store};
use super::{run, write};
use serde_json::{Value, json};
use std::fs;
use std::process::Command;

#[test]
fn project_grant_accepts_only_its_exact_payload_through_cli_and_mcp() {
    let root = store("exact");
    let own = draft(
        "agent.lesson.v1",
        Some(json!(["project-a"])),
        Some(json!("project")),
    );
    let cli = record(&root, PM, "own-cli.json", &own);
    assert!(
        cli.status.success(),
        "own project refused: {}",
        stderr(&cli)
    );
    let finding = draft("agent.finding.v1", Some(json!(["project-a"])), None);
    let response = mcp(&root, PM, json!({ "draft": finding }));
    assert_eq!(response["result"]["isError"], false, "{response}");
    let accepted = equill::record::read_all(&root).expect("ledger").len();

    for (name, project) in [
        ("multi", Some(json!(["project-a", "project-b"]))),
        ("other", Some(json!(["project-b"]))),
        ("empty", Some(json!([]))),
        ("null", Some(Value::Null)),
        ("missing", None),
        ("string", Some(json!("project-a"))),
    ] {
        let value = draft("agent.finding.v1", project, None);
        let out = record(&root, PM, &format!("refused-{name}.json"), &value);
        assert!(!out.status.success(), "{name} escaped the project grant");
    }
    for (name, scope) in [
        ("wrong", Some(json!("other"))),
        ("global", Some(json!("global"))),
        ("missing", None),
    ] {
        let value = draft("agent.lesson.v1", Some(json!(["project-a"])), scope);
        let out = record(&root, PM, &format!("scope-{name}.json"), &value);
        assert!(
            !out.status.success(),
            "{name} scope escaped the lesson grant"
        );
    }
    let outside = draft(
        "agent.process.v1",
        Some(json!(["project-a"])),
        Some(json!("project")),
    );
    assert!(
        !record(&root, PM, "outside-type.json", &outside)
            .status
            .success()
    );
    assert_eq!(
        equill::record::read_all(&root).expect("ledger").len(),
        accepted
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn routing_metadata_cannot_spoof_scope_or_cross_project_history() {
    let root = store("spoof");
    let other = draft(
        "agent.lesson.v1",
        Some(json!(["project-b"])),
        Some(json!("project")),
    );
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

    let mut replacement = draft(
        "agent.lesson.v1",
        Some(json!(["project-a"])),
        Some(json!("project")),
    );
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
    let own = draft(
        "agent.lesson.v1",
        Some(json!(["project-a"])),
        Some(json!("project")),
    );

    let cli = record(&root, "lane", "lane.json", &own);
    assert!(!cli.status.success());
    let response = mcp(&root, "lane", json!({ "draft": own }));
    assert_eq!(response["result"]["isError"], true, "{response}");
    let _ = fs::remove_dir_all(root);
}
