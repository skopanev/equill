use crate::record::StoredRecord;
use serde::Serialize;
use std::fmt::{Display, Formatter};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectionState {
    Ready,
    Degraded,
    Missing,
}

impl Display for ProjectionState {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready => formatter.write_str("ready"),
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
