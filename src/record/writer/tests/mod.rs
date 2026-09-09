mod atomic;
mod authorization;
mod compact;
mod crash;
mod idempotency;
mod preflight;
mod publication;
mod snapshot;

use crate::record::{AppendRequest, RecordDraft};

fn request(key: Option<&str>, rule: &str) -> AppendRequest {
    AppendRequest {
        draft: draft(rule),
        idempotency_key: key.map(str::to_owned),
    }
}

fn draft(rule: &str) -> RecordDraft {
    crate::record::tests::lesson(rule)
}

fn pending_id(root: &std::path::Path) -> uuid::Uuid {
    let journal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("transactions/batch.json")).unwrap())
            .unwrap();
    serde_json::from_value(journal["records"][0]["id"].clone()).unwrap()
}
