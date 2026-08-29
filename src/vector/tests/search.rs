use super::super::model::{VectorSearchHit, VectorSearchRequest};
use super::super::{
    QueryEmbedder, SearchStrategy, VectorIndex, canonical, corpus, retrieve,
    search as strategy_search,
};
use crate::command::init;
use crate::kernel::error::Error;
use crate::projection::SearchRequest;
use crate::record::{RecordDraft, StoredRecord, append};
use crate::schema::{self, TypeDefinition};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

struct FakeIndex(Vec<VectorSearchHit>);
struct FakeEmbedder;

impl VectorIndex for FakeIndex {
    fn search(&self, _: &VectorSearchRequest) -> Result<Vec<VectorSearchHit>, Error> {
        Ok(self.0.clone())
    }
}

impl QueryEmbedder for FakeEmbedder {
    fn embed_query(&self, _: &str) -> Result<Vec<f32>, Error> {
        Ok(vec![0.0, 1.0, 0.0])
    }
}

/// Provider hydration already refuses a point that names no record here or whose
/// record hash moved. What is left to the operator is the embedding input: a
/// point embedded from text the record no longer carries is stale and must be
/// dropped, or a writable index becomes a way to put words in the ledger's mouth.
#[test]
fn only_hits_the_ledger_still_backs_are_returned() {
    let root = store("verify");
    let real = add(&root, "Always run the build checks before merging.");
    let (ledger, _) = corpus(&root).expect("corpus");
    let (record, digest) = ledger
        .iter()
        .find(|(item, _)| item.id == real)
        .expect("stored");
    let truth = canonical(record, digest).expect("canonical");

    let hits = vec![
        hit(record.clone(), &truth.input_sha256),
        hit(record.clone(), &"e".repeat(64)),
    ];
    let verified = retrieve(
        &FakeIndex(hits),
        &FakeEmbedder,
        "how do I verify a change",
        request(10),
    )
    .expect("retrieve");

    assert_eq!(verified.records.len(), 1);
    assert_eq!(verified.records[0].id, real);
    let reasons: Vec<_> = verified.rejected.iter().map(|item| item.reason).collect();
    assert_eq!(reasons, vec!["embedding input is stale"]);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn an_empty_query_is_refused_before_the_index_is_asked() {
    let root = store("empty");

    let error =
        retrieve(&FakeIndex(Vec::new()), &FakeEmbedder, "  ", request(5)).expect_err("empty query");

    assert!(error.to_string().contains("requires a query"));
    fs::remove_dir_all(root).expect("cleanup");
}

/// Hybrid is the strategy a caller reaches for when it wants semantics but can
/// live without them. The point of the report is that living without them is
/// never silent: the answer says it came from text and why.
#[test]
fn hybrid_falls_back_to_text_and_says_so_while_vector_refuses() {
    let root = store("fallback");
    add(&root, "Always run the build checks before merging.");
    let request = SearchRequest {
        query: Some("build checks".into()),
        namespace: None,
        type_name: None,
        limit: 10,
    };

    let hybrid = strategy_search(&root, &request, SearchStrategy::Hybrid).expect("hybrid");
    let plain = strategy_search(&root, &request, SearchStrategy::Fts).expect("fts");
    let strict = strategy_search(&root, &request, SearchStrategy::Vector)
        .expect_err("vector must not answer without an index");

    assert_eq!(hybrid.answered_by, "fts");
    assert_eq!(hybrid.hits.len(), 1);
    let reason = hybrid
        .fallback
        .expect("the receipt must name the degradation");
    assert!(
        reason.contains("not ready") || reason.contains("not configured"),
        "{reason}"
    );
    assert_eq!(plain.answered_by, "fts");
    assert!(
        plain.fallback.is_none(),
        "a plain text search is not a fallback"
    );
    assert!(strict.to_string().contains("vector"));
    fs::remove_dir_all(root).expect("cleanup");
}

fn request(limit: u16) -> VectorSearchRequest {
    VectorSearchRequest {
        vector: Vec::new(),
        namespaces: Vec::new(),
        type_names: Vec::new(),
        limit,
    }
}

fn hit(record: StoredRecord, input_sha256: &str) -> VectorSearchHit {
    VectorSearchHit {
        record,
        score: 0.9,
        input_sha256: input_sha256.into(),
    }
}

fn store(name: &str) -> PathBuf {
    let root = super::support::root(name);
    init::create(&root, "owner", "agent.memory").expect("initialize");
    schema::register(
        &root,
        TypeDefinition {
            type_name: "agent.lesson.v1".into(),
            uri: "equill://agent.lesson/v1".into(),
            owner: "owner".into(),
            payload_schema: json!({
                "$schema": "https://json-schema.org/draft/2020-12/schema",
                "type": "object",
                "properties": { "rule": { "type": "string" } },
                "required": ["rule"],
                "additionalProperties": false
            }),
            lifecycle: Default::default(),
        },
        "owner",
    )
    .expect("register schema");
    root
}

fn add(root: &Path, rule: &str) -> Uuid {
    append(
        root,
        RecordDraft {
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            observed_at: "2026-01-01T00:00:00Z".into(),
            valid_at: None,
            payload: json!({ "rule": rule }),
            evidence: Vec::new(),
            tags: Vec::new(),
            supersedes: None,
        },
        "owner",
    )
    .expect("append")
    .id
}
