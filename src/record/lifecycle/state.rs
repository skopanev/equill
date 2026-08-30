use super::super::StoredRecord;
use super::watermark::{self, Watermark};
use crate::kernel::error::Error;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use uuid::Uuid;

const STATE: &str = "projections/lifecycle/state.jsonl";

/// What lifecycle validation needs to know about a record, and nothing else.
///
/// Not the payload, not the evidence, not the timestamps — only the fields the
/// rules actually read. A store with a hundred thousand records has a hundred
/// thousand of these, which is small; it has a hundred thousand full records,
/// which is not.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Entry {
    pub(crate) type_name: String,
    pub(crate) namespace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) supersedes: Option<Uuid>,
    /// The lifecycle keys this record presents, one per linear type that could
    /// claim it as a head.
    ///
    /// Not one key but a map, because a linear type checks head uniqueness
    /// across every type it accepts as a predecessor, reading the key through
    /// ITS OWN pointer. A record of an older type therefore has a key under the
    /// newer type's pointer, and storing only its own would lose exactly the
    /// case a migration creates.
    ///
    /// Values, not text: the rules compare by JSON equality, and stringifying
    /// would make 1 and "1" indistinguishable.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) keys: BTreeMap<String, serde_json::Value>,
}

/// One appended line: an entry with the id it belongs to.
#[derive(Deserialize, Serialize)]
struct Line {
    id: Uuid,
    #[serde(flatten)]
    entry: Entry,
}

#[derive(Debug)]
pub(crate) struct LifecycleState {
    pub(crate) watermark: Watermark,
    pub(crate) entries: BTreeMap<Uuid, Entry>,
    /// Ids that something supersedes. A head is a record absent from this.
    pub(crate) superseded: BTreeSet<Uuid>,
    /// Ids gained since the last save — the only lines a save has to write.
    pending: Vec<Uuid>,
    /// The chained digest of every line written so far.
    chain: String,
    /// Whether the file on disk has to be replaced rather than extended. True
    /// for a state built from the ledger, because whatever is on disk was not
    /// the source it was built from.
    rewrite: bool,
}

impl LifecycleState {
    pub(crate) fn head(&self, id: &Uuid) -> bool {
        !self.superseded.contains(id)
    }

    /// Every current head presenting this key to this linear type — of that
    /// type or of any type it accepts as a predecessor.
    pub(crate) fn heads_claiming(
        &self,
        linear_type: &str,
        namespace: &str,
        key: &serde_json::Value,
    ) -> Vec<Uuid> {
        self.entries
            .iter()
            .filter(|(id, entry)| {
                self.head(id)
                    && entry.namespace == namespace
                    && entry.keys.get(linear_type) == Some(key)
            })
            .map(|(id, _)| *id)
            .collect()
    }

    pub(crate) fn record(
        &mut self,
        record: &StoredRecord,
        keys: BTreeMap<String, serde_json::Value>,
    ) {
        if let Some(target) = record.supersedes {
            self.superseded.insert(target);
        }
        self.pending.push(record.id);
        self.entries.insert(
            record.id,
            Entry {
                type_name: record.type_name.clone(),
                namespace: record.namespace.clone(),
                supersedes: record.supersedes,
                keys,
            },
        );
    }
}

/// Load the state, or refuse it.
///
/// `None` means there is no usable state and one must be built. It is never a
/// reason to proceed without validating: a caller that cannot load this has to
/// fall back to the full ledger, and the point of the watermark is that a state
/// which no longer describes the ledger is treated as absent rather than
/// trusted.
pub(crate) fn load(store: &Path) -> Result<Option<LifecycleState>, Error> {
    let lines = store.join(STATE);
    let (Some((claimed, chain)), true) = (watermark::read(store)?, lines.is_file()) else {
        return Ok(None);
    };
    if watermark::observe(store)? != claimed {
        // The ledger moved without this state moving with it. Refuse it.
        return Ok(None);
    }
    let mut state = empty();
    state.rewrite = false;
    for line in fs::read_to_string(&lines)?.lines() {
        if line.trim().is_empty() {
            continue;
        }
        state.chain = watermark::extend(&state.chain, line);
        let Ok(Line { id, entry }) = serde_json::from_str::<Line>(line) else {
            return Ok(None);
        };
        if let Some(target) = entry.supersedes {
            state.superseded.insert(target);
        }
        state.entries.insert(id, entry);
    }
    if state.chain != chain {
        // The lines are not the lines this marker was written for. Something
        // edited them; whether by damage or by hand does not matter, because
        // this state authorizes appends and an unverified authority is not one.
        return Ok(None);
    }
    state.watermark = claimed;
    Ok(Some(state))
}

/// Persist what this state has gained.
///
/// One line per new record, appended — because rewriting every entry on every
/// write would trade a linear read for a linear write, which is not a trade.
/// The marker is written after the lines and replaced atomically, so a crash
/// between them leaves a watermark that no longer matches the ledger: the next
/// write rebuilds, which costs one scan rather than a state that claims to
/// cover records it does not hold.
pub(crate) fn save(store: &Path, state: &mut LifecycleState) -> Result<(), Error> {
    let lines = store.join(STATE);
    let directory = lines
        .parent()
        .ok_or_else(|| Error::Integrity("lifecycle state path is invalid".into()))?;
    fs::create_dir_all(directory)?;
    if state.rewrite {
        // Built from the ledger rather than extended, so whatever is on disk is
        // not a prefix of this and must go.
        watermark::discard(store);
        state.pending = state.entries.keys().copied().collect();
        state.chain = String::new();
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(!state.rewrite)
        .write(state.rewrite)
        .truncate(state.rewrite)
        .open(&lines)?;
    let mut buffer = Vec::new();
    for id in std::mem::take(&mut state.pending) {
        let Some(entry) = state.entries.get(&id) else {
            continue;
        };
        let line = serde_json::to_string(&Line {
            id,
            entry: entry.clone(),
        })?;
        state.chain = watermark::extend(&state.chain, &line);
        buffer.extend_from_slice(line.as_bytes());
        buffer.push(b'\n');
    }
    file.write_all(&buffer)?;
    file.sync_data()?;
    drop(file);
    state.rewrite = false;
    state.watermark = watermark::commit(store, directory, state.chain.clone())?;
    Ok(())
}

pub(crate) fn empty() -> LifecycleState {
    LifecycleState {
        watermark: Watermark::default(),
        entries: BTreeMap::new(),
        superseded: BTreeSet::new(),
        pending: Vec::new(),
        chain: String::new(),
        rewrite: true,
    }
}
