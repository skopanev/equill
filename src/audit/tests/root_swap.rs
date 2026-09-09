use super::{event, root};
use crate::audit::{Scope, list_at, writer};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root).expect("snapshot") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            result.insert(path.clone(), None);
            result.extend(snapshot(&path));
        } else {
            result.insert(path.clone(), Some(fs::read(path).expect("bytes")));
        }
    }
    result
}

#[test]
fn replaced_root_or_ancestor_cannot_redirect_checkpoint_finish_or_recovery() {
    for ancestor in [false, true] {
        for complete in [false, true] {
            let fixture = root();
            let container = fixture.join("container");
            let audit = container.join("audit");
            let project = fixture.join("project");
            let replacement = if ancestor {
                project.join("audit")
            } else {
                project.clone()
            };
            fs::create_dir_all(&replacement).expect("synthetic project");
            fs::write(project.join("store.json"), b"{}").expect("store marker");
            let mut record = event("2026-01-01T00:00:00Z");
            record.output.ids = vec![uuid::Uuid::now_v7()];
            record.output.durable = Some(true);
            let reservation = writer::Reservation::begin(&audit, &record).expect("intent");
            for name in [
                format!("outcome-stage-{}.json", record.id),
                format!("outcome-{}.json", record.id),
                "pending.tmp".into(),
                "pending.json".into(),
                "writer.lock".into(),
                "2026-01.jsonl".into(),
            ] {
                fs::write(replacement.join(name), b"synthetic untouched bytes").expect("sentinel");
            }
            let before = snapshot(&project);
            let saved = fixture.join("saved");
            let swapped = if ancestor { &container } else { &audit };
            fs::rename(swapped, &saved).expect("move original directory");
            std::os::unix::fs::symlink(&project, swapped).expect("replace with project link");
            reservation
                .checkpoint(&record)
                .expect("checkpoint pinned original");
            if complete {
                reservation.finish(&record).expect("finish pinned original");
            }
            drop(reservation);
            let original = if ancestor { saved.join("audit") } else { saved };
            let events = list_at(&original, &Scope::default(), 10)
                .expect("recover original")
                .events;
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].id, record.id);
            assert_eq!(events[0].output, record.output);
            assert_eq!(events[0].domain_outcome, "success");
            assert_eq!(
                events[0].error_class.as_deref(),
                if complete { None } else { Some("interrupted") }
            );
            assert_eq!(snapshot(&project), before);
            assert!(writer::Reservation::begin(&audit, &record).is_err());
            assert_eq!(snapshot(&project), before);
            fs::remove_file(swapped).expect("remove fixture link");
            fs::remove_dir_all(fixture).expect("cleanup");
        }
    }
}
