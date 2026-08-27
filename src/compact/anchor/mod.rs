pub mod model;
mod provider;

use crate::kernel::error::Error;
use model::Anchor;

pub use provider::manifest::ManifestResolver;

pub fn from_tags(tags: &[String]) -> Result<Option<Anchor>, Error> {
    let kinds = ["anchor:diff", "anchor:trunk", "anchor:ticket"];
    let mut found = Vec::new();
    for tag in tags {
        for kind in kinds {
            if tag == kind {
                found.push(Anchor {
                    kind: kind.into(),
                    target: target_tag(tags),
                });
            } else if let Some(target) = tag.strip_prefix(&format!("{kind}:")) {
                found.push(Anchor {
                    kind: kind.into(),
                    target: nonempty(target),
                });
            }
        }
    }
    if found.len() > 1 {
        return Err(Error::Compact(
            "record declares multiple lifecycle anchors".into(),
        ));
    }
    Ok(found.pop())
}

fn target_tag(tags: &[String]) -> Option<String> {
    tags.iter()
        .find_map(|tag| tag.strip_prefix("anchor-target:"))
        .and_then(nonempty)
}

fn nonempty(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.to_owned())
}
