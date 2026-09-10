//! Context for a native editor lifecycle hook.
//!
//! A thin adapter and nothing more: the event name is validated, the existing
//! `context` tool does the work, and its bundle is returned in the envelope a
//! hook expects. No question is invented here — the launcher supplies one as
//! `query`, and every cap, skip rule and budget stays where it already was.
use super::arguments::text;
use crate::kernel::error::Error;
use serde_json::{Value, json};
use std::path::Path;

/// The events this adapter answers for. A harness that sends something else
/// gets a refusal naming what it sent: guessing which event it meant would put
/// one editor's event name into another editor's output, and a harness that
/// receives the wrong one ignores the result.
const EVENTS: [&str; 3] = ["UserPromptSubmit", "PostToolBatch", "PostToolUse"];

pub(super) fn context(
    store: &Path,
    actor: &str,
    log_queries: bool,
    arguments: &Value,
) -> Result<Value, Error> {
    let event = text(arguments, "hook_event_name")?;
    if !EVENTS.contains(&event) {
        return Err(Error::InvalidRecord(format!(
            "hook_event_name must be one of {}; got {event}",
            EVENTS.join(", ")
        )));
    }
    let bundle = super::tools::assemble(store, actor, log_queries, arguments)?;
    // Named, not defaulted. An empty `additionalContext` is how a harness sees
    // "this store had nothing to say", so a bundle this adapter cannot read
    // must not borrow that appearance.
    let additional = bundle["content"].as_str().ok_or_else(|| {
        Error::InvalidRecord("the assembled context carried no content field".into())
    })?;
    Ok(json!({
        "hookSpecificOutput": {
            "hookEventName": event,
            "additionalContext": additional,
        }
    }))
}
