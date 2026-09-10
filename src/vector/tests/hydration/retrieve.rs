use super::{candidate, ledger_store, request};
use crate::kernel::error::Error;
use crate::vector::{QueryEmbedder, VectorProjection, canonical, retrieve};
use std::fs;

struct Query;
impl QueryEmbedder for Query {
    fn embed_query(&self, _: &str, _: &str) -> Result<Vec<f32>, Error> {
        Ok(vec![0.1, 0.2, 0.3])
    }
}

/// Public semantic retrieval -> actual VectorProjection::search -> immutable
/// hydration -> canonical embedding-input verification. Only model output and
/// provider candidates are synthetic; no already-hydrated records are injected.
#[test]
fn semantic_retrieval_performs_zero_full_ledger_reads_and_rechecks_embedding_input() {
    let (root, record) = ledger_store("hydrate-retrieve");
    let config = crate::vector::tests::support::config(&root);
    crate::vector::tests::support::write(&root, &config);
    let index = VectorProjection::open(&root).unwrap().unwrap();
    let mut hit = candidate(&record);
    hit.input_sha256 = canonical(
        &record,
        &hit.record_sha256,
        crate::vector::DEFAULT_MAX_CHARS,
    )
    .unwrap()
    .input_sha256;
    fs::write(
        root.join("records/2019-01.jsonl"),
        b"{ unrelated corruption }\n",
    )
    .unwrap();
    crate::record::hotpath::reset();
    crate::record::located::shard_reads();
    let found = crate::vector::hydrate::with_candidates(vec![hit.clone()], || {
        retrieve(
            &index,
            &Query,
            "synthetic query",
            request(),
            crate::vector::DEFAULT_MAX_CHARS,
        )
    })
    .expect("semantic retrieval");
    assert_eq!(found.records.len(), 1);
    assert_eq!(found.records[0].id, record.id);
    assert!(found.rejected.is_empty());
    assert_eq!(crate::record::hotpath::touched().ledger_reads, 0);
    assert_eq!(crate::record::located::shard_reads(), 1);

    hit.input_sha256 = "f".repeat(64);
    let stale = crate::vector::hydrate::with_candidates(vec![hit], || {
        retrieve(
            &index,
            &Query,
            "synthetic query",
            request(),
            crate::vector::DEFAULT_MAX_CHARS,
        )
    })
    .unwrap();
    assert!(stale.records.is_empty());
    assert_eq!(stale.rejected.len(), 1);
    assert_eq!(stale.rejected[0].record_id, record.id);
    assert_eq!(crate::record::hotpath::touched().ledger_reads, 0);
    fs::remove_dir_all(root).unwrap();
}

/// Local canonical hydration, not network/model or MCP startup latency.
/// The first call is reported separately from subsequent calls on one index.
#[cfg(not(debug_assertions))]
#[test]
fn release_hydration_reports_first_and_warm_local_overhead() {
    use crate::kernel::digest::sha256_hex;
    use std::time::{Duration, Instant};

    let (root, first) = ledger_store("hydrate-release");
    let ledger = format!("records/{}.jsonl", &first.recorded_at[..7]);
    let records: Vec<_> = (0..200)
        .map(|n| {
            let mut record = first.clone();
            record.id = uuid::Uuid::now_v7();
            record.payload = serde_json::json!({"rule": format!("synthetic lesson {n}")});
            record
        })
        .collect();
    let lines: String = records
        .iter()
        .map(|record| format!("{}\n", serde_json::to_string(record).unwrap()))
        .collect();
    fs::write(root.join(&ledger), lines).unwrap();
    for record in &records {
        crate::projection::index(
            &root,
            record,
            &sha256_hex(&serde_json::to_vec(record).unwrap()),
            &ledger,
        )
        .unwrap();
    }
    let config = crate::vector::tests::support::config(&root);
    crate::vector::tests::support::write(&root, &config);
    let index = VectorProjection::open(&root).unwrap().unwrap();
    let hits: Vec<_> = records
        .iter()
        .step_by(20)
        .map(|record| {
            let mut hit = candidate(record);
            hit.input_sha256 = canonical(record, &hit.record_sha256).unwrap().input_sha256;
            hit
        })
        .collect();
    let mut taken = Vec::new();
    for _ in 0..51 {
        crate::record::hotpath::reset();
        crate::record::located::shard_reads();
        let elapsed = crate::vector::hydrate::with_candidates(hits.clone(), || {
            let started = Instant::now();
            let result = retrieve(
                &index,
                &Query,
                "synthetic query",
                request(),
                crate::vector::DEFAULT_MAX_CHARS,
            )
            .unwrap();
            let elapsed = started.elapsed();
            assert_eq!(result.records.len(), 10);
            assert!(result.rejected.is_empty());
            elapsed
        });
        assert_eq!(crate::record::hotpath::touched().ledger_reads, 0);
        assert_eq!(crate::record::located::shard_reads(), 1);
        taken.push(elapsed);
    }
    let first = taken.remove(0);
    taken.sort();
    let maximum = *taken.last().unwrap();
    eprintln!(
        "local hydration: records=200 candidates=10 first={first:?}; warm p50={:?} p95={:?} max={maximum:?}; model/network excluded",
        taken[25], taken[47],
    );
    assert!(first.max(maximum) <= Duration::from_millis(250));
    fs::remove_dir_all(root).unwrap();
}
