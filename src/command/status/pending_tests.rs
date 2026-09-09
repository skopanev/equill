//! When the backlog is a number and when saying a number would be a lie.
use super::counts_tests::plain;
use super::pending::{Pending, assess};
use crate::kernel::digest::sha256_hex;
use crate::record::StoredRecord;

/// The corpus digest, built the way the real one is: each record's line hash,
/// concatenated in id order, hashed.
fn corpus(count: usize) -> (Vec<(StoredRecord, String)>, String) {
    let mut records: Vec<(StoredRecord, String)> = (0..count)
        .map(|index| {
            let record = plain(uuid::Uuid::now_v7(), None, false);
            let digest = sha256_hex(format!("record-{index}").as_bytes());
            (record, digest)
        })
        .collect();
    records.sort_by_key(|(record, _)| record.id);
    let digest = digest_of(&records);
    (records, digest)
}

fn digest_of(records: &[(StoredRecord, String)]) -> String {
    let mut accumulator = String::new();
    for (_, digest) in records {
        accumulator.push_str(digest);
    }
    sha256_hex(accumulator.as_bytes())
}

#[test]
fn a_matching_digest_owes_nothing() {
    let (records, digest) = corpus(3);

    assert!(matches!(
        assess(Some((3, &digest)), &records, &digest),
        Pending::None
    ));
}

/// Records appended after the checkpoint, with everything it covered still in
/// place: the count is real.
#[test]
fn records_appended_after_an_unchanged_checkpoint_are_counted() {
    let (records, digest) = corpus(5);
    let covered = digest_of(&records[..3]);

    match assess(Some((3, &covered)), &records, &digest) {
        Pending::Records { count } => assert_eq!(count, 2),
        other => panic!("expected two records outside the checkpoint, got {other:?}"),
    }
}

/// The case a length subtraction gets wrong in silence: replacing a record
/// leaves the count unchanged while the work is real. Reporting zero here would
/// say the index is up to date when it is not.
#[test]
fn replacing_a_record_is_unknown_rather_than_zero() {
    let (records, digest) = corpus(4);
    let stale = sha256_hex(b"a digest from before the replacement");

    match assess(Some((4, &stale)), &records, &digest) {
        Pending::Unknown { .. } => {}
        other => panic!("a replacement was reported as {other:?}"),
    }
}

/// And the case it gets wrong in the other direction: superseding one record
/// while appending another grows the corpus by one, but two records are new.
#[test]
fn a_replacement_plus_an_append_is_unknown_rather_than_one() {
    let (records, digest) = corpus(5);
    // The checkpoint covered four records, but not the four that are there now.
    let stale = sha256_hex(b"four records, one of which has since been replaced");

    match assess(Some((4, &stale)), &records, &digest) {
        Pending::Unknown { .. } => {}
        other => panic!("expected unknown, got {other:?}"),
    }
}

/// A corpus that shrank below the checkpoint says so instead of subtracting
/// into nonsense.
#[test]
fn a_shrinking_corpus_is_unknown() {
    let (records, digest) = corpus(2);
    let stale = sha256_hex(b"a larger corpus");

    match assess(Some((5, &stale)), &records, &digest) {
        Pending::Unknown { .. } => {}
        other => panic!("expected unknown, got {other:?}"),
    }
}

/// No checkpoint at all is not a checkpoint at zero.
#[test]
fn a_missing_checkpoint_is_unknown_not_everything() {
    let (records, digest) = corpus(3);

    match assess(None, &records, &digest) {
        Pending::Unknown { .. } => {}
        other => panic!("a missing checkpoint was read as {other:?}"),
    }
}
