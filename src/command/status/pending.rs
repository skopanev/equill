//! How much work the vector index still owes the ledger, when that can be said
//! at all.
//!
//! Subtracting the checkpoint's record count from the corpus size looks like an
//! answer and is not one. Replacing a record leaves the count unchanged while
//! the work is real; superseding one and appending another grows it by one
//! while two records are new. The difference is a lower bound on nothing.
//!
//! It becomes a real count in exactly one case: when the records the checkpoint
//! covered are still the first ones in the corpus, unchanged. The corpus digest
//! is the hash of each record's line hash concatenated in id order, so hashing
//! the first `indexed` of them and comparing against the stored checkpoint
//! answers that without asking the provider anything.
use crate::kernel::digest::sha256_hex;
use crate::vector::{Checkpoint, Position};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Pending {
    /// The index matches the ledger.
    None,
    /// Records outside the last successful checkpoint. Not what an in-flight
    /// pass has yet to upsert — nothing durable records that.
    Records { count: usize },
    /// There is work, and its size cannot be derived from what the store keeps.
    Unknown { reason: &'static str },
}

/// Decides between the three from the reading the whole report shares.
pub fn assess(position: &Position) -> Pending {
    let (indexed, digest) = match &position.checkpoint {
        Checkpoint::Unusable { reason } => return Pending::Unknown { reason },
        Checkpoint::Usable {
            indexed, digest, ..
        } => (*indexed, digest),
    };
    let corpus = position.corpus.as_slice();
    if *digest == position.corpus_digest {
        return Pending::None;
    }
    if indexed > corpus.len() {
        return Pending::Unknown {
            reason: "the checkpoint covers more records than the ledger holds",
        };
    }
    if prefix_digest(&corpus[..indexed]) != *digest {
        // Something inside the covered range changed, so the records beyond it
        // are not the whole of the outstanding work.
        return Pending::Unknown {
            reason: "records inside the checkpoint changed",
        };
    }
    Pending::Records {
        count: corpus.len() - indexed,
    }
}

impl Pending {
    /// The same answer as a plain number, for the component line that only has
    /// room for one. `None` where the size is unknown — never a zero standing in
    /// for "cannot say", which is what the component used to print beside a
    /// store block saying the opposite.
    pub fn records(&self) -> Option<usize> {
        match self {
            Pending::None => Some(0),
            Pending::Records { count } => Some(*count),
            Pending::Unknown { .. } => Option::None,
        }
    }
}

fn prefix_digest(records: &[(crate::record::StoredRecord, String)]) -> String {
    let mut accumulator = String::new();
    for (_, digest) in records {
        accumulator.push_str(digest);
    }
    sha256_hex(accumulator.as_bytes())
}
