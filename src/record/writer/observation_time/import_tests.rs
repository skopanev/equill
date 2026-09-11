use super::historical_fixture::seed;
use crate::kernel::digest::sha256_hex;
use crate::record::{
    EvidenceRef, read_all,
    tests::{lesson, store},
};
use jiff::Timestamp;
use serde_json::json;
use std::fs;

#[test]
fn pre_fix_import_skip_preserves_record_but_fresh_shadow_import_is_refused() {
    let root = store();
    let source = root.join("source.jsonl");
    let manifest = root.join("manifest.jsonl");
    let mut draft = lesson("synthetic future import");
    draft.observed_at = Timestamp::MAX.to_string();
    let mut legacy = serde_json::to_value(&draft).unwrap();
    legacy["id"] = "synthetic-legacy".into();
    legacy["actor"] = "writer".into();
    legacy["ts"] = "2000-01-01T00:00:00Z".into();
    let line = legacy.to_string();
    draft.evidence.push(EvidenceRef {
        kind: "equill.import.line".into(),
        reference: "legacy-jsonl".into(),
        sha256: Some(sha256_hex(line.as_bytes())),
    });
    let original = seed(&root, draft, "writer", None);
    fs::write(&source, format!("{line}\n")).unwrap();
    fs::write(
        &manifest,
        format!("{}\n", json!({"path":"source.jsonl", "role":"lessons"})),
    )
    .unwrap();
    let before = fs::read(root.join(&original.ledger)).unwrap();
    let source_before = fs::read(&source).unwrap();
    let imported = crate::ingest::import_manifest(&root, &manifest, "writer").unwrap();
    assert_eq!(imported.imported, 0);
    assert_eq!(imported.skipped, 1);
    assert_eq!(read_all(&root).unwrap()[0].id, original.id);
    let error = crate::compact::run(&root, &manifest, true, "writer")
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("observed_at exceeds writer recorded_at"),
        "{error}"
    );
    assert_eq!(fs::read(&source).unwrap(), source_before);
    assert_eq!(fs::read(root.join(&original.ledger)).unwrap(), before);
    assert_eq!(read_all(&root).unwrap().len(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn manifest_keeps_existing_per_input_commit_boundary() {
    let root = store();
    let manifest = root.join("manifest.jsonl");
    for (index, observed) in [
        "1900-01-01T00:00:00Z".to_string(),
        Timestamp::MAX.to_string(),
    ]
    .iter()
    .enumerate()
    {
        let line = json!({"id":format!("legacy-{index}"), "ts":"2000-01-01T00:00:00Z",
            "actor":"legacy", "namespace":"agent.memory", "type":"agent.lesson.v1",
            "observed_at":observed, "payload":{"rule":"synthetic import"}});
        fs::write(
            root.join(format!("source-{index}.jsonl")),
            format!("{line}\n"),
        )
        .unwrap();
    }
    fs::write(&manifest, "{\"path\":\"source-0.jsonl\",\"role\":\"lessons\"}\n{\"path\":\"source-1.jsonl\",\"role\":\"lessons\"}\n").unwrap();
    let error = crate::ingest::import_manifest(&root, &manifest, "writer")
        .unwrap_err()
        .to_string();
    assert!(error.contains("observed_at exceeds writer recorded_at"));
    assert_eq!(read_all(&root).unwrap().len(), 1);
    fs::remove_dir_all(root).unwrap();
}
