//! What comes back, and the ways it can be wrong that nothing downstream would
//! notice.
use super::super::super::super::super::embedder::Embedder;
use super::super::voyage::for_tests;
use super::{Recorder, config, documents, embedding, key_file, ok, runtime, vector};
use crate::kernel::error::Error;

fn body(value: serde_json::Value) -> Result<(u16, String), Error> {
    Ok((200, value.to_string()))
}

/// The batch is zipped against the documents that produced it, so a response
/// that arrives out of order and is trusted by position attaches every vector
/// to the wrong record. Nothing later can see it: the sizes match, the vectors
/// are valid, and the store answers plausible nonsense forever.
#[test]
fn vectors_are_placed_by_index_not_by_arrival() {
    let key = key_file("response-order", "sk-test");
    let (embedder, _) = runtime(
        &key,
        vec![body(serde_json::json!({
            "model": "voyage-4-large",
            "data": [
                { "embedding": vector(2.0), "index": 1 },
                { "embedding": vector(1.0), "index": 0 }
            ]
        }))],
    );

    let vectors = embedder.embed(&documents(2)).expect("embed");

    assert_eq!(
        vectors[0][0], 1.0,
        "the first document took the second vector"
    );
    assert_eq!(vectors[1][0], 2.0);
}

#[test]
fn a_repeated_index_is_refused_rather_than_silently_overwriting() {
    let key = key_file("response-repeat", "sk-test");
    let (embedder, _) = runtime(
        &key,
        vec![body(serde_json::json!({
            "model": "voyage-4-large",
            "data": [
                { "embedding": vector(1.0), "index": 0 },
                { "embedding": vector(2.0), "index": 0 }
            ]
        }))],
    );

    let error = embedder
        .embed(&documents(2))
        .expect_err("a repeated index was accepted");

    assert!(error.to_string().contains("repeated index"), "{error}");
}

#[test]
fn an_index_outside_the_batch_is_refused() {
    let key = key_file("response-range", "sk-test");
    let (embedder, _) = runtime(
        &key,
        vec![body(serde_json::json!({
            "model": "voyage-4-large",
            "data": [{ "embedding": vector(1.0), "index": 7 }]
        }))],
    );

    let error = embedder
        .embed(&documents(1))
        .expect_err("an out-of-range index was accepted");

    assert!(error.to_string().contains("out-of-range"), "{error}");
}

/// A different model is a different vector space. Mixing two of them in one
/// collection produces distances that mean nothing, and no later check looks.
#[test]
fn an_answer_from_another_model_is_refused() {
    let key = key_file("response-model", "sk-test");
    let (embedder, _) = runtime(
        &key,
        vec![body(serde_json::json!({
            "model": "voyage-3.5-lite",
            "data": [{ "embedding": vector(1.0), "index": 0 }]
        }))],
    );

    let error = embedder
        .embed(&documents(1))
        .expect_err("an answer from another model was accepted");

    assert!(error.to_string().contains("different model"), "{error}");
}

#[test]
fn a_short_batch_is_refused() {
    let key = key_file("response-count", "sk-test");
    let (embedder, _) = runtime(&key, vec![ok(1)]);

    let error = embedder
        .embed(&documents(2))
        .expect_err("a short batch was accepted");

    assert!(error.to_string().contains("wrong batch size"), "{error}");
}

/// Dimensions and an all-zero vector, through the same check every other
/// provider answers to.
#[test]
fn a_vector_of_the_wrong_shape_is_refused() {
    for (name, embedding_value, expected) in [
        ("dims", serde_json::json!(vec![1.0_f32; 1024]), "dimensions"),
        ("zero", serde_json::json!(vec![0.0_f32; 2048]), "all-zero"),
    ] {
        let key = key_file(&format!("response-{name}"), "sk-test");
        let (embedder, _) = runtime(
            &key,
            vec![body(serde_json::json!({
                "model": "voyage-4-large",
                "data": [{ "embedding": embedding_value, "index": 0 }]
            }))],
        );

        let error = embedder
            .embed(&documents(1))
            .expect_err("a malformed vector was accepted");

        assert!(
            error.to_string().contains(expected),
            "{name}: unexpected error {error}"
        );
    }
}

/// Finiteness is checked, and cannot be reached from here.
///
/// `validate_vector` refuses a non-finite component, but no response can carry
/// one: JSON has no literal for NaN or infinity, and a number past the range of
/// the type is refused by the parser before the vector exists. So the honest
/// statement this test can make is the one it makes — such a response is
/// rejected, as a malformed body rather than as a malformed vector. The
/// finiteness check stands behind it for callers that build vectors another
/// way, and is exercised where those callers are tested.
#[test]
fn a_component_outside_the_range_of_the_type_never_becomes_a_vector() {
    let mut components = vec!["1.0"; 2048];
    components[7] = "1e400";
    let raw = format!(
        r#"{{"model":"voyage-4-large","data":[{{"embedding":[{}],"index":0}}]}}"#,
        components.join(",")
    );
    let key = key_file("response-infinite", "sk-test");
    let (embedder, _) = runtime(&key, vec![Ok((200, raw))]);

    let error = embedder
        .embed(&documents(1))
        .expect_err("an out-of-range component was accepted");

    assert!(error.to_string().contains("response is invalid"), "{error}");
}

/// Status text says what to do and never repeats the provider. A body can echo
/// the request, and the request carried a key.
#[test]
fn a_refusal_is_reported_by_status_without_quoting_the_body() {
    let secret = "sk-live-should-never-appear";
    for (status, expected) in [
        (429_u16, "rate limit"),
        (401, "rejected the API key"),
        (500, "unavailable"),
        (404, "does not offer"),
    ] {
        let key = key_file(&format!("status-{status}"), secret);
        let (embedder, _) = runtime(
            &key,
            vec![Ok((
                status,
                serde_json::json!({ "detail": format!("Bearer {secret} was used") }).to_string(),
            ))],
        );

        let error = embedder
            .embed(&documents(1))
            .expect_err("a refusal was reported as success");
        let message = error.to_string();

        assert!(message.contains(expected), "{status}: {message}");
        assert!(
            !message.contains(secret),
            "the key reached an error message: {message}"
        );
        assert!(
            !message.contains("Bearer"),
            "the provider's body was quoted back: {message}"
        );
    }
}

/// A rate limit has to read as "try again", not as a broken store: the pass
/// keeps its checkpoint and the next one picks the tail up.
#[test]
fn a_rate_limit_says_the_pass_can_be_run_again() {
    let key = key_file("status-429-text", "sk-test");
    let (embedder, _) = runtime(&key, vec![Ok((429, String::new()))]);

    let error = embedder.embed(&documents(1)).expect_err("429 was accepted");

    assert!(error.to_string().contains("run again"), "{error}");
}

/// The runtime asks the same question the config reader does. A runtime that
/// trusts its caller stops being safe the first time it is called from
/// somewhere else.
#[test]
fn the_runtime_refuses_a_configuration_the_reader_would_refuse() {
    let key = key_file("load-optout", "sk-test");
    let mut opted_out = embedding(&key);
    opted_out.allow_remote = false;

    // Matched rather than unwrapped because `VoyageRuntime` deliberately has no
    // `Debug`: the struct is what a future diagnostic would print, and nothing
    // that touches the key should be printable by accident.
    let error = match for_tests(&config(), &opted_out, Box::new(Recorder::new(Vec::new()))) {
        Err(error) => error,
        Ok(_) => panic!("the runtime loaded without the remote opt-in"),
    };

    assert!(
        error.to_string().contains("remote embedding opt-in"),
        "{error}"
    );
}
