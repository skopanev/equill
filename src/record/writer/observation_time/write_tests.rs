use crate::record::{AppendRequest, append, append_only_request, read_all, tests::store};
use jiff::Timestamp;
use std::fs;

fn future() -> crate::record::RecordDraft {
    let mut draft = crate::record::tests::lesson("synthetic observation");
    draft.observed_at = Timestamp::MAX.to_string();
    draft
}

fn assert_no_commit(root: &std::path::Path) {
    assert!(read_all(root).unwrap().is_empty());
    for path in [
        "receipts/writes",
        "receipts/pending",
        "transactions",
        "projections/qdrant/desired.json",
    ] {
        assert!(!root.join(path).exists(), "unexpected {path}");
    }
}

#[test]
fn fresh_core_and_keyed_writes_reject_future_observations() {
    let root = store();
    let error = append(&root, future(), "writer").unwrap_err().to_string();
    assert!(error.contains("observed_at exceeds writer recorded_at"));
    assert!(!error.contains("synthetic observation"));
    assert_no_commit(&root);
    assert!(
        append_only_request(
            &root,
            AppendRequest {
                draft: future(),
                idempotency_key: Some("new key".into()),
            },
            "writer"
        )
        .is_err()
    );
    assert_no_commit(&root);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn historical_observation_and_future_validity_are_preserved() {
    let root = store();
    let mut draft = crate::record::tests::lesson("historical observation");
    draft.observed_at = "1900-01-01T01:00:00+01:00".into();
    draft.valid_at = Some(Timestamp::MAX.to_string());
    append(&root, draft, "writer").unwrap();
    let mut draft = crate::record::tests::lesson("default validity");
    draft.observed_at = "1900-01-02T00:00:00Z".into();
    append(&root, draft, "writer").unwrap();
    let records = read_all(&root).unwrap();
    assert_eq!(records[0].observed_at, "1900-01-01T01:00:00+01:00");
    assert_eq!(records[0].valid_at, Timestamp::MAX.to_string());
    assert_eq!(records[1].valid_at, records[1].observed_at);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn atomic_import_checks_every_member_before_any_commit_artifact() {
    for bad_index in 0..3 {
        let root = store();
        let path = root.join("input.jsonl");
        let lines = (0..3)
            .map(|index| {
                let mut draft =
                    serde_json::to_value(crate::record::tests::lesson("synthetic import")).unwrap();
                draft["id"] = format!("legacy-{index}").into();
                draft["ts"] = "2026-01-01T00:00:00Z".into();
                draft["actor"] = "legacy".into();
                if index == bad_index {
                    draft["observed_at"] = Timestamp::MAX.to_string().into();
                }
                format!("{draft}\n")
            })
            .collect::<String>();
        fs::write(&path, lines).unwrap();
        let error = crate::ingest::import_jsonl(&root, &path, "writer")
            .unwrap_err()
            .to_string();
        assert!(error.contains("observed_at exceeds writer recorded_at"));
        assert_no_commit(&root);
        fs::remove_dir_all(root).unwrap();
    }
}
