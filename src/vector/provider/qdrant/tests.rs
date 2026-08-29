use super::collection::Collection;
use super::point::{physical_id, qdrant_point};
use super::qdrant::{
    CollectionSchema, ProviderHit, ProviderMetadata, ProviderPoint, Query, Transport, sanitized,
};
use super::test_support::{config, point, schema, search};
use crate::kernel::error::Error;
use qdrant_client::{Payload, QdrantError};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

#[derive(Clone, Default)]
struct FakeTransport {
    inner: Arc<Mutex<FakeState>>,
}

#[derive(Default)]
struct FakeState {
    schemas: HashMap<String, CollectionSchema>,
    aliases: HashMap<String, String>,
    points: Vec<ProviderPoint>,
    hits: Vec<ProviderHit>,
    queries: usize,
    fail_retarget: bool,
}

impl Transport for FakeTransport {
    fn collection_schema(&self, name: &str) -> Result<Option<CollectionSchema>, Error> {
        let state = self.inner.lock().unwrap();
        let physical = state.aliases.get(name).map(String::as_str).unwrap_or(name);
        Ok(state.schemas.get(physical).cloned())
    }

    fn create_collection(&self, name: &str, schema: CollectionSchema) -> Result<(), Error> {
        self.inner
            .lock()
            .unwrap()
            .schemas
            .insert(name.into(), schema);
        Ok(())
    }

    fn upsert(&self, _collection: &str, points: &[ProviderPoint]) -> Result<(), Error> {
        self.inner.lock().unwrap().points.extend_from_slice(points);
        Ok(())
    }

    fn metadata(
        &self,
        _collection: &str,
        point_ids: &[Uuid],
    ) -> Result<Vec<ProviderMetadata>, Error> {
        let state = self.inner.lock().unwrap();
        Ok(state
            .points
            .iter()
            .filter(|item| point_ids.contains(&physical_id(item.store_id, item.point.record_id)))
            .map(|item| ProviderMetadata {
                store_id: item.store_id,
                model_sha256: item.model_sha256.clone(),
                record_id: item.point.record_id,
                record_sha256: item.point.record_sha256.clone(),
                input_sha256: item.point.input_sha256.clone(),
            })
            .collect())
    }

    fn query(&self, _query: Query) -> Result<Vec<ProviderHit>, Error> {
        let mut state = self.inner.lock().unwrap();
        state.queries += 1;
        Ok(state.hits.clone())
    }

    fn alias_target(&self, alias: &str) -> Result<Option<String>, Error> {
        Ok(self.inner.lock().unwrap().aliases.get(alias).cloned())
    }

    fn retarget_alias(
        &self,
        alias: &str,
        previous: Option<&str>,
        target: Option<&str>,
    ) -> Result<(), Error> {
        let mut state = self.inner.lock().unwrap();
        if state.fail_retarget || state.aliases.get(alias).map(String::as_str) != previous {
            return Err(crate::vector::model::vector_error("retarget alias failed"));
        }
        match target {
            Some(collection) => {
                state.aliases.insert(alias.into(), collection.into());
            }
            None => {
                state.aliases.remove(alias);
            }
        }
        Ok(())
    }
}

#[test]
fn prepare_is_idempotent_and_mismatch_is_fail_closed() {
    let transport = FakeTransport::default();
    let config = config();
    let collection = Collection::new(config.clone(), transport.clone());

    assert!(collection.prepare("equill_stage_1").unwrap().created);
    assert!(!collection.prepare("equill_stage_1").unwrap().created);
    let mut incompatible = schema(&config);
    incompatible.dimensions = 99;
    transport
        .inner
        .lock()
        .unwrap()
        .schemas
        .insert("equill_stage_2".into(), incompatible);

    assert!(collection.prepare("equill_stage_2").is_err());
    let mut foreign_store = schema(&config);
    foreign_store.store_id = Uuid::now_v7();
    transport
        .inner
        .lock()
        .unwrap()
        .schemas
        .insert("equill_stage_3".into(), foreign_store);
    assert!(collection.prepare("equill_stage_3").is_err());
    assert!(collection.upsert("equill_stage_3", &[point()]).is_err());
}

