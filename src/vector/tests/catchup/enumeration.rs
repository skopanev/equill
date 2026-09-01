//! Whether an answer says how much of itself it is.
use super::{add, store};
use std::fs;

/// A page and the whole answer must never look alike. Reading a hundred results
/// and concluding there are a hundred is what froze a project: the count was
/// right, the page was right, and nothing said they were different numbers.
///
/// A total is reported only where it was actually established. A plain page
/// never enumerated the scope, so it says it was cut and declines to guess how
/// much was left; `--all` enumerates, and then the number is real.
#[test]
fn a_page_says_how_much_it_left_behind() {
    let root = store("truncation");
    for index in 0..150 {
        add(&root, &format!("rule number {index} about deployment"));
    }

    let page = search(&root, 100, false);
    let everything = search(&root, 100, true);
    let small_page = search(&root, 20, false);

    // The page admits it is a page, without inventing a total it never counted.
    assert_eq!(page["returned_count"], 100);
    assert_eq!(page["truncated"], true);
    assert!(page.get("total_matches").is_none(), "no unproven total");
    assert_eq!(small_page["returned_count"], 20);
    assert_eq!(small_page["truncated"], true);
    // --all enumerates, so the total is real and the answer is complete.
    assert_eq!(everything["returned_count"], 150);
    assert_eq!(everything["total_matches"], 150);
    assert_eq!(everything["truncated"], false);
    fs::remove_dir_all(root).expect("cleanup");
}

/// The case that actually stopped the project: a filter narrows to more than a
/// page holds. Here the scope is enumerated, so the total is established and
/// the gap between it and the page is stated outright.
#[test]
fn a_filtered_page_states_the_total_it_could_not_fit() {
    let root = store("filtered-truncation");
    for index in 0..150 {
        add(&root, &format!("rule number {index} about deployment"));
    }

    let page = filtered(&root, 100, false);
    let everything = filtered(&root, 100, true);

    assert_eq!(
        page["total_matches"], 150,
        "the filter enumerated the scope"
    );
    assert_eq!(page["returned_count"], 100);
    assert_eq!(page["truncated"], true);
    assert_eq!(everything["total_matches"], 150);
    assert_eq!(everything["returned_count"], 150);
    assert_eq!(everything["truncated"], false);
    fs::remove_dir_all(root).expect("cleanup");
}

fn filtered(root: &std::path::Path, limit: u16, all: bool) -> serde_json::Value {
    run(root, limit, all, vec!["type=agent.lesson.v1".to_string()])
}

pub(super) fn search(root: &std::path::Path, limit: u16, all: bool) -> serde_json::Value {
    run(root, limit, all, Vec::new())
}

pub(super) fn run(
    root: &std::path::Path,
    limit: u16,
    all: bool,
    filters: Vec<String>,
) -> serde_json::Value {
    let output = crate::command::query::search(
        true,
        root.to_path_buf(),
        Some("deployment claim rule".into()),
        None,
        None,
        limit,
        Some(crate::command::cli::StrategyArg::Fts),
        filters,
        false,
        crate::command::cli::FormatArg::Jsonl,
        Vec::new(),
        all,
    )
    .expect("search");
    serde_json::from_str(&output).expect("report json")
}

/// Both surfaces must answer "is this all of it" the same way. A candidate pool
/// that filled up is not a total, and reporting it as one on either side would
/// let a caller trust a page.
#[test]
fn the_adapter_and_the_command_settle_a_page_identically() {
    let root = store("parity");
    for index in 0..150 {
        add(&root, &format!("rule number {index} about deployment"));
    }

    let command = search(&root, 100, false);
    let adapter = through_mcp(&root, 100, None);
    let filtered_command = run(&root, 100, false, vec!["type=agent.lesson.v1".into()]);
    let filtered_adapter = through_mcp(&root, 100, Some("type=agent.lesson.v1"));

    // Unfiltered: neither claims a total, both admit the page was cut.
    assert!(command.get("total_matches").is_none());
    assert!(adapter.get("total_matches").is_none());
    assert_eq!(command["truncated"], adapter["truncated"]);
    assert_eq!(command["truncated"], true);
    // Filtered: the scope was enumerated, so both state the same total.
    assert_eq!(filtered_command["total_matches"], 150);
    assert_eq!(filtered_adapter["total_matches"], 150);
    assert_eq!(filtered_adapter["returned_count"], 100);
    assert_eq!(filtered_adapter["truncated"], true);
    fs::remove_dir_all(root).expect("cleanup");
}

/// Completeness is refused where it cannot be proven, and refused before any
/// results are printed rather than qualified afterwards.
#[test]
fn asking_for_everything_from_an_approximate_index_is_refused_upfront() {
    let root = store("all-boundary");
    add(&root, "a rule about deployment");

    let semantic = crate::command::query::search(
        true,
        root.clone(),
        Some("deployment".into()),
        None,
        None,
        100,
        Some(crate::command::cli::StrategyArg::Vector),
        Vec::new(),
        false,
        crate::command::cli::FormatArg::Jsonl,
        Vec::new(),
        true,
    );
    let hybrid = crate::command::query::search(
        true,
        root.clone(),
        Some("deployment".into()),
        None,
        None,
        100,
        Some(crate::command::cli::StrategyArg::Hybrid),
        Vec::new(),
        false,
        crate::command::cli::FormatArg::Jsonl,
        Vec::new(),
        true,
    );

    for refusal in [semantic, hybrid] {
        let message = refusal
            .expect_err("--all cannot be promised here")
            .to_string();
        assert!(message.contains("enumerate"), "{message}");
    }
    // The same request without --all is fine: only the promise was impossible.
    search(&root, 100, false);
    fs::remove_dir_all(root).expect("cleanup");
}

/// An unset strategy is chosen for the request, and the choice is the one the
/// request can be served by.
///
/// The refusal above is the reason this matters: making `hybrid` the default
/// outright would have turned every existing `--all` script into an error
/// overnight. So the default is a decision, not a constant, and `--all` still
/// reaches the path that can enumerate.
#[test]
fn an_unstated_strategy_follows_what_the_request_promised() {
    let root = store("auto-strategy");
    add(&root, "a rule about deployment");

    let complete = crate::command::query::search(
        true,
        root.clone(),
        Some("deployment".into()),
        None,
        None,
        100,
        None,
        Vec::new(),
        false,
        crate::command::cli::FormatArg::Jsonl,
        Vec::new(),
        true,
    )
    .expect("--all with no stated strategy is served by text");
    let ordinary = crate::command::query::search(
        true,
        root.clone(),
        Some("deployment".into()),
        None,
        None,
        100,
        None,
        Vec::new(),
        false,
        crate::command::cli::FormatArg::Jsonl,
        Vec::new(),
        false,
    )
    .expect("an ordinary query is served");

    // `--all` keeps the exact total it always had; without it the store has no
    // vector index here, so the semantic half stands aside and says so.
    assert!(complete.contains("\"total_matches\""), "{complete}");
    assert!(ordinary.contains("\"answered_by\":\"fts\""), "{ordinary}");
    assert!(ordinary.contains("\"fallback\""), "{ordinary}");
    fs::remove_dir_all(root).expect("cleanup");
}

fn through_mcp(root: &std::path::Path, limit: u16, filter: Option<&str>) -> serde_json::Value {
    let mut arguments = serde_json::json!({ "query": "deployment claim rule", "limit": limit });
    if let Some(filter) = filter {
        arguments["where"] = serde_json::json!([filter]);
    }
    crate::mcp::tools_call(root, "owner", false, "search", &arguments).expect("mcp search")
}
