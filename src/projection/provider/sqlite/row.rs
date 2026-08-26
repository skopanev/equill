use crate::kernel::error::Error;
use crate::record::{EvidenceRef, StoredRecord};
use serde_json::Value;
use uuid::Uuid;

pub struct ProjectedRow {
    id: String,
    namespace: String,
    type_name: String,
    actor: String,
    recorded_at: String,
    observed_at: String,
    valid_at: String,
    payload: String,
    evidence: String,
    tags: String,
    supersedes: Option<String>,
}

impl ProjectedRow {
    pub fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            namespace: row.get(1)?,
            type_name: row.get(2)?,
            actor: row.get(3)?,
            recorded_at: row.get(4)?,
            observed_at: row.get(5)?,
            valid_at: row.get(6)?,
            payload: row.get(7)?,
            evidence: row.get(8)?,
            tags: row.get(9)?,
            supersedes: row.get(10)?,
        })
    }

    pub fn record(self) -> Result<StoredRecord, Error> {
        Ok(StoredRecord {
            id: Uuid::parse_str(&self.id)
                .map_err(|error| Error::Projection(format!("invalid projected id: {error}")))?,
            namespace: self.namespace,
            type_name: self.type_name,
            actor: self.actor,
            recorded_at: self.recorded_at,
            observed_at: self.observed_at,
            valid_at: self.valid_at,
            payload: serde_json::from_str::<Value>(&self.payload)?,
            evidence: serde_json::from_str::<Vec<EvidenceRef>>(&self.evidence)?,
            tags: serde_json::from_str::<Vec<String>>(&self.tags)?,
            supersedes: self
                .supersedes
                .map(|value| Uuid::parse_str(&value))
                .transpose()
                .map_err(|error| {
                    Error::Projection(format!("invalid projected supersedes id: {error}"))
                })?,
        })
    }
}
