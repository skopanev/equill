//! What goes out: the shape of the call, and the difference between a document
//! and a question.
use super::super::super::super::super::embedder::Embedder;
use super::{documents, key_file, ok, runtime};

#[test]
fn shared_runtime_caps_queries_but_keeps_long_documents_intact() {
    use crate::vector::embedding::{EmbeddingRuntime, Runtime};

    let key = key_file("query-limit", "sk-test");
    let (provider, recorder) = runtime(&key, vec![ok(1), ok(1)]);
    let embedder = EmbeddingRuntime {
        max_query_chars: crate::vector::DEFAULT_MAX_CHARS,
        inner: Runtime::Voyage(Box::new(provider)),
    };
    let prefix = "я🦀".repeat(1_000);
    let text = format!("{prefix}keep this document tail");
    embedder.embed_query("instruction", &text).expect("query");
    let mut records = documents(1);
    records[0].text = text.clone();
    embedder.embed(&records).expect("document");

    let calls = recorder.calls();
    assert_eq!(calls[0].body["input"][0], prefix);
    assert_eq!(calls[1].body["input"][0], text);
    assert_eq!(calls[0].body["input_type"], "query");
    assert_eq!(calls[1].body["input_type"], "document");
}

/// Documents are embedded as documents. `input_type` is how the provider is
/// told which of two embeddings of the same words is wanted, and getting it
/// wrong is invisible: the vectors come back, they are the right size, and they
/// answer the wrong question slightly worse forever.
#[test]
fn documents_are_sent_as_documents_untruncated_and_as_floats() {
    let key = key_file("request-documents", "sk-test\n");
    let (embedder, recorder) = runtime(&key, vec![ok(2)]);

    embedder.embed(&documents(2)).expect("embed");

    let calls = recorder.calls();
    let body = &calls[0].body;
    assert_eq!(body["input_type"], "document");
    assert_eq!(body["truncation"], false);
    assert_eq!(body["output_dtype"], "float");
    // Sent explicitly. The provider's default is 1024 and the collection is
    // 2048, so leaving it out fails every real call — after the configuration
    // has already been accepted as valid, which is the worst moment to find out.
    assert_eq!(body["output_dimension"], 2048);
    assert_eq!(body["model"], "voyage-4-large");
    assert_eq!(body["input"][0], "document 0");
    assert_eq!(body["input"][1], "document 1");
}

/// The query goes as written. The local providers wrap it in "Instruct: …" —
/// the prefix Qwen needs to be told what kind of similarity to look for — and
/// carrying that here would embed the instruction as part of the question.
#[test]
fn a_query_is_sent_raw_as_a_query_without_the_local_models_prefix() {
    let key = key_file("request-query", "sk-test");
    let (embedder, recorder) = runtime(&key, vec![ok(1)]);

    embedder
        .embed_query(
            "Retrieve durable memory directly applicable to the current request.",
            "why did the compaction pass stall?",
        )
        .expect("embed query");

    let calls = recorder.calls();
    let body = &calls[0].body;
    assert_eq!(body["input_type"], "query");
    assert_eq!(
        body["output_dimension"], 2048,
        "a query embedded at another width cannot be compared with the documents"
    );
    assert_eq!(body["input"][0], "why did the compaction pass stall?");
    assert_eq!(
        body["input"].as_array().expect("input array").len(),
        1,
        "the instruction was sent as a second input"
    );
    let sent = body.to_string();
    assert!(
        !sent.contains("Instruct:") && !sent.contains("Retrieve durable memory"),
        "the local models' instruction prefix reached the request: {sent}"
    );
}

/// Eight at a time. A batch the provider refuses costs the whole pass, and a
/// rate limit hit halfway through a hundred is expensive to resume.
#[test]
fn documents_are_sent_eight_at_a_time() {
    let key = key_file("request-batch", "sk-test");
    let (embedder, recorder) = runtime(&key, vec![ok(8), ok(8), ok(1)]);

    let vectors = embedder.embed(&documents(17)).expect("embed");

    let calls = recorder.calls();
    let sizes = calls
        .iter()
        .map(|call| call.body["input"].as_array().expect("input").len())
        .collect::<Vec<_>>();
    assert_eq!(sizes, vec![8, 8, 1]);
    assert_eq!(vectors.len(), 17);
}

/// The key is read when it is used, not held from load. An operator who rotates
/// the file has rotated the credential; a runtime that cached it would keep
/// presenting the old one until something restarted it.
#[test]
fn the_key_is_read_for_every_call_rather_than_held() {
    let key = key_file("request-rotate", "sk-first");
    let (embedder, recorder) = runtime(&key, vec![ok(1), ok(1)]);
    embedder.embed(&documents(1)).expect("first");

    std::fs::write(&key, "sk-second").expect("rotate");
    embedder.embed(&documents(1)).expect("second");

    let calls = recorder.calls();
    assert_eq!(calls[0].key, "sk-first");
    assert_eq!(calls[1].key, "sk-second", "the runtime held the old key");
}
