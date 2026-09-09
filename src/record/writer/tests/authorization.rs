use super::super::seam::{self, Step};
use crate::{
    ingest::import_jsonl,
    record::{read_all, tests::store},
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

fn input(root: &Path, id: &str) -> PathBuf {
    let path = root.join(format!("synthetic-{id}.jsonl"));
    fs::write(
        &path,
        format!(
            "{}\n",
            json!({"id":id,"ts":"2026-01-01T00:00:00Z",
        "actor":"legacy-writer","namespace":"agent.memory","type":"agent.lesson.v1",
        "observed_at":"2026-01-01T00:00:00Z","payload":{"rule":"synthetic scope"}})
        ),
    )
    .unwrap();
    path
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, at: &Path, found: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                found.insert(path.strip_prefix(root).unwrap().to_owned(), Vec::new());
                visit(root, &path, found);
            } else {
                found.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut found = BTreeMap::new();
    visit(root, root, &mut found);
    found
}

fn configure(root: &Path) {
    let mut config: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("store.json")).unwrap()).unwrap();
    config["writers"] = json!(["reader"]);
    config["read_only"] = json!(["reader"]);
    config["write_grants"] = json!([
        {"actors":["inside"],"namespace":"agent.memory","types":["agent.lesson.v1"],"payload_equals":{"/rule":"synthetic scope"}},
        {"actors":["outside-type"],"namespace":"agent.memory","types":["other.record.v1"]},
        {"actors":["outside-payload"],"namespace":"agent.memory","types":["agent.lesson.v1"],"payload_equals":{"/rule":"different scope"}}
    ]);
    fs::write(root.join("store.json"), config.to_string()).unwrap();
}

fn no_spawn(_: &Path) -> Result<(), crate::kernel::error::Error> {
    panic!("unauthorized import tried to spawn a worker");
}

#[test]
fn denied_reimports_cannot_recover_or_repair_any_store_bytes() {
    let root = store();
    let original = input(&root, "original");
    import_jsonl(&root, &original, "writer").unwrap();
    let pending = input(&root, "pending");
    seam::fail(Some(Step::BeforeAppend));
    assert!(import_jsonl(&root, &pending, "writer").is_err());
    seam::fail(None);
    assert!(root.join("transactions/batch.json").exists());
    fs::remove_file(root.join("projections/sqlite/watermark.json")).unwrap();
    configure(&root);
    let before = snapshot(&root);
    for actor in ["reader", "revoked", "outside-type", "outside-payload"] {
        let error = crate::vector::catchup::starter::with_starter(no_spawn, || {
            import_jsonl(&root, &original, actor)
        })
        .unwrap_err();
        assert!(error.to_string().contains("not allowed"), "{error}");
        assert_eq!(snapshot(&root), before, "actor {actor} mutated store");
    }
    let report = import_jsonl(&root, &original, "inside").unwrap();
    assert_eq!((report.imported, report.skipped), (0, 1));
    assert!(!root.join("transactions/batch.json").exists());
    assert_eq!(read_all(&root).unwrap().len(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_scoped_writer_can_import_and_repeat_its_own_payload() {
    let root = store();
    configure(&root);
    let source = input(&root, "scoped");
    let imported = import_jsonl(&root, &source, "inside").unwrap();
    assert_eq!(imported.imported, 1);
    assert_eq!(import_jsonl(&root, &source, "inside").unwrap().skipped, 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn every_skipped_source_line_still_requires_its_own_scope() {
    let root = store();
    let source = input(&root, "first");
    let second = input(&root, "second");
    let mut outside: serde_json::Value =
        serde_json::from_slice(&fs::read(second).unwrap()).unwrap();
    outside["payload"]["rule"] = json!("outside scope");
    fs::write(
        &source,
        format!("{}{outside}\n", fs::read_to_string(&source).unwrap()),
    )
    .unwrap();
    import_jsonl(&root, &source, "writer").unwrap();
    configure(&root);
    fs::remove_file(root.join("projections/sqlite/watermark.json")).unwrap();
    let before = snapshot(&root);
    let error = crate::vector::catchup::starter::with_starter(no_spawn, || {
        import_jsonl(&root, &source, "inside")
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("line 2:"), "{error}");
    assert_eq!(snapshot(&root), before);
    fs::remove_dir_all(root).unwrap();
}

fn pending_store() -> PathBuf {
    let root = store();
    let source = input(&root, "pending");
    seam::fail(Some(Step::BeforeAppend));
    assert!(import_jsonl(&root, &source, "writer").is_err());
    seam::fail(None);
    assert!(root.join("transactions/batch.json").exists());
    configure(&root);
    root
}

fn revoke_scope(root: &Path) {
    let path = root.join("store.json");
    let mut config: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    config["write_grants"] = json!([]);
    fs::write(path, config.to_string()).unwrap();
}

fn assert_only_revocation_changed(root: &Path, mut before: BTreeMap<PathBuf, Vec<u8>>) {
    let mut expected: serde_json::Value =
        serde_json::from_slice(before.get(Path::new("store.json")).unwrap()).unwrap();
    expected["write_grants"] = json!([]);
    before.insert(
        PathBuf::from("store.json"),
        expected.to_string().into_bytes(),
    );
    assert_eq!(snapshot(root), before);
    assert!(root.join("transactions/batch.json").exists());
}

#[test]
fn a_grant_revoked_after_preparation_cannot_trigger_single_write_recovery() {
    for key in [None, Some("synthetic-retry-key")] {
        let root = pending_store();
        let before = snapshot(&root);
        seam::before_lock_once(revoke_scope);
        let error = crate::vector::catchup::starter::with_starter(no_spawn, || {
            crate::record::append_request(&root, super::request(key, "synthetic scope"), "inside")
        })
        .unwrap_err();
        assert!(matches!(
            error,
            crate::kernel::error::Error::PermissionDenied
        ));
        assert_only_revocation_changed(&root, before);
        fs::remove_dir_all(root).unwrap();
    }
}

fn blocked(
    root: &Path,
    actor: &str,
) -> Result<crate::record::AppendReport, crate::kernel::error::Error> {
    super::super::blocked::block_write(
        root,
        &super::draft("synthetic scope"),
        actor,
        "2026-01-01T00:00:00Z",
        "2026-01",
        crate::defense::DefenseResult {
            mode: crate::defense::DefenseMode::Block,
            findings: vec![crate::defense::DefenseFinding {
                rule: "synthetic-rule".into(),
                line: 1,
                column: 1,
            }],
        },
    )
}

#[test]
fn unauthorized_blocked_drafts_cannot_recover_or_publish_receipts() {
    let root = pending_store();
    let before = snapshot(&root);
    for actor in ["reader", "revoked", "outside-type", "outside-payload"] {
        let error =
            crate::vector::catchup::starter::with_starter(no_spawn, || blocked(&root, actor))
                .unwrap_err();
        assert!(matches!(
            error,
            crate::kernel::error::Error::PermissionDenied
                | crate::kernel::error::Error::ReadOnlyActor(_)
        ));
        assert_eq!(snapshot(&root), before);
    }
    seam::before_lock_once(revoke_scope);
    let error =
        crate::vector::catchup::starter::with_starter(no_spawn, || blocked(&root, "inside"))
            .unwrap_err();
    assert!(matches!(
        error,
        crate::kernel::error::Error::PermissionDenied
    ));
    assert_only_revocation_changed(&root, before);
    fs::remove_dir_all(root).unwrap();
}
