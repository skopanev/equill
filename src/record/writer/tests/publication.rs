use super::{
    super::seam::{self, Step},
    request,
};
use crate::{
    record::{append_request, read_all, tests::store},
    vector,
};
use serde_json::json;
use std::{fs, path::Path};

fn configured(root: &Path) {
    let artifact = |name: &str| {
        fs::write(root.join(name), b"synthetic artifact").unwrap();
        json!({"path":name,"sha256":crate::kernel::digest::sha256_hex(b"synthetic artifact")})
    };
    let config = json!({"schema":"equill.qdrant-config.v1","enabled":true,
        "endpoint":"http://127.0.0.1:1","collection_alias":"synthetic_retry",
        "store_id":uuid::Uuid::now_v7(),"dimensions":1024,"distance":"cosine",
        "embedding":{"model_id":"Qwen/Qwen3-Embedding-0.6B","input_schema":"equill.record.embedding.v1",
            "model":artifact("model.safetensors"),"tokenizer":artifact("tokenizer.json"),"config":artifact("model.json")}});
    fs::create_dir_all(root.join("registry/vector")).unwrap();
    fs::write(root.join("registry/vector/qdrant.json"), config.to_string()).unwrap();
    vector::desired::publish(root, 41).unwrap();
    vector::VectorProjection::open(root)
        .unwrap()
        .unwrap()
        .mark_indexed("synthetic_generation", 0, &"0".repeat(64), 41, None)
        .unwrap();
    assert!(!vector::drain::outstanding_for_tests(root));
}

fn import(root: &Path) -> Result<(), crate::kernel::error::Error> {
    let input = root.join("synthetic.jsonl");
    if !input.exists() {
        let lines = (0..3).map(|i| format!("{}\n", json!({"id":format!("legacy-{i}"),
            "ts":"2026-01-01T00:00:00Z","actor":"legacy-writer","namespace":"agent.memory",
            "type":"agent.lesson.v1","observed_at":"2026-01-01T00:00:00Z","payload":{"rule":"synthetic retry"}}))).collect::<String>();
        fs::write(&input, lines).unwrap();
    }
    crate::ingest::import_jsonl(root, &input, "writer").map(|_| ())
}

#[test]
fn replay_repairs_only_the_reserved_revision_after_crash_or_publication_failure() {
    for (atomic, step) in [
        (false, Step::AfterReceipts),
        (false, Step::BeforeOutcome),
        (true, Step::AfterReceipts),
        (true, Step::BeforeProjection),
    ] {
        let root = store();
        configured(&root);
        let _held = crate::kernel::lock::TryLock::acquire(&root, "vector-drain.lock")
            .unwrap()
            .unwrap();
        let run = || {
            if atomic {
                import(&root)
            } else {
                append_request(
                    &root,
                    request(Some("literal retry"), "synthetic retry"),
                    "writer",
                )
                .map(|_| ())
            }
        };
        seam::fail(Some(step));
        assert!(run().is_err());
        seam::fail(None);
        if step == Step::AfterReceipts {
            let desired = root.join("projections/qdrant/desired.json");
            let saved = root.join("synthetic-desired-backup.json");
            fs::rename(&desired, &saved).unwrap();
            fs::create_dir(&desired).unwrap();
            assert!(run().is_err());
            fs::remove_dir(&desired).unwrap();
            fs::rename(saved, desired).unwrap();
        }
        run().unwrap();
        let count = if atomic { 3 } else { 1 };
        assert_eq!(read_all(&root).unwrap().len(), count);
        assert_eq!(
            vector::desired::read(&root).unwrap().unwrap().revision,
            41 + count as u64
        );
        assert!(vector::drain::outstanding_for_tests(&root));
        run().unwrap();
        assert_eq!(read_all(&root).unwrap().len(), count);
        assert_eq!(
            vector::desired::read(&root).unwrap().unwrap().revision,
            41 + count as u64
        );
        fs::remove_dir_all(root).unwrap();
    }
}
