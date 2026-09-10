use equill::audit::Event;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub struct Fixture {
    pub root: PathBuf,
    pub store: PathBuf,
    pub audit: PathBuf,
    pub record: uuid::Uuid,
}

impl Fixture {
    pub fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("equill-audit-process-{}", uuid::Uuid::now_v7()));
        fs::create_dir_all(&root).expect("root");
        let store = root.join("memory");
        equill::command::init::create(&store, "owner", "agent.memory").expect("store");
        let schema = root.join("schema.json");
        write(
            &schema,
            &json!({
                "type":"agent.note.v1", "uri":"equill://agent.note/v1", "owner":"owner",
                "payload_schema":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"],"additionalProperties":false}
            }),
        );
        equill::schema::register_file(&store, &schema, "owner").expect("schema");
        write(&root.join("draft.json"), &draft());
        let report = equill::record::append_only(
            &store,
            serde_json::from_value(draft()).expect("draft"),
            "owner",
        )
        .expect("seed");
        let selector = root.join("selector.json");
        write(
            &selector,
            &json!({"id":"notes","version":"1","type":"agent.note.v1","strategies":["recency"],"required_tags":[],"core_tags":[]}),
        );
        equill::context::register_selector(&store, &selector, "owner").expect("selector");
        let profile = root.join("profile.json");
        write(
            &profile,
            &json!({"id":"reader","version":"1","actors":["*"],"grants":[{"namespace":"agent.memory","types":["agent.note.v1"]}],"selectors":["notes"],"budget":{}}),
        );
        equill::context::register_profile(&store, &profile, "owner").expect("profile");
        equill::projection::rebuild(&store).expect("index");
        Self {
            audit: root.join("audit"),
            root,
            store,
            record: report.id,
        }
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_equill"));
        command
            .env("EQUILL_AUDIT_DIR", &self.audit)
            .env("EQUILL_ACTOR", "owner")
            .env("EQUILL_PROJECT", "demo")
            .env("EQUILL_ROLE", "reviewer")
            .env("EQUILL_PROCESS", "process-a")
            .env("EQUILL_LANE", "lane-a")
            .env("EQUILL_INSTANCE", "instance-a")
            .env("EQUILL_SESSION", "session-a");
        command
    }

    /// The ledger, read once the store will let a snapshot be taken.
    ///
    /// A write is durable before the process that made it exits, but the work
    /// it handed off is not: the worker it started may still hold the writer
    /// lock, and a committed snapshot refuses to read across an active writer
    /// rather than show a torn one. That refusal is the product working, so the
    /// test waits for the condition it actually needs instead of weakening it.
    ///
    /// Not a sleep: this retries the exact read, and only while the exact
    /// reason is "writer active". Any other error fails immediately, and a
    /// writer that never lets go fails loudly rather than hanging.
    pub fn truth(&self) -> Vec<equill::record::StoredRecord> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            match equill::record::read_all(&self.store) {
                Ok(records) => return records,
                Err(error)
                    if std::time::Instant::now() < deadline
                        && error.to_string().contains("writer active") =>
                {
                    std::thread::yield_now();
                }
                Err(error) => panic!("truth: {error}"),
            }
        }
    }

    pub fn cli(&self, args: &[&str]) -> Output {
        self.command().args(args).output().expect("CLI")
    }

    pub fn scoped(&self, args: &[&str]) -> Output {
        self.command()
            .arg("--json")
            .args(args)
            .arg("--store")
            .arg(&self.store)
            .output()
            .expect("scoped CLI")
    }

    pub fn events(&self) -> Vec<Event> {
        events(&self.audit)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

pub fn events(root: &Path) -> Vec<Event> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for entry in entries {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|s| s == "jsonl") {
            let data = fs::read_to_string(path).expect("ledger");
            assert!(data.ends_with('\n'));
            result.extend(
                data.lines()
                    .map(|line| serde_json::from_str::<Event>(line).expect("event")),
            );
        }
    }
    result.sort_by_key(|event| event.id);
    result
}

pub fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).expect("json")).expect("write");
}

pub fn draft() -> Value {
    json!({"namespace":"agent.memory","type":"agent.note.v1","observed_at":"2026-01-01T00:00:00Z","payload":{"text":"synthetic audit fixture"}})
}

pub fn snapshot(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut result = std::collections::BTreeMap::new();
    for entry in fs::read_dir(root).expect("tree") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            result.extend(snapshot(&path));
        } else {
            result.insert(path.clone(), fs::read(path).expect("read"));
        }
    }
    result
}

/// The reason the audit tests once failed, reproduced on purpose.
///
/// A write is durable before the process that made it exits, but the worker it
/// handed off to may still hold the writer lock. A committed snapshot refuses
/// to read across an active writer rather than show a torn ledger — correct
/// behaviour that a test reading truth immediately can lose a race to. The
/// race is rare and load-dependent, so it is staged here instead of waited
/// for: a writer is held deliberately, and both halves of the contract are
/// checked.
#[test]
fn reading_truth_waits_for_an_active_writer_instead_of_failing() {
    use fs2::FileExt;
    let fixture = Fixture::new();
    let writer = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(fixture.store.join("locks/writer.lock"))
        .expect("writer lock");
    writer.lock_exclusive().expect("hold the writer lock");

    // The premise: while a writer is held, the snapshot refuses, and it refuses
    // for this exact reason. Without this the test below could pass by reading
    // a store nobody was writing to.
    let refused = equill::record::read_all(&fixture.store)
        .expect_err("a snapshot was taken across an active writer");
    assert!(
        refused.to_string().contains("writer active"),
        "refused for another reason: {refused}"
    );

    let released = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        FileExt::unlock(&writer).expect("release");
    });
    let records = fixture.truth();
    released.join().expect("releasing thread");

    assert!(
        records.iter().any(|record| record.id == fixture.record),
        "the wait returned a ledger without the seeded record"
    );
}
