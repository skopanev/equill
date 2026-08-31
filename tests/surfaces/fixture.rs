//! A store shaped to ask about roles, and nothing else.
use crate::harness::{equill, store, write_json, write_line};
use serde_json::json;
use std::path::Path;

/// Roles the fixture writes, and what each one is for.
///
/// `null` stands for a record that names no role at all: under
/// `set_or_wildcard` it is universal and must come back for every request.
/// The named ones are what a request can ask for, one at a time or several.
pub const ROLES: [Option<&str>; 5] = [None, Some("pm"), Some("gm"), Some("lane"), Some("backend")];

/// Records with no role, so "universal" is more than a single row.
pub const ROLELESS: usize = 2;

/// One record per role, plus a second roleless one so "universal" is more than
/// a single row, and one record in another project so the project coordinate
/// has something to exclude.
pub fn fixture(name: &str) -> std::path::PathBuf {
    let root = store(name);
    let _ = std::fs::remove_file(root.join("registry/vector/qdrant.json"));
    register(&root);
    for (index, role) in ROLES.iter().enumerate() {
        append(&root, index, "finik", *role);
    }
    append(&root, ROLES.len(), "finik", None);
    append(&root, ROLES.len() + 1, "other", Some("pm"));
    root
}

fn append(root: &Path, index: usize, project: &str, role: Option<&str>) {
    let path = root.join(format!("draft-{index}.json"));
    write_line(
        &path,
        &json!({
            "namespace": "agent.memory",
            "type": "agent.roleset.v1",
            "observed_at": "2026-01-01T00:00:00Z",
            "payload": {
                "rule": format!("synthetic rule {index}"),
                "title": format!("record-{index}"),
                "project": project,
                "role": role
            }
        }),
    );
    let out = equill(root, &["record", "--input", path.to_str().expect("path")]);
    assert!(
        out.status.success(),
        "seed {index} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A selector that asks by coordinate alone — no query, no tags — so the
/// answer is decided by the coordinates and nothing else.
fn register(root: &Path) {
    let schema = root.join("roleset.json");
    write_json(
        &schema,
        &json!({
            "type": "agent.roleset.v1",
            "uri": "equill://agent.roleset/v1",
            "owner": "owner",
            "payload_schema": {
                "type": "object",
                "properties": {
                    "rule": { "type": "string" },
                    "title": { "type": "string" },
                    "project": { "type": "string" },
                    "role": { "type": ["string", "null"] }
                },
                "required": ["title"],
                "additionalProperties": false
            }
        }),
    );
    let out = equill(
        root,
        &[
            "schema",
            "register",
            "--file",
            schema.to_str().expect("path"),
        ],
    );
    assert!(
        out.status.success(),
        "schema register failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    for (command, name, body) in [
        (
            "selector",
            "selector.json",
            json!({
                "id": "roles",
                "version": "1",
                "type": "agent.roleset.v1",
                "strategies": ["recency"],
                "required_tags": [],
                "core_tags": [],
                "coordinate_pointers": { "project": "/project", "role": "/role" },
                "coordinate_modes": { "project": "set_or_wildcard", "role": "set_or_wildcard" }
            }),
        ),
        (
            "profile",
            "profile.json",
            json!({
                "id": "roles",
                "version": "1",
                "actors": ["*"],
                "grants": [{ "namespace": "agent.memory", "types": ["agent.roleset.v1"] }],
                "selectors": ["roles"],
                "budget": {}
            }),
        ),
    ] {
        let path = root.join(name);
        write_json(&path, &body);
        let out = equill(
            root,
            &[command, "register", "--file", path.to_str().expect("path")],
        );
        assert!(
            out.status.success(),
            "{command} register failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
