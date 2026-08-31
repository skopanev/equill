use crate::record::StoredRecord;
use serde::Serialize;
use std::fmt::{Display, Formatter};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectionState {
    Ready,
    /// The record is durable and the index has not caught up with it yet. The
    /// ordinary answer for a fresh write, and not a failure.
    Queued,
    Degraded,
    Missing,
}

impl Display for ProjectionState {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready => formatter.write_str("ready"),
            Self::Queued => formatter.write_str("queued"),
            Self::Degraded => formatter.write_str("degraded"),
            Self::Missing => formatter.write_str("missing"),
        }
    }
}

#[derive(Debug)]
pub struct SearchRequest {
    /// Absent when a filter alone decides the result set.
    pub query: Option<String>,
    pub namespace: Option<String>,
    pub type_name: Option<String>,
    pub limit: u16,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub record: StoredRecord,
}

#[derive(Debug, Serialize)]
pub struct SearchReport {
    pub ok: bool,
    pub projection: &'static str,
    pub state: ProjectionState,
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Serialize)]
pub struct RebuildReport {
    pub ok: bool,
    pub projection: &'static str,
    pub records: usize,
}

/// The scope a lifecycle question is asked about. Both absent asks about the
/// whole store.
#[derive(Debug, Default)]
pub struct LifecycleScope {
    pub namespace: Option<String>,
    pub type_name: Option<String>,
}

/// Which of the records asked about the projection knows to be history.
///
/// Stated negatively on purpose. A projection that has not yet indexed a record
/// says nothing about it, and the caller keeps it — a read that dropped every
/// record the projection had not caught up on would silently lose live answers
/// whenever the projection was behind, which is the ordinary state of a store
/// being written to.
#[derive(Debug)]
pub struct HistoricRecords {
    pub state: ProjectionState,
    pub history: std::collections::HashSet<uuid::Uuid>,
}

/// How much of a scope is history rather than current.
#[derive(Debug)]
pub struct HistoryCount {
    pub state: ProjectionState,
    pub history: usize,
}
