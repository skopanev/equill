use crate::compact::anchor::model::{Anchor, AnchorFact};
use crate::compact::model::AnchorState;
use crate::kernel::error::Error;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub struct ManifestResolver {
    available: bool,
    states: HashMap<(String, String), AnchorState>,
}

impl ManifestResolver {
    pub fn load(path: Option<&Path>) -> Result<Self, Error> {
        let Some(path) = path else {
            return Ok(Self::unavailable());
        };
        if !path.is_file() {
            return Ok(Self::unavailable());
        }
        let text = fs::read_to_string(path)?;
        let mut states = HashMap::new();
        for (index, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let fact: AnchorFact = serde_json::from_str(line).map_err(|error| {
                Error::Compact(format!("anchor resolver line {}: {error}", index + 1))
            })?;
            if !matches!(
                fact.kind.as_str(),
                "anchor:diff" | "anchor:trunk" | "anchor:ticket"
            ) || fact.target.trim().is_empty()
            {
                return Err(Error::Compact(format!(
                    "anchor resolver line {} is invalid",
                    index + 1
                )));
            }
            if states
                .insert((fact.kind, fact.target), fact.state)
                .is_some()
            {
                return Err(Error::Compact(format!(
                    "anchor resolver line {} repeats a target",
                    index + 1
                )));
            }
        }
        Ok(Self {
            available: true,
            states,
        })
    }

    pub fn state(&self, anchor: &Anchor) -> Option<AnchorState> {
        if !self.available {
            return None;
        }
        anchor.target.as_ref().and_then(|target| {
            self.states
                .get(&(anchor.kind.clone(), target.clone()))
                .copied()
        })
    }

    fn unavailable() -> Self {
        Self {
            available: false,
            states: HashMap::new(),
        }
    }
}
