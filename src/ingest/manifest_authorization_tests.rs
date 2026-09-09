use super::{import_manifest, manifest::import_resolved};
use crate::record::read_all;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn store() -> PathBuf {
    let root = std::env::temp_dir().join(format!("equill-manifest-auth-{}", uuid::Uuid::now_v7()));
    crate::command::init::create(&root, "writer", "agent.memory").unwrap();
    crate::schema::register(&root, crate::schema::TypeDefinition {
        type_name: "agent.lesson.v1".into(), uri: "equill://agent.lesson/v1".into(), owner: "writer".into(),
        payload_schema: json!({"type":"object","properties":{"rule":{"type":"string"}},"required":["rule"]}),
        lifecycle: Default::default(),
    }, "writer").unwrap();
    root
}

fn configure(root: &Path) {
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("store.json")).unwrap()).unwrap();
    config["writers"] = json!(["legacy", "reader"]);
    config["read_only"] = json!(["reader"]);
    config["write_grants"] =
        json!([{"actors":["scoped"],"namespace":"agent.memory","types":["agent.lesson.v1"]}]);
    fs::write(root.join("store.json"), config.to_string()).unwrap();
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, at: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            let name = path.strip_prefix(root).unwrap().to_owned();
            if path.is_dir() {
                files.insert(name, Vec::new());
                visit(root, &path, files);
            } else {
                files.insert(name, fs::read(path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

#[test]
fn denied_empty_manifests_leave_every_store_byte_unchanged() {
    let root = store();
    configure(&root);
    let manifest = root.join("empty-manifest.jsonl");
    fs::write(&manifest, b"").unwrap();
    let before = snapshot(&root);
    for actor in ["guest", "reader", "scoped"] {
        assert!(import_manifest(&root, &manifest, actor).is_err());
        assert!(import_resolved(&root, b"", Vec::new(), actor).is_err());
        assert_eq!(snapshot(&root), before, "actor {actor} changed the store");
    }
    assert!(!root.join("receipts/imports").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn store_writers_can_publish_and_repeat_a_legitimate_empty_manifest() {
    let root = store();
    configure(&root);
    let first = import_resolved(&root, b"", Vec::new(), "writer").unwrap();
    let again = import_resolved(&root, b"", Vec::new(), "legacy").unwrap();
    assert_eq!(first.receipt, again.receipt);
    assert_eq!((first.inputs, first.imported, first.skipped), (0, 0, 0));
    assert_eq!(
        fs::read_dir(root.join("receipts/imports")).unwrap().count(),
        1
    );
    assert!(read_all(&root).unwrap().is_empty());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn scoped_nonempty_manifests_still_import_and_repeat() {
    let root = store();
    configure(&root);
    let input = root.join("synthetic.jsonl");
    fs::write(
        &input,
        format!(
            "{}\n",
            json!({"id":"synthetic-one","ts":"2026-01-01T00:00:00Z",
        "actor":"legacy-author","namespace":"agent.memory","type":"agent.lesson.v1",
        "observed_at":"2026-01-01T00:00:00Z","payload":{"rule":"synthetic scoped import"}})
        ),
    )
    .unwrap();
    let manifest = root.join("manifest.jsonl");
    fs::write(&manifest, "{\"path\":\"synthetic.jsonl\"}\n").unwrap();
    assert_eq!(
        import_manifest(&root, &manifest, "scoped")
            .unwrap()
            .imported,
        1
    );
    assert_eq!(
        import_manifest(&root, &manifest, "scoped").unwrap().skipped,
        1
    );
    fs::remove_dir_all(root).unwrap();
}
