use super::super::VectorSearchRequest;
use super::super::hydrate::{from_ledger, test_hydrate};
use super::super::provider::qdrant::ProviderHit;
use crate::kernel::digest::sha256_hex;
use crate::record::StoredRecord;
use crate::record::{RecordDraft, append};
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::fs;
use uuid::Uuid;

#[test]
fn valid_candidate_returns_the_canonical_record() {
    let record = stored();
    let hits = test_hydrate(&request(), vec![candidate(&record)], vec![record.clone()])
        .expect("canonical hit");

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].record.id, record.id);
    assert_eq!(hits[0].record.payload, record.payload);
}

#[test]
fn public_hydration_reads_the_immutable_ledger() {
    let root = super::support::root("hydrate-ledger");
    crate::command::init::create(&root, "writer", "agent.memory").expect("store");
    schema::register(
        &root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "schema-owner".into(),
            payload_schema: json!({ "type": "object" }),
            lifecycle: Default::default(),
        },
        "writer",
    )
    .expect("schema");
    let report = append(
        &root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-08-29T10:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": "ledger truth" }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: None,
        },
        "writer",
    )
    .expect("record");
    let record = crate::record::read_all(&root).expect("ledger").remove(0);
    let hits = from_ledger(&root, &request(), vec![candidate(&record)]).expect("hydrate");

    assert_eq!(hits[0].record.id, report.id);
    assert_eq!(hits[0].record.payload, json!({ "rule": "ledger truth" }));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn absent_or_non_v7_candidate_is_rejected() {
    let record = stored();
    let mut absent = candidate(&record);
    absent.record_id = Uuid::now_v7();
    assert!(test_hydrate(&request(), vec![absent], vec![record.clone()]).is_err());

    let mut invalid = candidate(&record);
    invalid.record_id = Uuid::nil();
    assert!(test_hydrate(&request(), vec![invalid], vec![record]).is_err());
}

#[test]
fn record_sha_and_requested_filters_are_rechecked() {
    let record = stored();
    let mut wrong_sha = candidate(&record);
    wrong_sha.record_sha256 = "f".repeat(64);
    assert!(test_hydrate(&request(), vec![wrong_sha], vec![record.clone()]).is_err());

    let mut wrong_filter = request();
    wrong_filter.namespaces = vec!["other.namespace".into()];
    assert!(test_hydrate(&wrong_filter, vec![candidate(&record)], vec![record]).is_err());
}

fn candidate(record: &StoredRecord) -> ProviderHit {
    ProviderHit {
        store_id: Uuid::now_v7(),
        model_sha256: "a".repeat(64),
        record_id: record.id,
        score: 0.8,
        record_sha256: sha256_hex(&serde_json::to_vec(record).expect("record JSON")),
        input_sha256: "b".repeat(64),
    }
}

fn request() -> VectorSearchRequest {
    VectorSearchRequest {
        vector: vec![0.1, 0.2, 0.3],
        namespaces: vec!["agent.memory".into()],
        type_names: vec!["agent.lesson.v1".into()],
        limit: 10,
    }
}

fn stored() -> StoredRecord {
    StoredRecord {
        id: Uuid::now_v7(),
        namespace: "agent.memory".into(),
        type_name: "agent.lesson.v1".into(),
        actor: "writer".into(),
        recorded_at: "2026-08-29T10:00:00Z".into(),
        observed_at: "2026-08-29T10:00:00Z".into(),
        valid_at: "2026-08-29T10:00:00Z".into(),
        payload: json!({ "rule": "synthetic only" }),
        evidence: Vec::new(),
        tags: vec!["test".into()],
        supersedes: None,
    }
}
