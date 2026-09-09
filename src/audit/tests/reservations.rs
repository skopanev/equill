use super::{event, root};
use crate::audit::{Scope, list_at, writer};
use std::fs;

#[test]
fn partial_unpublished_intent_does_not_poison_the_next_invocation() {
    let root = root();
    let stage = root.join(format!("intent-stage-{}.json", uuid::Uuid::now_v7()));
    fs::write(&stage, b"{\"schema_version\":").expect("partial intent before publication");
    let record = event("2026-01-01T00:00:00Z");
    let reservation = writer::Reservation::begin(&root, &record).expect("next invocation starts");
    assert!(!stage.exists());
    reservation
        .finish(&record)
        .expect("next invocation finishes");
    assert_eq!(
        list_at(&root, &Scope::default(), 10).expect("list").events,
        vec![record]
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn mismatched_pending_reservation_cannot_remove_another_intent() {
    let root = root();
    let other = event("2026-01-01T00:00:00Z");
    let reservation = writer::Reservation::begin(&root, &other).expect("live intent");
    let record = event("2026-01-01T00:00:00Z");
    let line = format!("{}\n", serde_json::to_string(&record).expect("event"));
    let pending = serde_json::json!({"month":"2026-01","offset":0,"sha256":crate::kernel::digest::sha256_hex(line.as_bytes()),"line":line,"reservation":other.id});
    fs::write(
        root.join("pending.json"),
        serde_json::to_vec(&pending).expect("pending"),
    )
    .expect("corrupt binding");
    assert!(list_at(&root, &Scope::default(), 10).is_err());
    assert!(root.join(format!("request-{}.json", other.id)).is_file());
    assert!(!root.join("2026-01.jsonl").exists());
    drop(reservation);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn live_intents_are_skipped_and_abandoned_outcomes_recover_once() {
    for checkpoint in [false, true] {
        let root = root();
        let mut record = event("2026-01-01T00:00:00Z");
        record.domain_outcome = "unknown".into();
        record.outcome = "error".into();
        record.error_class = Some("interrupted".into());
        let reservation = writer::Reservation::begin(&root, &record).expect("durable intent");
        assert!(
            list_at(&root, &Scope::default(), 10)
                .expect("active is not interrupted")
                .events
                .is_empty()
        );
        if checkpoint {
            record.domain_outcome = "success".into();
            record.outcome = "success".into();
            record.error_class = None;
            record.output.ids = vec![uuid::Uuid::now_v7()];
            record.output.durable = Some(true);
            reservation.checkpoint(&record).expect("bounded outcome");
        }
        drop(reservation); // The operating system releases this same lease on process death.
        let events = list_at(&root, &Scope::default(), 10)
            .expect("recover")
            .events;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, record.id);
        assert_eq!(events[0].error_class.as_deref(), Some("interrupted"));
        assert_eq!(
            events[0].domain_outcome,
            if checkpoint { "success" } else { "unknown" }
        );
        assert_eq!(events[0].output, record.output);
        assert_eq!(
            list_at(&root, &Scope::default(), 10).expect("again").events,
            events
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}

#[test]
fn final_pending_recovery_removes_intent_before_clearing_journal() {
    let root = root();
    let record = event("2026-01-01T00:00:00Z");
    let reservation = writer::Reservation::begin(&root, &record).expect("intent");
    let line = format!("{}\n", serde_json::to_string(&record).expect("event"));
    let pending = serde_json::json!({"month":"2026-01","offset":0,"sha256":crate::kernel::digest::sha256_hex(line.as_bytes()),"line":line,"reservation":record.id});
    fs::write(
        root.join("pending.json"),
        serde_json::to_vec(&pending).expect("pending"),
    )
    .expect("stage");
    fs::write(root.join("2026-01.jsonl"), &line).expect("already appended");
    drop(reservation);
    let events = list_at(&root, &Scope::default(), 10)
        .expect("recover")
        .events;
    assert_eq!(events, vec![record]);
    assert_eq!(
        list_at(&root, &Scope::default(), 10).expect("again").events,
        events
    );
    fs::remove_dir_all(root).expect("cleanup");
}
