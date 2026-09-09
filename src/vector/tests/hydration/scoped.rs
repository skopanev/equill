use super::{candidate, from_ledger, ledger_store, request};
use crate::kernel::digest::sha256_hex;
use crate::projection::{LedgerLocator, LocatorRequest};
use std::fs;

#[test]
fn hydration_reads_only_the_shards_its_candidates_name() {
    let (root, record) = ledger_store("hydrate-scoped");
    fs::write(
        root.join("records/2019-01.jsonl"),
        b"{ unrelated corrupt shard }\n",
    )
    .unwrap();
    crate::record::hotpath::reset();
    crate::record::located::shard_reads();
    let hits =
        from_ledger(&root, &request(), vec![candidate(&record)]).expect("candidate shard only");
    assert_eq!(hits[0].record.id, record.id);
    assert_eq!(crate::record::hotpath::touched().ledger_reads, 0);
    assert_eq!(crate::record::located::shard_reads(), 1);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn shared_and_distinct_shards_preserve_candidate_order_and_canonical_payloads() {
    let (root, first) = ledger_store("hydrate-shards");
    let mut second = first.clone();
    second.id = uuid::Uuid::now_v7();
    second.payload = serde_json::json!({"rule":"second canonical value"});
    let first_month = &first.recorded_at[..7];
    let mut third = first.clone();
    third.id = uuid::Uuid::now_v7();
    third.recorded_at = "2020-02-01T00:00:00Z".into();
    third.payload = serde_json::json!({"rule":"third canonical value"});
    fs::write(
        root.join(format!("records/{first_month}.jsonl")),
        format!(
            "{}\n{}\n",
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&second).unwrap()
        ),
    )
    .unwrap();
    fs::write(
        root.join("records/2020-02.jsonl"),
        format!("{}\n", serde_json::to_string(&third).unwrap()),
    )
    .unwrap();
    for record in [&second, &third] {
        crate::projection::index(
            &root,
            record,
            &sha256_hex(&serde_json::to_vec(record).unwrap()),
            &format!("records/{}.jsonl", &record.recorded_at[..7]),
        )
        .unwrap();
    }
    crate::record::located::shard_reads();
    let hits = from_ledger(
        &root,
        &request(),
        [&third, &second, &first].map(candidate).into(),
    )
    .unwrap();
    assert_eq!(
        hits.iter().map(|hit| hit.record.id).collect::<Vec<_>>(),
        vec![third.id, second.id, first.id]
    );
    assert_eq!(hits[1].record.payload, second.payload);
    assert_eq!(crate::record::located::shard_reads(), 2);
    assert!(
        from_ledger(
            &root,
            &request(),
            vec![candidate(&first), candidate(&first)]
        )
        .is_err()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_missing_projected_coordinate_is_not_replaced_with_a_full_scan() {
    let (root, record) = ledger_store("hydrate-missing-locator");
    let mut absent = candidate(&record);
    absent.record_id = uuid::Uuid::now_v7();
    crate::record::hotpath::reset();
    assert!(from_ledger(&root, &request(), vec![absent]).is_err());
    assert_eq!(crate::record::hotpath::touched().ledger_reads, 0);
    fs::remove_dir_all(root).unwrap();
}

fn locator(root: &std::path::Path, id: uuid::Uuid) -> LedgerLocator {
    crate::projection::locators(root, &LocatorRequest { ids: vec![id] })
        .unwrap()
        .located
        .remove(0)
}

#[test]
fn candidate_and_projected_digests_each_have_to_match_immutable_truth() {
    let (root, record) = ledger_store("hydrate-digests");
    let mut indexed = locator(&root, record.id);
    assert!(crate::record::read_located(&root, &[indexed.clone(), indexed.clone()]).is_err());
    let mut absent = indexed.clone();
    absent.record_id = uuid::Uuid::now_v7();
    assert!(crate::record::read_located(&root, &[absent]).is_err());
    indexed.record_sha256 = "f".repeat(64);
    assert!(crate::record::read_located(&root, &[indexed]).is_err());
    let mut vector = candidate(&record);
    vector.record_sha256 = "e".repeat(64);
    assert!(from_ledger(&root, &request(), vec![vector]).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn unsafe_locators_and_linked_shards_fail_without_private_path_disclosure() {
    let (root, record) = ledger_store("hydrate-paths");
    let original = locator(&root, record.id);
    for path in [
        "/private/synthetic-secret.jsonl",
        "records/../synthetic-secret.jsonl",
        "records/nested/synthetic-secret.jsonl",
        "records/2026-99.jsonl",
    ] {
        let mut bad = original.clone();
        bad.ledger = path.into();
        let error = crate::record::read_located(&root, &[bad])
            .unwrap_err()
            .to_string();
        assert!(!error.contains(path));
    }
    let shard = root.join(&original.ledger);
    let outside = root.join("outside.jsonl");
    fs::rename(&shard, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &shard).unwrap();
    assert!(crate::record::read_located(&root, std::slice::from_ref(&original)).is_err());
    fs::remove_file(&shard).unwrap();
    fs::hard_link(&outside, &shard).unwrap();
    assert!(crate::record::read_located(&root, std::slice::from_ref(&original)).is_err());
    fs::remove_file(&shard).unwrap();
    fs::remove_dir(root.join("records")).unwrap();
    let relocated = root.join("relocated");
    fs::create_dir(&relocated).unwrap();
    fs::rename(&outside, relocated.join(shard.file_name().unwrap())).unwrap();
    std::os::unix::fs::symlink(&relocated, root.join("records")).unwrap();
    assert!(crate::record::read_located(&root, &[original]).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn malformed_invalid_duplicate_and_unfinished_named_rows_fail_closed() {
    let (root, record) = ledger_store("hydrate-corruption");
    let located = locator(&root, record.id);
    let valid = serde_json::to_string(&record).unwrap();
    let mut invalid = record.clone();
    invalid.actor = "private-synthetic\nactor".into();
    let mut unrelated = record.clone();
    unrelated.id = uuid::Uuid::now_v7();
    let unrelated = serde_json::to_string(&unrelated).unwrap();
    for content in [
        format!("{valid}\n{valid}\n"),
        format!("{valid}\n{unrelated}\n{unrelated}\n"),
        format!("{valid}\n{{ private-synthetic }}\n"),
        format!("{valid}\n{{}}\n"),
        format!("{valid}\n\n"),
        valid.clone(),
        format!("{}\n", serde_json::to_string(&invalid).unwrap()),
    ] {
        fs::write(root.join(&located.ledger), content).unwrap();
        let error = crate::record::read_located(&root, std::slice::from_ref(&located))
            .unwrap_err()
            .to_string();
        assert!(!error.contains("private-synthetic"));
    }
    fs::remove_file(root.join(&located.ledger)).unwrap();
    assert!(crate::record::read_located(&root, &[located]).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn concurrent_snapshot_ignores_partial_tail_but_exclusive_lookup_refuses_it() {
    let (root, record) = ledger_store("hydrate-tail");
    let located = locator(&root, record.id);
    fs::write(
        root.join(&located.ledger),
        format!("{}\n{{unfinished", serde_json::to_string(&record).unwrap()),
    )
    .unwrap();
    assert_eq!(
        crate::record::read_located(&root, std::slice::from_ref(&located)).unwrap()[0].id,
        record.id
    );
    assert!(crate::record::read_located_exclusive(&root, &[located]).is_err());
    fs::remove_dir_all(root).unwrap();
}
