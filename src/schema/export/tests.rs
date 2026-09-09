use super::*;
use serde_json::{Value, json};
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "equill-schema-export-test-{}",
            uuid::Uuid::now_v7()
        ));
        crate::command::init::create(&root.join("store"), "owner", "demo.memory").unwrap();
        Self(root)
    }
    fn store(&self) -> PathBuf {
        self.0.join("store")
    }
    fn output(&self) -> PathBuf {
        self.0.join("exported")
    }
    fn put(&self, name: &str, predecessors: &[&str]) {
        let definition = json!({
            "type": name,
            "uri": format!("equill://{}/v1", name.strip_suffix(".v1").unwrap()),
            "owner": "owner",
            "payload_schema": {"type":"object"},
            "lifecycle": {"mode":"dag", "allowed_predecessor_types": predecessors}
        });
        fs::write(
            self.store().join(format!("registry/types/{name}.json")),
            serde_json::to_vec(&definition).unwrap(),
        )
        .unwrap();
    }
    fn seed(&self) {
        for suffix in ["a", "b", "c", "d"] {
            let old = format!("demo.old{suffix}.v1");
            self.put(&old, &[]);
            self.put(&format!("demo.current{suffix}.v1"), &[&old]);
        }
        self.put("demo.currente.v1", &[]);
        self.put("demo.currentf.v1", &[]);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, path: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(
                    path.strip_prefix(root).unwrap().into(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn default_and_full_exports_preserve_definitions_and_exact_file_hashes() {
    let fixture = Fixture::new();
    fixture.seed();
    fs::write(
        fixture.store().join("records/private.jsonl"),
        b"synthetic-record-sentinel",
    )
    .unwrap();
    let before = tree(&fixture.store());
    let report = export(&fixture.store(), &fixture.output(), false).unwrap();
    assert_eq!((report.exported, report.current, report.legacy), (6, 6, 0));
    let default = tree(&fixture.output());
    assert_eq!(default.len(), 7);
    assert!(
        !default
            .keys()
            .any(|path| path.to_string_lossy().contains("demo.old"))
    );
    // References are part of the immutable schema, not silently removed history.
    let current: Value =
        serde_json::from_slice(&default[Path::new("demo.currenta.v1.json")]).unwrap();
    assert_eq!(
        current["lifecycle"]["allowed_predecessor_types"],
        json!(["demo.olda.v1"])
    );
    let second = fixture.0.join("second");
    export(&fixture.store(), &second, false).unwrap();
    assert_eq!(tree(&second), default);
    let full = fixture.0.join("full");
    let report = export(&fixture.store(), &full, true).unwrap();
    assert_eq!((report.exported, report.current, report.legacy), (10, 6, 4));
    let files = tree(&full);
    assert_eq!(files.len(), 11);
    let manifest: Value = serde_json::from_slice(&files[Path::new("manifest.json")]).unwrap();
    assert_eq!(manifest["schema"], "equill.schema-export.v1");
    let entries = manifest["entries"].as_array().unwrap();
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry["status"] == "legacy")
            .count(),
        4
    );
    let mut names = Vec::new();
    for entry in entries {
        let name = entry["type"].as_str().unwrap();
        names.push(name);
        let bytes = &files[Path::new(entry["filename"].as_str().unwrap())];
        assert_eq!(entry["sha256"], sha256_hex(bytes));
        let exported: TypeDefinition = serde_json::from_slice(bytes).unwrap();
        assert_eq!(
            exported,
            crate::schema::load(&fixture.store(), name).unwrap()
        );
    }
    assert!(names.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(tree(&fixture.store()), before);
    for bytes in files.values() {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains("synthetic-record-sentinel"));
        assert!(!text.contains(fixture.0.to_str().unwrap()));
    }
}

#[test]
fn invalid_or_ambiguous_registry_never_publishes_partial_output() {
    for kind in [
        "ambiguous",
        "cycle",
        "missing",
        "malformed",
        "filename",
        "symlink",
    ] {
        let fixture = Fixture::new();
        fixture.put("demo.old.v1", &[]);
        fixture.put("demo.current.v1", &["demo.old.v1"]);
        let path = fixture.store().join("registry/types/demo.current.v1.json");
        match kind {
            "ambiguous" => fixture.put("demo.branch.v1", &["demo.old.v1"]),
            "cycle" => fixture.put("demo.old.v1", &["demo.current.v1"]),
            "missing" => fixture.put("demo.current.v1", &["demo.missing.v1"]),
            "malformed" => fs::write(&path, b"{ synthetic-private-malformed }").unwrap(),
            "filename" => fs::rename(&path, path.with_file_name("wrong.json")).unwrap(),
            "symlink" => {
                let saved = fixture.0.join("source.json");
                fs::rename(&path, &saved).unwrap();
                std::os::unix::fs::symlink(saved, &path).unwrap();
            }
            _ => unreachable!(),
        }
        for all in [false, true] {
            let error = export(&fixture.store(), &fixture.output(), all)
                .unwrap_err()
                .to_string();
            assert!(!fixture.output().exists(), "{kind}");
            assert!(!error.contains("synthetic-private-malformed"));
            assert!(!error.contains(fixture.0.to_str().unwrap()));
        }
        assert!(!fs::read_dir(&fixture.0).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".equill-schema-export-")
        }));
    }
}

#[test]
fn export_refuses_existing_and_source_destinations_without_overwriting() {
    let fixture = Fixture::new();
    fixture.seed();
    fs::create_dir(fixture.output()).unwrap();
    fs::write(fixture.output().join("keep"), b"unchanged").unwrap();
    assert!(export(&fixture.store(), &fixture.output(), false).is_err());
    assert_eq!(
        fs::read(fixture.output().join("keep")).unwrap(),
        b"unchanged"
    );
    let before = tree(&fixture.store());
    assert!(export(&fixture.store(), &fixture.store().join("new"), false).is_err());
    assert_eq!(tree(&fixture.store()), before);
}

#[test]
fn cli_export_is_not_a_vector_resume_trigger() {
    use clap::Parser;
    let fixture = Fixture::new();
    fixture.seed();
    let store = fixture.store();
    let output = fixture.output();
    let args = [
        "equill",
        "--json",
        "schema",
        "export",
        "--store",
        store.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ];
    let parsed = crate::command::cli::Cli::try_parse_from(args).unwrap();
    assert!(parsed.command.store_to_resume().is_none());
    let before = tree(&store);
    let result: Value = serde_json::from_str(&crate::run(args).unwrap()).unwrap();
    assert_eq!(result["exported"], 6);
    assert_eq!(tree(&store), before);
}
