//! What goes out: the shape of the call, and the difference between a document
//! and a question — which here exists ONLY in the text.
use super::super::super::super::super::embedder::Embedder;
use super::{DIMENSIONS, MODEL, documents, key_file, ok, runtime};

/// Documents go bare. Qwen's model card is explicit that retrieval documents
/// take no instruction, and adding one would put them in a different space from
/// every document embedded before.
#[test]
fn documents_are_sent_bare_as_floats_without_a_dimensions_parameter() {
    let key = key_file("request-documents", "di-test\n");
    let (embedder, recorder) = runtime(&key, vec![ok(2)]);

    embedder.embed(&documents(2)).expect("embed");

    let calls = recorder.calls();
    let body = &calls[0].body;
    assert_eq!(body["model"], MODEL);
    assert_eq!(body["encoding_format"], "float");
    assert_eq!(body["input"][0], "document 0");
    assert_eq!(body["input"][1], "document 1");
    assert!(
        !body.to_string().contains("Instruct:"),
        "a document carried the query instruction: {body}"
    );
    // DeepInfra documents no `dimensions` parameter for this endpoint. Sending
    // one invented field is how a request starts being refused for a reason
    // nobody can find.
    assert!(
        body.get("dimensions").is_none() && body.get("input_type").is_none(),
        "the request carried a field the API does not document: {body}"
    );
}

/// The opposite of Voyage, and this is the whole reason both providers exist
/// separately. DeepInfra has no field that says "this is a question", so the
/// instruction has to be in the text — and Qwen's card gives the exact
/// template, `Instruct: {task}\nQuery:{query}`, with no space after `Query:`.
#[test]
fn a_query_carries_qwens_instruction_prefix_verbatim() {
    let key = key_file("request-query", "di-test");
    let (embedder, recorder) = runtime(&key, vec![ok(1)]);

    embedder
        .embed_query(
            "Retrieve durable memory directly applicable to the current request.",
            "why did the compaction pass stall?",
        )
        .expect("embed query");

    let calls = recorder.calls();
    let sent = calls[0].body["input"][0]
        .as_str()
        .expect("input is a string");
    assert_eq!(
        sent,
        "Instruct: Retrieve durable memory directly applicable to the current request.\nQuery:why did the compaction pass stall?"
    );
    assert_eq!(
        calls[0].body["input"]
            .as_array()
            .expect("input array")
            .len(),
        1,
        "the instruction was sent as a second input instead of a prefix"
    );
}

/// Eight at a time — ours, not the provider's: DeepInfra documents no batch
/// limit for this model. A small batch keeps a refused call cheap to retry and
/// a rate limit cheap to hit.
#[test]
fn documents_are_sent_eight_at_a_time() {
    let key = key_file("request-batch", "di-test");
    let (embedder, recorder) = runtime(&key, vec![ok(8), ok(8), ok(1)]);

    let vectors = embedder.embed(&documents(17)).expect("embed");

    let calls = recorder.calls();
    let sizes = calls
        .iter()
        .map(|call| call.body["input"].as_array().expect("input").len())
        .collect::<Vec<_>>();
    assert_eq!(sizes, vec![8, 8, 1]);
    assert_eq!(vectors.len(), 17);
    assert!(vectors.iter().all(|vector| vector.len() == DIMENSIONS));
}

/// One call per batch and no hidden retry. Against a paid endpoint a silent
/// retry doubles a bill and hides the rate limit from whoever has to act on it.
#[test]
fn a_refused_call_is_not_retried_behind_the_callers_back() {
    let key = key_file("request-no-retry", "di-test");
    let (embedder, recorder) = runtime(&key, vec![Ok((429, String::new()))]);

    embedder
        .embed(&documents(1))
        .expect_err("a rate limit was swallowed");

    assert_eq!(recorder.calls().len(), 1, "the provider retried by itself");
}

/// The key is read when it is used, not held from load. An operator who
/// rotates the file has rotated the credential.
#[test]
fn the_key_is_read_for_every_call_rather_than_held() {
    let key = key_file("request-rotate", "di-first");
    let (embedder, recorder) = runtime(&key, vec![ok(1), ok(1)]);
    embedder.embed(&documents(1)).expect("first");

    std::fs::write(&key, "di-second").expect("rotate");
    embedder.embed(&documents(1)).expect("second");

    let calls = recorder.calls();
    assert_eq!(calls[0].key, "di-first");
    assert_eq!(calls[1].key, "di-second", "the runtime held the old key");
}
