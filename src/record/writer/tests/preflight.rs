use crate::record::{
    AtomicDraft, append_atomic, read_all,
    tests::{lesson, store},
};
use serde_json::json;
use std::fs;

#[test]
fn atomic_preflight_enforces_grants_and_word_limits_without_a_tombstone_bypass() {
    for actor in ["guest", "writer"] {
        let root = store();
        fs::write(
            root.join("settings.json"),
            json!({"records":{
                "agent.lesson.v1":{"rule":{"max_words":3}}
            }})
            .to_string(),
        )
        .unwrap();
        let requested = [crate::record::AtomicScope {
            line: 1,
            namespace: "agent.memory".into(),
            type_name: "agent.lesson.v1".into(),
            payload: json!({"rule":"short synthetic rule"}),
        }];
        let error = append_atomic(&root, actor, &requested, |_| {
            let mut too_long = lesson("synthetic content over its word limit");
            too_long.tags.push(crate::record::REVOKED_TAG.into());
            Ok((
                vec![
                    AtomicDraft {
                        id: uuid::Uuid::now_v7(),
                        line: 1,
                        draft: lesson("short synthetic rule"),
                    },
                    AtomicDraft {
                        id: uuid::Uuid::now_v7(),
                        line: 2,
                        draft: too_long,
                    },
                ],
                (),
            ))
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains(if actor == "guest" {
            "line 1:"
        } else {
            "line 2:"
        }));
        assert!(!error.contains("synthetic content"));
        assert!(read_all(&root).unwrap().is_empty());
        assert!(!root.join("transactions").exists());
        assert!(!root.join("receipts/pending").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
