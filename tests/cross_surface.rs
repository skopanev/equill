//! The same question, asked three ways.
//!
//! A caller reaches context through the CLI as JSON, through the CLI as text,
//! or through an MCP session. Those are three surfaces over one selection, and
//! a difference between them is a difference nobody asked for: the same
//! request must choose the same records, whichever door it came through.
//!
//! Black box on purpose. Nothing here reaches into the matcher; every claim is
//! made from what a caller can actually see — the ids a JSON answer names, the
//! records a text answer prints, the ids an MCP result carries.
mod harness;

use harness::session::Session;
use harness::{equill, store, write_line};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::path::Path;

/// Roles the fixture writes, and what each one is for.
///
/// `null` stands for a record that names no role at all: under
/// `set_or_wildcard` it is universal and must come back for every request.
/// The named ones are what a request can ask for, one at a time or several.
const ROLES: [Option<&str>; 5] = [None, Some("pm"), Some("gm"), Some("lane"), Some("backend")];

#[test]
fn every_surface_selects_the_same_records_for_a_scalar_role() {
    let root = fixture("scalar");
    let asked = &["project=finik", "role=pm"];

    let json = cli_json(&root, asked);
    let mcp = mcp_ids(&root, asked);
    let text = cli_text(&root, asked);

    assert_eq!(json, mcp, "CLI JSON and MCP disagree on a scalar role");
    assert_eq!(
        text, json,
        "CLI text named a different set than the JSON answer"
    );
    assert!(!json.is_empty(), "the fixture selected nothing at all");
    let _ = std::fs::remove_dir_all(root);
}

/// A request naming several roles. Written as one comma-separated coordinate,
/// which is how both the CLI and MCP express a set.
#[test]
fn every_surface_selects_the_same_records_for_a_role_set() {
    let root = fixture("set");
    for asked in [
        &["project=finik", "role=lane,backend"],
        &["project=finik", "role=lane,kyc"],
    ] {
        let json = cli_json(&root, asked);
        let mcp = mcp_ids(&root, asked);
        let text = cli_text(&root, asked);

        assert_eq!(json, mcp, "CLI JSON and MCP disagree on {asked:?}");
        assert_eq!(
            text, json,
            "CLI text named a different set than the JSON answer for {asked:?}"
        );
    }
    let _ = std::fs::remove_dir_all(root);
}

/// A role nobody wrote. Every surface must answer with the roleless records
/// alone — and, whatever that number is, answer it identically.
#[test]
fn every_surface_agrees_on_a_role_no_record_carries() {
    let root = fixture("absent");
    let asked = &["project=finik", "role=nobody"];

    let json = cli_json(&root, asked);
    let mcp = mcp_ids(&root, asked);

    assert_eq!(json, mcp, "the surfaces disagree about an unmatched role");
    let _ = std::fs::remove_dir_all(root);
}

/// The ids a JSON answer names.
fn cli_json(root: &Path, coordinates: &[&str]) -> BTreeSet<String> {
    let mut args = vec!["context", "--profile", "roles", "--json"];
    for entry in coordinates {
        args.push("--coordinate");
        args.push(entry);
    }
    let out = equill(root, &args);
    assert!(
        out.status.success(),
        "cli json failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let answer: Value = serde_json::from_slice(&out.stdout).expect("cli json");
    answer["selected_record_ids"]
        .as_array()
        .expect("selected ids")
        .iter()
        .map(|id| id.as_str().expect("id").to_owned())
        .collect()
}

/// The ids a text answer prints. Text puts the record id first, so the three
/// surfaces can be compared on the same thing rather than on a rendering.
fn cli_text(root: &Path, coordinates: &[&str]) -> BTreeSet<String> {
    let mut args = vec!["context", "--profile", "roles", "--format", "text"];
    for entry in coordinates {
        args.push("--coordinate");
        args.push(entry);
    }
    let out = equill(root, &args);
    assert!(
        out.status.success(),
        "cli text failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.split('\t')
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned()
        })
        .collect()
}

/// The ids an MCP session names, through a real session rather than a helper.
fn mcp_ids(root: &Path, coordinates: &[&str]) -> BTreeSet<String> {
    let mut session = Session::open(root);
    let (_, response) = session.tool(
        "context",
        json!({
            "profile": "roles",
            "coordinates": coordinates.iter().map(|item| json!(item)).collect::<Vec<_>>()
        }),
    );
    assert!(
        response["error"].is_null(),
        "mcp context failed: {response}"
    );
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("mcp text");
    let answer: Value = serde_json::from_str(text).expect("mcp json");
    answer["selected_record_ids"]
        .as_array()
        .expect("selected ids")
        .iter()
        .map(|id| id.as_str().expect("id").to_owned())
        .collect()
}

/// One record per role, plus a second roleless one so "universal" is more than
/// a single row, and one record in another project so the project coordinate
/// has something to exclude.
fn fixture(name: &str) -> std::path::PathBuf {
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
    harness::write_json(
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
        harness::write_json(&path, &body);
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

/// What the surfaces agree ON, not just that they agree.
///
/// Agreement is necessary and not sufficient: three surfaces over one matcher
/// agree on a wrong answer as readily as on a right one. This pins the answer
/// itself. The fixture writes two records with no role, one for each named
/// role, and one in another project.
///
/// A request naming one role must return the roleless records and that role.
/// A request naming several must return the roleless records and every named
/// role it asked for — a set is a list of alternatives, not a narrower filter.
#[test]
fn a_role_set_returns_every_role_it_names() {
    let root = fixture("membership");
    let roleless = 2;

    let pm = cli_json(&root, &["project=finik", "role=pm"]);
    let pair = cli_json(&root, &["project=finik", "role=lane,backend"]);
    let missing = cli_json(&root, &["project=finik", "role=lane,kyc"]);

    assert_eq!(
        pm.len(),
        roleless + 1,
        "a scalar role must return the roleless records and its own"
    );
    assert_eq!(
        pair.len(),
        roleless + 2,
        "a set naming two roles the store holds must return both, not neither"
    );
    assert_eq!(
        missing.len(),
        roleless + 1,
        "a set naming one role the store holds and one it does not must return the one it holds"
    );
    let _ = std::fs::remove_dir_all(root);
}
