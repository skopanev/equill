//! What comes back, and the ways it can be wrong that nothing downstream would
//! notice.
use super::super::super::super::super::embedder::Embedder;
use super::super::deepinfra::for_tests;
use super::{
    DIMENSIONS, MODEL, Recorder, config, documents, embedding, key_file, ok, runtime, vector,
};
use crate::kernel::error::Error;

fn body(value: serde_json::Value) -> Result<(u16, String), Error> {
    Ok((200, value.to_string()))
}

fn datum(index: usize, value: f32) -> serde_json::Value {
    serde_json::json!({ "object": "embedding", "index": index, "embedding": vector(value) })
}

/// The batch is zipped against the documents that produced it, so a response
/// trusted by position attaches every vector to the wrong record. Nothing later
/// can see it: the sizes match, the vectors are valid, and the store answers
/// plausible nonsense for as long as it stands.
#[test]
fn vectors_are_placed_by_index_not_by_arrival() {
    let key = key_file("response-order", "di-test");
    let (embedder, _) = runtime(
        &key,
        vec![body(serde_json::json!({
            "model": MODEL,
            "data": [datum(1, 2.0), datum(0, 1.0)]
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
    let key = key_file("response-repeat", "di-test");
    let (embedder, _) = runtime(
        &key,
        vec![body(serde_json::json!({
            "model": MODEL,
            "data": [datum(0, 1.0), datum(0, 2.0)]
        }))],
    );

    let error = embedder
        .embed(&documents(2))
        .expect_err("a repeated index was accepted");

    assert!(error.to_string().contains("repeated index"), "{error}");
}

#[test]
fn an_index_outside_the_batch_is_refused() {
    let key = key_file("response-range", "di-test");
    let (embedder, _) = runtime(
        &key,
        vec![body(
            serde_json::json!({ "model": MODEL, "data": [datum(7, 1.0)] }),
        )],
    );

    let error = embedder
        .embed(&documents(1))
        .expect_err("an out-of-range index was accepted");

    assert!(error.to_string().contains("out-of-range"), "{error}");
}

/// A different model is a different vector space. Mixing two in one collection
/// produces distances that mean nothing, and no later check looks.
#[test]
fn an_answer_from_another_model_is_refused() {
    let key = key_file("response-model", "di-test");
    let (embedder, _) = runtime(
        &key,
        vec![body(serde_json::json!({
            "model": "Qwen/Qwen3-Embedding-4B",
            "data": [datum(0, 1.0)]
        }))],
    );

    let error = embedder
        .embed(&documents(1))
        .expect_err("an answer from another model was accepted");

    assert!(error.to_string().contains("different model"), "{error}");
}

#[test]
fn a_short_batch_is_refused() {
    let key = key_file("response-count", "di-test");
    let (embedder, _) = runtime(&key, vec![ok(1)]);

    let error = embedder
        .embed(&documents(2))
        .expect_err("a short batch was accepted");

    assert!(error.to_string().contains("wrong batch size"), "{error}");
}

/// The width is not requested — DeepInfra documents no `dimensions` parameter —
/// so checking it on the way back is the only place it can be checked at all.
#[test]
fn a_vector_of_the_wrong_width_is_refused() {
    let key = key_file("response-width", "di-test");
    let (embedder, _) = runtime(
        &key,
        vec![body(serde_json::json!({
            "model": MODEL,
            "data": [{ "object": "embedding", "index": 0, "embedding": vec![1.0_f32; 2048] }]
        }))],
    );

    let error = embedder
        .embed(&documents(1))
        .expect_err("a 2048-wide vector was accepted for a 4096 collection");

    assert!(error.to_string().contains("dimensions"), "{error}");
    assert_eq!(DIMENSIONS, 4096);
}

#[test]
fn an_all_zero_vector_is_refused() {
    let key = key_file("response-zero", "di-test");
    let (embedder, _) = runtime(
        &key,
        vec![body(
            serde_json::json!({ "model": MODEL, "data": [datum(0, 0.0)] }),
        )],
    );

    let error = embedder
        .embed(&documents(1))
        .expect_err("an all-zero vector was accepted");

    assert!(error.to_string().contains("all-zero"), "{error}");
}

/// Status text says what to do and never repeats the provider. DeepInfra
/// documents no error body shape, and a body can echo the request — which
/// carried a key.
#[test]
fn a_refusal_is_reported_by_status_without_quoting_the_body() {
    let secret = "di-live-should-never-appear";
    for (status, expected) in [
        (429_u16, "rate limit"),
        (401, "rejected the API key"),
        (402, "cannot be billed"),
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

/// A rate limit has to read as "try again", not as a broken store.
#[test]
fn a_rate_limit_says_the_pass_can_be_run_again() {
    let key = key_file("status-429-text", "di-test");
    let (embedder, _) = runtime(&key, vec![Ok((429, String::new()))]);

    let error = embedder.embed(&documents(1)).expect_err("429 was accepted");

    assert!(error.to_string().contains("run again"), "{error}");
}

/// The runtime asks the same question the config reader does. A runtime that
/// trusts its caller stops being safe the first time it is called from
/// somewhere else.
#[test]
fn the_runtime_refuses_a_configuration_without_the_remote_opt_in() {
    let key = key_file("load-optout", "di-test");
    let mut opted_out = embedding(&key);
    opted_out.allow_remote = false;

    // Matched rather than unwrapped because `DeepInfraRuntime` deliberately has
    // no `Debug`: the struct is what a future diagnostic would print, and
    // nothing that touches the key should be printable by accident.
    let error = match for_tests(&config(), &opted_out, Box::new(Recorder::new(Vec::new()))) {
        Err(error) => error,
        Ok(_) => panic!("the runtime loaded without the remote opt-in"),
    };

    assert!(
        error.to_string().contains("remote embedding opt-in"),
        "{error}"
    );
}

/// And the rest of the descriptor contract, refused as one.
#[test]
fn the_runtime_refuses_another_model_or_width() {
    let key = key_file("load-contract", "di-test");
    for wrong in ["Qwen/Qwen3-Embedding-4B", "text-embedding-3-large"] {
        let mut other = embedding(&key);
        other.model_id = wrong.into();
        let refused = match for_tests(&config(), &other, Box::new(Recorder::new(Vec::new()))) {
            Err(error) => error,
            Ok(_) => panic!("{wrong} was accepted"),
        };
        assert!(
            refused.to_string().contains("Qwen/Qwen3-Embedding-8B"),
            "{refused}"
        );
    }
    let mut narrow = config();
    narrow.dimensions = 2048;
    let refused = match for_tests(
        &narrow,
        &embedding(&key),
        Box::new(Recorder::new(Vec::new())),
    ) {
        Err(error) => error,
        Ok(_) => panic!("a 2048-wide collection was accepted"),
    };
    assert!(refused.to_string().contains("4096"), "{refused}");
}
