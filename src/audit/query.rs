use super::{Event, capture, destination, projection, writer};
use crate::kernel::error::Error;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct Scope {
    pub since: Option<String>,
    pub until: Option<String>,
    pub surface: Option<String>,
    pub operation: Option<String>,
    pub project: Option<String>,
    pub role: Option<String>,
    pub process: Option<String>,
    pub outcome: Option<String>,
    pub instance: Option<String>,
    pub session: Option<String>,
    pub actor: Option<String>,
    pub lane: Option<String>,
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Statistics {
    pub count: u64,
    pub failures: u64,
    pub error_rate: f64,
    pub duration_min_us: u64,
    pub duration_max_us: u64,
    pub duration_mean_us: f64,
    pub duration_p95_us: u64,
}

#[derive(Debug, Serialize)]
pub struct Listing {
    pub events: Vec<Event>,
    pub limit: u16,
}

impl Scope {
    pub(super) fn normalized(&self) -> Result<Self, Error> {
        let mut scope = self.clone();
        for value in [&mut scope.since, &mut scope.until].into_iter().flatten() {
            *value = value
                .parse::<jiff::Timestamp>()
                .map_err(|_| Error::Audit("time filter must be an RFC3339 timestamp".into()))?
                .to_string();
        }
        if let (Some(since), Some(until)) = (&scope.since, &scope.until)
            && since.parse::<jiff::Timestamp>().ok() > until.parse::<jiff::Timestamp>().ok()
        {
            return Err(Error::Audit("since must not follow until".into()));
        }
        for raw in [
            &mut scope.project,
            &mut scope.role,
            &mut scope.process,
            &mut scope.instance,
            &mut scope.session,
            &mut scope.actor,
            &mut scope.lane,
        ]
        .into_iter()
        .flatten()
        {
            *raw = capture::coordinate(raw);
        }
        Ok(scope)
    }
}

pub fn list(scope: &Scope, limit: u16) -> Result<Listing, Error> {
    list_at(&destination()?, scope, limit)
}

pub fn stats(scope: &Scope) -> Result<Statistics, Error> {
    stats_at(&destination()?, scope)
}

pub fn list_at(root: &Path, scope: &Scope, limit: u16) -> Result<Listing, Error> {
    if !(1..=1000).contains(&limit) {
        return Err(Error::Audit("limit must be between 1 and 1000".into()));
    }
    let scope = scope.normalized()?;
    writer::check_root(root).map_err(|_| writer::failure())?;
    let events = if root.exists() {
        projection::list(root, &scope, limit)?
    } else {
        Vec::new()
    };
    Ok(Listing { events, limit })
}

pub fn stats_at(root: &Path, scope: &Scope) -> Result<Statistics, Error> {
    let scope = scope.normalized()?;
    writer::check_root(root).map_err(|_| writer::failure())?;
    if !root.exists() {
        return Ok(Statistics::default());
    }
    projection::stats(root, &scope)
}