#[test]
fn point_payload_contains_only_coordinates_and_hashes() {
    let provider = ProviderPoint {
        point: point(),
        store_id: Uuid::now_v7(),
        model_sha256: "e".repeat(64),
    };

    let point = qdrant_point(&provider).expect("point");
    let value = serde_json::Value::from(Payload::from(point.payload));
    let object = value.as_object().expect("payload object");

    assert_eq!(object.len(), 8);
    for forbidden in ["payload", "text", "actor", "evidence", "store_path"] {
        assert!(!object.contains_key(forbidden));
    }
}

#[test]
fn point_identity_is_scoped_by_store() {
    let record_id = Uuid::now_v7();
    let first_store = Uuid::now_v7();
    let second_store = Uuid::now_v7();

    assert_ne!(
        physical_id(first_store, record_id),
        physical_id(second_store, record_id)
    );
}

#[test]
fn upsert_and_query_validate_dimensions_and_metadata() {
    let transport = FakeTransport::default();
    let config = config();
    let store_id = config.store_id;
    let model_sha256 = config.embedding.model.sha256.clone();
    let collection = Collection::new(config, transport.clone());
    let mut invalid = point();
    invalid.vector.pop();

    assert!(collection.upsert("equill_stage", &[invalid]).is_err());
    collection.prepare("equill_stage").expect("prepare");
    collection
        .upsert("equill_stage", &[point()])
        .expect("upsert");
    assert_eq!(transport.inner.lock().unwrap().points.len(), 1);

    transport.inner.lock().unwrap().hits = vec![ProviderHit {
        store_id,
        model_sha256,
        record_id: Uuid::now_v7(),
        score: 0.9,
        record_sha256: "a".repeat(64),
        input_sha256: "b".repeat(64),
    }];
    collection.activate("equill_stage").expect("activate");
    assert_eq!(collection.search(&search()).unwrap().len(), 1);

    transport.inner.lock().unwrap().hits[0].store_id = Uuid::now_v7();
    assert!(collection.search(&search()).is_err());
}

#[test]
fn activation_retargets_atomically_and_failure_keeps_old_target() {
    let transport = FakeTransport::default();
    let collection = Collection::new(config(), transport.clone());

    collection.prepare("equill_stage_1").expect("prepare");
    collection.prepare("equill_stage_2").expect("prepare other");
    collection.activate("equill_stage_1").expect("activate");
    collection.activate("equill_stage_1").expect("idempotent");
    transport.inner.lock().unwrap().fail_retarget = true;
    assert!(collection.activate("equill_stage_2").is_err());
    assert_eq!(
        transport.inner.lock().unwrap().aliases["equill_records_test"],
        "equill_stage_1"
    );
    transport.inner.lock().unwrap().fail_retarget = false;
    collection.activate("equill_stage_2").expect("retarget");
    assert_eq!(
        transport.inner.lock().unwrap().aliases["equill_records_test"],
        "equill_stage_2"
    );
}

#[test]
fn activation_writes_ready_only_after_alias_success() {
    let root = std::env::temp_dir().join(format!(
        "equill-qdrant-activate-{}-{}",
        std::process::id(),
        Uuid::now_v7()
    ));
    let transport = FakeTransport::default();
    let config = config();
    let collection = Collection::new(config.clone(), transport.clone());
    collection.prepare("equill_stage_1").expect("prepare");
    transport.inner.lock().unwrap().fail_retarget = true;

    assert!(
        crate::vector::activate_collection(&root, &config, &collection, "equill_stage_1").is_err()
    );
    assert!(!root.join("projections/qdrant/state.json").exists());
    transport.inner.lock().unwrap().fail_retarget = false;
    crate::vector::activate_collection(&root, &config, &collection, "equill_stage_1")
        .expect("activate with marker");
    assert_eq!(
        crate::vector::state::read(&root, Some(&config)).expect("state"),
        crate::vector::VectorState::Ready
    );
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn transport_errors_never_expose_the_source() {
    let source = std::io::Error::other("secret endpoint and payload");
    let error = sanitized("query points", QdrantError::Io(source)).to_string();

    assert!(error.contains("query points failed"));
    assert!(!error.contains("secret endpoint"));
}
