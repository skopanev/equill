//! The credential: where it is read from, what is refused, and what is said
//! when it will not load.
use super::super::super::super::super::embedder::Embedder;
use super::{documents, key_file, ok, runtime};
use std::path::PathBuf;

/// A trailing newline is what a file written by an editor or a here-doc has,
/// and it would otherwise travel into the header and come back as an
/// authentication failure — sending the operator to look at the wrong thing.
#[test]
fn surrounding_whitespace_is_trimmed_rather_than_sent() {
    let key = key_file("key-trim", "  di-test\n");
    let (embedder, recorder) = runtime(&key, vec![ok(1)]);

    embedder.embed(&documents(1)).expect("embed");

    assert_eq!(recorder.calls()[0].key, "di-test");
}

/// Whitespace inside is not something to trim away: it means the file holds
/// something that is not a key, and guessing which part is the key is worse
/// than refusing.
#[test]
fn a_key_a_header_cannot_carry_is_refused() {
    for (name, contents) in [
        ("space", "di-test and a comment"),
        ("newline", "di-first\ndi-second"),
        ("tab", "di\ttest"),
    ] {
        let key = key_file(&format!("key-{name}"), contents);
        let (embedder, _) = runtime(&key, vec![ok(1)]);

        let error = embedder
            .embed(&documents(1))
            .expect_err("a malformed key was sent");

        assert!(
            error.to_string().contains("a header cannot carry"),
            "{name}: {error}"
        );
    }
}

#[test]
fn an_empty_key_file_is_refused() {
    let key = key_file("key-empty", "   \n");
    let (embedder, _) = runtime(&key, vec![ok(1)]);

    let error = embedder
        .embed(&documents(1))
        .expect_err("an empty key file was accepted");

    assert!(error.to_string().contains("is empty"), "{error}");
}

/// A path pointing at something enormous is a mistake, and reading it is the
/// harm. It fails as a bad key file rather than as a process that grew.
#[test]
fn a_file_too_large_to_be_a_key_is_refused_without_being_read() {
    let key = key_file("key-large", &"k".repeat(8193));
    let (embedder, _) = runtime(&key, vec![ok(1)]);

    let error = embedder
        .embed(&documents(1))
        .expect_err("an oversized key file was accepted");

    assert!(error.to_string().contains("too large"), "{error}");
}

/// The path is named by the operator and the failure is theirs to fix, so the
/// message says what went wrong — and the file's contents never appear in it,
/// because the contents are the secret. Nor does a request go out without one.
#[test]
fn a_missing_key_file_fails_without_quoting_anything() {
    let missing = PathBuf::from("/nonexistent/equill/deepinfra/token");
    let (embedder, recorder) = runtime(&missing, vec![ok(1)]);

    let error = embedder
        .embed(&documents(1))
        .expect_err("a missing key file was accepted");

    assert!(error.to_string().contains("could not be read"), "{error}");
    assert!(
        recorder.calls().is_empty(),
        "the request went out without a key"
    );
}
