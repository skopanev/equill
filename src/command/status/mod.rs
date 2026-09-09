pub mod counts;
#[cfg(test)]
mod counts_tests;
#[cfg(test)]
mod marker_tests;
pub mod pending;
#[cfg(test)]
mod pending_tests;
#[cfg(test)]
mod report_tests;
mod vector_counts;

use crate::kernel::error::Error;
use crate::kernel::store;
use crate::projection::{self, ProjectionState};
use crate::vector::{self, Position, VectorFreshness, VectorState};
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
    /// What the ledger holds. Absent for a store that has none yet.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub counts: Option<counts::LedgerCounts>,
    /// What the vector projection has and still owes. Absent when there is no
    /// projection configured — which is not the same as one that owes nothing.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub vector: Option<VectorCounts>,
}

/// The vector side of the same question.
#[derive(Debug, Serialize)]
pub struct VectorCounts {
    /// Live records this store would embed. A property of the ledger, so it is
    /// counted even when no provider is configured.
    pub vector_eligible_records: usize,
    /// How many records the last successful pass covered.
    ///
    /// A checkpoint, not a count of points in the collection: status does not
    /// ask the provider anything, and the two can differ.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vector_checkpoint_records: Option<usize>,
    pub vector_pending: pending::Pending,
    /// Whether a pass is running. Nothing durable records that, so this says
    /// so rather than inferring a number from the backlog.
    pub vector_processing: &'static str,
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

/// The vector position is read once and handed to both halves. Reading it twice
/// is how the store block and the component block came to disagree about the
/// same marker inside one document.
pub fn report(store_root: Option<&Path>) -> Result<StatusReport, Error> {
    let position = match store_root.filter(|root| root.join("store.json").is_file()) {
        Some(root) => Some(vector::position(root)?),
        None => None,
    };
    let store_status = store_root
        .map(|root| inspect_store(root, position.as_ref()))
        .transpose()?;
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
        components: components(store_root, initialized, position.as_ref())?,
    })
}

fn inspect_store(root: &Path, position: Option<&Position>) -> Result<StoreStatus, Error> {
    if !root.join("store.json").is_file() {
        return Ok(StoreStatus {
            initialized: false,
            namespaces: Vec::new(),
            schemas: Vec::new(),
            counts: None,
            vector: None,
        });
    }
    let config = store::load(root)?;
    let mut schemas = file_stems(&root.join("registry/types"))?;
    schemas.sort();
    let records = crate::record::read_all(root)?;
    Ok(StoreStatus {
        initialized: true,
        namespaces: config.namespaces,
        schemas,
        counts: Some(counts::of(&records)),
        vector: position.map(vector_counts::vector_counts),
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

fn components(
    store_root: Option<&Path>,
    store_initialized: bool,
    position: Option<&Position>,
) -> Result<Vec<Component>, Error> {
    let sqlite = match (store_root, store_initialized) {
        (None, _) => "built-in",
        (Some(_), false) => "missing",
        (Some(root), true) => match projection::state(root)? {
            ProjectionState::Ready => "ready",
            ProjectionState::Queued => "queued",
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
    // Freshness is only meaningful for a store we can actually look at, and it
    // is the same reading the store block was built from.
    let vector_health = position.filter(|_| store_initialized).map(|position| {
        let pending = pending::assess(position);
        VectorHealth {
            vector_state: vector,
            vector_freshness: position.freshness(),
            vector_indexed_records: position.checkpoint.indexed(),
            vector_pending_records: pending.records(),
        }
    });
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
