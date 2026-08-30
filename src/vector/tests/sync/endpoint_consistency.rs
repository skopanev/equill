use super::super::endpoint::{add, artifacts, config, endpoint, store};
use crate::vector::{VectorProjection, VectorState, configure, corpus, rebuild, state, sync};
use serde_json::Value;
use std::fs;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[test]
fn endpoint_gated_concurrency_and_outage_stay_degraded() {
    let (Some(endpoint), Some(artifacts)) = (endpoint(), artifacts()) else {
        return;
    };
    let root = store("consistency");
    add(&root, "Initial synthetic lesson.");
    let file = root.join("vector.json");
    fs::write(&file, config(&endpoint, &artifacts)).unwrap();
    configure(&root, &file, "owner").unwrap();
    rebuild(&root, "owner").unwrap();
    let projection = VectorProjection::open(&root).unwrap().unwrap();
    let physical = projection.active_collection().unwrap();

    for index in 0..10 {
        add(&root, &format!("Pending synthetic lesson {index}."));
    }
    // Ready is staged only as an observable test signal. Sync must demote it
    // after planning but before embedding; the append then lands after snapshot.
    let vector_config = crate::vector::config::load(&root).unwrap().unwrap();
    crate::vector::state::stage_ready(&root, &vector_config, &physical, None)
        .unwrap()
        .commit()
        .unwrap();
    let (started_tx, started_rx) = mpsc::channel();
    let worker_root = root.clone();
    let worker = thread::spawn(move || {
        started_tx.send(()).unwrap();
        sync(&worker_root, "owner")
    });
    started_rx.recv().unwrap();
    for _ in 0..200 {
        if state(&root).unwrap() == VectorState::Degraded {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state(&root).unwrap(), VectorState::Degraded);
    let concurrent = add(&root, "Concurrent synthetic lesson.");
    let error = worker.join().unwrap().expect_err("digest race must fail");

    assert!(error.to_string().contains("ledger changed"));
    assert_eq!(state(&root).unwrap(), VectorState::Degraded);
    let ids = corpus(&root)
        .unwrap()
        .0
        .into_iter()
        .map(|(record, _)| record.id)
        .collect::<Vec<_>>();
    let metadata = projection.metadata(&physical, &ids).unwrap();
    assert_eq!(metadata.len(), 11);
    assert!(!metadata.iter().any(|item| item.record_id == concurrent));
    let repair = sync(&root, "owner").unwrap();
    assert_eq!((repair.embeddings, repair.points_upserted), (1, 1));
    assert_eq!(state(&root).unwrap(), VectorState::Ready);

    add(&root, "Provider outage synthetic lesson.");
    let before = corpus(&root).unwrap();
    let path = root.join("registry/vector/qdrant.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["endpoint"] = Value::String("http://127.0.0.1:9".into());
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(sync(&root, "owner").is_err());
    let after = corpus(&root).unwrap();
    assert_eq!((after.0.len(), after.1), (before.0.len(), before.1));
    assert_eq!(state(&root).unwrap(), VectorState::Degraded);
    fs::remove_dir_all(root).unwrap();
}
