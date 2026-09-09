use super::support::{Fixture, snapshot};
use clap::CommandFactory;
use std::collections::BTreeSet;

fn leaves(command: clap::Command, prefix: Vec<String>, result: &mut Vec<Vec<String>>) {
    let children: Vec<_> = command.get_subcommands().cloned().collect();
    if children.is_empty() {
        result.push(prefix);
    } else {
        for child in children {
            if child.get_name() == "audit" {
                continue;
            }
            let mut prefix = prefix.clone();
            prefix.push(child.get_name().into());
            leaves(child, prefix, result);
        }
    }
}

#[test]
fn complete_cli_catalog_has_one_help_success_and_one_parse_failure_per_leaf() {
    let fixture = Fixture::new();
    let mut catalog = Vec::new();
    leaves(
        equill::command::cli::Cli::command(),
        Vec::new(),
        &mut catalog,
    );
    for path in catalog {
        for (flag, success) in [("--help", true), ("--not-an-option", false)] {
            let before = fixture.events().len();
            let mut args: Vec<_> = path.iter().map(String::as_str).collect();
            args.push(flag);
            let output = fixture.cli(&args);
            assert_eq!(output.status.success(), success, "{path:?} {flag}");
            let events = fixture.events();
            assert_eq!(events.len(), before + 1, "{path:?} {flag}");
            let event = events.last().expect("event");
            assert_eq!(event.operation, path.join("."));
            assert_eq!(event.outcome, if success { "success" } else { "error" });
        }
    }
}

#[test]
fn executed_commands_and_errors_are_audited_once_without_touching_project_history() {
    let fixture = Fixture::new();
    let id = fixture.record.to_string();
    let draft = fixture.root.join("draft.json");
    let schema = fixture.root.join("schema.json");
    let selector = fixture.root.join("selector.json");
    let profile = fixture.root.join("profile.json");
    let commands = vec![
        vec!["status"],
        vec!["doctor"],
        vec!["schema", "list"],
        vec!["schema", "show", "--type", "agent.note.v1"],
        vec![
            "schema",
            "register",
            "--file",
            schema.to_str().expect("path"),
        ],
        vec![
            "selector",
            "register",
            "--file",
            selector.to_str().expect("path"),
        ],
        vec![
            "profile",
            "register",
            "--file",
            profile.to_str().expect("path"),
        ],
        vec!["record", "--input", draft.to_str().expect("path")],
        vec!["search", "--query", "synthetic", "--strategy", "fts"],
        vec!["context", "--profile", "reader", "--query", "synthetic"],
        vec!["get", "--id", &id],
        vec!["owner", "show"],
        vec!["grant", "list"],
        vec!["reader", "list"],
        vec!["rebuild"],
        vec!["compact", "--dry-run"],
    ];
    for args in commands {
        let operation =
            if ["schema", "profile", "selector", "owner", "grant", "reader"].contains(&args[0]) {
                args[..2].join(".")
            } else {
                args[0].into()
            };
        let before = fixture
            .events()
            .iter()
            .filter(|e| e.operation == operation)
            .count();
        let output = fixture.scoped(&args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let events = fixture.events();
        let matching: Vec<_> = events.iter().filter(|e| e.operation == operation).collect();
        assert_eq!(matching.len(), before + 1, "{args:?}");
        assert_eq!(matching.last().expect("event").outcome, "success");
    }
    for args in [
        vec!["get", "--id", "invalid"],
        vec!["record", "--input", "missing-synthetic-input"],
        vec!["schema", "show", "--type", "missing.type.v1"],
    ] {
        let before = fixture.events().len();
        assert!(!fixture.scoped(&args).status.success());
        let events = fixture.events();
        assert_eq!(events.len(), before + 1);
        assert_eq!(events.last().expect("event").outcome, "error");
    }
    let events = fixture.events();
    assert_eq!(
        events
            .iter()
            .map(|event| event.id)
            .collect::<BTreeSet<_>>()
            .len(),
        events.len()
    );
    let before = snapshot(&fixture.store);
    for args in [
        vec!["audit", "list"],
        vec!["audit", "stats"],
        vec!["audit", "list", "--limit", "invalid"],
    ] {
        fixture.cli(&args);
        assert_eq!(fixture.events().len(), events.len());
    }
    assert_eq!(snapshot(&fixture.store), before);
    let raw = serde_json::to_string(&events).expect("json");
    assert!(!raw.contains(fixture.root.to_str().expect("path")));
    assert!(!raw.contains("synthetic audit fixture"));
    assert!(
        events
            .iter()
            .all(|event| event.project.as_deref() == Some("demo"))
    );
}

#[test]
fn concurrent_processes_and_restart_keep_exactly_one_event_each() {
    let fixture = Fixture::new();
    let mut children = Vec::new();
    for _ in 0..12 {
        children.push(
            fixture
                .command()
                .args(["status", "--store"])
                .arg(&fixture.store)
                .stdout(std::process::Stdio::null())
                .spawn()
                .expect("spawn"),
        );
    }
    for mut child in children {
        assert!(child.wait().expect("wait").success());
    }
    let events = fixture.events();
    assert_eq!(events.len(), 12);
    assert_eq!(
        events
            .iter()
            .map(|event| event.id)
            .collect::<BTreeSet<_>>()
            .len(),
        12
    );
    let stats = fixture.cli(&["--json", "audit", "stats", "--project", "demo"]);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&stats.stdout).expect("stats")["count"],
        12
    );
    assert_eq!(fixture.events().len(), 12);
}
