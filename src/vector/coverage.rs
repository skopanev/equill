//! Selectors that ask the index for a type the index was told not to hold.
//!
//! `embed_types` narrows what is embedded. A selector with a vector strategy
//! over a type outside that list is not an error anywhere — it simply returns
//! nothing, which reads to whoever wrote the profile as "no relevant memory"
//! rather than "never indexed". That is the failure this exists to make
//! visible: a wrong answer nobody has any reason to doubt.
use crate::context::Strategy;
use crate::kernel::error::Error;
use serde::Deserialize;
use serde::Serialize;
use std::fs;
use std::path::Path;

/// Which types this store embeds, read without verifying model artifacts.
///
/// What a store *would* embed is a property of its registry, not of whether
/// its model files are present and hash correctly. A status report has to
/// answer while the model is missing, and the corpus is counted on paths that
/// never load a model at all — so this reads the one field and nothing else.
pub(crate) fn embed_types(store: &Path) -> Result<Vec<String>, Error> {
    let path = store.join(super::config::CONFIG);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    #[derive(Deserialize)]
    struct Filter {
        #[serde(default)]
        embed_types: Vec<String>,
    }
    let filter: Filter = serde_json::from_slice(&fs::read(path)?)?;
    Ok(filter.embed_types)
}

/// A type name has to be one, whatever the registry happens to hold today.
pub(crate) fn validate_names(names: &[String]) -> Result<(), Error> {
    if names
        .iter()
        .any(|name| name.trim().is_empty() || name.chars().any(char::is_control))
    {
        return Err(super::model::vector_error(
            "embed_types entries must be non-empty type names",
        ));
    }
    Ok(())
}

/// Every registered selector, whether or not a profile names it. A selector
/// nobody uses yet is exactly the one a coverage check has to see.
fn selector_ids(store_root: &Path) -> Result<Vec<String>, Error> {
    let directory = store_root.join("registry/selectors");
    if !directory.is_dir() {
        return Ok(Vec::new());
    }
    let mut ids = std::fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|value| value == "json"))
        .filter_map(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    ids.sort();
    Ok(ids)
}

#[derive(Debug, Serialize)]
pub struct Uncovered {
    pub selector: String,
    #[serde(rename = "type")]
    pub type_name: String,
}

/// Every registered selector that would search vectors for a type this store
/// does not embed. Empty when no filter is configured, because then there is
/// no type the index was told to leave out.
pub fn uncovered(store_root: &Path) -> Result<Vec<Uncovered>, Error> {
    let embed_types = embed_types(store_root)?;
    if embed_types.is_empty() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for id in selector_ids(store_root)? {
        // A selector that will not load is a fault the health check already
        // reports on its own terms; it is not evidence about coverage.
        let Ok((selector, _)) = crate::context::load_selector(store_root, &id) else {
            continue;
        };
        let searches_vectors = selector.strategies.contains(&Strategy::Hybrid);
        if searches_vectors && !embed_types.contains(&selector.type_name) {
            found.push(Uncovered {
                selector: selector.id,
                type_name: selector.type_name,
            });
        }
    }
    Ok(found)
}
