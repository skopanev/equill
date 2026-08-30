use crate::kernel::error::Error;
use crate::kernel::store;
use crate::projection::{self, ProjectionState};
use crate::vector::{self, VectorFreshness, VectorState};
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub ok: bool,
    pub version: &'static str,
    pub store: Option<StoreStatus>,
    pub components: Vec<Component>,
}

#[derive(Debug, Serialize)]
pub struct StoreStatus {
    pub initialized: bool,
    pub namespaces: Vec<String>,
    pub schemas: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Component {
    pub id: &'static str,
    pub kind: &'static str,
    pub state: &'static str,
    pub installable: bool,
    /// How far behind a component is, when that is a separate question from
    /// whether it works. `ready` on the left is health; this is freshness.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub vector: Option<VectorHealth>,
}

#[derive(Debug, Serialize)]
pub struct VectorHealth {
    pub vector_state: &'static str,
    pub vector_freshness: VectorFreshness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_indexed_records: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_pending_records: Option<usize>,
}

pub fn report(store_root: Option<&Path>) -> Result<StatusReport, Error> {
    let store_status = store_root.map(inspect_store).transpose()?;
    let initialized = store_status
        .as_ref()
        .is_some_and(|status| status.initialized);
    let ok = match &store_status {
        Some(status) => status.initialized,
        None => true,
    };
    Ok(StatusReport {
        ok,
        version: env!("CARGO_PKG_VERSION"),
        store: store_status,
        components: components(store_root, initialized)?,
    })
}

fn inspect_store(root: &Path) -> Result<StoreStatus, Error> {
    if !root.join("store.json").is_file() {
        return Ok(StoreStatus {
            initialized: false,
            namespaces: Vec::new(),
            schemas: Vec::new(),
        });
    }
    let config = store::load(root)?;
    let mut schemas = file_stems(&root.join("registry/types"))?;
    schemas.sort();
    Ok(StoreStatus {
        initialized: true,
        namespaces: config.namespaces,
        schemas,
    })
}

fn file_stems(directory: &Path) -> Result<Vec<String>, Error> {
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && let Some(name) = path.file_stem().and_then(|name| name.to_str())
        {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

fn components(store_root: Option<&Path>, store_initialized: bool) -> Result<Vec<Component>, Error> {
    let sqlite = match (store_root, store_initialized) {
        (None, _) => "built-in",
        (Some(_), false) => "missing",
        (Some(root), true) => match projection::state(root)? {
            ProjectionState::Ready => "ready",
            ProjectionState::Degraded => "degraded",
            ProjectionState::Missing => "missing",
        },
    };
    let vector = match (store_root, store_initialized) {
        (None, _) => "optional",
        (Some(_), false) => "missing",
        (Some(root), true) => match vector::state(root)? {
            VectorState::Disabled => "disabled",
            VectorState::Ready => "ready",
            VectorState::Degraded => "degraded",
            VectorState::Missing => "missing",
        },
    };
    // Freshness is only meaningful for a store we can actually look at.
    let vector_health = match store_root.filter(|_| store_initialized) {
        None => None,
        Some(root) => {
            let reading = vector::freshness_of(root)?;
            Some(VectorHealth {
                vector_state: vector,
                vector_freshness: reading.freshness,
                vector_indexed_records: reading.indexed_records,
                vector_pending_records: reading.pending_records,
            })
        }
    };
    Ok(vec![
        Component {
            id: "ledger.jsonl",
            kind: "storage",
            state: if store_initialized {
                "ready"
            } else {
                "built-in"
            },
            installable: false,
            vector: None,
        },
        Component {
            id: "schema.json-schema-2020-12",
            kind: "validation",
            state: "built-in",
            installable: false,
            vector: None,
        },
        Component {
            id: "projection.sqlite-fts",
            kind: "projection",
            state: sqlite,
            installable: false,
            vector: None,
        },
        Component {
            id: "vector.qdrant",
            kind: "projection",
            state: vector,
            installable: false,
            vector: vector_health,
        },
        Component {
            id: "transport.mcp.stdio.2025",
            kind: "transport",
            state: "built-in",
            installable: false,
            vector: None,
        },
    ])
}

#[cfg(test)]
mod tests {
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
        assert_eq!(lagging, "  ready      vector.qdrant — 1 processing");
        assert_eq!(many, "  ready      vector.qdrant — 11 processing");
    }

    fn component_line(pending: Option<usize>) -> String {
        let mut report = report(None).expect("status");
        report.components.retain(|item| item.id == "vector.qdrant");
        let component = report.components.first_mut().expect("vector component");
        component.state = "ready";
        component.vector = Some(super::VectorHealth {
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
