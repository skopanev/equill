//! Merging what two searches found into one answer, reproducibly.
//!
//! Semantic and text search disagree by design: one ranks by meaning, the other
//! by words, and neither score means anything in the other's units. So the
//! merge uses positions rather than scores — reciprocal rank fusion — and a
//! record that both found is rewarded for it once, not counted twice.
use crate::projection::SearchHit;
use std::collections::HashMap;
use uuid::Uuid;

/// Damps the first place's advantage so one list cannot decide the answer
/// alone: with `K` this large, being first in one list and absent from the
/// other beats being second in both only barely, which is the point of fusing
/// at all.
const K: u128 = 60;

/// One record's standing, as an exact fraction.
///
/// Deliberately not a float. The obvious `1.0 / (k + rank)` summed in whatever
/// order the lists arrive is not associative, so two runs over identical inputs
/// can order two close records differently — and a store that promises the same
/// answer for the same question cannot afford an ordering that depends on how
/// the addition happened to be scheduled. A record appears in at most two
/// lists, so the sum is one fraction and comparison is exact.
struct Score {
    numerator: u128,
    denominator: u128,
}

impl Score {
    /// `1/(K+v) + 1/(K+t)`, with an absent rank contributing nothing rather
    /// than a penalty: a record the other search never saw is not evidence
    /// against it, it is silence.
    fn of(vector_rank: Option<u128>, text_rank: Option<u128>) -> Self {
        match (vector_rank, text_rank) {
            (Some(v), Some(t)) => Score {
                numerator: (K + t) + (K + v),
                denominator: (K + v) * (K + t),
            },
            (Some(v), None) => Score {
                numerator: 1,
                denominator: K + v,
            },
            (None, Some(t)) => Score {
                numerator: 1,
                denominator: K + t,
            },
            // Never constructed: a standing exists because some list produced
            // it. Ranked last rather than unwrapped, so a future caller cannot
            // turn a logic slip into a panic on a read path.
            (None, None) => Score {
                numerator: 0,
                denominator: 1,
            },
        }
    }

    /// Cross-multiplied, so the comparison stays in integers. Ranks are bounded
    /// by the page size and `K` is small, so the products cannot overflow.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.numerator * other.denominator).cmp(&(other.numerator * self.denominator))
    }
}

/// Fuse two ranked lists into one, best first.
///
/// The result is a total order, not merely a sorted one: equal scores are
/// broken by record id. Without that, records that tie — which reciprocal rank
/// fusion produces routinely, since rank 3 in one list scores exactly rank 3 in
/// the other — would come out in whatever order the map happened to yield, and
/// the answer would stop being reproducible for a reason no caller could see.
pub fn fuse(vector: Vec<SearchHit>, text: Vec<SearchHit>) -> Vec<SearchHit> {
    // The semantic list is read first, so a record both searches found keeps
    // that copy. The two carry the same record — they are the same ledger
    // entry — but choosing on purpose beats choosing by whichever loop ran
    // last.
    let mut standings: HashMap<Uuid, (SearchHit, Option<u128>, Option<u128>)> = HashMap::new();
    for (position, hit) in vector.into_iter().enumerate() {
        let rank = position as u128 + 1;
        let entry = standings.entry(hit.record.id).or_insert((hit, None, None));
        // First place wins a repeat: a list that somehow named a record twice
        // should be scored on where it ranked it best, not last.
        entry.1 = entry.1.or(Some(rank));
    }
    for (position, hit) in text.into_iter().enumerate() {
        let rank = position as u128 + 1;
        let entry = standings.entry(hit.record.id).or_insert((hit, None, None));
        entry.2 = entry.2.or(Some(rank));
    }
    let mut fused = standings
        .into_values()
        .map(|(hit, vector_rank, text_rank)| (Score::of(vector_rank, text_rank), hit))
        .collect::<Vec<_>>();
    fused.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.record.id.cmp(&right.1.record.id))
    });
    fused.into_iter().map(|(_, hit)| hit).collect()
}
