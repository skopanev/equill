use super::model::{VectorSearchHit, VectorSearchRequest, vector_error};
use super::provider::qdrant::ProviderHit;
use crate::kernel::digest::sha256_hex;
use crate::kernel::error::Error;
use crate::record::{self, StoredRecord};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use uuid::Version;

pub(super) fn from_ledger(
    store: &Path,
    request: &VectorSearchRequest,
    candidates: Vec<ProviderHit>,
) -> Result<Vec<VectorSearchHit>, Error> {
    let locators = crate::projection::locators(
        store,
        &crate::projection::LocatorRequest {
            ids: candidates
                .iter()
                .map(|candidate| candidate.record_id)
                .collect(),
        },
    )?;
    if locators.located.len() != candidates.len() {
        return Err(vector_error(
            "query candidate locator is missing; rebuild the text projection",
        ));
    }
    let records = record::read_located(store, &locators.located)?;
    hydrate(request, candidates, records)
}

fn hydrate(
    request: &VectorSearchRequest,
    candidates: Vec<ProviderHit>,
    records: Vec<StoredRecord>,
) -> Result<Vec<VectorSearchHit>, Error> {
    let by_id = records
        .into_iter()
        .map(|record| (record.id, record))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .map(|candidate| {
            if candidate.record_id.get_version() != Some(Version::SortRand)
                || !seen.insert(candidate.record_id)
            {
                return Err(vector_error("query returned an invalid record coordinate"));
            }
            let record = by_id
                .get(&candidate.record_id)
                .ok_or_else(|| vector_error("query candidate is absent from immutable ledger"))?;
            if !matches_filter(&request.namespaces, &record.namespace)
                || !matches_filter(&request.type_names, &record.type_name)
            {
                return Err(vector_error("query candidate violates requested filters"));
            }
            let actual = sha256_hex(&serde_json::to_vec(record)?);
            if actual != candidate.record_sha256 {
                return Err(vector_error("query candidate record SHA-256 mismatch"));
            }
            Ok(VectorSearchHit {
                record: record.clone(),
                score: candidate.score,
                input_sha256: candidate.input_sha256,
            })
        })
        .collect()
}

fn matches_filter(values: &[String], actual: &str) -> bool {
    values.is_empty() || values.iter().any(|value| value == actual)
}

#[cfg(test)]
pub(super) fn test_hydrate(
    request: &VectorSearchRequest,
    candidates: Vec<ProviderHit>,
    records: Vec<StoredRecord>,
) -> Result<Vec<VectorSearchHit>, Error> {
    hydrate(request, candidates, records)
}

#[cfg(test)]
thread_local! {
    static CANDIDATES: std::cell::RefCell<Option<Vec<ProviderHit>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn test_candidates() -> Option<Vec<ProviderHit>> {
    CANDIDATES.with(|slot| slot.borrow().clone())
}

/// Only the external index response is substituted; canonical hydration and
/// embedding-input verification remain on the production retrieval path.
#[cfg(test)]
pub(super) fn with_candidates<T>(hits: Vec<ProviderHit>, body: impl FnOnce() -> T) -> T {
    struct Restore(Option<Vec<ProviderHit>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CANDIDATES.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(CANDIDATES.with(|slot| slot.replace(Some(hits))));
    body()
}
