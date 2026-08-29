use crate::kernel::error::Error;
use crate::kernel::store;
use crate::projection::{self, ProjectionState};
use crate::vector::{self, VectorState};
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
        },
        Component {
            id: "schema.json-schema-2020-12",
            kind: "validation",
            state: "built-in",
            installable: false,
        },
        Component {
            id: "projection.sqlite-fts",
            kind: "projection",
            state: sqlite,
            installable: false,
        },
        Component {
            id: "vector.qdrant",
            kind: "projection",
            state: vector,
            installable: false,
        },
        Component {
            id: "transport.mcp.stdio.2025",
            kind: "transport",
            state: "built-in",
            installable: false,
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
