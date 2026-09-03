use super::{FakeEmbedder, PHYSICAL, embedder, fixture};
use crate::kernel::error::Error;
use crate::vector::operator::execute_with_progress;
use crate::vector::{VectorState, state};
use std::fs;

#[test]
fn sync_upserts_delta_then_noop_loads_no_embedder() {
    let (root, config, index) = fixture("delta");
    let mut first_events = Vec::new();
    let first = {
        let mut sink = |event| first_events.push(event);
        execute_with_progress(
            &root,
            &config,
            &index,
            || Ok(embedder(&config, None)),
            Some(&mut sink),
        )
        .unwrap()
    };
    let mut noop_events = Vec::new();
    let second = {
        let mut sink = |event| noop_events.push(event);
        execute_with_progress(
            &root,
            &config,
            &index,
            || -> Result<FakeEmbedder, Error> { panic!("no-op sync loaded the embedder") },
            Some(&mut sink),
        )
        .unwrap()
    };

    assert_eq!((first.embeddings, first.points_upserted), (1, 1));
    assert_eq!((second.embeddings, second.points_upserted), (0, 0));
    assert_eq!(
        first_events,
        super::super::support::sync_events(PHYSICAL, &first.corpus_sha256, 1)
    );
    assert_eq!(
        noop_events,
        super::super::support::sync_events(PHYSICAL, &second.corpus_sha256, 0)
    );
    let counts = index.inner.lock().unwrap();
    assert_eq!(counts.points_upserted, 1);
    assert_eq!(counts.ready_marks, 2);
    drop(counts);
    assert_eq!(state(&root).unwrap(), VectorState::Ready);
    fs::remove_dir_all(root).unwrap();
}
