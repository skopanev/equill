//! What a status report says about a store, and what it keeps to itself.
use super::report;
use crate::command::init;
use std::fs;

#[test]
fn reports_initialized_store_without_exposing_owner() {
    let path = std::env::temp_dir().join(format!("equill-status-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    init::create(&path, "private-owner", "agent.memory").expect("initialize");
    let value =
        serde_json::to_value(report(Some(&path)).expect("status")).expect("serialize status");

    assert_eq!(value["store"]["initialized"], true);
    assert_eq!(value["store"]["namespaces"][0], "agent.memory");
    assert_eq!(value["components"][2]["state"], "ready");
    assert!(!value.to_string().contains("private-owner"));
    fs::remove_dir_all(path).expect("remove test store");
}

#[cfg(test)]
mod freshness_tests {
    use super::report;
    use crate::command::output;

    /// `ready` on the left is health. A reader only needs the number when the
    /// index has not caught up, so a current one says nothing extra.
    #[test]
    fn the_human_line_mentions_a_tail_only_when_there_is_one() {
        let current = component_line(None);
        let lagging = component_line(Some(1));
        let many = component_line(Some(11));

        assert_eq!(current, "  ready      vector.qdrant");
        assert_eq!(
            lagging,
            "  ready      vector.qdrant — 1 outside the checkpoint"
        );
        assert_eq!(
            many,
            "  ready      vector.qdrant — 11 outside the checkpoint"
        );
    }

    fn component_line(pending: Option<usize>) -> String {
        let mut report = report(None).expect("status");
        report.components.retain(|item| item.id == "vector.qdrant");
        let component = report.components.first_mut().expect("vector component");
        component.state = "ready";
        component.vector = Some(super::super::VectorHealth {
            vector_state: "ready",
            vector_freshness: match pending {
                Some(_) => crate::vector::VectorFreshness::Lagging,
                None => crate::vector::VectorFreshness::Current,
            },
            vector_indexed_records: Some(1374),
            vector_pending_records: pending.or(Some(0)),
        });
        output::status(&report)
            .lines()
            .find(|line| line.contains("vector.qdrant"))
            .expect("component line")
            .to_owned()
    }
}
