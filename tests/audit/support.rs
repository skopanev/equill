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
