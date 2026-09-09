use super::*;
use crate::record::StoredRecord;
use serde_json::json;

#[test]
fn locator_query_uses_only_metadata_and_preserves_order_across_chunks() {
    let root = std::env::temp_dir().join(format!("equill-locator-{}", uuid::Uuid::now_v7()));
    crate::command::init::create(&root, "owner", "agent.memory").unwrap();
    let mut connection = open(&database(&root)).unwrap();
    let transaction = connection.transaction().unwrap();
    let ids: Vec<_> = (0..260).map(|_| uuid::Uuid::now_v7()).collect();
    for id in &ids {
        let record = StoredRecord {
            id: *id,
            namespace: "agent.memory".into(),
            type_name: "agent.note.v1".into(),
            actor: "owner".into(),
            recorded_at: "2026-01-01T00:00:00Z".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: "2026-01-01T00:00:00Z".into(),
            payload: json!({}),
            evidence: vec![],
            tags: vec![],
            supersedes: None,
        };
        // An invalid projected payload must not matter to locator selection.
        transaction.execute(
            "INSERT INTO records(id,namespace,type_name,actor,recorded_at,observed_at,valid_at,payload_json,evidence_json,tags_json,supersedes,record_sha256,ledger) VALUES (?1,?2,?3,?4,?5,?5,?5,'not JSON','[]','[]',NULL,?6,?7)",
            rusqlite::params![record.id.to_string(), record.namespace, record.type_name, record.actor, record.recorded_at, "a".repeat(64), "records/2026-01.jsonl"],
        ).unwrap();
    }
    transaction.commit().unwrap();
    let mut reversed = ids.clone();
    reversed.reverse();
    reversed.push(uuid::Uuid::now_v7());
    let report = locators(&root, &LocatorRequest { ids: reversed }).unwrap();
    assert_eq!(
        report
            .located
            .iter()
            .map(|item| item.record_id)
            .collect::<Vec<_>>(),
        ids.iter().rev().copied().collect::<Vec<_>>()
    );
    assert!(
        report
            .located
            .iter()
            .all(|item| item.ledger == "records/2026-01.jsonl")
    );
    assert!(
        locators(
            &root,
            &LocatorRequest {
                ids: vec![ids[0], ids[0]]
            }
        )
        .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}
