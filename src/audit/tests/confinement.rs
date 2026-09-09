use super::{event, root};
use crate::audit::{Scope, list_at, writer};
use std::fs;
use std::os::unix::fs::symlink;

#[test]
fn audit_child_symlinks_and_hardlinks_never_change_outside_targets() {
    for name in [
        "writer.lock",
        "pending.tmp",
        "pending.json",
        "2026-01.jsonl",
        "index.lock",
        "index.sqlite3",
        "index.sqlite3-journal",
    ] {
        let base = root();
        let audit = base.join("audit");
        fs::create_dir(&audit).expect("audit");
        let outside = base.join("project-record");
        fs::write(&outside, b"synthetic immutable bytes\n").expect("target");
        symlink(&outside, audit.join(name)).expect("symlink");
        let result = if name.starts_with("index") {
            list_at(&audit, &Scope::default(), 10).map(|_| ())
        } else {
            writer::append(&audit, &event("2026-01-01T00:00:00Z"))
        };
        assert!(result.is_err(), "{name}");
        assert_eq!(
            fs::read(&outside).expect("target unchanged"),
            b"synthetic immutable bytes\n",
            "{name}"
        );
        fs::remove_dir_all(base).expect("cleanup");
    }
    let base = root();
    let audit = base.join("audit");
    fs::create_dir(&audit).expect("audit");
    let outside = base.join("project-record");
    fs::write(&outside, b"unchanged").expect("target");
    fs::hard_link(&outside, audit.join("pending.tmp")).expect("hard link");
    assert!(writer::append(&audit, &event("2026-01-01T00:00:00Z")).is_err());
    assert_eq!(fs::read(&outside).expect("target"), b"unchanged");
    fs::remove_dir_all(base).expect("cleanup");
}
